use crate::{
    protocol::{
        self, BrewMessage, CallPayload, SubscriberMessage, CALL_ALERT, CALL_CONNECT_CONFIRM,
        CALL_CONNECT_REQUEST, CALL_GROUP_IDLE, CALL_GROUP_TX, CALL_RELEASE, CALL_SETUP_ACCEPT,
        CALL_SETUP_REJECT, CALL_SETUP_REQUEST, CALL_SHORT_TRANSFER, CALL_SIMPLEX_GRANTED,
        CALL_SIMPLEX_IDLE, FRAME_SDS_REPORT, FRAME_SDS_TRANSFER, FRAME_TRAFFIC_CHANNEL,
        SUB_AFFILIATE, SUB_DEAFFILIATE, SUB_DEREGISTER, SUB_REGISTER, SUB_REREGISTER,
    },
    state::{ActiveCall, AppState, CallKind, ClientId, SdsRoute, Subscriber},
};
use std::{collections::{HashMap, HashSet}, sync::Arc, time::Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Wire shape of a `SERVICE_RSSI` message's JSON payload.
#[derive(serde::Deserialize)]
struct RssiReport {
    issi: u32,
    rssi_dbfs: f32,
}

pub async fn handle_packet(state: Arc<AppState>, source: ClientId, raw: Vec<u8>) {
    state.purge_ephemeral().await;
    // TEMPORARY (position debugging): log inbound Brew packets, but skip the
    // high-volume voice traffic frames (class 0xf2 / type 0x00) unless they
    // actually contain the LIP protocol id (0x0A). This keeps a PTT from burying
    // the SDS/position beacons we're looking for.
    {
        let class = raw.first().copied().unwrap_or(0);
        let subtype = raw.get(1).copied().unwrap_or(0);
        let is_voice = class == crate::protocol::CLASS_FRAME && subtype == crate::protocol::FRAME_TRAFFIC_CHANNEL;
        // Only flag a genuine LIP candidate: an SDS-bearing frame/message whose
        // SDS payload begins with the 0x0A LIP protocol id. Scanning voice
        // traffic for a stray 0x0A byte gave false positives (ACELP payloads are
        // high-entropy), so restrict to SDS frame types and check the SDS data
        // start, not "contains 0x0A anywhere".
        let is_sds = (class == crate::protocol::CLASS_FRAME && (subtype == crate::protocol::FRAME_SDS_TRANSFER || subtype == crate::protocol::FRAME_SDS_REPORT))
            || (class == crate::protocol::CLASS_CALL_CONTROL && subtype == crate::protocol::CALL_SHORT_TRANSFER);
        let has_lip = is_sds && raw.get(20..).map(|b| b.contains(&0x0A)).unwrap_or(false);
        if !is_voice || has_lip {
            info!(%source, class = format!("0x{class:02x}"), subtype = format!("0x{subtype:02x}"), bytes = raw.len(), lip = has_lip, hex = %hex_dump(&raw), "RX Brew packet");
        }
    }
    let version = state.client_version(source).await;
    let (parsed, detected) = match protocol::parse_with_version(&raw, version) {
        Ok(v) => v,
        Err(e) => {
            warn!(%source, error = %e, bytes = raw.len(), "dropping malformed Brew packet");
            return;
        }
    };
    // Lazily resolve the connection version from message content, mirroring the
    // client side. Log the promotion once so operators can see when a peer is
    // confirmed to speak v1 despite sending no X-Brew-Version handshake header.
    if detected.as_u8() > version.as_u8() && state.promote_client_version(source, detected).await {
        info!(%source, from = version.as_u8(), to = detected.as_u8(), "Brew connection version promoted from message content");
    }

    match parsed {
        BrewMessage::Subscriber(msg) => handle_subscriber(&state, source, msg).await,
        BrewMessage::CallControl(cc) if cc.call_state == CALL_GROUP_TX => {
            handle_group_tx(&state, source, cc.identifier, cc.payload, raw).await;
        }
        BrewMessage::CallControl(cc) if cc.call_state == CALL_SHORT_TRANSFER => {
            handle_sds_header(&state, source, cc.identifier, cc.payload, raw).await;
        }
        BrewMessage::Frame(frame) if frame.frame_type == FRAME_SDS_TRANSFER => {
            handle_sds_transfer(&state, source, frame.identifier, raw).await;
        }
        BrewMessage::Frame(frame) if frame.frame_type == FRAME_SDS_REPORT => {
            handle_sds_report(&state, source, frame.identifier, raw).await;
        }
        BrewMessage::CallControl(cc) if cc.call_state == CALL_SETUP_REQUEST => {
            handle_private_setup(&state, source, cc.identifier, cc.payload, raw).await;
        }
        BrewMessage::CallControl(cc)
            if matches!(cc.call_state, CALL_SETUP_ACCEPT | CALL_SETUP_REJECT | CALL_ALERT |
                CALL_CONNECT_REQUEST | CALL_CONNECT_CONFIRM | CALL_SIMPLEX_GRANTED | CALL_SIMPLEX_IDLE) =>
        {
            route_private_control(&state, source, cc.identifier, raw).await;
        }
        BrewMessage::CallControl(cc)
            if cc.call_state == CALL_GROUP_IDLE || cc.call_state == CALL_RELEASE =>
        {
            end_call(&state, source, cc.identifier, raw).await;
        }
        BrewMessage::Frame(frame) if frame.frame_type == FRAME_TRAFFIC_CHANNEL => {
            route_call_frame(&state, source, frame.identifier, raw).await;
        }
        BrewMessage::Frame(frame) if frame.frame_type == protocol::FRAME_DTMF => {
            // Same header shape and same participant routing as a voice
            // frame (see route_call_frame); not in this server's original
            // protocol coverage, but sent by at least one real client
            // (nexus-bs). Forwarding it exactly like a traffic frame reaches
            // every other Brew-side participant, and reaches the SIP bridge's
            // virtual client the same way audio does, where the transcoder
            // turns it into an RFC 2833 telephone-event RTP packet (see
            // transcode::task) instead of silently dropping it.
            route_dtmf_frame(&state, source, frame.identifier, raw).await;
        }
        BrewMessage::Service(svc) if svc.service_type == protocol::SERVICE_RSSI => {
            match serde_json::from_str::<RssiReport>(&svc.json_data) {
                Ok(r) => {
                    state.telemetry.write().await.record_brew_rssi(r.issi, r.rssi_dbfs);
                }
                Err(e) => warn!(%source, error = %e, json = %svc.json_data, "malformed RSSI service message"),
            }
        }
        BrewMessage::Service(svc) => {
            debug!(%source, service_type = svc.service_type, json = %svc.json_data, "service message ignored");
        }
        BrewMessage::Error(err) => {
            warn!(%source, error_type = err.error_type, bytes = err.data.len(), "client sent Brew error");
        }
        other => debug!(%source, ?other, "Brew message not handled"),
    }
}

async fn handle_group_tx(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, payload: CallPayload, raw: Vec<u8>) {
    let CallPayload::GroupTransmission(gt) = payload else { return };
    let mut inner = state.inner.write().await;
    let mut preempted = None;

    if !state.config.allow_multiple_calls_per_group {
        if let Some(existing_id) = inner.group_floor.get(&gt.destination).copied() {
            if existing_id != id {
                if let Some(existing) = inner.calls.get(&existing_id).cloned() {
                    let wins = if state.config.higher_priority_number_wins {
                        gt.priority > existing.priority
                    } else {
                        gt.priority < existing.priority
                    };
                    if !wins {
                        warn!(%source, gssi=gt.destination, priority=gt.priority, active_priority=existing.priority,
                            rejected_uuid=%id, "group floor occupied by equal/higher priority call");
                        return;
                    }

                    let release = protocol::build_call_cause(CALL_GROUP_IDLE, &existing_id, state.config.preempt_cause);
                    let mut notify = existing.peers.clone();
                    notify.insert(existing.owner);
                    let txs = notify.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
                    for tx in txs { let _ = tx.send(release.clone()); }
                    inner.calls.remove(&existing_id);
                    preempted = Some(existing_id);
                    info!(old_uuid=%existing_id, new_uuid=%id, gssi=gt.destination,
                        old_priority=existing.priority, new_priority=gt.priority, "pre-empted group call");
                }
            }
        }
    }

    let mut targets = if state.config.route_without_affiliations {
        inner.clients.keys().copied().collect::<HashSet<_>>()
    } else {
        inner.group_clients.get(&gt.destination).cloned().unwrap_or_default()
    };

    // Basestation can be connected and forwarding calls before an AFFILIATE event
    // has reached Brew (for example during startup/resync or while debugging MM
    // group-affiliation propagation). In that case a strict affiliation-only core
    // silently produces target_count=0. For small/private networks we support an
    // explicit fallback to every other connected BS. Once affiliations exist, the
    // normal selective routing above is used.
    if targets.is_empty()
        && !state.config.route_without_affiliations
        && state.config.fallback_broadcast_when_no_affiliations
    {
        targets = inner.clients.keys().copied().collect::<HashSet<_>>();
        warn!(
            %source,
            gssi = gt.destination,
            connected_clients = inner.clients.len(),
            "no Brew affiliations recorded for GSSI; falling back to all connected Basestations"
        );
    }
    targets.remove(&source);
    inner.group_floor.insert(gt.destination, id);
    inner.calls.insert(id, ActiveCall {
        kind: CallKind::Group,
        owner: source,
        source_issi: gt.source,
        destination: gt.destination,
        priority: gt.priority,
        peers: targets.clone(),
        started_at: std::time::Instant::now(),
        last_activity_ms: ActiveCall::new_activity(),
    });
    let txs = targets.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
    drop(inner);
    for tx in txs { let _ = tx.send(raw.clone()); }
    if let Some(old) = preempted {
        state.monitor.call_ended(old).await;
        if let Some(h) = state.sip.read().await.as_ref() {
            if let Some(bridge) = h.transport.bridge.read().await.clone() {
                bridge.teardown_by_brew_call(old).await;
            }
        }
    }
    state.monitor.call_started(id, "group", gt.source, gt.destination, gt.priority).await;
    info!(%source, uuid=%id, src_issi=gt.source, gssi=gt.destination, priority=gt.priority,
        target_count=targets.len(), "routed GROUP_TX");
}

async fn handle_sds_header(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, payload: CallPayload, raw: Vec<u8>) {
    let CallPayload::ShortTransfer { source: source_issi, destination } = payload else { return };
    // TEMPORARY (position debugging): SHORT_TRANSFER sometimes carries the SDS
    // user-data inline. Dump it and try a position decode here too, so a beacon
    // that never produces a separate SDS_TRANSFER frame is still caught.
    info!(uuid=%id, source_issi, destination, hex=%hex_dump(&raw), "SDS header (SHORT_TRANSFER) raw");
    if let Some((lat, lon, note)) = extract_sds_position(&raw) {
        let now = crate::telemetry::now_ms();
        state.telemetry.write().await.record_sds_position(source_issi, lat, lon, now, note);
        crate::aprs::report_position(state, source_issi, lat, lon);
        info!(uuid=%id, source_issi, lat, lon, "decoded MS position from SDS header");
    }
    let mut inner = state.inner.write().await;
    let mut targets = HashSet::new();
    if let Some(sub) = inner.subscribers.get(&destination) { targets.insert(sub.client_id); }
    if let Some(group_targets) = inner.group_clients.get(&destination) { targets.extend(group_targets.iter().copied()); }
    targets.remove(&source);
    // Always remember the UUID -> source ISSI mapping so a following
    // SDS_TRANSFER can be attributed (and its position decoded) even when the
    // destination is not a registered Brew subscriber — position beacons are
    // often addressed to an external app/gateway ISSI that never registers.
    inner.sds_routes.insert(id, SdsRoute { source_client: source, targets: targets.clone(), source_issi, destination, created_at: Instant::now() });
    if targets.is_empty() {
        drop(inner);
        warn!(%source, uuid=%id, channel="brew", source_issi, destination, lip=sds_is_lip(&raw), "SDS has no registered destination (position still tracked)");
        return;
    }
    let txs = targets.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
    drop(inner);
    for tx in txs { let _ = tx.send(raw.clone()); }
    state.monitor.sds(id, source_issi, destination).await;
    info!(%source, uuid=%id, channel="brew", source_issi, destination, lip=sds_is_lip(&raw), target_count=targets.len(), "routed SDS header");
}

async fn handle_sds_transfer(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    // Look up the route (stored by the SHORT_TRANSFER header, even when the SDS
    // was undeliverable) to recover the source ISSI and any delivery targets.
    let (source_issi, txs) = {
        let inner = state.inner.read().await;
        match inner.sds_routes.get(&id) {
            Some(route) if route.source_client == source => {
                let txs = route.targets.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
                (route.source_issi, txs)
            }
            Some(_) => { warn!(%source, uuid=%id, "SDS_TRANSFER from non-originating client"); return; }
            None => { warn!(uuid=%id, "SDS_TRANSFER without SHORT_TRANSFER (position may still decode)"); (0u32, Vec::new()) }
        }
    };
    for tx in &txs { let _ = tx.send(raw.clone()); }

    // TEMPORARY (position debugging): dump the raw SDS_TRANSFER frame so the LIP
    // payload offset can be confirmed against live traffic. Remove once binary
    // LIP positions are confirmed decoding on the map.
    info!(uuid=%id, source_issi, bytes=%raw.len(), hex=%hex_dump(&raw), "SDS_TRANSFER raw frame");

    // Position extraction from the relayed SDS. Basestation cannot be modified,
    // but it relays the full SDS (including binary LIP payloads) over the Brew
    // channel, so we decode positions here regardless of deliverability.
    if let Some((lat, lon, note)) = extract_sds_position(&raw) {
        let now = crate::telemetry::now_ms();
        state.telemetry.write().await.record_sds_position(source_issi, lat, lon, now, note);
        crate::aprs::report_position(state, source_issi, lat, lon);
        info!(uuid=%id, source_issi, lat, lon, "decoded MS position from SDS");
    }
}

/// Renders bytes as a compact hex string for debug logging (capped so a large
/// frame does not flood the log).
fn hex_dump(bytes: &[u8]) -> String {
    const MAX: usize = 64;
    let shown = &bytes[..bytes.len().min(MAX)];
    let mut s = shown.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    if bytes.len() > MAX {
        s.push_str(&format!(" … (+{} more)", bytes.len() - MAX));
    }
    s
}

/// Quick check whether a raw SDS_TRANSFER/SHORT_TRANSFER frame carries a LIP
/// position payload: either the short-report PID 0x0A or the MTH-style long
/// report marker 0x83, in the SDS data region (after the 20-byte frame header).
/// Used only for log annotation, so it is deliberately lenient.
fn sds_is_lip(raw: &[u8]) -> bool {
    let body = if raw.len() > 20 { &raw[20..] } else { raw };
    body.iter().any(|&b| b == 0x0A || b == 0x83)
}

/// Attempts to pull a geographic position out of a raw SDS_TRANSFER frame:
/// first a binary LIP short location report (scanning for the 0x0A PID), then a
/// textual beacon in any embedded ASCII. Returns (lat, lon, source_note).
fn extract_sds_position(raw: &[u8]) -> Option<(f64, f64, String)> {
    // The frame carries a 20-byte Brew frame header (class, type, uuid, len)
    // before the SDS content; scan the whole buffer defensively for the LIP PID.
    let body = if raw.len() > 20 { &raw[20..] } else { raw };
    // Motorola MTH-series "long location report": SDS data begins 0x83. Try this
    // first since its 0x0A appears mid-PDU (not as the leading PID).
    for i in 0..body.len() {
        if body[i] == 0x83 {
            if let Some(ll) = crate::position::decode_lip_long(&body[i..]) {
                return Some((ll.lat, ll.lon, "LIP long location report".to_string()));
            }
        }
    }
    // Standard LIP short location report: scan for the 0x0A PID.
    for i in 0..body.len() {
        if body[i] == 0x0A {
            if let Some(ll) = crate::position::decode_lip(&body[i..]) {
                return Some((ll.lat, ll.lon, "LIP short location report".to_string()));
            }
        }
    }
    // Fall back to textual coordinates in any ASCII run of the body.
    let text: String = body.iter().map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { ' ' }).collect();
    crate::position::parse_position(&text).map(|ll| (ll.lat, ll.lon, text.trim().to_string()))
}

