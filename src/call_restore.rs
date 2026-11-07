//! Restoring an individual call on another Basestation (extension
//! `call-restore`).
//!
//! A radio in an individual call that changes cells keeps the call: on the new
//! cell it sends U-CALL RESTORE with the old cell's call identifier and the
//! other party's address (TS 100 392-2 clause 14.5.1.2.4). When the new cell
//! belongs to another Basestation, that Basestation does not hold the call.
//! The server moves it there:
//!
//! - When the radio registers on a Basestation that announced the
//!   `call-restore` capability, the radio's side of each Brew-to-Brew
//!   individual call it takes part in moves to that Basestation. The
//!   Basestation it left gets `CALL_RELEASE` with "SwMI requested
//!   disconnection" for the call, which ends its leg without a release back
//!   to the network (as any Basestation handles a network release); the
//!   other party is not told anything.
//! - The new Basestation asks with `SERVICE_CALL_RESTORE`
//!   (`{"issi":N,"peer":N}`, `peer` 0 for the dummy address) once the radio
//!   sends U-CALL RESTORE, and gets the call's `uuid`, `duplex`, `priority`
//!   and the ISSI holding a simplex call's `floor` (0 for none), or the same
//!   request back without `uuid` when there is no such call.
//! - A call not restored within `RESTORE_WINDOW` ends: the other party gets
//!   `CALL_RELEASE` with "call restoration of the other user failed" (table
//!   14.55).
//! - It ends the same way at once when the radio shows it has given the call
//!   up: its Basestation reports a plain roaming registration with
//!   `SERVICE_CALL_NOT_RESTORED` (`{"issi":N}`; a radio with a circuit mode
//!   call registers with "service restoration roaming", clause 16.4 a)), or
//!   the radio sets up a new call (after a cell change it sends no call
//!   related signalling for the call other than U-CALL RESTORE, clause
//!   14.5.1.2.4).
//!
//! Calls bridged to SIP are left alone: their media path is bound to the
//! connection that set them up.
//!
//! Calls between two radios of one Basestation (extension `call-handoff`,
//! `SERVICE_CALL_HANDOFF`):
//!
//! - A call between two radios of one Basestation runs there alone; the server
//!   does not hold it. When one radio restores it on another Basestation, that
//!   one asks with `SERVICE_CALL_RESTORE` as for any call. Holding none, the
//!   server asks the Basestation where `peer` is registered, if it announced
//!   `call-handoff`: `{"issi":N,"peer":N}`. That Basestation bridges `peer`'s
//!   leg to Brew and answers with the call (`uuid`, `calling`, `duplex`,
//!   `priority`, `floor`), or without `uuid`. The server records it as a
//!   private call between the two Basestations and answers the restore
//!   request with it. No answer within `HANDOFF_TIMEOUT`: no call.
//! - The other way round, a radio that registers on the Basestation its peer
//!   is on, if that one announced `call-handoff`: the Basestation it left lets
//!   its leg go ("SwMI requested disconnection"), and the new one joins the two
//!   legs itself and ends the Brew call with the same cause. Not joined within
//!   `RESTORE_WINDOW`, the call ends for the peer as any moved call.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::protocol;
use crate::state::{ActiveCall, AppState, CallKind, ClientId, Inner};

/// Capability in `SERVICE_BTS_INFO` (and extension in `X-Brew-Extensions`)
/// for `SERVICE_CALL_RESTORE`.
pub const CAPABILITY: &str = "call-restore";

/// How long a moved call waits for the radio to restore it. Project value,
/// not an ETSI timer: the radio registers on the new cell first and then
/// waits for D-CALL RESTORE with T306 (4 s to 6 s, clause 14.6); the field
/// test of 2026-10-30 had the U-CALL RESTORE 0.9 s after the registration.
pub const RESTORE_WINDOW: Duration = Duration::from_secs(8);

/// Capability in `SERVICE_BTS_INFO` (and extension in `X-Brew-Extensions`)
/// for `SERVICE_CALL_HANDOFF`.
pub const CAPABILITY_HANDOFF: &str = "call-handoff";

/// How long a restore request waits for the Basestation asked to hand its
/// local call over. Project value: the asking Basestation gives the whole
/// answer 2 s (FlowStation `NETWORK_RESTORE_TIMEOUT_TS`).
pub const HANDOFF_TIMEOUT: Duration = Duration::from_millis(1500);

/// What the server knows of a Brew-to-Brew individual call beyond `ActiveCall`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CallInfo {
    duplex: bool,
    /// ISSI of the party a simplex call's floor was last granted to.
    floor: Option<u32>,
}

/// A call moved to Basestation `on` for `issi`, not restored yet.
#[derive(Debug, Clone, Copy)]
struct Waiting {
    issi: u32,
    on: ClientId,
    /// The radio came to the Basestation its peer is on, which joins the two
    /// legs itself (`call-handoff`); this is the Basestation it left. Not
    /// joined in time, the call ends for the peer on `on`.
    joins_from: Option<ClientId>,
}

/// A restore request from `asker` for `issi`'s call with `peer`, waiting for
/// the Basestation holding the call locally to hand it over.
#[derive(Debug, Clone, Copy)]
struct PendingHandoff {
    asker: ClientId,
    peer: u32,
}

#[derive(Default)]
pub struct CallRestores {
    calls: HashMap<Uuid, CallInfo>,
    waiting: HashMap<Uuid, Waiting>,
    /// (Basestation asked, issi) -> the restore request waiting for its answer
    handoffs: HashMap<(ClientId, u32), PendingHandoff>,
}

/// A private call between two Brew connections was set up.
pub fn note_setup(inner: &mut Inner, id: Uuid, duplex: bool) {
    let Inner { calls, call_restores, .. } = inner;
    call_restores.calls.retain(|id, _| calls.contains_key(id));
    call_restores.waiting.retain(|id, _| calls.contains_key(id));
    call_restores.calls.insert(id, CallInfo { duplex, floor: None });
}

/// `CALL_SIMPLEX_GRANTED` (`granted`) or `CALL_SIMPLEX_IDLE` from `source`
/// for private call `id`: the floor is with the radio on `source`'s side, or
/// free.
pub fn note_floor(inner: &mut Inner, source: ClientId, id: Uuid, granted: bool) {
    let Some(call) = inner.calls.get(&id) else { return };
    let holder = if !granted {
        None
    } else if call.owner == source {
        Some(call.source_issi)
    } else {
        Some(call.destination)
    };
    if let Some(info) = inner.call_restores.calls.get_mut(&id) {
        info.floor = holder;
    }
}

