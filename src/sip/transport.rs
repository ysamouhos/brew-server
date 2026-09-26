//! SIP UDP transport and the request handlers layered on it: a registrar for
//! extensions, a digest-authenticating UAS, a back-to-back user agent (B2BUA)
//! for INVITE handling with the RTP relay, and a UAC that registers outbound
//! trunks to their peers.
//!
//! The design is a single UDP socket serving all SIP signalling, with a small
//! amount of per-transaction state held in `SipState` plus a nonce cache. It is
//! deliberately compact rather than a complete RFC 3261 transaction state
//! machine; it handles the request/response flows the server actually needs
//! (REGISTER, INVITE/ACK/BYE/CANCEL, OPTIONS) and answers auth challenges.

use crate::config::{Config, SipTrunkConfig, TrunkDirection};
use crate::sip::auth;
use crate::sip::media::{negotiate_payloads, RtpRelay, Sdp};
use crate::sip::message::{extract_uri, uri_user, Method, SipMessage};
use crate::sip::routing::{self, CallOrigin};
use crate::sip::state::{
    LegEndpoint, Registration, SipCall, SipState, TrunkState, TrunkStatus,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn rand_hex(n: usize) -> String {
    // Cheap unique-ish token from a UUID; good enough for branch/nonce/tag.
    let mut s = String::new();
    while s.len() < n {
        s.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    s.truncate(n);
    s
}

/// `[sip] advertised_host` must be a bare host: it is written verbatim into
/// the SDP `o=`/`c=` connection-address lines (`c=IN IP4 {addr}`), which per
/// the SDP spec never carry a port. A common misconfiguration is copying the
/// `host:port` style used by `listen`/`remote_host` into this field, which
/// produces malformed SDP that peers like Asterisk reject outright (its
/// `netsock2.c` logs "Port disallowed in host:port" when asked to parse one
/// as a bare host). If the configured value parses as `host:port`, strip the
/// port and warn instead of silently emitting broken SDP.
/// Best-effort local IP detection for when `sip.listen`/`sip.advertised_host`
/// leave us with a wildcard bind address (`0.0.0.0`/`::`, the default: most
/// deployments listen on all interfaces). Opens a UDP "connection" to a
/// public address -- no packet is actually sent, this just asks the OS routing
/// table which local interface/IP would be used -- and reads back that local
/// address. Falls back to the wildcard string (with a loud warning) if this
/// fails, e.g. no route at all (offline host, sandboxed/isolated network).
fn detect_outbound_local_ip(wildcard: &str) -> String {
    match std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        s.connect("8.8.8.8:80")?;
        s.local_addr()
    }) {
        Ok(addr) => addr.ip().to_string(),
        Err(e) => {
            warn!(error = %e, "sip: could not detect an outbound-facing local IP; falling back to the wildcard bind address, which is NOT valid in SDP (peers cannot route RTP to it) -- set sip.advertised_host explicitly");
            wildcard.to_string()
        }
    }
}

fn sanitize_advertised_host(configured: &str) -> String {
    if let Some((host, port)) = configured.rsplit_once(':') {
        // Only strip a trailing :port, not an IPv6 literal (which has more
        // than one colon and isn't valid unbracketed in this field either,
        // but that's a separate, unrelated limitation of the IP4-only SDP
        // builder -- not this typo).
        if !host.contains(':') && port.parse::<u16>().is_ok() {
            warn!(configured, host, "sip.advertised_host has a port; only a bare host belongs here (it's written into SDP c= lines, which never carry a port) -- stripping the port");
            return host.to_string();
        }
    }
    configured.to_string()
}

/// Shared handle used by handlers and by the bridge/dashboard.
pub struct SipTransport {
    pub sock: Arc<UdpSocket>,
    pub state: Arc<SipState>,
    pub relay: Arc<RtpRelay>,
    pub config: Arc<Config>,
    pub advertised_host: String,
    /// Issued digest nonces -> issue time, for replay/expiry control.
    nonces: RwLock<HashMap<String, Instant>>,
    /// Active bridged RTP relay tasks by Call-ID, aborted on BYE/CANCEL.
    relay_tasks: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Optional hook into the Brew core for SIP<->Brew bridging.
    pub bridge: RwLock<Option<Arc<crate::sip::bridge::BrewBridge>>>,
}

impl SipTransport {
    /// Sends a datagram to a peer.
    pub async fn send_to(&self, msg: &SipMessage, dst: SocketAddr) {
        let bytes = msg.to_bytes();
        if let Err(e) = self.sock.send_to(&bytes, dst).await {
            warn!(error = %e, %dst, "SIP send failed");
        }
    }

    /// Issues a fresh nonce and remembers it.
    async fn new_nonce(&self) -> String {
        let nonce = rand_hex(32);
        self.nonces.write().await.insert(nonce.clone(), Instant::now());
        nonce
    }

