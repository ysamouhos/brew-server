use std::fmt;
use uuid::Uuid;

/// Brew protocol version implemented by this server, advertised to and expected
/// from clients via the `X-Brew-Version` HTTP header during discovery. See the
/// TETRA Homebrew Protocol specification, "Endpoint, authentication".
pub const BREW_PROTOCOL_VERSION: u8 = 1;

pub const CLASS_SUBSCRIBER: u8 = 0xf0;
pub const CLASS_CALL_CONTROL: u8 = 0xf1;
pub const CLASS_FRAME: u8 = 0xf2;
pub const CLASS_ERROR: u8 = 0xf3;
pub const CLASS_SERVICE: u8 = 0xf4;
/// Not part of the Brew spec: only exchanged between brew-servers that
/// negotiated loop-safe federation (`X-Brew-Federation`); never sent to anyone
/// else. Route adverts cannot ride on `CLASS_SUBSCRIBER`: its group list runs
/// to the end of the message, so an older peer would read any added field as
/// groups. Kept clear of 0xf5 upwards, left for the spec. Layout in
/// `fedroute::FedMessage`.
pub const CLASS_FEDERATION: u8 = 0xfe;

/// `CLASS_FEDERATION` types.
pub const FED_WITHDRAW: u8 = 0;
pub const FED_ROUTE: u8 = 1;
pub const FED_PRUNE: u8 = 2;
/// Capability announcement: the sender understands `FED_BTS`. Sent once per
/// negotiated link; `FED_BTS` goes only to links that announced it.
pub const FED_BTS_HELLO: u8 = 3;
/// A Basestation position advert, flooded loop-safely (see `fedroute`).
pub const FED_BTS: u8 = 4;
/// A mobile station position advert (needs a `FED_BTS_HELLO` of version >= 2).
pub const FED_MS_POS: u8 = 5;
/// An emergency alarm advert (needs a `FED_BTS_HELLO` of version >= 3).
pub const FED_EMERGENCY: u8 = 6;

pub const SUB_DEREGISTER: u8 = 0;
pub const SUB_REGISTER: u8 = 1;
pub const SUB_REREGISTER: u8 = 2;
pub const SUB_AFFILIATE: u8 = 8;
pub const SUB_DEAFFILIATE: u8 = 9;

pub const CALL_GROUP_TX: u8 = 2;
pub const CALL_GROUP_IDLE: u8 = 3;
pub const CALL_SETUP_REQUEST: u8 = 4;
pub const CALL_SETUP_ACCEPT: u8 = 5;
pub const CALL_SETUP_REJECT: u8 = 6;
pub const CALL_ALERT: u8 = 7;
pub const CALL_CONNECT_REQUEST: u8 = 8;
pub const CALL_CONNECT_CONFIRM: u8 = 9;
pub const CALL_RELEASE: u8 = 10;
pub const CALL_SHORT_TRANSFER: u8 = 11;
pub const CALL_SIMPLEX_GRANTED: u8 = 12;
pub const CALL_SIMPLEX_IDLE: u8 = 13;

/// Disconnect causes (ETSI EN 300 392-2 clause 14.8.18) for the cause byte of
/// call-control messages this server originates itself.
/// "Called party not reachable": a private call's destination is not
/// registered anywhere this server can route to.
pub const CAUSE_CALLED_PARTY_NOT_REACHABLE: u8 = 3;
/// "SwMI requested disconnection": the network ended the call, here because
/// a participant's connection dropped.
pub const CAUSE_SWMI_REQUESTED_DISCONNECTION: u8 = 14;
/// "Call restoration of the other user failed": a radio that left its
/// Basestation mid-call did not restore the call on the new one in time.
pub const CAUSE_CALL_RESTORATION_OF_THE_OTHER_USER_FAILED: u8 = 19;

pub const FRAME_TRAFFIC_CHANNEL: u8 = 0;
pub const FRAME_SDS_TRANSFER: u8 = 1;
pub const FRAME_SDS_REPORT: u8 = 2;
/// In-call DTMF: one ASCII digit ('0'-'9', '*', '#', 'A'-'D') per frame,
/// `length_bits = 8`, 1-byte payload. Not in the original Brew spec this
/// server was built against, but sent by at least one real client (nexus-bs,
/// a FlowStation-derived Basestation). The frame header shape is identical
/// to `FRAME_TRAFFIC_CHANNEL`/`FRAME_SDS_*`, so no new parser is needed --
/// only routing for this frame type.
pub const FRAME_DTMF: u8 = 3;

/// `CLASS_SERVICE` type carrying a per-ISSI RSSI report:
/// `{"issi": N, "rssi_dbfs": F}`. Not in the original Brew spec this server
/// was built against, but sent by at least one real client (nexus-bs).
pub const SERVICE_RSSI: u8 = 0x10;

/// `CLASS_SERVICE` type this server pushes to dispatch consoles: the active
/// emergencies, `{"emergencies":[{"issi":N,"dest":G|null}]}` (see `emergency`).
pub const SERVICE_EMERGENCY: u8 = 0x11;

/// `CLASS_SERVICE` type a dispatch console sends to edit the ISSI blacklist:
/// `{"action":"block"|"unblock","issi":N}` (see `blacklist`).
pub const SERVICE_BLACKLIST_CMD: u8 = 0x12;

/// `CLASS_SERVICE` type this server pushes to dispatch consoles: the blacklist,
/// `{"blacklist":[N,...],"can_edit":bool}`, or `{"error":"..."}` for a refused edit.
pub const SERVICE_BLACKLIST: u8 = 0x13;

/// `CLASS_SERVICE` type a Basestation sends after connecting, and again when
/// its cell config changes: the cells it serves, for the carrier registry
/// (see `carriers`). Only sent to a server that offered the `bts-info`
/// extension on the WebSocket upgrade.
pub const SERVICE_BTS_INFO: u8 = 0x14;

/// `CLASS_SERVICE` type a serving Basestation sends when a radio announced a
/// cell change (U-PREPARE) without a registration: the server relays it to
/// the Basestation serving the target carrier (see `handover`).
pub const SERVICE_HO_PREPARE: u8 = 0x15;

/// As `SERVICE_HO_PREPARE`, but the U-PREPARE carried a registration (MM
/// SDU) for the target cell, which the target Basestation processes.
pub const SERVICE_HO_FORWARD_REG: u8 = 0x16;

/// `CLASS_SERVICE` type answering `SERVICE_HO_PREPARE` / `SERVICE_HO_FORWARD_REG`:
/// from the target Basestation via the server to the serving one, or from the
/// server itself when there is no target or it did not answer in time.
pub const SERVICE_HO_RESULT: u8 = 0x17;

/// `CLASS_SERVICE` type a Basestation sends after registering an ISSI: the
/// cell (carrier) the radio is on, for stations that serve several cells on
/// one connection (see `carriers`). Only sent to a server that offered the
/// `ms-cell` extension.
pub const SERVICE_MS_CELL: u8 = 0x18;

/// `CLASS_SERVICE` type the server sends to the Basestation an ISSI was
/// registered on when it registers on another Basestation without a handover
/// prepared here (`{"issi":N}`): the radio has left. Only sent to a
/// Basestation that announced the `ms-moved` capability (see `handover`).
pub const SERVICE_MS_MOVED: u8 = 0x19;

/// `CLASS_SERVICE` type a Basestation sends when a radio left it, naming the
/// category of cell reselection (`{"issi":N,"type":"announced_type_3"}`, see
/// `roaming_log::Reselection`). Only sent to a server that offered the
/// `ms-reselection` extension.
pub const SERVICE_MS_RESELECTION: u8 = 0x1A;

/// `CLASS_SERVICE` type for restoring an individual call on another
/// Basestation: the Basestation a radio restores the call on asks
/// (`{"issi":N,"peer":N}`) and the server answers with the call
/// (`uuid`, `duplex`, `priority`, `floor`) or without `uuid` when it has
/// none (see `call_restore`). Only used with a Basestation that announced the
/// `call-restore` capability.
pub const SERVICE_CALL_RESTORE: u8 = 0x1B;

