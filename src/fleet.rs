//! Fleet Control: stations that let this server (the SwMI) manage them, over
//! the Basestation control channel. Protocol and behaviour: `docs/Fleet-Control.md`.
//!
//! The server offers `fleet-control` in `X-Brew-Extensions` (Brew and control
//! listener) whenever `[control]` is enabled. A FlowStation that sees it sends
//! `StationHello` after connecting and whenever its local switch changes; the
//! general answer to Fleet Control commands is `CommandResult`.
//!
//! The Fleet Manager lists every station whose last hello had the switch on,
//! keyed by its control login. A station stays listed while it is offline;
//! it leaves the list when it says the switch is off (at once while online,
//! else at its next login), with the INFO "Basestation XYZ switched off Fleet
//! Control", or when a user with Fleet Control management removes it. The
//! change log keeps 90 days. Both survive restarts in `<storage path>.fleet`.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
};

/// Name in `X-Brew-Extensions`.
pub const CAPABILITY: &str = "fleet-control";

/// Fleet Control protocol version this server speaks (`StationHello.protocol`).
pub const PROTOCOL: u32 = 1;

/// How long change log entries are kept.
pub const LOG_KEEP_MS: u64 = 90 * 24 * 3600 * 1000;

/// What a station reports in its hello (see `control::ControlResponse::StationHello`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
    pub brew_login: Option<String>,
    pub mcc: u16,
    pub mnc: u16,
    pub fleet_control: bool,
    pub capabilities: Vec<String>,
}

/// One station in the Fleet Manager.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Station {
    /// Control login (the key).
    pub id: String,
    pub brew_login: Option<String>,
    pub version: String,
    pub mcc: u16,
    pub mnc: u16,
    pub protocol: u32,
    pub capabilities: Vec<String>,
    /// Control channel connected now (not persisted: false after a restart).
    #[serde(skip)]
    pub online: bool,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// Last update check (`CheckUpdate`), phase 2.
    #[serde(default)]
    pub check: Option<CheckInfo>,
    /// Last `UpdateProgress` of an OTA run (whoever started it), phase 2.
    #[serde(default)]
    pub progress: Option<ProgressInfo>,
    /// Maintenance flag set from here (`maintenance-v1`), phase 3.
    #[serde(default)]
    pub maintenance: Option<MaintenanceInfo>,
    /// A restart / reboot was sent and the station is not back yet, phase 3.
    #[serde(default)]
    pub restart: Option<RestartInfo>,
    /// Config copy, local changes, pending and running changes (`config-v1`), phases 4/5.
    #[serde(default)]
    pub cfg: crate::fleet_config::StationConfig,
}

/// Maintenance flag of a station (the station's own copy runs out at the same time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceInfo {
    pub by: Option<String>,
    pub reason: String,
    pub since_ms: u64,
    pub until_ms: u64,
}

/// A restart / reboot waiting for the station to come back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestartInfo {
    pub host: bool,
    pub by: Option<String>,
    pub at_ms: u64,
}

/// Longest maintenance time (minutes).
pub const MAX_MAINTENANCE_MIN: u32 = 240;
/// A rebooted / restarted station not back after this long: alarm.
pub const RESTART_RETURN_MS: u64 = 10 * 60 * 1000;
/// Capability: maintenance flag.
pub const MAINTENANCE_CAPABILITY: &str = "maintenance-v1";
/// Capability: restart / reboot.
pub const RESTART_CAPABILITY: &str = "restart-v1";

/// Result of the last update check of a station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckInfo {
    pub at_ms: u64,
    /// The check reached the repository.
    pub ok: bool,
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
    /// OTA source of the station.
    pub channel: String,
    pub repo: String,
    pub branch: String,
    pub message: String,
}

/// Last reported OTA progress of a station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressInfo {
    pub at_ms: u64,
    pub stage: String,
    pub percent: u8,
    pub message: String,
}

/// Server setting "Updates repo": the reference source the Fleet Manager
/// compares the stations' OTA sources with (empty: the product repository,
/// branch `main`). Phase 2 only shows a hint; stations keep their own source.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub branch: String,
}

/// Product repository and branch used when "Updates repo" is empty.
pub const DEFAULT_UPDATE_REPO: &str = "https://github.com/Aitorrio/bost-flowstation";
pub const DEFAULT_UPDATE_BRANCH: &str = "main";

impl UpdateSettings {
    /// Repository and branch in effect.
    pub fn effective(&self) -> (String, String) {
        let repo = if self.repo.trim().is_empty() { DEFAULT_UPDATE_REPO } else { self.repo.trim() };
        let branch = if self.branch.trim().is_empty() { DEFAULT_UPDATE_BRANCH } else { self.branch.trim() };
        (repo.to_owned(), branch.to_owned())
    }
}

/// Repository URLs compared without case, `.git`, trailing slash or credentials.
pub fn same_repo(a: &str, b: &str) -> bool {
    fn norm(u: &str) -> String {
        let u = u.trim().to_ascii_lowercase();
        let u = match u.split_once("://") {
            Some((scheme, rest)) => format!("{scheme}://{}", rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest)),
            None => u,
        };
        let u = u.trim_end_matches('/');
        u.strip_suffix(".git").unwrap_or(u).to_owned()
    }
    norm(a) == norm(b)
}

/// One change log line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Running number (the dashboard dismisses INFO popups by it).
    pub id: u64,
    pub at_ms: u64,
    /// Control login of the station.
    pub station: String,
    /// Name or number of the station at the time.
    pub label: String,
    /// Dashboard user who did it (`None`: the station itself).
    pub user: Option<String>,
    /// `fleet_on`, `fleet_off`, `version`, `removed`; phase 2: `update`,
    /// `update_done`, `update_alarm` (shown red), `update_skip`, `update_end`,
    /// `settings`.
    pub kind: String,
    pub text: String,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    stations: BTreeMap<String, Station>,
    log: VecDeque<LogEntry>,
    next_id: u64,
    /// The current or last update run (phase 2).
    #[serde(default)]
    job: Option<crate::fleet_update::Job>,
    #[serde(default)]
    next_job: u64,
    #[serde(default)]
    update_settings: UpdateSettings,
    /// Phase 6: templates, the current or last bulk config change, counted cell changes, settings.
    #[serde(default)]
    templates: Vec<crate::fleet_bulk::Template>,
    #[serde(default)]
    next_template: u64,
    #[serde(default)]
    bulk: Option<crate::fleet_bulk::Run>,
    #[serde(default)]
    next_bulk: u64,
    #[serde(default)]
    moves: Vec<crate::fleet_bulk::MoveCount>,
    #[serde(default)]
    config_settings: ConfigSettings,
}

/// Server setting for config changes (Server settings → Fleet Control, admins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigSettings {
    /// Confirmation window a station gets after a config change (s).
    #[serde(default = "default_confirm_secs")]
    pub confirm_secs: u32,
}

fn default_confirm_secs() -> u32 {
    crate::fleet_config::DEFAULT_CONFIRM_SECS
}

impl Default for ConfigSettings {
    fn default() -> Self {
        Self { confirm_secs: default_confirm_secs() }
    }
}

/// The Fleet Manager's data.
#[derive(Default)]
pub struct Fleet {
    path: Option<PathBuf>,
    data: File,
    /// Open edit pages per station (not kept over a restart), phase 5.
    pub edits: std::collections::HashMap<String, crate::fleet_config::EditSession>,
}

