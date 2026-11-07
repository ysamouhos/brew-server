//! Talkgroup pools: groups switched together inside this server.
//!
//! A call on a group of a pool that is switched on is played 1:1 on the
//! pool's other groups. Each copy (a *leg*) is an ordinary group call on its
//! own GSSI with its own call identifier, so a Basestation sets it up for its
//! radios like any other group call from the network and the radios see
//! nothing unusual. The voice frames of the call are copied to every leg, and
//! the legs end with the call.
//!
//! A group that has a call of its own when the pool call starts is not cut
//! off: its leg waits until that call has ended and then joins the running
//! pool call (late entry, the same way a radio joining mid-over does).
//!
//! Pools are edited live from the dashboard and written to the config file
//! without a restart (as the blacklist is). Only this server's own
//! connections get the legs; federation peers do not.
//!
//! A pool may also list groups of other networks (`remote`, see `netlink`):
//! inside the pool they are addressed by their alias, so a leg towards
//! another network goes to that network's link only, and a call coming in
//! over a link puts up legs on this server's groups like any other call.

use crate::config::TalkgroupPoolConfig;
use crate::protocol;
use crate::state::{ActiveCall, AppState, CallKind, ClientId, ClientMode, Inner};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

/// Owner of every leg: not a connection, so no connection can send voice or
/// end a leg as if it were the talker. Legs end with their pool call.
pub const POOL_OWNER: ClientId = Uuid::from_u128(0x706f_6f6c_0000_4000_8000_0000_0000_0001);

/// Longest pool name accepted from the dashboard.
const MAX_NAME: usize = 40;

/// The legs of one running pool call, keyed by the call that started them.
#[derive(Debug, Clone)]
pub struct PoolRun {
    /// Pool (name) that put the first leg up.
    pub pool: String,
    /// Group the call is on.
    pub origin_gssi: u32,
    /// Connection the call came in on (its talker's station).
    pub owner: ClientId,
    pub source_issi: u32,
    pub priority: u8,
    /// Running legs: target group -> leg call identifier.
    pub legs: BTreeMap<u32, Uuid>,
    /// Target groups that had a call of their own; they join when it ends.
    pub waiting: BTreeSet<u32>,
    /// A radio answering in one of the legs (a new over on the leg's call
    /// identifier, as any Brew station does when a radio talks in a call it
    /// hears): that leg and the connection the answer comes from. Its voice
    /// then goes to the original call and to the other legs, so the answer
    /// is heard on every group of the pool, across networks too.
    pub answer: Option<(Uuid, ClientId)>,
}

/// Pool state kept with the calls (under `AppState::inner`).
#[derive(Debug, Default)]
pub struct PoolCalls {
    pub runs: std::collections::HashMap<Uuid, PoolRun>,
    /// Leg call identifier -> the call it copies.
    pub legs: std::collections::HashMap<Uuid, Uuid>,
}

/// Members of `pool` as this server addresses them: its own groups, then the
/// aliases of its groups of other networks (`slots`: network name -> slot; a
/// network without one is left out).
pub fn members(pool: &TalkgroupPoolConfig, slots: &HashMap<String, u8>) -> Vec<u32> {
    let remote = pool.remote.iter()
        .filter_map(|r| slots.get(&r.network).map(|&s| crate::netlink::alias(s, r.gssi)));
    pool.groups.iter().copied().chain(remote).collect()
}

/// Groups a call on `gssi` is copied to by the pools switched on in `pools`,
/// each with the name of the (first) pool that copies it there.
pub fn targets_for(pools: &[TalkgroupPoolConfig], slots: &HashMap<String, u8>, gssi: u32) -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    for p in pools.iter().filter(|p| p.enabled) {
        let groups = members(p, slots);
        let copies = if p.both_ways { groups.contains(&gssi) } else { groups.first() == Some(&gssi) };
        if !copies { continue; }
        for &t in groups.iter().filter(|&&t| t != gssi) {
            out.entry(t).or_insert_with(|| p.name.clone());
        }
    }
    out
}

