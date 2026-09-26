//! Bridge between the SIP subsystem and the Brew/TETRA core.
//!
//! This is the single place the two subsystems meet. It maps a routed SIP call
//! onto a Brew call: a SIP->Brew-private call becomes a Brew private
//! (individual) call setup toward the ISSI's registered Basestation, and a
//! SIP->Brew-group call becomes a group transmission to a GSSI reaching the
//! affiliated basestation mobile stations and brew mobile clients.
//!
//! Media reality check: TETRA carries ACELP voice inside Brew traffic frames,
//! while SIP legs here are steered to G.711 (PCMU/PCMA). To bridge media end
//! to end we register a *virtual* Brew client for the duration of the call: it
//! has no socket of its own, but sits in `AppState` exactly like a real
//! Basestation connection (in `clients`, as an `ActiveCall` participant, and
//! for group calls in `group_clients`), so the existing router delivers voice
//! frames to it like any other peer. A `transcode::task` owns that virtual
//! client's receive side on one end and the SIP RTP leg on the other, running
//! the vendored ACELP codec (`transcode::acelp`) and G.711 (`transcode::g711`)
//! between them.

use crate::protocol::{self, ConnVersion};
use crate::sip::media::{RtpLeg, RtpRelay, Sdp};
use crate::sip::message::{extract_uri, uri_user, SipMessage};
use crate::sip::state::SipCall;
use crate::sip::transport::SipTransport;
use crate::state::{ActiveCall, AppState, CallKind, Client, ClientId, ClientMode};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

/// Identifies the real Brew call/participant a Brew->SIP bridge is placed on
/// behalf of, so `place_outbound` can hook a transcoder into the same
/// `ActiveCall` id the subscriber's traffic frames are already tagged with.
pub struct BrewCallLink {
    pub call_id: uuid::Uuid,
    pub client: ClientId,
    /// The originating ISSI, used as the SIP From/Contact user part (`sip:
    /// <issi>@host`) instead of a generic literal, so the far end sees a
    /// real caller identity to route/display/dial back to.
    pub source_issi: u32,
}

/// Enough of a SIP dialog to send a best-effort in-dialog BYE toward the
/// other party when the *Brew* side hangs up first (rather than the usual
/// SIP BYE/CANCEL). Dialog tracking in this bridge is intentionally minimal
/// (see `terminate_to_extension`'s note on the same simplification): this
/// reuses the exact From/To header values already exchanged rather than
/// tracking full RFC 3261 dialog state (route sets, remote CSeq, etc).
#[derive(Clone)]
struct DialogBye {
    request_uri: String,
    target_addr: SocketAddr,
    from: String,
    to: String,
}

/// Bookkeeping for one bridged call's virtual Brew participant + transcoder,
/// so it can be torn down when the SIP call ends (BYE/CANCEL) or when the
/// Brew side ends it first (CALL_RELEASE/CALL_GROUP_IDLE).
struct BridgedLeg {
    virtual_client: ClientId,
    brew_call_id: uuid::Uuid,
    group: Option<u32>,
    /// `None` while a Brew-originated (place_outbound) leg is still ringing:
    /// the transcoder isn't started until the SIP peer answers and we know
    /// which codec it actually picked (see `pending_media`). Always `Some`
    /// immediately for a SIP-originated leg, where the codec is already
    /// known from the caller's own offer.
    task: Option<tokio::task::JoinHandle<()>>,
    /// Set when this bridge placed the outbound/inbound SIP dialog itself
    /// (i.e. every case here), so a Brew-initiated hangup can notify the SIP
    /// peer instead of leaving its dialog dangling.
    bye: Option<DialogBye>,
    /// The real Brew subscriber to signal ringing/answered/rejected toward,
    /// for a Brew-originated (Brew->SIP) leg placed by `place_outbound`.
    /// `None` for a SIP-originated leg, where that signalling instead flows
    /// through the private-call-control task spawned by `sip_to_brew_private`.
    subscriber: Option<ClientId>,
    /// `subscriber`'s ISSI, needed as the `destination` field of the
    /// `CALL_CONNECT_REQUEST` `on_sip_response` sends it on answer. Only
    /// meaningful alongside `subscriber` (`Some` for the same leg kind).
    subscriber_issi: u32,
    /// Whether CALL_ALERT has already been sent to `subscriber`, so a
    /// retransmitted SIP 180 doesn't re-trigger it.
    rang: AtomicBool,
    /// Whether CALL_CONNECT_REQUEST/ACK has already been sent/processed, so a
    /// retransmitted SIP 200 doesn't re-trigger it.
    connected: AtomicBool,
    /// For a Brew-originated leg only: the RTP leg and Brew-side channel
    /// ends needed to start the transcoder, held here until `on_sip_response`
    /// sees the SIP peer's actual 200 OK SDP answer and knows which of
    /// PCMU/PCMA it picked -- rather than guessing PCMU upfront and getting
    /// it wrong whenever the peer answers PCMA (garbled audio one way,
    /// nothing intelligible the other, since encode/decode disagree with
    /// what's actually on the wire).
    pending_media: Option<PendingMedia>,
}

