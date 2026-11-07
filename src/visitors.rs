//! Multi Network visitors (extension `mni`, see docs/Brew-Extensions.md 12b).
//!
//! A station with the `mni` capability tells this server which network a
//! radio comes from (`SERVICE_MS_NETWORK` before its `SUB_REGISTER`). Such a
//! visitor is kept apart from this server's own subscribers, keyed by
//! (network slot, ISSI), so the same ISSI in two networks does not clash.
//! Its registration and affiliations go on to its home network over that
//! network's link (`netlink`), so the home network routes its calls here;
//! a group call of its home network reaches the stations of the visitors in
//! that group (`SERVICE_CALL_NETWORK` before the `GROUP_TX`), and a group
//! call of a visitor on a home group (`SERVICE_CALL_NETWORK` from the
//! station) goes to the home network and to the other visitors.
//!
//! None of this is ETSI: everything between station and servers is inside
//! the SwMI. What the radio does on air (migration, TS 100 392-2 clause
//! 16.4.1.1 case b)) is the station's business.

use crate::netlink::{self, alias, split_alias, GSSI_MASK};
use crate::protocol::{self, SubscriberMessage, SUB_AFFILIATE, SUB_DEAFFILIATE, SUB_DEREGISTER, SUB_REGISTER, SUB_REREGISTER};
use crate::state::{AppState, ClientId, ClientMode, Inner};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Station capability and server extension.
pub const CAPABILITY: &str = netlink::CAPABILITY_MNI;

/// How long a `SERVICE_CALL_NETWORK` waits for its `GROUP_TX`.
const CALL_NOTE_FOR: Duration = Duration::from_secs(5);
/// Bound on visitors (all networks).
pub const MAX_VISITORS: usize = 4096;

/// One visitor: the station it is on and its groups in its home network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visitor {
    pub station: ClientId,
    pub groups: BTreeSet<u32>,
    pub since_ms: u64,
}

/// Visitors of this server (see the module comment).
#[derive(Debug, Default)]
pub struct Visitors {
    /// (network slot, ISSI) -> visitor.
    pub by_key: HashMap<(u8, u32), Visitor>,
    /// (station, ISSI) -> network slot: what `SERVICE_MS_NETWORK` said, until
    /// the station deregisters the ISSI.
    of: HashMap<(ClientId, u32), u8>,
    /// Call id -> (station, alias): a `SERVICE_CALL_NETWORK` waiting for its
    /// `GROUP_TX`.
    calls: HashMap<Uuid, (ClientId, u32, Instant)>,
    /// Call id -> (station, network slot): a `SERVICE_CALL_NETWORK` naming
    /// the called party's network, waiting for its individual call / SDS.
    called: HashMap<Uuid, (ClientId, u8, Instant)>,
    /// (calling ISSI, called ISSI) -> when an unknown target was last handed
    /// to another network: a second time within [`FORWARD_GUARD`] is a loop.
    forwarded: HashMap<(u32, u32), Instant>,
    /// (station, ISSI, GSSI) -> network slot: a group of a linked network
    /// that one of this server's radios (migrated onto a cell of that
    /// network) attached, as `SERVICE_GROUP_NETWORK` said. Its affiliation is
    /// kept as that network's group (an alias), see [`group_alias`].
    foreign: HashMap<(ClientId, u32, u32), u8>,
}

/// Loop guard of "forward unknown targets" (as `fedroute::CALL_DEDUP_WINDOW`).
pub const FORWARD_GUARD: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct Mni {
    pub mcc: u16,
    pub mnc: u16,
}

fn has_capability(inner: &Inner, id: ClientId) -> bool {
    inner.clients.get(&id).is_some_and(|c| c.mode == ClientMode::Basestation)
        && inner.carriers.info(id).is_some_and(|i| i.capabilities.iter().any(|c| c == CAPABILITY))
}

