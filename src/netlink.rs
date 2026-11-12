//! Multi Network: links from this server to other Brew networks.
//!
//! Each `[[multi_network.networks]]` entry is one outbound connection to that
//! network's Brew server in plain Basestation mode, so the far server needs
//! nothing new: to it this server is one more Basestation, logged in with the
//! Basestation login that network gave out and introducing itself with the
//! name the operator chose (`user_agent`). The credentials stay on this
//! server; no station ever sees them.
//!
//! What crosses a link is decided by the talkgroup pools (see `pools`): a
//! pool may list groups of other networks (`remote`) next to this server's
//! own groups, which makes it a bridge across networks. A call on one member
//! is played on the others as an ordinary group call on their own GSSI, with
//! the talker's ISSI passed on as it is. Nothing else crosses: registrations,
//! services and calls that are not on a bridged group are dropped in both
//! directions.
//!
//! Inside this server a group of another network is addressed by an *alias*:
//! `(slot << 24) | gssi`, where the slot (1 to 255) stands for the network.
//! A GSSI has 24 bits (TS 100 392-2 V3.10.1 clause 16.10.28, Table 16.60)
//! and Brew carries it in a 32-bit field, so an alias can never collide with
//! a real group. The link puts the real GSSI back on everything it sends out and
//! the alias on everything it lets in, so the router and the pool machinery
//! work unchanged and a station never sees an alias.
//!
//! How a network is reached is not defined by ETSI: Brew is not an ETSI
//! interface, and the inter-system interface (ISI, EN 300 392-3) is not used
//! here. The links are edited live from the dashboard and written to the
//! config file without a restart.

use crate::config::{MultiNetworkConfig, NetworkConfig, TalkgroupPoolConfig};
use crate::federation::{self, Keepalive, LinkIdentity};
use crate::protocol::{self, ConnVersion};
use crate::state::{AppState, Client, ClientId, ClientMode};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Longest network name accepted from the dashboard.
const MAX_NAME: usize = 40;
/// Longest `user_agent` accepted.
const MAX_USER_AGENT: usize = 80;
/// Bits of a GSSI; the slot sits above them.
const GSSI_BITS: u32 = 24;
pub const GSSI_MASK: u32 = 0xFF_FFFF;

/// Internal address of group `gssi` of the network in `slot`.
pub fn alias(slot: u8, gssi: u32) -> u32 {
    (u32::from(slot) << GSSI_BITS) | (gssi & GSSI_MASK)
}

/// `(slot, gssi)` of an alias; `None` for a group of this server.
pub fn split_alias(value: u32) -> Option<(u8, u32)> {
    let slot = (value >> GSSI_BITS) as u8;
    (slot != 0).then_some((slot, value & GSSI_MASK))
}

/// The running links and what the dashboard shows of them.
#[derive(Default)]
pub struct NetLinks {
    reg: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    /// Set once the services run (an HA standby node starts no links).
    started: bool,
    /// Network name -> slot, kept for the life of the process so an alias
    /// never changes under a running call.
    slots: HashMap<String, u8>,
    tasks: HashMap<String, LinkTask>,
    status: HashMap<String, LinkStatus>,
    /// Numbers the link tasks, so a stopped task cannot overwrite the state
    /// of the one that replaced it.
    next_gen: u64,
}

struct LinkTask {
    cfg: NetworkConfig,
    stop: watch::Sender<bool>,
    gen: u64,
}

/// State of one link.
#[derive(Debug, Clone, Default)]
struct LinkStatus {
    connected: bool,
    since_ms: u64,
    last_error: String,
    /// Raw upstream channel of the connected link (bypasses the filter).
    up: Option<mpsc::UnboundedSender<Vec<u8>>>,
    /// Groups the bridge ISSI is affiliated to there right now.
    affiliated: BTreeSet<u32>,
    /// Radios this link registered there, with their groups there (only
    /// with `register_talkers`).
    members: Option<SharedExport>,
}

impl NetLinks {
    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.reg.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Slot of every network in `cfg`, assigning new ones as needed. A
    /// network that finds no free slot (more than 255 in one process life)
    /// is left out.
    pub fn slots(&self, cfg: &MultiNetworkConfig) -> HashMap<String, u8> {
        let mut reg = self.lock();
        for n in &cfg.networks {
            if reg.slots.contains_key(&n.name) { continue; }
            let used: HashSet<u8> = reg.slots.values().copied().collect();
            if let Some(free) = (1..=u8::MAX).find(|s| !used.contains(s)) {
                reg.slots.insert(n.name.clone(), free);
            }
        }
        cfg.networks.iter().filter_map(|n| reg.slots.get(&n.name).map(|s| (n.name.clone(), *s))).collect()
    }
}

/// Slot of every configured network (see `NetLinks::slots`).
pub fn slots(state: &AppState) -> HashMap<String, u8> {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    state.netlinks.slots(&cfg)
}

/// Upstream channel of the connected link to network `slot`.
pub fn up_for_slot(state: &AppState, slot: u8) -> Option<mpsc::UnboundedSender<Vec<u8>>> {
    let reg = state.netlinks.lock();
    let name = reg.slots.iter().find(|(_, s)| **s == slot)?.0.clone();
    reg.status.get(&name).and_then(|s| s.up.clone())
}

/// Names of the networks whose link is up.
pub fn connected(state: &AppState) -> HashSet<String> {
    state.netlinks.lock().status.iter().filter(|(_, s)| s.connected).map(|(n, _)| n.clone()).collect()
}

/// Notes that `issi` is (or is no longer) a visitor from network `slot`, so
/// the link does not register it there as one of this server's talkers.
pub fn set_visitor(state: &AppState, slot: u8, issi: u32, present: bool) {
    let reg = state.netlinks.lock();
    let Some(name) = reg.slots.iter().find(|(_, s)| **s == slot).map(|(n, _)| n.clone()) else { return };
    if let Some(e) = reg.status.get(&name).and_then(|s| s.members.clone()) {
        let mut e = export_lock(&e);
        if present { e.visitors.insert(issi); } else { e.visitors.remove(&issi); }
    }
}

/// Has the link to network `slot` registered `issi` there (one of this
/// server's radios)?
pub fn exported(state: &AppState, slot: u8, issi: u32) -> bool {
    let reg = state.netlinks.lock();
    let Some(name) = reg.slots.iter().find(|(_, s)| **s == slot).map(|(n, _)| n.clone()) else { return false };
    reg.status.get(&name).and_then(|s| s.members.clone()).is_some_and(|e| export_lock(&e).have.contains_key(&issi))
}

/// Registers one of this server's radios in network `slot` (the caller of an
/// individual call or SDS going there), as a Basestation would; only with
/// `register_talkers`, and only if that network may have the ISSI (never
/// export list, ISSI check: asked first, then left out this time). It is
/// deregistered again at the next sync once its call is over.
pub fn ensure_registered(state: &AppState, slot: u8, issi: u32) {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    let reg = state.netlinks.lock();
    let Some(name) = reg.slots.iter().find(|(_, s)| **s == slot).map(|(n, _)| n.clone()) else { return };
    if !cfg.networks.iter().any(|n| n.name == name && n.register_talkers && n.bridge_issi != issi) { return; }
    let Some(status) = reg.status.get(&name) else { return };
    let (Some(up), Some(members)) = (status.up.clone(), status.members.clone()) else { return };
    let mut e = export_lock(&members);
    if issi == 0 || issi > GSSI_MASK || e.have.contains_key(&issi) || e.visitors.contains(&issi) || e.have.len() >= MAX_TALKERS { return; }
    match e.allow(issi, Instant::now()) {
        Allow::Yes => { let _ = up.send(protocol::build_subscriber_message(protocol::SUB_REGISTER, issi, &[])); e.have.insert(issi, BTreeSet::new()); e.settle(); }
        Allow::Ask => { let _ = up.send(build_check(&[issi])); e.asked(&[issi], Instant::now()); }
        Allow::No => {}
    }
}

/// Starts the links (called once the services run).
pub async fn run(state: Arc<AppState>) {
    state.netlinks.lock().started = true;
    reconcile(&state);
}

/// Brings the running links in line with `state.networks`: stops the ones
/// removed, switched off or changed, starts the missing ones.
pub fn reconcile(state: &Arc<AppState>) {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    let slots = state.netlinks.slots(&cfg);
    let mut reg = state.netlinks.lock();
    reg.status.retain(|name, _| cfg.networks.iter().any(|n| &n.name == name));
    if !reg.started { return; }
    let stale: Vec<String> = reg.tasks.iter()
        .filter(|(name, task)| !cfg.enabled || !cfg.networks.iter().any(|n| n.enabled && &n.name == *name && *n == task.cfg))
        .map(|(name, _)| name.clone()).collect();
    for name in stale {
        info!(network = %name, "multi network: stopping link");
        if let Some(task) = reg.tasks.remove(&name) { let _ = task.stop.send(true); }
        reg.status.remove(&name);
    }
    for net in cfg.networks.iter().filter(|n| cfg.enabled && n.enabled) {
        if reg.tasks.contains_key(&net.name) { continue; }
        let Some(&slot) = slots.get(&net.name) else { continue };
        let (stop, stop_rx) = watch::channel(false);
        reg.next_gen += 1;
        let gen = reg.next_gen;
        reg.tasks.insert(net.name.clone(), LinkTask { cfg: net.clone(), stop, gen });
        reg.status.insert(net.name.clone(), LinkStatus::default());
        let (state, net) = (state.clone(), net.clone());
        tokio::spawn(async move { link_loop(state, net, slot, gen, stop_rx).await });
    }
}

/// Updates the state of link task `gen` of network `name`; a task that has
/// been stopped or replaced changes nothing.
fn set_status(state: &AppState, name: &str, gen: u64, f: impl FnOnce(&mut LinkStatus)) {
    let mut reg = state.netlinks.lock();
    if reg.tasks.get(name).is_some_and(|t| t.gen == gen) {
        f(reg.status.entry(name.to_owned()).or_default());
    }
}

fn stopped(stop: &watch::Receiver<bool>) -> bool {
    *stop.borrow() || stop.has_changed().is_err()
}

async fn link_loop(state: Arc<AppState>, net: NetworkConfig, slot: u8, gen: u64, mut stop: watch::Receiver<bool>) {
    let interval = Duration::from_secs(net.reconnect_interval_seconds.max(5));
    loop {
        if stopped(&stop) { break; }
        info!(network = %net.name, remote_host = %net.remote_host, "multi network: connecting");
        match connect_and_run(&state, &net, slot, gen, &mut stop).await {
            Ok(()) => set_status(&state, &net.name, gen, |s| { s.connected = false; s.up = None; }),
            Err(e) => {
                warn!(network = %net.name, error = %format!("{e:#}"), "multi network: link failed");
                set_status(&state, &net.name, gen, |s| { s.connected = false; s.up = None; s.last_error = format!("{e:#}"); });
            }
        }
        crate::visitors::announce(&state, None).await;
        if stopped(&stop) { break; }
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = stop.changed() => {}
        }
    }
    info!(network = %net.name, "multi network: link stopped");
}

/// The `User-Agent` a link introduces itself with: `user_agent` when it is a
/// valid header value (printable ASCII), else this server's own.
pub fn user_agent(net: &NetworkConfig) -> &str {
    let ua = net.user_agent.trim();
    if valid_user_agent(ua) && !ua.is_empty() { ua } else { federation::USER_AGENT }
}

fn valid_user_agent(ua: &str) -> bool {
    ua.len() <= MAX_USER_AGENT && ua.bytes().all(|b| (0x20..0x7f).contains(&b))
}

async fn connect_and_run(state: &Arc<AppState>, net: &NetworkConfig, slot: u8, gen: u64, stop: &mut watch::Receiver<bool>) -> anyhow::Result<()> {
    let peer = net.as_peer();
    let tls = if peer.tls { Some(federation::tls_connector(&peer)?) } else { None };
    let who = LinkIdentity { mode: "Basestation", user_agent: user_agent(net) };
    let dial = async {
        // Paths as configured / discovered: a trailing slash may matter to
        // the far server.
        let path = leading_slash(&peer.path);
        let ws_path = federation::discover(&peer, tls.as_ref(), &path, &[], who).await?;
        let remote_addr = tokio::net::lookup_host(&peer.remote_host).await.ok().and_then(|mut it| it.next());
        let scheme = if tls.is_some() { "wss" } else { "ws" };
        let ws_url = format!("{scheme}://{}{}", peer.remote_host, leading_slash(&ws_path));
        let mut request = ws_url.into_client_request()?;
        request.headers_mut().insert("User-Agent", who.user_agent.parse()?);
        request.headers_mut().insert("X-Brew-Mode", "Basestation".parse()?);
        request.headers_mut().insert("X-Brew-Version", protocol::BREW_PROTOCOL_VERSION.to_string().parse()?);
        request.headers_mut().insert("Sec-WebSocket-Protocol", "brew".parse()?);
        let stream = federation::dial(&peer, tls.as_ref()).await?;
        let (ws, resp) = tokio_tungstenite::client_async(request, stream).await?;
        // A brew-server there answers the ISSI check.
        let checks = resp.headers().get(crate::carriers::X_BREW_EXTENSIONS).and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|x| x.trim() == EXT_ISSI_CHECK));
        anyhow::Ok((ws, remote_addr, checks))
    };
    let (ws, remote_addr, checks) = tokio::select! {
        r = tokio::time::timeout(federation::DIAL_TIMEOUT, dial) => r
            .map_err(|_| anyhow::anyhow!("timed out after {}s connecting", federation::DIAL_TIMEOUT.as_secs()))??,
        _ = stop.changed() => return Ok(()),
    };
    info!(network = %net.name, slot, "multi network: link up");
    run_session(state, net, slot, gen, ws, (remote_addr, checks), stop).await;
    Ok(())
}

fn leading_slash(path: &str) -> String {
    let p = path.trim();
    if p.starts_with('/') { p.to_string() } else { format!("/{p}") }
}

/// Groups of network `name` that the switched-on pools bridge.
pub fn bridged_groups(pools: &[TalkgroupPoolConfig], name: &str) -> BTreeSet<u32> {
    pools.iter().filter(|p| p.enabled)
        .flat_map(|p| p.remote.iter())
        .filter(|r| r.network == name)
        .map(|r| r.gssi)
        .collect()
}

fn pools_now(state: &AppState) -> Vec<TalkgroupPoolConfig> {
    state.pools.read().map(|p| p.clone()).unwrap_or_default()
}

/// Upstream messages that bring the bridge ISSI's affiliations from `have`
/// to `want` (nothing without a bridge ISSI).
fn affiliation_changes(issi: u32, have: &BTreeSet<u32>, want: &BTreeSet<u32>) -> Vec<Vec<u8>> {
    if issi == 0 { return Vec::new(); }
    let mut out = Vec::new();
    let gone: Vec<u32> = have.difference(want).copied().collect();
    let new: Vec<u32> = want.difference(have).copied().collect();
    if !gone.is_empty() { out.push(protocol::build_subscriber_message(protocol::SUB_DEAFFILIATE, issi, &gone)); }
    if !new.is_empty() { out.push(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, issi, &new)); }
    out
}

/// After a pool change: every connected link affiliates its bridge ISSI to
/// the groups now bridged and leaves the others.
pub fn refresh_affiliations(state: &AppState) {
    let pools = pools_now(state);
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    let mut reg = state.netlinks.lock();
    for net in &cfg.networks {
        let Some(status) = reg.status.get_mut(&net.name) else { continue };
        let Some(up) = status.up.clone() else { continue };
        let want = bridged_groups(&pools, &net.name);
        for msg in affiliation_changes(net.bridge_issi, &status.affiliated, &want) { let _ = up.send(msg); }
        if net.bridge_issi != 0 { status.affiliated = want; }
    }
}

/// Radios to have registered in network `name`, with their groups there:
/// every radio registered here (not over a peer or another network) that is
/// affiliated to a local group of a switched-on pool bridging that network
/// gets the pool's groups of that network. The bridge ISSI is left out.
pub fn mirror<'a>(
    pools: &[TalkgroupPoolConfig],
    name: &str,
    bridge_issi: u32,
    radios: impl Iterator<Item = (u32, &'a HashSet<u32>)>,
) -> HashMap<u32, BTreeSet<u32>> {
    let bridges: Vec<(&[u32], Vec<u32>)> = pools.iter().filter(|p| p.enabled).filter_map(|p| {
        let there: Vec<u32> = p.remote.iter().filter(|r| r.network == name).map(|r| r.gssi).collect();
        (!there.is_empty() && !p.groups.is_empty()).then_some((p.groups.as_slice(), there))
    }).collect();
    let mut out: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    if bridges.is_empty() { return out; }
    for (issi, groups) in radios {
        if issi == 0 || issi > GSSI_MASK || issi == bridge_issi { continue; }
        for (here, there) in &bridges {
            if here.iter().any(|g| groups.contains(g)) {
                out.entry(issi).or_default().extend(there.iter().copied());
            }
        }
    }
    out
}