struct PendingMedia {
    leg: RtpLeg,
    virtual_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    brew_targets: Vec<mpsc::UnboundedSender<Vec<u8>>>,
}

/// Couples the SIP transport to the Brew core.
pub struct BrewBridge {
    app: Arc<AppState>,
    transport: Arc<SipTransport>,
    /// SIP Call-ID -> bridged-leg cleanup info, for calls this bridge placed
    /// (both directions). Entries are removed by `teardown`.
    legs: RwLock<std::collections::HashMap<String, BridgedLeg>>,
}

impl BrewBridge {
    pub fn new(app: Arc<AppState>, transport: Arc<SipTransport>) -> Self {
        Self { app, transport, legs: RwLock::new(std::collections::HashMap::new()) }
    }

    /// Negotiated G.711 payload type (0=PCMU, 8=PCMA) to run the transcoder
    /// at, matching whatever `Sdp::build`'s answer actually offered.
    fn transcoder_payload_type(payloads: &[u8]) -> u8 {
        payloads.iter().copied().find(|p| *p == 0 || *p == 8).unwrap_or(0)
    }

    /// Starts the transcoder for a Brew-originated leg once its SIP peer has
    /// actually answered, using the codec that answer's own SDP picked (not
    /// a guess made before we could know). No-op if this leg has no
    /// `pending_media` (already started, or a leg kind that starts
    /// immediately) or the answer's SDP doesn't parse.
    async fn start_pending_media(&self, call_id: &str, body: &[u8]) {
        let Some(offer) = Sdp::parse(body) else {
            warn!(%call_id, "SIP 200 OK had no parseable SDP; leaving media unstarted");
            return;
        };
        let payload_type = Self::transcoder_payload_type(&offer.payload_types);
        let mut legs = self.legs.write().await;
        let Some(leg) = legs.get_mut(call_id) else { return };
        let Some(pending) = leg.pending_media.take() else { return };
        if let Ok(addr) = format!("{}:{}", offer.connection_addr, offer.audio_port).parse::<SocketAddr>() {
            pending.leg.set_remote(addr).await;
        }
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let task = crate::transcode::task::spawn(
            pending.leg, payload_type, leg.brew_call_id, pending.virtual_rx, pending.brew_targets, control_tx,
        );
        leg.task = Some(task);
        info!(%call_id, payload_type, "media started at the SIP peer's actual answered codec");
    }

    /// Tears down a bridged call's virtual Brew participant: aborts the
    /// transcoder task, removes the virtual client from every place it was
    /// registered (connection table, active call, group affiliation), and
    /// notifies the real Brew participant(s) the call ended (a synthesized
    /// CALL_RELEASE/CALL_GROUP_IDLE) so their UI doesn't show a phantom
    /// in-progress call. Called from SIP BYE/CANCEL handling; safe to call
    /// for a call this bridge did not place (no-op).
    pub async fn teardown(&self, call_id: &str) {
        let Some(leg) = self.legs.write().await.remove(call_id) else { return };
        if let Some(task) = &leg.task { task.abort(); }
        let mut inner = self.app.inner.write().await;
        inner.clients.remove(&leg.virtual_client);
        let call = inner.calls.remove(&leg.brew_call_id);
        if let Some(gssi) = leg.group {
            if let Some(members) = inner.group_clients.get_mut(&gssi) {
                members.remove(&leg.virtual_client);
            }
            if inner.group_floor.get(&gssi) == Some(&leg.brew_call_id) {
                inner.group_floor.remove(&gssi);
            }
        }
        let notify = call.map(|call| {
            let mut targets = call.peers.clone();
            targets.insert(call.owner);
            targets.remove(&leg.virtual_client);
            let release_state = if call.kind == CallKind::Group { protocol::CALL_GROUP_IDLE } else { protocol::CALL_RELEASE };
            let msg = protocol::build_call_cause(release_state, &leg.brew_call_id, 0);
            let txs = targets.iter().filter_map(|c| inner.clients.get(c).map(|cl| cl.tx.clone())).collect::<Vec<_>>();
            (msg, txs)
        });
        drop(inner);
        if let Some((msg, txs)) = notify {
            for tx in txs { let _ = tx.send(msg.clone()); }
        }
        self.app.monitor.call_ended(leg.brew_call_id).await;
    }

    /// Called when the Brew core ends a call (CALL_RELEASE/CALL_GROUP_IDLE)
    /// that turns out to be one this bridge placed toward SIP: the Brew side
    /// has already been told (that's how we got here), so this only needs to
    /// notify the SIP peer with a best-effort BYE and clean up our side.
    pub async fn teardown_by_brew_call(&self, brew_call_id: uuid::Uuid) {
        let call_id = {
            let legs = self.legs.read().await;
            legs.iter().find(|(_, l)| l.brew_call_id == brew_call_id).map(|(k, _)| k.clone())
        };
        let Some(call_id) = call_id else { return };
        if let Some(bye) = self.legs.read().await.get(&call_id).and_then(|l| l.bye.clone()) {
            self.send_bye(&call_id, &bye).await;
        }
        // `call` is already gone from AppState (the Brew side removed it
        // before calling us), so this only aborts the task and cleans the
        // virtual-client bookkeeping; it will not re-notify Brew.
        self.teardown(&call_id).await;
        self.transport.state.end_call(&call_id).await;
    }