/// A station of another network: it speaks the `mni` extension and every
/// cell it announced broadcasts an MNI other than this server's own (a cell
/// serving radios of another network through this server). Such a station
/// only takes this server's group calls where a radio of this network that
/// migrated there is in the group (an affiliation), never a broadcast.
/// Without the extension, or without this server's MNI set, nothing changes.
pub fn other_network_station(state: &AppState, inner: &Inner, id: ClientId) -> bool {
    let cfg = state.networks.read().map(|c| (c.mcc, c.mnc)).unwrap_or_default();
    if cfg == (0, 0) || !has_capability(inner, id) { return false; }
    inner.carriers.info(id).is_some_and(|i| !i.cells.is_empty() && i.cells.iter().all(|c| (c.key.mcc, c.key.mnc) != cfg))
}

/// Slot of the switched-on network with this MNI (`None`: this server's own
/// network, an unknown one, or Multi Network off).
fn slot_of(state: &AppState, mni: Mni) -> Option<u8> {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    if !cfg.enabled || (mni.mcc == cfg.mcc && mni.mnc == cfg.mnc) { return None; }
    let name = cfg.networks.iter().find(|n| n.enabled && n.mcc == mni.mcc && n.mnc == mni.mnc && (n.mcc, n.mnc) != (0, 0))?.name.clone();
    netlink::slots(state).get(&name).copied()
}

/// MNI of a network slot, if configured.
pub fn mni_of(state: &AppState, slot: u8) -> Option<Mni> {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    let slots = netlink::slots(state);
    cfg.networks.iter().find(|n| slots.get(&n.name) == Some(&slot)).map(|n| Mni { mcc: n.mcc, mnc: n.mnc })
}

/// `SERVICE_MS_NETWORK` (0x21): `{"issi":N,"mcc":N,"mnc":N}` right before
/// the `SUB_REGISTER` of a radio from another network.
pub async fn handle_ms_network(state: &AppState, source: ClientId, json: &str) {
    #[derive(Deserialize)]
    struct Msg { issi: u32, mcc: u16, mnc: u16 }
    let Ok(m) = serde_json::from_str::<Msg>(json) else { warn!(%source, json, "malformed MS network message"); return };
    if m.issi == 0 || m.issi > GSSI_MASK { return; }
    let slot = slot_of(state, Mni { mcc: m.mcc, mnc: m.mnc });
    let mut inner = state.inner.write().await;
    if !has_capability(&inner, source) {
        debug!(%source, issi = m.issi, "MS network from a station without the mni capability ignored");
        return;
    }
    match slot {
        Some(slot) => {
            if inner.visitors.of.len() >= MAX_VISITORS && !inner.visitors.of.contains_key(&(source, m.issi)) {
                warn!(%source, issi = m.issi, "multi network: too many visitors, radio treated as this network's");
                return;
            }
            info!(%source, issi = m.issi, mcc = m.mcc, mnc = m.mnc, slot, "multi network: radio from another network");
            if let Some(old) = inner.visitors.of.insert((source, m.issi), slot).filter(|old| *old != slot) {
                leave_home_network(state, &mut inner, source, old, m.issi);
            }
        }
        None => {
            // This server's own network, or a network it does not link to:
            // the radio is one of ours (rule 1).
            if let Some(old) = inner.visitors.of.remove(&(source, m.issi)) {
                info!(%source, issi = m.issi, "multi network: radio is this network's again");
                leave_home_network(state, &mut inner, source, old, m.issi);
            } else {
                debug!(%source, issi = m.issi, mcc = m.mcc, mnc = m.mnc, "MS network names no linked network; radio is this network's");
            }
        }
    }
}