async fn handle_sds_report(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    let mut inner = state.inner.write().await;
    let Some(route) = inner.sds_routes.get(&id).cloned() else { return };
    if !route.targets.contains(&source) { warn!(%source, uuid=%id, "SDS_REPORT from unexpected client"); return; }
    let tx = inner.clients.get(&route.source_client).map(|c| c.tx.clone());
    // For unicast the transaction is complete. For multicast keep it until TTL so multiple reports can return.
    if route.targets.len() == 1 { inner.sds_routes.remove(&id); }
    drop(inner);
    if let Some(tx) = tx { let _ = tx.send(raw); }
    state.monitor.sds_report(id).await;
    info!(%source, uuid=%id, source_issi=route.source_issi, destination=route.destination, "routed SDS report");
}

async fn handle_private_setup(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, payload: CallPayload, raw: Vec<u8>) {
    // Prefer the structured CircularCall payload (parsed per Brew v1). Fall back
    // to the conservative raw source/destination pair for any peer that sends a
    // payload we could not fully structure.
    let (source_issi, destination, number, mnemonic) = match &payload {
        CallPayload::CircularCall(c) => (c.source, c.destination, c.number.clone(), c.mnemonic.clone()),
        other => match protocol::raw_peer_pair(other) {
            Some((s, d)) => (s, d, String::new(), None),
            None => {
                warn!(%source, uuid=%id, "private SETUP_REQUEST has no routable source/destination pair");
                return;
            }
        },
    };
    let mut inner = state.inner.write().await;
    let Some(target_client) = inner.subscribers.get(&destination).map(|s| s.client_id) else {
        drop(inner);
        // The destination is not a registered Brew subscriber. Before giving up,
        // offer it to the SIP subsystem: a voice route may bridge this TETRA
        // private call out to a SIP extension or trunk (Brew -> SIP direction).
        // The dialled string is the ASCII `number` field when the caller sent
        // one (a PBX/phone call to a non-ISSI number, e.g. "9" + a 10-digit
        // PSTN number: destination is 0/unrouted and the actual digits live in
        // `number`, not `destination` -- see BrewCircularCall), falling back to
        // the destination ISSI rendered as decimal for ordinary ISSI-to-ISSI
        // calls that never set `number`. Route patterns can match either shape
        // (e.g. "9*" for a PSTN prefix, "7*" or an exact ISSI string).
        let dialled = {
            let trimmed = number.trim();
            if trimmed.is_empty() { destination.to_string() } else { trimmed.to_string() }
        };
        let bridged = {
            let guard = state.sip.read().await;
            match guard.as_ref() {
                Some(h) => {
                    if let Some(bridge) = h.transport.bridge.read().await.clone() {
                        let origin = crate::sip::routing::CallOrigin::BrewPrivate(source_issi);
                        let link = crate::sip::bridge::BrewCallLink { call_id: id, client: source, source_issi };
                        bridge.brew_to_sip(origin, &dialled, link).await
                    } else { false }
                }
                None => false,
            }
        };
        if bridged {
            state.monitor.call_started(id, "private", source_issi, destination, 0).await;
            info!(%source, uuid=%id, source_issi, destination, dialled = %dialled, mnemonic=?mnemonic, "routed private SETUP_REQUEST to SIP");
        } else {
            warn!(%source, uuid=%id, destination, dialled = %dialled, "private call destination not registered (no SIP route)");
        }
        return;
    };
    if target_client == source { return; }
    let peers = HashSet::from([target_client]);
    inner.calls.insert(id, ActiveCall { kind: CallKind::Private, owner: source, source_issi, destination, priority: 0, peers: peers.clone(), started_at: std::time::Instant::now(), last_activity_ms: ActiveCall::new_activity() });
    let tx = inner.clients.get(&target_client).map(|c| c.tx.clone());
    drop(inner);
    if let Some(tx) = tx { let _ = tx.send(raw); }
    state.monitor.call_started(id, "private", source_issi, destination, 0).await;
    info!(%source, uuid=%id, source_issi, destination, mnemonic=?mnemonic, "routed private SETUP_REQUEST");
}