    /// Force-ends a call this bridge placed, in either direction, regardless
    /// of who or what triggered it (e.g. the SIP max-call-duration sweep in
    /// `transport::run`). Sends a best-effort BYE to the SIP peer if this leg
    /// stored dialog info, then runs the same cleanup/Brew-side notify as any
    /// other teardown. A no-op for a call this bridge did not place.
    pub async fn force_end(&self, call_id: &str) {
        if let Some(bye) = self.legs.read().await.get(call_id).and_then(|l| l.bye.clone()) {
            self.send_bye(call_id, &bye).await;
        }
        self.teardown(call_id).await;
    }

    async fn send_bye(&self, call_id: &str, d: &DialogBye) {
        use crate::sip::message::Method;
        let mut bye = SipMessage::new_request(Method::Bye, d.request_uri.clone());
        bye.push_header("Via", format!("SIP/2.0/UDP {};branch=z9hG4bK{}",
            self.transport.advertised_host, uuid::Uuid::new_v4().simple()));
        bye.push_header("Max-Forwards", "70");
        bye.push_header("From", d.from.clone());
        bye.push_header("To", d.to.clone());
        bye.push_header("Call-ID", call_id.to_string());
        bye.push_header("CSeq", "2 BYE");
        self.transport.send_to(&bye, d.target_addr).await;
        info!(%call_id, "Brew hangup: sent SIP BYE");
    }

    /// Drives a Brew-originated (Brew->SIP) private call's ringing/answer
    /// signalling from the SIP response to our outbound INVITE (see
    /// `place_outbound`): a provisional response sends CALL_ALERT to the
    /// originating ISSI, 200 OK sends CALL_CONNECT_CONFIRM (plus the SIP ACK
    /// this dialog now needs to stay up), and a failure response releases the
    /// Brew side and cleans up. No-op for a call this bridge did not place as
    /// a Brew-originated leg (`subscriber` unset), e.g. a plain SIP-SIP relay.
    pub async fn on_sip_response(&self, call_id: &str, code: u16, to_header: Option<&str>, peer: SocketAddr, body: &[u8]) {
        let (subscriber, subscriber_issi, brew_call_id, already_rang, already_connected, bye) = {
            let legs = self.legs.read().await;
            let Some(leg) = legs.get(call_id) else { return };
            let Some(subscriber) = leg.subscriber else { return };
            (subscriber, leg.subscriber_issi, leg.brew_call_id, leg.rang.load(Ordering::Relaxed), leg.connected.load(Ordering::Relaxed), leg.bye.clone())
        };
        if already_connected { return; }
        let tx = {
            let inner = self.app.inner.read().await;
            inner.clients.get(&subscriber).map(|c| c.tx.clone())
        };
        let Some(tx) = tx else { return };

        match code {
            180 | 183 => {
                if !already_rang {
                    if let Some(leg) = self.legs.read().await.get(call_id) { leg.rang.store(true, Ordering::Relaxed); }
                    let _ = tx.send(protocol::build_call_control_empty(protocol::CALL_ALERT, &brew_call_id));
                    info!(%call_id, code, "SIP peer ringing (CALL_ALERT sent to ISSI)");
                }
            }
            200 => {
                if let Some(leg) = self.legs.read().await.get(call_id) { leg.connected.store(true, Ordering::Relaxed); }
                // CALL_CONNECT_CONFIRM here would go silently ignored: real
                // hardware (confirmed against FlowStation's cc_bs) only
                // honours it for a call where Brew/PSTN is the *calling*
                // party (calling_over_brew), i.e. an inbound SIP->Brew call.
                // For this direction -- the MS itself originated the call --
                // the message that actually drives the MS's own D-CONNECT
                // and flips its UI out of "calling..." is CALL_CONNECT_REQUEST.
                let _ = tx.send(protocol::build_circular_connect_request(&brew_call_id, 0, subscriber_issi, 0));
                self.transport.state.answer_call(call_id).await;
                self.start_pending_media(call_id, body).await;
                if let Some(d) = bye {
                    self.send_ack(call_id, &d, to_header, peer).await;
                }
                info!(%call_id, "SIP peer answered (CALL_CONNECT_REQUEST sent to ISSI)");
            }
            code if code >= 400 => {
                if let Some(leg) = self.legs.read().await.get(call_id) { leg.connected.store(true, Ordering::Relaxed); }
                let _ = tx.send(protocol::build_call_cause(protocol::CALL_RELEASE, &brew_call_id, 0));
                self.teardown(call_id).await;
                self.transport.state.end_call(call_id).await;
                warn!(%call_id, code, "SIP peer rejected/failed (CALL_RELEASE sent to ISSI)");
            }
            _ => {}
        }
    }

