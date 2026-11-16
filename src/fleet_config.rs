//! Fleet Control phases 4/5: read and change the config of a station
//! (`config-v1`). Protocol and behaviour: `docs/Fleet-Control.md` section 10.
//!
//! - The server keeps a copy of every station's config (`GetConfig`: settings
//!   as JSON and the file as TOML, both without passwords and keys), read at the
//!   first login and after every confirmed change.
//! - Every hello carries the station's config hash. A hash other than the copy's
//!   means the config was changed on the station: the server reads it and shows
//!   it as a difference; nothing can be changed from here until a user imports
//!   the station's config or applies the server copy.
//! - A change from the edit page goes out as `SetConfig` with the hash of the
//!   config it was made on. The station restarts; when it is back (with the
//!   change waiting for confirmation) and, if it has a Brew login, on Brew
//!   again, the server sends `ConfirmConfig`. Without it the station restores
//!   its old config after 180 s by itself. "Undo" sends `RollbackConfig`.
//! - For an offline station the change is kept as pending and sent at its next
//!   login, only if its config is still the one the change was made on
//!   (otherwise: conflict).
//! - While an edit page is open the station shows the maintenance banner
//!   (refreshed every minute, off when the page closes, runs out by itself).

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::control::{send_command, ControlCommand, ControlResponse, SendError};
use crate::fleet::now_ms;
use crate::state::AppState;

/// Capability of stations that can read and change their config.
pub const CONFIG_CAPABILITY: &str = "config-v1";
/// Confirmation window the server asks for by default (seconds after the
/// restart); Server settings can change it (`[fleet] confirm window`, phase 6).
pub const DEFAULT_CONFIRM_SECS: u32 = 90;
/// Bounds of the window (the station clamps to the same).
pub const MIN_CONFIRM_SECS: u32 = 60;
pub const MAX_CONFIRM_SECS: u32 = 900;
/// A station not back with the change this long after `SetConfig`: alarm
/// (restart + window + margin; the station has rolled back by then).
pub fn apply_limit_ms(confirm_secs: u32) -> u64 {
    (10 + confirm_secs as u64 + 60) * 1000
}

fn old_confirm_secs() -> u32 {
    180
}

/// How the last change sent to a station ended (the bulk run reads it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub at_ms: u64,
    /// Confirmed and kept.
    pub ok: bool,
    pub text: String,
}
/// Maintenance time set per heartbeat of an open edit page (minutes).
pub const EDIT_MAINTENANCE_MIN: u32 = 3;
/// An edit page without a heartbeat for this long counts as closed.
pub const EDIT_TIMEOUT_MS: u64 = EDIT_MAINTENANCE_MIN as u64 * 60_000;
/// Retry of a pending change while a call is in progress.
const RETRY_MS: u64 = 10_000;

/// A config as read from the station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigCopy {
    pub hash: String,
    /// The settings (the station's Infrastructure page), passwords only as `password_set`.
    pub config: serde_json::Value,
    /// The file as TOML, secrets as `"***"`.
    pub text: String,
    pub at_ms: u64,
    /// `read` (first login), `applied` (after a confirmed change), `imported`.
    pub how: String,
}

/// A change for an offline station, sent at its next login.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingChange {
    /// Hash of the config the change was made on.
    pub base_hash: String,
    pub changes: serde_json::Value,
    /// What changes, one line per setting (for the log and the page).
    pub summary: Vec<String>,
    pub by: Option<String>,
    pub at_ms: u64,
    /// The station's config changed since: not sent.
    #[serde(default)]
    pub conflict: bool,
    /// Last word from the station or the server (waiting for a call, refused, …).
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub next_try_ms: u64,
}

/// A change sent to the station and not confirmed yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyRun {
    pub by: Option<String>,
    pub at_ms: u64,
    pub old_hash: String,
    pub summary: Vec<String>,
    /// It was a pending change sent at the login.
    #[serde(default)]
    pub offline: bool,
    /// The station is back with the change waiting for confirmation.
    #[serde(default)]
    pub back_ms: Option<u64>,
    #[serde(default)]
    pub new_hash: Option<String>,
    #[serde(default)]
    pub confirm_sent_ms: Option<u64>,
    /// "Undo" was pressed.
    #[serde(default)]
    pub rollback_by: Option<String>,
    /// Confirmation window asked for (older runs: 180 s).
    #[serde(default = "old_confirm_secs")]
    pub confirm_secs: u32,
}

/// Config state of one station on this server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StationConfig {
    /// Hash in the station's last hello.
    #[serde(default)]
    pub reported_hash: Option<String>,
    #[serde(default)]
    pub copy: Option<ConfigCopy>,
    /// The station's own config while it differs from the copy.
    #[serde(default)]
    pub drift: Option<ConfigCopy>,
    #[serde(default)]
    pub pending: Option<PendingChange>,
    #[serde(default)]
    pub apply: Option<ApplyRun>,
    /// How the last change ended (phase 6).
    #[serde(default)]
    pub last_outcome: Option<Outcome>,
    /// The copy was dropped after a confirmed change and is being read again.
    #[serde(skip)]
    pub reread: bool,
}

/// Small config state for the Fleet Control list: change running (sent at), copy and station
/// differ, settings in a pending change and whether it is held back by a conflict.
pub fn list_summary(c: &StationConfig) -> serde_json::Value {
    serde_json::json!({
        "apply": c.apply.as_ref().map(|a| a.at_ms),
        "drift": c.drift.is_some(),
        "pending": c.pending.as_ref().map(|p| p.summary.len().max(1)),
        "conflict": c.pending.as_ref().is_some_and(|p| p.conflict),
        "copy": c.copy.is_some(),
    })
}

/// An open edit page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditSession {
    pub user: Option<String>,
    pub until_ms: u64,
    /// The page turned maintenance on (and turns it off again).
    pub auto_maintenance: bool,
}

/// What a hello asks the server to do.
#[derive(Debug, Clone, PartialEq)]
pub enum HelloAction {
    Nothing,
    /// Read the config (first time, or the hash differs from the copy).
    Read,
}