/// Name for log lines: the name given to the station on this server, else its
/// Brew number, else its control login.
pub fn label(names: &dyn Fn(&str) -> Option<String>, id: &str, brew_login: Option<&str>) -> String {
    brew_login.and_then(|l| names(l)).or_else(|| names(id)).unwrap_or_else(|| brew_login.unwrap_or(id).to_owned())
}

impl Fleet {
    /// Loads `path` (missing or broken: empty). `None`: memory only.
    pub fn load(path: Option<PathBuf>, now_ms: u64) -> Self {
        let data = path
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .unwrap_or_default();
        let mut fleet = Self { path, data, edits: Default::default() };
        fleet.prune(now_ms);
        if let Some(job) = fleet.data.job.as_mut() {
            job.after_restart();
        }
        if let Some(run) = fleet.data.bulk.as_mut() {
            run.after_restart();
        }
        fleet
    }

    /// The file next to the history log.
    pub fn path_for(storage: &Path) -> PathBuf {
        let mut p = storage.as_os_str().to_owned();
        p.push(".fleet");
        p.into()
    }

    pub fn stations(&self) -> Vec<Station> {
        self.data.stations.values().cloned().collect()
    }

    pub fn log(&self) -> Vec<LogEntry> {
        self.data.log.iter().rev().cloned().collect()
    }

    /// Change log entries newer than `id` (all of them for `None`), oldest first.
    pub fn log_since(&self, id: Option<u64>) -> Vec<LogEntry> {
        self.data.log.iter().filter(|e| id.is_none_or(|i| e.id > i)).cloned().collect()
    }

    pub fn get(&self, id: &str) -> Option<&Station> {
        self.data.stations.get(id)
    }

    /// Change one station and save; `None` when it is not listed.
    pub fn with_station<R>(&mut self, id: &str, f: impl FnOnce(&mut Station) -> R) -> Option<R> {
        let r = f(self.data.stations.get_mut(id)?);
        self.save();
        Some(r)
    }

    /// A change log line about station `id` (its label from `names`).
    pub fn log_station(&mut self, id: &str, user: Option<&str>, kind: &str, text: impl FnOnce(&str) -> String, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) {
        let brew = self.data.stations.get(id).and_then(|s| s.brew_login.clone());
        let label = label(names, id, brew.as_deref());
        let text = text(&label);
        self.push(now_ms, id, label, user, kind, text);
        self.save();
    }

    /// Drops log entries older than 90 days.
    pub fn prune(&mut self, now_ms: u64) {
        while self.data.log.front().is_some_and(|e| e.at_ms + LOG_KEEP_MS < now_ms) {
            self.data.log.pop_front();
        }
    }

    fn push(&mut self, now_ms: u64, station: &str, label: String, user: Option<&str>, kind: &str, text: String) {
        self.data.next_id += 1;
        tracing::info!(station, kind, "Fleet Control: {text}");
        self.data.log.push_back(LogEntry {
            id: self.data.next_id,
            at_ms: now_ms,
            station: station.to_owned(),
            label,
            user: user.map(str::to_owned),
            kind: kind.to_owned(),
            text,
        });
        self.prune(now_ms);
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(bytes) = serde_json::to_vec_pretty(&self.data) else { return };
        let tmp = path.with_extension("fleet.tmp");
        if let Err(e) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
            tracing::warn!(error = %e, path = %path.display(), "cannot write the Fleet Control file");
        }
    }

    /// A hello from the station logged in as `id`. Returns true when the list changed.
    pub fn hello(&mut self, id: &str, h: &Hello, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) -> bool {
        let label = label(names, id, h.brew_login.as_deref());
        if !h.fleet_control {
            // Off: the station leaves the list (it was listed) with the INFO.
            if self.data.stations.remove(id).is_none() {
                return false;
            }
            self.push(now_ms, id, label.clone(), None, "fleet_off", format!("Basestation {label} switched off Fleet Control"));
            self.save();
            return true;
        }
        let entry = self.data.stations.get(id).cloned();
        let station = Station {
            id: id.to_owned(),
            brew_login: h.brew_login.clone(),
            version: h.version.clone(),
            mcc: h.mcc,
            mnc: h.mnc,
            protocol: h.protocol,
            capabilities: h.capabilities.clone(),
            online: true,
            first_seen_ms: entry.as_ref().map(|s| s.first_seen_ms).unwrap_or(now_ms),
            last_seen_ms: now_ms,
            // A check is only valid for the software it was made with.
            check: entry.as_ref().and_then(|s| s.check.clone()).filter(|c| c.current == h.version),
            progress: entry.as_ref().and_then(|s| s.progress.clone()),
            maintenance: entry.as_ref().and_then(|s| s.maintenance.clone()),
            restart: None,
            cfg: entry.as_ref().map(|s| s.cfg.clone()).unwrap_or_default(),
        };
        if let Some(r) = entry.as_ref().and_then(|s| s.restart.as_ref()) {
            let secs = now_ms.saturating_sub(r.at_ms) / 1000;
            self.push(now_ms, id, label.clone(), None, "restart", format!("Basestation {label}: back after the {} ({secs} s)", if r.host { "reboot" } else { "service restart" }));
        }
        // An update run that ends with this hello logs the software change itself.
        let events = self.data.job.as_mut().map(|j| j.hello(id, &h.version, now_ms)).unwrap_or_default();
        let run_logs_it = events.iter().any(|e| e.kind == "update_done");
        match &entry {
            None => self.push(now_ms, id, label.clone(), None, "fleet_on", format!("Basestation {label} switched on Fleet Control")),
            Some(_) if run_logs_it => {}
            Some(old) if old.version != h.version => {
                self.push(now_ms, id, label.clone(), None, "version", format!("Basestation {label}: software {} → {}", old.version, h.version))
            }
            Some(_) => {}
        }
        self.data.stations.insert(id.to_owned(), station);
        self.apply(now_ms, events);
        self.save();
        true
    }

    /// The control channel of `id` went up or down (no log line: not a change).
    pub fn set_online(&mut self, id: &str, online: bool, now_ms: u64) {
        if let Some(s) = self.data.stations.get_mut(id) {
            s.online = online;
            s.last_seen_ms = now_ms;
            if !online {
                self.save();
            }
        }
    }

    /// A user removes a station from the list (it comes back with its next
    /// hello if its switch is still on).
    pub fn remove(&mut self, id: &str, user: Option<&str>, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) -> bool {
        let Some(s) = self.data.stations.remove(id) else { return false };
        let label = label(names, id, s.brew_login.as_deref());
        self.push(now_ms, id, label.clone(), user, "removed", format!("Basestation {label} removed from the Fleet Manager"));
        self.save();
        true
    }
}

/// Phase 2: update checks and runs.
impl Fleet {
    /// Write run events to the change log.
    fn apply(&mut self, now_ms: u64, events: Vec<crate::fleet_update::Event>) {
        for e in events {
            self.push(now_ms, &e.station, e.label, e.user.as_deref(), e.kind, e.text);
        }
    }

    pub fn job(&self) -> Option<&crate::fleet_update::Job> {
        self.data.job.as_ref()
    }

    pub fn update_settings(&self) -> &UpdateSettings {
        &self.data.update_settings
    }

    /// Admin changes "Updates repo".
    pub fn set_update_settings(&mut self, s: UpdateSettings, user: Option<&str>, now_ms: u64) {
        let (repo, branch) = s.effective();
        self.data.update_settings = s;
        self.push(now_ms, "", String::new(), user, "settings", format!("Updates repo set to {repo} ({branch})"));
        self.save();
    }