    /// Sends the SIP ACK a 200 OK to our own outbound INVITE requires (we are
    /// the UAC on this leg: without it, the peer keeps retransmitting the 200
    /// and eventually tears the dialog down). Reuses `place_outbound`'s stored
    /// Request-URI/From (see `DialogBye`'s note on this bridge's minimal
    /// dialog tracking) and the peer's own assigned To-tag from the response.
    async fn send_ack(&self, call_id: &str, d: &DialogBye, to_header: Option<&str>, peer: SocketAddr) {
        use crate::sip::message::Method;
        let mut ack = SipMessage::new_request(Method::Ack, d.request_uri.clone());
        ack.push_header("Via", format!("SIP/2.0/UDP {};branch=z9hG4bK{}",
            self.transport.advertised_host, uuid::Uuid::new_v4().simple()));
        ack.push_header("Max-Forwards", "70");
        ack.push_header("From", d.from.clone());
        ack.push_header("To", to_header.unwrap_or(&d.to).to_string());
        ack.push_header("Call-ID", call_id.to_string());
        ack.push_header("CSeq", "1 ACK");
        self.transport.send_to(&ack, peer).await;
    }

    /// Answers the SIP caller and sets up a Brew *private* call to `issi`.
    ///
    /// Signalling: we locate the Basestation that owns `issi` (its registered
    /// subscriber) and record an active call so the panel shows it. Media: we
    /// allocate one relay leg toward the SIP caller; the Brew side of the media
    /// path is where the ACELP<->PCM transcoder attaches.
    pub async fn sip_to_brew_private(
        &self,
        caller: SocketAddr,
        req: &SipMessage,
        issi: u32,
        offer: &Sdp,
        payloads: &[u8],
        call_id: &str,
    ) {
        // Is the target ISSI reachable (registered on some Basestation)?
        let target_client = {
            let inner = self.app.inner.read().await;
            inner.subscribers.get(&issi).map(|s| s.client_id)
        };
        let Some(target_client) = target_client else {
            warn!(issi, %call_id, "SIP->Brew private: ISSI not registered");
            let resp = self.transport.base_response_pub(req, 480, "Temporarily Unavailable");
            self.transport.send_to(&resp, caller).await;
            self.transport.state.end_call(call_id).await;
            return;
        };

        // Allocate the SIP-facing relay leg and latch the caller's media addr.
        let leg = match self.transport.relay.alloc_leg().await {
            Ok(l) => l,
            Err(_) => {
                let resp = self.transport.base_response_pub(req, 500, "Server Internal Error");
                self.transport.send_to(&resp, caller).await;
                self.transport.state.end_call(call_id).await;
                return;
            }
        };
        if let Ok(addr) = format!("{}:{}", offer.connection_addr, offer.audio_port).parse::<SocketAddr>() {
            leg.set_remote(addr).await;
        }
        self.transport.state.set_call_rtp(call_id, Some(leg.local_port), None).await;
        self.transport.state.track_media(call_id, leg.activity()).await;

        // Media bridge point: register a virtual Brew client standing in for
        // the SIP caller, wire it into an ActiveCall with the real ISSI's
        // client as its sole peer (so the router delivers that peer's voice
        // frames to our virtual client exactly like a normal private call),
        // ring the ISSI with a synthesized SETUP_REQUEST, and start the
        // transcoder between `leg` and the virtual client's frame channel.
        let brew_call_id = uuid::Uuid::new_v4();
        let virtual_client = uuid::Uuid::new_v4();
        let (virtual_tx, virtual_rx) = mpsc::unbounded_channel();
        let target_tx = {
            let mut inner = self.app.inner.write().await;
            inner.clients.insert(virtual_client, Client { tx: virtual_tx, mode: ClientMode::Terminal, version: ConnVersion::V1, remote_addr: None, connected_at_ms: crate::telemetry::now_ms(), username: None });
            inner.calls.insert(brew_call_id, ActiveCall {
                kind: CallKind::Private,
                owner: virtual_client,
                source_issi: 0,
                destination: issi,
                priority: 0,
                peers: HashSet::from([target_client]),
                started_at: std::time::Instant::now(),
                last_activity_ms: ActiveCall::new_activity(),
            });
            inner.clients.get(&target_client).map(|c| c.tx.clone())
        };
        if let Some(tx) = &target_tx {
            let setup = protocol::build_circular_call_setup(&brew_call_id, 0, issi, 0);
            let _ = tx.send(setup);
        }
        let local_port = leg.local_port;
        // Pre-build every response this dialog might need, but do not send
        // the 200 OK yet: sending it immediately (as this used to) answers
        // the SIP caller before the ISSI has even been told about the call,
        // so the caller gets no ringback and, if the callee never picks up,
        // no error either. Instead these are sent by the call-control task
        // below, driven by the ISSI's actual SETUP_ACCEPT/ALERT/CONNECT_REQUEST.
        //
        // All three MUST share one To-tag: base_response_pub generates a
        // fresh random one on every call (the request has none of its own),
        // so building them independently gave each response a different tag
        // -- whichever one we actually ended up sending established the real
        // dialog with the peer, but a later BYE built from a *different*
        // response's tag (e.g. `ok`'s, when only `ringing` was ever sent) is
        // for a dialog the peer never heard of, and gets 481'd. One tag,
        // reused everywhere, keeps the dialog identity consistent regardless
        // of which response actually goes out.
        let to_header = format!("{};tag={}", req.header("to").unwrap_or_default(), uuid::Uuid::new_v4().simple());
        let mut ringing = self.transport.base_response_pub(req, 180, "Ringing");
        let mut ok = self.transport.base_response_pub(req, 200, "OK");
        let mut reject = self.transport.base_response_pub(req, 486, "Busy Here");
        for resp in [&mut ringing, &mut ok, &mut reject] {
            resp.set_header("To", to_header.clone());
        }
        for resp in [&mut ringing, &mut ok] {
            resp.push_header("Contact", format!("<sip:brew@{}>", self.transport.advertised_host));
        }
        ok.push_header("Content-Type", "application/sdp");
        ok.body = Sdp::build(&self.transport.advertised_host, local_port, payloads);
        // Capture enough of this dialog (our to-tag, their from-tag) to send
        // a BYE toward `caller` later if the Brew side hangs up first.
        let bye = match (ok.header("to"), ok.header("from"), req.header("contact")) {
            (Some(to), Some(from), contact) => Some(DialogBye {
                request_uri: contact.map(extract_uri).unwrap_or_else(|| format!("sip:{caller}")),
                target_addr: caller,
                from: to.to_string(),
                to: from.to_string(),
            }),
            _ => None,
        };

        let payload_type = Self::transcoder_payload_type(payloads);
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let confirm_tx = target_tx.clone();
        let task = crate::transcode::task::spawn(
            leg, payload_type, brew_call_id, virtual_rx,
            target_tx.into_iter().collect(), control_tx,
        );
        self.legs.write().await.insert(call_id.to_string(), BridgedLeg {
            virtual_client, brew_call_id, group: None, task: Some(task), bye,
            subscriber: None, subscriber_issi: 0, rang: AtomicBool::new(false), connected: AtomicBool::new(false),
            pending_media: None,
        });

        // Same instance as `self` (this is how `sip_to_brew_private` is
        // reached in the first place -- see transport.rs's INVITE handling),
        // just re-fetched as an Arc so the spawned task can call back into it.
        if let Some(bridge) = self.transport.bridge.read().await.clone() {
            let handshake = PrivateCallHandshake { caller, brew_call_id, confirm_tx, ringing, ok, reject };
            spawn_private_call_control(bridge, call_id.to_string(), handshake, control_rx);
        }
        info!(issi, %call_id, "bridged SIP call to Brew private, awaiting ISSI accept/answer (media: ACELP<->G.711 transcoder active)");
    }

