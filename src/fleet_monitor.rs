//! Fleet Control phase 7: graphs and alarms per station.
//!
//! Stations with `metrics-v1` send `FleetMetrics` once a minute over the
//! control channel (CPU load, CPU temperature, SDR temperature and RF state per
//! cell, registered radios, calls, stack health). Older stations are sampled
//! from their telemetry (temperatures, radios, calls; no CPU load). The server
//! pings every online station every few minutes (`FleetPing`) for the ping
//! graph. Everything is folded into one point per sample interval and kept
//! for the configured time (ring buffer: the oldest points are overwritten).
//! While a station is unreachable no points are written, so its graph shows a
//! gap there.
//!
//! Alarms: rules (Server settings → Fleet Control alarms, admins) for
//! "unreachable", "SDR error", "CPU temperature", "SDR temperature", "CPU
//! load" and "ping", each with on/off, severity (ALARM / warning), threshold
//! and minutes. A condition alarm stays active until its cause is gone; an
//! ALARM from the change log (update, restart, config) stays until someone
//! acknowledges it. Acknowledging marks an alarm with the user; finished
//! alarms go to the history (90 days).
//!
//! Files next to the history log: `<storage>.fleet-alarms` (settings, alarms;
//! written on every change) and `<storage>.fleet-metrics` (graphs; written
//! every 5 minutes). Protocol and behaviour: `docs/Fleet-Control.md` section 12.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::{Path, PathBuf},
};

/// Capability: the station sends `FleetMetrics`.
pub const METRICS_CAPABILITY: &str = "metrics-v1";
/// Finished alarms are kept this long.
pub const HISTORY_KEEP_MS: u64 = 90 * 24 * 3600 * 1000;
/// Most points kept per station (7 days at one point per minute).
pub const MAX_POINTS: u64 = 10_080;
/// A metrics report older than this is not used for the alarms.
const SAMPLE_FRESH_MS: u64 = 180_000;
/// How often the graphs are written to disk.
const METRICS_SAVE_MS: u64 = 5 * 60_000;
/// Finished alarms the Fleet Control page gets with the list.
pub const HISTORY_SHOWN: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Alarm,
    Warning,
}

/// One alarm rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub enabled: bool,
    pub severity: Severity,
    /// °C, percent or missed pings, depending on the rule (unused for some).
    #[serde(default)]
    pub threshold: f32,
    /// How long the condition must hold before the alarm comes (0: at once).
    #[serde(default)]
    pub minutes: u32,
}

const fn rule(severity: Severity, threshold: f32, minutes: u32) -> Rule {
    Rule { enabled: true, severity, threshold, minutes }
}

/// The alarm rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    /// Station not connected for `minutes`.
    #[serde(default = "d_offline")]
    pub offline: Rule,
    /// A cell's SDR could not be opened, or the stack health is critical.
    #[serde(default = "d_sdr_error")]
    pub sdr_error: Rule,
    /// CPU temperature at or above `threshold` °C.
    #[serde(default = "d_cpu_temp")]
    pub cpu_temp: Rule,
    /// SDR temperature (any cell) at or above `threshold` °C.
    #[serde(default = "d_sdr_temp")]
    pub sdr_temp: Rule,
    /// CPU load at or above `threshold` % for `minutes`.
    #[serde(default = "d_cpu_load")]
    pub cpu_load: Rule,
    /// `threshold` pings in a row without an answer while connected.
    #[serde(default = "d_ping")]
    pub ping: Rule,
    /// ALARM entries of the change log (update, restart, config).
    #[serde(default = "d_events")]
    pub events: Rule,
}

fn d_offline() -> Rule {
    rule(Severity::Alarm, 0.0, 2)
}
fn d_sdr_error() -> Rule {
    rule(Severity::Alarm, 0.0, 0)
}
fn d_cpu_temp() -> Rule {
    rule(Severity::Alarm, 80.0, 0)
}
fn d_sdr_temp() -> Rule {
    rule(Severity::Alarm, 70.0, 0)
}
fn d_cpu_load() -> Rule {
    rule(Severity::Warning, 90.0, 5)
}
fn d_ping() -> Rule {
    rule(Severity::Warning, 3.0, 0)
}
fn d_events() -> Rule {
    rule(Severity::Alarm, 0.0, 0)
}

impl Default for Rules {
    fn default() -> Self {
        Self { offline: d_offline(), sdr_error: d_sdr_error(), cpu_temp: d_cpu_temp(), sdr_temp: d_sdr_temp(), cpu_load: d_cpu_load(), ping: d_ping(), events: d_events() }
    }
}

impl Rules {
    fn get(&self, kind: &str) -> &Rule {
        match kind {
            "offline" => &self.offline,
            "sdr_error" => &self.sdr_error,
            "cpu_temp" => &self.cpu_temp,
            "sdr_temp" => &self.sdr_temp,
            "cpu_load" => &self.cpu_load,
            "ping" => &self.ping,
            _ => &self.events,
        }
    }
}

/// Server settings for graphs, ping and alarms (Server settings, admins).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// One graph point per this many minutes (1–60).
    #[serde(default = "d_sample")]
    pub sample_min: u32,
    /// Graphs are kept this many hours, then overwritten (1–720).
    #[serde(default = "d_keep")]
    pub keep_hours: u32,
    /// The server pings every online station this often (minutes, 0: off).
    #[serde(default = "d_ping_min")]
    pub ping_min: u32,
    /// Optional graph lines.
    #[serde(default)]
    pub show_radios: bool,
    #[serde(default)]
    pub show_calls: bool,
    #[serde(default)]
    pub rules: Rules,
}

fn d_sample() -> u32 {
    1
}
fn d_keep() -> u32 {
    24
}
fn d_ping_min() -> u32 {
    5
}

impl Default for Settings {
    fn default() -> Self {
        Self { sample_min: d_sample(), keep_hours: d_keep(), ping_min: d_ping_min(), show_radios: false, show_calls: false, rules: Rules::default() }
    }
}

impl Settings {
    pub fn sample_ms(&self) -> u64 {
        self.sample_min.max(1) as u64 * 60_000
    }
    pub fn keep_ms(&self) -> u64 {
        self.keep_hours.max(1) as u64 * 3_600_000
    }

    /// Checks every value; the text says what is wrong.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=60).contains(&self.sample_min) {
            return Err("Sample interval: 1 to 60 min".into());
        }
        if !(1..=720).contains(&self.keep_hours) {
            return Err("Keep graphs: 1 to 720 h (30 days)".into());
        }
        if self.keep_hours as u64 * 60 / self.sample_min as u64 > MAX_POINTS {
            return Err(format!("Too many points per station ({} max): keep the graphs shorter or raise the sample interval", MAX_POINTS));
        }
        if self.ping_min > 60 {
            return Err("Ping interval: 0 (off) to 60 min".into());
        }
        let r = &self.rules;
        let range = |name: &str, rule: &Rule, lo: f32, hi: f32, unit: &str| -> Result<(), String> {
            if !(lo..=hi).contains(&rule.threshold) {
                return Err(format!("{name}: {lo} to {hi} {unit}"));
            }
            Ok(())
        };
        range("CPU temperature", &r.cpu_temp, 30.0, 120.0, "°C")?;
        range("SDR temperature", &r.sdr_temp, 20.0, 120.0, "°C")?;
        range("CPU load", &r.cpu_load, 10.0, 100.0, "%")?;
        range("Ping", &r.ping, 1.0, 20.0, "missed pings")?;
        if !(1..=60).contains(&r.offline.minutes) {
            return Err("Unreachable: 1 to 60 min".into());
        }
        for (name, rule) in [("SDR error", &r.sdr_error), ("CPU temperature", &r.cpu_temp), ("SDR temperature", &r.sdr_temp), ("CPU load", &r.cpu_load)] {
            if rule.minutes > 60 {
                return Err(format!("{name}: 0 to 60 min"));
            }
        }
        Ok(())
    }
}

