//! Handover relay: an announced cell change between two Basestations.
//!
//! A radio that wants to move to a neighbour cell sends U-PREPARE to its
//! serving cell (A), optionally with a registration (MM SDU) for the target.
//! When that neighbour is served by another Basestation (B), A asks the
//! server with `SERVICE_HO_PREPARE` / `SERVICE_HO_FORWARD_REG`; the server
//! finds B by the target carrier (`carriers`, narrowed by the target's
//! location area when A names it), forwards the request and relays
//! B's `SERVICE_HO_RESULT` back to A. A then answers the radio (D-NEW-CELL or
//! D-PREPARE-FAIL). On the air interface this is announced cell reselection
//! (TS 100 392-2 clause 18.3.4.7); TETRA has no network-controlled handover,
//! "handover" here only names the relay. The radio-side state machines stay
//! in the Basestations:
//! the server only finds the target, keeps the transaction and its timeout.
//!
//! Both sides must have announced the `handover-v1` capability in
//! `SERVICE_BTS_INFO`, which a Basestation only sends to a server that offered
//! the `bts-info` extension on the WebSocket upgrade. A Basestation without
//! it (TetraPack, Brandmeister, older FlowStation) is never sent any of these
//! messages, and nothing else in the routing changes.
//!
//! Every request carries a transaction id chosen by A. The server forwards it
//! to B under an id of its own, so ids from different serving cells cannot
//! clash at B, and maps B's answer back. The server answers A itself with
//! "no target" when the carrier is unknown, served by A itself, or served by
//! a Basestation without the capability, and with "timeout" when B does not
//! answer within `[handover] timeout_ms` (or goes away first: "no target").
//! A "no target" carries a `reason` (`NoTargetReason`), so A can tell a target
//! that went away (the radio should not be sent there: D-PREPARE-FAIL) from a
//! neighbour this server does not prepare (D-NEW-CELL without preparation).

use crate::{carriers::CarrierKey, protocol, state::{AppState, ClientId, ClientMode, Inner}};
use std::{collections::HashMap, sync::Arc, time::{Duration, Instant}};
use tracing::{info, warn};

/// Capability a Basestation announces in `SERVICE_BTS_INFO` when it speaks
/// the handover messages.
pub const CAPABILITY: &str = "handover-v1";

/// Longest MM SDU a request or result may carry (an MM PDU in U-PREPARE /
/// D-NEW-CELL is a few hundred bits).
pub const MAX_SDU_BITS: u16 = 4096;

/// Requests one serving Basestation may have open at a time.
const MAX_PENDING_PER_SOURCE: usize = 32;

/// `status` of a `SERVICE_HO_RESULT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The target accepted; the radio may move (D-NEW-CELL).
    Ok = 0,
    /// The target's MM rejected the forwarded registration (D-PREPARE-FAIL).
    Rejected = 1,
    /// No Basestation with `handover-v1` serves the target carrier.
    NoTarget = 2,
    /// The target did not answer in time.
    Timeout = 3,
}

impl Status {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Status::Ok),
            1 => Some(Status::Rejected),
            2 => Some(Status::NoTarget),
            3 => Some(Status::Timeout),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Rejected => "rejected",
            Status::NoTarget => "no target",
            Status::Timeout => "timeout",
        }
    }
}

/// Why the server answered "no target" itself (`reason` in the result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoTargetReason {
    /// No Basestation announced the target cell.
    Unknown,
    /// The target is a cell of the serving Basestation itself.
    OwnCell,
    /// The Basestation serving the target does not speak `handover-v1`.
    NotCapable,
    /// The target Basestation went away while the request waited for it.
    Gone,
    /// The serving Basestation announced no cells, so its network is unknown.
    NoNetwork,
}

impl NoTargetReason {
    pub fn as_str(self) -> &'static str {
        match self {
            NoTargetReason::Unknown => "unknown",
            NoTargetReason::OwnCell => "own_cell",
            NoTargetReason::NotCapable => "not_capable",
            NoTargetReason::Gone => "gone",
            NoTargetReason::NoNetwork => "no_network",
        }
    }
}

/// An MM PDU as carried inside the JSON: `sdu` (hex) plus `sdu_len_bits`, as
/// the PDU need not end on a byte boundary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sdu {
    pub len_bits: u16,
    pub bytes: Vec<u8>,
}

impl Sdu {
    fn parse(hex: &str, len_bits: u16) -> Result<Self, String> {
        if len_bits > MAX_SDU_BITS { return Err(format!("sdu_len_bits {len_bits}, at most {MAX_SDU_BITS}")); }
        let bytes = decode_hex(hex).ok_or_else(|| "sdu is not hex".to_string())?;
        if bytes.len() != usize::from(len_bits).div_ceil(8) {
            return Err(format!("sdu has {} bytes, sdu_len_bits {len_bits} needs {}", bytes.len(), usize::from(len_bits).div_ceil(8)));
        }
        Ok(Self { len_bits, bytes })
    }

    fn hex(&self) -> String {
        self.bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) { return None; }
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok())).collect()
}

/// A `SERVICE_HO_PREPARE` / `SERVICE_HO_FORWARD_REG` as the serving cell sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub transaction_id: u32,
    pub issi: u32,
    pub target_carrier: u16,
    pub target_carrier_ext: u16,
    /// Network of the target cell; the serving cell's own when left out.
    pub target_mcc: Option<u16>,
    pub target_mnc: Option<u16>,
    /// Location area of the target cell, from the serving cell's neighbour
    /// entry (TS 100 392-2 clause 18.5.17). Carriers are reused across a
    /// network, so when given only a cell in this LA is the target.
    pub target_location_area: Option<u16>,
    /// The registration for the target cell (`SERVICE_HO_FORWARD_REG` only).
    pub sdu: Option<Sdu>,
    /// Group of the call the MS listens to, when the serving cell offers announced
    /// type 1 reselection (`"call":{"gssi":N}`). Passed on to the target.
    pub call_gssi: Option<u32>,
}

#[derive(serde::Deserialize)]
struct CallMessage {
    gssi: u32,
}

#[derive(serde::Deserialize)]
struct RequestMessage {
    transaction_id: u32,
    issi: u32,
    target_carrier: u16,
    target_carrier_ext: u16,
    #[serde(default)]
    target_mcc: Option<u16>,
    #[serde(default)]
    target_mnc: Option<u16>,
    #[serde(default)]
    target_location_area: Option<u16>,
    #[serde(default)]
    sdu: Option<String>,
    #[serde(default)]
    sdu_len_bits: Option<u16>,
    #[serde(default)]
    call: Option<serde_json::Value>,
}

fn check_issi(issi: u32) -> Result<(), String> {
    if issi > 0xff_ffff { Err(format!("issi {issi} out of range (24 bits)")) } else { Ok(()) }
}