/// `SERVICE_GROUP_NETWORK` (0x22): groups of a radio that are in another
/// network than the radio's own. A group of a linked network attached by
/// one of this server's radios (it migrated onto a cell of that network and
/// attached the group without an MNI, TS 100 392-2 clause 14.2.3) is that
/// network's group: its affiliation that follows is kept as the alias and
/// mirrored there, so that network's calls on it reach the radio. Other
/// cases (a visitor in a third network's group) are noted in the log only.
pub async fn handle_group_network(state: &AppState, source: ClientId, json: &str) {
    #[derive(Deserialize)]
    struct G { gssi: u32, mcc: u16, mnc: u16 }
    #[derive(Deserialize)]
    struct Msg { issi: u32, groups: Vec<G> }
    let Ok(m) = serde_json::from_str::<Msg>(json) else { warn!(%source, json, "malformed group network message"); return };
    let mut inner = state.inner.write().await;
    if !has_capability(&inner, source) { return; }
    let visitor = inner.visitors.of.contains_key(&(source, m.issi));
    for g in m.groups {
        if g.gssi == 0 || g.gssi > GSSI_MASK { continue; }
        if let (false, Some(slot)) = (visitor, slot_of(state, Mni { mcc: g.mcc, mnc: g.mnc })) {
            if inner.visitors.foreign.len() < MAX_VISITORS || inner.visitors.foreign.contains_key(&(source, m.issi, g.gssi)) {
                info!(%source, issi = m.issi, gssi = g.gssi, mcc = g.mcc, mnc = g.mnc, slot, "multi network: radio of this network in a group of a linked network");
                inner.visitors.foreign.insert((source, m.issi, g.gssi), slot);
            }
            continue;
        }
        warn!(%source, issi = m.issi, gssi = g.gssi, mcc = g.mcc, mnc = g.mnc, "multi network: group of a third network not supported yet, ignored");
    }
}

/// `SERVICE_CALL_NETWORK` (0x23) from a station: the next `GROUP_TX` with
/// this call id is on a group of another network.
pub async fn handle_call_network(state: &AppState, source: ClientId, json: &str) {
    #[derive(Deserialize)]
    struct G { gssi: u32, mcc: u16, mnc: u16 }
    #[derive(Deserialize)]
    struct Msg { uuid: Uuid, group: Option<G>, called: Option<Mni> }
    let Ok(m) = serde_json::from_str::<Msg>(json) else { warn!(%source, json, "malformed call network message"); return };
    // An individual call or SDS to a party of another network (TSI, TS 100
    // 392-2 table 14.39).
    if let Some(called) = m.called {
        let slot = slot_of(state, called);
        let mut inner = state.inner.write().await;
        if !has_capability(&inner, source) { return; }
        let now = Instant::now();
        inner.visitors.called.retain(|_, (_, _, at)| now.duration_since(*at) < CALL_NOTE_FOR);
        match slot {
            Some(slot) if inner.visitors.called.len() < MAX_VISITORS => { inner.visitors.called.insert(m.uuid, (source, slot, now)); }
            Some(_) => {}
            None => debug!(%source, uuid = %m.uuid, mcc = called.mcc, mnc = called.mnc, "call network names this network or no linked one; routed here"),
        }
        return;
    }
    let Some(g) = m.group else { return };
    if g.gssi == 0 || g.gssi > GSSI_MASK { return; }
    let slot = slot_of(state, Mni { mcc: g.mcc, mnc: g.mnc });
    let mut inner = state.inner.write().await;
    if !has_capability(&inner, source) { return; }
    let Some(slot) = slot else { return };
    let now = Instant::now();
    inner.visitors.calls.retain(|_, (_, _, at)| now.duration_since(*at) < CALL_NOTE_FOR);
    if inner.visitors.calls.len() < MAX_VISITORS {
        inner.visitors.calls.insert(m.uuid, (source, alias(slot, g.gssi), now));
    }
}

/// The group a station's radio affiliates to: the alias of a linked
/// network's group if `SERVICE_GROUP_NETWORK` named it (see
/// [`handle_group_network`]), else the GSSI itself.
pub fn group_alias(inner: &Inner, source: ClientId, issi: u32, gssi: u32) -> u32 {
    inner.visitors.foreign.get(&(source, issi, gssi)).map_or(gssi, |slot| alias(*slot, gssi))
}

/// A radio left its station (deregistered, or the station went away): its
/// groups of linked networks are forgotten.
pub fn forget_foreign(inner: &mut Inner, source: ClientId, issi: Option<u32>) {
    inner.visitors.foreign.retain(|(s, i, _), _| *s != source || issi.is_some_and(|x| x != *i));
}