    async fn nonce_valid(&self, nonce: &str) -> bool {
        let mut n = self.nonces.write().await;
        // Drop nonces older than 2 minutes as we go.
        n.retain(|_, t| t.elapsed() < Duration::from_secs(120));
        n.contains_key(nonce)
    }

    /// Copies the mandatory response headers (Via stack, From, To, Call-ID,
    /// CSeq) from a request into a response, adding a To-tag if absent.
    fn base_response(&self, req: &SipMessage, code: u16, reason: &str) -> SipMessage {
        let mut resp = SipMessage::new_response(code, reason);
        for via in req.headers("via") {
            resp.push_header("Via", via.to_string());
        }
        if let Some(from) = req.header("from") { resp.push_header("From", from.to_string()); }
        // Ensure the To header carries a tag (required on non-100 responses).
        if let Some(to) = req.header("to") {
            if req.to_tag().is_none() && code != 100 {
                resp.push_header("To", format!("{};tag={}", to, rand_hex(12)));
            } else {
                resp.push_header("To", to.to_string());
            }
        }
        if let Some(cid) = req.header("call-id") { resp.push_header("Call-ID", cid.to_string()); }
        if let Some(cseq) = req.header("cseq") { resp.push_header("CSeq", cseq.to_string()); }
        resp.push_header("Server", "brew-server");
        resp
    }

    /// Public wrapper around `base_response` for the bridge module, which builds
    /// SIP responses for calls it terminates onto the Brew side.
    pub fn base_response_pub(&self, req: &SipMessage, code: u16, reason: &str) -> SipMessage {
        self.base_response(req, code, reason)
    }
}

/// Entry point: binds the SIP socket, initializes trunk state, spawns the
/// outbound-trunk registration loops, and runs the receive loop.
pub async fn run(app: Arc<crate::state::AppState>) -> anyhow::Result<()> {
    let cfg = &app.config.sip;
    if !cfg.enabled {
        return Ok(());
    }
    let sock = Arc::new(UdpSocket::bind(cfg.listen).await?);
    let local = sock.local_addr()?;
    let advertised_host = if cfg.advertised_host.is_empty() {
        if local.ip().is_unspecified() {
            // `sip.listen = 0.0.0.0:PORT` (the default): local.ip() is
            // literally "0.0.0.0", which is unroutable and not a valid SDP
            // c=/o= address -- some UAs even read c=0.0.0.0 as "this stream
            // is on hold" (RFC 3264 5.1) and never send media at all. That
            // silently broke one whole direction of every SIP<->Brew call
            // (whichever leg's SDP we generate: our own outbound INVITE
            // offer, or our answer to an inbound one) until a real routable
            // address was detected here instead.
            detect_outbound_local_ip(&local.ip().to_string())
        } else {
            local.ip().to_string()
        }
    } else {
        sanitize_advertised_host(&cfg.advertised_host)
    };

    let state = Arc::new(SipState::new(true, local.to_string(), cfg.realm.clone()));
    let relay = Arc::new(RtpRelay::new(local.ip().to_string(), cfg.rtp_port_min, cfg.rtp_port_max));

    // Seed trunk status entries from config.
    for (name, tc) in &cfg.trunks {
        let peer_addr = tc.remote_host.parse::<SocketAddr>().ok();
        state.set_trunk(TrunkState {
            name: name.clone(),
            direction: format!("{:?}", tc.direction).to_lowercase(),
            remote_host: tc.remote_host.clone(),
            status: if tc.enabled { TrunkStatus::Down } else { TrunkStatus::Down },
            peer_addr,
            last_event_ms: now_ms(),
            detail: if tc.enabled { "provisioned".into() } else { "disabled".into() },
            active_calls: 0,
        }).await;
    }

    let transport = Arc::new(SipTransport {
        sock: sock.clone(),
        state: state.clone(),
        relay,
        config: Arc::new(app.config.clone()),
        advertised_host: advertised_host.clone(),
        nonces: RwLock::new(HashMap::new()),
        relay_tasks: Mutex::new(HashMap::new()),
        bridge: RwLock::new(None),
    });

    // Wire the Brew bridge so SIP<->TETRA routes can resolve.
    let bridge = Arc::new(crate::sip::bridge::BrewBridge::new(app.clone(), transport.clone()));
    *transport.bridge.write().await = Some(bridge);

    // Register this transport on the app so the dashboard can read SIP state.
    app.set_sip(state.clone(), transport.clone()).await;

    info!(listen = %local, advertised = %advertised_host, realm = %cfg.realm,
        extensions = cfg.extensions.len(), trunks = cfg.trunks.len(), routes = cfg.routes.len(),
        "SIP subsystem started");

    // Spawn registration loops for outbound trunks.
    for (name, tc) in &cfg.trunks {
        if tc.enabled && tc.direction == TrunkDirection::Outbound && !tc.remote_host.is_empty() {
            let t = transport.clone();
            let name = name.clone();
            let tc = tc.clone();
            tokio::spawn(async move { outbound_register_loop(t, name, tc).await; });
        }
    }

    // Periodic registration purge.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(15));
            loop { tick.tick().await; state.purge_expired().await; }
        });
    }

    // Periodic max-call-duration / media-inactivity sweep, disabled (never
    // spawned) when both limits are 0.
    let limit = Duration::from_secs(cfg.max_call_duration_seconds);
    let idle = Duration::from_secs(app.config.call_inactivity_timeout_seconds);
    if !limit.is_zero() || !idle.is_zero() {
        let transport = transport.clone();
        tokio::spawn(async move { call_duration_sweep_loop(transport, limit, idle).await; });
    }

    // Receive loop.
    let mut buf = vec![0u8; 65535];
    loop {
        let (n, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => { warn!(error = %e, "SIP recv error"); continue; }
        };
        let data = buf[..n].to_vec();
        let transport = transport.clone();
        tokio::spawn(async move {
            handle_datagram(transport, peer, data).await;
        });
    }
}