/// Note a hello's config fields in `cfg`; returns what to do next and log lines.
pub fn on_hello(cfg: &mut StationConfig, capable: bool, hash: Option<String>, pending: Option<u32>, now: u64) -> (HelloAction, Vec<(&'static str, String)>) {
    let mut logs = Vec::new();
    cfg.reported_hash = hash.clone();
    let Some(hash) = hash.filter(|_| capable) else { return (HelloAction::Nothing, logs) };
    if let Some(run) = cfg.apply.as_mut() {
        if pending.is_some() {
            // Back with the change; the driver confirms it (after Brew is up).
            if run.back_ms.is_none() {
                run.back_ms = Some(now);
            }
            run.new_hash = Some(hash);
            return (HelloAction::Nothing, logs);
        }
        let run = cfg.apply.take().expect("checked");
        if hash == run.old_hash {
            let undone = run.rollback_by.is_some();
            cfg.last_outcome = Some(Outcome { at_ms: now, ok: false, text: if undone { "undone".into() } else { "rolled back by the station (not confirmed in time)".into() } });
            logs.push(if run.rollback_by.is_some() {
                ("config", "config change undone, old config back".to_owned())
            } else {
                ("config_alarm", "ALARM config change rolled back by the station (not confirmed in time)".to_owned())
            });
        }
    }
    if let Some(p) = cfg.pending.as_mut() {
        if !p.conflict && p.base_hash != hash {
            p.conflict = true;
            p.message = "The config on the station changed since this change was made".into();
            logs.push(("config_alarm", "pending config change not sent: the config on the station changed".to_owned()));
        }
    }
    match &cfg.copy {
        Some(c) if c.hash == hash => {
            cfg.drift = None;
            (HelloAction::Nothing, logs)
        }
        _ => (HelloAction::Read, logs),
    }
}

/// Store a `GetConfig` answer; returns log lines.
pub fn on_config(cfg: &mut StationConfig, read: ConfigCopy) -> Vec<(&'static str, String)> {
    let mut logs = Vec::new();
    match &cfg.copy {
        None => {
            if !std::mem::take(&mut cfg.reread) {
                logs.push(("config", "config read (first copy on this server)".to_owned()));
            }
            cfg.copy = Some(ConfigCopy { how: "read".into(), ..read });
            cfg.drift = None;
        }
        Some(c) if c.hash == read.hash => {
            // Same file: refresh what the station reports around it (links up, …).
            cfg.copy = Some(ConfigCopy { how: c.how.clone(), ..read });
            cfg.drift = None;
        }
        Some(_) => {
            if cfg.drift.as_ref().map(|d| &d.hash) != Some(&read.hash) {
                logs.push(("config_alarm", "config changed on the station: it differs from the server copy".to_owned()));
            }
            if let Some(p) = cfg.pending.as_mut() {
                if !p.conflict && p.base_hash != read.hash {
                    p.conflict = true;
                    p.message = "The config on the station changed since this change was made".into();
                }
            }
            cfg.drift = Some(read);
        }
    }
    logs
}

fn copy_from(r: ControlResponse) -> Result<ConfigCopy, String> {
    match r {
        ControlResponse::ConfigInfo { ok: true, hash, config, text, .. } => Ok(ConfigCopy { hash, config, text, at_ms: now_ms(), how: String::new() }),
        ControlResponse::ConfigInfo { code, message, .. } | ControlResponse::CommandResult { code, message, .. } => Err(format!("{code}: {message}")),
        _ => Err("unexpected answer from the station".into()),
    }
}

fn names(state: &AppState) -> std::collections::HashMap<String, String> {
    crate::control::station_names(state)
}

fn log_all(state: &AppState, id: &str, user: Option<&str>, logs: Vec<(&'static str, String)>) {
    if logs.is_empty() {
        return;
    }
    let n = names(state);
    let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    for (kind, text) in logs {
        fleet.log_station(id, user, kind, |label| format!("Basestation {label}: {text}"), now_ms(), &|k| n.get(k).cloned());
    }
}

fn capable(state: &AppState, id: &str) -> bool {
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(id).is_some_and(|s| s.capabilities.iter().any(|c| c == CONFIG_CAPABILITY))
}

/// Read the station's config and store it (copy or difference).
pub async fn read_config(state: &Arc<AppState>, id: &str) -> Result<(), String> {
    let r = send_command(state, id, ControlCommand::GetConfig { handle: 0 }).await.map_err(send_error)?;
    let read = copy_from(r.ok_or("no answer")?)?;
    let logs = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| on_config(&mut s.cfg, read)).unwrap_or_default();
    log_all(state, id, None, logs);
    state.monitor.emit("fleet_changed", serde_json::json!({"id": id}));
    Ok(())
}

fn send_error(e: SendError) -> String {
    match e {
        SendError::NotConnected => "The station is not connected".into(),
        SendError::Timeout => "No answer from the station within 5 s".into(),
    }
}

/// After a station's hello (switch on): note its config hash, read the config
/// when needed.
pub fn after_hello(state: &Arc<AppState>, id: &str, hash: Option<String>, pending: Option<u32>) {
    let cap = capable(state, id);
    let (action, logs) = state
        .fleet
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .with_station(id, |s| on_hello(&mut s.cfg, cap, hash, pending, now_ms()))
        .unwrap_or((HelloAction::Nothing, Vec::new()));
    log_all(state, id, None, logs);
    if action == HelloAction::Read {
        let (state, id) = (state.clone(), id.to_owned());
        tokio::spawn(async move {
            if let Err(e) = read_config(&state, &id).await {
                tracing::warn!(station = %id, "Fleet Control: cannot read the config: {e}");
            }
        });
    }
}

/// Is the station's Brew link up (by its Brew login)? Stations without Brew count as up.
async fn brew_up(state: &AppState, brew_login: Option<&str>) -> bool {
    let Some(login) = brew_login else { return true };
    state.inner.read().await.clients.values().any(|c| c.username.as_deref() == Some(login))
}

/// Every 2 s (from the update driver): confirm changes that are back, raise the
/// alarm for those that are not, send pending changes, close stale edit pages.
pub async fn tick(state: &Arc<AppState>) {
    let now = now_ms();
    // Edit pages without a heartbeat: maintenance off again.
    let stale: Vec<(String, EditSession)> = {
        let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<String> = fleet.edits.iter().filter(|(_, e)| e.until_ms <= now).map(|(k, _)| k.clone()).collect();
        ids.into_iter().filter_map(|k| fleet.edits.remove(&k).map(|e| (k, e))).collect()
    };
    for (id, e) in stale {
        if e.auto_maintenance {
            maintenance_off(state, &id, e.user.as_deref(), "edit page closed").await;
        }
    }

    let stations = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).stations();
    for s in stations {
        let cap = s.capabilities.iter().any(|c| c == CONFIG_CAPABILITY);
        if let Some(run) = &s.cfg.apply {
            if run.back_ms.is_some() && run.rollback_by.is_none() {
                let retry = run.confirm_sent_ms.is_none_or(|t| now.saturating_sub(t) >= RETRY_MS);
                if s.online && retry && brew_up(state, s.brew_login.as_deref()).await {
                    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(&s.id, |st| {
                        if let Some(r) = st.cfg.apply.as_mut() {
                            r.confirm_sent_ms = Some(now);
                        }
                    });
                    confirm(state, &s.id).await;
                }
            } else if now.saturating_sub(run.at_ms) > apply_limit_ms(run.confirm_secs) {
                let run = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(&s.id, |st| {
                    let r = st.cfg.apply.take();
                    if r.is_some() {
                        st.cfg.last_outcome = Some(Outcome { at_ms: now, ok: false, text: "station not back in time".into() });
                    }
                    r
                }).flatten();
                if let Some(r) = run.filter(|r| r.rollback_by.is_none()) {
                    log_all(state, &s.id, None, vec![("config_alarm", format!("ALARM config change not confirmed: station not back within {} s, it restores its old config", r.confirm_secs))]);
                }
            }
            continue;
        }
        let Some(p) = &s.cfg.pending else { continue };
        if !s.online || !cap || p.conflict || p.next_try_ms > now || s.cfg.reported_hash.as_deref() != Some(p.base_hash.as_str()) || s.cfg.drift.is_some() {
            continue;
        }
        let p = p.clone();
        send_change(state, &s.id, p.base_hash.clone(), p.changes.clone(), p.summary.clone(), p.by.clone(), false, true).await.ok();
    }
}

