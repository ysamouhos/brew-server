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
pub struct Snapshot { pub connected_basestations: usize, pub subscribers: usize, pub groups: usize, pub active_calls: Vec<Tagged<CallRecord>>, pub recent_calls: Vec<Tagged<CallRecord>>, pub recent_sds: Vec<Tagged<SdsRecord>>, pub total_calls: u64, pub total_sds: u64, pub voice_frames: u64 }

/// Other Brew networks (Multi Network) a call or SDS crossed: the one it came
/// from over a link, and the ones it was sent to.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct NetTag {
    #[serde(rename = "from_network", skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(rename = "to_networks", skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
}

/// A history record as the dashboard gets it, with its networks. The
/// networks are kept in memory only: the history file stays readable by
/// older versions (bincode has no optional fields).
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

#[derive(Default)] struct Inner { active: HashMap<Uuid, CallRecord>, calls: VecDeque<CallRecord>, sds: VecDeque<SdsRecord>, total_calls: u64, total_sds: u64, voice_frames: u64, nets: HashMap<Uuid, NetTag> }

impl Inner {
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
}

pub struct Monitor { inner: RwLock<Inner>, tx: broadcast::Sender<LiveEvent>, store: Option<std::sync::Arc<crate::store::Store>> }
impl Monitor {
    pub fn new() -> Self { let (tx, _) = broadcast::channel(256); Self { inner: RwLock::new(Inner::default()), tx, store: None } }

    /// Creates a Monitor backed by an append-only store, replaying existing
    /// history from disk so counters and recent lists survive restarts.
    pub fn with_store(store: std::sync::Arc<crate::store::Store>) -> Self {
        let (tx, _) = broadcast::channel(256);
        let mut inner = Inner::default();
        if let Ok(records) = crate::store::Store::replay(store.path()) {
            for rec in records {
                apply_record(&mut inner, rec);
            }
            tracing::info!(path = %store.path().display(), calls = inner.total_calls, sds = inner.total_sds, "replayed persisted history");
        }
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
    pub async fn call_ended(&self, uuid: Uuid) { let mut i = self.inner.write().await; if let Some(mut r)=i.active.remove(&uuid) { r.ended_at_ms=Some(now_ms()); i.calls.push_front(r.clone()); while i.calls.len()>200 { i.calls.pop_back(); } drop(i); self.persist(&crate::store::StoredRecord::Call(r.clone())); self.emit("call_ended", serde_json::json!(r)); } }
    pub async fn voice_frame(&self, uuid: Uuid) { let mut i=self.inner.write().await; i.voice_frames+=1; if let Some(r)=i.active.get_mut(&uuid){r.voice_frames+=1;} }
    pub async fn sds(&self, uuid: Uuid, source: u32, destination: u32) { let r=SdsRecord{uuid,source,destination,at_ms:now_ms(),reports:0}; let mut i=self.inner.write().await; i.total_sds+=1; i.sds.push_front(r.clone()); while i.sds.len()>200{i.sds.pop_back();} drop(i); self.persist(&crate::store::StoredRecord::Sds(r.clone())); self.emit("sds",serde_json::json!(r)); }
    pub async fn sds_report(&self, uuid: Uuid) { let mut i=self.inner.write().await; if let Some(r)=i.sds.iter_mut().find(|r|r.uuid==uuid){r.reports+=1;} drop(i); self.persist(&crate::store::StoredRecord::SdsReport { uuid }); }
    /// Notes that call or SDS `uuid` came from network `from` and/or went to
    /// networks `to` (added to what is known of it already).
    pub async fn note_networks(&self, uuid: Uuid, from: Option<String>, to: Vec<String>) {
        if from.is_none() && to.is_empty() { return; }
        let mut i = self.inner.write().await;
        let tag = i.nets.entry(uuid).or_default();
        if tag.from.is_none() { tag.from = from; }
        for n in to { if !tag.to.contains(&n) { tag.to.push(n); } }
        tag.to.sort();
        i.prune_tags();
    }
    pub async fn snapshot(&self, clients: usize, subscribers: usize, groups: usize) -> Snapshot { let i=self.inner.read().await; Snapshot{connected_basestations:clients,subscribers,groups,active_calls:i.active.values().map(|c|i.tagged(c.uuid,c)).collect(),recent_calls:i.calls.iter().take(50).map(|c|i.tagged(c.uuid,c)).collect(),recent_sds:i.sds.iter().take(50).map(|s|i.tagged(s.uuid,s)).collect(),total_calls:i.total_calls,total_sds:i.total_sds,voice_frames:i.voice_frames} }
}

/// Folds one persisted record into the in-memory history (startup replay and
/// HA replication).
fn apply_record(inner: &mut Inner, rec: crate::store::StoredRecord) {
    match rec {
        crate::store::StoredRecord::Call(r) => {
            inner.total_calls += 1;
            inner.voice_frames += r.voice_frames;
            inner.calls.push_front(r);
            while inner.calls.len() > 200 { inner.calls.pop_back(); }
        }
        crate::store::StoredRecord::Sds(r) => {
            inner.total_sds += 1;
            inner.sds.push_front(r);
            while inner.sds.len() > 200 { inner.sds.pop_back(); }
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
}
