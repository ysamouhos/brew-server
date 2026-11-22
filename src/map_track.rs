//! The map's radio layer: the last positions per radio (`[map] track_points`,
//! kept across a restart in `<storage>.positions`) and where a radio is drawn.
//!
//! A radio with a fresh position is drawn there. A radio without one is
//! *docked*: drawn next to the station it is registered on, so the map shows
//! which radio is on which station. It docks when it has no position, when its
//! last position is older than `[map] dock_after_min`, or when it registered on
//! another station after that position (the position is from the old place).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::state::{AppState, ClientMode};

/// The store is written at most this often.
const SAVE_EVERY_MS: u64 = 5_000;
/// Radios not positioned for this long are dropped from the store.
const KEEP_MS: u64 = 30 * 24 * 3_600_000;
/// At most this many radios are kept.
const MAX_RADIOS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackFix {
    pub lat: f64,
    pub lon: f64,
    pub at_ms: u64,
    /// Map id of the station the radio was on when the fix came (see
    /// [`dock_station`]), so a later registration elsewhere docks it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    /// Sent by a terminal (see [`terminal_issis`]): an app often registers
    /// only to send its position and leaves again, so the map keeps drawing
    /// it as a terminal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terminal: bool,
}

/// The same position again within this time is one report seen twice.
const SAME_FIX_MS: u64 = 15_000;

pub struct Tracks {
    path: Option<PathBuf>,
    data: BTreeMap<u32, VecDeque<TrackFix>>,
    points: usize,
    dirty: bool,
    saved_ms: u64,
}

impl Tracks {
    /// File `<storage>.positions`; `None` keeps everything in memory.
    pub fn load(storage: Option<&Path>, points: usize) -> Self {
        let path = storage.map(|p| {
            let mut s = p.as_os_str().to_owned();
            s.push(".positions");
            PathBuf::from(s)
        });
        let data = path.as_deref().and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| tracing::warn!(error = %e, "map positions file unreadable, starting empty")).ok())
            .unwrap_or_default();
        let mut t = Self { path, data, points, dirty: false, saved_ms: 0 };
        t.set_points(points);
        t.dirty = false;
        t
    }

    /// Adds a fix. The same time again (a resend) is kept once, and so is the
    /// same position within `SAME_FIX_MS` (one report seen twice: decoded here
    /// from the Brew channel and reported by the station over telemetry). An
    /// older fix goes into its place in the track. Returns whether it changed.
    pub fn record(&mut self, issi: u32, fix: TrackFix, now_ms: u64) -> bool {
        if self.points == 0 || !crate::fedroute::valid_position(fix.lat, fix.lon) { return false; }
        let t = self.data.entry(issi).or_default();
        if t.iter().any(|f| f.at_ms == fix.at_ms) { return false; }
        let same = |f: &TrackFix| (f.lat - fix.lat).abs() < 1e-6 && (f.lon - fix.lon).abs() < 1e-6 && f.at_ms.abs_diff(fix.at_ms) < SAME_FIX_MS;
        if t.iter().any(same) { return false; }
        let at = fix.at_ms;
        let i = t.iter().position(|f| f.at_ms > at).unwrap_or(t.len());
        t.insert(i, fix);
        while t.len() > self.points { t.pop_front(); }
        if !t.iter().any(|f| f.at_ms == at) { return false; }
        self.prune(now_ms);
        self.dirty = true;
        true
    }

    pub fn track(&self, issi: u32) -> Vec<TrackFix> {
        self.data.get(&issi).map(|t| t.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn newest(&self, issi: u32) -> Option<&TrackFix> { self.data.get(&issi).and_then(|t| t.back()) }

    pub fn len(&self, issi: u32) -> usize { self.data.get(&issi).map_or(0, |t| t.len()) }

    /// The track length (`[map] track_points`); a shorter one cuts every track.
    pub fn set_points(&mut self, n: usize) {
        self.points = n;
        for t in self.data.values_mut() {
            while t.len() > n { t.pop_front(); self.dirty = true; }
        }
        let before = self.data.len();
        self.data.retain(|_, t| !t.is_empty());
        if self.data.len() != before { self.dirty = true; }
    }

    /// Admin: forget every track.
    pub fn clear(&mut self) {
        self.data.clear();
        self.dirty = true;
    }

    fn prune(&mut self, now_ms: u64) {
        self.data.retain(|_, t| t.back().is_some_and(|f| now_ms.saturating_sub(f.at_ms) < KEEP_MS));
        if self.data.len() > MAX_RADIOS {
            let mut by_age: Vec<(u64, u32)> = self.data.iter().map(|(i, t)| (t.back().map_or(0, |f| f.at_ms), *i)).collect();
            by_age.sort_unstable();
            for (_, issi) in by_age.iter().take(self.data.len() - MAX_RADIOS) { self.data.remove(issi); }
        }
    }

    /// Writes the file when something changed, at most every few seconds unless `force`.
    pub fn flush(&mut self, now_ms: u64, force: bool) {
        if !self.dirty || (!force && now_ms.saturating_sub(self.saved_ms) < SAVE_EVERY_MS) { return; }
        self.dirty = false;
        self.saved_ms = now_ms;
        let Some(path) = &self.path else { return };
        let Ok(bytes) = serde_json::to_vec(&self.data) else { return };
        let tmp = path.with_extension("positions.tmp");
        if let Err(e) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
            tracing::warn!(error = %e, path = %path.display(), "cannot write the map positions file");
        }
    }
}

