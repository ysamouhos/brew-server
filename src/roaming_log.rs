//! Cell change log ("Cell Change Log" on the dashboard): the last cell
//! changes of the radios. Not "roaming" in the ETSI sense (a change of
//! location area, TS 100 392-2), although the module keeps its name.
//!
//! A change is logged when an ISSI registers on another Basestation than the
//! one holding it (announced reselection prepared here, or any other), and when a
//! station reports the radio on another of its own cells (`SERVICE_MS_CELL`,
//! multi-cell stations). The target cell of a move between stations is only
//! known once the new station reports it, so the entry is completed by that
//! report. Kept in memory only.
//!
//! The station the radio left may report which category of cell reselection
//! the radio used (`SERVICE_MS_RESELECTION`, extension `ms-reselection`): an
//! announced type it answered with D-NEW-CELL, or, once the server told it
//! that the radio left unprepared (`SERVICE_MS_MOVED`), whether the radio had
//! a circuit mode call there. The report completes the entry, or waits for the
//! registration elsewhere when it comes first (type 3 is answered before the
//! radio leaves; a forwarded registration moves the ISSI before D-NEW-CELL).

use serde::Serialize;
use std::collections::VecDeque;

/// Entries kept.
pub const MAX_ENTRIES: usize = 200;

/// How long after a move the new station's cell report still completes it.
const CELL_REPORT_MS: u64 = 10_000;

/// How long a reselection report and the move it belongs to may be apart.
const RESELECTION_REPORT_MS: u64 = 10_000;

/// Reports kept while they wait for their move.
const MAX_WAITING_REPORTS: usize = 64;

/// How the radio changed cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Announced cell reselection (TS 100 392-2 clause 18.3.4.7) prepared
    /// through this server's handover relay. Shown as "announced"; the wire
    /// value stays `handover` for API compatibility.
    Handover,
    /// The radio registered on another Basestation without a preparation
    /// here: unannounced or undeclared reselection, or announced to a cell
    /// this server did not prepare. Shown as "not announced".
    Reselection,
    /// Another cell of the same station (multi-cell FlowStation).
    WithinStation,
}

/// The category of cell reselection the radio used (TS 100 392-2 clause
/// 18.3.4.7.0), as the station it left reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Reselection {
    /// No circuit mode call on the old cell (18.3.4.7.2).
    #[serde(rename = "undeclared")]
    Undeclared,
    /// In a circuit mode call, without telling the old cell (18.3.4.7.3).
    #[serde(rename = "unannounced")]
    Unannounced,
    /// D-NEW-CELL "follow MAC channel change" (18.3.4.7.5).
    #[serde(rename = "announced_type_1")]
    AnnouncedType1,
    /// D-NEW-CELL "change channel immediately" after U-PREPARE naming the
    /// cell (18.3.4.7.5).
    #[serde(rename = "announced_type_2")]
    AnnouncedType2,
    /// U-PREPARE without a cell, answered with D-NEW-CELL (18.3.4.7.4).
    #[serde(rename = "announced_type_3")]
    AnnouncedType3,
}

impl Reselection {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "undeclared" => Self::Undeclared,
            "unannounced" => Self::Unannounced,
            "announced_type_1" => Self::AnnouncedType1,
            "announced_type_2" => Self::AnnouncedType2,
            "announced_type_3" => Self::AnnouncedType3,
            _ => return None,
        })
    }

    fn is_announced(self) -> bool {
        matches!(self, Self::AnnouncedType1 | Self::AnnouncedType2 | Self::AnnouncedType3)
    }
}

/// Where a radio was: the station (its Brew login, see
/// `carriers::station_name`) and, when reported, the cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    pub station: String,
    /// Cell name `<station>/<cell>`, when known.
    pub cell: Option<String>,
    pub carrier: Option<u16>,
}

impl Place {
    pub fn station(station: String) -> Self {
        Self { station, cell: None, carrier: None }
    }

