//! Carrier registry: which Basestation connection serves which TETRA cell.
//!
//! A Basestation announces its cells with `SERVICE_BTS_INFO` (see `protocol`)
//! once it is connected, and again whenever its cell config changes. Each cell
//! is keyed by `(mcc, mnc, carrier, carrier_ext)`: what a radio's serving cell
//! knows about a neighbour, so a cell-to-cell handover can find the target
//! Basestation by the carrier the radio names. A Basestation only sends the
//! message to a server that offered the `bts-info` extension on the WebSocket
//! upgrade (`X-Brew-Extensions`), so servers that do not know it (TetraPack,
//! Brandmeister) never see it.
//!
//! A carrier should be served by one Basestation. When two connections claim
//! the same key, the one that announced last serves it (a Basestation that
//! reconnects before its old connection timed out must not be shadowed by that
//! dead connection), the clash is logged and the dashboard flags it. When the
//! serving connection goes away, the carrier falls back to the other claimant.

use crate::state::{AppState, ClientId, ClientMode};
use std::{collections::{BTreeMap, HashMap, HashSet}, sync::Arc};
use tracing::{info, warn};

/// Response header on the WebSocket upgrade listing the Brew extensions this
/// server understands, comma separated.
pub const X_BREW_EXTENSIONS: &str = "X-Brew-Extensions";

/// What this server offers in `X_BREW_EXTENSIONS`: `bts-info`
/// (`SERVICE_BTS_INFO`), `handover-v1` (the handover messages, see `handover`),
/// `ms-cell` (`SERVICE_MS_CELL`), `ms-moved` (`SERVICE_MS_MOVED`, to a
/// Basestation that also names it in its capabilities), `ms-reselection`
/// (`SERVICE_MS_RESELECTION`, see `roaming_log`), `call-restore`
/// (`SERVICE_CALL_RESTORE`, see `call_restore`), `call-handoff`
/// (`SERVICE_CALL_HANDOFF`, see `call_restore`) and
/// `cell-directory` (`SERVICE_CELL_DIRECTORY`, to a Basestation that also names
/// it in its capabilities, see `directory_updates`) and `group-access`
/// (`SERVICE_GROUP_ACCESS`, likewise, see `group_access::updates`) and
/// `issi-check` (`SERVICE_ISSI_CHECK` answers, see `netlink`).
pub const EXTENSIONS: &str = "bts-info, handover-v1, ms-cell, ms-moved, ms-reselection, call-restore, call-handoff, cell-directory, group-access, issi-check";

/// Capability in `SERVICE_BTS_INFO` (and extension in `X-Brew-Extensions`) for
/// `SERVICE_CELL_DIRECTORY`.
pub const CAPABILITY_CELL_DIRECTORY: &str = "cell-directory";

/// Most neighbours one cell may list: a cell broadcasts at most 7 (TS 100 392-2
/// table 18.71, "number of CA neighbour cells", 3 bits).
pub const MAX_NEIGHBOURS: usize = 7;

/// Stations (by login) whose cells the directory keeps as offline after they
/// disconnect; the oldest is forgotten first.
pub const MAX_OFFLINE_STATIONS: usize = 64;

/// Most cells one `SERVICE_CELL_DIRECTORY` lists.
pub const MAX_DIRECTORY_CELLS: usize = 256;

/// BS service details flags a cell may report (TS 100 392-2 table 18.26, names
/// as FlowStation's config uses them).
pub const SERVICE_DETAILS: &[&str] = &[
    "registration",
    "deregistration",
    "priority_cell",
    "no_minimum_mode",
    "migration",
    "system_wide_services",
    "voice_service",
    "circuit_mode_data_service",
    "sndcp_service",
    "aie_service",
    "advanced_link",
];

/// Most cells one `SERVICE_BTS_INFO` may announce (a FlowStation station runs
/// at most a handful of SDRs on one Brew connection).
pub const MAX_CELLS: usize = 16;

const MAX_CAPABILITIES: usize = 16;
const MAX_CAPABILITY_LEN: usize = 32;

/// Identifies a cell network wide: network code plus main carrier number and
/// its band plan (the absolute "main carrier number extension" of TS 100 392-2:
/// frequency band, offset, duplex spacing, reverse operation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct CarrierKey {
    pub mcc: u16,
    pub mnc: u16,
    pub carrier: u16,
    pub carrier_ext: u16,
}

impl CarrierKey {
    /// Downlink frequency in Hz (TS 100 392-2 clause 21.4.4.1): band (bits 9-6 of
    /// the extension) x 100 MHz + carrier x 25 kHz + offset (bits 5-4: 0, +6.25,
    /// -6.25, +12.5 kHz). The uplink also needs the duplex spacing table, which a
    /// Basestation may override, so it is not derived here.
    pub fn downlink_hz(&self) -> u64 {
        let band = u64::from((self.carrier_ext >> 6) & 0xf);
        let offset: i64 = match (self.carrier_ext >> 4) & 0x3 {
            1 => 6_250,
            2 => -6_250,
            3 => 12_500,
            _ => 0,
        };
        (band * 100_000_000 + u64::from(self.carrier) * 25_000).saturating_add_signed(offset)
    }

    /// Whether `other` is the same cell on the downlink: same network, carrier,
    /// band and offset (bits 9-4 of the extension). Duplex spacing and reverse
    /// operation (bits 3-0) only set the uplink, and a neighbour entry without
    /// an extension takes the serving cell's band plan (TS 100 392-2 table 18.64
    /// note 1), so they may differ between how a cell is listed and how its
    /// station announces it.
    pub fn same_downlink(&self, other: &CarrierKey) -> bool {
        self.mcc == other.mcc && self.mnc == other.mnc && self.carrier == other.carrier
            && (self.carrier_ext & 0x3f0) == (other.carrier_ext & 0x3f0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Cell {
    #[serde(flatten)]
    pub key: CarrierKey,
    pub location_area: u16,
    /// The cell's number within its station (FlowStation: 0 = primary, then the
    /// `[[cells]]` ids). Only a label, see `cell_name`.
    pub cell: u8,
    /// What the cell broadcasts, when the station reported it (optional fields).
    #[serde(flatten)]
    pub extra: CellExtra,
}

/// Optional fields of a cell in `SERVICE_BTS_INFO`: what the cell broadcasts,
/// so other stations can list it as a neighbour with the right values. Absent
/// = not reported.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct CellExtra {
    /// MS_TXPWR_MAX_CELL in dBm: 15 to 45 in 5 dB steps (table 18.57).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms_txpwr_max_dbm: Option<u8>,
    /// RXLEV_ACCESS_MIN in dBm: -125 to -50 in 5 dB steps (table 21.65).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rxlev_access_min_dbm: Option<i16>,
    /// BS service details (table 18.26), flag name -> set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_details: Option<BTreeMap<String, bool>>,
    /// The neighbour cells the cell broadcasts (D-NWRK-BROADCAST).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neighbours: Option<Vec<NeighbourRef>>,
}

/// A neighbour cell a cell broadcasts: its network, carrier and band plan, and
/// its LA when the station knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct NeighbourRef {
    pub mcc: u16,
    pub mnc: u16,
    pub carrier: u16,
    pub carrier_ext: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_area: Option<u16>,
}

/// Name of a cell for the dashboard and the logs: the station (its Brew login,
/// see `station_name`) and the cell's number within it, e.g. `2320997/3`. Not
/// a radio-side identity: on the air interface a cell is only its network
/// code, carrier and band plan (`CarrierKey`).
pub fn cell_name(station: &str, cell: u8) -> String {
    format!("{station}/{cell}")
}

/// A station's name: the Brew login it authenticated with, or without `[auth]`
/// the first 8 hex digits of its connection id.
pub fn station_name(id: ClientId, username: Option<&str>) -> String {
    match username {
        Some(user) if !user.is_empty() => user.to_string(),
        _ => id.simple().to_string()[..8].to_string(),
    }
}

/// What one connection announced last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtsInfo {
    pub cells: Vec<Cell>,
    pub capabilities: Vec<String>,
}