/// `CLASS_SERVICE` type a Basestation sends when a radio registered on it
/// with a plain roaming location update (`{"issi":N}`): by TS 100 392-2
/// clause 16.4 a) a radio with a circuit mode call registers with "service
/// restoration roaming" instead, so it has no call to restore (see
/// `call_restore`). Only used with a Basestation that announced the
/// `call-restore` capability.
pub const SERVICE_CALL_NOT_RESTORED: u8 = 0x1C;

/// `CLASS_SERVICE` type for a call between two radios of one Basestation when
/// one of them goes to another Basestation: the server asks the Basestation
/// holding it (`{"issi":N,"peer":N}`) and that one answers with the call it
/// bridged to Brew (`uuid`, `calling`, `duplex`, `priority`, `floor`) or
/// without `uuid` (see `call_restore`). Only used with a Basestation that
/// announced the `call-handoff` capability.
pub const SERVICE_CALL_HANDOFF: u8 = 0x1E;

/// `CLASS_SERVICE` type the server sends to a Basestation with the cells of the
/// other Basestations it knows, online and offline, with what they broadcast
/// (`{"cells":[...]}`), so the Basestation can show and pick its neighbours.
/// Only sent to a Basestation that announced the `cell-directory` capability
/// (see `carriers::directory_updates`).
pub const SERVICE_CELL_DIRECTORY: u8 = 0x1D;

/// `CLASS_SERVICE` type the server sends to a Basestation with who may use
/// which group (`{"groups":[{"gssi":N,"mode":"allow"|"block","issis":[..],
/// "rx_only":bool,"rx_only_issis":[..]}]}`), so it refuses a group
/// attachment or a request to talk itself. Only sent to a Basestation that
/// announced the `group-access` capability (see `group_access::updates`).
pub const SERVICE_GROUP_ACCESS: u8 = 0x1F;

/// `CLASS_SERVICE` types of the `mni` extension (Multi Network, see
/// `visitors` and docs/Brew-Extensions.md 12b): this server's network and the
/// linked ones (server -> station), the network of a radio, of groups, and
/// of a call (both ways).
pub const SERVICE_NETWORKS: u8 = 0x20;
pub const SERVICE_MS_NETWORK: u8 = 0x21;
pub const SERVICE_GROUP_NETWORK: u8 = 0x22;
pub const SERVICE_CALL_NETWORK: u8 = 0x23;

/// `CLASS_SERVICE` type a server's Multi Network link sends (as a
/// Basestation) to a server that offered `issi-check`, asking whether ISSIs
/// are in use there before registering them (`{"query":[..]}`); the server
/// answers with the same type (`{"taken":[..],"free":[..]}`). See `netlink`.
pub const SERVICE_ISSI_CHECK: u8 = 0x24;

/// Builds a `CLASS_SERVICE` message: class, type, NUL-terminated JSON.
pub fn build_service(service_type: u8, json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + json.len());
    out.push(CLASS_SERVICE);
    out.push(service_type);
    out.extend_from_slice(json.as_bytes());
    out.push(0);
    out
}

#[derive(Debug, Clone)]
pub enum BrewMessage {
    Subscriber(SubscriberMessage),
    CallControl(CallControlMessage),
    Frame(FrameMessage),
    Error(ErrorMessage),
    Service(ServiceMessage),
}

#[derive(Debug, Clone)]
pub struct SubscriberMessage {
    pub msg_type: u8,
    pub issi: u32,
    pub timestamp: u64,
    pub fraction: u32,
    pub groups: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct GroupTransmission {
    pub source: u32,
    pub destination: u32,
    pub priority: u8,
    pub access: u8,
    pub service: u16,
    /// SS-TPI talking-party mnemonic, decoded from the optional trailing
    /// `mnemonic[34]` field added in Brew protocol version 1. `None` when the
    /// peer speaks version 0 or sends an empty mnemonic.
    pub mnemonic: Option<String>,
}

/// Circuit-mode (individual) call setup payload, `struct BrewCircularCall`.
///
/// Wire layout (all little-endian, packed), from the specification:
///   source(4) destination(4) number[32] priority(1) service(1) mode(1)
///   duplex(1) method(1) communication(1) grant(1) permission(1) timeout(1)
///   ownership(1) queued(1) mnemonic[34]
///
/// The trailing `mnemonic[34]` exists only from protocol version 1 and only on
/// `CALL_STATE_SETUP_REQUEST`. `CALL_STATE_CONNECT_REQUEST` carries the same
/// struct truncated exactly at `offsetof(BrewCircularCall, mnemonic)` — i.e. no
/// mnemonic — in every version.
#[derive(Debug, Clone)]
pub struct CircularCall {
    pub source: u32,
    pub destination: u32,
    pub number: String,
    pub priority: u8,
    /// TETRA basic service byte. Relayed untouched; the router reads it only
    /// to recognise an ambience-listening call setup (see `router`).
    pub service: u8,
    /// Duplex (true) or simplex call, as set up; read only to restore the
    /// call on another Basestation (see `call_restore`).
    pub duplex: bool,
    pub mnemonic: Option<String>,
}

/// `offsetof(struct BrewCircularCall, mnemonic)`: the 40-byte
/// source/destination/number head plus 11 single-byte fields (priority,
/// service, mode, duplex, method, communication, grant, permission, timeout,
/// ownership, queued). This is exactly the payload length of a
/// `CONNECT_REQUEST` and the pre-mnemonic length of a `SETUP_REQUEST`.
const CIRCULAR_CALL_BASE_LEN: usize = 4 + 4 + 32 + 11;
/// `sizeof(struct BrewGroupTransmission)` without the version-1 mnemonic:
/// source(4) destination(4) priority(1) access(1) service(2).
const GROUP_TX_BASE_LEN: usize = 4 + 4 + 1 + 1 + 2;
/// Size in bytes of the SS-TPI `mnemonic[34]` field.
const MNEMONIC_FIELD_LEN: usize = 34;

/// Negotiated Brew protocol version for a single WebSocket connection.
///
/// Real clients (e.g. Basestation) do **not** carry `X-Brew-Version` on the
/// WebSocket handshake — only, optionally, on the preceding HTTP discovery GET.
/// The version is therefore treated as a per-connection property that starts at
/// `V0` and is promoted to `V1` either by an explicit discovery header or lazily
/// when a message is observed whose length can only belong to a v1 layout (a
/// call-control payload that includes the `mnemonic[34]` tail). Once promoted,
/// a connection never falls back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnVersion {
    #[default]
    V0,
    V1,
}

impl ConnVersion {
    pub fn as_u8(self) -> u8 {
        match self { ConnVersion::V0 => 0, ConnVersion::V1 => 1 }
    }
    pub fn has_mnemonic(self) -> bool {
        matches!(self, ConnVersion::V1)
    }
    /// Promotes from a discovery `X-Brew-Version` header value, if trustworthy.
    pub fn from_header_value(v: Option<u8>) -> Self {
        match v { Some(x) if x >= 1 => ConnVersion::V1, _ => ConnVersion::V0 }
    }
}

/// `raw` as a connection that negotiated `version` may receive it. Brew v1
/// appends the SS-TPI `mnemonic[34]` to CALL_GROUP_TX and CALL_SETUP_REQUEST
/// only (CONNECT_REQUEST is cut at the mnemonic offset in every version); a
/// v0 recipient gets those two cut back to their v0 length, so a v1 talker
/// name never reaches a parser that does not expect it. Only ever strips: a
/// v0 message goes to a v1 recipient unchanged, which parses it fine.
pub fn adapt_to_version(raw: &[u8], version: ConnVersion) -> std::borrow::Cow<'_, [u8]> {
    use std::borrow::Cow;
    if version.has_mnemonic() || raw.len() < 2 || raw[0] != CLASS_CALL_CONTROL {
        return Cow::Borrowed(raw);
    }
    // 18 = class + call state + 16-byte call UUID, ahead of the payload.
    let v0_len = 18 + match raw[1] {
        CALL_GROUP_TX => GROUP_TX_BASE_LEN,
        CALL_SETUP_REQUEST => CIRCULAR_CALL_BASE_LEN,
        _ => return Cow::Borrowed(raw),
    };
    if raw.len() > v0_len { Cow::Owned(raw[..v0_len].to_vec()) } else { Cow::Borrowed(raw) }
}