/// Dispatches one received datagram.
async fn handle_datagram(t: Arc<SipTransport>, peer: SocketAddr, data: Vec<u8>) {
    let Some(msg) = SipMessage::parse(&data) else {
        debug!(%peer, bytes = data.len(), "dropping unparseable SIP datagram");
        return;
    };
    if msg.is_request() {
        handle_request(t, peer, msg).await;
    } else {
        handle_response(t, peer, msg).await;
    }
}

async fn handle_request(t: Arc<SipTransport>, peer: SocketAddr, req: SipMessage) {
    let method = req.method().cloned().unwrap_or(Method::Other(String::new()));
    match method {
        Method::Register => handle_register(t, peer, req).await,
        Method::Invite => handle_invite(t, peer, req).await,
        Method::Ack => { /* ACK is absorbed; media already bridged on 200 */ }
        Method::Bye => handle_bye(t, peer, req).await,
        Method::Cancel => handle_cancel(t, peer, req).await,
        Method::Options => {
            let resp = t.base_response(&req, 200, "OK");
            t.send_to(&resp, peer).await;
        }
        other => {
            debug!(%peer, method = other.as_str(), "unhandled SIP method");
            let resp = t.base_response(&req, 405, "Method Not Allowed");
            t.send_to(&resp, peer).await;
        }
    }
}

/// Registrar: authenticates the extension and records/refreshes its binding.
async fn handle_register(t: Arc<SipTransport>, peer: SocketAddr, req: SipMessage) {
    let to_uri = req.header("to").map(extract_uri).unwrap_or_default();
    let Some(aor) = uri_user(&to_uri) else {
        let resp = t.base_response(&req, 400, "Bad Request");
        t.send_to(&resp, peer).await;
        return;
    };

    // Look up the provisioned extension. Unknown AORs are rejected.
    let Some(ext) = t.config.sip.extensions.get(&aor).cloned() else {
        warn!(%peer, aor = %aor, "REGISTER for unprovisioned extension");
        let resp = t.base_response(&req, 404, "Not Found");
        t.send_to(&resp, peer).await;
        return;
    };

    // Digest auth (unless the extension has an empty password).
    if !ext.password.is_empty() {
        let authed = match req.header("authorization") {
            Some(h) => {
                let params = auth::parse_params(h);
                let nonce = params.get("nonce").cloned().unwrap_or_default();
                t.nonce_valid(&nonce).await
                    && auth::verify_authorization(h, &ext.password, "REGISTER", &nonce)
            }
            None => false,
        };
        if !authed {
            let nonce = t.new_nonce().await;
            let mut resp = t.base_response(&req, 401, "Unauthorized");
            resp.push_header("WWW-Authenticate", auth::build_challenge(&t.config.sip.realm, &nonce));
            t.send_to(&resp, peer).await;
            return;
        }
    }

    // Determine requested expiry.
    let expires = req.expires().unwrap_or(t.config.sip.registration_ttl_seconds);
    let contact_hdr = req.header("contact").map(str::to_string).unwrap_or_default();

    if expires == 0 {
        // Unregister.
        t.state.remove_registration(&aor).await;
        info!(%peer, aor = %aor, "extension unregistered");
        let mut resp = t.base_response(&req, 200, "OK");
        if !contact_hdr.is_empty() { resp.push_header("Contact", format!("{contact_hdr};expires=0")); }
        t.send_to(&resp, peer).await;
        return;
    }

    let contact_uri = extract_uri(&contact_hdr);
    let reg = Registration {
        aor: aor.clone(),
        contact: contact_uri,
        source: peer,
        user_agent: req.header("user-agent").unwrap_or("").to_string(),
        expires_at_ms: now_ms() + expires * 1000,
        registered_at_ms: now_ms(),
        authenticated: !ext.password.is_empty(),
    };
    let is_new = t.state.upsert_registration(reg).await;
    info!(%peer, aor = %aor, expires, new = is_new, "extension registered");

    let mut resp = t.base_response(&req, 200, "OK");
    if !contact_hdr.is_empty() {
        resp.push_header("Contact", format!("{contact_hdr};expires={expires}"));
    }
    resp.push_header("Expires", expires.to_string());
    t.send_to(&resp, peer).await;
}

