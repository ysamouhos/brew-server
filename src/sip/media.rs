//! Media handling for SIP calls: a tiny SDP parser/builder and a UDP RTP relay.
//!
//! The relay is a symmetric-RTP "latching" forwarder: for each call leg we open
//! one RTP socket from the configured port pool, learn the peer's real source
//! address from the first inbound packet (NAT-safe latching), and forward RTP
//! between the two legs of a bridged call. This lets SIP extensions, SIP trunks
//! and (via the bridge module) Brew endpoints exchange audio without the server
//! needing to transcode: both SIP legs are steered to a common codec at
//! offer/answer time.
//!
//! Transcoding TETRA ACELP <-> G.711 is out of scope here; the bridge module
//! documents where a codec shim would attach. The relay moves RTP payloads
//! verbatim, which is correct for SIP<->SIP trunking and for SIP<->Brew when a
//! gateway on the Brew side already speaks a SIP-compatible codec.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;

/// A minimal parsed SDP: the connection address and the first audio media port,
/// plus the raw payload-type list so we can echo a compatible answer.
#[derive(Debug, Clone)]
pub struct Sdp {
    /// Connection address from the `c=` line (session or media level).
    pub connection_addr: String,
    /// Audio media port from the `m=audio` line.
    pub audio_port: u16,
    /// Payload type numbers advertised on the `m=audio` line.
    pub payload_types: Vec<u8>,
}

impl Sdp {
    /// Parses the subset of SDP we need. Returns None if there is no audio media
    /// line or no usable connection address.
    pub fn parse(body: &[u8]) -> Option<Sdp> {
        let text = String::from_utf8_lossy(body);
        let mut session_conn: Option<String> = None;
        let mut media_conn: Option<String> = None;
        let mut audio_port: Option<u16> = None;
        let mut payload_types: Vec<u8> = Vec::new();
        let mut in_audio = false;

        for line in text.lines() {
            let line = line.trim_end();
            if let Some(rest) = line.strip_prefix("c=") {
                // c=IN IP4 1.2.3.4
                let addr = rest.split_whitespace().nth(2).map(str::to_string);
                if in_audio { media_conn = addr; } else { session_conn = addr; }
            } else if let Some(rest) = line.strip_prefix("m=audio ") {
                in_audio = true;
                let mut it = rest.split_whitespace();
                audio_port = it.next().and_then(|p| p.parse::<u16>().ok());
                // Skip the transport token (RTP/AVP), then collect payload types.
                let _ = it.next();
                for pt in it {
                    if let Ok(n) = pt.parse::<u8>() { payload_types.push(n); }
                }
            } else if line.starts_with("m=") {
                in_audio = false;
            }
        }

        let connection_addr = media_conn.or(session_conn)?;
        Some(Sdp {
            connection_addr,
            audio_port: audio_port?,
            payload_types,
        })
    }

    /// Builds an SDP body advertising our relay's address and port for one leg.
    /// `payload_types` should be the negotiated common set. We always include a
    /// telephone-event line when DTMF (PT 101) is present.
    pub fn build(relay_addr: &str, relay_port: u16, payload_types: &[u8]) -> Vec<u8> {
        let pts: Vec<String> = payload_types.iter().map(|p| p.to_string()).collect();
        let pt_list = if pts.is_empty() { "0".to_string() } else { pts.join(" ") };
        let mut sdp = format!(
            "v=0\r\n\
o=brew-server 0 0 IN IP4 {addr}\r\n\
s=brew\r\n\
c=IN IP4 {addr}\r\n\
t=0 0\r\n\
m=audio {port} RTP/AVP {pts}\r\n",
            addr = relay_addr, port = relay_port, pts = pt_list,
        );
        // Standard rtpmap lines for the codecs we commonly bridge.
        for pt in payload_types {
            match pt {
                0 => sdp.push_str("a=rtpmap:0 PCMU/8000\r\n"),
                8 => sdp.push_str("a=rtpmap:8 PCMA/8000\r\n"),
                101 => sdp.push_str("a=rtpmap:101 telephone-event/8000\r\na=fmtp:101 0-16\r\n"),
                _ => {}
            }
        }
        sdp.push_str("a=sendrecv\r\n");
        sdp.into_bytes()
    }
}

/// One relayed media leg: a UDP socket plus the (latched) remote address.
pub struct RtpLeg {
    pub local_port: u16,
    socket: Arc<UdpSocket>,
    remote: Arc<RwLock<Option<SocketAddr>>>,
    /// When RTP was last received on this leg (ms since epoch, 0 = never).
    last_rx_ms: Arc<AtomicU64>,
}