    pub fn cell(station: String, cell: u8, carrier: u16) -> Self {
        let name = crate::carriers::cell_name(&station, cell);
        Self { station, cell: Some(name), carrier: Some(carrier) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub at_ms: u64,
    pub issi: u32,
    pub from: Place,
    pub to: Place,
    pub kind: Kind,
    /// As reported by the station the radio left; `None` when it did not.
    pub reselection: Option<Reselection>,
}

/// A reselection report that came before its move.
struct WaitingReport {
    at_ms: u64,
    issi: u32,
    station: String,
    reselection: Reselection,
}

#[derive(Default)]
pub struct RoamingLog {
    entries: VecDeque<Entry>,
    waiting: VecDeque<WaitingReport>,
}

impl RoamingLog {
    /// Logs a move, with the reselection its old station reported shortly
    /// before (if any and the entry has none).
    pub fn push(&mut self, mut entry: Entry) {
        let at = entry.at_ms;
        self.waiting.retain(|w| at.saturating_sub(w.at_ms) <= RESELECTION_REPORT_MS);
        if let Some(i) = self.waiting.iter().rposition(|w| w.issi == entry.issi && w.station == entry.from.station) {
            let w = self.waiting.remove(i).expect("found");
            entry.reselection = entry.reselection.or(Some(w.reselection));
        }
        if self.entries.len() == MAX_ENTRIES { self.entries.pop_front(); }
        self.entries.push_back(entry);
    }

    /// `station` reported at `now_ms` that `issi` left it by `reselection`.
    /// Completes the ISSI's latest move away from that station if it is
    /// recent and has no category yet; otherwise keeps the report for the
    /// move to come. An announced type replaces a category the station
    /// reported before for the same move only if that was not announced:
    /// the D-NEW-CELL is what the radio got.
    pub fn reselection_reported(&mut self, now_ms: u64, issi: u32, station: &str, reselection: Reselection) {
        let open = self.entries.iter_mut().rev().find(|e| e.issi == issi)
            .filter(|e| e.from.station == station && now_ms.saturating_sub(e.at_ms) <= RESELECTION_REPORT_MS);
        if let Some(entry) = open {
            match entry.reselection {
                None => { entry.reselection = Some(reselection); return; }
                Some(r) if !r.is_announced() && reselection.is_announced() => { entry.reselection = Some(reselection); return; }
                // Known already. A report for a later move away from this
                // station cannot come before the radio is back there, which
                // would be the newer entry.
                Some(_) => return,
            }
        }
        self.waiting.retain(|w| now_ms.saturating_sub(w.at_ms) <= RESELECTION_REPORT_MS && !(w.issi == issi && w.station == station));
        if self.waiting.len() == MAX_WAITING_REPORTS { self.waiting.pop_front(); }
        self.waiting.push_back(WaitingReport { at_ms: now_ms, issi, station: station.to_string(), reselection });
    }

    /// `station` reported `issi` on `to` at `now_ms`, coming from cell
    /// `previous` of the same station (if it had reported one). Completes the
    /// ISSI's latest move to this station when that still lacks its cell,
    /// otherwise logs a change within the station when the cell is another.
    pub fn cell_reported(&mut self, now_ms: u64, issi: u32, to: Place, previous: Option<Place>) {
        let open = self.entries.iter_mut().rev().find(|e| e.issi == issi)
            .filter(|e| e.to.station == to.station && e.to.cell.is_none() && now_ms.saturating_sub(e.at_ms) <= CELL_REPORT_MS);
        if let Some(entry) = open {
            entry.to = to;
            return;
        }
        if let Some(from) = previous.filter(|p| p.cell != to.cell) {
            self.push(Entry { at_ms: now_ms, issi, from, to, kind: Kind::WithinStation, reselection: None });
        }
    }

    /// Newest first.
    pub fn newest_first(&self) -> Vec<Entry> {
        self.entries.iter().rev().cloned().collect()
    }
}

/// Where `client` has `issi`: its station and, when reported, the cell.
fn place(inner: &crate::state::Inner, issi: u32, client: crate::state::ClientId) -> Place {
    let station = crate::carriers::station_name(client, inner.clients.get(&client).and_then(|c| c.username.as_deref()));
    match inner.carriers.ms_cell(issi, client) {
        Some(cell) => Place::cell(station, cell.cell, cell.key.carrier),
        None => Place::station(station),
    }
}

fn is_basestation(inner: &crate::state::Inner, id: crate::state::ClientId) -> bool {
    inner.clients.get(&id).is_some_and(|c| c.mode == crate::state::ClientMode::Basestation)
}

/// `issi`, held by Basestation `from`, registered on Basestation `to`
/// (`router::handle_subscriber`, before the registration moves it).
/// `handed_over`: it came with a handover (`handover::Handovers`).
pub fn registration_moved(inner: &mut crate::state::Inner, issi: u32, from: crate::state::ClientId, to: crate::state::ClientId, handed_over: bool) {
    if from == to || !is_basestation(inner, from) || !is_basestation(inner, to) { return; }
    let entry = Entry {
        at_ms: crate::telemetry::now_ms(),
        issi,
        from: place(inner, issi, from),
        // Its cell there follows with the station's report (`cell_reported`).
        to: Place::station(crate::carriers::station_name(to, inner.clients.get(&to).and_then(|c| c.username.as_deref()))),
        kind: if handed_over { Kind::Handover } else { Kind::Reselection },
        reselection: None,
    };
    let (from_name, to_name, kind) = (entry.from.cell.clone().unwrap_or_else(|| entry.from.station.clone()), entry.to.station.clone(), entry.kind);
    inner.roaming_log.push(entry);
    let reselection = inner.roaming_log.entries.back().and_then(|e| e.reselection);
    tracing::info!(issi, from = %from_name, to = %to_name, ?kind, ?reselection, "subscriber changed cells");
}

#[derive(serde::Deserialize)]
struct ReselectionMessage {
    issi: u32,
    #[serde(rename = "type")]
    reselection: String,
}

/// A `SERVICE_MS_RESELECTION` from `source`: `{"issi":N,"type":"..."}`, how
/// the radio left that station (`Reselection`). Only from a Basestation; an
/// unknown type is ignored, so later stations may name more.
pub async fn handle_reselection(state: &std::sync::Arc<crate::state::AppState>, source: crate::state::ClientId, json: &str) {
    let msg: ReselectionMessage = match serde_json::from_str(json) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(%source, error = %e, json, "malformed MS reselection service message");
            return;
        }
    };
    let Some(reselection) = Reselection::parse(&msg.reselection) else {
        tracing::debug!(%source, issi = msg.issi, kind = %msg.reselection, "MS reselection of an unknown type ignored");
        return;
    };
    let mut inner = state.inner.write().await;
    if !is_basestation(&inner, source) { return; }
    let station = crate::carriers::station_name(source, inner.clients.get(&source).and_then(|c| c.username.as_deref()));
    tracing::info!(%source, issi = msg.issi, ?reselection, "radio left the Basestation by this cell reselection");
    inner.roaming_log.reselection_reported(crate::telemetry::now_ms(), msg.issi, &station, reselection);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(at_ms: u64, issi: u32, kind: Kind) -> Entry {
        Entry { at_ms, issi, from: Place::cell("bs1".into(), 0, 1528), to: Place::station("bs2".into()), kind, reselection: None }
    }