/// Private call `id` ended.
pub fn forget(inner: &mut Inner, id: &Uuid) {
    inner.call_restores.calls.remove(id);
    inner.call_restores.waiting.remove(id);
}

/// `issi` registered on `to` while its side of Brew-to-Brew individual calls
/// is held by `from`: those calls move to `to` if it announced the capability.
/// Returns the moved calls; the caller releases them on `from`
/// (`release_on_old`) and starts their restore window (`spawn_window`) once the
/// lock is released.
pub fn move_calls(inner: &mut Inner, issi: u32, from: ClientId, to: ClientId) -> Vec<Uuid> {
    if from == to || !crate::handover::announced(inner, to, CAPABILITY) {
        return Vec::new();
    }
    let joins = crate::handover::announced(inner, to, CAPABILITY_HANDOFF);
    let Inner { calls, call_restores, .. } = inner;
    let mut moved = Vec::new();
    for (id, call) in calls.iter_mut() {
        if call.kind != CallKind::Private || !call_restores.calls.contains_key(id) {
            continue;
        }
        let mut joins_from = None;
        if call.source_issi == issi && call.owner == from && !call.peers.contains(&to) {
            call.owner = to;
        } else if call.destination == issi && call.peers == HashSet::from([from]) && call.owner != to {
            call.peers = HashSet::from([to]);
        } else if joins && call.source_issi == issi && call.owner == from && call.peers == HashSet::from([to]) {
            // To the peer's Basestation, which joins the legs itself and then
            // ends the Brew call. `from` no longer speaks for the call.
            call.owner = to;
            joins_from = Some(from);
        } else if joins && call.destination == issi && call.peers == HashSet::from([from]) && call.owner == to {
            call.peers = HashSet::from([to]);
            joins_from = Some(from);
        } else {
            // Both parties on `to` (without `call-handoff`), or the radio's
            // side is not on `from`.
            continue;
        }
        call_restores.waiting.insert(*id, Waiting { issi, on: to, joins_from });
        moved.push(*id);
    }
    moved
}

/// Ends the leg of moved call `id` on the Basestation the radio left: a
/// network release with "SwMI requested disconnection", which a Basestation
/// does not answer with a release of its own.
pub fn release_on_old(inner: &Inner, from: ClientId, id: &Uuid) {
    if let Some(c) = inner.clients.get(&from) {
        let _ = c.tx.send(protocol::build_call_cause(protocol::CALL_RELEASE, id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION));
    }
}

/// Starts the restore window of moved call `id`.
pub fn spawn_window(state: &Arc<AppState>, id: Uuid) {
    let state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(RESTORE_WINDOW).await;
        window_closed(&state, id).await;
    });
}

/// The restore window of moved call `id` is over: if the radio did not restore
/// it, the call ends for the other party.
pub async fn window_closed(state: &Arc<AppState>, id: Uuid) {
    let waiting = state.inner.write().await.call_restores.waiting.remove(&id);
    let Some(w) = waiting else { return };
    info!(uuid = %id, issi = w.issi, "call restore: the radio did not restore the call, ending it");
    end_unrestored(state, w, id).await;
}

async fn end_unrestored(state: &Arc<AppState>, w: Waiting, id: Uuid) {
    let raw = protocol::build_call_cause(protocol::CALL_RELEASE, &id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED);
    if w.joins_from.is_none() {
        crate::router::end_call(state, w.on, id, raw).await;
        return;
    }
    // A call to be joined on `on`: both sides are there now, so the release
    // goes to `on` for the party that stayed.
    let tx = {
        let mut inner = state.inner.write().await;
        if inner.calls.remove(&id).is_none() {
            return;
        }
        forget(&mut inner, &id);
        inner.clients.get(&w.on).map(|c| c.tx.clone())
    };
    if let Some(tx) = tx {
        let _ = tx.send(raw);
    }
    state.monitor.call_ended(id).await;
}

/// Radio `issi` on Basestation `source` has given up the calls that moved
/// there with it (`why`, for the log): they end now instead of at the end of
/// their restore window.
pub async fn abandoned(state: &Arc<AppState>, source: ClientId, issi: u32, why: &str) {
    let ended: Vec<(Uuid, Waiting)> = {
        let mut inner = state.inner.write().await;
        let ids: Vec<Uuid> = inner
            .call_restores
            .waiting
            .iter()
            .filter(|(_, w)| w.issi == issi && w.on == source)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter().filter_map(|id| inner.call_restores.waiting.remove(&id).map(|w| (id, w))).collect()
    };
    for (id, w) in ended {
        info!(%source, issi, uuid = %id, why, "call restore: the radio gave up the call, ending it");
        end_unrestored(state, w, id).await;
    }
}

#[derive(Deserialize)]
struct NotRestored {
    issi: u32,
}

/// A `SERVICE_CALL_NOT_RESTORED` report from Basestation `source`: radio
/// `issi` registered there with a plain roaming location update, so it has
/// no call to restore (clause 16.4 a)).
pub async fn handle_not_restored(state: &Arc<AppState>, source: ClientId, json: &str) {
    let report: NotRestored = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed call not restored report");
            return;
        }
    };
    if !crate::handover::announced(&*state.inner.read().await, source, CAPABILITY) {
        warn!(%source, issi = report.issi, "call not restored report from a connection without call-restore ignored");
        return;
    }
    abandoned(state, source, report.issi, "registered by roaming, not service restoration roaming").await;
}

#[derive(Deserialize)]
struct Request {
    issi: u32,
    #[serde(default)]
    peer: u32,
}

/// The answer to a `SERVICE_CALL_RESTORE` request: the call, or the request
/// back without `uuid`.
fn answer(issi: u32, peer: u32, call: Option<(Uuid, CallInfo, u8)>) -> Vec<u8> {
    let json = match call {
        Some((id, info, priority)) => serde_json::json!({
            "issi": issi,
            "peer": peer,
            "uuid": id.to_string(),
            "duplex": info.duplex,
            "priority": priority,
            "floor": info.floor.unwrap_or(0),
        }),
        None => serde_json::json!({ "issi": issi, "peer": peer }),
    };
    protocol::build_service(protocol::SERVICE_CALL_RESTORE, &json.to_string())
}