/// Parses a request: `{"transaction_id":N,"issi":N,"target_carrier":N,
/// "target_carrier_ext":N,"target_mcc":N,"target_mnc":N,"target_location_area":N,
/// "sdu":"hex","sdu_len_bits":N,"call":{"gssi":N}}`.
/// `with_sdu`: a `SERVICE_HO_FORWARD_REG`, which must carry the registration.
/// A malformed `call` is dropped (the handover goes on as type 2), not refused.
pub fn parse_request(json: &str, with_sdu: bool) -> Result<Request, String> {
    let msg: RequestMessage = serde_json::from_str(json).map_err(|e| e.to_string())?;
    check_issi(msg.issi)?;
    if msg.target_carrier > 0xfff { return Err(format!("target_carrier {} out of range (0-4095)", msg.target_carrier)); }
    if msg.target_carrier_ext > 0x3ff { return Err(format!("target_carrier_ext {} out of range (0-1023)", msg.target_carrier_ext)); }
    if msg.target_mcc.is_some_and(|m| m > 0x3ff) { return Err("target_mcc out of range (0-1023)".into()); }
    if msg.target_mnc.is_some_and(|m| m > 0x3fff) { return Err("target_mnc out of range (0-16383)".into()); }
    if msg.target_location_area.is_some_and(|la| la > 0x3fff) { return Err("target_location_area out of range (0-16383)".into()); }
    let sdu = match (with_sdu, msg.sdu, msg.sdu_len_bits) {
        (false, _, _) => None,
        (true, Some(hex), Some(bits)) => Some(Sdu::parse(&hex, bits)?),
        (true, _, _) => return Err("forwarded registration without sdu / sdu_len_bits".into()),
    };
    let call_gssi = msg
        .call
        .filter(|c| !c.is_null())
        .and_then(|c| match serde_json::from_value::<CallMessage>(c) {
            Ok(c) if c.gssi <= 0xff_ffff => Some(c.gssi),
            _ => {
                warn!(issi = msg.issi, "handover request with a malformed call; passed on without it");
                None
            }
        });
    Ok(Request {
        transaction_id: msg.transaction_id,
        issi: msg.issi,
        target_carrier: msg.target_carrier,
        target_carrier_ext: msg.target_carrier_ext,
        target_mcc: msg.target_mcc,
        target_mnc: msg.target_mnc,
        target_location_area: msg.target_location_area,
        sdu,
        call_gssi,
    })
}

/// A `SERVICE_HO_RESULT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoResult {
    pub transaction_id: u32,
    pub issi: u32,
    pub status: Status,
    /// The target's MM answer for D-NEW-CELL, if any.
    pub sdu: Option<Sdu>,
    /// The target's traffic channel for the call named in the request (type 1).
    pub channel: Option<Channel>,
}

/// The target cell's channel for the MS's group call: call identifier (14 bits), carrier
/// (12 bits, the serving cell's band plan), timeslot 1-4 and usage marker 4-63.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Channel {
    pub call_id: u16,
    pub carrier: u16,
    pub timeslot: u8,
    pub usage: u8,
}

impl Channel {
    fn valid(&self) -> bool {
        self.call_id <= 0x3fff && self.carrier <= 0xfff && (1..=4).contains(&self.timeslot) && (4..=63).contains(&self.usage)
    }
}

#[derive(serde::Deserialize)]
struct ResultMessage {
    transaction_id: u32,
    issi: u32,
    status: u8,
    #[serde(default)]
    sdu: Option<String>,
    #[serde(default)]
    sdu_len_bits: Option<u16>,
    #[serde(default)]
    channel: Option<serde_json::Value>,
}

/// Parses `{"transaction_id":N,"issi":N,"status":N,"sdu":"hex","sdu_len_bits":N,
/// "channel":{"call_id":N,"carrier":N,"timeslot":N,"usage":N}}` (`sdu` / `sdu_len_bits`
/// optional, together; `channel` optional, dropped when malformed or not with status ok).
pub fn parse_result(json: &str) -> Result<HoResult, String> {
    let msg: ResultMessage = serde_json::from_str(json).map_err(|e| e.to_string())?;
    check_issi(msg.issi)?;
    let status = Status::from_u8(msg.status).ok_or_else(|| format!("status {} unknown (0-3)", msg.status))?;
    let sdu = match (msg.sdu, msg.sdu_len_bits) {
        (None, None) => None,
        (Some(hex), Some(bits)) => Some(Sdu::parse(&hex, bits)?),
        _ => return Err("sdu and sdu_len_bits go together".into()),
    };
    let channel = msg
        .channel
        .filter(|c| !c.is_null())
        .and_then(|c| match serde_json::from_value::<Channel>(c) {
            Ok(c) if c.valid() => Some(c),
            _ => {
                warn!(issi = msg.issi, "handover result with a malformed channel; relayed without it");
                None
            }
        })
        .filter(|_| status == Status::Ok);
    Ok(HoResult { transaction_id: msg.transaction_id, issi: msg.issi, status, sdu, channel })
}

fn request_json(req: &Request, transaction_id: u32, key: &CarrierKey, location_area: Option<u16>) -> String {
    let mut v = serde_json::json!({
        "transaction_id": transaction_id,
        "issi": req.issi,
        "target_carrier": key.carrier,
        "target_carrier_ext": key.carrier_ext,
        "target_mcc": key.mcc,
        "target_mnc": key.mnc,
    });
    if let Some(la) = location_area {
        v["target_location_area"] = la.into();
    }
    if let Some(sdu) = &req.sdu {
        v["sdu"] = sdu.hex().into();
        v["sdu_len_bits"] = sdu.len_bits.into();
    }
    if let Some(gssi) = req.call_gssi {
        v["call"] = serde_json::json!({ "gssi": gssi });
    }
    v.to_string()
}

/// A `SERVICE_HO_RESULT` packet.
pub fn build_result(transaction_id: u32, issi: u32, status: Status, sdu: Option<&Sdu>, channel: Option<&Channel>) -> Vec<u8> {
    let mut v = serde_json::json!({ "transaction_id": transaction_id, "issi": issi, "status": status as u8 });
    if let Some(c) = channel {
        v["channel"] = serde_json::json!(c);
    }
    result_packet(v, sdu)
}

/// A "no target" `SERVICE_HO_RESULT` the server sends itself, with its reason.
pub fn build_no_target(transaction_id: u32, issi: u32, reason: NoTargetReason) -> Vec<u8> {
    let v = serde_json::json!({ "transaction_id": transaction_id, "issi": issi, "status": Status::NoTarget as u8, "reason": reason.as_str() });
    result_packet(v, None)
}

fn result_packet(mut v: serde_json::Value, sdu: Option<&Sdu>) -> Vec<u8> {
    if let Some(sdu) = sdu {
        v["sdu"] = sdu.hex().into();
        v["sdu_len_bits"] = sdu.len_bits.into();
    }
    protocol::build_service(protocol::SERVICE_HO_RESULT, &v.to_string())
}

/// One request waiting for the target's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub source: ClientId,
    /// The id the serving cell chose.
    pub source_transaction: u32,
    pub issi: u32,
    pub target: ClientId,
    pub key: CarrierKey,
    pub started: Instant,
}

/// The handovers in flight, keyed by the id the server forwarded them under,
/// and the ISSIs that were handed over lately.
#[derive(Default)]
pub struct Handovers {
    pending: HashMap<u32, Pending>,
    last_id: u32,
    /// ISSI -> when its last handover succeeded, and to which Basestation.
    completed: HashMap<u32, (Instant, ClientId)>,
}