#[derive(Debug, Clone)]
pub enum CallPayload {
    GroupTransmission(GroupTransmission),
    CircularCall(CircularCall),
    Cause(u8),
    Empty,
    ShortTransfer { source: u32, destination: u32 },
    Raw(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct CallControlMessage {
    pub call_state: u8,
    pub identifier: Uuid,
    pub payload: CallPayload,
}

#[derive(Debug, Clone)]
pub struct FrameMessage {
    pub frame_type: u8,
    pub identifier: Uuid,
    pub length_bits: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ErrorMessage {
    pub error_type: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ServiceMessage {
    pub service_type: u8,
    pub json_data: String,
}

#[derive(Debug)]
pub enum ParseError {
    TooShort(usize),
    UnknownClass(u8),
    InvalidUtf8,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "Brew packet too short: {n} bytes"),
            Self::UnknownClass(c) => write!(f, "unknown Brew class 0x{c:02x}"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 service payload"),
        }
    }
}

impl std::error::Error for ParseError {}

fn u16le(data: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([data[o], data[o + 1]])
}

fn u32le(data: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
}

fn u64le(data: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(data[o..o + 8].try_into().expect("checked length"))
}

/// Decodes an SS-TPI mnemonic name as carried inside Brew (protocol version 1),
/// following ETSI EN 300 392-9 clause 8.4.2, table 17:
///   - Octet 0: bit 7 = 0, bits 6-0 = text coding scheme
///   - Octet 1: length of the following text in bits
///   - Octet 2+: encoded character data
///
/// Only the coding schemes practically used by Basestation/Basestation are
/// decoded to text: 0x00 (ISO 8859-1 / 8-bit) and the 7-bit GSM-like packing
/// (scheme 0x01) described in ETSI EN 300 392-2 clause 29.5.4. Unknown schemes
/// return the raw bytes rendered as lossy UTF-8 so information is not silently
/// dropped. Returns `None` for an all-zero / empty field.
pub fn decode_mnemonic(field: &[u8]) -> Option<String> {
    if field.len() < 2 {
        return None;
    }
    let scheme = field[0] & 0x7f;
    let len_bits = field[1] as usize;
    if len_bits == 0 {
        return None;
    }
    let text = &field[2..];

    let decoded = match scheme {
        // 8-bit schemes: one octet per character. Coding scheme 0 is the raw
        // 8-bit default; treat it as ISO 8859-1.
        0x00 => {
            let nchars = len_bits / 8;
            let take = nchars.min(text.len());
            text[..take].iter().map(|&b| b as char).collect::<String>()
        }
        // 7-bit packed alphabet (ETSI EN 300 392-2, 29.5.4).
        0x01 => decode_7bit_packed(text, len_bits),
        // Unknown / other coding schemes: best-effort lossy rendering.
        _ => {
            let nbytes = len_bits.div_ceil(8);
            let take = nbytes.min(text.len());
            String::from_utf8_lossy(&text[..take]).into_owned()
        }
    };

    let trimmed = decoded.trim_end_matches(['\0', ' ']).to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

/// Unpacks a 7-bit packed character string (septets, LSB-first) into text.
fn decode_7bit_packed(data: &[u8], len_bits: usize) -> String {
    let nchars = len_bits / 7;
    let mut out = String::with_capacity(nchars);
    let mut bit_pos = 0usize;
    for _ in 0..nchars {
        let byte_idx = bit_pos / 8;
        let bit_off = bit_pos % 8;
        if byte_idx >= data.len() {
            break;
        }
        let mut value = (data[byte_idx] >> bit_off) as u16;
        if bit_off > 1 && byte_idx + 1 < data.len() {
            value |= (data[byte_idx + 1] as u16) << (8 - bit_off);
        }
        let septet = (value & 0x7f) as u8;
        out.push(septet as char);
        bit_pos += 7;
    }
    out
}

/// Extracts a NUL-terminated ASCII field of fixed width into an owned String.
fn ascii_field(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).into_owned()
}

/// Parses a Brew message using the legacy version-agnostic path. Retained for
/// callers and tests that do not track a connection version; assumes the most
/// capable layout (v1) so an explicit mnemonic tail is still decoded when
/// present. Prefer [`parse_with_version`] on real connections.
pub fn parse(data: &[u8]) -> Result<BrewMessage, ParseError> {
    parse_with_version(data, ConnVersion::V1).map(|(msg, _)| msg)
}

/// Parses a Brew message in the context of a connection's negotiated version and
/// reports the version implied by this message. The returned `ConnVersion` is
/// the max of the input version and any version lazily detected from the
/// message length (mirroring how Basestation resolves the version from message
/// content when no `X-Brew-Version` handshake header is present). Callers should
/// store `max(previous, returned)` as the connection's version.
pub fn parse_with_version(data: &[u8], version: ConnVersion) -> Result<(BrewMessage, ConnVersion), ParseError> {
    if data.len() < 2 {
        return Err(ParseError::TooShort(data.len()));
    }

    match data[0] {
        CLASS_SUBSCRIBER => parse_subscriber(data).map(|m| (m, version)),
        CLASS_CALL_CONTROL => parse_call_control(data, version),
        CLASS_FRAME => parse_frame(data).map(|m| (m, version)),
        CLASS_ERROR => Ok((BrewMessage::Error(ErrorMessage {
            error_type: data[1],
            data: data[2..].to_vec(),
        }), version)),
        CLASS_SERVICE => parse_service(data).map(|m| (m, version)),
        c => Err(ParseError::UnknownClass(c)),
    }
}

fn parse_subscriber(data: &[u8]) -> Result<BrewMessage, ParseError> {
    if data.len() < 18 {
        return Err(ParseError::TooShort(data.len()));
    }

    let mut groups = Vec::new();
    let mut o = 18;
    while o + 4 <= data.len() {
        groups.push(u32le(data, o));
        o += 4;
    }

    Ok(BrewMessage::Subscriber(SubscriberMessage {
        msg_type: data[1],
        issi: u32le(data, 2),
        timestamp: u64le(data, 6),
        fraction: u32le(data, 14),
        groups,
    }))
}

fn parse_call_control(data: &[u8], version: ConnVersion) -> Result<(BrewMessage, ConnVersion), ParseError> {
    if data.len() < 18 {
        return Err(ParseError::TooShort(data.len()));
    }

    let id = Uuid::from_bytes(data[2..18].try_into().expect("checked length"));
    let payload = &data[18..];
    // Version lazily resolved from this message; promoted to V1 if the payload
    // length can only correspond to a v1 (mnemonic-bearing) layout.
    let mut detected = version;

    let parsed_payload = match data[1] {
        CALL_GROUP_TX => {
            if payload.len() < GROUP_TX_BASE_LEN {
                return Err(ParseError::TooShort(data.len()));
            }
            // A GROUP_TX whose payload extends past the fixed head carries the
            // v1 mnemonic[34]. Its presence alone proves the peer speaks v1, so
            // we promote regardless of the previously-assumed version. A v0 peer
            // sends exactly GROUP_TX_BASE_LEN bytes and no mnemonic.
            let has_tail = payload.len() > GROUP_TX_BASE_LEN;
            if has_tail {
                detected = ConnVersion::V1;
            }
            let mnemonic = if has_tail {
                payload.get(GROUP_TX_BASE_LEN..GROUP_TX_BASE_LEN + MNEMONIC_FIELD_LEN)
                    .or(payload.get(GROUP_TX_BASE_LEN..))
                    .and_then(decode_mnemonic)
            } else {
                None
            };
            CallPayload::GroupTransmission(GroupTransmission {
                source: u32le(payload, 0),
                destination: u32le(payload, 4),
                priority: payload[8],
                access: payload[9],
                service: u16le(payload, 10),
                mnemonic,
            })
        }
        CALL_SETUP_REQUEST => {
            if payload.len() < CIRCULAR_CALL_BASE_LEN {
                return Err(ParseError::TooShort(data.len()));
            }
            // SETUP_REQUEST is the only state that carries the mnemonic[34], and
            // only from v1. A payload longer than the pre-mnemonic base means v1.
            let has_tail = payload.len() >= CIRCULAR_CALL_BASE_LEN + MNEMONIC_FIELD_LEN;
            if has_tail {
                detected = ConnVersion::V1;
            }
            let mnemonic = if has_tail {
                payload.get(CIRCULAR_CALL_BASE_LEN..CIRCULAR_CALL_BASE_LEN + MNEMONIC_FIELD_LEN)
                    .and_then(decode_mnemonic)
            } else {
                None
            };
            CallPayload::CircularCall(CircularCall {
                source: u32le(payload, 0),
                destination: u32le(payload, 4),
                number: ascii_field(&payload[8..40]),
                priority: payload[40],
                service: payload[41],
                duplex: payload[43] != 0,
                mnemonic,
            })
        }
        CALL_CONNECT_REQUEST => {
            // CONNECT_REQUEST is the circular struct truncated exactly at the
            // mnemonic offset in every version — never carries a mnemonic.
            if payload.len() < CIRCULAR_CALL_BASE_LEN {
                return Err(ParseError::TooShort(data.len()));
            }
            CallPayload::CircularCall(CircularCall {
                source: u32le(payload, 0),
                destination: u32le(payload, 4),
                number: ascii_field(&payload[8..40]),
                priority: payload[40],
                service: payload[41],
                duplex: payload[43] != 0,
                mnemonic: None,
            })
        }
        CALL_GROUP_IDLE | CALL_SETUP_REJECT | CALL_RELEASE => {
            if payload.is_empty() {
                return Err(ParseError::TooShort(data.len()));
            }
            CallPayload::Cause(payload[0])
        }
        CALL_SETUP_ACCEPT | CALL_ALERT => CallPayload::Empty,
        CALL_SHORT_TRANSFER => {
            if payload.len() < 8 {
                return Err(ParseError::TooShort(data.len()));
            }
            CallPayload::ShortTransfer {
                source: u32le(payload, 0),
                destination: u32le(payload, 4),
            }
        }
        _ => CallPayload::Raw(payload.to_vec()),
    };

    Ok((BrewMessage::CallControl(CallControlMessage {
        call_state: data[1],
        identifier: id,
        payload: parsed_payload,
    }), detected))
}

fn parse_frame(data: &[u8]) -> Result<BrewMessage, ParseError> {
    if data.len() < 20 {
        return Err(ParseError::TooShort(data.len()));
    }

    Ok(BrewMessage::Frame(FrameMessage {
        frame_type: data[1],
        identifier: Uuid::from_bytes(data[2..18].try_into().expect("checked length")),
        length_bits: u16le(data, 18),
        data: data[20..].to_vec(),
    }))
}

fn parse_service(data: &[u8]) -> Result<BrewMessage, ParseError> {
    let raw = &data[2..];
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    let json_data = std::str::from_utf8(&raw[..end])
        .map_err(|_| ParseError::InvalidUtf8)?
        .to_owned();
    Ok(BrewMessage::Service(ServiceMessage {
        service_type: data[1],
        json_data,
    }))
}


pub fn build_call_cause(call_state: u8, id: &Uuid, cause: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(19);
    out.push(CLASS_CALL_CONTROL);
    out.push(call_state);
    out.extend_from_slice(id.as_bytes());
    out.push(cause);
    out
}

/// Builds a call-control message with no payload: `CALL_SETUP_ACCEPT` and
/// `CALL_ALERT` are defined with none (real Brew clients -- see e.g.
/// FlowStation's `net_brew::protocol::parse_frame` -- special-case these two
/// as `// No extra payload`). This server's own parser is lenient about
/// `CALL_CONNECT_CONFIRM`'s shape, but real clients are not (see
/// `build_call_connect_confirm`) -- do not use this builder for it. Used by
/// the SIP<->Brew bridge to drive a private call's accept/ring/answer
/// handshake from the server side (there is no Brew client on the SIP leg to
/// have sent one).
pub fn build_call_control_empty(call_state: u8, id: &Uuid) -> Vec<u8> {
    let mut out = Vec::with_capacity(18);
    out.push(CLASS_CALL_CONTROL);
    out.push(call_state);
    out.extend_from_slice(id.as_bytes());
    out
}

/// Builds `CALL_CONNECT_CONFIRM`, which -- unlike `CALL_SETUP_ACCEPT`/
/// `CALL_ALERT` -- is *not* an empty-payload message: it carries a 2-byte
/// `grant`/`permission` pair (real clients reject anything shorter; see e.g.
/// FlowStation's `net_brew::protocol::BrewCircularGrant` and its
/// `CALL_STATE_CONNECT_CONFIRM => if payload_data.len() < 2 { return
/// Err(TooShort) }`). `0, 0` (no restriction) is the value this bridge sends,
/// since it has no real grant/permission semantics of its own to convey.
pub fn build_call_connect_confirm(id: &Uuid, grant: u8, permission: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.push(CLASS_CALL_CONTROL);
    out.push(CALL_CONNECT_CONFIRM);
    out.extend_from_slice(id.as_bytes());
    out.push(grant);
    out.push(permission);
    out
}

/// Builds a `CLASS_SUBSCRIBER` message (`SUB_REGISTER`/`SUB_DEREGISTER`/
/// `SUB_AFFILIATE`/`SUB_DEAFFILIATE`/...). `timestamp`/`fraction` are left
/// zero: nothing in this server's own parsing or the federation full-sync
/// that uses this builder reads them, only the LIP/position decoders on the
/// SDS side care about a wire timestamp, and those never touch this class.
pub fn build_subscriber_message(msg_type: u8, issi: u32, groups: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(18 + groups.len() * 4);
    out.push(CLASS_SUBSCRIBER);
    out.push(msg_type);
    out.extend_from_slice(&issi.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // timestamp
    out.extend_from_slice(&0u32.to_le_bytes()); // fraction
    for g in groups {
        out.extend_from_slice(&g.to_le_bytes());
    }
    out
}

/// Builds a `CALL_SHORT_TRANSFER` (SDS header) exactly as Basestation/
/// FlowStation send it: uuid, source ISSI, destination ISSI and a 32-byte
/// zero-filled external `number` field (58 bytes total). Used by the SMS
/// Center to originate a stored SDS towards its destination.
pub fn build_short_transfer(id: &Uuid, source: u32, destination: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(58);
    out.push(CLASS_CALL_CONTROL);
    out.push(CALL_SHORT_TRANSFER);
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&source.to_le_bytes());
    out.extend_from_slice(&destination.to_le_bytes());
    out.extend_from_slice(&[0u8; 32]);
    out
}

/// Builds a `FRAME_SDS_TRANSFER` carrying the SDS user data that follows a
/// `CALL_SHORT_TRANSFER` with the same uuid.
pub fn build_sds_transfer_frame(id: &Uuid, length_bits: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + data.len());
    out.push(CLASS_FRAME);
    out.push(FRAME_SDS_TRANSFER);
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&length_bits.to_le_bytes());
    out.extend_from_slice(data);
    out
}