/// One metrics report (from `FleetMetrics` or from telemetry).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub cpu_pct: Option<f32>,
    pub cpu_temp_c: Option<f32>,
    /// SDR temperature per cell.
    pub sdr: BTreeMap<u8, f32>,
    /// Cells whose SDR is in error, with the text.
    pub rf_errors: Vec<(u8, String)>,
    pub radios: Option<u32>,
    pub calls: Option<u32>,
    /// `ok`, `degraded`, `critical` (`None`: not known).
    pub health: Option<String>,
}

/// One graph point (short keys: the file holds thousands of them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Start of the interval (ms).
    pub t: u64,
    /// CPU load, mean (%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<f32>,
    /// CPU temperature, highest (°C).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ct: Option<f32>,
    /// SDR temperature per cell, highest (°C).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub s: BTreeMap<u8, f32>,
    /// Registered radios, highest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r: Option<u32>,
    /// Calls in progress, highest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k: Option<u32>,
}

/// One server ping: round trip in ms, `None` when unanswered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ping {
    pub t: u64,
    pub ms: Option<u32>,
}

/// The point being collected.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Acc {
    t: u64,
    cpu_sum: f32,
    cpu_n: u32,
    point: Option<Point>,
}

impl Acc {
    fn add(&mut self, s: &Sample) {
        let p = self.point.get_or_insert_with(|| Point { t: self.t, c: None, ct: None, s: BTreeMap::new(), r: None, k: None });
        if let Some(c) = s.cpu_pct {
            self.cpu_sum += c;
            self.cpu_n += 1;
            p.c = Some(round1(self.cpu_sum / self.cpu_n as f32));
        }
        p.ct = max_opt(p.ct, s.cpu_temp_c.map(round1));
        for (cell, t) in &s.sdr {
            let e = p.s.entry(*cell).or_insert(round1(*t));
            *e = e.max(round1(*t));
        }
        p.r = s.radios.max(p.r);
        p.k = s.calls.max(p.k);
    }
}

fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}
fn max_opt(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Graph data of one station.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub points: VecDeque<Point>,
    pub pings: VecDeque<Ping>,
    #[serde(default)]
    acc: Option<Acc>,
}

/// One alarm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: u64,
    /// Control login of the station.
    pub station: String,
    /// Name or number of the station.
    pub label: String,
    /// `offline`, `sdr_error`, `cpu_temp`, `sdr_temp`, `cpu_load`, `ping` or the
    /// change log kind (`update_alarm`, `restart_alarm`, `config_alarm`).
    pub kind: String,
    /// Cell of a per-cell alarm (SDR temperature).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<u8>,
    pub severity: Severity,
    pub text: String,
    pub raised_ms: u64,
    /// Finished (cause gone, or acknowledged for a change log alarm).
    #[serde(default)]
    pub cleared_ms: Option<u64>,
    #[serde(default)]
    pub clear_text: Option<String>,
    #[serde(default)]
    pub acked_by: Option<String>,
    #[serde(default)]
    pub acked_ms: Option<u64>,
}

impl Alarm {
    pub fn active(&self) -> bool {
        self.cleared_ms.is_none()
    }
    /// A change log alarm: no condition, finished by acknowledging.
    pub fn event(&self) -> bool {
        self.kind.ends_with("_alarm")
    }
}

#[derive(Default, Serialize, Deserialize)]
struct AlarmFile {
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    alarms: VecDeque<Alarm>,
    #[serde(default)]
    next_id: u64,
    /// Change log entries up to this id were looked at (`None`: first start,
    /// older ALARMs are not raised again).
    #[serde(default)]
    last_log_id: Option<u64>,
}

/// What the monitor needs to know about a listed station.
#[derive(Debug, Clone)]
pub struct StationInfo {
    pub id: String,
    pub label: String,
    pub online: bool,
    pub last_seen_ms: u64,
    /// An OTA update runs (or just finished): no CPU load alarm, the build takes the CPU.
    pub updating: bool,
}

/// A running OTA step (download, build, install) counts this long after its last report.
const UPDATE_RUNNING_MS: u64 = 30 * 60_000;
/// After the update ended (restart, failed, up to date) the CPU load alarm waits this long.
const UPDATE_GRACE_MS: u64 = 5 * 60_000;

/// True while an OTA update of the station runs, from its last `UpdateProgress`
/// (also for updates started on the station's own dashboard), and a few minutes after.
pub fn update_running(progress: Option<&crate::fleet::ProgressInfo>, now_ms: u64) -> bool {
    let Some(p) = progress else { return false };
    let age = now_ms.saturating_sub(p.at_ms);
    match p.stage.as_str() {
        "restart" | "failed" | "up_to_date" => age < UPDATE_GRACE_MS,
        _ => age < UPDATE_RUNNING_MS,
    }
}

/// Live state of a station (not kept over a restart).
#[derive(Debug, Default)]
struct Live {
    last: Option<(u64, Sample)>,
    /// Since when a condition holds, per (kind, cell).
    since: HashMap<(&'static str, Option<u8>), u64>,
    ping_misses: u32,
}

/// Graphs and alarms.
pub struct FleetMonitor {
    alarm_path: Option<PathBuf>,
    metrics_path: Option<PathBuf>,
    data: AlarmFile,
    series: BTreeMap<String, Series>,
    live: HashMap<String, Live>,
    /// Server start: an offline time is counted from here at the earliest.
    started_ms: u64,
    metrics_saved_ms: u64,
}

impl FleetMonitor {
    /// Loads both files (missing or broken: empty). `None`: memory only.
    pub fn load(storage: Option<&Path>, now_ms: u64) -> Self {
        let with = |ext: &str| {
            storage.map(|p| {
                let mut s = p.as_os_str().to_owned();
                s.push(ext);
                PathBuf::from(s)
            })
        };
        let alarm_path = with(".fleet-alarms");
        let metrics_path = with(".fleet-metrics");
        let data = alarm_path.as_deref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<AlarmFile>(&b).ok()).unwrap_or_default();
        let series = metrics_path.as_deref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { alarm_path, metrics_path, data, series, live: HashMap::new(), started_ms: now_ms, metrics_saved_ms: now_ms }
    }

    pub fn settings(&self) -> &Settings {
        &self.data.settings
    }

    /// Admin changes the settings. A rule switched off finishes its alarms.
    pub fn set_settings(&mut self, s: Settings, user: Option<&str>, now_ms: u64) -> Result<(), String> {
        s.validate()?;
        let who = user.unwrap_or("an admin").to_owned();
        for a in self.data.alarms.iter_mut().filter(|a| a.active()) {
            let kind = if a.event() { "events" } else { a.kind.as_str() };
            if !s.rules.get(kind).enabled {
                a.cleared_ms = Some(now_ms);
                a.clear_text = Some(format!("rule switched off by {who}"));
            }
        }
        self.data.settings = s;
        let keep = self.data.settings.keep_ms();
        for series in self.series.values_mut() {
            trim(series, now_ms, keep);
        }
        self.save_alarms();
        Ok(())
    }