    /// Store the result of an update check.
    pub fn set_check(&mut self, id: &str, check: CheckInfo) {
        if let Some(s) = self.data.stations.get_mut(id) {
            s.check = Some(check);
            self.save();
        }
    }

    /// An `UpdateProgress` from station `id` (also for runs started on its own dashboard).
    pub fn progress(&mut self, id: &str, stage: &str, percent: u8, message: &str, _version: &str, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) {
        let Some(s) = self.data.stations.get_mut(id) else { return };
        s.progress = Some(ProgressInfo { at_ms: now_ms, stage: stage.to_owned(), percent, message: message.to_owned() });
        let in_job = self.data.job.as_ref().is_some_and(|j| !j.is_finished() && j.items.iter().any(|i| i.id == id));
        let events = match self.data.job.as_mut() {
            Some(j) if in_job => j.progress(id, stage, percent, message, now_ms),
            _ => {
                // An update the server did not start (local dashboard): log its end only.
                let label = label(names, id, self.data.stations.get(id).and_then(|s| s.brew_login.as_deref()));
                match stage {
                    "restart" => vec![crate::fleet_update::Event { station: id.to_owned(), label: label.clone(), kind: "update", text: format!("Basestation {label}: local update built, restarting"), user: None }],
                    "failed" => vec![crate::fleet_update::Event { station: id.to_owned(), label: label.clone(), kind: "update_alarm", text: format!("ALARM Basestation {label}: local update failed ({message})"), user: None }],
                    _ => vec![],
                }
            }
        };
        let changed = !events.is_empty();
        self.apply(now_ms, events);
        if changed || matches!(stage, "restart" | "failed" | "up_to_date") {
            self.save();
        }
    }

    /// Stations that can be updated now: listed, online, `update-v1`, and their
    /// last check (for the software they run) found a newer version.
    pub fn updatable(&self, id: &str) -> bool {
        self.data.stations.get(id).is_some_and(|s| {
            s.online && s.capabilities.iter().any(|c| c == UPDATE_CAPABILITY) && s.check.as_ref().is_some_and(|c| c.ok && c.update_available && c.current == s.version)
        })
    }

    /// Maintenance on (`minutes`) or off for `id`, set from here; logged.
    pub fn set_maintenance(&mut self, id: &str, on: bool, minutes: u32, reason: &str, user: Option<&str>, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) -> bool {
        let Some(s) = self.data.stations.get_mut(id) else { return false };
        let label = label(names, id, s.brew_login.as_deref());
        let text = if on {
            let minutes = minutes.clamp(1, MAX_MAINTENANCE_MIN);
            s.maintenance = Some(MaintenanceInfo { by: user.map(str::to_owned), reason: reason.trim().to_owned(), since_ms: now_ms, until_ms: now_ms + minutes as u64 * 60_000 });
            format!("Basestation {label}: maintenance on for {minutes} min{}", if reason.trim().is_empty() { String::new() } else { format!(" ({})", reason.trim()) })
        } else {
            s.maintenance = None;
            format!("Basestation {label}: maintenance off")
        };
        self.push(now_ms, id, label, user, "maintenance", text);
        self.save();
        true
    }

    /// After a hello: the `SetMaintenance` to send again if maintenance is still on.
    pub fn maintenance_resend(&self, id: &str, now_ms: u64) -> Option<crate::control::ControlCommand> {
        let s = self.data.stations.get(id)?;
        let m = s.maintenance.as_ref().filter(|m| m.until_ms > now_ms)?;
        if !s.capabilities.iter().any(|c| c == MAINTENANCE_CAPABILITY) {
            return None;
        }
        let minutes = (m.until_ms - now_ms).div_ceil(60_000) as u32;
        Some(crate::control::ControlCommand::SetMaintenance { handle: 0, on: true, minutes, by: m.by.clone().unwrap_or_default(), reason: m.reason.clone() })
    }

    /// A restart / reboot was accepted by `id`; logged, the return is watched.
    pub fn restart_sent(&mut self, id: &str, host: bool, user: Option<&str>, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) {
        let Some(s) = self.data.stations.get_mut(id) else { return };
        let label = label(names, id, s.brew_login.as_deref());
        s.restart = Some(RestartInfo { host, by: user.map(str::to_owned), at_ms: now_ms });
        self.push(now_ms, id, label.clone(), user, "restart", format!("Basestation {label}: {}", if host { "system reboot" } else { "service restart" }));
        self.save();
    }

    /// Run-out maintenance and stations not back after a restart (every 2 s).
    pub fn tick_stations(&mut self, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) -> bool {
        let mut logs = Vec::new();
        for s in self.data.stations.values_mut() {
            let label = label(names, &s.id, s.brew_login.as_deref());
            if s.maintenance.as_ref().is_some_and(|m| m.until_ms <= now_ms) {
                s.maintenance = None;
                logs.push((s.id.clone(), label.clone(), "maintenance", format!("Basestation {label}: maintenance ended (time up)")));
            }
            if s.restart.as_ref().is_some_and(|r| now_ms.saturating_sub(r.at_ms) > RESTART_RETURN_MS) {
                let host = s.restart.take().is_some_and(|r| r.host);
                logs.push((s.id.clone(), label.clone(), "restart_alarm", format!("ALARM Basestation {label}: not back 10 min after the {}", if host { "reboot" } else { "service restart" })));
            }
        }
        let changed = !logs.is_empty();
        for (id, label, kind, text) in logs {
            self.push(now_ms, &id, label, None, kind, text);
        }
        if changed {
            self.save();
        }
        changed
    }

    /// Stations that can take a live update now: listed, online, `update-live-v1`.
    pub fn live_capable(&self, id: &str) -> bool {
        self.data.stations.get(id).is_some_and(|s| s.online && s.capabilities.iter().any(|c| c == LIVE_UPDATE_CAPABILITY))
    }

    /// "Clear" (admins): empties the change log; one entry says who cleared it.
    pub fn clear_log(&mut self, user: Option<&str>, now_ms: u64) {
        self.data.log.clear();
        let who = user.unwrap_or("an admin");
        self.push(now_ms, "", String::new(), user, "log_clear", format!("Log cleared by {who}"));
        self.save();
    }

