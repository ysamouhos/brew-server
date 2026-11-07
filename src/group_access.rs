//! Who may use a group (`[group_access."GSSI"]`): an allow list (only the
//! listed ISSIs) or a block list (everyone but the listed ISSIs); no entry or
//! an empty list leaves the group open. `rx_only` makes radios listeners only
//! (all, or the ones in `rx_only_issis`). The server records an affiliation
//! only for an admitted ISSI and drops a group call from one that may not
//! talk (emergency calls always pass), so such a radio neither talks on the
//! group across the network nor (when not admitted) pulls the group's calls
//! to its station.
//!
//! A Basestation that announced the `group-access` capability gets the lists
//! (`SERVICE_GROUP_ACCESS`) after each `SERVICE_BTS_INFO` and on every change,
//! and refuses the attachment or the request to talk on the air itself. Base
//! Brew has no such message, so a Basestation without it still lets the radio
//! attach and talk locally.
//!
//! Edited live on the dashboard's Directory page and written to the config
//! file without a restart.

use crate::config::{GroupAccessConfig, GroupAccessMode};
use crate::state::{AppState, ClientId, ClientMode, Inner};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

const MAX_ISSIS: usize = 5_000;

/// Capability in `SERVICE_BTS_INFO` (and extension in `X-Brew-Extensions`) for
/// `SERVICE_GROUP_ACCESS`.
pub const CAPABILITY: &str = "group-access";

/// `SERVICE_GROUP_ACCESS` for every Basestation that announced the
/// `group-access` capability (or only for `only`): the whole table,
/// `{"groups":[{"gssi":N,"mode":"allow"|"block","issis":[..],"rx_only":bool,
/// "rx_only_issis":[..]}]}`, groups sorted by GSSI. An empty list means every
/// group is open.
pub fn updates(state: &AppState, inner: &Inner, only: Option<ClientId>) -> Vec<(ClientId, Vec<u8>)> {
    let table = state.group_access.read().map(|g| g.clone()).unwrap_or_default();
    let mut groups: Vec<(u32, GroupAccessConfig)> = table.into_iter().filter_map(|(k, a)| Some((k.trim().parse().ok()?, a))).collect();
    groups.sort_by_key(|(g, _)| *g);
    let groups: Vec<serde_json::Value> = groups
        .into_iter()
        .filter(|(_, a)| !a.is_open())
        .map(|(gssi, a)| serde_json::json!({
            "gssi": gssi, "mode": a.mode, "issis": a.issis, "rx_only": a.rx_only, "rx_only_issis": a.rx_only_issis,
        }))
        .collect();
    let packet = crate::protocol::build_service(crate::protocol::SERVICE_GROUP_ACCESS, &serde_json::json!({ "groups": groups }).to_string());
    inner
        .clients
        .keys()
        .filter(|id| only.is_none_or(|o| o == **id))
        .filter(|id| crate::handover::announced(inner, **id, CAPABILITY))
        .map(|id| (*id, packet.clone()))
        .collect()
}

fn snapshot(state: &AppState) -> BTreeMap<String, GroupAccessConfig> {
    state.group_access.read().map(|g| g.clone()).unwrap_or_default()
}

/// ISSIs separated by commas, semicolons, spaces or new lines.
pub fn parse_issis(text: &str) -> Result<Vec<u32>, String> {
    let mut out = Vec::new();
    for t in text.split([',', ';', ' ', '\t', '\n', '\r']).map(str::trim).filter(|t| !t.is_empty()) {
        match t.parse::<u32>() {
            Ok(n) if (1..=0xFF_FFFF).contains(&n) => if !out.contains(&n) { out.push(n) },
            _ => return Err(format!("'{t}' is not an ISSI (1 to 16777215)")),
        }
    }
    if out.len() > MAX_ISSIS {
        return Err(format!("at most {MAX_ISSIS} ISSIs per group"));
    }
    out.sort_unstable();
    Ok(out)
}

/// Forgets the affiliations to `gssi` of radios on this server's own
/// stations that the group no longer admits, so its calls stop going to a
/// station only for them. Federation peers are told as for a deaffiliation.
async fn drop_affiliations(state: &Arc<AppState>, gssi: u32) {
    let mut inner = state.inner.write().await;
    let gone: Vec<u32> = inner
        .subscribers
        .iter()
        .filter(|(issi, s)| {
            s.groups.contains(&gssi)
                && !state.group_permits(gssi, **issi)
                && inner.clients.get(&s.client_id).is_some_and(|c| c.mode != ClientMode::Peer)
        })
        .map(|(issi, _)| *issi)
        .collect();
    for issi in gone {
        let Some(old) = inner.subscribers.get(&issi).cloned() else { continue };
        let mut sub = old.clone();
        sub.groups.remove(&gssi);
        crate::fedroute::set_effective(&mut inner, issi, Some(sub));
        crate::fedroute::publish(&inner, issi, Some(&old));
        tracing::info!(issi, gssi, "affiliation dropped: the group no longer admits this ISSI (group_access)");
    }
}

