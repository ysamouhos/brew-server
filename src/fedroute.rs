//! Federation routing: the loop-safe route exchange between brew-servers, and
//! de-duplication of calls and SDS that reach a server more than once.
//!
//! Negotiation. Loop-safe federation is agreed per peer link at connect time,
//! only between servers that both enable `[federation] loop_safe`: the
//! dialling side sends `X-Brew-Federation: 1` and its `X-Brew-Server-Id` on
//! the WebSocket upgrade, and the accepting side answers with its own in the
//! `101`. Anyone else -- an older brew-server, another Brew server, a
//! Basestation -- ignores the headers and does not echo them, so its link
//! stays a plain one. `FedState::links` holds the negotiated links; nothing of
//! class `protocol::CLASS_FEDERATION` is ever sent on any other connection.
//!
//! Route table. On a negotiated link registrations do not travel as relayed
//! SUB messages but as path-vector route adverts (`FED_ROUTE`/`FED_WITHDRAW`):
//! the ISSI, its registration clock at the origin server (`reg`, a hybrid
//! logical clock -- see `FedState::tick`), the servers it crossed (`path`,
//! sender first, origin last) and its groups. Each server keeps every
//! neighbour's current offer (`FedState::rib_in`) next to its own local
//! registrations and picks, per ISSI, the origin with the newest registration
//! (tie: higher server id) and the shortest path to it (`select`). Only that
//! choice -- the effective route -- is kept in `Inner::subscribers` and
//! `Inner::group_clients`, so call, SDS, SIP and SMS Center routing need no
//! change; and only a change of it is advertised on (`publish`), with this
//! server's id put in front. Nobody accepts a path holding its own id, so
//! adverts cannot loop whatever the topology (ring, full mesh, redundant
//! links), and when a link drops (`state::cleanup_client`) every ISSI routed
//! or offered over it fails over to the next best offer or is withdrawn. A
//! legacy peer link (an older server, or `loop_safe = false`) still gets SUB
//! messages, derived from the same effective table, and whatever it sends
//! counts as registered at this server -- so such a peer must hang off the
//! mesh by a single link: a cycle through it is not loop-safe.
//!
//! Call/SDS de-duplication: in any federation topology with more than one
//! path between two servers (a ring, a mesh, two links between the same pair)
//! the same GROUP_TX, private SETUP_REQUEST or SDS header can reach a server
//! more than once, over different peer links. Without a check the second copy
//! would take the call over (new owner, forwarded again) and could circulate
//! forever. Each copy is keyed by (uuid, source ISSI): a talker change inside
//! a group call reuses the uuid with another source ISSI and must still be
//! accepted. A server that drops a duplicate GROUP_TX from a loop-safe link
//! answers `FED_PRUNE`, and the sender stops feeding that link the call's
//! voice: one stream per server, not one per redundant link.

use crate::protocol::{self, CLASS_FEDERATION, FED_BTS, FED_BTS_HELLO, FED_EMERGENCY, FED_MS_POS, FED_PRUNE, FED_ROUTE, FED_WITHDRAW};
use crate::state::{AppState, CallKind, ClientId, ClientMode, Inner, Subscriber};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Request (and `101` response) header offering (accepting) loop-safe
/// federation; its value is the version, currently 1.
pub const X_BREW_FEDERATION: &str = "X-Brew-Federation";
/// This server's id, exactly 16 hex digits (see `FedState::self_id`).
pub const X_BREW_SERVER_ID: &str = "X-Brew-Server-Id";

/// Most servers a route may cross, origin included: a longer advert is not
/// passed on, which also bounds how long a withdrawn route can be chased
/// around a ring.
pub const MAX_PATH: usize = 16;
/// An advert registered more than this far ahead of our own clock is refused
/// (taken as a withdrawal of that link's offer): it would win every
/// comparison, and an ISSI roaming away from a server whose clock is wrong
/// would stay stuck there.
pub const MAX_FUTURE_MS: u64 = 86_400_000;

/// Where an effective route leads (`Subscriber::route`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Route {
    /// Registration clock at the origin when the ISSI last (re)registered.
    pub reg: u64,
    /// Servers to cross, next hop first and origin last; empty for an ISSI
    /// registered here (by a Basestation, Terminal or legacy peer link).
    pub path: Vec<u64>,
}

/// One neighbour's current offer for an ISSI (a received `FED_ROUTE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advert {
    pub reg: u64,
    /// `path[0]` is the neighbour itself, the last entry the origin.
    pub path: Vec<u64>,
    pub groups: Vec<u32>,
}

/// Loop-safe federation state, in `Inner`.
#[derive(Debug)]
pub struct FedState {
    /// This server's id: random, non-zero and new on every start. Never
    /// persisted or configured -- a cloned VM or config file would duplicate
    /// it, and each server would then silently reject the other's routes as
    /// its own. A restart closes every link, so nothing keeps the old id.
    pub self_id: u64,
    /// Hybrid logical clock for local registrations (`tick`).
    clock: u64,
    /// Negotiated (loop-safe) peer links -> neighbour server id.
    pub links: HashMap<ClientId, u64>,
    /// Latest advert per ISSI per negotiated link (each neighbour's current offer).
    pub rib_in: HashMap<u32, HashMap<ClientId, Advert>>,
    /// Negotiated links whose far end announced `FED_BTS` support
    /// (`FED_BTS_HELLO`): the only ones Basestation positions are sent to.
    pub bts_links: HashSet<ClientId>,
    /// Links that also announced mobile station position support (hello version >= 2).
    pub ms_links: HashSet<ClientId>,
    /// Latest position per mobile station ISSI seen by this server's own
    /// decoding (empty `path`) or relayed by another (`path` set).
    pub ms_pos: HashMap<u32, MsEntry>,
    /// Links that also announced emergency alarm support (hello version >= 3).
    pub em_links: HashSet<ClientId>,
    /// This server's own active emergency alarms (from telemetry), by ISSI, as last advertised.
    pub em_local: HashMap<u32, LocalEm>,
    /// Alarms relayed by other servers, by (origin server id, ISSI).
    pub em_remote: HashMap<(u64, u32), RemoteEm>,
    /// Positions of this server's own Basestations (from telemetry), by
    /// telemetry identity, as last advertised.
    pub bts_local: HashMap<String, LocalBts>,
    /// Positions learned from other servers, by (origin server id, identity).
    pub bts_remote: HashMap<(u64, String), RemoteBts>,
}

/// Longest identity / name carried in a `FED_BTS`, in bytes.
pub const MAX_BTS_TEXT: usize = 64;
/// Origin re-advertises a live position at least this often ...
pub const BTS_REFRESH_MS: u64 = 30_000;
/// ... and a learned one not refreshed for this long is dropped (origin or
/// the path to it is gone; there is no explicit withdrawal on a lost link).
pub const BTS_STALE_MS: u64 = 120_000;

/// Hello version that adds mobile station positions to Basestation ones.
pub const HELLO_VERSION: u8 = 3;

/// The origin re-advertises an active alarm this often ...
pub const EM_REFRESH_MS: u64 = 10_000;
/// ... and a relayed one not refreshed for this long is dropped (the origin or
/// the path to it is gone).
pub const EM_STALE_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq)]
pub struct LocalEm {
    pub dest: Option<u32>,
    pub bts: String,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteEm {
    pub dest: Option<u32>,
    pub bts: String,
    /// False once the origin advertised the alarm cleared.
    pub active: bool,
    pub seq: u64,
    pub path: Vec<u64>,
    pub seen_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MsEntry {
    pub lat: f64,
    pub lon: f64,
    /// When the fix was made at the origin (ms); only a newer one replaces it.
    pub at_ms: u64,
    /// Reporting station label at the origin.
    pub station: String,
    /// Servers crossed, next hop first and origin last; empty when decoded here.
    pub path: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalBts {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteBts {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// False once the origin advertised the Basestation as gone.
    pub online: bool,
    /// Origin's advert clock; only a higher one replaces the entry.
    pub seq: u64,
    /// Servers crossed, next hop first and origin last.
    pub path: Vec<u64>,
    /// Local time the newest advert arrived (ms), for `BTS_STALE_MS`.
    pub seen_ms: u64,
}

impl Default for FedState {
    fn default() -> Self {
        let self_id = loop {
            let id = Uuid::new_v4().as_u64_pair().0;
            if id != 0 { break id; }
        };
        Self {
            self_id, clock: 0, links: HashMap::new(), rib_in: HashMap::new(),
            bts_links: HashSet::new(), ms_links: HashSet::new(), ms_pos: HashMap::new(), em_links: HashSet::new(), em_local: HashMap::new(), em_remote: HashMap::new(), bts_local: HashMap::new(), bts_remote: HashMap::new(),
        }
    }
}

impl FedState {
    /// Registration clock for a local (re)registration: wall-clock ms, but
    /// always above every registration seen so far (`observe`), so one made
    /// here after a remote one wins even when the other server's clock runs
    /// ahead. Clock skew only orders truly concurrent registrations of the
    /// same ISSI on two servers (keep the servers on NTP).
    pub fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1).max(crate::telemetry::now_ms());
        self.clock
    }

    /// Takes an accepted remote registration clock into account.
    pub fn observe(&mut self, reg: u64) {
        self.clock = self.clock.max(reg);
    }
}

/// `X-Brew-Server-Id` value: 16 lowercase hex digits.
pub fn format_server_id(id: u64) -> String {
    format!("{id:016x}")
}

/// Parses an `X-Brew-Server-Id` value: exactly 16 hex digits.
pub fn parse_server_id(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.len() != 16 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(value, 16).ok()
}

/// Whether an `X-Brew-Federation` value offers (accepts) a version we speak.
pub fn federation_offered(value: Option<&str>) -> bool {
    value.and_then(|v| v.trim().parse::<u32>().ok()).is_some_and(|v| v >= 1)
}

// ─── Wire format ─────────────────────────────────────────────────────────

/// One `CLASS_FEDERATION` message, little-endian:
///
/// - `FED_WITHDRAW`: `0xfe 0x00 issi:u32` -- this link no longer offers a
///   route to `issi`.
/// - `FED_ROUTE`: `0xfe 0x01 issi:u32 reg:u64 n:u8 path:u64[n] groups:u32[..]`
///   -- `issi` is reachable via the sender, `path[0]` being the sender and
///   `path[n-1]` the origin, `1 <= n <= MAX_PATH`; the groups run to the end.
/// - `FED_PRUNE`: `0xfe 0x02 uuid[16] source_issi:u32` -- the sender already
///   gets this turn of group call `uuid` over another link: stop sending it.
#[derive(Debug, Clone, PartialEq)]
pub enum FedMessage {
    Withdraw { issi: u32 },
    Route { issi: u32, advert: Advert },
    Prune { id: Uuid, source_issi: u32 },
    /// The sender understands `FED_BTS`.
    BtsHello { version: u8 },
    Bts(BtsAdvert),
    MsPos(MsAdvert),
    Emergency(EmAdvert),
    /// A type this version does not know (from a newer peer): ignored.
    Unknown(u8),
}

/// One Basestation position advert. Wire, little-endian:
/// `0xfe 0x04 seq:u64 online:u8 lat:f64 lon:f64 n:u8 path:u64[n] klen:u8 key nlen:u8 name`
/// -- `path[0]` is the sender and `path[n-1]` the origin, `1 <= n <= MAX_PATH`;
/// key and name are UTF-8, at most `MAX_BTS_TEXT` bytes each.
#[derive(Debug, Clone, PartialEq)]
pub struct BtsAdvert {
    pub key: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub online: bool,
    pub seq: u64,
    pub path: Vec<u64>,
}

/// Whether a position is a real fix (finite, in range, not the 0/0 "empty").
pub fn valid_position(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)
        && (lat != 0.0 || lon != 0.0)
}

fn clip(s: &str) -> &str {
    let mut end = s.len().min(MAX_BTS_TEXT);
    while !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

pub fn build_bts_hello() -> Vec<u8> {
    vec![CLASS_FEDERATION, FED_BTS_HELLO, HELLO_VERSION]
}

/// One mobile station position advert. Wire, little-endian:
/// `0xfe 0x05 at_ms:u64 issi:u32 lat:f64 lon:f64 n:u8 path:u64[n] slen:u8 station`
/// -- `path[0]` is the sender and `path[n-1]` the origin, `1 <= n <= MAX_PATH`.
#[derive(Debug, Clone, PartialEq)]
pub struct MsAdvert {
    pub issi: u32,
    pub lat: f64,
    pub lon: f64,
    pub at_ms: u64,
    pub station: String,
    pub path: Vec<u64>,
}

/// One emergency alarm advert. Wire, little-endian:
/// `0xfe 0x06 seq:u64 active:u8 issi:u32 dest:u32 n:u8 path:u64[n] blen:u8 bts`
/// -- `dest` 0 means none; `path[0]` is the sender and `path[n-1]` the origin,
/// `1 <= n <= MAX_PATH`.
#[derive(Debug, Clone, PartialEq)]
pub struct EmAdvert {
    pub issi: u32,
    pub dest: Option<u32>,
    pub bts: String,
    pub active: bool,
    pub seq: u64,
    pub path: Vec<u64>,
}

pub fn build_emergency(a: &EmAdvert) -> Vec<u8> {
    let bts = clip(&a.bts);
    let mut out = Vec::with_capacity(28 + 8 * a.path.len() + bts.len());
    out.extend_from_slice(&[CLASS_FEDERATION, FED_EMERGENCY]);
    out.extend_from_slice(&a.seq.to_le_bytes());
    out.push(a.active as u8);
    out.extend_from_slice(&a.issi.to_le_bytes());
    out.extend_from_slice(&a.dest.unwrap_or(0).to_le_bytes());
    out.push(a.path.len() as u8);
    for id in &a.path { out.extend_from_slice(&id.to_le_bytes()); }
    out.push(bts.len() as u8);
    out.extend_from_slice(bts.as_bytes());
    out
}

fn parse_emergency(raw: &[u8], nbr_id: u64) -> Result<EmAdvert, &'static str> {
    // 0xfe 0x06 | seq 2..10 | active 10 | issi 11..15 | dest 15..19 | n 19 | path 20.. | blen | bts
    if raw.len() < 21 {
        return Err("FED_EMERGENCY too short");
    }
    let u32_at = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().expect("checked length"));
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().expect("checked length"));
    let n = raw[19] as usize;
    if n == 0 || n > MAX_PATH {
        return Err("FED_EMERGENCY path length out of range");
    }
    let at = 20 + 8 * n;
    let blen = *raw.get(at).ok_or("FED_EMERGENCY of wrong length")? as usize;
    if blen > MAX_BTS_TEXT || raw.len() != at + 1 + blen {
        return Err("FED_EMERGENCY of wrong length");
    }
    let path: Vec<u64> = (0..n).map(|i| u64_at(20 + 8 * i)).collect();
    if path[0] != nbr_id {
        return Err("FED_EMERGENCY path does not start at the server that sent it");
    }
    if path.iter().enumerate().any(|(i, id)| path[..i].contains(id)) {
        return Err("FED_EMERGENCY path crosses a server twice");
    }
    let bts = String::from_utf8(raw[at + 1..].to_vec()).map_err(|_| "FED_EMERGENCY text is not UTF-8")?;
    let dest = u32_at(15);
    Ok(EmAdvert { issi: u32_at(11), dest: (dest != 0).then_some(dest), bts, active: raw[10] != 0, seq: u64_at(2), path })
}