    /// Start a run over `ids` (in this order). Err: a run is still going, or no station.
    pub fn start_job(&mut self, ids: &[String], options: crate::fleet_update::Options, user: Option<&str>, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) -> Result<u64, &'static str> {
        if self.data.job.as_ref().is_some_and(|j| !j.is_finished()) {
            return Err("An update run is in progress: wait for it or cancel it");
        }
        // A live update needs no check (the station may switch source); a normal
        // one only starts where the check found a newer version.
        let live = options.live.is_some();
        let cap = if live { LIVE_UPDATE_CAPABILITY } else { UPDATE_CAPABILITY };
        let stations: Vec<(String, String, String, bool)> = ids
            .iter()
            .filter_map(|id| self.data.stations.get(id))
            .filter(|s| s.capabilities.iter().any(|c| c == cap))
            .map(|s| (s.id.clone(), label(names, &s.id, s.brew_login.as_deref()), s.version.clone(), live || self.updatable(&s.id)))
            .collect();
        if stations.is_empty() {
            return Err("No station to update");
        }
        self.data.next_job += 1;
        let id = self.data.next_job;
        let (job, events) = crate::fleet_update::Job::new(id, now_ms, user.map(str::to_owned), options, stations);
        self.data.job = Some(job);
        self.apply(now_ms, events);
        self.save();
        Ok(id)
    }

    /// Advance the run; returns the `StartUpdate` commands to send.
    pub fn tick_job(&mut self, now_ms: u64) -> Vec<crate::fleet_update::Start> {
        let online: std::collections::HashSet<String> = self.data.stations.values().filter(|s| s.online).map(|s| s.id.clone()).collect();
        let Some(job) = self.data.job.as_mut() else { return Vec::new() };
        if job.is_finished() {
            return Vec::new();
        }
        let before = job.clone();
        let (starts, events) = job.tick(now_ms, &|id| online.contains(id));
        let changed = *job != before;
        self.apply(now_ms, events);
        if changed {
            self.save();
        }
        starts
    }

    pub fn start_result(&mut self, id: &str, outcome: crate::fleet_update::StartOutcome, now_ms: u64) {
        let Some(job) = self.data.job.as_mut() else { return };
        let events = job.start_result(id, outcome, now_ms);
        self.apply(now_ms, events);
        self.save();
    }

    /// Cancel the whole run (`station` None) or one station of it. False: nothing to cancel.
    pub fn cancel(&mut self, station: Option<&str>, user: Option<&str>, now_ms: u64) -> bool {
        let Some(job) = self.data.job.as_mut().filter(|j| !j.is_finished()) else { return false };
        let events = match station {
            None => job.cancel_all(user, now_ms),
            Some(id) => match job.cancel_one(id, user, now_ms) {
                Some(e) => e,
                None => return false,
            },
        };
        self.apply(now_ms, events);
        self.save();
        true
    }
}

/// Phase 6: templates, bulk config changes, counted cell changes.
impl Fleet {
    pub fn confirm_secs(&self) -> u32 {
        self.data.config_settings.confirm_secs.clamp(crate::fleet_config::MIN_CONFIRM_SECS, crate::fleet_config::MAX_CONFIRM_SECS)
    }

    pub fn set_confirm_secs(&mut self, secs: u32, user: Option<&str>, now_ms: u64) {
        self.data.config_settings.confirm_secs = secs;
        self.push(now_ms, "", String::new(), user, "settings", format!("Confirmation window of config changes set to {secs} s"));
        self.save();
    }

    pub fn templates(&self) -> Vec<crate::fleet_bulk::Template> {
        self.data.templates.clone()
    }

    /// New (`id` None) or changed template; logged. Err: unknown id, name taken, too many.
    pub fn save_template(&mut self, id: Option<u64>, name: String, note: String, sections: serde_json::Map<String, serde_json::Value>, user: Option<&str>, now_ms: u64) -> Result<u64, String> {
        if self.data.templates.iter().any(|t| Some(t.id) != id && t.name.eq_ignore_ascii_case(&name)) {
            return Err(format!("A template named \"{name}\" exists already"));
        }
        let what = sections.keys().cloned().collect::<Vec<_>>().join(", ");
        let id = match id {
            Some(id) => {
                let t = self.data.templates.iter_mut().find(|t| t.id == id).ok_or("No such template")?;
                *t = crate::fleet_bulk::Template { id, name: name.clone(), sections, note, by: user.map(str::to_owned), updated_ms: now_ms };
                self.push(now_ms, "", String::new(), user, "template", format!("Template \"{name}\" changed ({what})"));
                id
            }
            None => {
                if self.data.templates.len() >= crate::fleet_bulk::MAX_TEMPLATES {
                    return Err(format!("At most {} templates", crate::fleet_bulk::MAX_TEMPLATES));
                }
                self.data.next_template += 1;
                let id = self.data.next_template;
                self.data.templates.push(crate::fleet_bulk::Template { id, name: name.clone(), sections, note, by: user.map(str::to_owned), updated_ms: now_ms });
                self.push(now_ms, "", String::new(), user, "template", format!("Template \"{name}\" created ({what})"));
                id
            }
        };
        self.save();
        Ok(id)
    }

    pub fn delete_template(&mut self, id: u64, user: Option<&str>, now_ms: u64) -> bool {
        let Some(i) = self.data.templates.iter().position(|t| t.id == id) else { return false };
        let t = self.data.templates.remove(i);
        self.push(now_ms, "", String::new(), user, "template", format!("Template \"{}\" deleted", t.name));
        self.save();
        true
    }

    pub fn bulk(&self) -> Option<&crate::fleet_bulk::Run> {
        self.data.bulk.as_ref()
    }

