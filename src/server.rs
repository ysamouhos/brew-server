use crate::{fedroute, router, state::{AppState, Client, ClientMode}};
use crate::protocol::ConnVersion;
use anyhow::Context;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, FromRequestParts, Path, State,
    },
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let base = normalized_path(&state.config.websocket_path);
    let session_route = format!("{}/session/{{token}}", base);
    let mut app = Router::new().route(&base, get(brew_discovery));
    let slash = format!("{}/", base);
    if slash != base { app = app.route(&slash, get(brew_discovery)); }
    let app = app
        .route(&session_route, get(brew_session_endpoint))
        .route("/healthz", get(|| async { "ok\n" }))
        .with_state(state.clone());

    if state.config.tls.enabled {
        let tls = &state.config.tls;
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            &tls.cert_path,
            &tls.key_path,
        )
        .await
        .with_context(|| {
            format!(
                "loading TLS cert {} and key {}",
                tls.cert_path.display(),
                tls.key_path.display()
            )
        })?;
        let listen = state.service_bind(state.config.listen);
        info!(%listen, websocket_path=%base, auth=state.config.auth.enabled, tls=true, "Brew server listening (TLS)");
        // TCP_NODELAY: see `nodelay_listener` below; axum-server's default
        // acceptor leaves Nagle on.
        let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(rustls_config)
            .acceptor(axum_server::accept::NoDelayAcceptor::new());
        axum_server::bind(listen)
            .acceptor(acceptor)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;
    } else {
        let listen = state.service_bind(state.config.listen);
        let listener = nodelay_listener(tokio::net::TcpListener::bind(listen).await?);
        info!(%listen, websocket_path=%base, auth=state.config.auth.enabled, tls=false, "Brew server listening");
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    }
    Ok(())
}

/// Wraps the Brew listener so every accepted connection gets `TCP_NODELAY`.
///
/// Voice is a small (~56 byte) WebSocket message every 60ms per call. With
/// Nagle on, a frame written while the previous one is still unacknowledged
/// is held back until that ACK arrives -- and a Basestation that only
/// receives during a call sends nothing to piggyback ACKs on, so they are
/// delayed (Linux: >= 40ms). Once RTT + delayed ACK exceeds the 60ms frame
/// interval, frames leave late and bunched, which the Basestation's TDMA
/// downlink hears as choppy/garbled audio. axum 0.8 does not set it itself.
fn nodelay_listener(listener: tokio::net::TcpListener) -> axum::serve::TapIo<tokio::net::TcpListener, fn(&mut tokio::net::TcpStream)> {
    use axum::serve::ListenerExt;
    fn set_nodelay(tcp: &mut tokio::net::TcpStream) {
        if let Err(e) = tcp.set_nodelay(true) {
            warn!(error = %e, "failed to set TCP_NODELAY on Brew connection");
        }
    }
    listener.tap_io(set_nodelay as fn(&mut tokio::net::TcpStream))
}

fn normalized_path(path: &str) -> String {
    let mut p = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    while p.len() > 1 && p.ends_with('/') { p.pop(); }
    p
}

/// HTTP header carrying the Brew protocol version, per the specification.
const X_BREW_VERSION: &str = "X-Brew-Version";
/// HTTP header carrying the Brew client mode (Terminal | Basestation).
const X_BREW_MODE: &str = "X-Brew-Mode";

fn brew_mode(headers: &HeaderMap) -> ClientMode {
    ClientMode::from_header(headers.get(X_BREW_MODE).and_then(|v| v.to_str().ok()))
}

/// The request's `User-Agent`, if it sent a non-empty one: what ties an
/// upgrade to its own discovery GET when `[auth]` is disabled.
fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::trim).filter(|ua| !ua.is_empty())
}

