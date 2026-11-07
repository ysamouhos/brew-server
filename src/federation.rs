//! Server-to-server federation: connects this brew-server out to configured
//! peer brew-server instances over the *same* Brew WebSocket protocol real
//! Basestations use -- a peer link authenticates and upgrades exactly like a
//! Basestation would, just tagged `X-Brew-Mode: Peer` (see
//! `state::ClientMode::Peer`).
//!
//! Once connected, a peer is registered in `AppState.inner.clients` like any
//! other connection, and `router::handle_packet` processes whatever it sends
//! completely normally. That is deliberate: it means private/group call
//! routing and SDS routing need *no* federation-specific code at all -- they
//! already resolve a destination via `inner.subscribers`/`inner.group_clients`
//! and forward raw bytes to whatever `ClientId` owns it, peer or not. The one
//! piece that genuinely is federation-specific is registration propagation
//! (`fedroute::publish`, called wherever the table changes -- path-vector
//! route adverts on a loop-safe link, relayed SUB messages on a plain one)
//! and the full-table sync a newly connected peer needs (`attach_client`
//! below), since a peer link only sees registration *events* going forward
//! otherwise.
//!
//! This module owns the *outbound* half (dialing peers configured in
//! `[[federation.peers]]`). The inbound half needs no special code: an
//! incoming peer connection is just another `server::client_session`
//! WebSocket upgrade, same as a Basestation, differing only in the
//! `X-Brew-Mode: Peer` header it sends (and, for loop-safe mode, the
//! `X-Brew-Federation` offer -- see `fedroute`).

use crate::config::{FederationConfig, FederationPeerConfig};
use crate::fedroute::{self, X_BREW_FEDERATION, X_BREW_SERVER_ID};
use crate::protocol::{self, ConnVersion};
use crate::state::{AppState, Client, ClientId, ClientMode};
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderMap;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

/// Spawns one reconnecting dial loop per enabled `[[federation.peers]]`
/// entry. No-op when federation is disabled.
pub async fn run(state: Arc<AppState>) {
    if state.config.federation.loop_safe {
        let id = state.inner.read().await.fed.self_id;
        info!(server_id = %fedroute::format_server_id(id), "federation: loop-safe mode");
    }
    if !state.config.federation.enabled {
        return;
    }
    for peer in state.config.federation.peers.clone() {
        if !peer.enabled {
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move { peer_loop(state, peer).await });
    }
}

/// Upper bound on one dial attempt, discovery through the WebSocket
/// handshake. Without it a peer that accepts the TCP connection but never
/// answers would hang the attempt -- and with it this peer's reconnect loop
/// -- forever.
pub(crate) const DIAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Keepalive of one federation link (either direction), from `[federation]`.
///
/// Nothing else notices a half-open link -- a peer that lost power, a NAT
/// entry that expired, a route that went away: TCP only gives up after many
/// minutes of unacknowledged data, and an idle link sends none. Until then
/// its registrations keep pointing calls and SDS into the void. Plain
/// WebSocket pings are answered by any RFC 6455 peer (tungstenite and axum
/// reply on their own while the socket is read), older brew-server versions
/// included, so this needs nothing from the far end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keepalive {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Keepalive {
    /// `None` when `keepalive_interval_seconds` is 0 (off).
    pub fn from_config(cfg: &FederationConfig) -> Option<Self> {
        if cfg.keepalive_interval_seconds == 0 {
            return None;
        }
        let interval = Duration::from_secs(cfg.keepalive_interval_seconds);
        // Below two intervals a single late pong would already close the link.
        let timeout = Duration::from_secs(cfg.keepalive_timeout_seconds).max(interval * 2);
        Some(Self { interval, timeout })
    }

    /// First tick one interval from now (no ping right after connecting).
    pub fn ping_timer(self) -> tokio::time::Interval {
        let mut i = tokio::time::interval_at(tokio::time::Instant::now() + self.interval, self.interval);
        i.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        i
    }
}

/// Resolves when the next ping is due; never when keepalive is off.
pub async fn ping_due(timer: &mut Option<tokio::time::Interval>) {
    match timer {
        Some(t) => { t.tick().await; }
        None => std::future::pending::<()>().await,
    }
}

/// Next item from a link's read half, or `Err` once it has been silent for
/// the keepalive timeout (any frame, data or control, counts as life).
/// `StreamExt::next` is cancel-safe, so a timeout never loses a frame.
pub async fn next_frame<S: futures_util::Stream + Unpin>(
    rx: &mut S,
    keepalive: Option<Keepalive>,
) -> Result<Option<S::Item>, tokio::time::error::Elapsed> {
    match keepalive {
        Some(k) => tokio::time::timeout(k.timeout, rx.next()).await,
        None => Ok(rx.next().await),
    }
}

async fn peer_loop(state: Arc<AppState>, peer: FederationPeerConfig) {
    let interval = Duration::from_secs(peer.reconnect_interval_seconds.max(5));
    loop {
        info!(peer = %peer.name, remote_host = %peer.remote_host, "federation: connecting to peer");
        if let Err(e) = connect_and_run(&state, &peer).await {
            warn!(peer = %peer.name, error = %e, "federation: peer link failed");
        }
        tokio::time::sleep(interval).await;
    }
}

/// Plain TCP or TLS stream to a peer, so discovery and the WebSocket upgrade
/// share one dial path.
pub trait PeerIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> PeerIo for T {}