pub fn raw_peer_pair(payload: &CallPayload) -> Option<(u32, u32)> {
    let CallPayload::Raw(raw) = payload else { return None };
    if raw.len() < 8 { return None; }
    Some((u32le(raw, 0), u32le(raw, 4)))
}

/// Bit count of one ACELP-coded TETRA speech frame (30ms @ 8kHz), per
/// ETSI EN 300 395-2. This is the codec's own native unit (what
/// `transcode::acelp` encodes/decodes one call at a time), packed
/// big-endian-bit into `ACELP_CODED_FRAME_BYTES` with trailing zero padding
/// -- NOT the on-wire `FRAME_TRAFFIC_CHANNEL` unit, see
/// `build_traffic_frame`.
pub const ACELP_CODED_FRAME_BITS: u16 = 137;
/// `ceil(ACELP_CODED_FRAME_BITS / 8)`.
pub const ACELP_CODED_FRAME_BYTES: usize = 18;
/// PCM samples per ACELP frame (30ms @ 8kHz).
pub const ACELP_PCM_SAMPLES: usize = 240;

/// One `FRAME_TRAFFIC_CHANNEL` payload carries TWO ACELP-coded subframes
/// (60ms of speech), not one: TETRA's TCH/S burst structure interleaves a
/// pair of 137-bit coded frames per over-the-air delivery, and every real
/// Basestation (confirmed against FlowStation's `net_brew::entity`
/// `handle_voice_frame`/`handle_ul_voice`, which calls this the "STE"
/// format) sends and expects that same pairing on the Brew wire: 1 header
/// byte (0x00 = normal speech) followed by 35 bytes packing the two
/// subframes' 274 bits back-to-back, MSB-first, with the packed *pair*
/// treated as one unit -- not two independently-padded 18-byte codec
/// blocks concatenated (which would leave 7 wasted padding bits after each
/// subframe instead of 6 at the very end). Sending single-subframe
/// 18-byte-payload frames (this server's original, wrong assumption) gets
/// silently discarded by a real Basestation (`data.len() < 36` -> dropped
/// with a warning, never reaching the radio) and receiving them the same
/// way misreads a real Basestation's paired frames as one corrupt subframe
/// per message -- garbled audio at exactly half the real frame rate.
pub const STE_VOICE_PAYLOAD_BYTES: usize = 36;
const STE_VOICE_PACKED_BYTES: usize = STE_VOICE_PAYLOAD_BYTES - 1; // 35: the two subframes' 274 bits
const STE_VOICE_TOTAL_BITS: usize = 2 * ACELP_CODED_FRAME_BITS as usize; // 274