/// Send `SetConfig`; on `ok` the run is watched until it is confirmed.
/// `offline`: a pending change (kept and retried while a call is in progress).
#[allow(clippy::too_many_arguments)]
async fn send_change(
    state: &Arc<AppState>,
    id: &str,
    base_hash: String,
    changes: serde_json::Value,
    summary: Vec<String>,
    by: Option<String>,
    during_call: bool,
    offline: bool,
) -> Result<(), (axum::http::StatusCode, String)> {
    use axum::http::StatusCode;
    let confirm_secs = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).confirm_secs();
    let cmd = ControlCommand::SetConfig { handle: 0, base_hash: base_hash.clone(), changes, confirm_secs, during_call, by: by.clone().unwrap_or_default() };
    let answer = send_command(state, id, cmd).await;
    let now = now_ms();
    let lines = summary_text(&summary);
    match answer {
        Ok(Some(ControlResponse::CommandResult { ok: true, .. })) => {
            state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
                if offline {
                    s.cfg.pending = None;
                }
                s.cfg.apply = Some(ApplyRun { by: by.clone(), at_ms: now, old_hash: base_hash, summary, offline, back_ms: None, new_hash: None, confirm_sent_ms: None, rollback_by: None, confirm_secs });
            });
            log_all(state, id, by.as_deref(), vec![("config", format!("{} sent, station restarts ({lines})", if offline { "pending config change" } else { "config change" }))]);
            state.monitor.emit("fleet_changed", serde_json::json!({"id": id}));
            Ok(())
        }
        Ok(Some(ControlResponse::CommandResult { code, message, .. })) => {
            if offline {
                let conflict = code == "conflict";
                let wait = code == "call_active" || code == "busy";
                let msg = if wait { format!("Waiting: {message}") } else { message.clone() };
                state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
                    if let Some(p) = s.cfg.pending.as_mut() {
                        p.message = msg;
                        p.conflict = conflict || (!wait && p.conflict);
                        p.next_try_ms = if wait { now + RETRY_MS } else { u64::MAX };
                    }
                });
                if !wait {
                    log_all(state, id, None, vec![("config_alarm", format!("pending config change refused by the station ({code}: {message})"))]);
                }
            }
            let status = if code == "invalid" { StatusCode::BAD_REQUEST } else { StatusCode::CONFLICT };
            Err((status, format!("{code}: {message}")))
        }
        Ok(_) => Err((StatusCode::BAD_GATEWAY, "unexpected answer from the station".into())),
        Err(e) => Err((StatusCode::GATEWAY_TIMEOUT, send_error(e))),
    }
}

/// `send_change` for a bulk run: `Ok(confirm_secs)` when the station wrote the change, else the
/// station's `code` and message (`unreachable` when it did not answer).
pub async fn send_change_coded(
    state: &Arc<AppState>,
    id: &str,
    base_hash: String,
    changes: serde_json::Value,
    summary: Vec<String>,
    by: Option<String>,
    during_call: bool,
) -> Result<u32, (String, String)> {
    match send_change(state, id, base_hash, changes, summary, by, during_call, false).await {
        Ok(()) => Ok(state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(id).and_then(|s| s.cfg.apply.as_ref().map(|a| a.confirm_secs)).unwrap_or(DEFAULT_CONFIRM_SECS)),
        Err((axum::http::StatusCode::GATEWAY_TIMEOUT, m)) => Err(("unreachable".into(), m)),
        Err((_, m)) => match m.split_once(": ") {
            Some((code, msg)) if !code.contains(' ') => Err((code.to_owned(), msg.to_owned())),
            _ => Err(("error".into(), m)),
        },
    }
}