#[derive(Default)]
pub struct CarrierRegistry {
    /// Each connection's last announcement, with the order it was made in.
    by_client: HashMap<ClientId, (u64, BtsInfo)>,
    /// The connection serving each carrier.
    table: HashMap<CarrierKey, ClientId>,
    seq: u64,
    /// ISSI -> the connection and cell it last reported the radio on
    /// (`SERVICE_MS_CELL`), and when (ms).
    ms_cells: HashMap<u32, (ClientId, CarrierKey, u64)>,
    /// The Brew login of each announcing connection that has one.
    logins: HashMap<ClientId, String>,
    /// Login -> the cells its station announced last and when it went away
    /// (ms): shown as offline in the cell directory until it announces again.
    offline: HashMap<String, (Vec<Cell>, u64)>,
}

/// One cell of the cell directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub station: String,
    /// The connection serving it, `None` when offline.
    pub client: Option<ClientId>,
    pub cell: Cell,
    /// When an offline station went away (ms).
    pub last_seen_ms: Option<u64>,
}

impl CarrierRegistry {
    /// Records `client`'s announcement, replacing its previous one. Returns
    /// every other connection that also claims one of its carriers (`client`
    /// serves those now).
    pub fn announce(&mut self, client: ClientId, info: BtsInfo) -> Vec<(CarrierKey, ClientId)> {
        self.seq += 1;
        let mut touched: HashSet<CarrierKey> = info.cells.iter().map(|c| c.key).collect();
        if let Some((_, old)) = self.by_client.insert(client, (self.seq, info)) {
            touched.extend(old.cells.iter().map(|c| c.key));
        }
        let mut clashes = Vec::new();
        let mut keys: Vec<_> = touched.into_iter().collect();
        keys.sort_unstable();
        for key in keys {
            self.reassign(key);
            if self.table.get(&key) == Some(&client) {
                clashes.extend(self.claimants(key).filter(|(other, _)| *other != client).map(|(other, _)| (key, other)));
            }
        }
        clashes
    }

    /// Forgets a connection. Returns the carriers it served that another
    /// claimant serves now.
    pub fn remove(&mut self, client: ClientId) -> Vec<(CarrierKey, ClientId)> {
        self.ms_cells.retain(|_, (id, _, _)| *id != client);
        let login = self.logins.remove(&client);
        let Some((_, info)) = self.by_client.remove(&client) else { return Vec::new() };
        if let Some(login) = login {
            // Another live connection with the same login keeps it online.
            if !self.logins.values().any(|l| *l == login) {
                self.offline.insert(login, (info.cells.clone(), crate::telemetry::now_ms()));
                while self.offline.len() > MAX_OFFLINE_STATIONS {
                    let Some(oldest) = self.offline.iter().min_by_key(|(_, (_, at))| *at).map(|(l, _)| l.clone()) else { break };
                    self.offline.remove(&oldest);
                }
            }
        }
        let mut handed_over = Vec::new();
        for cell in &info.cells {
            let served = self.table.get(&cell.key) == Some(&client);
            self.reassign(cell.key);
            if served {
                if let Some(next) = self.table.get(&cell.key) { handed_over.push((cell.key, *next)); }
            }
        }
        handed_over
    }

    /// The Brew login `client` authenticated with: its cells stay in the
    /// directory as offline after it goes, under this name.
    pub fn set_login(&mut self, client: ClientId, login: &str) {
        self.offline.remove(login);
        self.logins.insert(client, login.to_string());
    }

    /// Every cell known to the server: the announced ones (online) and those of
    /// stations with a login that went away (offline), sorted by station and
    /// cell. `station` names a connection (see `station_name`).
    pub fn directory(&self, station: impl Fn(ClientId) -> String) -> Vec<DirectoryEntry> {
        let mut entries: Vec<DirectoryEntry> = self
            .by_client
            .iter()
            .flat_map(|(id, (_, info))| {
                let name = station(*id);
                info.cells.iter().map(move |cell| DirectoryEntry { station: name.clone(), client: Some(*id), cell: cell.clone(), last_seen_ms: None })
            })
            .collect();
        entries.extend(self.offline.iter().flat_map(|(login, (cells, at))| {
            cells.iter().map(move |cell| DirectoryEntry { station: login.clone(), client: None, cell: cell.clone(), last_seen_ms: Some(*at) })
        }));
        entries.sort_by(|a, b| (&a.station, a.cell.cell, a.client).cmp(&(&b.station, b.cell.cell, b.client)));
        entries
    }

    /// `client` reports `issi` on its cell `(carrier, carrier_ext)`. Returns
    /// that cell, or `None` (nothing recorded) when `client` announced no
    /// such cell.
    pub fn set_ms_cell(&mut self, issi: u32, client: ClientId, carrier: u16, carrier_ext: u16) -> Option<Cell> {
        let (_, info) = self.by_client.get(&client)?;
        let cell = info.cells.iter().find(|c| c.key.carrier == carrier && c.key.carrier_ext == carrier_ext)?.clone();
        self.ms_cells.insert(issi, (client, cell.key, crate::telemetry::now_ms()));
        Some(cell)
    }

    /// The cell `client` last reported `issi` on, as long as it still
    /// announces it. A report from another connection does not count: the
    /// radio is wherever its registration is.
    pub fn ms_cell(&self, issi: u32, client: ClientId) -> Option<&Cell> {
        let (id, key, _) = self.ms_cells.get(&issi).filter(|(id, _, _)| *id == client)?;
        self.info(*id)?.cells.iter().find(|c| c.key == *key)
    }

    /// When `issi`'s cell was last reported, by whichever connection.
    pub fn ms_cell_reported_at(&self, issi: u32) -> Option<(ClientId, u64)> {
        self.ms_cells.get(&issi).map(|(id, _, at)| (*id, *at))
    }

    /// The connection serving `key`, if any.
    pub fn owner(&self, key: &CarrierKey) -> Option<ClientId> {
        self.table.get(key).copied()
    }

    /// The connection serving `key` in location area `la`: the newest
    /// claimant whose cell on `key` is in `la`. Carriers are reused across a
    /// network, so the same key may name cells in different LAs.
    pub fn owner_in_location_area(&self, key: &CarrierKey, la: u16) -> Option<ClientId> {
        self.by_client.iter()
            .filter(|(_, (_, info))| info.cells.iter().any(|c| c.key == *key && c.location_area == la))
            .max_by_key(|(_, (seq, _))| *seq)
            .map(|(id, _)| *id)
    }

