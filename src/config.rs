use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, net::{IpAddr, Ipv4Addr, SocketAddr}, path::Path, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub listen: SocketAddr,
    pub websocket_path: String,
    pub websocket_subprotocol: String,
    pub route_without_affiliations: bool,
    pub fallback_broadcast_when_no_affiliations: bool,
    pub allow_multiple_calls_per_group: bool,
    pub higher_priority_number_wins: bool,
    pub preempt_cause: u8,
    /// Maximum duration (seconds) a Brew call (private or group -- a station
    /// call between BlueStations/mobiles, or the Brew leg of a SIP-bridged
    /// call) may run before the server force-ends it with a normal
    /// CALL_RELEASE/CALL_GROUP_IDLE, the same as if a participant had hung
    /// up. 0 disables the limit.
    pub max_call_duration_seconds: u64,
    /// Seconds a call may go without any media before the server ends it as
    /// dead, shared by Brew calls (no voice/DTMF frames or call control) and
    /// SIP calls (no RTP received once answered). Catches calls whose
    /// GROUP_IDLE/RELEASE/BYE was lost so they don't linger on the
    /// dashboards. 0 disables it.
    pub call_inactivity_timeout_seconds: u64,
    pub auth: AuthConfig,
    pub tls: TlsConfig,
    pub telemetry: TelemetryConfig,
    pub control: ControlConfig,
    pub dashboard: DashboardConfig,
    pub storage: StorageConfig,
    pub sip: SipConfig,
    pub federation: FederationConfig,
    pub aprs: AprsConfig,
    /// Store-and-forward for individual SDS addressed to a subscriber that is
    /// not currently registered on any connected Basestation/peer. See
    /// `sms_center`.
    pub sms_center: SmsCenterConfig,
    /// Fixed geographic locations for Basestations, keyed by the same numeric
    /// Brew username each one authenticates with under `[auth.users]` -- so a
    /// location entry automatically matches whichever connection actually
    /// logs in as that Basestation, no separate ID scheme needed. Purely
    /// informational (dashboard map markers); has no effect on routing.
    pub bts_locations: HashMap<String, BtsLocationConfig>,
    /// ISSIs barred from communicating through this server. See `BlacklistConfig`.
    pub blacklist: BlacklistConfig,
    /// The ISSIs admitted to this network; everything else is barred. See
    /// `AllowlistConfig`.
    pub allowlist: AllowlistConfig,
    /// Active/standby pair: two brew-servers on one LAN sharing a virtual IP.
    /// See `ha`.
    pub ha: HaConfig,
    /// Behaviour when a subscriber moves between Basestations. See `RoamingConfig`.
    pub roaming: RoamingConfig,
    /// Announced cell changes between Basestations. See `HandoverConfig`.
    pub handover: HandoverConfig,
    /// Talkgroup pools: groups whose calls are played on the other groups of
    /// the pool as well. Edited live from the dashboard (no restart). See
    /// `pools`.
    pub talkgroup_pools: Vec<TalkgroupPoolConfig>,
    /// `[issi_names]`: ISSI -> name ("2320972" = "OE4SXM Sebastian"), shown
    /// next to the ISSI on the dashboard. Edited live on its Directory page.
    pub issi_names: std::collections::BTreeMap<String, String>,
    /// `[gssi_names]`: GSSI -> name ("2321" = "District North"), shown next to the
    /// group on the dashboard. Edited live on the same directory page.
    pub gssi_names: std::collections::BTreeMap<String, String>,
    /// `[group_access."GSSI"]`: which radios may use a group. No entry (or an
    /// empty list) = open to everyone. See `group_access`.
    pub group_access: std::collections::BTreeMap<String, GroupAccessConfig>,
    /// `[multi_network]`: other Brew networks this server connects to itself
    /// and bridges talkgroups with (see `netlink`). Edited live from the
    /// dashboard (no restart).
    #[serde(skip_serializing_if = "MultiNetworkConfig::is_unset")]
    pub multi_network: MultiNetworkConfig,
}

/// Who may use one group: `mode = "allow"` lets only the listed ISSIs
/// affiliate to it and talk on it, `mode = "block"` keeps the listed ISSIs
/// out. An empty list leaves the group open. `rx_only` makes radios listeners
/// only: all of them when `rx_only_issis` is empty, else the listed ones.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct GroupAccessConfig {
    pub mode: GroupAccessMode,
    pub issis: Vec<u32>,
    pub rx_only: bool,
    pub rx_only_issis: Vec<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GroupAccessMode {
    #[default]
    Allow,
    Block,
}

impl GroupAccessConfig {
    /// Nothing limited: no entry needed.
    pub fn is_open(&self) -> bool {
        self.issis.is_empty() && !self.rx_only
    }

    /// May `issi` talk on the group (admitted and not a listener only)?
    pub fn may_talk(&self, issi: u32) -> bool {
        self.permits(issi) && !(self.rx_only && (self.rx_only_issis.is_empty() || self.rx_only_issis.contains(&issi)))
    }

    /// May `issi` affiliate to (and listen on) the group?
    pub fn permits(&self, issi: u32) -> bool {
        if self.issis.is_empty() {
            return true;
        }
        let listed = self.issis.contains(&issi);
        match self.mode {
            GroupAccessMode::Allow => listed,
            GroupAccessMode::Block => !listed,
        }
    }
}

/// Checks `[group_access]`: keys are GSSIs and the lists hold ISSIs (both 1 to
/// 16777215).
pub fn validate_group_access(g: &std::collections::BTreeMap<String, GroupAccessConfig>) -> Result<()> {
    for (k, a) in g {
        let ok = k.trim().parse::<u32>().is_ok_and(|n| (1..=0xFF_FFFF).contains(&n));
        anyhow::ensure!(ok, "group_access: '{k}' is not a GSSI (1 to 16777215)");
        if let Some(i) = a.issis.iter().chain(&a.rx_only_issis).find(|i| !(1..=0xFF_FFFF).contains(*i)) {
            anyhow::bail!("group_access.{k}: {i} is not an ISSI (1 to 16777215)");
        }
    }
    Ok(())
}