/// Checks a pool list from the dashboard; the error says what is wrong.
pub fn validate(pools: &[TalkgroupPoolConfig]) -> Result<(), String> {
    let mut names = HashSet::new();
    for p in pools {
        let name = p.name.trim();
        if name.is_empty() { return Err("every pool needs a name".into()); }
        if name.chars().count() > MAX_NAME { return Err(format!("pool name '{name}' is longer than {MAX_NAME} characters")); }
        if !names.insert(name.to_lowercase()) { return Err(format!("two pools are named '{name}'")); }
        if p.groups.len() + p.remote.len() < 2 { return Err(format!("pool '{name}' needs at least two groups")); }
        let mut seen = HashSet::new();
        for &g in &p.groups {
            if g == 0 || g > 0xFF_FFFF { return Err(format!("pool '{name}': {g} is not a valid GSSI (1 to 16777215)")); }
            if !seen.insert(g) { return Err(format!("pool '{name}' lists group {g} twice")); }
        }
        let mut seen_remote = HashSet::new();
        for r in &p.remote {
            let g = r.gssi;
            if r.network.trim().is_empty() { return Err(format!("pool '{name}': a group of another network needs its network")); }
            if g == 0 || g > 0xFF_FFFF { return Err(format!("pool '{name}': {g} is not a valid GSSI (1 to 16777215)")); }
            if !seen_remote.insert((r.network.as_str(), g)) { return Err(format!("pool '{name}' lists group {g} of {} twice", r.network)); }
        }
    }
    Ok(())
}