async fn route_private_control(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    // A rejected setup is the end of the call: route it like a release so the
    // call is removed (and the dashboard updated) instead of lingering.
    if raw.get(1) == Some(&CALL_SETUP_REJECT) {
        end_call(state, source, id, raw).await;
        return;
    }
    let inner = state.inner.read().await;
    let Some(call) = inner.calls.get(&id) else { debug!(uuid=%id, "private control for unknown call"); return; };
    if call.kind != CallKind::Private { return; }
    call.touch();
    let mut recipients = call.peers.clone();
    recipients.insert(call.owner);
    recipients.remove(&source);
    let txs = recipients.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
    drop(inner);
    for tx in txs { let _ = tx.send(raw.clone()); }
}

async fn route_call_frame(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    let Some(txs) = call_frame_recipients(state, source, id, "voice").await else { return };
    state.monitor.voice_frame(id).await;
    for tx in txs { let _ = tx.send(raw.clone()); }
}

async fn route_dtmf_frame(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    let Some(txs) = call_frame_recipients(state, source, id, "DTMF").await else { return };
    for tx in txs { let _ = tx.send(raw.clone()); }
}

/// Shared participant/permission check and recipient lookup for both call
/// audio (`route_call_frame`) and DTMF (`route_dtmf_frame`): only the current
/// group floor holder or a private call's two participants may inject a
/// frame, and it fans out to every other participant.
async fn call_frame_recipients(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, kind: &str) -> Option<Vec<mpsc::UnboundedSender<Vec<u8>>>> {
    let inner = state.inner.read().await;
    let Some(call) = inner.calls.get(&id) else { debug!(uuid=%id, "{kind} frame for unknown call"); return None; };
    let mut allowed = call.peers.contains(&source) || call.owner == source;
    if call.kind == CallKind::Group { allowed = call.owner == source; }
    if !allowed { warn!(%source, uuid=%id, "{kind} frame from non-participant"); return None; }
    call.touch();
    let mut recipients = call.peers.clone();
    if call.kind == CallKind::Private { recipients.insert(call.owner); }
    recipients.remove(&source);
    Some(recipients.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect())
}