/// `[[talkgroup_pools]]`: a named set of groups that are switched together
/// inside this server. A call on one group of the pool is played 1:1 on the
/// other groups, each as an ordinary group call on its own GSSI, so the radios
/// see nothing unusual. Only this server's own connections get the copies
/// (federation peers do not).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TalkgroupPoolConfig {
    /// Name shown on the dashboard; unique among the pools.
    pub name: String,
    /// Switched on. An off pool is kept but does nothing.
    pub enabled: bool,
    /// `true`: a call on any group of the pool reaches all the others.
    /// `false` (one way): only calls on the first group are copied, to the
    /// rest.
    pub both_ways: bool,
    /// The groups (GSSI). With `both_ways = false` the first one is the source.
    pub groups: Vec<u32>,
    /// Groups of other networks (`[[multi_network.networks]]`) in the pool: a
    /// bridge across networks. They come after `groups`; with
    /// `both_ways = false` and no `groups`, the first of them is the source.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remote: Vec<RemotePoolGroup>,
    /// A linked talkgroup: one group of this server bridged both ways to one
    /// group of another network. Works like any pool; the dashboard lists it
    /// under "Linked talkgroups" with its own editor.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub linked: bool,
}

impl Default for TalkgroupPoolConfig {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, both_ways: true, groups: Vec::new(), remote: Vec::new(), linked: false }
    }
}

/// One group of another network in a talkgroup pool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RemotePoolGroup {
    /// `name` of the `[[multi_network.networks]]` entry.
    pub network: String,
    /// The group's GSSI in that network.
    pub gssi: u32,
}

/// `[multi_network]`: this server's own network and the other Brew networks it
/// connects to. Each other network is reached over one outbound connection in
/// Basestation mode, so the far server needs nothing new: to it this server is
/// one more Basestation. Credentials stay here; nothing is passed on to the
/// stations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MultiNetworkConfig {
    /// Main switch: `false` stops every link to other networks (their
    /// settings and the pools stay). Default on.
    pub enabled: bool,
    /// Name of this server's own network ("this server"), for the dashboard.
    pub name: String,
    /// MCC/MNC of this server's own network (the MNI of the stations that
    /// connect here). 0 = not set.
    pub mcc: u16,
    pub mnc: u16,
    pub networks: Vec<NetworkConfig>,
}

impl Default for MultiNetworkConfig {
    fn default() -> Self {
        Self { enabled: true, name: String::new(), mcc: 0, mnc: 0, networks: Vec::new() }
    }
}

impl MultiNetworkConfig {
    /// Nothing configured: left out of a written config file.
    pub fn is_unset(&self) -> bool {
        *self == Self::default()
    }
}

/// One other Brew network (`[[multi_network.networks]]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NetworkConfig {
    /// Unique name; talkgroup pools refer to the network by it.
    pub name: String,
    /// Connect to it. An off network keeps its settings but has no link.
    pub enabled: bool,
    /// MCC/MNC of that network, for the dashboard. 0 = not set.
    pub mcc: u16,
    pub mnc: u16,
    /// `host:port` of its Brew listener.
    pub remote_host: String,
    /// Discovery path there (default `/brew`).
    pub path: String,
    /// Brew login this server uses there (the Basestation login that network
    /// gave out), and its password.
    pub username: String,
    pub password: String,
    /// Name this server introduces itself with there (`User-Agent`), free
    /// text, e.g. "TMO-Austria Infra 1.0". Empty: `brew-server/<version>`.
    pub user_agent: String,
    /// ISSI of the bridge in that network: registered there and affiliated to
    /// the network's groups used in talkgroup pools, so its server sends
    /// their calls here. 0: nothing is registered (only for a server that
    /// routes groups without affiliations).
    pub bridge_issi: u32,
    /// Register radios there, the way a Basestation registers its radios: a
    /// radio affiliated here to a group of a switched-on bridge pool is
    /// registered there and affiliated to the pool's groups of that
    /// network, and leaves them (deregistered) when it leaves here. A
    /// talker not known that way is registered before its first over.
    /// Off: talkers are passed through unregistered.
    pub register_talkers: bool,
    /// "Register all": also register there every radio of a station with
    /// the `mni` capability or named in `export_stations`, in a bridged
    /// group or not (needs `register_talkers`). The app and radios from
    /// peers or other networks are never exported.
    pub register_all: bool,
    /// Station logins whose radios "register all" exports as well; an app
    /// (Terminal) named by its login or its ISSI is exported too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub export_stations: Vec<String>,
    /// ISSIs never registered there (an ISSI that network has itself).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub never_export: Vec<u32>,
    /// "Forward unknown targets": an individual call or SDS from a station
    /// that nobody here takes goes once to this network (loop guard 5 s).
    pub forward_unknown: bool,
    pub reconnect_interval_seconds: u64,
    /// TLS settings, as for `[[federation.peers]]`.
    pub tls: bool,
    pub tls_ca_path: PathBuf,
    pub tls_server_name: String,
    pub tls_pinned_cert_path: PathBuf,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            mcc: 0,
            mnc: 0,
            remote_host: String::new(),
            path: "/brew".into(),
            username: String::new(),
            password: String::new(),
            user_agent: String::new(),
            bridge_issi: 0,
            register_talkers: true,
            register_all: false,
            export_stations: Vec::new(),
            never_export: Vec::new(),
            forward_unknown: false,
            reconnect_interval_seconds: 15,
            tls: false,
            tls_ca_path: "/etc/ssl/certs/ca-certificates.crt".into(),
            tls_server_name: String::new(),
            tls_pinned_cert_path: PathBuf::new(),
        }
    }
}

impl NetworkConfig {
    /// The dial settings in the shape the federation dialer takes.
    pub fn as_peer(&self) -> FederationPeerConfig {
        FederationPeerConfig {
            name: self.name.clone(),
            remote_host: self.remote_host.clone(),
            path: self.path.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            reconnect_interval_seconds: self.reconnect_interval_seconds,
            enabled: self.enabled,
            tls: self.tls,
            tls_ca_path: self.tls_ca_path.clone(),
            tls_server_name: self.tls_server_name.clone(),
            tls_pinned_cert_path: self.tls_pinned_cert_path.clone(),
        }
    }
}

