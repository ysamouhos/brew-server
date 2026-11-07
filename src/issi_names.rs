//! Directory of the dashboard: names for ISSIs (`[issi_names]`) and for
//! groups (`[gssi_names]`), shown next to the number on the dashboard pages.
//! Edited live and written to the config file without a restart.

use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

const MAX_NAME: usize = 60;
const MAX_ENTRIES: usize = 20_000;

type Names = std::sync::RwLock<BTreeMap<String, String>>;

/// Which list: radios (ISSI) or groups (GSSI). Both are 24-bit short
/// subscriber identities.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind { Issi, Gssi }

impl Kind {
    fn label(self) -> &'static str { match self { Kind::Issi => "ISSI", Kind::Gssi => "GSSI" } }
    fn names(self, state: &AppState) -> &Names {
        match self { Kind::Issi => &state.issi_names, Kind::Gssi => &state.gssi_names }
    }
}

fn snapshot(state: &AppState, kind: Kind) -> BTreeMap<String, String> {
    kind.names(state).read().map(|n| n.clone()).unwrap_or_default()
}

fn parse_ssi(s: &str, kind: Kind) -> Result<String, String> {
    match s.trim().parse::<u32>() {
        Ok(n) if (1..=0xFF_FFFF).contains(&n) => Ok(n.to_string()),
        _ => Err(format!("'{}' is not an {} (1 to 16777215)", s.trim(), kind.label())),
    }
}

fn check_name(name: &str) -> Result<(), String> {
    if name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(format!("name: up to {MAX_NAME} characters"));
    }
    Ok(())
}

/// Lines of `ISSI name`, `ISSI,name`, `ISSI;name` or `ISSI<tab>name`; empty
/// lines and lines starting with `#` are skipped, a header line without a
/// number in front too.
fn parse_list(text: &str, kind: Kind) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let split = line.find([',', ';', '\t', ' ']).unwrap_or(line.len());
        let (issi, name) = line.split_at(split);
        if i == 0 && issi.trim().parse::<u32>().is_err() { continue; } // header
        let issi = parse_ssi(issi, kind).map_err(|e| format!("line {}: {e}", i + 1))?;
        let name = name.trim_start_matches([',', ';', '\t', ' ']).trim().trim_matches('"').to_owned();
        check_name(&name).map_err(|e| format!("line {}: {e}", i + 1))?;
        out.push((issi, name));
    }
    Ok(out)
}