    /// Answers the SIP caller and sets up a Brew *group* call to `gssi`,
    /// reaching affiliated basestation mobile stations and brew mobile clients.
    pub async fn sip_to_brew_group(
        &self,
        caller: SocketAddr,
        req: &SipMessage,
        gssi: u32,
        offer: &Sdp,
        payloads: &[u8],
        call_id: &str,
    ) {
        // How many clients are affiliated to this group right now?
        let affiliated = {
            let inner = self.app.inner.read().await;
            inner.group_clients.get(&gssi).map(|c| c.len()).unwrap_or(0)
        };
        if affiliated == 0 && !self.app.config.fallback_broadcast_when_no_affiliations {
            warn!(gssi, %call_id, "SIP->Brew group: no affiliations");
            let resp = self.transport.base_response_pub(req, 480, "Temporarily Unavailable");
            self.transport.send_to(&resp, caller).await;
            self.transport.state.end_call(call_id).await;
            return;
        }

        let leg = match self.transport.relay.alloc_leg().await {
            Ok(l) => l,
            Err(_) => {
                let resp = self.transport.base_response_pub(req, 500, "Server Internal Error");
                self.transport.send_to(&resp, caller).await;
                self.transport.state.end_call(call_id).await;
                return;
            }
        };
        if let Ok(addr) = format!("{}:{}", offer.connection_addr, offer.audio_port).parse::<SocketAddr>() {
            leg.set_remote(addr).await;
        }
        self.transport.state.set_call_rtp(call_id, Some(leg.local_port), None).await;
        self.transport.state.track_media(call_id, leg.activity()).await;

        // Media bridge point: register a virtual Brew client as an affiliated
        // member of the group (so it keeps receiving that group's traffic for
        // the life of the call, exactly like a real Basestation), seize the
        // floor with a synthesized GROUP_TX toward the currently-affiliated
        // members, and start the transcoder between `leg` and the virtual
        // client's frame channel.
        let brew_call_id = uuid::Uuid::new_v4();
        let virtual_client = uuid::Uuid::new_v4();
        let (virtual_tx, virtual_rx) = mpsc::unbounded_channel();
        let target_txs = {
            let mut inner = self.app.inner.write().await;
            inner.clients.insert(virtual_client, Client { tx: virtual_tx, mode: ClientMode::Terminal, version: ConnVersion::V1, remote_addr: None, connected_at_ms: crate::telemetry::now_ms(), username: None });
            let members = inner.group_clients.entry(gssi).or_default();
            members.insert(virtual_client);
            let targets: HashSet<ClientId> = members.iter().copied().filter(|c| *c != virtual_client).collect();
            inner.calls.insert(brew_call_id, ActiveCall {
                kind: CallKind::Group,
                owner: virtual_client,
                source_issi: 0,
                destination: gssi,
                priority: 0,
                peers: targets.clone(),
                started_at: std::time::Instant::now(),
                last_activity_ms: ActiveCall::new_activity(),
            });
            inner.group_floor.insert(gssi, brew_call_id);
            targets.iter().filter_map(|c| inner.clients.get(c).map(|cl| cl.tx.clone())).collect::<Vec<_>>()
        };
        let seize = protocol::build_group_tx(&brew_call_id, 0, gssi, 0);
        for tx in &target_txs { let _ = tx.send(seize.clone()); }

        let local_port = leg.local_port;
        let mut ok = self.transport.base_response_pub(req, 200, "OK");
        ok.push_header("Contact", format!("<sip:brew@{}>", self.transport.advertised_host));
        ok.push_header("Content-Type", "application/sdp");
        ok.body = Sdp::build(&self.transport.advertised_host, local_port, payloads);
        let bye = match (ok.header("to"), ok.header("from"), req.header("contact")) {
            (Some(to), Some(from), contact) => Some(DialogBye {
                request_uri: contact.map(extract_uri).unwrap_or_else(|| format!("sip:{caller}")),
                target_addr: caller,
                from: to.to_string(),
                to: from.to_string(),
            }),
            _ => None,
        };

        let payload_type = Self::transcoder_payload_type(payloads);
        // Group calls have no accept/ring/answer handshake in this protocol
        // (route_private_control only acts on CallKind::Private) -- members
        // just start receiving as soon as the floor is seized above, so there
        // is nothing for a control task to react to; the receiver is simply
        // dropped rather than spawning one that would never see traffic.
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let task = crate::transcode::task::spawn(leg, payload_type, brew_call_id, virtual_rx, target_txs, control_tx);
        self.legs.write().await.insert(call_id.to_string(), BridgedLeg {
            virtual_client, brew_call_id, group: Some(gssi), task: Some(task), bye,
            subscriber: None, subscriber_issi: 0, rang: AtomicBool::new(false), connected: AtomicBool::new(false),
            pending_media: None,
        });

        self.transport.send_to(&ok, caller).await;
        self.transport.state.answer_call(call_id).await;
        info!(gssi, affiliated, %call_id, "bridged SIP call to Brew group (media: ACELP<->G.711 transcoder active)");
    }