/// Does "register all" of network `n` export radio `issi` of this
/// connection? A Basestation with the `mni` capability or named in
/// `export_stations`; a Terminal (the app) only when its login or its ISSI
/// is named there; never a peer or another network's link.
pub fn exports_station(n: &NetworkConfig, mode: ClientMode, login: Option<&str>, capabilities: &[String], issi: u32) -> bool {
    let named = |x: &str| n.export_stations.iter().any(|s| s.trim() == x);
    match mode {
        ClientMode::Basestation => capabilities.iter().any(|c| c == CAPABILITY_MNI) || login.is_some_and(named),
        ClientMode::Terminal => login.is_some_and(named) || named(&issi.to_string()),
        _ => false,
    }
}

/// Upstream messages that bring the radios registered there from `have` to
/// `want`: new radios register and affiliate, radios that left every bridged
/// group deregister, the rest affiliate / deaffiliate the difference. A
/// radio in `keep` (talking right now) is not deregistered.
pub fn member_changes(have: &mut HashMap<u32, BTreeSet<u32>>, want: &HashMap<u32, BTreeSet<u32>>, keep: &HashSet<u32>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut issis: Vec<u32> = have.keys().chain(want.keys()).copied().collect();
    issis.sort_unstable();
    issis.dedup();
    for issi in issis {
        let empty = BTreeSet::new();
        let w = want.get(&issi).unwrap_or(&empty);
        match have.get_mut(&issi) {
            None => {
                if have.len() >= MAX_TALKERS { continue; }
                out.push(protocol::build_subscriber_message(protocol::SUB_REGISTER, issi, &[]));
                let groups: Vec<u32> = w.iter().copied().collect();
                out.push(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, issi, &groups));
                have.insert(issi, w.clone());
            }
            Some(_) if w.is_empty() => {
                if keep.contains(&issi) { continue; }
                out.push(protocol::build_subscriber_message(protocol::SUB_DEREGISTER, issi, &[]));
                have.remove(&issi);
            }
            Some(h) => {
                out.extend(affiliation_changes(issi, h, w));
                *h = w.clone();
            }
        }
    }
    out
}

/// After a registration, affiliation or pool change: every link that
/// registers radios brings them in line with [`mirror`] (a radio joins a
/// bridged group there while it is in the group here, and leaves when it
/// leaves it here).
pub async fn sync_members(state: &AppState) {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    if !cfg.enabled || !cfg.networks.iter().any(|n| n.enabled && n.register_talkers) { return; }
    let pools = pools_now(state);
    let slots = state.netlinks.slots(&cfg);
    let (wants, talking) = {
        let inner = state.inner.read().await;
        let local = |c: &crate::state::ClientId| inner.clients.get(c).is_some_and(|c| matches!(c.mode, ClientMode::Terminal | ClientMode::Basestation));
        let radios: Vec<(u32, &HashSet<u32>)> = inner.subscribers.iter()
            .filter(|(_, s)| s.route.path.is_empty() && local(&s.client_id))
            .map(|(i, s)| (*i, &s.groups)).collect();
        // Radios of the stations "register all" exports: a station with the
        // `mni` capability or one named in `export_stations`; the app
        // (Terminal mode) only when named there; never radios from peers or
        // other networks.
        let exporting = |n: &NetworkConfig, c: &crate::state::ClientId, issi: u32| inner.clients.get(c).is_some_and(|cl| {
            let caps = inner.carriers.info(*c).map(|i| i.capabilities.as_slice()).unwrap_or_default();
            exports_station(n, cl.mode, cl.username.as_deref(), caps, issi)
        });
        let wants: Vec<(String, HashMap<u32, BTreeSet<u32>>)> = cfg.networks.iter()
            .filter(|n| n.enabled && n.register_talkers)
            .map(|n| {
                let mut want = mirror(&pools, &n.name, n.bridge_issi, radios.iter().map(|(i, g)| (*i, *g)));
                // Groups of that network the radios attached themselves (a
                // radio migrated onto a cell of that network, see
                // `visitors::group_alias`).
                if let Some(&slot) = slots.get(&n.name) {
                    for (issi, groups) in &radios {
                        if *issi == 0 || *issi > GSSI_MASK || *issi == n.bridge_issi { continue; }
                        for (s, g) in groups.iter().filter_map(|a| split_alias(*a)) {
                            if s == slot { want.entry(*issi).or_default().insert(g); }
                        }
                    }
                }
                if n.register_all {
                    for (issi, sub) in inner.subscribers.iter() {
                        if *issi == 0 || *issi > GSSI_MASK || *issi == n.bridge_issi || !sub.route.path.is_empty() { continue; }
                        if exporting(n, &sub.client_id, *issi) { want.entry(*issi).or_default(); }
                    }
                }
                (n.name.clone(), want)
            })
            .collect();
        let talking: HashSet<u32> = inner.calls.values().map(|c| c.source_issi).collect();
        (wants, talking)
    };
    let reg = state.netlinks.lock();
    for (name, want) in wants {
        let Some(status) = reg.status.get(&name) else { continue };
        let (Some(up), Some(members)) = (status.up.clone(), status.members.clone()) else { continue };
        let mut e = export_lock(&members);
        let now = Instant::now();
        // An ISSI not registered there yet goes only if that network may
        // have it: never-export list, and the ISSI check where offered.
        let mut ask = Vec::new();
        let want: HashMap<u32, BTreeSet<u32>> = want.into_iter().filter(|(issi, _)| match e.allow(*issi, now) {
            Allow::Yes => true,
            Allow::No => false,
            Allow::Ask => { if ask.len() < MAX_CHECK { ask.push(*issi); } false }
        }).collect();
        if !ask.is_empty() {
            e.asked(&ask, now);
            let _ = up.send(build_check(&ask));
        }
        let Export { have, .. } = &mut *e;
        for msg in member_changes(have, &want, &talking) { let _ = up.send(msg); }
        e.settle();
    }
}

/// Call identifiers of a link: this server's own on one side, the other
/// network's on the other. A call crossing a link gets a new identifier
/// there, so a copy that comes back (a loop through the same server, or
/// over another way) is never taken for the call it came from, and an
/// answer in a call over there finds the call it answers here.
#[derive(Debug, Default)]
pub struct CallIds {
    to_remote: HashMap<Uuid, Uuid>,
    to_local: HashMap<Uuid, Uuid>,
    /// Group calls started here and sent over (local ids): only this side's
    /// own end closes them, see `Inbound::translate_with`.
    ours: HashSet<Uuid>,
}

impl CallIds {
    fn pair(&mut self, local: Uuid, remote: Uuid) {
        if self.to_remote.len() >= MAX_CALLS { self.to_remote.clear(); self.to_local.clear(); self.ours.clear(); }
        self.to_remote.insert(local, remote);
        self.to_local.insert(remote, local);
    }

    fn forget_local(&mut self, local: &Uuid) {
        if let Some(r) = self.to_remote.remove(local) { self.to_local.remove(&r); }
        self.ours.remove(local);
    }
}

/// Shared by the two directions of one link.
pub type SharedCallIds = Arc<Mutex<CallIds>>;

fn ids_lock(ids: &SharedCallIds) -> std::sync::MutexGuard<'_, CallIds> {
    ids.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a link lets out: group calls on an alias of its own slot (with the
/// real GSSI put back) and the call control and frames of those calls, all
/// under the call's identifier over there. Everything else this server sends
/// its connections stays here.
#[derive(Debug)]
pub struct Outbound {
    slot: u8,
    ids: SharedCallIds,
    /// Talkers registered over there by this link, with their groups
    /// (`None` = talkers are not registered).
    talkers: Option<SharedExport>,
    bridge_issi: u32,
}

/// What a link exports into the other network: the radios it registered
/// there, the answers of that network's ISSI check and the conflicts seen.
#[derive(Debug, Default)]
pub struct Export {
    /// Radios registered there by this link, with their groups there.
    pub have: HashMap<u32, BTreeSet<u32>>,
    /// That network answers `SERVICE_ISSI_CHECK` (offered `issi-check`).
    pub checks: bool,
    /// ISSIs never registered there (`never_export`).
    pub never: HashSet<u32>,
    /// Check answers: taken there (true) or free, and when.
    verdict: HashMap<u32, (bool, Instant)>,
    /// Asked, no answer yet.
    pending: HashMap<u32, Instant>,
    /// Newest first, at most [`MAX_CONFLICTS`].
    pub conflicts: VecDeque<Conflict>,
    /// Visitors from that network on this server's stations: they are that
    /// network's radios, registered there by `visitors`, not as talkers.
    pub visitors: HashSet<u32>,
    /// Last conflict per ISSI (rate limit).
    last_conflict: HashMap<u32, Instant>,
    /// Name of the network, for the log.
    pub network: String,
    /// When each radio in `have` was registered there (ms since the epoch),
    /// kept in step by [`Export::settle`].
    pub since: HashMap<u32, u64>,
}

/// One radio a link registered in the other network, for the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct ExportedView {
    pub issi: u32,
    /// Its groups there (that network's GSSIs).
    pub groups: Vec<u32>,
    pub since_ms: u64,
}

/// One ISSI collision seen on a link, for the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub issi: u32,
    pub at_ms: u64,
    pub what: String,
}

/// May a link register `issi` there?
#[derive(Debug, PartialEq, Eq)]
pub enum Allow { Yes, No, Ask }

/// A "taken" answer is asked again after this long.
const TAKEN_FOR: Duration = Duration::from_secs(300);
/// An unanswered check is asked again after this long.
const PENDING_FOR: Duration = Duration::from_secs(10);
const MAX_CONFLICTS: usize = 50;
/// Most ISSIs one `SERVICE_ISSI_CHECK` asks about.
pub const MAX_CHECK: usize = 256;

impl Export {
    pub fn allow(&self, issi: u32, now: Instant) -> Allow {
        if self.never.contains(&issi) { return Allow::No; }
        if !self.checks || self.have.contains_key(&issi) { return Allow::Yes; }
        match self.verdict.get(&issi) {
            Some((false, _)) => Allow::Yes,
            Some((true, at)) if now.duration_since(*at) < TAKEN_FOR => Allow::No,
            _ if self.pending.get(&issi).is_some_and(|at| now.duration_since(*at) < PENDING_FOR) => Allow::No,
            _ => Allow::Ask,
        }
    }

    fn asked(&mut self, issis: &[u32], now: Instant) {
        for i in issis { self.pending.insert(*i, now); }
    }

    /// Takes in a check answer; returns the ISSIs found taken.
    pub fn answered(&mut self, taken: &[u32], free: &[u32], now: Instant) -> Vec<u32> {
        for i in free { self.pending.remove(i); self.verdict.insert(*i, (false, now)); }
        for i in taken { self.pending.remove(i); self.verdict.insert(*i, (true, now)); }
        if self.verdict.len() > 4 * MAX_TALKERS { self.verdict.retain(|_, (_, at)| now.duration_since(*at) < TAKEN_FOR); }
        taken.to_vec()
    }

    /// Brings `since` in step with `have` after a change and logs each radio
    /// registered there or deregistered there. Brew has no answer to a
    /// registration, so this is what this server sent, not a confirmation.
    pub fn settle(&mut self) {
        let now = crate::telemetry::now_ms();
        let mut gone: Vec<u32> = self.since.keys().filter(|i| !self.have.contains_key(i)).copied().collect();
        gone.sort_unstable();
        for issi in gone {
            self.since.remove(&issi);
            info!(network = %self.network, issi, "multi network: ISSI {issi} deregistered in {}", self.network);
        }
        let mut new: Vec<u32> = self.have.keys().filter(|i| !self.since.contains_key(i)).copied().collect();
        new.sort_unstable();
        for issi in new {
            self.since.insert(issi, now);
            info!(network = %self.network, issi, "multi network: ISSI {issi} registered in {}", self.network);
        }
    }

    /// The radios registered there, by ISSI, for the dashboard.
    pub fn exported_view(&self) -> Vec<ExportedView> {
        let mut v: Vec<ExportedView> = self.have.iter().map(|(issi, groups)| ExportedView {
            issi: *issi, groups: groups.iter().copied().collect(), since_ms: self.since.get(issi).copied().unwrap_or(0),
        }).collect();
        v.sort_unstable_by_key(|x| x.issi);
        v
    }

    /// Notes a conflict (at most one a minute per ISSI).
    pub fn conflict(&mut self, issi: u32, what: String, now: Instant) -> bool {
        if self.last_conflict.get(&issi).is_some_and(|at| now.duration_since(*at) < Duration::from_secs(60)) { return false; }
        self.last_conflict.insert(issi, now);
        if self.last_conflict.len() > MAX_TALKERS { self.last_conflict.retain(|_, at| now.duration_since(*at) < Duration::from_secs(60)); }
        self.conflicts.push_front(Conflict { issi, at_ms: crate::telemetry::now_ms(), what });
        self.conflicts.truncate(MAX_CONFLICTS);
        true
    }
}

pub type SharedExport = Arc<std::sync::Mutex<Export>>;

fn export_lock(e: &SharedExport) -> std::sync::MutexGuard<'_, Export> {
    e.lock().unwrap_or_else(|e| e.into_inner())
}

/// `SERVICE_ISSI_CHECK` question: `{"query":[issi,..]}`.
pub fn build_check(issis: &[u32]) -> Vec<u8> {
    protocol::build_service(protocol::SERVICE_ISSI_CHECK, &serde_json::json!({ "query": issis }).to_string())
}

/// Extension a server offers when it answers `SERVICE_ISSI_CHECK`.
pub const EXT_ISSI_CHECK: &str = "issi-check";
/// Station capability (`SERVICE_BTS_INFO`) of a station with the `mni`
/// extension: "register all" exports its radios.
pub const CAPABILITY_MNI: &str = "mni";

/// Bound on the talkers a link keeps registered.
const MAX_TALKERS: usize = 1024;

/// What a link lets in: group calls on the groups it bridges (with the alias
/// put on) and the call control and frames of those calls, under this
/// server's identifier of the call.
#[derive(Debug)]
pub struct Inbound {
    slot: u8,
    ids: SharedCallIds,
}

/// Bound on the calls a link remembers (a call whose end was lost).
const MAX_CALLS: usize = 4096;
const DEST_AT: usize = 2 + 16 + 4;

fn call_id(pkt: &[u8]) -> Option<Uuid> {
    pkt.get(2..18).and_then(|b| Uuid::from_slice(b).ok())
}

fn set_call_id(pkt: &mut [u8], id: Uuid) {
    pkt[2..18].copy_from_slice(id.as_bytes());
}

fn is_call_or_frame(pkt: &[u8]) -> bool {
    matches!(pkt.first(), Some(&protocol::CLASS_CALL_CONTROL) | Some(&protocol::CLASS_FRAME))
}

fn is_group_tx(pkt: &[u8]) -> bool {
    pkt.first() == Some(&protocol::CLASS_CALL_CONTROL) && pkt.get(1) == Some(&protocol::CALL_GROUP_TX) && pkt.len() >= DEST_AT + 4
}

/// The start of an individual call or SDS (setup request, SDS header).
fn is_individual_start(pkt: &[u8]) -> bool {
    pkt.first() == Some(&protocol::CLASS_CALL_CONTROL)
        && matches!(pkt.get(1), Some(&protocol::CALL_SETUP_REQUEST) | Some(&protocol::CALL_SHORT_TRANSFER))
        && pkt.len() >= DEST_AT + 4
}

/// The end of an individual call.
fn is_call_end(pkt: &[u8]) -> bool {
    pkt.first() == Some(&protocol::CLASS_CALL_CONTROL)
        && matches!(pkt.get(1), Some(&protocol::CALL_RELEASE) | Some(&protocol::CALL_SETUP_REJECT))
}

fn is_group_idle(pkt: &[u8]) -> bool {
    pkt.first() == Some(&protocol::CLASS_CALL_CONTROL) && pkt.get(1) == Some(&protocol::CALL_GROUP_IDLE)
}

fn dest(pkt: &[u8]) -> u32 {
    u32::from_le_bytes(pkt[DEST_AT..DEST_AT + 4].try_into().unwrap_or_default())
}

fn src(pkt: &[u8]) -> u32 {
    u32::from_le_bytes(pkt[DEST_AT - 4..DEST_AT].try_into().unwrap_or_default())
}

/// One readable log line for a call crossing the link (GROUP_TX = a new
/// over, GROUP_IDLE = call end); other packets are not logged.
fn log_crossing(network: &str, dir: &str, before: &[u8], after: Option<&[u8]>) {
    if is_group_tx(before) {
        let (talker, gssi) = (src(before), dest(before));
        match (dir, after) {
            ("in", Some(_)) => info!("multi network: call in from {network}:{gssi}, talker {talker}"),
            ("in", None) => debug!("multi network: call on {network}:{gssi} not bridged, dropped"),
            (_, Some(p)) => info!("multi network: call out to {network}:{}, talker {talker}", dest(p)),
            _ => {}
        }
    } else if is_group_idle(before) && after.is_some() {
        info!("multi network: call {dir} ended ({network})");
    }
}

fn set_dest(pkt: &mut [u8], value: u32) {
    pkt[DEST_AT..DEST_AT + 4].copy_from_slice(&value.to_le_bytes());
}

impl Outbound {
    pub fn new(slot: u8, ids: SharedCallIds) -> Self { Self { slot, ids, talkers: None, bridge_issi: 0 } }