/// Periodically ends Brew calls (private or group -- a station call directly
/// between Basestations/mobiles, or the Brew leg of a SIP-bridged call) that
/// have run longer than `Config::max_call_duration_seconds`. Reuses `end_call`
/// so a timed-out call ends exactly like a normal hangup (CALL_RELEASE /
/// CALL_GROUP_IDLE to participants, dashboard event, SIP-bridge teardown),
/// not a silent kill. A no-op (never spawned as a busy loop) when the limit
/// is 0 (disabled) -- see `main.rs`, which only spawns this when non-zero.
/// Also ends calls that have carried no voice/DTMF frame or call control for
/// `Config::call_inactivity_timeout_seconds` (e.g. a GROUP_IDLE that never
/// arrived), except SIP-bridged ones, whose inactivity is judged on the RTP
/// side by the SIP sweep with the same timeout.
/// Pure filter: which calls in `calls` have been running at least `limit`
/// (zero = no limit). Split out from `run_call_duration_sweep` so it's
/// testable without an actual timer/interval.
fn expired_calls(calls: &HashMap<uuid::Uuid, ActiveCall>, limit: std::time::Duration) -> Vec<(uuid::Uuid, ClientId, CallKind)> {
    if limit.is_zero() { return Vec::new(); }
    calls.iter()
        .filter(|(_, call)| call.started_at.elapsed() >= limit)
        .map(|(id, call)| (*id, call.owner, call.kind))
        .collect()
}