/// B2BUA INVITE handling: authenticate (for extensions), resolve the route,
/// allocate RTP relay legs, and place the outgoing leg to the destination.
async fn handle_invite(t: Arc<SipTransport>, peer: SocketAddr, req: SipMessage) {
    let call_id = req.call_id().unwrap_or("").to_string();
    let from_uri = req.header("from").map(extract_uri).unwrap_or_default();
    let to_uri = req.header("to").map(extract_uri).unwrap_or_default();
    let dialled = uri_user(&to_uri).unwrap_or_default();
    let from_user = uri_user(&from_uri).unwrap_or_default();

    // Provisional 100 Trying.
    let trying = t.base_response(&req, 100, "Trying");
    t.send_to(&trying, peer).await;

    // Determine the call origin: a known trunk peer, else a SIP extension.
    let origin = if let Some(trunk) = t.state.trunk_for_peer(&peer).await {
        CallOrigin::SipTrunk(trunk)
    } else {
        // Require the extension to be provisioned and (if it has a password)
        // authenticated before it may originate a call.
        if let Some(ext) = t.config.sip.extensions.get(&from_user).cloned() {
            if !ext.password.is_empty() {
                let authed = match req.header("authorization").or(req.header("proxy-authorization")) {
                    Some(h) => {
                        let params = auth::parse_params(h);
                        let nonce = params.get("nonce").cloned().unwrap_or_default();
                        t.nonce_valid(&nonce).await
                            && auth::verify_authorization(h, &ext.password, "INVITE", &nonce)
                    }
                    None => false,
                };
                if !authed {
                    let nonce = t.new_nonce().await;
                    let mut resp = t.base_response(&req, 407, "Proxy Authentication Required");
                    resp.push_header("Proxy-Authenticate", auth::build_challenge(&t.config.sip.realm, &nonce));
                    t.send_to(&resp, peer).await;
                    return;
                }
            }
            if !ext.allow_outbound {
                let resp = t.base_response(&req, 403, "Forbidden");
                t.send_to(&resp, peer).await;
                return;
            }
            CallOrigin::SipExtension(from_user.clone())
        } else {
            warn!(%peer, from = %from_user, "INVITE from unknown origin");
            let resp = t.base_response(&req, 403, "Forbidden");
            t.send_to(&resp, peer).await;
            return;
        }
    };

    // Resolve the destination through the route table.
    let Some((dest_leg, route)) = routing::resolve(&t.config.sip.routes, &origin, &dialled) else {
        info!(%peer, dialled = %dialled, "no matching voice route");
        let resp = t.base_response(&req, 404, "Not Found");
        t.send_to(&resp, peer).await;
        return;
    };
    info!(%peer, dialled = %dialled, route = %route.name, "routing INVITE");

    // Parse the caller's SDP offer.
    let Some(offer) = Sdp::parse(&req.body) else {
        let resp = t.base_response(&req, 488, "Not Acceptable Here");
        t.send_to(&resp, peer).await;
        return;
    };
    let payloads = negotiate_payloads(&offer.payload_types);

    // Track the call.
    t.state.start_call(SipCall {
        call_id: call_id.clone(),
        from: origin.to_leg(),
        to: dest_leg.clone(),
        started_at_ms: now_ms(),
        answered_at_ms: None,
        state: "routing".into(),
        rtp_a_port: None,
        rtp_b_port: None,
    }).await;

    // Dispatch by destination type.
    match dest_leg.clone() {
        LegEndpoint::SipExtension { aor } => {
            terminate_to_extension(t.clone(), peer, req, aor, offer, payloads, call_id).await;
        }
        LegEndpoint::SipTrunk { trunk, number } => {
            terminate_to_trunk(t.clone(), peer, req, trunk, number, offer, payloads, call_id).await;
        }
        LegEndpoint::BrewPrivate { issi } => {
            if let Some(bridge) = t.bridge.read().await.clone() {
                bridge.sip_to_brew_private(peer, &req, issi, &offer, &payloads, &call_id).await;
            } else {
                let resp = t.base_response(&req, 480, "Temporarily Unavailable");
                t.send_to(&resp, peer).await;
            }
        }
        LegEndpoint::BrewGroup { gssi } => {
            if let Some(bridge) = t.bridge.read().await.clone() {
                bridge.sip_to_brew_group(peer, &req, gssi, &offer, &payloads, &call_id).await;
            } else {
                let resp = t.base_response(&req, 480, "Temporarily Unavailable");
                t.send_to(&resp, peer).await;
            }
        }
        LegEndpoint::SipExternal { .. } => {
            let resp = t.base_response(&req, 404, "Not Found");
            t.send_to(&resp, peer).await;
        }
    }
}

