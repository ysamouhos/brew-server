//! Runtime state for the SIP subsystem: current extension registrations, trunk
//! status, and active calls. This is the read model the dashboard renders and
//! the write model the transport/registrar/bridge mutate. It is intentionally
//! separate from `crate::state::AppState` (the Brew core) so the two subsystems
//! stay loosely coupled; the bridge module is the only place they meet.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::net::SocketAddr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// A live REGISTER binding for one SIP extension AOR.
#[derive(Debug, Clone, Serialize)]
pub struct Registration {
    /// AOR user part (the extension number/name).
    pub aor: String,
    /// Contact URI the extension can be reached at.
    pub contact: String,
    /// Socket the REGISTER arrived from (where we actually send requests).
    #[serde(serialize_with = "ser_addr")]
    pub source: SocketAddr,
    pub user_agent: String,
    /// Absolute expiry (epoch ms) after which this binding is stale.
    pub expires_at_ms: u64,
    pub registered_at_ms: u64,
    /// Whether the extension authenticated successfully.
    pub authenticated: bool,
}

fn ser_addr<S: serde::Serializer>(addr: &SocketAddr, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&addr.to_string())
}

fn ser_opt_addr<S: serde::Serializer>(addr: &Option<SocketAddr>, s: S) -> Result<S::Ok, S::Error> {
    match addr {
        Some(a) => s.serialize_str(&a.to_string()),
        None => s.serialize_none(),
    }
}

/// Health of a trunk (particularly an outbound registration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrunkStatus {
    /// Outbound: registered OK. Inbound/Peer: peer seen recently.
    Up,
    /// Outbound: registration attempted, not yet confirmed.
    Registering,
    /// Registration failed (bad auth, no response).
    Failed,
    /// Not attempted / disabled.
    Down,
}

/// Live status of a provisioned SIP trunk.
#[derive(Debug, Clone, Serialize)]
pub struct TrunkState {
    pub name: String,
    pub direction: String,
    pub remote_host: String,
    pub status: TrunkStatus,
    /// Last address we actually reached the peer at (learned for inbound).
    #[serde(serialize_with = "ser_opt_addr")]
    pub peer_addr: Option<SocketAddr>,
    pub last_event_ms: u64,
    /// Human-readable last status detail (e.g. "401 challenged", "200 OK").
    pub detail: String,
    pub active_calls: u32,
}

/// Which kind of endpoint a call leg terminates on. Mirrors the routing config
/// but resolved to concrete runtime identifiers.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LegEndpoint {
    SipExtension { aor: String },
    SipTrunk { trunk: String, number: String },
    BrewPrivate { issi: u32 },
    BrewGroup { gssi: u32 },
    /// An unresolved/external SIP party (raw URI), before routing.
    SipExternal { uri: String },
}

/// An active bridged call tracked for the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct SipCall {
    /// SIP Call-ID of the originating leg.
    pub call_id: String,
    pub from: LegEndpoint,
    pub to: LegEndpoint,
    pub started_at_ms: u64,
    /// Set when the call is answered (200 OK to INVITE).
    pub answered_at_ms: Option<u64>,
    pub state: String,
    /// RTP relay ports for each leg, for diagnostics.
    pub rtp_a_port: Option<u16>,
    pub rtp_b_port: Option<u16>,
}

/// Dashboard snapshot of the whole SIP subsystem.
#[derive(Debug, Clone, Serialize)]
pub struct SipSnapshot {
    pub enabled: bool,
    pub listen: String,
    pub realm: String,
    pub registrations: Vec<Registration>,
    pub trunks: Vec<TrunkState>,
    pub active_calls: Vec<SipCall>,
    pub total_calls: u64,
    pub total_registrations: u64,
}

#[derive(Default)]
struct Inner {
    registrations: HashMap<String, Registration>,
    trunks: HashMap<String, TrunkState>,
    calls: HashMap<String, SipCall>,
    /// Per-call last-RTP-received clocks of the call's relay legs (see
    /// `RtpLeg::activity`), for the call inactivity sweep.
    media: HashMap<String, Vec<Arc<AtomicU64>>>,
    total_calls: u64,
    total_registrations: u64,
}

/// Shared SIP runtime state.
pub struct SipState {
    inner: RwLock<Inner>,
    /// Monotonic clock base so we can compute registration TTLs consistently.
    _started: Instant,
    pub enabled: bool,
    pub listen: String,
    pub realm: String,
}