/// Pure filter: calls idle for at least `idle_ms` (zero = disabled) that
/// don't involve a SIP bridge virtual client (per `is_bridged`).
fn idle_calls(calls: &HashMap<uuid::Uuid, ActiveCall>, idle_ms: u64, is_bridged: impl Fn(&ClientId) -> bool) -> Vec<(uuid::Uuid, ClientId, CallKind)> {
    if idle_ms == 0 { return Vec::new(); }
    calls.iter()
        .filter(|(_, call)| call.idle_ms() >= idle_ms)
        .filter(|(_, call)| !is_bridged(&call.owner) && !call.peers.iter().any(&is_bridged))
        .map(|(id, call)| (*id, call.owner, call.kind))
        .collect()
}

pub async fn run_call_duration_sweep(state: Arc<AppState>) {
    let limit = std::time::Duration::from_secs(state.config.max_call_duration_seconds);
    let idle_ms = state.config.call_inactivity_timeout_seconds * 1000;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        ticker.tick().await;
        let expired = {
            let inner = state.inner.read().await;
            // SIP bridge virtual clients are the only local, address-less terminals.
            let is_bridged = |cid: &ClientId| inner.clients.get(cid)
                .is_some_and(|c| c.mode == crate::state::ClientMode::Terminal && c.remote_addr.is_none());
            let mut v: Vec<_> = expired_calls(&inner.calls, limit).into_iter().map(|c| (c, "exceeded max duration")).collect();
            for c in idle_calls(&inner.calls, idle_ms, is_bridged) {
                if !v.iter().any(|((id, ..), _)| *id == c.0) { v.push((c, "inactive (no media)")); }
            }
            v
        };
        for ((id, owner, kind), why) in expired {
            let release_state = if kind == CallKind::Group { protocol::CALL_GROUP_IDLE } else { protocol::CALL_RELEASE };
            let raw = protocol::build_call_cause(release_state, &id, 0);
            warn!(uuid=%id, ?kind, reason = why, "call timed out; force-ending");
            end_call(&state, owner, id, raw).await;
        }
    }
}

