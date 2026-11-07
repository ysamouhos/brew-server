//! Dashboard API of the Routing page: the registered radios with their groups
//! and last DGNA answer, DGNA to several radios at once, and the talkgroup
//! pools (see `pools`).

use crate::config::TalkgroupPoolConfig;
use crate::control::{self, ControlCommand, SendError};
use crate::state::{AppState, ClientMode};
use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Longest group name a DGNA carries (FlowStation's limit too).
const MAX_MNEMONIC: usize = 15;

#[derive(Debug, Serialize)]
pub struct RadioView {
    pub issi: u32,
    /// Brew login of the station (or connection) the radio is registered through.
    pub station: String,
    /// `login/cell`, when the station reported the cell.
    pub cell: Option<String>,
    /// Whether the registration came from a Basestation (a radio) rather
    /// than a terminal such as an app, which DGNA cannot reach.
    pub radio: bool,
    /// Groups the radio is in, as Brew knows them.
    pub groups: Vec<u32>,
    /// The station's view of the radio's groups (with names, DGNA flag),
    /// when its telemetry reported them.
    pub catalog: Vec<crate::telemetry::MsGroupInfo>,
    /// Last DGNA answer and when it came (ms since the epoch).
    pub dgna: Option<crate::telemetry::DgnaStatusInfo>,
    pub dgna_at_ms: Option<u64>,
}

/// `GET /api/radios`: everyone registered here, for the Routing page.
pub async fn radios(State(state): State<Arc<AppState>>) -> Json<Vec<RadioView>> {
    let inner = state.inner.read().await;
    let t = state.telemetry.read().await;
    let mut out: Vec<RadioView> = inner.subscribers.iter()
        .filter(|(_, s)| s.route.path.is_empty() && inner.clients.get(&s.client_id).is_some_and(|c| c.mode != ClientMode::Peer))
        .map(|(&issi, s)| {
            let username = inner.clients.get(&s.client_id).and_then(|c| c.username.clone());
            let station = crate::carriers::station_name(s.client_id, username.as_deref());
            let cell = inner.carriers.ms_cell(issi, s.client_id).map(|c| crate::carriers::cell_name(&station, c.cell));
            let mut groups: Vec<u32> = s.groups.iter().copied().collect();
            groups.sort_unstable();
            let (dgna, dgna_at_ms) = match t.dgna_status.get(&issi) {
                Some((d, at)) => (Some(d.clone()), Some(*at)),
                None => (None, None),
            };
            RadioView {
                issi, station, cell, radio: s.mode == ClientMode::Basestation, groups,
                catalog: t.ms_groups.get(&issi).cloned().unwrap_or_default(),
                dgna, dgna_at_ms,
            }
        })
        .collect();
    out.sort_unstable_by_key(|r| r.issi);
    Json(out)
}

#[derive(Debug, Deserialize)]
pub struct DgnaRequest {
    pub issis: Vec<u32>,
    pub gssi: u32,
    #[serde(default)]
    pub name: Option<String>,
    /// `true` assigns the group, `false` takes it away again.
    #[serde(default = "yes")]
    pub assign: bool,
    #[serde(default)]
    pub attachment_mode: u8,
}

fn yes() -> bool { true }

#[derive(Debug, Serialize)]
pub struct DgnaResult {
    pub issi: u32,
    /// Control login of the station the command went to.
    pub station: Option<String>,
    pub sent: bool,
    pub error: Option<String>,
}

/// The control session to reach `issi` through: the station whose telemetry
/// saw it register last, or the only control session when there is just one.
fn control_station_for(issi: u32, last_registration: &std::collections::HashMap<u32, String>, sessions: &[String]) -> Option<String> {
    if let Some(s) = last_registration.get(&issi).filter(|s| sessions.contains(s)) {
        return Some(s.clone());
    }
    (sessions.len() == 1).then(|| sessions[0].clone())
}

/// `POST /api/dgna` (admins): DGNA to several radios at once, each through
/// the station it is registered on. The radios' answers come in over
/// telemetry and show on `/api/radios`.
pub async fn dgna(State(state): State<Arc<AppState>>, Json(req): Json<DgnaRequest>) -> Response {
    if req.gssi == 0 || req.gssi > 0xFF_FFFF {
        return (StatusCode::BAD_REQUEST, "GSSI must be 1 to 16777215").into_response();
    }
    if req.issis.is_empty() || req.issis.len() > 500 {
        return (StatusCode::BAD_REQUEST, "select 1 to 500 radios").into_response();
    }
    if req.attachment_mode > 5 {
        return (StatusCode::BAD_REQUEST, "attachment mode must be 0 to 5").into_response();
    }
    let name = req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
    if name.as_ref().is_some_and(|n| n.chars().count() > MAX_MNEMONIC) {
        return (StatusCode::BAD_REQUEST, "group name is longer than 15 characters").into_response();
    }
    let sessions: Vec<String> = state.control.read().await.connected_ids();
    let last = state.telemetry.read().await.last_registration.clone();
    let mut results = Vec::new();
    for &issi in &req.issis {
        let Some(station) = control_station_for(issi, &last, &sessions) else {
            results.push(DgnaResult { issi, station: None, sent: false, error: Some("no control connection to the radio's station".into()) });
            continue;
        };
        let cmd = ControlCommand::Dgna { issi, gssi: req.gssi, mnemonic: name.clone(), attachment_mode: req.attachment_mode, attach: req.assign };
        let r = control::send_command(&state, &station, cmd).await;
        let error = match r {
            Ok(_) => None,
            Err(SendError::NotConnected) => Some("station not connected".into()),
            Err(SendError::Timeout) => Some("no answer from the station".into()),
        };
        tracing::warn!(issi, gssi = req.gssi, assign = req.assign, station = %station, ok = error.is_none(), "DGNA sent from the Routing page");
        results.push(DgnaResult { issi, station: Some(station), sent: error.is_none(), error });
    }
    Json(results).into_response()
}

/// `GET /api/pools`: the pools and their running calls.
pub async fn pools_get(State(state): State<Arc<AppState>>) -> Json<Vec<crate::pools::PoolView>> {
    Json(crate::pools::snapshot(&state).await)
}

/// `POST /api/pools/save` (admins): the whole pool list, applied at once and
/// written to the config file without a restart.
pub async fn pools_put(State(state): State<Arc<AppState>>, Json(pools): Json<Vec<TalkgroupPoolConfig>>) -> Response {
    match crate::pools::apply(&state, pools, "dashboard").await {
        Ok(()) => Json(serde_json::json!({"saved": true})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

/// `GET /api/networks` (admins): Multi Network, the other networks and the
/// state of their links (never a password).
pub async fn networks_get(State(state): State<Arc<AppState>>) -> Json<crate::netlink::MultiNetworkView> {
    Json(crate::netlink::snapshot(&state).await)
}

/// `POST /api/networks/save` (admins): the whole `[multi_network]` section,
/// applied at once (only changed links reconnect) and written to the config
/// file without a restart. An empty password keeps the saved one.
pub async fn networks_put(State(state): State<Arc<AppState>>, Json(cfg): Json<crate::config::MultiNetworkConfig>) -> Response {
    match crate::netlink::apply(&state, cfg, "dashboard").await {
        Ok(()) => Json(serde_json::json!({"saved": true})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dgna_goes_to_the_station_the_radio_registered_on() {
        let last = std::collections::HashMap::from([(7, "100000002".to_string())]);
        let sessions = vec!["100000001".to_string(), "100000002".to_string()];
        assert_eq!(control_station_for(7, &last, &sessions).as_deref(), Some("100000002"));
        assert_eq!(control_station_for(8, &last, &sessions), None);
        assert_eq!(control_station_for(8, &last, &sessions[..1]).as_deref(), Some("100000001"));
    }
}