impl SipState {
    pub fn new(enabled: bool, listen: String, realm: String) -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
            _started: Instant::now(),
            enabled,
            listen,
            realm,
        }
    }

    /// Records/refreshes a registration binding. Returns true if this was a new
    /// AOR (first-time registration) rather than a refresh.
    pub async fn upsert_registration(&self, reg: Registration) -> bool {
        let mut i = self.inner.write().await;
        let is_new = !i.registrations.contains_key(&reg.aor);
        if is_new { i.total_registrations += 1; }
        i.registrations.insert(reg.aor.clone(), reg);
        is_new
    }

    /// Removes a registration (Expires: 0 / unregister).
    pub async fn remove_registration(&self, aor: &str) {
        self.inner.write().await.registrations.remove(aor);
    }

    /// Looks up the contact/source for an AOR if it has a live binding.
    pub async fn lookup_registration(&self, aor: &str) -> Option<Registration> {
        let i = self.inner.read().await;
        i.registrations.get(aor).cloned()
    }

    /// Drops expired registrations. Call periodically.
    pub async fn purge_expired(&self) {
        let now = now_ms();
        self.inner.write().await.registrations.retain(|_, r| r.expires_at_ms > now);
    }

    /// Initializes/updates a trunk's status entry.
    pub async fn set_trunk(&self, trunk: TrunkState) {
        self.inner.write().await.trunks.insert(trunk.name.clone(), trunk);
    }

    /// Updates just the status/detail of an existing trunk.
    pub async fn update_trunk_status(&self, name: &str, status: TrunkStatus, detail: impl Into<String>, peer_addr: Option<SocketAddr>) {
        let mut i = self.inner.write().await;
        if let Some(t) = i.trunks.get_mut(name) {
            t.status = status;
            t.detail = detail.into();
            t.last_event_ms = now_ms();
            if peer_addr.is_some() { t.peer_addr = peer_addr; }
        }
    }

    /// Returns the learned peer address for a named trunk, if any (used for
    /// Brew->SIP calls toward inbound trunks whose address we only know from
    /// their REGISTER).
    pub async fn trunk_for_peer_addr(&self, name: &str) -> Option<SocketAddr> {
        self.inner.read().await.trunks.get(name).and_then(|t| t.peer_addr)
    }

    /// Finds a trunk by the remote peer address (for inbound INVITE matching).
    pub async fn trunk_for_peer(&self, addr: &SocketAddr) -> Option<String> {
        let i = self.inner.read().await;
        i.trunks.iter()
            .find(|(_, t)| t.peer_addr.as_ref() == Some(addr))
            .map(|(name, _)| name.clone())
    }

    /// Registers a new active call and bumps the counter.
    pub async fn start_call(&self, call: SipCall) {
        let mut i = self.inner.write().await;
        i.total_calls += 1;
        // Reflect the call against each trunk leg's active count.
        for ep in [&call.from, &call.to] {
            if let LegEndpoint::SipTrunk { trunk, .. } = ep {
                if let Some(t) = i.trunks.get_mut(trunk) { t.active_calls += 1; }
            }
        }
        i.calls.insert(call.call_id.clone(), call);
    }

    /// Marks a call answered.
    pub async fn answer_call(&self, call_id: &str) {
        let mut i = self.inner.write().await;
        if let Some(c) = i.calls.get_mut(call_id) {
            c.answered_at_ms = Some(now_ms());
            c.state = "answered".into();
        }
    }

    pub async fn set_call_rtp(&self, call_id: &str, a: Option<u16>, b: Option<u16>) {
        let mut i = self.inner.write().await;
        if let Some(c) = i.calls.get_mut(call_id) {
            if a.is_some() { c.rtp_a_port = a; }
            if b.is_some() { c.rtp_b_port = b; }
        }
    }

    /// Registers an RTP leg's activity clock against a call.
    pub async fn track_media(&self, call_id: &str, activity: Arc<AtomicU64>) {
        self.inner.write().await.media.entry(call_id.to_string()).or_default().push(activity);
    }

    /// Answered calls that have received no RTP on any leg for `timeout_ms`
    /// (counted from the answer when no RTP ever arrived). Unanswered calls
    /// are left alone: ringing carries no media.
    pub async fn idle_calls(&self, timeout_ms: u64) -> Vec<String> {
        let now = now_ms();
        let i = self.inner.read().await;
        i.calls.values().filter_map(|c| {
            let answered = c.answered_at_ms?;
            let last_rx = i.media.get(&c.call_id).into_iter().flatten()
                .map(|a| a.load(Ordering::Relaxed)).max().unwrap_or(0);
            (now.saturating_sub(answered.max(last_rx)) >= timeout_ms).then(|| c.call_id.clone())
        }).collect()
    }

    /// Ends a call, decrementing trunk active counts.
    pub async fn end_call(&self, call_id: &str) {
        let mut i = self.inner.write().await;
        i.media.remove(call_id);
        if let Some(c) = i.calls.remove(call_id) {
            for ep in [&c.from, &c.to] {
                if let LegEndpoint::SipTrunk { trunk, .. } = ep {
                    if let Some(t) = i.trunks.get_mut(trunk) {
                        t.active_calls = t.active_calls.saturating_sub(1);
                    }
                }
            }
        }
    }

    pub async fn snapshot(&self) -> SipSnapshot {
        let i = self.inner.read().await;
        let mut registrations: Vec<_> = i.registrations.values().cloned().collect();
        registrations.sort_by(|a, b| a.aor.cmp(&b.aor));
        let mut trunks: Vec<_> = i.trunks.values().cloned().collect();
        trunks.sort_by(|a, b| a.name.cmp(&b.name));
        let mut active_calls: Vec<_> = i.calls.values().cloned().collect();
        active_calls.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
        SipSnapshot {
            enabled: self.enabled,
            listen: self.listen.clone(),
            realm: self.realm.clone(),
            registrations,
            trunks,
            active_calls,
            total_calls: i.total_calls,
            total_registrations: i.total_registrations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(aor: &str, ttl_ms: u64) -> Registration {
        Registration {
            aor: aor.into(),
            contact: format!("sip:{aor}@10.0.0.5:5060"),
            source: "10.0.0.5:5060".parse().unwrap(),
            user_agent: "test".into(),
            expires_at_ms: now_ms() + ttl_ms,
            registered_at_ms: now_ms(),
            authenticated: true,
        }
    }

    #[tokio::test]
    async fn registration_lifecycle() {
        let s = SipState::new(true, "0.0.0.0:5060".into(), "brew".into());
        assert!(s.upsert_registration(reg("1001", 60_000)).await);
        assert!(!s.upsert_registration(reg("1001", 60_000)).await, "refresh is not new");
        assert!(s.lookup_registration("1001").await.is_some());
        let snap = s.snapshot().await;
        assert_eq!(snap.registrations.len(), 1);
        assert_eq!(snap.total_registrations, 1);
        s.remove_registration("1001").await;
        assert!(s.lookup_registration("1001").await.is_none());
    }

    #[tokio::test]
    async fn purges_expired_registrations() {
        let s = SipState::new(true, "0.0.0.0:5060".into(), "brew".into());
        // Already-expired binding.
        let mut r = reg("2002", 0);
        r.expires_at_ms = now_ms().saturating_sub(1000);
        s.upsert_registration(r).await;
        s.purge_expired().await;
        assert!(s.lookup_registration("2002").await.is_none());
    }

    #[tokio::test]
    async fn call_counts_reflect_on_trunk() {
        let s = SipState::new(true, "0.0.0.0:5060".into(), "brew".into());
        s.set_trunk(TrunkState {
            name: "asterisk".into(), direction: "outbound".into(), remote_host: "pbx:5060".into(),
            status: TrunkStatus::Up, peer_addr: None, last_event_ms: now_ms(), detail: "ok".into(), active_calls: 0,
        }).await;
        s.start_call(SipCall {
            call_id: "c1".into(),
            from: LegEndpoint::SipExtension { aor: "1001".into() },
            to: LegEndpoint::SipTrunk { trunk: "asterisk".into(), number: "5551234".into() },
            started_at_ms: now_ms(), answered_at_ms: None, state: "ringing".into(),
            rtp_a_port: None, rtp_b_port: None,
        }).await;
        let snap = s.snapshot().await;
        assert_eq!(snap.trunks[0].active_calls, 1);
        assert_eq!(snap.active_calls.len(), 1);
        s.end_call("c1").await;
        let snap = s.snapshot().await;
        assert_eq!(snap.trunks[0].active_calls, 0);
        assert_eq!(snap.active_calls.len(), 0);
        assert_eq!(snap.total_calls, 1, "counter retained after end");
    }
}