/// Validates the client's advertised `X-Brew-Version` header against the version
/// this server implements and returns the connection's seed version. A missing
/// header is accepted for backward compatibility and seeds `V0` (the version is
/// then resolved lazily from message content, exactly as real clients do). A
/// present but unsupported version yields `426 Upgrade Required` (as listed in
/// the spec's supported response codes).
fn check_brew_version(headers: &HeaderMap) -> Result<ConnVersion, Response> {
    let Some(raw) = headers.get(X_BREW_VERSION) else {
        debug!("no X-Brew-Version header; seeding V0 and detecting lazily");
        return Ok(ConnVersion::V0);
    };
    let requested = raw.to_str().ok().and_then(|s| s.trim().parse::<u8>().ok());
    match requested {
        // Any version from 1 up to the version we implement is accepted; we seed
        // the highest layout we mutually support.
        Some(v) if v >= 1 && v <= crate::protocol::BREW_PROTOCOL_VERSION => {
            Ok(ConnVersion::from_header_value(Some(v)))
        }
        Some(0) => Ok(ConnVersion::V0),
        other => {
            warn!(requested=?other, supported=crate::protocol::BREW_PROTOCOL_VERSION, "unsupported X-Brew-Version");
            let mut resp = (
                StatusCode::UPGRADE_REQUIRED,
                [(header::CONTENT_TYPE, "text/plain")],
                format!("Unsupported Brew version; this server implements version {}\n", crate::protocol::BREW_PROTOCOL_VERSION),
            ).into_response();
            if let Ok(v) = HeaderValue::from_str(&crate::protocol::BREW_PROTOCOL_VERSION.to_string()) {
                resp.headers_mut().insert(X_BREW_VERSION, v);
            }
            Err(resp)
        }
    }
}

/// Mode and seed version a WebSocket upgrade runs with. Brew clients announce
/// both on the discovery GET (`discovered`); FlowStation sends neither on the
/// upgrade itself, while brew-server peers up to 1.12 sent them only on the
/// upgrade. An X-Brew-Mode on the upgrade wins; the version is the higher of
/// the two, so neither request can demote the other, and `None` when neither
/// announced one (see `Client::version_announced`).
fn upgrade_mode_version(headers: &HeaderMap, discovered: Option<(ClientMode, Option<ConnVersion>)>) -> (ClientMode, Option<ConnVersion>) {
    let (mode, version) = discovered.unwrap_or_default();
    let mode = if headers.contains_key(X_BREW_MODE) { brew_mode(headers) } else { mode };
    // Only consulted when present, so a plain upgrade does not log the
    // "seeding V0" fallback for a version discovery already settled; a value
    // we cannot use announces nothing.
    let announced = if headers.contains_key(X_BREW_VERSION) { check_brew_version(headers).ok() } else { None };
    let version = match (version, announced) {
        (Some(v), Some(a)) if a.as_u8() > v.as_u8() => Some(a),
        (v, a) => v.or(a),
    };
    (mode, version)
}

/// Loop-safe federation negotiation for one inbound connection: Ok(Some(neighbour_id))
/// when both sides speak it, Ok(None) for a legacy peer, Basestation or Terminal,
/// Err(()) when the peer claims our own server id (a link to ourselves).
fn negotiate_federation(mode: ClientMode, headers: &HeaderMap, loop_safe: bool, own_id: u64) -> Result<Option<u64>, ()> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if mode != ClientMode::Peer || !loop_safe || !fedroute::federation_offered(header(fedroute::X_BREW_FEDERATION)) {
        return Ok(None);
    }
    match header(fedroute::X_BREW_SERVER_ID).and_then(fedroute::parse_server_id) {
        Some(id) if id == own_id => Err(()),
        neighbour => Ok(neighbour),
    }
}

/// Whether a `User-Agent` is a Tetra Dispatch console (`TetraDispatch/<version>`).
fn is_dispatch_console(user_agent: &str) -> bool {
    user_agent.trim_start().to_ascii_lowercase().starts_with("tetradispatch/")
}

