//! Fleet Control phase 6: templates, bulk config changes and neighbour
//! suggestions. Protocol and behaviour: `docs/Fleet-Control.md` section 11.
//!
//! - A **template** holds settings several stations share (Brew server,
//!   reselection, access level, migration, presence check, Multi Network,
//!   Home Mode Display, ISSI whitelist, telemetry server, update source), never
//!   what belongs to one station (carrier, colour code, LA, cells, neighbours,
//!   logins, passwords). It is applied once: a station gets the values, nothing
//!   stays bound to the template.
//! - A **bulk change** applies a template (or settings given only for this
//!   run, or the missing reciprocal neighbours) to many stations, parallel or
//!   rolling like an update run. Each station gets its own `SetConfig` with
//!   the confirmation window (rollback when it is not back in time). A change
//!   that rolls back is an ALARM, the run goes on without it. An offline
//!   station gets the change as pending (one per station); one with a pending
//!   change already, a running change or a config that differs from the
//!   server copy is skipped.
//! - **Neighbour suggestions** come from the server's cell directory: cells
//!   that list this cell but are not listed back (clause 18.3.4.1: both should
//!   list each other), cells radios moved to or from (counted for 30 days),
//!   and cells within a radius of the station's position. A listed neighbour
//!   that is not in the directory any more, or whose LA differs, is marked.
//!   Nothing is added by itself.
//!
//! The planning and the run are pure state; `run` drives them.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::fleet::now_ms;
use crate::fleet_update::Mode;
use crate::state::AppState;

/// Sections a template may hold, in the order of the page.
pub const SECTIONS: &[&str] =
    &["brew", "multi_network", "reselect", "rxlev_access_min_dbm", "migration", "presence_check_secs", "home_mode_display", "access", "telemetry", "ota"];
/// Most templates kept.
pub const MAX_TEMPLATES: usize = 50;
/// Cell changes counted for neighbour suggestions are kept this long.
pub const MOVES_KEEP_MS: u64 = 30 * 24 * 3600 * 1000;
/// At most this many neighbours per cell, siblings included (D-NWRK-BROADCAST).
pub const MAX_NEIGHBOURS: usize = 7;
/// Default radius for "close by" suggestions (km).
pub const DEFAULT_RADIUS_KM: f64 = 30.0;
/// A started change without a result this long after its window: failed.
const RESULT_MARGIN_MS: u64 = 60_000;
/// How often a change is tried again while a call is in progress.
const CALL_RETRY_MS: u64 = 10_000;

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub id: u64,
    pub name: String,
    /// Section key -> value (see `SECTIONS`).
    pub sections: Map<String, Value>,
    #[serde(default)]
    pub note: String,
    pub by: Option<String>,
    pub updated_ms: u64,
}

/// Keep only known sections, checked for their shape. Err: what is wrong.
pub fn clean_sections(sections: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (k, v) in sections {
        if !SECTIONS.contains(&k.as_str()) {
            return Err(format!("{k} cannot be in a template (only settings several stations share)"));
        }
        let ok = match k.as_str() {
            "brew" => v.get("host").and_then(Value::as_str).is_some_and(|h| !h.trim().is_empty() && !h.contains(char::is_whitespace)),
            "reselect" | "access" | "telemetry" | "ota" | "multi_network" => v.is_object(),
            "home_mode_display" => v.is_null() || v.is_object(),
            "rxlev_access_min_dbm" => v.is_null() || v.as_i64().is_some_and(|d| (-125..=-50).contains(&d) && d % 5 == 0),
            "migration" => v.is_boolean(),
            "presence_check_secs" => v.as_u64().is_some_and(|s| s <= 86_400),
            _ => false,
        };
        if !ok {
            return Err(format!("{k}: value not valid"));
        }
        if k == "ota" {
            let ch = v.get("channel").and_then(Value::as_str).unwrap_or_default();
            if !["installed", "stable", "beta", "custom"].contains(&ch) {
                return Err("ota.channel: installed, stable, beta or custom".into());
            }
        }
        if k == "telemetry" && v.get("enabled").and_then(Value::as_bool) != Some(false) && v.get("host").and_then(Value::as_str).is_none_or(|h| h.trim().is_empty()) {
            return Err("telemetry: a host is needed".into());
        }
        out.insert(k.clone(), strip_secrets(v));
    }
    Ok(out)
}

/// A template never carries a login or a password.
fn strip_secrets(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().filter(|(k, _)| !matches!(k.as_str(), "password" | "password_set" | "username")).map(|(k, v)| (k.clone(), strip_secrets(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(strip_secrets).collect()),
        x => x.clone(),
    }
}

/// The template sections of a station's config (`ConfigInfo.config`): what "Save as template"
/// starts from.
pub fn sections_of(cfg: &Value) -> Map<String, Value> {
    let mut s = Map::new();
    if cfg.get("networks_explicit").and_then(Value::as_bool) != Some(true) {
        if let Some(l) = cfg.pointer("/brew/link").filter(|l| l.is_object()) {
            s.insert("brew".into(), json!({"host": l.get("host"), "port": l.get("port"), "tls": l.get("tls").and_then(Value::as_bool).unwrap_or(true)}));
        }
    }
    let nets: Vec<Value> = cfg
        .get("networks")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|n| n.get("mode").and_then(Value::as_str) != Some("core") || cfg.get("networks_explicit").and_then(Value::as_bool) == Some(true))
                .map(|n| {
                    let mut e = json!({"name": n.get("name"), "mcc": n.get("mcc"), "mnc": n.get("mnc"), "mode": n.get("mode"), "via": n.get("via")});
                    if let Some(b) = n.get("brew").filter(|b| b.is_object()) {
                        e["host"] = b.get("host").cloned().unwrap_or(Value::Null);
                        e["port"] = b.get("port").cloned().unwrap_or(Value::Null);
                        e["tls"] = b.get("tls").cloned().unwrap_or(json!(true));
                    }
                    e
                })
                .collect()
        })
        .unwrap_or_default();
    s.insert("multi_network".into(), json!({"enabled": cfg.get("multi_network_enabled").and_then(Value::as_bool).unwrap_or(false), "networks": if cfg.get("multi_network_enabled").and_then(Value::as_bool) == Some(true) { nets } else { vec![] }}));
    for k in ["reselect", "rxlev_access_min_dbm", "migration", "presence_check_secs", "home_mode_display", "access"] {
        if let Some(v) = cfg.get(k) {
            s.insert(k.into(), v.clone());
        }
    }
    if let Some(t) = cfg.get("telemetry").filter(|t| t.is_object()) {
        s.insert("telemetry".into(), json!({"enabled": t.get("enabled").and_then(Value::as_bool).unwrap_or(true), "host": t.get("host"), "port": t.get("port"), "use_tls": t.get("use_tls").and_then(Value::as_bool).unwrap_or(false)}));
    }
    if let Some(o) = cfg.get("ota").filter(|o| o.is_object()) {
        s.insert("ota".into(), json!({"channel": o.get("channel"), "repo": o.get("repo"), "branch": o.get("branch")}));
    }
    s
}

/// What applying `sections` to a station with config `cfg` sends.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// `SetConfig.changes` (empty object: nothing to change).
    pub changes: Value,
    /// One line per setting, old → new.
    pub summary: Vec<String>,
    /// Parts left out for this station and why.
    pub notes: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.changes.as_object().is_none_or(|o| o.is_empty())
    }
}

fn shown(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "—".into(),
        Some(Value::Bool(b)) => (if *b { "on" } else { "off" }).into(),
        Some(Value::String(s)) if s.is_empty() => "—".into(),
        Some(Value::String(s)) => s.clone(),
        Some(x) => x.to_string(),
    }
}

fn ln(summary: &mut Vec<String>, what: &str, a: Option<&Value>, b: Option<&Value>) {
    summary.push(format!("{what}: {} → {}", shown(a), shown(b)));
}

fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    a.unwrap_or(&Value::Null) == b.unwrap_or(&Value::Null)
}