impl Handovers {
    /// Opens a transaction and returns the id to forward it under. `None`
    /// when the serving cell already has this transaction open (a repeat) or
    /// too many open at once.
    pub fn begin(&mut self, pending: Pending) -> Option<u32> {
        let open = self.pending.values().filter(|p| p.source == pending.source);
        let mut count = 0;
        for p in open {
            if p.source_transaction == pending.source_transaction { return None; }
            count += 1;
        }
        if count >= MAX_PENDING_PER_SOURCE { return None; }
        loop {
            self.last_id = self.last_id.wrapping_add(1);
            if self.last_id != 0 && !self.pending.contains_key(&self.last_id) { break; }
        }
        self.pending.insert(self.last_id, pending);
        Some(self.last_id)
    }

    /// Closes transaction `id` with the answer `from` sent. `None` when it is
    /// not open (unknown, already answered or timed out) or `from` is not its
    /// target.
    pub fn finish(&mut self, id: u32, from: ClientId) -> Option<Pending> {
        if self.pending.get(&id)?.target != from { return None; }
        self.pending.remove(&id)
    }

    /// Closes transaction `id` because its time is up.
    pub fn expire(&mut self, id: u32) -> Option<Pending> {
        self.pending.remove(&id)
    }

    /// A connection went away: its own requests are dropped, and the
    /// requests waiting for it are returned (to answer "no target").
    pub fn client_gone(&mut self, id: ClientId) -> Vec<Pending> {
        self.pending.retain(|_, p| p.source != id);
        let ids: Vec<u32> = self.pending.iter().filter(|(_, p)| p.target == id).map(|(tid, _)| *tid).collect();
        ids.into_iter().filter_map(|tid| self.pending.remove(&tid)).collect()
    }

    /// Records a successful handover of `issi` to `target`, forgetting those
    /// older than `keep`.
    pub fn note_completed(&mut self, issi: u32, target: ClientId, now: Instant, keep: Duration) {
        self.completed.retain(|_, (at, _)| now.duration_since(*at) < keep);
        self.completed.insert(issi, (now, target));
    }

    /// Whether `issi` is being handed over right now, or was handed over
    /// successfully within `window`: its registration on the new cell came
    /// from the handover, and the old cell lets the radio go by itself.
    pub fn recently_handed_over(&self, issi: u32, now: Instant, window: Duration) -> bool {
        self.pending.values().any(|p| p.issi == issi)
            || self.completed.get(&issi).is_some_and(|(at, _)| now.duration_since(*at) < window)
    }

    /// As `recently_handed_over`, but only a handover to `target`: whether
    /// `target` registering `issi` is that handover.
    pub fn handed_over_to(&self, issi: u32, target: ClientId, now: Instant, window: Duration) -> bool {
        self.pending.values().any(|p| p.issi == issi && p.target == target)
            || self.completed.get(&issi).is_some_and(|(at, t)| *t == target && now.duration_since(*at) < window)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// How long after a handover the `[roaming]` kick leaves the ISSI alone: the
/// kick of the old registration fires `kick_delay_ms` after the target
/// reports the registration that came with the handover (before its result;
/// `timeout_ms` covers the telemetry report arriving later than the result).
pub fn kick_guard(config: &crate::config::Config) -> Duration {
    Duration::from_millis(config.roaming.kick_delay_ms + config.handover.timeout_ms)
}

fn has_capability(inner: &Inner, id: ClientId) -> bool {
    announced(inner, id, CAPABILITY)
}

/// Capability in `SERVICE_BTS_INFO` for `SERVICE_MS_MOVED`.
pub const CAPABILITY_MS_MOVED: &str = "ms-moved";

/// Whether Basestation `id` announced `capability` in its `SERVICE_BTS_INFO`.
pub fn announced(inner: &Inner, id: ClientId, capability: &str) -> bool {
    inner.clients.get(&id).is_some_and(|c| c.mode == ClientMode::Basestation)
        && inner.carriers.info(id).is_some_and(|info| info.capabilities.iter().any(|c| c == capability))
}

/// A radio registered on `to` while it was registered on `from`, without a
/// handover prepared here (an unannounced cell change, or announced to a cell
/// this server did not prepare). Returns what the caller does once the lock
/// is released:
/// - `SERVICE_MS_MOVED` to `from`, if it announced `ms-moved`: it stops serving
///   the radio (no SUB_DEREGISTER expected);
/// - the group calls the radio is talking in through `from`, to be ended, if
///   `to` announced `handover-v1`: the talker is gone from `from`, and `to`
///   may now give it the floor (TS 100 392-2 clause 14.5.2.2.1 a), the SwMI
///   controls who transmits). Without this the call would hold the group's
///   floor until `from` noticed the silence.
pub fn radio_moved(inner: &Inner, issi: u32, from: ClientId, to: ClientId) -> (Option<Vec<u8>>, Vec<uuid::Uuid>) {
    let notice = announced(inner, from, CAPABILITY_MS_MOVED)
        .then(|| protocol::build_service(protocol::SERVICE_MS_MOVED, &serde_json::json!({ "issi": issi }).to_string()));
    let talking = if has_capability(inner, to) {
        inner
            .calls
            .iter()
            .filter(|(_, c)| c.kind == crate::state::CallKind::Group && c.owner == from && c.source_issi == issi)
            .map(|(id, _)| *id)
            .collect()
    } else {
        Vec::new()
    };
    (notice, talking)
}

fn send(inner: &Inner, to: ClientId, packet: Vec<u8>) {
    if let Some(client) = inner.clients.get(&to) {
        let _ = client.tx.send(packet);
    }
}

/// A `SERVICE_HO_PREPARE` (`with_sdu` false) or `SERVICE_HO_FORWARD_REG` from
/// the serving cell `source`.
pub async fn handle_request(state: &Arc<AppState>, source: ClientId, service_type: u8, json: &str) {
    let with_sdu = service_type == protocol::SERVICE_HO_FORWARD_REG;
    let req = match parse_request(json, with_sdu) {
        Ok(req) => req,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed handover request");
            return;
        }
    };
    let mut inner = state.inner.write().await;
    if !has_capability(&inner, source) {
        // It could not read the answer either.
        warn!(%source, issi = req.issi, "handover request from a connection without handover-v1 ignored");
        return;
    }
    // A Basestation serves one network: its cells all carry the same codes.
    let own = inner.carriers.info(source).and_then(|info| info.cells.first()).map(|c| (c.key.mcc, c.key.mnc));
    let (Some(mcc), Some(mnc)) = (req.target_mcc.or(own.map(|o| o.0)), req.target_mnc.or(own.map(|o| o.1))) else {
        warn!(%source, issi = req.issi, "handover request without a network code from a Basestation that announced no cells");
        send(&inner, source, build_no_target(req.transaction_id, req.issi, NoTargetReason::NoNetwork));
        return;
    };
    let key = CarrierKey { mcc, mnc, carrier: req.target_carrier, carrier_ext: req.target_carrier_ext };
    let owner = inner.carriers.find_target(&key, req.target_location_area);
    let target = match owner {
        None => Err(NoTargetReason::Unknown),
        Some(t) if t == source => Err(NoTargetReason::OwnCell),
        Some(t) if !has_capability(&inner, t) => Err(NoTargetReason::NotCapable),
        Some(t) => Ok(t),
    };
    let target = match target {
        Ok(t) => t,
        Err(why) => {
            info!(%source, issi = req.issi, carrier = key.carrier, carrier_ext = key.carrier_ext,
                location_area = ?req.target_location_area, reason = why.as_str(), "handover: no target");
            send(&inner, source, build_no_target(req.transaction_id, req.issi, why));
            return;
        }
    };
    // B learns which of its cells is meant, also when A left the LA out.
    let location_area = req.target_location_area.or_else(|| {
        inner.carriers.info(target).and_then(|info| info.cells.iter().find(|c| c.key == key)).map(|c| c.location_area)
    });
    let pending = Pending { source, source_transaction: req.transaction_id, issi: req.issi, target, key, started: Instant::now() };
    let Some(id) = inner.handovers.begin(pending) else {
        warn!(%source, issi = req.issi, transaction_id = req.transaction_id, "handover request repeated or too many open; dropped");
        return;
    };
    send(&inner, target, protocol::build_service(service_type, &request_json(&req, id, &key, location_area)));
    info!(%source, %target, issi = req.issi, carrier = key.carrier, transaction_id = req.transaction_id, forwarded_as = id,
        registration = with_sdu, "handover: request forwarded to the target Basestation");
    drop(inner);

    let state = state.clone();
    let timeout = Duration::from_millis(state.config.handover.timeout_ms);
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        let mut inner = state.inner.write().await;
        if let Some(p) = inner.handovers.expire(id) {
            warn!(source = %p.source, target = %p.target, issi = p.issi, transaction_id = p.source_transaction, "handover: target did not answer in time");
            send(&inner, p.source, build_result(p.source_transaction, p.issi, Status::Timeout, None, None));
        }
    });
}