    /// The connection serving `key` (in location area `la`, when given): the
    /// exact key first, else the newest claimant of a cell on the same downlink
    /// (`CarrierKey::same_downlink`).
    pub fn find_target(&self, key: &CarrierKey, la: Option<u16>) -> Option<ClientId> {
        let exact = match la {
            Some(la) => self.owner_in_location_area(key, la),
            None => self.owner(key),
        };
        exact.or_else(|| {
            self.by_client.iter()
                .filter(|(_, (_, info))| info.cells.iter().any(|c| c.key.same_downlink(key) && la.is_none_or(|la| la == c.location_area)))
                .max_by_key(|(_, (seq, _))| *seq)
                .map(|(id, _)| *id)
        })
    }

    /// What `client` announced last.
    pub fn info(&self, client: ClientId) -> Option<&BtsInfo> {
        self.by_client.get(&client).map(|(_, info)| info)
    }

    /// Every connection's last announcement.
    pub fn announcements(&self) -> impl Iterator<Item = (ClientId, &BtsInfo)> + '_ {
        self.by_client.iter().map(|(id, (_, info))| (*id, info))
    }

    /// Whether more than one connection claims `key`.
    pub fn is_duplicate(&self, key: &CarrierKey) -> bool {
        self.claimants(*key).nth(1).is_some()
    }

    /// Every carrier claimed by more than one connection, sorted.
    pub fn duplicates(&self) -> Vec<CarrierKey> {
        let mut keys: Vec<_> = self.table.keys().copied().filter(|k| self.is_duplicate(k)).collect();
        keys.sort_unstable();
        keys
    }

    fn claimants(&self, key: CarrierKey) -> impl Iterator<Item = (ClientId, u64)> + '_ {
        self.by_client.iter()
            .filter(move |(_, (_, info))| info.cells.iter().any(|c| c.key == key))
            .map(|(id, (seq, _))| (*id, *seq))
    }

    /// Hands `key` to its newest claimant, or drops it when nobody claims it.
    fn reassign(&mut self, key: CarrierKey) {
        match self.claimants(key).max_by_key(|(_, seq)| *seq).map(|(id, _)| id) {
            Some(id) => { self.table.insert(key, id); }
            None => { self.table.remove(&key); }
        }
    }
}

#[derive(serde::Deserialize)]
struct BtsInfoMessage {
    mcc: u16,
    mnc: u16,
    cells: Vec<CellMessage>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(serde::Deserialize)]
struct CellMessage {
    carrier: u16,
    carrier_ext: u16,
    location_area: u16,
    /// Optional; defaults to the cell's position in the list.
    #[serde(default)]
    cell: Option<u8>,
    // Optional, checked one by one: a bad one is dropped, the cell is kept.
    #[serde(default)]
    ms_txpwr_max_dbm: Option<serde_json::Value>,
    #[serde(default)]
    rxlev_access_min_dbm: Option<serde_json::Value>,
    #[serde(default)]
    service_details: Option<serde_json::Value>,
    #[serde(default)]
    neighbours: Option<serde_json::Value>,
}

/// The optional fields of a cell. A value that is out of range or of the wrong
/// type is logged and left out: the cell itself still counts, so a station
/// with a newer or wrong optional field keeps its handovers.
fn parse_cell_extra(c: &CellMessage, mcc: u16, mnc: u16) -> CellExtra {
    let carrier = c.carrier;
    let mut extra = CellExtra::default();
    if let Some(v) = &c.ms_txpwr_max_dbm {
        match v.as_u64().filter(|p| (15..=45).contains(p) && p % 5 == 0) {
            Some(p) => extra.ms_txpwr_max_dbm = Some(p as u8),
            None => warn!(carrier, value = %v, "BTS info: ms_txpwr_max_dbm must be 15-45 in 5 dB steps, ignored"),
        }
    }
    if let Some(v) = &c.rxlev_access_min_dbm {
        match v.as_i64().filter(|r| (-125..=-50).contains(r) && r % 5 == 0) {
            Some(r) => extra.rxlev_access_min_dbm = Some(r as i16),
            None => warn!(carrier, value = %v, "BTS info: rxlev_access_min_dbm must be -125 to -50 in 5 dB steps, ignored"),
        }
    }
    if let Some(v) = &c.service_details {
        let flags: Option<BTreeMap<String, bool>> = v.as_object().and_then(|o| {
            o.iter().filter(|(k, _)| SERVICE_DETAILS.contains(&k.as_str())).map(|(k, b)| b.as_bool().map(|b| (k.clone(), b))).collect()
        });
        match flags {
            Some(f) => extra.service_details = Some(f),
            None => warn!(carrier, value = %v, "BTS info: service_details must be an object of true/false, ignored"),
        }
    }
    if let Some(v) = &c.neighbours {
        match parse_neighbours(v, mcc, mnc) {
            Ok(n) => extra.neighbours = Some(n),
            Err(e) => warn!(carrier, error = %e, "BTS info: neighbours ignored"),
        }
    }
    extra
}

fn parse_neighbours(v: &serde_json::Value, mcc: u16, mnc: u16) -> Result<Vec<NeighbourRef>, String> {
    let list = v.as_array().ok_or("must be a list")?;
    if list.len() > MAX_NEIGHBOURS { return Err(format!("{} neighbours, at most {MAX_NEIGHBOURS}", list.len())); }
    let field = |n: &serde_json::Value, name: &str, max: u64, default: Option<u16>| -> Result<Option<u16>, String> {
        match n.get(name) {
            None | Some(serde_json::Value::Null) => Ok(default),
            Some(x) => x.as_u64().filter(|x| *x <= max).map(|x| Some(x as u16)).ok_or_else(|| format!("{name} must be 0-{max}")),
        }
    };
    list.iter()
        .map(|n| {
            Ok(NeighbourRef {
                mcc: field(n, "mcc", 0x3ff, Some(mcc))?.unwrap_or(mcc),
                mnc: field(n, "mnc", 0x3fff, Some(mnc))?.unwrap_or(mnc),
                carrier: field(n, "carrier", 0xfff, None)?.ok_or("carrier missing")?,
                carrier_ext: field(n, "carrier_ext", 0x3ff, None)?.ok_or("carrier_ext missing")?,
                location_area: field(n, "location_area", 0x3fff, None)?,
            })
        })
        .collect()
}

