//! The groups given to radios by DGNA (SS-DGNA ASSIGN, TS 100 392-12-22),
//! so they can be taken back later: assigned from this server's DGNA page or
//! on a station's own dashboard. Stations with the `dgna-report` extension
//! report every assign and deassign over the control channel
//! (`DgnaChanged`), and send what they know when the channel connects
//! (`DgnaGroups`, adds only: a station forgets radios over its own restart).
//!
//! Kept across a restart in `<storage>.dgna`. An entry goes away when a
//! station reports the deassign, or when an admin forgets it on the page.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::state::AppState;

/// Name in `X-Brew-Extensions` with which the server asks for the reports.
pub const CAPABILITY: &str = "dgna-report";
/// A station's report counts as the answer to a DGNA sent from this server
/// when it comes within this time.
const SERVER_MATCH_MS: u64 = 120_000;
/// The store is written at most this often.
const SAVE_EVERY_MS: u64 = 5_000;
/// At most this many entries are kept (the oldest go first).
const MAX_ENTRIES: usize = 50_000;

/// One group a station reports for a radio (`DgnaGroups` / `DgnaChanged`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DgnaEntry {
    pub issi: u32,
    pub gssi: u32,
    #[serde(default)]
    pub mnemonic: Option<String>,
    #[serde(default)]
    pub attachment_mode: u8,
}

/// Where an assignment was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// From this server's DGNA page.
    Server,
    /// On the station (its dashboard), or a DGNA from this server whose
    /// report came too late to tell.
    Station,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub issi: u32,
    pub gssi: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub attachment_mode: u8,
    /// Control login of the station that reported it last.
    pub station: String,
    pub source: Source,
    /// Dashboard user who sent it from this server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// When it was assigned (first report), ms since the epoch.
    pub at_ms: u64,
}

/// A DGNA this server sent and waits to see reported.
struct Sent {
    assign: bool,
    by: Option<String>,
    at_ms: u64,
}

pub struct DgnaRegistry {
    path: Option<PathBuf>,
    data: BTreeMap<(u32, u32), Assignment>,
    sent: HashMap<(u32, u32), Sent>,
    dirty: bool,
    saved_ms: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    #[serde(default)]
    assignments: Vec<Assignment>,
}