/// `[roaming]`: what happens when a mobile station registers on one
/// Basestation while another one still holds it. A radio does not deregister
/// from the cell it leaves, so without this the old Basestation keeps listing
/// the ISSI as registered until its own timeout (if any).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoamingConfig {
    /// When set (`1`/`true`), a registration reported over telemetry by one
    /// Basestation makes the server kick the same ISSI (control channel
    /// `KickMs`) on every other Basestation that still lists it. The old
    /// entry is logged as deregistered "(roamed to other BS)". `0`/`false`
    /// (default) leaves the old registration alone.
    #[serde(deserialize_with = "bool_or_int")]
    pub kick_old_issi: bool,
    /// Hold-off before the kick is sent. If the radio registers back on the
    /// old Basestation within this time (ping-pong at a cell border), the
    /// kick is skipped, so it can never hit the radio's fresh registration.
    pub kick_delay_ms: u64,
}

impl Default for RoamingConfig {
    fn default() -> Self { Self { kick_old_issi: false, kick_delay_ms: 2000 } }
}

/// `[handover]`: relaying an announced cell change (U-PREPARE) from the
/// serving Basestation to the target one (see `handover`). Only Basestations
/// that announced the `handover-v1` capability take part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HandoverConfig {
    /// How long the target Basestation has to answer before the serving one
    /// is told "timeout". Must stay below FlowStation's own 3 s fallback for
    /// D-NEW-CELL, so the serving cell hears the answer before giving up.
    pub timeout_ms: u64,
}

impl Default for HandoverConfig {
    fn default() -> Self { Self { timeout_ms: 2000 } }
}

/// Accepts `true`/`false` as well as `1`/`0` for a boolean switch.
fn bool_or_int<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrInt { Bool(bool), Int(i64) }
    match BoolOrInt::deserialize(d)? {
        BoolOrInt::Bool(b) => Ok(b),
        BoolOrInt::Int(0) => Ok(false),
        BoolOrInt::Int(1) => Ok(true),
        BoolOrInt::Int(n) => Err(serde::de::Error::custom(format!("expected 0 or 1, got {n}"))),
    }
}

/// Active/standby high availability. Two nodes exchange signed UDP
/// heartbeats over their real IPs; the Active one holds `vip` on
/// `vip_interface` (moved with `ip addr` + gratuitous ARP, so both nodes must
/// share a Layer 2 segment). Runtime toggles made from the dashboard (persist
/// override) live in `state_path`, not here, so they never restart the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HaConfig {
    pub enabled: bool,
    /// This node's name; must differ from the peer's. Breaks weight ties.
    pub node_name: String,
    /// Higher weight is the preferred Active node.
    pub weight: u32,
    /// Default for "once Active, stay Active even when a higher-weight peer
    /// comes back". The dashboard can override it (stored in `state_path`).
    pub persist: bool,
    /// Virtual IPv4 address with prefix length, e.g. "192.0.2.100/24".
    pub vip: String,
    pub vip_interface: String,
    /// This node's own address: heartbeat bind address.
    pub real_ip: IpAddr,
    /// The other node's real address.
    pub peer_ip: IpAddr,
    /// UDP heartbeat port, the same on both nodes.
    pub heartbeat_port: u16,
    pub heartbeat_interval_ms: u64,
    /// The peer counts as dead after this long without a valid heartbeat.
    pub dead_after_ms: u64,
    /// HMAC-SHA256 key for heartbeats; identical on both nodes.
    pub shared_secret: String,
    /// Optional IP that must answer a ping before this node may take the VIP
    /// (guards against a node that lost its uplink grabbing the VIP). Empty
    /// disables the check.
    pub check_gateway: String,
    /// JSON file holding the runtime toggles set from the dashboard.
    pub state_path: PathBuf,
}

impl Default for HaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            node_name: String::new(),
            weight: 100,
            persist: false,
            vip: String::new(),
            vip_interface: "eth0".into(),
            real_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            peer_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            heartbeat_port: 9010,
            heartbeat_interval_ms: 500,
            dead_after_ms: 2000,
            shared_secret: String::new(),
            check_gateway: String::new(),
            state_path: PathBuf::from("ha-state.json"),
        }
    }
}

impl HaConfig {
    /// Parses `vip` ("a.b.c.d/nn") into the address and prefix length.
    pub fn vip_parts(&self) -> Result<(Ipv4Addr, u8)> {
        let (addr, prefix) = self.vip.split_once('/')
            .context("ha.vip must be an IPv4 address with prefix, e.g. 192.0.2.100/24")?;
        let addr: Ipv4Addr = addr.trim().parse().context("ha.vip: invalid IPv4 address")?;
        let prefix: u8 = prefix.trim().parse().context("ha.vip: invalid prefix length")?;
        anyhow::ensure!((1..=32).contains(&prefix), "ha.vip: prefix length must be 1..=32");
        Ok((addr, prefix))
    }

    /// Checks an enabled `[ha]` section for mistakes that would otherwise
    /// only show up at failover time.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        anyhow::ensure!(!self.node_name.trim().is_empty(), "ha.node_name must be set");
        anyhow::ensure!(!self.vip_interface.trim().is_empty(), "ha.vip_interface must be set");
        anyhow::ensure!(!self.shared_secret.is_empty(), "ha.shared_secret must be set");
        let (vip, prefix) = self.vip_parts()?;
        let IpAddr::V4(real) = self.real_ip else { anyhow::bail!("ha.real_ip must be IPv4") };
        let IpAddr::V4(peer) = self.peer_ip else { anyhow::bail!("ha.peer_ip must be IPv4") };
        anyhow::ensure!(!real.is_unspecified(), "ha.real_ip must be set");
        anyhow::ensure!(!peer.is_unspecified(), "ha.peer_ip must be set");
        anyhow::ensure!(real != peer, "ha.peer_ip must differ from ha.real_ip");
        anyhow::ensure!(vip != real && vip != peer, "ha.vip must differ from both real IPs");
        let mask = u32::MAX << (32 - prefix as u32);
        anyhow::ensure!(
            u32::from(vip) & mask == u32::from(real) & mask,
            "ha.vip {} is not in the same /{prefix} subnet as ha.real_ip {real}", self.vip
        );
        anyhow::ensure!(self.heartbeat_port != 0, "ha.heartbeat_port must be set");
        anyhow::ensure!(self.heartbeat_interval_ms >= 100, "ha.heartbeat_interval_ms must be at least 100");
        anyhow::ensure!(
            self.dead_after_ms >= 3 * self.heartbeat_interval_ms,
            "ha.dead_after_ms must be at least 3 x ha.heartbeat_interval_ms"
        );
        if !self.check_gateway.is_empty() {
            self.check_gateway.parse::<IpAddr>().context("ha.check_gateway: invalid IP address")?;
        }
        Ok(())
    }
}

