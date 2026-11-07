//! Basestation Control channel: bidirectional, BTS-initiated WebSocket
//! (subprotocol `basestation-control-v1`) that a control server uses to push
//! commands (kick, DGNA, live SDS, emergency clear, restart/shutdown) and
//! read back responses for the few command types that define one.
use crate::{fsnet, state::AppState};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{atomic::{AtomicU32, Ordering}, Arc},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum ControlCommand {
    SendSds { #[serde(default)] handle: u32, source_ssi: u32, dest_ssi: u32, dest_is_group: bool, len_bits: u16, payload: Vec<u8> },
    SendRawSdsType4 { #[serde(default)] handle: u32, source_ssi: u32, dest_ssi: u32, dest_is_group: bool, len_bits: u16, payload: Vec<u8> },
    KickMs { issi: u32 },
    Dgna { issi: u32, gssi: u32, mnemonic: Option<String>, attachment_mode: u8, attach: bool },
    RestartService,
    ShutdownService,
    AddLiveSds { text: String, protocol_id: u8, source_issi: u32, repeat_count: u32 },
    DeleteLiveSds { id: u32 },
    ClearLiveSds,
    ClearEmergency { issi: u32 },
    CommandA { #[serde(default)] handle: u32, parameter: u32 },
    TestCmdB { #[serde(default)] handle: u32, source_ssi: u32, is_group: bool, payload: Vec<u8> },
    /// Ambience listening (SS-AL): command the Basestation to set up a
    /// one-way speech call to `issi` in which the target radio auto-answers
    /// and keys its transmitter, so the dispatcher hears its surroundings.
    /// `enable` false releases an active ambience-listening call. The target
    /// radio indicates the call like any other (not covert).
    ///
    /// PROVISIONAL: the Basestation must implement this command and the
    /// over-the-air AL service for it to take effect; the exact wire shape may
    /// need to match the Basestation's own codec.
    AmbienceListen { issi: u32, enable: bool },
}

impl ControlCommand {
    /// The wire format is serde's default externally-tagged representation
    /// (`{"KickMs":{"issi":1}}`), matching Basestation's own derive-based
    /// JSON codec -- not the internal `#[serde(tag = "action")]` shape used
    /// for the dashboard's HTTP request body.
    fn to_wire_json(&self) -> serde_json::Value {
        let tagged = serde_json::to_value(self).expect("ControlCommand always serializes");
        let serde_json::Value::Object(mut map) = tagged else { return tagged };
        let Some(serde_json::Value::String(tag)) = map.remove("action") else { return serde_json::Value::Object(map) };
        if map.is_empty() {
            // Unit variants (RestartService, ShutdownService, ClearLiveSds) serialize
            // as a bare string under serde's default externally-tagged representation.
            serde_json::Value::String(tag)
        } else {
            serde_json::json!({ tag: serde_json::Value::Object(map) })
        }
    }

    fn handle(&self) -> Option<u32> {
        match self {
            Self::SendSds { handle, .. } | Self::SendRawSdsType4 { handle, .. }
            | Self::CommandA { handle, .. } | Self::TestCmdB { handle, .. } => Some(*handle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ControlResponse {
    CommandAResponse { handle: u32, result: u32 },
    SendSdsResponse { handle: u32, success: bool },
    KickMsResponse { issi: u32, success: bool },
    AmbienceListenResponse { issi: u32, success: bool },
}

struct ControlSession {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    pending_handle: HashMap<u32, oneshot::Sender<ControlResponse>>,
    pending_kick: HashMap<u32, oneshot::Sender<ControlResponse>>,
    pending_ambience: HashMap<u32, oneshot::Sender<ControlResponse>>,
}

#[derive(Default)]
pub struct ControlState {
    sessions: HashMap<String, ControlSession>,
    next_handle: AtomicU32,
}

impl ControlState {
    pub fn connected_ids(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    fn next_handle(&self) -> u32 {
        self.next_handle.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub enum SendError {
    NotConnected,
    Timeout,
}

/// Sends a command to a connected control BTS. If the command defines a
/// response, waits up to 5s for it (correlated by `handle`, or by `issi` for
/// `KickMs`) and returns it; otherwise returns `Ok(None)` once the frame is
/// written.
pub async fn send_command(state: &Arc<AppState>, bts_id: &str, mut command: ControlCommand) -> Result<Option<ControlResponse>, SendError> {
    let mut ctl = state.control.write().await;
    if !ctl.sessions.contains_key(bts_id) { return Err(SendError::NotConnected); }

    // Assign a fresh handle for correlated commands so dashboard callers
    // don't need to manage one themselves.
    let fresh_handle = ctl.next_handle();
    let session = ctl.sessions.get_mut(bts_id).expect("checked above");
    let assigned_handle = match &mut command {
        ControlCommand::SendSds { handle, .. } | ControlCommand::SendRawSdsType4 { handle, .. }
        | ControlCommand::CommandA { handle, .. } | ControlCommand::TestCmdB { handle, .. } => {
            *handle = fresh_handle;
            Some(*handle)
        }
        _ => None,
    };

    let waiter = match &command {
        ControlCommand::KickMs { issi } => {
            let (tx, rx) = oneshot::channel();
            session.pending_kick.insert(*issi, tx);
            Some(rx)
        }
        ControlCommand::AmbienceListen { issi, .. } => {
            let (tx, rx) = oneshot::channel();
            session.pending_ambience.insert(*issi, tx);
            Some(rx)
        }
        _ if matches!(command, ControlCommand::SendSds { .. } | ControlCommand::CommandA { .. }) => {
            let handle = assigned_handle.or_else(|| command.handle()).expect("assigned above");
            let (tx, rx) = oneshot::channel();
            session.pending_handle.insert(handle, tx);
            Some(rx)
        }
        _ => None,
    };

    let frame = serde_json::to_vec(&command.to_wire_json()).expect("control command always serializes");
    if session.tx.send(frame).is_err() {
        return Err(SendError::NotConnected);
    }
    drop(ctl);

    let Some(rx) = waiter else { return Ok(None) };
    match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(response)) => Ok(Some(response)),
        Ok(Err(_)) | Err(_) => Err(SendError::Timeout),
    }
}

pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    if !state.config.control.enabled {
        return Ok(());
    }
    let cfg = fsnet::ListenerConfig {
        name: "control",
        listen: state.service_bind(state.config.control.listen),
        subprotocol: "bluestation-control-v1",
        users: state.logins.control.clone(),
        tls: state.config.control.tls.clone(),
    };
    fsnet::serve(cfg, move |socket, identity, _peer| {
        let state = state.clone();
        async move { session(state, socket, identity).await }
    })
    .await
}

async fn session(state: Arc<AppState>, socket: WebSocket, identity: Option<String>) {
    let id = identity.unwrap_or_else(|| format!("control-{}", uuid::Uuid::new_v4().simple()));
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    {
        let mut ctl = state.control.write().await;
        ctl.sessions.insert(id.clone(), ControlSession { tx, pending_handle: HashMap::new(), pending_kick: HashMap::new(), pending_ambience: HashMap::new() });
    }
    info!(bts = %id, "Basestation control connected");
    state.monitor.emit("control_connected", serde_json::json!({"id": id}));

    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if ws_tx.send(Message::Binary(frame.into())).await.is_err() { break; }
        }
    });

    while let Some(item) = ws_rx.next().await {
        match item {
            Ok(Message::Binary(data)) => handle_response(&state, &id, &data).await,
            Ok(Message::Text(_)) => warn!(bts = %id, "unexpected text frame on control channel, ignoring"),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Err(e) => { warn!(bts = %id, error = %e, "control WebSocket receive error"); break; }
        }
    }

    writer.abort();
    state.control.write().await.sessions.remove(&id);
    state.monitor.emit("control_disconnected", serde_json::json!({"id": id}));
    info!(bts = %id, "Basestation control disconnected");
}

async fn handle_response(state: &Arc<AppState>, id: &str, data: &[u8]) {
    let response: ControlResponse = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(e) => {
            warn!(bts = %id, error = %e, bytes = data.len(), "dropping malformed control response");
            return;
        }
    };

    let mut ctl = state.control.write().await;
    let Some(session) = ctl.sessions.get_mut(id) else { return };
    match &response {
        ControlResponse::KickMsResponse { issi, .. } => {
            if let Some(tx) = session.pending_kick.remove(issi) { let _ = tx.send(response); }
        }
        ControlResponse::AmbienceListenResponse { issi, .. } => {
            if let Some(tx) = session.pending_ambience.remove(issi) { let _ = tx.send(response); }
        }
        ControlResponse::CommandAResponse { handle, .. } | ControlResponse::SendSdsResponse { handle, .. } => {
            if let Some(tx) = session.pending_handle.remove(handle) { let _ = tx.send(response); }
        }
    }
}

/// Test hook: registers a control session for `id` without a WebSocket and
/// returns the frames the server sends to it.
#[cfg(test)]
pub(crate) async fn attach_test_session(state: &Arc<AppState>, id: &str) -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.control.write().await.sessions.insert(id.to_string(), ControlSession {
        tx, pending_handle: HashMap::new(), pending_kick: HashMap::new(), pending_ambience: HashMap::new(),
    });
    rx
}

/// Test hook: feeds a response frame as if the Basestation `id` had sent it.
#[cfg(test)]
pub(crate) async fn deliver_test_response(state: &Arc<AppState>, id: &str, data: &[u8]) {
    handle_response(state, id, data).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambience_listen_wire_shape() {
        // Externally-tagged, matching the Basestation's derive-based codec.
        let json = ControlCommand::AmbienceListen { issi: 2001, enable: true }.to_wire_json();
        assert_eq!(json, serde_json::json!({"AmbienceListen": {"issi": 2001, "enable": true}}));
    }

    #[test]
    fn ambience_listen_response_parses() {
        let r: ControlResponse =
            serde_json::from_value(serde_json::json!({"AmbienceListenResponse": {"issi": 2001, "success": true}})).unwrap();
        assert!(matches!(r, ControlResponse::AmbienceListenResponse { issi: 2001, success: true }));
    }
}