    fn apply_bulk(&mut self, now_ms: u64, events: Vec<crate::fleet_bulk::Event>) {
        for e in events {
            self.push(now_ms, &e.station, e.label, e.user.as_deref(), e.kind, e.text);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_bulk(&mut self, ids: &[String], what: crate::fleet_bulk::What, mode: crate::fleet_update::Mode, during_call: bool, interrupt_min: u32, user: Option<&str>, now_ms: u64, names: &dyn Fn(&str) -> Option<String>) -> Result<u64, &'static str> {
        if self.data.bulk.as_ref().is_some_and(|r| !r.is_finished()) {
            return Err("A bulk config change is in progress: wait for it or cancel it");
        }
        let stations: Vec<(String, String)> = ids.iter().filter_map(|id| self.data.stations.get(id)).map(|s| (s.id.clone(), label(names, &s.id, s.brew_login.as_deref()))).collect();
        if stations.is_empty() {
            return Err("No station to change");
        }
        self.data.next_bulk += 1;
        let id = self.data.next_bulk;
        let (run, events) = crate::fleet_bulk::Run::new(id, now_ms, user.map(str::to_owned), what, mode, during_call, interrupt_min, stations);
        self.data.bulk = Some(run);
        self.apply_bulk(now_ms, events);
        self.save();
        Ok(id)
    }

    /// Advance the bulk run; returns its turns and whether anything changed.
    pub fn tick_bulk(&mut self, now_ms: u64, dir: &[crate::fleet_bulk::DirCell], _names: &dyn Fn(&str) -> Option<String>) -> (Vec<crate::fleet_bulk::Turn>, bool) {
        let Some(run) = self.data.bulk.as_ref().filter(|r| !r.is_finished()) else { return (Vec::new(), false) };
        let mut run = run.clone();
        let stations = &self.data.stations;
        let look = |id: &str| {
            stations.get(id).map(|s| (s.online, s.capabilities.iter().any(|c| c == crate::fleet_config::CONFIG_CAPABILITY), s.cfg.clone(), s.brew_login.clone()))
        };
        let (turns, events) = run.tick(now_ms, &look, dir);
        let changed = Some(&run) != self.data.bulk.as_ref();
        self.data.bulk = Some(run);
        self.apply_bulk(now_ms, events);
        if changed {
            self.save();
        }
        (turns, changed)
    }

    pub fn bulk_send_result(&mut self, id: &str, result: Result<u32, (String, String)>, now_ms: u64, _names: &dyn Fn(&str) -> Option<String>) {
        let Some(run) = self.data.bulk.as_mut() else { return };
        let events = run.send_result(id, result, now_ms);
        self.apply_bulk(now_ms, events);
        self.save();
    }

    pub fn cancel_bulk(&mut self, user: Option<&str>, now_ms: u64) -> bool {
        let Some(run) = self.data.bulk.as_mut().filter(|r| !r.is_finished()) else { return false };
        let events = run.cancel_all(user, now_ms);
        self.apply_bulk(now_ms, events);
        self.save();
        true
    }

    /// Count cell changes (cell names `from`, `to`).
    pub fn count_moves(&mut self, moves: &[(String, String)], now_ms: u64) {
        for (a, b) in moves {
            crate::fleet_bulk::count_move(&mut self.data.moves, a, b, now_ms);
        }
        self.save();
    }

    pub fn move_map(&self) -> std::collections::HashMap<(String, String), u32> {
        self.data.moves.iter().map(|m| ((m.from.clone(), m.to.clone()), m.count)).collect()
    }
}

/// Capability of phase 2 (remote update).
pub const UPDATE_CAPABILITY: &str = "update-v1";
/// Live update with the server's repository and branch, radio kept on.
pub const LIVE_UPDATE_CAPABILITY: &str = "update-live-v1";

/// Station as the Fleet Manager shows it.
#[derive(Serialize)]
struct StationView {
    #[serde(flatten)]
    station: Station,
    online: bool,
    label: String,
    /// Its last check found a newer version (the Update button is offered).
    updatable: bool,
    /// Its OTA source differs from the server's "Updates repo" (hint only).
    other_source: bool,
    /// Online and can take a live update (`update-live-v1`).
    live: bool,
    /// Config state for the list (the copy itself is asked per station).
    config: serde_json::Value,
}

/// `GET /api/fleet` (Fleet Control visibility): the listed stations, the change
/// log (newest first, 90 days) and whether this login may manage them.
pub async fn api_list(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::Json<serde_json::Value> {
    let a = crate::dashboard::access(&state, &headers);
    let names = crate::control::station_names(&state);
    let lookup = |k: &str| names.get(k).cloned();
    let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    fleet.prune(now_ms());
    let (ref_repo, ref_branch) = fleet.update_settings().effective();
    let stations: Vec<StationView> = fleet
        .stations()
        .into_iter()
        .map(|s| StationView {
            online: s.online,
            label: label(&lookup, &s.id, s.brew_login.as_deref()),
            updatable: fleet.updatable(&s.id),
            live: fleet.live_capable(&s.id),
            other_source: s.check.as_ref().is_some_and(|c| !c.repo.is_empty() && (!same_repo(&c.repo, &ref_repo) || c.branch != ref_branch)),
            config: crate::fleet_config::list_summary(&s.cfg),
            station: Station { cfg: Default::default(), ..s },
        })
        .collect();
    axum::Json(serde_json::json!({
        "offered": state.config.control.enabled,
        "protocol": PROTOCOL,
        "can_manage": a.manages_fleet(),
        "is_admin": a.admin,
        "stations": stations,
        "log": fleet.log(),
        "job": fleet.job(),
        "update_settings": fleet.update_settings(),
        "templates": fleet.templates(),
        "bulk": fleet.bulk(),
        "confirm_secs": fleet.confirm_secs(),
        "alarms": crate::fleet_monitor::list_json(&state, &stations.iter().map(|s| s.station.id.clone()).collect::<Vec<_>>()),
        "update_source": {"repo": ref_repo, "branch": ref_branch},
        "limits": {"build_min": crate::fleet_update::BUILD_LIMIT_MS / 60_000, "return_min": crate::fleet_update::RETURN_LIMIT_MS / 60_000, "max_interrupt_min": crate::fleet_update::MAX_INTERRUPT_MIN},
    }))
}

#[derive(Deserialize)]
pub struct IdRequest {
    pub id: String,
}

fn refuse() -> axum::response::Response {
    use axum::response::IntoResponse;
    (axum::http::StatusCode::FORBIDDEN, "This needs the right Fleet Control management").into_response()
}

/// `POST /api/fleet/remove` `{"id": "<control login>"}` (Fleet Control management).
pub async fn api_remove(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<IdRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let names = crate::control::station_names(&state);
    let removed = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).remove(&req.id, a.user.as_deref(), now_ms(), &|k| names.get(k).cloned());
    if removed {
        axum::Json(serde_json::json!({"ok": true})).into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response()
    }
}

/// `POST /api/fleet/ping` `{"id": "<control login>"}` (Fleet Control management):
/// sends `FleetPing` and returns the station's `CommandResult`.
pub async fn api_ping(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<IdRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !crate::dashboard::access(&state, &headers).manages_fleet() {
        return refuse();
    }
    let started = std::time::Instant::now();
    match crate::control::send_command(&state, &req.id, crate::control::ControlCommand::FleetPing { handle: 0 }).await {
        Ok(Some(crate::control::ControlResponse::CommandResult { ok, code, message, .. })) => {
            axum::Json(serde_json::json!({"ok": ok, "code": code, "message": message, "ms": started.elapsed().as_millis() as u64})).into_response()
        }
        Ok(_) => (axum::http::StatusCode::BAD_GATEWAY, "unexpected answer from the station").into_response(),
        Err(crate::control::SendError::NotConnected) => (axum::http::StatusCode::NOT_FOUND, "The station is not connected").into_response(),
        Err(crate::control::SendError::Timeout) => (axum::http::StatusCode::GATEWAY_TIMEOUT, "No answer from the station within 5 s").into_response(),
    }
}

#[derive(Deserialize, Default)]
pub struct IdsRequest {
    /// Control logins; empty: every station that can do it.
    #[serde(default)]
    pub ids: Vec<String>,
}

/// `POST /api/fleet/check` `{"ids": [...]}` (Fleet Control management): every
/// station (online, `update-v1`; empty list: all of them) runs its update check
/// against its own OTA source. Waits for all answers (up to 60 s).
pub async fn api_check(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<IdsRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !crate::dashboard::access(&state, &headers).manages_fleet() {
        return refuse();
    }
    let ids: Vec<String> = {
        let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        fleet
            .stations()
            .into_iter()
            .filter(|s| s.online && s.capabilities.iter().any(|c| c == UPDATE_CAPABILITY))
            .filter(|s| req.ids.is_empty() || req.ids.contains(&s.id))
            .map(|s| s.id)
            .collect()
    };
    let asks = ids.into_iter().map(|id| {
        let state = state.clone();
        async move {
            let r = crate::control::send_command(&state, &id, crate::control::ControlCommand::CheckUpdate { handle: 0 }).await;
            (id, r)
        }
    });
    let mut out = Vec::new();
    for (id, r) in futures_util::future::join_all(asks).await {
        let check = match r {
            Ok(Some(crate::control::ControlResponse::UpdateInfo { ok, current, latest, update_available, channel, repo, branch, message, .. })) => {
                CheckInfo { at_ms: now_ms(), ok, current, latest, update_available, channel, repo, branch, message }
            }
            Ok(Some(crate::control::ControlResponse::CommandResult { message, .. })) => {
                CheckInfo { at_ms: now_ms(), ok: false, current: String::new(), latest: None, update_available: false, channel: String::new(), repo: String::new(), branch: String::new(), message }
            }
            Ok(_) | Err(_) => CheckInfo {
                at_ms: now_ms(),
                ok: false,
                current: String::new(),
                latest: None,
                update_available: false,
                channel: String::new(),
                repo: String::new(),
                branch: String::new(),
                message: "no answer from the station".into(),
            },
        };
        let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        // Keep the running software: a failed check has no `current`.
        let current = fleet.stations().into_iter().find(|s| s.id == id).map(|s| s.version).unwrap_or_default();
        let check = CheckInfo { current: if check.current.is_empty() { current } else { check.current.clone() }, ..check };
        fleet.set_check(&id, check.clone());
        out.push(serde_json::json!({"id": id, "check": check}));
    }
    axum::Json(serde_json::json!({"results": out})).into_response()
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    /// Control logins; empty: every station whose check found a newer version.
    #[serde(default)]
    pub ids: Vec<String>,
    pub mode: crate::fleet_update::Mode,
    #[serde(default)]
    pub during_call: bool,
    #[serde(default = "default_interrupt")]
    pub interrupt_min: u32,
    /// Live update with the server's "Updates repo", radio kept on.
    #[serde(default)]
    pub live: bool,
}

fn default_interrupt() -> u32 {
    10
}

/// `POST /api/fleet/update` (Fleet Control management): start an update run.
pub async fn api_update(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<UpdateRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if req.interrupt_min > crate::fleet_update::MAX_INTERRUPT_MIN {
        return (axum::http::StatusCode::BAD_REQUEST, "Interrupt time: at most 120 min").into_response();
    }
    let names = crate::control::station_names(&state);
    let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    let live = if req.live {
        let (repo, branch) = fleet.update_settings().effective();
        let l = crate::fleet_update::LiveSource { repo, branch };
        if !l.valid() {
            return (axum::http::StatusCode::BAD_REQUEST, "Live update needs an https:// repository and a plain branch name in Updates repo").into_response();
        }
        Some(l)
    } else {
        None
    };
    let ids: Vec<String> = if req.ids.is_empty() {
        fleet.stations().into_iter().filter(|s| if req.live { fleet.live_capable(&s.id) } else { fleet.updatable(&s.id) }).map(|s| s.id).collect()
    } else {
        req.ids.clone()
    };
    let options = crate::fleet_update::Options { mode: req.mode, during_call: req.during_call, interrupt_min: req.interrupt_min, live };
    match fleet.start_job(&ids, options, a.user.as_deref(), now_ms(), &|k| names.get(k).cloned()) {
        Ok(job) => {
            drop(fleet);
            state.monitor.emit("fleet_changed", serde_json::json!({"job": job}));
            axum::Json(serde_json::json!({"ok": true, "job": job})).into_response()
        }
        Err(e) => (axum::http::StatusCode::CONFLICT, e).into_response(),
    }
}

#[derive(Deserialize, Default)]
pub struct CancelRequest {
    /// One station; absent: the whole run ("Cancel all").
    #[serde(default)]
    pub id: Option<String>,
}

/// `POST /api/fleet/update/cancel` (Fleet Control management): no further
/// starts (the whole run, or one station that has not started yet).
pub async fn api_cancel(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<CancelRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if state.fleet.lock().unwrap_or_else(|e| e.into_inner()).cancel(req.id.as_deref(), a.user.as_deref(), now_ms()) {
        axum::Json(serde_json::json!({"ok": true})).into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "Nothing to cancel").into_response()
    }
}