/// ISSIs barred from communicating through this server. A listed ISSI may still
/// register (so dispatch can reach it) but cannot transmit to a group, make or
/// receive private calls, or send or receive SDS -- except that a dispatch
/// ambience-listening (SS-AL) call to it, and LIP position traffic (SDS protocol
/// 0x0A / 0x83) in either direction, still work. It cannot be silenced as a
/// listener of a group call: group audio is delivered per Basestation, not per
/// radio.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BlacklistConfig {
    pub issis: Vec<u32>,
    /// Brew usernames of Tetra Dispatch consoles allowed to edit the blacklist
    /// over their Brew link. Empty (the default): consoles can see the list
    /// but not change it.
    pub console_users: Vec<String>,
}

/// `[allowlist]`: the closed-network counterpart of the blacklist. With
/// `enabled = true` only the ISSIs listed may communicate through this server;
/// any other ISSI is barred exactly as a blacklisted one is (no group
/// transmissions, no private calls, no SDS; SS-AL, LIP positions and emergency
/// calls still pass, and it may still register so dispatch can see and reach
/// it). The blacklist still applies on top: a listed ISSI that is also
/// blacklisted stays barred. With `enabled = false` (the default) the list is
/// kept but not enforced, so a network can be populated before it is closed.
///
/// An empty list with `enabled = true` bars every radio; the server says so at
/// startup rather than guessing that an open network was meant.
///
/// Enforced here, at the server, whichever Basestation presents the radio:
/// a Basestation's own registration checks stay local to that cell, this is the
/// one list for the whole network. Edited live from the dashboard like the
/// blacklist.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowlistConfig {
    pub enabled: bool,
    pub issis: Vec<u32>,
    /// With `enabled`, also kick a radio that is not on the list off the
    /// Basestation it registers on (`KickMs` over the control channel, as the
    /// roaming kick does), after `kick_delay_ms`. Needs `[telemetry]` and
    /// `[control]`. Off by default: a barred radio then stays registered but
    /// silent, where dispatch can still see it.
    pub kick_unknown: bool,
    pub kick_delay_ms: u64,
}

/// One Basestation's fixed location, for the dashboard MS map. Keyed by Brew
/// username (see `Config::bts_locations`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BtsLocationConfig {
    /// Operator-facing label shown on the map marker/popup.
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

impl Default for BtsLocationConfig {
    fn default() -> Self {
        Self { name: String::new(), lat: 0.0, lon: 0.0 }
    }
}

/// SMS Center: store-and-forward of individual SDS for offline subscribers.
///
/// When an SDS arrives for an ISSI that is not registered anywhere on the
/// Brew network, the message is written to a JSON file instead of being
/// dropped, and delivered as soon as that ISSI registers again (on any
/// Basestation or federation peer).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SmsCenterConfig {
    pub enabled: bool,
    /// JSON file holding the queue and the set of known ISSIs.
    pub path: PathBuf,
    /// How long an undelivered message is kept before it is discarded.
    pub message_ttl_seconds: u64,
    /// Only queue SDS for ISSIs that have registered on this server at least
    /// once. Prevents queuing for gateway/app ISSIs that never register (e.g.
    /// LIP beacons to a dispatcher address) and for GSSIs with no members.
    pub only_known_destinations: bool,
    /// SDS protocol identifiers (first payload byte) that are never stored.
    /// Default: 10 (0x0A, LIP location reports -- stale positions are useless).
    pub exclude_protocol_ids: Vec<u8>,
    /// Per-destination queue limit.
    pub max_messages_per_destination: usize,
    /// Global queue limit.
    pub max_messages_total: usize,
    /// Send the originator an SDS-TL report (`stored_report_status`) when a
    /// message is queued, and "validity period
    /// expired" (0x48) / "delivery failed" (0x4A) if it is later discarded.
    /// Only for SDS-TL messages whose sender asked for a delivery report.
    pub send_status_reports: bool,
    /// SDS-TL delivery status reported to the sender when its message is
    /// queued. Default 0x00 ("receipt acknowledged by destination"): terminals
    /// show any non-zero status -- including the ETSI-correct 0x22
    /// "destination not reachable, message stored" -- as "failed". Set 34
    /// (0x22) for radios that render temporary-error statuses properly.
    pub stored_report_status: u8,
    /// Wait this long after a subscriber registers before delivering its
    /// queued messages, so the Basestation finishes registration signalling.
    pub delivery_delay_ms: u64,
    /// Gap between consecutive queued messages delivered to the same ISSI.
    pub delivery_spacing_ms: u64,
    /// If the destination Basestation does not answer a delivery with an
    /// SDS_REPORT within this time, the attempt is considered failed.
    pub ack_timeout_seconds: u64,
    /// Minimum time between retries while the destination stays registered.
    pub retry_interval_seconds: u64,
    /// Delivery attempts before a message is dropped (0 = unlimited, until TTL).
    pub max_attempts: u32,
}

