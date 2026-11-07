use crate::{config::Config, control::ControlState, monitor::Monitor, protocol::ConnVersion, telemetry::TelemetryState};
use std::{collections::{HashMap, HashSet}, sync::{atomic::{AtomicU64, Ordering}, Arc}, time::{Duration, Instant}};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

pub type ClientId = Uuid;

/// Brew client role advertised via the `X-Brew-Mode` HTTP header at discovery.
/// Per the specification a `Terminal` does not need registration updates pushed
/// from the server, whereas a `Basestation` does. Defaults to `Basestation`
/// (the conservative choice) when the header is absent.
///
/// `Peer` is a third role, not part of the original Brew spec: another
/// brew-server federated with this one (see `federation`), connected exactly
/// like a Basestation but exchanging subscriber/group registrations and
/// relaying calls/SDS between servers rather than representing a real radio
/// site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientMode {
    Terminal,
    #[default]
    Basestation,
    Peer,
}

impl ClientMode {
    pub fn from_header(value: Option<&str>) -> Self {
        match value.map(|v| v.trim()) {
            Some(v) if v.eq_ignore_ascii_case("Terminal") => ClientMode::Terminal,
            Some(v) if v.eq_ignore_ascii_case("Peer") => ClientMode::Peer,
            _ => ClientMode::Basestation,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ClientMode::Terminal => "Terminal",
            ClientMode::Basestation => "Basestation",
            ClientMode::Peer => "Peer",
        }
    }

    /// Whether the server should push subscriber-registration updates to this
    /// client. Terminals opt out to save resources, per the spec; a
    /// federation peer needs them for the same reason a Basestation does (it
    /// relays them onward to its own other peers).
    pub fn wants_registration_updates(self) -> bool {
        matches!(self, ClientMode::Basestation | ClientMode::Peer)
    }
}

#[derive(Clone)]
pub struct Client {
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
    pub mode: ClientMode,
    /// Negotiated Brew protocol version for this connection. Seeded from the
    /// discovery `X-Brew-Version` header (if any) and promoted lazily as v1
    /// message layouts are observed on the wire.
    pub version: ConnVersion,
    /// Whether the client announced `version` (an `X-Brew-Version` on its
    /// discovery GET or upgrade) rather than it being the v0 default; see
    /// `forward_version`.
    pub version_announced: bool,
    /// Remote address of the WebSocket connection, when known. `None` for the
    /// virtual clients the SIP bridge registers (see `sip::bridge`), which
    /// have no real socket.
    pub remote_addr: Option<std::net::SocketAddr>,
    /// When this connection was accepted, for the dashboard's live
    /// connections page.
    pub connected_at_ms: u64,
    /// The Brew digest username this connection authenticated as, when
    /// `[auth]` is enabled (`None` otherwise, or for the SIP bridge's virtual
    /// clients). Used to match a live connection to its `[bts_locations]`
    /// entry on the dashboard map.
    pub username: Option<String>,
}

impl Client {
    /// The layout CALL_GROUP_TX and CALL_SETUP_REQUEST are forwarded to this
    /// connection in (see `protocol::adapt_to_version`). Only a connection
    /// that announced v0 has the v1 mnemonic stripped; one that announced no
    /// version gets them as sent, as it always did -- FlowStation without
    /// digest credentials upgrades with no X-Brew headers and never sends a
    /// mnemonic itself, so it stays v0 here, yet parses and shows the talker
    /// name.
    pub fn forward_version(&self) -> ConnVersion {
        if self.version_announced { self.version } else { ConnVersion::V1 }
    }
}

#[derive(Debug, Clone)]
pub struct Subscriber {
    pub client_id: ClientId,
    pub groups: HashSet<u32>,
    /// The connection mode of the client that registered this ISSI (Terminal
    /// or Basestation). Only `Terminal` connections represent an actual mobile
    /// station; a `Basestation` (Basestation gateway) registering on a
    /// subscriber's behalf is not itself an MS. Dashboard MS-registration
    /// counts should filter on this.
    pub mode: ClientMode,
    /// Registration clock and server path of this entry (see `fedroute`):
    /// an empty path for an ISSI registered here, by a Basestation, Terminal
    /// or legacy peer link; otherwise the route learnt over a loop-safe peer
    /// link, whose first server is that link's.
    pub route: crate::fedroute::Route,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    Group,
    Private,
}