/// Opens a connection to `peer.remote_host`, wrapped in TLS when `tls` is
/// given (`peer.tls`).
pub(crate) async fn dial(peer: &FederationPeerConfig, tls: Option<&TlsConnector>) -> anyhow::Result<Box<dyn PeerIo>> {
    let tcp = TcpStream::connect(&peer.remote_host).await?;
    // Same reason as the Brew listener (`server::nodelay_listener`): a peer
    // link carries every call's 60ms voice frames, and Nagle + delayed ACKs
    // would hold them back and send them bunched.
    tcp.set_nodelay(true)?;
    match tls {
        None => Ok(Box::new(tcp)),
        Some(c) => {
            let name = tls_server_name(peer);
            let server_name = ServerName::try_from(name.to_string())
                .with_context(|| format!("invalid TLS server name {name:?}"))?;
            let stream = c.connect(server_name, tcp).await
                .with_context(|| format!("TLS handshake with {}", peer.remote_host))?;
            Ok(Box::new(stream))
        }
    }
}

/// Name the peer's certificate is checked against and sent as SNI:
/// `tls_server_name`, or else the host part of `remote_host` (`host:port`,
/// `[v6]:port` or a bare host). An IP address becomes an IP `ServerName`,
/// which webpki matches against an IP subjectAltName.
fn tls_server_name(peer: &FederationPeerConfig) -> &str {
    if !peer.tls_server_name.is_empty() {
        return &peer.tls_server_name;
    }
    let host = peer.remote_host.as_str();
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split_once(']').map_or(rest, |(h, _)| h);
    }
    host.rsplit_once(':').map_or(host, |(h, _)| h)
}

/// TLS client settings for one peer, built on every dial attempt so a
/// renewed CA bundle or pinned certificate is picked up on the next reconnect
/// without a restart. TLS 1.3 only: rustls is built without `tls12`, the
/// same as this server's own listener.
pub(crate) fn tls_connector(peer: &FederationPeerConfig) -> anyhow::Result<TlsConnector> {
    let builder = rustls::ClientConfig::builder();
    let config = if !peer.tls_pinned_cert_path.as_os_str().is_empty() {
        let cert = CertificateDer::from_pem_file(&peer.tls_pinned_cert_path)
            .with_context(|| format!("reading tls_pinned_cert_path {}", peer.tls_pinned_cert_path.display()))?;
        let algs = builder.crypto_provider().signature_verification_algorithms;
        builder.dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedCert { cert, algs }))
            .with_no_client_auth()
    } else {
        let certs = CertificateDer::pem_file_iter(&peer.tls_ca_path)
            .and_then(|it| it.collect::<Result<Vec<_>, _>>())
            .with_context(|| format!("reading tls_ca_path {}", peer.tls_ca_path.display()))?;
        let mut roots = rustls::RootCertStore::empty();
        let (added, _ignored) = roots.add_parsable_certificates(certs);
        if added == 0 {
            anyhow::bail!("no usable CA certificate in {}", peer.tls_ca_path.display());
        }
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Trusts exactly one certificate (`tls_pinned_cert_path`): the usual way to
/// link to a peer with a self-signed certificate, which webpki would reject
/// (no CA, often no SAN, often CA:TRUE). The handshake signature is still
/// verified against that certificate's key, so only its holder can pass.
#[derive(Debug)]
struct PinnedCert {
    cert: CertificateDer<'static>,
    algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedCert {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.cert.as_ref() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// Minimal parsed HTTP/1.1 response: just enough to drive the Brew discovery
/// digest dance (status, headers, body), not a general-purpose HTTP client.
struct HttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// `User-Agent` this server identifies itself with when dialling a peer.
pub(crate) const USER_AGENT: &str = concat!("brew-server/", env!("CARGO_PKG_VERSION"));

/// How an outbound link introduces itself: its `X-Brew-Mode` and its
/// `User-Agent`. A federation link is a `Peer`; a link to another network
/// (`netlink`) is a `Basestation` with a name the operator chooses.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkIdentity<'a> {
    pub mode: &'a str,
    pub user_agent: &'a str,
}

const PEER_IDENTITY: LinkIdentity<'static> = LinkIdentity { mode: "Peer", user_agent: USER_AGENT };

/// Sends one plain (non-upgrade) `GET` (over TLS when `tls` is given), closes
/// the connection after reading the response. Used only for the
/// discovery/digest pre-flight -- the actual WebSocket upgrade is a separate
/// connection via `tokio_tungstenite`.
///
/// Every request -- the unauthenticated first one and the digest retry alike
/// -- announces `X-Brew-Mode: Peer` and `X-Brew-Version`, the way a
/// Basestation announces its own mode on discovery. That matters: with
/// `[auth]` enabled, the far end records the mode of the *authorized*
/// discovery request in the session token and ignores whatever the upgrade
/// says, so without them this link used to be registered over there as a
/// Basestation (no table sync, no relay) and federation only worked in one
/// direction. Some Brew servers also refuse a discovery without a
/// `User-Agent` (400). The `extra` headers (the loop-safe federation offer)
/// go on every request too.
async fn http_get(
    peer: &FederationPeerConfig,
    tls: Option<&TlsConnector>,
    path: &str,
    authorization: Option<&str>,
    extra: &[(&str, String)],
    who: LinkIdentity<'_>,
) -> anyhow::Result<HttpResponse> {
    let mut stream = dial(peer, tls).await?;
    let remote_host = &peer.remote_host;
    let LinkIdentity { mode, user_agent } = who;
    let mut req = format!(
        "GET {path} HTTP/1.1\r\nHost: {remote_host}\r\nUser-Agent: {user_agent}\r\nX-Brew-Mode: {mode}\r\nX-Brew-Version: {}\r\nConnection: close\r\n",
        protocol::BREW_PROTOCOL_VERSION,
    );
    if let Some(a) = authorization {
        req.push_str(&format!("Authorization: {a}\r\n"));
    }
    for (name, value) in extra {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf).await {
        Ok(_) => {}
        // A TLS server may close without close_notify; the response is complete.
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !buf.is_empty() => {}
        Err(e) => return Err(e.into()),
    }
    parse_http_response(&buf)
}

fn parse_http_response(buf: &[u8]) -> anyhow::Result<HttpResponse> {
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("malformed HTTP response (no header/body split)"))?;
    let header_text = std::str::from_utf8(&buf[..split])?;
    let body = buf[split + 4..].to_vec();
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().ok_or_else(|| anyhow::anyhow!("empty HTTP response"))?;
    let status = status_line.split_whitespace().nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("bad HTTP status line: {status_line}"))?;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok(HttpResponse { status, headers, body })
}