pub(crate) async fn brew_discovery(
    State(state): State<Arc<AppState>>,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    request: Request<axum::body::Body>,
) -> Response {
    state.purge_ephemeral().await;
    let (mut parts, _body) = request.into_parts();
    let request_uri = parts.uri.path().to_string();

    // `None` when the request announced no version at all.
    let seed_version = match check_brew_version(&parts.headers) {
        Ok(v) => parts.headers.contains_key(X_BREW_VERSION).then_some(v),
        Err(resp) => return resp,
    };

    let mode = brew_mode(&parts.headers);

    let is_upgrade = parts.headers.get(header::UPGRADE).and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket")).unwrap_or(false);

    // Direct WS mode remains available only when Digest is disabled. A
    // Basestation announces its mode and version on the discovery GET and
    // upgrades without them, so pick up what the same address announced
    // there (see `discovery_hints`) -- only when the upgrade comes with the
    // same User-Agent, as FlowStation's does, so another client behind the
    // same NAT never inherits it. With no hint this is the upgrade's own
    // headers, as before.
    if is_upgrade && !state.config.auth.enabled {
        let hint = {
            let mut inner = state.inner.write().await;
            let agent = user_agent(&parts.headers);
            let ours = inner.discovery_hints.get(&remote_addr.ip()).is_some_and(|(_, hinted, _, _)| Some(hinted.as_str()) == agent);
            if ours { inner.discovery_hints.remove(&remote_addr.ip()).map(|(_, _, m, v)| (m, v)) } else { None }
        };
        let (mode, seed_version) = upgrade_mode_version(&parts.headers, hint);
        return upgrade_from_parts(state, &mut parts, mode, seed_version, remote_addr, None).await;
    }

    if state.config.auth.enabled {
        let Some(username) = verify_digest(&state, &parts.headers, "GET", &request_uri).await else {
            return digest_challenge(&state).await;
        };
        let token = Uuid::new_v4().simple().to_string();
        state.inner.write().await.auth_sessions.insert(token.clone(), (Instant::now(), mode, seed_version, Some(username)));
        let path = format!("{}/session/{}", normalized_path(&state.config.websocket_path), token);
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/plain"),
                (header::HeaderName::from_static("x-brew-version"), version_header_value()),
            ],
            path,
        ).into_response();
    }

    // Never for a peer: every brew-server announces Peer on the upgrade itself,
    // and an address alone must not make some other client a peer (full table
    // sync, relayed registrations).
    let announced = parts.headers.contains_key(X_BREW_MODE) || parts.headers.contains_key(X_BREW_VERSION);
    if announced && mode != ClientMode::Peer {
        if let Some(agent) = user_agent(&parts.headers) {
            let hint = (Instant::now(), agent.to_string(), mode, seed_version);
            state.inner.write().await.discovery_hints.insert(remote_addr.ip(), hint);
        }
    }
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain"),
            (header::HeaderName::from_static("x-brew-version"), version_header_value()),
        ],
        normalized_path(&state.config.websocket_path),
    ).into_response()
}

fn version_header_value() -> &'static str {
    // BREW_PROTOCOL_VERSION is a small constant; map it to a static string so it
    // can be used directly in the header array without allocation.
    match crate::protocol::BREW_PROTOCOL_VERSION {
        1 => "1",
        _ => "1",
    }
}

async fn brew_session_endpoint(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    request: Request<axum::body::Body>,
) -> Response {
    state.purge_ephemeral().await;
    if !state.config.auth.enabled { return StatusCode::NOT_FOUND.into_response(); }
    let valid = state.inner.read().await.auth_sessions.contains_key(&token);
    if !valid { return StatusCode::UNAUTHORIZED.into_response(); }

    let (mut parts, _body) = request.into_parts();
    let is_upgrade = parts.headers.get(header::UPGRADE).and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket")).unwrap_or(false);
    if !is_upgrade { return StatusCode::BAD_REQUEST.into_response(); }

    // Session URLs are single-use. The established WebSocket is the authenticated
    // session. Recover the mode, seed version and authenticated username
    // captured during the discovery GET; a Basestation's WebSocket handshake
    // carries neither the X-Brew-Mode/X-Brew-Version headers nor
    // Authorization. Older brew-server peers announce mode and version only
    // on the handshake, so those still count when present (see
    // `upgrade_mode_version`).
    let (stored_mode, stored_version, username) = state.inner.write().await.auth_sessions.remove(&token)
        .map(|(_, mode, ver, user)| (mode, ver, user))
        .unwrap_or_default();
    let (mode, seed_version) = upgrade_mode_version(&parts.headers, Some((stored_mode, stored_version)));
    upgrade_from_parts(state, &mut parts, mode, seed_version, remote_addr, username).await
}