    /// Brew -> SIP direction: called by the Brew core when a TETRA subscriber or
    /// group originates a call whose destination resolves (via routes) to a SIP
    /// extension or trunk. Places the outbound SIP INVITE and tracks the call.
    ///
    /// This is invoked opportunistically; if SIP is disabled or no route
    /// matches, it is a no-op returning false.
    pub async fn brew_to_sip(
        &self,
        origin: crate::sip::routing::CallOrigin,
        dialled: &str,
        link: BrewCallLink,
    ) -> bool {
        let routes = &self.app.config.sip.routes;
        let Some((dest, route)) = crate::sip::routing::resolve(routes, &origin, dialled) else {
            return false;
        };
        info!(dialled = %dialled, route = %route.name, "Brew->SIP route matched");

        // Only extension/trunk destinations make sense coming from Brew.
        use crate::sip::state::LegEndpoint;
        let call_id = format!("brew-{}", uuid::Uuid::new_v4().simple());
        match dest.clone() {
            LegEndpoint::SipExtension { aor } => {
                let Some(reg) = self.transport.state.lookup_registration(&aor).await else {
                    warn!(aor = %aor, "Brew->SIP: destination extension not registered");
                    return false;
                };
                self.place_outbound(&call_id, origin.to_leg(), dest.clone(), reg.contact, reg.source, link).await;
                true
            }
            LegEndpoint::SipTrunk { trunk, number } => {
                let Some(tc) = self.app.config.sip.trunks.get(&trunk).cloned() else { return false };
                let Some(peer) = tc.remote_host.parse::<SocketAddr>().ok()
                    .or(self.transport.state.trunk_for_peer_addr(&trunk).await) else { return false };
                let host = tc.remote_host.split(':').next().unwrap_or(&tc.remote_host);
                let uri = format!("sip:{number}@{host}");
                self.place_outbound(&call_id, origin.to_leg(), dest.clone(), uri, peer, link).await;
                true
            }
            _ => false,
        }
    }

