use crate::{
    protocol::{
        self, BrewMessage, CallPayload, SubscriberMessage, CALL_ALERT, CALL_CONNECT_CONFIRM,
        CALL_CONNECT_REQUEST, CALL_GROUP_IDLE, CALL_GROUP_TX, CALL_RELEASE, CALL_SETUP_ACCEPT,
        CALL_SETUP_REJECT, CALL_SETUP_REQUEST, CALL_SHORT_TRANSFER, CALL_SIMPLEX_GRANTED,
        CALL_SIMPLEX_IDLE, FRAME_SDS_REPORT, FRAME_SDS_TRANSFER, FRAME_TRAFFIC_CHANNEL,
        SUB_AFFILIATE, SUB_DEAFFILIATE, SUB_DEREGISTER, SUB_REGISTER, SUB_REREGISTER,
    },
    state::{ActiveCall, AppState, CallKind, ClientId, ClientMode, Inner, SdsRoute, Subscriber},
};
use std::{collections::{HashMap, HashSet}, sync::Arc, time::Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// TETRA basic-service byte a dispatch console sets in a private-call
/// SETUP_REQUEST to ask for ambience listening. Matches tetra-dispatch's
/// `SERVICE_AMBIENCE_LISTENING`. Provisional — adjust per deployment.
const AMBIENCE_LISTENING_SERVICE: u8 = 9;

/// Offset of the priority byte in a `CALL_GROUP_TX`: class, call state, uuid(16),
/// source(4), destination(4).
const GROUP_TX_PRIORITY_AT: usize = 2 + 16 + 4 + 4;

/// Whether a call is an emergency call: priority 15 on the wire, or its
/// originating ISSI has an emergency alarm active on some Basestation (a
/// FlowStation forwards its radios' calls to Brew at priority 0).
async fn is_emergency_call(state: &Arc<AppState>, source_issi: u32, priority: u8) -> bool {
    priority >= EMERGENCY_PRIORITY || state.telemetry.read().await.is_emergency(source_issi)
}

/// `raw` (a `CALL_GROUP_TX`) with its priority raised to the emergency level,
/// for recipients that tell emergencies by it -- dispatch consoles and peers.
fn with_emergency_priority(raw: &[u8]) -> Vec<u8> {
    let mut out = raw.to_vec();
    if let Some(p) = out.get_mut(GROUP_TX_PRIORITY_AT) { *p = EMERGENCY_PRIORITY; }
    out
}

/// An emergency alarm just came up for `issi`: a group call it has running
/// becomes an emergency call -- raised priority, and the dispatch consoles and
/// peers that were not part of it are brought in with a raised GROUP_TX, so its
/// voice and end reach them from here on.
pub async fn emergency_raised(state: &Arc<AppState>, issi: u32) {
    let mut inner = state.inner.write().await;
    // Consoles and every peer link: the emergency goes to all servers.
    let consoles: Vec<ClientId> = inner.consoles.iter().copied()
        .chain(inner.clients.iter().filter(|(_, c)| c.mode == ClientMode::Peer).map(|(id, _)| *id)).collect();
    let calls: Vec<uuid::Uuid> = inner.calls.iter()
        .filter(|(_, c)| c.kind == CallKind::Group && c.source_issi == issi && c.priority < EMERGENCY_PRIORITY)
        .map(|(id, _)| *id).collect();
    let mut sends = Vec::new();
    for id in calls {
        let Some(call) = inner.calls.get_mut(&id) else { continue };
        call.priority = EMERGENCY_PRIORITY;
        let (dest, owner) = (call.destination, call.owner);
        let existing: Vec<ClientId> = call.peers.iter().copied().collect();
        let new: Vec<ClientId> = consoles.iter().copied().filter(|c| *c != owner && !call.peers.contains(c)).collect();
        call.peers.extend(new.iter().copied());
        let msg = protocol::build_group_tx(&id, issi, dest, EMERGENCY_PRIORITY);
        // Consoles and peers already in the call get the raised header too; the new
        // ones hear the call for the first time.
        for cid in existing.into_iter().chain(new) {
            if inner.consoles.contains(&cid) || is_peer(&inner, cid) {
                if let Some(c) = inner.clients.get(&cid) { sends.push((c.tx.clone(), msg.clone())); }
            }
        }
        info!(uuid=%id, issi, gssi=dest, "group call became an emergency call (alarm raised)");
    }
    drop(inner);
    for (tx, msg) in sends { let _ = tx.send(msg); }
}

/// Call priority of an emergency call (ETSI EN 300 392-2 clause 14.8: 15, what
/// a terminal's emergency button generates). Emergency calls are never held back
/// by the ISSI blacklist, from or to a blacklisted terminal.
pub const EMERGENCY_PRIORITY: u8 = 15;

/// Wire shape of a `SERVICE_RSSI` message's JSON payload.
#[derive(serde::Deserialize)]
struct RssiReport {
    issi: u32,
    rssi_dbfs: f32,
}

pub async fn handle_packet(state: Arc<AppState>, source: ClientId, raw: Vec<u8>) {
    state.purge_ephemeral().await;
    // Route adverts between loop-safe brew-servers: not Brew, see `fedroute`.
    if raw.first() == Some(&protocol::CLASS_FEDERATION) {
        return crate::fedroute::handle(&state, source, &raw).await;
    }
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
        BrewMessage::Service(svc) if svc.service_type == protocol::SERVICE_BLACKLIST_CMD => {
            crate::blacklist::handle_command(&state, source, &svc.json_data).await;
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
    let emergency = is_emergency_call(state, gt.source, gt.priority).await;
    if state.is_blocked(gt.source) && !emergency {
        info!(%source, uuid=%id, src_issi=gt.source, gssi=gt.destination, reason=state.bar_reason(gt.source).unwrap_or(""), "GROUP_TX from a barred ISSI dropped");
        return;
    }
    let mut inner = state.inner.write().await;
    let now = Instant::now();
    // The same transmission reaching us again over another peer link (a ring,
    // a mesh, a redundant link): drop it before it can pre-empt or take over
    // the call it duplicates.
    if is_peer(&inner, source) && crate::fedroute::is_duplicate(&inner, id, gt.source, source, now) {
        debug!(%source, uuid=%id, src_issi=gt.source, gssi=gt.destination, "duplicate GROUP_TX from a second peer link dropped");
        // A loop-safe peer is asked to stop sending this turn's voice our
        // way too (an older peer would not understand it).
        if inner.fed.links.contains_key(&source) {
            if let Some(client) = inner.clients.get(&source) {
                let _ = client.tx.send(crate::fedroute::build_prune(&id, gt.source));
            }
        }
        return;
    }
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
    // The talker's own GROUP_TX again (a new over of the same call), over the
    // link that delivered it: peer links that already carry the call keep
    // it, even if the route that put them there has changed since, so a
    // re-route half way through a call does not cut off a server hearing it.
    if let Some(existing) = inner.calls.get(&id).filter(|c| c.owner == source && c.source_issi == gt.source) {
        targets.extend(existing.peers.iter().copied().filter(|p| is_peer(&inner, *p)));
    }
    // An emergency call is pushed to every connected dispatch console, whatever
    // groups it listens to, so the operator can hear and see it. They join the
    // call's peers, so its voice and its end reach them too.
    // Likewise to every federation peer, so the emergency reaches all servers (and their
    // consoles) however the groups are routed; a peer that hears it twice over a ring or
    // mesh prunes the duplicate as for any call.
    if emergency {
        targets.extend(inner.consoles.iter().copied());
        targets.extend(inner.clients.iter().filter(|(_, c)| c.mode == ClientMode::Peer).map(|(id, _)| *id));
    }
    targets.remove(&source);
    inner.group_floor.insert(gt.destination, id);
    inner.calls.insert(id, ActiveCall {
        kind: CallKind::Group,
        owner: source,
        source_issi: gt.source,
        destination: gt.destination,
        priority: if emergency { gt.priority.max(EMERGENCY_PRIORITY) } else { gt.priority },
        peers: targets.clone(),
        started_at: std::time::Instant::now(),
        last_activity_ms: ActiveCall::new_activity(),
    });
    {
        let Inner { calls, group_tx_raw, .. } = &mut *inner;
        group_tx_raw.retain(|call_id, _| calls.contains_key(call_id));
        group_tx_raw.insert(id, raw.clone());
    }
    crate::fedroute::note_call(&mut inner, id, gt.source, source, now);
    // Each recipient gets the GROUP_TX in its own layout: a connection that
    // announced v0 is not handed the v1 talker-name tail (see
    // `Client::forward_version`).
    // An emergency the wire does not say so about (priority 0 from FlowStation) is
    // raised for consoles and peers, which tell it by the priority.
    let raised = (emergency && gt.priority < EMERGENCY_PRIORITY).then(|| with_emergency_priority(&raw));
    let txs = targets.iter().filter_map(|cid| inner.clients.get(cid).map(|c| {
        let raise = raised.is_some() && (inner.consoles.contains(cid) || c.mode == ClientMode::Peer);
        (c.tx.clone(), c.forward_version(), raise)
    })).collect::<Vec<_>>();
    drop(inner);
    for (tx, version, raise) in txs {
        let packet = if raise { raised.as_deref().unwrap_or(&raw) } else { &raw };
        let _ = tx.send(protocol::adapt_to_version(packet, version).into_owned());
    }
    if let Some(old) = preempted {
        state.monitor.call_ended(old).await;
        if let Some(h) = state.sip.read().await.as_ref() {
            if let Some(bridge) = h.transport.bridge.read().await.clone() {
                bridge.teardown_by_brew_call(old).await;
            }
        }
    }
    state.monitor.call_started(id, "group", gt.source, gt.destination, if emergency { gt.priority.max(EMERGENCY_PRIORITY) } else { gt.priority }).await;
    info!(%source, uuid=%id, src_issi=gt.source, gssi=gt.destination, priority=gt.priority,
        target_count=targets.len(), "routed GROUP_TX");
}

/// Relays a position decoded here to the peers -- unless it came out of an SDS
/// a peer forwarded: its origin server decoded it too and advertises it.
async fn advertise_if_local(state: &Arc<AppState>, source: ClientId, issi: u32, lat: f64, lon: f64, at_ms: u64) {
    if is_peer(&*state.inner.read().await, source) { return; }
    crate::fedroute::advertise_ms(state, issi, lat, lon, at_ms, "brew-sds").await;
}

async fn handle_sds_header(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, payload: CallPayload, raw: Vec<u8>) {
    let CallPayload::ShortTransfer { source: source_issi, destination } = payload else { return };
    // A copy of an SDS already accepted over another peer link: dropped
    // before its position is reported (APRS, telemetry) a second time.
    // Checked and noted in one critical section, so two copies racing in
    // over two links cannot both pass.
    {
        let mut inner = state.inner.write().await;
        let now = Instant::now();
        if is_peer(&inner, source) && crate::fedroute::is_duplicate(&inner, id, source_issi, source, now) {
            debug!(%source, uuid=%id, source_issi, destination, "duplicate SDS from a second peer link dropped");
            return;
        }
        crate::fedroute::note_call(&mut inner, id, source_issi, source, now);
    }
    // TEMPORARY (position debugging): SHORT_TRANSFER sometimes carries the SDS
    // user-data inline. Dump it and try a position decode here too, so a beacon
    // that never produces a separate SDS_TRANSFER frame is still caught.
    info!(uuid=%id, source_issi, destination, hex=%hex_dump(&raw), "SDS header (SHORT_TRANSFER) raw");
    // A blacklisted ISSI as sender or addressee: the SDS passes only if its
    // payload is LIP (see `SdsRoute::lip_only`), so nothing is decoded or sent
    // from the header.
    let lip_only = state.is_blocked(source_issi) || state.is_blocked(destination);
    if let Some((lat, lon, note)) = extract_sds_position(&raw).filter(|_| !lip_only) {
        let now = crate::telemetry::now_ms();
        state.telemetry.write().await.record_sds_position(source_issi, lat, lon, now, note);
        advertise_if_local(state, source, source_issi, lat, lon, now).await;
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
    // SMS Center: an individual destination that is offline everywhere (not a
    // GSSI with affiliated members) is kept for later delivery once the
    // SDS_TRANSFER carrying the payload arrives.
    let store_offline = !lip_only
        && targets.is_empty()
        && !inner.group_clients.contains_key(&destination)
        && state.sms_center.wants(destination);
    inner.sds_routes.insert(id, SdsRoute { source_client: source, targets: targets.clone(), source_issi, destination, created_at: Instant::now(), store_offline, lip_only, held_header: lip_only.then(|| raw.clone()) });
    if targets.is_empty() {
        drop(inner);
        if store_offline {
            info!(%source, uuid=%id, source_issi, destination, "SDS destination offline; SMS Center will store it");
        } else {
            warn!(%source, uuid=%id, channel="brew", source_issi, destination, lip=sds_is_lip(&raw), "SDS has no registered destination (position still tracked)");
        }
        return;
    }
    drop(inner);
    if lip_only {
        debug!(%source, uuid=%id, source_issi, destination, "SDS involving a barred ISSI held until its payload shows whether it is LIP");
        return;
    }
    let inner = state.inner.read().await;
    let txs = targets.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
    drop(inner);
    for tx in txs { let _ = tx.send(raw.clone()); }
    state.monitor.sds(id, source_issi, destination).await;
    info!(%source, uuid=%id, channel="brew", source_issi, destination, lip=sds_is_lip(&raw), target_count=targets.len(), "routed SDS header");
}

async fn handle_sds_transfer(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, raw: Vec<u8>) {
    // Look up the route (stored by the SHORT_TRANSFER header, even when the SDS
    // was undeliverable) to recover the source ISSI and any delivery targets.
    let (source_issi, txs, store_for, held) = {
        let mut inner = state.inner.write().await;
        let from_peer = is_peer(&inner, source);
        // A blacklisted ISSI is a party and the payload is not LIP (PID 0x0A
        // short report / 0x83 long report, the first user-data byte): dropped.
        let blocked_payload = matches!(inner.sds_routes.get(&id),
            Some(r) if r.source_client == source && r.lip_only && !matches!(raw.get(20), Some(0x0A) | Some(0x83)));
        if blocked_payload {
            if let Some(r) = inner.sds_routes.remove(&id) {
                info!(%source, uuid=%id, source_issi = r.source_issi, destination = r.destination, "SDS involving a barred ISSI dropped (not LIP)");
            }
            return;
        }
        match inner.sds_routes.get_mut(&id) {
            Some(route) if route.source_client == source => {
                // Store at most once per transaction, even if a client repeats the frame.
                let store_for = std::mem::take(&mut route.store_offline).then_some(route.destination);
                let (source_issi, targets) = (route.source_issi, route.targets.clone());
                let held = route.held_header.take().map(|h| (h, route.destination));
                let txs = targets.iter().filter_map(|cid| inner.clients.get(cid).map(|c| c.tx.clone())).collect::<Vec<_>>();
                (source_issi, txs, store_for, held)
            }
            // Over a second peer link this is the payload of a duplicate SDS
            // (its header was dropped), expected in a ring or mesh.
            Some(_) if from_peer => { debug!(%source, uuid=%id, "SDS_TRANSFER from non-originating peer link"); return; }
            Some(_) => { warn!(%source, uuid=%id, "SDS_TRANSFER from non-originating client"); return; }
            None => {
                // The same, once the route is gone -- removed by the
                // destination's SDS_REPORT or a dropped connection before the
                // slower copy got here, or not stored yet while its header is
                // still being routed. Taken for an orphan it would be reported
                // as ISSI 0 (map, APRS).
                let now = Instant::now();
                if from_peer && inner.recent_calls.iter().any(|((call, _), (link, at))|
                    *call == id && *link != source && now.duration_since(*at) < crate::fedroute::CALL_DEDUP_WINDOW)
                {
                    debug!(%source, uuid=%id, "SDS_TRANSFER of an SDS accepted over another peer link");
                    return;
                }
                warn!(uuid=%id, "SDS_TRANSFER without SHORT_TRANSFER (position may still decode)");
                (0u32, Vec::new(), None, None)
            }
        }
    };
    // The header of a held (LIP-only) SDS goes out just before its payload.
    if let Some((header, destination)) = held {
        for tx in &txs { let _ = tx.send(header.clone()); }
        state.monitor.sds(id, source_issi, destination).await;
    }
    for tx in &txs { let _ = tx.send(raw.clone()); }

    if let Some(destination) = store_for {
        store_offline_sds(state, source, id, source_issi, destination, &raw).await;
    }

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
        advertise_if_local(state, source, source_issi, lat, lon, now).await;
        crate::aprs::report_position(state, source_issi, lat, lon);
        info!(uuid=%id, source_issi, lat, lon, "decoded MS position from SDS");
    }
}

/// Hands an undeliverable SDS to the SMS Center and, when the sender asked
/// for an SDS-TL report, tells it the message was stored.
async fn store_offline_sds(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, source_issi: u32, destination: u32, raw: &[u8]) {
    let Ok(BrewMessage::Frame(frame)) = protocol::parse(raw) else { return };
    use crate::sms_center::{report_to_originator, status, StoreOutcome};
    match state.sms_center.store(source_issi, destination, frame.length_bits, &frame.data, crate::telemetry::now_ms()) {
        StoreOutcome::Stored(m) => {
            info!(uuid=%id, sms_id=%m.id, source_issi, destination, text=?m.text, "SMS Center: stored SDS for offline subscriber");
            state.monitor.sds(id, source_issi, destination).await;
            report_to_originator(state, source_issi, destination, &frame.data, state.sms_center.config().stored_report_status, Some(source)).await;
        }
        StoreOutcome::Duplicate(m) => {
            debug!(uuid=%id, sms_id=%m.id, source_issi, destination, "SMS Center: retransmission of an already stored SDS");
            report_to_originator(state, source_issi, destination, &frame.data, state.sms_center.config().stored_report_status, Some(source)).await;
        }
        StoreOutcome::QueueFull => {
            warn!(uuid=%id, source_issi, destination, "SMS Center: queue full, SDS not stored");
            report_to_originator(state, source_issi, destination, &frame.data, status::DEST_QUEUE_FULL, Some(source)).await;
        }
        StoreOutcome::Skipped(why) => debug!(uuid=%id, source_issi, destination, why, "SMS Center: SDS not stored"),
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
    if route.source_client == crate::sms_center::SMS_CENTER_CLIENT {
        if let Some(m) = state.sms_center.on_report(id) {
            info!(%source, uuid=%id, sms_id=%m.id, source_issi=m.source_issi, destination=m.destination, attempts=m.attempts, "SMS Center: stored SDS delivered");
        }
    }
    if let Some(tx) = tx { let _ = tx.send(raw); }
    state.monitor.sds_report(id).await;
    info!(%source, uuid=%id, source_issi=route.source_issi, destination=route.destination, "routed SDS report");
}

async fn handle_private_setup(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid, payload: CallPayload, raw: Vec<u8>) {
    // Prefer the structured CircularCall payload (parsed per Brew v1). Fall back
    // to the conservative raw source/destination pair for any peer that sends a
    // payload we could not fully structure.
    let (source_issi, destination, number, mnemonic, service, priority) = match &payload {
        CallPayload::CircularCall(c) => (c.source, c.destination, c.number.clone(), c.mnemonic.clone(), c.service, c.priority),
        other => match protocol::raw_peer_pair(other) {
            Some((s, d)) => (s, d, String::new(), None, 0, 0),
            None => {
                warn!(%source, uuid=%id, "private SETUP_REQUEST has no routable source/destination pair");
                return;
            }
        },
    };
    let ambience = service == AMBIENCE_LISTENING_SERVICE;
    let mut inner = state.inner.write().await;
    // A copy of a setup already accepted over another peer link: dropped
    // silently -- never rejected, never offered to SIP a second time.
    let now = Instant::now();
    let from_peer = is_peer(&inner, source);
    if from_peer && crate::fedroute::is_duplicate(&inner, id, source_issi, source, now) {
        debug!(%source, uuid=%id, source_issi, destination, "duplicate private SETUP_REQUEST from a second peer link dropped");
        return;
    }
    crate::fedroute::note_call(&mut inner, id, source_issi, source, now);
    // Blacklist: a blacklisted caller is refused and a blacklisted callee only
    // accepts dispatch's ambience-listening (SS-AL) call -- except an emergency
    // call, which always goes through.
    let barred = state.is_blocked(source_issi) || (state.is_blocked(destination) && !ambience);
    if barred && !is_emergency_call(state, source_issi, priority).await {
        drop(inner);
        let reason = state.bar_reason(source_issi).or_else(|| state.bar_reason(destination)).unwrap_or("");
        info!(%source, uuid=%id, source_issi, destination, reason, "private SETUP_REQUEST involving a barred ISSI rejected");
        reject_setup(state, source, id).await;
        return;
    }
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
            state.monitor.call_started(id, "private", source_issi, destination, priority).await;
            info!(%source, uuid=%id, source_issi, destination, dialled = %dialled, mnemonic=?mnemonic, "routed private SETUP_REQUEST to SIP");
        } else {
            warn!(%source, uuid=%id, destination, dialled = %dialled, "private call destination not registered (no SIP route); rejected");
            reject_setup(state, source, id).await;
        }
        return;
    };
    if target_client == source {
        drop(inner);
        if from_peer {
            // Routes still converging can briefly point back the way a setup
            // came (the far server routes it to us and we to it). Dropped
            // like any looped copy: a reject is never sent for one.
            debug!(%source, uuid=%id, destination, "private SETUP route points back to its source link (transient loop)");
            return;
        }
        // A Basestation only hands a private call to Brew when the called
        // ISSI is not registered on it, so a destination registered on the
        // caller's own connection is a stale entry.
        warn!(%source, uuid=%id, destination, "private call destination resolves back to its caller's own link; rejecting");
        reject_setup(state, source, id).await;
        return;
    }
    let peers = HashSet::from([target_client]);
    inner.calls.insert(id, ActiveCall { kind: CallKind::Private, owner: source, source_issi, destination, priority, peers: peers.clone(), started_at: std::time::Instant::now(), last_activity_ms: ActiveCall::new_activity() });
    let target = inner.clients.get(&target_client).map(|c| (c.tx.clone(), c.forward_version()));
    // For an ambience-listening setup, also learn the Brew username the target's
    // Basestation authenticated as, so we can send it the control-channel
    // AmbienceListen command that actually forces the radio to key its mic.
    let bts_username = ambience.then(|| inner.clients.get(&target_client).and_then(|c| c.username.clone())).flatten();
    drop(inner);
    if let Some((tx, version)) = target { let _ = tx.send(protocol::adapt_to_version(&raw, version).into_owned()); }
    let kind = if ambience { "ambience" } else { "private" };
    state.monitor.call_started(id, kind, source_issi, destination, priority).await;
    if ambience {
        trigger_ambience_listen(state, bts_username, destination).await;
    }
    info!(%source, uuid=%id, source_issi, destination, ambience, mnemonic=?mnemonic, "routed private SETUP_REQUEST");
}

/// Fires the Basestation Control `AmbienceListen` command for a relayed
/// ambience-listening private setup, so the target radio auto-keys its mic
/// while the normal call plumbing carries the speech path. Best-effort: if the
/// target's Basestation has no control session (or none could be resolved), the
/// call still proceeds as an ordinary relayed private call. Spawned so the
/// command's response wait never blocks routing.
async fn trigger_ambience_listen(state: &Arc<AppState>, bts_username: Option<String>, issi: u32) {
    let Some(bts) = bts_username else {
        warn!(destination = issi, "ambience-listen setup: target Basestation has no known control identity; relayed as an ordinary private call only");
        return;
    };
    let state = state.clone();
    tokio::spawn(async move {
        let cmd = crate::control::ControlCommand::AmbienceListen { issi, enable: true };
        match crate::control::send_command(&state, &bts, cmd).await {
            Ok(resp) => info!(bts = %bts, destination = issi, ?resp, "sent AmbienceListen to Basestation control"),
            Err(e) => warn!(bts = %bts, destination = issi, ?e, "AmbienceListen control command not delivered (Basestation may not support it)"),
        }
    });
}

/// Answers a private SETUP_REQUEST this server cannot route with
/// CALL_SETUP_REJECT, so the calling radio is released at once ("called party
/// not reachable") instead of waiting out its own setup timer. Sent back on
/// the connection the SETUP came in on -- a Basestation, or a peer link, whose
/// server relays it to the caller exactly like a callee's own reject
/// (route_private_control -> end_call). No call is recorded, so a
/// CALL_RELEASE the caller's side may still send for it is ignored as one for
/// an unknown call.
async fn reject_setup(state: &Arc<AppState>, source: ClientId, id: uuid::Uuid) {
    let tx = state.inner.read().await.clients.get(&source).map(|c| c.tx.clone());
    if let Some(tx) = tx {
        let _ = tx.send(protocol::build_call_cause(CALL_SETUP_REJECT, &id, protocol::CAUSE_CALLED_PARTY_NOT_REACHABLE));
    }
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
    if !allowed {
        // A peer link still sending the stream of a duplicate it was not
        // accepted from (ring, mesh): expected, so not worth a warning.
        if is_peer(&inner, source) { debug!(%source, uuid=%id, "{kind} frame from non-participant peer link"); }
        else { warn!(%source, uuid=%id, "{kind} frame from non-participant"); }
        return None;
    }
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
    // A copy of the call's setup still on its way over another peer link must
    // not bring it back as a zombie that holds the group floor.
    crate::fedroute::note_call(&mut inner, id, call.source_issi, call.owner, Instant::now());
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

/// Whether `client` is a federation peer link (only such traffic is checked
/// for duplicates).
fn is_peer(inner: &Inner, client: ClientId) -> bool {
    inner.clients.get(&client).is_some_and(|c| c.mode == ClientMode::Peer)
}

/// Late entry: `client` has just joined `groups` (a registration that keeps
/// its ISSI's groups, or an affiliation). Every group call to one of those
/// groups that is running now and does not reach `client` yet takes it on as
/// a participant, so its remaining voice frames and its end reach `client`
/// too. Returns the GROUP_TX to send it per such call (the one that set the
/// call up, in `client`'s layout; a call without one, such as a SIP-bridged
/// one, gets a synthesized header). The caller sends them while it still
/// holds the lock, so they go out ahead of any further frame of those calls.
///
/// Federation peer links are left out: they learn of calls by route and
/// prune duplicates themselves.
fn late_entry(inner: &mut Inner, client: ClientId, groups: &HashSet<u32>) -> Vec<(mpsc::UnboundedSender<Vec<u8>>, Vec<u8>)> {
    if groups.is_empty() || is_peer(inner, client) {
        return Vec::new();
    }
    let Some(c) = inner.clients.get(&client) else { return Vec::new() };
    let (tx, version) = (c.tx.clone(), c.forward_version());
    // A dispatch console is told an emergency call by its priority, as at call
    // set-up (`handle_group_tx`) and when the alarm is raised mid-call.
    let console = inner.consoles.contains(&client);
    let Inner { calls, group_tx_raw, .. } = inner;
    group_tx_raw.retain(|call_id, _| calls.contains_key(call_id));
    let mut out = Vec::new();
    for (id, call) in calls.iter_mut() {
        if call.kind != CallKind::Group || !groups.contains(&call.destination) {
            continue;
        }
        if call.owner == client || call.peers.contains(&client) {
            continue;
        }
        call.peers.insert(client);
        let mut header = group_tx_raw
            .get(id)
            .cloned()
            .unwrap_or_else(|| protocol::build_group_tx(id, call.source_issi, call.destination, call.priority));
        if console && call.priority >= EMERGENCY_PRIORITY {
            header = with_emergency_priority(&header);
        }
        out.push((tx.clone(), protocol::adapt_to_version(&header, version).into_owned()));
        info!(%client, uuid=%id, gssi=call.destination, src_issi=call.source_issi, "late entry: joined running group call");
    }
    out
}

async fn handle_subscriber(state: &Arc<AppState>, source: ClientId, msg: SubscriberMessage) {
    let mut inner = state.inner.write().await;
    if inner.fed.links.contains_key(&source) {
        // A loop-safe link carries registrations as route adverts only.
        warn!(%source, issi=msg.issi, msg_type=msg.msg_type, "SUB message on a loop-safe peer link ignored");
        return;
    }
    // The connecting client's advertised mode (Terminal/Basestation), used to
    // tag the subscriber registration so MS-registration counts can exclude
    // Basestation (Basestation gateway) registrations, which are not an MS.
    let source_mode = inner.clients.get(&source).map(|c| c.mode).unwrap_or_default();
    // Set below when this message is a Terminal-mode register/deregister, so
    // it can be logged to the dashboard's registration log once `inner` is
    // released (mirrors how position decoding logs via `state.telemetry`
    // outside of the `inner` lock elsewhere in this module).
    let mut ms_reg_event: Option<&'static str> = None;
    // Whatever registers here -- a Basestation, a Terminal or a legacy peer
    // link, for everything behind it -- is a local registration, stamped with
    // this server's registration clock (see `fedroute`).
    let old = inner.subscribers.get(&msg.issi).cloned();
    let local = |inner: &mut Inner, groups| Subscriber {
        client_id: source, groups, mode: source_mode,
        route: crate::fedroute::Route { reg: inner.fed.tick(), path: Vec::new() },
    };
    let new = match msg.msg_type {
        SUB_REGISTER | SUB_REREGISTER => {
            info!(%source, issi=msg.issi, mode=source_mode.as_str(), "subscriber registered");
            if source_mode == ClientMode::Terminal { ms_reg_event = Some("register"); }
            Some(local(&mut inner, old.as_ref().map(|o| o.groups.clone()).unwrap_or_default()))
        }
        SUB_DEREGISTER => {
            // Only the connection the ISSI is registered on can deregister it
            // (and nobody else's DEREGISTER is passed on to peers).
            let Some(sub) = old.as_ref().filter(|o| o.client_id == source) else { return };
            info!(%source, issi=msg.issi, mode=sub.mode.as_str(), "subscriber deregistered");
            if sub.mode == ClientMode::Terminal { ms_reg_event = Some("deregister"); }
            None
        }
        SUB_AFFILIATE => {
            let mut sub = match old.clone() {
                Some(o) if o.client_id != source => { warn!(%source, issi=msg.issi, "affiliation from non-owner"); return; }
                Some(o) => o,
                None => local(&mut inner, HashSet::new()),
            };
            for gssi in &msg.groups {
                sub.groups.insert(*gssi);
                info!(%source, issi=msg.issi, gssi, "subscriber affiliated");
            }
            Some(sub)
        }
        SUB_DEAFFILIATE => {
            let Some(mut sub) = old.clone().filter(|o| o.client_id == source) else { return };
            for gssi in &msg.groups {
                sub.groups.remove(gssi);
                info!(%source, issi=msg.issi, gssi, "subscriber deaffiliated");
            }
            Some(sub)
        }
        _ => { debug!(%source, msg_type=msg.msg_type, "unknown subscriber message"); return; }
    };
    // From here on call/SDS routing needs no federation-specific code at all:
    // it resolves via `inner.subscribers`/`inner.group_clients`. Every peer
    // link is told of the change (`fedroute::publish`): a loop-safe one with a
    // route advert, a legacy one with the SUB messages it lacks -- never
    // echoing anything back to the link it came from.
    let deregistered = new.is_none();
    crate::fedroute::set_effective(&mut inner, msg.issi, new);
    if deregistered {
        // Still reachable elsewhere, if a loop-safe peer offers it.
        crate::fedroute::reroute(&mut inner, msg.issi);
    }
    let registered = crate::fedroute::publish(&inner, msg.issi, old.as_ref());
    // Late entry: a registration (which keeps the ISSI's groups) or an
    // affiliation can put this connection into a group that has a call
    // running right now. It was not a target when that call was set up, so
    // hand it the call's GROUP_TX and add it to the call: the rest of the
    // over reaches it instead of only the next one. All of the ISSI's groups
    // are checked: calls to groups it was already in reach this connection
    // already, so only the new ones add anything. (The owner check is
    // defensive: a non-owner's affiliation was turned away above.)
    let joined: HashSet<u32> = match msg.msg_type {
        SUB_REGISTER | SUB_REREGISTER | SUB_AFFILIATE => inner
            .subscribers
            .get(&msg.issi)
            .filter(|s| s.client_id == source)
            .map(|s| s.groups.clone())
            .unwrap_or_default(),
        _ => HashSet::new(),
    };
    // Sent before the lock is released: a voice frame of the call can only be
    // routed to the new participant under the lock, so it cannot overtake
    // the header (an unbounded send never blocks).
    for (tx, packet) in late_entry(&mut inner, source, &joined) {
        let _ = tx.send(packet);
    }
    drop(inner);
    // Log Terminal-mode (actual MS) registration lifecycle events to the same
    // dashboard registration log Basestation telemetry registrations use, so
    // an MS registering directly over the Brew protocol is visible there too.
    if let Some(kind) = ms_reg_event {
        state.telemetry.write().await.record_brew_registration(msg.issi, kind);
    }
    if registered {
        subscriber_registered(state, msg.issi);
    }
}

/// An ISSI (re)registered somewhere on the Brew network -- here or at a peer
/// server: the SMS Center delivers whatever it holds for it.
pub(crate) fn subscriber_registered(state: &Arc<AppState>, issi: u32) {
    state.sms_center.note_known(issi);
    if state.sms_center.has_pending_for(issi) {
        info!(issi, "SMS Center: subscriber back online, delivering stored SDS");
        tokio::spawn(crate::sms_center::deliver_pending(state.clone(), issi, true));
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

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::{protocol::ConnVersion, state::{Client, ClientMode}};

    /// A Basestation connection (so registrations are not relayed anywhere)
    /// that announced `version`, plus the queue of what it is sent.
    async fn connect(state: &AppState, version: ConnVersion) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = uuid::Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode: ClientMode::Basestation, version, version_announced: true, remote_addr: None, connected_at_ms: 0, username: None });
        (id, rx)
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn with_mnemonic(mut wire: Vec<u8>, name: &[u8]) -> Vec<u8> {
        let mut mnem = vec![0x00u8, (name.len() * 8) as u8];
        mnem.extend_from_slice(name);
        mnem.resize(34, 0);
        wire.extend_from_slice(&mnem);
        wire
    }

    #[tokio::test]
    async fn blacklisted_issi_cannot_transmit_to_a_group() {
        let state = AppState::for_test();
        let (talker, _talker_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        state.set_blocked(1001, true);
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), talker, protocol::build_group_tx(&id, 1001, 91, 0)).await;
        assert!(drain(&mut listener_rx).is_empty());
        assert!(state.inner.read().await.calls.is_empty());
        // Another ISSI on the same Basestation is unaffected, and so is the ISSI once unblocked.
        let id2 = uuid::Uuid::new_v4();
        let tx = protocol::build_group_tx(&id2, 1002, 91, 0);
        handle_packet(state.clone(), talker, tx.clone()).await;
        assert_eq!(drain(&mut listener_rx), vec![tx]);
        state.set_blocked(1001, false);
        let id3 = uuid::Uuid::new_v4();
        handle_packet(state.clone(), talker, protocol::build_group_tx(&id3, 1001, 92, 0)).await;
        assert_eq!(drain(&mut listener_rx).len(), 1);
    }

    #[tokio::test]
    async fn an_enforced_allowlist_bars_unlisted_issis_except_in_an_emergency() {
        let state = AppState::for_test();
        let (talker, _talker_rx) = connect(&state, ConnVersion::V0).await;
        let (listener, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_AFFILIATE, 6001, &[91])).await;
        state.set_allowed(1001, true);

        // One over on group 91 from `issi`, ended at once; what the listener received.
        let over = |issi: u32, priority: u8| {
            let (state, talker) = (state.clone(), talker);
            async move {
                let id = uuid::Uuid::new_v4();
                handle_packet(state.clone(), talker, protocol::build_group_tx(&id, issi, 91, priority)).await;
                handle_packet(state, talker, protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
            }
        };

        // Not enforced yet: everyone talks.
        over(2002, 0).await;
        assert_eq!(drain(&mut listener_rx).len(), 2, "GROUP_TX and GROUP_IDLE");

        // Enforced: the listed ISSI talks, the unlisted one is dropped, the
        // unlisted one's emergency call still goes through.
        state.set_allowlist_enabled(true);
        over(1001, 0).await;
        assert_eq!(drain(&mut listener_rx).len(), 2);
        over(2002, 0).await;
        assert!(drain(&mut listener_rx).is_empty());
        over(2002, EMERGENCY_PRIORITY).await;
        assert_eq!(drain(&mut listener_rx).len(), 2);

        // A private call to a listed ISSI from an unlisted one is rejected at the caller.
        let (caller, mut caller_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_REGISTER, 1001, &[])).await;
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), caller, protocol::build_circular_call_setup(&id, 2002, 1001, 0)).await;
        assert_eq!(drain(&mut caller_rx), vec![setup_reject(&id)]);
        assert!(drain(&mut listener_rx).is_empty());

        // The blacklist still bars a listed ISSI.
        state.set_blocked(1001, true);
        over(1001, 0).await;
        assert!(drain(&mut listener_rx).is_empty());
    }

    #[tokio::test]
    async fn emergency_group_calls_are_pushed_to_dispatch_consoles_only() {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.fallback_broadcast_when_no_affiliations = false;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let (bs, _bs_rx) = connect(&state, ConnVersion::V0).await;
        let (listener, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        let (console, mut console_rx) = connect(&state, ConnVersion::V0).await;
        let (_other, mut other_rx) = connect(&state, ConnVersion::V0).await;
        // Only `listener` is affiliated to group 91; `console` is a dispatch console listening to nothing.
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_AFFILIATE, 6001, &[91])).await;
        state.inner.write().await.consoles.insert(console);

        // An ordinary call reaches only the affiliated Basestation.
        let normal = protocol::build_group_tx(&uuid::Uuid::new_v4(), 4013, 91, 0);
        handle_packet(state.clone(), bs, normal.clone()).await;
        assert_eq!(drain(&mut listener_rx), vec![normal]);
        assert!(drain(&mut console_rx).is_empty());

        // An emergency call also reaches the console, and its voice and end follow.
        let id = uuid::Uuid::new_v4();
        let emergency = protocol::build_group_tx(&id, 4013, 91, EMERGENCY_PRIORITY);
        handle_packet(state.clone(), bs, emergency.clone()).await;
        assert_eq!(drain(&mut console_rx), vec![emergency.clone()]);
        // The listener first hears the ordinary call being pre-empted, then the emergency call.
        assert_eq!(drain(&mut listener_rx).last(), Some(&emergency));
        assert!(drain(&mut other_rx).is_empty(), "not pushed to ordinary Basestations");
        let voice = protocol::build_traffic_frame(&id, &[0x11; protocol::ACELP_CODED_FRAME_BYTES], &[0x22; protocol::ACELP_CODED_FRAME_BYTES]);
        handle_packet(state.clone(), bs, voice.clone()).await;
        assert_eq!(drain(&mut console_rx), vec![voice]);
        let idle = protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0);
        handle_packet(state.clone(), bs, idle.clone()).await;
        assert_eq!(drain(&mut console_rx), vec![idle]);
    }

    /// FlowStation forwards a radio's emergency call to Brew at priority 0; the
    /// emergency alarm over telemetry is what marks it.
    #[tokio::test]
    async fn an_alarmed_issis_priority_0_call_is_an_emergency_call() {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.fallback_broadcast_when_no_affiliations = false;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let (bs, _bs_rx) = connect(&state, ConnVersion::V0).await;
        let (listener, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        let (console, mut console_rx) = connect(&state, ConnVersion::V0).await;
        let (peer, mut peer_rx) = connect_as(&state, ClientMode::Peer).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_AFFILIATE, 6001, &[91])).await;
        {
            let mut inner = state.inner.write().await;
            inner.consoles.insert(console);
            // A legacy peer link that joined group 91.
            inner.group_clients.entry(91).or_default().insert(peer);
        }
        state.set_blocked(4013, true);
        drain(&mut peer_rx); // the registration messages a legacy peer link was sent
        let mut t = state.telemetry.write().await;
        t.add_test_station("bts2", None, "bts2");
        t.stations.get_mut("bts2").unwrap().emergencies.insert(4013);
        drop(t);

        let id = uuid::Uuid::new_v4();
        let wire = protocol::build_group_tx(&id, 4013, 91, 0);
        handle_packet(state.clone(), bs, wire.clone()).await;
        // Blacklisted, yet it goes through: the console and the peer see it raised to priority 15,
        // the ordinary Basestation as sent.
        let raised = with_emergency_priority(&wire);
        assert_eq!(drain(&mut console_rx), vec![raised.clone()]);
        assert_eq!(drain(&mut peer_rx), vec![raised]);
        assert_eq!(drain(&mut listener_rx), vec![wire]);
        assert_eq!(state.inner.read().await.calls[&id].priority, EMERGENCY_PRIORITY);
        // The ribbon lists it once, with the called group.
        let axum::Json(list) = crate::dashboard::emergencies_snapshot(axum::extract::State(state.clone())).await;
        assert_eq!(list.len(), 1);
        assert!(list[0].issi == 4013 && list[0].destination == Some(91) && list[0].bts.as_deref() == Some("bts2"));
    }

    #[tokio::test]
    async fn emergency_group_calls_go_to_every_peer_even_one_not_routing_the_group() {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.fallback_broadcast_when_no_affiliations = false;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let (bs, _bs_rx) = connect(&state, ConnVersion::V0).await;
        let (listener, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        let (_peer_a, mut peer_a_rx) = connect_as(&state, ClientMode::Peer).await;
        let (_peer_b, mut peer_b_rx) = connect_as(&state, ClientMode::Peer).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_AFFILIATE, 6001, &[91])).await;
        drain(&mut peer_a_rx);
        drain(&mut peer_b_rx);

        // An ordinary call stays off peers that do not route the group.
        let ordinary = protocol::build_group_tx(&uuid::Uuid::new_v4(), 4013, 91, 0);
        handle_packet(state.clone(), bs, ordinary).await;
        assert!(drain(&mut peer_a_rx).is_empty() && drain(&mut peer_b_rx).is_empty());
        drain(&mut listener_rx);

        // An emergency call goes to all of them, at priority 15, with its voice and its end.
        let id = uuid::Uuid::new_v4();
        let em = protocol::build_group_tx(&id, 4014, 91, EMERGENCY_PRIORITY);
        handle_packet(state.clone(), bs, em.clone()).await;
        assert_eq!(drain(&mut peer_a_rx).last(), Some(&em));
        assert_eq!(drain(&mut peer_b_rx).last(), Some(&em));
        let voice = protocol::build_traffic_frame(&id, &[0x11; protocol::ACELP_CODED_FRAME_BYTES], &[0x22; protocol::ACELP_CODED_FRAME_BYTES]);
        handle_packet(state.clone(), bs, voice.clone()).await;
        assert_eq!(drain(&mut peer_a_rx), vec![voice.clone()]);
        assert_eq!(drain(&mut peer_b_rx), vec![voice]);
    }

    #[tokio::test]
    async fn an_alarm_raised_during_a_call_upgrades_it_for_consoles() {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.fallback_broadcast_when_no_affiliations = false;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let (bs, _bs_rx) = connect(&state, ConnVersion::V0).await;
        let (listener, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        let (console, mut console_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        handle_packet(state.clone(), listener, protocol::build_subscriber_message(SUB_AFFILIATE, 6001, &[91])).await;
        state.inner.write().await.consoles.insert(console);

        // The call starts before the alarm reaches us: an ordinary call.
        let id = uuid::Uuid::new_v4();
        let wire = protocol::build_group_tx(&id, 4013, 91, 0);
        handle_packet(state.clone(), bs, wire.clone()).await;
        assert!(drain(&mut console_rx).is_empty());
        drain(&mut listener_rx);

        // The alarm arrives: the console joins the call, with the raised header first, then its voice.
        let (_peer, mut peer_rx) = connect_as(&state, ClientMode::Peer).await;
        drain(&mut peer_rx);
        emergency_raised(&state, 4013).await;
        assert_eq!(drain(&mut console_rx), vec![protocol::build_group_tx(&id, 4013, 91, EMERGENCY_PRIORITY)]);
        assert_eq!(drain(&mut peer_rx), vec![protocol::build_group_tx(&id, 4013, 91, EMERGENCY_PRIORITY)], "a peer joins too");
        assert!(drain(&mut listener_rx).is_empty(), "ordinary Basestations are not told again");
        let voice = protocol::build_traffic_frame(&id, &[0x11; protocol::ACELP_CODED_FRAME_BYTES], &[0x22; protocol::ACELP_CODED_FRAME_BYTES]);
        handle_packet(state.clone(), bs, voice.clone()).await;
        assert_eq!(drain(&mut console_rx), vec![voice]);
        assert_eq!(state.inner.read().await.calls[&id].priority, EMERGENCY_PRIORITY);
    }

    #[tokio::test]
    async fn emergency_calls_of_a_blacklisted_issi_still_work() {
        let state = AppState::for_test();
        let (bs, mut bs_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut listener_rx) = connect(&state, ConnVersion::V0).await;
        let (callee, mut callee_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), callee, protocol::build_subscriber_message(SUB_REGISTER, 5001, &[])).await;
        state.set_blocked(4013, true);

        // Emergency group call (priority 15): delivered, and shown as an emergency.
        let id = uuid::Uuid::new_v4();
        let tx = protocol::build_group_tx(&id, 4013, 91, EMERGENCY_PRIORITY);
        handle_packet(state.clone(), bs, tx.clone()).await;
        assert_eq!(drain(&mut listener_rx), vec![tx]);
        let axum::Json(list) = crate::dashboard::emergencies_snapshot(axum::extract::State(state.clone())).await;
        assert!(list.iter().any(|e| e.issi == 4013 && e.kind == "call" && e.destination == Some(91) && e.blacklisted));

        // Ordinary priority from the same ISSI is still dropped.
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), bs, protocol::build_group_tx(&id, 4013, 91, 0)).await;
        assert!(drain(&mut listener_rx).is_empty());

        // Emergency private call from it is routed, not rejected.
        drain(&mut callee_rx);
        let id = uuid::Uuid::new_v4();
        let mut setup = protocol::build_circular_call_setup(&id, 4013, 5001, EMERGENCY_PRIORITY);
        setup[2 + 16 + 4 + 4 + 32] = EMERGENCY_PRIORITY;
        handle_packet(state.clone(), bs, setup).await;
        assert_eq!(drain(&mut callee_rx).len(), 1);
        assert!(drain(&mut bs_rx).is_empty(), "no reject");
    }

    #[tokio::test]
    async fn blacklisted_issi_private_calls_are_rejected_except_ambience_listening() {
        let state = AppState::for_test();
        let (bs, mut bs_rx) = connect(&state, ConnVersion::V0).await;
        let (dispatch, mut dispatch_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), bs, protocol::build_subscriber_message(SUB_REGISTER, 4013, &[])).await;
        state.set_blocked(4013, true);

        // From the blacklisted ISSI: refused.
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), bs, protocol::build_circular_call_setup(&id, 4013, 5001, 0)).await;
        assert_eq!(drain(&mut bs_rx), vec![setup_reject(&id)]);

        // To it, an ordinary call: refused back to the caller.
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), dispatch, protocol::build_circular_call_setup(&id, 5001, 4013, 0)).await;
        assert_eq!(drain(&mut dispatch_rx), vec![setup_reject(&id)]);
        assert!(drain(&mut bs_rx).is_empty());

        // To it, dispatch ambience listening: goes through.
        let id = uuid::Uuid::new_v4();
        let mut al = protocol::build_circular_call_setup(&id, 5001, 4013, 0);
        al[59] = AMBIENCE_LISTENING_SERVICE;
        handle_packet(state.clone(), dispatch, al).await;
        assert_eq!(drain(&mut bs_rx).len(), 1, "the AL setup reaches the radio's Basestation");
        assert!(drain(&mut dispatch_rx).is_empty());

        // But a blacklisted ISSI cannot use AL to call out.
        let id = uuid::Uuid::new_v4();
        let mut al = protocol::build_circular_call_setup(&id, 4013, 5001, 0);
        al[59] = AMBIENCE_LISTENING_SERVICE;
        handle_packet(state.clone(), bs, al).await;
        assert_eq!(drain(&mut bs_rx), vec![setup_reject(&id)]);
    }

    #[tokio::test]
    async fn sds_of_a_blacklisted_issi_passes_only_when_it_is_lip() {
        let state = AppState::for_test();
        let (bs, mut bs_rx) = connect(&state, ConnVersion::V0).await;
        let (dest, mut dest_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), dest, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        handle_packet(state.clone(), bs, protocol::build_subscriber_message(SUB_REGISTER, 4013, &[])).await;
        drain(&mut dest_rx);
        drain(&mut bs_rx);
        state.set_blocked(4013, true);

        // Text from the blacklisted ISSI: dropped, header never delivered.
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), bs, protocol::build_short_transfer(&id, 4013, 6002)).await;
        handle_packet(state.clone(), bs, protocol::build_sds_transfer_frame(&id, 16, &[0x82, 0, 0, 0, b'h', b'i'])).await;
        assert!(drain(&mut dest_rx).is_empty());

        // A LIP report (PID 0x0A): delivered, header first.
        let id = uuid::Uuid::new_v4();
        let header = protocol::build_short_transfer(&id, 4013, 6002);
        let lip = protocol::build_sds_transfer_frame(&id, 64, &[0x0A, 1, 2, 3, 4, 5, 6, 7]);
        handle_packet(state.clone(), bs, header.clone()).await;
        assert!(drain(&mut dest_rx).is_empty(), "held until the payload shows what it is");
        handle_packet(state.clone(), bs, lip.clone()).await;
        assert_eq!(drain(&mut dest_rx), vec![header, lip]);

        // Text TO the blacklisted ISSI from someone else: dropped, no route kept.
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), dest, protocol::build_short_transfer(&id, 6002, 4013)).await;
        handle_packet(state.clone(), dest, protocol::build_sds_transfer_frame(&id, 16, &[0x82, 0, 0, 0, b'h', b'i'])).await;
        assert!(drain(&mut bs_rx).is_empty());
        assert!(!state.inner.read().await.sds_routes.contains_key(&id));

        // A LIP request to it (dispatch asking for its position): delivered.
        let id = uuid::Uuid::new_v4();
        let header = protocol::build_short_transfer(&id, 6002, 4013);
        let request = protocol::build_sds_transfer_frame(&id, 32, &[0x0A, 0x40, 0, 0]);
        handle_packet(state.clone(), dest, header.clone()).await;
        handle_packet(state.clone(), dest, request.clone()).await;
        assert_eq!(drain(&mut bs_rx), vec![header, request]);
    }

    #[tokio::test]
    async fn group_tx_reaches_each_listener_in_its_own_version() {
        let state = AppState::for_test();
        let (talker, _talker_rx) = connect(&state, ConnVersion::V1).await;
        let (_, mut v0_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut v1_rx) = connect(&state, ConnVersion::V1).await;
        let id = uuid::Uuid::new_v4();
        let v0_tx = protocol::build_group_tx(&id, 1001, 91, 0);
        let v1_tx = with_mnemonic(v0_tx.clone(), b"BOB");

        handle_packet(state.clone(), talker, v1_tx.clone()).await;
        assert_eq!(drain(&mut v0_rx), vec![v0_tx]);
        assert_eq!(drain(&mut v1_rx), vec![v1_tx]);
    }

    #[tokio::test]
    async fn private_setup_reaches_a_v0_callee_without_mnemonic() {
        let state = AppState::for_test();
        let (caller, _caller_rx) = connect(&state, ConnVersion::V1).await;
        let (v0_site, mut v0_rx) = connect(&state, ConnVersion::V0).await;
        let (v1_site, mut v1_rx) = connect(&state, ConnVersion::V1).await;
        handle_packet(state.clone(), v0_site, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        handle_packet(state.clone(), v1_site, protocol::build_subscriber_message(SUB_REGISTER, 6003, &[])).await;

        let to_v0 = uuid::Uuid::new_v4();
        let v0_setup = protocol::build_circular_call_setup(&to_v0, 5001, 6002, 0);
        handle_packet(state.clone(), caller, with_mnemonic(v0_setup.clone(), b"CTRL")).await;
        assert_eq!(drain(&mut v0_rx), vec![v0_setup]);

        let to_v1 = uuid::Uuid::new_v4();
        let v1_setup = with_mnemonic(protocol::build_circular_call_setup(&to_v1, 5001, 6003, 0), b"CTRL");
        handle_packet(state.clone(), caller, v1_setup.clone()).await;
        assert_eq!(drain(&mut v1_rx), vec![v1_setup]);
    }

    /// FlowStation without digest credentials: no X-Brew-Version anywhere and
    /// never a mnemonic of its own, so v0 here -- but it parses the v1 tail
    /// and shows the talker name, so it keeps getting it.
    #[tokio::test]
    async fn a_connection_that_announced_no_version_keeps_the_mnemonic() {
        let state = AppState::for_test();
        let (talker, _talker_rx) = connect(&state, ConnVersion::V1).await;
        let (tx, mut silent_rx) = mpsc::unbounded_channel();
        let silent = uuid::Uuid::new_v4();
        state.inner.write().await.clients.insert(silent, Client { tx, mode: ClientMode::Basestation, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
        handle_packet(state.clone(), silent, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;

        let v1_tx = with_mnemonic(protocol::build_group_tx(&uuid::Uuid::new_v4(), 1001, 91, 0), b"BOB");
        handle_packet(state.clone(), talker, v1_tx.clone()).await;
        assert_eq!(drain(&mut silent_rx), vec![v1_tx]);

        let v1_setup = with_mnemonic(protocol::build_circular_call_setup(&uuid::Uuid::new_v4(), 5001, 6002, 0), b"CTRL");
        handle_packet(state.clone(), talker, v1_setup.clone()).await;
        assert_eq!(drain(&mut silent_rx), vec![v1_setup]);
    }

    fn setup_reject(id: &uuid::Uuid) -> Vec<u8> {
        protocol::build_call_cause(CALL_SETUP_REJECT, id, protocol::CAUSE_CALLED_PARTY_NOT_REACHABLE)
    }

    #[tokio::test]
    async fn unroutable_private_setup_is_rejected_to_the_caller() {
        let state = AppState::for_test();
        let (caller, mut caller_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut other_rx) = connect(&state, ConnVersion::V0).await;
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), caller, protocol::build_circular_call_setup(&id, 5001, 7777, 0)).await;
        assert_eq!(drain(&mut caller_rx), vec![setup_reject(&id)]);
        assert!(drain(&mut other_rx).is_empty());
        assert!(state.inner.read().await.calls.is_empty(), "a rejected setup leaves no call behind");
        // The caller's side releasing it afterwards is harmless.
        handle_packet(state.clone(), caller, protocol::build_call_cause(CALL_RELEASE, &id, 3)).await;
        assert!(drain(&mut other_rx).is_empty());
    }

    #[tokio::test]
    async fn private_setup_back_to_its_own_link_is_rejected() {
        let state = AppState::for_test();
        let (link, mut link_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), link, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), link, protocol::build_circular_call_setup(&id, 5001, 6002, 0)).await;
        assert_eq!(drain(&mut link_rx), vec![setup_reject(&id)]);
        assert!(state.inner.read().await.calls.is_empty());
    }

    /// A GROUP_TX from `talker`; with no affiliations recorded, every other
    /// connection hears it (`fallback_broadcast_when_no_affiliations`).
    async fn group_call(state: &Arc<AppState>, talker: ClientId) -> uuid::Uuid {
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), talker, protocol::build_group_tx(&id, 1001, 91, 0)).await;
        id
    }

    #[tokio::test]
    async fn talker_dropping_ends_the_group_call_for_its_listeners() {
        let state = AppState::for_test();
        let (talker, _talker_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut a_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut b_rx) = connect(&state, ConnVersion::V0).await;
        let id = group_call(&state, talker).await;
        drain(&mut a_rx);
        drain(&mut b_rx);

        state.cleanup_client(talker).await;
        let idle = protocol::build_call_cause(CALL_GROUP_IDLE, &id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION);
        assert_eq!(drain(&mut a_rx), vec![idle.clone()]);
        assert_eq!(drain(&mut b_rx), vec![idle]);
        let inner = state.inner.read().await;
        assert!(inner.calls.is_empty() && inner.group_floor.is_empty());
    }

    #[tokio::test]
    async fn listener_dropping_keeps_the_group_call_for_the_others() {
        let state = AppState::for_test();
        let (talker, mut talker_rx) = connect(&state, ConnVersion::V0).await;
        let (gone, _gone_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut stays_rx) = connect(&state, ConnVersion::V0).await;
        let id = group_call(&state, talker).await;
        drain(&mut stays_rx);

        state.cleanup_client(gone).await;
        assert!(drain(&mut talker_rx).is_empty(), "nobody is told about a departing listener");
        assert!(drain(&mut stays_rx).is_empty());
        let voice = protocol::build_traffic_frame(&id, &[0x11; protocol::ACELP_CODED_FRAME_BYTES], &[0x22; protocol::ACELP_CODED_FRAME_BYTES]);
        handle_packet(state.clone(), talker, voice.clone()).await;
        assert_eq!(drain(&mut stays_rx), vec![voice], "the remaining listener keeps hearing the talker");
        assert_eq!(state.inner.read().await.group_floor.get(&91), Some(&id));
    }

    #[tokio::test]
    async fn private_call_party_dropping_releases_the_other() {
        let state = AppState::for_test();
        let (caller, mut caller_rx) = connect(&state, ConnVersion::V0).await;
        let (callee, mut callee_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), callee, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        handle_packet(state.clone(), caller, protocol::build_subscriber_message(SUB_REGISTER, 5001, &[])).await;
        let to_callee = uuid::Uuid::new_v4();
        handle_packet(state.clone(), caller, protocol::build_circular_call_setup(&to_callee, 5001, 6002, 0)).await;
        let to_caller = uuid::Uuid::new_v4();
        handle_packet(state.clone(), callee, protocol::build_circular_call_setup(&to_caller, 6002, 5001, 0)).await;
        drain(&mut caller_rx);
        drain(&mut callee_rx);

        // Whichever side placed the call, the party left behind is released.
        state.cleanup_client(callee).await;
        let mut got = drain(&mut caller_rx);
        got.sort();
        let mut want = [to_callee, to_caller].map(|id| protocol::build_call_cause(CALL_RELEASE, &id, protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION)).to_vec();
        want.sort();
        assert_eq!(got, want);
        assert!(state.inner.read().await.calls.is_empty());
    }

    /// A connection in `mode` (e.g. a federation peer link), negotiated v0.
    async fn connect_as(state: &AppState, mode: ClientMode) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = uuid::Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: true, remote_addr: None, connected_at_ms: 0, username: None });
        (id, rx)
    }

    fn voice(id: &uuid::Uuid) -> Vec<u8> {
        protocol::build_traffic_frame(id, &[0x11; protocol::ACELP_CODED_FRAME_BYTES], &[0x22; protocol::ACELP_CODED_FRAME_BYTES])
    }

    #[tokio::test]
    async fn group_tx_over_a_second_peer_link_is_dropped() {
        let state = AppState::for_test();
        let (first, _first_rx) = connect_as(&state, ClientMode::Peer).await;
        let (second, mut second_rx) = connect_as(&state, ClientMode::Peer).await;
        let (_, mut site_rx) = connect(&state, ConnVersion::V0).await;
        let id = uuid::Uuid::new_v4();
        let group_tx = protocol::build_group_tx(&id, 1001, 91, 0);
        handle_packet(state.clone(), first, group_tx.clone()).await;
        assert_eq!(drain(&mut site_rx), vec![group_tx.clone()]);
        drain(&mut second_rx);

        // The same transmission back over the other link: not forwarded
        // again, and it does not take the call over.
        handle_packet(state.clone(), second, group_tx.clone()).await;
        assert!(drain(&mut site_rx).is_empty());
        assert_eq!(state.inner.read().await.calls[&id].owner, first);
        handle_packet(state.clone(), second, voice(&id)).await;
        assert!(drain(&mut site_rx).is_empty(), "nor is its voice stream");
        handle_packet(state.clone(), first, voice(&id)).await;
        assert_eq!(drain(&mut site_rx), vec![voice(&id)]);

        // Once the call ended, a late copy does not resurrect it.
        let idle = protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0);
        handle_packet(state.clone(), first, idle.clone()).await;
        assert_eq!(drain(&mut site_rx), vec![idle]);
        handle_packet(state.clone(), second, group_tx).await;
        assert!(drain(&mut site_rx).is_empty());
        let inner = state.inner.read().await;
        assert!(inner.calls.is_empty() && inner.group_floor.is_empty());
    }

    #[tokio::test]
    async fn talker_change_over_another_peer_link_is_accepted() {
        let state = AppState::for_test();
        let (first, _first_rx) = connect_as(&state, ClientMode::Peer).await;
        let (second, _second_rx) = connect_as(&state, ClientMode::Peer).await;
        let (_, mut site_rx) = connect(&state, ConnVersion::V0).await;
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), first, protocol::build_group_tx(&id, 1001, 91, 0)).await;
        drain(&mut site_rx);
        // Same call, another radio answering from behind the other link.
        let answer = protocol::build_group_tx(&id, 1002, 91, 0);
        handle_packet(state.clone(), second, answer.clone()).await;
        assert_eq!(drain(&mut site_rx), vec![answer]);
        let inner = state.inner.read().await;
        assert_eq!((inner.calls[&id].owner, inner.calls[&id].source_issi), (second, 1002));
    }

    #[tokio::test]
    async fn basestation_traffic_is_never_treated_as_a_duplicate() {
        let state = AppState::for_test();
        let (bs1, _bs1_rx) = connect(&state, ConnVersion::V0).await;
        let (bs2, _bs2_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut site_rx) = connect(&state, ConnVersion::V0).await;
        let id = uuid::Uuid::new_v4();
        let group_tx = protocol::build_group_tx(&id, 1001, 91, 0);
        handle_packet(state.clone(), bs1, group_tx.clone()).await;
        handle_packet(state.clone(), bs2, group_tx.clone()).await;
        assert_eq!(drain(&mut site_rx), vec![group_tx.clone(), group_tx]);
        assert_eq!(state.inner.read().await.calls[&id].owner, bs2);
    }

    #[tokio::test]
    async fn duplicate_private_setup_is_dropped_not_rejected() {
        let state = AppState::for_test();
        let (first, _first_rx) = connect_as(&state, ClientMode::Peer).await;
        let (second, mut second_rx) = connect_as(&state, ClientMode::Peer).await;
        let (callee, mut callee_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), callee, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        drain(&mut second_rx);
        let id = uuid::Uuid::new_v4();
        let setup = protocol::build_circular_call_setup(&id, 5001, 6002, 0);
        handle_packet(state.clone(), first, setup.clone()).await;
        assert_eq!(drain(&mut callee_rx), vec![setup.clone()]);
        handle_packet(state.clone(), second, setup).await;
        assert!(drain(&mut callee_rx).is_empty());
        assert!(drain(&mut second_rx).is_empty(), "a duplicate is never answered with a reject");
        assert_eq!(state.inner.read().await.calls[&id].owner, first);
    }

    #[tokio::test]
    async fn deregister_from_a_non_owner_is_not_relayed() {
        let state = AppState::for_test();
        let (owner, _owner_rx) = connect(&state, ConnVersion::V0).await;
        let (other, _other_rx) = connect(&state, ConnVersion::V0).await;
        let (_, mut peer_rx) = connect_as(&state, ClientMode::Peer).await;
        handle_packet(state.clone(), owner, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        assert_eq!(drain(&mut peer_rx), vec![protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])]);
        handle_packet(state.clone(), other, protocol::build_subscriber_message(protocol::SUB_DEREGISTER, 6001, &[])).await;
        assert!(drain(&mut peer_rx).is_empty());
        assert_eq!(state.inner.read().await.subscribers[&6001].client_id, owner);
    }

    #[tokio::test]
    async fn moving_one_issi_keeps_its_old_link_in_groups_others_use() {
        let state = AppState::for_test();
        let (link, _link_rx) = connect_as(&state, ClientMode::Peer).await;
        let (site, _site_rx) = connect(&state, ConnVersion::V0).await;
        for issi in [6001, 6002] {
            handle_packet(state.clone(), link, protocol::build_subscriber_message(SUB_REGISTER, issi, &[])).await;
            handle_packet(state.clone(), link, protocol::build_subscriber_message(protocol::SUB_AFFILIATE, issi, &[91])).await;
        }
        // 6001 roams to a local site; 6002 is still behind the link, in 91.
        handle_packet(state.clone(), site, protocol::build_subscriber_message(SUB_REGISTER, 6001, &[])).await;
        assert_eq!(state.inner.read().await.group_clients[&91], HashSet::from([link, site]));
    }

    #[tokio::test]
    async fn private_setup_bounced_back_over_a_peer_link_is_dropped_not_rejected() {
        let state = AppState::for_test();
        let (link, mut link_rx) = connect_as(&state, ClientMode::Peer).await;
        handle_packet(state.clone(), link, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), link, protocol::build_circular_call_setup(&id, 5001, 6002, 0)).await;
        assert!(drain(&mut link_rx).is_empty(), "a transient loop gets no reject");
        assert!(state.inner.read().await.calls.is_empty());
    }

    #[tokio::test]
    async fn duplicate_group_tx_over_a_loop_safe_link_is_pruned() {
        let state = AppState::for_test();
        let (first, _first_rx) = connect_as(&state, ClientMode::Peer).await;
        let (second, mut second_rx) = connect_as(&state, ClientMode::Peer).await;
        let (legacy, mut legacy_rx) = connect_as(&state, ClientMode::Peer).await;
        let (_, mut site_rx) = connect(&state, ConnVersion::V0).await;
        state.inner.write().await.fed.links.extend([(first, 1), (second, 2)]);
        let id = uuid::Uuid::new_v4();
        let group_tx = protocol::build_group_tx(&id, 1001, 91, 0);
        handle_packet(state.clone(), first, group_tx.clone()).await;
        drain(&mut second_rx);
        drain(&mut legacy_rx);
        drain(&mut site_rx);

        handle_packet(state.clone(), second, group_tx.clone()).await;
        assert_eq!(drain(&mut second_rx), vec![crate::fedroute::build_prune(&id, 1001)]);
        handle_packet(state.clone(), legacy, group_tx).await;
        assert!(drain(&mut legacy_rx).is_empty(), "an older peer is never sent one");

        // A prune from a recipient takes it off this turn's stream only.
        let peers = || async { state.inner.read().await.calls[&id].peers.clone() };
        handle_packet(state.clone(), second, crate::fedroute::build_prune(&id, 1002)).await;
        assert!(peers().await.contains(&second), "another talker's turn: ignored");
        handle_packet(state.clone(), second, crate::fedroute::build_prune(&id, 1001)).await;
        assert!(!peers().await.contains(&second));
        handle_packet(state.clone(), first, voice(&id)).await;
        assert!(drain(&mut second_rx).is_empty());
        assert_eq!(drain(&mut site_rx), vec![voice(&id)]);
    }

    #[tokio::test]
    async fn duplicate_sds_is_dropped() {
        let state = AppState::for_test();
        let (first, _first_rx) = connect_as(&state, ClientMode::Peer).await;
        let (second, mut second_rx) = connect_as(&state, ClientMode::Peer).await;
        let (dest, mut dest_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), dest, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        drain(&mut second_rx);
        let id = uuid::Uuid::new_v4();
        let header = protocol::build_short_transfer(&id, 5001, 6002);
        let payload = protocol::build_sds_transfer_frame(&id, 16, b"hi");
        for link in [first, second] {
            handle_packet(state.clone(), link, header.clone()).await;
            handle_packet(state.clone(), link, payload.clone()).await;
        }
        assert_eq!(drain(&mut dest_rx), vec![header, payload], "delivered once");
        assert!(drain(&mut second_rx).is_empty());
    }

    /// The destination's SDS_REPORT ends a unicast route before the copy over
    /// the slower link arrives: its payload must not be taken for an orphan
    /// SDS and its position reported as ISSI 0.
    #[tokio::test]
    async fn duplicate_sds_payload_after_the_report_is_dropped() {
        let state = AppState::for_test();
        let (first, _first_rx) = connect_as(&state, ClientMode::Peer).await;
        let (second, _second_rx) = connect_as(&state, ClientMode::Peer).await;
        let (dest, mut dest_rx) = connect(&state, ConnVersion::V0).await;
        handle_packet(state.clone(), dest, protocol::build_subscriber_message(SUB_REGISTER, 6002, &[])).await;
        let id = uuid::Uuid::new_v4();
        let header = protocol::build_short_transfer(&id, 5001, 6002);
        // The live LIP short location report verified in `position::decode_lip`.
        let lip = [0x0a, 0x01, 0x0e, 0x62, 0x39, 0xb0, 0x43, 0x9a, 0xff, 0xe0, 0x20];
        let payload = protocol::build_sds_transfer_frame(&id, 88, &lip);
        handle_packet(state.clone(), first, header.clone()).await;
        handle_packet(state.clone(), first, payload.clone()).await;
        let mut report = vec![protocol::CLASS_FRAME, FRAME_SDS_REPORT];
        report.extend_from_slice(id.as_bytes());
        report.extend_from_slice(&8u16.to_le_bytes());
        report.push(0);
        handle_packet(state.clone(), dest, report).await;
        assert!(!state.inner.read().await.sds_routes.contains_key(&id));

        handle_packet(state.clone(), second, header.clone()).await;
        handle_packet(state.clone(), second, payload.clone()).await;
        assert_eq!(drain(&mut dest_rx), vec![header, payload], "delivered once");
        let positions = state.telemetry.read().await.sds_positions.keys().copied().collect::<Vec<_>>();
        assert_eq!(positions, vec![5001]);
    }
}