/// Unpacks the first `n_bits` MSB-first bits of `packed` into one bit (0/1)
/// per output byte -- the inverse of `pack_bits_msb`.
fn unpack_bits_msb(packed: &[u8], n_bits: usize) -> Vec<u8> {
    (0..n_bits).map(|i| (packed[i / 8] >> (7 - i % 8)) & 1).collect()
}

/// Packs `bits` (one bit, 0 or nonzero, per input byte) MSB-first into
/// bytes, zero-padding the trailing partial byte -- the inverse of
/// `unpack_bits_msb`.
fn pack_bits_msb(bits: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &b) in bits.iter().enumerate() {
        if b != 0 {
            out[i / 8] |= 1 << (7 - i % 8);
        }
    }
    out
}

/// Packs two codec-native 18-byte ACELP subframes into one 36-byte STE Brew
/// voice-frame payload (see `STE_VOICE_PAYLOAD_BYTES`): strips each
/// subframe's own 7 trailing pad bits, concatenates the two 137-bit
/// payloads back-to-back, and repacks that combined 274-bit stream with a
/// single 6-bit pad at the very end, prefixed by the STE header byte
/// (`0x00`, "normal speech frame" -- the only value FlowStation's own
/// encoder ever sends).
pub fn pack_ste_voice_payload(
    sub1: &[u8; ACELP_CODED_FRAME_BYTES],
    sub2: &[u8; ACELP_CODED_FRAME_BYTES],
) -> [u8; STE_VOICE_PAYLOAD_BYTES] {
    let bits_per_subframe = ACELP_CODED_FRAME_BITS as usize;
    let mut bits = Vec::with_capacity(STE_VOICE_TOTAL_BITS);
    bits.extend_from_slice(&unpack_bits_msb(sub1, bits_per_subframe));
    bits.extend_from_slice(&unpack_bits_msb(sub2, bits_per_subframe));
    let packed = pack_bits_msb(&bits);
    let mut out = [0u8; STE_VOICE_PAYLOAD_BYTES];
    out[0] = 0x00; // STE header: normal speech frame
    out[1..].copy_from_slice(&packed);
    out
}

/// Inverse of `pack_ste_voice_payload`: unpacks a 36-byte STE payload back
/// into the two subframes in the codec's own native (individually
/// 7-bit-padded) 18-byte representation, ready for `AcelpDecoder::decode`.
/// The STE header byte (`payload[0]`) is not inspected here -- this server
/// has no use for the (vendor-specific) control bits it might carry beyond
/// "normal speech frame", the only value ever observed on real traffic.
pub fn unpack_ste_voice_payload(
    payload: &[u8; STE_VOICE_PAYLOAD_BYTES],
) -> ([u8; ACELP_CODED_FRAME_BYTES], [u8; ACELP_CODED_FRAME_BYTES]) {
    let bits_per_subframe = ACELP_CODED_FRAME_BITS as usize;
    let packed: &[u8; STE_VOICE_PACKED_BYTES] = payload[1..].try_into().expect("payload is STE_VOICE_PAYLOAD_BYTES");
    let bits = unpack_bits_msb(packed, STE_VOICE_TOTAL_BITS);
    let sub1 = pack_bits_msb(&bits[..bits_per_subframe]);
    let sub2 = pack_bits_msb(&bits[bits_per_subframe..]);
    let mut out1 = [0u8; ACELP_CODED_FRAME_BYTES];
    let mut out2 = [0u8; ACELP_CODED_FRAME_BYTES];
    out1[..sub1.len()].copy_from_slice(&sub1);
    out2[..sub2.len()].copy_from_slice(&sub2);
    (out1, out2)
}

/// Builds a server-originated `CALL_SETUP_REQUEST` toward a registered Brew
/// subscriber, used to originate a call from the SIP side (there is no Brew
/// client on that leg to have sent one). `number`/the trailing single-byte
/// fields are left zeroed: only source/destination/priority are meaningful
/// for this bridge, and `CircularCall` parsing ignores the rest.
pub fn build_circular_call_setup(id: &Uuid, source: u32, destination: u32, priority: u8) -> Vec<u8> {
    build_circular_call(CALL_SETUP_REQUEST, id, source, destination, priority)
}

/// Builds a server-originated `CALL_CONNECT_REQUEST`: what this bridge sends
/// to the originating ISSI of a Brew-originated (MS-calling-out) call once
/// the SIP/PSTN side answers (see `sip::bridge::on_sip_response`'s `200`
/// case). This is *not* `CALL_CONNECT_CONFIRM` -- confirmed against
/// FlowStation's cc_bs: `CALL_CONNECT_CONFIRM` is explicitly ignored for a
/// call where the MS is the calling party
/// (`fsm_on_network_circuit_connect_confirm` checks `calling_over_brew` and
/// returns early otherwise); `CALL_CONNECT_REQUEST` is what
/// `fsm_on_network_circuit_connect_request` expects for exactly this
/// direction, and is what actually builds and sends the over-the-air
/// D-CONNECT that flips the MS's UI out of "calling...".
pub fn build_circular_connect_request(id: &Uuid, source: u32, destination: u32, priority: u8) -> Vec<u8> {
    build_circular_call(CALL_CONNECT_REQUEST, id, source, destination, priority)
}

/// Shared builder for the two server-originated `CircularCall`-payload
/// messages above. `duplex`=1 and `method`=1 (full duplex, hook signalling)
/// on both: real hardware (confirmed against FlowStation's cc_bs) presents
/// `duplex=0`/`method=0` as a simplex PTT-style call with no real accept
/// gesture or working duplex audio, not a normal phone call. `mode`=0 (TchS
/// speech) and `communication`=0 (P2p -- the only individual-call variant
/// TETRA CMCE defines; there is no separate "phone"/"PABX" type) are
/// correctly zero.
fn build_circular_call(call_state: u8, id: &Uuid, source: u32, destination: u32, priority: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 16 + CIRCULAR_CALL_BASE_LEN);
    out.push(CLASS_CALL_CONTROL);
    out.push(call_state);
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&source.to_le_bytes());
    out.extend_from_slice(&destination.to_le_bytes());
    out.extend_from_slice(&[0u8; 32]); // number[32]: unused for a SIP-originated call
    out.push(priority);
    // service, mode, duplex, method, communication, grant, permission, timeout, ownership, queued
    out.extend_from_slice(&[0, 0, 1, 1, 0, 0, 0, 0, 0, 0]);
    out
}