/// Runs the Brew discovery flow (the same 401-challenge-then-session-token
/// dance a real Basestation performs, or a single request when the peer has
/// `[auth]` disabled) and returns the path to actually upgrade the WebSocket
/// at.
pub(crate) async fn discover(
    peer: &FederationPeerConfig,
    tls: Option<&TlsConnector>,
    path: &str,
    extra: &[(&str, String)],
    who: LinkIdentity<'_>,
) -> anyhow::Result<String> {
    // A server may move its discovery path (`/brew` -> `/brew/`) with a
    // redirect: followed a few times, on the same server only.
    let mut path = path.to_string();
    let mut resp = http_get(peer, tls, &path, None, extra, who).await?;
    for _ in 0..MAX_REDIRECTS {
        if !matches!(resp.status, 301 | 302 | 303 | 307 | 308) { break; }
        let location = resp.headers.get("location")
            .ok_or_else(|| anyhow::anyhow!("discovery status {} with no Location", resp.status))?;
        path = redirect_path(peer, tls.is_some(), location)?;
        resp = http_get(peer, tls, &path, None, extra, who).await?;
    }
    let path = path.as_str();
    match resp.status {
        200 => ws_path_from(resp.body),
        401 => {
            let challenge_hdr = resp.headers.get("www-authenticate")
                .ok_or_else(|| anyhow::anyhow!("401 with no WWW-Authenticate"))?;
            let challenge = crate::sip::auth::parse_params(challenge_hdr);
            let cnonce = uuid::Uuid::new_v4().simple().to_string();
            let authz = crate::sip::auth::build_authorization(
                &challenge, &peer.username, &peer.password, "GET", path, &cnonce, 1,
            );
            let resp2 = http_get(peer, tls, path, Some(&authz), extra, who).await?;
            if resp2.status != 200 {
                anyhow::bail!("digest auth rejected (HTTP {})", resp2.status);
            }
            ws_path_from(resp2.body)
        }
        other => anyhow::bail!("unexpected discovery status {other}"),
    }
}

/// The WebSocket path a discovery answered with. Anything that is not a
/// plain path (a web page behind a redirect, an error text) is refused, so
/// it is never dialled.
fn ws_path_from(body: Vec<u8>) -> anyhow::Result<String> {
    let text = String::from_utf8(body)?;
    let p = text.trim();
    if p.is_empty() || p.len() > 256 || p.chars().any(|c| c.is_whitespace() || c == '<') {
        anyhow::bail!("discovery answered with something that is not a path (wrong path configured?)");
    }
    Ok(p.to_string())
}

/// Redirects of one discovery that are followed.
const MAX_REDIRECTS: usize = 3;