impl Default for SmsCenterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: PathBuf::from("sms-center.json"),
            message_ttl_seconds: 7 * 24 * 3600,
            only_known_destinations: true,
            exclude_protocol_ids: vec![0x0A],
            max_messages_per_destination: 50,
            max_messages_total: 5000,
            send_status_reports: true,
            stored_report_status: 0x00,
            delivery_delay_ms: 2000,
            delivery_spacing_ms: 1500,
            ack_timeout_seconds: 30,
            retry_interval_seconds: 60,
            max_attempts: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// When enabled, completed calls and SDS are appended to a binary log and
    /// replayed on startup so history survives restarts.
    pub enabled: bool,
    /// Path to the append-only binary history log.
    pub path: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: PathBuf::from("brew-history.bin"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsConfig {
    pub enabled: bool,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: PathBuf::from("cert.pem"),
            key_path: PathBuf::from("key.pem"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub enabled: bool,
    pub realm: String,
    pub users: HashMap<String, String>,
    pub session_ttl_seconds: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            realm: "brew-server".into(),
            users: HashMap::new(),
            session_ttl_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub listen: SocketAddr,
    /// HTTP Basic Auth username -> password. Empty means no auth required.
    pub users: HashMap<String, String>,
    pub tls: TlsConfig,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "0.0.0.0:9001".parse().unwrap(),
            users: HashMap::new(),
            tls: TlsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlConfig {
    pub enabled: bool,
    pub listen: SocketAddr,
    /// HTTP Basic Auth username -> password. Empty means no auth required.
    pub users: HashMap<String, String>,
    pub tls: TlsConfig,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "0.0.0.0:9002".parse().unwrap(),
            users: HashMap::new(),
            tls: TlsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub listen: SocketAddr,
    /// HTTP Basic Auth username -> password. Empty means no auth required.
    pub users: HashMap<String, String>,
    /// Usernames (must be keys of `users`) allowed to view/edit `/settings`
    /// and its `/api/config/*` endpoints -- every other authenticated user
    /// can still read the rest of the dashboard, just not that page or its
    /// APIs (a plain `403` either way, not a hidden/blank page). Empty means
    /// every authenticated user may access settings, the same
    /// all-or-nothing behavior this had before privileged users existed;
    /// set this once you want to split "can view" from "can change config".
    /// Has no effect when `users` itself is empty (auth disabled entirely).
    pub admins: Vec<String>,
    pub realm: String,
    /// Name of this server: the main dashboard heading and the browser tab
    /// title of every dashboard page.
    pub servername: String,
    /// UI behavior toggles (currently just read-only/open mode).
    pub ui: DashboardUiConfig,
    /// Serve the classic dashboard at `/` instead of the redesigned one
    /// (sidebar layout, light and dark following the system, Routing page).
    /// `false` (the default) serves the new one. Both stay reachable: the
    /// classic dashboard under `/classic`, the new one under `/ui`.
    pub legacy_ui: bool,
    pub tls: TlsConfig,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: "0.0.0.0:9003".parse().unwrap(),
            users: HashMap::new(),
            admins: Vec::new(),
            realm: "brew-server-dashboard".into(),
            servername: "Tetra Network Monitor".into(),
            ui: DashboardUiConfig::default(),
            legacy_ui: false,
            tls: TlsConfig::default(),
        }
    }
}

/// `[dashboard.ui]` -- how the dashboard presents itself to visitors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DashboardUiConfig {
    /// When `true` (`ui.open = true`), the dashboard is viewable by anyone
    /// without logging in and is read-only for visitors who aren't signed in
    /// as an admin: a Login button in the header lets an admin authenticate
    /// and unlock the Settings page and Basestation control. When `false`
    /// (the default), the configured `users` must log in just to view, the
    /// original all-or-nothing behavior. Independent of `users`/`admins`:
    /// those still define *who* the admins are; this only decides whether
    /// unauthenticated viewing is allowed.
    pub open: bool,
}

impl Default for DashboardUiConfig {
    fn default() -> Self {
        Self { open: false }
    }
}

/// Configuration for the SIP subsystem: a UDP SIP listener that terminates
/// SIP extensions (user/pass registrations) and SIP trunks (peer gateways such
/// as Asterisk), plus the voice routes that bridge SIP to the Brew/TETRA side.
///
/// Trunks and extensions can be provisioned here in the TOML file *or* added at
/// runtime through the dashboard control API; runtime additions are held in
/// memory only and are lost on the config-file reload/restart, so anything that
/// must survive a restart belongs in the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SipConfig {
    /// Master switch for the whole SIP subsystem.
    pub enabled: bool,
    /// UDP address the SIP stack binds for signalling (default 0.0.0.0:5060).
    pub listen: SocketAddr,
    /// Address advertised to peers in Contact/Via when it must differ from
    /// `listen` (e.g. behind NAT). Empty means use the socket's local address.
    pub advertised_host: String,
    /// UDP port range for RTP media the relay allocates from, inclusive.
    pub rtp_port_min: u16,
    pub rtp_port_max: u16,
    /// SIP digest authentication realm presented to registering extensions.
    pub realm: String,
    /// Seconds a REGISTER binding is kept before it is considered expired when
    /// the client does not supply its own Expires.
    pub registration_ttl_seconds: u64,
    /// Maximum duration (seconds) a SIP call may run before the server
    /// force-ends it: a BYE to the SIP peer(s), and, for a call bridged to
    /// Brew, the same CALL_RELEASE the Brew side gets from a normal hangup.
    /// 0 disables the limit.
    pub max_call_duration_seconds: u64,
    /// Statically provisioned SIP extensions (user/pass), keyed by the AOR user
    /// part (the number/name the extension registers as).
    pub extensions: HashMap<String, SipExtensionConfig>,
    /// Statically provisioned SIP trunks, keyed by an operator-chosen name.
    pub trunks: HashMap<String, SipTrunkConfig>,
    /// Voice routes bridging SIP and Brew endpoints. Evaluated top to bottom;
    /// the first matching route wins.
    pub routes: Vec<VoiceRouteConfig>,
}

impl Default for SipConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: "0.0.0.0:5060".parse().unwrap(),
            advertised_host: String::new(),
            rtp_port_min: 16000,
            rtp_port_max: 17000,
            realm: "brew-server".into(),
            registration_ttl_seconds: 3600,
            max_call_duration_seconds: 14400,
            extensions: HashMap::new(),
            trunks: HashMap::new(),
            routes: Vec::new(),
        }
    }
}

/// A provisioned SIP extension: a username/password the server authenticates on
/// REGISTER and INVITE. The extension's AOR user part is the map key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SipExtensionConfig {
    /// Shared secret for SIP digest auth. Required in practice; an empty
    /// password disables authentication for this extension (not recommended).
    pub password: String,
    /// Optional human-readable label shown on the dashboard.
    pub display_name: String,
    /// Optional TETRA ISSI this extension maps to, so calls to/from the Brew
    /// side can address it as a subscriber. 0 means "not mapped".
    pub issi: u32,
    /// Whether this extension is allowed to place calls out to trunks.
    pub allow_outbound: bool,
}

impl Default for SipExtensionConfig {
    fn default() -> Self {
        Self {
            password: String::new(),
            display_name: String::new(),
            issi: 0,
            allow_outbound: true,
        }
    }
}