impl RtpLeg {
    /// Shared handle to this leg's last-RTP-received time, for the call
    /// inactivity sweep.
    pub fn activity(&self) -> Arc<AtomicU64> { self.last_rx_ms.clone() }

    /// Sets/overrides the remote address (e.g. from SDP before latching).
    pub async fn set_remote(&self, addr: SocketAddr) {
        *self.remote.write().await = Some(addr);
    }

    /// Receives one datagram, latching the sender as the remote (symmetric-RTP
    /// NAT traversal), same as `bridge()` does. Used by a transcoder that owns
    /// this leg directly instead of relaying it verbatim to another leg.
    pub async fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        let (n, from) = self.socket.recv_from(buf).await?;
        self.last_rx_ms.store(crate::telemetry::now_ms(), Ordering::Relaxed);
        *self.remote.write().await = Some(from);
        Ok(n)
    }

    /// Sends a datagram to the latched remote, if any (no-op otherwise: we
    /// have not heard from the peer yet).
    pub async fn send(&self, data: &[u8]) -> std::io::Result<()> {
        if let Some(dst) = *self.remote.read().await {
            self.socket.send_to(data, dst).await?;
        }
        Ok(())
    }
}

/// Manages RTP port allocation and spawns relay tasks. One instance per server.
pub struct RtpRelay {
    bind_host: String,
    port_min: u16,
    port_max: u16,
    next_port: RwLock<u16>,
}

impl RtpRelay {
    pub fn new(bind_host: impl Into<String>, port_min: u16, port_max: u16) -> Self {
        let port_min = port_min.max(1024);
        let port_max = port_max.max(port_min + 1);
        Self {
            bind_host: bind_host.into(),
            port_min,
            port_max,
            next_port: RwLock::new(port_min),
        }
    }