    use crate::{protocol::{self, ConnVersion, SUB_REGISTER}, state::{AppState, Client, ClientId, ClientMode}};
    use std::sync::Arc;

    async fn station(state: &Arc<AppState>, login: &str, mode: ClientMode, carriers: &[u16]) -> ClientId {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        std::mem::forget(rx);
        let id = uuid::Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: Some(login.into()) });
        let cells: Vec<String> = carriers.iter().enumerate()
            .map(|(i, c)| format!(r#"{{"cell":{i},"carrier":{c},"carrier_ext":268,"location_area":{c}}}"#)).collect();
        let info = format!(r#"{{"mcc":232,"mnc":5,"cells":[{}],"capabilities":["handover-v1"]}}"#, cells.join(","));
        crate::router::handle_packet(state.clone(), id, protocol::build_service(protocol::SERVICE_BTS_INFO, &info)).await;
        id
    }

    async fn register(state: &Arc<AppState>, from: ClientId, issi: u32, carrier: Option<u16>) {
        crate::router::handle_packet(state.clone(), from, protocol::build_subscriber_message(SUB_REGISTER, issi, &[])).await;
        if let Some(c) = carrier {
            crate::router::handle_packet(state.clone(), from, protocol::build_service(protocol::SERVICE_MS_CELL,
                &format!(r#"{{"issi":{issi},"carrier":{c},"carrier_ext":268}}"#))).await;
        }
    }

    async fn log(state: &Arc<AppState>) -> Vec<(u32, Option<String>, String, Option<String>, Kind)> {
        state.inner.read().await.roaming_log.newest_first().into_iter()
            .map(|e| (e.issi, e.from.cell, e.to.station, e.to.cell, e.kind)).collect()
    }

    #[tokio::test]
    async fn moves_between_and_within_stations_are_logged() {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.handover.timeout_ms = 20;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let a = station(&state, "bs1", ClientMode::Basestation, &[1528]).await;
        let b = station(&state, "bs2", ClientMode::Basestation, &[1530]).await;
        let site = station(&state, "site", ClientMode::Basestation, &[1532, 1534]).await;
        register(&state, a, 7, Some(1528)).await;
        assert!(log(&state).await.is_empty(), "a first registration is no cell change");

        // Announced handover A -> B: B registers while the request is open.
        crate::router::handle_packet(state.clone(), a, protocol::build_service(protocol::SERVICE_HO_FORWARD_REG,
            r#"{"transaction_id":1,"issi":7,"target_carrier":1530,"target_carrier_ext":268,"sdu":"00","sdu_len_bits":8}"#)).await;
        register(&state, b, 7, Some(1530)).await;
        // B never answers: the request times out and later moves to B are no handover.
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        // Unannounced back to A, then to the multi-cell site and across its cells.
        register(&state, a, 7, Some(1528)).await;
        register(&state, site, 7, Some(1532)).await;
        register(&state, site, 7, Some(1534)).await;
        // A station that does not report cells: only the station is known.
        register(&state, b, 7, None).await;

        assert_eq!(log(&state).await, vec![
            (7, Some("site/1".into()), "bs2".into(), None, Kind::Reselection),
            (7, Some("site/0".into()), "site".into(), Some("site/1".into()), Kind::WithinStation),
            (7, Some("bs1/0".into()), "site".into(), Some("site/0".into()), Kind::Reselection),
            (7, Some("bs2/0".into()), "bs1".into(), Some("bs1/0".into()), Kind::Reselection),
            (7, Some("bs1/0".into()), "bs2".into(), Some("bs2/0".into()), Kind::Handover),
        ]);
    }

    #[tokio::test]
    async fn reregistering_on_the_same_station_and_peers_are_not_moves() {
        let state = AppState::for_test();
        let a = station(&state, "bs1", ClientMode::Basestation, &[1528]).await;
        let peer = station(&state, "peer", ClientMode::Peer, &[]).await;
        let terminal = station(&state, "term", ClientMode::Terminal, &[]).await;
        register(&state, a, 7, Some(1528)).await;
        register(&state, a, 7, Some(1528)).await;
        register(&state, peer, 7, None).await;
        register(&state, a, 8, None).await;
        register(&state, terminal, 8, None).await;
        assert!(log(&state).await.is_empty());
    }

    fn reselection(state_issi: u32, kind: &str) -> Vec<u8> {
        protocol::build_service(protocol::SERVICE_MS_RESELECTION, &format!(r#"{{"issi":{state_issi},"type":"{kind}"}}"#))
    }

    async fn categories(state: &Arc<AppState>) -> Vec<(u32, Kind, Option<Reselection>)> {
        state.inner.read().await.roaming_log.newest_first().into_iter().map(|e| (e.issi, e.kind, e.reselection)).collect()
    }

    #[tokio::test]
    async fn the_station_left_names_the_reselection_before_or_after_the_move() {
        let state = AppState::for_test();
        let a = station(&state, "bs1", ClientMode::Basestation, &[1528]).await;
        let b = station(&state, "bs2", ClientMode::Basestation, &[1530]).await;
        register(&state, a, 7, Some(1528)).await;
        register(&state, a, 8, Some(1528)).await;
        register(&state, a, 9, Some(1528)).await;
        // Type 3: A answers U-PREPARE before the radio registers on B.
        crate::router::handle_packet(state.clone(), a, reselection(7, "announced_type_3")).await;
        register(&state, b, 7, Some(1530)).await;
        // Unannounced: A reports once it is told the radio left.
        register(&state, b, 8, Some(1530)).await;
        crate::router::handle_packet(state.clone(), a, reselection(8, "unannounced")).await;
        // No report: the category stays unknown.
        register(&state, b, 9, Some(1530)).await;
        // Back to A: the old report of 7 is used up, B reports nothing.
        register(&state, a, 7, Some(1528)).await;
        assert_eq!(categories(&state).await, vec![
            (7, Kind::Reselection, None),
            (9, Kind::Reselection, None),
            (8, Kind::Reselection, Some(Reselection::Unannounced)),
            (7, Kind::Reselection, Some(Reselection::AnnouncedType3)),
        ]);
    }

    #[tokio::test]
    async fn a_forwarded_registration_gets_its_type_from_the_d_new_cell() {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        let a = station(&state, "bs1", ClientMode::Basestation, &[1528]).await;
        let b = station(&state, "bs2", ClientMode::Basestation, &[1530]).await;
        register(&state, a, 7, Some(1528)).await;
        // B registers the forwarded registration first; A's D-NEW-CELL follows.
        crate::router::handle_packet(state.clone(), a, protocol::build_service(protocol::SERVICE_HO_FORWARD_REG,
            r#"{"transaction_id":1,"issi":7,"target_carrier":1530,"target_carrier_ext":268,"sdu":"00","sdu_len_bits":8}"#)).await;
        register(&state, b, 7, Some(1530)).await;
        crate::router::handle_packet(state.clone(), a, reselection(7, "announced_type_1")).await;
        // A repeat or a later "unannounced" changes nothing.
        crate::router::handle_packet(state.clone(), a, reselection(7, "unannounced")).await;
        assert_eq!(categories(&state).await, vec![(7, Kind::Handover, Some(Reselection::AnnouncedType1))]);
        let json = serde_json::to_value(state.inner.read().await.roaming_log.newest_first()).unwrap();
        assert_eq!(json[0]["kind"], "handover");
        assert_eq!(json[0]["reselection"], "announced_type_1");
    }

    #[tokio::test]
    async fn reports_from_others_unknown_types_and_garbage_change_nothing() {
        let state = AppState::for_test();
        let a = station(&state, "bs1", ClientMode::Basestation, &[1528]).await;
        let b = station(&state, "bs2", ClientMode::Basestation, &[1530]).await;
        let c = station(&state, "bs3", ClientMode::Basestation, &[1532]).await;
        let terminal = station(&state, "term", ClientMode::Terminal, &[]).await;
        register(&state, a, 7, Some(1528)).await;
        crate::router::handle_packet(state.clone(), terminal, reselection(7, "announced_type_2")).await;
        crate::router::handle_packet(state.clone(), c, reselection(7, "announced_type_2")).await;
        crate::router::handle_packet(state.clone(), a, reselection(7, "announced_type_9")).await;
        crate::router::handle_packet(state.clone(), a, protocol::build_service(protocol::SERVICE_MS_RESELECTION, "{\"issi\":7}")).await;
        crate::router::handle_packet(state.clone(), a, protocol::build_service(protocol::SERVICE_MS_RESELECTION, "nonsense")).await;
        register(&state, b, 7, Some(1530)).await;
        assert_eq!(categories(&state).await, vec![(7, Kind::Reselection, None)]);
    }

    #[test]
    fn a_report_waits_only_so_long() {
        let mut log = RoamingLog::default();
        log.reselection_reported(1000, 7, "bs1", Reselection::AnnouncedType3);
        log.push(Entry { at_ms: 1000 + RESELECTION_REPORT_MS + 1, ..moved(0, 7, Kind::Reselection) });
        log.reselection_reported(30_000, 8, "bs1", Reselection::AnnouncedType2);
        log.push(Entry { at_ms: 30_500, ..moved(0, 8, Kind::Reselection) });
        // A report after the move completes it only while it is recent.
        log.push(moved(50_000, 9, Kind::Reselection));
        log.reselection_reported(50_000 + RESELECTION_REPORT_MS + 1, 9, "bs1", Reselection::Undeclared);
        let all: Vec<_> = log.newest_first().into_iter().map(|e| (e.issi, e.reselection)).collect();
        assert_eq!(all, vec![(9, None), (8, Some(Reselection::AnnouncedType2)), (7, None)]);
    }

    #[test]
    fn announced_replaces_a_guess_but_not_the_other_way_round() {
        let mut log = RoamingLog::default();
        log.push(moved(1000, 7, Kind::Reselection));
        log.reselection_reported(1100, 7, "bs1", Reselection::Unannounced);
        log.reselection_reported(1200, 7, "bs1", Reselection::AnnouncedType2);
        log.reselection_reported(1300, 7, "bs1", Reselection::Undeclared);
        assert_eq!(log.newest_first()[0].reselection, Some(Reselection::AnnouncedType2));
        // Nothing was left waiting for a later move.
        log.push(moved(1400, 7, Kind::Reselection));
        assert_eq!(log.newest_first()[0].reselection, None);
    }

    #[test]
    fn categories_parse_and_serialize_alike() {
        for r in [Reselection::Undeclared, Reselection::Unannounced, Reselection::AnnouncedType1, Reselection::AnnouncedType2, Reselection::AnnouncedType3] {
            let name = serde_json::to_value(r).unwrap();
            assert_eq!(Reselection::parse(name.as_str().unwrap()), Some(r));
        }
        assert_eq!(Reselection::parse("handover"), None);
    }

    /// Simulation: 10 stations, 50 radios, 1000 moves over Brew. Each move
    /// gets a random category, reported by the station the radio left
    /// before the registration elsewhere, after it, or not at all; every
    /// fifth move goes straight back (ping-pong). Every logged move must
    /// carry exactly the category reported for it (the log keeps the last 200).
    #[tokio::test]
    async fn sim_10_stations_50_radios_categories_match_the_reports() {
        const STATIONS: usize = 10;
        const RADIOS: u32 = 50;
        const MOVES: usize = 1000;
        let state = AppState::for_test();
        let mut stations = Vec::new();
        for i in 0..STATIONS {
            stations.push(station(&state, &format!("bs{i}"), ClientMode::Basestation, &[1500 + i as u16]).await);
        }
        let names = ["undeclared", "unannounced", "announced_type_1", "announced_type_2", "announced_type_3"];
        let mut seed: u64 = 0x5EB_A571A;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        let mut loc = vec![0usize; RADIOS as usize];
        for r in 0..RADIOS {
            loc[r as usize] = next(STATIONS);
            register(&state, stations[loc[r as usize]], 7000 + r, None).await;
        }
        let mut expected = Vec::new();
        let mut back: Option<(u32, usize)> = None;
        for m in 0..MOVES {
            let (r, to) = match back.take() {
                Some(b) => b,
                None => {
                    let r = next(RADIOS as usize) as u32;
                    let mut to = next(STATIONS);
                    if to == loc[r as usize] { to = (to + 1) % STATIONS; }
                    (r, to)
                }
            };
            let from = loc[r as usize];
            let issi = 7000 + r;
            let choice = next(names.len() + 1);
            let category = names.get(choice).copied();
            let before = next(2) == 0;
            if let (Some(c), true) = (category, before) {
                crate::router::handle_packet(state.clone(), stations[from], reselection(issi, c)).await;
            }
            register(&state, stations[to], issi, None).await;
            if let (Some(c), false) = (category, before) {
                crate::router::handle_packet(state.clone(), stations[from], reselection(issi, c)).await;
            }
            loc[r as usize] = to;
            expected.push((issi, category.and_then(Reselection::parse)));
            if m % 5 == 0 { back = Some((r, from)); }
        }
        let got: Vec<(u32, Option<Reselection>)> = state.inner.read().await.roaming_log.newest_first().into_iter().rev().map(|e| (e.issi, e.reselection)).collect();
        assert_eq!(got.len(), MAX_ENTRIES);
        assert_eq!(got, expected[MOVES - MAX_ENTRIES..]);
    }

    #[test]
    fn keeps_the_newest_entries() {
        let mut log = RoamingLog::default();
        for i in 0..(MAX_ENTRIES as u32 + 5) { log.push(moved(u64::from(i), i, Kind::Reselection)); }
        let all = log.newest_first();
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0].issi, MAX_ENTRIES as u32 + 4);
        assert_eq!(all.last().unwrap().issi, 5);
    }

    #[test]
    fn cell_report_completes_the_move() {
        let mut log = RoamingLog::default();
        log.push(moved(1000, 7, Kind::Handover));
        log.cell_reported(1500, 7, Place::cell("bs2".into(), 0, 1530), None);
        let all = log.newest_first();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].to.cell.as_deref(), all[0].to.carrier), (Some("bs2/0"), Some(1530)));
        // A second report of the same cell adds nothing.
        log.cell_reported(2000, 7, Place::cell("bs2".into(), 0, 1530), Some(Place::cell("bs2".into(), 0, 1530)));
        assert_eq!(log.newest_first().len(), 1);
    }

    #[test]
    fn late_or_foreign_reports_do_not_complete_a_move() {
        let mut log = RoamingLog::default();
        log.push(moved(1000, 7, Kind::Reselection));
        log.cell_reported(1000 + CELL_REPORT_MS + 1, 7, Place::cell("bs2".into(), 0, 1530), None);
        log.cell_reported(1500, 7, Place::cell("bs3".into(), 0, 1532), None);
        assert!(log.newest_first()[0].to.cell.is_none());
    }

    #[test]
    fn another_cell_of_the_station_is_a_move_within_it() {
        let mut log = RoamingLog::default();
        log.cell_reported(1000, 7, Place::cell("site".into(), 3, 1530), Some(Place::cell("site".into(), 0, 1528)));
        let all = log.newest_first();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, Kind::WithinStation);
        assert_eq!((all[0].from.cell.as_deref(), all[0].to.cell.as_deref()), (Some("site/0"), Some("site/3")));
        // The first report of a radio is no move.
        log.cell_reported(1000, 8, Place::cell("site".into(), 0, 1528), None);
        assert_eq!(log.newest_first().len(), 1);
    }
}