/// How a trunk associates with a remote peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrunkDirection {
    /// The remote peer registers to us (we are the registrar). We learn its
    /// contact from its REGISTER; `remote_host` may be left empty.
    Inbound,
    /// We register to the remote peer and originate/receive calls to/from a
    /// fixed `remote_host`. Used for Asterisk/ITSP style trunks.
    Outbound,
    /// No registration in either direction; a static IP-authenticated peer
    /// identified purely by `remote_host`.
    Peer,
}

impl Default for TrunkDirection {
    fn default() -> Self { TrunkDirection::Peer }
}

/// A provisioned SIP trunk to a VoIP gateway (Asterisk, an ITSP, another PBX).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SipTrunkConfig {
    pub direction: TrunkDirection,
    /// `host:port` of the remote peer. Required for Outbound/Peer trunks;
    /// optional for Inbound trunks (filled in from the peer's REGISTER).
    pub remote_host: String,
    /// Auth username used when we register to / are challenged by the peer
    /// (Outbound), or the username the peer must use to register to us
    /// (Inbound). Defaults to the trunk name when empty.
    pub username: String,
    pub password: String,
    /// Auth realm expected from/presented to the peer. Empty uses the peer's
    /// challenged realm (Outbound) or the global SIP realm (Inbound).
    pub realm: String,
    /// Re-registration interval in seconds for Outbound trunks.
    pub register_interval_seconds: u64,
    /// Whether this trunk is currently enabled.
    pub enabled: bool,
}

impl Default for SipTrunkConfig {
    fn default() -> Self {
        Self {
            direction: TrunkDirection::default(),
            remote_host: String::new(),
            username: String::new(),
            password: String::new(),
            realm: String::new(),
            register_interval_seconds: 300,
            enabled: true,
        }
    }
}

/// One side of a voice route: which kind of endpoint, and its address.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteEndpoint {
    /// A SIP extension identified by its AOR user part.
    SipExtension { user: String },
    /// A SIP trunk identified by its configured name; `number` is the dialled
    /// number sent to / matched from the trunk.
    SipTrunk { trunk: String, #[serde(default)] number: String },
    /// A Brew private (individual) subscriber identified by ISSI.
    BrewPrivate { issi: u32 },
    /// A Brew group call identified by GSSI.
    BrewGroup { gssi: u32 },
}

/// A voice route rule. A call whose origin matches `from` and whose dialled
/// destination matches `match_pattern` is bridged to `to`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceRouteConfig {
    /// Operator label for the route.
    pub name: String,
    /// Glob-ish destination match against the dialled number/AOR. `*` matches
    /// any, a trailing `*` is a prefix match, otherwise exact.
    pub match_pattern: String,
    /// Endpoint the matching call is bridged to. Serialized as an inline table.
    pub to: Option<RouteEndpoint>,
    /// Optional restriction: only calls originating from this endpoint match.
    pub from: Option<RouteEndpoint>,
    /// A literal prefix stripped from the dialled string before it is handed
    /// to `to` (e.g. `to = { kind = "sip_trunk", trunk = "..." }` with an
    /// empty `number`, which passes the dialled string through as-is). Useful
    /// for a PBX-style outside-line prefix: `match_pattern = "9*"` selects
    /// the route on the leading "9", `strip_prefix = "9"` removes it so the
    /// trunk dials the bare number. Matching itself always runs against the
    /// *un-stripped* dialled string. Empty (the default) strips nothing.
    pub strip_prefix: String,
    pub enabled: bool,
}

impl Default for VoiceRouteConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            match_pattern: "*".into(),
            to: None,
            from: None,
            strip_prefix: String::new(),
            enabled: true,
        }
    }
}

/// Server-to-server federation: connects this brew-server to other
/// brew-server instances over the same Brew WebSocket protocol real
/// Basestations use (a peer dials in/out exactly like a Basestation would,
/// tagged `X-Brew-Mode: Peer`). Subscriber/group registrations propagate
/// peer to peer (each server relays what it learns, from any source, to its
/// *other* peers -- split-horizon, so it never echoes an advertisement back
/// out the link it arrived on), and private/group calls and SDS route
/// transparently hop to hop the same way they already route to any other
/// connected client: call/SDS routing has no federation-specific code at all,
/// it Just Works once a remote ISSI/GSSI's registration has propagated to
/// this server. Split-horizon alone is only correct for a loop-free topology
/// (a chain or a star, i.e. any tree of peer links); with `loop_safe` on
/// every server any topology is, rings and full meshes included.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FederationConfig {
    pub enabled: bool,
    /// Peers this server dials out to. An inbound peer connection (another
    /// server dialling in to us) needs no entry here: it just authenticates
    /// like a Basestation would, with `X-Brew-Mode: Peer`.
    pub peers: Vec<FederationPeerConfig>,
    /// WebSocket ping interval on every federation link: the peers dialled from
    /// `peers` and any `X-Brew-Mode: Peer` connection accepted from another
    /// server (even with `enabled = false`). 0 disables pings and the timeout.
    pub keepalive_interval_seconds: u64,
    /// A federation link that has received nothing -- no frame, not even a
    /// pong -- for this long is closed: its registrations are withdrawn and an
    /// outbound link is redialled. Raised to at least twice the interval.
    pub keepalive_timeout_seconds: u64,
    /// Loop-safe federation with peers that also enable it (negotiated per
    /// link with the `X-Brew-Federation` header, on the links we dial and the
    /// ones we accept, even with `enabled = false`). Registrations then travel
    /// as path-vector route adverts (origin server, registration clock,
    /// servers crossed) instead of relayed SUB messages, so any topology --
    /// ring, full mesh, redundant links -- is safe: the newest registration
    /// wins network-wide, it is reached by the shortest loop-free path, and
    /// routing fails over when a link drops. Links to peers without it (older
    /// servers, or `loop_safe = false`) keep the split-horizon relay, so such
    /// a peer must hang off the mesh by a single link -- no cycle may pass
    /// through it. Default false: split-horizon only, safe for a chain or
    /// star.
    pub loop_safe: bool,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self { enabled: false, peers: Vec::new(), keepalive_interval_seconds: 15, keepalive_timeout_seconds: 45, loop_safe: false }
    }
}