    /// Allocates and binds one RTP socket, returning a leg. RTP uses even ports
    /// by convention (odd is RTCP), so we step by two.
    pub async fn alloc_leg(&self) -> std::io::Result<RtpLeg> {
        let mut attempts = 0;
        loop {
            let port = {
                let mut np = self.next_port.write().await;
                let p = *np;
                let mut next = p.wrapping_add(2);
                if next >= self.port_max || next < self.port_min { next = self.port_min; }
                *np = next;
                p
            };
            match UdpSocket::bind(format!("{}:{}", self.bind_host, port)).await {
                Ok(sock) => {
                    return Ok(RtpLeg {
                        local_port: port,
                        socket: Arc::new(sock),
                        remote: Arc::new(RwLock::new(None)),
                        last_rx_ms: Arc::new(AtomicU64::new(0)),
                    });
                }
                Err(e) => {
                    attempts += 1;
                    if attempts > (self.port_max - self.port_min) / 2 {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Bridges two legs: RTP arriving on either socket is forwarded to the
    /// other's latched remote. Runs until the returned handle is aborted (call
    /// teardown). Latches each remote from the first packet seen, so it works
    /// through NAT even if the SDP address was private.
    pub fn bridge(a: &RtpLeg, b: &RtpLeg) -> tokio::task::JoinHandle<()> {
        let a_sock = a.socket.clone();
        let b_sock = b.socket.clone();
        let a_remote = a.remote.clone();
        let b_remote = b.remote.clone();
        let a_rx = a.last_rx_ms.clone();
        let b_rx = b.last_rx_ms.clone();

        tokio::spawn(async move {
            let mut buf_a = [0u8; 2048];
            let mut buf_b = [0u8; 2048];
            loop {
                tokio::select! {
                    r = a_sock.recv_from(&mut buf_a) => {
                        let Ok((n, from)) = r else { break };
                        a_rx.store(crate::telemetry::now_ms(), Ordering::Relaxed);
                        // Latch A's real source address.
                        { *a_remote.write().await = Some(from); }
                        if let Some(dst) = *b_remote.read().await {
                            let _ = b_sock.send_to(&buf_a[..n], dst).await;
                        }
                    }
                    r = b_sock.recv_from(&mut buf_b) => {
                        let Ok((n, from)) = r else { break };
                        b_rx.store(crate::telemetry::now_ms(), Ordering::Relaxed);
                        { *b_remote.write().await = Some(from); }
                        if let Some(dst) = *a_remote.read().await {
                            let _ = a_sock.send_to(&buf_b[..n], dst).await;
                        }
                    }
                }
            }
        })
    }
}

/// Chooses a common audio payload type between an offer and our supported set.
/// We support PCMU (0) and PCMA (8), the universal SIP baseline. Returns the
/// negotiated list (audio codec + telephone-event if offered).
pub fn negotiate_payloads(offer: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for pt in [0u8, 8u8] {
        if offer.contains(&pt) { out.push(pt); break; }
    }
    if out.is_empty() {
        // Fall back to PCMU; most gateways accept it.
        out.push(0);
    }
    if offer.contains(&101) { out.push(101); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER: &str = "v=0\r\n\
o=alice 1 1 IN IP4 10.0.0.5\r\n\
s=-\r\n\
c=IN IP4 10.0.0.5\r\n\
t=0 0\r\n\
m=audio 40000 RTP/AVP 0 8 101\r\n\
a=rtpmap:0 PCMU/8000\r\n\
a=rtpmap:101 telephone-event/8000\r\n";

    #[test]
    fn parses_offer() {
        let sdp = Sdp::parse(OFFER.as_bytes()).unwrap();
        assert_eq!(sdp.connection_addr, "10.0.0.5");
        assert_eq!(sdp.audio_port, 40000);
        assert_eq!(sdp.payload_types, vec![0, 8, 101]);
    }

    #[test]
    fn negotiates_common_codec() {
        assert_eq!(negotiate_payloads(&[0, 8, 101]), vec![0, 101]);
        assert_eq!(negotiate_payloads(&[8, 101]), vec![8, 101]);
        assert_eq!(negotiate_payloads(&[9]), vec![0]); // fallback to PCMU
    }

    #[test]
    fn builds_answer_with_rtpmap() {
        let ans = String::from_utf8(Sdp::build("1.2.3.4", 16000, &[0, 101])).unwrap();
        assert!(ans.contains("m=audio 16000 RTP/AVP 0 101\r\n"));
        assert!(ans.contains("a=rtpmap:0 PCMU/8000\r\n"));
        assert!(ans.contains("telephone-event/8000"));
        assert!(ans.contains("c=IN IP4 1.2.3.4"));
    }

    #[tokio::test]
    async fn relay_allocates_distinct_ports() {
        let relay = RtpRelay::new("127.0.0.1", 41000, 41010);
        let a = relay.alloc_leg().await.unwrap();
        let b = relay.alloc_leg().await.unwrap();
        assert_ne!(a.local_port, b.local_port);
        assert!(a.local_port >= 41000 && a.local_port < 41010);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn relay_forwards_after_latch() {
        let relay = RtpRelay::new("127.0.0.1", 42000, 42020);
        let a = relay.alloc_leg().await.unwrap();
        let b = relay.alloc_leg().await.unwrap();
        let a_port = a.local_port;
        let b_port = b.local_port;

        // Two fake endpoints.
        let ep_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let ep_b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        // Pre-latch both remotes explicitly so forwarding does not depend on the
        // ordering/timing of the first inbound packet on each leg (which is
        // racy on a loaded loopback under the test runner).
        a.set_remote(ep_a.local_addr().unwrap()).await;
        b.set_remote(ep_b.local_addr().unwrap()).await;

        let _h = RtpRelay::bridge(&a, &b);

        // A -> relay leg A -> should be forwarded to ep_b. Retry a few times to
        // absorb scheduler jitter; any single delivery proves the path works.
        let mut got = false;
        let mut buf = [0u8; 64];
        for _ in 0..10 {
            ep_a.send_to(b"hello", format!("127.0.0.1:{a_port}")).await.unwrap();
            if let Ok(Ok((n, _))) = tokio::time::timeout(
                std::time::Duration::from_millis(200), ep_b.recv_from(&mut buf)).await
            {
                assert_eq!(&buf[..n], b"hello");
                got = true;
                break;
            }
        }
        assert!(got, "RTP from A must be relayed to latched B");

        // And the reverse path B -> ep_a.
        let mut got_rev = false;
        for _ in 0..10 {
            ep_b.send_to(b"world", format!("127.0.0.1:{b_port}")).await.unwrap();
            if let Ok(Ok((n, _))) = tokio::time::timeout(
                std::time::Duration::from_millis(200), ep_a.recv_from(&mut buf)).await
            {
                assert_eq!(&buf[..n], b"world");
                got_rev = true;
                break;
            }
        }
        assert!(got_rev, "RTP from B must be relayed to latched A");
    }
}
