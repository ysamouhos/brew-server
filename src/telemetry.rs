//! Basestation Telemetry channel: one-way BTS -> collector push of live
//! station state over a BTS-initiated WebSocket (subprotocol
//! `basestation-telemetry-v2`). See basestation-telemetry-control-api.md.
use crate::{fsnet, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, info, warn};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsGroupInfo {
    pub gssi: u32,
    pub mnemonic: Option<String>,
    pub attachment_mode: Option<u8>,
    pub is_dynamic: bool,
    pub is_attached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DgnaStatusInfo {
    pub issi: u32,
    pub gssi: u32,
    pub attach: bool,
    pub accepted: bool,
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthLevel {
    Ok,
    Degraded,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthDomain {
    Service,
    Backhaul,
    Radios,
    Congestion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainHealth {
    pub domain: HealthDomain,
    pub level: HealthLevel,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub overall: HealthLevel,
    pub domains: Vec<DomainHealth>,
    pub last_action: Option<String>,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SysSensorKind {
    Temperature,
    Voltage,
    Current,
    Power,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysSensor {
    pub name: String,
    pub kind: SysSensorKind,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxVisual {
    pub sample_rate: f32,
    pub center_freq_hz: f64,
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub spectrum_db_tenths: Vec<i16>,
    pub constellation_iq: Vec<i16>,
    pub carriers: Vec<(u16, f64)>,
    pub constellation_carrier: Option<(u16, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxQuality {
    pub papr_db: f32,
    pub evm_pct: f32,
    pub dc_offset_i: f32,
    pub dc_offset_q: f32,
    pub iq_amplitude_imbalance_db: f32,
    pub iq_phase_imbalance_deg: f32,
    pub carrier_leakage_db: f32,
    pub occupied_bandwidth_hz: f32,
    pub evm_carrier: Option<(u16, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdrHealth {
    pub temperature_c: Option<f32>,
    pub tx_gains: Vec<(String, f32)>,
    pub rx_gains: Vec<(String, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysHealthInfo {
    pub total_power_w: Option<f32>,
    pub sensors: Vec<SysSensor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryEvent {
    MsRegistration { issi: u32 },
    MsDeregistration { issi: u32 },
    MsTimeoutDrop { issi: u32 },
    MsGroupAttach { issi: u32, gssis: Vec<u32> },
    MsGroupDetach { issi: u32, gssis: Vec<u32> },
    MsGroupsSnapshot { issi: u32, gssis: Vec<u32> },
    MsGroupCatalogSnapshot { issi: u32, groups: Vec<MsGroupInfo> },
    MsRssi { issi: u32, rssi_dbfs: f32 },
    MsEnergySaving { issi: u32, mode: u8 },
    DgnaStatus(DgnaStatusInfo),

    GroupCallStarted { call_id: u16, gssi: u32, caller_issi: u32, carrier_num: u16, ts: u8, priority: u8 },
    GroupCallEnded { call_id: u16, gssi: u32 },
    IndividualCallStarted {
        call_id: u16, calling_issi: u32, called_issi: u32, simplex: bool,
        carrier_num: u16, ts: u8,
        peer_carrier_num: Option<u16>, peer_ts: Option<u8>,
        priority: u8,
    },
    IndividualCallEnded { call_id: u16 },
    CallSpeakerChanged { call_id: u16, is_group: bool, dest_addr: u32, speaker_issi: u32, carrier_num: u16, ts: u8 },
    TsVoiceActivity { carrier_num: u16, ts: u8, speaker_issi: Option<u32> },

    SdsActivity { source_issi: u32, dest_issi: u32 },
    SdsLog { direction: String, source_issi: u32, dest_issi: u32, is_group: bool, protocol_id: u8, text: String },

    TxVisual(TxVisual),
    TxQuality(TxQuality),
    SdrHealth(SdrHealth),
    SysHealth(SysHealthInfo),
    HealthSnapshot(HealthSnapshot),

    EmergencyAlarm { source_issi: u32, dest_ssi: u32 },
    EmergencyCancel { source_issi: u32 },

    BrewConnected { connected: bool, server_version: u8 },
    DapnetLog { direction: String, id: String, callsign: String, recipient: String, text: String, priority: Option<u8>, paths: Vec<String> },
    /// Multi-cell station: the MS that just registered is on this cell (0 = primary).
    MsCell { issi: u32, cell: u8 },
    /// Every cell the station runs (primary first), sent periodically.
    CellsSnapshot { site_linked: bool, cells: Vec<CellInfo> },
    /// Sent by Bost FlowStation first on every telemetry connect: its release
    /// ("v0.5.1"), release plus build hash, and upstream FlowStation version.
    StationVersion { version: String, build: String, upstream: String },
    /// Multi-cell: RF data from an additional cell's SDR (the primary's comes
    /// untagged, as `TxQuality` / `SdrHealth`).
    CellRf { cell: u8, event: CellRfEvent },
    /// The Basestation's configured position (`[telemetry]` latitude/longitude),
    /// sent periodically and only when it has one -- never (0, 0).
    SiteLocation { name: Option<String>, lat: f64, lon: f64 },
}

/// The RF events an additional cell reports inside `CellRf`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellRfEvent {
    /// Spectrum snapshot; newer Basestations keep it off the backhaul. Not used.
    TxVisual(serde_json::Value),
    TxQuality(TxQuality),
    SdrHealth(SdrHealth),
}

/// Latest RF metrics of one additional cell.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CellRfState {
    pub tx_quality: Option<TxQuality>,
    pub sdr_health: Option<SdrHealth>,
}

/// One cell of a Basestation, from its `CellsSnapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellInfo {
    pub id: u8,
    pub primary: bool,
    pub main_carrier: u16,
    pub secondary_carrier: Option<u16>,
    pub carriers: Vec<CellCarrierInfo>,
    /// Mobile country / network code; absent from builds that predate the fields.
    #[serde(default)]
    pub mcc: Option<u16>,
    #[serde(default)]
    pub mnc: Option<u16>,
    pub colour_code: u8,
    pub location_area: u16,
    pub neighbours: u16,
    pub device: Option<String>,
    pub registered_radios: u32,
    /// ISSIs registered on this cell. `None` from FlowStation builds that
    /// predate the field; those keep the event-derived registration list.
    #[serde(default)]
    pub registered_issis: Option<Vec<u32>>,
    pub rf_state: Option<String>,
    pub rf_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellCarrierInfo {
    pub carrier_num: u16,
    pub tx_freq_hz: u32,
    pub rx_freq_hz: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryCall {
    pub call_id: u16,
    pub is_group: bool,
    pub gssi_or_called: u32,
    pub source_issi: u32,
    pub carrier_num: u16,
    pub ts: u8,
    pub priority: u8,
    pub started_at_ms: u64,
}

/// One subscriber registration lifecycle event (register / deregister /
/// timeout-drop) on a Basestation, kept as a rolling log so the dashboard can
/// show registration activity over time rather than only the current
/// registered set (see `registrations_list`).
#[derive(Debug, Clone, Serialize)]
pub struct RegLogEntry {
    pub at_ms: u64,
    pub issi: u32,
    /// "register", "deregister", "timeout" (a silent drop after the
    /// subscriber stopped renewing its registration) or "roamed" (it
    /// registered on another Basestation, see `[roaming]`).
    pub kind: &'static str,
    /// For "roamed": the Basestation the subscriber moved to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roamed_to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SdsLogEntry {
    pub at_ms: u64,
    pub direction: String,
    pub source_issi: u32,
    pub dest_issi: u32,
    pub is_group: bool,
    pub protocol_id: u8,
    pub text: String,
}

/// A telemetry SDS log entry as persisted to the history store, tagged with the
/// reporting station so it can be told apart on replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdsTelemetryRecord {
    pub bts: String,
    pub at_ms: u64,
    pub direction: String,
    pub source_issi: u32,
    pub dest_issi: u32,
    pub is_group: bool,
    pub protocol_id: u8,
    pub text: String,
}

impl SdsTelemetryRecord {
    fn from_entry(bts: &str, entry: &SdsLogEntry) -> Self {
        Self {
            bts: bts.to_string(),
            at_ms: entry.at_ms,
            direction: entry.direction.clone(),
            source_issi: entry.source_issi,
            dest_issi: entry.dest_issi,
            is_group: entry.is_group,
            protocol_id: entry.protocol_id,
            text: entry.text.clone(),
        }
    }
}

/// Live, in-memory picture of one connected Basestation BTS derived from its
/// telemetry stream. Resets when the connection drops (no persistence, mirrors
/// the rest of this server's dashboard state).
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryBts {
    pub id: String,
    /// Remote IP address of the Basestation's telemetry connection, e.g.
    /// "10.19.144.201". `None` if the peer address could not be determined.
    pub ip: Option<String>,
    /// Software release the Basestation reported (`StationVersion`), e.g.
    /// "v0.5.1". `None` for stations that do not announce one.
    pub version: Option<String>,
    /// Release plus build hash, e.g. "v0.5.1-3ac72f7".
    pub build: Option<String>,
    pub connected_at_ms: u64,
    pub last_event_at_ms: u64,
    pub health: Option<HealthSnapshot>,
    pub backhaul_connected: Option<bool>,
    #[serde(skip)]
    pub registrations: HashSet<u32>,
    pub registration_count: usize,
    /// Sorted list of registered subscriber ISSIs, derived from `registrations`.
    /// Exposed to the dashboard so it can show who is registered on this station
    /// (the `registrations` HashSet itself is skipped for stable JSON ordering).
    pub registrations_list: Vec<u32>,
    /// Rolling log of registration lifecycle events (register/deregister/
    /// timeout) on this station, newest first, capped like `recent_sds`.
    #[serde(skip)]
    pub recent_regs: VecDeque<RegLogEntry>,
    /// Serialized view of `recent_regs` for the dashboard.
    pub recent_regs_out: Vec<RegLogEntry>,
    /// Multi-cell stations only: cell each registered ISSI is on (0 = primary).
    #[serde(skip)]
    pub ms_cell: HashMap<u32, u8>,
    /// Serialized view of `ms_cell` as sorted `[issi, cell]` pairs.
    pub ms_cell_out: Vec<(u32, u8)>,
    /// Latest `CellsSnapshot`: the station's cells, primary first (empty until one arrives).
    pub cells: Vec<CellInfo>,
    /// Position the Basestation reported (`SiteLocation`), if it has one.
    pub site_name: Option<String>,
    pub site_location: Option<(f64, f64)>,
    /// When that position was last advertised to federation peers (ms).
    #[serde(skip)]
    pub site_advertised_ms: u64,
    /// Whether the cells share the backhaul through the site switch.
    pub site_linked: Option<bool>,
    pub active_calls: HashMap<u16, TelemetryCall>,
    pub emergencies: HashSet<u32>,
    pub last_tx_quality: Option<TxQuality>,
    pub last_sdr_health: Option<SdrHealth>,
    /// Multi-cell: latest RF metrics of each additional cell, keyed by cell id
    /// (the primary's are `last_tx_quality` / `last_sdr_health`).
    pub cell_rf: BTreeMap<u8, CellRfState>,
    pub last_sys_health: Option<SysHealthInfo>,
    #[serde(skip)]
    pub recent_sds: VecDeque<SdsLogEntry>,
    pub recent_sds_out: Vec<SdsLogEntry>,
    /// Latest decoded position per subscriber ISSI, from textual position
    /// beacons seen in the SDS stream on this station. Binary LIP beacons carry
    /// no recoverable coordinates (see the `position` module), so this only
    /// contains subscribers that beacon a textual position.
    #[serde(skip)]
    pub positions: HashMap<u32, MsPosition>,
    /// Serialized view of `positions` for the dashboard/map, newest first.
    pub positions_out: Vec<MsPosition>,
    /// Position beacons that were seen but could NOT be turned into coordinates,
    /// keyed by ISSI: binary LIP with no payload on the telemetry channel, or
    /// no-fix (all-zero) reports. Lets the dashboard show "beaconing but not
    /// plottable" instead of the radio appearing silent.
    #[serde(skip)]
    pub undecoded_beacons: HashMap<u32, UndecodedBeacon>,
    /// Serialized view of `undecoded_beacons`, newest first.
    pub undecoded_beacons_out: Vec<UndecodedBeacon>,
    /// Latest transmit EVM (error vector magnitude, %) from TxQuality. Lower is
    /// better; shown as an SNR-adjacent quality indicator (not a true SNR).
    pub evm_pct: Option<f32>,
    /// Latest received signal strength (dBFS) reported for an MS via MsRssi.
    /// This is RSSI, not SNR. Retained as the station's most-recent reading.
    pub rssi_dbfs: Option<f32>,
    /// Latest RSSI (dBFS) per subscriber ISSI, so a specific call's mobile
    /// station can be shown with its own received-signal reading.
    #[serde(skip)]
    pub ms_rssi: HashMap<u32, f32>,
    /// Serialized [issi, rssi_dbfs] pairs of `ms_rssi` for the dashboard.
    pub ms_rssi_out: Vec<(u32, f32)>,
}

/// A position beacon that was observed but not decodable to coordinates.
#[derive(Debug, Clone, Serialize)]
pub struct UndecodedBeacon {
    pub issi: u32,
    pub at_ms: u64,
    pub count: u32,
    /// Why it could not be plotted (e.g. "binary LIP, no coordinates on
    /// telemetry channel").
    pub reason: String,
}

/// A decoded mobile-station position for the map.
#[derive(Debug, Clone, Serialize)]
pub struct MsPosition {
    pub issi: u32,
    pub lat: f64,
    pub lon: f64,
    pub at_ms: u64,
    /// The raw SDS text the coordinates were parsed from (for the popup).
    pub source_text: String,
}

impl TelemetryBts {
    fn new(id: String, ip: Option<String>) -> Self {
        Self {
            id,
            ip,
            version: None,
            build: None,
            connected_at_ms: now_ms(),
            last_event_at_ms: now_ms(),
            health: None,
            backhaul_connected: None,
            registrations: HashSet::new(),
            registration_count: 0,
            registrations_list: Vec::new(),
            recent_regs: VecDeque::new(),
            recent_regs_out: Vec::new(),
            ms_cell: HashMap::new(),
            ms_cell_out: Vec::new(),
            cells: Vec::new(),
            site_name: None,
            site_location: None,
            site_advertised_ms: 0,
            site_linked: None,
            active_calls: HashMap::new(),
            emergencies: HashSet::new(),
            last_tx_quality: None,
            last_sdr_health: None,
            cell_rf: BTreeMap::new(),
            last_sys_health: None,
            recent_sds: VecDeque::new(),
            recent_sds_out: Vec::new(),
            positions: HashMap::new(),
            positions_out: Vec::new(),
            undecoded_beacons: HashMap::new(),
            undecoded_beacons_out: Vec::new(),
            evm_pct: None,
            rssi_dbfs: None,
            ms_rssi: HashMap::new(),
            ms_rssi_out: Vec::new(),
        }
    }

    fn push_sds(&mut self, entry: SdsLogEntry) {
        // If the SDS body carries a textual position (APRS/decimal/Maidenhead),
        // record the latest coordinates for the sending subscriber so the map
        // can plot it. Binary LIP beacons (empty text) yield nothing here.
        if let Some(ll) = crate::position::parse_position(&entry.text) {
            self.positions.insert(entry.source_issi, MsPosition {
                issi: entry.source_issi,
                lat: ll.lat,
                lon: ll.lon,
                at_ms: entry.at_ms,
                source_text: entry.text.clone(),
            });
            self.undecoded_beacons.remove(&entry.source_issi);
            self.sync_positions();
            self.sync_undecoded();
        } else if entry.protocol_id == 10 {
            // A LIP position beacon arrived via telemetry, but with no decodable
            // coordinates (binary LIP payloads are stripped to empty text on the
            // telemetry channel; no-fix reports have no position). Track it so the
            // dashboard can show the radio is beaconing-but-not-plottable rather
            // than silent. If the same ISSI is already plotted from the Brew
            // channel, don't overwrite that — just note the beacon.
            let e = self.undecoded_beacons.entry(entry.source_issi).or_insert(UndecodedBeacon {
                issi: entry.source_issi,
                at_ms: entry.at_ms,
                count: 0,
                reason: "binary LIP / no coordinates on telemetry channel".to_string(),
            });
            e.at_ms = entry.at_ms;
            e.count = e.count.saturating_add(1);
            self.sync_undecoded();
        }
        self.recent_sds.push_front(entry);
        while self.recent_sds.len() > 50 {
            self.recent_sds.pop_back();
        }
        self.recent_sds_out = self.recent_sds.iter().cloned().collect();
    }

    /// Recomputes the serialized undecoded-beacon list (newest first).
    fn sync_undecoded(&mut self) {
        let mut list: Vec<UndecodedBeacon> = self.undecoded_beacons.values().cloned().collect();
        list.sort_unstable_by(|a, b| b.at_ms.cmp(&a.at_ms));
        self.undecoded_beacons_out = list;
    }

    /// Recomputes the serialized position list (newest first) after `positions`
    /// changes.
    fn sync_positions(&mut self) {
        let mut list: Vec<MsPosition> = self.positions.values().cloned().collect();
        list.sort_unstable_by(|a, b| b.at_ms.cmp(&a.at_ms));
        self.positions_out = list;
    }

    /// Recomputes the serialized registration view (count + sorted ISSI list)
    /// after the `registrations` set changes.
    fn sync_registrations(&mut self) {
        let regs = &self.registrations;
        self.ms_cell.retain(|issi, _| regs.contains(issi));
        let mut cells: Vec<(u32, u8)> = self.ms_cell.iter().map(|(k, v)| (*k, *v)).collect();
        cells.sort_unstable_by_key(|(k, _)| *k);
        self.ms_cell_out = cells;
        self.registration_count = self.registrations.len();
        let mut list: Vec<u32> = self.registrations.iter().copied().collect();
        list.sort_unstable();
        self.registrations_list = list;
    }

    /// Stores a `CellsSnapshot`. When every cell lists its registered ISSIs,
    /// the station's registration list (and each ISSI's cell) is resynced from
    /// it, repairing whatever registration events were missed -- e.g. radios
    /// that registered before the telemetry connection (re)started.
    fn apply_cells_snapshot(&mut self, cells: Vec<CellInfo>) {
        if cells.iter().all(|c| c.registered_issis.is_some()) {
            self.registrations.clear();
            self.ms_cell.clear();
            for c in &cells {
                for &issi in c.registered_issis.iter().flatten() {
                    self.registrations.insert(issi);
                    if cells.len() > 1 {
                        self.ms_cell.insert(issi, c.id);
                    }
                }
            }
            self.sync_registrations();
        }
        self.cells = cells;
    }

    /// Appends a registration lifecycle event to the rolling log (newest
    /// first, capped at 50 like `recent_sds`).
    fn push_reg(&mut self, issi: u32, kind: &'static str) {
        self.push_reg_entry(RegLogEntry { at_ms: now_ms(), issi, kind, roamed_to: None });
    }

    fn push_reg_entry(&mut self, entry: RegLogEntry) {
        self.recent_regs.push_front(entry);
        while self.recent_regs.len() > 50 {
            self.recent_regs.pop_back();
        }
        self.recent_regs_out = self.recent_regs.iter().cloned().collect();
    }
}

#[derive(Default)]
pub struct TelemetryState {
    pub stations: HashMap<String, TelemetryBts>,
    /// Basestation each ISSI most recently registered on (telemetry
    /// `MsRegistration`), used by `[roaming]` to skip a kick when the radio
    /// has already come back to the old Basestation.
    pub last_registration: HashMap<u32, String>,
    /// Positions decoded from the Brew SDS channel (LIP binary or text), keyed by
    /// subscriber ISSI. This is the primary source when Basestation cannot be
    /// modified: the raw SDS (incl. LIP payloads) is relayed over the Brew
    /// protocol and decoded here, independent of the lossy telemetry SdsLog.
    pub sds_positions: HashMap<u32, PositionFix>,
    /// SDS entries observed on any Basestation telemetry channel, newest first,
    /// persisted to the history store so this survives restarts (unlike the
    /// per-station `recent_sds`, which resets when a BTS reconnects).
    recent_sds: VecDeque<SdsTelemetryRecord>,
    /// Serialized view of `recent_sds` for the dashboard.
    pub recent_sds_out: Vec<SdsTelemetryRecord>,
    /// Mobile-station registration lifecycle events (register/deregister)
    /// observed on the Brew protocol channel directly, i.e. a `Terminal`-mode
    /// client registering/deregistering an ISSI with this server — distinct
    /// from the per-Basestation telemetry registrations in `stations`. Tagged
    /// with a synthetic "brew" station id (see `RegLogEntry`/`bts` below) so
    /// the dashboard's registration log can show both sources together.
    /// Newest first, capped like the per-station logs; not persisted.
    recent_brew_regs: VecDeque<RegLogEntry>,
    /// Serialized view of `recent_brew_regs` for the dashboard.
    pub recent_brew_regs_out: Vec<RegLogEntry>,
    /// Latest per-ISSI RSSI reported directly on the main Brew protocol
    /// channel (`CLASS_SERVICE` type `0x10`, `{"issi":N,"rssi_dbfs":F}`) --
    /// not part of this server's original protocol coverage, but sent by at
    /// least one real client (nexus-bs). Distinct from `TelemetryBts::ms_rssi`
    /// (which comes from the separate Basestation Telemetry WebSocket): this
    /// is keyed only by ISSI, with no BTS-id concept on the wire, so it isn't
    /// nested under a station the way that one is.
    pub brew_ms_rssi: HashMap<u32, f32>,
    store: Option<Arc<crate::store::Store>>,
}

impl TelemetryState {
    /// Whether any Basestation currently reports an emergency alarm for `issi`
    /// (set by `EmergencyAlarm`, cleared by `EmergencyCancel` or an operator).
    /// FlowStation forwards a radio's group call to Brew at priority 0, so the
    /// alarm is how the server learns the call is an emergency.
    pub fn is_emergency(&self, issi: u32) -> bool {
        self.stations.values().any(|s| s.emergencies.contains(&issi))
    }

    #[cfg(test)]
    pub fn add_test_station(&mut self, id: &str, location: Option<(f64, f64)>, name: &str) {
        let mut bts = TelemetryBts::new(id.to_string(), None);
        bts.site_location = location;
        bts.site_name = Some(name.to_string());
        self.stations.insert(id.to_string(), bts);
    }

    /// Creates a `TelemetryState` backed by an append-only store, replaying
    /// persisted SDS telemetry history from disk.
    pub fn with_store(store: Arc<crate::store::Store>) -> Self {
        let mut recent_sds: VecDeque<SdsTelemetryRecord> = VecDeque::new();
        if let Ok(records) = crate::store::Store::replay(store.path()) {
            for rec in records {
                if let crate::store::StoredRecord::SdsTelemetry(r) = rec {
                    recent_sds.push_front(r);
                    while recent_sds.len() > 200 { recent_sds.pop_back(); }
                }
            }
        }
        let recent_sds_out = recent_sds.iter().cloned().collect();
        Self { recent_sds, recent_sds_out, store: Some(store), ..Self::default() }
    }

    fn persist(&self, rec: &crate::store::StoredRecord) {
        if let Some(store) = &self.store {
            if let Err(e) = store.append(rec) {
                tracing::error!(error = %e, "failed to persist telemetry history record");
            }
        }
    }

    /// Adds an SDS telemetry record replicated from the HA peer (already in
    /// the store) to the dashboard's recent list.
    pub fn ingest_replicated(&mut self, rec: SdsTelemetryRecord) {
        self.recent_sds.push_front(rec);
        while self.recent_sds.len() > 200 { self.recent_sds.pop_back(); }
        self.recent_sds_out = self.recent_sds.iter().cloned().collect();
    }

    /// Records an SDS telemetry entry for `bts` in the durable, cross-restart
    /// log (independent of the ephemeral per-station `recent_sds`).
    fn record_sds_telemetry(&mut self, bts: &str, entry: &SdsLogEntry) {
        let rec = SdsTelemetryRecord::from_entry(bts, entry);
        self.persist(&crate::store::StoredRecord::SdsTelemetry(rec.clone()));
        self.recent_sds.push_front(rec);
        while self.recent_sds.len() > 200 { self.recent_sds.pop_back(); }
        self.recent_sds_out = self.recent_sds.iter().cloned().collect();
    }

    pub fn snapshot(&self) -> Vec<TelemetryBts> {
        self.stations.values().cloned().collect()
    }

    /// Flat list of the latest decoded MS positions across all stations, each
    /// tagged with the station that reported it, newest first. Only subscribers
    /// that beacon a *textual* position appear (see the `position` module).
    pub fn positions(&self) -> Vec<PositionFix> {
        // Merge Brew-channel SDS positions (primary) with any per-station
        // textual positions. On ISSI collision the newer fix wins.
        let mut by_issi: HashMap<u32, PositionFix> = self.sds_positions.clone();
        for s in self.stations.values() {
            for p in s.positions.values() {
                let fix = PositionFix {
                    issi: p.issi, lat: p.lat, lon: p.lon, at_ms: p.at_ms,
                    bts: s.id.clone(), source_text: p.source_text.clone(), emergency: false,
                };
                by_issi.entry(p.issi)
                    .and_modify(|e| if fix.at_ms >= e.at_ms { *e = fix.clone(); })
                    .or_insert(fix);
            }
        }
        let mut out: Vec<PositionFix> = by_issi.into_values().collect();
        out.sort_unstable_by(|a, b| b.at_ms.cmp(&a.at_ms));
        out
    }

    /// Records a position decoded from the Brew SDS channel for `issi`.
    pub fn record_sds_position(&mut self, issi: u32, lat: f64, lon: f64, at_ms: u64, source_text: String) {
        // 0/0 is "no fix": keep any earlier real position and plot nothing new.
        if lat == 0.0 && lon == 0.0 {
            return;
        }
        self.sds_positions.insert(issi, PositionFix {
            issi, lat, lon, at_ms, bts: "brew-sds".to_string(), source_text, emergency: false,
        });
        // This ISSI now has a real fix, so clear any "beaconing but not
        // plottable" markers for it across all stations.
        for s in self.stations.values_mut() {
            if s.undecoded_beacons.remove(&issi).is_some() {
                s.sync_undecoded();
            }
        }
    }

    /// Records a mobile-station registration lifecycle event seen directly on
    /// the Brew protocol channel (a `Terminal`-mode client registering or
    /// deregistering an ISSI with this server), so it appears in the same
    /// registration log as Basestation telemetry registrations. `kind` is
    /// "register" or "deregister".
    pub fn record_brew_registration(&mut self, issi: u32, kind: &'static str) {
        self.recent_brew_regs.push_front(RegLogEntry { at_ms: now_ms(), issi, kind, roamed_to: None });
        while self.recent_brew_regs.len() > 50 {
            self.recent_brew_regs.pop_back();
        }
        self.recent_brew_regs_out = self.recent_brew_regs.iter().cloned().collect();
    }

    /// Records an RSSI report for `issi` seen directly on the main Brew
    /// protocol channel (see `brew_ms_rssi`'s doc comment).
    pub fn record_brew_rssi(&mut self, issi: u32, rssi_dbfs: f32) {
        self.brew_ms_rssi.insert(issi, rssi_dbfs);
    }

    /// `issi` just registered on `new_bts`: drops it from every other
    /// Basestation that still lists it, logs it there as "roamed", and returns
    /// those stations' ids so the caller can kick it on them.
    pub fn take_roamed(&mut self, issi: u32, new_bts: &str) -> Vec<String> {
        let mut old = Vec::new();
        for (id, bts) in self.stations.iter_mut() {
            if id == new_bts || !bts.registrations.remove(&issi) { continue; }
            bts.ms_cell.remove(&issi);
            bts.sync_registrations();
            bts.push_reg_entry(RegLogEntry { at_ms: now_ms(), issi, kind: "roamed", roamed_to: Some(new_bts.to_string()) });
            old.push(id.clone());
        }
        old.sort_unstable();
        old
    }

    /// Flat, newest-first list of registration lifecycle events across every
    /// Basestation's telemetry channel *and* the Brew protocol channel
    /// directly (tagged with the synthetic station id "brew"), for the
    /// `/registrations` dashboard page. Capped to the most recent 100 entries.
    pub fn registration_log(&self) -> Vec<RegLogRow> {
        let mut out: Vec<RegLogRow> = self.stations.values()
            .flat_map(|s| s.recent_regs_out.iter().map(|e| RegLogRow { bts: s.id.clone(), entry: e.clone() }))
            .chain(self.recent_brew_regs.iter().map(|e| RegLogRow { bts: "brew".to_string(), entry: e.clone() }))
            .collect();
        out.sort_unstable_by(|a, b| b.entry.at_ms.cmp(&a.entry.at_ms));
        out.truncate(100);
        out
    }
}

/// One row of the combined registration log served to the dashboard: a
/// registration lifecycle event tagged with the station that reported it
/// ("brew" for events seen directly on the Brew protocol channel, rather than
/// via a Basestation's telemetry channel).
#[derive(Debug, Clone, Serialize)]
pub struct RegLogRow {
    pub bts: String,
    #[serde(flatten)]
    pub entry: RegLogEntry,
}

/// A position fix as served to the map, including which station reported it.
#[derive(Debug, Clone, Serialize)]
pub struct PositionFix {
    pub issi: u32,
    pub lat: f64,
    pub lon: f64,
    pub at_ms: u64,
    pub bts: String,
    pub source_text: String,
    /// The radio has an active emergency (alarm or emergency call): the map draws it red.
    /// Set when served (`dashboard::positions_snapshot`), false here.
    pub emergency: bool,
}

pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    if !state.config.telemetry.enabled {
        return Ok(());
    }
    let cfg = fsnet::ListenerConfig {
        name: "telemetry",
        listen: state.service_bind(state.config.telemetry.listen),
        subprotocol: "bluestation-telemetry-v2",
        users: state.config.telemetry.users.clone(),
        tls: state.config.telemetry.tls.clone(),
    };
    fsnet::serve(cfg, move |socket, identity, peer| {
        let state = state.clone();
        async move { session(state, socket, identity, peer).await }
    })
    .await
}

async fn session(state: Arc<AppState>, socket: WebSocket, identity: Option<String>, peer: Option<std::net::SocketAddr>) {
    let id = identity.unwrap_or_else(|| format!("telemetry-{}", uuid::Uuid::new_v4().simple()));
    let ip = peer.map(|p| p.ip().to_string());
    {
        let mut t = state.telemetry.write().await;
        t.stations.insert(id.clone(), TelemetryBts::new(id.clone(), ip.clone()));
    }
    info!(bts = %id, ip = ip.as_deref().unwrap_or("unknown"), "Basestation telemetry connected");

    let (_tx, mut ws_rx) = socket.split();
    while let Some(item) = ws_rx.next().await {
        match item {
            Ok(Message::Binary(data)) => handle_event(&state, &id, &data).await,
            Ok(Message::Text(_)) => warn!(bts = %id, "unexpected text frame on telemetry channel, ignoring"),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Err(e) => { warn!(bts = %id, error = %e, "telemetry WebSocket receive error"); break; }
        }
    }

    let removed = state.telemetry.write().await.stations.remove(&id);
    if removed.is_some_and(|b| b.site_location.is_some()) {
        crate::fedroute::advertise_bts(&state, &id, "", 0.0, 0.0, false).await;
    }
    state.monitor.emit("telemetry_disconnected", serde_json::json!({"id": id}));
    info!(bts = %id, "Basestation telemetry disconnected");
}

async fn handle_event(state: &Arc<AppState>, id: &str, data: &[u8]) {
    let event: TelemetryEvent = match serde_json::from_slice(data) {
        Ok(e) => e,
        // A newer Basestation may send event types this server does not know
        // yet; skip those quietly instead of warning on every message.
        Err(e) if e.to_string().starts_with("unknown variant") => {
            debug!(bts = %id, error = %e, "skipping unknown telemetry event");
            return;
        }
        Err(e) => {
            warn!(bts = %id, error = %e, bytes = data.len(), "dropping malformed telemetry event");
            return;
        }
    };
    debug!(bts = %id, ?event, "telemetry event");

    let mut t = state.telemetry.write().await;
    let Some(bts) = t.stations.get_mut(id) else { return };
    bts.last_event_at_ms = now_ms();

    // Only state-changing events wake the live dashboard socket; high-rate
    // instrumentation (TxQuality ~1/s, SdrHealth ~5s, SysHealth ~2s, TxVisual
    // ~5/s) is still recorded below but picked up by the existing 2s poll.
    let notify = !matches!(event, TelemetryEvent::TxVisual(_) | TelemetryEvent::TxQuality(_)
        | TelemetryEvent::SdrHealth(_) | TelemetryEvent::SysHealth(_)
        | TelemetryEvent::MsRssi { .. } | TelemetryEvent::TsVoiceActivity { .. }
        | TelemetryEvent::CellsSnapshot { .. } | TelemetryEvent::CellRf { .. });

    let mut sds_entry: Option<SdsLogEntry> = None;
    // Set when a (changed or due) station position must go out to federation peers.
    let mut advertise: Option<(String, f64, f64)> = None;
    // Set when an emergency alarm just came up for an ISSI.
    let mut alarm_raised: Option<u32> = None;
    // Set when an MS just registered here (for `[roaming]`).
    let mut registered: Option<u32> = None;
    match event {
        TelemetryEvent::SiteLocation { name, lat, lon } => {
            if crate::fedroute::valid_position(lat, lon) {
                let name = name.unwrap_or_default();
                let now = now_ms();
                let changed = bts.site_location != Some((lat, lon)) || bts.site_name.as_deref() != Some(name.as_str());
                if changed || now.saturating_sub(bts.site_advertised_ms) >= crate::fedroute::BTS_REFRESH_MS {
                    bts.site_advertised_ms = now;
                    advertise = Some((name.clone(), lat, lon));
                }
                bts.site_name = Some(name);
                bts.site_location = Some((lat, lon));
            }
        }
        TelemetryEvent::MsRegistration { issi } => {
            bts.registrations.insert(issi); bts.sync_registrations();
            bts.push_reg(issi, "register");
            registered = Some(issi);
        }
        TelemetryEvent::MsDeregistration { issi } => {
            let was_registered = bts.registrations.remove(&issi);
            bts.sync_registrations();
            // Already logged as "roamed" when it moved away and got kicked
            // here (`[roaming]`): the Basestation confirming that is not a
            // second deregistration.
            if was_registered || !state.config.roaming.kick_old_issi {
                bts.push_reg(issi, "deregister");
            }
        }
        TelemetryEvent::MsCell { issi, cell } => {
            if bts.registrations.contains(&issi) {
                bts.ms_cell.insert(issi, cell); bts.sync_registrations();
            }
        }
        TelemetryEvent::CellsSnapshot { site_linked, cells } => {
            bts.site_linked = Some(site_linked);
            bts.apply_cells_snapshot(cells);
        }
        TelemetryEvent::MsTimeoutDrop { issi } => {
            bts.registrations.remove(&issi); bts.sync_registrations();
            bts.push_reg(issi, "timeout");
        }
        TelemetryEvent::GroupCallStarted { call_id, gssi, caller_issi, carrier_num, ts, priority } => {
            bts.active_calls.insert(call_id, TelemetryCall {
                call_id, is_group: true, gssi_or_called: gssi, source_issi: caller_issi,
                carrier_num, ts, priority, started_at_ms: now_ms(),
            });
        }
        TelemetryEvent::GroupCallEnded { call_id, .. } => { bts.active_calls.remove(&call_id); }
        TelemetryEvent::IndividualCallStarted { call_id, calling_issi, called_issi, carrier_num, ts, priority, .. } => {
            bts.active_calls.insert(call_id, TelemetryCall {
                call_id, is_group: false, gssi_or_called: called_issi, source_issi: calling_issi,
                carrier_num, ts, priority, started_at_ms: now_ms(),
            });
        }
        TelemetryEvent::IndividualCallEnded { call_id } => { bts.active_calls.remove(&call_id); }
        TelemetryEvent::SdsLog { direction, source_issi, dest_issi, is_group, protocol_id, text } => {
            info!(bts = %id, channel = "telemetry", source_issi, dest_issi, is_group, protocol_id,
                lip = (protocol_id == 10), has_text = !text.trim().is_empty(),
                "SDS ({direction})");
            let entry = SdsLogEntry { at_ms: now_ms(), direction, source_issi, dest_issi, is_group, protocol_id, text };
            bts.push_sds(entry.clone());
            sds_entry = Some(entry);
        }
        TelemetryEvent::TxQuality(q) => { bts.evm_pct = Some(q.evm_pct); bts.last_tx_quality = Some(q); }
        TelemetryEvent::SdrHealth(h) => bts.last_sdr_health = Some(h),
        TelemetryEvent::CellRf { cell, event } => {
            let rf = bts.cell_rf.entry(cell).or_default();
            match event {
                CellRfEvent::TxQuality(q) => rf.tx_quality = Some(q),
                CellRfEvent::SdrHealth(h) => rf.sdr_health = Some(h),
                CellRfEvent::TxVisual(_) => {}
            }
        }
        TelemetryEvent::SysHealth(h) => bts.last_sys_health = Some(h),
        TelemetryEvent::HealthSnapshot(h) => bts.health = Some(h),
        TelemetryEvent::EmergencyAlarm { source_issi, .. } => {
            if bts.emergencies.insert(source_issi) { alarm_raised = Some(source_issi); }
        }
        TelemetryEvent::EmergencyCancel { source_issi } => { bts.emergencies.remove(&source_issi); }
        TelemetryEvent::BrewConnected { connected, .. } => bts.backhaul_connected = Some(connected),
        TelemetryEvent::StationVersion { version, build, .. } => {
            info!(bts = %id, %version, %build, "Basestation version");
            bts.version = Some(version);
            bts.build = Some(build);
        }
        TelemetryEvent::MsRssi { issi, rssi_dbfs } => {
            bts.rssi_dbfs = Some(rssi_dbfs);
            bts.ms_rssi.insert(issi, rssi_dbfs);
            let mut v: Vec<(u32, f32)> = bts.ms_rssi.iter().map(|(k, val)| (*k, *val)).collect();
            v.sort_unstable_by_key(|(k, _)| *k);
            bts.ms_rssi_out = v;
        }
        _ => {}
    }
    let mut ms_advert: Option<(u32, f64, f64, u64)> = None;
    if let Some(entry) = sds_entry {
        t.record_sds_telemetry(id, &entry);
        // A textual position just decoded for this subscriber, to relay to peers.
        ms_advert = t.stations.get(id)
            .and_then(|b| b.positions.get(&entry.source_issi))
            .filter(|p| p.at_ms == entry.at_ms)
            .map(|p| (p.issi, p.lat, p.lon, p.at_ms));
    }
    // `[roaming] kick_old_issi`: the radio left whatever other Basestation
    // still lists it (radios never deregister from the cell they leave).
    if let Some(issi) = registered {
        t.last_registration.insert(issi, id.to_string());
    }
    let roamed: Vec<(u32, String)> = match registered {
        Some(issi) if state.config.roaming.kick_old_issi => {
            t.take_roamed(issi, id).into_iter().map(|old| (issi, old)).collect()
        }
        _ => Vec::new(),
    };
    drop(t);
    // `[allowlist] kick_unknown`: a radio that is not admitted to the network
    // has still registered on the cell (the Basestation's own checks are
    // local); it is already silent here, so this only clears it off the cell.
    if let Some(issi) = registered.filter(|&issi| state.config.allowlist.kick_unknown && state.allowlist_refuses(issi)) {
        warn!(issi, bts = %id, "registration of an ISSI not on the allowlist: kicking it off the Basestation");
        let state = state.clone();
        let bts = id.to_string();
        tokio::spawn(async move {
            let delay = state.config.allowlist.kick_delay_ms;
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
            // Added to the list in the meantime: leave it alone.
            if !state.allowlist_refuses(issi) {
                return;
            }
            match crate::control::send_command(&state, &bts, crate::control::ControlCommand::KickMs { issi }).await {
                Ok(Some(crate::control::ControlResponse::KickMsResponse { success: false, .. })) =>
                    warn!(issi, bts = %bts, "allowlist: Basestation refused the kick"),
                Ok(_) => {}
                Err(e) => warn!(issi, bts = %bts, error = ?e, "allowlist: could not kick the radio"),
            }
        });
    }
    for (issi, old) in roamed {
        info!(issi, from = %old, to = %id, "subscriber roamed to other BS, kicking old registration");
        let state = state.clone();
        tokio::spawn(async move {
            let delay = state.config.roaming.kick_delay_ms;
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
            // Roamed back in the meantime: the old Basestation now holds the
            // radio's current registration, which must not be kicked.
            if state.telemetry.read().await.last_registration.get(&issi).is_some_and(|bts| *bts == old) {
                info!(issi, bts = %old, "roaming: radio came back to the old BS, kick skipped");
                return;
            }
            match crate::control::send_command(&state, &old, crate::control::ControlCommand::KickMs { issi }).await {
                Ok(Some(crate::control::ControlResponse::KickMsResponse { success: false, .. })) =>
                    warn!(issi, bts = %old, "roaming: old Basestation refused the kick"),
                Ok(_) => {}
                Err(e) => warn!(issi, bts = %old, error = ?e, "roaming: could not kick old registration"),
            }
        });
    }
    if let Some(issi) = alarm_raised {
        // A call this ISSI already has running becomes an emergency call too.
        crate::router::emergency_raised(state, issi).await;
    }
    if let Some((issi, lat, lon, at_ms)) = ms_advert {
        crate::fedroute::advertise_ms(state, issi, lat, lon, at_ms, id).await;
    }
    if let Some((name, lat, lon)) = advertise {
        crate::fedroute::advertise_bts(state, id, &name, lat, lon, true).await;
    }
    if notify {
        state.monitor.emit("telemetry", serde_json::json!({"id": id}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sds(source_issi: u32, protocol_id: u8, text: &str, at_ms: u64) -> SdsLogEntry {
        SdsLogEntry {
            at_ms,
            direction: "rx".to_string(),
            source_issi,
            dest_issi: 0,
            is_group: false,
            protocol_id,
            text: text.to_string(),
        }
    }

    #[test]
    fn textual_position_beacon_is_stored() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.push_sds(sds(1001, 3, "44.4353, 26.1092", 100));
        assert_eq!(bts.positions.len(), 1);
        let p = &bts.positions[&1001];
        assert!((p.lat - 44.4353).abs() < 0.01 && (p.lon - 26.1092).abs() < 0.01);
        assert_eq!(bts.positions_out.len(), 1);
    }

    #[test]
    fn empty_lip_beacon_stores_no_position() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        // PID 10 with empty text (the binary-LIP case) yields no coordinates.
        bts.push_sds(sds(1001, 10, "", 100));
        assert!(bts.positions.is_empty());
        assert!(bts.positions_out.is_empty());
    }

    #[test]
    fn latest_position_replaces_older_for_same_issi() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.push_sds(sds(1001, 3, "44.00, 26.00", 100));
        bts.push_sds(sds(1001, 3, "45.00, 27.00", 200));
        assert_eq!(bts.positions.len(), 1);
        let p = &bts.positions[&1001];
        assert!((p.lat - 45.0).abs() < 0.01, "should keep newest");
        assert_eq!(p.at_ms, 200);
    }

    #[test]
    fn positions_across_stations_are_aggregated_and_tagged() {
        let mut state = TelemetryState::default();
        let mut a = TelemetryBts::new("BTS-A".to_string(), None);
        a.push_sds(sds(1, 3, "44.0, 26.0", 100));
        let mut b = TelemetryBts::new("BTS-B".to_string(), None);
        b.push_sds(sds(2, 3, "45.0, 27.0", 200));
        state.stations.insert("BTS-A".to_string(), a);
        state.stations.insert("BTS-B".to_string(), b);
        let fixes = state.positions();
        assert_eq!(fixes.len(), 2);
        // newest first
        assert_eq!(fixes[0].issi, 2);
        assert_eq!(fixes[0].bts, "BTS-B");
        assert!(fixes.iter().any(|f| f.issi == 1 && f.bts == "BTS-A"));
    }

    #[test]
    fn sync_registrations_sorts_and_counts() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.registrations.insert(300);
        bts.registrations.insert(100);
        bts.registrations.insert(200);
        bts.sync_registrations();
        assert_eq!(bts.registration_count, 3);
        assert_eq!(bts.registrations_list, vec![100, 200, 300]);
    }

    #[test]
    fn site_location_event_parses() {
        let e: TelemetryEvent = serde_json::from_str(r#"{"SiteLocation":{"name":"Hill","lat":37.9,"lon":23.7}}"#).unwrap();
        assert!(matches!(e, TelemetryEvent::SiteLocation { lat, .. } if (lat - 37.9).abs() < 1e-9));
        let e: TelemetryEvent = serde_json::from_str(r#"{"SiteLocation":{"name":null,"lat":1.0,"lon":2.0}}"#).unwrap();
        assert!(matches!(e, TelemetryEvent::SiteLocation { name: None, .. }));
    }

    #[test]
    fn sync_registrations_after_removal() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        for issi in [10u32, 20, 30] { bts.registrations.insert(issi); }
        bts.sync_registrations();
        bts.registrations.remove(&20);
        bts.sync_registrations();
        assert_eq!(bts.registration_count, 2);
        assert_eq!(bts.registrations_list, vec![10, 30]);
    }

    #[test]
    fn ms_cell_tracks_registered_issis_only() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.registrations.insert(7);
        bts.ms_cell.insert(7, 1);
        bts.ms_cell.insert(8, 0);
        bts.sync_registrations();
        assert_eq!(bts.ms_cell_out, vec![(7, 1)], "cell of an unregistered ISSI is dropped");
        bts.registrations.remove(&7);
        bts.sync_registrations();
        assert!(bts.ms_cell_out.is_empty());
    }

    #[test]
    fn cells_snapshot_decodes() {
        let json = r#"{"CellsSnapshot":{"site_linked":true,"cells":[{"id":0,"primary":true,"main_carrier":1521,
            "secondary_carrier":null,"carriers":[{"carrier_num":1521,"tx_freq_hz":438025000,"rx_freq_hz":433025000}],
            "colour_code":1,"location_area":2,"neighbours":1,"device":"driver=plutosdr","registered_radios":3,
            "rf_state":"online","rf_detail":"ok"}]}}"#;
        let e: TelemetryEvent = serde_json::from_str(json).unwrap();
        let TelemetryEvent::CellsSnapshot { site_linked, cells } = e else { panic!("expected CellsSnapshot") };
        assert!(site_linked);
        assert_eq!(cells[0].carriers[0].tx_freq_hz, 438_025_000);
        assert_eq!(cells[0].rf_state.as_deref(), Some("online"));
    }

    fn cell(id: u8, issis: Option<Vec<u32>>) -> CellInfo {
        CellInfo { mcc: Some(202), mnc: Some(1),
            id, primary: id == 0, main_carrier: 1, secondary_carrier: None, carriers: Vec::new(),
            colour_code: 1, location_area: 1, neighbours: 0, device: None,
            registered_radios: issis.as_ref().map_or(0, |v| v.len() as u32),
            registered_issis: issis, rf_state: None, rf_detail: None,
        }
    }

    #[test]
    fn cells_snapshot_resyncs_registrations() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.registrations.insert(99); // stale: deregistered while telemetry was down
        bts.apply_cells_snapshot(vec![cell(0, Some(vec![1, 2])), cell(1, Some(vec![3]))]);
        assert_eq!(bts.registrations_list, vec![1, 2, 3]);
        assert_eq!(bts.registration_count, 3);
        assert_eq!(bts.ms_cell_out, vec![(1, 0), (2, 0), (3, 1)]);
    }

    #[test]
    fn cells_snapshot_without_issis_keeps_event_registrations() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.registrations.insert(7);
        bts.sync_registrations();
        bts.apply_cells_snapshot(vec![cell(0, None)]);
        assert_eq!(bts.registrations_list, vec![7]);
        assert_eq!(bts.cells.len(), 1);
    }

    #[test]
    fn ms_cell_event_decodes() {
        let e: TelemetryEvent = serde_json::from_str(r#"{"MsCell":{"issi":7,"cell":1}}"#).unwrap();
        assert!(matches!(e, TelemetryEvent::MsCell { issi: 7, cell: 1 }));
    }

    #[test]
    fn push_reg_logs_events_newest_first_and_capped() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.push_reg(1001, "register");
        bts.push_reg(1002, "register");
        bts.push_reg(1001, "deregister");
        assert_eq!(bts.recent_regs_out.len(), 3);
        assert_eq!(bts.recent_regs_out[0].issi, 1001);
        assert_eq!(bts.recent_regs_out[0].kind, "deregister");
        assert_eq!(bts.recent_regs_out[2].kind, "register");
        for i in 0..60 {
            bts.push_reg(i, "register");
        }
        assert_eq!(bts.recent_regs_out.len(), 50, "log capped at 50 entries");
    }

    #[test]
    fn registration_events_recorded_via_handle_event_path() {
        // Exercises the same transitions handle_event applies, directly on the
        // TelemetryBts state, to confirm registrations and the event log agree.
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.registrations.insert(2001);
        bts.sync_registrations();
        bts.push_reg(2001, "register");
        bts.registrations.remove(&2001);
        bts.sync_registrations();
        bts.push_reg(2001, "timeout");
        assert!(bts.registrations_list.is_empty());
        assert_eq!(bts.recent_regs_out.len(), 2);
        assert_eq!(bts.recent_regs_out[0].kind, "timeout");
        assert_eq!(bts.recent_regs_out[1].kind, "register");
    }

    #[test]
    fn registrations_list_serialized_in_snapshot() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        bts.registrations.insert(4242);
        bts.sync_registrations();
        let json = serde_json::to_string(&bts).unwrap();
        assert!(json.contains("registrations_list"), "list must be serialized");
        assert!(json.contains("4242"));
        // the raw HashSet field stays skipped
        assert!(!json.contains("\"registrations\":"), "raw set must be skipped");
    }
}

#[cfg(test)]
mod ip_snr_tests {
    use super::*;

    #[test]
    fn bts_serializes_ip_evm_rssi() {
        let mut bts = TelemetryBts::new("100000002".to_string(), Some("10.19.144.201".to_string()));
        bts.evm_pct = Some(1.25);
        bts.rssi_dbfs = Some(-3.3);
        let json = serde_json::to_string(&bts).unwrap();
        assert!(json.contains("10.19.144.201"), "ip serialized");
        assert!(json.contains("\"evm_pct\":1.25"), "evm serialized");
        assert!(json.contains("\"rssi_dbfs\":-3.3"), "rssi serialized");
    }

    #[test]
    fn ms_rssi_per_issi_serializes() {
        let mut bts = TelemetryBts::new("bts-1".to_string(), None);
        // simulate two MsRssi events
        bts.ms_rssi.insert(90, -3.3);
        bts.ms_rssi.insert(100, -8.8);
        let mut v: Vec<(u32,f32)> = bts.ms_rssi.iter().map(|(k,val)|(*k,*val)).collect();
        v.sort_unstable_by_key(|(k,_)|*k);
        bts.ms_rssi_out = v;
        let json = serde_json::to_string(&bts).unwrap();
        assert!(json.contains("ms_rssi_out"));
        assert!(json.contains("90") && json.contains("-3.3"));
        assert!(json.contains("100") && json.contains("-8.8"));
    }

    #[test]
    fn bts_ip_optional() {
        let bts = TelemetryBts::new("bts-x".to_string(), None);
        let json = serde_json::to_string(&bts).unwrap();
        assert!(json.contains("\"ip\":null"));
    }
}

#[cfg(test)]
mod sds_persist_tests {
    use super::*;
    use std::sync::Arc;

    fn entry(source_issi: u32, text: &str) -> SdsLogEntry {
        SdsLogEntry {
            at_ms: now_ms(),
            direction: "rx".to_string(),
            source_issi,
            dest_issi: 0,
            is_group: false,
            protocol_id: 3,
            text: text.to_string(),
        }
    }

    #[test]
    fn sds_telemetry_survives_restart_via_store() {
        let path = std::env::temp_dir().join(format!("brew-telemetry-test-{}.bin", uuid::Uuid::new_v4().simple()));
        {
            let store = Arc::new(crate::store::Store::open(&path).unwrap());
            let mut t = TelemetryState::with_store(store);
            t.record_sds_telemetry("bts-1", &entry(1001, "hello"));
            assert_eq!(t.recent_sds_out.len(), 1);
        }
        // A new TelemetryState over the same file must replay the history.
        {
            let store = Arc::new(crate::store::Store::open(&path).unwrap());
            let t = TelemetryState::with_store(store);
            assert_eq!(t.recent_sds_out.len(), 1, "sds telemetry restored");
            assert_eq!(t.recent_sds_out[0].bts, "bts-1");
            assert_eq!(t.recent_sds_out[0].source_issi, 1001);
            assert_eq!(t.recent_sds_out[0].text, "hello");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn recent_sds_out_is_newest_first_and_capped() {
        let mut t = TelemetryState::default();
        for i in 0..205u32 {
            t.record_sds_telemetry("bts-1", &entry(i, "x"));
        }
        assert_eq!(t.recent_sds_out.len(), 200, "capped at 200");
        assert_eq!(t.recent_sds_out[0].source_issi, 204, "newest first");
    }
}

#[cfg(test)]
mod registration_log_tests {
    use super::*;

    #[test]
    fn record_brew_registration_logs_newest_first_and_capped() {
        let mut t = TelemetryState::default();
        t.record_brew_registration(1001, "register");
        t.record_brew_registration(1002, "register");
        t.record_brew_registration(1001, "deregister");
        assert_eq!(t.recent_brew_regs_out.len(), 3);
        assert_eq!(t.recent_brew_regs_out[0].issi, 1001);
        assert_eq!(t.recent_brew_regs_out[0].kind, "deregister");
        for i in 0..60 {
            t.record_brew_registration(i, "register");
        }
        assert_eq!(t.recent_brew_regs_out.len(), 50, "log capped at 50 entries");
    }

    #[test]
    fn record_brew_rssi_keeps_latest_per_issi() {
        let mut t = TelemetryState::default();
        t.record_brew_rssi(4013, -12.5);
        t.record_brew_rssi(4013, -8.0);
        t.record_brew_rssi(5551, -20.0);
        assert_eq!(t.brew_ms_rssi.get(&4013), Some(&-8.0));
        assert_eq!(t.brew_ms_rssi.get(&5551), Some(&-20.0));
    }

    #[test]
    fn registration_log_merges_and_tags_brew_and_basestation_events() {
        let mut t = TelemetryState::default();
        t.stations.insert("bts-1".to_string(), TelemetryBts::new("bts-1".to_string(), None));
        t.stations.get_mut("bts-1").unwrap().push_reg(2001, "register");
        // Ensure a distinct, later timestamp so ordering is deterministic
        // rather than relying on both events landing in the same millisecond.
        std::thread::sleep(std::time::Duration::from_millis(2));
        t.record_brew_registration(3001, "register");

        let log = t.registration_log();
        assert_eq!(log.len(), 2);
        // Newest first: the Brew-channel event was recorded after the
        // Basestation one, so it must sort first.
        assert_eq!(log[0].bts, "brew");
        assert_eq!(log[0].entry.issi, 3001);
        assert_eq!(log[1].bts, "bts-1");
        assert_eq!(log[1].entry.issi, 2001);
    }

    #[test]
    fn registration_log_caps_at_100_across_all_sources() {
        let mut t = TelemetryState::default();
        for i in 0..60u32 {
            t.record_brew_registration(i, "register");
        }
        t.stations.insert("bts-1".to_string(), TelemetryBts::new("bts-1".to_string(), None));
        for i in 0..60u32 {
            t.stations.get_mut("bts-1").unwrap().push_reg(i, "register");
        }
        assert_eq!(t.registration_log().len(), 100);
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn station_version_parses_and_serializes() {
        let ev: TelemetryEvent = serde_json::from_str(
            r#"{"StationVersion":{"version":"v0.5.1","build":"v0.5.1-3ac72f7","upstream":"0.1.0"}}"#,
        ).unwrap();
        let TelemetryEvent::StationVersion { version, build, .. } = ev else { panic!("wrong variant") };
        let mut bts = TelemetryBts::new("bts".into(), None);
        bts.version = Some(version);
        bts.build = Some(build);
        let json = serde_json::to_string(&bts).unwrap();
        assert!(json.contains("\"version\":\"v0.5.1\""));
        assert!(json.contains("\"build\":\"v0.5.1-3ac72f7\""));
    }

    #[test]
    fn cell_rf_parses() {
        let ev: TelemetryEvent = serde_json::from_str(
            r#"{"CellRf":{"cell":1,"event":{"SdrHealth":{"temperature_c":41.0,"tx_gains":[],"rx_gains":[]}}}}"#,
        ).unwrap();
        let TelemetryEvent::CellRf { cell: 1, event: CellRfEvent::SdrHealth(h) } = ev else { panic!("wrong event") };
        assert_eq!(h.temperature_c, Some(41.0));
    }
}

#[cfg(test)]
mod roaming_tests {
    use super::*;

    fn with_station(t: &mut TelemetryState, id: &str, issis: &[u32]) {
        let mut bts = TelemetryBts::new(id.to_string(), None);
        for &i in issis { bts.registrations.insert(i); }
        bts.sync_registrations();
        t.stations.insert(id.to_string(), bts);
    }

    #[test]
    fn take_roamed_drops_issi_from_other_stations_only() {
        let mut t = TelemetryState::default();
        with_station(&mut t, "1000001", &[2320972]);
        with_station(&mut t, "1000002", &[2320972, 2320001]);
        with_station(&mut t, "1000003", &[2320001]);
        let old = t.take_roamed(2320972, "1000001");
        assert_eq!(old, vec!["1000002".to_string()]);
        assert_eq!(t.stations["1000001"].registrations_list, vec![2320972], "new station keeps it");
        assert_eq!(t.stations["1000002"].registrations_list, vec![2320001], "other ISSIs untouched");
        let e = &t.stations["1000002"].recent_regs_out[0];
        assert_eq!((e.issi, e.kind, e.roamed_to.as_deref()), (2320972, "roamed", Some("1000001")));
        assert!(t.stations["1000003"].recent_regs_out.is_empty());
    }

    #[test]
    fn take_roamed_without_other_holder_is_noop() {
        let mut t = TelemetryState::default();
        with_station(&mut t, "1000001", &[7]);
        assert!(t.take_roamed(7, "1000001").is_empty());
        assert!(t.stations["1000001"].recent_regs_out.is_empty());
    }

    #[test]
    fn roamed_entry_serializes_target_and_plain_entries_do_not() {
        let roamed = RegLogEntry { at_ms: 1, issi: 7, kind: "roamed", roamed_to: Some("1000001".into()) };
        assert!(serde_json::to_string(&roamed).unwrap().contains("\"roamed_to\":\"1000001\""));
        let plain = RegLogEntry { at_ms: 1, issi: 7, kind: "register", roamed_to: None };
        assert!(!serde_json::to_string(&plain).unwrap().contains("roamed_to"));
    }
}

/// Load simulation for `[roaming] kick_old_issi`: 10 Basestations, 50 radios
/// roaming between them. Each fake Basestation answers a `KickMs` like
/// FlowStation does (KickMsResponse + an MsDeregistration on its telemetry).
/// Run with `cargo test roaming_sim -- --nocapture` for the numbers.
#[cfg(test)]
mod roaming_sim_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const STATIONS: usize = 10;
    const RADIOS: usize = 50;
    const ISSI_BASE: u32 = 2_320_000;
    /// Scaled-down `kick_delay_ms` so the simulation stays fast.
    const SIM_KICK_DELAY_MS: u64 = 20;

    fn ev(kind: &str, issi: u32) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ kind: { "issi": issi } })).unwrap()
    }

    fn bts_id(i: usize) -> String { format!("10000{:02}", i + 1) }

    /// Small deterministic PRNG so every run sees the same roaming pattern.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n
        }
    }

    #[derive(Debug, Default)]
    struct Report {
        moves: usize,
        kicks: usize,
        /// Kicks that did not go to the station the radio left (`None`: not checked).
        wrong_kicks: Option<usize>,
        /// Registrations listed beyond one per radio (stale entries).
        stale: usize,
        /// Radios not listed on the Basestation they are actually on.
        misplaced: usize,
        /// Visible "roamed" log rows (each station keeps its last 50 rows).
        roamed_log_entries: usize,
        duplicate_deregister_entries: usize,
    }

    struct Sim {
        state: Arc<AppState>,
        kicks: Arc<Mutex<Vec<(String, u32)>>>,
        done: Arc<AtomicUsize>,
        loc: Vec<usize>,
    }

    impl Sim {
        async fn new(kick_old_issi: bool) -> Self {
            let mut config = crate::config::Config::default();
            config.storage.enabled = false;
            config.roaming.kick_old_issi = kick_old_issi;
            config.roaming.kick_delay_ms = SIM_KICK_DELAY_MS;
            let (state, _rx) = AppState::new(config, std::path::PathBuf::from("test.toml"));
            let state = Arc::new(state);
            let kicks = Arc::new(Mutex::new(Vec::new()));
            let done = Arc::new(AtomicUsize::new(0));
            for i in 0..STATIONS {
                let id = bts_id(i);
                state.telemetry.write().await.stations.insert(id.clone(), TelemetryBts::new(id.clone(), None));
                let mut rx = crate::control::attach_test_session(&state, &id).await;
                let (state, kicks, done) = (state.clone(), kicks.clone(), done.clone());
                tokio::spawn(async move {
                    while let Some(frame) = rx.recv().await {
                        let v: serde_json::Value = serde_json::from_slice(&frame).unwrap();
                        let issi = v["KickMs"]["issi"].as_u64().expect("only KickMs expected") as u32;
                        kicks.lock().unwrap().push((id.clone(), issi));
                        let resp = serde_json::to_vec(&crate::control::ControlResponse::KickMsResponse { issi, success: true }).unwrap();
                        crate::control::deliver_test_response(&state, &id, &resp).await;
                        handle_event(&state, &id, &ev("MsDeregistration", issi)).await;
                        done.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
            Self { state, kicks, done, loc: vec![usize::MAX; RADIOS] }
        }

        /// Radio `r` registers on station `s`. With `settle`, waits until every
        /// kick this caused has been answered (cells far apart in time).
        async fn register(&mut self, r: usize, s: usize, settle: bool) {
            let before = self.kicks.lock().unwrap().len();
            let holders = {
                let t = self.state.telemetry.read().await;
                t.stations.iter().filter(|(id, b)| **id != bts_id(s) && b.registrations.contains(&(ISSI_BASE + r as u32))).count()
            };
            handle_event(&self.state, &bts_id(s), &ev("MsRegistration", ISSI_BASE + r as u32)).await;
            self.loc[r] = s;
            if settle && self.state.config.roaming.kick_old_issi {
                self.wait_done(before + holders).await;
            }
        }

        async fn wait_done(&self, n: usize) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while self.done.load(Ordering::SeqCst) < n {
                assert!(std::time::Instant::now() < deadline, "kicks not answered in time");
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        }

        /// Waits until the kick count stops changing and every kick is answered.
        async fn quiesce(&self) {
            loop {
                let k = self.kicks.lock().unwrap().len();
                self.wait_done(k).await;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                if self.kicks.lock().unwrap().len() == k { break; }
            }
        }

        async fn report(&self, moves: usize, expected_kicks: Option<&[(String, u32)]>) -> Report {
            let t = self.state.telemetry.read().await;
            let kicks = self.kicks.lock().unwrap().clone();
            let wrong_kicks = expected_kicks.map(|want| {
                let (mut want, mut got) = (want.to_vec(), kicks.clone());
                want.sort(); got.sort();
                got.iter().filter(|k| !want.contains(k)).count() + want.iter().filter(|k| !got.contains(k)).count()
            });
            let listed: usize = t.stations.values().map(|b| b.registrations.len()).sum();
            let misplaced = (0..RADIOS).filter(|&r| !t.stations[&bts_id(self.loc[r])].registrations.contains(&(ISSI_BASE + r as u32))).count();
            let entries: Vec<&RegLogEntry> = t.stations.values().flat_map(|b| b.recent_regs.iter()).collect();
            Report {
                moves,
                kicks: kicks.len(),
                wrong_kicks,
                stale: listed.saturating_sub(RADIOS - misplaced),
                misplaced,
                roamed_log_entries: entries.iter().filter(|e| e.kind == "roamed").count(),
                duplicate_deregister_entries: entries.iter().filter(|e| e.kind == "deregister").count(),
            }
        }
    }

    /// Normal roaming: radios move one at a time, each kick is answered
    /// before the next move (realistic: re-registering takes seconds, the
    /// kick milliseconds).
    async fn run_settled(kick: bool, moves: usize) -> Report {
        let mut sim = Sim::new(kick).await;
        let mut rng = Lcg(0x5EB_A571A);
        for r in 0..RADIOS { sim.register(r, rng.next(STATIONS), true).await; }
        let mut expected = Vec::new();
        for _ in 0..moves {
            let r = rng.next(RADIOS);
            let mut s = rng.next(STATIONS);
            if s == sim.loc[r] { s = (s + 1) % STATIONS; }
            expected.push((bts_id(sim.loc[r]), ISSI_BASE + r as u32));
            sim.register(r, s, true).await;
        }
        sim.quiesce().await;
        sim.report(moves, Some(if kick { &expected[..] } else { &[] })).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn roaming_sim_10_stations_50_radios_with_kick() {
        let rep = run_settled(true, 1000).await;
        println!("kick_old_issi=1, settled: {rep:?}");
        assert_eq!(rep.kicks, 1000, "one kick per move");
        assert_eq!(rep.wrong_kicks, Some(0), "every kick went to the station the radio left");
        assert_eq!(rep.stale, 0, "no radio listed twice");
        assert_eq!(rep.misplaced, 0, "every radio listed where it is");
        assert_eq!(rep.duplicate_deregister_entries, 0, "kick confirmations not logged twice");
        assert!(rep.roamed_log_entries > 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn roaming_sim_10_stations_50_radios_without_kick() {
        let rep = run_settled(false, 1000).await;
        println!("kick_old_issi=0, settled: {rep:?}");
        assert_eq!(rep.kicks, 0, "switch off: nothing is kicked");
        assert_eq!(rep.roamed_log_entries, 0);
        assert!(rep.stale > 0, "without the kick old registrations pile up (old behaviour)");
    }

    /// Worst case: ping-pong (A -> B -> A back to back) fired without any
    /// pause, faster than `kick_delay_ms`. The kick to A must be skipped
    /// (the radio is back there), the kick to B must still happen.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn roaming_sim_burst_and_ping_pong() {
        let mut sim = Sim::new(true).await;
        let mut rng = Lcg(0xB0B);
        for r in 0..RADIOS { sim.register(r, rng.next(STATIONS), true).await; }
        let mut moves = 0;
        // 10 rounds; in each round every radio ping-pongs once, all 50 back
        // to back, then the network gets time to settle before the next round.
        for _ in 0..10 {
            for r in 0..RADIOS {
                let a = sim.loc[r];
                let b = (a + 1 + rng.next(STATIONS - 1)) % STATIONS;
                sim.register(r, b, false).await;
                sim.register(r, a, false).await;
                moves += 2;
            }
            tokio::time::sleep(std::time::Duration::from_millis(3 * SIM_KICK_DELAY_MS)).await;
            sim.quiesce().await;
        }
        sim.quiesce().await;
        let rep = sim.report(moves, None).await;
        println!("kick_old_issi=1, burst + ping-pong: {rep:?}");
        assert_eq!(rep.misplaced, 0, "no radio kicked off the cell it is on");
        assert_eq!(rep.stale, 0, "no radio listed twice");
        assert_eq!(rep.duplicate_deregister_entries, 0);
        assert_eq!(rep.kicks, moves / 2, "only the B side of each ping-pong is kicked");
    }
}