/// Parses and checks a `SERVICE_BTS_INFO` payload:
/// `{"mcc":N,"mnc":N,"cells":[{"carrier":N,"carrier_ext":N,"location_area":N,"cell":N}],"capabilities":["..."]}`.
pub fn parse_bts_info(json: &str) -> Result<BtsInfo, String> {
    let msg: BtsInfoMessage = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if msg.mcc > 0x3ff { return Err(format!("mcc {} out of range (0-1023)", msg.mcc)); }
    if msg.mnc > 0x3fff { return Err(format!("mnc {} out of range (0-16383)", msg.mnc)); }
    if msg.cells.len() > MAX_CELLS { return Err(format!("{} cells, at most {MAX_CELLS}", msg.cells.len())); }
    let mut cells: Vec<Cell> = Vec::with_capacity(msg.cells.len());
    for (index, c) in msg.cells.into_iter().enumerate() {
        if c.carrier > 0xfff { return Err(format!("carrier {} out of range (0-4095)", c.carrier)); }
        if c.carrier_ext > 0x3ff { return Err(format!("carrier_ext {} out of range (0-1023)", c.carrier_ext)); }
        if c.location_area > 0x3fff { return Err(format!("location_area {} out of range (0-16383)", c.location_area)); }
        let key = CarrierKey { mcc: msg.mcc, mnc: msg.mnc, carrier: c.carrier, carrier_ext: c.carrier_ext };
        if cells.iter().any(|known| known.key == key) { return Err(format!("carrier {} listed twice", c.carrier)); }
        // At most MAX_CELLS entries, so the position always fits.
        let cell = c.cell.unwrap_or(index as u8);
        if cells.iter().any(|known| known.cell == cell) { return Err(format!("cell number {cell} listed twice")); }
        let extra = parse_cell_extra(&c, msg.mcc, msg.mnc);
        cells.push(Cell { key, location_area: c.location_area, cell, extra });
    }
    if msg.capabilities.len() > MAX_CAPABILITIES { return Err(format!("{} capabilities, at most {MAX_CAPABILITIES}", msg.capabilities.len())); }
    if let Some(bad) = msg.capabilities.iter().find(|c| c.is_empty() || c.len() > MAX_CAPABILITY_LEN) {
        return Err(format!("capability {bad:?} must be 1-{MAX_CAPABILITY_LEN} characters"));
    }
    Ok(BtsInfo { cells, capabilities: msg.capabilities })
}

#[derive(serde::Deserialize)]
struct MsCellMessage {
    issi: u32,
    carrier: u16,
    carrier_ext: u16,
}

/// How close in time a cell report and a telemetry registration must be to
/// belong together (registration log, see `TelemetryState::tag_cell`).
pub const CELL_MATCH_MS: u64 = 10_000;

/// The cell `issi` was reported on within `CELL_MATCH_MS` before `now_ms`,
/// by the connection it is registered on: its name and carrier.
pub fn recent_cell(inner: &crate::state::Inner, issi: u32, now_ms: u64) -> Option<(String, u16)> {
    let (client, at) = inner.carriers.ms_cell_reported_at(issi)?;
    if now_ms.saturating_sub(at) > CELL_MATCH_MS { return None; }
    if inner.subscribers.get(&issi).is_none_or(|s| s.client_id != client) { return None; }
    let cell = inner.carriers.ms_cell(issi, client)?;
    let station = station_name(client, inner.clients.get(&client).and_then(|c| c.username.as_deref()));
    Some((cell_name(&station, cell.cell), cell.key.carrier))
}

/// A `SERVICE_MS_CELL` from `source`: `{"issi":N,"carrier":N,"carrier_ext":N}`,
/// the cell of the station the radio registered on. Only counts for a cell
/// the connection announced; shown per subscriber on the dashboard.
pub async fn handle_ms_cell(state: &Arc<AppState>, source: ClientId, json: &str) {
    let msg: MsCellMessage = match serde_json::from_str(json) {
        Ok(m) => m,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed MS cell service message");
            return;
        }
    };
    let mut inner = state.inner.write().await;
    if inner.clients.get(&source).is_none_or(|c| c.mode != ClientMode::Basestation) { return; }
    let station = station_name(source, inner.clients.get(&source).and_then(|c| c.username.as_deref()));
    let previous = inner.carriers.ms_cell(msg.issi, source).map(|c| crate::roaming_log::Place::cell(station.clone(), c.cell, c.key.carrier));
    match inner.carriers.set_ms_cell(msg.issi, source, msg.carrier, msg.carrier_ext) {
        Some(cell) => {
            tracing::debug!(%source, issi = msg.issi, cell = cell.cell, carrier = msg.carrier, "subscriber is on cell");
            let to = crate::roaming_log::Place::cell(station.clone(), cell.cell, cell.key.carrier);
            inner.roaming_log.cell_reported(crate::telemetry::now_ms(), msg.issi, to, previous);
            drop(inner);
            // The registration log is kept with the telemetry.
            state.telemetry.write().await.tag_cell(msg.issi, &cell_name(&station, cell.cell), cell.key.carrier, crate::telemetry::now_ms());
        }
        None => warn!(%source, issi = msg.issi, carrier = msg.carrier, carrier_ext = msg.carrier_ext, "MS cell report for a cell the Basestation did not announce ignored"),
    }
}

/// A `SERVICE_BTS_INFO` from `source`. Only Basestation connections serve
/// cells; anything else is ignored.
pub async fn handle_bts_info(state: &Arc<AppState>, source: ClientId, json: &str) {
    let info = match parse_bts_info(json) {
        Ok(info) => info,
        Err(e) => {
            warn!(%source, error = %e, json, "malformed BTS info service message");
            return;
        }
    };
    let mut inner = state.inner.write().await;
    let Some(client) = inner.clients.get(&source) else { return };
    if client.mode != ClientMode::Basestation {
        warn!(%source, mode = client.mode.as_str(), "BTS info from a connection that is not a Basestation ignored");
        return;
    }
    let addr = client.remote_addr;
    let station = station_name(source, client.username.as_deref());
    let login = client.username.clone().filter(|u| !u.is_empty());
    let before = directory_fingerprint(&inner);
    if let Some(login) = login { inner.carriers.set_login(source, &login); }
    let cells: Vec<String> = info.cells.iter().map(|c| format!("{} carrier {} LA {}", cell_name(&station, c.cell), c.key.carrier, c.location_area)).collect();
    let capabilities = info.capabilities.join(",");
    let clashes = inner.carriers.announce(source, info);
    info!(%source, remote_addr = ?addr, ?cells, capabilities, "Basestation announced its cells");
    for (key, other) in clashes {
        let other_addr = inner.clients.get(&other).and_then(|c| c.remote_addr);
        warn!(%source, remote_addr = ?addr, other = %other, other_addr = ?other_addr, mcc = key.mcc, mnc = key.mnc,
            carrier = key.carrier, carrier_ext = key.carrier_ext, "carrier announced by two Basestations; the newest one serves it");
    }
    // The announcer always gets the directory; the others when it changed.
    let changed = before != directory_fingerprint(&inner);
    for (to, packet) in directory_updates(&inner, (!changed).then_some(source)) {
        if let Some(client) = inner.clients.get(&to) { let _ = client.tx.send(packet); }
    }
    // And who may use which group, if it asked for it.
    for (to, packet) in crate::group_access::updates(state, &inner, Some(source)) {
        if let Some(client) = inner.clients.get(&to) { let _ = client.tx.send(packet); }
    }
    drop(inner);
    // And the networks it may take radios from (extension `mni`).
    crate::visitors::announce(state, Some(source)).await;
}

fn directory_entries(inner: &crate::state::Inner) -> Vec<DirectoryEntry> {
    inner.carriers.directory(|id| station_name(id, inner.clients.get(&id).and_then(|c| c.username.as_deref())))
}