pub fn build_ms_pos(a: &MsAdvert) -> Vec<u8> {
    let station = clip(&a.station);
    let mut out = Vec::with_capacity(32 + 8 * a.path.len() + station.len());
    out.extend_from_slice(&[CLASS_FEDERATION, FED_MS_POS]);
    out.extend_from_slice(&a.at_ms.to_le_bytes());
    out.extend_from_slice(&a.issi.to_le_bytes());
    out.extend_from_slice(&a.lat.to_le_bytes());
    out.extend_from_slice(&a.lon.to_le_bytes());
    out.push(a.path.len() as u8);
    for id in &a.path { out.extend_from_slice(&id.to_le_bytes()); }
    out.push(station.len() as u8);
    out.extend_from_slice(station.as_bytes());
    out
}

fn parse_ms_pos(raw: &[u8], nbr_id: u64) -> Result<MsAdvert, &'static str> {
    if raw.len() < 32 {
        return Err("FED_MS_POS too short");
    }
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().expect("checked length"));
    let f64_at = |o: usize| f64::from_le_bytes(raw[o..o + 8].try_into().expect("checked length"));
    let n = raw[30] as usize;
    if n == 0 || n > MAX_PATH {
        return Err("FED_MS_POS path length out of range");
    }
    let at = 31 + 8 * n;
    let slen = *raw.get(at).ok_or("FED_MS_POS of wrong length")? as usize;
    if slen > MAX_BTS_TEXT || raw.len() != at + 1 + slen {
        return Err("FED_MS_POS of wrong length");
    }
    let path: Vec<u64> = (0..n).map(|i| u64_at(31 + 8 * i)).collect();
    if path[0] != nbr_id {
        return Err("FED_MS_POS path does not start at the server that sent it");
    }
    if path.iter().enumerate().any(|(i, id)| path[..i].contains(id)) {
        return Err("FED_MS_POS path crosses a server twice");
    }
    let station = String::from_utf8(raw[at + 1..].to_vec()).map_err(|_| "FED_MS_POS text is not UTF-8")?;
    Ok(MsAdvert {
        issi: u32::from_le_bytes(raw[10..14].try_into().expect("checked length")),
        lat: f64_at(14), lon: f64_at(22), at_ms: u64_at(2), station, path,
    })
}

pub fn build_bts(a: &BtsAdvert) -> Vec<u8> {
    let (key, name) = (clip(&a.key), clip(&a.name));
    let mut out = Vec::with_capacity(30 + 8 * a.path.len() + key.len() + name.len());
    out.extend_from_slice(&[CLASS_FEDERATION, FED_BTS]);
    out.extend_from_slice(&a.seq.to_le_bytes());
    out.push(a.online as u8);
    out.extend_from_slice(&a.lat.to_le_bytes());
    out.extend_from_slice(&a.lon.to_le_bytes());
    out.push(a.path.len() as u8);
    for id in &a.path { out.extend_from_slice(&id.to_le_bytes()); }
    out.push(key.len() as u8);
    out.extend_from_slice(key.as_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
    out
}

fn parse_bts(raw: &[u8], nbr_id: u64) -> Result<BtsAdvert, &'static str> {
    if raw.len() < 28 {
        return Err("FED_BTS too short");
    }
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().expect("checked length"));
    let f64_at = |o: usize| f64::from_le_bytes(raw[o..o + 8].try_into().expect("checked length"));
    let n = raw[27] as usize;
    if n == 0 || n > MAX_PATH {
        return Err("FED_BTS path length out of range");
    }
    let mut at = 28 + 8 * n;
    let text = |at: &mut usize| -> Result<String, &'static str> {
        let len = *raw.get(*at).ok_or("FED_BTS of wrong length")? as usize;
        let bytes = raw.get(*at + 1..*at + 1 + len).ok_or("FED_BTS of wrong length")?;
        *at += 1 + len;
        if len > MAX_BTS_TEXT { return Err("FED_BTS text too long"); }
        String::from_utf8(bytes.to_vec()).map_err(|_| "FED_BTS text is not UTF-8")
    };
    if raw.len() < at {
        return Err("FED_BTS of wrong length");
    }
    let path: Vec<u64> = (0..n).map(|i| u64_at(28 + 8 * i)).collect();
    if path[0] != nbr_id {
        return Err("FED_BTS path does not start at the server that sent it");
    }
    if path.iter().enumerate().any(|(i, id)| path[..i].contains(id)) {
        return Err("FED_BTS path crosses a server twice");
    }
    let key = text(&mut at)?;
    let name = text(&mut at)?;
    if at != raw.len() {
        return Err("FED_BTS of wrong length");
    }
    Ok(BtsAdvert { key, name, lat: f64_at(11), lon: f64_at(19), online: raw[10] != 0, seq: u64_at(2), path })
}

pub fn build_withdraw(issi: u32) -> Vec<u8> {
    let mut out = vec![CLASS_FEDERATION, FED_WITHDRAW];
    out.extend_from_slice(&issi.to_le_bytes());
    out
}

pub fn build_prune(id: &Uuid, source_issi: u32) -> Vec<u8> {
    let mut out = vec![CLASS_FEDERATION, FED_PRUNE];
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&source_issi.to_le_bytes());
    out
}

pub fn build_route(issi: u32, reg: u64, path: &[u64], groups: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(15 + 8 * path.len() + 4 * groups.len());
    out.extend_from_slice(&[CLASS_FEDERATION, FED_ROUTE]);
    out.extend_from_slice(&issi.to_le_bytes());
    out.extend_from_slice(&reg.to_le_bytes());
    out.push(path.len() as u8);
    for id in path { out.extend_from_slice(&id.to_le_bytes()); }
    for g in groups { out.extend_from_slice(&g.to_le_bytes()); }
    out
}

/// Parses a `CLASS_FEDERATION` message received from neighbour `nbr_id`.
/// A route must start at that neighbour and cross no server twice.
pub fn parse(raw: &[u8], nbr_id: u64) -> Result<FedMessage, &'static str> {
    let u32_at = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().expect("checked length"));
    let u64_at = |o: usize| u64::from_le_bytes(raw[o..o + 8].try_into().expect("checked length"));
    match raw.get(1).copied() {
        None => Err("too short"),
        Some(FED_WITHDRAW) if raw.len() == 6 => Ok(FedMessage::Withdraw { issi: u32_at(2) }),
        Some(FED_WITHDRAW) => Err("FED_WITHDRAW of wrong length"),
        Some(FED_PRUNE) if raw.len() == 22 => Ok(FedMessage::Prune {
            id: Uuid::from_bytes(raw[2..18].try_into().expect("checked length")),
            source_issi: u32_at(18),
        }),
        Some(FED_PRUNE) => Err("FED_PRUNE of wrong length"),
        Some(FED_ROUTE) => {
            if raw.len() < 15 {
                return Err("FED_ROUTE too short");
            }
            let n = raw[14] as usize;
            if n == 0 || n > MAX_PATH {
                return Err("FED_ROUTE path length out of range");
            }
            let groups_at = 15 + 8 * n;
            if raw.len() < groups_at || !(raw.len() - groups_at).is_multiple_of(4) {
                return Err("FED_ROUTE of wrong length");
            }
            let path: Vec<u64> = (0..n).map(|i| u64_at(15 + 8 * i)).collect();
            if path[0] != nbr_id {
                return Err("FED_ROUTE path does not start at the server that sent it");
            }
            if path.iter().enumerate().any(|(i, id)| path[..i].contains(id)) {
                return Err("FED_ROUTE path crosses a server twice");
            }
            let groups = raw[groups_at..].chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes"))).collect();
            Ok(FedMessage::Route { issi: u32_at(2), advert: Advert { reg: u64_at(6), path, groups } })
        }
        Some(FED_BTS_HELLO) => Ok(FedMessage::BtsHello { version: raw.get(2).copied().unwrap_or(1) }),
        Some(FED_MS_POS) => parse_ms_pos(raw, nbr_id).map(FedMessage::MsPos),
        Some(FED_EMERGENCY) => parse_emergency(raw, nbr_id).map(FedMessage::Emergency),
        Some(FED_BTS) => parse_bts(raw, nbr_id).map(FedMessage::Bts),
        Some(other) => Ok(FedMessage::Unknown(other)),
    }
}

// ─── Route selection ─────────────────────────────────────────────────────

/// Outcome of `select` for one ISSI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The local registration.
    Local,
    /// The offer received over this negotiated link.
    Remote(ClientId),
    Unreachable,
}

/// Picks the effective route for an ISSI, the same way on every server: the
/// origin with the newest registration wins (tie: the higher server id), and
/// among the offers from that origin the shortest path (tie: the lower
/// neighbour id, then link). The second step ignores `reg`, so a
/// re-registration at the origin does not move the next hop around: its new
/// clock and groups arrive over the shortest path anyway. `local` is the
/// clock of this server's own registration, if any.
pub fn select(
    self_id: u64,
    local: Option<u64>,
    links: &HashMap<ClientId, u64>,
    cands: Option<&HashMap<ClientId, Advert>>,
) -> Choice {
    let offers = || cands.into_iter().flat_map(|c| c.iter());
    let newest_remote = offers().filter_map(|(_, a)| a.path.last().map(|origin| (a.reg, *origin))).max();
    let origin = match newest_remote {
        Some((reg, origin)) if local.is_none_or(|own| (own, self_id) < (reg, origin)) => origin,
        _ if local.is_some() => return Choice::Local,
        _ => return Choice::Unreachable,
    };
    offers()
        .filter(|(_, a)| a.path.last() == Some(&origin))
        .min_by_key(|(link, a)| (a.path.len(), links.get(*link).copied().unwrap_or(u64::MAX), **link))
        .map_or(Choice::Unreachable, |(link, _)| Choice::Remote(*link))
}