async fn end_call(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    let mut inner = state.inner.write().await;
    let Some(call) = inner.calls.remove(&id) else { debug!(uuid=%id, "call end for unknown call"); return; };
    let participant = call.owner == source || call.peers.contains(&source);
    if !participant { inner.calls.insert(id, call); return; }
    if call.kind == CallKind::Group && call.owner != source { inner.calls.insert(id, call); return; }
    if call.kind == CallKind::Group && inner.group_floor.get(&call.destination) == Some(&id) { inner.group_floor.remove(&call.destination); }
    let mut recipients = call.peers.clone();
    if call.kind == CallKind::Private { recipients.insert(call.owner); }
    recipients.remove(&source);
    let txs = recipients.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
    drop(inner);
    for tx in txs { let _ = tx.send(raw.clone()); }
    state.monitor.call_ended(id).await;
    info!(%source, uuid=%id, kind=?call.kind, "routed call end");

    // If this call was bridged to SIP (Brew subscriber calling out), the Brew
    // side just ended it first: tell the SIP peer too, instead of leaving its
    // dialog dangling with a dead RTP stream.
    if let Some(h) = state.sip.read().await.as_ref() {
        if let Some(bridge) = h.transport.bridge.read().await.clone() {
            bridge.teardown_by_brew_call(id).await;
        }
    }
}