/// Path a discovery redirect points to: a path, or a URL on the same host
/// and scheme (anything else is an error that says what to configure).
fn redirect_path(peer: &FederationPeerConfig, tls: bool, location: &str) -> anyhow::Result<String> {
    let location = location.trim();
    if location.starts_with('/') { return Ok(location.to_string()); }
    let (scheme, rest) = location.split_once("://")
        .ok_or_else(|| anyhow::anyhow!("discovery redirected to {location:?}"))?;
    let want = if tls { "https" } else { "http" };
    if !scheme.eq_ignore_ascii_case(want) {
        anyhow::bail!("discovery redirected to {location} ({}: switch TLS {})", scheme, if tls { "off" } else { "on" });
    }
    let (authority, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let host_of = |h: &str| h.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(h, |(h, _)| h).to_ascii_lowercase();
    if host_of(authority) != host_of(&peer.remote_host) {
        anyhow::bail!("discovery redirected to another server: {location}");
    }
    Ok(path)
}

/// Neighbour id from the 101 response when the peer accepted loop-safe mode;
/// Err when it reports our own id.
fn accepted_neighbour(resp: &HeaderMap, loop_safe: bool, own_id: u64) -> anyhow::Result<Option<u64>> {
    let header = |name: &str| resp.get(name).and_then(|v| v.to_str().ok());
    if !loop_safe || !fedroute::federation_offered(header(X_BREW_FEDERATION)) {
        return Ok(None);
    }
    match header(X_BREW_SERVER_ID).and_then(fedroute::parse_server_id) {
        Some(id) if id == own_id => anyhow::bail!("peer reports this server's own id: a link to ourselves"),
        neighbour => Ok(neighbour),
    }
}

async fn connect_and_run(state: &Arc<AppState>, peer: &FederationPeerConfig) -> anyhow::Result<()> {
    let tls = if peer.tls { Some(tls_connector(peer)?) } else { None };
    // Loop-safe federation is offered on discovery and on the upgrade; the
    // peer decides on the upgrade and accepts in its 101.
    let loop_safe = state.config.federation.loop_safe;
    let own_id = state.inner.read().await.fed.self_id;
    let fed_headers: Vec<(&str, String)> = if loop_safe {
        vec![(X_BREW_FEDERATION, "1".to_string()), (X_BREW_SERVER_ID, fedroute::format_server_id(own_id))]
    } else {
        Vec::new()
    };
    let (ws_stream, remote_addr, neighbour) = tokio::time::timeout(DIAL_TIMEOUT, async {
        let path = normalize_path(&peer.path);
        let ws_path = discover(peer, tls.as_ref(), &path, &fed_headers, PEER_IDENTITY).await?;

        let remote_addr = tokio::net::lookup_host(&peer.remote_host).await.ok()
            .and_then(|mut it| it.next());

        let scheme = if tls.is_some() { "wss" } else { "ws" };
        let ws_url = format!("{scheme}://{}{}", peer.remote_host, normalize_path(&ws_path));
        let mut request = ws_url.into_client_request()?;
        request.headers_mut().insert("User-Agent", USER_AGENT.parse()?);
        // Also on the upgrade, not just discovery: a peer with `[auth]` disabled
        // takes the mode straight from the upgrade request.
        request.headers_mut().insert("X-Brew-Mode", "Peer".parse()?);
        request.headers_mut().insert("X-Brew-Version", protocol::BREW_PROTOCOL_VERSION.to_string().parse()?);
        request.headers_mut().insert("Sec-WebSocket-Protocol", "brew".parse()?);
        for (name, value) in &fed_headers {
            request.headers_mut().insert(*name, value.parse()?);
        }

        let stream = dial(peer, tls.as_ref()).await?;
        let (ws_stream, resp) = tokio_tungstenite::client_async(request, stream).await?;
        // On Err the stream is dropped here, closing the link to ourselves.
        let neighbour = accepted_neighbour(resp.headers(), loop_safe, own_id)?;
        anyhow::Ok((ws_stream, remote_addr, neighbour))
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out after {}s dialling peer", DIAL_TIMEOUT.as_secs()))??;
    info!(peer = %peer.name, loop_safe_neighbour = neighbour.map(fedroute::format_server_id).unwrap_or_default(),
        "federation: peer link established");
    let keepalive = Keepalive::from_config(&state.config.federation);
    run_peer_session(state.clone(), peer.clone(), ws_stream, remote_addr, keepalive, neighbour).await;
    Ok(())
}

pub(crate) fn normalize_path(path: &str) -> String {
    let mut p = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    while p.len() > 1 && p.ends_with('/') { p.pop(); }
    p
}

/// Runs one established peer session: registers the peer in `inner.clients`
/// (mode `Peer`, loop-safe when `neighbour` is set), sends it a full snapshot
/// of everything this server currently knows (see `attach_client`), then
/// pumps inbound frames through the normal router and outbound frames from its `tx` queue -- the same shape
/// as `server::client_session`, just over a `tokio_tungstenite` client
/// socket (plain or TLS) instead of axum's server-side one. With `keepalive`
/// the link is pinged and closed once silent past its timeout (see
/// `Keepalive`); `peer_loop` then redials it.
async fn run_peer_session(
    state: Arc<AppState>,
    peer: FederationPeerConfig,
    ws_stream: tokio_tungstenite::WebSocketStream<Box<dyn PeerIo>>,
    remote_addr: Option<SocketAddr>,
    keepalive: Option<Keepalive>,
    neighbour: Option<u64>,
) {
    let id = uuid::Uuid::new_v4();
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let connected_at_ms = crate::telemetry::now_ms();
    attach_client(&state, id, Client {
        tx, mode: ClientMode::Peer, version: ConnVersion::V1, version_announced: false, remote_addr, connected_at_ms, username: None,
    }, neighbour).await;
    info!(%id, peer = %peer.name, "federation: peer registered");

    let mut ping = keepalive.map(Keepalive::ping_timer);
    let writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                packet = rx.recv() => {
                    let Some(packet) = packet else { break };
                    if ws_tx.send(Message::Binary(packet.into())).await.is_err() { break; }
                }
                _ = ping_due(&mut ping) => {
                    if ws_tx.send(Message::Ping(Default::default())).await.is_err() { break; }
                }
            }
        }
    });

    loop {
        let item = match next_frame(&mut ws_rx, keepalive).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => {
                warn!(%id, peer = %peer.name, timeout_s = keepalive.map_or(0, |k| k.timeout.as_secs()),
                    "federation: peer link silent past keepalive timeout; closing");
                break;
            }
        };
        match item {
            Ok(Message::Binary(data)) => crate::router::handle_packet(state.clone(), id, data.to_vec()).await,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Ok(Message::Text(_)) | Ok(Message::Frame(_)) => {}
            Err(e) => { warn!(%id, peer = %peer.name, error = %e, "federation: peer link read error"); break; }
        }
    }

    writer.abort();
    state.cleanup_client(id).await;
    info!(%id, peer = %peer.name, "federation: peer link closed");
}