#[derive(Debug, Clone)]
pub struct ActiveCall {
    pub kind: CallKind,
    pub owner: ClientId,
    pub source_issi: u32,
    pub destination: u32,
    pub priority: u8,
    pub peers: HashSet<ClientId>,
    /// When this call was set up, for max-call-duration enforcement and for
    /// showing call age on the dashboard.
    pub started_at: Instant,
    /// Last time (ms, `telemetry::now_ms`) a voice/DTMF frame or GROUP_TX was
    /// seen for this call. Shared so the frame path can bump it under the
    /// read lock; used to end group calls whose GROUP_IDLE never arrived.
    pub last_activity_ms: Arc<AtomicU64>,
}

impl ActiveCall {
    pub fn new_activity() -> Arc<AtomicU64> { Arc::new(AtomicU64::new(crate::telemetry::now_ms())) }
    pub fn touch(&self) { self.last_activity_ms.store(crate::telemetry::now_ms(), Ordering::Relaxed); }
    pub fn idle_ms(&self) -> u64 { crate::telemetry::now_ms().saturating_sub(self.last_activity_ms.load(Ordering::Relaxed)) }
}

#[derive(Debug, Clone)]
pub struct SdsRoute {
    pub source_client: ClientId,
    pub targets: HashSet<ClientId>,
    pub source_issi: u32,
    pub destination: u32,
    pub created_at: Instant,
    /// Nobody could receive this SDS and the SMS Center wants to keep it: the
    /// following `SDS_TRANSFER` payload is stored for later delivery.
    pub store_offline: bool,
    /// An ISSI on the blacklist is a party: the SDS is passed on only if its
    /// payload turns out to be LIP (the header carries no protocol id), so the
    /// header is held until the `SDS_TRANSFER` arrives.
    pub lip_only: bool,
    pub held_header: Option<Vec<u8>>,
}

#[derive(Default)]
pub struct Inner {
    pub clients: HashMap<ClientId, Client>,
    pub subscribers: HashMap<u32, Subscriber>,
    pub group_clients: HashMap<u32, HashSet<ClientId>>,
    pub calls: HashMap<Uuid, ActiveCall>,
    pub group_floor: HashMap<u32, Uuid>,
    /// The GROUP_TX that set up each running group call (as it arrived), so a
    /// connection that joins the call's group while it runs can be handed the
    /// same header (late entry, see `router::late_entry`). Entries whose call
    /// is gone are pruned whenever a GROUP_TX or a late entry touches this.
    pub group_tx_raw: HashMap<Uuid, Vec<u8>>,
    pub sds_routes: HashMap<Uuid, SdsRoute>,
    pub digest_nonces: HashMap<String, Instant>,
    /// Session token -> what the discovery GET announced: mode, version (`None`
    /// when it sent no `X-Brew-Version`) and the digest username.
    pub auth_sessions: HashMap<String, (Instant, ClientMode, Option<ConnVersion>, Option<String>)>,
    /// With `[auth]` disabled, what a client's discovery GET announced
    /// (`X-Brew-Mode` / `X-Brew-Version`) and the `User-Agent` it sent, for its
    /// WebSocket upgrade from the same address: a Basestation announces both
    /// on discovery only, and without a session token there is nothing else to
    /// carry them over. One per address; only an upgrade with the same
    /// `User-Agent` takes it, so another client behind the same NAT does not.
    pub discovery_hints: HashMap<std::net::IpAddr, (Instant, String, ClientMode, Option<ConnVersion>)>,
    /// (call/SDS uuid, source ISSI) -> (link it was accepted from, when): drops
    /// a copy of a call or SDS that reaches this server again over another
    /// peer link (see `fedroute::is_duplicate`).
    pub recent_calls: HashMap<(Uuid, u32), (ClientId, Instant)>,
    /// Loop-safe federation: this server's id and its negotiated peer links.
    pub fed: crate::fedroute::FedState,
    /// Connections that identified as a Tetra Dispatch console (its
    /// `User-Agent`): they are pushed every emergency group call, affiliated
    /// to its group or not (see `router::handle_group_tx`).
    pub consoles: HashSet<ClientId>,
}