/// Writes pending track changes every few seconds.
pub async fn run(state: Arc<AppState>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(SAVE_EVERY_MS));
    loop {
        tick.tick().await;
        if let Ok(mut t) = state.tracks.lock() { t.flush(crate::telemetry::now_ms(), false); }
    }
}

/// The map id (`/api/bts-locations` `id`) of the station each radio is
/// registered on, for the radios registered here whose station has a
/// position: a station that reports its position over telemetry by its
/// telemetry registrations, else the Brew login with a static position.
pub async fn dock_stations(state: &AppState) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    {
        let inner = state.inner.read().await;
        let located: std::collections::HashSet<String> = state.locations.read().map(|l| {
            l.iter().filter(|(_, v)| crate::fedroute::valid_position(v.lat, v.lon)).map(|(k, _)| k.clone()).collect()
        }).unwrap_or_default();
        for (&issi, s) in inner.subscribers.iter().filter(|(_, s)| s.route.path.is_empty()) {
            let Some(c) = inner.clients.get(&s.client_id).filter(|c| c.mode != ClientMode::Peer) else { continue };
            if let Some(u) = c.username.as_ref().filter(|u| located.contains(*u)) { out.insert(issi, u.clone()); }
        }
    }
    // Telemetry wins: it is the position the station reports itself.
    for st in state.telemetry.read().await.stations.values() {
        if st.site_location.is_none_or(|(la, lo)| !crate::fedroute::valid_position(la, lo)) { continue; }
        for issi in &st.registrations { out.insert(*issi, st.id.clone()); }
    }
    out
}

/// The ISSIs registered here as terminals: over a Terminal connection, unless
/// the login or the number is set to "bridge" in `[issi_kinds]` (a bridge logs
/// in as Terminal for the radios behind it), or a number set to "terminal".
pub async fn terminal_issis(state: &AppState) -> std::collections::HashSet<u32> {
    let kinds = state.issi_kinds.read().map(|k| k.clone()).unwrap_or_default();
    let kind = |n: &str| kinds.get(n).map(String::as_str);
    let inner = state.inner.read().await;
    inner.subscribers.iter().filter(|(_, s)| s.route.path.is_empty()).filter_map(|(&issi, s)| {
        let own = kind(&issi.to_string());
        if own == Some("terminal") { return Some(issi); }
        if own == Some("bridge") || s.mode != ClientMode::Terminal { return None; }
        let login = inner.clients.get(&s.client_id).and_then(|c| c.username.as_deref());
        (login.and_then(kind) != Some("bridge")).then_some(issi)
    }).collect()
}