// ─── Effective table ─────────────────────────────────────────────────────

/// Whether two effective entries route the same way (link, groups, route).
fn same(a: Option<&Subscriber>, b: Option<&Subscriber>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.client_id == b.client_id && a.groups == b.groups && a.route == b.route,
        _ => false,
    }
}

fn sorted(groups: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut v: Vec<u32> = groups.into_iter().collect();
    v.sort_unstable();
    v
}

/// Replaces the effective route of `issi` (`None`: unreachable). The writer
/// of `inner.subscribers` and of the subscriber part of `inner.group_clients`
/// (the SIP bridge adds its own virtual members; `state::cleanup_client`
/// drops a departed connection's entries and memberships wholesale), which it
/// keeps consistent: a connection is in a group's set exactly while one of
/// the ISSIs routed over it is in that group. So an ISSI moving off a peer
/// link leaves the link in every group another ISSI behind it still uses.
pub fn set_effective(inner: &mut Inner, issi: u32, new: Option<Subscriber>) {
    let old = match &new {
        Some(n) => inner.subscribers.insert(issi, n.clone()),
        None => inner.subscribers.remove(&issi),
    };
    if let Some(old) = old {
        for g in &old.groups {
            let still_used = inner.subscribers.values().any(|s| s.client_id == old.client_id && s.groups.contains(g));
            if still_used { continue; }
            if let Some(members) = inner.group_clients.get_mut(g) {
                members.remove(&old.client_id);
                if members.is_empty() { inner.group_clients.remove(g); }
            }
        }
    }
    if let Some(n) = new {
        for g in n.groups {
            inner.group_clients.entry(g).or_default().insert(n.client_id);
        }
    }
}

/// Re-runs `select` for `issi` between its local registration (if the
/// effective entry is one) and the neighbours' offers, and applies the result.
/// True when the effective route changed.
pub fn reroute(inner: &mut Inner, issi: u32) -> bool {
    let current = inner.subscribers.get(&issi).cloned();
    let local = current.clone().filter(|s| !inner.fed.links.contains_key(&s.client_id));
    let cands = inner.fed.rib_in.get(&issi);
    let new = match select(inner.fed.self_id, local.as_ref().map(|s| s.route.reg), &inner.fed.links, cands) {
        Choice::Local => local.clone(),
        Choice::Remote(link) => cands.and_then(|c| c.get(&link)).map(|a| Subscriber {
            client_id: link,
            groups: a.groups.iter().copied().collect(),
            mode: ClientMode::Peer,
            route: Route { reg: a.reg, path: a.path.clone() },
        }),
        Choice::Unreachable => None,
    };
    if same(current.as_ref(), new.as_ref()) {
        return false;
    }
    if let (Some(_), Some(origin)) = (&local, new.as_ref().and_then(|n| n.route.path.last())) {
        // Last writer wins, as it always has, now network-wide: the ISSI
        // registered again elsewhere (it roamed), so the stale local
        // registration is dropped.
        info!(issi, origin = %format_server_id(*origin), "registration superseded by a newer one at another server");
    }
    set_effective(inner, issi, new);
    true
}

/// What a negotiated link to neighbour `nbr_id` is told about `issi` after
/// its effective route became `eff`: the route with this server in front,
/// or a withdrawal when there is none, when it runs over that very link
/// (poison reverse), when the neighbour is already on it (it would refuse
/// it; also covers a second link to the same server) or when it would grow
/// past `MAX_PATH`.
pub fn fed_update(self_id: u64, nbr_link: ClientId, nbr_id: u64, issi: u32, eff: Option<&Subscriber>) -> Vec<u8> {
    match eff {
        Some(e) if e.client_id != nbr_link && !e.route.path.contains(&nbr_id) && e.route.path.len() < MAX_PATH => {
            let path: Vec<u64> = std::iter::once(self_id).chain(e.route.path.iter().copied()).collect();
            build_route(issi, e.route.reg, &path, &sorted(e.groups.iter().copied()))
        }
        _ => build_withdraw(issi),
    }
}

/// The SUB messages a legacy peer link `peer` gets when the effective entry
/// of `issi` goes from `old` to `new`. It sees an entry only while it does
/// not run over itself (split horizon), and gets the difference: what it
/// sends back is local to this server, so it is never echoed what it said.
pub fn legacy_updates(issi: u32, old: Option<&Subscriber>, new: Option<&Subscriber>, peer: ClientId) -> Vec<Vec<u8>> {
    let before = old.filter(|o| o.client_id != peer);
    let after = new.filter(|n| n.client_id != peer);
    let sub = |t, groups: &[u32]| protocol::build_subscriber_message(t, issi, groups);
    let mut out = Vec::new();
    match (before, after) {
        (None, None) => {}
        // Gone -- unless it is `peer` itself that registered it now.
        (Some(_), None) => if new.is_none() { out.push(sub(protocol::SUB_DEREGISTER, &[])) },
        (None, Some(n)) => {
            out.push(sub(protocol::SUB_REGISTER, &[]));
            if !n.groups.is_empty() { out.push(sub(protocol::SUB_AFFILIATE, &sorted(n.groups.iter().copied()))); }
        }
        (Some(o), Some(n)) => {
            let added = sorted(n.groups.difference(&o.groups).copied());
            let removed = sorted(o.groups.difference(&n.groups).copied());
            if o.route.reg != n.route.reg {
                // A new registration: an older server keeps the groups it
                // had across a REGISTER, so the full set follows.
                out.push(sub(protocol::SUB_REGISTER, &[]));
                if !n.groups.is_empty() { out.push(sub(protocol::SUB_AFFILIATE, &sorted(n.groups.iter().copied()))); }
            } else if !added.is_empty() {
                out.push(sub(protocol::SUB_AFFILIATE, &added));
            }
            if !removed.is_empty() { out.push(sub(protocol::SUB_DEAFFILIATE, &removed)); }
        }
    }
    out
}

/// Queues the updates for a change of `issi`'s effective entry from `old` to
/// what `inner.subscribers` holds now, on every peer link. Done with the lock
/// held: the sends never block, and every link gets every change in the order
/// the table went through them. Basestations and Terminals get nothing, as
/// before. True when a registration appeared (new entry, or a new `reg`):
/// the SMS Center's cue to deliver what it holds for the ISSI.
pub fn publish(inner: &Inner, issi: u32, old: Option<&Subscriber>) -> bool {
    let new = inner.subscribers.get(&issi);
    if same(old, new) {
        return false;
    }
    for (link, client) in &inner.clients {
        if client.mode != ClientMode::Peer {
            continue;
        }
        match inner.fed.links.get(link) {
            Some(&nbr_id) => { let _ = client.tx.send(fed_update(inner.fed.self_id, *link, nbr_id, issi, new)); }
            None => for msg in legacy_updates(issi, old, new, *link) { let _ = client.tx.send(msg); },
        }
    }
    new.is_some_and(|n| old.is_none_or(|o| o.route.reg != n.route.reg))
}

/// The full table a newly attached peer `link` starts from (see
/// `federation::attach_client`): every effective route it may use, as
/// adverts on a negotiated link, as `SUB_REGISTER` + `SUB_AFFILIATE` on a
/// legacy one.
pub fn sync_messages(inner: &Inner, link: ClientId) -> Vec<Vec<u8>> {
    let issis = sorted(inner.subscribers.keys().copied());
    match inner.fed.links.get(&link) {
        Some(&nbr_id) => issis.iter()
            .map(|issi| fed_update(inner.fed.self_id, link, nbr_id, *issi, inner.subscribers.get(issi)))
            .filter(|msg| msg[1] == FED_ROUTE)
            .collect(),
        None => issis.iter().flat_map(|issi| legacy_updates(*issi, None, inner.subscribers.get(issi), link)).collect(),
    }
}

/// Sends `advert` (path as received, sender first) to every link that
/// announced `FED_BTS` support except `except`, with this server in front.
/// A link whose neighbour is already on the path is skipped (it would refuse
/// it), as is a path that would outgrow `MAX_PATH`.
fn relay_bts(inner: &Inner, except: Option<ClientId>, advert: &BtsAdvert) {
    if advert.path.len() >= MAX_PATH { return; }
    let path: Vec<u64> = std::iter::once(inner.fed.self_id).chain(advert.path.iter().copied()).collect();
    let msg = build_bts(&BtsAdvert { path, ..advert.clone() });
    for link in &inner.fed.bts_links {
        if Some(*link) == except { continue; }
        let Some(nbr_id) = inner.fed.links.get(link) else { continue };
        if advert.path.contains(nbr_id) { continue; }
        if let Some(client) = inner.clients.get(link) { let _ = client.tx.send(msg.clone()); }
    }
}

/// Everything a newly announced link to neighbour `nbr_id` is told:
/// this server's own Basestation positions and every live learned one.
fn bts_sync(inner: &Inner, nbr_id: u64) -> Vec<Vec<u8>> {
    let now = crate::telemetry::now_ms();
    let own = inner.fed.bts_local.iter().map(|(key, b)| BtsAdvert {
        key: key.clone(), name: b.name.clone(), lat: b.lat, lon: b.lon, online: true, seq: b.seq,
        path: vec![inner.fed.self_id],
    });
    let learned = inner.fed.bts_remote.iter()
        .filter(|(_, b)| b.online && now.saturating_sub(b.seen_ms) < BTS_STALE_MS)
        .filter(|(_, b)| !b.path.contains(&nbr_id) && b.path.len() < MAX_PATH)
        .map(|((_, key), b)| BtsAdvert {
            key: key.clone(), name: b.name.clone(), lat: b.lat, lon: b.lon, online: true, seq: b.seq,
            path: std::iter::once(inner.fed.self_id).chain(b.path.iter().copied()).collect(),
        });
    own.chain(learned).map(|a| build_bts(&a)).collect()
}

/// Stores a received advert when it is newer than what is held, and passes it
/// on. An advert through ourselves, from ourselves or with an unusable
/// position is ignored; seq ordering makes each server forward each advert at
/// most once, so it cannot loop whatever the topology.
fn accept_bts(inner: &mut Inner, source: ClientId, a: BtsAdvert) {
    let Some(&origin) = a.path.last() else { return };
    if origin == inner.fed.self_id || a.path.contains(&inner.fed.self_id) { return; }
    if a.online && !valid_position(a.lat, a.lon) { return; }
    let key = (origin, a.key.clone());
    if inner.fed.bts_remote.get(&key).is_some_and(|old| old.seq >= a.seq) { return; }
    inner.fed.bts_remote.insert(key, RemoteBts {
        name: a.name.clone(), lat: a.lat, lon: a.lon, online: a.online, seq: a.seq,
        path: a.path.clone(), seen_ms: crate::telemetry::now_ms(),
    });
    relay_bts(inner, Some(source), &a);
}

/// Advertises (or, with `online == false`, withdraws) one of this server's own
/// Basestations to every link that supports it.
pub async fn advertise_bts(state: &Arc<AppState>, key: &str, name: &str, lat: f64, lon: f64, online: bool) {
    let mut inner = state.inner.write().await;
    let now = crate::telemetry::now_ms();
    let prev = inner.fed.bts_local.get(key).map(|b| b.seq).unwrap_or(0);
    let seq = now.max(prev.saturating_add(1));
    if online {
        if !valid_position(lat, lon) { return; }
        inner.fed.bts_local.insert(key.to_string(), LocalBts { name: name.to_string(), lat, lon, seq });
    } else if inner.fed.bts_local.remove(key).is_none() {
        return;
    }
    let advert = BtsAdvert { key: key.to_string(), name: name.to_string(), lat, lon, online, seq, path: Vec::new() };
    if inner.fed.bts_links.is_empty() {
        debug!(key, "Basestation position not relayed: no peer link announced support (needs loop_safe on both servers)");
    }
    relay_bts(&inner, None, &advert);
}