async fn handle_subscriber(state: &Arc<AppState>, source: ClientId, msg: SubscriberMessage) {
    let mut inner = state.inner.write().await;
    // The connecting client's advertised mode (Terminal/Basestation), used to
    // tag the subscriber registration so MS-registration counts can exclude
    // Basestation (Basestation gateway) registrations, which are not an MS.
    let source_mode = inner.clients.get(&source).map(|c| c.mode).unwrap_or_default();
    // Set below when this message is a Terminal-mode register/deregister, so
    // it can be logged to the dashboard's registration log once `inner` is
    // released (mirrors how position decoding logs via `state.telemetry`
    // outside of the `inner` lock elsewhere in this module).
    let mut ms_reg_event: Option<&'static str> = None;
    // Captured before the match below (which may consume `msg.groups`), for
    // the federation relay after it.
    let relay_groups = msg.groups.clone();
    match msg.msg_type {
        SUB_REGISTER | SUB_REREGISTER => {
            let previous = inner.subscribers.get(&msg.issi).map(|s| (s.client_id, s.groups.clone()));
            let old_groups = previous.as_ref().map(|(_, groups)| groups.clone()).unwrap_or_default();
            if let Some((old_client, groups)) = previous {
                if old_client != source {
                    for gssi in &groups {
                        if let Some(clients) = inner.group_clients.get_mut(gssi) { clients.remove(&old_client); clients.insert(source); }
                    }
                }
            }
            inner.subscribers.insert(msg.issi, Subscriber { client_id: source, groups: old_groups, mode: source_mode });
            info!(%source, issi=msg.issi, mode=source_mode.as_str(), "subscriber registered");
            if source_mode == crate::state::ClientMode::Terminal { ms_reg_event = Some("register"); }
        }
        SUB_DEREGISTER => {
            if let Some(sub) = inner.subscribers.remove(&msg.issi) {
                if sub.client_id == source {
                    for gssi in sub.groups {
                        let still_present = inner.subscribers.values().any(|other| other.client_id == source && other.groups.contains(&gssi));
                        if !still_present { if let Some(clients) = inner.group_clients.get_mut(&gssi) { clients.remove(&source); } }
                    }
                    info!(%source, issi=msg.issi, mode=sub.mode.as_str(), "subscriber deregistered");
                    if sub.mode == crate::state::ClientMode::Terminal { ms_reg_event = Some("deregister"); }
                } else { inner.subscribers.insert(msg.issi, sub); }
            }
        }
        SUB_AFFILIATE => {
            let owner = inner.subscribers.get(&msg.issi).map(|s| s.client_id);
            if let Some(owner) = owner { if owner != source { warn!(%source, issi=msg.issi, "affiliation from non-owner"); return; } }
            else { inner.subscribers.insert(msg.issi, Subscriber { client_id: source, groups: HashSet::new(), mode: source_mode }); }
            for gssi in msg.groups {
                if let Some(sub) = inner.subscribers.get_mut(&msg.issi) { sub.groups.insert(gssi); }
                inner.group_clients.entry(gssi).or_default().insert(source);
                info!(%source, issi=msg.issi, gssi, "subscriber affiliated");
            }
        }
        SUB_DEAFFILIATE => {
            if inner.subscribers.get(&msg.issi).map(|s| s.client_id) != Some(source) { return; }
            for gssi in msg.groups {
                if let Some(sub) = inner.subscribers.get_mut(&msg.issi) { sub.groups.remove(&gssi); }
                let still_present = inner.subscribers.values().any(|other| other.client_id == source && other.groups.contains(&gssi));
                if !still_present { if let Some(clients) = inner.group_clients.get_mut(&gssi) { clients.remove(&source); } }
                info!(%source, issi=msg.issi, gssi, "subscriber deaffiliated");
            }
        }
        _ => debug!(%source, msg_type=msg.msg_type, "unknown subscriber message"),
    }
    // Federation: relay this registration/affiliation event to every *other*
    // connected peer (split-horizon -- never echo it back out the peer link
    // it arrived on). Works for a message that originated locally (source is
    // a real client) and for one already relayed in from another peer (this
    // server is then a transit hop, propagating it further out); either way
    // this is what makes a remote ISSI/GSSI's registration reachable from
    // this server, and from here on call/SDS routing needs no federation-
    // specific code at all -- it already resolves via `inner.subscribers`/
    // `inner.group_clients`, which now includes this entry.
    let relay_targets: Vec<_> = if matches!(msg.msg_type, SUB_REGISTER | SUB_REREGISTER | SUB_DEREGISTER | SUB_AFFILIATE | SUB_DEAFFILIATE) {
        inner.clients.iter()
            .filter(|(cid, c)| c.mode == crate::state::ClientMode::Peer && **cid != source)
            .map(|(_, c)| c.tx.clone())
            .collect()
    } else {
        Vec::new()
    };
    drop(inner);
    if !relay_targets.is_empty() {
        let relay = protocol::build_subscriber_message(msg.msg_type, msg.issi, &relay_groups);
        for tx in relay_targets { let _ = tx.send(relay.clone()); }
    }
    // Log Terminal-mode (actual MS) registration lifecycle events to the same
    // dashboard registration log Basestation telemetry registrations use, so
    // an MS registering directly over the Brew protocol is visible there too.
    if let Some(kind) = ms_reg_event {
        state.telemetry.write().await.record_brew_registration(msg.issi, kind);
    }
}

#[cfg(test)]
mod position_tests {
    use super::extract_sds_position;

