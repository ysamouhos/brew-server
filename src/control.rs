//! Basestation Control channel: bidirectional, BTS-initiated WebSocket
//! (subprotocol `basestation-control-v1`) that a control server uses to push
//! commands (kick, DGNA, live SDS, emergency clear, restart/shutdown) and
//! read back responses for the few command types that define one.
use crate::{fsnet, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{atomic::{AtomicU32, Ordering}, Arc},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ControlCommand {
    SendSds { #[serde(default)] handle: u32, source_ssi: u32, dest_ssi: u32, dest_is_group: bool, len_bits: u16, payload: Vec<u8> },
    SendRawSdsType4 { #[serde(default)] handle: u32, source_ssi: u32, dest_ssi: u32, dest_is_group: bool, len_bits: u16, payload: Vec<u8> },
    KickMs { issi: u32 },
    Dgna { issi: u32, gssi: u32, mnemonic: Option<String>, attachment_mode: u8, attach: bool },
    RestartService,
    ShutdownService,
    AddLiveSds { text: String, protocol_id: u8, source_issi: u32, repeat_count: u32 },
    DeleteLiveSds { id: u32 },
    ClearLiveSds,
    ClearEmergency { issi: u32 },
    CommandA { #[serde(default)] handle: u32, parameter: u32 },
    TestCmdB { #[serde(default)] handle: u32, source_ssi: u32, is_group: bool, payload: Vec<u8> },
    /// Ambience listening (SS-AL): command the Basestation to set up a
    /// one-way speech call to `issi` in which the target radio auto-answers
    /// and keys its transmitter, so the dispatcher hears its surroundings.
    /// `enable` false releases an active ambience-listening call. The target
    /// radio indicates the call like any other (not covert).
    ///
    /// PROVISIONAL: the Basestation must implement this command and the
    /// over-the-air AL service for it to take effect; the exact wire shape may
    /// need to match the Basestation's own codec.
    AmbienceListen { issi: u32, enable: bool },
    /// Fleet Control (`fleet-v1`): round-trip check, answered with
    /// `CommandResult` (`ok`, or `fleet_control_off`). Only for stations that
    /// listed `fleet-v1` in their `StationHello`.
    FleetPing { #[serde(default)] handle: u32 },
    /// Fleet Control (`update-v1`): the station runs its update check against
    /// its own OTA source and answers with `UpdateInfo` (may take ~45 s).
    CheckUpdate { #[serde(default)] handle: u32 },
    /// Fleet Control (`update-v1`): start the station's OTA from its own
    /// source; answered with `CommandResult` (`ok`, `busy`, `call_active`,
    /// `unavailable`, `fleet_control_off`), then `UpdateProgress` reports.
    /// `source` (capability `update-live-v1`): live update from this
    /// repository and branch with the radio kept on until the service restart.
    StartUpdate {
        #[serde(default)]
        handle: u32,
        #[serde(default)]
        during_call: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<crate::fleet_update::LiveSource>,
    },
    /// Fleet Control (`maintenance-v1`): maintenance flag on the station
    /// (red banner on every page of its dashboard) for `minutes`; answered
    /// with `CommandResult`.
    SetMaintenance {
        #[serde(default)]
        handle: u32,
        on: bool,
        #[serde(default)]
        minutes: u32,
        #[serde(default)]
        by: String,
        #[serde(default)]
        reason: String,
    },
    /// Fleet Control (`restart-v1`): restart the service (`host` false) or
    /// reboot the host, 5 s after the `CommandResult` (`ok`, `busy`,
    /// `call_active`, `fleet_control_off`).
    RestartStation {
        #[serde(default)]
        handle: u32,
        #[serde(default)]
        host: bool,
        #[serde(default)]
        during_call: bool,
    },
    /// Fleet Control (`config-v1`): read the station's config; answered with
    /// `ConfigInfo` (settings as JSON, file as TOML, both without secrets).
    GetConfig { #[serde(default)] handle: u32 },
    /// Fleet Control (`config-v1`): change the config (keys of the station's
    /// Infrastructure page, see `docs/Fleet-Control.md` 10), only on the config
    /// with `base_hash`; the station restarts and rolls back unless it gets
    /// `ConfirmConfig` within `confirm_secs` after the restart. Answered with
    /// `CommandResult` (`ok`, `conflict`, `invalid`, `busy`, `call_active`).
    SetConfig {
        #[serde(default)]
        handle: u32,
        base_hash: String,
        changes: serde_json::Value,
        #[serde(default)]
        confirm_secs: u32,
        #[serde(default)]
        during_call: bool,
        #[serde(default)]
        by: String,
    },
    /// Fleet Control (`config-v1`): keep the config of the last `SetConfig`.
    ConfirmConfig { #[serde(default)] handle: u32 },
    /// Fleet Control (`config-v1`): go back to the config before the last
    /// `SetConfig` at once (restart).
    RollbackConfig { #[serde(default)] handle: u32 },
}

impl ControlCommand {
    /// The wire format is serde's default externally-tagged representation
    /// (`{"KickMs":{"issi":1}}`), matching Basestation's own derive-based
    /// JSON codec -- not the internal `#[serde(tag = "action")]` shape used
    /// for the dashboard's HTTP request body.
    fn to_wire_json(&self) -> serde_json::Value {
        let tagged = serde_json::to_value(self).expect("ControlCommand always serializes");
        let serde_json::Value::Object(mut map) = tagged else { return tagged };
        let Some(serde_json::Value::String(tag)) = map.remove("action") else { return serde_json::Value::Object(map) };
        if map.is_empty() {
            // Unit variants (RestartService, ShutdownService, ClearLiveSds) serialize
            // as a bare string under serde's default externally-tagged representation.
            serde_json::Value::String(tag)
        } else {
            serde_json::json!({ tag: serde_json::Value::Object(map) })
        }
    }

    fn handle(&self) -> Option<u32> {
        match self {
            Self::SendSds { handle, .. } | Self::SendRawSdsType4 { handle, .. }
            | Self::CommandA { handle, .. } | Self::TestCmdB { handle, .. }
            | Self::FleetPing { handle } | Self::CheckUpdate { handle }
            | Self::StartUpdate { handle, .. } | Self::SetMaintenance { handle, .. }
            | Self::RestartStation { handle, .. } | Self::GetConfig { handle }
            | Self::SetConfig { handle, .. } | Self::ConfirmConfig { handle }
            | Self::RollbackConfig { handle } => Some(*handle),
            _ => None,
        }
    }

    /// How long `send_command` waits for the answer.
    fn answer_timeout(&self) -> Duration {
        match self {
            // The station asks GitHub (its own timeouts reach about 45 s).
            Self::CheckUpdate { .. } => Duration::from_secs(60),
            _ => Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ControlResponse {
    CommandAResponse { handle: u32, result: u32 },
    SendSdsResponse { handle: u32, success: bool },
    KickMsResponse { issi: u32, success: bool },
    AmbienceListenResponse { issi: u32, success: bool },
    /// Fleet Control: what the station is, sent after connecting (only to a
    /// server offering `fleet-control`) and whenever its switch changes.
    StationHello {
        protocol: u32,
        version: String,
        #[serde(default)]
        brew_login: Option<String>,
        #[serde(default)]
        mcc: u16,
        #[serde(default)]
        mnc: u16,
        fleet_control: bool,
        #[serde(default)]
        capabilities: Vec<String>,
        /// `config-v1`: hash of the station's config file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config_hash: Option<String>,
        /// `config-v1`: seconds left to confirm the last `SetConfig`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config_pending: Option<u32>,
    },
    /// Fleet Control (`config-v1`): the answer to `GetConfig`.
    ConfigInfo {
        handle: u32,
        ok: bool,
        #[serde(default)]
        code: String,
        #[serde(default)]
        message: String,
        #[serde(default)]
        hash: String,
        #[serde(default)]
        config: serde_json::Value,
        #[serde(default)]
        text: String,
        #[serde(default)]
        pending: Option<u32>,
    },
    /// Fleet Control: the answer to every Fleet Control command.
    CommandResult { handle: u32, ok: bool, code: String, #[serde(default)] message: String },
    /// Fleet Control (`update-v1`): the answer to `CheckUpdate`.
    UpdateInfo {
        handle: u32,
        ok: bool,
        #[serde(default)]
        current: String,
        #[serde(default)]
        latest: Option<String>,
        #[serde(default)]
        update_available: bool,
        #[serde(default)]
        channel: String,
        #[serde(default)]
        repo: String,
        #[serde(default)]
        branch: String,
        #[serde(default)]
        message: String,
    },
    /// Fleet Control (`metrics-v1`): load and temperatures, every minute.
    FleetMetrics {
        #[serde(default)]
        interval_secs: u32,
        #[serde(default)]
        cpu_pct: Option<f32>,
        #[serde(default)]
        cpu_temp_c: Option<f32>,
        #[serde(default)]
        cells: Vec<CellMetrics>,
        #[serde(default)]
        radios: u32,
        #[serde(default)]
        calls: u32,
        #[serde(default)]
        health: String,
    },
    /// Fleet Control (`update-v1`): progress of an OTA run on the station.
    UpdateProgress {
        stage: String,
        #[serde(default)]
        percent: u8,
        #[serde(default)]
        message: String,
        #[serde(default)]
        version: String,
        #[serde(default)]
        elapsed_secs: u64,
    },
}

/// One cell in `FleetMetrics`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CellMetrics {
    pub cell: u8,
    /// `online`, `off`, `error`, `starting`.
    #[serde(default)]
    pub rf: String,
    #[serde(default)]
    pub sdr_temp_c: Option<f32>,
    #[serde(default)]
    pub detail: String,
}

struct ControlSession {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    pending_handle: HashMap<u32, oneshot::Sender<ControlResponse>>,
    pending_kick: HashMap<u32, oneshot::Sender<ControlResponse>>,
    pending_ambience: HashMap<u32, oneshot::Sender<ControlResponse>>,
}

#[derive(Default)]
pub struct ControlState {
    sessions: HashMap<String, ControlSession>,
    next_handle: AtomicU32,
}

impl ControlState {
    pub fn connected_ids(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    fn next_handle(&self) -> u32 {
        self.next_handle.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub enum SendError {
    NotConnected,
    Timeout,
}

/// Sends a command to a connected control BTS. If the command defines a
/// response, waits up to 5s for it (correlated by `handle`, or by `issi` for
/// `KickMs`) and returns it; otherwise returns `Ok(None)` once the frame is
/// written.
pub async fn send_command(state: &Arc<AppState>, bts_id: &str, mut command: ControlCommand) -> Result<Option<ControlResponse>, SendError> {
    let mut ctl = state.control.write().await;
    if !ctl.sessions.contains_key(bts_id) { return Err(SendError::NotConnected); }

    // Assign a fresh handle for correlated commands so dashboard callers
    // don't need to manage one themselves.
    let fresh_handle = ctl.next_handle();
    let session = ctl.sessions.get_mut(bts_id).expect("checked above");
    let assigned_handle = match &mut command {
        ControlCommand::SendSds { handle, .. } | ControlCommand::SendRawSdsType4 { handle, .. }
        | ControlCommand::CommandA { handle, .. } | ControlCommand::TestCmdB { handle, .. }
        | ControlCommand::FleetPing { handle } | ControlCommand::CheckUpdate { handle }
        | ControlCommand::StartUpdate { handle, .. } | ControlCommand::SetMaintenance { handle, .. }
        | ControlCommand::RestartStation { handle, .. } | ControlCommand::GetConfig { handle }
        | ControlCommand::SetConfig { handle, .. } | ControlCommand::ConfirmConfig { handle }
        | ControlCommand::RollbackConfig { handle } => {
            *handle = fresh_handle;
            Some(*handle)
        }
        _ => None,
    };

    let waiter = match &command {
        ControlCommand::KickMs { issi } => {
            let (tx, rx) = oneshot::channel();
            session.pending_kick.insert(*issi, tx);
            Some(rx)
        }
        ControlCommand::AmbienceListen { issi, .. } => {
            let (tx, rx) = oneshot::channel();
            session.pending_ambience.insert(*issi, tx);
            Some(rx)
        }
        _ if matches!(
            command,
            ControlCommand::SendSds { .. } | ControlCommand::CommandA { .. } | ControlCommand::FleetPing { .. }
                | ControlCommand::CheckUpdate { .. } | ControlCommand::StartUpdate { .. }
                | ControlCommand::SetMaintenance { .. } | ControlCommand::RestartStation { .. }
                | ControlCommand::GetConfig { .. } | ControlCommand::SetConfig { .. }
                | ControlCommand::ConfirmConfig { .. } | ControlCommand::RollbackConfig { .. }
        ) => {
            let handle = assigned_handle.or_else(|| command.handle()).expect("assigned above");
            let (tx, rx) = oneshot::channel();
            session.pending_handle.insert(handle, tx);
            Some(rx)
        }
        _ => None,
    };

    let wait = command.answer_timeout();
    let frame = serde_json::to_vec(&command.to_wire_json()).expect("control command always serializes");
    if session.tx.send(frame).is_err() {
        return Err(SendError::NotConnected);
    }
    drop(ctl);

    let Some(rx) = waiter else { return Ok(None) };
    match tokio::time::timeout(wait, rx).await {
        Ok(Ok(response)) => Ok(Some(response)),
        Ok(Err(_)) | Err(_) => Err(SendError::Timeout),
    }
}

pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    if !state.config.control.enabled {
        return Ok(());
    }
    let cfg = fsnet::ListenerConfig {
        name: "control",
        listen: state.service_bind(state.config.control.listen),
        subprotocol: "bluestation-control-v1",
        users: state.logins.control.clone(),
        tls: state.config.control.tls.clone(),
        // Fleet Control is offered whenever the control channel runs.
        extensions: Some(crate::fleet::CAPABILITY),
    };
    fsnet::serve(cfg, move |socket, identity, _peer| {
        let state = state.clone();
        async move { session(state, socket, identity).await }
    })
    .await
}

async fn session(state: Arc<AppState>, socket: WebSocket, identity: Option<String>) {
    let id = identity.unwrap_or_else(|| format!("control-{}", uuid::Uuid::new_v4().simple()));
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    {
        let mut ctl = state.control.write().await;
        ctl.sessions.insert(id.clone(), ControlSession { tx, pending_handle: HashMap::new(), pending_kick: HashMap::new(), pending_ambience: HashMap::new() });
    }
    info!(bts = %id, "Basestation control connected");
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).set_online(&id, true, crate::fleet::now_ms());
    state.monitor.emit("control_connected", serde_json::json!({"id": id}));

    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if ws_tx.send(Message::Binary(frame.into())).await.is_err() { break; }
        }
    });

    while let Some(item) = ws_rx.next().await {
        match item {
            Ok(Message::Binary(data)) => handle_response(&state, &id, &data).await,
            Ok(Message::Text(_)) => warn!(bts = %id, "unexpected text frame on control channel, ignoring"),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Err(e) => { warn!(bts = %id, error = %e, "control WebSocket receive error"); break; }
        }
    }

    writer.abort();
    state.control.write().await.sessions.remove(&id);
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).set_online(&id, false, crate::fleet::now_ms());
    state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).disconnected(&id);
    state.monitor.emit("control_disconnected", serde_json::json!({"id": id}));
    info!(bts = %id, "Basestation control disconnected");
}

async fn handle_response(state: &Arc<AppState>, id: &str, data: &[u8]) {
    let response: ControlResponse = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(e) => {
            warn!(bts = %id, error = %e, bytes = data.len(), "dropping malformed control response");
            return;
        }
    };

    if let ControlResponse::StationHello { protocol, version, brew_login, mcc, mnc, fleet_control, capabilities, config_hash, config_pending } = &response {
        let hello = crate::fleet::Hello {
            protocol: *protocol, version: version.clone(), brew_login: brew_login.clone(), mcc: *mcc, mnc: *mnc,
            fleet_control: *fleet_control, capabilities: capabilities.clone(),
        };
        let (changed, resend) = {
            let names = station_names(state);
            let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
            let changed = fleet.hello(id, &hello, crate::fleet::now_ms(), &|k| names.get(k).cloned());
            (changed, fleet.maintenance_resend(id, crate::fleet::now_ms()))
        };
        // Maintenance still on (e.g. the station restarted while it was set): set it again.
        if let Some(cmd) = resend {
            let (state, id) = (state.clone(), id.to_owned());
            tokio::spawn(async move {
                let _ = send_command(&state, &id, cmd).await;
            });
        }
        if changed {
            state.monitor.emit("fleet_changed", serde_json::json!({"id": id, "fleet_control": fleet_control}));
        }
        if *fleet_control {
            crate::fleet_config::after_hello(state, id, config_hash.clone(), *config_pending);
        }
        return;
    }
    if let ControlResponse::FleetMetrics { cpu_pct, cpu_temp_c, cells, radios, calls, health, .. } = &response {
        // Only from listed stations (the switch is on): the graphs belong to the Fleet Manager.
        if state.fleet.lock().unwrap_or_else(|e| e.into_inner()).get(id).is_some() {
            let sample = crate::fleet_monitor::sample_from_report(*cpu_pct, *cpu_temp_c, cells, *radios, *calls, health);
            state.fleet_monitor.lock().unwrap_or_else(|e| e.into_inner()).record(id, sample, crate::fleet::now_ms());
        }
        return;
    }
    if let ControlResponse::UpdateProgress { stage, percent, message, version, .. } = &response {
        let names = station_names(state);
        state.fleet.lock().unwrap_or_else(|e| e.into_inner()).progress(id, stage, *percent, message, version, crate::fleet::now_ms(), &|k| names.get(k).cloned());
        state.monitor.emit("fleet_changed", serde_json::json!({"id": id}));
        return;
    }
    let mut ctl = state.control.write().await;
    let Some(session) = ctl.sessions.get_mut(id) else { return };
    match &response {
        ControlResponse::KickMsResponse { issi, .. } => {
            if let Some(tx) = session.pending_kick.remove(issi) { let _ = tx.send(response); }
        }
        ControlResponse::AmbienceListenResponse { issi, .. } => {
            if let Some(tx) = session.pending_ambience.remove(issi) { let _ = tx.send(response); }
        }
        ControlResponse::CommandAResponse { handle, .. } | ControlResponse::SendSdsResponse { handle, .. }
        | ControlResponse::CommandResult { handle, .. } | ControlResponse::UpdateInfo { handle, .. }
        | ControlResponse::ConfigInfo { handle, .. } => {
            if let Some(tx) = session.pending_handle.remove(handle) { let _ = tx.send(response); }
        }
        ControlResponse::StationHello { .. } | ControlResponse::UpdateProgress { .. } | ControlResponse::FleetMetrics { .. } => {}
    }
}

/// Station names given on this server (Accounts: login -> name).
pub(crate) fn station_names(state: &AppState) -> HashMap<String, String> {
    state.locations.read().map(|l| l.iter().filter(|(_, v)| !v.name.is_empty()).map(|(k, v)| (k.clone(), v.name.clone())).collect()).unwrap_or_default()
}

/// Test hook: registers a control session for `id` without a WebSocket and
/// returns the frames the server sends to it.
#[cfg(test)]
pub(crate) async fn attach_test_session(state: &Arc<AppState>, id: &str) -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.control.write().await.sessions.insert(id.to_string(), ControlSession {
        tx, pending_handle: HashMap::new(), pending_kick: HashMap::new(), pending_ambience: HashMap::new(),
    });
    rx
}

/// Test hook: feeds a response frame as if the Basestation `id` had sent it.
#[cfg(test)]
pub(crate) async fn deliver_test_response(state: &Arc<AppState>, id: &str, data: &[u8]) {
    handle_response(state, id, data).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambience_listen_wire_shape() {
        // Externally-tagged, matching the Basestation's derive-based codec.
        let json = ControlCommand::AmbienceListen { issi: 2001, enable: true }.to_wire_json();
        assert_eq!(json, serde_json::json!({"AmbienceListen": {"issi": 2001, "enable": true}}));
    }

    #[test]
    fn ambience_listen_response_parses() {
        let r: ControlResponse =
            serde_json::from_value(serde_json::json!({"AmbienceListenResponse": {"issi": 2001, "success": true}})).unwrap();
        assert!(matches!(r, ControlResponse::AmbienceListenResponse { issi: 2001, success: true }));
    }

    fn test_state() -> Arc<AppState> {
        let (state, _rx) = AppState::new(crate::config::Config::default(), std::path::PathBuf::from("test.toml"));
        Arc::new(state)
    }

    #[test]
    fn fleet_wire_shapes_match_flowstation() {
        // Same JSON as FlowStation's net_control::fleet tests.
        let json = ControlCommand::FleetPing { handle: 5 }.to_wire_json();
        assert_eq!(json, serde_json::json!({"FleetPing": {"handle": 5}}));
        let hello: ControlResponse = serde_json::from_value(serde_json::json!({"StationHello": {"protocol": 1, "version": "v0.5.9-abc", "brew_login": "1000001", "mcc": 232, "mnc": 20, "fleet_control": true, "capabilities": ["fleet-v1"]}})).unwrap();
        assert!(matches!(hello, ControlResponse::StationHello { protocol: 1, fleet_control: true, .. }));
        let res: ControlResponse = serde_json::from_value(serde_json::json!({"CommandResult": {"handle": 3, "ok": false, "code": "fleet_control_off", "message": "off"}})).unwrap();
        assert!(matches!(res, ControlResponse::CommandResult { handle: 3, ok: false, .. }));
    }

    #[tokio::test]
    async fn hello_lists_and_unlists_the_station() {
        let state = test_state();
        let _rx = attach_test_session(&state, "bs1").await;
        let on = br#"{"StationHello":{"protocol":1,"version":"v1","brew_login":"1000001","mcc":232,"mnc":20,"fleet_control":true,"capabilities":["fleet-v1"]}}"#;
        deliver_test_response(&state, "bs1", on).await;
        assert_eq!(state.fleet.lock().unwrap().stations().len(), 1);
        let off = br#"{"StationHello":{"protocol":1,"version":"v1","brew_login":"1000001","mcc":232,"mnc":20,"fleet_control":false,"capabilities":[]}}"#;
        deliver_test_response(&state, "bs1", off).await;
        let fleet = state.fleet.lock().unwrap();
        assert!(fleet.stations().is_empty());
        assert_eq!(fleet.log()[0].text, "Basestation 1000001 switched off Fleet Control");
    }

    /// `metrics-v1`: 10 listed stations with 25 radios report; the graphs get
    /// the values, a station that is not listed (switch off) is ignored.
    #[tokio::test]
    async fn fleet_metrics_from_ten_stations_fill_the_graphs() {
        let state = test_state();
        for i in 1..=10 {
            let id = format!("bs{i}");
            let _rx = attach_test_session(&state, &id).await;
            let hello = serde_json::json!({"StationHello": {"protocol": 1, "version": "v1", "brew_login": id, "mcc": 232, "mnc": 20, "fleet_control": true, "capabilities": ["fleet-v1", "metrics-v1"]}});
            deliver_test_response(&state, &id, &serde_json::to_vec(&hello).unwrap()).await;
            let radios = 25 / 10 + u32::from(i <= 25 % 10);
            let m = serde_json::json!({"FleetMetrics": {"interval_secs": 60, "cpu_pct": 10.0 + i as f32, "cpu_temp_c": 50.0, "cells": [{"cell": 0, "rf": "online", "sdr_temp_c": 40.0 + i as f32}], "radios": radios, "calls": 1, "health": "ok"}});
            deliver_test_response(&state, &id, &serde_json::to_vec(&m).unwrap()).await;
        }
        let _rx = attach_test_session(&state, "stranger").await;
        let m = br#"{"FleetMetrics":{"interval_secs":60,"cpu_pct":99.0,"cpu_temp_c":99.0,"cells":[],"radios":1,"calls":0,"health":"ok"}}"#;
        deliver_test_response(&state, "stranger", m).await;
        let mon = state.fleet_monitor.lock().unwrap();
        let mut total = 0;
        for i in 1..=10 {
            let (pts, _) = mon.graph(&format!("bs{i}"), 0);
            assert_eq!(pts.len(), 1);
            assert_eq!(pts[0].c, Some(10.0 + i as f32));
            assert_eq!(pts[0].s[&0], 40.0 + i as f32);
            total += pts[0].r.unwrap();
        }
        assert_eq!(total, 25);
        assert!(mon.graph("stranger", 0).0.is_empty(), "not in Fleet Control: no graph");
    }

    #[tokio::test]
    async fn fleet_ping_gets_its_command_result() {
        let state = test_state();
        let mut rx = attach_test_session(&state, "bs1").await;
        let st = state.clone();
        let ask = tokio::spawn(async move { send_command(&st, "bs1", ControlCommand::FleetPing { handle: 0 }).await });
        let frame = rx.recv().await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        let handle = v["FleetPing"]["handle"].as_u64().unwrap();
        let answer = serde_json::json!({"CommandResult": {"handle": handle, "ok": true, "code": "ok", "message": "Fleet Control is on"}});
        deliver_test_response(&state, "bs1", &serde_json::to_vec(&answer).unwrap()).await;
        let r = ask.await.unwrap().unwrap();
        assert!(matches!(r, Some(ControlResponse::CommandResult { ok: true, .. })));
    }

    /// A FlowStation stand-in: answers update checks (newer version), starts,
    /// reports progress, restarts and comes back with `v2`. Records when its
    /// update ran so the test can check that rolling never overlaps.
    async fn fake_station(state: Arc<AppState>, id: String, mut rx: mpsc::UnboundedReceiver<Vec<u8>>, log: Arc<std::sync::Mutex<Vec<(String, &'static str)>>>) {
        let hello = |v: &str| serde_json::to_vec(&serde_json::json!({"StationHello": {"protocol": 1, "version": v, "brew_login": id, "mcc": 232, "mnc": 20, "fleet_control": true, "capabilities": ["fleet-v1", "update-v1"]}})).unwrap();
        deliver_test_response(&state, &id, &hello("v1")).await;
        while let Some(frame) = rx.recv().await {
            let v: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            if let Some(h) = v.get("CheckUpdate") {
                let a = serde_json::json!({"UpdateInfo": {"handle": h["handle"], "ok": true, "current": "v1", "latest": "v2", "update_available": true, "channel": "installed", "repo": "https://github.com/sebastianmadl/bost-flowstation.git", "branch": "fleet-control", "message": "newer version available"}});
                deliver_test_response(&state, &id, &serde_json::to_vec(&a).unwrap()).await;
            } else if let Some(h) = v.get("StartUpdate") {
                log.lock().unwrap().push((id.clone(), "start"));
                let a = serde_json::json!({"CommandResult": {"handle": h["handle"], "ok": true, "code": "ok", "message": "Update started"}});
                deliver_test_response(&state, &id, &serde_json::to_vec(&a).unwrap()).await;
                let st = state.clone();
                let id2 = id.clone();
                let log2 = log.clone();
                tokio::spawn(async move {
                    for (stage, pct) in [("download", 5), ("build", 50), ("install", 92), ("restart", 100)] {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        let p = serde_json::json!({"UpdateProgress": {"stage": stage, "percent": pct, "message": "", "version": "v1", "elapsed_secs": 0}});
                        deliver_test_response(&st, &id2, &serde_json::to_vec(&p).unwrap()).await;
                    }
                    st.fleet.lock().unwrap().set_online(&id2, false, crate::fleet::now_ms());
                    tokio::time::sleep(Duration::from_secs(90)).await;
                    log2.lock().unwrap().push((id2.clone(), "back"));
                    let hello2 = serde_json::to_vec(&serde_json::json!({"StationHello": {"protocol": 1, "version": "v2", "brew_login": id2, "mcc": 232, "mnc": 20, "fleet_control": true, "capabilities": ["fleet-v1", "update-v1"]}})).unwrap();
                    deliver_test_response(&st, &id2, &hello2).await;
                });
            }
        }
    }

    async fn ten_station_run(mode: crate::fleet_update::Mode) -> (Arc<AppState>, Vec<(String, &'static str)>) {
        let state = test_state();
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        for i in 1..=10 {
            let id = format!("10000{i:02}");
            let rx = attach_test_session(&state, &id).await;
            tokio::spawn(fake_station(state.clone(), id, rx, log.clone()));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(state.fleet.lock().unwrap().stations().len(), 10);
        // Without a check nothing is updatable.
        let first = state.fleet.lock().unwrap().stations()[0].id.clone();
        assert!(!state.fleet.lock().unwrap().updatable(&first));
        let r = crate::fleet::api_check(axum::extract::State(state.clone()), axum::http::HeaderMap::new(), axum::Json(crate::fleet::IdsRequest::default())).await;
        assert_eq!(r.status(), 200);
        let ids: Vec<String> = state.fleet.lock().unwrap().stations().into_iter().map(|s| s.id).collect();
        assert!(ids.iter().all(|id| state.fleet.lock().unwrap().updatable(id)), "every station found a newer version");
        let req = crate::fleet::UpdateRequest { ids: vec![], mode, during_call: false, interrupt_min: 10, live: false };
        let r = crate::fleet::api_update(axum::extract::State(state.clone()), axum::http::HeaderMap::new(), axum::Json(req)).await;
        assert_eq!(r.status(), 200);
        tokio::spawn(crate::fleet::run_updates(state.clone()));
        for _ in 0..2000 {
            tokio::time::sleep(Duration::from_secs(10)).await;
            if state.fleet.lock().unwrap().job().is_some_and(|j| j.is_finished()) {
                break;
            }
        }
        let l = log.lock().unwrap().clone();
        (state, l)
    }

    #[tokio::test(start_paused = true)]
    async fn rolling_update_of_ten_stations_one_at_a_time() {
        let (state, log) = ten_station_run(crate::fleet_update::Mode::Rolling).await;
        let fleet = state.fleet.lock().unwrap();
        let job = fleet.job().unwrap();
        assert!(job.is_finished(), "the run ends");
        assert!(job.items.iter().all(|i| matches!(i.state, crate::fleet_update::ItemState::Done { .. })), "{:?}", job.items);
        // Rolling: start, back, start, back, ... never two at once.
        assert_eq!(log.len(), 20);
        for pair in log.chunks(2) {
            assert_eq!((pair[0].1, pair[1].1), ("start", "back"));
            assert_eq!(pair[0].0, pair[1].0, "the next station starts only after the previous is back");
        }
        assert!(fleet.stations().iter().all(|s| s.version == "v2"));
        let texts: Vec<String> = fleet.log().iter().map(|e| e.text.clone()).collect();
        assert!(texts.iter().any(|t| t == "Update run finished: 10 updated, 0 failed, 0 skipped"), "{texts:?}");
        // The checks were for v1: after the update they are dropped, a new check is needed.
        assert!(fleet.stations().iter().all(|s| s.check.is_none()));
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_update_of_ten_stations_at_once() {
        let (state, log) = ten_station_run(crate::fleet_update::Mode::Parallel).await;
        let job = state.fleet.lock().unwrap().job().unwrap().clone();
        assert!(job.is_finished());
        assert!(job.items.iter().all(|i| matches!(i.state, crate::fleet_update::ItemState::Done { .. })));
        assert!(log[..10].iter().all(|(_, what)| *what == "start"), "all ten start before any is back");
    }

    #[test]
    fn update_wire_shapes_match_flowstation() {
        // Same JSON as FlowStation's net_control::fleet tests.
        assert_eq!(ControlCommand::CheckUpdate { handle: 9 }.to_wire_json(), serde_json::json!({"CheckUpdate": {"handle": 9}}));
        assert_eq!(ControlCommand::StartUpdate { handle: 11, during_call: true, source: None }.to_wire_json(), serde_json::json!({"StartUpdate": {"handle": 11, "during_call": true}}));
        let live = crate::fleet_update::LiveSource { repo: "https://github.com/sebastianmadl/bost-flowstation".into(), branch: "fleet-control".into() };
        assert_eq!(
            ControlCommand::StartUpdate { handle: 12, during_call: false, source: Some(live) }.to_wire_json(),
            serde_json::json!({"StartUpdate": {"handle": 12, "during_call": false, "source": {"repo": "https://github.com/sebastianmadl/bost-flowstation", "branch": "fleet-control"}}})
        );
        let info: ControlResponse = serde_json::from_value(serde_json::json!({"UpdateInfo": {"handle": 9, "ok": true, "current": "v0.5.9-aaa", "latest": "v0.5.9-bbb", "update_available": true, "channel": "installed", "repo": "https://github.com/sebastianmadl/bost-flowstation", "branch": "fleet-control", "message": "newer version available"}})).unwrap();
        assert!(matches!(info, ControlResponse::UpdateInfo { handle: 9, update_available: true, .. }));
        let p: ControlResponse = serde_json::from_value(serde_json::json!({"UpdateProgress": {"stage": "build", "percent": 42, "message": "Compiling x", "version": "v0.5.9-aaa", "elapsed_secs": 120}})).unwrap();
        assert!(matches!(p, ControlResponse::UpdateProgress { percent: 42, .. }));
        assert_eq!(ControlCommand::CheckUpdate { handle: 0 }.answer_timeout(), Duration::from_secs(60));
        assert_eq!(
            ControlCommand::SetMaintenance { handle: 3, on: true, minutes: 30, by: "oe4sxm".into(), reason: String::new() }.to_wire_json(),
            serde_json::json!({"SetMaintenance": {"handle": 3, "on": true, "minutes": 30, "by": "oe4sxm", "reason": ""}})
        );
        assert_eq!(
            ControlCommand::RestartStation { handle: 4, host: true, during_call: false }.to_wire_json(),
            serde_json::json!({"RestartStation": {"handle": 4, "host": true, "during_call": false}})
        );
    }

}