/// Plan `sections` (a template, or settings for one run) against a station's config.
pub fn plan(sections: &Map<String, Value>, cfg: &Value) -> Plan {
    let mut changes = Map::new();
    let mut summary = Vec::new();
    let mut notes = Vec::new();
    let mut raw: Vec<Value> = Vec::new();
    let explicit = cfg.get("networks_explicit").and_then(Value::as_bool) == Some(true);

    if let Some(t) = sections.get("brew") {
        match cfg.pointer("/brew/link").filter(|l| l.is_object()) {
            _ if explicit => notes.push("Brew server: this station uses networks, its Brew servers are set per network".into()),
            None => notes.push("Brew server: no Brew link on this station".into()),
            Some(l) if l.get("username").is_none_or(Value::is_null) => notes.push("Brew server: no Brew login on this station".into()),
            Some(l) => {
                let port = t.get("port").cloned().filter(|p| !p.is_null()).unwrap_or(json!(443));
                let tls = t.get("tls").cloned().unwrap_or(json!(true));
                let host = t.get("host").cloned().unwrap_or(Value::Null);
                let mut any = false;
                for (k, v) in [("host", &host), ("port", &port), ("tls", &tls)] {
                    if !same(l.get(k), Some(v)) {
                        ln(&mut summary, &format!("Brew {k}"), l.get(k), Some(v));
                        any = true;
                    }
                }
                if any {
                    changes.insert("brew".into(), json!({"host": host, "port": port, "tls": tls, "username": l.get("username"), "password": ""}));
                }
            }
        }
    }

    if let Some(t) = sections.get("multi_network") {
        let on = t.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        let was = cfg.get("multi_network_enabled").and_then(Value::as_bool).unwrap_or(false);
        if on != was {
            changes.insert("multi_network_enabled".into(), json!(on));
            ln(&mut summary, "Multi Network", Some(&json!(was)), Some(&json!(on)));
        }
        if on {
            let have = cfg.get("networks").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut nets = Vec::new();
            for n in t.get("networks").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
                let Some(name) = n.get("name").and_then(Value::as_str) else { continue };
                let mode = n.get("mode").and_then(Value::as_str).unwrap_or("local");
                let old = have.iter().find(|h| h.get("name").and_then(Value::as_str) == Some(name));
                let via = if mode == "via" { n.get("via").cloned().unwrap_or(json!("")) } else { json!("") };
                let mut e = json!({"name": name, "mcc": n.get("mcc"), "mnc": n.get("mnc"), "via": via});
                let mut diffs = Vec::new();
                if mode == "core" {
                    let login = old.and_then(|o| o.pointer("/brew/username")).filter(|u| !u.is_null());
                    let Some(login) = login else {
                        notes.push(format!("Network {name}: its own Brew server needs a login, set it on the station's page"));
                        continue;
                    };
                    let b = json!({"host": n.get("host"), "port": n.get("port").cloned().filter(|p| !p.is_null()).unwrap_or(json!(443)), "tls": n.get("tls").cloned().unwrap_or(json!(true)), "username": login, "password": ""});
                    for k in ["host", "port", "tls"] {
                        if !same(old.and_then(|o| o.pointer(&format!("/brew/{k}"))), b.get(k)) {
                            diffs.push(format!("Network {name} Brew {k}: {} → {}", shown(old.and_then(|o| o.pointer(&format!("/brew/{k}")))), shown(b.get(k))));
                        }
                    }
                    e["brew"] = b;
                    e["via"] = json!("");
                }
                match old {
                    None => {
                        nets.push(e);
                        summary.push(format!("Network {name}: added ({}/{})", shown(n.get("mcc")), shown(n.get("mnc"))));
                    }
                    Some(o) => {
                        for k in ["mcc", "mnc"] {
                            if !same(o.get(k), n.get(k)) {
                                diffs.push(format!("Network {name} {k}: {} → {}", shown(o.get(k)), shown(n.get(k))));
                            }
                        }
                        let ovia = if o.get("mode").and_then(Value::as_str) == Some("via") { o.get("via").cloned().unwrap_or(Value::Null) } else { json!("") };
                        let omode = o.get("mode").and_then(Value::as_str).unwrap_or("local");
                        if omode != mode || (mode == "via" && !same(Some(&ovia), Some(&via))) {
                            diffs.push(format!("Network {name}: {omode}{} → {mode}{}", if omode == "via" { format!(" {}", shown(Some(&ovia))) } else { String::new() }, if mode == "via" { format!(" {}", shown(Some(&via))) } else { String::new() }));
                        }
                        if !diffs.is_empty() {
                            nets.push(e);
                            summary.append(&mut diffs);
                        }
                    }
                }
            }
            if !nets.is_empty() {
                changes.insert("networks".into(), Value::Array(nets));
            }
        }
    }

    if let Some(t) = sections.get("reselect").and_then(Value::as_object) {
        let mut merged = cfg.get("reselect").and_then(Value::as_object).cloned().unwrap_or_default();
        let mut any = false;
        for (k, v) in t {
            if !same(merged.get(k), Some(v)) {
                let label = match k.as_str() {
                    "fast_threshold_db" => "Fast threshold (dB)",
                    "slow_threshold_db" => "Slow threshold above fast (dB)",
                    "fast_hysteresis_db" => "Fast hysteresis (dB)",
                    "slow_hysteresis_db" => "Slow hysteresis (dB)",
                    other => other,
                };
                ln(&mut summary, label, merged.get(k), Some(v));
                merged.insert(k.clone(), v.clone());
                any = true;
            }
        }
        if any {
            changes.insert("reselect".into(), Value::Object(merged));
        }
    }
    for (k, label) in [("rxlev_access_min_dbm", "Min. access level (dBm)"), ("migration", "Migration"), ("presence_check_secs", "Presence check (s)")] {
        if let Some(v) = sections.get(k) {
            if !same(cfg.get(k), Some(v)) {
                ln(&mut summary, label, cfg.get(k), Some(v));
                changes.insert(k.into(), v.clone());
            }
        }
    }

    if let Some(h) = sections.get("home_mode_display") {
        let old = cfg.get("home_mode_display");
        let clean = |h: &Value| Value::Object(h.as_object().map(|o| o.iter().filter(|(_, x)| !x.is_null() && x.as_str() != Some("")).map(|(k, x)| (k.clone(), x.clone())).collect()).unwrap_or_default());
        let want = if h.is_null() { Value::Null } else { clean(h) };
        let have = old.filter(|o| !o.is_null()).map(clean).unwrap_or(Value::Null);
        if want != have {
            if want.is_null() {
                raw.push(json!({"path": ["cell_info", "home_mode_display"], "remove": true}));
                summary.push("Home mode display: off".into());
            } else {
                raw.push(json!({"path": ["cell_info", "home_mode_display"], "value": want}));
                summary.push(if have.is_null() { format!("Home mode display: on (\"{}\")", want.get("text").and_then(Value::as_str).unwrap_or_default()) } else { format!("Home mode display: {} → {}", have, want) });
            }
        }
    }
    if let Some(a) = sections.get("access") {
        if let Some(w) = a.get("issi_whitelist") {
            let old = cfg.pointer("/access/issi_whitelist");
            if !same(old, Some(w)) {
                raw.push(json!({"path": ["security", "issi_whitelist"], "value": w}));
                let list = |v: Option<&Value>| v.and_then(Value::as_array).map(|a| if a.is_empty() { "empty".to_string() } else { a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ") }).unwrap_or_else(|| "empty".into());
                summary.push(format!("ISSI whitelist: {} → {}", list(old), list(Some(w))));
            }
        }
        if let Some(m) = a.get("whitelist_mode") {
            let old = cfg.pointer("/access/whitelist_mode");
            if !same(old, Some(m)) {
                raw.push(json!({"path": ["security", "whitelist_mode"], "value": m}));
                ln(&mut summary, "Whitelist mode", old, Some(m));
            }
        }
    }

    if let Some(t) = sections.get("telemetry") {
        match cfg.get("telemetry").filter(|x| x.is_object()) {
            None => notes.push("Telemetry: not set up on this station (its login is set on the station's page)".into()),
            Some(old) => {
                let mut body = json!({
                    "enabled": old.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                    "host": old.get("host"), "port": old.get("port"), "use_tls": old.get("use_tls").and_then(Value::as_bool).unwrap_or(false),
                    "username": old.get("username").cloned().filter(|u| !u.is_null()).unwrap_or(json!("")), "password": "",
                    "site_name": old.get("site_name").cloned().filter(|u| !u.is_null()).unwrap_or(json!("")),
                    "latitude": old.get("latitude"), "longitude": old.get("longitude"),
                });
                let mut any = false;
                for k in ["enabled", "host", "port", "use_tls"] {
                    if let Some(v) = t.get(k) {
                        if !same(old.get(k), Some(v)) {
                            ln(&mut summary, &format!("Telemetry {}", k.replace('_', " ")), old.get(k), Some(v));
                            body[k] = v.clone();
                            any = true;
                        }
                    }
                }
                if any {
                    changes.insert("telemetry".into(), body);
                }
            }
        }
    }

    if let Some(t) = sections.get("ota") {
        let old = cfg.get("ota");
        let ch = t.get("channel").and_then(Value::as_str).unwrap_or("installed");
        let och = old.and_then(|o| o.get("channel")).and_then(Value::as_str).unwrap_or_default();
        let custom_diff = ch == "custom" && (!same(old.and_then(|o| o.get("repo")), t.get("repo")) || !same(old.and_then(|o| o.get("branch")), t.get("branch")));
        if ch != och || custom_diff {
            let desc = |c: &str, o: Option<&Value>| if c == "custom" { format!("{} ({})", shown(o.and_then(|o| o.get("repo"))), shown(o.and_then(|o| o.get("branch")))) } else { c.to_string() };
            summary.push(format!("Update source: {} → {}", desc(och, old), desc(ch, Some(t))));
            changes.insert("ota".into(), json!({"channel": ch, "repo": t.get("repo").cloned().filter(|r| r.as_str().is_some_and(|s| !s.is_empty())), "branch": t.get("branch").cloned().filter(|r| r.as_str().is_some_and(|s| !s.is_empty()))}));
        }
    }

    if !raw.is_empty() {
        changes.insert("raw".into(), Value::Array(raw));
    }
    Plan { changes: Value::Object(changes), summary, notes }
}

// ---------------------------------------------------------------------------
// Neighbour suggestions
// ---------------------------------------------------------------------------

/// A cell of the server's cell directory, as the suggestions need it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirCell {
    /// `<station>/<cell>`.
    pub name: String,
    pub station: String,
    pub cell: u8,
    pub online: bool,
    pub mcc: u16,
    pub mnc: u16,
    pub carrier: u16,
    pub carrier_ext: u16,
    pub location_area: u16,
    #[serde(skip)]
    pub rxlev_access_min_dbm: Option<i16>,
    #[serde(skip)]
    pub ms_txpwr_max_dbm: Option<u8>,
    #[serde(skip)]
    pub service_details: Option<std::collections::BTreeMap<String, bool>>,
    /// What it broadcasts as neighbours: (mcc, mnc, carrier, carrier_ext).
    #[serde(skip)]
    pub neighbours: Option<Vec<(u16, u16, u16, u16)>>,
}