/// One outbound federation peer link.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FederationPeerConfig {
    /// Operator label, used in logs and the dashboard.
    pub name: String,
    /// `host:port` of the peer's Brew listener (the same port real
    /// Basestations connect to).
    pub remote_host: String,
    /// Discovery path on the peer, matching its `websocket_path` (default
    /// `/brew`).
    pub path: String,
    /// Brew digest username this server presents to the peer (numeric, max 7
    /// digits, same rule as a Basestation's). Only needed if the peer has
    /// `[auth]` enabled; ignored otherwise.
    pub username: String,
    pub password: String,
    /// Seconds between reconnect attempts after a dropped/failed link.
    pub reconnect_interval_seconds: u64,
    pub enabled: bool,
    /// Dial this peer over TLS: `https://` discovery and a `wss://` link, for a
    /// peer whose Brew listener has `[tls] enabled = true`. Plain `ws://` when
    /// false (default).
    pub tls: bool,
    /// PEM bundle of CA certificates the peer's certificate must chain to.
    /// Default: the Debian/Ubuntu/Alpine system bundle.
    pub tls_ca_path: PathBuf,
    /// Name the peer's certificate must be valid for, also sent as SNI. Empty:
    /// the host part of `remote_host`.
    pub tls_server_name: String,
    /// For a self-signed peer: a copy of the peer's own certificate (PEM, first
    /// certificate in the file). When set, exactly that certificate is trusted,
    /// whatever its issuer, name or expiry, and `tls_ca_path`/`tls_server_name`
    /// are ignored. Empty: off.
    pub tls_pinned_cert_path: PathBuf,
}

impl Default for FederationPeerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            remote_host: String::new(),
            path: "/brew".into(),
            username: String::new(),
            password: String::new(),
            reconnect_interval_seconds: 15,
            enabled: true,
            tls: false,
            tls_ca_path: "/etc/ssl/certs/ca-certificates.crt".into(),
            tls_server_name: String::new(),
            tls_pinned_cert_path: PathBuf::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:9000".parse().unwrap(),
            websocket_path: "/brew".into(),
            websocket_subprotocol: "brew".into(),
            route_without_affiliations: false,
            fallback_broadcast_when_no_affiliations: true,
            allow_multiple_calls_per_group: false,
            higher_priority_number_wins: true,
            preempt_cause: 1,
            max_call_duration_seconds: 14400,
            call_inactivity_timeout_seconds: 60,
            auth: AuthConfig::default(),
            tls: TlsConfig::default(),
            telemetry: TelemetryConfig::default(),
            control: ControlConfig::default(),
            dashboard: DashboardConfig::default(),
            storage: StorageConfig::default(),
            sip: SipConfig::default(),
            federation: FederationConfig::default(),
            aprs: AprsConfig::default(),
            sms_center: SmsCenterConfig::default(),
            bts_locations: HashMap::new(),
            blacklist: BlacklistConfig::default(),
            allowlist: AllowlistConfig::default(),
            ha: HaConfig::default(),
            roaming: RoamingConfig::default(),
            handover: HandoverConfig::default(),
            talkgroup_pools: Vec::new(),
            issi_names: std::collections::BTreeMap::new(),
            gssi_names: std::collections::BTreeMap::new(),
            group_access: std::collections::BTreeMap::new(),
            multi_network: MultiNetworkConfig::default(),
        }
    }
}

/// Settings for forwarding decoded mobile-station LIP positions to APRS-IS as
/// APRS object reports, one object per ISSI, all sent under this server's own
/// login -- the same technique real DMR/D-STAR-to-APRS gateways use to relay
/// many radios' positions through a single APRS-IS connection, rather than
/// needing a distinct callsign/passcode per mobile station.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AprsConfig {
    pub enabled: bool,
    /// APRS-IS server, `host:port` (e.g. `rotate.aprs2.net:14580`).
    pub server: String,
    /// This gateway's own APRS-IS login callsign (with SSID if desired, e.g.
    /// `MYCALL-10`).
    pub callsign: String,
    /// This callsign's APRS-IS passcode. Not derivable here -- obtain it the
    /// same way any APRS client does (it is tied to the callsign).
    pub passcode: String,
    /// APRS symbol table identifier and symbol code for reported objects.
    /// Defaults to the primary table's jeep icon, a reasonable generic mobile
    /// marker; override to taste (e.g. `/>` for a car).
    pub symbol_table: char,
    pub symbol_code: char,
    /// Free-text appended to every object report (e.g. "TETRA MS").
    pub comment: String,
    /// Prefix used to build each object's 9-character APRS object name from
    /// its ISSI (`"{prefix}{issi}"`, truncated/padded to 9 chars). Keep short
    /// so the ISSI digits still fit.
    pub object_name_prefix: String,
    /// Minimum seconds between two object reports for the same ISSI, so a
    /// noisy beacon source cannot flood APRS-IS. 0 disables rate limiting.
    pub min_report_interval_seconds: u64,
    /// Seconds between reconnect attempts after a dropped/failed APRS-IS link.
    pub reconnect_interval_seconds: u64,
}