    /// A metrics report of station `id`.
    pub fn record(&mut self, id: &str, sample: Sample, now_ms: u64) {
        let step = self.data.settings.sample_ms();
        let keep = self.data.settings.keep_ms();
        let series = self.series.entry(id.to_owned()).or_default();
        let bucket = now_ms - now_ms % step;
        if series.acc.as_ref().is_some_and(|a| a.t != bucket) {
            flush(series);
        }
        series.acc.get_or_insert_with(|| Acc { t: bucket, ..Default::default() }).add(&sample);
        trim(series, now_ms, keep);
        self.live.entry(id.to_owned()).or_default().last = Some((now_ms, sample));
    }

    /// A server ping of station `id` (`None`: no answer).
    pub fn record_ping(&mut self, id: &str, ms: Option<u32>, now_ms: u64) {
        let keep = self.data.settings.keep_ms();
        let series = self.series.entry(id.to_owned()).or_default();
        series.pings.push_back(Ping { t: now_ms, ms });
        trim(series, now_ms, keep);
        let live = self.live.entry(id.to_owned()).or_default();
        live.ping_misses = if ms.is_some() { 0 } else { live.ping_misses + 1 };
    }

    /// The control channel of `id` went down: no metrics, no missed pings any more.
    pub fn disconnected(&mut self, id: &str) {
        if let Some(l) = self.live.get_mut(id) {
            l.last = None;
            l.ping_misses = 0;
            l.since.clear();
        }
    }

    /// Stations that have reported metrics themselves recently (no telemetry sampling).
    pub fn reports_itself(&self, id: &str, now_ms: u64) -> bool {
        self.live.get(id).and_then(|l| l.last.as_ref()).is_some_and(|(at, _)| now_ms.saturating_sub(*at) < SAMPLE_FRESH_MS)
    }

    fn raise(&mut self, st: &StationInfo, kind: &str, cell: Option<u8>, severity: Severity, text: String, now_ms: u64) -> bool {
        if self.data.alarms.iter().any(|a| a.active() && a.station == st.id && a.kind == kind && a.cell == cell) {
            return false;
        }
        self.data.next_id += 1;
        tracing::warn!(station = %st.id, kind, "Fleet Control alarm: {text}");
        self.data.alarms.push_back(Alarm {
            id: self.data.next_id,
            station: st.id.clone(),
            label: st.label.clone(),
            kind: kind.to_owned(),
            cell,
            severity,
            text,
            raised_ms: now_ms,
            cleared_ms: None,
            clear_text: None,
            acked_by: None,
            acked_ms: None,
        });
        true
    }

    fn clear(&mut self, station: &str, kind: &str, cell: Option<u8>, text: &str, now_ms: u64) -> bool {
        let mut changed = false;
        for a in self.data.alarms.iter_mut().filter(|a| a.active() && a.station == station && a.kind == kind && a.cell == cell) {
            a.cleared_ms = Some(now_ms);
            a.clear_text = Some(text.to_owned());
            changed = true;
        }
        changed
    }

    /// Raise or clear one condition alarm. `on`: the condition holds; `off`:
    /// it is clearly gone (hysteresis); neither: leave it as it is.
    #[allow(clippy::too_many_arguments)]
    fn condition(&mut self, st: &StationInfo, kind: &'static str, cell: Option<u8>, on: bool, off: bool, text: impl FnOnce() -> String, clear_text: &str, now_ms: u64) -> bool {
        self.condition_since(st, kind, cell, on, off, None, text, clear_text, now_ms)
    }

    /// [`Self::condition`] with the start of the condition given (`None`: first seen now).
    #[allow(clippy::too_many_arguments)]
    fn condition_since(&mut self, st: &StationInfo, kind: &'static str, cell: Option<u8>, on: bool, off: bool, start: Option<u64>, text: impl FnOnce() -> String, clear_text: &str, now_ms: u64) -> bool {
        let rule = self.data.settings.rules.get(kind).clone();
        let live = self.live.entry(st.id.clone()).or_default();
        if !rule.enabled || off {
            live.since.remove(&(kind, cell));
            return rule.enabled && self.clear(&st.id, kind, cell, clear_text, now_ms);
        }
        if !on {
            return false;
        }
        let since = start.unwrap_or_else(|| *live.since.entry((kind, cell)).or_insert(now_ms));
        if now_ms.saturating_sub(since) >= rule.minutes as u64 * 60_000 {
            return self.raise(st, kind, cell, rule.severity, text(), now_ms);
        }
        false
    }

