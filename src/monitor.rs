use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, VecDeque}, time::{SystemTime, UNIX_EPOCH}};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord { pub uuid: Uuid, pub kind: String, pub source: u32, pub destination: u32, pub priority: u8, pub started_at_ms: u64, pub ended_at_ms: Option<u64>, pub voice_frames: u64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdsRecord { pub uuid: Uuid, pub source: u32, pub destination: u32, pub at_ms: u64, pub reports: u32 }
#[derive(Debug, Clone, Serialize)]
pub struct LiveEvent { pub event: String, pub at_ms: u64, pub data: serde_json::Value }
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot { pub connected_basestations: usize, pub subscribers: usize, pub groups: usize, pub active_calls: Vec<Tagged<CallRecord>>, pub recent_calls: Vec<Tagged<CallRecord>>, pub recent_sds: Vec<Tagged<SdsRecord>>, pub total_calls: u64, pub total_sds: u64, pub voice_frames: u64,
    /// Calls / SDS kept in the history (all of them, beyond what is listed).
    pub kept_calls: usize, pub kept_sds: usize,
    /// `[storage] keep_days` (0 = no age limit).
    pub keep_days: u32 }

/// Other Brew networks (Multi Network) a call or SDS crossed: the one it came
/// from over a link, and the ones it was sent to; for an SDS also its
/// protocol identifier (ETSI TS 100 392-2 clause 29.4.3.9, the first byte of
/// the type 4 user data), which tells text, position, WAP and so on apart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetTag {
    #[serde(rename = "from_network", default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(rename = "to_networks", default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    #[serde(rename = "protocol_id", default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u8>,
}

/// One line of the networks file `<history path>.nets` (JSON lines): the
/// whole tag of a record, the last line of a uuid wins.
#[derive(Serialize, Deserialize)]
struct NetLine {
    uuid: Uuid,
    #[serde(flatten)]
    tag: NetTag,
}

/// A history record as the dashboard gets it, with its networks. The
/// networks are not in the history file, which stays readable by older
/// versions (bincode has no optional fields): they go to the sidecar file
/// `<history path>.nets`, so they survive a restart.
#[derive(Debug, Clone, Serialize)]
pub struct Tagged<T> {
    #[serde(flatten)]
    pub rec: T,
    #[serde(flatten)]
    pub nets: NetTag,
}

impl<T> std::ops::Deref for Tagged<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.rec }
}

/// How many network tags are kept at most (active calls plus both history
/// lists hold far fewer).
const MAX_TAGS: usize = 1000;

#[derive(Default)] struct Inner { active: HashMap<Uuid, CallRecord>, calls: VecDeque<CallRecord>, sds: VecDeque<SdsRecord>, total_calls: u64, total_sds: u64, voice_frames: u64, nets: HashMap<Uuid, NetTag>, keep: crate::config::StorageConfig,
    /// Lines in the networks file (rewritten once it holds far more than the tags).
    net_lines: usize }

impl Inner {
    /// Trims the lists to the `[storage]` limits (count, and age when `age`).
    fn trim(&mut self, age: bool) {
        let (mc, ms) = (self.keep.memory_calls(), self.keep.memory_sds());
        while self.calls.len() > mc { self.calls.pop_back(); }
        while self.sds.len() > ms { self.sds.pop_back(); }
        if age && self.keep.keep_days > 0 {
            let cut = now_ms().saturating_sub(self.keep.keep_days as u64 * 86_400_000);
            self.calls.retain(|c| c.ended_at_ms.unwrap_or(c.started_at_ms) >= cut);
            self.sds.retain(|r| r.at_ms >= cut);
        }
    }

    fn tagged<T: Clone>(&self, uuid: Uuid, rec: &T) -> Tagged<T> {
        Tagged { rec: rec.clone(), nets: self.nets.get(&uuid).cloned().unwrap_or_default() }
    }

    /// Drops the tags of records no longer shown.
    fn prune_tags(&mut self) {
        if self.nets.len() <= MAX_TAGS { return; }
        let keep: std::collections::HashSet<Uuid> = self.active.keys().copied()
            .chain(self.calls.iter().map(|c| c.uuid)).chain(self.sds.iter().map(|s| s.uuid)).collect();
        self.nets.retain(|u, _| keep.contains(u));
    }

