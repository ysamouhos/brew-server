//! HA replication (phase 2): the Standby node keeps a copy of the Active
//! node's history log and SMS Center queue, so a failover loses neither.
//!
//! The Standby dials the Active node's real IP on TCP `heartbeat_port` (the
//! same number as the UDP heartbeat). Both prove knowledge of
//! `shared_secret` with an HMAC challenge/response; the stream itself is not
//! encrypted (the pair shares a trusted LAN, like the heartbeat).
//!
//! - History: the Active node streams its append-only log from the byte
//!   offset the Standby asks for (kept in `<state_path>.repl.json` across
//!   restarts), then tails it. The Standby appends records it does not
//!   already have (see `Store::append_replicated`), so a log that went back
//!   and forth between the nodes never duplicates.
//! - SMS Center: the Active node sends the whole queue whenever it changes;
//!   the Standby replaces its own.
//! - `CaughtUp` marks the end of the initial transfer. Until it arrives the
//!   Standby reports `synced = false` in its heartbeat, and the Active node
//!   does not hand over to it for preemption (see `Election::tick`).

use crate::ha::Role;
use crate::state::AppState;
use anyhow::{bail, Context, Result};
use aws_lc_rs::hmac;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

const MAX_FRAME: usize = 16 << 20;
const HISTORY_CHUNK: usize = 256 << 10;
const POLL: Duration = Duration::from_millis(500);
const PING_EVERY: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, Deserialize)]
enum Msg {
    /// Standby -> Active.
    Hello { node: String, nonce: [u8; 16], history_from: Option<u64>, want_sms: bool },
    /// Active -> Standby: proves the Active knows the secret, challenges back.
    Challenge { nonce: [u8; 16], mac: Vec<u8> },
    /// Standby -> Active.
    Auth { mac: Vec<u8> },
    /// Complete log frames covering bytes `start..end` of the Active's log.
    History { start: u64, end: u64, frames: Vec<u8> },
    Sms { json: Vec<u8> },
    CaughtUp,
    Ping,
}