/// Sends an emergency advert (path as received, sender first) to every link that
/// announced support except `except`, this server in front; skip rules as `relay_bts`.
fn relay_em(inner: &Inner, except: Option<ClientId>, advert: &EmAdvert) {
    if advert.path.len() >= MAX_PATH { return; }
    let path: Vec<u64> = std::iter::once(inner.fed.self_id).chain(advert.path.iter().copied()).collect();
    let msg = build_emergency(&EmAdvert { path, ..advert.clone() });
    for link in &inner.fed.em_links {
        if Some(*link) == except { continue; }
        let Some(nbr_id) = inner.fed.links.get(link) else { continue };
        if advert.path.contains(nbr_id) { continue; }
        if let Some(client) = inner.clients.get(link) { let _ = client.tx.send(msg.clone()); }
    }
}

/// Every active alarm held (own and relayed) for a link that just announced support.
fn em_sync(inner: &Inner, nbr_id: u64) -> Vec<Vec<u8>> {
    let now = crate::telemetry::now_ms();
    let own = inner.fed.em_local.iter().map(|(issi, e)| EmAdvert {
        issi: *issi, dest: e.dest, bts: e.bts.clone(), active: true, seq: e.seq, path: vec![inner.fed.self_id],
    });
    let learned = inner.fed.em_remote.iter()
        .filter(|((_, _), e)| e.active && now.saturating_sub(e.seen_ms) < EM_STALE_MS)
        .filter(|(_, e)| !e.path.contains(&nbr_id) && e.path.len() < MAX_PATH)
        .map(|((_, issi), e)| EmAdvert {
            issi: *issi, dest: e.dest, bts: e.bts.clone(), active: true, seq: e.seq,
            path: std::iter::once(inner.fed.self_id).chain(e.path.iter().copied()).collect(),
        });
    own.chain(learned).map(|a| build_emergency(&a)).collect()
}

/// Stores a relayed alarm advert when newer than what is held for (origin, ISSI)
/// and passes it on; ignored when it went through us or is from us.
fn accept_em(inner: &mut Inner, source: ClientId, a: EmAdvert) {
    let Some(&origin) = a.path.last() else { return };
    if origin == inner.fed.self_id || a.path.contains(&inner.fed.self_id) { return; }
    let key = (origin, a.issi);
    if inner.fed.em_remote.get(&key).is_some_and(|old| old.seq >= a.seq) { return; }
    inner.fed.em_remote.insert(key, RemoteEm {
        dest: a.dest, bts: a.bts.clone(), active: a.active, seq: a.seq,
        path: a.path.clone(), seen_ms: crate::telemetry::now_ms(),
    });
    relay_em(inner, Some(source), &a);
}

/// Advertises (or, with `active == false`, clears) one of this server's own
/// emergency alarms to every link that supports it.
pub async fn advertise_emergency(state: &Arc<AppState>, issi: u32, dest: Option<u32>, bts: &str, active: bool) {
    let mut inner = state.inner.write().await;
    let prev = inner.fed.em_local.get(&issi).map(|e| e.seq).unwrap_or(0);
    let seq = crate::telemetry::now_ms().max(prev.saturating_add(1));
    if active {
        inner.fed.em_local.insert(issi, LocalEm { dest, bts: bts.to_string(), seq });
    } else if inner.fed.em_local.remove(&issi).is_none() {
        return;
    }
    let advert = EmAdvert { issi, dest, bts: bts.to_string(), active, seq, path: Vec::new() };
    relay_em(&inner, None, &advert);
}

/// Sends a mobile station advert (path as received, sender first) to every
/// link that announced support except `except`, this server in front; same
/// skip rules as `relay_bts`.
fn relay_ms(inner: &Inner, except: Option<ClientId>, advert: &MsAdvert) {
    if advert.path.len() >= MAX_PATH { return; }
    let path: Vec<u64> = std::iter::once(inner.fed.self_id).chain(advert.path.iter().copied()).collect();
    let msg = build_ms_pos(&MsAdvert { path, ..advert.clone() });
    for link in &inner.fed.ms_links {
        if Some(*link) == except { continue; }
        let Some(nbr_id) = inner.fed.links.get(link) else { continue };
        if advert.path.contains(nbr_id) { continue; }
        if let Some(client) = inner.clients.get(link) { let _ = client.tx.send(msg.clone()); }
    }
}

/// Every position held, for a link that just announced support.
fn ms_sync(inner: &Inner, nbr_id: u64) -> Vec<Vec<u8>> {
    inner.fed.ms_pos.iter()
        .filter(|(_, e)| !e.path.contains(&nbr_id) && e.path.len() < MAX_PATH)
        .map(|(issi, e)| build_ms_pos(&MsAdvert {
            issi: *issi, lat: e.lat, lon: e.lon, at_ms: e.at_ms, station: e.station.clone(),
            path: std::iter::once(inner.fed.self_id).chain(e.path.iter().copied()).collect(),
        }))
        .collect()
}

/// Stores a relayed position when it is newer than the one held for the ISSI
/// and passes it on; ignored when it went through us, is from us, or is no
/// real fix. `at_ms` ordering makes each server forward each fix at most once.
fn accept_ms(inner: &mut Inner, source: ClientId, a: MsAdvert) {
    let Some(&origin) = a.path.last() else { return };
    if origin == inner.fed.self_id || a.path.contains(&inner.fed.self_id) { return; }
    if !valid_position(a.lat, a.lon) { return; }
    if inner.fed.ms_pos.get(&a.issi).is_some_and(|old| old.at_ms >= a.at_ms) { return; }
    inner.fed.ms_pos.insert(a.issi, MsEntry {
        lat: a.lat, lon: a.lon, at_ms: a.at_ms, station: a.station.clone(), path: a.path.clone(),
    });
    relay_ms(inner, Some(source), &a);
}

/// Advertises a mobile station position this server decoded itself. Not for
/// one decoded from an SDS relayed by a peer: its origin advertises that.
pub async fn advertise_ms(state: &Arc<AppState>, issi: u32, lat: f64, lon: f64, at_ms: u64, station: &str) {
    if !valid_position(lat, lon) { return; }
    let mut inner = state.inner.write().await;
    if inner.fed.ms_pos.get(&issi).is_some_and(|old| old.at_ms >= at_ms) { return; }
    inner.fed.ms_pos.insert(issi, MsEntry { lat, lon, at_ms, station: station.to_string(), path: Vec::new() });
    let advert = MsAdvert { issi, lat, lon, at_ms, station: station.to_string(), path: Vec::new() };
    relay_ms(&inner, None, &advert);
}

/// Drops learned positions not refreshed for `BTS_STALE_MS`.
pub fn purge_bts(inner: &mut Inner) {
    let now = crate::telemetry::now_ms();
    inner.fed.em_remote.retain(|_, e| now.saturating_sub(e.seen_ms) < EM_STALE_MS);
    inner.fed.bts_remote.retain(|_, b| now.saturating_sub(b.seen_ms) < BTS_STALE_MS);
}

/// Drops `link`'s offer for `issi`; true if it had one.
fn withdraw_offer(inner: &mut Inner, issi: u32, link: ClientId) -> bool {
    let Some(offers) = inner.fed.rib_in.get_mut(&issi) else { return false };
    let had = offers.remove(&link).is_some();
    if offers.is_empty() {
        inner.fed.rib_in.remove(&issi);
    }
    had
}

/// Handles a `CLASS_FEDERATION` message from `source` (see `router::handle_packet`).
pub async fn handle(state: &Arc<AppState>, source: ClientId, raw: &[u8]) {
    let registered = {
        let mut inner = state.inner.write().await;
        let Some(&nbr_id) = inner.fed.links.get(&source) else {
            // Only possible if a proxy dropped the headers from the 101 alone.
            warn!(%source, "federation message on a link that did not negotiate loop-safe mode; dropped");
            return;
        };
        let msg = match parse(raw, nbr_id) {
            Ok(msg) => msg,
            Err(why) => {
                warn!(%source, why, bytes = raw.len(), "malformed federation message dropped");
                return;
            }
        };
        match msg {
            FedMessage::Route { issi, advert } => {
                let old = inner.subscribers.get(&issi).cloned();
                let too_new = advert.reg > crate::telemetry::now_ms().saturating_add(MAX_FUTURE_MS);
                if too_new {
                    warn!(%source, issi, reg = advert.reg, "route advert registered over a day in the future (peer clock wrong?); taken as a withdrawal");
                }
                if too_new || advert.path.contains(&inner.fed.self_id) {
                    // A path through ourselves is a loop: that link has no
                    // usable route to the ISSI.
                    withdraw_offer(&mut inner, issi, source);
                } else {
                    inner.fed.observe(advert.reg);
                    inner.fed.rib_in.entry(issi).or_default().insert(source, advert);
                }
                // Passed on only when our own choice changed.
                let changed = reroute(&mut inner, issi);
                (changed && publish(&inner, issi, old.as_ref())).then_some(issi)
            }
            FedMessage::Withdraw { issi } => {
                let old = inner.subscribers.get(&issi).cloned();
                let changed = withdraw_offer(&mut inner, issi, source) && reroute(&mut inner, issi);
                (changed && publish(&inner, issi, old.as_ref())).then_some(issi)
            }
            FedMessage::Prune { id, source_issi } => {
                // Only for the turn it was meant for: after a talker change
                // the link may need the call again, and gets it.
                if let Some(call) = inner.calls.get_mut(&id).filter(|c| c.kind == CallKind::Group && c.source_issi == source_issi) {
                    if call.peers.remove(&source) {
                        debug!(%source, uuid = %id, source_issi, "group call pruned off a redundant peer link");
                    }
                }
                None
            }
            FedMessage::BtsHello { version } => {
                inner.fed.bts_links.insert(source);
                let mut msgs = bts_sync(&inner, nbr_id);
                if version >= 2 {
                    inner.fed.ms_links.insert(source);
                    msgs.extend(ms_sync(&inner, nbr_id));
                }
                if version >= 3 {
                    inner.fed.em_links.insert(source);
                    msgs.extend(em_sync(&inner, nbr_id));
                }
                info!(%source, neighbour = %format_server_id(nbr_id), positions = msgs.len(), "federation: peer supports Basestation positions; synced");
                if let Some(client) = inner.clients.get(&source) {
                    for m in msgs { let _ = client.tx.send(m); }
                }
                None
            }
            FedMessage::Emergency(advert) => {
                accept_em(&mut inner, source, advert);
                None
            }
            FedMessage::MsPos(advert) => {
                accept_ms(&mut inner, source, advert);
                None
            }
            FedMessage::Bts(advert) => {
                accept_bts(&mut inner, source, advert);
                None
            }
            FedMessage::Unknown(t) => {
                debug!(%source, msg_type = t, "unknown federation message type ignored");
                None
            }
        }
    };
    if let Some(issi) = registered {
        crate::router::subscriber_registered(state, issi);
    }
}

// ─── Call/SDS de-duplication ─────────────────────────────────────────────

/// How long a (call/SDS uuid, source ISSI) is remembered after it was
/// accepted, changed talker or ended: a copy of it arriving over another peer
/// link within this window is a duplicate, not a new transmission.
pub const CALL_DEDUP_WINDOW: Duration = Duration::from_secs(5);

/// Whether a GROUP_TX, SETUP_REQUEST or SDS header `id` from talker `src`,
/// arriving over peer link `link`, is a copy of one already accepted over
/// another link. Only meant for peer links: a Basestation's own traffic is
/// never second-guessed.
pub fn is_duplicate(inner: &Inner, id: Uuid, src: u32, link: ClientId, now: Instant) -> bool {
    // 1. Exact: the live call/SDS already has this talker, accepted over another link.
    let live = inner.calls.get(&id).map(|c| (c.source_issi, c.owner))
        .or_else(|| inner.sds_routes.get(&id).map(|r| (r.source_issi, r.source_client)));
    if matches!(live, Some((s, owner)) if s == src && owner != link) {
        return true;
    }
    // 2. Recent: a late copy of a turn/call/SDS that just changed talker or ended.
    matches!(inner.recent_calls.get(&(id, src)),
        Some((first, at)) if *first != link && now.duration_since(*at) < CALL_DEDUP_WINDOW)
}