    /// Drops the tags of every record no longer in the history.
    fn drop_stale_tags(&mut self) {
        let keep: std::collections::HashSet<Uuid> = self.active.keys().copied()
            .chain(self.calls.iter().map(|c| c.uuid)).chain(self.sds.iter().map(|s| s.uuid)).collect();
        self.nets.retain(|u, _| keep.contains(u));
    }
}

/// The networks file next to the history file.
fn nets_path(store: &crate::store::Store) -> std::path::PathBuf {
    let mut p = store.path().as_os_str().to_owned();
    p.push(".nets");
    p.into()
}

/// Reads the networks file (missing or broken lines are skipped).
fn read_nets(path: &std::path::Path) -> (HashMap<Uuid, NetTag>, usize) {
    let mut nets = HashMap::new();
    let mut lines = 0;
    if let Ok(text) = std::fs::read_to_string(path) {
        for l in text.lines() {
            lines += 1;
            if let Ok(n) = serde_json::from_str::<NetLine>(l) { nets.insert(n.uuid, n.tag); }
        }
    }
    (nets, lines)
}

/// Rewrites the networks file with `nets` only (temporary file + rename).
fn write_nets(path: &std::path::Path, nets: &HashMap<Uuid, NetTag>) -> std::io::Result<()> {
    let mut out = String::new();
    for (uuid, tag) in nets {
        out.push_str(&serde_json::to_string(&NetLine { uuid: *uuid, tag: tag.clone() }).unwrap_or_default());
        out.push('\n');
    }
    let tmp = path.with_extension("nets.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)
}

/// Appends the tag of `uuid` to the networks file.
fn append_net(path: &std::path::Path, uuid: Uuid, tag: &NetTag) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = serde_json::to_string(&NetLine { uuid, tag: tag.clone() }).unwrap_or_default();
    line.push('\n');
    f.write_all(line.as_bytes())
}

pub struct Monitor { inner: RwLock<Inner>, tx: broadcast::Sender<LiveEvent>, store: Option<std::sync::Arc<crate::store::Store>> }
impl Monitor {
    pub fn new() -> Self { let (tx, _) = broadcast::channel(256); Self { inner: RwLock::new(Inner::default()), tx, store: None } }

    /// Creates a Monitor backed by an append-only store, replaying existing
    /// history from disk so counters and recent lists survive restarts.
    #[cfg(test)]
    pub fn with_store(store: std::sync::Arc<crate::store::Store>) -> Self {
        Self::with_store_cfg(store, &crate::config::StorageConfig::default())
    }

    /// `with_store` with the `[storage]` limits. The totals start from what
    /// earlier compactions dropped (`<path>.totals`).
    pub fn with_store_cfg(store: std::sync::Arc<crate::store::Store>, keep: &crate::config::StorageConfig) -> Self {
        let (tx, _) = broadcast::channel(256);
        let mut inner = Inner { keep: keep.clone(), ..Inner::default() };
        let base = store.dropped_totals();
        inner.total_calls = base.calls;
        inner.total_sds = base.sds;
        inner.voice_frames = base.voice_frames;
        if let Ok(records) = crate::store::Store::replay(store.path()) {
            for rec in records {
                apply_record(&mut inner, rec);
            }
            inner.trim(true);
            tracing::info!(path = %store.path().display(), calls = inner.total_calls, sds = inner.total_sds, "replayed persisted history");
        }
        let np = nets_path(&store);
        let (nets, lines) = read_nets(&np);
        inner.nets = nets;
        inner.drop_stale_tags();
        if lines > 0 {
            if let Err(e) = write_nets(&np, &inner.nets) { tracing::warn!(error = %e, "cannot rewrite the networks file"); }
        }
        inner.net_lines = inner.nets.len();
        Self { inner: RwLock::new(inner), tx, store: Some(store) }
    }

    /// Adds a history record replicated from the HA peer (already appended
    /// to the store by the caller) to the in-memory lists and counters.
    pub async fn ingest_replicated(&self, rec: crate::store::StoredRecord) {
        apply_record(&mut *self.inner.write().await, rec);
    }