impl Inner {
    /// Number of registered subscribers that represent an actual mobile
    /// station, i.e. registered by a `Terminal`-mode client. A `Basestation`
    /// (Basestation gateway) can also hold a subscriber registration, but it
    /// is not itself an MS, so it is excluded from MS-registration counts.
    pub fn ms_registration_count(&self) -> usize {
        self.subscribers.values().filter(|s| s.mode == ClientMode::Terminal).count()
    }

    /// Number of connected clients that are actual Basestation (Basestation)
    /// gateways, i.e. `Basestation`-mode connections. A `Terminal`-mode
    /// connection is a mobile station registering directly over the Brew
    /// protocol, not a Basestation, so it is excluded here (it is counted
    /// instead by `ms_registration_count`).
    pub fn basestation_count(&self) -> usize {
        self.clients.values().filter(|c| c.mode == ClientMode::Basestation).count()
    }
}

#[cfg(test)]
mod blacklist_tests {
    use super::*;

    #[test]
    fn blacklist_starts_from_config_and_is_edited_live() {
        let mut c = Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        c.blacklist.issis = vec![4013, 7];
        let state = AppState::new(c, "test.toml".into()).0;
        assert!(state.is_blocked(4013) && state.is_blocked(7) && !state.is_blocked(8));
        assert!(state.set_blocked(8, true) && !state.set_blocked(8, true));
        assert!(state.set_blocked(7, false));
        assert_eq!(state.blocked_list(), vec![8, 4013]);
        // What a settings edit saves carries the live list, not the startup one.
        assert_eq!(state.config_snapshot().blacklist.issis, vec![8, 4013]);
        let text = state.config_snapshot().to_toml_pretty().unwrap();
        assert_eq!(Config::parse(&text).unwrap().blacklist.issis, vec![8, 4013]);
    }
}

#[cfg(test)]
mod ms_registration_tests {
    use super::*;

    fn subscriber(mode: ClientMode) -> Subscriber {
        Subscriber { client_id: Uuid::new_v4(), groups: HashSet::new(), mode, route: crate::fedroute::Route::default() }
    }

    #[test]
    fn counts_only_terminal_mode_subscribers() {
        let mut inner = Inner::default();
        inner.subscribers.insert(1001, subscriber(ClientMode::Terminal));
        inner.subscribers.insert(1002, subscriber(ClientMode::Terminal));
        inner.subscribers.insert(2001, subscriber(ClientMode::Basestation));
        assert_eq!(inner.ms_registration_count(), 2);
        assert_eq!(inner.subscribers.len(), 3, "raw map still holds every registration");
    }

    #[test]
    fn zero_when_only_basestations_registered() {
        let mut inner = Inner::default();
        inner.subscribers.insert(2001, subscriber(ClientMode::Basestation));
        inner.subscribers.insert(2002, subscriber(ClientMode::Basestation));
        assert_eq!(inner.ms_registration_count(), 0);
    }

    #[test]
    fn zero_when_no_subscribers() {
        assert_eq!(Inner::default().ms_registration_count(), 0);
    }
}

#[cfg(test)]
mod basestation_count_tests {
    use super::*;

    fn client(mode: ClientMode) -> Client {
        let (tx, _rx) = mpsc::unbounded_channel();
        Client { tx, mode, version: ConnVersion::default(), version_announced: false, remote_addr: None, connected_at_ms: 0, username: None }
    }

    #[test]
    fn counts_only_basestation_mode_clients() {
        let mut inner = Inner::default();
        inner.clients.insert(Uuid::new_v4(), client(ClientMode::Basestation));
        inner.clients.insert(Uuid::new_v4(), client(ClientMode::Basestation));
        inner.clients.insert(Uuid::new_v4(), client(ClientMode::Terminal));
        inner.clients.insert(Uuid::new_v4(), client(ClientMode::Terminal));
        // Reproduces the reported scenario: 2 Terminal MS + 2 Basestation
        // (Basestation) connections must show 2 Basestations, not 4.
        assert_eq!(inner.basestation_count(), 2);
        assert_eq!(inner.clients.len(), 4, "raw client map still holds every connection");
    }

