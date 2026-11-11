//! SMS Center: store-and-forward for individual SDS.
//!
//! Without this, an SDS addressed to an ISSI that is not registered anywhere
//! on the Brew network is dropped by `router::handle_sds_header` ("SDS has no
//! registered destination"). With the SMS Center enabled the message is
//! instead written to a JSON file and delivered as soon as that ISSI
//! registers again, on any Basestation or federation peer.
//!
//! Flow:
//! 1. `SHORT_TRANSFER` arrives with no route -> the router asks [`SmsCenter::wants`]
//!    and flags the `SdsRoute` as `store_offline`.
//! 2. The following `SDS_TRANSFER` carries the payload -> [`SmsCenter::store`].
//!    If the sender asked for an SDS-TL delivery report, it is told
//!    "destination not reachable, message stored" (0x22).
//! 3. The destination registers (`SUB_REGISTER`/`SUB_REREGISTER`) ->
//!    [`deliver_pending`] originates the stored SDS towards the destination's
//!    Basestation as a normal `SHORT_TRANSFER` + `SDS_TRANSFER`, from the
//!    original source ISSI, so the receiving radio sees the real sender and
//!    its own end-to-end SDS-TL report goes back to the original sender.
//! 4. The destination Basestation answers with `SDS_REPORT` ->
//!    [`SmsCenter::on_report`] removes the message from the queue. No report
//!    within `ack_timeout_seconds` -> retried later ([`run`] sweep).
//!
//! Storage is a single human-readable JSON file, rewritten atomically
//! (write to `<path>.tmp`, then rename) on every change.

use crate::{
    config::SmsCenterConfig,
    protocol,
    state::{AppState, ClientId, SdsRoute},
    telemetry::now_ms,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Pseudo client id used as the `source_client` of SDS routes the SMS Center
/// originates, so the destination's `SDS_REPORT` is matched back to us by
/// `router::handle_sds_report` (no real client ever has the nil uuid).
pub const SMS_CENTER_CLIENT: ClientId = Uuid::nil();

/// SDS-TL delivery status codes (ETSI EN 300 392-2, clause 29.4.3.2).
pub mod status {
    /// ETSI "destination not reachable, message stored"; see `stored_report_status`.
    #[allow(dead_code)]
    pub const DEST_NOT_REACHABLE_STORED: u8 = 0x22;
    pub const VALIDITY_EXPIRED_NOT_RECEIVED: u8 = 0x48;
    pub const DELIVERY_FAILED: u8 = 0x4A;
    pub const DEST_QUEUE_FULL: u8 = 0x4C;
}

/// One queued message, as persisted in the JSON file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSms {
    pub id: Uuid,
    pub source_issi: u32,
    pub destination: u32,
    pub length_bits: u16,
    /// SDS user data (Type 4 payload, starting with the protocol identifier), hex.
    pub data_hex: String,
    /// Best-effort decoded text, for the dashboard only.
    #[serde(default)]
    pub text: Option<String>,
    pub stored_at_ms: u64,
    pub expires_at_ms: u64,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub last_attempt_ms: Option<u64>,
    /// Brew uuid of the delivery attempt currently awaiting an `SDS_REPORT`.
    #[serde(default)]
    pub in_flight: Option<Uuid>,
}