/// Registers a new connection and, for a peer link, queues its initial full sync in the
/// same critical section, so a registration change racing the sync can never reach the
/// peer before (and be overwritten by) the stale snapshot.
///
/// The sync is everything this server currently knows, regardless of whether
/// it learned it locally or from another peer (transit): every effective
/// route as a `FED_ROUTE` on a loop-safe link (`neighbour` is the far
/// server's id), or every registered ISSI (`SUB_REGISTER`) and its group
/// affiliations (`SUB_AFFILIATE`) on a plain one. Without it, a peer only
/// ever learns about registrations that happen to change *after* it
/// connects, so a freshly (re)started link would be blind to everything
/// already in place.
pub async fn attach_client(state: &Arc<AppState>, id: ClientId, client: Client, neighbour: Option<u64>) {
    let mut inner = state.inner.write().await;
    let tx = client.tx.clone();
    let peer = client.mode == ClientMode::Peer;
    inner.clients.insert(id, client);
    if let Some(neighbour) = neighbour {
        inner.fed.links.insert(id, neighbour);
        // Tell the far end we understand Basestation position adverts; it
        // answers with its own hello, and each side then syncs what it has.
        let _ = tx.send(fedroute::build_bts_hello());
    }
    if peer {
        for msg in fedroute::sync_messages(&inner, id) {
            let _ = tx.send(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Loopback HTTP(S) server: answers one connection per entry of
    /// `responses`, in order, and returns the request head each connection
    /// sent. With `tls`, connections are TLS; one whose handshake fails (the
    /// client rejected the certificate) is skipped.
    async fn http_stub(
        responses: Vec<&'static str>,
        tls: Option<tokio_rustls::TlsAcceptor>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut sock: Box<dyn PeerIo> = match &tls {
                    None => Box::new(tcp),
                    Some(acceptor) => match acceptor.accept(tcp).await {
                        Ok(s) => Box::new(s),
                        Err(_) => continue,
                    },
                };
                let mut buf = Vec::new();
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut chunk = [0u8; 1024];
                    let n = sock.read(&mut chunk).await.unwrap();
                    if n == 0 { break; }
                    buf.extend_from_slice(&chunk[..n]);
                }
                requests.push(String::from_utf8(buf).unwrap());
                sock.write_all(response.as_bytes()).await.unwrap();
                sock.flush().await.unwrap();
                // Dropped without a TLS close_notify, like some servers do.
            }
            requests
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn discovery_follows_a_redirect_on_the_same_server() {
        const MOVED: &str = "HTTP/1.1 301 Moved Permanently\r\nLocation: /brew/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (addr, server) = http_stub(vec![MOVED, OK], None).await;
        let peer = FederationPeerConfig { remote_host: addr, ..Default::default() };
        assert_eq!(discover(&peer, None, "/brew", &[], PEER_IDENTITY).await.unwrap(), "/brew/");
        let requests = server.await.unwrap();
        assert!(requests[1].starts_with("GET /brew/ HTTP/1.1\r\n"), "{:?}", requests[1]);
    }

    #[tokio::test]
    async fn discovery_refuses_a_web_page() {
        const PAGE: &str = "HTTP/1.1 200 OK\r\nContent-Length: 27\r\nConnection: close\r\n\r\n<html><body>Hi</body></html";
        let (addr, _server) = http_stub(vec![PAGE], None).await;
        let peer = FederationPeerConfig { remote_host: addr, ..Default::default() };
        let err = discover(&peer, None, "/brew", &[], PEER_IDENTITY).await.unwrap_err().to_string();
        assert!(err.contains("not a path"), "{err}");
    }

    #[test]
    fn redirect_path_stays_on_the_same_server() {
        let peer = FederationPeerConfig { remote_host: "core.example.net:443".into(), ..Default::default() };
        assert_eq!(redirect_path(&peer, true, "/brew/").unwrap(), "/brew/");
        assert_eq!(redirect_path(&peer, true, "https://core.example.net/brew/").unwrap(), "/brew/");
        assert_eq!(redirect_path(&peer, true, "https://CORE.example.net:443/x").unwrap(), "/x");
        assert!(redirect_path(&peer, true, "https://evil.example/brew").is_err());
        let err = redirect_path(&peer, false, "https://core.example.net/brew/").unwrap_err().to_string();
        assert!(err.contains("switch TLS on"), "{err}");
    }

    fn assert_identifies_as_peer(request: &str) {
        let expected_ua = format!("User-Agent: brew-server/{}\r\n", env!("CARGO_PKG_VERSION"));
        assert!(request.contains(&expected_ua), "no User-Agent in {request:?}");
        assert!(request.contains("X-Brew-Mode: Peer\r\n"), "no X-Brew-Mode in {request:?}");
        let expected_version = format!("X-Brew-Version: {}\r\n", protocol::BREW_PROTOCOL_VERSION);
        assert!(request.contains(&expected_version), "no X-Brew-Version in {request:?}");
    }

    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n/brew/";

    #[tokio::test]
    async fn discovery_without_auth_identifies_as_peer() {
        let (addr, server) = http_stub(vec![OK], None).await;
        let peer = FederationPeerConfig { remote_host: addr, ..Default::default() };
        assert_eq!(discover(&peer, None, "/brew", &[], PEER_IDENTITY).await.unwrap(), "/brew/");
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with("GET /brew HTTP/1.1\r\n"));
        assert_identifies_as_peer(&requests[0]);
    }

    #[tokio::test]
    async fn both_digest_requests_identify_as_peer() {
        // The far end keeps the mode of the *authorized* request, so the
        // retry must carry the headers as well as the first attempt.
        let challenge = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"r\", nonce=\"n\", qop=\"auth\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (addr, server) = http_stub(vec![challenge, OK], None).await;
        let peer = FederationPeerConfig {
            remote_host: addr, username: "9000001".into(), password: "secret".into(), ..Default::default()
        };
        assert_eq!(discover(&peer, None, "/brew", &[], PEER_IDENTITY).await.unwrap(), "/brew/");
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2);
        for request in &requests { assert_identifies_as_peer(request); }
        assert!(!requests[0].contains("Authorization:"));
        assert!(requests[1].contains("Authorization: Digest username=\"9000001\""));
    }

    #[tokio::test]
    async fn discovery_carries_the_loop_safe_offer() {
        let challenge = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"r\", nonce=\"n\", qop=\"auth\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (addr, server) = http_stub(vec![challenge, OK], None).await;
        let peer = FederationPeerConfig { remote_host: addr, username: "9000001".into(), password: "x".into(), ..Default::default() };
        let offer = [(X_BREW_FEDERATION, "1".to_string()), (X_BREW_SERVER_ID, "00a1b2c3d4e5f607".to_string())];
        assert_eq!(discover(&peer, None, "/brew", &offer, PEER_IDENTITY).await.unwrap(), "/brew/");
        for request in server.await.unwrap() {
            assert!(request.contains("X-Brew-Federation: 1\r\n"), "{request:?}");
            assert!(request.contains("X-Brew-Server-Id: 00a1b2c3d4e5f607\r\n"), "{request:?}");
        }
    }

    fn response_headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs { h.insert(*k, v.parse().unwrap()); }
        h
    }

    #[test]
    fn accepted_neighbour_from_the_101() {
        let accepted = response_headers(&[("X-Brew-Federation", "1"), ("X-Brew-Server-Id", "00000000000000bb")]);
        assert_eq!(accepted_neighbour(&accepted, true, 7).unwrap(), Some(0xbb));
        // An older peer answers without them: a plain link.
        assert_eq!(accepted_neighbour(&response_headers(&[]), true, 7).unwrap(), None);
        assert_eq!(accepted_neighbour(&response_headers(&[("X-Brew-Federation", "1")]), true, 7).unwrap(), None);
        // Not offered (loop_safe off): never switched on by the far end alone.
        assert_eq!(accepted_neighbour(&accepted, false, 7).unwrap(), None);
    }

    #[test]
    fn accepted_neighbour_refuses_our_own_id() {
        let ourselves = response_headers(&[("X-Brew-Federation", "1"), ("X-Brew-Server-Id", "00000000000000bb")]);
        assert!(accepted_neighbour(&ourselves, true, 0xbb).is_err());
    }

    #[test]
    fn tls_server_name_defaults_to_host_of_remote_host() {
        let name = |remote_host: &str, tls_server_name: &str| {
            let peer = FederationPeerConfig {
                remote_host: remote_host.into(), tls_server_name: tls_server_name.into(), ..Default::default()
            };
            super::tls_server_name(&peer).to_string()
        };
        assert_eq!(name("brew.example.org:9000", ""), "brew.example.org");
        assert_eq!(name("brew.example.org", ""), "brew.example.org");
        assert_eq!(name("10.0.0.20:9000", ""), "10.0.0.20");
        assert_eq!(name("[2001:db8::1]:9000", ""), "2001:db8::1");
        assert_eq!(name("10.0.0.20:9000", "brew.example.org"), "brew.example.org");
        // An address is matched against an IP subjectAltName, not as a DNS name.
        assert!(matches!(ServerName::try_from("10.0.0.20".to_string()), Ok(ServerName::IpAddress(_))));
        assert!(matches!(ServerName::try_from("2001:db8::1".to_string()), Ok(ServerName::IpAddress(_))));
    }

    // Test PKI, valid until 2126: a CA, and a leaf it signed for
    // IP:127.0.0.1 and DNS:localhost (EC P-256).
    const TEST_CA: &str = "-----BEGIN CERTIFICATE-----