/// Terminates a call to a locally-registered SIP extension by relaying media
/// and forwarding the INVITE to the registered contact.
async fn terminate_to_extension(
    t: Arc<SipTransport>, caller: SocketAddr, req: SipMessage,
    aor: String, offer: Sdp, payloads: Vec<u8>, call_id: String,
) {
    let Some(reg) = t.state.lookup_registration(&aor).await else {
        let resp = t.base_response(&req, 480, "Temporarily Unavailable");
        t.send_to(&resp, caller).await;
        t.state.end_call(&call_id).await;
        return;
    };

    // Allocate two relay legs: one toward the caller, one toward the callee.
    let (leg_a, leg_b) = match (t.relay.alloc_leg().await, t.relay.alloc_leg().await) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let resp = t.base_response(&req, 500, "Server Internal Error");
            t.send_to(&resp, caller).await;
            t.state.end_call(&call_id).await;
            return;
        }
    };
    // Point leg A at the caller's advertised media address.
    if let Ok(addr) = format!("{}:{}", offer.connection_addr, offer.audio_port).parse::<SocketAddr>() {
        leg_a.set_remote(addr).await;
    }
    t.state.set_call_rtp(&call_id, Some(leg_a.local_port), Some(leg_b.local_port)).await;
    t.state.track_media(&call_id, leg_a.activity()).await;
    t.state.track_media(&call_id, leg_b.activity()).await;

    // Build the outgoing INVITE to the callee with our relay's SDP (leg B).
    let mut invite = SipMessage::new_request(Method::Invite, reg.contact.clone());
    let branch = format!("z9hG4bK{}", rand_hex(16));
    invite.push_header("Via", format!("SIP/2.0/UDP {};branch={}", t.advertised_host, branch));
    invite.push_header("Max-Forwards", "70");
    let local_tag = rand_hex(12);
    invite.push_header("From", format!("<sip:{}@{}>;tag={}",
        uri_user(&extract_uri(req.header("from").unwrap_or(""))).unwrap_or_default(),
        t.advertised_host, local_tag));
    invite.push_header("To", format!("<{}>", reg.contact));
    invite.push_header("Call-ID", call_id.clone());
    invite.push_header("CSeq", "1 INVITE");
    invite.push_header("Contact", format!("<sip:brew@{}>", t.advertised_host));
    invite.push_header("Content-Type", "application/sdp");
    invite.body = Sdp::build(&t.advertised_host, leg_b.local_port, &payloads);

    // Bridge the two relay legs now; media latches when RTP starts flowing.
    let handle = RtpRelay::bridge(&leg_a, &leg_b);
    t.relay_tasks.lock().await.insert(call_id.clone(), handle);

    // NB: full end-to-end response correlation (ringing/answer relayed back to
    // the caller) requires dialog state we keep minimal here; we optimistically
    // answer the caller with our relay SDP so audio can flow once the callee
    // picks up. A production build would relay the callee's 180/200 through.
    let mut ok = t.base_response(&req, 200, "OK");
    ok.push_header("Contact", format!("<sip:brew@{}>", t.advertised_host));
    ok.push_header("Content-Type", "application/sdp");
    ok.body = Sdp::build(&t.advertised_host, leg_a.local_port, &payloads);
    t.send_to(&ok, caller).await;
    t.send_to(&invite, reg.source).await;
    t.state.answer_call(&call_id).await;
    info!(aor = %aor, %call_id, "bridged SIP call to extension");
}