    #[test]
    fn zero_when_only_terminals_connected() {
        let mut inner = Inner::default();
        inner.clients.insert(Uuid::new_v4(), client(ClientMode::Terminal));
        inner.clients.insert(Uuid::new_v4(), client(ClientMode::Terminal));
        assert_eq!(inner.basestation_count(), 0);
    }

    #[test]
    fn zero_when_no_clients() {
        assert_eq!(Inner::default().basestation_count(), 0);
    }
}

pub struct AppState {
    pub config: Config,
    /// Path of the config file this process was started with, so the
    /// dashboard's config editor can write changes back to the same file the
    /// startup `config_watcher` polls (which then restarts the process to
    /// apply them).
    pub config_path: std::path::PathBuf,
    pub inner: RwLock<Inner>,
    pub monitor: Monitor,
    pub telemetry: RwLock<TelemetryState>,
    pub control: RwLock<ControlState>,
    /// SIP subsystem runtime handles, populated when the SIP listener starts.
    /// `None` until then (and when SIP is disabled) so the dashboard can render
    /// an appropriate "disabled" state without panicking.
    pub sip: RwLock<Option<SipHandles>>,
    /// Feeds decoded MS positions to the APRS-IS forwarder (`aprs::run`),
    /// decoupled via a channel so a slow/down APRS-IS link never blocks call
    /// or SDS routing. Sends are safely dropped if `aprs::run` was never
    /// started or has exited.
    pub aprs_tx: mpsc::UnboundedSender<crate::aprs::PositionReport>,
    /// Store-and-forward queue for SDS to offline subscribers.
    pub sms_center: crate::sms_center::SmsCenter,
    /// Active/standby role and dashboard commands (see `ha`). Always reports
    /// Active when `[ha]` is disabled.
    pub ha: crate::ha::HaHandle,
    /// The history log, shared with `monitor` and `telemetry`; HA replication
    /// reads and extends it. `None` with `[storage]` disabled.
    pub store: Option<std::sync::Arc<crate::store::Store>>,
    /// The ISSI blacklist as enforced right now: starts as `[blacklist]` and is
    /// edited live from the dashboard, which also writes the config file
    /// without restarting (`config` itself is a startup snapshot).
    pub blocked: std::sync::RwLock<HashSet<u32>>,
    /// The ISSI allowlist as enforced right now (`[allowlist]`), edited live
    /// like `blocked`. `enabled` false: kept, not enforced.
    pub allowlist: std::sync::RwLock<Allowlist>,
}

/// The live `[allowlist]`: which ISSIs are admitted, and whether that is
/// enforced at all.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    pub enabled: bool,
    pub issis: HashSet<u32>,
}

impl Allowlist {
    /// Whether the allowlist, on its own, bars `issi`.
    pub fn refuses(&self, issi: u32) -> bool {
        self.enabled && !self.issis.contains(&issi)
    }
}

impl AppState {
    /// Whether `issi` is barred from communicating through this server, by the
    /// blacklist or by an enforced allowlist it is not on. Every routing check
    /// asks this one question; `bar_reason` says which list it was.
    pub fn is_blocked(&self, issi: u32) -> bool {
        self.is_blacklisted(issi) || self.allowlist_refuses(issi)
    }

    /// Whether `issi` is on the blacklist (regardless of the allowlist).
    pub fn is_blacklisted(&self, issi: u32) -> bool {
        self.blocked.read().is_ok_and(|b| b.contains(&issi))
    }

    /// Whether the allowlist is enforced and `issi` is not on it.
    pub fn allowlist_refuses(&self, issi: u32) -> bool {
        self.allowlist.read().is_ok_and(|a| a.refuses(issi))
    }