/// Notes a position this server decoded for `issi` (Brew SDS, a station's
/// telemetry), with the station the radio is on now.
pub async fn record_local(state: &Arc<AppState>, issi: u32, lat: f64, lon: f64, at_ms: u64) {
    let mut docks = dock_stations(state).await;
    // A visitor's (V)ASSI: the track goes under its home ISSI.
    let station = docks.remove(&issi);
    let issi = visitor_homes(state).await.get(&issi).copied().unwrap_or(issi);
    let terminal = terminal_issis(state).await.contains(&issi);
    record(state, issi, TrackFix { lat, lon, at_ms, station, terminal });
}

/// (V)ASSI -> home ISSI of the visitors the stations reported (`MsVisitor`).
pub async fn visitor_homes(state: &AppState) -> HashMap<u32, u32> {
    let t = state.telemetry.read().await;
    t.stations.values().flat_map(|b| b.visitor_home.iter().map(|(v, h)| (*v, *h))).filter(|(v, h)| v != h).collect()
}

pub fn record(state: &AppState, issi: u32, fix: TrackFix) {
    if let Ok(mut t) = state.tracks.lock() {
        t.record(issi, fix, crate::telemetry::now_ms());
    }
}

/// Whether a radio is drawn next to its station: it has one (`dock`), and it
/// has no position, the position is older than `dock_after_min` (0 = never by
/// age), or it came while the radio was on another station.
pub fn docked(dock: Option<&str>, fix: Option<&crate::telemetry::PositionFix>, fix_station: Option<&str>, dock_after_min: u32, now_ms: u64) -> bool {
    let Some(dock) = dock else { return false };
    let Some(fix) = fix else { return true };
    if dock_after_min > 0 && now_ms.saturating_sub(fix.at_ms) > dock_after_min as u64 * 60_000 { return true; }
    fix_station.is_some_and(|s| s != dock)
}

#[derive(Serialize)]
pub struct RadioOut {
    pub issi: u32,
    /// Map id of the station the radio is registered on (with a position).
    pub dock: Option<String>,
    /// Drawn next to `dock` instead of at `fix`.
    pub docked: bool,
    pub fix: Option<crate::telemetry::PositionFix>,
    /// Positions in the track.
    pub points: usize,
    pub emergency: bool,
    /// Registered here over a Terminal connection (app, console), drawn with
    /// the terminal marker instead of the radio.
    pub terminal: bool,
    /// Registered somewhere now (here, on a station that reports it, or on
    /// a linked server); otherwise drawn as offline.
    pub online: bool,
    /// The visitor (V)ASSI the radio is known by here, when the position
    /// shown or its registration came under it; the radio is drawn under
    /// its home ISSI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vassi: Option<u32>,
}

#[derive(Serialize)]
pub struct MapSnapshot {
    pub settings: crate::config::MapConfig,
    pub now_ms: u64,
    pub radios: Vec<RadioOut>,
}