/// The group of a `GROUP_TX` from `source`: the other network's group (an
/// alias) if a `SERVICE_CALL_NETWORK` announced it, else `None`. A call
/// keeps its alias for its later overs.
pub fn call_alias(inner: &mut Inner, source: ClientId, id: Uuid) -> Option<u32> {
    if let Some((station, a, _)) = inner.visitors.calls.remove(&id) {
        if station == source { return Some(a); }
    }
    inner.calls.get(&id).filter(|c| c.owner == source && split_alias(c.destination).is_some()).map(|c| c.destination)
}

/// Where Multi Network sends an individual call or SDS.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// To this station, with this `SERVICE_CALL_NETWORK` before it (if any).
    Station(ClientId, Option<Vec<u8>>),
    /// To the link of another network.
    Link(ClientId),
    /// Nowhere: refused.
    Refuse,
}

/// `SERVICE_CALL_NETWORK` for an individual call or SDS to a station: the
/// networks of the calling and called parties, only where they are not this
/// server's own.
pub fn build_party_network(id: &Uuid, calling: Option<Mni>, called: Option<Mni>) -> Vec<u8> {
    let mut v = serde_json::json!({ "uuid": id.to_string() });
    if let Some(m) = calling { v["calling"] = serde_json::json!({ "mcc": m.mcc, "mnc": m.mnc }); }
    if let Some(m) = called { v["called"] = serde_json::json!({ "mcc": m.mcc, "mnc": m.mnc }); }
    protocol::build_service(protocol::SERVICE_CALL_NETWORK, &v.to_string())
}

/// The network a radio on station `station` with `issi` belongs to, if it
/// is a visitor.
fn visitor_slot(inner: &Inner, station: ClientId, issi: u32) -> Option<u8> {
    inner.visitors.of.get(&(station, issi)).copied()
}

/// Where an individual call or SDS (`id`, `calling` -> `called`) that came
/// in over `source` goes, when Multi Network decides it; `None`: as without
/// Multi Network (this server's own radios, SIP, SMS Center).
///
/// - Over the link of network X: to a visitor from X here, or to one of this
///   server's radios that the link registered there; anything else is
///   refused (that network sent it to an ISSI this server never gave it).
/// - From a station that named the called party's network X
///   (`SERVICE_CALL_NETWORK` with `called`): to a visitor from X here, else
///   over the link to X (refused while it is down).
pub fn individual_route(state: &AppState, inner: &mut Inner, source: ClientId, id: Uuid, calling: u32, called: u32) -> Option<Decision> {
    let note_for = |inner: &Inner, to: ClientId, calling: Option<Mni>, called: Option<Mni>| {
        has_capability(inner, to).then(|| build_party_network(&id, calling, called))
    };
    if let Some((&slot, _)) = inner.netlinks.iter().find(|(_, c)| **c == source) {
        let mni = mni_of(state, slot);
        if let Some(v) = inner.visitors.by_key.get(&(slot, called)) {
            let to = v.station;
            return Some(Decision::Station(to, note_for(inner, to, mni, mni)));
        }
        if netlink::exported(state, slot, called) {
            if let Some(to) = inner.subscribers.get(&called).map(|s| s.client_id) {
                return Some(Decision::Station(to, note_for(inner, to, mni, None)));
            }
        }
        info!(network = slot, uuid = %id, calling, called, "multi network: individual call / SDS for an ISSI this server has not registered there refused");
        return Some(Decision::Refuse);
    }
    let Some((station, slot, _)) = inner.visitors.called.remove(&id) else {
        return forwarded_to_visitor(state, inner, source, calling, called)
            .map(|(to, slot)| Decision::Station(to, note_for(inner, to, None, mni_of(state, slot))));
    };
    if station != source { return None; }
    let mni = mni_of(state, slot);
    let calling_mni = visitor_slot(inner, source, calling).and_then(|s| mni_of(state, s));
    if let Some(v) = inner.visitors.by_key.get(&(slot, called)) {
        let to = v.station;
        return Some(Decision::Station(to, note_for(inner, to, calling_mni, mni)));
    }
    // A radio of ours the link registered in that network ("register all",
    // a talker): its station here. Over the link it would come back the
    // same way, and the far server does not send a call back to the
    // connection it came from (field check 2026-11-07: the MTP as a
    // TetraPack radio calling the app got "party not available").
    if netlink::exported(state, slot, called) {
        if let Some(to) = inner.subscribers.get(&called).filter(|s| s.route.path.is_empty()).map(|s| s.client_id) {
            info!(network = slot, uuid = %id, calling, called, "multi network: called party is one of ours registered in that network; delivered here");
            return Some(Decision::Station(to, note_for(inner, to, calling_mni, None)));
        }
    }
    match inner.netlinks.get(&slot).copied() {
        Some(link) => {
            // The caller should be known there, as a Basestation's radio is
            // (not a visitor from that network: it is registered there).
            if visitor_slot(inner, source, calling) != Some(slot) { netlink::ensure_registered(state, slot, calling); }
            Some(Decision::Link(link))
        }
        None => {
            info!(network = slot, uuid = %id, calling, called, "multi network: link to the called party's network is down; refused");
            Some(Decision::Refuse)
        }
    }
}