    /// Why `issi` is barred, for the log: "blacklisted" or "not on the
    /// allowlist"; `None` when it is not barred.
    pub fn bar_reason(&self, issi: u32) -> Option<&'static str> {
        if self.is_blacklisted(issi) {
            Some("blacklisted")
        } else if self.allowlist_refuses(issi) {
            Some("not on the allowlist")
        } else {
            None
        }
    }

    /// Sorted copy of the allowlist, and whether it is enforced.
    pub fn allowlist_snapshot(&self) -> (bool, Vec<u32>) {
        let (enabled, mut v) = self.allowlist.read()
            .map(|a| (a.enabled, a.issis.iter().copied().collect::<Vec<u32>>()))
            .unwrap_or_default();
        v.sort_unstable();
        (enabled, v)
    }

    /// Adds (`true`) or removes an ISSI from the allowlist; whether the set changed.
    pub fn set_allowed(&self, issi: u32, allowed: bool) -> bool {
        let Ok(mut a) = self.allowlist.write() else { return false };
        if allowed { a.issis.insert(issi) } else { a.issis.remove(&issi) }
    }

    /// Turns enforcement of the allowlist on or off; whether that changed anything.
    pub fn set_allowlist_enabled(&self, enabled: bool) -> bool {
        let Ok(mut a) = self.allowlist.write() else { return false };
        std::mem::replace(&mut a.enabled, enabled) != enabled
    }

    /// Sorted copy of the blacklist.
    pub fn blocked_list(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.blocked.read().map(|b| b.iter().copied().collect()).unwrap_or_default();
        v.sort_unstable();
        v
    }

    /// Adds (`true`) or removes an ISSI; whether the set changed.
    pub fn set_blocked(&self, issi: u32, blocked: bool) -> bool {
        let Ok(mut set) = self.blocked.write() else { return false };
        if blocked { set.insert(issi) } else { set.remove(&issi) }
    }

    /// The startup config with the live blacklist and allowlist applied: what
    /// is saved when a settings edit rewrites the file, so it never reverts
    /// edits made to either list.
    pub fn config_snapshot(&self) -> Config {
        let mut cfg = self.config.clone();
        cfg.blacklist.issis = self.blocked_list();
        let (enabled, issis) = self.allowlist_snapshot();
        cfg.allowlist.enabled = enabled;
        cfg.allowlist.issis = issis;
        cfg
    }
}

#[cfg(test)]
impl AppState {
    /// In-memory state for tests: no history store, no SMS Center file.
    pub fn for_test() -> Arc<Self> {
        let mut c = Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        Arc::new(Self::new(c, "test.toml".into()).0)
    }
}

/// Runtime handles for the SIP subsystem, shared with the dashboard.
#[derive(Clone)]
pub struct SipHandles {
    pub state: std::sync::Arc<crate::sip::SipState>,
    pub transport: std::sync::Arc<crate::sip::SipTransport>,
}

impl AppState {
    /// Returns the new state plus the receiving half of `aprs_tx`, which the
    /// caller must hand to `aprs::run` (the only consumer) exactly once.
    pub fn new(config: Config, config_path: std::path::PathBuf) -> (Self, mpsc::UnboundedReceiver<crate::aprs::PositionReport>) {
        let store = if config.storage.enabled {
            match crate::store::Store::open(&config.storage.path) {
                Ok(store) => Some(std::sync::Arc::new(store)),
                Err(e) => {
                    tracing::error!(error = %e, path = %config.storage.path.display(), "cannot open history store; continuing without persistence");
                    None
                }
            }
        } else {
            None
        };
        let monitor = match &store {
            Some(store) => Monitor::with_store(store.clone()),
            None => Monitor::new(),
        };
        let telemetry = match &store {
            Some(store) => TelemetryState::with_store(store.clone()),
            None => TelemetryState::default(),
        };
        let (aprs_tx, aprs_rx) = mpsc::unbounded_channel();
        let sms_center = crate::sms_center::SmsCenter::open(config.sms_center.clone());
        let blocked: HashSet<u32> = config.blacklist.issis.iter().copied().collect();
        let allowlist = Allowlist { enabled: config.allowlist.enabled, issis: config.allowlist.issis.iter().copied().collect() };
        let config_hash = crate::ha::config_hash(&config);
        let ha = crate::ha::HaHandle::new(&config.ha, config_hash, crate::ha::config_sections(&config, false));
        (
            Self {
                config,
                config_path,
                inner: RwLock::new(Inner::default()),
                monitor,
                telemetry: RwLock::new(telemetry),
                control: RwLock::new(ControlState::default()),
                sip: RwLock::new(None),
                aprs_tx,
                sms_center,
                ha,
                store,
                blocked: std::sync::RwLock::new(blocked),
                allowlist: std::sync::RwLock::new(allowlist),
            },
            aprs_rx,
        )
    }