/// Late entry: a connection that joins a group while a call to it runs gets
/// the call's GROUP_TX and the rest of its voice, not only the next over.
#[cfg(test)]
mod late_entry_tests {
    use super::*;
    use crate::protocol::{ConnVersion, CALL_GROUP_IDLE, SUB_AFFILIATE, SUB_DEAFFILIATE, SUB_REGISTER};
    use crate::state::Client;

    /// Strict affiliation routing: a GROUP_TX reaches only affiliated
    /// connections, so a connection with no member is not in the call.
    fn strict_state() -> Arc<AppState> {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.fallback_broadcast_when_no_affiliations = false;
        Arc::new(AppState::new(config, "test.toml".into()).0)
    }

    async fn connect(state: &AppState, mode: ClientMode, version: ConnVersion) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = uuid::Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version, version_announced: true, remote_addr: None, connected_at_ms: 0, username: None });
        (id, rx)
    }

    async fn bts(state: &AppState) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        connect(state, ClientMode::Basestation, ConnVersion::V0).await
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn voice(id: &uuid::Uuid) -> Vec<u8> {
        protocol::build_traffic_frame(id, &[0x11; protocol::ACELP_CODED_FRAME_BYTES], &[0x22; protocol::ACELP_CODED_FRAME_BYTES])
    }

    fn idle(id: &uuid::Uuid) -> Vec<u8> {
        protocol::build_call_cause(CALL_GROUP_IDLE, id, 0)
    }

    async fn sub(state: &Arc<AppState>, from: ClientId, msg_type: u8, issi: u32, groups: &[u32]) {
        handle_packet(state.clone(), from, protocol::build_subscriber_message(msg_type, issi, groups)).await;
    }

    /// `talker` starts a group call 1001 → `gssi` and returns its uuid and GROUP_TX.
    async fn start_call(state: &Arc<AppState>, talker: ClientId, gssi: u32, priority: u8) -> (uuid::Uuid, Vec<u8>) {
        let id = uuid::Uuid::new_v4();
        let gt = protocol::build_group_tx(&id, 1001, gssi, priority);
        handle_packet(state.clone(), talker, gt.clone()).await;
        (id, gt)
    }

    fn group_tx_count(packets: &[Vec<u8>]) -> usize {
        packets.iter().filter(|p| p.len() > 1 && p[0] == protocol::CLASS_CALL_CONTROL && p[1] == CALL_GROUP_TX).count()
    }

    #[tokio::test]
    async fn affiliating_mid_call_gets_the_header_then_voice_and_end() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        let (id, gt) = start_call(&state, a, 91, 0).await;
        assert!(drain(&mut b_rx).is_empty(), "nobody on B listens to 91 at set-up");

        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert_eq!(drain(&mut b_rx), vec![gt], "late entry hands B the call's GROUP_TX");
        assert!(state.inner.read().await.calls[&id].peers.contains(&b));

        handle_packet(state.clone(), a, voice(&id)).await;
        assert_eq!(drain(&mut b_rx), vec![voice(&id)], "the rest of the over reaches B");
        handle_packet(state.clone(), a, idle(&id)).await;
        assert_eq!(drain(&mut b_rx), vec![idle(&id)], "and so does its end");
    }

    #[tokio::test]
    async fn without_late_entry_voice_would_not_reach_a_new_member() {
        // The behaviour this change replaces, pinned from the other side: a
        // member that was never added to the call does not get its voice.
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        let (id, _) = start_call(&state, a, 91, 0).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[92]).await;
        handle_packet(state.clone(), a, voice(&id)).await;
        assert!(drain(&mut b_rx).is_empty(), "a member of another group hears nothing");
    }

    #[tokio::test]
    async fn switching_talkgroups_away_and_back_in_one_over() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[92]).await;
        let (id, gt) = start_call(&state, a, 91, 0).await;
        assert!(drain(&mut b_rx).is_empty());

        // 92 → 91: joins the running call.
        sub(&state, b, SUB_DEAFFILIATE, 7001, &[92]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        handle_packet(state.clone(), a, voice(&id)).await;
        assert_eq!(drain(&mut b_rx), vec![gt, voice(&id)]);

        // 91 → 92 → 91 within the same over: B stays in the call, no second header.
        sub(&state, b, SUB_DEAFFILIATE, 7001, &[91]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[92]).await;
        handle_packet(state.clone(), a, voice(&id)).await;
        sub(&state, b, SUB_DEAFFILIATE, 7001, &[92]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        handle_packet(state.clone(), a, voice(&id)).await;
        let got = drain(&mut b_rx);
        assert_eq!(group_tx_count(&got), 0, "already a participant: no repeated GROUP_TX");
        assert_eq!(got, vec![voice(&id), voice(&id)], "voice keeps flowing to B until the over ends");

        // The next over is routed by affiliation again: B is in 91, so it is a target.
        handle_packet(state.clone(), a, idle(&id)).await;
        assert_eq!(drain(&mut b_rx), vec![idle(&id)]);
        let (_, next) = start_call(&state, a, 91, 0).await;
        assert_eq!(drain(&mut b_rx), vec![next]);
    }

    #[tokio::test]
    async fn switching_away_for_good_leaves_the_next_over_out() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, b, SUB_REGISTER, 7001, &[91]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        let (id, _) = start_call(&state, a, 91, 0).await;
        drain(&mut b_rx);
        sub(&state, b, SUB_DEAFFILIATE, 7001, &[91]).await;
        handle_packet(state.clone(), a, idle(&id)).await;
        drain(&mut b_rx);
        start_call(&state, a, 91, 0).await;
        assert!(drain(&mut b_rx).is_empty(), "no member of 91 left on B");
    }

    #[tokio::test]
    async fn an_issi_registering_on_another_bts_brings_its_groups_into_the_call() {
        // The handover case: 7001 listens on A, moves to B mid-over.
        let state = strict_state();
        let (talker, _t_rx) = bts(&state).await;
        let (a, mut a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, a, SUB_REGISTER, 7001, &[]).await;
        sub(&state, a, SUB_AFFILIATE, 7001, &[91]).await;
        let (id, gt) = start_call(&state, talker, 91, 0).await;
        assert_eq!(drain(&mut a_rx), vec![gt.clone()]);
        assert!(drain(&mut b_rx).is_empty());

        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        assert_eq!(drain(&mut b_rx), vec![gt], "the registration keeps 91, so B joins the call");
        handle_packet(state.clone(), talker, voice(&id)).await;
        assert_eq!(drain(&mut b_rx), vec![voice(&id)]);
        assert_eq!(drain(&mut a_rx), vec![voice(&id)], "A stays in the over it was part of");
    }

    #[tokio::test]
    async fn a_reregistration_also_joins_the_call() {
        let state = strict_state();
        let (talker, _t_rx) = bts(&state).await;
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, a, SUB_REGISTER, 7001, &[]).await;
        sub(&state, a, SUB_AFFILIATE, 7001, &[91]).await;
        let (_, gt) = start_call(&state, talker, 91, 0).await;
        sub(&state, b, protocol::SUB_REREGISTER, 7001, &[]).await;
        assert_eq!(drain(&mut b_rx), vec![gt]);
    }

    #[tokio::test]
    async fn a_connection_already_in_the_call_gets_no_second_header() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        let (_, gt) = start_call(&state, a, 91, 0).await;
        assert_eq!(drain(&mut b_rx), vec![gt]);
        // A second radio on B joins 91 during the over.
        sub(&state, b, SUB_REGISTER, 7002, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7002, &[91]).await;
        assert!(drain(&mut b_rx).is_empty());
    }

    #[tokio::test]
    async fn the_talkers_own_connection_is_never_sent_its_own_call() {
        let state = strict_state();
        let (a, mut a_rx) = bts(&state).await;
        let (id, _) = start_call(&state, a, 91, 0).await;
        sub(&state, a, SUB_REGISTER, 7001, &[]).await;
        sub(&state, a, SUB_AFFILIATE, 7001, &[91]).await;
        assert!(drain(&mut a_rx).is_empty());
        assert!(!state.inner.read().await.calls[&id].peers.contains(&a));
    }

    #[tokio::test]
    async fn affiliating_to_another_group_joins_nothing() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        let (id, _) = start_call(&state, a, 91, 0).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[92, 93]).await;
        assert!(drain(&mut b_rx).is_empty());
        assert!(!state.inner.read().await.calls[&id].peers.contains(&b));
    }

    #[tokio::test]
    async fn only_calls_to_the_joined_groups_are_entered() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (c, _c_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        let (_, gt91) = start_call(&state, a, 91, 0).await;
        let id92 = uuid::Uuid::new_v4();
        handle_packet(state.clone(), c, protocol::build_group_tx(&id92, 2002, 92, 0)).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert_eq!(drain(&mut b_rx), vec![gt91]);
        sub(&state, b, SUB_AFFILIATE, 7001, &[92]).await;
        assert_eq!(drain(&mut b_rx), vec![protocol::build_group_tx(&id92, 2002, 92, 0)]);
    }

    #[tokio::test]
    async fn the_header_comes_in_each_joiners_own_layout() {
        let state = strict_state();
        let (a, _a_rx) = connect(&state, ClientMode::Basestation, ConnVersion::V1).await;
        let (v0, mut v0_rx) = connect(&state, ClientMode::Basestation, ConnVersion::V0).await;
        let (v1, mut v1_rx) = connect(&state, ClientMode::Basestation, ConnVersion::V1).await;
        let id = uuid::Uuid::new_v4();
        let base = protocol::build_group_tx(&id, 1001, 91, 0);
        let mut named = base.clone();
        let mut name = [0u8; 34];
        name[..3].copy_from_slice(b"BOB");
        named.extend_from_slice(&name);
        handle_packet(state.clone(), a, named.clone()).await;
        for (client, issi) in [(v0, 7001), (v1, 7002)] {
            sub(&state, client, SUB_REGISTER, issi, &[]).await;
            sub(&state, client, SUB_AFFILIATE, issi, &[91]).await;
        }
        assert_eq!(drain(&mut v0_rx), vec![base], "v0: talker name stripped");
        assert_eq!(drain(&mut v1_rx), vec![named], "v1: as the talker sent it");
    }

    #[tokio::test]
    async fn no_late_entry_once_the_over_has_ended() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        let (id, _) = start_call(&state, a, 91, 0).await;
        handle_packet(state.clone(), a, idle(&id)).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert!(drain(&mut b_rx).is_empty());
        assert!(state.inner.read().await.group_tx_raw.is_empty(), "the ended call's header is pruned");
    }

    #[tokio::test]
    async fn a_preempted_call_is_not_entered_only_the_one_that_won() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (c, _c_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        start_call(&state, a, 91, 0).await;
        let winner = uuid::Uuid::new_v4();
        let winner_gt = protocol::build_group_tx(&winner, 2002, 91, 5);
        handle_packet(state.clone(), c, winner_gt.clone()).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert_eq!(drain(&mut b_rx), vec![winner_gt]);
        assert_eq!(state.inner.read().await.group_tx_raw.len(), 1, "the pre-empted call's header is pruned");
    }

    #[tokio::test]
    async fn a_new_talker_in_the_same_call_is_what_a_late_joiner_gets() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        let id = uuid::Uuid::new_v4();
        handle_packet(state.clone(), a, protocol::build_group_tx(&id, 1001, 91, 0)).await;
        let second = protocol::build_group_tx(&id, 1002, 91, 0);
        handle_packet(state.clone(), a, second.clone()).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert_eq!(drain(&mut b_rx), vec![second]);
    }

    #[tokio::test]
    async fn a_call_without_a_stored_header_gets_a_synthesized_one() {
        // A SIP-bridged group call is set up by the bridge, not by a GROUP_TX.
        let state = strict_state();
        let (b, mut b_rx) = bts(&state).await;
        let (owner, _o_rx) = connect(&state, ClientMode::Terminal, ConnVersion::V1).await;
        let id = uuid::Uuid::new_v4();
        state.inner.write().await.calls.insert(id, ActiveCall {
            kind: CallKind::Group, owner, source_issi: 0, destination: 91, priority: 0,
            peers: HashSet::new(), started_at: Instant::now(), last_activity_ms: ActiveCall::new_activity(),
        });
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert_eq!(drain(&mut b_rx), vec![protocol::build_group_tx(&id, 0, 91, 0)]);
    }

    #[tokio::test]
    async fn an_emergency_call_reaches_a_late_basestation_as_sent_and_a_late_console_raised() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        let (console, mut console_rx) = bts(&state).await;
        let id = uuid::Uuid::new_v4();
        let gt = protocol::build_group_tx(&id, 1001, 91, 0);
        handle_packet(state.clone(), a, gt.clone()).await;
        emergency_raised(&state, 1001).await;
        // A console that connects only after the alarm, then listens to 91.
        state.inner.write().await.consoles.insert(console);
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        sub(&state, console, SUB_REGISTER, 9001, &[]).await;
        sub(&state, console, SUB_AFFILIATE, 9001, &[91]).await;
        assert_eq!(drain(&mut b_rx), vec![gt.clone()], "a Basestation gets what the others got");
        assert_eq!(drain(&mut console_rx), vec![with_emergency_priority(&gt)], "a console is told by the priority");
    }

    #[tokio::test]
    async fn an_affiliation_from_a_non_owner_joins_nothing() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (owner, _o_rx) = bts(&state).await;
        let (b, mut b_rx) = bts(&state).await;
        sub(&state, owner, SUB_REGISTER, 7001, &[]).await;
        start_call(&state, a, 91, 0).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        assert!(drain(&mut b_rx).is_empty(), "7001 belongs to another connection");
    }

    #[tokio::test]
    async fn a_federation_peer_link_is_left_to_its_routes() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (peer, mut peer_rx) = connect(&state, ClientMode::Peer, ConnVersion::V1).await;
        start_call(&state, a, 91, 0).await;
        sub(&state, peer, SUB_REGISTER, 7001, &[]).await;
        sub(&state, peer, SUB_AFFILIATE, 7001, &[91]).await;
        assert_eq!(group_tx_count(&drain(&mut peer_rx)), 0);
    }

    #[tokio::test]
    async fn a_late_joiner_cannot_take_over_the_floor_by_sending_voice() {
        let state = strict_state();
        let (a, mut a_rx) = bts(&state).await;
        let (b, _b_rx) = bts(&state).await;
        let (id, _) = start_call(&state, a, 91, 0).await;
        sub(&state, b, SUB_REGISTER, 7001, &[]).await;
        sub(&state, b, SUB_AFFILIATE, 7001, &[91]).await;
        handle_packet(state.clone(), b, voice(&id)).await;
        assert!(drain(&mut a_rx).is_empty(), "only the talker's connection injects voice");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn under_concurrent_voice_the_header_always_comes_first() {
        // Voice streams in on other threads while Basestations join: none of
        // them may see a frame of the call before its GROUP_TX.
        for _round in 0..50 {
            let state = strict_state();
            let (a, _a_rx) = bts(&state).await;
            let (id, gt) = start_call(&state, a, 91, 0).await;
            let talker = {
                let state = state.clone();
                tokio::spawn(async move {
                    for _ in 0..200 {
                        handle_packet(state.clone(), a, voice(&id)).await;
                        tokio::task::yield_now().await;
                    }
                })
            };
            let mut joiners = Vec::new();
            for n in 0..8u32 {
                let (bs, rx) = bts(&state).await;
                let state = state.clone();
                joiners.push((rx, tokio::spawn(async move {
                    sub(&state, bs, SUB_REGISTER, 7000 + n, &[]).await;
                    sub(&state, bs, SUB_AFFILIATE, 7000 + n, &[91]).await;
                })));
            }
            talker.await.unwrap();
            for (mut rx, join) in joiners {
                join.await.unwrap();
                let got = drain(&mut rx);
                assert_eq!(got.first(), Some(&gt), "first packet is the header");
                assert_eq!(group_tx_count(&got), 1);
                assert!(got[1..].iter().all(|p| *p == voice(&id)));
            }
        }
    }

    #[tokio::test]
    async fn many_basestations_joining_one_over_each_get_one_header() {
        let state = strict_state();
        let (a, _a_rx) = bts(&state).await;
        let (id, gt) = start_call(&state, a, 91, 0).await;
        let mut joiners = Vec::new();
        for n in 0..10u32 {
            let (bs, rx) = bts(&state).await;
            sub(&state, bs, SUB_REGISTER, 7000 + n, &[]).await;
            sub(&state, bs, SUB_AFFILIATE, 7000 + n, &[91]).await;
            sub(&state, bs, SUB_AFFILIATE, 7000 + n, &[91]).await;
            joiners.push(rx);
        }
        handle_packet(state.clone(), a, voice(&id)).await;
        for rx in &mut joiners {
            assert_eq!(drain(rx), vec![gt.clone(), voice(&id)]);
        }
        assert_eq!(state.inner.read().await.calls[&id].peers.len(), 10);
    }
}