/// Builds a server-originated `CALL_GROUP_TX`, used to seize the group floor
/// on behalf of a SIP caller bridged into a Brew group call.
pub fn build_group_tx(id: &Uuid, source: u32, destination: u32, priority: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 16 + GROUP_TX_BASE_LEN);
    out.push(CLASS_CALL_CONTROL);
    out.push(CALL_GROUP_TX);
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&source.to_le_bytes());
    out.extend_from_slice(&destination.to_le_bytes());
    out.push(priority);
    out.push(0); // access
    out.extend_from_slice(&0u16.to_le_bytes()); // service
    out
}

/// Builds a `FRAME_TRAFFIC_CHANNEL` carrying two ACELP-coded speech
/// subframes (60ms), STE-packed exactly as a real Basestation sends/expects
/// -- see `STE_VOICE_PAYLOAD_BYTES`'s doc comment for why this is two
/// subframes and not one.
pub fn build_traffic_frame(
    id: &Uuid,
    sub1: &[u8; ACELP_CODED_FRAME_BYTES],
    sub2: &[u8; ACELP_CODED_FRAME_BYTES],
) -> Vec<u8> {
    let payload = pack_ste_voice_payload(sub1, sub2);
    let mut out = Vec::with_capacity(20 + STE_VOICE_PAYLOAD_BYTES);
    out.push(CLASS_FRAME);
    out.push(FRAME_TRAFFIC_CHANNEL);
    out.extend_from_slice(id.as_bytes());
    // Matches FlowStation's own convention (`length_bits: ste_data.len() * 8`):
    // the STE payload's total bit length including its header byte, not the
    // 274 bits of actual codec content within it. Receivers (including this
    // server's own) don't currently act on this field either way.
    out.extend_from_slice(&((STE_VOICE_PAYLOAD_BYTES * 8) as u16).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

/// Builds a `FRAME_DTMF` carrying one digit (see `FRAME_DTMF`'s doc comment
/// for the wire shape this mirrors).
pub fn build_dtmf_frame(id: &Uuid, digit: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(21);
    out.push(CLASS_FRAME);
    out.push(FRAME_DTMF);
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.push(digit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_transfer_and_sds_frame_round_trip() {
        let id = Uuid::new_v4();
        let st = build_short_transfer(&id, 1001, 2002);
        assert_eq!(st.len(), 58);
        match parse(&st).unwrap() {
            BrewMessage::CallControl(cc) => {
                assert_eq!(cc.call_state, CALL_SHORT_TRANSFER);
                assert_eq!(cc.identifier, id);
                assert!(matches!(cc.payload, CallPayload::ShortTransfer { source: 1001, destination: 2002 }));
            }
            other => panic!("unexpected {other:?}"),
        }
        let fr = build_sds_transfer_frame(&id, 40, &[0x82, 0x04, 0x07, 0x01, b'H']);
        match parse(&fr).unwrap() {
            BrewMessage::Frame(f) => {
                assert_eq!(f.frame_type, FRAME_SDS_TRANSFER);
                assert_eq!(f.identifier, id);
                assert_eq!(f.length_bits, 40);
                assert_eq!(f.data, vec![0x82, 0x04, 0x07, 0x01, b'H']);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn build_call_control_empty_round_trips_as_empty_payload() {
        let id = Uuid::new_v4();
        for state in [CALL_SETUP_ACCEPT, CALL_ALERT] {
            let wire = build_call_control_empty(state, &id);
            let BrewMessage::CallControl(cc) = parse(&wire).unwrap() else { panic!() };
            assert_eq!(cc.call_state, state);
            assert_eq!(cc.identifier, id);
            assert!(matches!(cc.payload, CallPayload::Empty));
        }
    }

    /// Real clients (e.g. FlowStation's `net_brew::protocol`) reject
    /// CALL_CONNECT_CONFIRM outright if it carries fewer than 2 payload
    /// bytes (grant, permission) -- unlike CALL_SETUP_ACCEPT/CALL_ALERT,
    /// which are genuinely empty. This pins the wire shape so a future
    /// change can't quietly regress back to the empty-payload bug.
    #[test]
    fn build_call_connect_confirm_carries_grant_and_permission() {
        let id = Uuid::new_v4();
        let wire = build_call_connect_confirm(&id, 1, 2);
        assert_eq!(wire.len(), 20, "18-byte header + 2-byte grant/permission");
        assert_eq!(&wire[18..20], &[1, 2]);
    }

    #[test]
    fn setup_request_parses_the_service_byte() {
        // Header: CLASS_CALL_CONTROL, CALL_SETUP_REQUEST, uuid[16]; then the
        // circular payload with service=9 (ambience listening) at offset 41.
        let mut wire = vec![CLASS_CALL_CONTROL, CALL_SETUP_REQUEST];
        wire.extend_from_slice(Uuid::new_v4().as_bytes());
        let mut payload = vec![0u8; CIRCULAR_CALL_BASE_LEN];
        payload[0..4].copy_from_slice(&1001u32.to_le_bytes()); // source
        payload[4..8].copy_from_slice(&2002u32.to_le_bytes()); // destination
        payload[40] = 3; // priority
        payload[41] = 9; // service
        wire.extend_from_slice(&payload);
        let BrewMessage::CallControl(cc) = parse(&wire).unwrap() else { panic!() };
        let CallPayload::CircularCall(c) = cc.payload else { panic!() };
        assert_eq!((c.source, c.destination, c.priority, c.service), (1001, 2002, 3, 9));
    }

    #[test]
    fn build_subscriber_message_round_trips() {
        let wire = build_subscriber_message(SUB_REGISTER, 4013, &[1001, 1002]);
        let BrewMessage::Subscriber(msg) = parse(&wire).unwrap() else { panic!() };
        assert_eq!(msg.msg_type, SUB_REGISTER);
        assert_eq!(msg.issi, 4013);
        assert_eq!(msg.groups, vec![1001, 1002]);
    }

    #[test]
    fn build_dtmf_frame_round_trips() {
        let id = Uuid::new_v4();
        let wire = build_dtmf_frame(&id, b'5');
        assert_eq!(wire.len(), 21);
        let BrewMessage::Frame(frame) = parse(&wire).unwrap() else { panic!() };
        assert_eq!(frame.frame_type, FRAME_DTMF);
        assert_eq!(frame.identifier, id);
        assert_eq!(frame.length_bits, 8);
        assert_eq!(frame.data, vec![b'5']);
    }

    /// Pins the fix for a real-world bug: an inbound SIP-bridged call could
    /// only be picked up via PTT, never the terminal's actual accept/green
    /// button, and had no working duplex audio once "answered" that way.
    /// Root cause (confirmed against FlowStation's cc_bs): `duplex=0`
    /// presents the call as simplex with a PTT-style TransmissionGrant, and
    /// `method=0` selects non-hook signalling. Both must be 1.
    #[test]
    fn build_circular_call_setup_sends_duplex_and_hook_signalling() {
        let id = Uuid::new_v4();
        let wire = build_circular_call_setup(&id, 1001, 4013, 0);
        // Trailer starts right after header(18) + source(4) + destination(4)
        // + number[32] + priority(1).
        let trailer = 18 + 4 + 4 + 32 + 1;
        assert_eq!(wire[trailer], 0, "service");
        assert_eq!(wire[trailer + 1], 0, "mode (TchS speech)");
        assert_eq!(wire[trailer + 2], 1, "duplex must be 1 (duplex, not simplex/PTT)");
        assert_eq!(wire[trailer + 3], 1, "method must be 1 (hook signalling: requires explicit answer)");
        assert_eq!(wire[trailer + 4], 0, "communication (P2p, the only individual-call variant TETRA defines)");

        let BrewMessage::CallControl(cc) = parse(&wire).unwrap() else { panic!() };
        let CallPayload::CircularCall(c) = cc.payload else { panic!() };
        assert_eq!(c.source, 1001);
        assert_eq!(c.destination, 4013);
    }

    /// Pins the fix for a real-world bug: an MS-originated call bridged out
    /// to SIP stayed stuck in "calling..." even after the PSTN side answered.
    /// Root cause (confirmed against FlowStation's cc_bs): CALL_CONNECT_CONFIRM
    /// is explicitly ignored for a call where the MS is the calling party;
    /// CALL_CONNECT_REQUEST is what actually drives the MS's D-CONNECT.
    #[test]
    fn build_circular_connect_request_has_correct_call_state_and_duplex() {
        let id = Uuid::new_v4();
        let wire = build_circular_connect_request(&id, 0, 4013, 0);
        assert_eq!(wire[1], CALL_CONNECT_REQUEST, "must be CONNECT_REQUEST, not CONNECT_CONFIRM");
        let trailer = 18 + 4 + 4 + 32 + 1;
        assert_eq!(wire[trailer + 2], 1, "duplex must be 1");
        assert_eq!(wire[trailer + 3], 1, "method must be 1 (hook signalling)");

        let BrewMessage::CallControl(cc) = parse(&wire).unwrap() else { panic!() };
        assert_eq!(cc.call_state, CALL_CONNECT_REQUEST);
        let CallPayload::CircularCall(c) = cc.payload else { panic!() };
        assert_eq!(c.destination, 4013, "destination is the ISSI being told the call connected");
    }

    #[test]
    fn parses_group_tx() {
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_GROUP_TX];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&1001u32.to_le_bytes());
        p.extend_from_slice(&91u32.to_le_bytes());
        p.push(3);
        p.push(0);
        p.extend_from_slice(&0u16.to_le_bytes());

        let BrewMessage::CallControl(cc) = parse(&p).unwrap() else { panic!() };
        assert_eq!(cc.identifier, id);
        let CallPayload::GroupTransmission(gt) = cc.payload else { panic!() };
        assert_eq!(gt.source, 1001);
        assert_eq!(gt.destination, 91);
    }

    #[test]
    fn v0_group_tx_has_no_mnemonic_and_stays_v0() {
        // A v0 peer sends exactly the base GROUP_TX with no mnemonic tail.
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_GROUP_TX];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&1001u32.to_le_bytes());
        p.extend_from_slice(&91u32.to_le_bytes());
        p.push(3);
        p.push(0);
        p.extend_from_slice(&0u16.to_le_bytes());

        let (msg, detected) = parse_with_version(&p, ConnVersion::V0).unwrap();
        assert_eq!(detected, ConnVersion::V0, "v0-length message must not promote");
        let BrewMessage::CallControl(cc) = msg else { panic!() };
        let CallPayload::GroupTransmission(gt) = cc.payload else { panic!() };
        assert_eq!(gt.mnemonic, None);
    }

    #[test]
    fn v1_group_tx_promotes_connection() {
        // A GROUP_TX carrying the mnemonic tail proves the peer is v1, even when
        // the connection was previously assumed v0.
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_GROUP_TX];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&1001u32.to_le_bytes());
        p.extend_from_slice(&91u32.to_le_bytes());
        p.push(3);
        p.push(0);
        p.extend_from_slice(&0u16.to_le_bytes());
        let mut mnem = vec![0x00u8, 24];
        mnem.extend_from_slice(b"BOB");
        mnem.resize(34, 0);
        p.extend_from_slice(&mnem);

        let (msg, detected) = parse_with_version(&p, ConnVersion::V0).unwrap();
        assert_eq!(detected, ConnVersion::V1, "mnemonic tail must promote to v1");
        let BrewMessage::CallControl(cc) = msg else { panic!() };
        let CallPayload::GroupTransmission(gt) = cc.payload else { panic!() };
        assert_eq!(gt.mnemonic.as_deref(), Some("BOB"));
    }

    #[test]
    fn connect_request_never_has_mnemonic() {
        // CONNECT_REQUEST is truncated at the mnemonic offset in every version.
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_CONNECT_REQUEST];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&5001u32.to_le_bytes());
        p.extend_from_slice(&6002u32.to_le_bytes());
        p.extend_from_slice(&[0u8; 32]);
        p.push(1);
        p.extend_from_slice(&[0u8; 10]);

        let (msg, detected) = parse_with_version(&p, ConnVersion::V1).unwrap();
        assert_eq!(detected, ConnVersion::V1);
        let BrewMessage::CallControl(cc) = msg else { panic!() };
        let CallPayload::CircularCall(c) = cc.payload else { panic!() };
        assert_eq!(c.mnemonic, None);
    }

    #[test]
    fn header_seeded_v1_still_parses_v0_length_messages() {
        // A connection seeded v1 by the discovery header must not choke on a
        // shorter (mnemonic-less) GROUP_TX; it simply yields no mnemonic.
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_GROUP_TX];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&7u32.to_le_bytes());
        p.extend_from_slice(&8u32.to_le_bytes());
        p.push(1);
        p.push(0);
        p.extend_from_slice(&0u16.to_le_bytes());

        let (msg, _detected) = parse_with_version(&p, ConnVersion::V1).unwrap();
        let BrewMessage::CallControl(cc) = msg else { panic!() };
        let CallPayload::GroupTransmission(gt) = cc.payload else { panic!() };
        assert_eq!(gt.mnemonic, None);
    }

    #[test]
    fn parses_traffic_frame() {
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_FRAME, FRAME_TRAFFIC_CHANNEL];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&274u16.to_le_bytes());
        p.extend_from_slice(&[0x80; 36]);

        let BrewMessage::Frame(frame) = parse(&p).unwrap() else { panic!() };
        assert_eq!(frame.identifier, id);
        assert_eq!(frame.length_bits, 274);
        assert_eq!(frame.data.len(), 36);
    }

    #[test]
    fn decodes_8bit_mnemonic() {
        // scheme 0x00 (8-bit), 5 chars = 40 bits, "HELLO"
        let mut field = vec![0x00, 40];
        field.extend_from_slice(b"HELLO");
        assert_eq!(decode_mnemonic(&field).as_deref(), Some("HELLO"));
    }

    #[test]
    fn empty_mnemonic_is_none() {
        assert_eq!(decode_mnemonic(&[0x00, 0]), None);
        assert_eq!(decode_mnemonic(&[]), None);
        assert_eq!(decode_mnemonic(&[0x00; 34]), None);
    }

    #[test]
    fn parses_group_tx_with_v1_mnemonic() {
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_GROUP_TX];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&1001u32.to_le_bytes());
        p.extend_from_slice(&91u32.to_le_bytes());
        p.push(3);
        p.push(0);
        p.extend_from_slice(&0u16.to_le_bytes());
        // v1 mnemonic[34]: scheme 0, 24 bits = "BOB"
        let mut mnem = vec![0x00u8, 24];
        mnem.extend_from_slice(b"BOB");
        mnem.resize(34, 0);
        p.extend_from_slice(&mnem);

        let BrewMessage::CallControl(cc) = parse(&p).unwrap() else { panic!() };
        let CallPayload::GroupTransmission(gt) = cc.payload else { panic!() };
        assert_eq!(gt.source, 1001);
        assert_eq!(gt.mnemonic.as_deref(), Some("BOB"));
    }

    #[test]
    fn parses_circular_setup_request() {
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_SETUP_REQUEST];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&5001u32.to_le_bytes()); // source
        p.extend_from_slice(&6002u32.to_le_bytes()); // destination
        let mut number = b"12345".to_vec();
        number.resize(32, 0);
        p.extend_from_slice(&number); // number[32]
        p.push(2); // priority
        p.extend_from_slice(&[0u8; 10]); // remaining 10 single-byte fields
        // v1 mnemonic
        let mut mnem = vec![0x00u8, 32];
        mnem.extend_from_slice(b"CTRL");
        mnem.resize(34, 0);
        p.extend_from_slice(&mnem);

        let BrewMessage::CallControl(cc) = parse(&p).unwrap() else { panic!() };
        let CallPayload::CircularCall(c) = cc.payload else { panic!() };
        assert_eq!(c.source, 5001);
        assert_eq!(c.destination, 6002);
        assert_eq!(c.number, "12345");
        assert_eq!(c.priority, 2);
        assert_eq!(c.mnemonic.as_deref(), Some("CTRL"));
    }

    #[test]
    fn parses_connect_request_without_mnemonic() {
        // CONNECT_REQUEST is the circular struct truncated before the mnemonic.
        let id = Uuid::new_v4();
        let mut p = vec![CLASS_CALL_CONTROL, CALL_CONNECT_REQUEST];
        p.extend_from_slice(id.as_bytes());
        p.extend_from_slice(&5001u32.to_le_bytes());
        p.extend_from_slice(&6002u32.to_le_bytes());
        p.extend_from_slice(&[0u8; 32]);
        p.push(1);
        p.extend_from_slice(&[0u8; 10]);

        let BrewMessage::CallControl(cc) = parse(&p).unwrap() else { panic!() };
        let CallPayload::CircularCall(c) = cc.payload else { panic!() };
        assert_eq!(c.source, 5001);
        assert_eq!(c.mnemonic, None);
    }

    fn with_mnemonic(mut wire: Vec<u8>, name: &[u8]) -> Vec<u8> {
        let mut mnem = vec![0x00u8, (name.len() * 8) as u8];
        mnem.extend_from_slice(name);
        mnem.resize(MNEMONIC_FIELD_LEN, 0);
        wire.extend_from_slice(&mnem);
        wire
    }

    #[test]
    fn v0_recipient_gets_group_tx_and_setup_without_mnemonic() {
        let id = Uuid::new_v4();
        let v0_tx = build_group_tx(&id, 1001, 91, 3);
        let v1_tx = with_mnemonic(v0_tx.clone(), b"BOB");
        assert_eq!(adapt_to_version(&v1_tx, ConnVersion::V0).as_ref(), v0_tx.as_slice());
        let (msg, detected) = parse_with_version(&adapt_to_version(&v1_tx, ConnVersion::V0), ConnVersion::V0).unwrap();
        assert_eq!(detected, ConnVersion::V0, "a stripped GROUP_TX reads as v0");
        let BrewMessage::CallControl(cc) = msg else { panic!() };
        let CallPayload::GroupTransmission(gt) = cc.payload else { panic!() };
        assert_eq!((gt.source, gt.destination, gt.priority, gt.mnemonic), (1001, 91, 3, None));

        let v0_setup = build_circular_call_setup(&id, 5001, 6002, 2);
        let v1_setup = with_mnemonic(v0_setup.clone(), b"CTRL");
        assert_eq!(adapt_to_version(&v1_setup, ConnVersion::V0).as_ref(), v0_setup.as_slice());
    }

    #[test]
    fn version_adaptation_only_ever_strips() {
        let id = Uuid::new_v4();
        let v0_tx = build_group_tx(&id, 1001, 91, 3);
        let v1_tx = with_mnemonic(v0_tx.clone(), b"BOB");
        // A v1 recipient gets everything as sent, v1 or v0.
        assert_eq!(adapt_to_version(&v1_tx, ConnVersion::V1).as_ref(), v1_tx.as_slice());
        assert_eq!(adapt_to_version(&v0_tx, ConnVersion::V1).as_ref(), v0_tx.as_slice());
        assert_eq!(adapt_to_version(&v0_tx, ConnVersion::V0).as_ref(), v0_tx.as_slice());
        // Nothing else carries the mnemonic: other call states, frames and
        // subscriber messages pass through untouched, whatever their length.
        let connect = build_circular_connect_request(&id, 5001, 6002, 2);
        assert_eq!(adapt_to_version(&connect, ConnVersion::V0).as_ref(), connect.as_slice());
        let release = build_call_cause(CALL_RELEASE, &id, 0);
        assert_eq!(adapt_to_version(&release, ConnVersion::V0).as_ref(), release.as_slice());
        let frame = build_traffic_frame(&id, &[0x55; ACELP_CODED_FRAME_BYTES], &[0xAA; ACELP_CODED_FRAME_BYTES]);
        assert_eq!(adapt_to_version(&frame, ConnVersion::V0).as_ref(), frame.as_slice());
        let sub = build_subscriber_message(SUB_AFFILIATE, 1001, &[91; 20]);
        assert_eq!(adapt_to_version(&sub, ConnVersion::V0).as_ref(), sub.as_slice());
        assert_eq!(adapt_to_version(&[CLASS_CALL_CONTROL], ConnVersion::V0).as_ref(), &[CLASS_CALL_CONTROL]);
    }

    /// Pins the real production bug: this server used to send/expect a
    /// FRAME_TRAFFIC_CHANNEL payload of one raw 18-byte ACELP subframe, no
    /// STE header. A real Basestation (confirmed against FlowStation's
    /// `net_brew::entity::handle_voice_frame`) requires 36 bytes -- 1 STE
    /// header byte + 35 bytes packing TWO subframes' 274 bits back-to-back
    /// -- and silently discards anything shorter. `build_traffic_frame` must
    /// produce exactly that shape.
    #[test]
    fn build_traffic_frame_produces_36_byte_ste_payload() {
        let id = Uuid::new_v4();
        let sub1 = [0xFFu8; ACELP_CODED_FRAME_BYTES];
        let sub2 = [0x00u8; ACELP_CODED_FRAME_BYTES];
        let wire = build_traffic_frame(&id, &sub1, &sub2);
        assert_eq!(wire.len(), 20 + STE_VOICE_PAYLOAD_BYTES, "20-byte header + 36-byte STE payload");
        assert_eq!(wire[0], CLASS_FRAME);
        assert_eq!(wire[1], FRAME_TRAFFIC_CHANNEL);
        assert_eq!(&wire[2..18], id.as_bytes());
        let length_bits = u16::from_le_bytes([wire[18], wire[19]]);
        assert_eq!(length_bits, (STE_VOICE_PAYLOAD_BYTES * 8) as u16, "matches FlowStation's own convention (total STE bytes * 8)");
        assert_eq!(wire[20], 0x00, "STE header byte: normal speech frame");
    }

    /// Bit-exact cross-check against FlowStation's own packing
    /// (`handle_ul_voice`'s "Pack 274 bits (1-per-byte) into 35 bytes ...
    /// MSB first" branch): two subframes, all-ones then all-zeros, must
    /// leave the first ~17 wire bytes solid 0xFF, a bit boundary in the
    /// middle byte, then solid 0x00 -- not two independently 7-bit-padded
    /// 18-byte blocks concatenated (which would show a 0-bit gap after byte
    /// 16's 5 low bits instead of a clean split exactly at bit 137).
    #[test]
    fn pack_ste_voice_payload_has_no_gap_between_subframes() {
        let sub1 = [0xFFu8; ACELP_CODED_FRAME_BYTES]; // 144 bits, all 1
        let sub2 = [0x00u8; ACELP_CODED_FRAME_BYTES]; // 144 bits, all 0
        let payload = pack_ste_voice_payload(&sub1, &sub2);
        // Bits 0..137 are subframe 1's 137 real bits (all 1): bytes 1..=17
        // (payload indices, i.e. wire bytes 1..18) are solid 0xFF, and byte
        // 18 holds bits 136 (subframe 1's last real bit) down through the
        // start of subframe 2 (all 0) -- so only its top bit is set.
        for b in &payload[1..18] {
            assert_eq!(*b, 0xFF, "subframe 1's 136 whole bits, no padding gap yet");
        }
        assert_eq!(payload[18], 0b1000_0000, "bit 136 (subframe1's last bit) then subframe 2's zeros begin immediately, no gap");
        for b in &payload[19..36] {
            assert_eq!(*b, 0x00, "subframe 2's zeros, and the final 6-bit pad");
        }
    }

    #[test]
    fn ste_voice_payload_round_trips_arbitrary_subframes() {
        let mut sub1 = [0u8; ACELP_CODED_FRAME_BYTES];
        let mut sub2 = [0u8; ACELP_CODED_FRAME_BYTES];
        for (i, b) in sub1.iter_mut().enumerate() { *b = (i as u8).wrapping_mul(37).wrapping_add(11); }
        for (i, b) in sub2.iter_mut().enumerate() { *b = (i as u8).wrapping_mul(53).wrapping_add(3); }
        // Clear each subframe's 7 don't-care padding bits (last byte's low 7
        // bits) so round-tripping is exact -- pack/unpack only guarantee the
        // 137 real bits survive, not arbitrary padding content.
        sub1[17] &= 0x80;
        sub2[17] &= 0x80;

        let payload = pack_ste_voice_payload(&sub1, &sub2);
        let (out1, out2) = unpack_ste_voice_payload(&payload);
        assert_eq!(out1, sub1);
        assert_eq!(out2, sub2);
    }
}