/// A `SERVICE_HO_RESULT` from the target cell `source`.
pub async fn handle_result(state: &Arc<AppState>, source: ClientId, json: &str) {
    let result = match parse_result(json) {
        Ok(r) => r,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed handover result");
            return;
        }
    };
    let mut inner = state.inner.write().await;
    let Some(p) = inner.handovers.finish(result.transaction_id, source) else {
        warn!(%source, issi = result.issi, transaction_id = result.transaction_id, "handover result for no open request (late, repeated or not the target); dropped");
        return;
    };
    if result.issi != p.issi {
        warn!(%source, issi = result.issi, expected = p.issi, "handover result names another ISSI than the request; relayed for the requested one");
    }
    if result.status == Status::Ok {
        let keep = kick_guard(&state.config);
        inner.handovers.note_completed(p.issi, source, Instant::now(), keep);
    }
    send(&inner, p.source, build_result(p.source_transaction, p.issi, result.status, result.sdu.as_ref(), result.channel.as_ref()));
    info!(source = %p.source, target = %source, issi = p.issi, status = result.status.as_str(),
        ms = p.started.elapsed().as_millis() as u64, "handover: result relayed to the serving Basestation");
}

/// A connection went away (`state::cleanup_client`, lock held): requests that
/// waited for it are answered "no target".
pub fn client_gone(inner: &mut Inner, id: ClientId) {
    for p in inner.handovers.client_gone(id) {
        info!(source = %p.source, target = %id, issi = p.issi, "handover: target Basestation went away");
        send(inner, p.source, build_no_target(p.source_transaction, p.issi, NoTargetReason::Gone));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::{BrewMessage, ConnVersion, SUB_AFFILIATE, SUB_REGISTER}, state::Client};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    const EXT: u16 = 268;

    fn test_state(timeout_ms: u64) -> Arc<AppState> {
        let mut c = crate::config::Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        c.handover.timeout_ms = timeout_ms;
        Arc::new(AppState::new(c, "test.toml".into()).0)
    }

    async fn connect(state: &AppState, mode: ClientMode) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
        (id, rx)
    }

    /// A Basestation that announced `carrier`, with or without `handover-v1`.
    async fn bts(state: &Arc<AppState>, carrier: u16, capable: bool) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (id, rx) = connect(state, ClientMode::Basestation).await;
        let caps = if capable { r#"["handover-v1"]"# } else { "[]" };
        let json = format!(r#"{{"mcc":232,"mnc":5,"cells":[{{"carrier":{carrier},"carrier_ext":{EXT},"location_area":{carrier}}}],"capabilities":{caps}}}"#);
        crate::router::handle_packet(state.clone(), id, protocol::build_service(protocol::SERVICE_BTS_INFO, &json)).await;
        (id, rx)
    }

    /// A Basestation with exactly these capabilities.
    async fn bts_with(state: &Arc<AppState>, carrier: u16, caps: &[&str]) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (id, rx) = connect(state, ClientMode::Basestation).await;
        let json = serde_json::json!({
            "mcc": 232, "mnc": 5,
            "cells": [{ "carrier": carrier, "carrier_ext": EXT, "location_area": carrier }],
            "capabilities": caps,
        });
        crate::router::handle_packet(state.clone(), id, protocol::build_service(protocol::SERVICE_BTS_INFO, &json.to_string())).await;
        (id, rx)
    }

    /// Every service message `rx` holds, as (type, JSON).
    fn services(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<(u8, serde_json::Value)> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|p| match protocol::parse_with_version(&p, ConnVersion::V0) {
                Ok((BrewMessage::Service(s), _)) => Some((s.service_type, serde_json::from_str(&s.json_data).unwrap())),
                _ => None,
            })
            .collect()
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn forward_reg(tid: u32, issi: u32, carrier: u16) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_HO_FORWARD_REG, &format!(
            r#"{{"transaction_id":{tid},"issi":{issi},"target_carrier":{carrier},"target_carrier_ext":{EXT},"sdu":"a1b2c0","sdu_len_bits":18}}"#))
    }

    fn prepare(tid: u32, issi: u32, carrier: u16) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_HO_PREPARE, &format!(
            r#"{{"transaction_id":{tid},"issi":{issi},"target_carrier":{carrier},"target_carrier_ext":{EXT}}}"#))
    }

    fn result(tid: u32, issi: u32, status: u8) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_HO_RESULT, &format!(
            r#"{{"transaction_id":{tid},"issi":{issi},"status":{status},"sdu":"0f80","sdu_len_bits":9}}"#))
    }

    async fn packet(state: &Arc<AppState>, from: ClientId, raw: Vec<u8>) {
        crate::router::handle_packet(state.clone(), from, raw).await;
    }

    #[test]
    fn parses_requests_and_results() {
        let req = parse_request(r#"{"transaction_id":7,"issi":2320001,"target_carrier":1530,"target_carrier_ext":268,"sdu":"A1B2c0","sdu_len_bits":18,"later":1}"#, true).unwrap();
        assert_eq!(req.sdu, Some(Sdu { len_bits: 18, bytes: vec![0xa1, 0xb2, 0xc0] }));
        assert_eq!((req.target_mcc, req.target_mnc), (None, None));
        // A plain prepare ignores an SDU.
        assert_eq!(parse_request(r#"{"transaction_id":7,"issi":1,"target_carrier":1,"target_carrier_ext":0,"sdu":"zz"}"#, false).unwrap().sdu, None);
        let res = parse_result(r#"{"transaction_id":7,"issi":1,"status":0}"#).unwrap();
        assert_eq!((res.status, res.sdu), (Status::Ok, None));
        assert_eq!(parse_result(r#"{"transaction_id":7,"issi":1,"status":1,"sdu":"","sdu_len_bits":0}"#).unwrap().sdu, Some(Sdu::default()));
    }

    #[test]
    fn rejects_bad_requests_and_results() {
        for (bad, with_sdu) in [
            ("nope", false),
            (r#"{"issi":1,"target_carrier":1,"target_carrier_ext":0}"#, false),
            (r#"{"transaction_id":1,"issi":16777216,"target_carrier":1,"target_carrier_ext":0}"#, false),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":4096,"target_carrier_ext":0}"#, false),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":1024}"#, false),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0,"target_mcc":1024}"#, false),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0}"#, true),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0,"sdu":"a1"}"#, true),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0,"sdu":"a1","sdu_len_bits":9}"#, true),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0,"sdu":"a","sdu_len_bits":4}"#, true),
            (r#"{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0,"sdu":"g0","sdu_len_bits":8}"#, true),
        ] {
            assert!(parse_request(bad, with_sdu).is_err(), "accepted {bad}");
        }
        let too_long = format!(r#"{{"transaction_id":1,"issi":1,"target_carrier":1,"target_carrier_ext":0,"sdu":"{}","sdu_len_bits":4104}}"#, "00".repeat(513));
        assert!(parse_request(&too_long, true).is_err());
        for bad in [
            r#"{"transaction_id":1,"issi":1,"status":4}"#,
            r#"{"transaction_id":1,"issi":1,"status":0,"sdu":"00"}"#,
            r#"{"transaction_id":1,"issi":1,"status":0,"sdu_len_bits":8}"#,
            r#"{"issi":1,"status":0}"#,
        ] {
            assert!(parse_result(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn transactions_open_close_and_expire() {
        let mut h = Handovers::default();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let key = CarrierKey { mcc: 232, mnc: 5, carrier: 1530, carrier_ext: EXT };
        let p = |tid| Pending { source: a, source_transaction: tid, issi: 1, target: b, key, started: Instant::now() };
        let id = h.begin(p(7)).unwrap();
        assert_eq!(h.begin(p(7)), None, "a repeat is not a second transaction");
        assert_eq!(h.finish(id, a), None, "only the target answers");
        assert_eq!(h.finish(id, b).map(|p| p.source_transaction), Some(7));
        assert_eq!(h.finish(id, b), None, "answered once");
        let id = h.begin(p(8)).unwrap();
        assert!(h.expire(id).is_some());
        assert_eq!(h.finish(id, b), None, "too late");
        for tid in 0..MAX_PENDING_PER_SOURCE as u32 { assert!(h.begin(p(100 + tid)).is_some()); }
        assert_eq!(h.begin(p(99)), None, "too many open");
        assert_eq!(h.client_gone(b).len(), MAX_PENDING_PER_SOURCE);
        assert_eq!(h.pending_count(), 0);
    }

    #[test]
    fn ids_skip_zero_and_open_ones() {
        let mut h = Handovers { last_id: u32::MAX - 1, ..Default::default() };
        let key = CarrierKey { mcc: 232, mnc: 5, carrier: 1530, carrier_ext: EXT };
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let p = |tid| Pending { source: a, source_transaction: tid, issi: 1, target: b, key, started: Instant::now() };
        assert_eq!(h.begin(p(1)), Some(u32::MAX));
        assert_eq!(h.begin(p(2)), Some(1), "wraps around, never 0");
    }

    #[test]
    fn recently_handed_over_covers_pending_and_the_window() {
        let mut h = Handovers::default();
        let now = Instant::now();
        let window = Duration::from_secs(5);
        let key = CarrierKey { mcc: 232, mnc: 5, carrier: 1530, carrier_ext: EXT };
        let id = h.begin(Pending { source: Uuid::new_v4(), source_transaction: 1, issi: 42, target: Uuid::new_v4(), key, started: now }).unwrap();
        assert!(h.recently_handed_over(42, now, window));
        assert!(!h.recently_handed_over(43, now, window));
        assert!(!h.handed_over_to(42, Uuid::new_v4(), now, window), "pending to another Basestation");
        h.expire(id);
        assert!(!h.recently_handed_over(42, now, window), "a failed handover does not count");
        let b = Uuid::new_v4();
        h.note_completed(42, b, now, window);
        assert!(h.recently_handed_over(42, now + Duration::from_secs(4), window));
        assert!(h.handed_over_to(42, b, now + Duration::from_secs(4), window));
        assert!(!h.handed_over_to(42, Uuid::new_v4(), now, window), "another Basestation");
        assert!(!h.recently_handed_over(42, now + Duration::from_secs(6), window));
    }

    /// The full exchange between two simulated Basestations: A asks, B gets
    /// the request under the server's id and the SDU unchanged, registers
    /// the ISSI (which moves it and hands B the running call), answers, and A
    /// gets the answer under its own id with B's SDU.
    #[tokio::test]
    async fn handover_between_two_basestations() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
        packet(&state, a, protocol::build_subscriber_message(SUB_AFFILIATE, 2320001, &[91])).await;
        // A group call to 91 runs from a third Basestation.
        let (c, _c_rx) = bts(&state, 1532, false).await;
        let call = Uuid::new_v4();
        packet(&state, c, protocol::build_group_tx(&call, 1001, 91, 0)).await;
        drain(&mut a_rx);
        drain(&mut b_rx);

        packet(&state, a, forward_reg(7, 2320001, 1530)).await;
        let got = services(&mut b_rx);
        assert_eq!(got.len(), 1);
        let (kind, req) = &got[0];
        assert_eq!(*kind, protocol::SERVICE_HO_FORWARD_REG);
        assert_eq!((req["issi"].as_u64(), req["sdu"].as_str(), req["sdu_len_bits"].as_u64()), (Some(2320001), Some("a1b2c0"), Some(18)));
        assert_eq!((req["target_carrier"].as_u64(), req["target_mcc"].as_u64(), req["target_mnc"].as_u64()), (Some(1530), Some(232), Some(5)));
        let tid = req["transaction_id"].as_u64().unwrap() as u32;
        assert!(services(&mut a_rx).is_empty(), "nothing back to A yet");
        assert!(state.inner.read().await.handovers.recently_handed_over(2320001, Instant::now(), Duration::ZERO));

        // B registers the radio: it moves to B with its groups and B joins the call.
        packet(&state, b, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
        let to_b = drain(&mut b_rx);
        assert!(to_b.iter().any(|p| p.len() > 1 && p[0] == protocol::CLASS_CALL_CONTROL && p[1] == protocol::CALL_GROUP_TX), "late entry");
        packet(&state, b, result(tid, 2320001, 0)).await;
        let got = services(&mut a_rx);
        assert_eq!(got.len(), 1);
        let (kind, res) = &got[0];
        assert_eq!(*kind, protocol::SERVICE_HO_RESULT);
        assert_eq!(res["transaction_id"].as_u64(), Some(7), "A's own id");
        assert_eq!((res["status"].as_u64(), res["sdu"].as_str(), res["sdu_len_bits"].as_u64()), (Some(0), Some("0f80"), Some(9)));

        let inner = state.inner.read().await;
        assert_eq!(inner.subscribers[&2320001].client_id, b);
        assert!(inner.subscribers[&2320001].groups.contains(&91));
        assert_eq!(inner.handovers.pending_count(), 0);
        assert!(inner.handovers.recently_handed_over(2320001, Instant::now(), Duration::from_secs(1)));
    }

    #[test]
    fn type1_call_and_channel_parse_and_bad_ones_are_dropped() {
        let base = r#""transaction_id":7,"issi":1,"target_carrier":1530,"target_carrier_ext":268,"sdu":"a1b2c0","sdu_len_bits":18"#;
        assert_eq!(parse_request(&format!(r#"{{{base},"call":{{"gssi":91}}}}"#), true).unwrap().call_gssi, Some(91));
        assert_eq!(parse_request(&format!("{{{base}}}"), true).unwrap().call_gssi, None);
        for bad in [r#""call":{"gssi":16777216}"#, r#""call":{"group":91}"#, r#""call":7"#, r#""call":null"#] {
            let req = parse_request(&format!("{{{base},{bad}}}"), true).unwrap_or_else(|e| panic!("refused {bad}: {e}"));
            assert_eq!(req.call_gssi, None, "{bad}");
        }

        let channel = Channel { call_id: 16383, carrier: 1530, timeslot: 4, usage: 63 };
        let ok = r#"{"transaction_id":7,"issi":1,"status":0,"channel":{"call_id":16383,"carrier":1530,"timeslot":4,"usage":63}}"#;
        assert_eq!(parse_result(ok).unwrap().channel, Some(channel));
        for bad in [
            r#""call_id":16384,"carrier":1530,"timeslot":2,"usage":9"#,
            r#""call_id":5,"carrier":4096,"timeslot":2,"usage":9"#,
            r#""call_id":5,"carrier":1530,"timeslot":0,"usage":9"#,
            r#""call_id":5,"carrier":1530,"timeslot":5,"usage":9"#,
            r#""call_id":5,"carrier":1530,"timeslot":2,"usage":3"#,
            r#""call_id":5,"carrier":1530,"timeslot":2"#,
        ] {
            let res = parse_result(&format!(r#"{{"transaction_id":7,"issi":1,"status":0,"channel":{{{bad}}}}}"#)).unwrap();
            assert_eq!(res.channel, None, "{bad}");
        }
        // Only an accepted handover has a channel.
        let rejected = r#"{"transaction_id":7,"issi":1,"status":1,"channel":{"call_id":5,"carrier":1530,"timeslot":2,"usage":9}}"#;
        assert_eq!(parse_result(rejected).unwrap().channel, None);
    }

    /// Announced type 1: the call A names goes to B, B's channel comes back to A.
    /// Without them nothing new is sent (an older Basestation on either side).
    #[tokio::test]
    async fn type1_call_and_channel_are_relayed() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        let req = protocol::build_service(protocol::SERVICE_HO_FORWARD_REG, &format!(
            r#"{{"transaction_id":3,"issi":2320001,"target_carrier":1530,"target_carrier_ext":{EXT},"sdu":"a1b2c0","sdu_len_bits":18,"call":{{"gssi":91}}}}"#));
        packet(&state, a, req).await;
        let got = services(&mut b_rx);
        assert_eq!(got[0].1["call"]["gssi"].as_u64(), Some(91));
        let tid = got[0].1["transaction_id"].as_u64().unwrap() as u32;
        let res = protocol::build_service(protocol::SERVICE_HO_RESULT, &format!(
            r#"{{"transaction_id":{tid},"issi":2320001,"status":0,"sdu":"0f80","sdu_len_bits":9,"channel":{{"call_id":12,"carrier":1530,"timeslot":3,"usage":9}}}}"#));
        packet(&state, b, res).await;
        let got = services(&mut a_rx);
        assert_eq!(got[0].1["transaction_id"].as_u64(), Some(3));
        assert_eq!(got[0].1["channel"], serde_json::json!({ "call_id": 12, "carrier": 1530, "timeslot": 3, "usage": 9 }));

        packet(&state, a, forward_reg(4, 2320002, 1530)).await;
        let got = services(&mut b_rx);
        assert!(got[0].1.get("call").is_none());
        let tid = got[0].1["transaction_id"].as_u64().unwrap() as u32;
        packet(&state, b, result(tid, 2320002, 0)).await;
        assert!(services(&mut a_rx)[0].1.get("channel").is_none());
    }

    /// A radio registering elsewhere without a handover: the Basestation it
    /// left hears of it only if it announced `ms-moved`, and not after a
    /// handover prepared here (it sent the radio away itself).
    #[tokio::test]
    async fn the_basestation_a_radio_left_unprepared_is_told() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts_with(&state, 1528, &["handover-v1", CAPABILITY_MS_MOVED]).await;
        let (old, mut old_rx) = bts_with(&state, 1532, &["handover-v1"]).await;
        let (b, mut b_rx) = bts_with(&state, 1530, &["handover-v1", CAPABILITY_MS_MOVED]).await;
        packet(&state, a, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
        packet(&state, old, protocol::build_subscriber_message(SUB_REGISTER, 2320002, &[])).await;
        drain(&mut a_rx);
        drain(&mut old_rx);

        packet(&state, b, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
        assert_eq!(services(&mut a_rx), vec![(protocol::SERVICE_MS_MOVED, serde_json::json!({ "issi": 2320001 }))]);
        packet(&state, b, protocol::build_subscriber_message(SUB_REGISTER, 2320002, &[])).await;
        assert!(services(&mut old_rx).is_empty(), "no ms-moved capability, nothing new");

        // Back to A through a prepared handover: B is not told.
        packet(&state, b, forward_reg(5, 2320001, 1528)).await;
        let tid = services(&mut a_rx)[0].1["transaction_id"].as_u64().unwrap() as u32;
        packet(&state, a, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
        packet(&state, a, result(tid, 2320001, 0)).await;
        let got = services(&mut b_rx);
        let types: Vec<u8> = got.iter().map(|(t, _)| *t).collect();
        assert_eq!(types, vec![protocol::SERVICE_HO_RESULT], "a radio handed over is not reported as gone");
    }

    /// The talker of a call through A shows up on B: its over through A ends at
    /// once (the listeners get GROUP_IDLE), so B's GROUP_TX for the same radio
    /// takes the group floor. Only when B speaks `handover-v1`.
    #[tokio::test]
    async fn a_talker_that_left_unprepared_has_its_over_ended() {
        for capable in [true, false] {
            let state = test_state(2000);
            let (a, _a_rx) = bts_with(&state, 1528, &["handover-v1"]).await;
            let b_caps: &[&str] = if capable { &["handover-v1"] } else { &[] };
            let (b, _b_rx) = bts_with(&state, 1530, b_caps).await;
            let (c, mut c_rx) = bts(&state, 1532, false).await;
            packet(&state, a, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
            packet(&state, a, protocol::build_subscriber_message(SUB_AFFILIATE, 2320001, &[91])).await;
            packet(&state, c, protocol::build_subscriber_message(SUB_REGISTER, 2320009, &[])).await;
            packet(&state, c, protocol::build_subscriber_message(SUB_AFFILIATE, 2320009, &[91, 92])).await;
            let over = Uuid::new_v4();
            packet(&state, a, protocol::build_group_tx(&over, 2320001, 91, 0)).await;
            // Another radio's over on A is not touched.
            packet(&state, a, protocol::build_subscriber_message(SUB_REGISTER, 2320002, &[])).await;
            packet(&state, a, protocol::build_subscriber_message(SUB_AFFILIATE, 2320002, &[92])).await;
            let other = Uuid::new_v4();
            packet(&state, a, protocol::build_group_tx(&other, 2320002, 92, 0)).await;
            drain(&mut c_rx);

            packet(&state, b, protocol::build_subscriber_message(SUB_REGISTER, 2320001, &[])).await;
            let to_c = drain(&mut c_rx);
            let idle = to_c.iter().any(|p| p.len() > 1 && p[0] == protocol::CLASS_CALL_CONTROL && p[1] == protocol::CALL_GROUP_IDLE);
            assert_eq!(idle, capable, "GROUP_IDLE to the listeners (B capable: {capable})");
            assert_eq!(state.inner.read().await.calls.contains_key(&over), !capable);
            assert!(state.inner.read().await.calls.contains_key(&other), "another talker's over runs on");

            let again = Uuid::new_v4();
            packet(&state, b, protocol::build_group_tx(&again, 2320001, 91, 0)).await;
            let to_c = drain(&mut c_rx);
            let tx = to_c.iter().any(|p| p.len() > 1 && p[0] == protocol::CLASS_CALL_CONTROL && p[1] == protocol::CALL_GROUP_TX);
            assert_eq!(tx, capable, "the radio talks on from B");
        }
    }

    #[tokio::test]
    async fn prepare_without_registration_and_a_rejection() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, prepare(9, 2320001, 1530)).await;
        let got = services(&mut b_rx);
        assert_eq!(got[0].0, protocol::SERVICE_HO_PREPARE);
        assert!(got[0].1.get("sdu").is_none());
        let tid = got[0].1["transaction_id"].as_u64().unwrap() as u32;
        packet(&state, b, result(tid, 2320001, 1)).await;
        let got = services(&mut a_rx);
        assert_eq!((got[0].1["transaction_id"].as_u64(), got[0].1["status"].as_u64()), (Some(9), Some(1)));
        assert!(!state.inner.read().await.handovers.recently_handed_over(2320001, Instant::now(), Duration::from_secs(60)), "rejected is no handover");
    }

    #[tokio::test]
    async fn no_target_is_answered_at_once() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (_old, mut old_rx) = bts(&state, 1530, false).await;
        let (_other, mut other_rx) = bts(&state, 1534, true).await;
        // Unknown carrier, A's own carrier, a Basestation without handover-v1,
        // another band plan and another network.
        packet(&state, a, forward_reg(1, 2320001, 1532)).await;
        packet(&state, a, forward_reg(2, 2320001, 1528)).await;
        packet(&state, a, forward_reg(3, 2320001, 1530)).await;
        packet(&state, a, protocol::build_service(protocol::SERVICE_HO_PREPARE,
            r#"{"transaction_id":4,"issi":2320001,"target_carrier":1534,"target_carrier_ext":0}"#)).await;
        packet(&state, a, protocol::build_service(protocol::SERVICE_HO_PREPARE,
            r#"{"transaction_id":5,"issi":2320001,"target_carrier":1534,"target_carrier_ext":268,"target_mnc":6}"#)).await;
        let got = services(&mut a_rx);
        let answers: Vec<_> = got.iter().map(|(k, v)| (*k, v["transaction_id"].as_u64().unwrap(), v["status"].as_u64().unwrap())).collect();
        assert_eq!(answers, (1..=5).map(|t| (protocol::SERVICE_HO_RESULT, t, 2)).collect::<Vec<_>>());
        let reasons: Vec<_> = got.iter().map(|(_, v)| v["reason"].as_str().unwrap().to_string()).collect();
        assert_eq!(reasons, ["unknown", "own_cell", "not_capable", "unknown", "unknown"]);
        assert!(services(&mut old_rx).is_empty(), "a Basestation without handover-v1 never sees a handover message");
        assert!(services(&mut other_rx).is_empty());
        assert_eq!(state.inner.read().await.handovers.pending_count(), 0);
    }

    #[tokio::test]
    async fn target_that_does_not_answer_times_out() {
        let state = test_state(30);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, forward_reg(7, 2320001, 1530)).await;
        let tid = services(&mut b_rx)[0].1["transaction_id"].as_u64().unwrap() as u32;
        tokio::time::sleep(Duration::from_millis(120)).await;
        let got = services(&mut a_rx);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].1["transaction_id"].as_u64(), got[0].1["status"].as_u64()), (Some(7), Some(3)));
        // B answers too late: dropped, A hears nothing more.
        packet(&state, b, result(tid, 2320001, 0)).await;
        assert!(services(&mut a_rx).is_empty());
        assert!(!state.inner.read().await.handovers.recently_handed_over(2320001, Instant::now(), Duration::from_secs(60)));
    }

    #[tokio::test]
    async fn answer_in_time_is_not_followed_by_a_timeout() {
        let state = test_state(30);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, forward_reg(7, 2320001, 1530)).await;
        let tid = services(&mut b_rx)[0].1["transaction_id"].as_u64().unwrap() as u32;
        packet(&state, b, result(tid, 2320001, 0)).await;
        tokio::time::sleep(Duration::from_millis(120)).await;
        let got = services(&mut a_rx);
        assert_eq!(got.iter().map(|(_, v)| v["status"].as_u64().unwrap()).collect::<Vec<_>>(), vec![0]);
    }

    #[tokio::test]
    async fn target_going_away_answers_no_target() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, forward_reg(7, 2320001, 1530)).await;
        assert_eq!(services(&mut b_rx).len(), 1);
        state.cleanup_client(b).await;
        let got = services(&mut a_rx);
        assert_eq!((got[0].1["transaction_id"].as_u64(), got[0].1["status"].as_u64()), (Some(7), Some(2)));
        assert_eq!(got[0].1["reason"], "gone", "A must not send the radio to a target that went away");
        // And the target is gone from the registry: the next request is answered at once.
        packet(&state, a, forward_reg(8, 2320001, 1530)).await;
        let got = services(&mut a_rx);
        assert_eq!((got[0].1["status"].as_u64(), got[0].1["reason"].as_str()), (Some(2), Some("unknown")));
    }

    #[tokio::test]
    async fn serving_cell_going_away_drops_its_requests() {
        let state = test_state(2000);
        let (a, _a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, forward_reg(7, 2320001, 1530)).await;
        let tid = services(&mut b_rx)[0].1["transaction_id"].as_u64().unwrap() as u32;
        state.cleanup_client(a).await;
        assert_eq!(state.inner.read().await.handovers.pending_count(), 0);
        packet(&state, b, result(tid, 2320001, 0)).await;
    }

    #[tokio::test]
    async fn only_the_target_can_answer() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        let (x, _x_rx) = bts(&state, 1534, true).await;
        packet(&state, a, forward_reg(7, 2320001, 1530)).await;
        let tid = services(&mut b_rx)[0].1["transaction_id"].as_u64().unwrap() as u32;
        packet(&state, x, result(tid, 2320001, 0)).await;
        packet(&state, a, result(tid, 2320001, 0)).await;
        assert!(services(&mut a_rx).is_empty());
        packet(&state, b, result(tid, 2320001, 0)).await;
        assert_eq!(services(&mut a_rx).len(), 1);
    }

    #[tokio::test]
    async fn two_serving_cells_with_the_same_id_do_not_clash() {
        let state = test_state(2000);
        let (a1, mut a1_rx) = bts(&state, 1528, true).await;
        let (a2, mut a2_rx) = bts(&state, 1532, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a1, forward_reg(1, 2320001, 1530)).await;
        packet(&state, a2, forward_reg(1, 2320002, 1530)).await;
        let got = services(&mut b_rx);
        let ids: Vec<(u64, u32)> = got.iter().map(|(_, v)| (v["issi"].as_u64().unwrap(), v["transaction_id"].as_u64().unwrap() as u32)).collect();
        assert_ne!(ids[0].1, ids[1].1);
        packet(&state, b, result(ids[1].1, 2320002, 1)).await;
        packet(&state, b, result(ids[0].1, 2320001, 0)).await;
        assert_eq!(services(&mut a1_rx).iter().map(|(_, v)| (v["issi"].as_u64().unwrap(), v["status"].as_u64().unwrap())).collect::<Vec<_>>(), vec![(2320001, 0)]);
        assert_eq!(services(&mut a2_rx).iter().map(|(_, v)| (v["issi"].as_u64().unwrap(), v["status"].as_u64().unwrap())).collect::<Vec<_>>(), vec![(2320002, 1)]);
    }

    #[tokio::test]
    async fn requests_from_connections_without_the_capability_are_ignored() {
        let state = test_state(2000);
        let (old, mut old_rx) = bts(&state, 1528, false).await;
        let (_b, mut b_rx) = bts(&state, 1530, true).await;
        let (term, mut term_rx) = connect(&state, ClientMode::Terminal).await;
        let (silent, mut silent_rx) = connect(&state, ClientMode::Basestation).await;
        for from in [old, term, silent] {
            packet(&state, from, forward_reg(7, 2320001, 1530)).await;
        }
        assert!(services(&mut b_rx).is_empty());
        assert!(services(&mut old_rx).is_empty() && services(&mut term_rx).is_empty() && services(&mut silent_rx).is_empty());
        assert_eq!(state.inner.read().await.handovers.pending_count(), 0);
    }

    #[tokio::test]
    async fn malformed_messages_change_nothing() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts(&state, 1528, true).await;
        let (b, mut b_rx) = bts(&state, 1530, true).await;
        packet(&state, a, protocol::build_service(protocol::SERVICE_HO_FORWARD_REG, r#"{"transaction_id":1}"#)).await;
        packet(&state, b, protocol::build_service(protocol::SERVICE_HO_RESULT, "{")).await;
        assert!(services(&mut a_rx).is_empty() && services(&mut b_rx).is_empty());
    }

    /// A Basestation with one cell on `carrier` in location area `la`.
    async fn bts_in_la(state: &Arc<AppState>, carrier: u16, la: u16) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (id, rx) = connect(state, ClientMode::Basestation).await;
        let json = format!(r#"{{"mcc":232,"mnc":5,"cells":[{{"carrier":{carrier},"carrier_ext":{EXT},"location_area":{la}}}],"capabilities":["handover-v1"]}}"#);
        crate::router::handle_packet(state.clone(), id, protocol::build_service(protocol::SERVICE_BTS_INFO, &json)).await;
        (id, rx)
    }

    fn prepare_in_la(tid: u32, carrier: u16, la: u16) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_HO_PREPARE, &format!(
            r#"{{"transaction_id":{tid},"issi":2320001,"target_carrier":{carrier},"target_carrier_ext":{EXT},"target_location_area":{la}}}"#))
    }

    #[test]
    fn parses_the_target_location_area() {
        let req = parse_request(r#"{"transaction_id":7,"issi":1,"target_carrier":1530,"target_carrier_ext":268,"target_location_area":16383}"#, false).unwrap();
        assert_eq!(req.target_location_area, Some(16383));
        assert_eq!(parse_request(r#"{"transaction_id":7,"issi":1,"target_carrier":1530,"target_carrier_ext":268}"#, false).unwrap().target_location_area, None);
        assert!(parse_request(r#"{"transaction_id":7,"issi":1,"target_carrier":1530,"target_carrier_ext":268,"target_location_area":16384}"#, false).is_err());
    }

    #[tokio::test]
    async fn a_reused_carrier_is_told_apart_by_the_location_area() {
        let state = test_state(2000);
        let (a, mut a_rx) = bts_in_la(&state, 1528, 2).await;
        // Two Basestations reuse carrier 1530 in different location areas;
        // the one in LA 7 announced last.
        let (near, mut near_rx) = bts_in_la(&state, 1530, 1).await;
        let (far, mut far_rx) = bts_in_la(&state, 1530, 7).await;
        assert_eq!(state.inner.read().await.carriers.owner(&CarrierKey { mcc: 232, mnc: 5, carrier: 1530, carrier_ext: EXT }), Some(far));
        packet(&state, a, prepare_in_la(1, 1530, 1)).await;
        assert!(services(&mut far_rx).is_empty(), "the newest claimant is not the neighbour in LA 1");
        let got = services(&mut near_rx);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1["target_location_area"].as_u64(), Some(1));
        // No cell on the carrier in that LA: no target, nobody else is asked.
        packet(&state, a, prepare_in_la(2, 1530, 3)).await;
        let got = services(&mut a_rx);
        assert_eq!((got[0].1["transaction_id"].as_u64(), got[0].1["status"].as_u64(), got[0].1["reason"].as_str()), (Some(2), Some(2), Some("unknown")));
        assert!(services(&mut near_rx).is_empty() && services(&mut far_rx).is_empty());
        // Without a location area the newest claimant serves, as before, and
        // is told which of its cells is meant.
        packet(&state, a, prepare(3, 2320001, 1530)).await;
        assert_eq!(services(&mut far_rx)[0].1["target_location_area"].as_u64(), Some(7));
        let _ = near;
    }

    #[test]
    fn kick_guard_outlasts_the_kick_delay() {
        let config = crate::config::Config::default();
        assert!(kick_guard(&config) > Duration::from_millis(config.roaming.kick_delay_ms));
    }
}