/// `POST /api/fleet/settings` `{"repo": "...", "branch": "..."}` (admins):
/// the server's "Updates repo".
pub async fn api_settings(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<UpdateSettings>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.admin {
        return (axum::http::StatusCode::FORBIDDEN, "Admins only").into_response();
    }
    let repo = req.repo.trim().to_owned();
    let branch = req.branch.trim().to_owned();
    if !repo.is_empty() && !(repo.starts_with("https://") || repo.starts_with("git@") || repo.starts_with("ssh://")) {
        return (axum::http::StatusCode::BAD_REQUEST, "Repository: an https:// or ssh git URL").into_response();
    }
    if branch.chars().any(|c| c.is_whitespace() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')) || branch.contains("..") {
        return (axum::http::StatusCode::BAD_REQUEST, "Branch: not a valid git branch name").into_response();
    }
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).set_update_settings(UpdateSettings { repo, branch }, a.user.as_deref(), now_ms());
    axum::Json(serde_json::json!({"ok": true})).into_response()
}

/// `POST /api/fleet/log/clear` (admins): empties the change log.
pub async fn api_clear_log(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.admin {
        return (axum::http::StatusCode::FORBIDDEN, "Admins only").into_response();
    }
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).clear_log(a.user.as_deref(), now_ms());
    state.monitor.emit("fleet_changed", serde_json::json!({}));
    axum::Json(serde_json::json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct MaintenanceRequest {
    pub id: String,
    pub on: bool,
    #[serde(default = "default_maintenance")]
    pub minutes: u32,
    #[serde(default)]
    pub reason: String,
}

fn default_maintenance() -> u32 {
    30
}

/// Map a station's `CommandResult` to an HTTP answer.
fn station_answer(r: Result<Option<crate::control::ControlResponse>, crate::control::SendError>) -> Result<(), axum::response::Response> {
    use axum::response::IntoResponse;
    match r {
        Ok(Some(crate::control::ControlResponse::CommandResult { ok: true, .. })) => Ok(()),
        Ok(Some(crate::control::ControlResponse::CommandResult { code, message, .. })) => Err((axum::http::StatusCode::CONFLICT, format!("{code}: {message}")).into_response()),
        Ok(_) => Err((axum::http::StatusCode::BAD_GATEWAY, "unexpected answer from the station").into_response()),
        Err(crate::control::SendError::NotConnected) => Err((axum::http::StatusCode::NOT_FOUND, "The station is not connected").into_response()),
        Err(crate::control::SendError::Timeout) => Err((axum::http::StatusCode::GATEWAY_TIMEOUT, "No answer from the station within 5 s").into_response()),
    }
}

fn has_capability(state: &crate::state::AppState, id: &str, cap: &str) -> bool {
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).stations().iter().any(|s| s.id == id && s.capabilities.iter().any(|c| c == cap))
}

/// `POST /api/fleet/maintenance` `{"id", "on", "minutes", "reason"}` (Fleet
/// Control management): the red maintenance banner on the station's dashboard.
pub async fn api_maintenance(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<MaintenanceRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if !has_capability(&state, &req.id, MAINTENANCE_CAPABILITY) {
        return (axum::http::StatusCode::BAD_REQUEST, "This station cannot show maintenance (needs maintenance-v1)").into_response();
    }
    let minutes = req.minutes.clamp(1, MAX_MAINTENANCE_MIN);
    let cmd = crate::control::ControlCommand::SetMaintenance { handle: 0, on: req.on, minutes, by: a.user.clone().unwrap_or_default(), reason: req.reason.trim().chars().take(120).collect() };
    if let Err(r) = station_answer(crate::control::send_command(&state, &req.id, cmd).await) {
        return r;
    }
    let names = crate::control::station_names(&state);
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).set_maintenance(&req.id, req.on, minutes, &req.reason, a.user.as_deref(), now_ms(), &|k| names.get(k).cloned());
    state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
    axum::Json(serde_json::json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct RestartRequest {
    pub id: String,
    /// true: reboot the host; false: restart the service.
    #[serde(default)]
    pub host: bool,
    #[serde(default)]
    pub during_call: bool,
}

/// `POST /api/fleet/restart` `{"id", "host", "during_call"}` (Fleet Control
/// management): restart the station's service or reboot its host.
pub async fn api_restart(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<RestartRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if !has_capability(&state, &req.id, RESTART_CAPABILITY) {
        return (axum::http::StatusCode::BAD_REQUEST, "This station cannot be restarted from here (needs restart-v1)").into_response();
    }
    let cmd = crate::control::ControlCommand::RestartStation { handle: 0, host: req.host, during_call: req.during_call };
    if let Err(r) = station_answer(crate::control::send_command(&state, &req.id, cmd).await) {
        return r;
    }
    let names = crate::control::station_names(&state);
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).restart_sent(&req.id, req.host, a.user.as_deref(), now_ms(), &|k| names.get(k).cloned());
    state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
    axum::Json(serde_json::json!({"ok": true})).into_response()
}