/// A party nobody here takes that "forward unknown targets" would hand to
/// a linked network, while that network's radio with this ISSI is a visitor
/// on one of this server's stations: delivered to it directly. Sent over
/// the link it would come back the same way, and the far server does not
/// send a call back to the connection it came from (field check
/// 2026-11-07: the app calling the MTP registered as a TetraPack radio).
fn forwarded_to_visitor(state: &AppState, inner: &Inner, source: ClientId, calling: u32, called: u32) -> Option<(ClientId, u8)> {
    if inner.subscribers.contains_key(&called) || inner.group_clients.contains_key(&called) { return None; }
    if visitor_slot(inner, source, calling).is_some() { return None; }
    if inner.clients.get(&source).is_none_or(|c| c.mode == ClientMode::Network) { return None; }
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    if !cfg.enabled { return None; }
    let slots = netlink::slots(state);
    let slot = cfg.networks.iter().filter(|n| n.enabled && n.forward_unknown).find_map(|n| slots.get(&n.name).copied())?;
    let to = inner.visitors.by_key.get(&(slot, called))?.station;
    info!(calling, called, network = slot, "multi network: unknown target is a visitor of the forwarding network here; delivered to its station");
    Some((to, slot))
}

/// "Forward unknown targets": an individual call or SDS from a station that
/// nobody here takes goes once to the first linked network that has it
/// switched on. Never from a link (so never back), and a second time for
/// the same pair within [`FORWARD_GUARD`] is taken for a loop.
pub fn forward_unknown(state: &AppState, inner: &mut Inner, source: ClientId, calling: u32, called: u32) -> Option<ClientId> {
    if inner.clients.get(&source).is_none_or(|c| c.mode == ClientMode::Network) { return None; }
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    if !cfg.enabled { return None; }
    let slots = netlink::slots(state);
    let link = cfg.networks.iter().filter(|n| n.enabled && n.forward_unknown)
        .find_map(|n| slots.get(&n.name).and_then(|s| inner.netlinks.get(s).copied().map(|l| (l, *s))))?;
    let now = Instant::now();
    inner.visitors.forwarded.retain(|_, at| now.duration_since(*at) < FORWARD_GUARD);
    if inner.visitors.forwarded.contains_key(&(calling, called)) {
        warn!(calling, called, "multi network: unknown target came back within the loop guard; not forwarded again");
        return None;
    }
    if inner.visitors.forwarded.len() < MAX_VISITORS { inner.visitors.forwarded.insert((calling, called), now); }
    netlink::ensure_registered(state, link.1, calling);
    info!(calling, called, network = link.1, "multi network: unknown target forwarded to another network");
    Some(link.0)
}

/// Upstream channel of the link to network `slot`.
fn up(state: &AppState, slot: u8) -> Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> {
    netlink::up_for_slot(state, slot)
}

fn send_up(state: &AppState, slot: u8, msg: Vec<u8>) {
    if let Some(up) = up(state, slot) { let _ = up.send(msg); }
}