impl StoredSms {
    pub fn data(&self) -> Vec<u8> {
        hex_decode(&self.data_hex).unwrap_or_default()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SmsFile {
    #[serde(default)]
    version: u32,
    /// Every ISSI that has registered on this server at least once.
    #[serde(default)]
    known_issis: BTreeSet<u32>,
    #[serde(default)]
    messages: Vec<StoredSms>,
}

/// Result of [`SmsCenter::store`].
#[derive(Debug, Clone, PartialEq)]
pub enum StoreOutcome {
    Stored(StoredSms),
    /// Same source/destination/payload already queued recently (a radio
    /// retransmission): not stored twice.
    Duplicate(StoredSms),
    QueueFull,
    Skipped(&'static str),
}

/// Messages removed by [`SmsCenter::sweep`].
#[derive(Debug, Default)]
pub struct SweepResult {
    pub expired: Vec<StoredSms>,
    pub failed: Vec<StoredSms>,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub enabled: bool,
    pub known_issis: usize,
    pub messages: Vec<StoredSms>,
}

pub struct SmsCenter {
    cfg: SmsCenterConfig,
    data: Mutex<SmsFile>,
    /// Bumped on every change, so HA replication knows when to resend.
    revision: AtomicU64,
}

/// Window in which an identical SDS is treated as a retransmission.
const DUPLICATE_WINDOW_MS: u64 = 60_000;

impl SmsCenter {
    /// Loads the queue from `cfg.path` (missing file = empty queue). A file
    /// that cannot be parsed is moved aside to `<path>.corrupt-<ms>` so it is
    /// never silently overwritten.
    pub fn open(cfg: SmsCenterConfig) -> Self {
        let data = if cfg.enabled { load(&cfg.path) } else { SmsFile::default() };
        if cfg.enabled {
            info!(path = %cfg.path.display(), queued = data.messages.len(), known = data.known_issis.len(), "SMS Center enabled");
        }
        Self { cfg, data: Mutex::new(data), revision: AtomicU64::new(1) }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    pub fn config(&self) -> &SmsCenterConfig {
        &self.cfg
    }

    /// Records that `issi` registered, making it an eligible destination when
    /// `only_known_destinations` is set.
    pub fn note_known(&self, issi: u32) {
        if !self.cfg.enabled {
            return;
        }
        let mut d = self.data.lock().unwrap();
        if d.known_issis.insert(issi) {
            self.persist(&d);
        }
    }

    /// Whether an SDS to an unreachable `destination` should be kept.
    pub fn wants(&self, destination: u32) -> bool {
        if !self.cfg.enabled {
            return false;
        }
        !self.cfg.only_known_destinations || self.data.lock().unwrap().known_issis.contains(&destination)
    }

    pub fn has_pending_for(&self, issi: u32) -> bool {
        self.cfg.enabled && self.data.lock().unwrap().messages.iter().any(|m| m.destination == issi)
    }

    pub fn store(&self, source_issi: u32, destination: u32, length_bits: u16, data: &[u8], now: u64) -> StoreOutcome {
        if !self.cfg.enabled {
            return StoreOutcome::Skipped("disabled");
        }
        let Some(&pid) = data.first() else { return StoreOutcome::Skipped("empty payload") };
        if self.cfg.exclude_protocol_ids.contains(&pid) {
            return StoreOutcome::Skipped("excluded protocol id");
        }
        if (length_bits as usize).div_ceil(8) > data.len() {
            return StoreOutcome::Skipped("length exceeds payload");
        }
        let data_hex = hex_encode(data);
        let mut d = self.data.lock().unwrap();
        if let Some(existing) = d.messages.iter().find(|m| {
            m.source_issi == source_issi
                && m.destination == destination
                && m.data_hex == data_hex
                && now.saturating_sub(m.stored_at_ms) < DUPLICATE_WINDOW_MS
        }) {
            return StoreOutcome::Duplicate(existing.clone());
        }
        let per_dest = d.messages.iter().filter(|m| m.destination == destination).count();
        if per_dest >= self.cfg.max_messages_per_destination || d.messages.len() >= self.cfg.max_messages_total {
            return StoreOutcome::QueueFull;
        }
        let msg = StoredSms {
            id: Uuid::new_v4(),
            source_issi,
            destination,
            length_bits,
            data_hex,
            text: text_preview(data),
            stored_at_ms: now,
            expires_at_ms: now.saturating_add(self.cfg.message_ttl_seconds.saturating_mul(1000)),
            attempts: 0,
            last_attempt_ms: None,
            in_flight: None,
        };
        d.messages.push(msg.clone());
        self.persist(&d);
        StoreOutcome::Stored(msg)
    }

    /// Picks the oldest message for `issi` that is not awaiting a report and
    /// marks it in flight under a fresh Brew uuid. `force` ignores the retry
    /// interval (used right after the destination registers).
    pub fn take_next_due(&self, issi: u32, force: bool, now: u64) -> Option<StoredSms> {
        if !self.cfg.enabled {
            return None;
        }
        let retry_ms = self.cfg.retry_interval_seconds.saturating_mul(1000);
        let mut d = self.data.lock().unwrap();
        let msg = d
            .messages
            .iter_mut()
            .filter(|m| m.destination == issi && m.in_flight.is_none() && now < m.expires_at_ms)
            .filter(|m| force || m.last_attempt_ms.is_none_or(|t| now.saturating_sub(t) >= retry_ms))
            .min_by_key(|m| m.stored_at_ms)?;
        msg.in_flight = Some(Uuid::new_v4());
        msg.attempts += 1;
        msg.last_attempt_ms = Some(now);
        let out = msg.clone();
        self.persist(&d);
        Some(out)
    }

    /// Destinations that currently have a message waiting that is due for a
    /// (re)try.
    pub fn due_destinations(&self, now: u64) -> HashSet<u32> {
        if !self.cfg.enabled {
            return HashSet::new();
        }
        let retry_ms = self.cfg.retry_interval_seconds.saturating_mul(1000);
        self.data
            .lock()
            .unwrap()
            .messages
            .iter()
            .filter(|m| m.in_flight.is_none() && m.last_attempt_ms.is_none_or(|t| now.saturating_sub(t) >= retry_ms))
            .map(|m| m.destination)
            .collect()
    }

    /// An attempt could not even be sent (destination vanished): release it
    /// without counting it as an attempt.
    pub fn release(&self, brew_uuid: Uuid) {
        let mut d = self.data.lock().unwrap();
        if let Some(m) = d.messages.iter_mut().find(|m| m.in_flight == Some(brew_uuid)) {
            m.in_flight = None;
            m.attempts = m.attempts.saturating_sub(1);
            self.persist(&d);
        }
    }

    /// The destination Basestation acknowledged delivery attempt `brew_uuid`:
    /// the message is done and removed from the queue.
    pub fn on_report(&self, brew_uuid: Uuid) -> Option<StoredSms> {
        let mut d = self.data.lock().unwrap();
        let idx = d.messages.iter().position(|m| m.in_flight == Some(brew_uuid))?;
        let msg = d.messages.remove(idx);
        self.persist(&d);
        Some(msg)
    }

    /// Housekeeping: drops expired messages, frees attempts whose report
    /// never arrived, and gives up on messages past `max_attempts`.
    pub fn sweep(&self, now: u64) -> SweepResult {
        let mut out = SweepResult::default();
        if !self.cfg.enabled {
            return out;
        }
        let ack_ms = self.cfg.ack_timeout_seconds.saturating_mul(1000);
        let mut d = self.data.lock().unwrap();
        let mut changed = false;
        let mut keep = Vec::with_capacity(d.messages.len());
        for mut m in std::mem::take(&mut d.messages) {
            if m.in_flight.is_some() && m.last_attempt_ms.is_none_or(|t| now.saturating_sub(t) >= ack_ms) {
                m.in_flight = None;
                changed = true;
            }
            if m.in_flight.is_none() && now >= m.expires_at_ms {
                out.expired.push(m);
                changed = true;
            } else if m.in_flight.is_none() && self.cfg.max_attempts > 0 && m.attempts >= self.cfg.max_attempts {
                out.failed.push(m);
                changed = true;
            } else {
                keep.push(m);
            }
        }
        d.messages = keep;
        if changed {
            self.persist(&d);
        }
        out
    }

    pub fn delete(&self, id: Uuid) -> bool {
        let mut d = self.data.lock().unwrap();
        let before = d.messages.len();
        d.messages.retain(|m| m.id != id);
        let removed = d.messages.len() != before;
        if removed {
            self.persist(&d);
        }
        removed
    }

    pub fn snapshot(&self) -> Snapshot {
        let d = self.data.lock().unwrap();
        Snapshot { enabled: self.cfg.enabled, known_issis: d.known_issis.len(), messages: d.messages.clone() }
    }

    /// Change counter for HA replication (see `export`).
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// The whole queue and known-ISSI set as JSON, for HA replication.
    pub fn export(&self) -> Vec<u8> {
        let d = self.data.lock().unwrap();
        let file = SmsFile { version: 1, known_issis: d.known_issis.clone(), messages: d.messages.clone() };
        serde_json::to_vec(&file).unwrap_or_default()
    }

    /// Replaces the queue with one exported by the HA Active node, and saves
    /// it, so this node continues from the same queue if it takes over.
    pub fn import(&self, json: &[u8]) -> anyhow::Result<()> {
        let file: SmsFile = serde_json::from_slice(json)?;
        let mut d = self.data.lock().unwrap();
        *d = file;
        self.persist(&d);
        Ok(())
    }

    fn persist(&self, d: &SmsFile) {
        self.revision.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = save(&self.cfg.path, d) {
            error!(path = %self.cfg.path.display(), error = %e, "SMS Center: cannot write queue file");
        }
    }
}

fn load(path: &Path) -> SmsFile {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<SmsFile>(&bytes) {
            Ok(f) => f,
            Err(e) => {
                let aside = path.with_extension(format!("corrupt-{}", now_ms()));
                error!(path = %path.display(), error = %e, moved_to = %aside.display(), "SMS Center: queue file unreadable; starting empty");
                let _ = std::fs::rename(path, &aside);
                SmsFile::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SmsFile::default(),
        Err(e) => {
            error!(path = %path.display(), error = %e, "SMS Center: cannot read queue file; starting empty");
            SmsFile::default()
        }
    }
}

fn save(path: &Path, d: &SmsFile) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = SmsFile { version: 1, known_issis: d.known_issis.clone(), messages: d.messages.clone() };
    let json = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

// ─── SDS-TL helpers ──────────────────────────────────────────────────────

/// The parts of an SDS-TL header the SMS Center needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdsTlInfo {
    pub protocol_id: u8,
    /// SDS-TRANSFER (as opposed to SDS-REPORT / SDS-ACK / ...).
    pub is_transfer: bool,
    /// Sender asked for a "received" and/or "consumed" report.
    pub report_requested: bool,
    pub message_reference: u8,
}

/// Parses the SDS-TL header of a Type 4 payload. `None` for non-TL protocol
/// identifiers (< 0x80) or a payload too short to carry a header.
///
/// Byte layout: `[PID, msg_type(4) | delivery_report_request(2) |
/// short_form_report(1) | store_forward_control(1), message_reference, ...]`
/// for SDS-TRANSFER; SDS-REPORT is `[PID, 0x1x, delivery_status, message_reference]`.
pub fn sds_tl_info(data: &[u8]) -> Option<SdsTlInfo> {
    let (&pid, rest) = data.split_first()?;
    if pid < 0x80 || rest.len() < 2 {
        return None;
    }
    let msg_type = rest[0] >> 4;
    let is_transfer = msg_type == 0;
    let message_reference = if is_transfer { rest[1] } else { *rest.get(2)? };
    Some(SdsTlInfo {
        protocol_id: pid,
        is_transfer,
        report_requested: is_transfer && (rest[0] >> 2) & 0x03 != 0,
        message_reference,
    })
}

/// SDS-TL SDS-REPORT payload: `[PID, 0x10 (report, no ack required), status, MR]`.
/// Same shape FlowStation uses for its own failure reports.
pub fn build_tl_report(protocol_id: u8, delivery_status: u8, message_reference: u8) -> Vec<u8> {
    vec![protocol_id, 0x10, delivery_status, message_reference]
}

/// Best-effort text extraction for the dashboard.
pub fn text_preview(data: &[u8]) -> Option<String> {
    let body: &[u8] = match *data.first()? {
        // Simple / immediate text without SDS-TL: [PID, coding, text...]
        0x02 | 0x09 => data.get(2..)?,
        // Same with SDS-TL transfer header: [PID, flags, MR, (ts|coding), (timestamp[3]), text...]
        0x82 | 0x89 => {
            let info = sds_tl_info(data)?;
            if !info.is_transfer || data[1] & 0x01 != 0 {
                return None; // report, or store/forward control present (variable length)
            }
            let coding = *data.get(3)?;
            let start = if coding & 0x80 != 0 { 7 } else { 4 };
            data.get(start..)?
        }
        _ => return None,
    };
    let text: String = body.iter().map(|&b| b as char).filter(|c| !c.is_control()).collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

// ─── Network side ────────────────────────────────────────────────────────

/// Sends an SDS originated by the SMS Center to `client`, registering an
/// `SdsRoute` so that client's `SDS_REPORT` reaches [`SmsCenter::on_report`].
async fn originate(state: &Arc<AppState>, client: ClientId, brew_uuid: Uuid, source_issi: u32, destination: u32, length_bits: u16, data: &[u8]) -> bool {
    if state.is_blocked(destination) {
        return false; // stays queued; delivered if the ISSI is taken off the blacklist
    }
    let tx = {
        let mut inner = state.inner.write().await;
        let Some(tx) = inner.clients.get(&client).map(|c| c.tx.clone()) else { return false };
        inner.sds_routes.insert(
            brew_uuid,
            SdsRoute {
                source_client: SMS_CENTER_CLIENT,
                targets: HashSet::from([client]),
                source_issi,
                destination,
                created_at: Instant::now(),
                store_offline: false,
                lip_only: false,
                held_header: None,
                no_lip: HashSet::new(),
                from_no_lip: false,
            },
        );
        tx
    };
    let ok = tx.send(protocol::build_short_transfer(&brew_uuid, source_issi, destination)).is_ok()
        && tx.send(protocol::build_sds_transfer_frame(&brew_uuid, length_bits, data)).is_ok();
    if ok {
        state.monitor.sds(brew_uuid, source_issi, destination).await;
    }
    ok
}

/// Where `issi` is registered right now, if anywhere.
async fn client_of(state: &Arc<AppState>, issi: u32) -> Option<ClientId> {
    let inner = state.inner.read().await;
    let cid = inner.subscribers.get(&issi)?.client_id;
    inner.clients.contains_key(&cid).then_some(cid)
}

/// Sends an SDS-TL status report about `msg` back to its originator, as if
/// from the destination ISSI (which is how the originating radio matches it
/// to the message it sent). Only when the originator asked for a report and
/// is registered right now.
pub async fn report_to_originator(state: &Arc<AppState>, source_issi: u32, destination: u32, data: &[u8], delivery_status: u8, via: Option<ClientId>) {
    if !state.sms_center.config().send_status_reports {
        return;
    }
    let Some(tl) = sds_tl_info(data) else { return };
    if !tl.report_requested {
        return;
    }
    let client = match via {
        Some(c) => Some(c),
        None => client_of(state, source_issi).await,
    };
    let Some(client) = client else { return };
    let report = build_tl_report(tl.protocol_id, delivery_status, tl.message_reference);
    let brew_uuid = Uuid::new_v4();
    if originate(state, client, brew_uuid, destination, source_issi, 32, &report).await {
        info!(to = source_issi, about = destination, mr = tl.message_reference, status = format!("0x{delivery_status:02x}"), "SMS Center: sent SDS-TL status report to originator");
    }
}

/// Delivers every queued message for `issi`, one at a time, while it stays
/// registered. Spawned when `issi` registers (with `force`) and by the
/// periodic sweep for retries.
pub async fn deliver_pending(state: Arc<AppState>, issi: u32, force: bool) {
    let cfg = state.sms_center.config().clone();
    if force && cfg.delivery_delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(cfg.delivery_delay_ms)).await;
    }
    let mut first = true;
    loop {
        let Some(client) = client_of(&state, issi).await else { break };
        let Some(msg) = state.sms_center.take_next_due(issi, force, now_ms()) else { break };
        if !first && cfg.delivery_spacing_ms > 0 {
            tokio::time::sleep(Duration::from_millis(cfg.delivery_spacing_ms)).await;
        }
        first = false;
        let brew_uuid = msg.in_flight.expect("take_next_due sets in_flight");
        if originate(&state, client, brew_uuid, msg.source_issi, msg.destination, msg.length_bits, &msg.data()).await {
            info!(id = %msg.id, uuid = %brew_uuid, source_issi = msg.source_issi, destination = issi, attempt = msg.attempts, "SMS Center: delivering stored SDS");
        } else {
            state.sms_center.release(brew_uuid);
            debug!(destination = issi, "SMS Center: destination client went away before delivery");
            break;
        }
    }
}

/// Background task: expiry, report timeouts and retries.
pub async fn run(state: Arc<AppState>) {
    if !state.sms_center.enabled() {
        return;
    }
    let mut ticker = tokio::time::interval(Duration::from_secs(10));
    loop {
        ticker.tick().await;
        let now = now_ms();
        let swept = state.sms_center.sweep(now);
        for m in swept.expired {
            warn!(id = %m.id, source_issi = m.source_issi, destination = m.destination, "SMS Center: message expired undelivered");
            report_to_originator(&state, m.source_issi, m.destination, &m.data(), status::VALIDITY_EXPIRED_NOT_RECEIVED, None).await;
        }
        for m in swept.failed {
            warn!(id = %m.id, source_issi = m.source_issi, destination = m.destination, attempts = m.attempts, "SMS Center: giving up after max attempts");
            report_to_originator(&state, m.source_issi, m.destination, &m.data(), status::DELIVERY_FAILED, None).await;
        }
        for issi in state.sms_center.due_destinations(now) {
            if client_of(&state, issi).await.is_some() {
                tokio::spawn(deliver_pending(state.clone(), issi, false));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(name: &str) -> SmsCenterConfig {
        SmsCenterConfig {
            path: std::env::temp_dir().join(format!("sms-center-test-{name}-{}.json", Uuid::new_v4().simple())),
            ..SmsCenterConfig::default()
        }
    }

    // "Hello" as SDS-TL simple text, delivery report requested (drr=01), MR=7, ISO-8859-1.
    const TEXT: [u8; 9] = [0x82, 0x04, 0x07, 0x01, b'H', b'e', b'l', b'l', b'o'];

    #[test]
    fn parses_sds_tl_header() {
        let i = sds_tl_info(&TEXT).unwrap();
        assert_eq!(i, SdsTlInfo { protocol_id: 0x82, is_transfer: true, report_requested: true, message_reference: 7 });
        let r = sds_tl_info(&build_tl_report(0x82, 0x00, 7)).unwrap();
        assert!(!r.is_transfer && !r.report_requested);
        assert_eq!(r.message_reference, 7);
        assert!(sds_tl_info(&[0x02, 0x01, b'x']).is_none(), "non-TL pid");
        let no_report = [0x82, 0x00, 0x09, 0x01, b'x'];
        assert!(!sds_tl_info(&no_report).unwrap().report_requested);
    }

    #[test]
    fn previews_text() {
        assert_eq!(text_preview(&TEXT).as_deref(), Some("Hello"));
        assert_eq!(text_preview(&[0x02, 0x01, b'H', b'i']).as_deref(), Some("Hi"));
        // with timestamp flag set: 3 timestamp bytes skipped
        assert_eq!(text_preview(&[0x82, 0x04, 0x01, 0x81, 1, 2, 3, b'O', b'K']).as_deref(), Some("OK"));
        assert!(text_preview(&[0x0A, 1, 2, 3]).is_none());
    }

    #[test]
    fn store_deliver_ack_cycle_persists() {
        let c = cfg("cycle");
        let path = c.path.clone();
        let sc = SmsCenter::open(c.clone());
        assert!(!sc.wants(2002), "unknown destinations are not stored by default");
        sc.note_known(2002);
        assert!(sc.wants(2002));

        let StoreOutcome::Stored(m) = sc.store(1001, 2002, 72, &TEXT, 1_000) else { panic!() };
        assert!(matches!(sc.store(1001, 2002, 72, &TEXT, 2_000), StoreOutcome::Duplicate(_)));

        // Survives a restart.
        drop(sc);
        let sc = SmsCenter::open(c);
        let snap = sc.snapshot();
        assert_eq!(snap.messages.len(), 1);
        assert_eq!(snap.messages[0].data(), TEXT.to_vec());
        assert_eq!(snap.messages[0].text.as_deref(), Some("Hello"));

        let t = sc.take_next_due(2002, true, 5_000).unwrap();
        assert_eq!(t.id, m.id);
        assert_eq!(t.attempts, 1);
        assert!(sc.take_next_due(2002, true, 5_001).is_none(), "in flight, not taken twice");
        assert_eq!(sc.on_report(t.in_flight.unwrap()).unwrap().id, m.id);
        assert!(sc.snapshot().messages.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn excluded_pid_and_limits() {
        let mut c = cfg("limits");
        c.max_messages_per_destination = 1;
        let path = c.path.clone();
        let sc = SmsCenter::open(c);
        assert!(matches!(sc.store(1, 2, 32, &[0x0A, 1, 2, 3], 0), StoreOutcome::Skipped(_)));
        assert!(matches!(sc.store(1, 2, 72, &TEXT, 0), StoreOutcome::Stored(_)));
        assert_eq!(sc.store(1, 2, 16, &[0x82, 0x04], 0), StoreOutcome::QueueFull);
        assert!(matches!(sc.store(1, 2, 64, &[0x82], 0), StoreOutcome::Skipped(_)), "length > payload");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn sweep_times_out_expires_and_fails() {
        let mut c = cfg("sweep");
        c.message_ttl_seconds = 100;
        c.ack_timeout_seconds = 10;
        c.retry_interval_seconds = 20;
        c.max_attempts = 2;
        let path = c.path.clone();
        let sc = SmsCenter::open(c);
        sc.store(1, 2, 72, &TEXT, 0);

        let a1 = sc.take_next_due(2, true, 1_000).unwrap();
        assert!(sc.sweep(5_000).expired.is_empty());
        assert!(sc.snapshot().messages[0].in_flight.is_some(), "still within ack timeout");
        sc.sweep(12_000); // ack timeout -> released for retry
        assert!(sc.snapshot().messages[0].in_flight.is_none());
        assert!(sc.take_next_due(2, false, 15_000).is_none(), "retry interval not elapsed");
        assert!(sc.due_destinations(15_000).is_empty());
        assert!(sc.due_destinations(21_000).contains(&2));
        let a2 = sc.take_next_due(2, false, 21_000).unwrap();
        assert_ne!(a1.in_flight, a2.in_flight);
        let r = sc.sweep(40_000); // second attempt timed out, max_attempts reached
        assert_eq!(r.failed.len(), 1);
        assert!(sc.snapshot().messages.is_empty());

        sc.store(1, 2, 72, &TEXT, 200_000);
        let r = sc.sweep(200_000 + 100_000);
        assert_eq!(r.expired.len(), 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let c = cfg("corrupt");
        std::fs::write(&c.path, b"{not json").unwrap();
        let sc = SmsCenter::open(c.clone());
        assert!(sc.snapshot().messages.is_empty());
        assert!(!c.path.exists(), "corrupt file renamed, not overwritten in place");
    }

    #[tokio::test]
    async fn end_to_end_offline_then_online() {
        use crate::{protocol::*, state::{Client, ClientMode}};
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center = cfg("e2e");
        config.sms_center.delivery_delay_ms = 0;
        let path = config.sms_center.path.clone();
        let (state, _rx) = AppState::new(config, std::path::PathBuf::from("test.toml"));
        let state = Arc::new(state);

        let add = |_: ()| {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let id = Uuid::new_v4();
            (id, tx, rx)
        };
        let (a, a_tx, mut a_rx) = add(());
        let (b, b_tx, mut b_rx) = add(());
        {
            let mut inner = state.inner.write().await;
            for (id, tx) in [(a, a_tx), (b, b_tx)] {
                inner.clients.insert(id, Client { tx, mode: ClientMode::Basestation, version: ConnVersion::V1, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
            }
        }
        let pkt = |st: &Arc<AppState>, from: Uuid, raw: Vec<u8>| crate::router::handle_packet(st.clone(), from, raw);

        // 2002 has been seen before, then goes offline.
        pkt(&state, a, build_subscriber_message(SUB_REGISTER, 1001, &[])).await;
        pkt(&state, b, build_subscriber_message(SUB_REGISTER, 2002, &[])).await;
        pkt(&state, b, build_subscriber_message(SUB_DEREGISTER, 2002, &[])).await;

        // 1001 sends an SDS to the offline 2002.
        let sid = Uuid::new_v4();
        pkt(&state, a, build_short_transfer(&sid, 1001, 2002)).await;
        pkt(&state, a, build_sds_transfer_frame(&sid, 72, &TEXT)).await;
        assert_eq!(state.sms_center.snapshot().messages.len(), 1, "stored");
        assert!(b_rx.try_recv().is_err(), "nothing sent to the offline side");

        // Originator gets "destination not reachable, message stored" from 2002.
        let hdr = parse(&a_rx.try_recv().unwrap()).unwrap();
        assert!(matches!(hdr, BrewMessage::CallControl(CallControlMessage { payload: CallPayload::ShortTransfer { source: 2002, destination: 1001 }, .. })));
        let BrewMessage::Frame(f) = parse(&a_rx.try_recv().unwrap()).unwrap() else { panic!() };
        assert_eq!(f.data, vec![0x82, 0x10, 0x00, 7]);

        // 2002 comes back on B: the stored SDS is delivered from 1001.
        pkt(&state, b, build_subscriber_message(SUB_REGISTER, 2002, &[])).await;
        let raw = tokio::time::timeout(Duration::from_secs(2), b_rx.recv()).await.unwrap().unwrap();
        let BrewMessage::CallControl(cc) = parse(&raw).unwrap() else { panic!() };
        assert!(matches!(cc.payload, CallPayload::ShortTransfer { source: 1001, destination: 2002 }));
        let BrewMessage::Frame(f) = parse(&b_rx.recv().await.unwrap()).unwrap() else { panic!() };
        assert_eq!((f.identifier, f.length_bits, f.data.clone()), (cc.identifier, 72, TEXT.to_vec()));
        assert_eq!(state.sms_center.snapshot().messages.len(), 1, "kept until acknowledged");

        // B acknowledges -> removed from the queue and from disk.
        let mut report = vec![CLASS_FRAME, FRAME_SDS_REPORT];
        report.extend_from_slice(cc.identifier.as_bytes());
        report.extend_from_slice(&8u16.to_le_bytes());
        report.push(0);
        pkt(&state, b, report).await;
        assert!(state.sms_center.snapshot().messages.is_empty());
        let on_disk: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk["messages"].as_array().unwrap().len(), 0);
        assert!(on_disk["known_issis"].as_array().unwrap().contains(&serde_json::json!(2002)));
        std::fs::remove_file(path).ok();
    }
}
