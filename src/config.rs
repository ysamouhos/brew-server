use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, net::SocketAddr, path::Path, path::PathBuf};

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
    /// Fixed geographic locations for Basestations, keyed by the same numeric
    /// Brew username each one authenticates with under `[auth.users]` -- so a
    /// location entry automatically matches whichever connection actually
    /// logs in as that Basestation, no separate ID scheme needed. Purely
    /// informational (dashboard map markers); has no effect on routing.
    pub bts_locations: HashMap<String, BtsLocationConfig>,
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
            tls: TlsConfig::default(),
        }
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
/// this server. This is correct for a loop-free topology (a chain or a star,
/// i.e. any tree of peer links); a topology with a cycle (e.g. a full mesh)
/// is not safe with split-horizon alone and needs additional loop prevention
/// (hop count / path vector) not implemented here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FederationConfig {
    pub enabled: bool,
    /// Peers this server dials out to. An inbound peer connection (another
    /// server dialling in to us) needs no entry here: it just authenticates
    /// like a Basestation would, with `X-Brew-Mode: Peer`.
    pub peers: Vec<FederationPeerConfig>,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self { enabled: false, peers: Vec::new() }
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
            bts_locations: HashMap::new(),
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
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Parses `text` as a config, same rules `load` applies to a file's
    /// contents. Used by the dashboard's config editor to validate a proposed
    /// change before writing it to disk.
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing config")
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
}