async fn upgrade_from_parts(state: Arc<AppState>, parts: &mut axum::http::request::Parts, mode: ClientMode, seed_version: Option<ConnVersion>, remote_addr: SocketAddr, username: Option<String>) -> Response {
    // Decided on the upgrade's own headers, which every brew-server that
    // speaks it sends whether or not [auth] put a discovery GET in between.
    let own_id = state.inner.read().await.fed.self_id;
    let fed_neighbour = match negotiate_federation(mode, &parts.headers, state.config.federation.loop_safe, own_id) {
        Ok(neighbour) => neighbour,
        Err(()) => {
            warn!(%remote_addr, "federation peer presented this server's own id (a link to ourselves); refusing");
            return (StatusCode::CONFLICT, [(header::CONTENT_TYPE, "text/plain")], "Federation link to this server itself\n").into_response();
        }
    };
    let console = parts.headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).is_some_and(is_dispatch_console);
    // What the client says it is (FlowStation: "Bost FlowStation/v0.5.6-<hash>"),
    // shown on the dashboard.
    let agent = user_agent(&parts.headers).map(|a| a.chars().take(120).collect::<String>());
    match WebSocketUpgrade::from_request_parts(parts, &state).await {
        Ok(ws) => {
            let requested = parts.headers.get(header::SEC_WEBSOCKET_PROTOCOL).and_then(|v| v.to_str().ok()).unwrap_or_default();
            debug!(requested_subprotocol=requested, mode=mode.as_str(), seed_version=?seed_version.map(ConnVersion::as_u8), loop_safe=fed_neighbour.is_some(), "WebSocket upgrade request");
            let protocol = state.config.websocket_subprotocol.clone();
            let state_mn_enabled = state.networks.read().map(|c| c.enabled && !c.networks.is_empty()).unwrap_or(false);
            let mut response = ws.protocols([protocol])
                .on_upgrade(move |socket| client_session(state, socket, mode, seed_version, remote_addr, username, fed_neighbour, console, agent))
                .into_response();
            if mode == ClientMode::Basestation {
                // Tells a Basestation which Brew extensions it may use here;
                // one that sees no such header (TetraPack, Brandmeister) sends
                // none of them.
                // `mni` only while Multi Network is on (its main switch).
                let mni_on = state_mn_enabled;
                let offer = if mni_on { format!("{}, {}", crate::carriers::EXTENSIONS, crate::visitors::CAPABILITY) } else { crate::carriers::EXTENSIONS.to_owned() };
                if let Ok(v) = HeaderValue::from_str(&offer) { response.headers_mut().insert(crate::carriers::X_BREW_EXTENSIONS, v); }
            }
            if fed_neighbour.is_some() {
                // Accepting: the dialling side only switches to loop-safe mode
                // when it sees these in the 101.
                let headers = response.headers_mut();
                headers.insert(fedroute::X_BREW_FEDERATION, HeaderValue::from_static("1"));
                if let Ok(id) = HeaderValue::from_str(&fedroute::format_server_id(own_id)) {
                    headers.insert(fedroute::X_BREW_SERVER_ID, id);
                }
            }
            response
        }
        Err(rejection) => rejection.into_response(),
    }
}

async fn digest_challenge(state: &Arc<AppState>) -> Response {
    let nonce = Uuid::new_v4().simple().to_string();
    state.inner.write().await.digest_nonces.insert(nonce.clone(), Instant::now());
    let opaque = md5_hex(&format!("{}:brew", state.config.auth.realm));
    let challenge = format!("Digest realm=\"{}\", nonce=\"{}\", qop=\"auth\", opaque=\"{}\"",
        state.config.auth.realm, nonce, opaque);
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    response.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_str(&challenge).unwrap());
    response
}

fn md5_hex(input: &str) -> String { format!("{:x}", md5::compute(input.as_bytes())) }

fn parse_digest(header_value: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let value = header_value.trim().strip_prefix("Digest ").unwrap_or(header_value.trim());
    for part in value.split(',') {
        if let Some((k, v)) = part.trim().split_once('=') {
            out.insert(k.trim().to_ascii_lowercase(), v.trim().trim_matches('"').to_string());
        }
    }
    out
}

/// Maximum number of digits allowed in a Brew (Basestation) username. TETRA
/// subscriber identities used as Brew usernames are constrained to at most 7
/// decimal digits.
const MAX_BREW_USERNAME_DIGITS: usize = 7;

/// A Brew username must be non-empty, all decimal digits, and at most
/// `MAX_BREW_USERNAME_DIGITS` long.
pub(crate) fn is_valid_brew_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= MAX_BREW_USERNAME_DIGITS
        && username.bytes().all(|b| b.is_ascii_digit())
}

/// Verifies a Digest `Authorization` header and, on success, returns the
/// authenticated Brew username -- the caller threads it through so the
/// connection's `Client.username` can be matched against `[bts_locations]`.
async fn verify_digest(state: &Arc<AppState>, headers: &HeaderMap, method: &str, expected_uri: &str) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())?;
    if !value.starts_with("Digest ") { return None; }
    let p = parse_digest(value);
    let username = p.get("username")?;
    if !is_valid_brew_username(username) {
        warn!(username = %username, "rejecting Brew auth: username must be 1-7 digits");
        return None;
    }
    let password = state.logins.brew.read().ok().and_then(|u| u.get(username).cloned())?;
    let nonce = p.get("nonce")?;
    if !state.inner.read().await.digest_nonces.contains_key(nonce) { return None; }
    let realm = p.get("realm").map(String::as_str).unwrap_or("");
    if realm != state.config.auth.realm { return None; }
    let uri = p.get("uri").map(String::as_str).unwrap_or("");
    if uri != expected_uri { return None; }
    let received = p.get("response")?;

    let ha1 = md5_hex(&format!("{}:{}:{}", username, realm, password));
    let ha2 = md5_hex(&format!("{}:{}", method, uri));
    let expected = if p.get("qop").map(|s| s.contains("auth")).unwrap_or(false) {
        let nc = p.get("nc").map(String::as_str).unwrap_or("");
        let cnonce = p.get("cnonce").map(String::as_str).unwrap_or("");
        md5_hex(&format!("{}:{}:{}:{}:auth:{}", ha1, nonce, nc, cnonce, ha2))
    } else {
        md5_hex(&format!("{}:{}:{}", ha1, nonce, ha2))
    };
    let ok = expected.eq_ignore_ascii_case(received);
    if !ok { return None; }
    state.inner.write().await.digest_nonces.remove(nonce);
    Some(username.clone())
}