/// Background driver of update runs: every 2 s advance the run and send the
/// `StartUpdate` commands it asks for. Runs for the whole server lifetime, so
/// a run goes on with no browser open.
pub async fn run_updates(state: std::sync::Arc<crate::state::AppState>) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        every.tick().await;
        {
            let names = crate::control::station_names(&state);
            state.fleet.lock().unwrap_or_else(|e| e.into_inner()).tick_stations(now_ms(), &|k| names.get(k).cloned());
        }
        // Config changes (phase 5): confirm, alarm, pending changes, edit pages.
        crate::fleet_config::tick(&state).await;
        // Bulk config changes and counted cell changes (phase 6).
        crate::fleet_bulk::tick(&state).await;
        let starts = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).tick_job(now_ms());
        for s in starts {
            let state = state.clone();
            tokio::spawn(async move {
                let cmd = crate::control::ControlCommand::StartUpdate { handle: 0, during_call: s.during_call, source: s.source.clone() };
                let outcome = match crate::control::send_command(&state, &s.id, cmd).await {
                    Ok(Some(crate::control::ControlResponse::CommandResult { ok: true, .. })) => crate::fleet_update::StartOutcome::Started,
                    Ok(Some(crate::control::ControlResponse::CommandResult { code, message, .. })) => match code.as_str() {
                        "busy" => crate::fleet_update::StartOutcome::Started,
                        "call_active" => crate::fleet_update::StartOutcome::CallActive,
                        _ => crate::fleet_update::StartOutcome::Refused(code, message),
                    },
                    Ok(_) => crate::fleet_update::StartOutcome::Refused("unexpected".into(), "unexpected answer".into()),
                    Err(_) => crate::fleet_update::StartOutcome::Unreachable,
                };
                state.fleet.lock().unwrap_or_else(|e| e.into_inner()).start_result(&s.id, outcome, now_ms());
                state.monitor.emit("fleet_changed", serde_json::json!({"id": s.id}));
            });
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(on: bool, version: &str) -> Hello {
        Hello { protocol: 1, version: version.into(), brew_login: Some("1000001".into()), mcc: 232, mnc: 20, fleet_control: on, capabilities: if on { vec!["fleet-v1".into()] } else { vec![] } }
    }
    fn no_names(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_station_without_fleet_control_is_never_listed() {
        let mut f = Fleet::load(None, 0);
        assert!(!f.hello("bs1", &hello(false, "v1"), 1, &no_names));
        assert!(f.stations().is_empty() && f.log().is_empty());
    }

    #[test]
    fn switched_on_listed_offline_kept_switched_off_removed_with_info() {
        let mut f = Fleet::load(None, 0);
        assert!(f.hello("bs1", &hello(true, "v1"), 10, &no_names));
        assert_eq!(f.stations().len(), 1);
        f.set_online("bs1", false, 20);
        let s = f.get("bs1").unwrap();
        assert!(!s.online && s.last_seen_ms == 20, "offline stations stay listed");
        // Next login with the switch off: removed, INFO in the log.
        assert!(f.hello("bs1", &hello(false, "v1"), 30, &no_names));
        assert!(f.stations().is_empty());
        let log = f.log();
        assert_eq!(log[0].kind, "fleet_off");
        assert_eq!(log[0].text, "Basestation 1000001 switched off Fleet Control");
        assert_eq!(log[1].kind, "fleet_on");
    }

    #[test]
    fn name_from_the_server_and_version_change_logged() {
        let names = |k: &str| (k == "1000001").then(|| "Wien Nord".to_string());
        let mut f = Fleet::load(None, 0);
        f.hello("bs1", &hello(true, "v0.5.9-aaa"), 1, &names);
        f.hello("bs1", &hello(true, "v0.5.9-aaa"), 2, &names);
        f.hello("bs1", &hello(true, "v0.6.0-bbb"), 3, &names);
        let log = f.log();
        assert_eq!(log.len(), 2, "a reconnect with the same version is not a change");
        assert_eq!(log[0].text, "Basestation Wien Nord: software v0.5.9-aaa → v0.6.0-bbb");
        assert_eq!(f.get("bs1").unwrap().first_seen_ms, 1);
    }

    #[test]
    fn removed_by_a_user() {
        let mut f = Fleet::load(None, 0);
        f.hello("bs1", &hello(true, "v1"), 1, &no_names);
        assert!(f.remove("bs1", Some("alice"), 2, &no_names));
        assert!(!f.remove("bs1", Some("alice"), 3, &no_names));
        let e = &f.log()[0];
        assert_eq!((e.kind.as_str(), e.user.as_deref()), ("removed", Some("alice")));
    }

    #[test]
    fn log_keeps_ninety_days_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("fleet-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = Fleet::path_for(&dir.join("history.bin"));
        let day = 24 * 3600 * 1000;
        let mut f = Fleet::load(Some(path.clone()), 0);
        f.hello("old", &hello(true, "v1"), day, &no_names);
        f.hello("bs1", &hello(true, "v1"), 50 * day, &no_names);
        f.set_online("bs1", false, 50 * day);
        let g = Fleet::load(Some(path.clone()), 92 * day);
        assert_eq!(g.log().len(), 1, "the entry from day 1 is older than 90 days");
        assert_eq!(g.stations().len(), 2, "stations stay until they switch off or are removed");
        assert!(g.stations().iter().all(|s| !s.online), "after a restart nobody is online yet");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn repositories_compared_loosely() {
        assert!(same_repo("https://github.com/Aitorrio/bost-flowstation", "https://github.com/aitorrio/bost-flowstation.git/"));
        assert!(same_repo("https://user:token@github.com/a/b.git", "https://github.com/a/b"));
        assert!(!same_repo("https://github.com/sebastianmadl/bost-flowstation", DEFAULT_UPDATE_REPO));
        let empty = UpdateSettings::default();
        assert_eq!(empty.effective(), (DEFAULT_UPDATE_REPO.to_string(), "main".to_string()));
        let fork = UpdateSettings { repo: " https://github.com/sebastianmadl/bost-flowstation ".into(), branch: "handover".into() };
        assert_eq!(fork.effective().1, "handover");
    }

    fn check(current: &str, newer: bool) -> CheckInfo {
        CheckInfo { at_ms: 1, ok: true, current: current.into(), latest: Some("v2".into()), update_available: newer, channel: "installed".into(), repo: DEFAULT_UPDATE_REPO.into(), branch: "main".into(), message: String::new() }
    }

    #[test]
    fn updatable_only_after_a_check_for_the_running_software() {
        let mut f = Fleet::load(None, 0);
        let mut h = hello(true, "v1");
        h.capabilities.push(UPDATE_CAPABILITY.into());
        f.hello("bs1", &h, 1, &no_names);
        assert!(!f.updatable("bs1"), "no check yet");
        f.set_check("bs1", check("v1", false));
        assert!(!f.updatable("bs1"), "up to date");
        f.set_check("bs1", check("v1", true));
        assert!(f.updatable("bs1"));
        f.set_online("bs1", false, 2);
        assert!(!f.updatable("bs1"), "offline");
        let mut h2 = hello(true, "v2");
        h2.capabilities.push(UPDATE_CAPABILITY.into());
        f.hello("bs1", &h2, 3, &no_names);
        assert!(f.get("bs1").unwrap().check.is_none(), "a check for other software is dropped");
        // Without update-v1 (older station) never.
        f.hello("bs2", &hello(true, "v1"), 4, &no_names);
        f.set_check("bs2", check("v1", true));
        assert!(!f.updatable("bs2"));
    }

    #[test]
    fn a_run_cannot_start_twice_and_the_job_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("fleet-job-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = Fleet::path_for(&dir.join("history.bin"));
        let mut f = Fleet::load(Some(path.clone()), 0);
        let mut h = hello(true, "v1");
        h.capabilities.push(UPDATE_CAPABILITY.into());
        f.hello("bs1", &h, 1, &no_names);
        f.set_check("bs1", check("v1", true));
        let o = crate::fleet_update::Options { mode: crate::fleet_update::Mode::Rolling, during_call: false, interrupt_min: 10, live: None };
        assert!(f.start_job(&["bs1".into()], o.clone(), Some("alice"), 2, &no_names).is_ok());
        assert!(f.start_job(&["bs1".into()], o, Some("alice"), 3, &no_names).is_err());
        let starts = f.tick_job(4);
        assert_eq!(starts.len(), 1);
        let g = Fleet::load(Some(path), 5);
        let job = g.job().unwrap();
        assert!(!job.is_finished());
        assert_eq!(job.items[0].state, crate::fleet_update::ItemState::Pending, "the unanswered start is tried again after the restart");
        assert!(g.log().iter().any(|e| e.text.starts_with("Update run started (rolling")));
        std::fs::remove_dir_all(&dir).ok();
    }


    #[test]
    fn live_update_needs_no_check_but_the_live_capability() {
        let mut f = Fleet::load(None, 0);
        let mut h = hello(true, "v1");
        h.capabilities.push(UPDATE_CAPABILITY.into());
        f.hello("old", &h, 1, &no_names);
        h.capabilities.push(LIVE_UPDATE_CAPABILITY.into());
        f.hello("bs1", &h, 1, &no_names);
        assert!(f.live_capable("bs1") && !f.live_capable("old"));
        let live = crate::fleet_update::LiveSource { repo: "https://github.com/sebastianmadl/bost-flowstation".into(), branch: "fleet-control".into() };
        let o = crate::fleet_update::Options { mode: crate::fleet_update::Mode::Rolling, during_call: false, interrupt_min: 10, live: Some(live.clone()) };
        f.start_job(&["bs1".into(), "old".into()], o, Some("oe4sxm"), 2, &no_names).unwrap();
        let job = f.job().unwrap();
        assert_eq!(job.items.len(), 1, "a station without update-live-v1 is left out");
        assert_eq!(job.items[0].state, crate::fleet_update::ItemState::Pending, "no check needed");
        let starts = f.tick_job(3);
        assert_eq!(starts[0].source, Some(live));
    }

    #[test]
    fn clear_log_leaves_one_entry_saying_who() {
        let mut f = Fleet::load(None, 0);
        f.hello("bs1", &hello(true, "v1"), 1, &no_names);
        f.hello("bs1", &hello(true, "v2"), 2, &no_names);
        assert!(f.log().len() >= 2);
        f.clear_log(Some("admin"), 3);
        let log = f.log();
        assert_eq!(log.len(), 1);
        assert_eq!((log[0].kind.as_str(), log[0].text.as_str(), log[0].user.as_deref()), ("log_clear", "Log cleared by admin", Some("admin")));
    }

    fn hello_with(caps: &[&str], version: &str) -> Hello {
        let mut h = hello(true, version);
        h.capabilities = caps.iter().map(|c| c.to_string()).collect();
        h
    }

    #[test]
    fn maintenance_is_logged_resent_after_a_restart_and_runs_out() {
        let mut f = Fleet::load(None, 0);
        f.hello("bs1", &hello_with(&["fleet-v1", MAINTENANCE_CAPABILITY], "v1"), 1, &no_names);
        assert!(f.set_maintenance("bs1", true, 30, "new carrier", Some("oe4sxm"), 1000, &no_names));
        assert_eq!(f.log()[0].text, "Basestation 1000001: maintenance on for 30 min (new carrier)");
        assert_eq!(f.log()[0].user.as_deref(), Some("oe4sxm"));
        // The station restarts after 10 min: the flag is sent again with the time left.
        f.set_online("bs1", false, 600_000);
        f.hello("bs1", &hello_with(&["fleet-v1", MAINTENANCE_CAPABILITY], "v1"), 601_000, &no_names);
        match f.maintenance_resend("bs1", 601_000) {
            Some(crate::control::ControlCommand::SetMaintenance { on: true, minutes, by, reason, .. }) => {
                assert_eq!((minutes, by.as_str(), reason.as_str()), (20, "oe4sxm", "new carrier"));
            }
            other => panic!("expected SetMaintenance, got {other:?}"),
        }
        assert!(!f.tick_stations(1_000_000, &no_names), "still on");
        assert!(f.tick_stations(1000 + 30 * 60_000, &no_names));
        assert_eq!(f.log()[0].text, "Basestation 1000001: maintenance ended (time up)");
        assert!(f.maintenance_resend("bs1", 1000 + 30 * 60_000).is_none());
        assert!(!f.set_maintenance("nobody", true, 5, "", None, 1, &no_names));
    }

    #[test]
    fn no_resend_to_a_station_without_the_capability() {
        let mut f = Fleet::load(None, 0);
        f.hello("bs1", &hello_with(&["fleet-v1", MAINTENANCE_CAPABILITY], "v1"), 1, &no_names);
        f.set_maintenance("bs1", true, 30, "", None, 2, &no_names);
        f.hello("bs1", &hello_with(&["fleet-v1"], "v0"), 3, &no_names);
        assert!(f.maintenance_resend("bs1", 4).is_none());
    }

    #[test]
    fn restart_return_is_logged_or_raises_an_alarm() {
        let mut f = Fleet::load(None, 0);
        f.hello("bs1", &hello_with(&["fleet-v1", RESTART_CAPABILITY], "v1"), 1, &no_names);
        f.hello("bs2", &hello_with(&["fleet-v1", RESTART_CAPABILITY], "v1"), 1, &no_names);
        f.restart_sent("bs1", true, Some("admin"), 10_000, &no_names);
        f.restart_sent("bs2", false, None, 10_000, &no_names);
        assert_eq!(f.log()[0].text, "Basestation 1000001: service restart");
        f.set_online("bs1", false, 15_000);
        f.hello("bs1", &hello_with(&["fleet-v1", RESTART_CAPABILITY], "v1"), 100_000, &no_names);
        assert_eq!(f.log()[0].text, "Basestation 1000001: back after the reboot (90 s)");
        assert!(f.get("bs1").unwrap().restart.is_none());
        assert!(f.tick_stations(10_000 + RESTART_RETURN_MS + 1, &no_names));
        assert_eq!(f.log()[0].text, "ALARM Basestation 1000001: not back 10 min after the service restart");
        assert_eq!(f.log()[0].kind, "restart_alarm");
        assert!(!f.tick_stations(10_000 + RESTART_RETURN_MS + 2, &no_names), "alarm only once");
    }
}