/// Terminates a call outward to a SIP trunk (e.g. Asterisk).
async fn terminate_to_trunk(
    t: Arc<SipTransport>, caller: SocketAddr, req: SipMessage,
    trunk: String, number: String, offer: Sdp, payloads: Vec<u8>, call_id: String,
) {
    let Some(tc) = t.config.sip.trunks.get(&trunk).cloned() else {
        let resp = t.base_response(&req, 404, "Not Found");
        t.send_to(&resp, caller).await;
        t.state.end_call(&call_id).await;
        return;
    };
    // Resolve the trunk's peer address.
    let peer_addr = t.state.snapshot().await.trunks.iter()
        .find(|x| x.name == trunk).and_then(|x| x.peer_addr)
        .or_else(|| tc.remote_host.parse().ok());
    let Some(peer_addr) = peer_addr else {
        let resp = t.base_response(&req, 502, "Bad Gateway");
        t.send_to(&resp, caller).await;
        t.state.end_call(&call_id).await;
        return;
    };

    let (leg_a, leg_b) = match (t.relay.alloc_leg().await, t.relay.alloc_leg().await) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let resp = t.base_response(&req, 500, "Server Internal Error");
            t.send_to(&resp, caller).await;
            t.state.end_call(&call_id).await;
            return;
        }
    };
    if let Ok(addr) = format!("{}:{}", offer.connection_addr, offer.audio_port).parse::<SocketAddr>() {
        leg_a.set_remote(addr).await;
    }
    t.state.set_call_rtp(&call_id, Some(leg_a.local_port), Some(leg_b.local_port)).await;
    t.state.track_media(&call_id, leg_a.activity()).await;
    t.state.track_media(&call_id, leg_b.activity()).await;

    let host = tc.remote_host.split(':').next().unwrap_or(&tc.remote_host);
    let mut invite = SipMessage::new_request(Method::Invite, format!("sip:{number}@{host}"));
    let branch = format!("z9hG4bK{}", rand_hex(16));
    invite.push_header("Via", format!("SIP/2.0/UDP {};branch={}", t.advertised_host, branch));
    invite.push_header("Max-Forwards", "70");
    let user = if tc.username.is_empty() { trunk.clone() } else { tc.username.clone() };
    invite.push_header("From", format!("<sip:{}@{}>;tag={}", user, t.advertised_host, rand_hex(12)));
    invite.push_header("To", format!("<sip:{number}@{host}>"));
    invite.push_header("Call-ID", call_id.clone());
    invite.push_header("CSeq", "1 INVITE");
    invite.push_header("Contact", format!("<sip:{user}@{}>", t.advertised_host));
    invite.push_header("Content-Type", "application/sdp");
    invite.body = Sdp::build(&t.advertised_host, leg_b.local_port, &payloads);

    let handle = RtpRelay::bridge(&leg_a, &leg_b);
    t.relay_tasks.lock().await.insert(call_id.clone(), handle);

    let mut ok = t.base_response(&req, 200, "OK");
    ok.push_header("Contact", format!("<sip:brew@{}>", t.advertised_host));
    ok.push_header("Content-Type", "application/sdp");
    ok.body = Sdp::build(&t.advertised_host, leg_a.local_port, &payloads);
    t.send_to(&ok, caller).await;
    t.send_to(&invite, peer_addr).await;
    t.state.answer_call(&call_id).await;
    info!(trunk = %trunk, number = %number, %call_id, "bridged SIP call to trunk");
}

/// Ends SIP calls (plain SIP-SIP relays and Brew-bridged legs alike) that
/// have run longer than `limit`, or have received no RTP for `idle` once
/// answered (either is skipped when zero). Mirrors `handle_bye`'s cleanup (abort the
/// relay task if any, tear down a bridged leg via `BrewBridge::force_end`,
/// remove from `SipState`) so a timed-out call is torn down the same way a
/// real BYE would, not silently killed.
async fn call_duration_sweep_loop(t: Arc<SipTransport>, limit: Duration, idle: Duration) {
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    loop {
        ticker.tick().await;
        let now = now_ms();
        let limit_ms = limit.as_millis() as u64;
        let mut expired: Vec<(String, &str)> = if limit.is_zero() { Vec::new() } else {
            t.state.snapshot().await.active_calls.iter()
                .filter(|c| now.saturating_sub(c.started_at_ms) >= limit_ms)
                .map(|c| (c.call_id.clone(), "exceeded max duration"))
                .collect()
        };
        if !idle.is_zero() {
            for call_id in t.state.idle_calls(idle.as_millis() as u64).await {
                if !expired.iter().any(|(id, _)| *id == call_id) { expired.push((call_id, "no RTP (inactive)")); }
            }
        }
        for (call_id, why) in expired {
            warn!(%call_id, reason = why, "SIP call timed out; force-ending");
            if let Some(handle) = t.relay_tasks.lock().await.remove(&call_id) {
                handle.abort();
            }
            if let Some(bridge) = t.bridge.read().await.clone() {
                bridge.force_end(&call_id).await;
            }
            t.state.end_call(&call_id).await;
        }
    }
}

async fn handle_bye(t: Arc<SipTransport>, peer: SocketAddr, req: SipMessage) {
    let call_id = req.call_id().unwrap_or("").to_string();
    if let Some(handle) = t.relay_tasks.lock().await.remove(&call_id) {
        handle.abort();
    }
    if let Some(bridge) = t.bridge.read().await.clone() {
        bridge.teardown(&call_id).await;
    }
    t.state.end_call(&call_id).await;
    let resp = t.base_response(&req, 200, "OK");
    t.send_to(&resp, peer).await;
    info!(%call_id, "SIP call ended (BYE)");
}