/// The directory from the carrier registry.
pub fn directory(inner: &crate::state::Inner) -> Vec<DirCell> {
    crate::carriers::directory_entries(inner)
        .into_iter()
        .map(|e| DirCell {
            name: crate::carriers::cell_name(&e.station, e.cell.cell),
            station: e.station.clone(),
            cell: e.cell.cell,
            online: e.client.is_some(),
            mcc: e.cell.key.mcc,
            mnc: e.cell.key.mnc,
            carrier: e.cell.key.carrier,
            carrier_ext: e.cell.key.carrier_ext,
            location_area: e.cell.location_area,
            rxlev_access_min_dbm: e.cell.extra.rxlev_access_min_dbm,
            ms_txpwr_max_dbm: e.cell.extra.ms_txpwr_max_dbm,
            service_details: e.cell.extra.service_details.clone(),
            neighbours: e.cell.extra.neighbours.as_ref().map(|l| l.iter().map(|n| (n.mcc, n.mnc, n.carrier, n.carrier_ext)).collect()),
        })
        .collect()
}

/// Great-circle distance in km.
pub fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

/// One own cell as the suggestions see it.
struct OwnCell<'a> {
    id: u8,
    cfg: &'a Value,
    mcc: u16,
    mnc: u16,
    carrier: u16,
    carrier_ext: Option<u16>,
}

fn own_cells<'a>(cfg: &'a Value, login: Option<&str>, dir: &[DirCell]) -> Vec<OwnCell<'a>> {
    let mcc = cfg.get("mcc").and_then(Value::as_u64).unwrap_or(0) as u16;
    let mnc = cfg.get("mnc").and_then(Value::as_u64).unwrap_or(0) as u16;
    cfg.get("cells")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| {
                    let id = c.get("id").and_then(Value::as_u64).unwrap_or(0) as u8;
                    let carrier = c.get("main_carrier").and_then(Value::as_u64).unwrap_or(0) as u16;
                    // The station's own announcement tells its network and band plan exactly.
                    let d = login.and_then(|l| dir.iter().find(|d| d.station == l && d.cell == id));
                    OwnCell {
                        id,
                        cfg: c,
                        mcc: d.map(|d| d.mcc).unwrap_or(mcc),
                        mnc: d.map(|d| d.mnc).unwrap_or(mnc),
                        carrier: d.map(|d| d.carrier).unwrap_or(carrier),
                        carrier_ext: d.map(|d| d.carrier_ext).or_else(|| c.get("carrier_ext").and_then(Value::as_u64).map(|x| x as u16)),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Does neighbour entry `nb` (of a station in `mcc`/`mnc`) name directory cell `d`?
fn entry_is(nb: &Value, mcc: u16, mnc: u16, d: &DirCell) -> bool {
    let n = |k: &str| nb.get(k).and_then(Value::as_u64);
    n("main_carrier_number") == Some(d.carrier as u64)
        && n("main_carrier_number_extension").is_none_or(|x| x == d.carrier_ext as u64)
        && n("mcc").unwrap_or(mcc as u64) == d.mcc as u64
        && n("mnc").unwrap_or(mnc as u64) == d.mnc as u64
}

/// Does directory cell `d` broadcast `c` as a neighbour?
fn lists(d: &DirCell, c: &OwnCell) -> bool {
    d.neighbours.as_ref().is_some_and(|l| l.iter().any(|&(mcc, mnc, car, ext)| mcc == c.mcc && mnc == c.mnc && car == c.carrier && c.carrier_ext.is_none_or(|e| e == ext)))
}

fn own_list(c: &Value) -> Vec<Value> {
    let own = c.get("primary").and_then(Value::as_bool) == Some(true) || c.get("own_list").and_then(Value::as_bool) == Some(true);
    if own { c.get("neighbours").and_then(Value::as_array).cloned().unwrap_or_default() } else { Vec::new() }
}

fn siblings(c: &Value) -> Vec<Value> {
    c.get("siblings").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// A neighbour entry for directory cell `d`, as "Add from server list" fills it in: what the
/// cell broadcasts, its own BS service details when it reported them (else this station's with
/// registration, voice and system wide services on), reselection type 2.
pub fn entry_for(d: &DirCell, cell: &Value, cfg: &Value, extra_used: &[u64]) -> Value {
    let used: Vec<u64> = own_list(cell)
        .iter()
        .chain(siblings(cell).iter())
        .filter_map(|n| n.get("cell_identifier_ca").and_then(Value::as_u64))
        .chain(extra_used.iter().copied())
        .collect();
    let id = (0..32u64).find(|i| !used.contains(i)).unwrap_or(31);
    let details: Value = match &d.service_details {
        Some(s) => json!(s),
        None => {
            let mut s = cfg.get("service_details").cloned().unwrap_or_else(|| json!({}));
            for k in ["registration", "voice_service", "system_wide_services"] {
                s[k] = json!(true);
            }
            s
        }
    };
    let mut e = json!({
        "cell_identifier_ca": id,
        "cell_reselection_types_supported": 2,
        "neighbor_cell_synchronized": false,
        "cell_load_ca": 0,
        "main_carrier_number": d.carrier,
        "main_carrier_number_extension": d.carrier_ext,
        "location_area": d.location_area,
        "bs_service_details": details,
    });
    let (mcc, mnc) = (cfg.get("mcc").and_then(Value::as_u64), cfg.get("mnc").and_then(Value::as_u64));
    if mcc != Some(d.mcc as u64) || mnc != Some(d.mnc as u64) {
        e["mcc"] = json!(d.mcc);
        e["mnc"] = json!(d.mnc);
    }
    if let Some(r) = d.rxlev_access_min_dbm {
        e["minimum_rx_access_level"] = json!((r + 125) / 5);
    }
    if let Some(p) = d.ms_txpwr_max_dbm {
        e["maximum_ms_transmit_power"] = json!((p.saturating_sub(10)) / 5);
    }
    e
}

/// Inputs of the suggestions besides the station itself.
pub struct NbContext<'a> {
    pub dir: &'a [DirCell],
    /// Station login -> position.
    pub positions: &'a HashMap<String, (f64, f64)>,
    /// (cell name, cell name) -> changes counted (both directions summed by the caller or not).
    pub moves: &'a HashMap<(String, String), u32>,
    pub radius_km: f64,
}

fn moves_between(ctx: &NbContext, a: &str, b: &str) -> u32 {
    ctx.moves.get(&(a.to_owned(), b.to_owned())).copied().unwrap_or(0) + ctx.moves.get(&(b.to_owned(), a.to_owned())).copied().unwrap_or(0)
}

/// Suggestions and marks per cell of the station (`login` its Brew login, `cfg` its config copy).
pub fn suggestions(login: Option<&str>, cfg: &Value, pos: Option<(f64, f64)>, ctx: &NbContext) -> Value {
    let cells = own_cells(cfg, login, ctx.dir);
    let out: Vec<Value> = cells
        .iter()
        .map(|c| {
            let list = own_list(c.cfg);
            let name = login.map(|l| crate::carriers::cell_name(l, c.id));
            let mut sug = Vec::new();
            for d in ctx.dir.iter().filter(|d| Some(d.station.as_str()) != login) {
                if list.iter().any(|nb| entry_is(nb, c.mcc, c.mnc, d)) {
                    continue;
                }
                let mut why = Vec::new();
                if lists(d, c) {
                    why.push(json!({"kind": "reciprocal"}));
                }
                let mv = name.as_deref().map(|n| moves_between(ctx, n, &d.name)).unwrap_or(0);
                if mv > 0 {
                    why.push(json!({"kind": "moves", "count": mv}));
                }
                let km = pos.zip(ctx.positions.get(&d.station).copied()).map(|(a, b)| distance_km(a, b));
                if let Some(km) = km.filter(|k| *k <= ctx.radius_km) {
                    why.push(json!({"kind": "distance", "km": (km * 10.0).round() / 10.0}));
                }
                if why.is_empty() {
                    continue;
                }
                let rec = lists(d, c);
                sug.push((rec, mv, km.unwrap_or(f64::MAX), json!({"cell": d, "why": why, "km": km.map(|k| (k * 10.0).round() / 10.0), "entry": entry_for(d, c.cfg, cfg, &[])})));
            }
            sug.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)));
            // Marks on listed neighbours: not in the directory, or another LA there.
            let marks: Vec<Value> = list
                .iter()
                .enumerate()
                .filter_map(|(i, nb)| {
                    let own_station = |d: &DirCell| Some(d.station.as_str()) == login;
                    let m = ctx.dir.iter().find(|d| entry_is(nb, c.mcc, c.mnc, d));
                    match m {
                        None => Some(json!({"index": i, "kind": "missing", "carrier": nb.get("main_carrier_number"), "text": "not in the server's cell directory (station gone, or never on this server)"})),
                        Some(d) if own_station(d) => None,
                        Some(d) => nb
                            .get("location_area")
                            .and_then(Value::as_u64)
                            .filter(|la| *la != d.location_area as u64)
                            .map(|la| json!({"index": i, "kind": "la", "carrier": d.carrier, "cell": d.name, "text": format!("LA {la} here, {} broadcasts LA {}", d.name, d.location_area), "location_area": d.location_area})),
                    }
                })
                .collect();
            json!({
                "id": c.id,
                "name": name,
                "count": list.len() + siblings(c.cfg).len(),
                "max": MAX_NEIGHBOURS,
                "suggestions": sug.into_iter().map(|s| s.3).collect::<Vec<_>>(),
                "marks": marks,
            })
        })
        .collect();
    json!({"cells": out, "radius_km": ctx.radius_km, "position": pos.map(|p| json!({"lat": p.0, "lon": p.1}))})
}