    /// Shared helper: allocate a relay leg, INVITE the SIP destination, track
    /// the call, and attach the transcoder. Media bridge point: registers a
    /// virtual Brew client as this call's SIP-side participant (peer of
    /// `link.client`, the real subscriber connection that originated the
    /// call), so the router delivers the subscriber's voice frames to it
    /// exactly like sip_to_brew_private's reverse case.
    async fn place_outbound(
        &self,
        call_id: &str,
        from: crate::sip::state::LegEndpoint,
        to: crate::sip::state::LegEndpoint,
        target_uri: String,
        target_addr: SocketAddr,
        link: BrewCallLink,
    ) {
        let leg = match self.transport.relay.alloc_leg().await {
            Ok(l) => l,
            Err(_) => { warn!(%call_id, "Brew->SIP: no RTP port"); return; }
        };
        self.transport.state.start_call(SipCall {
            call_id: call_id.to_string(),
            from,
            to,
            started_at_ms: now_ms(),
            answered_at_ms: None,
            state: "brew-originated".into(),
            rtp_a_port: Some(leg.local_port),
            rtp_b_port: None,
        }).await;
        self.transport.state.track_media(call_id, leg.activity()).await;

        let virtual_client = uuid::Uuid::new_v4();
        let (virtual_tx, virtual_rx) = mpsc::unbounded_channel();
        let brew_target_tx = {
            let mut inner = self.app.inner.write().await;
            inner.clients.insert(virtual_client, Client { tx: virtual_tx, mode: ClientMode::Terminal, version: ConnVersion::V1, remote_addr: None, connected_at_ms: crate::telemetry::now_ms(), username: None });
            inner.calls.insert(link.call_id, ActiveCall {
                kind: CallKind::Private,
                owner: link.client,
                source_issi: 0,
                destination: 0,
                priority: 0,
                peers: HashSet::from([virtual_client]),
                started_at: std::time::Instant::now(),
                last_activity_ms: ActiveCall::new_activity(),
            });
            inner.clients.get(&link.client).map(|c| c.tx.clone())
        };
        // Acknowledge the setup request so the originating ISSI's UI leaves
        // "dialling" for "ringing" state instead of waiting on nothing; actual
        // ringing (CALL_ALERT) and answer (CALL_CONNECT_CONFIRM) follow from
        // the SIP response in `on_sip_response` below.
        if let Some(tx) = &brew_target_tx {
            let _ = tx.send(protocol::build_call_control_empty(protocol::CALL_SETUP_ACCEPT, &link.call_id));
        }

        use crate::sip::message::Method;
        // Identify the call as the originating ISSI, not a generic "brew"
        // literal, so the far end (Asterisk/PSTN) sees a real caller identity
        // to display, route on, or dial back to. Falls back to "brew" only
        // for the degenerate case of an unknown/zero ISSI (shouldn't happen
        // for a real private call, since router::handle_private_setup always
        // has a source_issi by the time it builds a BrewCallLink).
        let caller_id = if link.source_issi != 0 { link.source_issi.to_string() } else { "brew".to_string() };
        let from_header = format!("<sip:{caller_id}@{}>;tag={}", self.transport.advertised_host, uuid::Uuid::new_v4().simple());
        let to_header = format!("<{}>", extract_uri(&target_uri));
        let mut invite = SipMessage::new_request(Method::Invite, target_uri.clone());
        invite.push_header("Via", format!("SIP/2.0/UDP {};branch=z9hG4bK{}",
            self.transport.advertised_host, uuid::Uuid::new_v4().simple()));
        invite.push_header("Max-Forwards", "70");
        invite.push_header("From", from_header.clone());
        invite.push_header("To", to_header.clone());
        invite.push_header("Call-ID", call_id.to_string());
        invite.push_header("CSeq", "1 INVITE");
        invite.push_header("Contact", format!("<sip:{caller_id}@{}>", self.transport.advertised_host));
        invite.push_header("Content-Type", "application/sdp");
        let offered_payloads = [0u8, 8, 101];
        invite.body = Sdp::build(&self.transport.advertised_host, leg.local_port, &offered_payloads);
        // Best-effort BYE target if the Brew side hangs up first: no remote
        // to-tag is tracked (see DialogBye's note), so `to_header` is reused
        // as-is rather than being a fully RFC 3261-correct dialog match.
        let bye = Some(DialogBye {
            request_uri: target_uri.clone(),
            target_addr,
            from: from_header,
            to: to_header,
        });

        // Which of PCMU/PCMA to run the transcoder at isn't known yet -- that
        // depends on what the SIP peer actually answers with, in its 200 OK's
        // SDP, not what we offered. Starting the transcoder now at a guessed
        // codec (this used to hardcode PCMU) means encode/decode silently
        // disagree with the real wire format whenever the peer picks PCMA:
        // garbled audio in one direction, unintelligible noise (heard as
        // near-silence once ACELP-encoded from garbage PCM) in the other.
        // So `leg`/`virtual_rx`/the target list are held in `pending_media`
        // and the transcoder is only started once `on_sip_response` sees the
        // real answer (its `200` case calls `start_pending_media`).
        self.legs.write().await.insert(call_id.to_string(), BridgedLeg {
            virtual_client, brew_call_id: link.call_id, group: None, task: None, bye,
            subscriber: Some(link.client), subscriber_issi: link.source_issi, rang: AtomicBool::new(false), connected: AtomicBool::new(false),
            pending_media: Some(PendingMedia { leg, virtual_rx, brew_targets: brew_target_tx.into_iter().collect() }),
        });

        self.transport.send_to(&invite, target_addr).await;
        info!(%call_id, uri = %target_uri, user = ?uri_user(&target_uri), "Brew->SIP INVITE sent (media: ACELP<->G.711 transcoder active)");
    }
}