    /// Looks at every listed station and the new change log entries; raises and
    /// finishes alarms. Returns true when the alarm list changed.
    pub fn evaluate(&mut self, stations: &[StationInfo], new_log: &[crate::fleet::LogEntry], now_ms: u64) -> bool {
        let mut changed = false;
        let step = self.data.settings.sample_ms();
        // Finish points whose interval is over.
        for series in self.series.values_mut() {
            if series.acc.as_ref().is_some_and(|a| a.t + step <= now_ms) {
                flush(series);
            }
        }
        // ALARMs from the change log.
        let first_start = self.data.last_log_id.is_none();
        let seen = self.data.last_log_id.unwrap_or(0);
        if let Some(max) = new_log.iter().map(|e| e.id).max() {
            if !first_start {
                let rule = self.data.settings.rules.events.clone();
                for e in new_log.iter().filter(|e| e.id > seen && e.kind.ends_with("_alarm")) {
                    if rule.enabled {
                        let st = StationInfo { id: e.station.clone(), label: e.label.clone(), online: true, last_seen_ms: now_ms, updating: false };
                        let text = e.text.trim_start_matches("ALARM ").to_owned();
                        // Every change log ALARM is its own alarm (two failed updates are two).
                        self.data.next_id += 1;
                        self.data.alarms.push_back(Alarm {
                            id: self.data.next_id,
                            station: st.id,
                            label: st.label,
                            kind: e.kind.clone(),
                            cell: None,
                            severity: rule.severity,
                            text,
                            raised_ms: e.at_ms,
                            cleared_ms: None,
                            clear_text: None,
                            acked_by: None,
                            acked_ms: None,
                        });
                        changed = true;
                    }
                }
            }
            self.data.last_log_id = Some(max.max(seen));
            if first_start {
                changed = true;
            }
        } else if first_start {
            self.data.last_log_id = Some(0);
        }

        // Stations no longer listed: their alarms are finished, their graphs dropped.
        let listed: std::collections::HashSet<&str> = stations.iter().map(|s| s.id.as_str()).collect();
        for a in self.data.alarms.iter_mut().filter(|a| a.active() && !a.event() && !listed.contains(a.station.as_str())) {
            a.cleared_ms = Some(now_ms);
            a.clear_text = Some("station no longer in Fleet Control".into());
            changed = true;
        }
        self.series.retain(|id, _| listed.contains(id.as_str()));
        self.live.retain(|id, _| listed.contains(id.as_str()));

        let rules = self.data.settings.rules.clone();
        for st in stations {
            // Unreachable.
            let offline_since = st.last_seen_ms.max(self.started_ms);
            let off_min = now_ms.saturating_sub(offline_since) / 60_000;
            changed |= self.condition_since(
                st,
                "offline",
                None,
                !st.online,
                st.online,
                Some(offline_since),
                || format!("Basestation {} not reachable (offline for {off_min} min)", st.label),
                "back online",
                now_ms,
            );
            if !st.online {
                continue;
            }
            // Missed pings.
            let misses = self.live.get(&st.id).map(|l| l.ping_misses).unwrap_or(0);
            let need = rules.ping.threshold.max(1.0) as u32;
            changed |= self.condition(st, "ping", None, misses >= need, misses == 0, || format!("Basestation {}: {misses} pings in a row without an answer", st.label), "ping answered", now_ms);

            let Some((at, s)) = self.live.get(&st.id).and_then(|l| l.last.clone()) else { continue };
            if now_ms.saturating_sub(at) > SAMPLE_FRESH_MS {
                continue;
            }
            // SDR error / critical health.
            let critical = s.health.as_deref() == Some("critical");
            let bad = !s.rf_errors.is_empty() || critical;
            let text = || {
                let mut parts: Vec<String> = s.rf_errors.iter().map(|(c, d)| if d.is_empty() { format!("SDR of cell {c} not running") } else { format!("SDR of cell {c}: {d}") }).collect();
                if critical {
                    parts.push("stack health critical".into());
                }
                format!("Basestation {}: {}", st.label, parts.join(", "))
            };
            changed |= self.condition(st, "sdr_error", None, bad, !bad, text, "SDR and stack ok again", now_ms);
            // CPU temperature (2 °C hysteresis).
            if let Some(t) = s.cpu_temp_c {
                let thr = rules.cpu_temp.threshold;
                changed |= self.condition(st, "cpu_temp", None, t >= thr, t < thr - 2.0, || format!("Basestation {}: CPU temperature {t:.1} °C (limit {thr:.0} °C)", st.label), "CPU temperature normal", now_ms);
            }
            // SDR temperature per cell.
            for (cell, t) in &s.sdr {
                let (cell, t) = (*cell, *t);
                let thr = rules.sdr_temp.threshold;
                changed |= self.condition(st, "sdr_temp", Some(cell), t >= thr, t < thr - 2.0, || format!("Basestation {}: SDR temperature of cell {cell} {t:.1} °C (limit {thr:.0} °C)", st.label), "SDR temperature normal", now_ms);
            }
            // CPU load (5 % hysteresis); not during an update (the build takes the CPU),
            // and the minutes count again from its end.
            if st.updating {
                if let Some(l) = self.live.get_mut(&st.id) {
                    l.since.remove(&("cpu_load", None));
                }
            } else if let Some(c) = s.cpu_pct {
                let thr = rules.cpu_load.threshold;
                let mins = rules.cpu_load.minutes;
                changed |= self.condition(st, "cpu_load", None, c >= thr, c < thr - 5.0, || format!("Basestation {}: CPU load {c:.0} % for {mins} min (limit {thr:.0} %)", st.label), "CPU load normal", now_ms);
            }
        }
        changed |= self.prune(now_ms);
        if changed {
            self.save_alarms();
        }
        if now_ms.saturating_sub(self.metrics_saved_ms) >= METRICS_SAVE_MS {
            self.save_metrics(now_ms);
        }
        changed
    }

    /// Drops finished alarms older than 90 days.
    fn prune(&mut self, now_ms: u64) -> bool {
        let before = self.data.alarms.len();
        self.data.alarms.retain(|a| a.cleared_ms.is_none_or(|c| c + HISTORY_KEEP_MS >= now_ms));
        before != self.data.alarms.len()
    }

    /// Acknowledge one alarm (`Some(id)`) or every active one. A change log
    /// alarm is finished by it. Returns how many were acknowledged.
    pub fn ack(&mut self, id: Option<u64>, user: Option<&str>, now_ms: u64) -> usize {
        let who = user.unwrap_or("dashboard").to_owned();
        let mut n = 0;
        for a in self.data.alarms.iter_mut().filter(|a| a.active() && a.acked_ms.is_none() && id.is_none_or(|i| i == a.id)) {
            a.acked_by = Some(who.clone());
            a.acked_ms = Some(now_ms);
            if a.event() {
                a.cleared_ms = Some(now_ms);
                a.clear_text = Some(format!("acknowledged by {who}"));
            }
            n += 1;
        }
        if n > 0 {
            self.save_alarms();
        }
        n
    }

    /// Active alarms (ALARM first, newest first).
    pub fn active(&self) -> Vec<Alarm> {
        let mut v: Vec<Alarm> = self.data.alarms.iter().filter(|a| a.active()).cloned().collect();
        v.sort_by_key(|a| (a.severity != Severity::Alarm, std::cmp::Reverse(a.raised_ms)));
        v
    }

    /// Finished alarms, newest first.
    pub fn history(&self, limit: usize) -> Vec<Alarm> {
        let mut v: Vec<Alarm> = self.data.alarms.iter().filter(|a| !a.active()).cloned().collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.cleared_ms));
        v.truncate(limit);
        v
    }

    /// Graph data of `id` from `from_ms` on (the running interval included).
    pub fn graph(&self, id: &str, from_ms: u64) -> (Vec<Point>, Vec<Ping>) {
        let Some(s) = self.series.get(id) else { return (Vec::new(), Vec::new()) };
        let mut points: Vec<Point> = s.points.iter().filter(|p| p.t >= from_ms).cloned().collect();
        if let Some(p) = s.acc.as_ref().and_then(|a| a.point.clone()) {
            points.push(p);
        }
        (points, s.pings.iter().filter(|p| p.t >= from_ms).cloned().collect())
    }

    /// Latest metrics of a station (for the list).
    pub fn latest(&self, id: &str, now_ms: u64) -> Option<serde_json::Value> {
        let (at, s) = self.live.get(id)?.last.as_ref()?;
        if now_ms.saturating_sub(*at) > SAMPLE_FRESH_MS {
            return None;
        }
        let ping = self.series.get(id).and_then(|s| s.pings.back()).map(|p| p.ms);
        Some(serde_json::json!({"at_ms": at, "cpu_pct": s.cpu_pct, "cpu_temp_c": s.cpu_temp_c, "sdr": s.sdr, "radios": s.radios, "calls": s.calls, "ping_ms": ping.flatten()}))
    }

    fn save_alarms(&self) {
        let Some(path) = &self.alarm_path else { return };
        write_atomic(path, &self.data);
    }

    /// Writes the graphs (also at shutdown-free intervals: at most 5 min are lost).
    pub fn save_metrics(&mut self, now_ms: u64) {
        self.metrics_saved_ms = now_ms;
        let Some(path) = &self.metrics_path else { return };
        write_atomic(path, &self.series);
    }
}

fn write_atomic<T: Serialize>(path: &Path, value: &T) {
    let Ok(bytes) = serde_json::to_vec(value) else { return };
    let tmp = path.with_extension("tmp-write");
    if let Err(e) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
        tracing::warn!(error = %e, path = %path.display(), "cannot write a Fleet Control file");
    }
}

fn flush(series: &mut Series) {
    if let Some(p) = series.acc.take().and_then(|a| a.point) {
        series.points.push_back(p);
    }
}