/// A subscriber message from a station about one of its visitors: kept in
/// the visitor table (never in `Inner::subscribers`) and passed on to the
/// visitor's home network. False if the ISSI is not a visitor there.
pub async fn handle_subscriber(state: &AppState, source: ClientId, msg: &SubscriberMessage) -> bool {
    let mut inner = state.inner.write().await;
    let Some(&slot) = inner.visitors.of.get(&(source, msg.issi)) else { return false };
    let key = (slot, msg.issi);
    match msg.msg_type {
        SUB_REGISTER | SUB_REREGISTER => {
            let (old, groups) = match inner.visitors.by_key.get(&key) {
                Some(v) => (Some(v.station), v.groups.clone()),
                None => (None, BTreeSet::new()),
            };
            if let Some(old) = old.filter(|o| *o != source) {
                // Moved between two of this server's stations: the home
                // network sees nothing of it.
                info!(from = %old, to = %source, issi = msg.issi, slot, "multi network: visitor moved to another station");
                inner.visitors.of.remove(&(old, msg.issi));
            }
            let new = !inner.visitors.by_key.contains_key(&key);
            inner.visitors.by_key.insert(key, Visitor { station: source, groups, since_ms: crate::telemetry::now_ms() });
            info!(%source, issi = msg.issi, slot, "multi network: visitor registered");
            if new {
                netlink::set_visitor(state, slot, msg.issi, true);
                send_up(state, slot, protocol::build_subscriber_message(SUB_REGISTER, msg.issi, &[]));
            }
        }
        SUB_DEREGISTER => {
            inner.visitors.of.remove(&(source, msg.issi));
            if inner.visitors.by_key.get(&key).is_some_and(|v| v.station == source) {
                inner.visitors.by_key.remove(&key);
                info!(%source, issi = msg.issi, slot, "multi network: visitor deregistered");
                netlink::set_visitor(state, slot, msg.issi, false);
                send_up(state, slot, protocol::build_subscriber_message(SUB_DEREGISTER, msg.issi, &[]));
            }
        }
        SUB_AFFILIATE | SUB_DEAFFILIATE => {
            let Some(v) = inner.visitors.by_key.get_mut(&key).filter(|v| v.station == source) else { return true };
            let groups: Vec<u32> = msg.groups.iter().copied().filter(|g| *g != 0 && *g <= GSSI_MASK).collect();
            let changed: Vec<u32> = if msg.msg_type == SUB_AFFILIATE {
                groups.into_iter().filter(|g| v.groups.insert(*g)).collect()
            } else {
                groups.into_iter().filter(|g| v.groups.remove(g)).collect()
            };
            if !changed.is_empty() {
                info!(%source, issi = msg.issi, slot, groups = ?changed, affiliate = msg.msg_type == SUB_AFFILIATE, "multi network: visitor groups changed");
                send_up(state, slot, protocol::build_subscriber_message(msg.msg_type, msg.issi, &changed));
            }
        }
        _ => {}
    }
    true
}

/// A radio of `source` that was registered in network `slot` as a visitor
/// now belongs elsewhere (it migrated, or registers with another identity):
/// it leaves that network, so it is not reachable there and here at once.
fn leave_home_network(state: &AppState, inner: &mut Inner, source: ClientId, slot: u8, issi: u32) {
    if inner.visitors.by_key.get(&(slot, issi)).is_some_and(|v| v.station == source) {
        inner.visitors.by_key.remove(&(slot, issi));
        info!(%source, issi, slot, "multi network: visitor left its home network registration");
        netlink::set_visitor(state, slot, issi, false);
        send_up(state, slot, protocol::build_subscriber_message(SUB_DEREGISTER, issi, &[]));
    }
}