    fn persist(&self, rec: &crate::store::StoredRecord) {
        if let Some(store) = &self.store {
            if let Err(e) = store.append(rec) {
                tracing::error!(error = %e, "failed to persist history record");
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> { self.tx.subscribe() }
    pub fn emit(&self, event: &str, data: serde_json::Value) { let _ = self.tx.send(LiveEvent { event: event.into(), at_ms: now_ms(), data }); }
    pub async fn call_started(&self, uuid: Uuid, kind: &str, source: u32, destination: u32, priority: u8) {
        let rec = CallRecord { uuid, kind: kind.into(), source, destination, priority, started_at_ms: now_ms(), ended_at_ms: None, voice_frames: 0 };
        let mut i = self.inner.write().await; if i.active.insert(uuid, rec.clone()).is_none() { i.total_calls += 1; }
        drop(i); self.emit("call_started", serde_json::json!(rec));
    }
    pub async fn call_ended(&self, uuid: Uuid) { let mut i = self.inner.write().await; if let Some(mut r)=i.active.remove(&uuid) { r.ended_at_ms=Some(now_ms()); i.calls.push_front(r.clone()); i.trim(false); drop(i); self.persist(&crate::store::StoredRecord::Call(r.clone())); self.emit("call_ended", serde_json::json!(r)); } }
    pub async fn voice_frame(&self, uuid: Uuid) { let mut i=self.inner.write().await; i.voice_frames+=1; if let Some(r)=i.active.get_mut(&uuid){r.voice_frames+=1;} }
    pub async fn sds(&self, uuid: Uuid, source: u32, destination: u32) { let r=SdsRecord{uuid,source,destination,at_ms:now_ms(),reports:0}; let mut i=self.inner.write().await; i.total_sds+=1; i.sds.push_front(r.clone()); i.trim(false); drop(i); self.persist(&crate::store::StoredRecord::Sds(r.clone())); self.emit("sds",serde_json::json!(r)); }
    pub async fn sds_report(&self, uuid: Uuid) { let mut i=self.inner.write().await; if let Some(r)=i.sds.iter_mut().find(|r|r.uuid==uuid){r.reports+=1;} drop(i); self.persist(&crate::store::StoredRecord::SdsReport { uuid }); }
    /// Notes that call or SDS `uuid` came from network `from` and/or went to
    /// networks `to` (added to what is known of it already).
    pub async fn note_networks(&self, uuid: Uuid, from: Option<String>, to: Vec<String>) {
        if from.is_none() && to.is_empty() { return; }
        self.update_tag(uuid, |tag| {
            if tag.from.is_none() { tag.from = from; }
            for n in to { if !tag.to.contains(&n) { tag.to.push(n); } }
            tag.to.sort();
        }).await;
    }

    /// Notes the protocol identifier of SDS `uuid` (the first one stays).
    pub async fn note_protocol(&self, uuid: Uuid, pid: u8) {
        self.update_tag(uuid, |tag| { tag.pid.get_or_insert(pid); }).await;
    }

    /// Changes the tag of `uuid` and saves it to the networks file.
    async fn update_tag(&self, uuid: Uuid, f: impl FnOnce(&mut NetTag)) {
        let mut i = self.inner.write().await;
        let tag = i.nets.entry(uuid).or_default();
        let before = tag.clone();
        f(tag);
        let (changed, tag) = (*tag != before, tag.clone());
        i.prune_tags();
        let Some(store) = &self.store else { return; };
        if !changed { return; }
        let np = nets_path(store);
        let res = if i.net_lines >= 2 * MAX_TAGS {
            i.net_lines = i.nets.len();
            write_nets(&np, &i.nets)
        } else {
            i.net_lines += 1;
            append_net(&np, uuid, &tag)
        };
        if let Err(e) = res { tracing::warn!(error = %e, "cannot write the networks file"); }
    }
    #[cfg(test)]
    pub async fn snapshot(&self, clients: usize, subscribers: usize, groups: usize) -> Snapshot { self.snapshot_n(clients, subscribers, groups, 50, 50).await }
    /// `snapshot` with the newest `calls` calls and `sds` SDS listed.
    pub async fn snapshot_n(&self, clients: usize, subscribers: usize, groups: usize, calls: usize, sds: usize) -> Snapshot { let i=self.inner.read().await; Snapshot{connected_basestations:clients,subscribers,groups,active_calls:i.active.values().map(|c|i.tagged(c.uuid,c)).collect(),recent_calls:i.calls.iter().take(calls).map(|c|i.tagged(c.uuid,c)).collect(),recent_sds:i.sds.iter().take(sds).map(|s|i.tagged(s.uuid,s)).collect(),total_calls:i.total_calls,total_sds:i.total_sds,voice_frames:i.voice_frames,kept_calls:i.calls.len(),kept_sds:i.sds.len(),keep_days:i.keep.keep_days} }

    /// Applies `[storage]` retention to the history (log and lists), and with
    /// `clear_calls` / `clear_sds` drops every call / SDS (the dashboard's
    /// Clear buttons; SDS includes delivery reports and the telemetry SDS
    /// log). The totals stay. Returns what was dropped from the log.
    pub async fn apply_retention(&self, clear_calls: bool, clear_sds: bool) -> crate::store::DroppedTotals {
        let keep = self.inner.read().await.keep.clone();
        let dropped = match &self.store {
            Some(store) => {
                let now = now_ms();
                match store.compact(|recs| crate::store::retention(recs, &keep, now, clear_calls, clear_sds)) {
                    Ok(d) => d,
                    Err(e) => { tracing::error!(error = %e, "cannot compact the history log"); Default::default() }
                }
            }
            None => Default::default(),
        };
        let mut i = self.inner.write().await;
        if clear_calls { i.calls.clear(); }
        if clear_sds { i.sds.clear(); }
        i.trim(true);
        i.drop_stale_tags();
        if let Some(store) = &self.store {
            let np = nets_path(store);
            if i.net_lines > 0 || np.exists() {
                if let Err(e) = write_nets(&np, &i.nets) { tracing::warn!(error = %e, "cannot rewrite the networks file"); }
            }
            i.net_lines = i.nets.len();
        }
        dropped
    }
}

/// Applies retention / Clear to the whole history (calls, SDS, telemetry SDS
/// log); see `Monitor::apply_retention`.
pub async fn prune_history(state: &crate::state::AppState, clear_calls: bool, clear_sds: bool) -> crate::store::DroppedTotals {
    let d = state.monitor.apply_retention(clear_calls, clear_sds).await;
    state.telemetry.write().await.prune_sds_history(state.config.storage.keep_days, clear_sds);
    if d.calls > 0 || d.sds > 0 || clear_calls || clear_sds {
        tracing::info!(calls = d.calls, sds = d.sds, clear_calls, clear_sds, "history trimmed");
    }
    d
}

/// Applies `[storage]` retention at the start and then once an hour.
pub async fn run_retention(state: std::sync::Arc<crate::state::AppState>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        tick.tick().await;
        prune_history(&state, false, false).await;
    }
}

/// Folds one persisted record into the in-memory history (startup replay and
/// HA replication).
fn apply_record(inner: &mut Inner, rec: crate::store::StoredRecord) {
    match rec {
        crate::store::StoredRecord::Call(r) => {
            inner.total_calls += 1;
            inner.voice_frames += r.voice_frames;
            inner.calls.push_front(r);
            inner.trim(false);
        }
        crate::store::StoredRecord::Sds(r) => {
            inner.total_sds += 1;
            inner.sds.push_front(r);
            inner.trim(false);
        }
        crate::store::StoredRecord::SdsReport { uuid } => {
            if let Some(r) = inner.sds.iter_mut().find(|r| r.uuid == uuid) { r.reports += 1; }
        }
        crate::store::StoredRecord::SdsTelemetry(_) => {}
    }
}

#[cfg(test)]
mod network_tag_tests {
    use super::*;

    /// Without a network nothing new is in the JSON; tags merge, and the
    /// stored record stays as before (no network in the history file).
    #[tokio::test]
    async fn network_tags_show_only_where_set() {
        let m = Monitor::new();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        m.call_started(a, "private", 1, 2, 0).await;
        m.call_started(b, "private", 3, 4, 0).await;
        m.note_networks(b, Some("Heim".into()), vec!["TP".into()]).await;
        m.note_networks(b, Some("Other".into()), vec!["Heim".into(), "TP".into()]).await;
        let snap = m.snapshot(0, 0, 0).await;
        let json = serde_json::to_value(&snap).unwrap();
        let row = |u: Uuid| json["active_calls"].as_array().unwrap().iter().find(|c| c["uuid"] == serde_json::json!(u)).unwrap().clone();
        assert!(row(a).get("from_network").is_none() && row(a).get("to_networks").is_none());
        assert_eq!(row(b)["from_network"], serde_json::json!("Heim"), "the first source stays");
        assert_eq!(row(b)["to_networks"], serde_json::json!(["Heim", "TP"]));
        m.call_ended(b).await;
        let snap = m.snapshot(0, 0, 0).await;
        assert_eq!(snap.recent_calls[0].nets.to, vec!["Heim".to_string(), "TP".to_string()], "kept in the call log");
        let stored = bincode::serialize(&crate::store::StoredRecord::Call(snap.recent_calls[0].rec.clone())).unwrap();
        assert!(!stored.windows(4).any(|w| w == b"Heim"), "networks are not written to the history file");
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn history_survives_restart_via_store() {
        let path = std::env::temp_dir().join(format!("brew-mon-test-{}.bin", Uuid::new_v4().simple()));
        let uuid_call = Uuid::new_v4();
        let uuid_sds = Uuid::new_v4();
        {
            let store = Arc::new(crate::store::Store::open(&path).unwrap());
            let m = Monitor::with_store(store);
            m.call_started(uuid_call, "group", 90, 1001, 0).await;
            m.call_ended(uuid_call).await;
            m.sds(uuid_sds, 90, 100).await;
            m.sds_report(uuid_sds).await;
            let snap = m.snapshot(0, 0, 0).await;
            assert_eq!(snap.total_calls, 1);
            assert_eq!(snap.total_sds, 1);
        }
        // New Monitor over the same file must replay the history.
        {
            let store = Arc::new(crate::store::Store::open(&path).unwrap());
            let m = Monitor::with_store(store);
            let snap = m.snapshot(0, 0, 0).await;
            assert_eq!(snap.total_calls, 1, "call count restored");
            assert_eq!(snap.total_sds, 1, "sds count restored");
            assert_eq!(snap.recent_calls.len(), 1);
            assert_eq!(snap.recent_sds.len(), 1);
            assert_eq!(snap.recent_sds[0].reports, 1, "sds report replayed");
        }
        std::fs::remove_file(&path).ok();
    }

    /// The networks of calls and SDS survive a restart (sidecar file), the
    /// history file holds none, and Clear drops them with their records.
    #[tokio::test]
    async fn network_tags_survive_restart() {
        let path = std::env::temp_dir().join(format!("brew-nets-test-{}.bin", Uuid::new_v4().simple()));
        let nets = format!("{}.nets", path.display());
        let (call, sds, plain) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        {
            let m = Monitor::with_store(Arc::new(crate::store::Store::open(&path).unwrap()));
            m.call_started(call, "group", 2320972, 91, 0).await;
            m.note_networks(call, None, vec!["TetraPack".into()]).await;
            m.note_networks(call, Some("Heim".into()), vec!["TetraPack".into()]).await;
            m.call_ended(call).await;
            m.sds(sds, 2320972, 9999).await;
            m.note_networks(sds, None, vec!["TetraPack".into()]).await;
            m.note_protocol(sds, 0x0A).await;
            m.note_protocol(sds, 0x82).await;
            m.sds(plain, 2320972, 2320001).await;
        }
        assert!(!std::fs::read(&path).unwrap().windows(9).any(|w| w == b"TetraPack"), "history file unchanged");
        {
            let m = Monitor::with_store(Arc::new(crate::store::Store::open(&path).unwrap()));
            let snap = m.snapshot(0, 0, 0).await;
            assert_eq!(snap.recent_calls[0].nets, NetTag { from: Some("Heim".into()), to: vec!["TetraPack".into()], pid: None });
            let tag = |u: Uuid| snap.recent_sds.iter().find(|s| s.uuid == u).unwrap().nets.clone();
            assert_eq!(tag(sds).to, vec!["TetraPack".to_string()]);
            assert_eq!(tag(sds).pid, Some(0x0A), "the protocol identifier survives, the first one stays");
            assert_eq!(tag(plain), NetTag::default(), "a local SDS has no network");
            assert_eq!(std::fs::read_to_string(&nets).unwrap().lines().count(), 2, "rewritten to one line per record");
            m.apply_retention(true, false).await;
        }
        {
            let m = Monitor::with_store(Arc::new(crate::store::Store::open(&path).unwrap()));
            let snap = m.snapshot(0, 0, 0).await;
            assert!(snap.recent_calls.is_empty());
            assert_eq!(snap.recent_sds.iter().find(|s| s.uuid == sds).unwrap().nets.to, vec!["TetraPack".to_string()]);
            assert_eq!(std::fs::read_to_string(&nets).unwrap().lines().count(), 1, "the cleared call's networks are gone");
        }
        for f in [path.display().to_string(), nets, format!("{}.totals", path.display())] { std::fs::remove_file(f).ok(); }
    }

    /// Retention: at most `max_calls` calls, nothing older than `keep_days`,
    /// Clear drops all; the totals keep counting and survive a restart.
    #[tokio::test]
    async fn retention_and_clear_keep_the_totals() {
        let path = std::env::temp_dir().join(format!("brew-ret-test-{}.bin", Uuid::new_v4().simple()));
        let keep = crate::config::StorageConfig { path: path.clone(), keep_days: 7, max_calls: 30, max_sds: 20, ..Default::default() };
        let old = now_ms() - 8 * 86_400_000;
        {
            // 10 calls and 5 SDS from 8 days ago, written straight to the log.
            let store = crate::store::Store::open(&path).unwrap();
            for n in 0..10u64 {
                store.append(&crate::store::StoredRecord::Call(CallRecord { uuid: Uuid::new_v4(), kind: "group".into(), source: 1, destination: 9, priority: 0, started_at_ms: old + n, ended_at_ms: Some(old + n + 1), voice_frames: 2 })).unwrap();
            }
            for n in 0..5u64 {
                store.append(&crate::store::StoredRecord::Sds(SdsRecord { uuid: Uuid::new_v4(), source: 1, destination: 2, at_ms: old + n, reports: 0 })).unwrap();
            }
        }
        let store = Arc::new(crate::store::Store::open(&path).unwrap());
        let m = Monitor::with_store_cfg(store.clone(), &keep);
        // 25 radios: 50 calls and 25 SDS now.
        for n in 0..50u32 { let u = Uuid::new_v4(); m.call_started(u, "group", 5000 + n % 25, 9, 0).await; m.call_ended(u).await; }
        for n in 0..25u32 { let u = Uuid::new_v4(); m.sds(u, 5000 + n, 2).await; m.sds_report(u).await; }
        let snap = m.snapshot_n(0, 0, 0, usize::MAX, usize::MAX).await;
        assert_eq!((snap.kept_calls, snap.kept_sds), (30, 20), "memory keeps the limits");
        assert_eq!((snap.total_calls, snap.total_sds), (60, 30));
        let d = m.apply_retention(false, false).await;
        assert_eq!((d.calls, d.sds), (30, 10), "old ones and those over the limit leave the log");
        let recs = crate::store::Store::replay(&path).unwrap();
        let count = |f: fn(&crate::store::StoredRecord) -> bool| recs.iter().filter(|r| f(r)).count();
        assert_eq!(count(|r| matches!(r, crate::store::StoredRecord::Call(_))), 30);
        assert_eq!(count(|r| matches!(r, crate::store::StoredRecord::Sds(_))), 20);
        assert_eq!(count(|r| matches!(r, crate::store::StoredRecord::SdsReport { .. })), 20, "reports of kept SDS only");
        assert!(recs.iter().all(|r| !matches!(r, crate::store::StoredRecord::Call(c) if c.started_at_ms < now_ms() - 86_400_000)), "nothing older than 7 days");
        // Appends still work after the rewrite.
        let u = Uuid::new_v4(); m.call_started(u, "private", 1, 2, 0).await; m.call_ended(u).await;
        assert_eq!(crate::store::Store::replay(&path).unwrap().len(), 71);
        m.apply_retention(false, true).await;
        let snap = m.snapshot(0, 0, 0).await;
        assert!(snap.recent_sds.is_empty() && snap.total_sds == 30, "Clear all SDS: list empty, total stays");
        drop(m);
        let m = Monitor::with_store_cfg(Arc::new(crate::store::Store::open(&path).unwrap()), &keep);
        let snap = m.snapshot(0, 0, 0).await;
        assert_eq!((snap.total_calls, snap.total_sds), (61, 30), "totals survive a restart");
        assert_eq!(snap.voice_frames, 20, "voice frames of dropped calls stay counted");
        assert!(snap.recent_sds.is_empty());
        m.apply_retention(true, false).await;
        assert!(m.snapshot(0, 0, 0).await.recent_calls.is_empty(), "Clear all calls");
        assert_eq!(m.snapshot(0, 0, 0).await.total_calls, 61);
        for p in [path.clone(), std::path::PathBuf::from(format!("{}.totals", path.display()))] { std::fs::remove_file(p).ok(); }
    }
}