/// A `SERVICE_CALL_RESTORE` request from Basestation `source`: radio `issi`
/// restores its call with `peer` there.
pub async fn handle_request(state: &Arc<AppState>, source: ClientId, json: &str) {
    let req: Request = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed call restore request");
            return;
        }
    };
    let mut inner = state.inner.write().await;
    if !crate::handover::announced(&inner, source, CAPABILITY) {
        warn!(%source, issi = req.issi, "call restore request from a connection without call-restore ignored");
        return;
    }
    // Registered here, but its calls are still held where it was (they move
    // at registration; this covers a registration seen before the calls).
    let mut moved = Vec::new();
    if inner.subscribers.get(&req.issi).is_some_and(|s| s.client_id == source) {
        let holders: HashSet<ClientId> = inner
            .calls
            .iter()
            .filter(|(id, c)| c.kind == CallKind::Private && inner.call_restores.calls.contains_key(*id))
            .filter_map(|(_, c)| {
                if c.source_issi == req.issi {
                    Some(c.owner)
                } else if c.destination == req.issi {
                    c.peers.iter().next().copied()
                } else {
                    None
                }
            })
            .filter(|holder| *holder != source)
            .collect();
        for from in holders {
            for id in move_calls(&mut inner, req.issi, from, source) {
                release_on_old(&inner, from, &id);
                moved.push(id);
            }
        }
    }
    let found = inner.calls.iter().find_map(|(id, c)| {
        if c.kind != CallKind::Private {
            return None;
        }
        let info = *inner.call_restores.calls.get(id)?;
        let peer = if c.source_issi == req.issi && c.owner == source {
            c.destination
        } else if c.destination == req.issi && c.peers.contains(&source) {
            c.source_issi
        } else {
            return None;
        };
        (req.peer == 0 || req.peer == peer).then_some((*id, info, c.priority, peer))
    });
    let reply = match found {
        Some((id, info, priority, peer)) => {
            inner.call_restores.waiting.remove(&id);
            info!(%source, issi = req.issi, peer, uuid = %id, duplex = info.duplex, "call restore: radio restores its call here");
            answer(req.issi, peer, Some((id, info, priority)))
        }
        None => {
            // A call between two radios of one Basestation: ask that one for it.
            if let Some(holder) = handoff_holder(&inner, source, req.issi, req.peer) {
                inner.call_restores.handoffs.insert((holder, req.issi), PendingHandoff { asker: source, peer: req.peer });
                if let Some(c) = inner.clients.get(&holder) {
                    let json = serde_json::json!({ "issi": req.issi, "peer": req.peer }).to_string();
                    let _ = c.tx.send(protocol::build_service(protocol::SERVICE_CALL_HANDOFF, &json));
                }
                info!(%source, %holder, issi = req.issi, peer = req.peer, "call restore: asking the peer's Basestation for its local call");
                drop(inner);
                spawn_handoff_timeout(state, holder, req.issi);
                for id in moved {
                    if state.inner.read().await.call_restores.waiting.contains_key(&id) {
                        spawn_window(state, id);
                    }
                }
                return;
            }
            info!(%source, issi = req.issi, peer = req.peer, "call restore: no call of this radio to restore");
            answer(req.issi, req.peer, None)
        }
    };
    if let Some(c) = inner.clients.get(&source) {
        let _ = c.tx.send(reply);
    }
    drop(inner);
    for id in moved {
        debug!(%source, issi = req.issi, uuid = %id, "call restore: call moved on request");
        // Answered above, unless it was not the call asked for.
        if state.inner.read().await.call_restores.waiting.contains_key(&id) {
            spawn_window(state, id);
        }
    }
}

/// The Basestation to ask for a local call of `issi` with `peer`, restored on
/// `source`: where `peer` is registered, if that is another Basestation that
/// announced `call-handoff`.
fn handoff_holder(inner: &Inner, source: ClientId, issi: u32, peer: u32) -> Option<ClientId> {
    if peer == 0 || peer == issi {
        return None;
    }
    let holder = inner.subscribers.get(&peer)?.client_id;
    (holder != source && crate::handover::announced(inner, holder, CAPABILITY_HANDOFF)).then_some(holder)
}

/// Ends a restore request whose Basestation did not answer the handoff in time.
fn spawn_handoff_timeout(state: &Arc<AppState>, holder: ClientId, issi: u32) {
    let state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(HANDOFF_TIMEOUT).await;
        handoff_timed_out(&state, holder, issi).await;
    });
}

/// The handoff asked of `holder` for `issi` was not answered in time: the
/// restore request gets no call.
pub async fn handoff_timed_out(state: &Arc<AppState>, holder: ClientId, issi: u32) {
    let mut inner = state.inner.write().await;
    let Some(pending) = inner.call_restores.handoffs.remove(&(holder, issi)) else { return };
    info!(%holder, issi, peer = pending.peer, "call restore: no handoff answer, no call");
    if let Some(c) = inner.clients.get(&pending.asker) {
        let _ = c.tx.send(answer(issi, pending.peer, None));
    }
}

#[derive(Deserialize)]
struct Handoff {
    issi: u32,
    peer: u32,
    uuid: Option<Uuid>,
    #[serde(default)]
    calling: u32,
    #[serde(default)]
    duplex: bool,
    #[serde(default)]
    priority: u8,
    #[serde(default)]
    floor: u32,
}