/// Replaces the pools on the running server and writes the config file
/// without a restart. Running pool calls keep their legs until they end.
pub async fn apply(state: &Arc<AppState>, mut pools: Vec<TalkgroupPoolConfig>, by: &str) -> Result<(), String> {
    for p in &mut pools {
        p.name = p.name.trim().to_owned();
        for r in &mut p.remote { r.network = r.network.trim().to_owned(); }
    }
    validate(&pools)?;
    let networks = state.networks.read().map(|c| c.clone()).unwrap_or_default();
    crate::netlink::validate(&networks, &pools)?;
    if let Ok(mut live) = state.pools.write() { *live = pools.clone(); }
    crate::netlink::refresh_affiliations(state);
    crate::netlink::sync_members(state).await;
    tracing::warn!(count = pools.len(), by, "talkgroup pools changed");
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    let saved = crate::dashboard::save_config(state, &cfg).await;
    if saved.is_err() {
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    saved.map_err(|e| format!("applied, but writing the config file failed: {e}"))
}

/// Whether `gssi` has a group call other than `origin` and the legs of
/// `origin`'s own run.
fn busy(inner: &Inner, gssi: u32, origin: Uuid) -> bool {
    let own = inner.pools.runs.get(&origin);
    inner.calls.iter().any(|(id, c)| {
        c.kind == CallKind::Group && c.destination == gssi && *id != origin
            && !own.is_some_and(|r| r.legs.values().any(|l| l == id))
    })
}

/// Connections a leg on `gssi` goes to: the ones routing the group, as for
/// any group call, but no federation peer and no link to another network. A
/// leg on a group of another network goes to that network's link only.
fn leg_targets(state: &AppState, inner: &Inner, gssi: u32) -> HashSet<ClientId> {
    if crate::netlink::split_alias(gssi).is_some() {
        // That network's link, and the stations with its visitors in the group.
        let mut t = crate::visitors::targets(inner, gssi);
        t.extend(crate::netlink::link_for(inner, gssi));
        return t;
    }
    // Never broadcast to a station of another network (see
    // `visitors::other_network_station`); it gets the leg where it routes the group.
    let broadcast = || inner.clients.keys().copied().filter(|c| !crate::visitors::other_network_station(state, inner, *c)).collect::<HashSet<_>>();
    let mut targets = if state.config.route_without_affiliations {
        let mut t = broadcast();
        t.extend(inner.group_clients.get(&gssi).into_iter().flatten().copied());
        t
    } else {
        inner.group_clients.get(&gssi).cloned().unwrap_or_default()
    };
    if targets.is_empty() && !state.config.route_without_affiliations && state.config.fallback_broadcast_when_no_affiliations {
        targets = broadcast();
    }
    targets.retain(|c| inner.clients.get(c).is_some_and(|c| !matches!(c.mode, ClientMode::Peer | ClientMode::Network)));
    targets
}

type Out = Vec<(mpsc::UnboundedSender<Vec<u8>>, Vec<u8>)>;

/// Queues `header` for connection `c`: a station gets a group of another
/// network as that group's own GSSI with its network before it (`mni`).
fn push_to(state: &AppState, c: &crate::state::Client, header: &[u8], out: &mut Out) {
    if c.mode == ClientMode::Network {
        out.push((c.tx.clone(), protocol::adapt_to_version(header, c.forward_version()).into_owned()));
        return;
    }
    for p in crate::visitors::for_station(state, header) {
        out.push((c.tx.clone(), protocol::adapt_to_version(&p, c.forward_version()).into_owned()));
    }
}

/// Announces talker `source_issi` on leg `leg` (group `gssi`) to everyone
/// the leg reaches but `except`, and makes the leg a copy again (owned by
/// the pool).
#[allow(clippy::too_many_arguments)]
fn reannounce_leg(state: &AppState, inner: &mut Inner, leg: Uuid, gssi: u32, source_issi: u32, priority: u8, except: Option<ClientId>, out: &mut Out) {
    let mut peers = leg_targets(state, inner, gssi);
    if let Some(x) = except { peers.remove(&x); }
    let header = protocol::build_group_tx(&leg, source_issi, gssi, priority);
    for cid in &peers {
        if let Some(c) = inner.clients.get(cid) {
            push_to(state, c, &header, out);
        }
    }
    if let Some(call) = inner.calls.get_mut(&leg) {
        call.owner = POOL_OWNER;
        call.source_issi = source_issi;
        call.peers = peers;
        call.touch();
    }
    inner.group_floor.insert(gssi, leg);
    inner.group_tx_raw.insert(leg, header);
}

/// A radio answers in leg `leg` of `origin`'s run, over connection
/// `source`: the original call and the other legs get the new talker.
#[allow(clippy::too_many_arguments)]
fn announce_answer(state: &AppState, inner: &mut Inner, origin: Uuid, leg: Uuid, source: ClientId, source_issi: u32, priority: u8, out: &mut Out) {
    let Some(run) = inner.pools.runs.get(&origin).cloned() else { return };
    // The original call: everyone it reaches, its own station included.
    if let Some(call) = inner.calls.get(&origin) {
        let gssi = call.destination;
        let mut audience = call.peers.clone();
        audience.insert(call.owner);
        audience.remove(&source);
        let header = protocol::build_group_tx(&origin, source_issi, gssi, priority);
        for cid in &audience {
            if let Some(c) = inner.clients.get(cid) {
                push_to(state, c, &header, out);
            }
        }
        call.touch();
        inner.group_tx_raw.insert(origin, header);
    }
    for (&t, &other) in run.legs.iter().filter(|(_, l)| **l != leg) {
        reannounce_leg(state, inner, other, t, source_issi, priority, Some(source), out);
    }
    if let Some(r) = inner.pools.runs.get_mut(&origin) { r.answer = Some((leg, source)); }
    info!(origin=%origin, leg=%leg, src_issi=source_issi, "talkgroup pool: answer in a leg, heard on the whole pool");
}

/// Sets up the leg of `origin`'s run on `gssi`: a new group call there, its
/// GROUP_TX queued for each target. Returns the leg's identifier.
fn start_leg(state: &AppState, inner: &mut Inner, origin: Uuid, gssi: u32, out: &mut Out) -> Option<Uuid> {
    let run = inner.pools.runs.get(&origin)?.clone();
    let leg = Uuid::new_v4();
    let peers = leg_targets(state, inner, gssi);
    let header = protocol::build_group_tx(&leg, run.source_issi, gssi, run.priority);
    for cid in &peers {
        if let Some(c) = inner.clients.get(cid) {
            push_to(state, c, &header, out);
        }
    }
    inner.calls.insert(leg, ActiveCall {
        kind: CallKind::Group,
        owner: POOL_OWNER,
        source_issi: run.source_issi,
        destination: gssi,
        priority: run.priority,
        peers: peers.clone(),
        started_at: std::time::Instant::now(),
        last_activity_ms: ActiveCall::new_activity(),
    });
    inner.group_floor.entry(gssi).or_insert(leg);
    inner.group_tx_raw.insert(leg, header);
    inner.pools.legs.insert(leg, origin);
    if let Some(r) = inner.pools.runs.get_mut(&origin) {
        r.legs.insert(gssi, leg);
        r.waiting.remove(&gssi);
    }
    info!(origin=%origin, leg=%leg, from_gssi=run.origin_gssi, gssi, pool=%run.pool, targets=peers.len(), "talkgroup pool: leg set up");
    Some(leg)
}

/// A group call was routed (`GROUP_TX` on `id` from `source`): puts up its
/// legs on the groups its pools copy it to, or, for a new over of a call
/// that already has legs, announces the new talker on them. Calls the legs
/// started for the monitor.
pub async fn on_group_tx(state: &Arc<AppState>, source: ClientId, id: Uuid, source_issi: u32, gssi: u32, priority: u8) {
    let pools = state.pools.read().map(|p| p.clone()).unwrap_or_default();
    let slots = crate::netlink::slots(state);
    let mut started = Vec::new();
    {
        let mut inner = state.inner.write().await;
        let mut out: Out = Vec::new();
        // A GROUP_TX on a leg's identifier: a radio answers in the leg's
        // call. The answer is announced on the original call and the other
        // legs too (see `PoolRun::answer`).
        if let Some(origin) = inner.pools.legs.get(&id).copied() {
            announce_answer(state, &mut inner, origin, id, source, source_issi, priority, &mut out);
            for (tx, msg) in out { let _ = tx.send(msg); }
            return;
        }
        let mut wanted = targets_for(&pools, &slots, gssi);
        // A group that does not admit the talker gets no copy (`group_access`).
        wanted.retain(|t, _| state.group_permits(*t, source_issi));
        if wanted.is_empty() && !inner.pools.runs.contains_key(&id) { return; }
        let pool = wanted.values().next().cloned().unwrap_or_default();
        let run = inner.pools.runs.entry(id).or_insert_with(|| PoolRun {
            pool, origin_gssi: gssi, owner: source, source_issi, priority,
            legs: BTreeMap::new(), waiting: BTreeSet::new(), answer: None,
        });
        run.owner = source;
        run.source_issi = source_issi;
        run.priority = priority;
        run.answer = None;
        let existing = run.legs.clone();
        // New over of a call with legs: the legs get the new talker (a leg a
        // radio answered in is a copy again, for its whole group).
        for (&t, &leg) in &existing {
            reannounce_leg(state, &mut inner, leg, t, source_issi, priority, None, &mut out);
        }
        for &t in wanted.keys().filter(|t| !existing.contains_key(t)) {
            if busy(&inner, t, id) {
                if let Some(r) = inner.pools.runs.get_mut(&id) { r.waiting.insert(t); }
                info!(origin=%id, gssi=t, "talkgroup pool: group has its own call, joins when it ends");
            } else if let Some(leg) = start_leg(state, &mut inner, id, t, &mut out) {
                started.push((leg, t));
            }
        }
        for (tx, msg) in out { let _ = tx.send(msg); }
    }
    for (leg, t) in started {
        state.monitor.call_started(leg, "group", source_issi, t, priority).await;
        // The call shows the networks its legs go to.
        let to = crate::netlink::note_call(state, leg).await;
        state.monitor.note_networks(id, None, to).await;
    }
}

/// A voice or DTMF frame of call `id` was routed: copies it to the call's
/// legs (same frame, the leg's call identifier).
pub async fn on_frame(state: &Arc<AppState>, id: Uuid, raw: &[u8]) {
    let inner = state.inner.read().await;
    if raw.len() < 18 { return; }
    // Voice of an answer in a leg: to the original call and the other legs.
    if let Some(origin) = inner.pools.legs.get(&id) {
        let Some(run) = inner.pools.runs.get(origin) else { return };
        let Some((_, from)) = run.answer.filter(|(l, _)| *l == id) else { return };
        if let Some(call) = inner.calls.get(origin) {
            let mut frame = raw.to_vec();
            frame[2..18].copy_from_slice(origin.as_bytes());
            let mut audience = call.peers.clone();
            audience.insert(call.owner);
            audience.remove(&from);
            for cid in &audience {
                if let Some(c) = inner.clients.get(cid) { let _ = c.tx.send(frame.clone()); }
            }
            call.touch();
        }
        for leg in run.legs.values().filter(|l| **l != id) {
            let Some(call) = inner.calls.get(leg) else { continue };
            call.touch();
            let mut frame = raw.to_vec();
            frame[2..18].copy_from_slice(leg.as_bytes());
            for cid in call.peers.iter().filter(|c| **c != from) {
                if let Some(c) = inner.clients.get(cid) { let _ = c.tx.send(frame.clone()); }
            }
        }
        return;
    }
    let Some(run) = inner.pools.runs.get(&id) else { return };
    for leg in run.legs.values() {
        let Some(call) = inner.calls.get(leg) else { continue };
        call.touch();
        let mut frame = raw.to_vec();
        frame[2..18].copy_from_slice(leg.as_bytes());
        for cid in &call.peers {
            if let Some(c) = inner.clients.get(cid) { let _ = c.tx.send(frame.clone()); }
        }
    }
}

/// Call `id` on `gssi` has ended. Returns the legs to end with it (the
/// caller ends them like any call), and starts the legs that waited for a
/// call on `gssi` to end.
pub async fn on_call_end(state: &Arc<AppState>, id: Uuid, gssi: u32) -> Vec<Uuid> {
    let mut to_end = Vec::new();
    let mut started = Vec::new();
    {
        let mut inner = state.inner.write().await;
        if let Some(origin) = inner.pools.legs.remove(&id) {
            let answered = inner.pools.runs.get(&origin).is_some_and(|r| r.answer.is_some_and(|(l, _)| l == id));
            if let Some(r) = inner.pools.runs.get_mut(&origin) { r.legs.retain(|_, l| *l != id); }
            // The station of the last talker ended the call it answered in:
            // as with any Brew group call, that ends the call for everyone,
            // so the original call (and with it the other legs) ends too.
            if answered {
                if let Some(call) = inner.calls.get_mut(&origin) {
                    let first = call.owner;
                    call.peers.insert(first);
                    call.owner = POOL_OWNER;
                    to_end.push(origin);
                    info!(origin=%origin, leg=%id, "talkgroup pool: the answer's call ended, ending the pool call");
                }
            }
        }
        if let Some(run) = inner.pools.runs.remove(&id) {
            to_end.extend(run.legs.values().copied());
            info!(origin=%id, legs=run.legs.len(), "talkgroup pool: call ended, ending its legs");
        }
        // Runs whose call went without an end (pre-empted): drop them too.
        let gone: Vec<Uuid> = inner.pools.runs.keys().filter(|o| !inner.calls.contains_key(o)).copied().collect();
        for o in gone {
            if let Some(run) = inner.pools.runs.remove(&o) { to_end.extend(run.legs.values().copied()); }
        }
        for leg in &to_end {
            inner.pools.legs.remove(leg);
            // A leg a radio answered in is owned by that radio's station
            // now: hand it back so it ends with the call, for everyone.
            if let Some(call) = inner.calls.get_mut(leg) {
                if call.owner != POOL_OWNER {
                    let answered_by = call.owner;
                    call.peers.insert(answered_by);
                    call.owner = POOL_OWNER;
                }
            }
        }
        // Legs waiting for this group to become free join now.
        let waiting: Vec<Uuid> = inner.pools.runs.iter()
            .filter(|(_, r)| r.waiting.contains(&gssi))
            .map(|(o, _)| *o).collect();
        let mut out: Out = Vec::new();
        for origin in waiting {
            if busy(&inner, gssi, origin) { continue; }
            if let Some(leg) = start_leg(state, &mut inner, origin, gssi, &mut out) {
                let r = &inner.pools.runs[&origin];
                started.push((leg, r.source_issi, r.priority, origin));
                info!(origin=%origin, leg=%leg, gssi, "talkgroup pool: waiting group joined the running call");
            }
        }
        for (tx, msg) in out { let _ = tx.send(msg); }
    }
    for (leg, src, prio, origin) in started {
        state.monitor.call_started(leg, "group", src, gssi, prio).await;
        let to = crate::netlink::note_call(state, leg).await;
        state.monitor.note_networks(origin, None, to).await;
    }
    to_end
}

/// What the dashboard shows of a pool.
#[derive(Debug, Serialize)]
pub struct PoolView {
    #[serde(flatten)]
    pub pool: TalkgroupPoolConfig,
    /// Running call on the pool, if any.
    pub live: Option<LiveView>,
}

#[derive(Debug, Serialize)]
pub struct LiveView {
    pub gssi: u32,
    pub talker: u32,
    pub legs: Vec<u32>,
    pub waiting: Vec<u32>,
}

/// The pools with their running calls, for `/api/pools`.
pub async fn snapshot(state: &Arc<AppState>) -> Vec<PoolView> {
    let pools = state.pools.read().map(|p| p.clone()).unwrap_or_default();
    let inner = state.inner.read().await;
    pools.into_iter().map(|p| {
        let live = inner.pools.runs.values()
            .find(|r| r.pool == p.name)
            .map(|r| LiveView { gssi: r.origin_gssi, talker: r.source_issi, legs: r.legs.keys().copied().collect(), waiting: r.waiting.iter().copied().collect() });
        PoolView { pool: p, live }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(name: &str, both: bool, groups: &[u32]) -> TalkgroupPoolConfig {
        TalkgroupPoolConfig { name: name.into(), enabled: true, both_ways: both, groups: groups.to_vec(), remote: Vec::new() }
    }

    fn none() -> HashMap<String, u8> { HashMap::new() }

    #[test]
    fn both_ways_pool_copies_from_every_group() {
        let p = [pool("Nord", true, &[2321, 2323, 264])];
        assert_eq!(targets_for(&p, &none(), 2321).keys().copied().collect::<Vec<_>>(), vec![264, 2323]);
        assert_eq!(targets_for(&p, &none(), 264).keys().copied().collect::<Vec<_>>(), vec![2321, 2323]);
        assert!(targets_for(&p, &none(), 9).is_empty());
    }

    #[test]
    fn one_way_pool_copies_only_from_its_first_group() {
        let p = [pool("Info", false, &[9, 2321, 2322])];
        assert_eq!(targets_for(&p, &none(), 9).keys().copied().collect::<Vec<_>>(), vec![2321, 2322]);
        assert!(targets_for(&p, &none(), 2321).is_empty());
    }

    #[test]
    fn switched_off_pool_copies_nothing() {
        let mut p = pool("Off", true, &[1, 2]);
        p.enabled = false;
        assert!(targets_for(&[p], &none(), 1).is_empty());
    }

    #[test]
    fn validation_rejects_bad_pools() {
        assert!(validate(&[pool("A", true, &[1, 2])]).is_ok());
        assert!(validate(&[pool("", true, &[1, 2])]).is_err());
        assert!(validate(&[pool("A", true, &[1])]).is_err());
        assert!(validate(&[pool("A", true, &[1, 1])]).is_err());
        assert!(validate(&[pool("A", true, &[1, 0x100_0000])]).is_err());
        assert!(validate(&[pool("A", true, &[1, 2]), pool("a", true, &[3, 4])]).is_err());
    }
}