/// Replication state shown on the dashboard and carried in heartbeats.
#[derive(Default)]
pub struct ReplStatus {
    /// Standby: connected to the Active node. Active: a Standby is connected.
    pub connected: AtomicBool,
    /// Standby: initial transfer done and the link is up.
    pub synced: AtomicBool,
    pub last_rx_ms: AtomicU64,
    /// Records added to this node's log from the peer (this process).
    pub records_received: AtomicU64,
    pub sms_snapshots_received: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplView {
    pub needed: bool,
    pub connected: bool,
    pub synced: bool,
    pub last_rx_ms_ago: Option<u64>,
    pub records_received: u64,
    pub sms_snapshots_received: u64,
}

impl ReplStatus {
    pub fn view(&self, needed: bool, now_ms: u64) -> ReplView {
        let last = self.last_rx_ms.load(Ordering::Relaxed);
        ReplView {
            needed,
            connected: self.connected.load(Ordering::Relaxed),
            synced: self.synced.load(Ordering::Relaxed),
            last_rx_ms_ago: (last > 0).then(|| now_ms.saturating_sub(last)),
            records_received: self.records_received.load(Ordering::Relaxed),
            sms_snapshots_received: self.sms_snapshots_received.load(Ordering::Relaxed),
        }
    }
}

/// Whether this node has anything to replicate.
pub fn needed(state: &AppState) -> bool {
    state.store.is_some() || state.sms_center.enabled()
}

/// Whether this node may claim to be in sync in its heartbeat.
pub fn synced(state: &AppState) -> bool {
    !needed(state) || state.ha.repl.synced.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Framing and authentication.

async fn send(stream: &mut TcpStream, msg: &Msg) -> Result<()> {
    let body = bincode::serialize(msg)?;
    stream.write_all(&(body.len() as u32).to_le_bytes()).await?;
    stream.write_all(&body).await?;
    Ok(())
}

async fn recv(stream: &mut TcpStream) -> Result<Msg> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        bail!("replication frame too large ({len} bytes)");
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok(bincode::deserialize(&body)?)
}

async fn recv_timeout(stream: &mut TcpStream, t: Duration) -> Result<Msg> {
    tokio::time::timeout(t, recv(stream)).await.context("replication peer timed out")?
}

fn mac(key: &hmac::Key, label: &[u8], a: &[u8; 16], b: &[u8; 16]) -> Vec<u8> {
    let mut ctx = hmac::Context::with_key(key);
    ctx.update(b"brew-ha-repl/");
    ctx.update(label);
    ctx.update(a);
    ctx.update(b);
    ctx.sign().as_ref().to_vec()
}

fn verify(key: &hmac::Key, label: &[u8], a: &[u8; 16], b: &[u8; 16], tag: &[u8]) -> bool {
    let mut msg = b"brew-ha-repl/".to_vec();
    msg.extend_from_slice(label);
    msg.extend_from_slice(a);
    msg.extend_from_slice(b);
    hmac::verify(key, &msg, tag).is_ok()
}

fn nonce() -> [u8; 16] {
    uuid::Uuid::new_v4().into_bytes()
}

fn now_ms() -> u64 {
    crate::telemetry::now_ms()
}

// ---------------------------------------------------------------------------
// Standby side: cursor file.

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cursor {
    /// Byte offset reached in the peer's history log.
    history_offset: u64,
}

fn cursor_path(state: &AppState) -> PathBuf {
    state.config.ha.state_path.with_extension("repl.json")
}

fn load_cursor(path: &std::path::Path) -> Cursor {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_cursor(path: &std::path::Path, c: &Cursor) {
    let tmp = path.with_extension("tmp");
    let r = std::fs::write(&tmp, serde_json::to_vec(c).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = r {
        warn!(error = %e, "cannot save the HA replication cursor");
    }
}

// ---------------------------------------------------------------------------
// Runtime.

/// Runs both sides for the life of the process: serves the peer while this
/// node is Active, pulls from it while this node is Standby.
pub async fn run(state: Arc<AppState>) -> Result<()> {
    let cfg = state.config.ha.clone();
    if !cfg.enabled || !needed(&state) {
        return Ok(());
    }
    let key = hmac::Key::new(hmac::HMAC_SHA256, cfg.shared_secret.as_bytes());
    let bind = SocketAddr::new(cfg.real_ip, cfg.heartbeat_port);
    let listener = TcpListener::bind(bind).await.with_context(|| format!("binding HA replication listener {bind}"))?;

    let serve_state = state.clone();
    let serve_key = key.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, from)) = listener.accept().await else { continue };
            if from.ip() != serve_state.config.ha.peer_ip {
                warn!(%from, "HA replication connection from a non-peer address refused");
                continue;
            }
            let (state, key) = (serve_state.clone(), serve_key.clone());
            tokio::spawn(async move {
                if let Err(e) = serve(&state, &key, stream).await {
                    info!(error = %e, "HA replication to the Standby ended");
                }
                state.ha.repl.connected.store(false, Ordering::Relaxed);
            });
        }
    });

    let peer = SocketAddr::new(cfg.peer_ip, cfg.heartbeat_port);
    let mut status = state.ha.status.subscribe();
    loop {
        // Pull only while we are Standby and the peer is Active.
        let _ = status
            .wait_for(|s| s.role == Role::Standby && s.peer.as_ref().is_some_and(|p| p.alive && p.role == Role::Active))
            .await;
        match pull(&state, &key, peer).await {
            Ok(()) => {}
            Err(e) => info!(error = %e, "HA replication from the Active node ended"),
        }
        state.ha.repl.connected.store(false, Ordering::Relaxed);
        state.ha.repl.synced.store(false, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Active side: one connected Standby.
async fn serve(state: &AppState, key: &hmac::Key, mut stream: TcpStream) -> Result<()> {
    let Msg::Hello { node, nonce: n_client, history_from, want_sms } = recv_timeout(&mut stream, HANDSHAKE_TIMEOUT).await? else {
        bail!("expected Hello");
    };
    let n_server = nonce();
    send(&mut stream, &Msg::Challenge { nonce: n_server, mac: mac(key, b"server", &n_client, &n_server) }).await?;
    let Msg::Auth { mac: tag } = recv_timeout(&mut stream, HANDSHAKE_TIMEOUT).await? else { bail!("expected Auth") };
    if !verify(key, b"client", &n_server, &n_client, &tag) {
        bail!("Standby {node} failed authentication (shared_secret differs?)");
    }
    if state.ha.role() != Role::Active {
        bail!("not Active; refusing to serve replication");
    }
    info!(standby = %node, "HA replication: Standby connected");
    state.ha.repl.connected.store(true, Ordering::Relaxed);

    // An offset from before a compaction of our log (retention, Clear) may
    // fall inside a record: start over, the Standby skips what it has.
    let mut offset = history_from.map(|o| match &state.store { Some(st) if !st.is_boundary(o) => 0, _ => o });
    let mut generation = state.store.as_ref().map_or(0, |s| s.generation());
    let mut sms_rev = 0u64;
    let mut caught_up = false;
    let mut last_send = tokio::time::Instant::now();
    loop {
        if state.ha.role() != Role::Active {
            bail!("no longer Active");
        }
        let mut sent = false;
        if let (Some(store), Some(mut off)) = (&state.store, offset) {
            if store.generation() != generation {
                generation = store.generation();
                off = 0;
            }
            loop {
                // `start` is 0 rather than `off` when our log was replaced
                // by a shorter one; carry on from there.
                let (start, frames, end) = store.read_frames(off, HISTORY_CHUNK)?;
                off = end;
                if frames.is_empty() {
                    break;
                }
                send(&mut stream, &Msg::History { start, end, frames }).await?;
                sent = true;
            }
            offset = Some(off);
        }
        if want_sms && state.sms_center.enabled() {
            let rev = state.sms_center.revision();
            if rev != sms_rev {
                send(&mut stream, &Msg::Sms { json: state.sms_center.export() }).await?;
                sms_rev = rev;
                sent = true;
            }
        }
        if !caught_up {
            send(&mut stream, &Msg::CaughtUp).await?;
            caught_up = true;
            sent = true;
        }
        if sent {
            last_send = tokio::time::Instant::now();
        } else if last_send.elapsed() >= PING_EVERY {
            send(&mut stream, &Msg::Ping).await?;
            last_send = tokio::time::Instant::now();
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Standby side: one connection to the Active node.
async fn pull(state: &AppState, key: &hmac::Key, peer: SocketAddr) -> Result<()> {
    let cursor_file = cursor_path(state);
    let mut cursor = load_cursor(&cursor_file);
    // From our real IP: the Active node only accepts its peer's address, and
    // the kernel could otherwise pick another local one (e.g. the VIP).
    let socket = match peer {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    socket.bind(SocketAddr::new(state.config.ha.real_ip, 0))?;
    let mut stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, socket.connect(peer))
        .await
        .context("connect timed out")??;
    stream.set_nodelay(true)?;

    let n_client = nonce();
    send(&mut stream, &Msg::Hello {
        node: state.config.ha.node_name.clone(),
        nonce: n_client,
        history_from: state.store.is_some().then_some(cursor.history_offset),
        want_sms: state.sms_center.enabled(),
    })
    .await?;
    let Msg::Challenge { nonce: n_server, mac: tag } = recv_timeout(&mut stream, HANDSHAKE_TIMEOUT).await? else {
        bail!("expected Challenge");
    };
    if !verify(key, b"server", &n_client, &n_server, &tag) {
        bail!("Active node failed authentication (shared_secret differs?)");
    }
    send(&mut stream, &Msg::Auth { mac: mac(key, b"client", &n_server, &n_client) }).await?;
    state.ha.repl.connected.store(true, Ordering::Relaxed);
    info!(%peer, from = cursor.history_offset, "HA replication: pulling from the Active node");

    loop {
        if state.ha.role() != Role::Standby {
            return Ok(());
        }
        let msg = recv_timeout(&mut stream, IDLE_TIMEOUT).await?;
        state.ha.repl.last_rx_ms.store(now_ms(), Ordering::Relaxed);
        match msg {
            Msg::History { start: _, end, frames } => {
                if let Some(store) = &state.store {
                    let mut added = 0u64;
                    for body in crate::store::split_frames(&frames).0 {
                        match store.append_replicated(body) {
                            Ok(Some(rec)) => {
                                added += 1;
                                ingest(state, rec).await;
                            }
                            Ok(None) => {}
                            Err(e) => warn!(error = %e, "skipping a replicated history record"),
                        }
                    }
                    state.ha.repl.records_received.fetch_add(added, Ordering::Relaxed);
                    cursor.history_offset = end;
                    save_cursor(&cursor_file, &cursor);
                }
            }
            Msg::Sms { json } => match state.sms_center.import(&json) {
                Ok(()) => {
                    state.ha.repl.sms_snapshots_received.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => warn!(error = %e, "cannot import the replicated SMS Center queue"),
            },
            Msg::CaughtUp => {
                info!("HA replication: caught up with the Active node");
                state.ha.repl.synced.store(true, Ordering::Relaxed);
            }
            Msg::Ping => {}
            other => bail!("unexpected replication message {other:?}"),
        }
    }
}

async fn ingest(state: &AppState, rec: crate::store::StoredRecord) {
    match rec {
        crate::store::StoredRecord::SdsTelemetry(r) => state.telemetry.write().await.ingest_replicated(r),
        other => state.monitor.ingest_replicated(other).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_macs_bind_both_nonces_and_direction() {
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"secret");
        let (a, b) = (nonce(), nonce());
        let t = mac(&key, b"server", &a, &b);
        assert!(verify(&key, b"server", &a, &b, &t));
        assert!(!verify(&key, b"client", &a, &b, &t), "a server proof is not a client proof");
        assert!(!verify(&key, b"server", &b, &a, &t), "nonce order matters");
        let other = hmac::Key::new(hmac::HMAC_SHA256, b"other");
        assert!(!verify(&other, b"server", &a, &b, &t));
    }
}