    /// Also register talkers over there before their first over.
    pub fn registering(mut self, talkers: SharedExport, bridge_issi: u32) -> Self {
        self.talkers = Some(talkers);
        self.bridge_issi = bridge_issi;
        self
    }

    /// Everything the link sends for `pkt`: the talker's registration and
    /// affiliation there when it is new (as a Basestation does for a radio
    /// before it talks), then the packet as it goes over, or nothing if
    /// the packet stays here.
    pub fn packets(&mut self, pkt: Vec<u8>) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if let (Some(talkers), true) = (&self.talkers, is_group_tx(&pkt)) {
            let talker = src(&pkt);
            if let Some((slot, gssi)) = split_alias(dest(&pkt)) {
                if slot == self.slot && talker != 0 && talker <= GSSI_MASK && talker != self.bridge_issi {
                    let mut e = export_lock(talkers);
                    if e.visitors.contains(&talker) { drop(e); return self.translate(pkt).into_iter().collect(); }
                    if !e.have.contains_key(&talker) && e.have.len() < MAX_TALKERS {
                        match e.allow(talker, Instant::now()) {
                            Allow::Yes => {
                                out.push(protocol::build_subscriber_message(protocol::SUB_REGISTER, talker, &[]));
                                e.have.insert(talker, BTreeSet::new());
                                e.settle();
                            }
                            // Asked first; the talker passes unregistered
                            // until the answer says it is free there.
                            Allow::Ask => { out.push(build_check(&[talker])); e.asked(&[talker], Instant::now()); }
                            Allow::No => {}
                        }
                    }
                    if let Some(groups) = e.have.get_mut(&talker) {
                        if groups.insert(gssi) {
                            out.push(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, talker, &[gssi]));
                        }
                    }
                }
            }
        }
        if let Some(p) = self.translate(pkt) { out.push(p); }
        out
    }

    /// The packet as it goes to the other network, or `None` if it stays.
    pub fn translate(&mut self, mut pkt: Vec<u8>) -> Option<Vec<u8>> {
        if !is_call_or_frame(&pkt) { return None; }
        let local = call_id(&pkt)?;
        let mut ids = ids_lock(&self.ids);
        if is_group_tx(&pkt) {
            let (slot, gssi) = split_alias(dest(&pkt))?;
            if slot != self.slot { return None; }
            set_dest(&mut pkt, gssi);
            let remote = match ids.to_remote.get(&local) {
                Some(r) => *r,
                None => { let r = Uuid::new_v4(); ids.pair(local, r); ids.ours.insert(local); r }
            };
            set_call_id(&mut pkt, remote);
            return Some(pkt);
        }
        // An individual call or SDS the router sends this way (Multi Network
        // decided it, see `visitors::individual_route`): a new id there.
        if is_individual_start(&pkt) && !ids.to_remote.contains_key(&local) {
            let r = Uuid::new_v4();
            ids.pair(local, r);
        }
        let remote = *ids.to_remote.get(&local)?;
        if is_group_idle(&pkt) || is_call_end(&pkt) { ids.forget_local(&local); }
        set_call_id(&mut pkt, remote);
        Some(pkt)
    }
}

impl Inbound {
    pub fn new(slot: u8, ids: SharedCallIds) -> Self { Self { slot, ids } }

    /// The packet as the router gets it, or `None` if it is dropped.
    /// `bridged` says whether a group of that network is in a pool.
    #[cfg(test)]
    pub fn translate(&mut self, pkt: Vec<u8>, bridged: impl Fn(u32) -> bool) -> Option<Vec<u8>> {
        self.translate_with(pkt, bridged, |_| false)
    }

    /// As [`Inbound::translate`]; `admit` says whether an individual call or
    /// SDS to an ISSI may come in (a visitor from that network here, or one
    /// of this server's radios registered there).
    pub fn translate_with(&mut self, mut pkt: Vec<u8>, bridged: impl Fn(u32) -> bool, admit: impl Fn(u32) -> bool) -> Option<Vec<u8>> {
        if !is_call_or_frame(&pkt) { return None; }
        let remote = call_id(&pkt)?;
        let mut ids = ids_lock(&self.ids);
        if is_group_tx(&pkt) {
            let gssi = dest(&pkt);
            if gssi == 0 || gssi > GSSI_MASK || !bridged(gssi) { return None; }
            set_dest(&mut pkt, alias(self.slot, gssi));
            // A call of ours over there (an answer in it) keeps our id.
            let local = match ids.to_local.get(&remote) {
                Some(l) => *l,
                None => { let l = Uuid::new_v4(); ids.pair(l, remote); l }
            };
            set_call_id(&mut pkt, local);
            return Some(pkt);
        }
        if is_individual_start(&pkt) && !ids.to_local.contains_key(&remote) {
            if !admit(dest(&pkt)) { return None; }
            let l = Uuid::new_v4();
            ids.pair(l, remote);
        }
        let local = *ids.to_local.get(&remote)?;
        // That network ending a group call that started here (TetraPack
        // answers some overs with GROUP_IDLE at once): the ids stay paired
        // until this side's own end has gone over, else that end and the
        // voice never reach it and the call hangs there.
        if is_group_idle(&pkt) && ids.ours.contains(&local) {
            info!(uuid=%local, "multi network: the other network ended a call of ours; its end from here still goes over");
        } else if is_group_idle(&pkt) || is_call_end(&pkt) {
            ids.forget_local(&local);
        }
        set_call_id(&mut pkt, local);
        Some(pkt)
    }
}