/// A station went away: its visitors leave their home networks.
pub fn station_gone(state: &AppState, inner: &mut Inner, id: ClientId) {
    inner.visitors.of.retain(|(station, _), _| *station != id);
    forget_foreign(inner, id, None);
    inner.visitors.calls.retain(|_, (station, _, _)| *station != id);
    let gone: Vec<(u8, u32)> = inner.visitors.by_key.iter().filter(|(_, v)| v.station == id).map(|(k, _)| *k).collect();
    for (slot, issi) in gone {
        inner.visitors.by_key.remove(&(slot, issi));
        info!(station = %id, issi, slot, "multi network: visitor gone with its station");
        netlink::set_visitor(state, slot, issi, false);
        send_up(state, slot, protocol::build_subscriber_message(SUB_DEREGISTER, issi, &[]));
    }
}

/// Registrations and affiliations of the visitors of network `slot`, as
/// that network should have them (after its link came up).
pub fn registrations(inner: &Inner, slot: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for ((s, issi), v) in &inner.visitors.by_key {
        if *s != slot { continue; }
        out.push(protocol::build_subscriber_message(SUB_REGISTER, *issi, &[]));
        if !v.groups.is_empty() {
            let g: Vec<u32> = v.groups.iter().copied().collect();
            out.push(protocol::build_subscriber_message(SUB_AFFILIATE, *issi, &g));
        }
    }
    out
}

/// Groups of network `slot` that some visitor here is in.
pub fn groups_of(inner: &Inner, slot: u8) -> BTreeSet<u32> {
    let mut g: BTreeSet<u32> = inner.visitors.by_key.iter().filter(|((s, _), _)| *s == slot).flat_map(|(_, v)| v.groups.iter().copied()).collect();
    // Groups of that network this server's radios are in (see `group_alias`).
    g.extend(inner.group_clients.keys().filter_map(|a| split_alias(*a)).filter(|(s, _)| *s == slot).map(|(_, x)| x));
    g
}

/// ISSIs of the visitors of network `slot`.
pub fn issis_of(inner: &Inner, slot: u8) -> HashSet<u32> {
    inner.visitors.by_key.keys().filter(|(s, _)| *s == slot).map(|(_, i)| *i).collect()
}

/// Stations with a visitor in group `alias` (another network's group), or
/// with a radio of this network affiliated to it (see `group_alias`).
pub fn targets(inner: &Inner, a: u32) -> HashSet<ClientId> {
    let Some((slot, gssi)) = split_alias(a) else { return HashSet::new() };
    let mut t: HashSet<ClientId> = inner.visitors.by_key.iter()
        .filter(|((s, _), v)| *s == slot && v.groups.contains(&gssi))
        .map(|(_, v)| v.station)
        .collect();
    t.extend(inner.group_clients.get(&a).into_iter().flatten().copied());
    t
}

/// `SERVICE_CALL_NETWORK` for a station: call `id` is on group `gssi` of
/// the network with MNI `mni`.
pub fn build_call_network(id: &Uuid, gssi: u32, mni: Mni) -> Vec<u8> {
    protocol::build_service(protocol::SERVICE_CALL_NETWORK, &serde_json::json!({
        "uuid": id.to_string(), "group": { "gssi": gssi, "mcc": mni.mcc, "mnc": mni.mnc },
    }).to_string())
}

/// What a station gets for a packet the router sends it: a `GROUP_TX` on
/// another network's group goes as that group's own GSSI with a
/// `SERVICE_CALL_NETWORK` before it (a station never sees an alias).
/// Everything else as it is.
pub fn for_station(state: &AppState, packet: &[u8]) -> Vec<Vec<u8>> {
    if packet.len() >= 26 && packet[0] == protocol::CLASS_CALL_CONTROL && packet[1] == protocol::CALL_GROUP_TX {
        let dest = u32::from_le_bytes(packet[22..26].try_into().unwrap_or_default());
        if let Some((slot, gssi)) = split_alias(dest) {
            let Ok(id) = Uuid::from_slice(&packet[2..18]) else { return vec![packet.to_vec()] };
            let mut real = packet.to_vec();
            real[22..26].copy_from_slice(&gssi.to_le_bytes());
            return match mni_of(state, slot) {
                Some(mni) => vec![build_call_network(&id, gssi, mni), real],
                None => vec![real],
            };
        }
    }
    vec![packet.to_vec()]
}