    /// Where a service (Brew, telemetry, control, SIP) binds: with `[ha]`
    /// enabled, a wildcard address means the VIP, so only the Active node
    /// answers on it (and two nodes can share one host). Explicit addresses
    /// are kept as configured.
    pub fn service_bind(&self, addr: std::net::SocketAddr) -> std::net::SocketAddr {
        match self.config.ha.vip_parts() {
            Ok((vip, _)) if self.config.ha.enabled && addr.ip().is_unspecified() => {
                std::net::SocketAddr::new(vip.into(), addr.port())
            }
            _ => addr,
        }
    }

    /// Where the dashboard binds: with `[ha]` enabled, a wildcard address
    /// means this node's real IP, so both nodes' dashboards stay reachable
    /// whatever their role.
    pub fn dashboard_bind(&self, addr: std::net::SocketAddr) -> std::net::SocketAddr {
        if self.config.ha.enabled && addr.ip().is_unspecified() {
            std::net::SocketAddr::new(self.config.ha.real_ip, addr.port())
        } else {
            addr
        }
    }

    /// Registers the SIP runtime handles once the SIP listener has bound. Called
    /// from the SIP transport during startup.
    pub async fn set_sip(
        &self,
        state: std::sync::Arc<crate::sip::SipState>,
        transport: std::sync::Arc<crate::sip::SipTransport>,
    ) {
        *self.sip.write().await = Some(SipHandles { state, transport });
    }

    /// Returns a SIP snapshot for the dashboard, or None when SIP is inactive.
    pub async fn sip_snapshot(&self) -> Option<crate::sip::SipSnapshot> {
        let guard = self.sip.read().await;
        match guard.as_ref() {
            Some(h) => Some(h.state.snapshot().await),
            None => None,
        }
    }

    /// Returns the current negotiated version for a connection (defaulting to
    /// V0 for unknown clients).
    pub async fn client_version(&self, id: ClientId) -> ConnVersion {
        self.inner.read().await.clients.get(&id).map(|c| c.version).unwrap_or_default()
    }

    /// Promotes a connection's stored version to at least `observed`, returning
    /// true if this raised the version (so callers can log the transition once).
    pub async fn promote_client_version(&self, id: ClientId, observed: ConnVersion) -> bool {
        let mut inner = self.inner.write().await;
        if let Some(client) = inner.clients.get_mut(&id) {
            if observed.as_u8() > client.version.as_u8() {
                client.version = observed;
                return true;
            }
        }
        false
    }

    pub async fn send_many(&self, clients: &HashSet<ClientId>, packet: &[u8]) {
        let inner = self.inner.read().await;
        for id in clients {
            if let Some(client) = inner.clients.get(id) {
                let _ = client.tx.send(packet.to_vec());
            }
        }
    }

    pub async fn purge_ephemeral(&self) {
        let now = Instant::now();
        let session_ttl = Duration::from_secs(self.config.auth.session_ttl_seconds.max(1));
        let mut inner = self.inner.write().await;
        inner.digest_nonces.retain(|_, at| now.duration_since(*at) < Duration::from_secs(120));
        inner.auth_sessions.retain(|_, (at, _, _, _)| now.duration_since(*at) < session_ttl);
        inner.discovery_hints.retain(|_, (at, _, _, _)| now.duration_since(*at) < session_ttl);
        inner.sds_routes.retain(|_, route| now.duration_since(route.created_at) < Duration::from_secs(60));
        inner.recent_calls.retain(|_, (_, at)| now.duration_since(*at) < crate::fedroute::CALL_DEDUP_WINDOW);
        crate::fedroute::purge_bts(&mut inner);
    }