/// Keep a change as the pending change of an offline station (bulk run); logged.
pub fn save_pending(state: &Arc<AppState>, id: &str, base_hash: String, changes: serde_json::Value, summary: Vec<String>, by: Option<String>) {
    let lines = summary_text(&summary);
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
        s.cfg.pending = Some(PendingChange { base_hash, changes, summary, by: by.clone(), at_ms: now_ms(), conflict: false, message: String::new(), next_try_ms: 0 });
    });
    log_all(state, id, by.as_deref(), vec![("config", format!("bulk config change saved, sent at the next login ({lines})"))]);
    state.monitor.emit("fleet_changed", serde_json::json!({"id": id}));
}

fn summary_text(summary: &[String]) -> String {
    if summary.is_empty() {
        return "no details".into();
    }
    let mut s = summary.iter().take(8).cloned().collect::<Vec<_>>().join("; ");
    if summary.len() > 8 {
        s.push_str(&format!("; … {} more", summary.len() - 8));
    }
    s
}

async fn confirm(state: &Arc<AppState>, id: &str) {
    match send_command(state, id, ControlCommand::ConfirmConfig { handle: 0 }).await {
        Ok(Some(ControlResponse::CommandResult { code, .. })) if code == "not_restarted" => {
            // Its hello before the restart already said "pending": wait for the one after it.
            state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
                if let Some(r) = s.cfg.apply.as_mut() {
                    r.back_ms = None;
                    r.confirm_sent_ms = None;
                }
            });
        }
        Ok(Some(ControlResponse::CommandResult { ok, code, message, .. })) => {
            if ok || code == "not_pending" {
                let run = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
                    let r = s.cfg.apply.take();
                    if r.is_some() {
                        s.cfg.last_outcome = Some(Outcome { at_ms: now_ms(), ok: true, text: "confirmed".into() });
                    }
                    r
                }).flatten();
                if let Some(r) = run {
                    let secs = r.back_ms.unwrap_or(now_ms()).saturating_sub(r.at_ms) / 1000;
                    log_all(state, id, r.by.as_deref(), vec![("config", format!("config change confirmed (back after {secs} s)"))]);
                }
                // The copy becomes the new config (also clears a difference).
                state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
                    s.cfg.copy = None;
                    s.cfg.drift = None;
                    s.cfg.reread = true;
                });
                if let Err(e) = read_config(state, id).await {
                    tracing::warn!(station = %id, "Fleet Control: cannot read the config after the change: {e}");
                }
                state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(id, |s| {
                    if let Some(c) = s.cfg.copy.as_mut() {
                        c.how = "applied".into();
                    }
                });
            } else {
                tracing::warn!(station = %id, "Fleet Control: ConfirmConfig refused: {code}: {message}");
            }
            state.monitor.emit("fleet_changed", serde_json::json!({"id": id}));
        }
        _ => {} // tried again in 10 s
    }
}

async fn maintenance_off(state: &Arc<AppState>, id: &str, user: Option<&str>, why: &str) {
    let cmd = ControlCommand::SetMaintenance { handle: 0, on: false, minutes: 0, by: user.unwrap_or_default().to_owned(), reason: String::new() };
    let _ = send_command(state, id, cmd).await;
    let n = names(state);
    let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    if fleet.get(id).is_some_and(|s| s.maintenance.is_some()) {
        fleet.with_station(id, |s| s.maintenance = None);
        fleet.log_station(id, user, "maintenance", |label| format!("Basestation {label}: maintenance off ({why})"), now_ms(), &|k| n.get(k).cloned());
    }
}

// ---------------------------------------------------------------------------
// HTTP API (Fleet Control management)
// ---------------------------------------------------------------------------

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

fn refuse() -> Response {
    (StatusCode::FORBIDDEN, "This needs the right Fleet Control management").into_response()
}

#[derive(Deserialize)]
pub struct IdQuery {
    pub id: String,
}