/// `SERVICE_NETWORKS` (0x20): this server's network and the linked ones,
/// with their state; never credentials.
pub fn build_networks(state: &AppState) -> Vec<u8> {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    let up = netlink::connected(state);
    let networks: Vec<serde_json::Value> = cfg.networks.iter()
        .filter(|n| cfg.enabled && n.enabled && (n.mcc, n.mnc) != (0, 0))
        .map(|n| serde_json::json!({ "mcc": n.mcc, "mnc": n.mnc, "name": n.name, "up": up.contains(&n.name) }))
        .collect();
    protocol::build_service(protocol::SERVICE_NETWORKS, &serde_json::json!({
        "this": { "mcc": cfg.mcc, "mnc": cfg.mnc }, "networks": networks,
    }).to_string())
}

/// Sends `SERVICE_NETWORKS` to every station with the `mni` capability (or
/// only to `only`).
pub async fn announce(state: &AppState, only: Option<ClientId>) {
    let packet = build_networks(state);
    let inner = state.inner.read().await;
    for (id, c) in &inner.clients {
        if only.is_some_and(|o| o != *id) || !has_capability(&inner, *id) { continue; }
        let _ = c.tx.send(packet.clone());
    }
}

/// Visitors of network `slot` for the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct VisitorView {
    pub issi: u32,
    pub station: String,
    pub groups: Vec<u32>,
    pub since_ms: u64,
}

pub fn view(inner: &Inner, slot: u8) -> Vec<VisitorView> {
    let mut v: Vec<VisitorView> = inner.visitors.by_key.iter().filter(|((s, _), _)| *s == slot).map(|((_, issi), v)| VisitorView {
        issi: *issi,
        station: crate::carriers::station_name(v.station, inner.clients.get(&v.station).and_then(|c| c.username.as_deref())),
        groups: v.groups.iter().copied().collect(),
        since_ms: v.since_ms,
    }).collect();
    v.sort_by_key(|x| x.issi);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, NetworkConfig};

    fn state() -> AppState {
        let mut cfg = Config::default();
        cfg.storage.enabled = false;
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig { name: "TP".into(), mcc: 901, mnc: 1234, remote_host: "tp:9000".into(), password: "secret".into(), ..Default::default() });
        AppState::new(cfg, "test.toml".into()).0
    }

    #[test]
    fn a_station_never_sees_an_alias_and_never_credentials() {
        let st = state();
        let slot = netlink::slots(&st)["TP"];
        let id = Uuid::new_v4();
        let out = for_station(&st, &protocol::build_group_tx(&id, 1234, alias(slot, 909), 0));
        assert_eq!(out.len(), 2);
        assert_eq!(&out[1][22..26], &909u32.to_le_bytes());
        let note = String::from_utf8_lossy(&out[0][2..]).trim_end_matches('\0').to_string();
        let v: serde_json::Value = serde_json::from_str(&note).unwrap();
        assert_eq!((v["uuid"].as_str(), v["group"]["gssi"].as_u64(), v["group"]["mnc"].as_u64()), (Some(id.to_string().as_str()), Some(909), Some(1234)));
        // Our own groups go as they are.
        assert_eq!(for_station(&st, &protocol::build_group_tx(&id, 1234, 9, 0)).len(), 1);
        let nets = String::from_utf8_lossy(&build_networks(&st)).to_string();
        assert!(nets.contains("\"name\":\"TP\"") && nets.contains("\"up\":false") && !nets.contains("secret") && !nets.contains("tp:9000"));
    }

    #[test]
    fn mni_maps_to_the_linked_network_only() {
        let st = state();
        let slot = netlink::slots(&st)["TP"];
        assert_eq!(slot_of(&st, Mni { mcc: 901, mnc: 1234 }), Some(slot));
        assert_eq!(slot_of(&st, Mni { mcc: 901, mnc: 9999 }), None, "this server's own network");
        assert_eq!(slot_of(&st, Mni { mcc: 901, mnc: 4321 }), None, "a network not linked");
        assert_eq!(mni_of(&st, slot), Some(Mni { mcc: 901, mnc: 1234 }));
    }
}