/// `GET /api/map`: every radio for the map, positioned or docked.
pub async fn api_snapshot(axum::extract::State(state): axum::extract::State<Arc<AppState>>, headers: axum::http::HeaderMap) -> axum::Json<MapSnapshot> {
    let hide = track_hidden(&state, &headers);
    let settings = state.map_cfg.read().map(|c| *c).unwrap_or_default();
    let now = crate::telemetry::now_ms();
    let homes = visitor_homes(&state).await;
    let home = |i: u32| homes.get(&i).copied().unwrap_or(i);
    // A visitor is drawn under its home ISSI: its (V)ASSI's position, dock and state go there.
    let mut vassi_of: HashMap<u32, u32> = HashMap::new();
    let mut fixes: Vec<crate::telemetry::PositionFix> = Vec::new();
    for mut f in crate::dashboard::collect_positions(&state).await {
        let h = home(f.issi);
        if fixes.iter().any(|g| g.issi == h && g.at_ms >= f.at_ms) { continue; }
        fixes.retain(|g| g.issi != h);
        if h != f.issi { vassi_of.insert(h, f.issi); } else { vassi_of.remove(&h); }
        f.issi = h;
        fixes.push(f);
    }
    let mut docks: HashMap<u32, String> = HashMap::new();
    for (i, d) in dock_stations(&state).await {
        let h = home(i);
        if h != i { vassi_of.entry(h).or_insert(i); }
        if h == i || !docks.contains_key(&h) { docks.insert(h, d); }
    }
    let emergencies: std::collections::HashSet<u32> = crate::emergency::snapshot(&state).await.iter().map(|e| home(e.issi)).collect();
    let terminals: std::collections::HashSet<u32> = terminal_issis(&state).await.into_iter().map(home).collect();
    let (registered, mut online): (std::collections::HashSet<u32>, std::collections::HashSet<u32>) = {
        let inner = state.inner.read().await;
        (inner.subscribers.iter().filter(|(_, s)| s.route.path.is_empty()).map(|(i, _)| *i).collect(), inner.subscribers.keys().copied().collect())
    };
    for st in state.telemetry.read().await.stations.values() { online.extend(st.registrations.iter().copied()); }
    let registered: std::collections::HashSet<u32> = registered.into_iter().map(home).collect();
    let online: std::collections::HashSet<u32> = online.into_iter().map(home).collect();
    let mut by_issi: BTreeMap<u32, Option<crate::telemetry::PositionFix>> = docks.keys().map(|i| (*i, None)).collect();
    for f in fixes { by_issi.insert(f.issi, Some(f)); }
    let t = state.tracks.lock().unwrap_or_else(|e| e.into_inner());
    let radios = by_issi.into_iter().map(|(issi, fix)| {
        let dock = docks.get(&issi).cloned();
        // The track's newest fix is the one the map shows when they agree.
        let shown = t.newest(issi).filter(|n| fix.as_ref().is_some_and(|f| f.at_ms == n.at_ms));
        let fix_station = shown.and_then(|n| n.station.clone());
        // Registered here now: as it is registered. Not registered: as it was when it sent the position shown.
        let terminal = terminals.contains(&issi) || (!registered.contains(&issi) && shown.is_some_and(|n| n.terminal));
        RadioOut {
            issi, docked: docked(dock.as_deref(), fix.as_ref(), fix_station.as_deref(), settings.dock_after_min, now),
            dock, points: if hide { 0 } else { t.len(issi) }, emergency: emergencies.contains(&issi), fix,
            terminal, online: online.contains(&issi), vassi: vassi_of.get(&issi).copied(),
        }
    }).collect();
    axum::Json(MapSnapshot { settings, now_ms: now, radios })
}

/// Tracks are hidden from this request: a visitor not signed in (Viewer,
/// with dashboard users set up) while `[map] guests_see_track` is off.
pub fn track_hidden(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let guests = state.map_cfg.read().map(|c| c.guests_see_track).unwrap_or(true);
    !guests && {
        let a = crate::dashboard::access(state, headers);
        a.auth && a.user.is_none()
    }
}

/// `GET /api/map/track?issi=N`: a radio's positions, oldest first (none for a
/// guest when `guests_see_track` is off).
pub async fn api_track(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> axum::Json<Vec<TrackFix>> {
    if track_hidden(&state, &headers) { return axum::Json(Vec::new()); }
    let issi = q.get("issi").and_then(|v| v.parse::<u32>().ok()).unwrap_or(u32::MAX);
    axum::Json(state.tracks.lock().map(|t| t.track(issi)).unwrap_or_default())
}

/// `POST /api/map/settings` (admins): `[map]`, applied at once and written to
/// the config file without a restart.
pub async fn api_settings(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(req): axum::Json<crate::config::MapConfig>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(e) = req.validate() { return (axum::http::StatusCode::BAD_REQUEST, e).into_response(); }
    if let Ok(mut c) = state.map_cfg.write() { *c = req; }
    if let Ok(mut t) = state.tracks.lock() {
        t.set_points(req.track_points as usize);
        t.flush(crate::telemetry::now_ms(), true);
    }
    tracing::warn!(track_points = req.track_points, dock_after_min = req.dock_after_min, guests_see_track = req.guests_see_track, "map settings changed");
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    match crate::dashboard::save_config(&state, &cfg).await {
        Ok(_) => axum::Json(req).into_response(),
        Err(e) => {
            crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("applied, but writing the config file failed: {e}")).into_response()
        }
    }
}