impl DgnaRegistry {
    /// File `<storage>.dgna`; `None` keeps everything in memory.
    pub fn load(storage: Option<&Path>) -> Self {
        let path = storage.map(|p| {
            let mut s = p.as_os_str().to_owned();
            s.push(".dgna");
            PathBuf::from(s)
        });
        let file: File = path.as_deref().and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| tracing::warn!(error = %e, "DGNA file unreadable, starting empty")).ok())
            .unwrap_or_default();
        let data = file.assignments.into_iter().map(|a| ((a.issi, a.gssi), a)).collect();
        Self { path, data, sent: HashMap::new(), dirty: false, saved_ms: 0 }
    }

    /// Notes a DGNA sent from this server, so the station's report is
    /// marked as coming from here (and by whom).
    pub fn sent(&mut self, issi: u32, gssi: u32, assign: bool, by: Option<String>, now_ms: u64) {
        self.sent.retain(|_, s| now_ms.saturating_sub(s.at_ms) < SERVER_MATCH_MS);
        self.sent.insert((issi, gssi), Sent { assign, by, at_ms: now_ms });
    }

    /// A station reports an assign (`assigned`) or a deassign. Returns
    /// whether the list changed.
    pub fn changed(&mut self, station: &str, e: &DgnaEntry, assigned: bool, now_ms: u64) -> bool {
        let key = (e.issi, e.gssi);
        let ours = self.sent.get(&key).filter(|s| s.assign == assigned && now_ms.saturating_sub(s.at_ms) < SERVER_MATCH_MS).is_some();
        let by = if ours { self.sent.remove(&key).and_then(|s| s.by) } else { None };
        if !assigned {
            let gone = self.data.remove(&key).is_some();
            self.dirty |= gone;
            return gone;
        }
        let name = e.mnemonic.clone().filter(|n| !n.trim().is_empty());
        match self.data.get_mut(&key) {
            // Assigned again: keep when and where it was first given, unless it
            // came from here now.
            Some(a) => {
                let before = a.clone();
                a.name = name;
                a.attachment_mode = e.attachment_mode;
                a.station = station.to_owned();
                if ours {
                    a.source = Source::Server;
                    a.by = by;
                    a.at_ms = now_ms;
                }
                let changed = *a != before;
                self.dirty |= changed;
                changed
            }
            None => {
                let source = if ours { Source::Server } else { Source::Station };
                self.data.insert(key, Assignment { issi: e.issi, gssi: e.gssi, name, attachment_mode: e.attachment_mode, station: station.to_owned(), source, by, at_ms: now_ms });
                self.trim();
                self.dirty = true;
                true
            }
        }
    }

    /// What a station knows when its channel connects: unknown entries are
    /// added (assigned while the channel was down), nothing is removed.
    pub fn snapshot(&mut self, station: &str, entries: &[DgnaEntry], now_ms: u64) -> bool {
        let mut changed = false;
        for e in entries {
            if !self.data.contains_key(&(e.issi, e.gssi)) {
                changed |= self.changed(station, e, true, now_ms);
            }
        }
        changed
    }

    /// Admin: drop an entry the radio no longer has (nothing goes on air).
    pub fn forget(&mut self, issi: u32, gssi: u32) -> bool {
        let gone = self.data.remove(&(issi, gssi)).is_some();
        self.dirty |= gone;
        gone
    }

    pub fn list(&self) -> Vec<Assignment> {
        self.data.values().cloned().collect()
    }

    #[cfg(test)]
    pub fn of(&self, issi: u32) -> Vec<Assignment> {
        self.data.range((issi, 0)..=(issi, u32::MAX)).map(|(_, a)| a.clone()).collect()
    }

    fn trim(&mut self) {
        if self.data.len() <= MAX_ENTRIES { return; }
        let mut by_age: Vec<(u64, (u32, u32))> = self.data.iter().map(|(k, a)| (a.at_ms, *k)).collect();
        by_age.sort_unstable();
        for (_, k) in by_age.iter().take(self.data.len() - MAX_ENTRIES) { self.data.remove(k); }
    }

    /// Writes the file when something changed, at most every few seconds unless `force`.
    pub fn flush(&mut self, now_ms: u64, force: bool) {
        if !self.dirty || (!force && now_ms.saturating_sub(self.saved_ms) < SAVE_EVERY_MS) { return; }
        self.dirty = false;
        self.saved_ms = now_ms;
        let Some(path) = &self.path else { return };
        let Ok(bytes) = serde_json::to_vec(&File { assignments: self.list() }) else { return };
        let tmp = path.with_extension("dgna.tmp");
        if let Err(e) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
            tracing::warn!(error = %e, path = %path.display(), "cannot write the DGNA file");
        }
    }
}

/// Writes pending changes every few seconds.
pub async fn run(state: Arc<AppState>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(SAVE_EVERY_MS));
    loop {
        tick.tick().await;
        if let Ok(mut d) = state.dgna.lock() { d.flush(crate::telemetry::now_ms(), false); }
    }
}

#[derive(Serialize)]
pub struct AssignmentView {
    #[serde(flatten)]
    pub a: Assignment,
    /// Display name of the station (Accounts), when one is given.
    pub station_name: Option<String>,
    /// The radio is registered here now (so it can be taken back).
    pub registered: bool,
}

/// `GET /api/dgna/assignments`: every remembered DGNA group, oldest radio first.
pub async fn assignments(axum::extract::State(state): axum::extract::State<Arc<AppState>>) -> axum::Json<Vec<AssignmentView>> {
    let list = state.dgna.lock().map(|d| d.list()).unwrap_or_default();
    let names = crate::control::station_names(&state);
    let inner = state.inner.read().await;
    axum::Json(list.into_iter().map(|a| AssignmentView {
        station_name: names.get(&a.station).cloned(),
        registered: inner.subscribers.contains_key(&a.issi),
        a,
    }).collect())
}

#[derive(Deserialize)]
pub struct ForgetRequest {
    pub issi: u32,
    pub gssi: u32,
}