MIIBojCCAUmgAwIBAgIUOF0/Y5MVE68sF6dcBT/RxpWf4mowCgYIKoZIzj0EAwIw
HjEcMBoGA1UEAwwTYnJldy1zZXJ2ZXIgdGVzdCBDQTAgFw0yNjEwMDQxMjQ4MTZa
GA8yMTI2MDkxMDEyNDgxNlowHjEcMBoGA1UEAwwTYnJldy1zZXJ2ZXIgdGVzdCBD
QTBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABBI2fmjuq+VyAsnyz6wWcEWjDWzl
SQ8TMBwiFFKo7zVfT/ZHCSGWy3l/Y4fliB2EcvRU3V/jNa1jOHX9H11lA6KjYzBh
MB0GA1UdDgQWBBTV3iXCb8IrinrDgfunvH67nJx6NzAfBgNVHSMEGDAWgBTV3iXC
b8IrinrDgfunvH67nJx6NzAPBgNVHRMBAf8EBTADAQH/MA4GA1UdDwEB/wQEAwIB
BjAKBggqhkjOPQQDAgNHADBEAiALb199VdAFm3GvaibB5wNxNtscTTi9gotaE7h2
KWlSAwIgE0Qu3ybEbHd3nF+8y+rtnqC9EJuFr1HNMTdXxhNgvPE=
-----END CERTIFICATE-----
";
    const TEST_LEAF: &str = "-----BEGIN CERTIFICATE-----