/// Ring buffer: drop what is older than the keep time.
fn trim(series: &mut Series, now_ms: u64, keep_ms: u64) {
    let from = now_ms.saturating_sub(keep_ms);
    while series.points.front().is_some_and(|p| p.t < from) {
        series.points.pop_front();
    }
    while series.pings.front().is_some_and(|p| p.t < from) {
        series.pings.pop_front();
    }
    while series.points.len() as u64 > MAX_POINTS {
        series.points.pop_front();
    }
    while series.pings.len() as u64 > MAX_POINTS {
        series.pings.pop_front();
    }
}

/// A `FleetMetrics` report as a sample.
pub fn sample_from_report(cpu_pct: Option<f32>, cpu_temp_c: Option<f32>, cells: &[crate::control::CellMetrics], radios: u32, calls: u32, health: &str) -> Sample {
    let ok = |v: &Option<f32>| v.filter(|x| x.is_finite());
    Sample {
        cpu_pct: ok(&cpu_pct).map(|c| c.clamp(0.0, 100.0)),
        cpu_temp_c: ok(&cpu_temp_c),
        sdr: cells.iter().filter_map(|c| ok(&c.sdr_temp_c).map(|t| (c.cell, t))).collect(),
        rf_errors: cells.iter().filter(|c| c.rf == "error").map(|c| (c.cell, c.detail.chars().take(200).collect())).collect(),
        radios: Some(radios),
        calls: Some(calls),
        health: (!health.is_empty()).then(|| health.to_owned()),
    }
}

/// A sample from the telemetry of a station without `metrics-v1`
/// (temperatures, radios, calls; no CPU load).
pub fn sample_from_telemetry(bts: &crate::telemetry::TelemetryBts) -> Sample {
    let temps: Vec<&crate::telemetry::SysSensor> =
        bts.last_sys_health.as_ref().map(|h| h.sensors.iter().filter(|s| matches!(s.kind, crate::telemetry::SysSensorKind::Temperature)).collect()).unwrap_or_default();
    let cpu = temps
        .iter()
        .find(|s| {
            let n = s.name.to_ascii_lowercase();
            n.contains("cpu") || n.contains("soc") || n.contains("core") || n.contains("package") || n.contains("k10temp")
        })
        .or(temps.first())
        .map(|s| s.value);
    let mut sdr = BTreeMap::new();
    if let Some(t) = bts.last_sdr_health.as_ref().and_then(|h| h.temperature_c) {
        sdr.insert(0u8, t);
    }
    for (cell, rf) in &bts.cell_rf {
        if let Some(t) = rf.sdr_health.as_ref().and_then(|h| h.temperature_c) {
            sdr.insert(*cell, t);
        }
    }
    let health = bts.health.as_ref().map(|h| format!("{:?}", h.overall).to_ascii_lowercase());
    Sample { cpu_pct: None, cpu_temp_c: cpu.filter(|t| t.is_finite()), sdr, rf_errors: Vec::new(), radios: Some(bts.registration_count as u32), calls: Some(bts.active_calls.len() as u32), health }
}

/// Background driver: every 10 s alarms and finished points, every minute a
/// telemetry sample of stations without `metrics-v1`, and the server pings at
/// the configured interval. Runs for the whole server lifetime.
pub async fn run_monitor(state: std::sync::Arc<crate::state::AppState>) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(10));
    let mut last_sampled = 0u64;
    let mut last_ping = 0u64;
    loop {
        every.tick().await;
        let now = crate::fleet::now_ms();
        let (infos, log, fleet_ids) = {
            let names = crate::control::station_names(&state);
            let since = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).data.last_log_id;
            let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
            let infos: Vec<StationInfo> = fleet
                .stations()
                .into_iter()
                .map(|s| StationInfo { label: crate::fleet::label(&|k| names.get(k).cloned(), &s.id, s.brew_login.as_deref()), updating: update_running(s.progress.as_ref(), now), id: s.id, online: s.online, last_seen_ms: s.last_seen_ms })
                .collect();
            let ids: Vec<(String, Option<String>, bool, bool)> = fleet
                .stations()
                .into_iter()
                .map(|s| {
                    let metrics = s.capabilities.iter().any(|c| c == METRICS_CAPABILITY);
                    let ping = s.capabilities.iter().any(|c| c == "fleet-v1");
                    (s.id, s.brew_login, s.online && !metrics, s.online && ping)
                })
                .collect();
            (infos, fleet.log_since(since), ids)
        };
        // Older stations: sample their telemetry once a minute.
        if now.saturating_sub(last_sampled) >= 60_000 {
            last_sampled = now;
            let tele = state.telemetry.read().await;
            let samples: Vec<(String, Sample)> = fleet_ids
                .iter()
                .filter(|(_, _, sample, _)| *sample)
                .filter_map(|(id, brew, _, _)| {
                    let bts = tele.stations.get(id).or_else(|| brew.as_ref().and_then(|b| tele.stations.get(b)))?;
                    Some((id.clone(), sample_from_telemetry(bts)))
                })
                .collect();
            drop(tele);
            let mut mon = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner());
            for (id, s) in samples {
                if !mon.reports_itself(&id, now) {
                    mon.record(&id, s, now);
                }
            }
        }
        // Server pings.
        let ping_ms = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).settings().ping_min as u64 * 60_000;
        if ping_ms > 0 && now.saturating_sub(last_ping) >= ping_ms {
            last_ping = now;
            for (id, ..) in fleet_ids.iter().filter(|x| x.3) {
                let (state, id) = (state.clone(), id.clone());
                tokio::spawn(async move {
                    let started = std::time::Instant::now();
                    let ms = match crate::control::send_command(&state, &id, crate::control::ControlCommand::FleetPing { handle: 0 }).await {
                        Ok(Some(crate::control::ControlResponse::CommandResult { .. })) => Some(started.elapsed().as_millis().min(u32::MAX as u128) as u32),
                        Err(crate::control::SendError::NotConnected) => return,
                        _ => None,
                    };
                    state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).record_ping(&id, ms, crate::fleet::now_ms());
                });
            }
        }
        let changed = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).evaluate(&infos, &log, now);
        if changed {
            state.monitor.emit("fleet_changed", serde_json::json!({"alarms": true}));
        }
    }
}

/// For `GET /api/fleet`: active alarms, recent history, the settings and the
/// latest values per station.
pub fn list_json(state: &crate::state::AppState, ids: &[String]) -> serde_json::Value {
    let now = crate::fleet::now_ms();
    let m = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner());
    let latest: serde_json::Map<String, serde_json::Value> = ids.iter().filter_map(|id| m.latest(id, now).map(|v| (id.clone(), v))).collect();
    serde_json::json!({"active": m.active(), "history": m.history(HISTORY_SHOWN), "settings": m.settings(), "latest": latest})
}

#[derive(Deserialize)]
pub struct GraphQuery {
    pub id: String,
    /// Hours back (default: everything kept).
    #[serde(default)]
    pub hours: Option<u32>,
}