/// A `SERVICE_CALL_HANDOFF` answer from Basestation `source`: its local call
/// of `issi` with `peer`, now bridged to Brew under `uuid`, or none. The call
/// becomes a private call between `source` (the peer's side) and the
/// Basestation that asked, which gets it as the answer to its restore request.
pub async fn handle_handoff(state: &Arc<AppState>, source: ClientId, json: &str) {
    let h: Handoff = match serde_json::from_str(json) {
        Ok(h) => h,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed call handoff");
            return;
        }
    };
    let mut inner = state.inner.write().await;
    if !crate::handover::announced(&inner, source, CAPABILITY_HANDOFF) {
        warn!(%source, issi = h.issi, "call handoff from a connection without call-handoff ignored");
        return;
    }
    let pending = inner.call_restores.handoffs.get(&(source, h.issi)).copied().filter(|p| p.peer == h.peer);
    let Some(pending) = pending else {
        // Nobody waits for it (answered too late): end the leg the Basestation
        // just bridged, or its radio would hold a call nobody joins.
        if let (Some(id), Some(c)) = (h.uuid, inner.clients.get(&source)) {
            info!(%source, issi = h.issi, uuid = %id, "call handoff came too late, ending it");
            let _ = c.tx.send(protocol::build_call_cause(protocol::CALL_RELEASE, &id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED));
        }
        return;
    };
    inner.call_restores.handoffs.remove(&(source, h.issi));
    let call = match h.uuid {
        Some(id) if !inner.calls.contains_key(&id) && (h.calling == h.issi || h.calling == h.peer) => {
            // The radio that moved is on the asking Basestation, the peer here.
            let (owner, source_issi, destination, peer_client) = if h.calling == h.peer {
                (source, h.peer, h.issi, pending.asker)
            } else {
                (pending.asker, h.issi, h.peer, source)
            };
            inner.calls.insert(id, ActiveCall {
                kind: CallKind::Private,
                owner,
                source_issi,
                destination,
                priority: h.priority,
                peers: HashSet::from([peer_client]),
                started_at: std::time::Instant::now(),
                last_activity_ms: ActiveCall::new_activity(),
            });
            note_setup(&mut inner, id, h.duplex);
            let info = CallInfo { duplex: h.duplex, floor: (h.floor != 0).then_some(h.floor) };
            inner.call_restores.calls.insert(id, info);
            info!(%source, asker = %pending.asker, issi = h.issi, peer = h.peer, uuid = %id, duplex = h.duplex, "call handoff: a local call now runs between two Basestations");
            Some((id, info, h.priority, source_issi, destination))
        }
        Some(id) => {
            warn!(%source, issi = h.issi, uuid = %id, calling = h.calling, "call handoff with a known uuid or wrong parties ignored");
            if let Some(c) = inner.clients.get(&source) {
                let _ = c.tx.send(protocol::build_call_cause(protocol::CALL_RELEASE, &id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED));
            }
            None
        }
        None => {
            info!(%source, issi = h.issi, peer = h.peer, "call handoff: the Basestation has no such call");
            None
        }
    };
    let reply = answer(h.issi, h.peer, call.map(|(id, info, priority, ..)| (id, info, priority)));
    if let Some(c) = inner.clients.get(&pending.asker) {
        let _ = c.tx.send(reply);
    }
    drop(inner);
    if let Some((id, _, priority, source_issi, destination)) = call {
        state.monitor.call_started(id, "private", source_issi, destination, priority).await;
        crate::netlink::note_call(state, id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BrewMessage, CallPayload, ConnVersion, CALL_RELEASE, CALL_SIMPLEX_GRANTED, CALL_SIMPLEX_IDLE, SUB_REGISTER};
    use crate::state::{Client, ClientMode};
    use tokio::sync::mpsc;

    type Rx = mpsc::UnboundedReceiver<Vec<u8>>;

    async fn connect(state: &AppState, mode: ClientMode) -> (ClientId, Rx) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
        (id, rx)
    }

    /// A Basestation on `carrier`, with or without the `call-restore` capability.
    async fn bts(state: &Arc<AppState>, carrier: u16, capable: bool) -> (ClientId, Rx) {
        let (id, rx) = connect(state, ClientMode::Basestation).await;
        let caps: &[&str] = if capable { &["handover-v1", "ms-moved", CAPABILITY] } else { &["handover-v1", "ms-moved"] };
        let json = serde_json::json!({
            "mcc": 232, "mnc": 5,
            "cells": [{ "carrier": carrier, "carrier_ext": 270, "location_area": carrier }],
            "capabilities": caps,
        });
        packet(state, id, protocol::build_service(protocol::SERVICE_BTS_INFO, &json.to_string())).await;
        (id, rx)
    }

    /// A Basestation on `carrier` that also hands local calls over (`call-handoff`).
    async fn bts_handoff(state: &Arc<AppState>, carrier: u16) -> (ClientId, Rx) {
        let (id, rx) = connect(state, ClientMode::Basestation).await;
        let json = serde_json::json!({
            "mcc": 232, "mnc": 5,
            "cells": [{ "carrier": carrier, "carrier_ext": 270, "location_area": carrier }],
            "capabilities": ["handover-v1", "ms-moved", CAPABILITY, CAPABILITY_HANDOFF],
        });
        packet(state, id, protocol::build_service(protocol::SERVICE_BTS_INFO, &json.to_string())).await;
        (id, rx)
    }

    /// The `SERVICE_CALL_HANDOFF` requests among `packets`.
    fn handoff_requests(packets: &[Vec<u8>]) -> Vec<serde_json::Value> {
        packets
            .iter()
            .filter_map(|p| match protocol::parse_with_version(p, ConnVersion::V0) {
                Ok((BrewMessage::Service(s), _)) if s.service_type == protocol::SERVICE_CALL_HANDOFF => serde_json::from_str(&s.json_data).ok(),
                _ => None,
            })
            .collect()
    }

    /// A Basestation's answer to a handoff request: its local call, now bridged as `id`.
    fn handoff(issi: u32, peer: u32, call: Option<(&Uuid, u32, bool, u32)>) -> Vec<u8> {
        let json = match call {
            Some((id, calling, duplex, floor)) => serde_json::json!({
                "issi": issi, "peer": peer, "uuid": id.to_string(), "calling": calling,
                "duplex": duplex, "priority": 0, "floor": floor,
            }),
            None => serde_json::json!({ "issi": issi, "peer": peer }),
        };
        protocol::build_service(protocol::SERVICE_CALL_HANDOFF, &json.to_string())
    }

    async fn packet(state: &Arc<AppState>, from: ClientId, raw: Vec<u8>) {
        crate::router::handle_packet(state.clone(), from, raw).await;
    }

    async fn register(state: &Arc<AppState>, on: ClientId, issi: u32) {
        packet(state, on, protocol::build_subscriber_message(SUB_REGISTER, issi, &[])).await;
    }

    fn drain(rx: &mut Rx) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    /// The call releases among `packets`, as (uuid, cause).
    fn releases(packets: &[Vec<u8>]) -> Vec<(Uuid, u8)> {
        packets
            .iter()
            .filter_map(|p| match protocol::parse_with_version(p, ConnVersion::V0) {
                Ok((BrewMessage::CallControl(cc), _)) if cc.call_state == CALL_RELEASE => match cc.payload {
                    CallPayload::Cause(c) => Some((cc.identifier, c)),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The `SERVICE_CALL_RESTORE` answers among `packets`.
    fn answers(packets: &[Vec<u8>]) -> Vec<serde_json::Value> {
        packets
            .iter()
            .filter_map(|p| match protocol::parse_with_version(p, ConnVersion::V0) {
                Ok((BrewMessage::Service(s), _)) if s.service_type == protocol::SERVICE_CALL_RESTORE => serde_json::from_str(&s.json_data).ok(),
                _ => None,
            })
            .collect()
    }

    fn not_restored(issi: u32) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_CALL_NOT_RESTORED, &serde_json::json!({ "issi": issi }).to_string())
    }

    /// The private call setups among `packets`.
    fn setups(packets: &[Vec<u8>]) -> Vec<Uuid> {
        packets
            .iter()
            .filter_map(|p| match protocol::parse_with_version(p, ConnVersion::V0) {
                Ok((BrewMessage::CallControl(cc), _)) if cc.call_state == protocol::CALL_SETUP_REQUEST => Some(cc.identifier),
                _ => None,
            })
            .collect()
    }

    fn restore_request(issi: u32, peer: u32) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_CALL_RESTORE, &serde_json::json!({ "issi": issi, "peer": peer }).to_string())
    }

    /// A private call setup from `source` to `destination`, duplex or simplex.
    fn setup(id: &Uuid, source: u32, destination: u32, duplex: bool) -> Vec<u8> {
        let mut wire = protocol::build_circular_call_setup(id, source, destination, 0);
        wire[2 + 16 + 43] = duplex as u8;
        wire
    }

    fn voice(id: &Uuid) -> Vec<u8> {
        protocol::build_traffic_frame(id, &[1; protocol::ACELP_CODED_FRAME_BYTES], &[2; protocol::ACELP_CODED_FRAME_BYTES])
    }

    const RADIO: u32 = 2320972;
    const APP: u32 = 2320997;

    /// The field case of 2026-10-30: the app calls the radio on BS1, duplex,
    /// and the radio moves to BS2.
    async fn field_case(bs2_capable: bool) -> (Arc<AppState>, Uuid, (ClientId, Rx), (ClientId, Rx), (ClientId, Rx)) {
        let state = AppState::for_test();
        let (bs1, mut bs1_rx) = bts(&state, 1528, true).await;
        let (bs2, bs2_rx) = bts(&state, 1530, bs2_capable).await;
        let (app, mut app_rx) = connect(&state, ClientMode::Terminal).await;
        register(&state, bs1, RADIO).await;
        register(&state, app, APP).await;
        let id = Uuid::new_v4();
        packet(&state, app, setup(&id, APP, RADIO, true)).await;
        packet(&state, bs1, protocol::build_call_control_empty(protocol::CALL_SETUP_ACCEPT, &id)).await;
        drain(&mut bs1_rx);
        drain(&mut app_rx);
        register(&state, bs2, RADIO).await;
        (state, id, (bs1, bs1_rx), (bs2, bs2_rx), (app, app_rx))
    }

    #[tokio::test]
    async fn the_call_moves_with_the_radio_and_the_old_station_lets_go_silently() {
        let (state, id, (bs1, mut bs1_rx), (bs2, mut bs2_rx), (app, mut app_rx)) = field_case(true).await;
        // BS1 ends its leg with "SwMI requested disconnection", the app hears nothing of it.
        assert_eq!(releases(&drain(&mut bs1_rx)), vec![(id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)]);
        assert!(drain(&mut app_rx).is_empty());
        assert_eq!(state.inner.read().await.calls[&id].peers, HashSet::from([bs2]));

        // BS2 asks once the radio sends U-CALL RESTORE.
        packet(&state, bs2, restore_request(RADIO, APP)).await;
        assert_eq!(
            answers(&drain(&mut bs2_rx)),
            vec![serde_json::json!({ "issi": RADIO, "peer": APP, "uuid": id.to_string(), "duplex": true, "priority": 0, "floor": 0 })]
        );

        // Voice runs between the app and BS2 now.
        packet(&state, app, voice(&id)).await;
        assert_eq!(drain(&mut bs2_rx), vec![voice(&id)]);
        assert!(drain(&mut bs1_rx).is_empty());
        packet(&state, bs2, voice(&id)).await;
        assert_eq!(drain(&mut app_rx), vec![voice(&id)]);

        // BS1's own release (its uplink timer) comes too late and changes nothing.
        packet(&state, bs1, protocol::build_call_cause(CALL_RELEASE, &id, 13)).await;
        packet(&state, bs1, protocol::build_call_control_empty(CALL_SIMPLEX_IDLE, &id)).await;
        assert!(drain(&mut app_rx).is_empty());
        assert!(state.inner.read().await.calls.contains_key(&id));

        // Restored: the window closing does nothing.
        window_closed(&state, id).await;
        assert!(drain(&mut app_rx).is_empty());

        // The call ends normally from BS2.
        packet(&state, bs2, protocol::build_call_cause(CALL_RELEASE, &id, 1)).await;
        assert_eq!(releases(&drain(&mut app_rx)), vec![(id, 1)]);
        assert!(state.inner.read().await.call_restores.calls.is_empty());
    }

    #[tokio::test]
    async fn a_call_the_radio_does_not_restore_ends_for_the_other_party() {
        let (state, id, (_bs1, _), (bs2, mut bs2_rx), (_app, mut app_rx)) = field_case(true).await;
        window_closed(&state, id).await;
        assert_eq!(releases(&drain(&mut app_rx)), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)]);
        assert!(!state.inner.read().await.calls.contains_key(&id));
        // Asked too late: no call.
        packet(&state, bs2, restore_request(RADIO, APP)).await;
        assert_eq!(answers(&drain(&mut bs2_rx)), vec![serde_json::json!({ "issi": RADIO, "peer": APP })]);
    }

    #[tokio::test]
    async fn a_plain_roaming_registration_ends_the_moved_call_at_once() {
        let (state, id, (bs1, _), (bs2, mut bs2_rx), (_app, mut app_rx)) = field_case(true).await;
        // Reports from the station the radio left, or for another radio, change nothing.
        packet(&state, bs1, not_restored(RADIO)).await;
        packet(&state, bs2, not_restored(APP)).await;
        assert!(drain(&mut app_rx).is_empty());
        assert!(state.inner.read().await.calls.contains_key(&id));
        // BS2: the radio registered by roaming, not service restoration roaming (16.4 a)).
        packet(&state, bs2, not_restored(RADIO)).await;
        assert_eq!(releases(&drain(&mut app_rx)), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)]);
        assert!(!state.inner.read().await.calls.contains_key(&id));
        // The window closing later does nothing more.
        window_closed(&state, id).await;
        assert!(drain(&mut app_rx).is_empty());
        assert!(drain(&mut bs2_rx).iter().all(|p| releases(std::slice::from_ref(p)).is_empty()));
    }

    #[tokio::test]
    async fn a_new_call_from_the_radio_ends_the_moved_call_first() {
        let (state, id, _, (bs2, _), (_app, mut app_rx)) = field_case(true).await;
        let new = Uuid::new_v4();
        packet(&state, bs2, setup(&new, RADIO, APP, true)).await;
        let got = drain(&mut app_rx);
        // The release of the old call reaches the app before the new setup.
        assert_eq!(releases(&got), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)]);
        assert_eq!(setups(&got), vec![new]);
        let release_at = got.iter().position(|p| !releases(std::slice::from_ref(p)).is_empty()).unwrap();
        let setup_at = got.iter().position(|p| !setups(std::slice::from_ref(p)).is_empty()).unwrap();
        assert!(release_at < setup_at);
        let inner = state.inner.read().await;
        assert!(!inner.calls.contains_key(&id));
        assert!(inner.calls.contains_key(&new));
    }

    #[tokio::test]
    async fn a_station_without_the_capability_keeps_the_old_behaviour() {
        let (state, id, (bs1, mut bs1_rx), (bs2, mut bs2_rx), (app, mut app_rx)) = field_case(false).await;
        assert!(releases(&drain(&mut bs1_rx)).is_empty(), "BS1 keeps its leg");
        assert_eq!(state.inner.read().await.calls[&id].peers, HashSet::from([bs1]));
        // BS2 cannot ask; a request from it is ignored.
        packet(&state, bs2, restore_request(RADIO, APP)).await;
        assert!(answers(&drain(&mut bs2_rx)).is_empty());
        // BS1's release ends the call as before.
        packet(&state, bs1, protocol::build_call_cause(CALL_RELEASE, &id, 13)).await;
        assert_eq!(releases(&drain(&mut app_rx)), vec![(id, 13)]);
        let _ = app;
    }

    #[tokio::test]
    async fn the_answer_names_who_holds_a_simplex_floor() {
        let state = AppState::for_test();
        let (bs1, _bs1_rx) = bts(&state, 1528, true).await;
        let (bs2, mut bs2_rx) = bts(&state, 1530, true).await;
        let (app, _app_rx) = connect(&state, ClientMode::Terminal).await;
        register(&state, bs1, RADIO).await;
        register(&state, app, APP).await;
        // The radio calls the app this time, simplex.
        let id = Uuid::new_v4();
        packet(&state, bs1, setup(&id, RADIO, APP, false)).await;
        packet(&state, app, protocol::build_call_control_empty(CALL_SIMPLEX_GRANTED, &id)).await;
        register(&state, bs2, RADIO).await;
        assert_eq!(state.inner.read().await.calls[&id].owner, bs2);
        packet(&state, bs2, restore_request(RADIO, 0)).await;
        let answer = answers(&drain(&mut bs2_rx));
        assert_eq!((answer[0]["duplex"].clone(), answer[0]["floor"].clone(), answer[0]["peer"].clone()), (false.into(), APP.into(), APP.into()));

        packet(&state, app, protocol::build_call_control_empty(CALL_SIMPLEX_IDLE, &id)).await;
        packet(&state, bs2, restore_request(RADIO, APP)).await;
        assert_eq!(answers(&drain(&mut bs2_rx))[0]["floor"], 0);
    }

    #[tokio::test]
    async fn a_request_for_another_peer_or_an_unknown_radio_gets_no_call() {
        let (state, _id, _, (bs2, mut bs2_rx), _) = field_case(true).await;
        packet(&state, bs2, restore_request(RADIO, 1234)).await;
        packet(&state, bs2, restore_request(4321, APP)).await;
        assert_eq!(
            answers(&drain(&mut bs2_rx)),
            vec![serde_json::json!({ "issi": RADIO, "peer": 1234 }), serde_json::json!({ "issi": 4321, "peer": APP })]
        );
    }

    /// Internal simulation (project rule): 10 Basestations, 50 radios in 25
    /// individual calls. One party of every call changes station; most restore
    /// on the new one, every fifth does not.
    #[tokio::test]
    async fn sim_10_stations_50_radios_calls_follow_their_radios() {
        let state = AppState::for_test();
        let mut stations = Vec::new();
        for n in 0..10u16 {
            stations.push(bts(&state, 1500 + n, true).await);
        }
        let ids: Vec<ClientId> = stations.iter().map(|(id, _)| *id).collect();
        let mut calls = Vec::new();
        for k in 0..25u32 {
            let (a, b) = (3_000_000 + 2 * k, 3_000_001 + 2 * k);
            let (sa, sb) = (ids[k as usize % 10], ids[(k as usize + 3) % 10]);
            register(&state, sa, a).await;
            register(&state, sb, b).await;
            let id = Uuid::new_v4();
            packet(&state, sa, setup(&id, a, b, k % 2 == 0)).await;
            calls.push((id, a, b, sa, sb));
        }
        for (_, rx) in stations.iter_mut() {
            drain(rx);
        }
        for (k, (id, a, b, sa, sb)) in calls.iter().enumerate() {
            // Caller `a` moves on to the next station that is not `sb`.
            let mut to = ids[(k + 1) % 10];
            if to == *sb {
                to = ids[(k + 2) % 10];
            }
            register(&state, to, *a).await;
            let old = stations.iter_mut().find(|(i, _)| i == sa).unwrap();
            assert_eq!(releases(&drain(&mut old.1)), vec![(*id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)], "call {k}");
            let restores = k % 5 != 0;
            if restores {
                packet(&state, to, restore_request(*a, *b)).await;
            } else if k % 10 == 0 {
                // Gives the call up with a plain roaming registration: ends at once.
                packet(&state, to, not_restored(*a)).await;
                assert!(!state.inner.read().await.calls.contains_key(id), "call {k}");
            }
            let new = stations.iter_mut().find(|(i, _)| *i == to).unwrap();
            assert_eq!(answers(&drain(&mut new.1)).len(), restores as usize, "call {k}");
            window_closed(&state, *id).await;
            let other = stations.iter_mut().find(|(i, _)| i == sb).unwrap();
            let other_got = releases(&drain(&mut other.1));
            if restores {
                assert!(other_got.is_empty(), "call {k}: the other party keeps the call");
                packet(&state, *sb, voice(id)).await;
                let new = stations.iter_mut().find(|(i, _)| *i == to).unwrap();
                assert_eq!(drain(&mut new.1), vec![voice(id)], "call {k}: voice reaches the new station");
            } else {
                assert_eq!(other_got, vec![(*id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)], "call {k}");
            }
        }
        let inner = state.inner.read().await;
        assert_eq!(inner.calls.len(), 20);
        assert!(inner.call_restores.waiting.is_empty());
    }

    const MTH: u32 = 2320980;

    /// Both radios in a local call on BS1; the radio moves to BS2.
    async fn local_case() -> (Arc<AppState>, (ClientId, Rx), (ClientId, Rx)) {
        let state = AppState::for_test();
        let (bs1, bs1_rx) = bts_handoff(&state, 1528).await;
        let (bs2, bs2_rx) = bts_handoff(&state, 1530).await;
        register(&state, bs1, RADIO).await;
        register(&state, bs1, MTH).await;
        register(&state, bs2, RADIO).await;
        (state, (bs1, bs1_rx), (bs2, bs2_rx))
    }

    #[tokio::test]
    async fn a_local_call_is_handed_over_when_one_radio_changes_station() {
        let (state, (bs1, mut bs1_rx), (bs2, mut bs2_rx)) = local_case().await;
        drain(&mut bs1_rx);
        drain(&mut bs2_rx);
        // The server knows no call: it asks the peer's station for its local one.
        packet(&state, bs2, restore_request(RADIO, MTH)).await;
        assert!(answers(&drain(&mut bs2_rx)).is_empty(), "no answer before the handoff");
        assert_eq!(handoff_requests(&drain(&mut bs1_rx)), vec![serde_json::json!({ "issi": RADIO, "peer": MTH })]);

        // BS1 bridges its call (MTH called the radio, duplex).
        let id = Uuid::new_v4();
        packet(&state, bs1, handoff(RADIO, MTH, Some((&id, MTH, true, 0)))).await;
        assert_eq!(
            answers(&drain(&mut bs2_rx)),
            vec![serde_json::json!({ "issi": RADIO, "peer": MTH, "uuid": id.to_string(), "duplex": true, "priority": 0, "floor": 0 })]
        );
        {
            let inner = state.inner.read().await;
            let call = &inner.calls[&id];
            assert_eq!((call.owner, call.source_issi, call.destination), (bs1, MTH, RADIO));
            assert_eq!(call.peers, HashSet::from([bs2]));
            assert!(inner.call_restores.handoffs.is_empty());
        }
        // Voice both ways.
        packet(&state, bs1, voice(&id)).await;
        assert_eq!(drain(&mut bs2_rx), vec![voice(&id)]);
        packet(&state, bs2, voice(&id)).await;
        assert_eq!(drain(&mut bs1_rx), vec![voice(&id)]);
        // A timeout after the answer does nothing.
        handoff_timed_out(&state, bs1, RADIO).await;
        assert!(drain(&mut bs2_rx).is_empty());
        // Ends like any call between two stations.
        packet(&state, bs2, protocol::build_call_cause(CALL_RELEASE, &id, 1)).await;
        assert_eq!(releases(&drain(&mut bs1_rx)), vec![(id, 1)]);
        assert!(state.inner.read().await.calls.is_empty());
    }

    #[tokio::test]
    async fn a_handed_over_simplex_call_keeps_the_caller_and_the_floor() {
        let (state, (bs1, mut bs1_rx), (bs2, mut bs2_rx)) = local_case().await;
        packet(&state, bs2, restore_request(RADIO, MTH)).await;
        drain(&mut bs1_rx);
        // The radio that moved called, MTH talks.
        let id = Uuid::new_v4();
        packet(&state, bs1, handoff(RADIO, MTH, Some((&id, RADIO, false, MTH)))).await;
        let answer = answers(&drain(&mut bs2_rx));
        assert_eq!((answer[0]["duplex"].clone(), answer[0]["floor"].clone()), (false.into(), MTH.into()));
        let inner = state.inner.read().await;
        let call = &inner.calls[&id];
        assert_eq!((call.owner, call.source_issi, call.destination), (bs2, RADIO, MTH));
        assert_eq!(call.peers, HashSet::from([bs1]));
    }

    #[tokio::test]
    async fn an_unanswered_handoff_gets_no_call_and_a_late_one_is_ended() {
        let (state, (bs1, mut bs1_rx), (bs2, mut bs2_rx)) = local_case().await;
        packet(&state, bs2, restore_request(RADIO, MTH)).await;
        drain(&mut bs1_rx);
        handoff_timed_out(&state, bs1, RADIO).await;
        assert_eq!(answers(&drain(&mut bs2_rx)), vec![serde_json::json!({ "issi": RADIO, "peer": MTH })]);
        // BS1 answers too late: its bridged leg is ended, BS2 hears nothing more.
        let id = Uuid::new_v4();
        packet(&state, bs1, handoff(RADIO, MTH, Some((&id, MTH, true, 0)))).await;
        assert_eq!(releases(&drain(&mut bs1_rx)), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)]);
        assert!(drain(&mut bs2_rx).is_empty());
        assert!(state.inner.read().await.calls.is_empty());
    }

    #[tokio::test]
    async fn a_station_without_the_call_or_without_call_handoff_answers_no_call() {
        let (state, (bs1, mut bs1_rx), (bs2, mut bs2_rx)) = local_case().await;
        packet(&state, bs2, restore_request(RADIO, MTH)).await;
        drain(&mut bs1_rx);
        packet(&state, bs1, handoff(RADIO, MTH, None)).await;
        assert_eq!(answers(&drain(&mut bs2_rx)), vec![serde_json::json!({ "issi": RADIO, "peer": MTH })]);
        // A handoff nobody asked for (or from a station not asked) changes nothing.
        let id = Uuid::new_v4();
        packet(&state, bs2, handoff(MTH, RADIO, Some((&id, MTH, true, 0)))).await;
        assert!(answers(&drain(&mut bs1_rx)).is_empty());
        assert!(!state.inner.read().await.calls.contains_key(&id));

        // Peer on a station without call-handoff: answered at once, nobody asked.
        let state = AppState::for_test();
        let (old, mut old_rx) = bts(&state, 1528, true).await;
        let (new, mut new_rx) = bts_handoff(&state, 1530).await;
        register(&state, old, RADIO).await;
        register(&state, old, MTH).await;
        register(&state, new, RADIO).await;
        drain(&mut old_rx);
        packet(&state, new, restore_request(RADIO, MTH)).await;
        assert_eq!(answers(&drain(&mut new_rx)), vec![serde_json::json!({ "issi": RADIO, "peer": MTH })]);
        assert!(handoff_requests(&drain(&mut old_rx)).is_empty());
    }

    /// The radio on BS1 and MTH on BS2 in a call over Brew; the radio comes to BS2.
    async fn join_case(duplex: bool) -> (Arc<AppState>, Uuid, (ClientId, Rx), (ClientId, Rx)) {
        let state = AppState::for_test();
        let (bs1, mut bs1_rx) = bts_handoff(&state, 1528).await;
        let (bs2, mut bs2_rx) = bts_handoff(&state, 1530).await;
        register(&state, bs1, RADIO).await;
        register(&state, bs2, MTH).await;
        let id = Uuid::new_v4();
        packet(&state, bs1, setup(&id, RADIO, MTH, duplex)).await;
        packet(&state, bs2, protocol::build_call_control_empty(protocol::CALL_SETUP_ACCEPT, &id)).await;
        drain(&mut bs1_rx);
        drain(&mut bs2_rx);
        register(&state, bs2, RADIO).await;
        (state, id, (bs1, bs1_rx), (bs2, bs2_rx))
    }

    #[tokio::test]
    async fn a_radio_coming_to_its_peers_station_is_joined_there() {
        for duplex in [true, false] {
            let (state, id, (bs1, mut bs1_rx), (bs2, mut bs2_rx)) = join_case(duplex).await;
            // BS1 lets its leg go; BS2 hears nothing: it joins the legs itself.
            assert_eq!(releases(&drain(&mut bs1_rx)), vec![(id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)]);
            assert!(drain(&mut bs2_rx).is_empty());
            // BS1's own late release does not reach BS2.
            packet(&state, bs1, protocol::build_call_cause(CALL_RELEASE, &id, 13)).await;
            assert!(drain(&mut bs2_rx).is_empty());
            assert!(state.inner.read().await.calls.contains_key(&id));
            // BS2 joined: it ends the Brew call, nobody else gets it.
            packet(&state, bs2, protocol::build_call_cause(CALL_RELEASE, &id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)).await;
            assert!(drain(&mut bs1_rx).is_empty() && drain(&mut bs2_rx).is_empty());
            let inner = state.inner.read().await;
            assert!(inner.calls.is_empty() && inner.call_restores.waiting.is_empty() && inner.call_restores.calls.is_empty());
            drop(inner);
            window_closed(&state, id).await;
            assert!(drain(&mut bs2_rx).is_empty());
        }
    }

    #[tokio::test]
    async fn a_joined_call_the_radio_does_not_restore_ends_for_the_peer() {
        let (state, id, (_bs1, mut bs1_rx), (bs2, mut bs2_rx)) = join_case(true).await;
        drain(&mut bs1_rx);
        window_closed(&state, id).await;
        assert_eq!(releases(&drain(&mut bs2_rx)), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)]);
        assert!(drain(&mut bs1_rx).is_empty());
        assert!(state.inner.read().await.calls.is_empty());
        // The same through a plain roaming registration.
        let (state, id, _, (bs2b, mut rx)) = join_case(false).await;
        packet(&state, bs2b, not_restored(RADIO)).await;
        assert_eq!(releases(&drain(&mut rx)), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)]);
        let _ = bs2;
    }

    #[tokio::test]
    async fn without_call_handoff_a_radio_coming_to_its_peer_keeps_the_old_behaviour() {
        let state = AppState::for_test();
        let (bs1, mut bs1_rx) = bts(&state, 1528, true).await;
        let (bs2, mut bs2_rx) = bts(&state, 1530, true).await;
        register(&state, bs1, RADIO).await;
        register(&state, bs2, MTH).await;
        let id = Uuid::new_v4();
        packet(&state, bs1, setup(&id, RADIO, MTH, true)).await;
        drain(&mut bs1_rx);
        drain(&mut bs2_rx);
        register(&state, bs2, RADIO).await;
        assert!(releases(&drain(&mut bs1_rx)).is_empty());
        assert!(state.inner.read().await.call_restores.waiting.is_empty());
    }

    /// Internal simulation (project rule): 10 Basestations, 52 radios in 26
    /// individual calls between two radios. 13 calls start on one station and
    /// one radio moves away (handed over); 13 run between two stations and one
    /// radio comes to its peer's station (joined there). Every fourth radio
    /// does not restore.
    #[tokio::test]
    async fn sim_10_stations_52_radios_radio_to_radio_calls_survive_station_changes() {
        let state = AppState::for_test();
        let mut stations = Vec::new();
        for n in 0..10u16 {
            stations.push(bts_handoff(&state, 1500 + n).await);
        }
        let ids: Vec<ClientId> = stations.iter().map(|(id, _)| *id).collect();
        let rx_of = |stations: &mut Vec<(ClientId, Rx)>, id: ClientId| drain(&mut stations.iter_mut().find(|(i, _)| *i == id).unwrap().1);
        let mut kept = 0;
        for k in 0..26u32 {
            let (a, b) = (4_000_000 + 2 * k, 4_000_001 + 2 * k);
            let duplex = k % 2 == 0;
            let restores = k % 4 != 3;
            if k < 13 {
                // Local call on `home`; `a` moves to `to`.
                let (home, to) = (ids[k as usize % 10], ids[(k as usize + 1) % 10]);
                register(&state, home, a).await;
                register(&state, home, b).await;
                register(&state, to, a).await;
                for (_, rx) in stations.iter_mut() {
                    drain(rx);
                }
                if !restores {
                    continue; // nothing asked, nothing happens
                }
                packet(&state, to, restore_request(a, b)).await;
                assert_eq!(handoff_requests(&rx_of(&mut stations, home)).len(), 1, "call {k}");
                let id = Uuid::new_v4();
                packet(&state, home, handoff(a, b, Some((&id, b, duplex, 0)))).await;
                let got = answers(&rx_of(&mut stations, to));
                assert_eq!(got[0]["uuid"], id.to_string(), "call {k}");
                packet(&state, to, voice(&id)).await;
                assert_eq!(rx_of(&mut stations, home), vec![voice(&id)], "call {k}");
                kept += 1;
            } else {
                // Over Brew between `sa` and `sb`; `a` comes to `sb`.
                let (sa, sb) = (ids[k as usize % 10], ids[(k as usize + 4) % 10]);
                register(&state, sa, a).await;
                register(&state, sb, b).await;
                let id = Uuid::new_v4();
                packet(&state, sa, setup(&id, a, b, duplex)).await;
                packet(&state, sb, protocol::build_call_control_empty(protocol::CALL_SETUP_ACCEPT, &id)).await;
                for (_, rx) in stations.iter_mut() {
                    drain(rx);
                }
                register(&state, sb, a).await;
                assert_eq!(releases(&rx_of(&mut stations, sa)), vec![(id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)], "call {k}");
                if restores {
                    packet(&state, sb, protocol::build_call_cause(CALL_RELEASE, &id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)).await;
                    assert!(rx_of(&mut stations, sb).is_empty(), "call {k}");
                } else {
                    window_closed(&state, id).await;
                    assert_eq!(releases(&rx_of(&mut stations, sb)), vec![(id, protocol::CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED)], "call {k}");
                }
                assert!(!state.inner.read().await.calls.contains_key(&id), "call {k}");
            }
        }
        let inner = state.inner.read().await;
        assert_eq!(kept, 10);
        assert_eq!(inner.calls.len(), 10, "the handed-over calls run between two stations");
        assert!(inner.call_restores.waiting.is_empty() && inner.call_restores.handoffs.is_empty());
    }
}