async fn client_session(state: Arc<AppState>, socket: WebSocket, mode: ClientMode, seed_version: Option<ConnVersion>, remote_addr: SocketAddr, username: Option<String>, fed_neighbour: Option<u64>, console: bool, agent: Option<String>) {
    let id = Uuid::new_v4();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let connected_at_ms = crate::telemetry::now_ms();
    // An inbound federation peer link gets this server's full table too:
    // `federation::run`'s outbound dial side syncs its own state to us once
    // connected, but that is only half the story -- an inbound-only link (or
    // one that reconnects from the far end) would otherwise never learn about
    // registrations that predate it.
    let version = seed_version.unwrap_or_default();
    let client = Client { tx, mode, version, version_announced: seed_version.is_some(), remote_addr: Some(remote_addr), connected_at_ms, username: username.clone() };
    crate::federation::attach_client(&state, id, client, fed_neighbour).await;
    if console {
        state.inner.write().await.consoles.insert(id);
    }
    if let Some(agent) = agent {
        state.inner.write().await.client_agents.insert(id, agent);
    }
    info!(%id, console, mode=mode.as_str(), version=version.as_u8(), version_announced=seed_version.is_some(), %remote_addr, username=username.as_deref().unwrap_or(""),
        loop_safe_neighbour=fed_neighbour.map(fedroute::format_server_id).unwrap_or_default(), "Basestation connected");

    // Every link is pinged and closed once silent (see `federation::Keepalive`).
    // Without this a Basestation or Terminal whose peer vanished without a FIN
    // (NAT/proxy expiry, power loss) stays "connected" until TCP gives up, an
    // idle socket sends nothing, so it lingers for days on the dashboard.
    // Standard WebSocket stacks answer pings on their own.
    let keepalive = crate::federation::Keepalive::from_config(&state.config.federation);
    let mut ping = keepalive.map(crate::federation::Keepalive::ping_timer);
    let writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                packet = rx.recv() => {
                    let Some(packet) = packet else { break };
                    if ws_tx.send(Message::Binary(packet.into())).await.is_err() { break; }
                }
                _ = crate::federation::ping_due(&mut ping) => {
                    if ws_tx.send(Message::Ping(Default::default())).await.is_err() { break; }
                }
            }
        }
    });

    loop {
        let item = match crate::federation::next_frame(&mut ws_rx, keepalive).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => { warn!(%id, %remote_addr, "link silent past keepalive timeout; closing"); break; }
        };
        match item {
            Ok(Message::Binary(data)) => router::handle_packet(state.clone(), id, data.to_vec()).await,
            Ok(Message::Ping(_)) => debug!(%id, "ping received"),
            Ok(Message::Pong(_)) => debug!(%id, "pong received"),
            Ok(Message::Close(_)) => break,
            Ok(Message::Text(_)) => warn!(%id, "text WebSocket message ignored"),
            Err(e) => { warn!(%id, error=%e, "WebSocket receive error"); break; }
        }
    }

    writer.abort();
    state.cleanup_client(id).await;
    info!(%id, "Basestation disconnected");
}