/// `POST /api/map/clear` (admins): forget every track and every radio's last
/// position.
pub async fn api_clear(axum::extract::State(state): axum::extract::State<Arc<AppState>>) -> axum::http::StatusCode {
    if let Ok(mut t) = state.tracks.lock() {
        t.clear();
        t.flush(crate::telemetry::now_ms(), true);
    }
    // The last positions too, else every radio that ever sent one stays on
    // the map (registered radios dock at their station until they send again).
    state.telemetry.write().await.clear_positions();
    state.inner.write().await.fed.ms_pos.retain(|_, e| e.path.is_empty());
    tracing::warn!("map cleared from the dashboard: tracks and last positions");
    axum::http::StatusCode::NO_CONTENT
}

/// Map markers from the design set v2 (`handover/map-icons/gen2.js`), shared
/// by the map script: type by colour and glyph (station TMO, tower TBS,
/// terminal login, radio), status badge top left (active, warning, error;
/// offline = grey with a dashed ring).
/// `v2icon(type, o, dx, dy)` gives a Leaflet icon whose point sits `dx`/`dy`
/// pixels away from the marker (docking).
pub const MARKERS_JS: &str = r##"<script>
const V2C={tmo:'#1f4fe0',tbs:'#475569',term:'#7b4fc4',ms:'#d9822b',emg:'#e53935',off:'#9aa4b1',act:'#2e9e4f',err:'#e53935',warn:'#f28c18'};
const V2G={
 tmo:'<path d="M12 10.8L8 21.5M12 10.8l4 10.7M6.4 21.5h11.2"/><path d="M10.4 15h3.2M9.2 18.3h5.6M10.4 15l4.4 3.3M13.6 15l-4.4 3.3" stroke-width="1.5"/><circle cx="12" cy="8.6" r="1.7" fill="#fff"/><path d="M8.4 5.6a5 5 0 0 0 0 6M15.6 5.6a5 5 0 0 1 0 6M5.6 3.4a9 9 0 0 0 0 10.4M18.4 3.4a9 9 0 0 1 0 10.4"/>',
 tbs:'<path d="M12 3v18M8 21h8M7.5 6.5h9M7.5 9.5h9M7.5 12.5h9M7.5 15.5h7"/>',
 term:'<path d="M5 13.5v-1.5a7 7 0 0 1 14 0v1.5"/><rect x="3.5" y="13" width="4" height="6.2" rx="1.4"/><rect x="16.5" y="13" width="4" height="6.2" rx="1.4"/><path d="M18.5 19.2c0 1.5-1.6 2.3-4 2.3h-2"/>',
 ms:'<path d="M9 2v4M7 6h10a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z"/><rect x="8.5" y="8.5" width="7" height="4" rx=".5"/><g fill="#fff" stroke="none"><circle cx="9.5" cy="16" r=".9"/><circle cx="12" cy="16" r=".9"/><circle cx="14.5" cy="16" r=".9"/><circle cx="9.5" cy="19" r=".9"/><circle cx="12" cy="19" r=".9"/><circle cx="14.5" cy="19" r=".9"/></g>'};
const V2ST={
 active:'<circle cx="9" cy="9" r="7" fill="#2e9e4f" stroke="#fff" stroke-width="1.8"/><path d="M5.8 9.2l2.2 2.2 4-4.2" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
 error:'<circle cx="9" cy="9" r="7" fill="#e53935" stroke="#fff" stroke-width="1.8"/><path d="M6.4 6.4l5.2 5.2M11.6 6.4l-5.2 5.2" stroke="#fff" stroke-width="2" stroke-linecap="round"/>',
 warning:'<path d="M9 1.6l7.6 13.2H1.4z" fill="#f28c18" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/><path d="M9 6.4v4" stroke="#fff" stroke-width="2" stroke-linecap="round"/><circle cx="9" cy="12.8" r="1.1" fill="#fff"/>'};