/// A station of the fleet as the neighbour check needs it.
pub struct CheckStation<'a> {
    pub id: &'a str,
    pub label: String,
    pub login: Option<&'a str>,
    pub cfg: &'a Value,
}

/// Fleet-wide: per station the cells that list one of its cells without being listed back, and
/// the marked entries; with the neighbour additions that "Add all reciprocal" would send.
pub fn check(stations: &[CheckStation], dir: &[DirCell]) -> Vec<Value> {
    let empty_pos = HashMap::new();
    let empty_moves = HashMap::new();
    let ctx = NbContext { dir, positions: &empty_pos, moves: &empty_moves, radius_km: 0.0 };
    stations
        .iter()
        .filter_map(|s| {
            let sug = suggestions(s.login, s.cfg, None, &ctx);
            let mut missing = Vec::new();
            let mut marks = Vec::new();
            for c in sug.get("cells").and_then(Value::as_array).cloned().unwrap_or_default() {
                for x in c.get("suggestions").and_then(Value::as_array).cloned().unwrap_or_default() {
                    missing.push(json!({"cell": c.get("id"), "from": x.pointer("/cell/name"), "carrier": x.pointer("/cell/carrier"), "online": x.pointer("/cell/online")}));
                }
                for m in c.get("marks").and_then(Value::as_array).cloned().unwrap_or_default() {
                    marks.push(json!({"cell": c.get("id"), "text": m.get("text")}));
                }
            }
            (!missing.is_empty() || !marks.is_empty()).then(|| json!({"id": s.id, "label": s.label, "missing": missing, "marks": marks}))
        })
        .collect()
}

/// What adding the missing reciprocal neighbours sends to one station: `cells[{id, neighbours}]`
/// with the new entries after the listed ones, at most `MAX_NEIGHBOURS` per cell with siblings.
pub fn reciprocal_plan(login: Option<&str>, cfg: &Value, dir: &[DirCell]) -> Plan {
    let mut cells_body = Vec::new();
    let mut summary = Vec::new();
    let mut notes = Vec::new();
    for c in own_cells(cfg, login, dir) {
        let mut list = own_list(c.cfg);
        let room = MAX_NEIGHBOURS.saturating_sub(list.len() + siblings(c.cfg).len());
        let want: Vec<&DirCell> = dir.iter().filter(|d| Some(d.station.as_str()) != login && lists(d, &c) && !list.iter().any(|nb| entry_is(nb, c.mcc, c.mnc, d))).collect();
        if want.is_empty() {
            continue;
        }
        let mut used = Vec::new();
        let mut added = Vec::new();
        for d in want.iter().take(room) {
            let e = entry_for(d, c.cfg, cfg, &used);
            used.push(e.get("cell_identifier_ca").and_then(Value::as_u64).unwrap_or(0));
            added.push(d.name.clone());
            list.push(e);
        }
        if want.len() > room {
            notes.push(format!("Cell {}: no room for {} more ({} neighbours at most, siblings included)", c.id, want.len() - room, MAX_NEIGHBOURS));
        }
        if !added.is_empty() {
            summary.push(format!("Cell {} neighbours: + {}", c.id, added.join(", ")));
            cells_body.push(json!({"id": c.id, "neighbours": list}));
        }
    }
    let changes = if cells_body.is_empty() { json!({}) } else { json!({"cells": cells_body}) };
    Plan { changes, summary, notes }
}

// ---------------------------------------------------------------------------
// Counted cell changes (for the suggestions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveCount {
    pub from: String,
    pub to: String,
    pub count: u32,
    pub last_ms: u64,
}

/// Count one change from cell `from` to cell `to` (cell names); drops counts older than 30 days.
pub fn count_move(list: &mut Vec<MoveCount>, from: &str, to: &str, now_ms: u64) {
    list.retain(|m| now_ms.saturating_sub(m.last_ms) <= MOVES_KEEP_MS);
    if from == to {
        return;
    }
    match list.iter_mut().find(|m| m.from == from && m.to == to) {
        Some(m) => {
            m.count = m.count.saturating_add(1);
            m.last_ms = now_ms;
        }
        None => list.push(MoveCount { from: from.to_owned(), to: to.to_owned(), count: 1, last_ms: now_ms }),
    }
}

// ---------------------------------------------------------------------------
// Bulk change run
// ---------------------------------------------------------------------------

/// What a run applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum What {
    /// A template's (or the dialog's) sections; `name`: the template, empty for a one-off.
    Sections { name: String, sections: Map<String, Value> },
    /// The missing reciprocal neighbours (from the cell directory at each station's turn).
    Reciprocal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ItemState {
    Pending,
    /// `SetConfig` sent, answer outstanding.
    Sending { at_ms: u64 },
    WaitingCall { since_ms: u64, last_try_ms: u64 },
    /// Written; waiting for the station to come back and the confirmation.
    Applying { sent_ms: u64, confirm_secs: u32 },
    Done,
    /// The station was offline: kept as its pending change.
    Saved,
    Failed { reason: String },
    Skipped { reason: String },
}

impl ItemState {
    fn active(&self) -> bool {
        matches!(self, Self::Sending { .. } | Self::WaitingCall { .. } | Self::Applying { .. })
    }
    fn open(&self) -> bool {
        matches!(self, Self::Pending) || self.active()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub state: ItemState,
    /// What was sent (or kept as pending), one line per setting.
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: u64,
    pub started_ms: u64,
    pub user: Option<String>,
    pub what: What,
    pub mode: Mode,
    pub during_call: bool,
    pub interrupt_min: u32,
    pub items: Vec<Item>,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub finished_ms: Option<u64>,
}

/// A change log line from a run (`station` empty: the run itself).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub station: String,
    pub label: String,
    pub kind: &'static str,
    pub text: String,
    pub user: Option<String>,
}

/// What the driver must do for one station at its turn.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// Send `SetConfig` (online station).
    Send { id: String, base_hash: String, changes: Value, summary: Vec<String>, during_call: bool },
    /// Keep the change as the station's pending change (offline).
    Save { id: String, base_hash: String, changes: Value, summary: Vec<String> },
}

/// A station's state as the run needs it at its turn.
pub struct StationNow<'a> {
    pub online: bool,
    pub capable: bool,
    pub cfg: &'a crate::fleet_config::StationConfig,
    pub login: Option<&'a str>,
}

impl Run {
    pub fn what_text(&self) -> String {
        match &self.what {
            What::Sections { name, .. } if !name.is_empty() => format!("template \"{name}\""),
            What::Sections { .. } => "settings".into(),
            What::Reciprocal => "reciprocal neighbours".into(),
        }
    }

    pub fn new(id: u64, now_ms: u64, user: Option<String>, what: What, mode: Mode, during_call: bool, interrupt_min: u32, stations: Vec<(String, String)>) -> (Self, Vec<Event>) {
        let items = stations.into_iter().map(|(id, label)| Item { id, label, state: ItemState::Pending, summary: Vec::new(), notes: Vec::new() }).collect();
        let run = Run { id, started_ms: now_ms, user: user.clone(), what, mode, during_call, interrupt_min, items, cancelled: false, finished_ms: None };
        let n = run.items.len();
        let call = if during_call { "change during call".to_string() } else { format!("calls interrupted after {interrupt_min} min") };
        let ev = Event { station: String::new(), label: String::new(), kind: "config", text: format!("Bulk config change started ({}, {}, {call}, {n} station{})", run.what_text(), mode.as_str(), if n == 1 { "" } else { "s" }), user };
        (run, vec![ev])
    }

    pub fn is_finished(&self) -> bool {
        self.finished_ms.is_some()
    }

    pub fn after_restart(&mut self) {
        for it in &mut self.items {
            if matches!(it.state, ItemState::Sending { .. }) {
                it.state = ItemState::Pending;
            }
        }
    }

    fn ev(it: &Item, kind: &'static str, text: String) -> Event {
        Event { station: it.id.clone(), label: it.label.clone(), kind, text, user: None }
    }

    /// Plan one station: Err = skip reason.
    fn plan_for(&self, st: &StationNow, dir: &[DirCell]) -> Result<(String, Plan), String> {
        if !st.capable {
            return Err("cannot change its config from here (needs config-v1)".into());
        }
        if st.cfg.drift.is_some() {
            return Err("its config differs from the server copy (resolve it first)".into());
        }
        if st.cfg.apply.is_some() {
            return Err("a config change is still running there".into());
        }
        let copy = st.cfg.copy.as_ref().ok_or("no server copy of its config yet")?;
        let p = match &self.what {
            What::Sections { sections, .. } => plan(sections, &copy.config),
            What::Reciprocal => reciprocal_plan(st.login, &copy.config, dir),
        };
        if p.is_empty() {
            return Err(if p.notes.is_empty() { "nothing to change".into() } else { format!("nothing to change ({})", p.notes.join("; ")) });
        }
        if !st.online && st.cfg.pending.is_some() {
            return Err("offline with a pending change already".into());
        }
        Ok((copy.hash.clone(), p))
    }