MIIByTCCAW+gAwIBAgIUEPG2bjIadT79+9j+w+V8KDEPZ4QwCgYIKoZIzj0EAwIw
HjEcMBoGA1UEAwwTYnJldy1zZXJ2ZXIgdGVzdCBDQTAgFw0yNjEwMDQxMjQ4MTZa
GA8yMTI2MDkxMDEyNDgxNlowFDESMBAGA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZI
zj0CAQYIKoZIzj0DAQcDQgAEC5wSiKDnNd00MxZ4cyvy7S6Awiu5a5+L5ma+gJnJ
WtIP2TxXuU8vRWEQaksLI6Yr/JBWhzrJXEk6L7ldBaqQ3aOBkjCBjzAMBgNVHRMB
Af8EAjAAMA4GA1UdDwEB/wQEAwIHgDATBgNVHSUEDDAKBggrBgEFBQcDATAaBgNV
HREEEzARhwR/AAABgglsb2NhbGhvc3QwHQYDVR0OBBYEFCRLs9xYKQMy0v1uGwHm
+Vhz6HmXMB8GA1UdIwQYMBaAFNXeJcJvwiuKesOB+6e8frucnHo3MAoGCCqGSM49
BAMCA0gAMEUCIQD31yo5uFLTzL9Hw14lJTkPJaKyoXglEDMKBDn3htpXigIgZEhy
1ARCi/WOvH3qBXwq7s5g2zMidxIBpuW8mBWG96w=
-----END CERTIFICATE-----
";
    const TEST_LEAF_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgwxf2huLXLEPlWF+O