/// Remembers that (`id`, `src`) was accepted from `link` (or ended there).
pub fn note_call(inner: &mut Inner, id: Uuid, src: u32, link: ClientId, now: Instant) {
    inner.recent_calls.insert((id, src), (link, now));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ActiveCall, CallKind, SdsRoute};
    use std::collections::HashSet;

    fn group_call(owner: ClientId, src: u32) -> ActiveCall {
        ActiveCall {
            kind: CallKind::Group, owner, source_issi: src, destination: 91, priority: 0,
            peers: HashSet::new(), started_at: Instant::now(), last_activity_ms: ActiveCall::new_activity(),
        }
    }

    fn advert(path: &[u64], seq: u64) -> BtsAdvert {
        BtsAdvert { key: "bts1".into(), name: "Athens Hill".into(), lat: 37.9917, lon: 23.764, online: true, seq, path: path.to_vec() }
    }

    #[test]
    fn bts_advert_round_trips_and_rejects_bad_paths() {
        let a = advert(&[7, 8], 42);
        assert_eq!(parse(&build_bts(&a), 7), Ok(FedMessage::Bts(a.clone())));
        assert!(parse(&build_bts(&a), 9).is_err(), "must start at the sender");
        assert!(parse(&build_bts(&advert(&[7, 8, 7], 1)), 7).is_err(), "no server twice");
        let mut short = build_bts(&a);
        short.pop();
        assert!(parse(&short, 7).is_err());
        assert_eq!(parse(&build_bts_hello(), 7), Ok(FedMessage::BtsHello { version: HELLO_VERSION }));
        // A version-1 hello (no mobile station support) has no version byte beyond 1.
        assert_eq!(parse(&[CLASS_FEDERATION, FED_BTS_HELLO, 1], 7), Ok(FedMessage::BtsHello { version: 1 }));
    }

    #[test]
    fn ms_advert_round_trips_and_rejects_bad_input() {
        let a = MsAdvert { issi: 4013, lat: 37.99, lon: 23.76, at_ms: 1_700_000_000_000, station: "bts2".into(), path: vec![7, 8] };
        assert_eq!(parse(&build_ms_pos(&a), 7), Ok(FedMessage::MsPos(a.clone())));
        assert!(parse(&build_ms_pos(&a), 9).is_err());
        let mut long = build_ms_pos(&a);
        long.push(0);
        assert!(parse(&long, 7).is_err());
        assert!(parse(&build_ms_pos(&MsAdvert { path: vec![7, 8, 7], ..a }), 7).is_err());
    }

    #[test]
    fn emergency_advert_round_trips_and_rejects_bad_input() {
        let a = EmAdvert { issi: 4013, dest: Some(91), bts: "bts2".into(), active: true, seq: 1_700_000_000_000, path: vec![7, 8] };
        assert_eq!(parse(&build_emergency(&a), 7), Ok(FedMessage::Emergency(a.clone())));
        let none = EmAdvert { dest: None, active: false, ..a.clone() };
        assert_eq!(parse(&build_emergency(&none), 7), Ok(FedMessage::Emergency(none)));
        assert!(parse(&build_emergency(&a), 9).is_err());
        let mut long = build_emergency(&a);
        long.push(0);
        assert!(parse(&long, 7).is_err());
        assert!(parse(&build_emergency(&EmAdvert { path: vec![7, 8, 7], ..a }), 7).is_err());
    }

    #[test]
    fn empty_position_is_not_valid() {
        assert!(!valid_position(0.0, 0.0) && !valid_position(91.0, 0.0) && !valid_position(f64::NAN, 1.0));
        assert!(valid_position(37.99, 23.76) && valid_position(0.0, 23.76));
    }

    #[test]
    fn server_ids_are_random_non_zero_and_round_trip() {
        let (a, b) = (FedState::default().self_id, FedState::default().self_id);
        assert!(a != 0 && b != 0 && a != b);
        assert_eq!(format_server_id(0x00a1_b2c3_d4e5_f607), "00a1b2c3d4e5f607");
        assert_eq!(parse_server_id(&format_server_id(a)), Some(a));
        assert_eq!(parse_server_id("00A1B2C3D4E5F607"), Some(0x00a1_b2c3_d4e5_f607));
        for bad in ["", "a1b2c3d4e5f607", "00a1b2c3d4e5f6071", "00a1b2c3d4e5f60g", "+0a1b2c3d4e5f607"] {
            assert_eq!(parse_server_id(bad), None, "{bad:?}");
        }
        assert!(federation_offered(Some("1")) && federation_offered(Some(" 2 ")));
        assert!(!federation_offered(Some("0")) && !federation_offered(Some("yes")) && !federation_offered(None));
    }

    #[test]
    fn route_and_withdraw_round_trip() {
        let wire = build_route(1001, 0x0102_0304_0506_0708, &[7, 8, 9], &[91, 92]);
        assert_eq!(wire.len(), 15 + 3 * 8 + 2 * 4);
        assert_eq!(&wire[..2], &[0xfe, 0x01]);
        let advert = Advert { reg: 0x0102_0304_0506_0708, path: vec![7, 8, 9], groups: vec![91, 92] };
        assert_eq!(parse(&wire, 7), Ok(FedMessage::Route { issi: 1001, advert }));
        // No groups is fine too.
        assert!(matches!(parse(&build_route(1001, 1, &[7], &[]), 7), Ok(FedMessage::Route { .. })));
        assert_eq!(build_withdraw(1001), vec![0xfe, 0x00, 0xe9, 0x03, 0x00, 0x00]);
        assert_eq!(parse(&build_withdraw(1001), 7), Ok(FedMessage::Withdraw { issi: 1001 }));
        let id = Uuid::new_v4();
        let prune = build_prune(&id, 1001);
        assert_eq!(prune.len(), 22);
        assert_eq!(parse(&prune, 7), Ok(FedMessage::Prune { id, source_issi: 1001 }));
        assert!(parse(&prune[..21], 7).is_err() && parse(&[prune.as_slice(), &[0]].concat(), 7).is_err());
        assert_eq!(parse(&[0xfe, 0x7f, 1, 2, 3], 7), Ok(FedMessage::Unknown(0x7f)));
    }

    #[test]
    fn malformed_federation_messages_are_refused() {
        let good = build_route(1001, 5, &[7, 8], &[91]);
        assert!(parse(&good[..14], 7).is_err(), "header cut short");
        assert!(parse(&good[..good.len() - 1], 7).is_err(), "groups not a multiple of 4");
        assert!(parse(&good[..15 + 8], 7).is_err(), "path cut short");
        let mut zero = build_route(1001, 5, &[7], &[]);
        zero[14] = 0;
        assert!(parse(&zero, 7).is_err(), "empty path");
        let long: Vec<u64> = (1..=MAX_PATH as u64 + 1).collect();
        assert!(parse(&build_route(1001, 5, &long, &[]), 1).is_err(), "path over MAX_PATH");
        assert!(parse(&build_route(1001, 5, &long[..MAX_PATH], &[]), 1).is_ok(), "MAX_PATH itself is fine");
        assert!(parse(&build_route(1001, 5, &[7, 8, 7], &[]), 7).is_err(), "server twice on the path");
        assert!(parse(&build_route(1001, 5, &[8, 7], &[]), 7).is_err(), "not sent by its first server");
        assert!(parse(&[0xfe, 0x00, 1, 2, 3], 7).is_err(), "short withdraw");
        assert!(parse(&[0xfe, 0x00, 1, 2, 3, 4, 5], 7).is_err(), "long withdraw");
        assert!(parse(&[0xfe], 7).is_err());
    }

    #[test]
    fn clock_ticks_past_everything_observed() {
        let mut fed = FedState::default();
        let now = crate::telemetry::now_ms();
        let a = fed.tick();
        assert!(a >= now);
        assert!(fed.tick() > a, "strictly increasing within the same millisecond");
        fed.observe(now + 3_600_000);
        assert!(fed.tick() > now + 3_600_000, "a peer clock running ahead does not make ours lose");
        fed.observe(1);
        assert!(fed.tick() > now + 3_600_000, "never goes back");
    }

    fn offers(list: &[(ClientId, u64, &[u64])]) -> HashMap<ClientId, Advert> {
        list.iter().map(|(link, reg, path)| (*link, Advert { reg: *reg, path: path.to_vec(), groups: vec![] })).collect()
    }

    #[test]
    fn select_prefers_the_newest_registration_then_the_higher_origin() {
        let (l1, l2) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let links = HashMap::from([(l1, 10), (l2, 20)]);
        // Origin 30 registered later than origin 40, even though its path is longer.
        let cands = offers(&[(l1, 200, &[10, 30]), (l2, 100, &[20, 40])]);
        assert_eq!(select(5, None, &links, Some(&cands)), Choice::Remote(l1));
        // Same clock: the higher origin id wins.
        let cands = offers(&[(l1, 100, &[10, 30]), (l2, 100, &[20, 40])]);
        assert_eq!(select(5, None, &links, Some(&cands)), Choice::Remote(l2));
        // A local registration competes like any origin.
        assert_eq!(select(50, Some(100), &links, Some(&cands)), Choice::Local);
        assert_eq!(select(5, Some(100), &links, Some(&cands)), Choice::Remote(l2));
        assert_eq!(select(5, Some(101), &links, Some(&cands)), Choice::Local);
        assert_eq!(select(5, Some(1), &links, None), Choice::Local);
        assert_eq!(select(5, None, &links, None), Choice::Unreachable);
    }

    #[test]
    fn select_takes_the_shortest_path_to_the_winning_origin() {
        let (l1, l2, l3) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let links = HashMap::from([(l1, 10), (l2, 20), (l3, 30)]);
        // The longer path carries a newer clock: still the shorter one, as
        // the origin is the same (no next-hop flapping on a re-register).
        let cands = offers(&[(l1, 200, &[10, 11, 40]), (l2, 100, &[20, 40])]);
        assert_eq!(select(5, None, &links, Some(&cands)), Choice::Remote(l2));
        // Equal length: the lower neighbour id, then the lower link.
        let cands = offers(&[(l3, 100, &[30, 40]), (l2, 100, &[20, 40])]);
        assert_eq!(select(5, None, &links, Some(&cands)), Choice::Remote(l2));
        let (m1, m2) = (Uuid::from_u128(8), Uuid::from_u128(9));
        let twin = HashMap::from([(m1, 10), (m2, 10)]);
        let cands = offers(&[(m2, 100, &[10, 40]), (m1, 100, &[10, 40])]);
        assert_eq!(select(5, None, &twin, Some(&cands)), Choice::Remote(m1));
    }

    fn sub(client: ClientId, groups: &[u32], reg: u64, path: &[u64]) -> Subscriber {
        Subscriber { client_id: client, groups: groups.iter().copied().collect(), mode: ClientMode::Peer, route: Route { reg, path: path.to_vec() } }
    }

    #[test]
    fn fed_update_follows_the_advertising_rules() {
        let (n, other) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let via_other = sub(other, &[92, 91], 100, &[30, 40]);
        assert_eq!(fed_update(5, n, 10, 1001, Some(&via_other)), build_route(1001, 100, &[5, 30, 40], &[91, 92]));
        assert_eq!(fed_update(5, n, 10, 1001, Some(&sub(other, &[], 100, &[]))), build_route(1001, 100, &[5], &[]), "a local one");
        assert_eq!(fed_update(5, n, 10, 1001, None), build_withdraw(1001));
        assert_eq!(fed_update(5, n, 10, 1001, Some(&sub(n, &[], 100, &[10]))), build_withdraw(1001), "poison reverse");
        assert_eq!(fed_update(5, n, 30, 1001, Some(&via_other)), build_withdraw(1001), "the neighbour is on the path");
        let full: Vec<u64> = (100..100 + MAX_PATH as u64).collect();
        assert_eq!(fed_update(5, n, 10, 1001, Some(&sub(other, &[], 1, &full))), build_withdraw(1001), "would exceed MAX_PATH");
        assert!(matches!(parse(&fed_update(5, n, 10, 1001, Some(&sub(other, &[], 1, &full[1..]))), 5), Ok(FedMessage::Route { .. })));
    }

    #[test]
    fn legacy_updates_send_only_what_the_peer_link_does_not_know() {
        let (p, a, b) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let m = |t, g: &[u32]| protocol::build_subscriber_message(t, 1001, g);
        use protocol::{SUB_AFFILIATE as AFF, SUB_DEAFFILIATE as DEAFF, SUB_DEREGISTER as DEREG, SUB_REGISTER as REG};
        assert!(legacy_updates(1001, None, None, p).is_empty());
        assert_eq!(legacy_updates(1001, None, Some(&sub(a, &[], 1, &[])), p), vec![m(REG, &[])]);
        assert_eq!(legacy_updates(1001, None, Some(&sub(a, &[92, 91], 1, &[])), p), vec![m(REG, &[]), m(AFF, &[91, 92])]);
        assert_eq!(legacy_updates(1001, Some(&sub(a, &[91], 1, &[])), None, p), vec![m(DEREG, &[])]);
        // Group changes of the same registration.
        assert_eq!(legacy_updates(1001, Some(&sub(a, &[91, 92], 1, &[])), Some(&sub(a, &[92, 93], 1, &[])), p),
            vec![m(AFF, &[93]), m(DEAFF, &[91])]);
        // Moved to another link, same registration: nothing changes for p.
        assert!(legacy_updates(1001, Some(&sub(a, &[91], 1, &[9])), Some(&sub(b, &[91], 1, &[8, 9])), p).is_empty());
        // A new registration: the full group set follows.
        assert_eq!(legacy_updates(1001, Some(&sub(a, &[91, 92], 1, &[])), Some(&sub(b, &[92], 2, &[])), p),
            vec![m(REG, &[]), m(AFF, &[92]), m(DEAFF, &[91])]);
        // Never echoed what p itself registered, nor told to drop it.
        assert!(legacy_updates(1001, None, Some(&sub(p, &[91], 1, &[])), p).is_empty());
        assert!(legacy_updates(1001, Some(&sub(a, &[91], 1, &[])), Some(&sub(p, &[91], 2, &[])), p).is_empty());
        assert!(legacy_updates(1001, Some(&sub(p, &[91], 1, &[])), None, p).is_empty());
        // ...but told when the ISSI is now reached through us instead.
        assert_eq!(legacy_updates(1001, Some(&sub(p, &[], 1, &[])), Some(&sub(a, &[], 2, &[])), p), vec![m(REG, &[])]);
    }

    #[test]
    fn set_effective_keeps_a_link_in_groups_its_other_issis_use() {
        let (link, bs) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let mut inner = Inner::default();
        set_effective(&mut inner, 1001, Some(sub(link, &[91], 1, &[])));
        set_effective(&mut inner, 1002, Some(sub(link, &[91, 92], 1, &[])));
        // 1001 moves to a Basestation: the link still carries 1002 in 91.
        set_effective(&mut inner, 1001, Some(sub(bs, &[91], 2, &[])));
        assert_eq!(inner.group_clients[&91], HashSet::from([link, bs]));
        set_effective(&mut inner, 1002, None);
        assert_eq!(inner.group_clients[&91], HashSet::from([bs]));
        assert!(!inner.group_clients.contains_key(&92), "an empty group goes");
        assert_eq!(inner.subscribers.len(), 1);
    }

    #[test]
    fn reroute_fails_over_and_supersedes() {
        let (l1, l2, bs) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let mut inner = Inner::default();
        inner.fed.links = HashMap::from([(l1, 10), (l2, 20)]);
        inner.fed.rib_in.insert(1001, offers(&[(l1, 100, &[10, 40]), (l2, 100, &[20, 30, 40])]));
        assert!(reroute(&mut inner, 1001));
        assert_eq!(inner.subscribers[&1001].client_id, l1);
        assert!(!reroute(&mut inner, 1001), "nothing changed");
        inner.fed.rib_in.get_mut(&1001).unwrap().remove(&l1);
        assert!(reroute(&mut inner, 1001));
        assert_eq!(inner.subscribers[&1001].route, Route { reg: 100, path: vec![20, 30, 40] }, "failed over");
        // Registered here again, later: local wins and stays.
        let reg = inner.fed.tick();
        set_effective(&mut inner, 1001, Some(Subscriber { client_id: bs, groups: HashSet::new(), mode: ClientMode::Basestation, route: Route { reg, path: vec![] } }));
        assert!(!reroute(&mut inner, 1001));
        // A newer registration elsewhere supersedes it.
        inner.fed.rib_in.insert(1001, offers(&[(l2, reg + 1, &[20, 30])]));
        assert!(reroute(&mut inner, 1001));
        assert_eq!(inner.subscribers[&1001].client_id, l2);
        inner.fed.rib_in.clear();
        assert!(reroute(&mut inner, 1001));
        assert!(inner.subscribers.is_empty(), "the superseded local registration is gone");
    }

    #[test]
    fn live_call_with_same_talker_from_another_link_is_a_duplicate() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let id = Uuid::new_v4();
        let now = Instant::now();
        let mut inner = Inner::default();
        inner.calls.insert(id, group_call(a, 1001));
        assert!(is_duplicate(&inner, id, 1001, b, now));
        assert!(!is_duplicate(&inner, id, 1001, a, now), "a repeat over the owning link is not a copy");
        assert!(!is_duplicate(&inner, id, 1002, b, now), "another talker is a talker change");
        assert!(!is_duplicate(&inner, Uuid::new_v4(), 1001, b, now));
    }

    #[test]
    fn live_sds_with_same_sender_from_another_link_is_a_duplicate() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let id = Uuid::new_v4();
        let mut inner = Inner::default();
        inner.sds_routes.insert(id, SdsRoute {
            source_client: a, targets: HashSet::new(), source_issi: 1001, destination: 2002,
            created_at: Instant::now(), store_offline: false, lip_only: false, held_header: None,
            no_lip: HashSet::new(), from_no_lip: false,
        });
        assert!(is_duplicate(&inner, id, 1001, b, Instant::now()));
        assert!(!is_duplicate(&inner, id, 1001, a, Instant::now()));
    }

    #[test]
    fn recent_entry_drops_late_copies_within_the_window_only() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let id = Uuid::new_v4();
        let t0 = Instant::now();
        let mut inner = Inner::default();
        note_call(&mut inner, id, 1001, a, t0);
        assert!(is_duplicate(&inner, id, 1001, b, t0 + Duration::from_secs(1)));
        assert!(!is_duplicate(&inner, id, 1001, a, t0 + Duration::from_secs(1)));
        assert!(!is_duplicate(&inner, id, 1001, b, t0 + CALL_DEDUP_WINDOW));
    }
}