/// `GET /api/fleet/config?id=` : the station's config state for the edit
/// page; an online station is read first, so the page shows its config now.
pub async fn api_get(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<IdQuery>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let (online, cap, busy) = {
        let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = fleet.get(&q.id) else { return (StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response() };
        (s.online, s.capabilities.iter().any(|c| c == CONFIG_CAPABILITY), s.cfg.apply.is_some())
    };
    let mut read_error = None;
    if online && cap && !busy {
        read_error = read_config(&state, &q.id).await.err();
    }
    let n = names(&state);
    let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = fleet.get(&q.id) else { return (StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response() };
    let label = crate::fleet::label(&|k| n.get(k).cloned(), &s.id, s.brew_login.as_deref());
    Json(serde_json::json!({
        "id": s.id,
        "label": label,
        "online": s.online,
        "last_seen_ms": s.last_seen_ms,
        "brew_login": s.brew_login,
        "version": s.version,
        "capable": cap,
        "capabilities": s.capabilities,
        "maintenance": s.maintenance,
        "edit": fleet.edits.get(&s.id),
        "cfg": s.cfg,
        // "Save as template" starts from these (phase 6).
        "template_sections": s.cfg.copy.as_ref().map(|c| crate::fleet_bulk::sections_of(&c.config)),
        "read_error": read_error,
        "is_admin": a.admin,
        "user": a.user,
        "confirm_secs": fleet.confirm_secs(),
        "now_ms": now_ms(),
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct ApplyRequest {
    pub id: String,
    /// Hash of the config the change was made on.
    pub base_hash: String,
    pub changes: serde_json::Value,
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub during_call: bool,
    /// "Apply server config to base station" after a difference.
    #[serde(default)]
    pub resolve: bool,
    /// The config editor's text: only what differs from the copy goes to the station.
    #[serde(default)]
    pub raw_text: Option<String>,
}

/// Single values that differ between the copy's text (`old`, secrets as `***`) and the edited
/// text, as `raw` patches for `SetConfig` plus one line each. Tables are compared key by key,
/// arrays element by element while the length stays. Hidden values cannot be written back.
pub fn raw_patches(old: &str, new: &str) -> Result<(Vec<serde_json::Value>, Vec<String>), String> {
    let old: toml::Value = toml::from_str(old).map_err(|e| format!("server copy does not parse: {e}"))?;
    let new: toml::Value = toml::from_str(new).map_err(|e| format!("config file: {e}"))?;
    let mut out = Vec::new();
    diff_value(&mut Vec::new(), Some(&old), Some(&new), &mut out);
    let mut patches = Vec::new();
    let mut lines = Vec::new();
    for (path, before, after) in out {
        let name = path.iter().map(|s| s.as_str().map(str::to_owned).unwrap_or_else(|| s.to_string())).collect::<Vec<_>>().join(".");
        let shown = |v: &Option<toml::Value>| v.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "—".into());
        if after.as_ref().is_some_and(|v| v.to_string().contains("\"***\"")) {
            return Err(format!("{name}: a hidden value (***) cannot be written back; set passwords in their fields"));
        }
        lines.push(format!("{name}: {} → {}", shown(&before), shown(&after)));
        patches.push(match after {
            Some(v) => serde_json::json!({"path": path, "value": serde_json::to_value(v).map_err(|e| e.to_string())?}),
            None => serde_json::json!({"path": path, "remove": true}),
        });
    }
    Ok((patches, lines))
}

/// Going back to the server copy: add `raw` patches that take the station's config file back to
/// the copy, also for values the page does not show. Values hidden on both sides (***) are equal
/// and left alone; a hidden value that would have to be written is refused.
pub fn add_resolve_patches(changes: &mut serde_json::Value, summary: &mut Vec<String>, station: &str, copy: &str) -> Result<(), String> {
    let (patches, lines) = raw_patches(station, copy).map_err(|e| format!("Cannot go back to the server copy: {e}"))?;
    if patches.is_empty() {
        return Ok(());
    }
    if let Some(o) = changes.as_object_mut() {
        let raw = o.entry("raw").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = raw.as_array_mut() {
            a.extend(patches);
        }
    }
    summary.extend(lines.into_iter().map(|l| format!("Config file: {}", l.chars().take(180).collect::<String>())));
    Ok(())
}

type RawDiff = (Vec<serde_json::Value>, Option<toml::Value>, Option<toml::Value>);

fn diff_value(path: &mut Vec<serde_json::Value>, a: Option<&toml::Value>, b: Option<&toml::Value>, out: &mut Vec<RawDiff>) {
    match (a, b) {
        (Some(toml::Value::Table(x)), Some(toml::Value::Table(y))) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                path.push(serde_json::Value::String(k.clone()));
                diff_value(path, x.get(k), y.get(k), out);
                path.pop();
            }
        }
        (Some(toml::Value::Array(x)), Some(toml::Value::Array(y))) if x.len() == y.len() && x.iter().chain(y).any(|v| v.is_table()) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                path.push(serde_json::json!(i));
                diff_value(path, Some(p), Some(q), out);
                path.pop();
            }
        }
        (x, y) if x != y => out.push((path.clone(), x.cloned(), y.cloned())),
        _ => {}
    }
}

/// `POST /api/fleet/config/apply`: send a change (online) or keep it as
/// pending (offline). Fleet Control management may change every section, the
/// control channel included (Sebastian 2026-11-12).
pub async fn api_apply(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<ApplyRequest>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if !req.changes.is_object() || (req.changes.as_object().is_some_and(|o| o.is_empty()) && req.raw_text.is_none() && !req.resolve) {
        return (StatusCode::BAD_REQUEST, "Nothing to change").into_response();
    }
    let mut summary: Vec<String> = req.summary.iter().take(60).map(|l| l.chars().take(200).collect()).collect();
    let mut changes = req.changes;
    if let Some(text) = req.raw_text.as_deref() {
        let copy = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(&req.id).and_then(|s| s.cfg.copy.as_ref().map(|c| c.text.clone()));
        let Some(copy) = copy else { return (StatusCode::CONFLICT, "No server copy of this station's config").into_response() };
        match raw_patches(&copy, text) {
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
            Ok((patches, lines)) => {
                if !patches.is_empty() {
                    if let Some(o) = changes.as_object_mut() {
                        let raw = o.entry("raw").or_insert_with(|| serde_json::json!([]));
                        if let Some(a) = raw.as_array_mut() {
                            a.extend(patches);
                        }
                    }
                    summary.extend(lines.into_iter().map(|l| format!("Config file: {}", l.chars().take(180).collect::<String>())));
                }
            }
        }
    }
    if req.resolve {
        // Back to the server copy: also every value the page does not show (the station's file
        // against the copy, passwords and keys excluded).
        let texts = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(&req.id).and_then(|s| Some((s.cfg.drift.as_ref()?.text.clone(), s.cfg.copy.as_ref()?.text.clone())));
        if let Some((station, copy)) = texts {
            if let Err(e) = add_resolve_patches(&mut changes, &mut summary, &station, &copy) {
                return (StatusCode::CONFLICT, e).into_response();
            }
        }
    }
    if changes.as_object().is_some_and(|o| o.is_empty()) {
        return (StatusCode::BAD_REQUEST, "Nothing to change").into_response();
    }
    let (online, cap, drift, running) = {
        let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = fleet.get(&req.id) else { return (StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response() };
        (s.online, s.capabilities.iter().any(|c| c == CONFIG_CAPABILITY), s.cfg.drift.clone(), s.cfg.apply.is_some())
    };
    if !cap {
        return (StatusCode::BAD_REQUEST, "This station cannot change its config from here (needs config-v1)").into_response();
    }
    if running {
        return (StatusCode::CONFLICT, "A config change on this station is not confirmed yet").into_response();
    }
    if drift.is_some() && !req.resolve {
        return (StatusCode::CONFLICT, "The config on the station differs from the server copy: import it or apply the server copy first").into_response();
    }
    if online {
        return match send_change(&state, &req.id, req.base_hash, changes, summary, a.user.clone(), req.during_call, false).await {
            Ok(()) => Json(serde_json::json!({"ok": true, "sent": true})).into_response(),
            Err((status, msg)) => (status, msg).into_response(),
        };
    }
    let lines = summary_text(&summary);
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(&req.id, |s| {
        s.cfg.pending = Some(PendingChange { base_hash: req.base_hash, changes, summary, by: a.user.clone(), at_ms: now_ms(), conflict: false, message: String::new(), next_try_ms: 0 });
    });
    log_all(&state, &req.id, a.user.as_deref(), vec![("config", format!("config change saved, sent at the next login ({lines})"))]);
    state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
    Json(serde_json::json!({"ok": true, "pending": true})).into_response()
}

#[derive(Deserialize)]
pub struct RawPreviewRequest {
    pub id: String,
    pub text: String,
}

/// `POST /api/fleet/config/raw_preview`: what the edited config file would change (the lines
/// of the Apply dialog), or why it cannot be sent.
pub async fn api_raw_preview(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<RawPreviewRequest>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let copy = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(&req.id).and_then(|s| s.cfg.copy.as_ref().map(|c| c.text.clone()));
    let Some(copy) = copy else { return (StatusCode::CONFLICT, "No server copy of this station's config").into_response() };
    match raw_patches(&copy, &req.text) {
        Ok((_, lines)) => Json(serde_json::json!({"lines": lines})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct IdRequest {
    pub id: String,
}

/// `POST /api/fleet/config/discard`: drop the pending change.
pub async fn api_discard(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<IdRequest>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let had = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(&req.id, |s| s.cfg.pending.take().is_some()).unwrap_or(false);
    if !had {
        return (StatusCode::NOT_FOUND, "No pending change").into_response();
    }
    log_all(&state, &req.id, a.user.as_deref(), vec![("config", "pending config change discarded".to_owned())]);
    state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
    Json(serde_json::json!({"ok": true})).into_response()
}

/// `POST /api/fleet/config/import`: the station's (changed) config becomes the server copy.
pub async fn api_import(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<IdRequest>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let ok = state
        .fleet
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .with_station(&req.id, |s| match s.cfg.drift.take() {
            Some(d) => {
                s.cfg.copy = Some(ConfigCopy { how: "imported".into(), ..d });
                true
            }
            None => false,
        })
        .unwrap_or(false);
    if !ok {
        return (StatusCode::NOT_FOUND, "The station's config does not differ from the server copy").into_response();
    }
    log_all(&state, &req.id, a.user.as_deref(), vec![("config", "station config imported as the server copy".to_owned())]);
    state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
    Json(serde_json::json!({"ok": true})).into_response()
}

/// `POST /api/fleet/config/rollback`: "Undo" while a change waits for confirmation.
pub async fn api_rollback(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<IdRequest>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    match send_command(&state, &req.id, ControlCommand::RollbackConfig { handle: 0 }).await {
        Ok(Some(ControlResponse::CommandResult { ok: true, .. })) => {
            state.fleet.lock().unwrap_or_else(|e| e.into_inner()).with_station(&req.id, |s| {
                if let Some(r) = s.cfg.apply.as_mut() {
                    r.rollback_by = a.user.clone().or(Some(String::new()));
                }
            });
            log_all(&state, &req.id, a.user.as_deref(), vec![("config", "config change undone (station restores the old config)".to_owned())]);
            state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Ok(Some(ControlResponse::CommandResult { code, message, .. })) => (StatusCode::CONFLICT, format!("{code}: {message}")).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, "unexpected answer from the station").into_response(),
        Err(e) => (StatusCode::GATEWAY_TIMEOUT, send_error(e)).into_response(),
    }
}

#[derive(Deserialize)]
pub struct EditRequest {
    pub id: String,
    /// true: the page is open (sent every minute); false: closed.
    pub open: bool,
}

/// `POST /api/fleet/edit`: heartbeat of an open edit page. Puts the station in
/// maintenance for 3 min per heartbeat (unless maintenance was set by hand),
/// off again when the page closes or stops sending.
pub async fn api_edit(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<EditRequest>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let now = now_ms();
    let (online, maint_cap, manual) = {
        let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = fleet.get(&req.id) else { return (StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response() };
        let session = fleet.edits.get(&req.id);
        let ours = session.is_some_and(|e| e.auto_maintenance);
        (s.online, s.capabilities.iter().any(|c| c == crate::fleet::MAINTENANCE_CAPABILITY), s.maintenance.as_ref().is_some_and(|m| m.until_ms > now) && !ours)
    };
    if !req.open {
        let session = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).edits.remove(&req.id);
        if session.as_ref().is_some_and(|e| e.auto_maintenance) {
            maintenance_off(&state, &req.id, a.user.as_deref(), "edit page closed").await;
        }
        return Json(serde_json::json!({"ok": true})).into_response();
    }
    let auto = online && maint_cap && !manual;
    let first = {
        let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        let first = !fleet.edits.get(&req.id).is_some_and(|e| e.auto_maintenance);
        // Maintenance this page turned on stays its own (and goes off with it) while the station is away.
        fleet.edits.insert(req.id.clone(), EditSession { user: a.user.clone(), until_ms: now + EDIT_TIMEOUT_MS, auto_maintenance: auto || !first });
        first
    };
    if auto {
        let reason = "Config editing from the Brew server";
        let cmd = ControlCommand::SetMaintenance { handle: 0, on: true, minutes: EDIT_MAINTENANCE_MIN, by: a.user.clone().unwrap_or_default(), reason: reason.into() };
        let _ = send_command(&state, &req.id, cmd).await;
        let n = names(&state);
        let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        if first {
            fleet.set_maintenance(&req.id, true, EDIT_MAINTENANCE_MIN, "editing config", a.user.as_deref(), now, &|k| n.get(k).cloned());
        } else {
            fleet.with_station(&req.id, |s| {
                if let Some(m) = s.maintenance.as_mut() {
                    m.until_ms = now + EDIT_MAINTENANCE_MIN as u64 * 60_000;
                }
            });
        }
    }
    state.monitor.emit("fleet_changed", serde_json::json!({"id": req.id}));
    Json(serde_json::json!({"ok": true, "maintenance": auto, "manual_maintenance": manual})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(hash: &str) -> ConfigCopy {
        ConfigCopy { hash: hash.into(), config: serde_json::json!({"x": hash}), text: String::new(), at_ms: 1, how: String::new() }
    }

    #[test]
    fn first_hello_reads_the_config_then_keeps_it() {
        let mut c = StationConfig::default();
        assert_eq!(on_hello(&mut c, true, Some("aa".into()), None, 10).0, HelloAction::Read);
        let logs = on_config(&mut c, copy("aa"));
        assert_eq!(c.copy.as_ref().unwrap().how, "read");
        assert_eq!(logs.len(), 1);
        assert_eq!(on_hello(&mut c, true, Some("aa".into()), None, 20).0, HelloAction::Nothing, "same hash: nothing to read");
    }

    #[test]
    fn a_station_without_the_capability_is_left_alone() {
        let mut c = StationConfig::default();
        assert_eq!(on_hello(&mut c, false, Some("aa".into()), None, 10).0, HelloAction::Nothing);
        assert_eq!(c.reported_hash.as_deref(), Some("aa"));
    }

    #[test]
    fn a_local_change_shows_as_difference_and_blocks_the_pending_change() {
        let mut c = StationConfig { copy: Some(copy("aa")), ..Default::default() };
        c.pending = Some(PendingChange { base_hash: "aa".into(), changes: serde_json::json!({}), summary: vec![], by: None, at_ms: 0, conflict: false, message: String::new(), next_try_ms: 0 });
        let (action, logs) = on_hello(&mut c, true, Some("bb".into()), None, 10);
        assert_eq!(action, HelloAction::Read);
        assert!(c.pending.as_ref().unwrap().conflict, "pending change not sent on another config");
        assert_eq!(logs[0].0, "config_alarm");
        let logs = on_config(&mut c, copy("bb"));
        assert_eq!(c.drift.as_ref().unwrap().hash, "bb");
        assert_eq!(c.copy.as_ref().unwrap().hash, "aa", "the copy stays until imported");
        assert_eq!(logs[0].0, "config_alarm");
        assert!(on_config(&mut c, copy("bb")).is_empty(), "the same difference is logged once");
        assert_eq!(on_hello(&mut c, true, Some("aa".into()), None, 20).0, HelloAction::Nothing);
        assert!(c.drift.is_none(), "back to the copy: no difference");
    }

    fn run(old: &str) -> ApplyRun {
        ApplyRun { by: Some("admin".into()), at_ms: 0, old_hash: old.into(), summary: vec![], offline: false, back_ms: None, new_hash: None, confirm_sent_ms: None, rollback_by: None, confirm_secs: 90 }
    }

    #[test]
    fn a_change_back_with_pending_waits_for_the_confirmation() {
        let mut c = StationConfig { copy: Some(copy("aa")), apply: Some(run("aa")), ..Default::default() };
        assert_eq!(on_hello(&mut c, true, Some("bb".into()), Some(170), 42).0, HelloAction::Nothing);
        let r = c.apply.as_ref().unwrap();
        assert_eq!((r.back_ms, r.new_hash.as_deref()), (Some(42), Some("bb")));
        assert!(c.drift.is_none(), "no difference while the change runs");
    }

    #[test]
    fn a_rollback_by_the_station_raises_the_alarm() {
        let mut c = StationConfig { copy: Some(copy("aa")), apply: Some(run("aa")), ..Default::default() };
        let (action, logs) = on_hello(&mut c, true, Some("aa".into()), None, 300_000);
        assert_eq!(action, HelloAction::Nothing);
        assert!(c.apply.is_none());
        assert_eq!(logs[0].0, "config_alarm");
        let mut c = StationConfig { copy: Some(copy("aa")), apply: Some(ApplyRun { rollback_by: Some("admin".into()), ..run("aa") }), ..Default::default() };
        let (_, logs) = on_hello(&mut c, true, Some("aa".into()), None, 300_000);
        assert_eq!(logs[0].0, "config", "an undo is no alarm");
    }

    /// Station stand-in: answers the next command with `answer(handle)`.
    async fn answer_next(state: &Arc<AppState>, rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>, answer: impl Fn(u64) -> serde_json::Value) -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_slice(&rx.recv().await.unwrap()).unwrap();
        let handle = v.get("handle").or_else(|| v.as_object().and_then(|o| o.values().find_map(|x| x.get("handle")))).and_then(|h| h.as_u64()).unwrap();
        crate::control::deliver_test_response(state, "bs1", &serde_json::to_vec(&answer(handle)).unwrap()).await;
        v
    }

    #[tokio::test]
    async fn a_confirmation_before_the_restart_keeps_the_change_waiting() {
        let (state, _rx) = AppState::new(crate::config::Config::default(), std::path::PathBuf::from("test.toml"));
        let state = Arc::new(state);
        let mut rx = crate::control::attach_test_session(&state, "bs1").await;
        let hello = br#"{"StationHello":{"protocol":1,"version":"v1","brew_login":"1000001","mcc":232,"mnc":20,"fleet_control":true,"capabilities":["fleet-v1","config-v1"],"config_hash":"aa"}}"#;
        let st = state.clone();
        let deliver = tokio::spawn(async move { crate::control::deliver_test_response(&st, "bs1", hello).await });
        let info = |h: u64, hash: &str| serde_json::json!({"ConfigInfo": {"handle": h, "ok": true, "code": "ok", "message": "", "hash": hash, "config": {"cells": []}, "text": ""}});
        answer_next(&state, &mut rx, |h| info(h, "aa")).await;
        deliver.await.unwrap();
        let cfg = |st: &Arc<AppState>| st.fleet.lock().unwrap().get("bs1").unwrap().cfg.clone();
        for _ in 0..100 {
            if cfg(&state).copy.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(cfg(&state).copy.unwrap().hash, "aa");
        // A change runs; the old process re-announces "pending" before it restarts.
        state.fleet.lock().unwrap().with_station("bs1", |s| s.cfg.apply = Some(ApplyRun { back_ms: Some(5), confirm_sent_ms: Some(5), ..run("aa") }));
        let st = state.clone();
        let c = tokio::spawn(async move { confirm(&st, "bs1").await });
        answer_next(&state, &mut rx, |h| serde_json::json!({"CommandResult": {"handle": h, "ok": false, "code": "not_restarted", "message": ""}})).await;
        c.await.unwrap();
        let r = cfg(&state).apply.expect("the change still waits");
        assert_eq!((r.back_ms, r.confirm_sent_ms), (None, None), "waits for the hello after the restart");
        // After the restart: confirmed, the copy is read again without a "first copy" line.
        state.fleet.lock().unwrap().with_station("bs1", |s| s.cfg.apply.as_mut().unwrap().back_ms = Some(9));
        let logs_before = state.fleet.lock().unwrap().log().len();
        let st = state.clone();
        let c = tokio::spawn(async move { confirm(&st, "bs1").await });
        answer_next(&state, &mut rx, |h| serde_json::json!({"CommandResult": {"handle": h, "ok": true, "code": "ok", "message": ""}})).await;
        answer_next(&state, &mut rx, |h| info(h, "bb")).await;
        c.await.unwrap();
        let after = cfg(&state);
        assert!(after.apply.is_none());
        assert_eq!((after.copy.as_ref().unwrap().hash.as_str(), after.copy.as_ref().unwrap().how.as_str()), ("bb", "applied"));
        let log = state.fleet.lock().unwrap().log();
        assert_eq!(log.len(), logs_before + 1, "only the confirmation is logged: {:?}", log.iter().map(|l| &l.text).collect::<Vec<_>>());
    }

    #[test]
    fn the_config_editor_sends_only_what_changed() {
        let old = "[cell_info]\nmain_carrier = 1521\nlocation_area = 1\n\n[brew]\nhost = \"h\"\npassword = \"***\"\n\n[[cells]]\nid = 1\n[cells.cell_info]\nmain_carrier = 1525\n";
        let new = "# edited\n[cell_info]\nmain_carrier = 1521\nlocation_area = 2\n\n[brew]\nhost = \"h\"\npassword = \"***\"\n\n[[cells]]\nid = 1\n[cells.cell_info]\nmain_carrier = 1527\n\n[security]\nissi_whitelist = [1, 2]\n";
        let (patches, lines) = raw_patches(old, new).unwrap();
        assert_eq!(patches, vec![
            serde_json::json!({"path": ["cell_info", "location_area"], "value": 2}),
            serde_json::json!({"path": ["cells", 0, "cell_info", "main_carrier"], "value": 1527}),
            serde_json::json!({"path": ["security"], "value": {"issi_whitelist": [1, 2]}}),
        ], "comments, layout and the hidden password do not count");
        assert_eq!(lines[0], "cell_info.location_area: 1 → 2");
        let (removed, _) = raw_patches(old, &old.replace("location_area = 1\n", "")).unwrap();
        assert_eq!(removed, vec![serde_json::json!({"path": ["cell_info", "location_area"], "remove": true})]);
        assert!(raw_patches(old, &old.replace("host = \"h\"", "host = \"***\"")).is_err(), "*** never goes back");
        assert!(raw_patches(old, "not toml [").is_err());
        assert!(raw_patches(old, old).unwrap().0.is_empty());
    }

    #[test]
    fn going_back_to_the_server_copy_covers_settings_the_page_does_not_show() {
        // Changed on the station only in a value the page has no field for: the page diff is
        // empty, the file still goes back to the copy (passwords stay as they are).
        let copy = "[phy_io]\nrx_gain = 30\n\n[brew]\nhost = \"h\"\npassword = \"***\"\n";
        let station = copy.replace("rx_gain = 30", "rx_gain = 42");
        let mut changes = serde_json::json!({});
        let mut summary = Vec::new();
        add_resolve_patches(&mut changes, &mut summary, &station, copy).unwrap();
        assert_eq!(changes, serde_json::json!({"raw": [{"path": ["phy_io", "rx_gain"], "value": 30}]}));
        assert_eq!(summary, vec!["Config file: phy_io.rx_gain: 42 → 30".to_owned()]);

        let mut same = serde_json::json!({"migration": true});
        add_resolve_patches(&mut same, &mut Vec::new(), copy, copy).unwrap();
        assert_eq!(same, serde_json::json!({"migration": true}), "nothing added when the files agree");
    }

    #[test]
    fn summary_is_shortened() {
        let lines: Vec<String> = (0..10).map(|i| format!("line {i}")).collect();
        assert!(summary_text(&lines).ends_with("… 2 more"));
        assert_eq!(summary_text(&[]), "no details");
    }

    #[test]
    fn set_config_wire_shape() {
        let cmd = ControlCommand::SetConfig { handle: 7, base_hash: "aa".into(), changes: serde_json::json!({"migration": true}), confirm_secs: 180, during_call: false, by: "admin".into() };
        let v = serde_json::to_value(&cmd).unwrap();
        assert_eq!(v["action"], "SetConfig");
        let info: ControlResponse = serde_json::from_value(serde_json::json!({"ConfigInfo": {"handle": 1, "ok": true, "code": "ok", "message": "", "hash": "aa", "config": {"cells": []}, "text": "x"}})).unwrap();
        assert!(matches!(info, ControlResponse::ConfigInfo { ref hash, .. } if hash == "aa"));
        let old_hello: ControlResponse = serde_json::from_value(serde_json::json!({"StationHello": {"protocol": 1, "version": "v", "fleet_control": true}})).unwrap();
        assert!(matches!(old_hello, ControlResponse::StationHello { config_hash: None, .. }), "a hello without config fields still parses");
    }
}