/// The whole directory as one string, to tell whether it changed.
pub fn directory_fingerprint(inner: &crate::state::Inner) -> String {
    directory_entries(inner)
        .iter()
        .map(|e| format!("{}|{:?}|{}|{}", e.station, e.client.is_some(), serde_json::to_string(&e.cell).unwrap_or_default(), e.last_seen_ms.unwrap_or(0)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `SERVICE_CELL_DIRECTORY` for every Basestation that announced the
/// `cell-directory` capability (or only for `only`), each without its own
/// cells: `{"cells":[{"id":"1000001/0","station":"1000001","cell":0,
/// "online":true,"mcc":N,"mnc":N,"carrier":N,"carrier_ext":N,
/// "location_area":N, ...optional fields of SERVICE_BTS_INFO...}]}`, offline
/// cells with `"online":false` and `last_seen_ms`. At most
/// `MAX_DIRECTORY_CELLS`, online cells first.
pub fn directory_updates(inner: &crate::state::Inner, only: Option<ClientId>) -> Vec<(ClientId, Vec<u8>)> {
    let entries = directory_entries(inner);
    let recipients: Vec<ClientId> = inner
        .carriers
        .announcements()
        .map(|(id, _)| id)
        .filter(|id| only.is_none_or(|o| o == *id))
        .filter(|id| crate::handover::announced(inner, *id, CAPABILITY_CELL_DIRECTORY))
        .collect();
    recipients
        .into_iter()
        .map(|to| {
            let mut cells: Vec<&DirectoryEntry> = entries.iter().filter(|e| e.client != Some(to)).collect();
            // Online first (stable, so still sorted by station within each).
            cells.sort_by_key(|e| e.client.is_none());
            let cells: Vec<serde_json::Value> = cells
                .into_iter()
                .take(MAX_DIRECTORY_CELLS)
                .map(|e| {
                    let mut v = serde_json::to_value(&e.cell).unwrap_or_default();
                    if let Some(o) = v.as_object_mut() {
                        o.insert("id".into(), cell_name(&e.station, e.cell.cell).into());
                        o.insert("station".into(), e.station.clone().into());
                        o.insert("online".into(), e.client.is_some().into());
                        if let Some(at) = e.last_seen_ms { o.insert("last_seen_ms".into(), at.into()); }
                    }
                    v
                })
                .collect();
            let json = serde_json::json!({ "cells": cells }).to_string();
            (to, crate::protocol::build_service(crate::protocol::SERVICE_CELL_DIRECTORY, &json))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::{self, ConnVersion}, state::Client};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn key(carrier: u16) -> CarrierKey {
        CarrierKey { mcc: 232, mnc: 5, carrier, carrier_ext: 0x10c }
    }

    fn info(carriers: &[u16]) -> BtsInfo {
        BtsInfo { cells: carriers.iter().enumerate().map(|(i, c)| Cell { key: key(*c), location_area: 1, cell: i as u8, ..Default::default() }).collect(), capabilities: Vec::new() }
    }

    #[test]
    fn register_and_look_up() {
        let mut reg = CarrierRegistry::default();
        let (bs1, bs2) = (Uuid::new_v4(), Uuid::new_v4());
        assert!(reg.announce(bs1, info(&[1528])).is_empty());
        assert!(reg.announce(bs2, info(&[1530])).is_empty());
        assert_eq!(reg.owner(&key(1528)), Some(bs1));
        assert_eq!(reg.owner(&key(1530)), Some(bs2));
        assert_eq!(reg.owner(&key(1532)), None);
        assert_eq!(reg.owner(&CarrierKey { carrier_ext: 0, ..key(1528) }), None, "another band plan is another cell");
        assert_eq!(reg.owner(&CarrierKey { mnc: 6, ..key(1528) }), None, "another network is another cell");
        assert!(reg.duplicates().is_empty());
    }

    #[test]
    fn one_connection_serves_several_cells() {
        let mut reg = CarrierRegistry::default();
        let site = Uuid::new_v4();
        reg.announce(site, info(&[1528, 1530]));
        assert_eq!(reg.owner(&key(1528)), Some(site));
        assert_eq!(reg.owner(&key(1530)), Some(site));
    }

    #[test]
    fn disconnect_removes_the_carriers() {
        let mut reg = CarrierRegistry::default();
        let (bs1, bs2) = (Uuid::new_v4(), Uuid::new_v4());
        reg.announce(bs1, info(&[1528]));
        reg.announce(bs2, info(&[1530]));
        assert!(reg.remove(bs1).is_empty());
        assert_eq!(reg.owner(&key(1528)), None);
        assert_eq!(reg.owner(&key(1530)), Some(bs2));
        assert!(reg.info(bs1).is_none());
        assert!(reg.remove(bs1).is_empty(), "removing twice is harmless");
    }

    #[test]
    fn new_announcement_replaces_the_old_one() {
        let mut reg = CarrierRegistry::default();
        let bs = Uuid::new_v4();
        reg.announce(bs, info(&[1528]));
        reg.announce(bs, info(&[1532]));
        assert_eq!(reg.owner(&key(1528)), None, "the old carrier is released");
        assert_eq!(reg.owner(&key(1532)), Some(bs));
        assert!(reg.announce(bs, info(&[1532])).is_empty(), "announcing the same again is no clash with itself");
    }

    #[test]
    fn duplicate_goes_to_the_newest_and_falls_back() {
        let mut reg = CarrierRegistry::default();
        let (bs1, bs2) = (Uuid::new_v4(), Uuid::new_v4());
        reg.announce(bs1, info(&[1528]));
        assert_eq!(reg.announce(bs2, info(&[1528])), vec![(key(1528), bs1)]);
        assert_eq!(reg.owner(&key(1528)), Some(bs2));
        assert!(reg.is_duplicate(&key(1528)));
        assert_eq!(reg.duplicates(), vec![key(1528)]);
        // The newer one goes away: the older claimant serves the carrier again.
        assert_eq!(reg.remove(bs2), vec![(key(1528), bs1)]);
        assert_eq!(reg.owner(&key(1528)), Some(bs1));
        assert!(reg.duplicates().is_empty());
    }

    #[test]
    fn duplicate_resolved_by_reannouncing_another_carrier() {
        let mut reg = CarrierRegistry::default();
        let (bs1, bs2) = (Uuid::new_v4(), Uuid::new_v4());
        reg.announce(bs1, info(&[1528]));
        reg.announce(bs2, info(&[1528]));
        assert!(reg.announce(bs2, info(&[1530])).is_empty());
        assert_eq!(reg.owner(&key(1528)), Some(bs1), "the carrier goes back to the remaining claimant");
        assert_eq!(reg.owner(&key(1530)), Some(bs2));
        assert!(reg.duplicates().is_empty());
    }

    #[test]
    fn reconnect_before_the_old_connection_is_gone() {
        let mut reg = CarrierRegistry::default();
        let (old, new) = (Uuid::new_v4(), Uuid::new_v4());
        reg.announce(old, info(&[1528]));
        reg.announce(new, info(&[1528]));
        assert_eq!(reg.owner(&key(1528)), Some(new));
        // The dead connection times out later: it must not take the carrier along.
        assert!(reg.remove(old).is_empty());
        assert_eq!(reg.owner(&key(1528)), Some(new));
        assert!(!reg.is_duplicate(&key(1528)));
    }

    #[test]
    fn reconnect_after_the_old_connection_is_gone() {
        let mut reg = CarrierRegistry::default();
        let (old, new) = (Uuid::new_v4(), Uuid::new_v4());
        reg.announce(old, info(&[1528]));
        reg.remove(old);
        assert!(reg.announce(new, info(&[1528])).is_empty());
        assert_eq!(reg.owner(&key(1528)), Some(new));
    }

    #[test]
    fn radio_cell_follows_reports_and_announcements() {
        let mut reg = CarrierRegistry::default();
        let (site, other) = (Uuid::new_v4(), Uuid::new_v4());
        reg.announce(site, info(&[1528, 1530]));
        reg.announce(other, info(&[1532]));
        assert_eq!(reg.set_ms_cell(7, site, 1530, 0x10c).map(|c| c.cell), Some(1));
        assert_eq!(reg.ms_cell(7, site).map(|c| c.key.carrier), Some(1530));
        assert!(reg.ms_cell(7, other).is_none(), "only the connection the radio is on counts");
        assert!(reg.set_ms_cell(7, site, 1532, 0x10c).is_none(), "not a cell of this station");
        assert!(reg.set_ms_cell(7, site, 1530, 0).is_none(), "another band plan");
        assert_eq!(reg.ms_cell(7, site).map(|c| c.key.carrier), Some(1530), "a bad report keeps the last one");
        reg.set_ms_cell(7, site, 1528, 0x10c);
        assert_eq!(reg.ms_cell(7, site).map(|c| c.cell), Some(0), "moved within the station");
        reg.announce(site, info(&[1530]));
        assert!(reg.ms_cell(7, site).is_none(), "cell no longer announced");
        reg.set_ms_cell(8, site, 1530, 0x10c);
        reg.remove(site);
        assert!(reg.ms_cell(8, site).is_none());
        assert!(reg.ms_cells.is_empty(), "forgotten with the connection");
    }

    #[test]
    fn parses_a_full_announcement() {
        let json = r#"{"mcc":232,"mnc":5,"cells":[{"carrier":1528,"carrier_ext":268,"location_area":10},{"carrier":1530,"carrier_ext":268,"location_area":11,"cell":3}],"capabilities":["handover-v1"]}"#;
        let info = parse_bts_info(json).unwrap();
        assert_eq!(info.cells, vec![
            Cell { key: key(1528), location_area: 10, cell: 0, ..Default::default() },
            Cell { key: key(1530), location_area: 11, cell: 3, ..Default::default() },
        ]);
        assert_eq!(info.capabilities, vec!["handover-v1".to_string()]);
        // Capabilities may be left out; unknown fields are ignored.
        let info = parse_bts_info(r#"{"mcc":1,"mnc":2,"cells":[],"future":true}"#).unwrap();
        assert!(info.cells.is_empty() && info.capabilities.is_empty());
    }

    #[test]
    fn names_and_frequencies() {
        assert_eq!(cell_name("2320997", 3), "2320997/3");
        let id = Uuid::parse_str("f2a462e4-0000-0000-0000-000000000000").unwrap();
        assert_eq!(station_name(id, Some("1000001")), "1000001");
        assert_eq!(station_name(id, None), "f2a462e4", "without [auth] the connection id stands in");
        // Band 4, offset 0, duplex 4: 400 MHz + 1528 x 25 kHz.
        assert_eq!(CarrierKey { mcc: 232, mnc: 5, carrier: 1528, carrier_ext: 264 }.downlink_hz(), 438_200_000);
        // Offsets: +6.25, -6.25, +12.5 kHz.
        assert_eq!(CarrierKey { mcc: 0, mnc: 0, carrier: 0, carrier_ext: (4 << 6) | (1 << 4) }.downlink_hz(), 400_006_250);
        assert_eq!(CarrierKey { mcc: 0, mnc: 0, carrier: 0, carrier_ext: (4 << 6) | (2 << 4) }.downlink_hz(), 399_993_750);
        assert_eq!(CarrierKey { mcc: 0, mnc: 0, carrier: 0, carrier_ext: (4 << 6) | (3 << 4) }.downlink_hz(), 400_012_500);
    }

    #[test]
    fn rejects_bad_announcements() {
        let cell = |c: &str| format!(r#"{{"mcc":232,"mnc":5,"cells":[{c}]}}"#);
        for bad in [
            "not json".to_string(),
            r#"{"mnc":5,"cells":[]}"#.to_string(),
            r#"{"mcc":1024,"mnc":5,"cells":[]}"#.to_string(),
            r#"{"mcc":232,"mnc":16384,"cells":[]}"#.to_string(),
            cell(r#"{"carrier":4096,"carrier_ext":0,"location_area":1}"#),
            cell(r#"{"carrier":1528,"carrier_ext":1024,"location_area":1}"#),
            cell(r#"{"carrier":1528,"carrier_ext":0,"location_area":16384}"#),
            cell(r#"{"carrier":1528,"location_area":1}"#),
            cell(r#"{"carrier":1528,"carrier_ext":0,"location_area":1},{"carrier":1528,"carrier_ext":0,"location_area":2}"#),
            cell(&vec![r#"{"carrier":1,"carrier_ext":0,"location_area":1}"#; MAX_CELLS + 1].join(",")),
            cell(r#"{"carrier":1528,"carrier_ext":0,"location_area":1,"cell":1},{"carrier":1530,"carrier_ext":0,"location_area":1}"#),
            cell(r#"{"carrier":1528,"carrier_ext":0,"location_area":1,"cell":256}"#),
            r#"{"mcc":232,"mnc":5,"cells":[],"capabilities":[""]}"#.to_string(),
            format!(r#"{{"mcc":232,"mnc":5,"cells":[],"capabilities":["{}"]}}"#, "x".repeat(MAX_CAPABILITY_LEN + 1)),
        ] {
            assert!(parse_bts_info(&bad).is_err(), "accepted {bad}");
        }
    }

    async fn connect(state: &AppState, mode: ClientMode) -> ClientId {
        let (tx, rx) = mpsc::unbounded_channel();
        std::mem::forget(rx);
        let id = Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None });
        id
    }

    fn bts_info_packet(carrier: u16) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_BTS_INFO,
            &format!(r#"{{"mcc":232,"mnc":5,"cells":[{{"carrier":{carrier},"carrier_ext":268,"location_area":1}}]}}"#))
    }

    #[tokio::test]
    async fn basestation_announcement_reaches_the_registry_and_leaves_with_it() {
        let state = AppState::for_test();
        let bs = connect(&state, ClientMode::Basestation).await;
        crate::router::handle_packet(state.clone(), bs, bts_info_packet(1528)).await;
        assert_eq!(state.inner.read().await.carriers.owner(&key(1528)), Some(bs));
        state.cleanup_client(bs).await;
        assert_eq!(state.inner.read().await.carriers.owner(&key(1528)), None);
    }

    #[tokio::test]
    async fn only_basestations_serve_carriers() {
        let state = AppState::for_test();
        for mode in [ClientMode::Terminal, ClientMode::Peer] {
            let id = connect(&state, mode).await;
            crate::router::handle_packet(state.clone(), id, bts_info_packet(1528)).await;
        }
        assert_eq!(state.inner.read().await.carriers.owner(&key(1528)), None);
    }

    #[tokio::test]
    async fn ms_cell_report_reaches_the_registry() {
        let state = AppState::for_test();
        let bs = connect(&state, ClientMode::Basestation).await;
        crate::router::handle_packet(state.clone(), bs, bts_info_packet(1528)).await;
        let report = |carrier: u16| protocol::build_service(protocol::SERVICE_MS_CELL, &format!(r#"{{"issi":7,"carrier":{carrier},"carrier_ext":268}}"#));
        crate::router::handle_packet(state.clone(), bs, report(1528)).await;
        assert_eq!(state.inner.read().await.carriers.ms_cell(7, bs).map(|c| c.key.carrier), Some(1528));
        // A Terminal cannot place radios, and garbage changes nothing.
        let term = connect(&state, ClientMode::Terminal).await;
        crate::router::handle_packet(state.clone(), term, report(1528)).await;
        crate::router::handle_packet(state.clone(), bs, protocol::build_service(protocol::SERVICE_MS_CELL, r#"{"issi":7}"#)).await;
        let inner = state.inner.read().await;
        assert!(inner.carriers.ms_cell(7, term).is_none());
        assert_eq!(inner.carriers.ms_cell(7, bs).map(|c| c.key.carrier), Some(1528));
    }

    #[test]
    fn extensions_keep_bts_info_and_add_the_new_ones() {
        let offered: Vec<&str> = EXTENSIONS.split(',').map(str::trim).collect();
        assert_eq!(offered, ["bts-info", crate::handover::CAPABILITY, "ms-cell", crate::handover::CAPABILITY_MS_MOVED, "ms-reselection", crate::call_restore::CAPABILITY, crate::call_restore::CAPABILITY_HANDOFF, CAPABILITY_CELL_DIRECTORY, crate::group_access::CAPABILITY, crate::netlink::EXT_ISSI_CHECK]);
    }

    #[test]
    fn parses_what_a_cell_broadcasts() {
        let info = parse_bts_info(r#"{"mcc":232,"mnc":5,"cells":[{"carrier":1528,"carrier_ext":264,"location_area":2,
            "ms_txpwr_max_dbm":30,"rxlev_access_min_dbm":-100,
            "service_details":{"registration":true,"voice_service":true,"migration":false,"future_flag":true},
            "neighbours":[{"carrier":1530,"carrier_ext":264,"location_area":1},{"mcc":901,"mnc":9,"carrier":1600,"carrier_ext":264}]}]}"#).unwrap();
        let extra = &info.cells[0].extra;
        assert_eq!(extra.ms_txpwr_max_dbm, Some(30));
        assert_eq!(extra.rxlev_access_min_dbm, Some(-100));
        let details = extra.service_details.as_ref().unwrap();
        assert_eq!(details.len(), 3, "unknown flags are left out");
        assert_eq!(details["registration"], true);
        assert_eq!(details["migration"], false);
        assert_eq!(extra.neighbours.as_deref(), Some(&[
            NeighbourRef { mcc: 232, mnc: 5, carrier: 1530, carrier_ext: 264, location_area: Some(1) },
            NeighbourRef { mcc: 901, mnc: 9, carrier: 1600, carrier_ext: 264, location_area: None },
        ][..]), "the network defaults to the announcer's own");
        // Without the optional fields nothing is reported.
        let plain = parse_bts_info(r#"{"mcc":232,"mnc":5,"cells":[{"carrier":1528,"carrier_ext":264,"location_area":2}]}"#).unwrap();
        assert_eq!(plain.cells[0].extra, CellExtra::default());
        assert_eq!(serde_json::to_value(&plain.cells[0]).unwrap(),
            serde_json::json!({"mcc":232,"mnc":5,"carrier":1528,"carrier_ext":264,"location_area":2,"cell":0}));
    }

    #[test]
    fn a_bad_optional_field_is_dropped_and_the_cell_kept() {
        for bad in [
            r#""ms_txpwr_max_dbm":32"#,
            r#""ms_txpwr_max_dbm":50"#,
            r#""rxlev_access_min_dbm":-130"#,
            r#""rxlev_access_min_dbm":-77"#,
            r#""service_details":{"registration":1}"#,
            r#""service_details":[]"#,
            r#""neighbours":[{"carrier":1530}]"#,
            r#""neighbours":[{"carrier":5000,"carrier_ext":264}]"#,
            r#""neighbours":{}"#,
        ] {
            let json = format!(r#"{{"mcc":232,"mnc":5,"cells":[{{"carrier":1528,"carrier_ext":264,"location_area":2,{bad}}}]}}"#);
            let info = parse_bts_info(&json).unwrap_or_else(|e| panic!("{bad}: {e}"));
            assert_eq!(info.cells[0].extra, CellExtra::default(), "{bad}");
            assert_eq!(info.cells[0].key.carrier, 1528);
        }
        let eight = vec![r#"{"carrier":1,"carrier_ext":264}"#; MAX_NEIGHBOURS + 1].join(",");
        let info = parse_bts_info(&format!(r#"{{"mcc":232,"mnc":5,"cells":[{{"carrier":1528,"carrier_ext":264,"location_area":2,"neighbours":[{eight}]}}]}}"#)).unwrap();
        assert_eq!(info.cells[0].extra.neighbours, None, "a cell broadcasts at most 7 neighbours (table 18.71)");
    }

    #[test]
    fn the_directory_keeps_stations_with_a_login_as_offline() {
        let mut reg = CarrierRegistry::default();
        let (bs1, bs2, anon) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        reg.set_login(bs1, "1000001");
        reg.set_login(bs2, "1000002");
        reg.announce(bs1, info(&[1528]));
        reg.announce(bs2, info(&[1530, 1532]));
        reg.announce(anon, info(&[1534]));
        let name = |id: ClientId| if id == bs1 { "1000001".to_string() } else if id == bs2 { "1000002".to_string() } else { "anon".to_string() };
        let dir = reg.directory(name);
        assert_eq!(dir.iter().map(|e| (e.station.as_str(), e.cell.key.carrier, e.client.is_some())).collect::<Vec<_>>(),
            [("1000001", 1528, true), ("1000002", 1530, true), ("1000002", 1532, true), ("anon", 1534, true)]);
        reg.remove(bs2);
        reg.remove(anon);
        let dir = reg.directory(name);
        assert_eq!(dir.iter().map(|e| (e.station.as_str(), e.cell.key.carrier, e.client.is_some(), e.last_seen_ms.is_some())).collect::<Vec<_>>(),
            [("1000001", 1528, true, false), ("1000002", 1530, false, true), ("1000002", 1532, false, true)],
            "a station without a login has no stable name, so it is not kept");
        assert_eq!(reg.owner(&key(1530)), None, "offline cells serve nothing");
        // Back online under its login: the offline entry goes.
        let bs2b = Uuid::new_v4();
        reg.set_login(bs2b, "1000002");
        reg.announce(bs2b, info(&[1530]));
        let dir = reg.directory(|id| if id == bs1 { "1000001".into() } else { "1000002".into() });
        assert_eq!(dir.iter().map(|e| (e.cell.key.carrier, e.client.is_some())).collect::<Vec<_>>(), [(1528, true), (1530, true)]);
    }

    #[test]
    fn a_target_is_found_by_its_downlink_when_the_duplex_bits_differ() {
        let mut reg = CarrierRegistry::default();
        let bs2 = Uuid::new_v4();
        // BS2 announces duplex spacing 7 (ext 270); BS1 lists it without an
        // extension, so its request carries BS1's own band plan (264).
        let k = |ext| CarrierKey { mcc: 232, mnc: 5, carrier: 1530, carrier_ext: ext };
        reg.announce(bs2, BtsInfo { cells: vec![Cell { key: k(270), location_area: 1, cell: 0, ..Default::default() }], capabilities: vec![] });
        assert_eq!(reg.owner(&k(264)), None);
        assert_eq!(reg.find_target(&k(264), None), Some(bs2));
        assert_eq!(reg.find_target(&k(264), Some(1)), Some(bs2));
        assert_eq!(reg.find_target(&k(264), Some(2)), None, "other LA");
        assert_eq!(reg.find_target(&k(264 | 0x10), None), None, "other offset = other frequency");
        assert_eq!(reg.find_target(&CarrierKey { carrier: 1531, ..k(270) }, None), None);
    }

    #[test]
    fn the_offline_list_forgets_the_oldest_station() {
        let mut reg = CarrierRegistry::default();
        for n in 0..=MAX_OFFLINE_STATIONS {
            let id = Uuid::new_v4();
            reg.set_login(id, &format!("s{n}"));
            reg.announce(id, info(&[n as u16]));
            reg.remove(id);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let dir = reg.directory(|_| String::new());
        assert_eq!(dir.len(), MAX_OFFLINE_STATIONS);
        assert!(!dir.iter().any(|e| e.station == "s0"), "the oldest went");
    }

    async fn connect_named(state: &AppState, login: &str) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode: ClientMode::Basestation, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: Some(login.into()) });
        (id, rx)
    }

    fn announce_packet(carrier: u16, capabilities: &[&str]) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_BTS_INFO, &serde_json::json!({
            "mcc": 232, "mnc": 5, "capabilities": capabilities,
            "cells": [{"carrier": carrier, "carrier_ext": 268, "location_area": 1, "rxlev_access_min_dbm": -100,
                "neighbours": [{"carrier": 1530, "carrier_ext": 268}]}],
        }).to_string())
    }

    /// The `SERVICE_CELL_DIRECTORY` messages in `rx`, as (id, online) per cell.
    fn directories(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<Vec<(String, bool)>> {
        let mut out = Vec::new();
        while let Ok(packet) = rx.try_recv() {
            if packet.len() > 2 && packet[0] == protocol::CLASS_SERVICE && packet[1] == protocol::SERVICE_CELL_DIRECTORY {
                let json: serde_json::Value = serde_json::from_slice(&packet[2..packet.len() - 1]).unwrap();
                out.push(json["cells"].as_array().unwrap().iter()
                    .map(|c| (c["id"].as_str().unwrap().to_string(), c["online"].as_bool().unwrap())).collect());
            }
        }
        out
    }

    #[tokio::test]
    async fn ten_stations_each_see_the_nine_others() {
        let state = AppState::for_test();
        let mut stations = Vec::new();
        for n in 0..10u16 {
            let (id, rx) = connect_named(&state, &format!("100000{n}")).await;
            crate::router::handle_packet(state.clone(), id, announce_packet(1520 + 2 * n, &[CAPABILITY_CELL_DIRECTORY])).await;
            stations.push((id, rx));
        }
        for (n, (_, rx)) in stations.iter_mut().enumerate() {
            let last = directories(rx).pop().unwrap();
            assert_eq!(last.len(), 9, "station {n}");
            assert!(last.iter().all(|(id, online)| *online && *id != format!("100000{n}/0")));
        }
        state.cleanup_client(stations[4].0).await;
        for (n, (_, rx)) in stations.iter_mut().enumerate().filter(|(n, _)| *n != 4) {
            let last = directories(rx).pop().unwrap();
            assert_eq!(last.len(), 9, "station {n}");
            assert_eq!(last.last(), Some(&("1000004/0".to_string(), false)), "offline cells come last");
        }
    }

    #[tokio::test]
    async fn stations_that_ask_get_the_other_stations_cells() {
        let state = AppState::for_test();
        let (bs1, mut rx1) = connect_named(&state, "1000001").await;
        let (bs2, mut rx2) = connect_named(&state, "1000002").await;
        let (old, mut rx_old) = connect_named(&state, "1000003").await;
        crate::router::handle_packet(state.clone(), bs1, announce_packet(1528, &[CAPABILITY_CELL_DIRECTORY])).await;
        assert_eq!(directories(&mut rx1), vec![vec![]], "the announcer gets the list at once, without its own cells");
        crate::router::handle_packet(state.clone(), old, announce_packet(1532, &[])).await;
        assert_eq!(directories(&mut rx_old), Vec::<Vec<(String, bool)>>::new(), "nothing for a station that did not ask");
        assert_eq!(directories(&mut rx1), vec![vec![("1000003/0".to_string(), true)]], "the others hear of the change");
        crate::router::handle_packet(state.clone(), bs2, announce_packet(1530, &[CAPABILITY_CELL_DIRECTORY])).await;
        assert_eq!(directories(&mut rx2), vec![vec![("1000001/0".to_string(), true), ("1000003/0".to_string(), true)]]);
        assert_eq!(directories(&mut rx1).len(), 1);
        // The same announcement again changes nothing for the others.
        crate::router::handle_packet(state.clone(), bs2, announce_packet(1530, &[CAPABILITY_CELL_DIRECTORY])).await;
        assert_eq!(directories(&mut rx2).len(), 1);
        assert!(directories(&mut rx1).is_empty());
        // A station that goes stays in the list as offline.
        state.cleanup_client(old).await;
        assert_eq!(directories(&mut rx1), vec![vec![("1000002/0".to_string(), true), ("1000003/0".to_string(), false)]]);
        // What the cell broadcasts is passed on.
        let (bs4, mut rx4) = connect_named(&state, "1000004").await;
        crate::router::handle_packet(state.clone(), bs4, announce_packet(1534, &[CAPABILITY_CELL_DIRECTORY])).await;
        let packet = std::iter::from_fn(|| rx4.try_recv().ok()).last().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&packet[2..packet.len() - 1]).unwrap();
        let first = &json["cells"][0];
        assert_eq!(first["id"], "1000001/0");
        assert_eq!(first["station"], "1000001");
        assert_eq!(first["rxlev_access_min_dbm"], -100);
        assert_eq!(first["neighbours"], serde_json::json!([{"mcc":232,"mnc":5,"carrier":1530,"carrier_ext":268}]));
        assert_eq!(json["cells"][2]["online"], false);
        assert!(json["cells"][2]["last_seen_ms"].as_u64().is_some());
    }

    #[tokio::test]
    async fn malformed_announcement_keeps_the_previous_one() {
        let state = AppState::for_test();
        let bs = connect(&state, ClientMode::Basestation).await;
        crate::router::handle_packet(state.clone(), bs, bts_info_packet(1528)).await;
        crate::router::handle_packet(state.clone(), bs, protocol::build_service(protocol::SERVICE_BTS_INFO, r#"{"mcc":232}"#)).await;
        assert_eq!(state.inner.read().await.carriers.owner(&key(1528)), Some(bs));
    }
}