async fn run_session(
    state: &Arc<AppState>,
    net: &NetworkConfig,
    slot: u8,
    gen: u64,
    ws: tokio_tungstenite::WebSocketStream<Box<dyn federation::PeerIo>>,
    // Remote address, and whether that server answers the ISSI check.
    (remote_addr, checks): (Option<std::net::SocketAddr>, bool),
    stop: &mut watch::Receiver<bool>,
) {
    let id = Uuid::new_v4();
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (up_tx, mut up_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    {
        let mut inner = state.inner.write().await;
        inner.clients.insert(id, Client {
            tx, mode: ClientMode::Network, version: ConnVersion::V1, version_announced: false,
            remote_addr, connected_at_ms: crate::telemetry::now_ms(), username: Some(net.username.clone()),
        });
        inner.netlinks.insert(slot, id);
    }
    // The bridge ISSI registers there and affiliates to the bridged groups,
    // so that network's server sends their calls over this link.
    let want = bridged_groups(&pools_now(state), &net.name);
    if net.bridge_issi != 0 {
        let _ = up_tx.send(protocol::build_subscriber_message(protocol::SUB_REGISTER, net.bridge_issi, &[]));
        for msg in affiliation_changes(net.bridge_issi, &BTreeSet::new(), &want) { let _ = up_tx.send(msg); }
    }
    let talkers: SharedExport = Arc::default();
    {
        let mut e = export_lock(&talkers);
        e.checks = checks;
        e.never = net.never_export.iter().copied().collect();
        e.network = net.name.clone();
    }
    set_status(state, &net.name, gen, |s| {
        // Conflicts seen before stay on the dashboard.
        if let Some(old) = &s.members { export_lock(&talkers).conflicts = export_lock(old).conflicts.clone(); }
        s.connected = true;
        s.since_ms = crate::telemetry::now_ms();
        s.last_error.clear();
        s.up = Some(up_tx.clone());
        s.affiliated = if net.bridge_issi != 0 { want } else { BTreeSet::new() };
        s.members = Some(talkers.clone());
    });
    // The radios already in bridged groups here join them there, and the
    // visitors from that network register there again.
    {
        let inner = state.inner.read().await;
        export_lock(&talkers).visitors = crate::visitors::issis_of(&inner, slot);
        for msg in crate::visitors::registrations(&inner, slot) { let _ = up_tx.send(msg); }
    }
    sync_members(state).await;
    crate::visitors::announce(state, None).await;

    let keepalive = Keepalive::from_config(&state.config.federation);
    let mut ping = keepalive.map(Keepalive::ping_timer);
    let ids: SharedCallIds = Arc::default();
    let mut outbound = Outbound::new(slot, ids.clone());
    if net.register_talkers { outbound = outbound.registering(talkers.clone(), net.bridge_issi); }
    let out_name = net.name.clone();
    let writer = tokio::spawn(async move {
        loop {
            let packet = tokio::select! {
                p = rx.recv() => match p {
                    Some(p) => {
                        let probe = (is_group_tx(&p) || is_group_idle(&p)).then(|| p.clone());
                        let out = outbound.packets(p);
                        if let Some(before) = probe { log_crossing(&out_name, "out", &before, out.last().map(|p| p.as_slice()).filter(|p| p.first() == Some(&protocol::CLASS_CALL_CONTROL))); }
                        out
                    }
                    None => break,
                },
                p = up_rx.recv() => match p { Some(p) => vec![p], None => break },
                _ = federation::ping_due(&mut ping) => {
                    if ws_tx.send(Message::Ping(Default::default())).await.is_err() { break; }
                    continue;
                }
            };
            let mut gone = false;
            for p in packet {
                if ws_tx.send(Message::Binary(p.into())).await.is_err() { gone = true; break; }
            }
            if gone { break; }
        }
        let _ = ws_tx.close().await;
    });

    let ours = ids.clone();
    let mut inbound = Inbound::new(slot, ids);
    let name = net.name.clone();
    loop {
        let item = tokio::select! {
            item = federation::next_frame(&mut ws_rx, keepalive) => item,
            _ = stop.changed() => break,
        };
        let item = match item {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => { warn!(network = %name, "multi network: link silent past keepalive timeout; closing"); break; }
        };
        match item {
            Ok(Message::Binary(data)) => {
                if data.len() > 2 && data[0] == protocol::CLASS_SERVICE && data[1] == protocol::SERVICE_ISSI_CHECK {
                    if checked(&name, &talkers, &data[2..]) { sync_members(state).await; }
                    continue;
                }
                // The same ISSI talking over there while this link has it
                // registered there: two radios with one ISSI.
                if is_group_tx(&data) && call_id(&data).is_some_and(|c| !ids_lock(&ours).to_local.contains_key(&c)) {
                    let talker = src(&data);
                    let mut e = export_lock(&talkers);
                    if e.have.contains_key(&talker) && e.conflict(talker, format!("talked in {name} while registered there from here (same ISSI twice?)"), Instant::now()) {
                        warn!(network = %name, issi = talker, "multi network: ISSI {talker} talks in {name} while this server has it registered there");
                    }
                }
                let pools = pools_now(state);
                let mut groups = bridged_groups(&pools, &name);
                if is_group_tx(&data) { groups.extend(crate::visitors::groups_of(&*state.inner.read().await, slot)); }
                let probe = (is_group_tx(&data) || is_group_idle(&data)).then(|| data.to_vec());
                // Individual calls and SDS only to a visitor from there or to
                // one of our radios the link registered there.
                let admitted: HashSet<u32> = if is_individual_start(&data) {
                    let mut a = crate::visitors::issis_of(&*state.inner.read().await, slot);
                    a.extend(export_lock(&talkers).have.keys().copied());
                    a
                } else { HashSet::new() };
                let p = inbound.translate_with(data.to_vec(), |g| groups.contains(&g), |i| admitted.contains(&i));
                if let Some(before) = probe { log_crossing(&name, "in", &before, p.as_deref()); }
                if let Some(p) = p {
                    crate::router::handle_packet(state.clone(), id, p).await;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) => { warn!(network = %name, error = %e, "multi network: read error"); break; }
        }
    }
    // Deregister the bridge ISSI on a deliberate stop (best effort).
    // Deregister the talkers it registered there, too.
    if stopped(stop) {
        let registered: Vec<u32> = export_lock(&talkers).have.keys().copied().collect();
        for issi in &registered {
            let _ = up_tx.send(protocol::build_subscriber_message(protocol::SUB_DEREGISTER, *issi, &[]));
        }
        if net.bridge_issi != 0 {
            let _ = up_tx.send(protocol::build_subscriber_message(protocol::SUB_DEREGISTER, net.bridge_issi, &[]));
        }
        if !registered.is_empty() { info!(network = %name, count = registered.len(), "multi network: deregistered {} radios in {name} (link stopped)", registered.len()); }
        if net.bridge_issi != 0 || !registered.is_empty() { tokio::time::sleep(Duration::from_millis(200)).await; }
    }
    drop(up_tx);
    writer.abort();
    {
        let mut inner = state.inner.write().await;
        if inner.netlinks.get(&slot) == Some(&id) { inner.netlinks.remove(&slot); }
    }
    state.cleanup_client(id).await;
    info!(network = %name, "multi network: link closed");
}

/// A `SERVICE_ISSI_CHECK` answer on a link (`{"taken":[..],"free":[..]}`):
/// noted, taken ISSIs as conflicts. True if a sync should follow.
fn checked(name: &str, export: &SharedExport, body: &[u8]) -> bool {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct Answer { taken: Vec<u32>, free: Vec<u32> }
    let text = String::from_utf8_lossy(body);
    let Ok(a) = serde_json::from_str::<Answer>(text.trim_end_matches('\0')) else {
        warn!(network = %name, "multi network: malformed ISSI check answer");
        return false;
    };
    let now = Instant::now();
    let mut e = export_lock(export);
    for issi in e.answered(&a.taken, &a.free, now) {
        if e.conflict(issi, format!("already registered in {name}, not exported"), now) {
            warn!(network = %name, issi, "multi network: ISSI {issi} is taken in {name}, not registered there");
        }
    }
    !a.free.is_empty()
}

/// Remote side of `SERVICE_ISSI_CHECK` (`issi-check`): a Basestation (here:
/// another server's link) asks whether ISSIs are in use here. Taken = an
/// ISSI registered here by any other connection, also learnt from a peer.
pub async fn handle_issi_check(state: &AppState, source: ClientId, json: &str) {
    #[derive(serde::Deserialize)]
    struct Query { query: Vec<u32> }
    let Ok(q) = serde_json::from_str::<Query>(json) else {
        debug!(%source, "ISSI check without a query ignored");
        return;
    };
    let inner = state.inner.read().await;
    let (mut taken, mut free) = (Vec::new(), Vec::new());
    for issi in q.query.into_iter().take(MAX_CHECK) {
        if inner.subscribers.get(&issi).is_some_and(|s| s.client_id != source) { taken.push(issi) } else { free.push(issi) }
    }
    info!(%source, taken = taken.len(), free = free.len(), "ISSI check answered");
    if let Some(c) = inner.clients.get(&source) {
        let _ = c.tx.send(protocol::build_service(protocol::SERVICE_ISSI_CHECK, &serde_json::json!({ "taken": taken, "free": free }).to_string()));
    }
}

/// The connection that carries calls to group `alias` of another network,
/// if its link is up.
pub fn link_for(inner: &crate::state::Inner, alias: u32) -> Option<ClientId> {
    let (slot, _) = split_alias(alias)?;
    inner.netlinks.get(&slot).copied()
}

/// An individual call or SDS between `calling` and `called` may not cross
/// the link that is connection `link`: one of them is on that network's
/// `never_forward` list. False for a connection that is not a link.
pub fn never_forward(state: &AppState, inner: &crate::state::Inner, link: ClientId, calling: u32, called: u32) -> bool {
    let Some(name) = link_name(state, inner, link) else { return false };
    state.networks.read().is_ok_and(|cfg| cfg.networks.iter().find(|n| n.name == name)
        .is_some_and(|n| n.never_forward.contains(&calling) || n.never_forward.contains(&called)))
}

/// Connection `link` is the link of a network with "Forward LIP" off: no
/// position report crosses it. False for a connection that is not a link.
pub fn lip_barred(state: &AppState, inner: &crate::state::Inner, link: ClientId) -> bool {
    let Some(name) = link_name(state, inner, link) else { return false };
    state.networks.read().is_ok_and(|cfg| cfg.networks.iter().any(|n| n.name == name && !n.forward_lip))
}

/// Name of the network whose link is connection `id` (`None`: not a link).
pub fn link_name(state: &AppState, inner: &crate::state::Inner, id: ClientId) -> Option<String> {
    if inner.clients.get(&id)?.mode != ClientMode::Network { return None; }
    let slot = inner.netlinks.iter().find(|(_, c)| **c == id).map(|(s, _)| *s)?;
    state.netlinks.lock().slots.iter().find(|(_, s)| **s == slot).map(|(n, _)| n.clone())
}

/// The networks traffic from `source` to `targets` crosses: the one it came
/// from (its source is a link) and the ones it goes to, for the dashboard.
pub fn crossing(state: &AppState, inner: &crate::state::Inner, source: ClientId, targets: impl IntoIterator<Item = ClientId>) -> (Option<String>, Vec<String>) {
    let mut to: Vec<String> = targets.into_iter().filter_map(|t| link_name(state, inner, t)).collect();
    to.sort();
    to.dedup();
    (link_name(state, inner, source), to)
}

/// Shows on the dashboard which networks call `id` crosses (see `crossing`);
/// returns the networks it goes to.
pub async fn note_call(state: &AppState, id: Uuid) -> Vec<String> {
    let (from, to) = {
        let inner = state.inner.read().await;
        let Some(call) = inner.calls.get(&id) else { return Vec::new() };
        crossing(state, &inner, call.owner, call.peers.iter().copied())
    };
    state.monitor.note_networks(id, from, to.clone()).await;
    to
}

/// Checks a network list from the dashboard; the error says what is wrong.
pub fn validate(cfg: &MultiNetworkConfig, pools: &[TalkgroupPoolConfig]) -> Result<(), String> {
    if cfg.name.chars().count() > MAX_NAME { return Err(format!("the network name is longer than {MAX_NAME} characters")); }
    check_mni(cfg.mcc, cfg.mnc, "this server")?;
    let mut names = HashSet::new();
    for n in &cfg.networks {
        let name = n.name.trim();
        if name.is_empty() { return Err("every network needs a name".into()); }
        if name.chars().count() > MAX_NAME { return Err(format!("network name '{name}' is longer than {MAX_NAME} characters")); }
        if !names.insert(name.to_lowercase()) { return Err(format!("two networks are named '{name}'")); }
        if n.enabled && n.remote_host.trim().is_empty() { return Err(format!("network '{name}' needs a server (host:port)")); }
        if !n.remote_host.trim().is_empty() && !has_port(&n.remote_host) {
            return Err(format!("network '{name}': the server needs a port (host:port, e.g. {}:443)", n.remote_host.trim()));
        }
        if n.bridge_issi > GSSI_MASK { return Err(format!("network '{name}': {} is not a valid ISSI (0 to 16777215)", n.bridge_issi)); }
        if n.never_export.len() > MAX_TALKERS { return Err(format!("network '{name}': at most {MAX_TALKERS} ISSIs never to export")); }
        if let Some(bad) = n.never_export.iter().find(|i| **i == 0 || **i > GSSI_MASK) {
            return Err(format!("network '{name}': {bad} is not a valid ISSI to never export (1 to 16777215)"));
        }
        if n.never_forward.len() > MAX_TALKERS { return Err(format!("network '{name}': at most {MAX_TALKERS} ISSIs never to forward")); }
        if let Some(bad) = n.never_forward.iter().find(|i| **i == 0 || **i > GSSI_MASK) {
            return Err(format!("network '{name}': {bad} is not a valid ISSI to never forward (1 to 16777215)"));
        }
        if n.export_stations.len() > 64 { return Err(format!("network '{name}': at most 64 stations to export")); }
        if let Some(bad) = n.export_stations.iter().find(|s| s.is_empty() || s.len() > 64 || s.chars().any(char::is_control)) {
            return Err(format!("network '{name}': station login {bad:?} must be 1 to 64 characters"));
        }
        if !valid_user_agent(n.user_agent.trim()) {
            return Err(format!("network '{name}': the connection name may only use plain characters (no umlauts), up to {MAX_USER_AGENT}"));
        }
        check_mni(n.mcc, n.mnc, name)?;
    }
    for p in pools {
        if let Some(r) = p.remote.iter().find(|r| !cfg.networks.iter().any(|n| n.name == r.network)) {
            return Err(format!("pool '{}' uses network '{}'", p.name, r.network));
        }
    }
    Ok(())
}

/// Whether `host` ends in `:port` (1..=65535): `name:port` or `[v6]:port`.
fn has_port(host: &str) -> bool {
    let host = host.trim();
    let port = match host.strip_prefix('[') {
        Some(rest) => rest.split_once("]:").map(|(_, p)| p),
        None => host.split_once(':').filter(|(_, p)| !p.contains(':')).map(|(_, p)| p),
    };
    port.and_then(|p| p.parse::<u16>().ok()).is_some_and(|p| p != 0)
}

/// MCC is 10 bits and MNC 14 bits (TS 100 392-2 clause 18.4.2.1, D-MLE-SYNC,
/// Table 18.17).
fn check_mni(mcc: u16, mnc: u16, what: &str) -> Result<(), String> {
    if mcc > 1023 { return Err(format!("{what}: MCC {mcc} is out of range (0 to 1023)")); }
    if mnc > 16383 { return Err(format!("{what}: MNC {mnc} is out of range (0 to 16383)")); }
    Ok(())
}

/// Replaces the network list on the running server, writes the config file
/// and restarts only the links whose settings changed. A network sent with
/// an empty password keeps the one it has.
pub async fn apply(state: &Arc<AppState>, mut cfg: MultiNetworkConfig, by: &str) -> Result<(), String> {
    let old = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    cfg.name = cfg.name.trim().to_owned();
    for n in &mut cfg.networks {
        n.name = n.name.trim().to_owned();
        n.remote_host = n.remote_host.trim().to_owned();
        n.user_agent = n.user_agent.trim().to_owned();
        if n.password.is_empty() {
            if let Some(o) = old.networks.iter().find(|o| o.name == n.name) { n.password = o.password.clone(); }
        }
    }
    validate(&cfg, &pools_now(state))?;
    if let Ok(mut live) = state.networks.write() { *live = cfg.clone(); }
    warn!(count = cfg.networks.len(), enabled = cfg.enabled, by, "multi network: networks changed");
    reconcile(state);
    crate::visitors::announce(state, None).await;
    let snapshot = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    let saved = crate::dashboard::save_config(state, &snapshot).await;
    if saved.is_err() {
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    saved.map_err(|e| format!("applied, but writing the config file failed: {e}"))
}

/// What the dashboard shows of one network (never its password).
#[derive(Debug, Serialize)]
pub struct NetworkView {
    pub name: String,
    /// Slot of its group aliases (see `alias`), so the dashboard can name
    /// them.
    pub slot: u8,
    pub enabled: bool,
    pub mcc: u16,
    pub mnc: u16,
    pub remote_host: String,
    pub path: String,
    pub username: String,
    pub has_password: bool,
    pub user_agent: String,
    pub bridge_issi: u32,
    pub register_talkers: bool,
    pub register_all: bool,
    pub export_stations: Vec<String>,
    pub never_export: Vec<u32>,
    pub never_forward: Vec<u32>,
    pub forward_unknown: bool,
    pub forward_lip: bool,
    /// That network answers the ISSI check (a brew-server).
    pub checks: bool,
    /// Radios registered there by the link right now.
    pub exported: Vec<u32>,
    /// The same radios with their groups there and since when.
    pub exported_detail: Vec<ExportedView>,
    pub conflicts: Vec<Conflict>,
    /// Radios of that network on this server's stations (extension `mni`).
    pub visitors: Vec<crate::visitors::VisitorView>,
    pub reconnect_interval_seconds: u64,
    pub tls: bool,
    pub tls_server_name: String,
    pub tls_ca_path: String,
    pub tls_pinned_cert_path: String,
    pub connected: bool,
    pub since_ms: u64,
    pub last_error: String,
    pub bridged: Vec<u32>,
    pub affiliated: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub struct MultiNetworkView {
    pub enabled: bool,
    pub name: String,
    pub mcc: u16,
    pub mnc: u16,
    pub default_user_agent: &'static str,
    pub networks: Vec<NetworkView>,
}

/// The networks with the state of their links, for `/api/networks`.
pub async fn snapshot(state: &AppState) -> MultiNetworkView {
    let cfg = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    let pools = pools_now(state);
    let slots = state.netlinks.slots(&cfg);
    let visitors: HashMap<u8, Vec<crate::visitors::VisitorView>> = {
        let inner = state.inner.read().await;
        slots.values().map(|s| (*s, crate::visitors::view(&inner, *s))).collect()
    };
    let reg = state.netlinks.lock();
    let networks = cfg.networks.iter().map(|n| {
        let s = reg.status.get(&n.name).cloned().unwrap_or_default();
        NetworkView {
            name: n.name.clone(), slot: slots.get(&n.name).copied().unwrap_or(0), enabled: n.enabled, mcc: n.mcc, mnc: n.mnc,
            remote_host: n.remote_host.clone(), path: n.path.clone(), username: n.username.clone(),
            has_password: !n.password.is_empty(), user_agent: n.user_agent.clone(), bridge_issi: n.bridge_issi,
            register_talkers: n.register_talkers, register_all: n.register_all,
            export_stations: n.export_stations.clone(), never_export: n.never_export.clone(), never_forward: n.never_forward.clone(), forward_unknown: n.forward_unknown, forward_lip: n.forward_lip,
            checks: s.connected && s.members.as_ref().is_some_and(|e| export_lock(e).checks),
            exported_detail: if s.connected { s.members.as_ref().map(|e| export_lock(e).exported_view()).unwrap_or_default() } else { Vec::new() },
            exported: if s.connected { s.members.as_ref().map(|e| { let mut v: Vec<u32> = export_lock(e).have.keys().copied().collect(); v.sort_unstable(); v }).unwrap_or_default() } else { Vec::new() },
            conflicts: s.members.as_ref().map(|e| export_lock(e).conflicts.iter().cloned().collect()).unwrap_or_default(),
            visitors: slots.get(&n.name).and_then(|s| visitors.get(s)).cloned().unwrap_or_default(),
            reconnect_interval_seconds: n.reconnect_interval_seconds, tls: n.tls,
            tls_server_name: n.tls_server_name.clone(),
            tls_ca_path: n.tls_ca_path.display().to_string(),
            tls_pinned_cert_path: n.tls_pinned_cert_path.display().to_string(),
            connected: s.connected, since_ms: s.since_ms, last_error: s.last_error,
            bridged: bridged_groups(&pools, &n.name).into_iter().collect(),
            affiliated: s.affiliated.into_iter().collect(),
        }
    }).collect();
    MultiNetworkView { enabled: cfg.enabled, name: cfg.name, mcc: cfg.mcc, mnc: cfg.mnc, default_user_agent: federation::USER_AGENT, networks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RemotePoolGroup;

    fn group_tx(id: Uuid, src: u32, dst: u32) -> Vec<u8> {
        protocol::build_group_tx(&id, src, dst, 0)
    }

    #[test]
    fn register_all_exports_only_stations_with_mni_or_named() {
        let mut n = net("TP");
        n.export_stations = vec!["bs-wien".into()];
        let mni = vec!["handover-v1".to_string(), "mni".to_string()];
        assert!(exports_station(&n, ClientMode::Basestation, Some("other"), &mni, 1));
        assert!(exports_station(&n, ClientMode::Basestation, Some("bs-wien"), &[], 1));
        assert!(!exports_station(&n, ClientMode::Basestation, Some("other"), &[], 1));
        assert!(!exports_station(&n, ClientMode::Basestation, None, &[], 1));
        assert!(!exports_station(&n, ClientMode::Terminal, Some("app"), &mni, 2_320_997), "an app not named is not exported");
        assert!(!exports_station(&n, ClientMode::Peer, Some("bs-wien"), &mni, 1));
        assert!(!exports_station(&n, ClientMode::Network, Some("bs-wien"), &mni, 1));
        // An app named by its login or its ISSI (field request 2026-11-07).
        n.export_stations = vec!["bs-wien".into(), "app-login".into(), "2320997".into()];
        assert!(exports_station(&n, ClientMode::Terminal, Some("app-login"), &[], 2_320_001));
        assert!(exports_station(&n, ClientMode::Terminal, None, &[], 2_320_997));
        assert!(!exports_station(&n, ClientMode::Terminal, None, &[], 2_320_998));
        assert!(!exports_station(&n, ClientMode::Basestation, None, &[], 2_320_997), "an ISSI names only an app");
    }

    #[test]
    fn export_asks_once_and_keeps_the_answer() {
        let now = Instant::now();
        let mut e = Export { checks: true, never: HashSet::from([7]), ..Default::default() };
        assert_eq!(e.allow(7, now), Allow::No, "never export");
        assert_eq!(e.allow(100, now), Allow::Ask);
        e.asked(&[100, 200], now);
        assert_eq!(e.allow(100, now), Allow::No, "waiting for the answer");
        assert_eq!(e.allow(100, now + PENDING_FOR), Allow::Ask, "no answer: asked again");
        assert_eq!(e.answered(&[200], &[100], now), vec![200]);
        assert_eq!(e.allow(100, now), Allow::Yes);
        assert_eq!(e.allow(200, now), Allow::No, "taken there");
        assert_eq!(e.allow(200, now + TAKEN_FOR), Allow::Ask, "taken is asked again later");
        // A server that cannot be asked: everything but the list.
        let plain = Export { never: HashSet::from([7]), ..Default::default() };
        assert_eq!((plain.allow(100, now), plain.allow(7, now)), (Allow::Yes, Allow::No));
        // Conflicts: newest first, one a minute per ISSI.
        assert!(e.conflict(200, "taken".into(), now));
        assert!(!e.conflict(200, "again".into(), now));
        assert!(e.conflict(300, "other".into(), now));
        assert_eq!(e.conflicts.iter().map(|c| c.issi).collect::<Vec<_>>(), vec![300, 200]);
    }

    #[test]
    fn radios_in_a_bridged_group_join_its_group_over_there() {
        let mut off = pool("Off", &[("TP", 911)]);
        off.enabled = false;
        off.groups = vec![9];
        let pools = vec![pool("A", &[("TP", 909), ("Other", 1)]), off];
        let (g9, g2, g10) = (HashSet::from([9]), HashSet::from([2]), HashSet::from([9, 2]));
        let radios = vec![(100, &g9), (200, &g2), (300, &g10), (9_000_001, &g9)];
        let m = mirror(&pools, "TP", 9_000_001, radios.into_iter());
        assert_eq!(m, HashMap::from([(100, BTreeSet::from([909])), (300, BTreeSet::from([909]))]));
    }

    #[test]
    fn member_changes_register_affiliate_and_deregister() {
        let kinds = |v: &[Vec<u8>]| v.iter().map(|p| (p[1], u32::from_le_bytes(p[2..6].try_into().unwrap()))).collect::<Vec<_>>();
        let mut have = HashMap::new();
        let want = HashMap::from([(100, BTreeSet::from([909]))]);
        assert_eq!(kinds(&member_changes(&mut have, &want, &HashSet::new())), vec![(protocol::SUB_REGISTER, 100), (protocol::SUB_AFFILIATE, 100)]);
        assert!(member_changes(&mut have, &want, &HashSet::new()).is_empty());
        let more = HashMap::from([(100, BTreeSet::from([909, 910]))]);
        assert_eq!(kinds(&member_changes(&mut have, &more, &HashSet::new())), vec![(protocol::SUB_AFFILIATE, 100)]);
        // Talking right now: stays until its over is done.
        assert!(member_changes(&mut have, &HashMap::new(), &HashSet::from([100])).is_empty());
        assert_eq!(kinds(&member_changes(&mut have, &HashMap::new(), &HashSet::new())), vec![(protocol::SUB_DEREGISTER, 100)]);
        assert!(have.is_empty());
    }

    #[test]
    fn settle_keeps_since_in_step_with_the_registered_radios() {
        let mut e = Export { network: "TP".into(), ..Default::default() };
        let want = HashMap::from([(100, BTreeSet::from([909])), (101, BTreeSet::new())]);
        member_changes(&mut e.have, &want, &HashSet::new());
        e.settle();
        let first = e.since[&100];
        assert!(first > 0 && e.since.contains_key(&101));
        let v = e.exported_view();
        assert_eq!(v.iter().map(|x| (x.issi, x.groups.clone())).collect::<Vec<_>>(), vec![(100, vec![909]), (101, vec![])]);
        // A radio that stays keeps its time; one that leaves is dropped.
        member_changes(&mut e.have, &HashMap::from([(100, BTreeSet::from([909, 910]))]), &HashSet::new());
        e.settle();
        assert_eq!(e.since.len(), 1);
        assert_eq!(e.since[&100], first);
        assert_eq!(e.exported_view()[0].groups, vec![909, 910]);
    }

    #[test]
    fn talker_is_registered_and_affiliated_once_before_its_first_over() {
        let talkers: SharedExport = Arc::default();
        let mut out = Outbound::new(2, Arc::default()).registering(talkers.clone(), 9_000_001);
        let kinds = |v: &[Vec<u8>]| v.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>();
        let reg = (protocol::CLASS_SUBSCRIBER, protocol::SUB_REGISTER);
        let aff = (protocol::CLASS_SUBSCRIBER, protocol::SUB_AFFILIATE);
        let gtx = (protocol::CLASS_CALL_CONTROL, protocol::CALL_GROUP_TX);
        let first = out.packets(group_tx(Uuid::new_v4(), 2320972, alias(2, 909)));
        assert_eq!(kinds(&first), vec![reg, aff, gtx]);
        assert_eq!(&first[1][18..22], &909u32.to_le_bytes());
        assert_eq!(kinds(&out.packets(group_tx(Uuid::new_v4(), 2320972, alias(2, 909)))), vec![gtx]);
        assert_eq!(kinds(&out.packets(group_tx(Uuid::new_v4(), 2320972, alias(2, 910)))), vec![aff, gtx]);
        // The bridge ISSI is registered already; other networks' calls stay.
        assert_eq!(kinds(&out.packets(group_tx(Uuid::new_v4(), 9_000_001, alias(2, 909)))), vec![gtx]);
        assert!(out.packets(group_tx(Uuid::new_v4(), 2320973, alias(3, 909))).is_empty());
        assert_eq!(talkers.lock().unwrap().have.get(&2320972).map(|g| g.len()), Some(2));
        // Switched off: passed through as before.
        let mut plain = Outbound::new(2, Arc::default());
        assert_eq!(kinds(&plain.packets(group_tx(Uuid::new_v4(), 2320972, alias(2, 909)))), vec![gtx]);
    }

    #[test]
    fn log_line_reads_talker_and_group() {
        let pkt = group_tx(Uuid::new_v4(), 2320972, 909);
        assert_eq!((src(&pkt), dest(&pkt)), (2320972, 909));
        // Short or foreign packets are not logged and do not panic.
        log_crossing("Test", "in", &[protocol::CLASS_CALL_CONTROL, protocol::CALL_GROUP_TX], None);
        log_crossing("Test", "out", &pkt, Some(&pkt));
    }

    #[test]
    fn alias_keeps_network_and_group_apart() {
        assert_eq!(alias(1, 9), 0x0100_0009);
        assert_eq!(split_alias(alias(3, 909)), Some((3, 909)));
        assert_eq!(split_alias(9), None);
        assert_eq!(split_alias(0xFF_FFFF), None);
    }

    fn gtx(pkt: &[u8]) -> (Uuid, u32, u32) {
        (call_id(pkt).unwrap(), u32::from_le_bytes(pkt[18..22].try_into().unwrap()), dest(pkt))
    }

    #[test]
    fn outbound_lets_out_only_calls_on_its_own_network_under_new_ids() {
        let ids: SharedCallIds = Arc::default();
        let mut out = Outbound::new(2, ids.clone());
        let call = Uuid::new_v4();
        // A local group call: stays here.
        assert!(out.translate(group_tx(call, 2320972, 9)).is_none());
        // A call on another network's alias: stays here.
        assert!(out.translate(group_tx(call, 2320972, alias(3, 9))).is_none());
        // Its frames stay too.
        assert!(out.translate(protocol::build_dtmf_frame(&call, 1)).is_none());
        // A call on this network's group 909: out with the real GSSI, talker
        // 1:1, under an identifier of its own there.
        let leg = Uuid::new_v4();
        let (remote, src, dst) = gtx(&out.translate(group_tx(leg, 2320972, alias(2, 909))).unwrap());
        assert_eq!((src, dst), (2320972, 909));
        assert_ne!(remote, leg);
        // A new over keeps that identifier; frames and the end use it too.
        assert_eq!(gtx(&out.translate(group_tx(leg, 2320973, alias(2, 909))).unwrap()).0, remote);
        assert_eq!(call_id(&out.translate(protocol::build_dtmf_frame(&leg, 1)).unwrap()), Some(remote));
        let idle = protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &leg, 0);
        assert_eq!(call_id(&out.translate(idle).unwrap()), Some(remote));
        // After its end nothing of the call goes out any more.
        assert!(out.translate(protocol::build_dtmf_frame(&leg, 1)).is_none());
        // Registrations and services never go out.
        assert!(out.translate(protocol::build_subscriber_message(protocol::SUB_REGISTER, 7, &[])).is_none());
        assert!(out.translate(protocol::build_service(protocol::SERVICE_BTS_INFO, "{}")).is_none());
    }

    #[test]
    fn inbound_lets_in_only_bridged_groups_as_aliases() {
        let ids: SharedCallIds = Arc::default();
        let mut inb = Inbound::new(2, ids.clone());
        let bridged = |g: u32| g == 909;
        let other = Uuid::new_v4();
        assert!(inb.translate(group_tx(other, 1234, 9), bridged).is_none());
        assert!(inb.translate(protocol::build_dtmf_frame(&other, 1), bridged).is_none());
        let call = Uuid::new_v4();
        let (local, src, dst) = gtx(&inb.translate(group_tx(call, 1234, 909), bridged).unwrap());
        assert_eq!((src, dst), (1234, alias(2, 909)));
        assert_ne!(local, call);
        assert_eq!(call_id(&inb.translate(protocol::build_dtmf_frame(&call, 1), bridged).unwrap()), Some(local));
        // Registrations of that network never reach this server's tables.
        assert!(inb.translate(protocol::build_subscriber_message(protocol::SUB_REGISTER, 1234, &[909]), bridged).is_none());
        assert!(inb.translate(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &call, 0), bridged).is_some());
        assert!(inb.translate(protocol::build_dtmf_frame(&call, 1), bridged).is_none());
    }

    #[test]
    fn an_answer_over_there_comes_back_under_our_call_id() {
        let ids: SharedCallIds = Arc::default();
        let (mut out, mut inb) = (Outbound::new(1, ids.clone()), Inbound::new(1, ids));
        let leg = Uuid::new_v4();
        let remote = gtx(&out.translate(group_tx(leg, 2320972, alias(1, 909))).unwrap()).0;
        // Someone over there answers in that call.
        let back = inb.translate(group_tx(remote, 1234, 909), |_| true).unwrap();
        assert_eq!(gtx(&back), (leg, 1234, alias(1, 909)));
        // The same call coming back as our own id (a loop) is a new call here.
        let looped = inb.translate(group_tx(leg, 2320972, 909), |_| true).unwrap();
        assert_ne!(call_id(&looped), Some(leg));
    }

    #[test]
    fn a_call_of_ours_ended_over_there_still_sends_its_voice_and_end() {
        // Field 2026-11-07: TetraPack answered an over from here with
        // GROUP_IDLE at once; the ids were dropped, so the rest of the over
        // and its end never went there and the call hung on Brandmeister.
        let ids: SharedCallIds = Arc::default();
        let (mut out, mut inb) = (Outbound::new(1, ids.clone()), Inbound::new(1, ids));
        let leg = Uuid::new_v4();
        let remote = gtx(&out.translate(group_tx(leg, 2320972, alias(1, 2324))).unwrap()).0;
        let theirs = inb.translate(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &remote, 0), |_| true).unwrap();
        assert_eq!(call_id(&theirs), Some(leg));
        assert_eq!(call_id(&out.translate(protocol::build_dtmf_frame(&leg, 1)).unwrap()), Some(remote));
        let end = protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &leg, 0);
        assert_eq!(call_id(&out.translate(end).unwrap()), Some(remote));
        // Our end closes it.
        assert!(out.translate(protocol::build_dtmf_frame(&leg, 1)).is_none());
    }

    fn pool(name: &str, remote: &[(&str, u32)]) -> TalkgroupPoolConfig {
        TalkgroupPoolConfig {
            name: name.into(), enabled: true, both_ways: true, groups: vec![9],
            remote: remote.iter().map(|(n, g)| RemotePoolGroup { network: (*n).into(), gssi: *g }).collect(),
            linked: false,
        }
    }

    #[test]
    fn bridged_groups_come_from_switched_on_pools() {
        let mut off = pool("Off", &[("TP", 5)]);
        off.enabled = false;
        let pools = [pool("A", &[("TP", 909), ("Other", 1)]), pool("B", &[("TP", 9)]), off];
        assert_eq!(bridged_groups(&pools, "TP").into_iter().collect::<Vec<_>>(), vec![9, 909]);
        assert_eq!(bridged_groups(&pools, "Other").into_iter().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn affiliation_changes_send_only_the_difference() {
        let have: BTreeSet<u32> = [9, 10].into();
        let want: BTreeSet<u32> = [10, 11].into();
        let msgs = affiliation_changes(5, &have, &want);
        assert_eq!(msgs, vec![
            protocol::build_subscriber_message(protocol::SUB_DEAFFILIATE, 5, &[9]),
            protocol::build_subscriber_message(protocol::SUB_AFFILIATE, 5, &[11]),
        ]);
        assert!(affiliation_changes(0, &have, &want).is_empty());
        assert!(affiliation_changes(5, &want, &want).is_empty());
    }

    fn net(name: &str) -> NetworkConfig {
        NetworkConfig { name: name.into(), remote_host: "tp.example:443".into(), ..Default::default() }
    }

    #[test]
    fn validation_rejects_bad_networks() {
        let ok = MultiNetworkConfig { networks: vec![net("TP")], ..Default::default() };
        assert!(validate(&ok, &[]).is_ok());
        let dup = MultiNetworkConfig { networks: vec![net("TP"), net("tp")], ..Default::default() };
        assert!(validate(&dup, &[]).is_err());
        let mut no_host = net("TP");
        no_host.remote_host.clear();
        assert!(validate(&MultiNetworkConfig { networks: vec![no_host.clone()], ..Default::default() }, &[]).is_err());
        no_host.enabled = false;
        assert!(validate(&MultiNetworkConfig { networks: vec![no_host], ..Default::default() }, &[]).is_ok());
        let mut ua = net("TP");
        ua.user_agent = "TMO-Austria Infra 1.0\r\nX-Evil: 1".into();
        assert!(validate(&MultiNetworkConfig { networks: vec![ua], ..Default::default() }, &[]).is_err());
        let mut no_port = net("TP");
        no_port.remote_host = "brew.oe1sxm.at".into();
        assert!(validate(&MultiNetworkConfig { networks: vec![no_port], ..Default::default() }, &[]).is_err());
        assert!(has_port("brew.example.net:9000") && has_port("[::1]:443"));
        assert!(!has_port("brew.example.net") && !has_port("brew:0") && !has_port("::1") && !has_port("[::1]"));
        let bad_mni = MultiNetworkConfig { mcc: 1024, ..Default::default() };
        assert!(validate(&bad_mni, &[]).is_err());
        // A pool may not refer to a network that is not there.
        assert!(validate(&ok, &[pool("A", &[("Gone", 9)])]).is_err());
        assert!(validate(&ok, &[pool("A", &[("TP", 9)])]).is_ok());
    }

    #[test]
    fn user_agent_is_free_text_with_a_safe_fallback() {
        let mut n = net("TP");
        assert_eq!(user_agent(&n), federation::USER_AGENT);
        n.user_agent = "TMO-Austria Infra 1.0".into();
        assert_eq!(user_agent(&n), "TMO-Austria Infra 1.0");
        n.user_agent = "Wien\nX".into();
        assert_eq!(user_agent(&n), federation::USER_AGENT);
    }

    #[test]
    fn config_file_round_trip_keeps_networks_and_bridges() {
        let mut c = crate::config::Config::default();
        assert!(!c.to_toml_pretty().unwrap().contains("multi_network"), "an unused section is not written");
        c.multi_network.name = "Main".into();
        c.multi_network.mcc = 901;
        c.multi_network.mnc = 9999;
        let mut n = net("TP");
        n.user_agent = "TMO-Austria Infra 1.0".into();
        n.bridge_issi = 9_000_001;
        c.multi_network.networks.push(n);
        c.talkgroup_pools.push(pool("A", &[("TP", 909)]));
        c.talkgroup_pools.push(pool("Local", &[]));
        let text = c.to_toml_pretty().unwrap();
        let back = crate::config::Config::parse(&text).unwrap();
        assert_eq!(back.multi_network, c.multi_network);
        assert_eq!(back.talkgroup_pools, c.talkgroup_pools);
        // An old file without the new keys still loads.
        let old = crate::config::Config::parse("[[talkgroup_pools]]\nname = \"N\"\ngroups = [1, 2]\n").unwrap();
        assert!(old.talkgroup_pools[0].remote.is_empty() && old.multi_network.is_unset());
    }

    #[test]
    fn slots_stay_with_their_network() {
        let links = NetLinks::default();
        let a = MultiNetworkConfig { networks: vec![net("A"), net("B")], ..Default::default() };
        let s1 = links.slots(&a);
        let b = MultiNetworkConfig { networks: vec![net("B"), net("C")], ..Default::default() };
        let s2 = links.slots(&b);
        assert_eq!(s1["B"], s2["B"]);
        assert_ne!(s2["C"], s1["A"]);
        assert!(!s2.contains_key("A"));
    }
}

#[cfg(test)]
mod e2e {
    //! Two servers on loopback: this one with a link to a "TetraPack" server,
    //! 10 Basestations with 25 radios here, one Basestation there.
    use super::*;
    use crate::config::{Config, RemotePoolGroup};
    use futures_util::{SinkExt, StreamExt};
    use std::net::SocketAddr;
    use tokio_tungstenite::tungstenite::Message;

    type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    async fn serve(cfg: Config) -> (Arc<AppState>, SocketAddr) {
        let state = Arc::new(AppState::new(cfg, "test.toml".into()).0);
        let app = axum::Router::new()
            .route("/brew", axum::routing::get(crate::server::brew_discovery))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await });
        (state, addr)
    }

    fn base_config() -> Config {
        let mut c = Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        c.fallback_broadcast_when_no_affiliations = true;
        c
    }

    async fn station(addr: SocketAddr, radios: &[u32], groups: &[u32]) -> Ws {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/brew")).await.unwrap();
        for &r in radios {
            ws.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, r, &[]).into())).await.unwrap();
            if !groups.is_empty() {
                ws.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, r, groups).into())).await.unwrap();
            }
        }
        ws
    }

    /// Next GROUP_TX a station gets within `ms`, as (source, destination).
    async fn next_group_tx(ws: &mut Ws, ms: u64) -> Option<(Uuid, u32, u32)> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(Ok(msg))) = tokio::time::timeout(left, ws.next()).await else { return None };
            if let Message::Binary(b) = msg {
                if is_group_tx(&b) {
                    let src = u32::from_le_bytes(b[18..22].try_into().unwrap());
                    return Some((call_id(&b).unwrap(), src, dest(&b)));
                }
            }
        }
    }

    async fn wait_until(what: &str, mut f: impl AsyncFnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !f().await {
            assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn bridge_pool_carries_calls_both_ways_with_gssi_translation() {
        let (tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.name = "Main".into();
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), remote_host: tp_addr.to_string(), bridge_issi: 9_000_001,
            user_agent: "TMO-Austria Infra 1.0".into(), reconnect_interval_seconds: 5, ..Default::default()
        });
        cfg.talkgroup_pools.push(TalkgroupPoolConfig {
            name: "Bridge".into(), enabled: true, both_ways: true, groups: vec![9],
            remote: vec![RemotePoolGroup { network: "TP".into(), gssi: 909 }],
            linked: false,
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;

        // 10 stations, 25 radios; the even stations have radios on TG 9.
        let mut stations = Vec::new();
        let mut issi = 2_320_001;
        for s in 0..10u32 {
            let n = if s < 5 { 3 } else { 2 };
            let radios: Vec<u32> = (0..n).map(|k| issi + k).collect();
            issi += n;
            let groups: &[u32] = if s % 2 == 0 { &[9] } else { &[2321] };
            stations.push(station(main_addr, &radios, groups).await);
        }
        assert_eq!(issi - 2_320_001, 25);
        let mut far = station(tp_addr, &[1234], &[909]).await;

        // The link is up: the bridge ISSI sits on TG 909 over there, under
        // the chosen connection name.
        wait_until("bridge ISSI on TG 909", async || {
            tp.inner.read().await.subscribers.get(&9_000_001).is_some_and(|s| s.groups.contains(&909))
        }).await;
        wait_until("all radios registered", async || main.inner.read().await.subscribers.len() == 25).await;
        let agents: Vec<String> = tp.inner.read().await.client_agents.values().cloned().collect();
        assert!(agents.iter().any(|a| a == "TMO-Austria Infra 1.0"), "{agents:?}");
        // The radios in TG 9 here are registered there and sit on TG 909
        // (like a Basestation's radios); the others are not.
        wait_until("TG 9 radios on TG 909 there", async || {
            tp.inner.read().await.subscribers.get(&2_320_001).is_some_and(|s| s.groups == HashSet::from([909]))
        }).await;
        assert!(!tp.inner.read().await.subscribers.contains_key(&2_320_004), "a radio without TG 9 stays here");
        // A radio leaving TG 9 here leaves TG 909 there and is deregistered.
        stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_DEAFFILIATE, 2_320_003, &[9]).into())).await.unwrap();
        wait_until("left TG 909 there", async || !tp.inner.read().await.subscribers.contains_key(&2_320_003)).await;
        stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, 2_320_003, &[9]).into())).await.unwrap();
        wait_until("back on TG 909 there", async || tp.inner.read().await.subscribers.get(&2_320_003).is_some_and(|s| s.groups.contains(&909))).await;
        // The dashboard lists the radios registered there, with their groups
        // there and since when, the same set the far server has.
        let view = snapshot(&main).await;
        let detail = &view.networks[0].exported_detail;
        let far_has: Vec<u32> = { let t = tp.inner.read().await; let mut v: Vec<u32> = t.subscribers.keys().copied().filter(|i| (2_320_001..2_320_026).contains(i)).collect(); v.sort_unstable(); v };
        assert_eq!(detail.iter().map(|x| x.issi).collect::<Vec<_>>(), far_has);
        assert!(detail.iter().all(|x| x.groups == vec![909] && x.since_ms > 0), "{detail:?}");
        assert_eq!(view.networks[0].exported, far_has);

        // A radio on station 0 talks on TG 9: heard on TG 909 over there,
        // talker ISSI 1:1.
        let call = Uuid::new_v4();
        stations[0].send(Message::Binary(protocol::build_group_tx(&call, 2_320_001, 9, 0).into())).await.unwrap();
        let (far_call, src, dst) = next_group_tx(&mut far, 2000).await.expect("call reaches the other network");
        assert_eq!((src, dst), (2_320_001, 909));
        // Like a Basestation, the link registered the talker there first
        // and affiliated it to the group.
        assert!(tp.inner.read().await.subscribers.get(&2_320_001).is_some_and(|s| s.groups.contains(&909)));
        // The radio over there answers in that call: heard on TG 9 here,
        // by the first talker's station too.
        far.send(Message::Binary(protocol::build_group_tx(&far_call, 1234, 909, 0).into())).await.unwrap();
        let (id0, src, dst) = next_group_tx(&mut stations[0], 2000).await.expect("answer reaches the first talker");
        assert_eq!((id0, src, dst), (call, 1234, 9));
        let first = next_group_tx(&mut stations[2], 2000).await.expect("call reaches TG 9");
        assert_eq!((first.1, first.2), (2_320_001, 9));
        let (_, src, dst) = next_group_tx(&mut stations[2], 2000).await.expect("answer reaches TG 9");
        assert_eq!((src, dst), (1234, 9));
        // Its call ends over there: the call ends here too (no hanging floor).
        far.send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &far_call, 0).into())).await.unwrap();
        wait_until("call ended here", async || main.inner.read().await.calls.is_empty()).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        for st in stations.iter_mut() { while next_group_tx(st, 50).await.is_some() {} }

        // A radio over there talks on TG 909: heard on TG 9 on the stations
        // with TG 9, nowhere else, and no station ever sees an alias.
        let back = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_group_tx(&back, 1234, 909, 0).into())).await.unwrap();
        for (s, st) in stations.iter_mut().enumerate() {
            let got = next_group_tx(st, if s % 2 == 0 { 2000 } else { 300 }).await;
            if s % 2 == 0 {
                let (_, src, dst) = got.unwrap_or_else(|| panic!("station {s} hears the call"));
                assert_eq!((src, dst), (1234, 9));
            } else {
                assert!(got.is_none(), "station {s} has no TG 9 radio: {got:?}");
            }
        }
        // A radio here answers in the call it hears: heard over there.
        let leg_here = {
            let inner = main.inner.read().await;
            *inner.pools.runs.values().next().expect("pool call running").legs.values().next().unwrap()
        };
        stations[2].send(Message::Binary(protocol::build_group_tx(&leg_here, 2_320_007, 9, 0).into())).await.unwrap();
        let (id_far, src, dst) = next_group_tx(&mut far, 2000).await.expect("answer reaches the other network");
        assert_eq!((id_far, src, dst), (back, 2_320_007, 909));
        // The answering station ends its call: over there the call ends too,
        // so the floor of TG 909 is free again.
        stations[2].send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &leg_here, 0).into())).await.unwrap();
        wait_until("call ended over there", async || tp.inner.read().await.calls.is_empty()).await;
        wait_until("call ended here", async || main.inner.read().await.calls.is_empty()).await;

        tokio::time::sleep(Duration::from_millis(300)).await;
        for st in stations.iter_mut() { while next_group_tx(st, 50).await.is_some() {} }
        while next_group_tx(&mut far, 50).await.is_some() {}
        // A station cannot send into the other network by itself.
        let sneaky = Uuid::new_v4();
        stations[1].send(Message::Binary(protocol::build_group_tx(&sneaky, 2_320_004, alias(1, 909), 0).into())).await.unwrap();
        assert!(next_group_tx(&mut far, 400).await.is_none());
        // Nor reach TG 9 through the pool as if it came from there.
        assert!(next_group_tx(&mut stations[0], 300).await.is_none());

        // Renaming the connection reconnects the link; the old link going
        // down does not mark the new one as down.
        let mut renamed = main.networks.read().unwrap().clone();
        renamed.networks[0].user_agent = "TMO-Austria Infra 2.0".into();
        if let Ok(mut live) = main.networks.write() { *live = renamed; }
        reconcile(&main);
        wait_until("new connection name over there", async || {
            tp.inner.read().await.client_agents.values().any(|a| a == "TMO-Austria Infra 2.0")
        }).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(snapshot(&main).await.networks[0].connected, "the new link shows as connected");

        // The main switch off ends every link, on brings them back.
        let mut main_off = main.networks.read().unwrap().clone();
        main_off.enabled = false;
        if let Ok(mut live) = main.networks.write() { *live = main_off; }
        reconcile(&main);
        wait_until("main switch off: link gone", async || main.inner.read().await.netlinks.is_empty()).await;
        wait_until("main switch off: bridge gone there", async || !tp.inner.read().await.subscribers.contains_key(&9_000_001)).await;
        wait_until("main switch off: talkers gone there", async || !tp.inner.read().await.subscribers.contains_key(&2_320_001)).await;
        assert!(!snapshot(&main).await.networks[0].connected);
        if let Ok(mut live) = main.networks.write() { live.enabled = true; }
        reconcile(&main);
        wait_until("main switch on: link back", async || !main.inner.read().await.netlinks.is_empty()).await;
        wait_until("main switch on: bridge back there", async || tp.inner.read().await.subscribers.contains_key(&9_000_001)).await;

        // Switching the network off ends the link and deregisters the bridge.
        let mut off = main.networks.read().unwrap().clone();
        off.networks[0].enabled = false;
        if let Ok(mut live) = main.networks.write() { *live = off; }
        reconcile(&main);
        wait_until("bridge ISSI gone over there", async || !tp.inner.read().await.subscribers.contains_key(&9_000_001)).await;
        wait_until("link gone here", async || main.inner.read().await.netlinks.is_empty()).await;
    }

    #[tokio::test]
    async fn register_all_checks_issis_and_reports_conflicts() {
        let (tp, tp_addr) = serve(base_config()).await;
        // TetraPack's own radio with an ISSI this server has as well.
        let mut far = station(tp_addr, &[2_320_002, 1234], &[909]).await;
        wait_until("far radios", async || tp.inner.read().await.subscribers.len() == 2).await;
        let mut cfg = base_config();
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), remote_host: tp_addr.to_string(), bridge_issi: 9_000_001,
            register_all: true, never_export: vec![2_320_003], reconnect_interval_seconds: 5, ..Default::default()
        });
        cfg.talkgroup_pools.push(TalkgroupPoolConfig {
            name: "Bridge".into(), enabled: true, both_ways: true, groups: vec![9],
            remote: vec![RemotePoolGroup { network: "TP".into(), gssi: 909 }],
            linked: false,
        });
        let (main, main_addr) = serve(cfg).await;
        // 10 stations, 25 radios, none in TG 9; station 0 has Multi Network
        // (`mni` capability), the others do not.
        let mut stations = Vec::new();
        let mut issi = 2_320_001;
        for s in 0..10u32 {
            let n = if s < 5 { 3 } else { 2 };
            let radios: Vec<u32> = (0..n).map(|k| issi + k).collect();
            issi += n;
            let mut st = station(main_addr, &radios, &[2321]).await;
            if s == 0 {
                st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":232,"mnc":5,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
            }
            stations.push(st);
        }
        wait_until("all radios registered", async || main.inner.read().await.subscribers.len() == 25).await;
        wait_until("mni station known", async || main.inner.read().await.carriers.info(main.inner.read().await.subscribers[&2_320_001].client_id).is_some()).await;
        run(main.clone()).await;

        // Station 0's free radio goes over (registered, no group); the
        // taken one stays (asked: taken), the listed one too; no radio of
        // another station goes.
        wait_until("free radio exported", async || tp.inner.read().await.subscribers.get(&2_320_001).is_some_and(|s| s.groups.is_empty())).await;
        wait_until("conflict noted", async || snapshot(&main).await.networks[0].conflicts.iter().any(|c| c.issi == 2_320_002)).await;
        let view = snapshot(&main).await;
        assert!(view.networks[0].checks, "the far brew-server answers the ISSI check");
        assert_eq!(view.networks[0].exported, vec![2_320_001]);
        {
            let tpi = tp.inner.read().await;
            assert_eq!(tpi.subscribers[&2_320_002].client_id, tpi.subscribers[&1234].client_id, "TetraPack's own radio keeps its ISSI");
            assert!(!tpi.subscribers.contains_key(&2_320_003), "never exported");
            assert!(!tpi.subscribers.contains_key(&2_320_004), "a station without mni is not exported");
        }
        // A radio over there talks with an ISSI this server has registered
        // there: noted as a conflict (all a server that cannot be asked
        // shows).
        far.send(Message::Binary(protocol::build_group_tx(&Uuid::new_v4(), 2_320_001, 909, 0).into())).await.unwrap();
        wait_until("talker conflict noted", async || snapshot(&main).await.networks[0].conflicts.iter().any(|c| c.issi == 2_320_001)).await;
    }

    /// Next `CLASS_SERVICE` message of `kind` within `ms`, as JSON.
    async fn next_service(ws: &mut Ws, kind: u8, ms: u64) -> Option<serde_json::Value> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(Ok(msg))) = tokio::time::timeout(left, ws.next()).await else { return None };
            if let Message::Binary(b) = msg {
                if b.len() > 2 && b[0] == protocol::CLASS_SERVICE && b[1] == kind {
                    let text = String::from_utf8_lossy(&b[2..]).trim_end_matches('\0').to_string();
                    return serde_json::from_str(&text).ok();
                }
            }
        }
    }

    fn svc(kind: u8, v: serde_json::Value) -> Message {
        Message::Binary(protocol::build_service(kind, &v.to_string()).into())
    }

    /// A radio registered at another network as a visitor that then names
    /// this network (it migrated here, e.g. after its codeplug was changed
    /// from ITSI attach to migration) leaves the other network: it must not
    /// stay reachable there and here at once.
    #[tokio::test]
    async fn visitor_that_becomes_this_networks_radio_leaves_its_old_network() {
        let (tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1638, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5, ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("link up", async || main.inner.read().await.netlinks.len() == 1).await;
        let mut st = station(main_addr, &[], &[]).await;
        st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":1638,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
        next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("SERVICE_NETWORKS");

        // ITSI attach on a 1638 cell: a TetraPack radio.
        st.send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_972, "mcc": 901, "mnc": 1638}))).await.unwrap();
        st.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_972, &[]).into())).await.unwrap();
        wait_until("visitor at TetraPack", async || tp.inner.read().await.subscribers.contains_key(&2_320_972)).await;

        // The same radio migrates with its home 9999: one of ours now.
        st.send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_972, "mcc": 901, "mnc": 9999}))).await.unwrap();
        st.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_972, &[]).into())).await.unwrap();
        wait_until("gone from TetraPack", async || !tp.inner.read().await.subscribers.contains_key(&2_320_972)).await;
        wait_until("registered here", async || main.inner.read().await.subscribers.contains_key(&2_320_972)).await;
        assert!(snapshot(&main).await.networks[0].visitors.is_empty());
    }

    #[tokio::test]
    async fn migrated_radio_in_a_group_of_the_cells_network_hears_that_network() {
        // Field 2026-11-07: the MTP (home 9999) migrated onto the 1638 cell
        // attached TetraPack groups; they were ignored there and kept as
        // groups of this network here.
        let (tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1638, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5, ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("link up", async || main.inner.read().await.netlinks.len() == 1).await;
        let mut st = station(main_addr, &[], &[]).await;
        st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":1638,"cells":[{"carrier":1530,"carrier_ext":268,"location_area":1}],"capabilities":["mni"]}"#).into())).await.unwrap();
        next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("SERVICE_NETWORKS");
        st.send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_972, "mcc": 901, "mnc": 9999}))).await.unwrap();
        st.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_972, &[]).into())).await.unwrap();
        st.send(svc(protocol::SERVICE_GROUP_NETWORK, serde_json::json!({"issi": 2_320_972, "groups": [{"gssi": 2324, "mcc": 901, "mnc": 1638}]}))).await.unwrap();
        st.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, 2_320_972, &[2324]).into())).await.unwrap();

        // Registered and in TG 2324 at TetraPack, not in this network's 2324.
        wait_until("in 2324 at TetraPack", async || tp.inner.read().await.subscribers.get(&2_320_972).is_some_and(|s| s.groups.contains(&2324))).await;
        assert!(!main.inner.read().await.group_clients.get(&2324).is_some_and(|c| !c.is_empty()));

        // A call of TetraPack on 2324 reaches the station, as group 2324.
        let mut far = station(tp_addr, &[2_329_001], &[2324]).await;
        let id = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_group_tx(&id, 2_329_001, 2324, 0).into())).await.unwrap();
        let (_, talker, gssi) = next_group_tx(&mut st, 3000).await.expect("TetraPack's call on the station");
        assert_eq!((talker, gssi), (2_329_001, 2324));

        // Leaving the group leaves it there.
        st.send(svc(protocol::SERVICE_GROUP_NETWORK, serde_json::json!({"issi": 2_320_972, "groups": [{"gssi": 2324, "mcc": 901, "mnc": 1638}]}))).await.unwrap();
        st.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_DEAFFILIATE, 2_320_972, &[2324]).into())).await.unwrap();
        wait_until("left 2324 at TetraPack", async || !tp.inner.read().await.subscribers.get(&2_320_972).is_some_and(|s| s.groups.contains(&2324))).await;
    }

    #[tokio::test]
    async fn own_group_calls_are_not_broadcast_to_a_station_of_another_network() {
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        let mut classic = station(main_addr, &[2_320_010], &[]).await;
        let mut own = station(main_addr, &[2_320_020], &[]).await;
        own.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":9999,"cells":[{"carrier":1528,"carrier_ext":268,"location_area":2}],"capabilities":["mni"]}"#).into())).await.unwrap();
        let mut via = station(main_addr, &[], &[]).await;
        via.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":1638,"cells":[{"carrier":1530,"carrier_ext":268,"location_area":1}],"capabilities":["mni"]}"#).into())).await.unwrap();
        let mut app = station(main_addr, &[2_320_001], &[]).await;
        wait_until("stations announced", async || main.inner.read().await.carriers.announcements().count() == 2).await;
        wait_until("radios registered", async || main.inner.read().await.subscribers.len() == 3).await;

        // Nobody routes TG 9: the fallback goes to every station of this
        // network, not to the one whose cell is TetraPack's.
        let id = Uuid::new_v4();
        app.send(Message::Binary(protocol::build_group_tx(&id, 2_320_001, 9, 0).into())).await.unwrap();
        assert_eq!(next_group_tx(&mut classic, 2000).await.map(|g| g.0), Some(id));
        assert_eq!(next_group_tx(&mut own, 2000).await.map(|g| g.0), Some(id));
        assert_eq!(next_group_tx(&mut via, 500).await, None);
        app.send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &id, 0).into())).await.unwrap();

        // A radio of this network migrated onto that cell and in TG 9: the
        // station gets the group through the affiliation.
        via.send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_972, "mcc": 901, "mnc": 9999}))).await.unwrap();
        via.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_972, &[]).into())).await.unwrap();
        via.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, 2_320_972, &[9]).into())).await.unwrap();
        wait_until("affiliated", async || main.inner.read().await.group_clients.get(&9).is_some_and(|c| !c.is_empty())).await;
        let id = Uuid::new_v4();
        app.send(Message::Binary(protocol::build_group_tx(&id, 2_320_001, 9, 0).into())).await.unwrap();
        assert_eq!(next_group_tx(&mut via, 2000).await.map(|g| g.0), Some(id));
    }

    #[tokio::test]
    async fn visitors_register_at_home_and_take_part_in_its_group_calls() {
        let (tp, tp_addr) = serve(base_config()).await;
        let (other, other_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1234, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5, ..Default::default()
        });
        cfg.multi_network.networks.push(NetworkConfig {
            name: "Other".into(), mcc: 901, mnc: 5555, remote_host: other_addr.to_string(), bridge_issi: 9_000_002, reconnect_interval_seconds: 5, ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("both links up", async || main.inner.read().await.netlinks.len() == 2).await;

        // 10 stations, 25 radios of this network on TG 2321; stations 0
        // and 1 have the mni extension.
        let mut stations = Vec::new();
        let mut issi = 2_320_001;
        for s in 0..10u32 {
            let n = if s < 5 { 3 } else { 2 };
            let radios: Vec<u32> = (0..n).map(|k| issi + k).collect();
            issi += n;
            let mut st = station(main_addr, &radios, &[2321]).await;
            if s < 2 {
                st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":9999,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
                // It learns the networks it may take radios from.
                let nets = next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("SERVICE_NETWORKS");
                assert_eq!(nets["this"]["mnc"], 9999);
                assert_eq!(nets["networks"].as_array().unwrap().len(), 2);
            }
            stations.push(st);
        }
        wait_until("all radios registered", async || main.inner.read().await.subscribers.len() == 25).await;
        let mut far = station(tp_addr, &[1234], &[909]).await;

        // A radio of TetraPack with the same ISSI as one of ours (2320002)
        // comes to station 0, one of "Other" to station 1.
        stations[0].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_002, "mcc": 901, "mnc": 1234}))).await.unwrap();
        stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_002, &[]).into())).await.unwrap();
        stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, 2_320_002, &[909]).into())).await.unwrap();
        stations[1].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 4_000_001, "mcc": 901, "mnc": 5555}))).await.unwrap();
        stations[1].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 4_000_001, &[]).into())).await.unwrap();
        stations[1].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_AFFILIATE, 4_000_001, &[77]).into())).await.unwrap();
        // Each is registered in its home network with its groups...
        wait_until("visitor at TetraPack", async || tp.inner.read().await.subscribers.get(&2_320_002).is_some_and(|s| s.groups.contains(&909))).await;
        wait_until("visitor at Other", async || other.inner.read().await.subscribers.get(&4_000_001).is_some_and(|s| s.groups.contains(&77))).await;
        // ...and our own 2320002 is untouched.
        {
            let inner = main.inner.read().await;
            assert_eq!(inner.subscribers.len(), 25);
            assert_eq!(inner.subscribers[&2_320_002].groups, HashSet::from([2321]));
            assert!(!inner.subscribers.contains_key(&4_000_001));
        }
        assert_eq!(snapshot(&main).await.networks[0].visitors.iter().map(|v| v.issi).collect::<Vec<_>>(), vec![2_320_002]);

        // A station without the extension cannot make a visitor.
        stations[2].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_007, "mcc": 901, "mnc": 1234}))).await.unwrap();
        stations[2].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REREGISTER, 2_320_007, &[]).into())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!tp.inner.read().await.subscribers.contains_key(&2_320_007));

        // TetraPack's radio talks on 909: heard on station 0 only, as 909
        // with its network before it.
        let call = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_group_tx(&call, 1234, 909, 0).into())).await.unwrap();
        let note = next_service(&mut stations[0], protocol::SERVICE_CALL_NETWORK, 2000).await.expect("call network before the call");
        assert_eq!((note["group"]["gssi"].as_u64(), note["group"]["mnc"].as_u64()), (Some(909), Some(1234)));
        let (id0, src, dst) = next_group_tx(&mut stations[0], 2000).await.expect("station 0 hears 909");
        assert_eq!((src, dst), (1234, 909));
        assert_eq!(note["uuid"], id0.to_string());
        for (s, st) in stations.iter_mut().enumerate().skip(1) {
            assert!(next_group_tx(st, 100).await.is_none(), "station {s} has no visitor on 909");
        }
        far.send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &call, 0).into())).await.unwrap();
        wait_until("call ended", async || main.inner.read().await.calls.is_empty()).await;

        // The visitor talks on its home group 909: heard at TetraPack.
        let v_call = Uuid::new_v4();
        stations[0].send(svc(protocol::SERVICE_CALL_NETWORK, serde_json::json!({"uuid": v_call.to_string(), "group": {"gssi": 909, "mcc": 901, "mnc": 1234}}))).await.unwrap();
        stations[0].send(Message::Binary(protocol::build_group_tx(&v_call, 2_320_002, 909, 0).into())).await.unwrap();
        let (_, src, dst) = next_group_tx(&mut far, 2000).await.expect("TetraPack hears the visitor");
        assert_eq!((src, dst), (2_320_002, 909));
        assert!(next_group_tx(&mut stations[3], 200).await.is_none(), "not a call on our own TG 909");
        stations[0].send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &v_call, 0).into())).await.unwrap();
        wait_until("visitor call ended", async || main.inner.read().await.calls.is_empty()).await;

        // It moves to station 1: TetraPack sees nothing of it, its calls
        // now reach station 1.
        stations[1].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_002, "mcc": 901, "mnc": 1234}))).await.unwrap();
        stations[1].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_002, &[]).into())).await.unwrap();
        let tp_slot = slots(&main)["TP"];
        wait_until("visitor moved", async || {
            let inner = main.inner.read().await;
            inner.visitors.by_key.get(&(tp_slot, 2_320_002)).map(|v| v.station) == Some(inner.subscribers[&2_320_004].client_id)
        }).await;
        let call2 = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_group_tx(&call2, 1234, 909, 0).into())).await.unwrap();
        assert!(next_group_tx(&mut stations[1], 2000).await.is_some_and(|(_, s, d)| (s, d) == (1234, 909)));
        assert!(next_group_tx(&mut stations[0], 200).await.is_none(), "station 0 has no visitor any more");
        far.send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &call2, 0).into())).await.unwrap();

        // Deregistered: gone at TetraPack. Station 1 goes away: its other
        // visitor leaves "Other".
        stations[1].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_DEREGISTER, 2_320_002, &[]).into())).await.unwrap();
        wait_until("visitor gone at TetraPack", async || !tp.inner.read().await.subscribers.contains_key(&2_320_002)).await;
        let st1 = stations.remove(1);
        drop(st1);
        wait_until("visitor gone at Other", async || !other.inner.read().await.subscribers.contains_key(&4_000_001)).await;
        assert_eq!(main.inner.read().await.subscribers[&2_320_002].groups, HashSet::from([2321]), "our own 2320002 still there");
    }

    /// Next call control message of `kind` within `ms`, as (id, source, destination).
    async fn next_call(ws: &mut Ws, kind: u8, ms: u64) -> Option<(Uuid, u32, u32)> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(Ok(msg))) = tokio::time::timeout(left, ws.next()).await else { return None };
            if let Message::Binary(b) = msg {
                if b.len() >= 18 && b[0] == protocol::CLASS_CALL_CONTROL && b[1] == kind {
                    let (s, d) = if b.len() >= 26 { (u32::from_le_bytes(b[18..22].try_into().unwrap()), dest(&b)) } else { (0, 0) };
                    return Some((call_id(&b).unwrap(), s, d));
                }
            }
        }
    }

    fn accept(id: &Uuid) -> Message {
        let mut p = vec![protocol::CLASS_CALL_CONTROL, protocol::CALL_SETUP_ACCEPT];
        p.extend_from_slice(id.as_bytes());
        Message::Binary(p.into())
    }

    fn setup(id: &Uuid, src: u32, dst: u32) -> Message {
        Message::Binary(protocol::build_circular_call_setup(id, src, dst, 0).into())
    }

    #[tokio::test]
    async fn individual_calls_and_sds_cross_networks() {
        let (tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1234, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5, ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("link up", async || main.inner.read().await.netlinks.len() == 1).await;
        // 10 stations, 25 radios; stations 0 to 2 have the mni extension.
        let mut stations = Vec::new();
        let mut issi = 2_320_001;
        for s in 0..10u32 {
            let n = if s < 5 { 3 } else { 2 };
            let radios: Vec<u32> = (0..n).map(|k| issi + k).collect();
            issi += n;
            let mut st = station(main_addr, &radios, &[2321]).await;
            if s < 3 {
                st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":9999,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
                next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("networks");
            }
            stations.push(st);
        }
        wait_until("all radios registered", async || main.inner.read().await.subscribers.len() == 25).await;
        let mut far = station(tp_addr, &[1234], &[909]).await;
        // TetraPack's 2320005 visits station 0 (ours with that ISSI is on station 1).
        stations[0].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_005, "mcc": 901, "mnc": 1234}))).await.unwrap();
        stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_005, &[]).into())).await.unwrap();
        wait_until("visitor at TetraPack", async || tp.inner.read().await.subscribers.contains_key(&2_320_005)).await;

        // TetraPack calls its radio: station 0 gets it, with both parties'
        // network before it; our own 2320005 is not called.
        let a = Uuid::new_v4();
        far.send(setup(&a, 1234, 2_320_005)).await.unwrap();
        let note = next_service(&mut stations[0], protocol::SERVICE_CALL_NETWORK, 2000).await.expect("party networks");
        assert_eq!((note["calling"]["mnc"].as_u64(), note["called"]["mnc"].as_u64()), (Some(1234), Some(1234)));
        let (a0, src, dst) = next_call(&mut stations[0], protocol::CALL_SETUP_REQUEST, 2000).await.expect("station 0 is called");
        assert_eq!((src, dst), (1234, 2_320_005));
        assert_eq!(note["uuid"], a0.to_string());
        assert!(next_call(&mut stations[1], protocol::CALL_SETUP_REQUEST, 200).await.is_none(), "our own 2320005 is not called");
        stations[0].send(accept(&a0)).await.unwrap();
        assert_eq!(next_call(&mut far, protocol::CALL_SETUP_ACCEPT, 2000).await.map(|c| c.0), Some(a), "the answer goes back under TetraPack's id");
        far.send(Message::Binary(protocol::build_call_cause(protocol::CALL_RELEASE, &a, 0).into())).await.unwrap();
        assert_eq!(next_call(&mut stations[0], protocol::CALL_RELEASE, 2000).await.map(|c| c.0), Some(a0));

        // One of ours dials a TetraPack TSI: the call goes there.
        let b = Uuid::new_v4();
        stations[2].send(svc(protocol::SERVICE_CALL_NETWORK, serde_json::json!({"uuid": b.to_string(), "called": {"mcc": 901, "mnc": 1234}}))).await.unwrap();
        stations[2].send(setup(&b, 2_320_008, 1234)).await.unwrap();
        let (bf, src, dst) = next_call(&mut far, protocol::CALL_SETUP_REQUEST, 2000).await.expect("TetraPack's radio is called");
        assert_eq!((src, dst), (2_320_008, 1234));
        far.send(accept(&bf)).await.unwrap();
        assert_eq!(next_call(&mut stations[2], protocol::CALL_SETUP_ACCEPT, 2000).await.map(|c| c.0), Some(b));
        stations[2].send(Message::Binary(protocol::build_call_cause(protocol::CALL_RELEASE, &b, 0).into())).await.unwrap();
        assert!(next_call(&mut far, protocol::CALL_RELEASE, 2000).await.is_some());

        // Without a network named, local first: our own 2320005.
        let c = Uuid::new_v4();
        stations[2].send(setup(&c, 2_320_008, 2_320_005)).await.unwrap();
        assert!(next_call(&mut stations[1], protocol::CALL_SETUP_REQUEST, 2000).await.is_some_and(|x| x.0 == c));
        assert!(next_call(&mut far, protocol::CALL_SETUP_REQUEST, 200).await.is_none());
        stations[2].send(Message::Binary(protocol::build_call_cause(protocol::CALL_RELEASE, &c, 0).into())).await.unwrap();

        // SDS both ways.
        let d = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_short_transfer(&d, 1234, 2_320_005).into())).await.unwrap();
        next_service(&mut stations[0], protocol::SERVICE_CALL_NETWORK, 2000).await.expect("SDS party networks");
        assert!(next_call(&mut stations[0], protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (1234, 2_320_005)));
        let e = Uuid::new_v4();
        stations[1].send(svc(protocol::SERVICE_CALL_NETWORK, serde_json::json!({"uuid": e.to_string(), "called": {"mcc": 901, "mnc": 1234}}))).await.unwrap();
        stations[1].send(Message::Binary(protocol::build_short_transfer(&e, 2_320_004, 1234).into())).await.unwrap();
        assert!(next_call(&mut far, protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (2_320_004, 1234)));

        // An unknown target is refused while "forward unknown targets" is
        // off, and goes to TetraPack once it is on.
        let f = Uuid::new_v4();
        stations[3].send(setup(&f, 2_320_010, 1234)).await.unwrap();
        assert_eq!(next_call(&mut stations[3], protocol::CALL_SETUP_REJECT, 2000).await.map(|x| x.0), Some(f));
        let mut on = main.networks.read().unwrap().clone();
        on.networks[0].forward_unknown = true;
        if let Ok(mut live) = main.networks.write() { *live = on; }
        reconcile(&main);
        wait_until("link back", async || main.inner.read().await.netlinks.len() == 1).await;
        let g = Uuid::new_v4();
        stations[3].send(setup(&g, 2_320_010, 1234)).await.unwrap();
        assert!(next_call(&mut far, protocol::CALL_SETUP_REQUEST, 2000).await.is_some_and(|x| (x.1, x.2) == (2_320_010, 1234)));
    }

    #[tokio::test]
    async fn an_unknown_target_that_is_a_visitor_here_is_not_sent_round_the_link() {
        // Field 2026-11-07: the app called the MTP registered here as a
        // TetraPack radio; forwarded to TetraPack it bounced and was refused.
        let (_tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1638, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5, forward_unknown: true, register_talkers: true, ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("link up", async || main.inner.read().await.netlinks.len() == 1).await;
        let mut st = station(main_addr, &[], &[]).await;
        st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":1638,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
        next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("SERVICE_NETWORKS");
        st.send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_972, "mcc": 901, "mnc": 1638}))).await.unwrap();
        st.send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_972, &[]).into())).await.unwrap();
        let mut app = station(main_addr, &[2_320_997], &[]).await;
        wait_until("visitor and app", async || {
            let i = main.inner.read().await;
            i.subscribers.contains_key(&2_320_997) && i.visitors.by_key.contains_key(&(1, 2_320_972))
        }).await;

        let id = Uuid::new_v4();
        app.send(setup(&id, 2_320_997, 2_320_972)).await.unwrap();
        assert!(next_call(&mut st, protocol::CALL_SETUP_REQUEST, 2000).await.is_some_and(|x| (x.0, x.1, x.2) == (id, 2_320_997, 2_320_972)));
        let sds = Uuid::new_v4();
        app.send(Message::Binary(protocol::build_short_transfer(&sds, 2_320_997, 2_320_972).into())).await.unwrap();
        assert!(next_call(&mut st, protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| x.0 == sds));

        // The other way: the visitor dials the app's ISSI, a party of its
        // network (clause 14.2.3), and the app is registered there by this
        // server: delivered here, not sent round TetraPack.
        // (TetraPack here is a brew-server: asked first, then registered.)
        wait_until("app registered there", async || { ensure_registered(&main, 1, 2_320_997); exported(&main, 1, 2_320_997) }).await;
        let back = Uuid::new_v4();
        st.send(svc(protocol::SERVICE_CALL_NETWORK, serde_json::json!({"uuid": back.to_string(), "called": {"mcc": 901, "mnc": 1638}, "calling": {"mcc": 901, "mnc": 1638}}))).await.unwrap();
        st.send(setup(&back, 2_320_972, 2_320_997)).await.unwrap();
        assert!(next_call(&mut app, protocol::CALL_SETUP_REQUEST, 2000).await.is_some_and(|x| (x.0, x.1, x.2) == (back, 2_320_972, 2_320_997)));
    }

    #[tokio::test]
    async fn never_forward_keeps_individual_calls_and_sds_of_listed_issis_off_the_link() {
        // Sebastian 2026-11-10: positions sent to the telemetry ISSI 200999
        // went to TetraPack through "forward unknown targets".
        let (tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1234, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5,
            forward_unknown: true, never_forward: vec![200_999, 2_320_003, 2_320_099], ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("link up", async || main.inner.read().await.netlinks.len() == 1).await;
        // 10 stations, 25 radios; station 0 has the mni extension.
        let mut stations = Vec::new();
        let mut issi = 2_320_001;
        for s in 0..10u32 {
            let n = if s < 5 { 3 } else { 2 };
            let radios: Vec<u32> = (0..n).map(|k| issi + k).collect();
            issi += n;
            let mut st = station(main_addr, &radios, &[2321]).await;
            if s == 0 {
                st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":9999,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
                next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("networks");
            }
            stations.push(st);
        }
        wait_until("all radios registered", async || main.inner.read().await.subscribers.len() == 25).await;
        let mut far = station(tp_addr, &[1234, 200_999], &[909]).await;
        // Two TetraPack radios visit station 0: 2320050 (not listed) and 2320099 (listed).
        for v in [2_320_050, 2_320_099] {
            stations[0].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": v, "mcc": 901, "mnc": 1234}))).await.unwrap();
            stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, v, &[]).into())).await.unwrap();
        }
        wait_until("visitors at TetraPack", async || {
            let t = tp.inner.read().await;
            t.subscribers.contains_key(&2_320_050) && t.subscribers.contains_key(&2_320_099)
        }).await;

        // Unknown here: a listed target stays here (an SDS goes nowhere, a
        // call is refused), an unlisted one is forwarded.
        let a = Uuid::new_v4();
        stations[3].send(Message::Binary(protocol::build_short_transfer(&a, 2_320_010, 200_999).into())).await.unwrap();
        let b = Uuid::new_v4();
        stations[3].send(Message::Binary(protocol::build_short_transfer(&b, 2_320_010, 1234).into())).await.unwrap();
        // (call ids are translated on the link, so the parties tell them apart)
        assert!(next_call(&mut far, protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (2_320_010, 1234)), "only the unlisted SDS crosses");
        let c = Uuid::new_v4();
        stations[3].send(setup(&c, 2_320_010, 200_999)).await.unwrap();
        assert_eq!(next_call(&mut stations[3], protocol::CALL_SETUP_REJECT, 2000).await.map(|x| x.0), Some(c));
        assert!(next_call(&mut far, protocol::CALL_SETUP_REQUEST, 200).await.is_none());

        // A listed caller of ours dialling a TetraPack TSI: refused.
        let d = Uuid::new_v4();
        stations[1].send(svc(protocol::SERVICE_CALL_NETWORK, serde_json::json!({"uuid": d.to_string(), "called": {"mcc": 901, "mnc": 1234}}))).await.unwrap();
        stations[1].send(setup(&d, 2_320_003, 1234)).await.unwrap();
        assert_eq!(next_call(&mut stations[1], protocol::CALL_SETUP_REJECT, 2000).await.map(|x| x.0), Some(d));
        assert!(next_call(&mut far, protocol::CALL_SETUP_REQUEST, 200).await.is_none());

        // From TetraPack: to the listed visitor, and from the listed 200999,
        // nothing reaches station 0; the unlisted pair still works.
        let e = Uuid::new_v4();
        far.send(setup(&e, 1234, 2_320_099)).await.unwrap();
        let g = Uuid::new_v4();
        far.send(setup(&g, 200_999, 2_320_050)).await.unwrap();
        assert!(next_call(&mut stations[0], protocol::CALL_SETUP_REQUEST, 500).await.is_none(), "no call crosses");
        let f = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_short_transfer(&f, 200_999, 2_320_050).into())).await.unwrap();
        let h = Uuid::new_v4();
        far.send(Message::Binary(protocol::build_short_transfer(&h, 1234, 2_320_050).into())).await.unwrap();
        assert!(next_call(&mut stations[0], protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (1234, 2_320_050)), "only the unlisted SDS crosses");
        // And the unlisted call still goes through.
        let k = Uuid::new_v4();
        far.send(setup(&k, 1234, 2_320_050)).await.unwrap();
        assert!(next_call(&mut stations[0], protocol::CALL_SETUP_REQUEST, 2000).await.is_some_and(|x| (x.1, x.2) == (1234, 2_320_050)));
    }

    /// Sends one SDS: header, then its payload (`data` starts with the
    /// protocol identifier).
    async fn send_sds(ws: &mut Ws, src: u32, dst: u32, data: &[u8]) {
        let id = Uuid::new_v4();
        ws.send(Message::Binary(protocol::build_short_transfer(&id, src, dst).into())).await.unwrap();
        ws.send(Message::Binary(protocol::build_sds_transfer_frame(&id, (data.len() * 8) as u16, data).into())).await.unwrap();
    }

    #[tokio::test]
    async fn forward_lip_off_keeps_position_reports_off_the_link() {
        let (tp, tp_addr) = serve(base_config()).await;
        let mut cfg = base_config();
        cfg.multi_network.mcc = 901;
        cfg.multi_network.mnc = 9999;
        cfg.multi_network.networks.push(NetworkConfig {
            name: "TP".into(), mcc: 901, mnc: 1234, remote_host: tp_addr.to_string(), bridge_issi: 9_000_001, reconnect_interval_seconds: 5,
            forward_unknown: true, forward_lip: false, ..Default::default()
        });
        let (main, main_addr) = serve(cfg).await;
        run(main.clone()).await;
        wait_until("link up", async || main.inner.read().await.netlinks.len() == 1).await;
        // 10 stations, 25 radios; station 0 has the mni extension.
        let mut stations = Vec::new();
        let mut issi = 2_320_001;
        for s in 0..10u32 {
            let n = if s < 5 { 3 } else { 2 };
            let radios: Vec<u32> = (0..n).map(|k| issi + k).collect();
            issi += n;
            let mut st = station(main_addr, &radios, &[2321]).await;
            if s == 0 {
                st.send(Message::Binary(protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":901,"mnc":9999,"cells":[],"capabilities":["mni"]}"#).into())).await.unwrap();
                next_service(&mut st, protocol::SERVICE_NETWORKS, 2000).await.expect("networks");
            }
            stations.push(st);
        }
        wait_until("all radios registered", async || main.inner.read().await.subscribers.len() == 25).await;
        let mut far = station(tp_addr, &[1234, 5678], &[909]).await;
        stations[0].send(svc(protocol::SERVICE_MS_NETWORK, serde_json::json!({"issi": 2_320_050, "mcc": 901, "mnc": 1234}))).await.unwrap();
        stations[0].send(Message::Binary(protocol::build_subscriber_message(protocol::SUB_REGISTER, 2_320_050, &[]).into())).await.unwrap();
        wait_until("visitor at TetraPack", async || tp.inner.read().await.subscribers.contains_key(&2_320_050)).await;

        // Out: LIP short report (0x0A), simple location (0x03) and location
        // system (0x83) stay here; a text SDS (0x82) is forwarded.
        send_sds(&mut stations[3], 2_320_010, 1234, &[0x0A, 0x00, 0x11, 0x22]).await;
        send_sds(&mut stations[3], 2_320_011, 1234, &[0x03, 0x00, 0x11]).await;
        send_sds(&mut stations[3], 2_320_012, 1234, &[0x83, 0x00, 0x11]).await;
        send_sds(&mut stations[3], 2_320_010, 5678, &[0x82, 0x04, 0x01, 0x01, b'H', b'i']).await;
        assert!(next_call(&mut far, protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (2_320_010, 5678)), "only the text SDS crosses");
        assert!(next_call(&mut far, protocol::CALL_SHORT_TRANSFER, 300).await.is_none(), "no position report crosses");

        // In: a position report from TetraPack is dropped, a text passes.
        send_sds(&mut far, 1234, 2_320_050, &[0x0A, 0x00, 0x11, 0x22]).await;
        send_sds(&mut far, 5678, 2_320_050, &[0x82, 0x04, 0x01, 0x01, b'H', b'i']).await;
        assert!(next_call(&mut stations[0], protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (5678, 2_320_050)), "only the text SDS comes in");
        assert!(next_call(&mut stations[0], protocol::CALL_SHORT_TRANSFER, 300).await.is_none());

        // Between our own stations nothing changes: a LIP report is delivered.
        send_sds(&mut stations[3], 2_320_010, 2_320_001, &[0x0A, 0x00, 0x11, 0x22]).await;
        assert!(next_call(&mut stations[0], protocol::CALL_SHORT_TRANSFER, 2000).await.is_some_and(|x| (x.1, x.2) == (2_320_010, 2_320_001)), "local LIP still delivered");
    }

    #[tokio::test]
    async fn forwarding_an_unknown_target_to_this_same_server_stops_at_once() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut cfg = base_config();
        cfg.multi_network.networks.push(NetworkConfig {
            name: "Loop".into(), remote_host: addr.to_string(), register_talkers: false, forward_unknown: true, ..Default::default()
        });
        let state = Arc::new(AppState::new(cfg, "test.toml".into()).0);
        let app = axum::Router::new()
            .route("/brew", axum::routing::get(crate::server::brew_discovery))
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await });
        run(state.clone()).await;
        wait_until("link up", async || state.inner.read().await.netlinks.len() == 1).await;
        let mut a = station(addr, &[100], &[9]).await;
        wait_until("radio", async || state.inner.read().await.subscribers.contains_key(&100)).await;
        let id = Uuid::new_v4();
        a.send(setup(&id, 100, 5555)).await.unwrap();
        // Out over the link, back in as a station's call, not forwarded a
        // second time: refused, and the refusal reaches the caller.
        assert_eq!(next_call(&mut a, protocol::CALL_SETUP_REJECT, 3000).await.map(|x| x.0), Some(id));
        wait_until("no call left", async || state.inner.read().await.calls.is_empty()).await;
    }

    #[test]
    fn individual_calls_come_in_only_for_admitted_issis() {
        let ids: SharedCallIds = Arc::default();
        let (mut out, mut inb) = (Outbound::new(1, ids.clone()), Inbound::new(1, ids));
        let remote = Uuid::new_v4();
        let p = protocol::build_circular_call_setup(&remote, 1234, 2_320_001, 0);
        assert!(inb.translate_with(p.clone(), |_| false, |i| i == 2_320_005).is_none(), "not ours to take");
        let inside = inb.translate_with(protocol::build_circular_call_setup(&remote, 1234, 2_320_005, 0), |_| false, |i| i == 2_320_005).expect("a visitor");
        let local = call_id(&inside).unwrap();
        assert_ne!(local, remote, "a new id here");
        // The answer goes back under the remote id, the release forgets it.
        let back = out.translate(protocol::build_call_cause(protocol::CALL_RELEASE, &local, 0)).unwrap();
        assert_eq!(call_id(&back), Some(remote));
        assert!(out.translate(protocol::build_call_cause(protocol::CALL_RELEASE, &local, 0)).is_none());
        // Our own setup going out gets a new id there.
        let mine = Uuid::new_v4();
        let sent = out.translate(protocol::build_short_transfer(&mine, 2_320_004, 1234)).unwrap();
        assert_ne!(call_id(&sent), Some(mine));
    }

    #[tokio::test]
    async fn only_the_link_may_start_a_call_on_its_networks_group() {
        let state = AppState::for_test();
        if let Ok(mut n) = state.networks.write() { n.networks.push(NetworkConfig { name: "TP".into(), ..Default::default() }); }
        let slot = slots(&state)["TP"];
        if let Ok(mut p) = state.pools.write() {
            p.push(TalkgroupPoolConfig { name: "B".into(), enabled: true, both_ways: true, groups: vec![9],
                remote: vec![RemotePoolGroup { network: "TP".into(), gssi: 909 }], linked: false });
        }
        let (bs, link) = (Uuid::new_v4(), Uuid::new_v4());
        {
            let mut inner = state.inner.write().await;
            for (id, mode) in [(bs, ClientMode::Basestation), (link, ClientMode::Network)] {
                let (tx, rx) = mpsc::unbounded_channel();
                std::mem::forget(rx);
                inner.clients.insert(id, Client { tx, mode, version: ConnVersion::V1, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
            }
            inner.netlinks.insert(slot, link);
        }
        let target = alias(slot, 909);
        crate::router::handle_packet(state.clone(), bs, protocol::build_group_tx(&Uuid::new_v4(), 7, target, 0)).await;
        assert!(state.inner.read().await.calls.is_empty(), "a station may not start a call on another network's group");
        crate::router::handle_packet(state.clone(), link, protocol::build_group_tx(&Uuid::new_v4(), 7, target, 0)).await;
        assert!(state.inner.read().await.calls.values().any(|c| c.destination == target), "the link may");
    }

    /// Test state with network "TP" linked (forwarding unknown targets), pool
    /// TG 9 <-> TP:909, a station and the link. Returns (state, station, link, slot).
    async fn linked_tp() -> (Arc<AppState>, ClientId, ClientId, u8) {
        let state = AppState::for_test();
        if let Ok(mut n) = state.networks.write() {
            n.enabled = true;
            n.networks.push(NetworkConfig { name: "TP".into(), enabled: true, forward_unknown: true, ..Default::default() });
        }
        let slot = slots(&state)["TP"];
        if let Ok(mut p) = state.pools.write() {
            p.push(TalkgroupPoolConfig { name: "B".into(), enabled: true, both_ways: true, groups: vec![9],
                remote: vec![RemotePoolGroup { network: "TP".into(), gssi: 909 }], linked: false });
        }
        let (bs, link) = (Uuid::new_v4(), Uuid::new_v4());
        {
            let mut inner = state.inner.write().await;
            for (id, mode) in [(bs, ClientMode::Basestation), (link, ClientMode::Network)] {
                let (tx, rx) = mpsc::unbounded_channel();
                std::mem::forget(rx);
                inner.clients.insert(id, Client { tx, mode, version: ConnVersion::V1, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
            }
            inner.netlinks.insert(slot, link);
        }
        (state, bs, link, slot)
    }

    /// The dashboard shows which network a call or SDS went to or came from.
    #[tokio::test]
    async fn dashboard_shows_the_network_a_call_or_sds_crosses() {
        let (state, bs, link, slot) = linked_tp().await;
        let tp = vec!["TP".to_string()];
        // Group call on TG 9: its pool leg goes to TP:909.
        let call = Uuid::new_v4();
        crate::router::handle_packet(state.clone(), bs, protocol::build_group_tx(&call, 2_320_001, 9, 0)).await;
        let snap = state.monitor.snapshot(0, 0, 0).await;
        let row = snap.active_calls.iter().find(|c| c.uuid == call).expect("call shown");
        assert_eq!(row.nets.to, tp, "the call shows the network its leg goes to");
        assert_eq!(row.nets.from, None);
        let leg = snap.active_calls.iter().find(|c| c.destination == alias(slot, 909)).expect("leg shown");
        assert_eq!(leg.nets.to, tp);
        // A call that came in over the link (TP:909 is not busy there).
        let (state, bs, link, slot) = linked_tp().await;
        let inbound = Uuid::new_v4();
        crate::router::handle_packet(state.clone(), link, protocol::build_group_tx(&inbound, 7, alias(slot, 909), 0)).await;
        let snap = state.monitor.snapshot(0, 0, 0).await;
        let row = snap.active_calls.iter().find(|c| c.uuid == inbound).expect("inbound call shown");
        assert_eq!(row.nets.from.as_deref(), Some("TP"));
        // SDS to a target nobody here takes: forwarded to TP.
        let sds = Uuid::new_v4();
        crate::router::handle_packet(state.clone(), bs, protocol::build_short_transfer(&sds, 2_320_001, 2_999_999)).await;
        let snap = state.monitor.snapshot(0, 0, 0).await;
        let row = snap.recent_sds.iter().find(|s| s.uuid == sds).expect("sds shown");
        assert_eq!(row.nets.to, tp);
        let json = serde_json::to_value(row).unwrap();
        assert_eq!(json["to_networks"], serde_json::json!(["TP"]));
        assert_eq!(json["source"], serde_json::json!(2_320_001));
    }

    fn voice(id: &Uuid) -> Vec<u8> {
        protocol::build_traffic_frame(id, &[1; protocol::ACELP_CODED_FRAME_BYTES], &[2; protocol::ACELP_CODED_FRAME_BYTES])
    }

    /// Call id of the next voice frame a station gets within `ms`.
    async fn next_voice(ws: &mut Ws, ms: u64) -> Option<Uuid> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(Ok(msg))) = tokio::time::timeout(left, ws.next()).await else { return None };
            if let Message::Binary(b) = msg {
                if b.first() == Some(&protocol::CLASS_FRAME) && b.get(1) == Some(&protocol::FRAME_TRAFFIC_CHANNEL) { return call_id(&b); }
            }
        }
    }

    /// Next CALL_GROUP_IDLE a station gets within `ms`.
    async fn next_idle(ws: &mut Ws, ms: u64) -> Option<Uuid> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(Ok(msg))) = tokio::time::timeout(left, ws.next()).await else { return None };
            if let Message::Binary(b) = msg {
                if is_group_idle(&b) { return call_id(&b); }
            }
        }
    }

    /// The setup of the first field test: the server linked to itself.
    /// Nothing may hang: every call ends when its station ends it.
    #[tokio::test]
    async fn loop_to_the_same_server_ends_every_call() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut cfg = base_config();
        cfg.multi_network.networks.push(NetworkConfig {
            // A link to this same server must not register the talkers:
            // that would move them off their own station here.
            name: "Loop".into(), remote_host: addr.to_string(), bridge_issi: 9_000_001, register_talkers: false, ..Default::default()
        });
        cfg.talkgroup_pools.push(TalkgroupPoolConfig {
            name: "TestLink".into(), enabled: true, both_ways: true, groups: vec![9],
            remote: vec![RemotePoolGroup { network: "Loop".into(), gssi: 909 }],
            linked: false,
        });
        let state = Arc::new(AppState::new(cfg, "test.toml".into()).0);
        let app = axum::Router::new()
            .route("/brew", axum::routing::get(crate::server::brew_discovery))
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await });
        run(state.clone()).await;
        let mut a = station(addr, &[100], &[9]).await;
        let mut b = station(addr, &[200], &[909]).await;
        wait_until("bridge on TG 909", async || {
            state.inner.read().await.subscribers.get(&9_000_001).is_some_and(|s| s.groups.contains(&909))
        }).await;
        wait_until("radios", async || state.inner.read().await.subscribers.len() == 3).await;

        // TG 9 -> TG 909 and the end.
        let call = Uuid::new_v4();
        a.send(Message::Binary(protocol::build_group_tx(&call, 100, 9, 0).into())).await.unwrap();
        let (b_call, src, dst) = next_group_tx(&mut b, 2000).await.expect("909 hears TG 9");
        assert_eq!((src, dst), (100, 909));
        // Voice crosses, under the call id 909 knows.
        a.send(Message::Binary(voice(&call).into())).await.unwrap();
        assert_eq!(next_voice(&mut b, 2000).await, Some(b_call), "909 hears the voice");
        a.send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &call, 0).into())).await.unwrap();
        assert!(next_idle(&mut b, 2000).await.is_some());
        wait_until("no call left", async || state.inner.read().await.calls.is_empty()).await;

        // TG 909 -> TG 9, TG 9 answers in the call, the answer ends it.
        let x = Uuid::new_v4();
        b.send(Message::Binary(protocol::build_group_tx(&x, 200, 909, 0).into())).await.unwrap();
        let (leg, src, dst) = next_group_tx(&mut a, 2000).await.expect("TG 9 hears 909");
        assert_eq!((src, dst), (200, 9));
        b.send(Message::Binary(voice(&x).into())).await.unwrap();
        assert_eq!(next_voice(&mut a, 2000).await, Some(leg), "TG 9 hears the voice");
        a.send(Message::Binary(protocol::build_group_tx(&leg, 100, 9, 0).into())).await.unwrap();
        let (id, src, dst) = next_group_tx(&mut b, 2000).await.expect("909 hears the answer");
        assert_eq!((id, src, dst), (x, 100, 909));
        a.send(Message::Binary(voice(&leg).into())).await.unwrap();
        assert_eq!(next_voice(&mut b, 2000).await, Some(x), "909 hears the answer's voice");
        a.send(Message::Binary(protocol::build_call_cause(protocol::CALL_GROUP_IDLE, &leg, 0).into())).await.unwrap();
        assert_eq!(next_idle(&mut b, 2000).await, Some(x));
        wait_until("no call left after the answer", async || state.inner.read().await.calls.is_empty()).await;
    }
}