/// Everything `spawn_private_call_control` needs to answer the SIP caller,
/// bundled to keep the spawn function's argument list short.
struct PrivateCallHandshake {
    caller: SocketAddr,
    brew_call_id: uuid::Uuid,
    /// Sends `CALL_CONNECT_CONFIRM` back to the ISSI once it presses accept.
    confirm_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    ringing: SipMessage,
    ok: SipMessage,
    reject: SipMessage,
}

/// Drives a SIP-originated private call's accept/ring/answer handshake from
/// the ISSI's call-control messages (forwarded here by the transcoder task's
/// `control_tx`, see `transcode::task::spawn`): SETUP_ACCEPT/ALERT sends SIP
/// 180 Ringing, CONNECT_REQUEST (the callee pressed accept) sends
/// CALL_CONNECT_CONFIRM back to the ISSI and SIP 200 OK to the caller,
/// SETUP_REJECT/RELEASE before answer sends SIP 486 and tears the call down.
/// Runs until one of those terminal outcomes or the channel closes (call
/// already torn down some other way, e.g. the caller sent BYE/CANCEL first).
fn spawn_private_call_control(
    bridge: Arc<BrewBridge>,
    call_id: String,
    h: PrivateCallHandshake,
    mut control_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    tokio::spawn(async move {
        let mut rang = false;
        while let Some(raw) = control_rx.recv().await {
            if raw.len() < 18 || raw[0] != protocol::CLASS_CALL_CONTROL { continue; }
            match raw[1] {
                protocol::CALL_SETUP_ACCEPT | protocol::CALL_ALERT => {
                    if !rang {
                        rang = true;
                        bridge.transport.send_to(&h.ringing, h.caller).await;
                        info!(%call_id, "ISSI ringing (SIP 180 sent)");
                    }
                }
                protocol::CALL_CONNECT_REQUEST => {
                    if let Some(tx) = &h.confirm_tx {
                        let _ = tx.send(protocol::build_call_connect_confirm(&h.brew_call_id, 0, 0));
                    }
                    bridge.transport.send_to(&h.ok, h.caller).await;
                    bridge.transport.state.answer_call(&call_id).await;
                    info!(%call_id, "ISSI answered (SIP 200 sent)");
                    return;
                }
                protocol::CALL_SETUP_REJECT | protocol::CALL_RELEASE => {
                    bridge.transport.send_to(&h.reject, h.caller).await;
                    bridge.teardown(&call_id).await;
                    bridge.transport.state.end_call(&call_id).await;
                    warn!(%call_id, "ISSI rejected/released before answer (SIP 486 sent)");
                    return;
                }
                _ => {}
            }
        }
    });
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[allow(unused_imports)]
use RtpRelay as _RtpRelayInUse; // keep the media import meaningful across cfgs

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the fix for a real-world bug: a Brew-originated call always ran
    /// its transcoder at PCMU, regardless of what the SIP peer actually
    /// answered with, causing garbled/one-way audio whenever the peer picked
    /// PCMA. `start_pending_media` must derive the codec from the real
    /// answer's SDP (via this same selection function `sip_to_brew_private`
    /// already uses on an inbound offer), not assume PCMU.
    #[test]
    fn transcoder_payload_type_honours_pcma_only_answer() {
        assert_eq!(BrewBridge::transcoder_payload_type(&[8]), 8, "PCMA-only answer must select PCMA");
        assert_eq!(BrewBridge::transcoder_payload_type(&[0]), 0, "PCMU-only answer must select PCMU");
        assert_eq!(BrewBridge::transcoder_payload_type(&[8, 101]), 8, "PCMA with telephone-event must still pick PCMA");
        assert_eq!(BrewBridge::transcoder_payload_type(&[]), 0, "no usable codec falls back to PCMU");
    }

    #[test]
    fn sdp_answer_with_pcma_only_parses_and_selects_pcma() {
        let sdp = b"v=0\r\no=- 1 1 IN IP4 10.0.0.5\r\ns=-\r\nc=IN IP4 10.0.0.5\r\nt=0 0\r\nm=audio 20000 RTP/AVP 8\r\na=rtpmap:8 PCMA/8000\r\n";
        let offer = Sdp::parse(sdp).expect("valid SDP");
        assert_eq!(BrewBridge::transcoder_payload_type(&offer.payload_types), 8);
    }
}