/// Whole servers wired together in process: each node is a real `AppState`
/// with one fake Basestation, links are `federation::attach_client` peer
/// connections whose queues are delivered to the far node's router, and
/// `Net::pump` runs the network until it is quiet -- failing if that takes
/// more than `PUMP_LIMIT` messages, i.e. if anything would circulate forever.
#[cfg(test)]
mod mesh_tests {
    use super::*;
    use crate::protocol::{
        build_call_cause, build_circular_call_setup, build_group_tx, build_short_transfer,
        build_subscriber_message, build_traffic_frame, ACELP_CODED_FRAME_BYTES, CALL_ALERT, CALL_GROUP_IDLE,
        CALL_GROUP_TX, CALL_SETUP_REQUEST, CLASS_CALL_CONTROL, CLASS_FRAME, FRAME_SDS_REPORT, FRAME_TRAFFIC_CHANNEL,
        SUB_AFFILIATE, SUB_DEREGISTER, SUB_REGISTER,
    };
    use crate::protocol::ConnVersion;
    use crate::router::handle_packet;
    use crate::state::Client;
    use std::collections::HashSet;
    use tokio::sync::mpsc;

    const PUMP_LIMIT: usize = 2000;

    struct Node {
        state: Arc<AppState>,
        bs: ClientId,
        bs_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    }

    /// One direction of a link: what node `from` queues on its link
    /// `from_link` arrives at node `to` from its link `to_link`.
    struct Wire {
        rx: mpsc::UnboundedReceiver<Vec<u8>>,
        from: usize,
        from_link: ClientId,
        to: usize,
        to_link: ClientId,
        negotiated: bool,
    }

    struct Net {
        nodes: Vec<Node>,
        wires: Vec<Wire>,
    }