impl Default for AprsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: "rotate.aprs2.net:14580".into(),
            callsign: String::new(),
            passcode: String::new(),
            symbol_table: '/',
            symbol_code: 'j',
            comment: "TETRA MS via brew-server".into(),
            object_name_prefix: "MS".into(),
            min_report_interval_seconds: 60,
            reconnect_interval_seconds: 15,
        }
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Self = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.ha.validate()?;
        validate_group_access(&cfg.group_access)?;
        Ok(cfg)
    }

    /// Parses `text` as a config, same rules `load` applies to a file's
    /// contents. Used by the dashboard's config editor to validate a proposed
    /// change before writing it to disk.
    pub fn parse(text: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(text).context("parsing config")?;
        cfg.ha.validate()?;
        validate_group_access(&cfg.group_access)?;
        Ok(cfg)
    }

    /// Renders this config back to TOML, the same shape `load` accepts. Used
    /// both to seed the dashboard's editor with the live config and to
    /// serialize a dashboard-made structured edit (e.g. one trunk added)
    /// before it is written to disk.
    pub fn to_toml_pretty(&self) -> Result<String> {
        toml::to_string_pretty(self).context("serializing config to TOML")
    }

    /// Atomically writes `text` to `path`: write to a sibling temp file, then
    /// rename over the target. A crash or concurrent read mid-write never
    /// observes a partial file, and the existing `config_watcher` (which polls
    /// the file's mtime) picks up the change as a single event.
    pub fn save_atomic(path: impl AsRef<Path>, text: &str) -> Result<()> {
        let path = path.as_ref();
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text)
            .with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_loads_when_file_missing() {
        let cfg = Config::load("/nonexistent/path/brew-server.toml").unwrap();
        assert_eq!(cfg.websocket_subprotocol, Config::default().websocket_subprotocol);
    }

    /// The dashboard config editor relies on serialize(edit)->parse being
    /// lossless for every shape actually used in a real config, including the
    /// trickiest bits: HashMap-keyed extensions/trunks and the internally
    /// tagged `RouteEndpoint` enum inside `Option`.
    #[test]
    fn to_toml_pretty_round_trips_through_parse() {
        let mut cfg = Config::default();
        cfg.sip.enabled = true;
        cfg.sip.extensions.insert("1001".into(), SipExtensionConfig {
            password: "secret".into(),
            display_name: "Front Desk".into(),
            issi: 42,
            allow_outbound: true,
        });
        cfg.sip.trunks.insert("asterisk".into(), SipTrunkConfig {
            direction: TrunkDirection::Outbound,
            remote_host: "10.0.0.5:5060".into(),
            username: "brew".into(),
            password: "hunter2".into(),
            realm: "asterisk".into(),
            register_interval_seconds: 120,
            enabled: true,
        });
        cfg.sip.routes.push(VoiceRouteConfig {
            name: "outbound".into(),
            match_pattern: "9*".into(),
            strip_prefix: "9".into(),
            to: Some(RouteEndpoint::SipTrunk { trunk: "asterisk".into(), number: "".into() }),
            from: Some(RouteEndpoint::BrewPrivate { issi: 42 }),
            enabled: true,
        });
        cfg.bts_locations.insert("1000001".into(), BtsLocationConfig {
            name: "Athens BTS".into(),
            lat: 37.9917,
            lon: 23.7640,
        });
        cfg.dashboard.users.insert("alice".into(), "secret".into());
        cfg.dashboard.users.insert("bob".into(), "secret2".into());
        cfg.dashboard.admins = vec!["alice".into()];

        let text = cfg.to_toml_pretty().expect("serialize");
        let parsed = Config::parse(&text).expect("re-parse");
        let athens = &parsed.bts_locations["1000001"];
        assert_eq!(athens.name, "Athens BTS");
        assert!((athens.lat - 37.9917).abs() < 1e-9);
        assert!((athens.lon - 23.7640).abs() < 1e-9);
        assert_eq!(parsed.dashboard.admins, vec!["alice".to_string()]);

        assert_eq!(parsed.sip.enabled, true);
        assert_eq!(parsed.sip.extensions["1001"].issi, 42);
        assert_eq!(parsed.sip.trunks["asterisk"].direction, TrunkDirection::Outbound);
        assert_eq!(parsed.sip.routes.len(), 1);
        match &parsed.sip.routes[0].to {
            Some(RouteEndpoint::SipTrunk { trunk, .. }) => assert_eq!(trunk, "asterisk"),
            other => panic!("unexpected: {other:?}"),
        }
        match &parsed.sip.routes[0].from {
            Some(RouteEndpoint::BrewPrivate { issi }) => assert_eq!(*issi, 42),
            other => panic!("unexpected: {other:?}"),
        }
    }

    fn ha_ok() -> HaConfig {
        HaConfig {
            enabled: true,
            node_name: "bs-a".into(),
            vip: "192.0.2.100/24".into(),
            real_ip: "192.0.2.11".parse().unwrap(),
            peer_ip: "192.0.2.12".parse().unwrap(),
            shared_secret: "s3cret".into(),
            ..HaConfig::default()
        }
    }

    #[test]
    fn ha_validate_accepts_a_sane_config() {
        ha_ok().validate().unwrap();
        assert!(HaConfig::default().validate().is_ok(), "disabled HA is never checked");
    }

    #[test]
    fn ha_validate_rejects_mistakes() {
        let cases: Vec<(&str, Box<dyn Fn(&mut HaConfig)>)> = vec![
            ("vip subnet", Box::new(|c| c.vip = "198.51.100.1/24".into())),
            ("vip prefix", Box::new(|c| c.vip = "192.0.2.100".into())),
            ("same ips", Box::new(|c| c.peer_ip = c.real_ip)),
            ("vip = real", Box::new(|c| c.vip = "192.0.2.11/24".into())),
            ("no secret", Box::new(|c| c.shared_secret.clear())),
            ("no name", Box::new(|c| c.node_name.clear())),
            ("dead_after", Box::new(|c| c.dead_after_ms = 1000)),
            ("gateway", Box::new(|c| c.check_gateway = "nope".into())),
        ];
        for (what, edit) in cases {
            let mut c = ha_ok();
            edit(&mut c);
            assert!(c.validate().is_err(), "{what} should be rejected");
        }
    }
}

#[cfg(test)]
mod roaming_config_tests {
    use super::*;

    fn parse(toml_src: &str) -> std::result::Result<Config, toml::de::Error> { toml::from_str(toml_src) }

    #[test]
    fn server_name_is_a_dashboard_setting() {
        assert_eq!(parse("").unwrap().dashboard.servername, "Tetra Network Monitor");
        let cfg = parse("[dashboard]\nservername = \"OE Tetra\"\n").unwrap();
        assert_eq!(cfg.dashboard.servername, "OE Tetra");
    }

    #[test]
    fn kick_old_issi_defaults_off() {
        assert!(!parse("").unwrap().roaming.kick_old_issi);
        assert!(!parse("[roaming]\n").unwrap().roaming.kick_old_issi);
    }

    #[test]
    fn kick_old_issi_accepts_int_and_bool() {
        assert!(parse("[roaming]\nkick_old_issi = 1\n").unwrap().roaming.kick_old_issi);
        assert!(!parse("[roaming]\nkick_old_issi = 0\n").unwrap().roaming.kick_old_issi);
        assert!(parse("[roaming]\nkick_old_issi = true\n").unwrap().roaming.kick_old_issi);
        assert!(parse("[roaming]\nkick_old_issi = 2\n").is_err());
    }

    #[test]
    fn handover_timeout_defaults_below_the_basestation_fallback() {
        assert_eq!(parse("").unwrap().handover.timeout_ms, 2000);
        assert_eq!(parse("[handover]\ntimeout_ms = 1500\n").unwrap().handover.timeout_ms, 1500);
    }
}