/// `GET /api/group-access`: GSSI -> mode and ISSIs.
pub async fn list(State(state): State<Arc<AppState>>) -> Json<BTreeMap<String, GroupAccessConfig>> {
    Json(snapshot(&state))
}

#[derive(Debug, Deserialize)]
pub struct AccessRequest {
    pub gssi: String,
    #[serde(default)]
    pub mode: GroupAccessMode,
    /// ISSIs as text (commas, spaces or new lines). Empty: everyone may join.
    #[serde(default)]
    pub issis: String,
    /// Listeners only: all radios, or those in `rx_only_issis`.
    #[serde(default)]
    pub rx_only: bool,
    #[serde(default)]
    pub rx_only_issis: String,
}

/// `POST /api/group-access/set` (admins): sets the list of one group; an
/// empty list removes the entry (open group).
pub async fn set(State(state): State<Arc<AppState>>, Json(req): Json<AccessRequest>) -> Response {
    let gssi = match req.gssi.trim().parse::<u32>() {
        Ok(n) if (1..=0xFF_FFFF).contains(&n) => n,
        _ => return (StatusCode::BAD_REQUEST, format!("'{}' is not a GSSI (1 to 16777215)", req.gssi.trim())).into_response(),
    };
    let issis = match parse_issis(&req.issis) { Ok(i) => i, Err(e) => return (StatusCode::BAD_REQUEST, e).into_response() };
    let rx_only_issis = match parse_issis(&req.rx_only_issis) { Ok(i) => i, Err(e) => return (StatusCode::BAD_REQUEST, e).into_response() };
    let entry = GroupAccessConfig { mode: req.mode, issis, rx_only: req.rx_only, rx_only_issis };
    if let Ok(mut g) = state.group_access.write() {
        if entry.is_open() {
            g.remove(&gssi.to_string());
        } else {
            g.insert(gssi.to_string(), entry.clone());
        }
    }
    tracing::warn!(gssi, mode = ?entry.mode, issis = entry.issis.len(), rx_only = entry.rx_only, rx_only_issis = entry.rx_only_issis.len(), "group access changed from the dashboard");
    drop_affiliations(&state, gssi).await;
    {
        let inner = state.inner.read().await;
        for (to, packet) in updates(&state, &inner, None) {
            if let Some(client) = inner.clients.get(&to) { let _ = client.tx.send(packet); }
        }
    }
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    match crate::dashboard::save_config(&state, &cfg).await {
        Ok(_) => Json(snapshot(&state)).into_response(),
        Err(e) => {
            crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("applied, but writing the config file failed: {e}")).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_and_block_lists() {
        let open = GroupAccessConfig::default();
        assert!(open.permits(1), "an empty list is open");
        let allow = GroupAccessConfig { mode: GroupAccessMode::Allow, issis: vec![2320972], ..Default::default() };
        assert!(allow.permits(2320972) && !allow.permits(2321100));
        let block = GroupAccessConfig { mode: GroupAccessMode::Block, issis: vec![2320972], ..Default::default() };
        assert!(!block.permits(2320972) && block.permits(2321100));
        // RX only: everyone listens, nobody talks; with a list only those listen only.
        let rx_all = GroupAccessConfig { rx_only: true, ..Default::default() };
        assert!(rx_all.permits(5) && !rx_all.may_talk(5) && !rx_all.is_open());
        let rx_some = GroupAccessConfig { mode: GroupAccessMode::Block, issis: vec![7], rx_only: true, rx_only_issis: vec![6] };
        assert!(!rx_some.may_talk(6) && rx_some.permits(6) && rx_some.may_talk(5) && !rx_some.may_talk(7));
        assert!(GroupAccessConfig { rx_only_issis: vec![6], ..Default::default() }.is_open(), "a list without the switch changes nothing");
        assert_eq!(parse_issis("2321100, 2320972\n2320972;9").unwrap(), vec![9, 2320972, 2321100]);
        assert!(parse_issis("12 abc").is_err() && parse_issis("16777216").is_err());
        let cfg: crate::config::Config = toml::from_str("[group_access.\"2321\"]\nmode = \"block\"\nissis = [4013]\n").unwrap();
        assert_eq!(cfg.group_access["2321"], GroupAccessConfig { mode: GroupAccessMode::Block, issis: vec![4013], ..Default::default() });
        assert!(crate::config::Config::parse("[group_access.\"x\"]\nissis = [1]\n").is_err());
        assert!(crate::config::Config::parse("[group_access.\"2321\"]\nissis = [0]\n").is_err());
    }
}