    pub async fn cleanup_client(&self, id: ClientId) {
        let mut inner = self.inner.write().await;
        inner.clients.remove(&id);
        let negotiated = inner.fed.links.remove(&id).is_some();
        inner.fed.bts_links.remove(&id);
        inner.consoles.remove(&id);
        inner.fed.ms_links.remove(&id);
        inner.fed.em_links.remove(&id);

        // Registrations: every ISSI routed over this connection, and for a
        // loop-safe link every one it offered, gets a new effective route --
        // another link's offer (failover), or none -- and every remaining
        // peer link hears about the change (a legacy one a SUB_DEREGISTER,
        // as before). The connection's own entries and group memberships go
        // in bulk first, so each ISSI is re-routed against the final state.
        let mut affected: Vec<u32> = inner.subscribers.iter()
            .filter_map(|(issi, sub)| (sub.client_id == id).then_some(*issi)).collect();
        if negotiated {
            affected.extend(inner.fed.rib_in.iter().filter_map(|(issi, offers)| offers.contains_key(&id).then_some(*issi)));
        }
        affected.sort_unstable();
        affected.dedup();
        let previous: Vec<(u32, Option<Subscriber>)> = affected.iter().map(|issi| (*issi, inner.subscribers.get(issi).cloned())).collect();
        for (issi, old) in &previous {
            if let Some(offers) = inner.fed.rib_in.get_mut(issi) {
                offers.remove(&id);
                if offers.is_empty() { inner.fed.rib_in.remove(issi); }
            }
            if old.as_ref().is_some_and(|o| o.client_id == id) { inner.subscribers.remove(issi); }
        }

        for clients in inner.group_clients.values_mut() { clients.remove(&id); }
        inner.group_clients.retain(|_, clients| !clients.is_empty());

        for (issi, old) in &previous {
            crate::fedroute::reroute(&mut inner, *issi);
            crate::fedroute::publish(&inner, *issi, old.as_ref());
        }

        // Calls this client took part in, ended the way router::end_call ends
        // them: a private call ends for both parties; a group call ends only
        // if its owner (the talker's side) is the one that vanished -- a
        // departing listener is just dropped from the recipients, so everyone
        // else keeps hearing it. The remaining participants get the
        // CALL_RELEASE / CALL_GROUP_IDLE a hangup would have relayed, so their
        // radios drop the call now instead of waiting for their own timers.
        // Removal happens under this write lock, so a call already ended
        // (end_call, the timeout sweep, the SIP bridge) is never ended twice.
        let involved: Vec<Uuid> = inner.calls.iter()
            .filter_map(|(uuid, call)| (call.owner == id || call.peers.contains(&id)).then_some(*uuid)).collect();
        let mut removed_calls = Vec::new();
        let mut end_notices: Vec<(mpsc::UnboundedSender<Vec<u8>>, Vec<u8>)> = Vec::new();
        for uuid in involved {
            let Some(call) = inner.calls.get_mut(&uuid) else { continue };
            if call.kind == CallKind::Group && call.owner != id {
                call.peers.remove(&id);
                continue;
            }
            let Some(call) = inner.calls.remove(&uuid) else { continue };
            if call.kind == CallKind::Group && inner.group_floor.get(&call.destination) == Some(&uuid) {
                inner.group_floor.remove(&call.destination);
            }
            let end_state = if call.kind == CallKind::Group { crate::protocol::CALL_GROUP_IDLE } else { crate::protocol::CALL_RELEASE };
            let msg = crate::protocol::build_call_cause(end_state, &uuid, crate::protocol::CAUSE_SWMI_REQUESTED_DISCONNECTION);
            let mut recipients = call.peers.clone();
            if call.kind == CallKind::Private { recipients.insert(call.owner); }
            recipients.remove(&id);
            end_notices.extend(recipients.iter().filter_map(|c| inner.clients.get(c)).map(|c| (c.tx.clone(), msg.clone())));
            removed_calls.push(uuid);
        }
        inner.sds_routes.retain(|_, route| route.source_client != id && !route.targets.contains(&id));
        // A call or SDS accepted over this link may now arrive over another
        // one (rerouted): that copy is no longer a duplicate.
        inner.recent_calls.retain(|_, (link, _)| *link != id);
        drop(inner);
        for (tx, msg) in end_notices { let _ = tx.send(msg); }

        // The calls ended above are gone: end them on the dashboard too, and
        // hang up any SIP leg bridged to one, otherwise both UIs keep showing
        // a call that no longer exists. A group call that only lost a
        // listener is still running and stays.
        let bridge = match self.sip.read().await.as_ref() {
            Some(h) => h.transport.bridge.read().await.clone(),
            None => None,
        };
        for uuid in removed_calls {
            self.monitor.call_ended(uuid).await;
            if let Some(bridge) = &bridge { bridge.teardown_by_brew_call(uuid).await; }
        }
    }
}