/// `GET /api/fleet/graph?id=..&hours=..` (Fleet Control visibility): the
/// points and pings of one station.
pub async fn api_graph(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    axum::extract::Query(q): axum::extract::Query<GraphQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let names = crate::control::station_names(&state);
    let station = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(&q.id).cloned();
    let Some(station) = station else { return (axum::http::StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response() };
    let now = crate::fleet::now_ms();
    let m = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner());
    let set = m.settings().clone();
    let back = q.hours.map(|h| h as u64 * 3_600_000).unwrap_or(set.keep_ms()).min(set.keep_ms());
    let from = now.saturating_sub(back);
    let (points, pings) = m.graph(&q.id, from);
    let alarms: Vec<Alarm> = m.active().into_iter().filter(|a| a.station == q.id).collect();
    axum::Json(serde_json::json!({
        "id": station.id,
        "label": crate::fleet::label(&|k| names.get(k).cloned(), &station.id, station.brew_login.as_deref()),
        "online": station.online,
        "last_seen_ms": station.last_seen_ms,
        "metrics": station.capabilities.iter().any(|c| c == METRICS_CAPABILITY),
        "from_ms": from,
        "now_ms": now,
        "sample_ms": set.sample_ms(),
        "ping_ms": set.ping_min as u64 * 60_000,
        "keep_hours": set.keep_hours,
        "show_radios": set.show_radios,
        "show_calls": set.show_calls,
        "cpu_temp_limit": set.rules.cpu_temp.enabled.then_some(set.rules.cpu_temp.threshold),
        "sdr_temp_limit": set.rules.sdr_temp.enabled.then_some(set.rules.sdr_temp.threshold),
        "cpu_load_limit": set.rules.cpu_load.enabled.then_some(set.rules.cpu_load.threshold),
        "points": points,
        "pings": pings,
        "alarms": alarms,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct AckRequest {
    /// One alarm; `None`: every active one.
    #[serde(default)]
    pub id: Option<u64>,
}

/// `POST /api/fleet/alarms/ack` `{"id": 7}` or `{}` for all (Fleet Control management).
pub async fn api_ack(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<AckRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return (axum::http::StatusCode::FORBIDDEN, "This needs the right Fleet Control management").into_response();
    }
    let n = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).ack(req.id, a.user.as_deref(), crate::fleet::now_ms());
    state.monitor.emit("fleet_changed", serde_json::json!({"alarms": true}));
    axum::Json(serde_json::json!({"ok": true, "acknowledged": n})).into_response()
}

/// `POST /api/fleet/monitor/settings` (admins): graphs, ping and alarm rules.
pub async fn api_settings(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::state::AppState>>,
    headers: axum::http::HeaderMap,
    axum::Json(req): axum::Json<Settings>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let a = crate::dashboard::access(&state, &headers);
    if !a.admin {
        return (axum::http::StatusCode::FORBIDDEN, "Admins only").into_response();
    }
    let now = crate::fleet::now_ms();
    let res = state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).set_settings(req, a.user.as_deref(), now);
    match res {
        Ok(()) => {
            let who = a.user.clone();
            state.fleet.lock().unwrap_or_else(|e| e.into_inner()).log_station("", who.as_deref(), "settings", |_| "Graph, ping and alarm settings changed".to_owned(), now, &|_| None);
            state.monitor.emit("fleet_changed", serde_json::json!({"alarms": true}));
            axum::Json(serde_json::json!({"ok": true})).into_response()
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn st(id: &str, online: bool, last_seen_ms: u64) -> StationInfo {
        StationInfo { id: id.into(), label: id.to_uppercase(), online, last_seen_ms, updating: false }
    }
    fn sample(cpu: f32, ct: f32, sdr: f32) -> Sample {
        Sample { cpu_pct: Some(cpu), cpu_temp_c: Some(ct), sdr: [(0u8, sdr)].into(), radios: Some(2), calls: Some(0), health: Some("ok".into()), ..Default::default() }
    }
    fn kinds(m: &FleetMonitor) -> Vec<String> {
        let mut v: Vec<String> = m.active().iter().map(|a| format!("{}:{}", a.station, a.kind)).collect();
        v.sort();
        v
    }

    #[test]
    fn defaults_match_the_agreed_values_and_are_valid() {
        let s = Settings::default();
        assert_eq!((s.sample_min, s.keep_hours, s.ping_min, s.show_radios, s.show_calls), (1, 24, 5, false, false));
        assert_eq!((s.rules.cpu_temp.threshold, s.rules.sdr_temp.threshold, s.rules.cpu_load.threshold, s.rules.cpu_load.minutes, s.rules.offline.minutes, s.rules.ping.threshold), (80.0, 70.0, 90.0, 5, 2, 3.0));
        assert_eq!(s.rules.cpu_load.severity, Severity::Warning);
        assert_eq!(s.rules.offline.severity, Severity::Alarm);
        assert!(s.validate().is_ok());
        // An old file without the section gets the defaults.
        let f: AlarmFile = serde_json::from_str("{}").unwrap();
        assert_eq!(f.settings, s);
    }

    #[test]
    fn settings_are_checked() {
        let bad = |f: fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            s.validate().is_err()
        };
        assert!(bad(|s| s.sample_min = 0));
        assert!(bad(|s| s.keep_hours = 721));
        assert!(bad(|s| s.keep_hours = 720)); // 43 200 points at 1 min: too many
        assert!(!bad(|s| {
            s.keep_hours = 720;
            s.sample_min = 5
        }));
        assert!(bad(|s| s.ping_min = 61));
        assert!(!bad(|s| s.ping_min = 0));
        assert!(bad(|s| s.rules.cpu_temp.threshold = 10.0));
        assert!(bad(|s| s.rules.offline.minutes = 0));
        assert!(bad(|s| s.rules.ping.threshold = 0.0));
    }

    #[test]
    fn one_point_per_interval_mean_load_highest_temperature() {
        let mut m = FleetMonitor::load(None, 0);
        m.record("bs1", sample(10.0, 50.0, 40.0), 10 * MIN);
        m.record("bs1", sample(30.0, 55.0, 42.0), 10 * MIN + 30_000);
        m.record("bs1", sample(50.0, 52.0, 41.0), 11 * MIN);
        let (pts, _) = m.graph("bs1", 0);
        assert_eq!(pts.len(), 2);
        assert_eq!((pts[0].t, pts[0].c, pts[0].ct, pts[0].s[&0]), (10 * MIN, Some(20.0), Some(55.0), 42.0));
        assert_eq!(pts[1].c, Some(50.0), "the running interval is shown too");

        // 5-minute points.
        m.set_settings(Settings { sample_min: 5, ..Default::default() }, None, 11 * MIN).unwrap();
        m.record("bs2", sample(10.0, 50.0, 40.0), 20 * MIN);
        m.record("bs2", sample(20.0, 50.0, 40.0), 24 * MIN);
        m.record("bs2", sample(90.0, 50.0, 40.0), 25 * MIN);
        let (pts, _) = m.graph("bs2", 0);
        assert_eq!(pts.iter().map(|p| (p.t, p.c)).collect::<Vec<_>>(), vec![(20 * MIN, Some(15.0)), (25 * MIN, Some(90.0))]);
    }

    #[test]
    fn graphs_are_a_ring_buffer_of_the_keep_time_with_gaps_while_offline() {
        let mut m = FleetMonitor::load(None, 0);
        m.set_settings(Settings { keep_hours: 1, ..Default::default() }, None, 0).unwrap();
        for i in 0..90 {
            if (30..40).contains(&i) {
                continue; // offline: no point
            }
            m.record("bs1", sample(i as f32, 50.0, 40.0), i * MIN);
        }
        let (pts, _) = m.graph("bs1", 0);
        assert_eq!(pts[0].t, 29 * MIN, "older than 1 h overwritten");
        assert_eq!(pts[1].t, 40 * MIN, "30-39 stays a gap");
        assert_eq!(pts.len(), 51);
    }

    #[test]
    fn unreachable_alarm_after_the_minutes_and_finished_when_back() {
        let mut m = FleetMonitor::load(None, 0);
        let off = [st("bs1", false, 10 * MIN)];
        assert!(!m.evaluate(&off, &[], 11 * MIN) || m.active().is_empty());
        assert!(m.active().is_empty());
        assert!(m.evaluate(&off, &[], 12 * MIN));
        assert_eq!(kinds(&m), vec!["bs1:offline"]);
        assert!(m.active()[0].text.contains("BS1 not reachable"));
        assert!(!m.evaluate(&off, &[], 13 * MIN), "raised once");
        assert!(m.evaluate(&[st("bs1", true, 14 * MIN)], &[], 14 * MIN));
        assert!(m.active().is_empty());
        assert_eq!(m.history(10)[0].clear_text.as_deref(), Some("back online"));
    }

    #[test]
    fn after_a_server_restart_the_offline_time_counts_from_the_start() {
        let mut m = FleetMonitor::load(None, 100 * MIN);
        // Last seen long ago (server was down): no alarm in the first 2 min.
        m.evaluate(&[st("bs1", false, 10 * MIN)], &[], 101 * MIN);
        assert!(m.active().is_empty());
        m.evaluate(&[st("bs1", false, 10 * MIN)], &[], 102 * MIN);
        assert_eq!(kinds(&m), vec!["bs1:offline"]);
    }

    #[test]
    fn temperature_load_and_sdr_alarms_with_hysteresis() {
        let mut m = FleetMonitor::load(None, 0);
        let on = [st("bs1", true, 0)];
        m.record("bs1", sample(95.0, 81.0, 71.0), MIN);
        m.evaluate(&on, &[], MIN);
        assert_eq!(kinds(&m), vec!["bs1:cpu_temp", "bs1:sdr_temp"], "load needs 5 min");
        for i in 2..=6 {
            m.record("bs1", sample(95.0, 79.0, 71.0), i * MIN);
            m.evaluate(&on, &[], i * MIN);
        }
        assert_eq!(kinds(&m), vec!["bs1:cpu_load", "bs1:cpu_temp", "bs1:sdr_temp"], "79 °C is within the 2 °C hysteresis");
        m.record("bs1", sample(87.0, 77.0, 60.0), 7 * MIN);
        m.evaluate(&on, &[], 7 * MIN);
        assert_eq!(kinds(&m), vec!["bs1:cpu_load"]);
        m.record("bs1", sample(80.0, 77.0, 60.0), 8 * MIN);
        m.evaluate(&on, &[], 8 * MIN);
        assert!(m.active().is_empty());

        // SDR error and critical health.
        let mut s = sample(10.0, 40.0, 40.0);
        s.rf_errors = vec![(1, "device not found".into())];
        m.record("bs1", s, 9 * MIN);
        m.evaluate(&on, &[], 9 * MIN);
        assert_eq!(m.active()[0].text, "Basestation BS1: SDR of cell 1: device not found");
        m.record("bs1", sample(10.0, 40.0, 40.0), 10 * MIN);
        m.evaluate(&on, &[], 10 * MIN);
        assert!(m.active().is_empty());
    }

    #[test]
    fn no_cpu_load_alarm_during_an_update() {
        let prog = |stage: &str, at_ms: u64| crate::fleet::ProgressInfo { at_ms, stage: stage.into(), percent: 50, message: String::new() };
        assert!(!update_running(None, MIN));
        assert!(update_running(Some(&prog("build", 0)), 29 * MIN));
        assert!(!update_running(Some(&prog("build", 0)), 31 * MIN), "a stuck run stops counting");
        assert!(update_running(Some(&prog("restart", 0)), 4 * MIN));
        assert!(!update_running(Some(&prog("restart", 0)), 5 * MIN));
        assert!(!update_running(Some(&prog("up_to_date", 0)), 6 * MIN));

        let mut m = FleetMonitor::load(None, 0);
        let mut upd = st("bs1", true, 0);
        upd.updating = true;
        // 20 min build at 99 % CPU: no warning.
        for i in 1..=20 {
            m.record("bs1", sample(99.0, 60.0, 40.0), i * MIN);
            m.evaluate(std::slice::from_ref(&upd), &[], i * MIN);
        }
        assert!(m.active().is_empty());
        // After the update the 5 minutes count from new.
        let on = [st("bs1", true, 0)];
        for i in 21..=24 {
            m.record("bs1", sample(99.0, 60.0, 40.0), i * MIN);
            m.evaluate(&on, &[], i * MIN);
        }
        assert!(m.active().is_empty(), "only 3 min since the update ended");
        m.record("bs1", sample(99.0, 60.0, 40.0), 26 * MIN);
        m.evaluate(&on, &[], 26 * MIN);
        assert_eq!(kinds(&m), vec!["bs1:cpu_load"]);
    }

    #[test]
    fn rules_are_editable_off_severity_threshold() {
        let mut m = FleetMonitor::load(None, 0);
        let on = [st("bs1", true, 0)];
        m.record("bs1", sample(10.0, 81.0, 40.0), MIN);
        m.evaluate(&on, &[], MIN);
        assert_eq!(kinds(&m), vec!["bs1:cpu_temp"]);
        let mut s = Settings::default();
        s.rules.cpu_temp.enabled = false;
        m.set_settings(s.clone(), Some("sebastian"), 2 * MIN).unwrap();
        assert!(m.active().is_empty(), "switched off: finished");
        assert_eq!(m.history(1)[0].clear_text.as_deref(), Some("rule switched off by sebastian"));
        s.rules.cpu_temp.enabled = true;
        s.rules.cpu_temp.threshold = 85.0;
        s.rules.cpu_temp.severity = Severity::Warning;
        m.set_settings(s, None, 3 * MIN).unwrap();
        m.record("bs1", sample(10.0, 84.0, 40.0), 4 * MIN);
        m.evaluate(&on, &[], 4 * MIN);
        assert!(m.active().is_empty(), "below the new limit");
        m.record("bs1", sample(10.0, 86.0, 40.0), 5 * MIN);
        m.evaluate(&on, &[], 5 * MIN);
        assert_eq!(m.active()[0].severity, Severity::Warning);
    }

    #[test]
    fn missed_pings_warning() {
        let mut m = FleetMonitor::load(None, 0);
        let on = [st("bs1", true, 0)];
        m.record_ping("bs1", Some(40), MIN);
        m.record_ping("bs1", None, 2 * MIN);
        m.record_ping("bs1", None, 3 * MIN);
        m.evaluate(&on, &[], 3 * MIN);
        assert!(m.active().is_empty());
        m.record_ping("bs1", None, 4 * MIN);
        m.evaluate(&on, &[], 4 * MIN);
        assert_eq!(m.active()[0].severity, Severity::Warning);
        assert!(m.active()[0].text.contains("3 pings"));
        m.record_ping("bs1", Some(35), 5 * MIN);
        m.evaluate(&on, &[], 5 * MIN);
        assert!(m.active().is_empty());
        let (_, pings) = m.graph("bs1", 0);
        assert_eq!(pings.iter().map(|p| p.ms).collect::<Vec<_>>(), vec![Some(40), None, None, None, Some(35)]);
    }

    fn log(id: u64, kind: &str, text: &str) -> crate::fleet::LogEntry {
        crate::fleet::LogEntry { id, at_ms: id * 1000, station: "bs1".into(), label: "BS1".into(), user: None, kind: kind.into(), text: text.into() }
    }

    #[test]
    fn change_log_alarms_stay_until_acknowledged_old_ones_are_not_raised() {
        let mut m = FleetMonitor::load(None, 0);
        let on = [st("bs1", true, 0)];
        // First start: ALARMs already in the log are history, not raised.
        m.evaluate(&on, &[log(1, "update_alarm", "ALARM Basestation BS1: old")], MIN);
        assert!(m.active().is_empty());
        m.evaluate(&on, &[log(2, "restart", "x"), log(3, "restart_alarm", "ALARM Basestation BS1: not back 10 min after the reboot")], 2 * MIN);
        assert_eq!(kinds(&m), vec!["bs1:restart_alarm"]);
        assert_eq!(m.active()[0].text, "Basestation BS1: not back 10 min after the reboot");
        m.evaluate(&on, &[log(3, "restart_alarm", "again the same entry")], 3 * MIN);
        assert_eq!(m.active().len(), 1, "an entry is raised once");
        assert_eq!(m.ack(None, Some("sebastian"), 4 * MIN), 1);
        assert!(m.active().is_empty());
        let h = &m.history(1)[0];
        assert_eq!((h.acked_by.as_deref(), h.clear_text.as_deref()), (Some("sebastian"), Some("acknowledged by sebastian")));
    }

    #[test]
    fn acknowledging_a_condition_alarm_keeps_it_active_until_the_cause_is_gone() {
        let mut m = FleetMonitor::load(None, 0);
        m.evaluate(&[st("bs1", false, 0)], &[], 3 * MIN);
        let id = m.active()[0].id;
        assert_eq!(m.ack(Some(id), Some("op"), 4 * MIN), 1);
        assert_eq!(m.ack(Some(id), Some("op"), 4 * MIN), 0, "only once");
        assert_eq!(m.active()[0].acked_by.as_deref(), Some("op"));
        m.evaluate(&[st("bs1", true, 5 * MIN)], &[], 5 * MIN);
        assert!(m.active().is_empty());
    }

    #[test]
    fn a_removed_station_finishes_its_alarms_and_drops_its_graph() {
        let mut m = FleetMonitor::load(None, 0);
        m.record("bs1", sample(10.0, 90.0, 40.0), MIN);
        m.evaluate(&[st("bs1", true, 0)], &[], MIN);
        assert_eq!(m.active().len(), 1);
        m.evaluate(&[], &[], 2 * MIN);
        assert!(m.active().is_empty());
        assert!(m.graph("bs1", 0).0.is_empty());
    }

    #[test]
    fn history_is_kept_90_days() {
        let mut m = FleetMonitor::load(None, 0);
        m.evaluate(&[st("bs1", false, 0)], &[], 3 * MIN);
        m.evaluate(&[st("bs1", true, 4 * MIN)], &[], 4 * MIN);
        assert_eq!(m.history(10).len(), 1);
        m.evaluate(&[st("bs1", true, 0)], &[], 4 * MIN + HISTORY_KEEP_MS + 1);
        assert!(m.history(10).is_empty());
    }

    #[test]
    fn files_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("fleetmon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("history.log");
        let mut m = FleetMonitor::load(Some(&base), 0);
        m.record("bs1", sample(10.0, 50.0, 40.0), MIN);
        m.record("bs1", sample(20.0, 50.0, 40.0), 2 * MIN);
        m.evaluate(&[st("bs1", false, 0)], &[], 3 * MIN);
        m.save_metrics(3 * MIN);
        let m2 = FleetMonitor::load(Some(&base), 4 * MIN);
        assert_eq!(m2.active().len(), 1);
        assert_eq!(m2.graph("bs1", 0).0.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_report_becomes_a_sample() {
        let cells = vec![
            crate::control::CellMetrics { cell: 0, rf: "online".into(), sdr_temp_c: Some(41.0), detail: String::new() },
            crate::control::CellMetrics { cell: 1, rf: "error".into(), sdr_temp_c: None, detail: "no device".into() },
        ];
        let s = sample_from_report(Some(140.0), Some(f32::NAN), &cells, 3, 1, "degraded");
        assert_eq!(s.cpu_pct, Some(100.0));
        assert_eq!(s.cpu_temp_c, None);
        assert_eq!(s.sdr, [(0u8, 41.0)].into());
        assert_eq!(s.rf_errors, vec![(1, "no device".to_string())]);
        assert_eq!((s.radios, s.calls, s.health.as_deref()), (Some(3), Some(1), Some("degraded")));
    }

    /// 10 stations, 25 radios: one simulated day at one report per minute,
    /// two stations go offline, one runs hot; the alarms and the graphs follow.
    #[test]
    fn ten_stations_25_radios_one_day() {
        let mut m = FleetMonitor::load(None, 0);
        let ids: Vec<String> = (1..=10).map(|i| format!("bs{i}")).collect();
        let radios = |i: usize| (25 / 10 + usize::from(i < 25 % 10)) as u32; // 25 radios spread over 10 stations
        assert_eq!((0..10).map(radios).sum::<u32>(), 25);
        m.set_settings(Settings { ping_min: 5, ..Default::default() }, None, 0).unwrap();
        let day = 24 * 60;
        for minute in 0..day {
            let now = minute * MIN;
            let infos: Vec<StationInfo> = ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let down = (i == 3 && (600..660).contains(&minute)) || (i == 7 && minute >= day - 10);
                    let last_seen = if !down { now } else if i == 3 { 600 * MIN } else { (day - 10) * MIN };
                    st(id, !down, last_seen)
                })
                .collect();
            for (i, info) in infos.iter().enumerate() {
                if !info.online {
                    m.disconnected(&info.id);
                    continue;
                }
                let hot = i == 5 && (300..320).contains(&minute);
                m.record(&info.id, Sample { cpu_pct: Some(20.0), cpu_temp_c: Some(if hot { 85.0 } else { 55.0 }), sdr: [(0u8, 40.0)].into(), radios: Some(radios(i)), calls: Some((minute % 3) as u32), health: Some("ok".into()), ..Default::default() }, now);
                if minute % 5 == 0 {
                    m.record_ping(&info.id, Some(30 + i as u32), now);
                }
            }
            m.evaluate(&infos, &[], now);
            if minute == 310 {
                assert_eq!(kinds(&m), vec!["bs6:cpu_temp"]);
            }
            if minute == 630 {
                assert_eq!(kinds(&m), vec!["bs4:offline"]);
            }
            if minute == 700 {
                assert!(m.active().is_empty());
            }
        }
        assert_eq!(kinds(&m), vec!["bs8:offline"]);
        let (pts, pings) = m.graph("bs1", 0);
        assert_eq!(pts.len() as u64, day, "one point per minute for a whole day");
        assert_eq!(pings.len() as u64, day / 5);
        let (pts4, _) = m.graph("bs4", 0);
        assert_eq!(pts4.len() as u64, day - 60, "an hour offline is a gap");
        let total: u32 = ids.iter().map(|id| m.graph(id, 0).0.iter().filter(|p| p.t == 100 * MIN).map(|p| p.r.unwrap_or(0)).sum::<u32>()).sum();
        assert_eq!(total, 25);
        assert_eq!(m.history(100).len(), 2, "cpu_temp of bs6 and offline of bs4 are history");
    }
}