async fn handle_cancel(t: Arc<SipTransport>, peer: SocketAddr, req: SipMessage) {
    let call_id = req.call_id().unwrap_or("").to_string();
    if let Some(handle) = t.relay_tasks.lock().await.remove(&call_id) {
        handle.abort();
    }
    if let Some(bridge) = t.bridge.read().await.clone() {
        bridge.teardown(&call_id).await;
    }
    t.state.end_call(&call_id).await;
    let resp = t.base_response(&req, 200, "OK");
    t.send_to(&resp, peer).await;
}

/// Handles responses to requests *we* originated (mainly outbound trunk
/// REGISTER challenges/confirmations).
async fn handle_response(t: Arc<SipTransport>, peer: SocketAddr, resp: SipMessage) {
    let Some((_, method)) = resp.cseq() else { return };
    let code = resp.status_code().unwrap_or(0);
    if method == "REGISTER" {
        // Correlate to a trunk by peer address.
        if let Some(trunk) = t.state.trunk_for_peer(&peer).await {
            match code {
                200 => t.state.update_trunk_status(&trunk, TrunkStatus::Up, "200 OK", Some(peer)).await,
                401 | 407 => { /* handled by the register loop which retries with auth */ }
                c => t.state.update_trunk_status(&trunk, TrunkStatus::Failed, format!("{c}"), Some(peer)).await,
            }
        }
    } else if method == "INVITE" {
        // A response to an INVITE *we* sent (place_outbound, Brew->SIP): drive
        // the originating ISSI's ringing/answer signalling from it. No-op for
        // a call this bridge didn't place as a Brew-originated leg.
        if let Some(call_id) = resp.header("call-id") {
            let call_id = call_id.to_string();
            let to_header = resp.header("to").map(|s| s.to_string());
            if let Some(bridge) = t.bridge.read().await.clone() {
                bridge.on_sip_response(&call_id, code, to_header.as_deref(), peer, &resp.body).await;
            }
        }
    }
    debug!(%peer, code, method = %method, "SIP response");
}

/// Outbound trunk registration loop: sends REGISTER, answers the 401/407
/// challenge with digest, and re-registers on the configured interval.
async fn outbound_register_loop(t: Arc<SipTransport>, name: String, tc: SipTrunkConfig) {
    let Ok(peer_addr) = tc.remote_host.parse::<SocketAddr>() else {
        warn!(trunk = %name, host = %tc.remote_host, "outbound trunk has unparseable remote_host");
        t.state.update_trunk_status(&name, TrunkStatus::Failed, "bad remote_host", None).await;
        return;
    };
    let interval = Duration::from_secs(tc.register_interval_seconds.max(30));
    let host = tc.remote_host.split(':').next().unwrap_or(&tc.remote_host).to_string();
    let user = if tc.username.is_empty() { name.clone() } else { tc.username.clone() };
    let mut cseq = 1u32;

    loop {
        t.state.update_trunk_status(&name, TrunkStatus::Registering, "sending REGISTER", Some(peer_addr)).await;
        let call_id = format!("{}-{}", name, rand_hex(8));

        // First REGISTER (unauthenticated) to obtain a challenge.
        let resp = register_once(&t, &host, peer_addr, &user, &call_id, cseq, None).await;
        cseq += 1;
        match resp {
            Some(r) if r.status_code() == Some(200) => {
                t.state.update_trunk_status(&name, TrunkStatus::Up, "200 OK", Some(peer_addr)).await;
                info!(trunk = %name, "outbound trunk registered");
            }
            Some(r) if matches!(r.status_code(), Some(401) | Some(407)) => {
                // Answer the challenge.
                let challenge_hdr = r.header("www-authenticate").or(r.header("proxy-authenticate"));
                if let Some(ch) = challenge_hdr {
                    let challenge = auth::parse_params(ch);
                    let uri = format!("sip:{host}");
                    let cnonce = rand_hex(16);
                    let authz = auth::build_authorization(&challenge, &user, &tc.password, "REGISTER", &uri, &cnonce, 1);
                    let hdr = if r.status_code() == Some(407) { "Proxy-Authorization" } else { "Authorization" };
                    let resp2 = register_once(&t, &host, peer_addr, &user, &call_id, cseq, Some((hdr, authz))).await;
                    cseq += 1;
                    match resp2.and_then(|x| x.status_code()) {
                        Some(200) => {
                            t.state.update_trunk_status(&name, TrunkStatus::Up, "200 OK (auth)", Some(peer_addr)).await;
                            info!(trunk = %name, "outbound trunk registered (after auth)");
                        }
                        Some(c) => {
                            t.state.update_trunk_status(&name, TrunkStatus::Failed, format!("{c} after auth"), Some(peer_addr)).await;
                            warn!(trunk = %name, code = c, "outbound trunk auth failed");
                        }
                        None => {
                            t.state.update_trunk_status(&name, TrunkStatus::Failed, "no response to auth", Some(peer_addr)).await;
                        }
                    }
                }
            }
            Some(r) => {
                let c = r.status_code().unwrap_or(0);
                t.state.update_trunk_status(&name, TrunkStatus::Failed, format!("{c}"), Some(peer_addr)).await;
            }
            None => {
                t.state.update_trunk_status(&name, TrunkStatus::Failed, "no response", Some(peer_addr)).await;
                warn!(trunk = %name, "outbound trunk REGISTER timed out");
            }
        }

        tokio::time::sleep(interval).await;
    }
}