#[cfg(test)]
mod tests {
    use super::{is_valid_brew_username, negotiate_federation, upgrade_mode_version, ClientMode, ConnVersion, HeaderMap};

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs { h.insert(*k, v.parse().unwrap()); }
        h
    }

    #[test]
    fn plain_upgrade_keeps_what_discovery_announced() {
        // FlowStation: mode/version on the discovery GET only.
        let discovered = Some((ClientMode::Terminal, Some(ConnVersion::V1)));
        assert_eq!(upgrade_mode_version(&headers(&[]), discovered), (ClientMode::Terminal, Some(ConnVersion::V1)));
        assert_eq!(upgrade_mode_version(&headers(&[]), None), (ClientMode::Basestation, None));
    }

    #[test]
    fn only_an_x_brew_version_announces_a_version() {
        // FlowStation without digest credentials: no discovery GET, a bare upgrade.
        assert_eq!(upgrade_mode_version(&headers(&[("X-Brew-Mode", "Basestation")]), None), (ClientMode::Basestation, None));
        // An explicit v0, on either request.
        assert_eq!(upgrade_mode_version(&headers(&[("X-Brew-Version", "0")]), None), (ClientMode::Basestation, Some(ConnVersion::V0)));
        assert_eq!(upgrade_mode_version(&headers(&[]), Some((ClientMode::Basestation, Some(ConnVersion::V0)))), (ClientMode::Basestation, Some(ConnVersion::V0)));
        // A value we cannot use is no announcement.
        assert_eq!(upgrade_mode_version(&headers(&[("X-Brew-Version", "99")]), None), (ClientMode::Basestation, None));
    }

    #[test]
    fn old_peer_announcing_on_upgrade_only_is_recognised() {
        // brew-server <= 1.12 dialling a server with [auth]: the discovery GET
        // carried nothing, so the session token holds the defaults.
        let upgrade = headers(&[("X-Brew-Mode", "Peer"), ("X-Brew-Version", "1")]);
        let discovered = Some((ClientMode::Basestation, None));
        assert_eq!(upgrade_mode_version(&upgrade, discovered), (ClientMode::Peer, Some(ConnVersion::V1)));
    }

    #[test]
    fn upgrade_never_demotes_the_discovered_version() {
        let upgrade = headers(&[("X-Brew-Version", "0")]);
        assert_eq!(upgrade_mode_version(&upgrade, Some((ClientMode::Peer, Some(ConnVersion::V1)))), (ClientMode::Peer, Some(ConnVersion::V1)));
        let bogus = headers(&[("X-Brew-Version", "99")]);
        assert_eq!(upgrade_mode_version(&bogus, Some((ClientMode::Peer, Some(ConnVersion::V1)))), (ClientMode::Peer, Some(ConnVersion::V1)));
    }

    #[test]
    fn negotiate_federation_only_between_loop_safe_peers() {
        let offer = headers(&[("X-Brew-Federation", "1"), ("X-Brew-Server-Id", "00a1b2c3d4e5f607")]);
        assert_eq!(negotiate_federation(ClientMode::Peer, &offer, true, 7), Ok(Some(0x00a1_b2c3_d4e5_f607)));
        // Not loop-safe here, not a peer, or no offer: a plain link.
        assert_eq!(negotiate_federation(ClientMode::Peer, &offer, false, 7), Ok(None));
        assert_eq!(negotiate_federation(ClientMode::Basestation, &offer, true, 7), Ok(None));
        assert_eq!(negotiate_federation(ClientMode::Terminal, &offer, true, 7), Ok(None));
        assert_eq!(negotiate_federation(ClientMode::Peer, &headers(&[]), true, 7), Ok(None));
    }

    #[test]
    fn negotiate_federation_needs_a_valid_offer() {
        let id = ("X-Brew-Server-Id", "00a1b2c3d4e5f607");
        for bad in [
            headers(&[("X-Brew-Federation", "0"), id]),
            headers(&[("X-Brew-Federation", "yes"), id]),
            headers(&[id]),
            headers(&[("X-Brew-Federation", "1")]),
            headers(&[("X-Brew-Federation", "1"), ("X-Brew-Server-Id", "a1b2c3d4e5f607")]),
            headers(&[("X-Brew-Federation", "1"), ("X-Brew-Server-Id", "00a1b2c3d4e5f60z")]),
        ] {
            assert_eq!(negotiate_federation(ClientMode::Peer, &bad, true, 7), Ok(None), "{bad:?}");
        }
        // A later version is still version 1 to us.
        let newer = headers(&[("X-Brew-Federation", "2"), id]);
        assert_eq!(negotiate_federation(ClientMode::Peer, &newer, true, 7), Ok(Some(0x00a1_b2c3_d4e5_f607)));
    }

    #[test]
    fn negotiate_federation_refuses_our_own_id() {
        let offer = headers(&[("X-Brew-Federation", "1"), ("X-Brew-Server-Id", "00a1b2c3d4e5f607")]);
        assert_eq!(negotiate_federation(ClientMode::Peer, &offer, true, 0x00a1_b2c3_d4e5_f607), Err(()));
    }

    #[tokio::test]
    async fn negotiate_federation_on_a_real_upgrade() {
        use super::{brew_discovery, get, AppState, Arc, SocketAddr};
        use std::time::Duration;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let mut cfg = crate::config::Config::default();
        cfg.storage.enabled = false;
        cfg.sms_center.enabled = false;
        cfg.federation.loop_safe = true;
        let state = Arc::new(AppState::new(cfg, "test.toml".into()).0);
        let own_id = state.inner.read().await.fed.self_id;
        let app = axum::Router::new().route("/brew", get(brew_discovery)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
        });
        let upgrade = |pairs: &[(&'static str, String)]| {
            let mut request = format!("ws://{addr}/brew").into_client_request().unwrap();
            request.headers_mut().insert("X-Brew-Mode", "Peer".parse().unwrap());
            for (k, v) in pairs { request.headers_mut().insert(*k, v.parse().unwrap()); }
            tokio_tungstenite::connect_async(request)
        };

        let (_link, resp) = upgrade(&[("X-Brew-Federation", "1".into()), ("X-Brew-Server-Id", "00000000000000aa".into())]).await.unwrap();
        assert_eq!(resp.headers()["X-Brew-Federation"], "1");
        assert_eq!(resp.headers()["X-Brew-Server-Id"], crate::fedroute::format_server_id(own_id).as_str());
        let (_legacy, resp) = upgrade(&[]).await.unwrap();
        assert!(!resp.headers().contains_key("X-Brew-Federation"), "an older peer is not answered with it");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while state.inner.read().await.clients.len() < 2 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let neighbours: Vec<u64> = state.inner.read().await.fed.links.values().copied().collect();
        assert_eq!(neighbours, vec![0xaa], "only the negotiated link is loop-safe");

        let ours = crate::fedroute::format_server_id(own_id);
        let err = upgrade(&[("X-Brew-Federation", "1".into()), ("X-Brew-Server-Id", ours)]).await.unwrap_err();
        assert!(err.to_string().contains("409"), "{err}");
    }

    #[tokio::test]
    async fn basestation_upgrade_offers_the_brew_extensions() {
        use super::{brew_discovery, get, AppState, SocketAddr};
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let state = AppState::for_test();
        let app = axum::Router::new().route("/brew", get(brew_discovery)).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
        });
        let upgrade = |mode: Option<&'static str>| {
            let mut request = format!("ws://{addr}/brew").into_client_request().unwrap();
            if let Some(mode) = mode { request.headers_mut().insert("X-Brew-Mode", mode.parse().unwrap()); }
            tokio_tungstenite::connect_async(request)
        };

        // FlowStation upgrades without any X-Brew header: a Basestation.
        let (_bs, resp) = upgrade(None).await.unwrap();
        assert_eq!(resp.headers()["X-Brew-Extensions"], "bts-info, handover-v1, ms-cell, ms-moved, ms-reselection, call-restore, call-handoff, cell-directory, group-access, issi-check");
        let (_peer, resp) = upgrade(Some("Peer")).await.unwrap();
        assert!(!resp.headers().contains_key("X-Brew-Extensions"));
        let (_terminal, resp) = upgrade(Some("Terminal")).await.unwrap();
        assert!(!resp.headers().contains_key("X-Brew-Extensions"));
    }

    #[test]
    fn dispatch_console_is_recognised_by_its_user_agent() {
        assert!(super::is_dispatch_console("TetraDispatch/1.0.0"));
        assert!(!super::is_dispatch_console("brew-server/1.16.0") && !super::is_dispatch_console("FlowStation/0.5"));
    }

    #[test]
    fn accepts_1_to_7_digits() {
        assert!(is_valid_brew_username("1"));
        assert!(is_valid_brew_username("1234567"));
        assert!(is_valid_brew_username("90"));
    }

    #[test]
    fn rejects_more_than_7_digits() {
        assert!(!is_valid_brew_username("12345678"));   // 8 digits
        assert!(!is_valid_brew_username("100000001"));  // old 9-digit example
    }

    #[test]
    fn rejects_empty_and_non_digits() {
        assert!(!is_valid_brew_username(""));
        assert!(!is_valid_brew_username("12a4567"));
        assert!(!is_valid_brew_username("bs1"));
        assert!(!is_valid_brew_username(" 123456"));
        assert!(!is_valid_brew_username("123-456"));
    }

    #[tokio::test]
    async fn silent_connections_are_dropped_but_answering_ones_stay() {
        use super::{brew_discovery, get, AppState, Arc, SocketAddr};
        use futures_util::StreamExt;
        use std::time::Duration;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let mut cfg = crate::config::Config::default();
        cfg.storage.enabled = false;
        cfg.sms_center.enabled = false;
        cfg.federation.keepalive_interval_seconds = 1;
        cfg.federation.keepalive_timeout_seconds = 2;
        let state = Arc::new(AppState::new(cfg, "test.toml".into()).0);
        let app = axum::Router::new().route("/brew", get(brew_discovery)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
        });

        // The peer and the first Basestation are never read from, so neither
        // answers a ping. The second Basestation is read, and the client stack
        // answers pings on its own.
        let mut peer_request = format!("ws://{addr}/brew").into_client_request().unwrap();
        peer_request.headers_mut().insert("X-Brew-Mode", "Peer".parse().unwrap());
        let (_peer, _) = tokio_tungstenite::connect_async(peer_request).await.unwrap();
        let (_silent, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/brew")).await.unwrap();
        let (mut answering, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/brew")).await.unwrap();
        tokio::spawn(async move { while answering.next().await.is_some() {} });

        let modes = || async {
            state.inner.read().await.clients.values().map(|c| c.mode).collect::<Vec<_>>()
        };
        let wait_for = |n: usize, within: Duration| async move {
            let deadline = tokio::time::Instant::now() + within;
            while modes().await.len() != n && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        wait_for(3, Duration::from_secs(2)).await;
        assert_eq!(modes().await.len(), 3, "all connections registered");
        wait_for(1, Duration::from_secs(6)).await;
        // Margin: had the answering one been timed out too, it would go now.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(modes().await, vec![ClientMode::Basestation]);
    }

    #[tokio::test]
    async fn without_auth_the_upgrade_takes_what_discovery_announced() {
        use super::{brew_discovery, get, AppState, SocketAddr};
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let state = AppState::for_test();
        let app = axum::Router::new().route("/brew", get(brew_discovery)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
        });
        let discover = |announce: &'static str| async move {
            let mut http = tokio::net::TcpStream::connect(addr).await.unwrap();
            http.write_all(format!("GET /brew HTTP/1.1\r\nHost: x\r\n{announce}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut response = Vec::new();
            http.read_to_end(&mut response).await.unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200"));
        };
        // A bare upgrade, with the given User-Agent: the mode and version it
        // was registered with (and whether the version was announced), once
        // it has gone again.
        let connect = |agent: Option<&'static str>| {
            let state = state.clone();
            async move {
                let mut request = format!("ws://{addr}/brew").into_client_request().unwrap();
                if let Some(agent) = agent { request.headers_mut().insert("User-Agent", agent.parse().unwrap()); }
                let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
                let registered = loop {
                    let clients: Vec<_> = state.inner.read().await.clients.values().map(|c| (c.mode, c.version, c.version_announced)).collect();
                    if !clients.is_empty() || tokio::time::Instant::now() >= deadline { break clients; }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                };
                // The software it named itself with is kept for the dashboard...
                let agents: Vec<String> = state.inner.read().await.client_agents.values().cloned().collect();
                assert_eq!(agents, agent.map(|a| vec![a.to_string()]).unwrap_or_default());
                drop(ws);
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !state.inner.read().await.clients.is_empty() { tokio::time::sleep(Duration::from_millis(20)).await; }
                }).await.expect("closed connection cleaned up");
                // ...and goes with the connection.
                assert!(state.inner.read().await.client_agents.is_empty());
                registered
            }
        };
        let defaults = vec![(ClientMode::Basestation, ConnVersion::V0, false)];

        // FlowStation: mode and version on the discovery GET, a bare upgrade
        // with the same User-Agent.
        discover("User-Agent: FlowStation/1\r\nX-Brew-Mode: Terminal\r\nX-Brew-Version: 1\r\n").await;
        // Another client behind the same address does not take it...
        assert_eq!(connect(Some("BlueStation/1")).await, defaults);
        assert_eq!(connect(None).await, defaults);
        // ...the one that announced it does, once.
        assert_eq!(connect(Some("FlowStation/1")).await, vec![(ClientMode::Terminal, ConnVersion::V1, true)]);
        assert_eq!(connect(Some("FlowStation/1")).await, defaults);

        // Nothing is kept for a client that does not name itself...
        discover("X-Brew-Mode: Terminal\r\nX-Brew-Version: 1\r\n").await;
        assert_eq!(connect(None).await, defaults);
        // ...nor for a peer, which announces Peer on its own upgrade.
        discover("User-Agent: brew-server/1\r\nX-Brew-Mode: Peer\r\nX-Brew-Version: 1\r\n").await;
        assert_eq!(connect(Some("brew-server/1")).await, defaults);
    }
}