4weJY9ew0+yUhdLhVMlCIkz+/muhRANCAAQLnBKIoOc13TQzFnhzK/LtLoDCK7lr
n4vmZr6Amcla0g/ZPFe5Ty9FYRBqSwsjpiv8kFaHOslcSTovuV0FqpDd
-----END PRIVATE KEY-----
";

    /// TLS acceptor presenting `TEST_LEAF` (without the CA in the chain).
    fn test_acceptor() -> tokio_rustls::TlsAcceptor {
        let cert = CertificateDer::from_pem_slice(TEST_LEAF.as_bytes()).unwrap();
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(TEST_LEAF_KEY.as_bytes()).unwrap();
        let config = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(vec![cert], key).unwrap();
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
    }

    /// Writes `pem` to a fresh temporary file, removed when dropped.
    struct TempPem(std::path::PathBuf);
    impl TempPem {
        fn new(pem: &str) -> Self {
            let path = std::env::temp_dir().join(format!("brew-federation-test-{}.pem", uuid::Uuid::new_v4().simple()));
            std::fs::write(&path, pem).unwrap();
            Self(path)
        }
    }
    impl Drop for TempPem {
        fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
    }

    /// Runs one TLS discovery against a stub presenting `TEST_LEAF`.
    async fn tls_discover(mut peer: FederationPeerConfig) -> anyhow::Result<String> {
        let (addr, server) = http_stub(vec![OK], Some(test_acceptor())).await;
        peer.remote_host = addr;
        peer.tls = true;
        let result = async {
            let connector = tls_connector(&peer)?;
            discover(&peer, Some(&connector), "/brew", &[], PEER_IDENTITY).await
        }.await;
        if result.is_ok() {
            let requests = server.await.unwrap();
            assert_identifies_as_peer(&requests[0]);
        } else {
            server.abort();
        }
        result
    }

    #[tokio::test]
    async fn tls_discovery_verifies_against_ca_bundle() {
        let ca = TempPem::new(TEST_CA);
        // remote_host is 127.0.0.1:port, so the leaf's IP SAN is what matches.
        let peer = FederationPeerConfig { tls_ca_path: ca.0.clone(), ..Default::default() };
        assert_eq!(tls_discover(peer).await.unwrap(), "/brew/");
    }

    #[tokio::test]
    async fn tls_rejects_certificate_for_another_name() {
        let ca = TempPem::new(TEST_CA);
        let peer = FederationPeerConfig {
            tls_ca_path: ca.0.clone(), tls_server_name: "brew.example.org".into(), ..Default::default()
        };
        let err = format!("{:#}", tls_discover(peer).await.unwrap_err());
        assert!(err.contains("not valid for name"), "{err}");
    }

    #[tokio::test]
    async fn tls_rejects_certificate_from_unknown_ca() {
        // A bundle holding only the leaf itself: not a trust anchor for it.
        let not_the_ca = TempPem::new(TEST_LEAF);
        let peer = FederationPeerConfig { tls_ca_path: not_the_ca.0.clone(), ..Default::default() };
        let err = format!("{:#}", tls_discover(peer).await.unwrap_err());
        assert!(err.contains("UnknownIssuer"), "{err}");
    }

    #[tokio::test]
    async fn tls_pinned_certificate_ignores_issuer_and_name() {
        let pin = TempPem::new(TEST_LEAF);
        let peer = FederationPeerConfig {
            tls_pinned_cert_path: pin.0.clone(),
            tls_ca_path: "/nonexistent/ca.pem".into(),
            tls_server_name: "brew.example.org".into(),
            ..Default::default()
        };
        assert_eq!(tls_discover(peer).await.unwrap(), "/brew/");
    }

    #[tokio::test]
    async fn tls_pin_of_another_certificate_is_rejected() {
        let pin = TempPem::new(TEST_CA);
        let peer = FederationPeerConfig { tls_pinned_cert_path: pin.0.clone(), ..Default::default() };
        let err = format!("{:#}", tls_discover(peer).await.unwrap_err());
        assert!(err.contains("ApplicationVerificationFailure"), "{err}");
    }

    #[test]
    fn tls_connector_needs_a_usable_ca() {
        let empty = TempPem::new("not a certificate\n");
        let peer = FederationPeerConfig { tls: true, tls_ca_path: empty.0.clone(), ..Default::default() };
        let err = tls_connector(&peer).err().expect("empty bundle must fail").to_string();
        assert!(err.contains("no usable CA certificate"), "{err}");
        let missing = FederationPeerConfig { tls: true, tls_ca_path: "/nonexistent/ca.pem".into(), ..Default::default() };
        assert!(tls_connector(&missing).is_err());
    }

    #[test]
    fn keepalive_from_config() {
        let mut cfg = FederationConfig::default();
        assert_eq!(Keepalive::from_config(&cfg), Some(Keepalive {
            interval: Duration::from_secs(15), timeout: Duration::from_secs(45),
        }));
        cfg.keepalive_interval_seconds = 30;
        cfg.keepalive_timeout_seconds = 10;
        assert_eq!(Keepalive::from_config(&cfg).unwrap().timeout, Duration::from_secs(60), "raised to 2x interval");
        cfg.keepalive_interval_seconds = 0;
        assert_eq!(Keepalive::from_config(&cfg), None);
    }

    /// Both ends of an in-memory WebSocket link: ours (the dialling side, as
    /// `run_peer_session` gets it) and the far server's.
    async fn ws_pair() -> (
        tokio_tungstenite::WebSocketStream<Box<dyn PeerIo>>,
        tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>,
    ) {
        use tokio_tungstenite::tungstenite::protocol::Role;
        let (near, far) = tokio::io::duplex(65536);
        let near = tokio_tungstenite::WebSocketStream::from_raw_socket(Box::new(near) as Box<dyn PeerIo>, Role::Client, None).await;
        let far = tokio_tungstenite::WebSocketStream::from_raw_socket(far, Role::Server, None).await;
        (near, far)
    }

    const FAST: Keepalive = Keepalive { interval: Duration::from_millis(50), timeout: Duration::from_millis(200) };

    #[tokio::test]
    async fn silent_peer_link_closes_after_keepalive_timeout() {
        let state = AppState::for_test();
        // The far end stays open but is never polled: a half-open link.
        let (near, _far) = ws_pair().await;
        let started = tokio::time::Instant::now();
        tokio::time::timeout(
            Duration::from_secs(5),
            run_peer_session(state.clone(), FederationPeerConfig::default(), near, None, Some(FAST), None),
        ).await.expect("a silent link must be closed by the keepalive");
        assert!(started.elapsed() >= FAST.timeout);
        assert!(state.inner.read().await.clients.is_empty(), "link deregistered");
    }

    #[tokio::test]
    async fn negotiated_peer_session_is_a_loop_safe_link_until_it_closes() {
        let state = AppState::for_test();
        let (near, far) = ws_pair().await;
        let session = tokio::spawn(run_peer_session(state.clone(), FederationPeerConfig::default(), near, None, None, Some(0xbb)));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while state.inner.read().await.fed.links.is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(state.inner.read().await.fed.links.values().copied().collect::<Vec<_>>(), vec![0xbb]);
        drop(far);
        tokio::time::timeout(Duration::from_secs(5), session).await.unwrap().unwrap();
        let inner = state.inner.read().await;
        assert!(inner.clients.is_empty() && inner.fed.links.is_empty());
    }

    #[tokio::test]
    async fn answering_peer_link_stays_up() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let state = AppState::for_test();
        let (near, mut far) = ws_pair().await;
        let pings = Arc::new(AtomicUsize::new(0));
        let counter = pings.clone();
        tokio::spawn(async move {
            // Just reading is enough: tungstenite answers each Ping itself.
            while let Some(Ok(msg)) = far.next().await {
                if msg.is_ping() { counter.fetch_add(1, Ordering::SeqCst); }
            }
        });
        let session = tokio::time::timeout(
            Duration::from_millis(600),
            run_peer_session(state.clone(), FederationPeerConfig::default(), near, None, Some(FAST), None),
        ).await;
        assert!(session.is_err(), "a link that answers pings must stay up");
        assert!(pings.load(Ordering::SeqCst) >= 2, "pinged every interval");
        assert_eq!(state.inner.read().await.clients.len(), 1);
    }
}