/// `POST /api/dgna/forget` `{issi, gssi}`: drop an entry here only.
pub async fn forget(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<ForgetRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let access = crate::dashboard::access(&state, &headers);
    if !access.manages_radios() {
        return (axum::http::StatusCode::FORBIDDEN, "This needs the right DGNA & Radio actions: send").into_response();
    }
    let gone = state.dgna.lock().map(|mut d| { let g = d.forget(req.issi, req.gssi); d.flush(crate::telemetry::now_ms(), true); g }).unwrap_or(false);
    tracing::warn!(issi = req.issi, gssi = req.gssi, user = access.user.as_deref().unwrap_or("-"), "DGNA entry forgotten on the dashboard");
    axum::Json(serde_json::json!({ "forgotten": gone })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(issi: u32, gssi: u32, name: Option<&str>) -> DgnaEntry {
        DgnaEntry { issi, gssi, mnemonic: name.map(str::to_owned), attachment_mode: 0 }
    }

    #[test]
    fn station_assign_and_deassign() {
        let mut r = DgnaRegistry::load(None);
        assert!(r.changed("bs1", &e(2001, 91, Some("Ops")), true, 1_000));
        let a = &r.list()[0];
        assert_eq!((a.issi, a.gssi, a.name.as_deref(), a.station.as_str(), a.source), (2001, 91, Some("Ops"), "bs1", Source::Station));
        assert!(r.changed("bs1", &e(2001, 91, None), false, 2_000));
        assert!(r.list().is_empty());
        // Deassign of something unknown changes nothing.
        assert!(!r.changed("bs1", &e(2001, 92, None), false, 3_000));
    }

    #[test]
    fn a_dgna_from_here_is_marked_with_its_user() {
        let mut r = DgnaRegistry::load(None);
        r.sent(2001, 91, true, Some("anna".into()), 1_000);
        r.changed("bs1", &e(2001, 91, None), true, 1_500);
        let a = &r.of(2001)[0];
        assert_eq!((a.source, a.by.as_deref()), (Source::Server, Some("anna")));
        // Too late, or the other direction: from the station.
        r.sent(2001, 92, true, Some("anna".into()), 1_000);
        r.changed("bs1", &e(2001, 92, None), true, 1_000 + SERVER_MATCH_MS + 1);
        r.sent(2001, 93, false, None, 1_000);
        r.changed("bs1", &e(2001, 93, None), true, 1_500);
        assert!(r.of(2001).iter().filter(|a| a.gssi != 91).all(|a| a.source == Source::Station));
    }

    #[test]
    fn deassign_on_another_station_removes_it() {
        let mut r = DgnaRegistry::load(None);
        r.changed("bs1", &e(2001, 91, None), true, 1_000);
        assert!(r.changed("bs2", &e(2001, 91, None), false, 2_000));
        assert!(r.list().is_empty());
    }

    #[test]
    fn snapshot_adds_but_never_removes() {
        let mut r = DgnaRegistry::load(None);
        r.changed("bs1", &e(2001, 91, Some("Ops")), true, 1_000);
        // bs2 restarted and knows only one other radio.
        assert!(r.snapshot("bs2", &[e(2002, 95, None)], 2_000));
        assert_eq!(r.list().len(), 2);
        // Known entries are left as they were.
        assert!(!r.snapshot("bs2", &[e(2001, 91, None)], 3_000));
        assert_eq!(r.of(2001)[0].station, "bs1");
        assert_eq!(r.of(2001)[0].name.as_deref(), Some("Ops"));
    }

    #[test]
    fn kept_across_a_restart() {
        let dir = std::env::temp_dir().join(format!("dgna-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = dir.join("brew-history.bin");
        let mut r = DgnaRegistry::load(Some(&storage));
        r.sent(2001, 91, true, Some("anna".into()), 1_000);
        r.changed("bs1", &e(2001, 91, Some("Ops")), true, 1_100);
        r.changed("bs1", &e(2002, 92, None), true, 1_200);
        r.flush(10_000, true);
        let back = DgnaRegistry::load(Some(&storage));
        assert_eq!(back.list(), r.list());
        assert!(dir.join("brew-history.bin.dgna").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn forget_drops_only_that_entry() {
        let mut r = DgnaRegistry::load(None);
        r.changed("bs1", &e(2001, 91, None), true, 1_000);
        r.changed("bs1", &e(2001, 92, None), true, 1_000);
        assert!(r.forget(2001, 91));
        assert!(!r.forget(2001, 91));
        assert_eq!(r.of(2001).iter().map(|a| a.gssi).collect::<Vec<_>>(), vec![92]);
    }
}