// o: px, off, emg, faded, sel, status (active/warning/error).
function v2svg(type,o){o=o||{};const px=o.px||36,bg=o.off?V2C.off:o.emg?V2C.emg:V2C[type];
 const off=o.off?`<circle cx="24" cy="25" r="18.6" fill="none" stroke="${V2C.off}" stroke-width="1.8" stroke-dasharray="3.2 2.6"/>`:'';
 const sel=o.sel?'<circle cx="24" cy="25" r="19.2" fill="none" stroke="#0f1b3d" stroke-width="2.4" stroke-dasharray="4 3"/>':'';
 return `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="0 0 48 48" style="display:block"><g${o.faded?' opacity=".55"':''}>${off}${sel}<circle cx="24" cy="25" r="15.5" fill="${bg}" stroke="#fff" stroke-width="2.4"/><g transform="translate(14 15) scale(.833)" fill="none" stroke="#fff" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round">${V2G[type]}</g>${o.status&&!o.off&&V2ST[o.status]?V2ST[o.status]:''}</g></svg>`;}
function v2icon(type,o,dx,dy){o=o||{};dx=dx||0;dy=dy||0;const px=o.px||36,h=px/2;
 // An emergency pulses around the marker's circle (centre 24,25 of 48, radius ~16.7).
 const pulse=o.emg?`<span class=emg-pin style="position:absolute;left:${(px*.152).toFixed(1)}px;top:${(px*.173).toFixed(1)}px;width:${(px*.696).toFixed(1)}px;height:${(px*.696).toFixed(1)}px;border-radius:50%"></span>`:'';
 return L.divIcon({html:`<div style="position:relative;width:${px}px;height:${px}px;filter:drop-shadow(0 1px 2px rgba(0,0,0,.45))">${pulse}${v2svg(type,o)}</div>`,className:'',iconSize:[px,px],iconAnchor:[h-dx,h-dy],popupAnchor:[dx,dy-h]});}