    // Real LIP beacon captured from Basestation (ISSI 90), Athens.
    const LIP: [u8; 11] = [0x0a, 0x01, 0x0e, 0x62, 0x39, 0xb0, 0x43, 0x9a, 0xff, 0xe0, 0x20];

    fn framed(payload: &[u8]) -> Vec<u8> {
        // 20-byte Brew frame header (contents irrelevant to the scan) + SDS body.
        let mut v = vec![0u8; 20];
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn decodes_mth850_long_report_from_framed_sds() {
        // Real captured SDS_TRANSFER: 20-byte frame header, then c8 00, then the
        // 0x83 LIP long report. extract_sds_position must find and decode it.
        let hex = "f2 01 2f 5a 58 59 e4 8e 4a 45 bf 8a b6 3c 8a be 54 b6 c8 00 83 00 11 80 13 13 23 2f 34 1f 5c 77 6d ea 66 36 08 68 e3 10 e6 16 c1 56 60";
        let raw: Vec<u8> = hex.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap()).collect();
        let (lat, lon, note) = super::extract_sds_position(&raw).expect("should decode long report");
        assert!((lat - 37.9917).abs() < 0.01, "lat={lat}");
        assert!((lon - 23.7640).abs() < 0.01, "lon={lon}");
        assert!(note.contains("long"));
    }

    #[test]
    fn decodes_lip_from_framed_sds() {
        let raw = framed(&LIP);
        let (lat, lon, note) = extract_sds_position(&raw).expect("should decode");
        assert!((lat - 37.9920).abs() < 0.01, "lat={lat}");
        assert!((lon - 23.7642).abs() < 0.01, "lon={lon}");
        assert!(note.contains("LIP"));
    }

    #[test]
    fn decodes_lip_with_sds_tl_header_prefix() {
        // Some stacks prepend an SDS-TL header before the 0x0A PID; the scan must
        // still find it.
        let mut payload = vec![0x82, 0x00, 0x00, 0x00];
        payload.extend_from_slice(&LIP);
        let raw = framed(&payload);
        assert!(extract_sds_position(&raw).is_some());
    }

    #[test]
    fn ignores_non_position_sds() {
        let raw = framed(b"\x01hello there");
        assert!(extract_sds_position(&raw).is_none());
    }

    #[test]
    fn decodes_textual_beacon() {
        let raw = framed(b"\x0144.4353, 26.1092");
        let (lat, lon, _) = extract_sds_position(&raw).expect("text decode");
        assert!((lat - 44.4353).abs() < 0.01 && (lon - 26.1092).abs() < 0.01);
    }
}

#[cfg(test)]
mod call_duration_tests {
    use super::*;
    use std::time::Duration;

    fn call(started_at: std::time::Instant, kind: CallKind) -> ActiveCall {
        ActiveCall {
            kind,
            owner: uuid::Uuid::new_v4(),
            source_issi: 1001,
            destination: 90,
            priority: 0,
            peers: HashSet::new(),
            started_at,
            last_activity_ms: ActiveCall::new_activity(),
        }
    }

    #[test]
    fn finds_only_calls_at_or_past_the_limit() {
        let now = std::time::Instant::now();
        let mut calls = HashMap::new();
        let old_id = uuid::Uuid::new_v4();
        calls.insert(old_id, call(now - Duration::from_secs(120), CallKind::Private));
        let fresh_id = uuid::Uuid::new_v4();
        calls.insert(fresh_id, call(now - Duration::from_secs(5), CallKind::Group));

        let expired = expired_calls(&calls, Duration::from_secs(60));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, old_id);
    }

    #[test]
    fn idle_filter_skips_active_and_bridged_calls() {
        let now = std::time::Instant::now();
        let mut calls = HashMap::new();
        let stale = call(now, CallKind::Group);
        stale.last_activity_ms.store(crate::telemetry::now_ms() - 120_000, std::sync::atomic::Ordering::Relaxed);
        let stale_id = uuid::Uuid::new_v4();
        let bridged = call(now, CallKind::Private);
        bridged.last_activity_ms.store(0, std::sync::atomic::Ordering::Relaxed);
        let bridged_owner = bridged.owner;
        calls.insert(stale_id, stale);
        calls.insert(uuid::Uuid::new_v4(), bridged);
        calls.insert(uuid::Uuid::new_v4(), call(now, CallKind::Group));

        let idle = idle_calls(&calls, 60_000, |c| *c == bridged_owner);
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].0, stale_id);
        assert!(idle_calls(&calls, 0, |_| false).is_empty());
    }

    #[test]
    fn empty_when_nothing_exceeds_limit() {
        let now = std::time::Instant::now();
        let mut calls = HashMap::new();
        calls.insert(uuid::Uuid::new_v4(), call(now, CallKind::Private));
        assert!(expired_calls(&calls, Duration::from_secs(60)).is_empty());
    }
}