/// Sends one REGISTER and waits (briefly) for a response, correlating by
/// Call-ID via a temporary receive. Because the main recv loop consumes all
/// datagrams, we instead do a short dedicated exchange on a throwaway socket so
/// the challenge/confirmation is captured deterministically.
async fn register_once(
    t: &Arc<SipTransport>, host: &str, peer: SocketAddr, user: &str,
    call_id: &str, cseq: u32, auth_hdr: Option<(&str, String)>,
) -> Option<SipMessage> {
    // Use a dedicated ephemeral socket for the trunk registration exchange so
    // the response is not swallowed by the shared recv loop.
    let sock = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let mut req = SipMessage::new_request(Method::Register, format!("sip:{host}"));
    let branch = format!("z9hG4bK{}", rand_hex(16));
    let local = sock.local_addr().ok()?;
    req.push_header("Via", format!("SIP/2.0/UDP {};branch={}", local, branch));
    req.push_header("Max-Forwards", "70");
    req.push_header("From", format!("<sip:{user}@{host}>;tag={}", rand_hex(12)));
    req.push_header("To", format!("<sip:{user}@{host}>"));
    req.push_header("Call-ID", call_id.to_string());
    req.push_header("CSeq", format!("{cseq} REGISTER"));
    req.push_header("Contact", format!("<sip:{user}@{}>", t.advertised_host));
    req.push_header("Expires", "300");
    if let Some((h, v)) = auth_hdr { req.push_header(h, v); }

    sock.send_to(&req.to_bytes(), peer).await.ok()?;
    let mut buf = vec![0u8; 65535];
    let recv = tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await;
    match recv {
        Ok(Ok((n, _))) => SipMessage::parse(&buf[..n]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_hex_has_requested_length() {
        assert_eq!(rand_hex(16).len(), 16);
        assert_eq!(rand_hex(40).len(), 40);
    }

    #[test]
    fn sanitize_advertised_host_strips_accidental_port() {
        // The exact misconfiguration that produced malformed SDP c= lines
        // Asterisk rejected ("Port disallowed in 10.31.175.162:5060").
        assert_eq!(sanitize_advertised_host("10.31.175.162:5060"), "10.31.175.162");
    }

    #[test]
    fn sanitize_advertised_host_leaves_bare_host_alone() {
        assert_eq!(sanitize_advertised_host("10.31.175.162"), "10.31.175.162");
        assert_eq!(sanitize_advertised_host("sip.example.com"), "sip.example.com");
    }

    #[test]
    fn sanitize_advertised_host_does_not_mangle_ipv6() {
        // Not a valid value for this field either (the SDP builder is IP4-only),
        // but it must not be misparsed as host:port and truncated.
        assert_eq!(sanitize_advertised_host("::1"), "::1");
    }

    #[test]
    fn detect_outbound_local_ip_never_returns_the_wildcard_when_a_route_exists() {
        // Reproduces the real-world bug: sip.listen = 0.0.0.0:PORT (the
        // default) with sip.advertised_host unset previously advertised the
        // literal string "0.0.0.0" in every SDP c=/o= line -- unroutable, and
        // read by some UAs as "this stream is on hold" (RFC 3264 5.1), which
        // silently broke one whole direction of every SIP<->Brew call. Any
        // sandboxed CI host still has *a* default route (even if just to a
        // link-local/private gateway), so this must never come back "0.0.0.0".
        let detected = detect_outbound_local_ip("0.0.0.0");
        assert_ne!(detected, "0.0.0.0", "must not advertise the unroutable wildcard address");
    }

    #[tokio::test]
    async fn nonce_roundtrip() {
        let sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let t = SipTransport {
            sock,
            state: Arc::new(SipState::new(true, "x".into(), "r".into())),
            relay: Arc::new(RtpRelay::new("127.0.0.1", 50000, 50010)),
            config: Arc::new(Config::default()),
            advertised_host: "127.0.0.1".into(),
            nonces: RwLock::new(HashMap::new()),
            relay_tasks: Mutex::new(HashMap::new()),
            bridge: RwLock::new(None),
        };
        let n = t.new_nonce().await;
        assert!(t.nonce_valid(&n).await);
        assert!(!t.nonce_valid("bogus").await);
    }
}