</script>"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(at: u64, station: Option<&str>) -> TrackFix {
        TrackFix { lat: 48.0 + at as f64 / 1e6, lon: 16.0, at_ms: at, station: station.map(str::to_owned), terminal: false }
    }
    fn pf(at: u64) -> crate::telemetry::PositionFix {
        crate::telemetry::PositionFix { issi: 1, lat: 48.0, lon: 16.0, at_ms: at, bts: "brew-sds".into(), source_text: String::new(), emergency: false }
    }

    #[test]
    fn track_keeps_the_last_n_once_each_in_time_order() {
        let mut t = Tracks::load(None, 30);
        for i in 1..=40u64 { assert!(t.record(7, fix(i * 1000, None), 50_000)); }
        assert_eq!(t.len(7), 30);
        assert_eq!(t.track(7)[0].at_ms, 11_000);
        assert!(!t.record(7, fix(40_000, None), 50_000), "a resend is kept once");
        assert!(!t.record(7, fix(500, None), 50_000), "older than the whole full track");
        assert!(t.record(7, fix(39_500, None), 50_000), "a late fix goes into its place");
        assert_eq!(t.newest(7).unwrap().at_ms, 40_000);
        assert!(!t.record(7, TrackFix { lat: 0.0, lon: 0.0, at_ms: 60_000, station: None, terminal: false }, 60_000), "0/0 is no fix");
    }

    /// Field 2026-11-22: after "Clear all tracks" radios long gone stayed on the map
    /// at their last position.
    #[tokio::test]
    async fn clearing_the_map_forgets_tracks_and_last_positions() {
        let state = AppState::for_test();
        let now = crate::telemetry::now_ms();
        state.telemetry.write().await.record_sds_position(2320972, 48.21, 16.37, now, "lip".into());
        record_local(&state, 2320972, 48.21, 16.37, now).await;
        let snap = |s: Arc<AppState>| async move { api_snapshot(axum::extract::State(s), axum::http::HeaderMap::new()).await.0 };
        assert_eq!(snap(state.clone()).await.radios.len(), 1);
        api_clear(axum::extract::State(state.clone())).await;
        assert!(snap(state.clone()).await.radios.is_empty(), "nothing left on the map");
        assert_eq!(state.tracks.lock().unwrap().len(2320972), 0);
    }

    #[test]
    fn one_report_seen_twice_is_kept_once() {
        let mut t = Tracks::load(None, 30);
        assert!(t.record(1, fix(10_000, None), 10_000));
        assert!(!t.record(1, TrackFix { at_ms: 10_400, ..fix(10_000, None) }, 10_400), "same position 0.4 s later (Brew + telemetry)");
        assert!(t.record(1, TrackFix { at_ms: 40_000, ..fix(10_000, None) }, 40_000), "standing still: a new report later counts");
        assert!(t.record(1, fix(41_000, None), 41_000), "moved");
        assert_eq!(t.len(1), 3);
    }

    #[test]
    fn shorter_setting_cuts_and_zero_keeps_nothing() {
        let mut t = Tracks::load(None, 30);
        for i in 1..=10u64 { t.record(7, fix(i, None), 100); }
        t.set_points(3);
        assert_eq!(t.len(7), 3);
        t.set_points(0);
        assert_eq!(t.len(7), 0);
        assert!(!t.record(7, fix(200, None), 300));
    }

    #[test]
    fn tracks_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("map-track-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = dir.join("history.bin");
        let mut t = Tracks::load(Some(&storage), 30);
        t.record(7, fix(1000, Some("1000001")), 2000);
        t.flush(2000, true);
        let back = Tracks::load(Some(&storage), 30);
        assert_eq!(back.track(7), vec![fix(1000, Some("1000001"))]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dock_rules() {
        let now = 100 * 60_000;
        assert!(!docked(None, None, None, 30, now), "no station with a position: nothing to dock to");
        assert!(docked(Some("bs1"), None, None, 30, now), "no position: docked");
        assert!(!docked(Some("bs1"), Some(&pf(now - 60_000)), Some("bs1"), 30, now), "fresh position: undocked");
        assert!(docked(Some("bs1"), Some(&pf(now - 31 * 60_000)), Some("bs1"), 30, now), "older than dock_after: docked");
        assert!(!docked(Some("bs1"), Some(&pf(0)), Some("bs1"), 0, now), "0 = never by age");
        assert!(docked(Some("bs2"), Some(&pf(now - 60_000)), Some("bs1"), 30, now), "registered on another station since the fix");
        assert!(!docked(Some("bs2"), Some(&pf(now - 60_000)), None, 30, now), "station of the fix unknown: trust the fix");
    }

    #[tokio::test]
    async fn radio_on_a_located_station_is_docked_until_it_sends_a_position() {
        use crate::{protocol::ConnVersion, state::Client};
        let state = AppState::for_test();
        state.locations.write().unwrap().insert("1000001".into(), crate::config::BtsLocationConfig { name: "BS1".into(), lat: 48.2, lon: 16.36, icon: None });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let bs = uuid::Uuid::new_v4();
        {
            let mut inner = state.inner.write().await;
            inner.clients.insert(bs, Client { tx, mode: ClientMode::Basestation, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: Some("1000001".into()) });
            inner.subscribers.insert(2320972, crate::state::Subscriber { client_id: bs, groups: Default::default(), mode: ClientMode::Basestation, route: Default::default() });
        }
        let snap = |s: Arc<AppState>| async move { api_snapshot(axum::extract::State(s), axum::http::HeaderMap::new()).await.0 };
        let r = snap(state.clone()).await;
        let radio = r.radios.iter().find(|r| r.issi == 2320972).unwrap();
        assert!(radio.docked && radio.dock.as_deref() == Some("1000001") && radio.fix.is_none());
        // It sends a position: undocked, and the track holds it with its station.
        let now = crate::telemetry::now_ms();
        state.telemetry.write().await.record_sds_position(2320972, 48.21, 16.37, now, "lip".into());
        record_local(&state, 2320972, 48.21, 16.37, now).await;
        let r = snap(state.clone()).await;
        let radio = r.radios.iter().find(|r| r.issi == 2320972).unwrap();
        assert!(!radio.docked && radio.fix.is_some() && radio.points == 1);
        assert_eq!(state.tracks.lock().unwrap().newest(2320972).unwrap().station.as_deref(), Some("1000001"));
    }

    #[tokio::test]
    async fn numbers_on_a_terminal_connection_get_the_terminal_marker() {
        use crate::{protocol::ConnVersion, state::Client};
        let state = AppState::for_test();
        let mut inner = state.inner.write().await;
        let mut login = |mode: ClientMode, user: &str| {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let id = uuid::Uuid::new_v4();
            inner.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: Some(user.into()) });
            id
        };
        let (bs, app, bridge) = (login(ClientMode::Basestation, "1000001"), login(ClientMode::Terminal, "2320100"), login(ClientMode::Terminal, "9100232"));
        for (issi, id, mode) in [(2320972, bs, ClientMode::Basestation), (2320100, app, ClientMode::Terminal), (2320500, bridge, ClientMode::Terminal), (2320973, bs, ClientMode::Basestation)] {
            inner.subscribers.insert(issi, crate::state::Subscriber { client_id: id, groups: Default::default(), mode, route: Default::default() });
        }
        drop(inner);
        state.issi_kinds.write().unwrap().insert("9100232".into(), "bridge".into());
        state.issi_kinds.write().unwrap().insert("2320973".into(), "terminal".into());
        let t = terminal_issis(&state).await;
        assert!(t.contains(&2320100), "app on a Terminal connection");
        assert!(!t.contains(&2320972), "radio on a station");
        assert!(!t.contains(&2320500), "radio behind a bridge login");
        assert!(t.contains(&2320973), "number set to terminal");

        // The app sends its position and leaves: still a terminal on the map, until it registers on a station.
        let now = crate::telemetry::now_ms();
        state.telemetry.write().await.record_sds_position(2320100, 48.21, 16.37, now, "lip".into());
        record_local(&state, 2320100, 48.21, 16.37, now).await;
        state.inner.write().await.subscribers.remove(&2320100);
        let snap = |s: Arc<AppState>| async move { api_snapshot(axum::extract::State(s), axum::http::HeaderMap::new()).await.0 };
        let term = |r: MapSnapshot| r.radios.iter().find(|r| r.issi == 2320100).map(|r| r.terminal);
        assert_eq!(term(snap(state.clone()).await), Some(true), "left after its position");
        assert!(snap(state.clone()).await.radios.iter().any(|r| r.issi == 2320100 && !r.online), "and is offline");
        state.inner.write().await.subscribers.insert(2320100, crate::state::Subscriber { client_id: bs, groups: Default::default(), mode: ClientMode::Basestation, route: Default::default() });
        assert_eq!(term(snap(state.clone()).await), Some(false), "now registered on a station");
        assert!(snap(state.clone()).await.radios.iter().any(|r| r.issi == 2320100 && r.online));
    }

    #[tokio::test]
    async fn guests_lose_the_track_only_when_switched_off() {
        use base64::Engine;
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.dashboard.users.insert("alice".into(), "secret".into());
        config.dashboard.admins = vec!["alice".into()];
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let mut alice = axum::http::HeaderMap::new();
        let creds = base64::engine::general_purpose::STANDARD.encode("alice:secret");
        alice.insert(axum::http::header::AUTHORIZATION, format!("Basic {creds}").parse().unwrap());
        let guest = axum::http::HeaderMap::new();
        state.tracks.lock().unwrap().record(2320972, fix(1_000, Some("1000001")), 2_000);
        let track = |h: axum::http::HeaderMap| { let s = state.clone(); async move {
            api_track(axum::extract::State(s), h, axum::extract::Query(HashMap::from([("issi".to_owned(), "2320972".to_owned())]))).await.0.len() } };
        assert_eq!((track(guest.clone()).await, track(alice.clone()).await), (1, 1), "default: everyone");
        state.map_cfg.write().unwrap().guests_see_track = false;
        assert!(track_hidden(&state, &guest) && !track_hidden(&state, &alice));
        assert_eq!((track(guest).await, track(alice).await), (0, 1), "off: signed-in users only");
    }

}