async fn save(state: &Arc<AppState>) -> Result<(), String> {
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    let saved = crate::dashboard::save_config(state, &cfg).await;
    if saved.is_err() {
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    saved.map_err(|e| format!("applied, but writing the config file failed: {e}"))
}

/// `GET /api/issi-names`: ISSI -> name.
pub async fn list(State(state): State<Arc<AppState>>) -> Json<BTreeMap<String, String>> {
    Json(snapshot(&state, Kind::Issi))
}

/// `GET /api/gssi-names`: GSSI -> name.
pub async fn list_gssi(State(state): State<Arc<AppState>>) -> Json<BTreeMap<String, String>> {
    Json(snapshot(&state, Kind::Gssi))
}

#[derive(Debug, Deserialize)]
pub struct NameRequest {
    pub issi: String,
    /// Empty removes the entry.
    #[serde(default)]
    pub name: String,
}

/// `POST /api/issi-names/set` (admins): sets or (empty name) removes one name.
pub async fn set(State(state): State<Arc<AppState>>, Json(req): Json<NameRequest>) -> Response {
    set_name(&state, Kind::Issi, req).await
}

/// `POST /api/gssi-names/set` (admins): the same for a group (`issi` = GSSI).
pub async fn set_gssi(State(state): State<Arc<AppState>>, Json(req): Json<NameRequest>) -> Response {
    set_name(&state, Kind::Gssi, req).await
}

async fn set_name(state: &Arc<AppState>, kind: Kind, req: NameRequest) -> Response {
    let issi = match parse_ssi(&req.issi, kind) { Ok(i) => i, Err(e) => return (StatusCode::BAD_REQUEST, e).into_response() };
    let name = req.name.trim().to_owned();
    if let Err(e) = check_name(&name) { return (StatusCode::BAD_REQUEST, e).into_response(); }
    if let Ok(mut n) = kind.names(state).write() {
        if name.is_empty() {
            n.remove(&issi);
        } else {
            if n.len() >= MAX_ENTRIES && !n.contains_key(&issi) {
                return (StatusCode::BAD_REQUEST, format!("at most {MAX_ENTRIES} names")).into_response();
            }
            n.insert(issi, name);
        }
    }
    match save(state).await {
        Ok(()) => Json(snapshot(state, kind)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub text: String,
    /// `true` replaces the whole directory, `false` adds and overwrites.
    #[serde(default)]
    pub replace: bool,
}

/// `POST /api/issi-names/import` (admins): a pasted list, one ISSI and name
/// per line. Nothing changes when a line is wrong.
pub async fn import(State(state): State<Arc<AppState>>, Json(req): Json<ImportRequest>) -> Response {
    import_names(&state, Kind::Issi, req).await
}

/// `POST /api/gssi-names/import` (admins): the same for groups.
pub async fn import_gssi(State(state): State<Arc<AppState>>, Json(req): Json<ImportRequest>) -> Response {
    import_names(&state, Kind::Gssi, req).await
}

async fn import_names(state: &Arc<AppState>, kind: Kind, req: ImportRequest) -> Response {
    let list = match parse_list(&req.text, kind) { Ok(l) => l, Err(e) => return (StatusCode::BAD_REQUEST, e).into_response() };
    if let Ok(mut n) = kind.names(state).write() {
        let mut next = if req.replace { BTreeMap::new() } else { n.clone() };
        for (issi, name) in list {
            if name.is_empty() { next.remove(&issi); } else { next.insert(issi, name); }
        }
        if next.len() > MAX_ENTRIES {
            return (StatusCode::BAD_REQUEST, format!("at most {MAX_ENTRIES} names")).into_response();
        }
        *n = next;
    }
    tracing::warn!(replace = req.replace, list = kind.label(), "names imported from the dashboard");
    match save(state).await {
        Ok(()) => Json(snapshot(state, kind)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_list_is_read_line_by_line() {
        let l = parse_list("ISSI,Name\n2320972, OE4SXM Sebastian\n2321100;\"Relais Nord\"\n# note\n\n2324001\tMTP 2\n9 Alle", Kind::Issi).unwrap();
        assert_eq!(l, vec![
            ("2320972".into(), "OE4SXM Sebastian".into()),
            ("2321100".into(), "Relais Nord".into()),
            ("2324001".into(), "MTP 2".into()),
            ("9".into(), "Alle".into()),
        ]);
        assert!(parse_list("2320972 ok\nabc nope", Kind::Issi).unwrap_err().starts_with("line 2"));
        assert!(parse_list("16777216 too big", Kind::Gssi).is_err());
    }

    #[tokio::test]
    async fn names_change_live_and_are_saved() {
        let path = std::env::temp_dir().join(format!("brew-names-{}.toml", uuid::Uuid::new_v4().simple()));
        let mut c = crate::config::Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        let state = Arc::new(AppState::new(c, path.clone()).0);
        assert_eq!(set(State(state.clone()), Json(NameRequest { issi: " 02320972".into(), name: " OE4SXM ".into() })).await.status(), StatusCode::OK);
        assert_eq!(snapshot(&state, Kind::Issi).get("2320972").map(String::as_str), Some("OE4SXM"));
        let r = import(State(state.clone()), Json(ImportRequest { text: "2321100 Nord\nx bad".into(), replace: true })).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        assert_eq!(snapshot(&state, Kind::Issi).len(), 1, "a wrong line changes nothing");
        import(State(state.clone()), Json(ImportRequest { text: "2321100 Nord".into(), replace: false })).await;
        assert_eq!(snapshot(&state, Kind::Issi).len(), 2);
        set(State(state.clone()), Json(NameRequest { issi: "2320972".into(), name: String::new() })).await;
        let saved = crate::config::Config::load(path.to_str().unwrap()).unwrap();
        assert_eq!(saved.issi_names, BTreeMap::from([("2321100".to_string(), "Nord".to_string())]));
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn group_names_are_a_list_of_their_own() {
        let path = std::env::temp_dir().join(format!("brew-gnames-{}.toml", uuid::Uuid::new_v4().simple()));
        let mut c = crate::config::Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        c.issi_names.insert("2320972".into(), "OE4SXM".into());
        let state = Arc::new(AppState::new(c, path.clone()).0);
        assert_eq!(set_gssi(State(state.clone()), Json(NameRequest { issi: "2321".into(), name: "Bezirk Nord".into() })).await.status(), StatusCode::OK);
        let r = import_gssi(State(state.clone()), Json(ImportRequest { text: "GSSI,Name\n2322;Bezirk Sued\n99999999 bad".into(), replace: false })).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let r = import_gssi(State(state.clone()), Json(ImportRequest { text: "GSSI,Name\n2322;Bezirk Sued".into(), replace: false })).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(snapshot(&state, Kind::Gssi).len(), 2);
        assert_eq!(snapshot(&state, Kind::Issi).len(), 1, "group names leave the ISSI names alone");
        let saved = crate::config::Config::load(path.to_str().unwrap()).unwrap();
        assert_eq!(saved.gssi_names, BTreeMap::from([("2321".to_string(), "Bezirk Nord".to_string()), ("2322".to_string(), "Bezirk Sued".to_string())]));
        assert_eq!(saved.issi_names.len(), 1);
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_file(&path);
    }
}