    /// Advance: results of running changes, call retries, next turns. `station`: the station's
    /// state now (`None`: not listed any more).
    pub fn tick(&mut self, now_ms: u64, station: &dyn Fn(&str) -> Option<(bool, bool, crate::fleet_config::StationConfig, Option<String>)>, dir: &[DirCell]) -> (Vec<Turn>, Vec<Event>) {
        let mut turns = Vec::new();
        let mut events = Vec::new();
        if self.is_finished() {
            return (turns, events);
        }
        let interrupt_ms = self.interrupt_min as u64 * 60_000;
        let during_call = self.during_call;
        // Results.
        for i in 0..self.items.len() {
            let it = &self.items[i];
            match it.state.clone() {
                ItemState::Applying { sent_ms, confirm_secs } => {
                    let Some((_, _, cfg, _)) = station(&it.id) else {
                        self.items[i].state = ItemState::Failed { reason: "removed from the Fleet Manager".into() };
                        continue;
                    };
                    if cfg.apply.is_some() {
                        continue;
                    }
                    match cfg.last_outcome.as_ref().filter(|o| o.at_ms >= sent_ms) {
                        Some(o) if o.ok => {
                            self.items[i].state = ItemState::Done;
                        }
                        Some(o) => {
                            let it = &mut self.items[i];
                            it.state = ItemState::Failed { reason: o.text.clone() };
                            events.push(Self::ev(it, "config_alarm", format!("ALARM Basestation {}: bulk config change not kept ({}), run goes on", it.label, o.text)));
                        }
                        None if now_ms.saturating_sub(sent_ms) > (10 + confirm_secs as u64) * 1000 + RESULT_MARGIN_MS => {
                            let it = &mut self.items[i];
                            it.state = ItemState::Failed { reason: "no result".into() };
                            events.push(Self::ev(it, "config_alarm", format!("ALARM Basestation {}: no result of the bulk config change, run goes on", it.label)));
                        }
                        None => {}
                    }
                }
                ItemState::WaitingCall { since_ms, last_try_ms } if !self.cancelled && now_ms.saturating_sub(last_try_ms) >= CALL_RETRY_MS => {
                    let force = during_call || now_ms.saturating_sub(since_ms) >= interrupt_ms;
                    let id = it.id.clone();
                    match station(&id).filter(|s| s.0) {
                        None => {
                            // Went offline while waiting: plan again as offline.
                            self.items[i].state = ItemState::Pending;
                        }
                        Some((online, capable, cfg, login)) => {
                            let now = StationNow { online, capable, cfg: &cfg, login: login.as_deref() };
                            match self.plan_for(&now, dir) {
                                Err(reason) => {
                                    let it = &mut self.items[i];
                                    events.push(Self::ev(it, "config", format!("Basestation {}: bulk config change skipped ({reason})", it.label)));
                                    it.state = ItemState::Skipped { reason };
                                }
                                Ok((base_hash, p)) => {
                                    if force && !during_call {
                                        events.push(Self::ev(&self.items[i], "config", format!("Basestation {}: call still in progress after {} min, changing anyway", self.items[i].label, self.interrupt_min)));
                                    }
                                    let it = &mut self.items[i];
                                    it.state = ItemState::WaitingCall { since_ms, last_try_ms: now_ms };
                                    it.summary = p.summary.clone();
                                    it.notes = p.notes.clone();
                                    turns.push(Turn::Send { id, base_hash, changes: p.changes, summary: p.summary, during_call: force });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // Next turns.
        if !self.cancelled {
            loop {
                let active = self.items.iter().filter(|i| i.state.active()).count();
                if self.mode == Mode::Rolling && active > 0 {
                    break;
                }
                let Some(idx) = self.items.iter().position(|i| i.state == ItemState::Pending) else { break };
                let id = self.items[idx].id.clone();
                let Some((online, capable, cfg, login)) = station(&id) else {
                    let it = &mut self.items[idx];
                    it.state = ItemState::Skipped { reason: "not in the Fleet Manager any more".into() };
                    continue;
                };
                let now = StationNow { online, capable, cfg: &cfg, login: login.as_deref() };
                match self.plan_for(&now, dir) {
                    Err(reason) => {
                        let it = &mut self.items[idx];
                        events.push(Self::ev(it, "config", format!("Basestation {}: bulk config change skipped ({reason})", it.label)));
                        it.state = ItemState::Skipped { reason };
                    }
                    Ok((base_hash, p)) => {
                        let it = &mut self.items[idx];
                        it.summary = p.summary.clone();
                        it.notes = p.notes.clone();
                        if online {
                            it.state = ItemState::Sending { at_ms: now_ms };
                            turns.push(Turn::Send { id, base_hash, changes: p.changes, summary: p.summary, during_call });
                            if self.mode == Mode::Rolling {
                                break;
                            }
                        } else {
                            it.state = ItemState::Saved;
                            turns.push(Turn::Save { id, base_hash, changes: p.changes, summary: p.summary });
                        }
                    }
                }
            }
        }
        self.check_finished(now_ms, &mut events);
        (turns, events)
    }

    /// The answer to a `SetConfig` of this run: `Ok(confirm_secs)` written, `Err((code, message))`.
    pub fn send_result(&mut self, id: &str, result: Result<u32, (String, String)>, now_ms: u64) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(it) = self.items.iter_mut().find(|i| i.id == id) else { return events };
        if !matches!(it.state, ItemState::Sending { .. } | ItemState::WaitingCall { .. }) {
            return events;
        }
        let call_since = match it.state {
            ItemState::WaitingCall { since_ms, .. } => Some(since_ms),
            _ => None,
        };
        match result {
            Ok(confirm_secs) => it.state = ItemState::Applying { sent_ms: now_ms, confirm_secs },
            Err((code, _)) if code == "call_active" => {
                if call_since.is_none() {
                    events.push(Self::ev(it, "config", format!("Basestation {}: call in progress, waiting for it to end", it.label)));
                }
                it.state = ItemState::WaitingCall { since_ms: call_since.unwrap_or(now_ms), last_try_ms: now_ms };
            }
            Err((code, message)) if code == "unreachable" => {
                // Went offline at its turn: planned again (as pending) at the next tick.
                let _ = message;
                it.state = ItemState::Pending;
            }
            Err((code, message)) => {
                events.push(Self::ev(it, "config_alarm", format!("ALARM Basestation {}: bulk config change refused ({code}: {message}), run goes on", it.label)));
                it.state = ItemState::Failed { reason: format!("{code}: {message}") };
            }
        }
        self.check_finished(now_ms, &mut events);
        events
    }

    fn check_finished(&mut self, now_ms: u64, events: &mut Vec<Event>) {
        if self.is_finished() || self.items.iter().any(|i| i.state.open()) {
            return;
        }
        self.finished_ms = Some(now_ms);
        let n = |f: &dyn Fn(&ItemState) -> bool| self.items.iter().filter(|i| f(&i.state)).count();
        let done = n(&|s| *s == ItemState::Done);
        let saved = n(&|s| *s == ItemState::Saved);
        let failed = n(&|s| matches!(s, ItemState::Failed { .. }));
        let skipped = n(&|s| matches!(s, ItemState::Skipped { .. }));
        events.push(Event {
            station: String::new(),
            label: String::new(),
            kind: "config",
            text: format!("Bulk config change {} ({}): {done} changed, {saved} pending (offline), {failed} failed, {skipped} skipped", if self.cancelled { "cancelled" } else { "finished" }, self.what_text()),
            user: None,
        });
    }

    pub fn cancel_all(&mut self, user: Option<&str>, now_ms: u64) -> Vec<Event> {
        let mut events = Vec::new();
        if self.is_finished() {
            return events;
        }
        self.cancelled = true;
        events.push(Event { station: String::new(), label: String::new(), kind: "config", text: "Bulk config change cancelled: no further stations".into(), user: user.map(str::to_owned) });
        for it in &mut self.items {
            if matches!(it.state, ItemState::Pending | ItemState::WaitingCall { .. }) {
                it.state = ItemState::Skipped { reason: "cancelled".into() };
            }
        }
        self.check_finished(now_ms, &mut events);
        events
    }
}

// ---------------------------------------------------------------------------
// Driver and HTTP API
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

/// Positions of stations: Accounts (`[bts_locations]`), else the station's own telemetry position.
fn positions(state: &AppState) -> HashMap<String, (f64, f64)> {
    let mut p: HashMap<String, (f64, f64)> = HashMap::new();
    for s in state.fleet.lock().unwrap_or_else(|e| e.into_inner()).stations() {
        let (Some(login), Some(c)) = (s.brew_login.as_ref(), s.cfg.copy.as_ref()) else { continue };
        let t = c.config.get("telemetry");
        if let (Some(lat), Some(lon)) = (t.and_then(|t| t.get("latitude")).and_then(Value::as_f64), t.and_then(|t| t.get("longitude")).and_then(Value::as_f64)) {
            if crate::fedroute::valid_position(lat, lon) {
                p.insert(login.clone(), (lat, lon));
            }
        }
    }
    if let Ok(l) = state.locations.read() {
        for (k, v) in l.iter() {
            if crate::fedroute::valid_position(v.lat, v.lon) {
                p.insert(k.clone(), (v.lat, v.lon));
            }
        }
    }
    p
}

/// Every 2 s from the update driver: count new cell changes, advance the bulk run.
pub async fn tick(state: &Arc<AppState>) {
    let (moves, dir) = {
        let mut inner = state.inner.write().await;
        let m = inner.roaming_log.take_moves();
        let d = directory(&inner);
        (m, d)
    };
    let now = now_ms();
    let names = crate::control::station_names(state);
    let (turns, changed) = {
        let mut fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
        if !moves.is_empty() {
            fleet.count_moves(&moves, now);
        }
        fleet.tick_bulk(now, &dir, &|k| names.get(k).cloned())
    };
    for t in turns {
        match t {
            Turn::Save { id, base_hash, changes, summary } => {
                let by = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).bulk().and_then(|r| r.user.clone());
                crate::fleet_config::save_pending(state, &id, base_hash, changes, summary, by);
            }
            Turn::Send { id, base_hash, changes, summary, during_call } => {
                let state = state.clone();
                tokio::spawn(async move {
                    let by = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).bulk().and_then(|r| r.user.clone());
                    let r = crate::fleet_config::send_change_coded(&state, &id, base_hash, changes, summary, by, during_call).await;
                    let names = crate::control::station_names(&state);
                    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).bulk_send_result(&id, r, now_ms(), &|k| names.get(k).cloned());
                    state.monitor.emit("fleet_changed", json!({"id": id}));
                });
            }
        }
    }
    if changed {
        state.monitor.emit("fleet_changed", json!({}));
    }
}

/// `GET /api/fleet/templates` (visibility): the templates.
pub async fn api_templates(State(state): State<Arc<AppState>>) -> Response {
    let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    Json(json!({"templates": fleet.templates(), "sections": SECTIONS})).into_response()
}

#[derive(Deserialize)]
pub struct TemplateSave {
    /// Absent: a new template.
    #[serde(default)]
    pub id: Option<u64>,
    pub name: String,
    #[serde(default)]
    pub note: String,
    pub sections: Map<String, Value>,
}

/// `POST /api/fleet/templates/save` (management): create or change a template.
pub async fn api_template_save(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<TemplateSave>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let name = req.name.trim().chars().take(60).collect::<String>();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "Give the template a name").into_response();
    }
    let sections = match clean_sections(&req.sections) {
        Ok(s) if s.is_empty() => return (StatusCode::BAD_REQUEST, "Tick at least one section").into_response(),
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let note = req.note.trim().chars().take(300).collect::<String>();
    match state.fleet.lock().unwrap_or_else(|e| e.into_inner()).save_template(req.id, name, note, sections, a.user.as_deref(), now_ms()) {
        Ok(id) => {
            state.monitor.emit("fleet_changed", json!({}));
            Json(json!({"ok": true, "id": id})).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct TemplateId {
    pub id: u64,
}

/// `POST /api/fleet/templates/delete` (management).
pub async fn api_template_delete(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<TemplateId>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if !state.fleet.lock().unwrap_or_else(|e| e.into_inner()).delete_template(req.id, a.user.as_deref(), now_ms()) {
        return (StatusCode::NOT_FOUND, "No such template").into_response();
    }
    state.monitor.emit("fleet_changed", json!({}));
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct BulkSpec {
    pub ids: Vec<String>,
    /// A saved template, or …
    #[serde(default)]
    pub template: Option<u64>,
    /// … settings for this run only, or …
    #[serde(default)]
    pub sections: Option<Map<String, Value>>,
    /// … the missing reciprocal neighbours.
    #[serde(default)]
    pub reciprocal: bool,
}

fn what_of(state: &AppState, spec: &BulkSpec) -> Result<What, Response> {
    if spec.reciprocal {
        return Ok(What::Reciprocal);
    }
    if let Some(id) = spec.template {
        let t = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).templates().into_iter().find(|t| t.id == id);
        return t.map(|t| What::Sections { name: t.name, sections: t.sections }).ok_or_else(|| (StatusCode::NOT_FOUND, "No such template").into_response());
    }
    match spec.sections.as_ref().map(clean_sections) {
        Some(Ok(s)) if !s.is_empty() => Ok(What::Sections { name: String::new(), sections: s }),
        Some(Err(e)) => Err((StatusCode::BAD_REQUEST, e).into_response()),
        _ => Err((StatusCode::BAD_REQUEST, "Choose a template or settings").into_response()),
    }
}

/// `POST /api/fleet/bulk/preview` (management): per station what would change, or why it is skipped.
pub async fn api_bulk_preview(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(spec): Json<BulkSpec>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let what = match what_of(&state, &spec) {
        Ok(w) => w,
        Err(r) => return r,
    };
    let dir = directory(&*state.inner.read().await);
    let names = crate::control::station_names(&state);
    let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    let probe = Run { id: 0, started_ms: 0, user: None, what, mode: Mode::Rolling, during_call: false, interrupt_min: 0, items: vec![], cancelled: false, finished_ms: None };
    let rows: Vec<Value> = spec
        .ids
        .iter()
        .filter_map(|id| fleet.get(id))
        .map(|s| {
            let label = crate::fleet::label(&|k| names.get(k).cloned(), &s.id, s.brew_login.as_deref());
            let now = StationNow { online: s.online, capable: s.capabilities.iter().any(|c| c == crate::fleet_config::CONFIG_CAPABILITY), cfg: &s.cfg, login: s.brew_login.as_deref() };
            match probe.plan_for(&now, &dir) {
                Ok((_, p)) => json!({"id": s.id, "label": label, "online": s.online, "ok": true, "summary": p.summary, "notes": p.notes}),
                Err(reason) => json!({"id": s.id, "label": label, "online": s.online, "ok": false, "reason": reason}),
            }
        })
        .collect();
    Json(json!({"stations": rows})).into_response()
}

#[derive(Deserialize)]
pub struct BulkStart {
    #[serde(flatten)]
    pub spec: BulkSpec,
    pub mode: Mode,
    #[serde(default)]
    pub during_call: bool,
    #[serde(default = "default_interrupt")]
    pub interrupt_min: u32,
}

fn default_interrupt() -> u32 {
    10
}

/// `POST /api/fleet/bulk/start` (management).
pub async fn api_bulk_start(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<BulkStart>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    let what = match what_of(&state, &req.spec) {
        Ok(w) => w,
        Err(r) => return r,
    };
    let names = crate::control::station_names(&state);
    let interrupt = req.interrupt_min.min(crate::fleet_update::MAX_INTERRUPT_MIN);
    let r = state.fleet.lock().unwrap_or_else(|e| e.into_inner()).start_bulk(&req.spec.ids, what, req.mode, req.during_call, interrupt, a.user.as_deref(), now_ms(), &|k| names.get(k).cloned());
    match r {
        Ok(id) => {
            state.monitor.emit("fleet_changed", json!({}));
            Json(json!({"ok": true, "id": id})).into_response()
        }
        Err(e) => (StatusCode::CONFLICT, e).into_response(),
    }
}

/// `POST /api/fleet/bulk/cancel` (management): no further station.
pub async fn api_bulk_cancel(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.manages_fleet() {
        return refuse();
    }
    if !state.fleet.lock().unwrap_or_else(|e| e.into_inner()).cancel_bulk(a.user.as_deref(), now_ms()) {
        return (StatusCode::NOT_FOUND, "No bulk change running").into_response();
    }
    state.monitor.emit("fleet_changed", json!({}));
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct NbQuery {
    pub id: String,
    #[serde(default)]
    pub radius_km: Option<f64>,
}

/// `GET /api/fleet/neighbours?id=&radius_km=` (visibility): suggestions and marks per cell.
pub async fn api_neighbours(State(state): State<Arc<AppState>>, Query(q): Query<NbQuery>) -> Response {
    let dir = directory(&*state.inner.read().await);
    let pos = positions(&state);
    let radius = q.radius_km.filter(|r| r.is_finite() && *r > 0.0).unwrap_or(DEFAULT_RADIUS_KM).min(500.0);
    let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    let Some(s) = fleet.get(&q.id) else { return (StatusCode::NOT_FOUND, "No such station in the Fleet Manager").into_response() };
    let Some(copy) = s.cfg.copy.as_ref() else { return (StatusCode::CONFLICT, "No server copy of this station's config yet").into_response() };
    let moves = fleet.move_map();
    let ctx = NbContext { dir: &dir, positions: &pos, moves: &moves, radius_km: radius };
    let own_pos = s.brew_login.as_ref().and_then(|l| pos.get(l).copied());
    Json(suggestions(s.brew_login.as_deref(), &copy.config, own_pos, &ctx)).into_response()
}

/// `GET /api/fleet/neighbours/check` (visibility): fleet-wide missing reciprocal entries and marks.
pub async fn api_neighbour_check(State(state): State<Arc<AppState>>) -> Response {
    let dir = directory(&*state.inner.read().await);
    let names = crate::control::station_names(&state);
    let fleet = state.fleet.lock().unwrap_or_else(|e| e.into_inner());
    let stations = fleet.stations();
    let list: Vec<CheckStation> = stations
        .iter()
        .filter_map(|s| {
            let cfg = &s.cfg.copy.as_ref()?.config;
            Some(CheckStation { id: &s.id, label: crate::fleet::label(&|k| names.get(k).cloned(), &s.id, s.brew_login.as_deref()), login: s.brew_login.as_deref(), cfg })
        })
        .collect();
    let rows = check(&list, &dir);
    Json(json!({"stations": rows, "directory_cells": dir.len()})).into_response()
}

#[derive(Deserialize)]
pub struct ConfigSettingsReq {
    pub confirm_secs: u32,
}

/// `POST /api/fleet/config/settings` (admins): confirmation window of config changes.
pub async fn api_config_settings(State(state): State<Arc<AppState>>, headers: HeaderMap, Json(req): Json<ConfigSettingsReq>) -> Response {
    let a = crate::dashboard::access(&state, &headers);
    if !a.admin {
        return (StatusCode::FORBIDDEN, "Admins only").into_response();
    }
    if !(crate::fleet_config::MIN_CONFIRM_SECS..=crate::fleet_config::MAX_CONFIRM_SECS).contains(&req.confirm_secs) {
        return (StatusCode::BAD_REQUEST, format!("Confirmation window: {}-{} s", crate::fleet_config::MIN_CONFIRM_SECS, crate::fleet_config::MAX_CONFIRM_SECS)).into_response();
    }
    state.fleet.lock().unwrap_or_else(|e| e.into_inner()).set_confirm_secs(req.confirm_secs, a.user.as_deref(), now_ms());
    state.monitor.emit("fleet_changed", json!({}));
    Json(json!({"ok": true})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Value {
        json!({
            "mcc": 232, "mnc": 5, "networks_explicit": false, "multi_network_enabled": false, "networks": [{"name": "default", "mcc": 232, "mnc": 5, "mode": "core", "via": null}],
            "brew": {"link": {"host": "old.example", "port": 443, "tls": true, "username": 1000001, "password_set": true}},
            "reselect": {"slow_threshold_db": 10, "fast_threshold_db": 30, "slow_hysteresis_db": 4, "fast_hysteresis_db": 4},
            "rxlev_access_min_dbm": null, "migration": false, "presence_check_secs": 30,
            "service_details": {"registration": true, "voice_service": false, "system_wide_services": false, "migration": false},
            "home_mode_display": null, "access": {"issi_whitelist": [], "whitelist_mode": "auto"},
            "telemetry": {"enabled": true, "host": "t.example", "port": 443, "use_tls": true, "username": "bs1", "password_set": true, "site_name": "Hill", "latitude": 47.1, "longitude": 15.4},
            "ota": {"channel": "installed", "repo": null, "branch": null},
            "cells": [
                {"id": 0, "primary": true, "main_carrier": 1528, "carrier_ext": 268, "location_area": 2, "own_list": true, "neighbours": [], "siblings": []},
            ],
        })
    }

    fn sec(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn template_keeps_no_login_or_password_and_no_station_settings() {
        let s = clean_sections(&sec(json!({"brew": {"host": "b.example", "port": 443, "tls": true, "username": 5, "password": "x"}}))).unwrap();
        assert_eq!(s["brew"], json!({"host": "b.example", "port": 443, "tls": true}));
        assert!(clean_sections(&sec(json!({"cells": []}))).is_err(), "cells belong to one station");
        assert!(clean_sections(&sec(json!({"rxlev_access_min_dbm": -93}))).is_err(), "5 dB steps");
        let from = sections_of(&cfg());
        assert!(!from.contains_key("cells") && from["telemetry"].get("username").is_none() && from["brew"].get("username").is_none());
    }

    #[test]
    fn plan_sends_only_what_differs_and_keeps_station_values() {
        let p = plan(&sec(json!({"migration": false, "presence_check_secs": 60, "brew": {"host": "new.example", "port": 443, "tls": true}, "telemetry": {"enabled": true, "host": "t2.example", "port": 443, "use_tls": true}})), &cfg());
        let c = p.changes.as_object().unwrap();
        assert!(!c.contains_key("migration"), "same value is not sent");
        assert_eq!(c["presence_check_secs"], 60);
        // The Brew login stays the station's, the password is kept (empty).
        assert_eq!(c["brew"]["username"], 1000001);
        assert_eq!(c["brew"]["password"], "");
        // Telemetry keeps the station's login, site name and position.
        assert_eq!(c["telemetry"]["username"], "bs1");
        assert_eq!(c["telemetry"]["site_name"], "Hill");
        assert_eq!(c["telemetry"]["latitude"], 47.1);
        assert_eq!(c["telemetry"]["host"], "t2.example");
        assert_eq!(p.summary.len(), 3);
        // Applying it again changes nothing.
        let mut after = cfg();
        after["presence_check_secs"] = json!(60);
        after["brew"]["link"]["host"] = json!("new.example");
        after["telemetry"]["host"] = json!("t2.example");
        assert!(plan(&sec(json!({"presence_check_secs": 60, "brew": {"host": "new.example", "port": 443, "tls": true}})), &after).is_empty());
    }

    #[test]
    fn plan_notes_what_does_not_fit_the_station() {
        let mut c = cfg();
        c["telemetry"] = Value::Null;
        c["networks_explicit"] = json!(true);
        let p = plan(&sec(json!({"brew": {"host": "x"}, "telemetry": {"enabled": true, "host": "t"}})), &c);
        assert!(p.is_empty());
        assert_eq!(p.notes.len(), 2);
    }

    #[test]
    fn plan_whitelist_hmd_reselect_and_update_source() {
        let p = plan(
            &sec(json!({"access": {"issi_whitelist": [1, 2], "whitelist_mode": "enforce"}, "home_mode_display": {"text": "HOME", "interval_multiframes": 96},
                "reselect": {"fast_threshold_db": 20}, "ota": {"channel": "custom", "repo": "https://github.com/x/y", "branch": "main"}})),
            &cfg(),
        );
        let c = p.changes.as_object().unwrap();
        assert_eq!(c["raw"].as_array().unwrap().len(), 3);
        assert_eq!(c["reselect"]["fast_threshold_db"], 20);
        assert_eq!(c["reselect"]["slow_threshold_db"], 10, "the other reselection values stay");
        assert_eq!(c["ota"]["channel"], "custom");
    }

    #[test]
    fn multi_network_core_network_needs_the_station_login() {
        let p = plan(&sec(json!({"multi_network": {"enabled": true, "networks": [{"name": "other", "mcc": 901, "mnc": 9999, "mode": "core", "host": "o.example"}, {"name": "tmo", "mcc": 232, "mnc": 6, "mode": "via", "via": "home"}]}})), &cfg());
        let c = p.changes.as_object().unwrap();
        assert_eq!(c["multi_network_enabled"], true);
        assert_eq!(c["networks"].as_array().unwrap().len(), 1, "only the via network: the core one has no login here");
        assert_eq!(p.notes.len(), 1);
    }

    fn dcell(station: &str, cell: u8, carrier: u16, la: u16, lists: &[u16]) -> DirCell {
        DirCell {
            name: format!("{station}/{cell}"),
            station: station.into(),
            cell,
            online: true,
            mcc: 232,
            mnc: 5,
            carrier,
            carrier_ext: 268,
            location_area: la,
            rxlev_access_min_dbm: Some(-100),
            ms_txpwr_max_dbm: Some(30),
            service_details: None,
            neighbours: Some(lists.iter().map(|c| (232, 5, *c, 268)).collect()),
        }
    }

    #[test]
    fn suggestions_reciprocal_moves_distance_and_marks() {
        let mut c = cfg();
        // Lists a gone cell (1600) and BS3 with a wrong LA.
        c["cells"][0]["neighbours"] = json!([
            {"cell_identifier_ca": 0, "main_carrier_number": 1600, "location_area": 9},
            {"cell_identifier_ca": 1, "main_carrier_number": 1540, "main_carrier_number_extension": 268, "location_area": 4},
        ]);
        let dir = vec![
            dcell("1000001", 0, 1528, 2, &[]),
            dcell("1000002", 0, 1530, 1, &[1528]),
            dcell("1000003", 0, 1540, 3, &[]),
            dcell("1000004", 0, 1550, 5, &[]),
            dcell("1000005", 0, 1560, 6, &[]),
        ];
        let mut pos = HashMap::new();
        pos.insert("1000001".to_string(), (47.0, 15.0));
        pos.insert("1000004".to_string(), (47.1, 15.0));
        pos.insert("1000005".to_string(), (48.0, 15.0));
        let mut moves = HashMap::new();
        moves.insert(("1000005/0".to_string(), "1000001/0".to_string()), 3);
        let ctx = NbContext { dir: &dir, positions: &pos, moves: &moves, radius_km: 30.0 };
        let s = suggestions(Some("1000001"), &c, Some((47.0, 15.0)), &ctx);
        let cell = &s["cells"][0];
        let names: Vec<&str> = cell["suggestions"].as_array().unwrap().iter().map(|x| x["cell"]["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["1000002/0", "1000005/0", "1000004/0"], "reciprocal first, then moves, then distance");
        assert_eq!(cell["suggestions"][2]["why"][0]["kind"], "distance");
        let e = &cell["suggestions"][0]["entry"];
        assert_eq!(e["cell_identifier_ca"], 2, "next free id");
        assert_eq!(e["bs_service_details"]["voice_service"], true);
        assert_eq!(e["minimum_rx_access_level"], 5);
        let marks = cell["marks"].as_array().unwrap();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0]["kind"], "missing");
        assert_eq!(marks[1]["kind"], "la");
    }

    #[test]
    fn reciprocal_plan_adds_missing_and_respects_seven() {
        let dir = vec![dcell("1000001", 0, 1528, 2, &[]), dcell("1000002", 0, 1530, 1, &[1528]), dcell("1000003", 0, 1540, 1, &[])];
        let p = reciprocal_plan(Some("1000001"), &cfg(), &dir);
        let cells = p.changes["cells"].as_array().unwrap();
        assert_eq!(cells[0]["neighbours"].as_array().unwrap().len(), 1);
        assert_eq!(cells[0]["neighbours"][0]["main_carrier_number"], 1530);
        let mut full = cfg();
        full["cells"][0]["neighbours"] = json!((0..7).map(|i| json!({"cell_identifier_ca": i, "main_carrier_number": 100 + i})).collect::<Vec<_>>());
        let p = reciprocal_plan(Some("1000001"), &full, &dir);
        assert!(p.is_empty());
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn moves_are_counted_and_age_out() {
        let mut l = Vec::new();
        count_move(&mut l, "a/0", "b/0", 1000);
        count_move(&mut l, "a/0", "b/0", 2000);
        count_move(&mut l, "a/0", "a/0", 2000);
        assert_eq!(l, vec![MoveCount { from: "a/0".into(), to: "b/0".into(), count: 2, last_ms: 2000 }]);
        count_move(&mut l, "c/0", "d/0", 2000 + MOVES_KEEP_MS + 1);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].from, "c/0");
    }

    fn st(_online: bool, hash: &str) -> crate::fleet_config::StationConfig {
        crate::fleet_config::StationConfig {
            copy: Some(crate::fleet_config::ConfigCopy { hash: hash.into(), config: cfg(), text: String::new(), at_ms: 0, how: "read".into() }),
            reported_hash: Some(hash.into()),
            ..Default::default()
        }
    }

    type Lookup = HashMap<String, (bool, bool, crate::fleet_config::StationConfig, Option<String>)>;

    fn run(mode: Mode, ids: &[&str]) -> Run {
        let what = What::Sections { name: "T".into(), sections: sec(json!({"presence_check_secs": 60})) };
        Run::new(1, 0, Some("admin".into()), what, mode, false, 10, ids.iter().map(|i| (i.to_string(), i.to_string())).collect()).0
    }

    #[test]
    fn rolling_run_one_after_another_alarm_on_rollback_and_goes_on() {
        let mut stations: Lookup = HashMap::new();
        for id in ["a", "b", "c"] {
            stations.insert(id.into(), (true, true, st(true, "h"), None));
        }
        let mut r = run(Mode::Rolling, &["a", "b", "c"]);
        let look = |s: &Lookup| { let s = s.clone(); move |id: &str| s.get(id).cloned() };
        let (t, _) = r.tick(1000, &look(&stations), &[]);
        assert_eq!(t.len(), 1, "rolling: one at a time");
        assert!(matches!(&t[0], Turn::Send { id, .. } if id == "a"));
        r.send_result("a", Ok(90), 1000);
        stations.get_mut("a").unwrap().2.apply = Some(crate::fleet_config::ApplyRun { by: None, at_ms: 1000, old_hash: "h".into(), summary: vec![], offline: false, back_ms: None, new_hash: None, confirm_sent_ms: None, rollback_by: None, confirm_secs: 90 });
        assert!(r.tick(2000, &look(&stations), &[]).0.is_empty(), "waits for a");
        // a rolled back: alarm, b goes next.
        stations.get_mut("a").unwrap().2.apply = None;
        stations.get_mut("a").unwrap().2.last_outcome = Some(crate::fleet_config::Outcome { at_ms: 5000, ok: false, text: "rolled back by the station".into() });
        let (t, ev) = r.tick(6000, &look(&stations), &[]);
        assert!(ev.iter().any(|e| e.kind == "config_alarm"));
        assert!(matches!(&t[0], Turn::Send { id, .. } if id == "b"));
        r.send_result("b", Err(("call_active".into(), "call".into())), 6000);
        assert!(matches!(r.items[1].state, ItemState::WaitingCall { .. }));
        // Confirmed after the call.
        let (t, _) = r.tick(16_500, &look(&stations), &[]);
        assert!(matches!(&t[0], Turn::Send { id, during_call: false, .. } if id == "b"));
        r.send_result("b", Ok(90), 16_500);
        stations.get_mut("b").unwrap().2.last_outcome = Some(crate::fleet_config::Outcome { at_ms: 20_000, ok: true, text: "confirmed".into() });
        let (t, _) = r.tick(21_000, &look(&stations), &[]);
        assert_eq!(r.items[1].state, ItemState::Done);
        assert!(matches!(&t[0], Turn::Send { id, .. } if id == "c"));
        r.send_result("c", Ok(90), 21_000);
        stations.get_mut("c").unwrap().2.last_outcome = Some(crate::fleet_config::Outcome { at_ms: 30_000, ok: true, text: "confirmed".into() });
        let (_, ev) = r.tick(31_000, &look(&stations), &[]);
        assert!(r.is_finished());
        assert!(ev.last().unwrap().text.contains("2 changed, 0 pending (offline), 1 failed"));
    }

    #[test]
    fn parallel_offline_pending_and_skips() {
        let mut stations: Lookup = HashMap::new();
        stations.insert("on".into(), (true, true, st(true, "h"), None));
        stations.insert("off".into(), (false, true, st(false, "h"), None));
        let mut busy = st(false, "h");
        busy.pending = Some(crate::fleet_config::PendingChange { base_hash: "h".into(), changes: json!({}), summary: vec![], by: None, at_ms: 0, conflict: false, message: String::new(), next_try_ms: 0 });
        stations.insert("offpending".into(), (false, true, busy, None));
        let mut drift = st(true, "h");
        drift.drift = drift.copy.clone();
        stations.insert("drift".into(), (true, true, drift, None));
        let mut same = st(true, "h");
        same.copy.as_mut().unwrap().config["presence_check_secs"] = json!(60);
        stations.insert("same".into(), (true, true, same, None));
        let mut r = run(Mode::Parallel, &["on", "off", "offpending", "drift", "same"]);
        let s2 = stations.clone();
        let (t, _) = r.tick(1000, &move |id| s2.get(id).cloned(), &[]);
        assert_eq!(t.iter().filter(|t| matches!(t, Turn::Send { .. })).count(), 1);
        assert_eq!(t.iter().filter(|t| matches!(t, Turn::Save { .. })).count(), 1);
        assert_eq!(r.items[1].state, ItemState::Saved);
        assert!(matches!(&r.items[2].state, ItemState::Skipped { reason } if reason.contains("pending")));
        assert!(matches!(&r.items[3].state, ItemState::Skipped { reason } if reason.contains("differs")));
        assert!(matches!(&r.items[4].state, ItemState::Skipped { reason } if reason.contains("nothing")));
    }

    #[tokio::test]
    async fn edit_page_answers_with_the_window_and_template_sections() {
        let (state, _rx) = AppState::new(crate::config::Config::default(), std::path::PathBuf::from("test.toml"));
        let state = Arc::new(state);
        let mut rx = crate::control::attach_test_session(&state, "bs1").await;
        let hello = br#"{"StationHello":{"protocol":1,"version":"v1","brew_login":"1000001","mcc":232,"mnc":20,"fleet_control":true,"capabilities":["fleet-v1","config-v1"],"config_hash":"aa"}}"#;
        let st = state.clone();
        let deliver = tokio::spawn(async move { crate::control::deliver_test_response(&st, "bs1", hello).await });
        let v: Value = serde_json::from_slice(&rx.recv().await.unwrap()).unwrap();
        let h = v["GetConfig"]["handle"].as_u64().unwrap();
        let info = json!({"ConfigInfo": {"handle": h, "ok": true, "code": "ok", "message": "", "hash": "aa", "config": cfg(), "text": ""}});
        crate::control::deliver_test_response(&state, "bs1", &serde_json::to_vec(&info).unwrap()).await;
        deliver.await.unwrap();
        for _ in 0..100 {
            if state.fleet.lock().unwrap().get("bs1").unwrap().cfg.copy.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        // A change runs: the page does not read the station, it answers from the copy.
        state.fleet.lock().unwrap().with_station("bs1", |s| s.cfg.apply = Some(crate::fleet_config::ApplyRun { by: None, at_ms: 0, old_hash: "aa".into(), summary: vec![], offline: false, back_ms: None, new_hash: None, confirm_sent_ms: None, rollback_by: None, confirm_secs: 90 }));
        let call = crate::fleet_config::api_get(State(state.clone()), HeaderMap::new(), Query(crate::fleet_config::IdQuery { id: "bs1".into() }));
        let resp = tokio::time::timeout(std::time::Duration::from_secs(5), call).await.expect("the edit page must not hang");
        let body: Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert_eq!(body["confirm_secs"], crate::fleet_config::DEFAULT_CONFIRM_SECS);
        assert_eq!(body["template_sections"]["brew"]["host"], "old.example");
        assert!(body["template_sections"].get("cells").is_none());
        // Server setting: another window goes into the next SetConfig.
        state.fleet.lock().unwrap().set_confirm_secs(120, Some("admin"), 1);
        assert_eq!(state.fleet.lock().unwrap().confirm_secs(), 120);
    }

    #[test]
    fn distance_is_great_circle() {
        let d = distance_km((48.2082, 16.3738), (47.0707, 15.4395));
        assert!((d - 145.0).abs() < 3.0, "Vienna - Graz about 145 km, got {d}");
    }
}