    fn connection(mode: ClientMode) -> (Client, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Client { tx, mode, version: ConnVersion::V1, version_announced: false, remote_addr: None, connected_at_ms: 0, username: None }, rx)
    }

    impl Net {
        /// `n` servers, each with `loop_safe` as given and one Basestation.
        async fn new(n: usize, loop_safe: bool) -> Self {
            let mut nodes = Vec::new();
            for _ in 0..n {
                let mut config = crate::config::Config::default();
                config.storage.enabled = false;
                config.sms_center.enabled = false;
                config.federation.loop_safe = loop_safe;
                let state = Arc::new(AppState::new(config, "test.toml".into()).0);
                let (bs_client, bs_rx) = connection(ClientMode::Basestation);
                let bs = Uuid::new_v4();
                crate::federation::attach_client(&state, bs, bs_client, None).await;
                nodes.push(Node { state, bs, bs_rx });
            }
            Net { nodes, wires: Vec::new() }
        }

        async fn id(&self, node: usize) -> u64 {
            self.nodes[node].state.inner.read().await.fed.self_id
        }

        /// Links `a` and `b`, loop-safe when `negotiated` (both ends must
        /// enable it), and lets the initial syncs settle.
        async fn link(&mut self, a: usize, b: usize, negotiated: bool) {
            let (la, lb) = (Uuid::new_v4(), Uuid::new_v4());
            let (ida, idb) = (self.id(a).await, self.id(b).await);
            let (ca, rxa) = connection(ClientMode::Peer);
            let (cb, rxb) = connection(ClientMode::Peer);
            crate::federation::attach_client(&self.nodes[a].state, la, ca, negotiated.then_some(idb)).await;
            crate::federation::attach_client(&self.nodes[b].state, lb, cb, negotiated.then_some(ida)).await;
            self.wires.push(Wire { rx: rxa, from: a, from_link: la, to: b, to_link: lb, negotiated });
            self.wires.push(Wire { rx: rxb, from: b, from_link: lb, to: a, to_link: la, negotiated });
            self.pump().await;
        }

        /// Drops every link between `a` and `b` (whatever is in flight is lost).
        async fn unlink(&mut self, a: usize, b: usize) {
            let (gone, kept): (Vec<Wire>, Vec<Wire>) = std::mem::take(&mut self.wires).into_iter()
                .partition(|w| (w.from, w.to) == (a, b) || (w.from, w.to) == (b, a));
            self.wires = kept;
            for w in gone {
                self.nodes[w.from].state.cleanup_client(w.from_link).await;
            }
            self.pump().await;
        }

        /// Delivers queued messages round-robin, one per wire per pass, until
        /// no wire has anything left; returns how many were delivered.
        async fn pump(&mut self) -> usize {
            let mut delivered = 0;
            loop {
                let mut progressed = false;
                for i in 0..self.wires.len() {
                    let Ok(msg) = self.wires[i].rx.try_recv() else { continue };
                    let w = &self.wires[i];
                    assert!(w.negotiated || msg.first() != Some(&CLASS_FEDERATION), "federation message on a legacy link");
                    let (state, link) = (self.nodes[w.to].state.clone(), w.to_link);
                    handle_packet(state, link, msg).await;
                    delivered += 1;
                    progressed = true;
                    assert!(delivered <= PUMP_LIMIT, "network not quiet after {PUMP_LIMIT} messages: something circulates");
                }
                if !progressed {
                    return delivered;
                }
            }
        }

        /// The Basestation of `node` sends `msg`; the network settles.
        /// Returns how many messages crossed links meanwhile.
        async fn bs_send(&mut self, node: usize, msg: Vec<u8>) -> usize {
            let (state, bs) = (self.nodes[node].state.clone(), self.nodes[node].bs);
            handle_packet(state, bs, msg).await;
            self.pump().await
        }

        async fn register(&mut self, node: usize, issi: u32, groups: &[u32]) {
            self.bs_send(node, build_subscriber_message(SUB_REGISTER, issi, &[])).await;
            if !groups.is_empty() {
                self.bs_send(node, build_subscriber_message(SUB_AFFILIATE, issi, groups)).await;
            }
        }

        /// What the Basestation of `node` was sent since the last call.
        fn heard(&mut self, node: usize) -> Vec<Vec<u8>> {
            std::iter::from_fn(|| self.nodes[node].bs_rx.try_recv().ok()).collect()
        }

        async fn route(&self, node: usize, issi: u32) -> Option<Subscriber> {
            self.nodes[node].state.inner.read().await.subscribers.get(&issi).cloned()
        }

        /// The servers `node` reaches `issi` through (empty: registered there).
        async fn path(&self, node: usize, issi: u32) -> Option<Vec<u64>> {
            self.route(node, issi).await.map(|s| s.route.path)
        }

        /// Every node's effective table and group sets agree with what was
        /// advertised: each one is consistent on its own (`group_clients`
        /// built from the routes), and no route crosses a server twice.
        async fn check_tables(&self) {
            for node in &self.nodes {
                let inner = node.state.inner.read().await;
                for (issi, sub) in &inner.subscribers {
                    let mut seen = HashSet::new();
                    assert!(sub.route.path.iter().all(|id| seen.insert(*id)), "ISSI {issi}: loop in {:?}", sub.route.path);
                    assert!(!sub.route.path.contains(&inner.fed.self_id), "ISSI {issi}: route through ourselves");
                    for g in &sub.groups {
                        assert!(inner.group_clients.get(g).is_some_and(|m| m.contains(&sub.client_id)), "ISSI {issi}: group {g} not routed");
                    }
                }
                for (g, members) in &inner.group_clients {
                    for m in members {
                        assert!(inner.subscribers.values().any(|s| s.client_id == *m && s.groups.contains(g)), "group {g}: stale member");
                    }
                }
            }
        }
    }

    fn of_type(msgs: &[Vec<u8>], class: u8, kind: u8) -> usize {
        msgs.iter().filter(|m| m.len() > 1 && m[0] == class && m[1] == kind).count()
    }

    fn voice(id: &Uuid) -> Vec<u8> {
        build_traffic_frame(id, &[0x11; ACELP_CODED_FRAME_BYTES], &[0x22; ACELP_CODED_FRAME_BYTES])
    }

    #[tokio::test]
    async fn bts_positions_flood_a_ring_once_and_skip_legacy_links() {
        let mut net = Net::new(4, true).await;
        net.link(0, 1, true).await;
        net.link(1, 2, true).await;
        net.link(2, 0, true).await;
        net.link(2, 3, false).await; // an older peer: gets nothing of class federation
        let origin = net.id(0).await;

        advertise_bts(&net.nodes[0].state.clone(), "bts1", "Athens Hill", 37.9917, 23.764, true).await;
        let crossed = net.pump().await;
        assert!(crossed <= 4, "each server forwards an advert at most once, saw {crossed}");
        for n in [1, 2] {
            let inner = net.nodes[n].state.inner.read().await;
            let b = inner.fed.bts_remote.get(&(origin, "bts1".to_string())).expect("learned");
            assert!(b.online && b.name == "Athens Hill" && b.path.last() == Some(&origin));
        }
        assert!(net.nodes[3].state.inner.read().await.fed.bts_remote.is_empty());

        // The empty position is never advertised.
        advertise_bts(&net.nodes[0].state.clone(), "bts0", "", 0.0, 0.0, true).await;
        assert_eq!(net.pump().await, 0);

        // A server that links later is synced; withdrawal reaches everyone.
        advertise_bts(&net.nodes[0].state.clone(), "bts1", "", 0.0, 0.0, false).await;
        net.pump().await;
        for n in [1, 2] {
            let inner = net.nodes[n].state.inner.read().await;
            assert!(!inner.fed.bts_remote[&(origin, "bts1".to_string())].online);
        }
    }

    #[tokio::test]
    async fn ms_positions_flood_a_ring_once_newest_wins_and_skip_legacy_links() {
        let mut net = Net::new(4, true).await;
        net.link(0, 1, true).await;
        net.link(1, 2, true).await;
        net.link(2, 0, true).await;
        net.link(2, 3, false).await;

        advertise_ms(&net.nodes[0].state.clone(), 4013, 37.99, 23.76, 1000, "bts2").await;
        let crossed = net.pump().await;
        assert!(crossed <= 4, "each server forwards a fix at most once, saw {crossed}");
        for n in [1, 2] {
            let inner = net.nodes[n].state.inner.read().await;
            let e = &inner.fed.ms_pos[&4013];
            assert!(e.at_ms == 1000 && e.station == "bts2" && !e.path.is_empty());
        }
        assert!(net.nodes[3].state.inner.read().await.fed.ms_pos.is_empty());

        // 0/0 is never advertised; an older fix never replaces a newer one.
        advertise_ms(&net.nodes[0].state.clone(), 4014, 0.0, 0.0, 1100, "bts2").await;
        advertise_ms(&net.nodes[1].state.clone(), 4013, 38.0, 23.8, 900, "bts3").await;
        assert_eq!(net.pump().await, 0);
        assert_eq!(net.nodes[2].state.inner.read().await.fed.ms_pos[&4013].lat, 37.99);

        // A newer fix from another server wins everywhere.
        advertise_ms(&net.nodes[1].state.clone(), 4013, 38.0, 23.8, 2000, "bts3").await;
        net.pump().await;
        for n in [0, 2] {
            assert_eq!(net.nodes[n].state.inner.read().await.fed.ms_pos[&4013].at_ms, 2000);
        }
    }

    /// An emergency alarm raised on one server is shown on every server -- kept
    /// there until it is cleared at its origin, not only while a call runs --
    /// and loses nothing in a ring (each server forwards each advert once).
    #[tokio::test]
    async fn emergency_alarms_flood_a_ring_stay_until_cleared_and_skip_older_peers() {
        let mut net = Net::new(4, true).await;
        net.link(0, 1, true).await;
        net.link(1, 2, true).await;
        net.link(2, 0, true).await;
        net.link(2, 3, false).await; // a legacy link: nothing of class federation
        {
            let mut t = net.nodes[0].state.telemetry.write().await;
            t.add_test_station("bts2", None, "bts2");
            t.stations.get_mut("bts2").unwrap().emergencies.insert(4013);
        }
        let mut adv = crate::emergency::AdvertState::default();
        crate::emergency::advertise_if_due(&net.nodes[0].state, &mut adv).await;
        let crossed = net.pump().await;
        assert!(crossed <= 4, "each server forwards the advert at most once, saw {crossed}");
        for n in [1, 2] {
            let list = crate::emergency::snapshot(&net.nodes[n].state).await;
            assert_eq!(list.len(), 1, "node {n}");
            assert!(list[0].issi == 4013 && list[0].bts.as_deref().is_some_and(|b| b.starts_with("bts2 @ ")));
        }
        assert!(crate::emergency::snapshot(&net.nodes[3].state).await.is_empty());

        // Unchanged and fresh: nothing is re-sent; cleared: it goes out as cleared.
        crate::emergency::advertise_if_due(&net.nodes[0].state, &mut adv).await;
        assert_eq!(net.pump().await, 0);
        net.nodes[0].state.telemetry.write().await.stations.get_mut("bts2").unwrap().emergencies.clear();
        crate::emergency::advertise_if_due(&net.nodes[0].state, &mut adv).await;
        net.pump().await;
        for n in [1, 2] {
            assert!(crate::emergency::snapshot(&net.nodes[n].state).await.is_empty(), "node {n}");
        }
    }

    #[tokio::test]
    async fn a_relayed_alarm_expires_without_refresh_and_a_new_peer_is_synced() {
        let mut net = Net::new(3, true).await;
        net.link(0, 1, true).await;
        crate::fedroute::advertise_emergency(&net.nodes[0].state.clone(), 4013, Some(91), "bts2", true).await;
        net.pump().await;
        // A server that links later is told what is active.
        net.link(1, 2, true).await;
        assert_eq!(crate::emergency::snapshot(&net.nodes[2].state).await.len(), 1);
        // The origin goes quiet: the relayed alarm times out.
        for n in [1, 2] {
            let mut inner = net.nodes[n].state.inner.write().await;
            for e in inner.fed.em_remote.values_mut() { e.seen_ms = e.seen_ms.saturating_sub(EM_STALE_MS + 1); }
        }
        for n in [1, 2] {
            assert!(crate::emergency::snapshot(&net.nodes[n].state).await.is_empty(), "node {n}");
        }
    }

    #[tokio::test]
    async fn bts_sync_on_hello_gives_a_new_link_what_is_known() {
        let mut net = Net::new(3, true).await;
        net.link(0, 1, true).await;
        advertise_bts(&net.nodes[0].state.clone(), "bts1", "A", 10.0, 20.0, true).await;
        net.pump().await;
        net.link(1, 2, true).await;
        let origin = net.id(0).await;
        assert!(net.nodes[2].state.inner.read().await.fed.bts_remote.contains_key(&(origin, "bts1".to_string())));
    }

    #[tokio::test]
    async fn ring_converges_on_shortest_paths_and_fails_over() {
        let mut net = Net::new(4, true).await;
        for i in 0..4 {
            net.link(i, (i + 1) % 4, true).await;
        }
        let ids = [net.id(0).await, net.id(1).await, net.id(2).await, net.id(3).await];
        net.register(0, 1001, &[91]).await;

        assert_eq!(net.path(0, 1001).await, Some(vec![]));
        assert_eq!(net.path(1, 1001).await, Some(vec![ids[0]]));
        assert_eq!(net.path(3, 1001).await, Some(vec![ids[0]]));
        assert_eq!(net.path(2, 1001).await.unwrap().len(), 2, "opposite side: two hops either way");
        assert!(net.route(2, 1001).await.unwrap().groups.contains(&91));
        net.check_tables().await;

        // The direct link goes: node 1 now gets there the long way round.
        net.unlink(0, 1).await;
        assert_eq!(net.path(1, 1001).await, Some(vec![ids[2], ids[3], ids[0]]));
        assert_eq!(net.path(2, 1001).await, Some(vec![ids[3], ids[0]]));
        net.check_tables().await;

        // Gone from the network once deregistered.
        net.bs_send(0, build_subscriber_message(SUB_DEREGISTER, 1001, &[])).await;
        for node in 0..4 {
            assert!(net.route(node, 1001).await.is_none(), "node {node}");
            assert!(net.nodes[node].state.inner.read().await.fed.rib_in.is_empty(), "node {node}: stale offer");
        }
        net.check_tables().await;
    }

    #[tokio::test]
    async fn ring_partition_withdraws_everything_behind_it() {
        let mut net = Net::new(4, true).await;
        for i in 0..4 {
            net.link(i, (i + 1) % 4, true).await;
        }
        net.register(2, 1002, &[91]).await;
        assert!(net.route(0, 1002).await.is_some());
        net.unlink(1, 2).await;
        assert!(net.route(0, 1002).await.is_some(), "still reachable through node 3");
        net.unlink(2, 3).await;
        for node in [0, 1, 3] {
            assert!(net.route(node, 1002).await.is_none(), "node {node}: cut off");
        }
        assert!(net.route(2, 1002).await.is_some(), "still registered where it is");
        // Linked back: learnt again from the initial sync.
        net.link(2, 3, true).await;
        for node in 0..4 {
            assert!(net.route(node, 1002).await.is_some(), "node {node}");
        }
        net.check_tables().await;
    }

    #[tokio::test]
    async fn ring_group_call_and_private_call_go_round_once() {
        let mut net = Net::new(6, true).await;
        for i in 0..6 {
            net.link(i, (i + 1) % 6, true).await;
        }
        for node in 0..6 {
            net.register(node, 2000 + node as u32, &[91]).await;
        }
        let id = Uuid::new_v4();
        net.bs_send(0, build_group_tx(&id, 2000, 91, 0)).await;
        net.bs_send(0, voice(&id)).await;
        for node in 1..6 {
            let heard = net.heard(node);
            assert_eq!(of_type(&heard, CLASS_CALL_CONTROL, CALL_GROUP_TX), 1, "node {node}");
            assert_eq!(of_type(&heard, CLASS_FRAME, FRAME_TRAFFIC_CHANNEL), 1, "node {node}");
        }
        let call = Uuid::new_v4();
        net.bs_send(1, build_circular_call_setup(&call, 2001, 2004, 0)).await;
        assert_eq!(of_type(&net.heard(4), CLASS_CALL_CONTROL, CALL_SETUP_REQUEST), 1);
        net.check_tables().await;
    }

    /// An emergency group call reaches the dispatch consoles of every server --
    /// even where nobody is in the group -- at priority 15, exactly once, and
    /// settles in a ring and in a mesh. An ordinary call stays on its route.
    #[tokio::test]
    async fn emergency_group_call_reaches_every_servers_consoles_once_in_a_ring_and_a_mesh() {
        for ring in [true, false] {
            let mut net = Net::new(5, true).await;
            for a in 0..5 {
                if ring {
                    net.link(a, (a + 1) % 5, true).await;
                } else {
                    for b in a + 1..5 { net.link(a, b, true).await; }
                }
            }
            // Group 91 has a member on node 0 only; each server has a console listening to nothing.
            net.register(0, 2000, &[91]).await;
            let mut consoles = Vec::new();
            for node in 0..5 {
                let (client, rx) = connection(ClientMode::Basestation);
                let id = Uuid::new_v4();
                let mut inner = net.nodes[node].state.inner.write().await;
                inner.clients.insert(id, client);
                inner.consoles.insert(id);
                consoles.push(rx);
            }
            for node in 0..5 { net.heard(node); }

            let group_tx = |rx: &mut mpsc::UnboundedReceiver<Vec<u8>>| -> Vec<Vec<u8>> {
                std::iter::from_fn(|| rx.try_recv().ok()).filter(|m| m.get(..2) == Some(&[CLASS_CALL_CONTROL, CALL_GROUP_TX])).collect()
            };
            let id = Uuid::new_v4();
            net.bs_send(2, build_group_tx(&id, 2002, 91, 15)).await;
            for (node, rx) in consoles.iter_mut().enumerate() {
                let heard = group_tx(rx);
                assert_eq!(heard.len(), 1, "ring={ring} node {node}");
                assert_eq!(heard[0][26], 15, "priority 15, ring={ring} node {node}");
            }
            // An ordinary call reaches no console.
            net.bs_send(2, build_group_tx(&Uuid::new_v4(), 2002, 91, 0)).await;
            for (node, rx) in consoles.iter_mut().enumerate() {
                assert!(group_tx(rx).is_empty(), "ring={ring} node {node}");
            }
        }
    }

    /// A full mesh of 5 with one member of group 91 behind each server.
    async fn mesh() -> Net {
        let mut net = Net::new(5, true).await;
        for a in 0..5 {
            for b in a + 1..5 {
                net.link(a, b, true).await;
            }
        }
        for node in 0..5 {
            net.register(node, 2000 + node as u32, &[91]).await;
        }
        for node in 0..5 {
            net.heard(node);
        }
        net
    }

    #[tokio::test]
    async fn full_mesh_routes_every_issi_directly() {
        let net = mesh().await;
        for node in 0..5 {
            for other in 0..5 {
                let path = net.path(node, 2000 + other as u32).await.expect("routed");
                let want = if node == other { vec![] } else { vec![net.id(other).await] };
                assert_eq!(path, want, "node {node} to {other}");
            }
        }
        net.check_tables().await;
    }

    #[tokio::test]
    async fn full_mesh_group_call_reaches_every_site_once() {
        let mut net = mesh().await;
        let id = Uuid::new_v4();
        net.bs_send(0, build_group_tx(&id, 2000, 91, 0)).await;
        // Every other server got the call straight from node 0 and pruned
        // the copies the others relayed: voice crosses 4 links, not 16.
        for node in 1..5 {
            let inner = net.nodes[node].state.inner.read().await;
            assert_eq!(inner.calls[&id].peers, HashSet::from([net.nodes[node].bs]), "node {node}");
        }
        assert_eq!(net.bs_send(0, voice(&id)).await, 4);
        net.bs_send(0, voice(&id)).await;
        net.bs_send(0, build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
        assert!(net.heard(0).is_empty(), "the talker's own site hears nothing back");
        for node in 1..5 {
            let heard = net.heard(node);
            assert_eq!(of_type(&heard, CLASS_CALL_CONTROL, CALL_GROUP_TX), 1, "node {node}");
            assert_eq!(of_type(&heard, CLASS_FRAME, FRAME_TRAFFIC_CHANNEL), 2, "node {node}");
            assert_eq!(of_type(&heard, CLASS_CALL_CONTROL, CALL_GROUP_IDLE), 1, "node {node}");
        }
        for node in 0..5 {
            let inner = net.nodes[node].state.inner.read().await;
            assert!(inner.calls.is_empty() && inner.group_floor.is_empty(), "node {node}: call left behind");
        }
    }

    #[tokio::test]
    async fn full_mesh_answer_from_another_site_is_heard_everywhere_once() {
        let mut net = mesh().await;
        let id = Uuid::new_v4();
        net.bs_send(0, build_group_tx(&id, 2000, 91, 0)).await;
        // Same call, the radio behind node 3 answers.
        net.bs_send(3, build_group_tx(&id, 2003, 91, 0)).await;
        net.bs_send(3, voice(&id)).await;
        for node in [1, 2, 4] {
            let heard = net.heard(node);
            assert_eq!(of_type(&heard, CLASS_CALL_CONTROL, CALL_GROUP_TX), 2, "node {node}");
            assert_eq!(of_type(&heard, CLASS_FRAME, FRAME_TRAFFIC_CHANNEL), 1, "node {node}");
        }
        let heard = net.heard(0);
        assert_eq!(of_type(&heard, CLASS_CALL_CONTROL, CALL_GROUP_TX), 1, "the first talker's site hears the answer");
        assert_eq!(of_type(&heard, CLASS_FRAME, FRAME_TRAFFIC_CHANNEL), 1);
    }

    #[tokio::test]
    async fn full_mesh_private_call_and_sds_take_one_path() {
        let mut net = mesh().await;
        let call = Uuid::new_v4();
        net.bs_send(0, build_circular_call_setup(&call, 2000, 2003, 0)).await;
        assert_eq!(of_type(&net.heard(3), CLASS_CALL_CONTROL, CALL_SETUP_REQUEST), 1);
        net.bs_send(3, build_call_cause(CALL_ALERT, &call, 0)).await;
        assert_eq!(of_type(&net.heard(0), CLASS_CALL_CONTROL, CALL_ALERT), 1, "the answer finds its way back");

        let sds = Uuid::new_v4();
        net.bs_send(1, build_short_transfer(&sds, 2001, 2004)).await;
        net.bs_send(1, crate::protocol::build_sds_transfer_frame(&sds, 16, b"hi")).await;
        assert_eq!(net.heard(4).len(), 2, "header and payload, once");
        let mut report = vec![CLASS_FRAME, FRAME_SDS_REPORT];
        report.extend_from_slice(sds.as_bytes());
        report.extend_from_slice(&8u16.to_le_bytes());
        report.push(0);
        net.bs_send(4, report.clone()).await;
        assert_eq!(net.heard(1), vec![report]);
        for node in [0, 2, 3] {
            assert!(net.heard(node).is_empty(), "node {node} is not on the way");
        }

        // A group SDS reaches every member's site once.
        let group_sds = Uuid::new_v4();
        net.bs_send(0, build_short_transfer(&group_sds, 2000, 91)).await;
        for node in 1..5 {
            assert_eq!(net.heard(node).len(), 1, "node {node}");
        }
    }

    #[tokio::test]
    async fn roaming_moves_the_issi_everywhere() {
        let mut net = mesh().await;
        let id3 = net.id(3).await;
        // 2000 shows up at node 3: the newer registration wins everywhere,
        // including at node 0, where the stale local one is dropped.
        net.register(3, 2000, &[92]).await;
        for node in [0, 1, 2, 4] {
            assert_eq!(net.path(node, 2000).await, Some(vec![id3]), "node {node}");
            assert_eq!(net.route(node, 2000).await.unwrap().groups, HashSet::from([91, 92]), "node {node}");
        }
        assert_eq!(net.path(3, 2000).await, Some(vec![]));
        // Node 0's Basestation no longer owns it: its stale DEREGISTER is ignored.
        net.bs_send(0, build_subscriber_message(SUB_DEREGISTER, 2000, &[])).await;
        assert_eq!(net.path(1, 2000).await, Some(vec![id3]));
        // Back at node 0 (re-registers): it moves back.
        net.register(0, 2000, &[]).await;
        let id0 = net.id(0).await;
        for node in 1..5 {
            assert_eq!(net.path(node, 2000).await, Some(vec![id0]), "node {node}");
        }
        net.check_tables().await;
    }

    #[tokio::test]
    async fn concurrent_registrations_of_one_issi_agree_everywhere() {
        let mut net = mesh().await;
        // 3000 registers at nodes 1 and 3 before either hears of the other.
        for node in [1, 3] {
            let (state, bs) = (net.nodes[node].state.clone(), net.nodes[node].bs);
            handle_packet(state, bs, build_subscriber_message(SUB_REGISTER, 3000, &[])).await;
        }
        net.pump().await;
        let mut origins = HashSet::new();
        for node in 0..5 {
            let path = net.path(node, 3000).await.expect("routed");
            origins.insert(match path.last() { Some(id) => *id, None => net.id(node).await });
        }
        assert_eq!(origins.len(), 1, "one winner, the same everywhere: {origins:?}");
        net.check_tables().await;
    }

    #[tokio::test]
    async fn parallel_links_between_two_servers() {
        let mut net = Net::new(2, true).await;
        net.link(0, 1, true).await;
        net.link(0, 1, true).await;
        net.register(0, 1001, &[91]).await;
        let id = Uuid::new_v4();
        net.bs_send(1, build_group_tx(&id, 3000, 91, 0)).await;
        assert_eq!(of_type(&net.heard(0), CLASS_CALL_CONTROL, CALL_GROUP_TX), 1);
        assert!(net.route(1, 1001).await.is_some());
        net.check_tables().await;
    }

    #[tokio::test]
    async fn legacy_peer_hangs_off_the_mesh() {
        let mut net = Net::new(4, true).await;
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            net.link(a, b, true).await;
        }
        // Node 3 dialled in without loop-safe mode (an older server).
        net.link(3, 0, false).await;
        net.register(3, 1003, &[91]).await;
        net.register(1, 1001, &[91]).await;
        let id0 = net.id(0).await;
        assert_eq!(net.path(0, 1003).await, Some(vec![]), "local to the server it hangs off");
        assert_eq!(net.path(2, 1003).await, Some(vec![id0]));
        assert!(net.route(3, 1001).await.is_some(), "learnt as a plain SUB_REGISTER");
        assert!(net.route(3, 1001).await.unwrap().groups.contains(&91));

        let id = Uuid::new_v4();
        net.bs_send(3, build_group_tx(&id, 1003, 91, 0)).await;
        assert_eq!(of_type(&net.heard(1), CLASS_CALL_CONTROL, CALL_GROUP_TX), 1);
        net.bs_send(1, build_subscriber_message(SUB_DEREGISTER, 1001, &[])).await;
        assert!(net.route(3, 1001).await.is_none());
        net.unlink(3, 0).await;
        for node in 0..3 {
            assert!(net.route(node, 1003).await.is_none(), "node {node}");
        }
        net.check_tables().await;
    }

    #[tokio::test]
    async fn legacy_chain_still_relays_registrations() {
        let mut net = Net::new(3, false).await;
        net.link(0, 1, false).await;
        net.link(1, 2, false).await;
        net.register(0, 1001, &[91]).await;
        assert!(net.route(2, 1001).await.unwrap().groups.contains(&91));
        net.bs_send(0, build_subscriber_message(SUB_DEREGISTER, 1001, &[])).await;
        assert!(net.route(2, 1001).await.is_none());
        net.check_tables().await;
    }

    #[tokio::test]
    async fn adverts_that_cannot_be_trusted_are_withdrawals() {
        let mut net = Net::new(2, true).await;
        net.link(0, 1, true).await;
        let (id0, id1) = (net.id(0).await, net.id(1).await);
        let link = net.wires.iter().find(|w| w.to == 0).unwrap().to_link;
        let state = net.nodes[0].state.clone();
        handle_packet(state.clone(), link, build_route(1001, 5, &[id1], &[91])).await;
        assert!(net.route(0, 1001).await.is_some());
        // Our own id on the path: a loop, so that link has no route.
        handle_packet(state.clone(), link, build_route(1001, 6, &[id1, id0, 77], &[91])).await;
        assert!(net.route(0, 1001).await.is_none());
        // A clock over a day ahead: refused too.
        let far = crate::telemetry::now_ms() + 2 * MAX_FUTURE_MS;
        handle_packet(state.clone(), link, build_route(1002, far, &[id1], &[])).await;
        assert!(net.route(0, 1002).await.is_none());
        // A SUB message on a loop-safe link is ignored.
        handle_packet(state.clone(), link, build_subscriber_message(SUB_REGISTER, 1003, &[])).await;
        assert!(net.route(0, 1003).await.is_none());
        // A federation message on a plain connection is dropped.
        let bs = net.nodes[0].bs;
        handle_packet(state.clone(), bs, build_route(1004, 5, &[id1], &[])).await;
        assert!(net.route(0, 1004).await.is_none());
        net.check_tables().await;
    }
}
