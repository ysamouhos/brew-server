//! Parrot: a number radios call to hear themselves back, to test a radio.
//!
//! `[parrot]` (Server settings page, live). An individual call to the parrot
//! ISSI is answered by the server itself: in a simplex call each over is
//! recorded and played back to the caller `delay_ms` after the caller lets go
//! of PTT; in a duplex call every voice frame comes back `delay_ms` later (an
//! echo), and the call is ended after `max_secs`. An over on the parrot group
//! (the same number unless a separate GSSI is set) is recorded and played back
//! on the group, as the parrot ISSI, `delay_ms` after it ended, so every radio
//! in the group hears it. An over is recorded for at most `max_secs`.
//!
//! The parrot sits on the server, so it works from every station and needs
//! nothing new on the stations: the individual call uses the Brew call
//! control a Basestation already speaks for calls to the network
//! (`CALL_SETUP_ACCEPT`, `CALL_CONNECT_REQUEST`, `CALL_SIMPLEX_GRANTED` /
//! `CALL_SIMPLEX_IDLE`, as the SIP bridge does), and the group playback is an
//! ordinary server-originated `CALL_GROUP_TX` (as a talkgroup pool leg). Only
//! calls from this server's own connections are answered: a parrot on another
//! server answers its own.

use crate::config::ParrotConfig;
use crate::protocol::{self, CALL_GROUP_IDLE, CALL_RELEASE, CALL_SIMPLEX_GRANTED, CALL_SIMPLEX_IDLE};
use crate::state::{ActiveCall, AppState, CallKind, ClientId, ClientMode, Inner};
use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// The parrot's own party in a call: owner of its group playbacks, peer of
/// the individual calls it answers. Never a real connection.
pub const PARROT_OWNER: ClientId = Uuid::from_u128(0x7061_7272_6f74_4000_8000_0000_0000_0001);

/// A simplex over ends with the caller's `CALL_SIMPLEX_IDLE`; without one, after
/// this long without voice.
const OVER_GAP_MS: u64 = 1000;
/// Before a simplex playback: the floor goes to the parrot first, so the
/// station has told the radio before the first frame comes.
const GRANT_LEAD_MS: u64 = 120;
/// How long a group playback waits for the group to become free.
const GROUP_BUSY_WAIT: Duration = Duration::from_secs(30);
/// Voice frames kept per over at most (60 ms each: 600 s).
const MAX_FRAMES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Group { gssi: u32 },
    Simplex,
    Duplex,
}

#[derive(Debug)]
struct Rec {
    kind: Kind,
    /// The radio that talks (the caller of an individual call).
    talker: u32,
    /// Voice frames of the over being recorded: ms after its first frame, frame.
    frames: Vec<(u64, Vec<u8>)>,
    first: Option<Instant>,
    last: Option<Instant>,
    /// Counts the overs of an individual call; a playback goes on only while
    /// it is the latest.
    gen: u64,
    /// The playback of this generation runs (simplex only).
    playing: Option<u64>,
    answered_at: Instant,
}

impl Rec {
    fn new(kind: Kind, talker: u32) -> Self {
        Self { kind, talker, frames: Vec::new(), first: None, last: None, gen: 0, playing: None, answered_at: Instant::now() }
    }

    /// Adds one voice frame, up to `max_ms` after the first.
    fn record(&mut self, raw: &[u8], now: Instant, max_ms: u64) {
        let first = *self.first.get_or_insert(now);
        self.last = Some(now);
        let at = now.duration_since(first).as_millis() as u64;
        if at <= max_ms && self.frames.len() < MAX_FRAMES {
            self.frames.push((at, raw.to_vec()));
        }
    }

    /// Takes the recorded over (empty if none).
    fn take_over(&mut self) -> Vec<(u64, Vec<u8>)> {
        self.first = None;
        self.last = None;
        std::mem::take(&mut self.frames)
    }
}

enum Job {
    Group { gssi: u32, frames: Vec<(u64, Vec<u8>)>, talker: u32 },
    Private { id: Uuid, gen: u64, frames: Vec<(u64, Vec<u8>)> },
    Echo { at: Instant, id: Uuid, raw: Vec<u8> },
    Hangup { id: Uuid },
}

/// Parrot state in `AppState`.
pub struct Parrot {
    pub cfg: std::sync::RwLock<ParrotConfig>,
    recs: std::sync::Mutex<HashMap<Uuid, Rec>>,
    jobs: mpsc::UnboundedSender<Job>,
    jobs_rx: std::sync::Mutex<Option<mpsc::UnboundedReceiver<Job>>>,
}

impl Parrot {
    pub fn new(cfg: ParrotConfig) -> Self {
        let (jobs, rx) = mpsc::unbounded_channel();
        Self { cfg: std::sync::RwLock::new(cfg), recs: Default::default(), jobs, jobs_rx: std::sync::Mutex::new(Some(rx)) }
    }

    pub fn config(&self) -> ParrotConfig {
        self.cfg.read().map(|c| *c).unwrap_or_default()
    }

    fn max_ms(&self) -> u64 {
        self.config().max_secs as u64 * 1000
    }

    /// Time after the last played frame before the floor / group call is
    /// released (`tail_ms`, at least the 60 ms a frame takes).
    fn tail(&self) -> Duration {
        Duration::from_millis(self.config().tail_ms.max(60) as u64)
    }

    fn delay(&self) -> Duration {
        Duration::from_millis(self.config().delay_ms as u64)
    }

    /// Whether `issi` is the parrot's individual number right now.
    pub fn is_parrot_issi(&self, issi: u32) -> bool {
        self.config().active_issi() == Some(issi)
    }

    fn push(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

fn is_local(inner: &Inner, client: ClientId) -> bool {
    inner.clients.get(&client).is_some_and(|c| !matches!(c.mode, ClientMode::Peer | ClientMode::Network))
}

/// `CALL_SIMPLEX_GRANTED` / `CALL_SIMPLEX_IDLE`: uuid, then grant and
/// permission (EN 300 392-2 table 14.80: 0 = granted, 1 = not granted).
fn build_floor(state_byte: u8, id: &Uuid, grant: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.push(protocol::CLASS_CALL_CONTROL);
    out.push(state_byte);
    out.extend_from_slice(id.as_bytes());
    out.push(grant);
    out.push(0);
    out
}

/// A copy of a voice frame for another call.
fn with_id(raw: &[u8], id: &Uuid) -> Vec<u8> {
    let mut out = raw.to_vec();
    if out.len() >= 18 { out[2..18].copy_from_slice(id.as_bytes()); }
    out
}

/// An individual call to the parrot ISSI (router, under its lock): the parrot
/// becomes the called party and answers at once. Returns the answer for the
/// caller's connection, `None` when the call is not for the parrot.
pub fn answer(
    state: &AppState,
    inner: &mut Inner,
    source: ClientId,
    id: Uuid,
    caller: u32,
    destination: u32,
    duplex: bool,
    priority: u8,
) -> Option<Vec<Vec<u8>>> {
    if !state.parrot.is_parrot_issi(destination) || !is_local(inner, source) { return None; }
    inner.calls.insert(id, ActiveCall {
        kind: CallKind::Private,
        owner: source,
        source_issi: caller,
        destination,
        priority,
        peers: HashSet::from([PARROT_OWNER]),
        started_at: Instant::now(),
        last_activity_ms: ActiveCall::new_activity(),
    });
    let kind = if duplex { Kind::Duplex } else { Kind::Simplex };
    if let Ok(mut r) = state.parrot.recs.lock() { r.insert(id, Rec::new(kind, caller)); }
    info!(%source, uuid=%id, caller, parrot=destination, duplex, "parrot answers an individual call");
    Some(vec![
        protocol::build_call_control_empty(protocol::CALL_SETUP_ACCEPT, &id),
        protocol::build_circular_connect_request_as(&id, destination, caller, priority, duplex),
    ])
}

/// A group call started (router): an over on the parrot group from one of
/// this server's connections is recorded.
pub async fn on_group_tx(state: &Arc<AppState>, source: ClientId, id: Uuid, talker: u32, gssi: u32) {
    if source == PARROT_OWNER || state.parrot.config().active_gssi() != Some(gssi) { return; }
    if !is_local(&*state.inner.read().await, source) { return; }
    if let Ok(mut r) = state.parrot.recs.lock() {
        r.entry(id).or_insert_with(|| Rec::new(Kind::Group { gssi }, talker));
    }
    debug!(uuid=%id, talker, gssi, "parrot records an over on its group");
}

/// A voice frame of a call (router, after the participant check).
pub fn on_frame(state: &AppState, id: Uuid, source: ClientId, raw: &[u8]) {
    if source == PARROT_OWNER { return; }
    let max_ms = state.parrot.max_ms();
    let now = Instant::now();
    let Ok(mut recs) = state.parrot.recs.lock() else { return };
    let Some(rec) = recs.get_mut(&id) else { return };
    match rec.kind {
        Kind::Duplex => {
            if rec.answered_at.elapsed().as_millis() as u64 >= max_ms {
                recs.remove(&id);
                state.parrot.push(Job::Hangup { id });
                return;
            }
            state.parrot.push(Job::Echo { at: now + state.parrot.delay(), id, raw: raw.to_vec() });
        }
        Kind::Simplex => {
            if rec.playing.is_some() {
                // The caller took the floor back: the playback stops.
                rec.playing = None;
            }
            if rec.first.is_none() { rec.gen += 1; }
            rec.record(raw, now, max_ms);
        }
        Kind::Group { .. } => rec.record(raw, now, max_ms),
    }
}

/// Floor control of an individual call with the parrot (router): the
/// caller's `CALL_SIMPLEX_IDLE` ends the over, its `CALL_SIMPLEX_GRANTED`
/// starts one (and stops a playback).
pub fn on_private_control(state: &AppState, id: Uuid, source: ClientId, state_byte: u8) {
    if source == PARROT_OWNER { return; }
    let Ok(mut recs) = state.parrot.recs.lock() else { return };
    let Some(rec) = recs.get_mut(&id) else { return };
    if rec.kind != Kind::Simplex { return; }
    match state_byte {
        CALL_SIMPLEX_GRANTED => {
            rec.playing = None;
            rec.gen += 1;
            rec.take_over();
        }
        CALL_SIMPLEX_IDLE => end_over(state, id, rec),
        _ => {}
    }
}

/// The caller let go: the over is played back after the delay.
fn end_over(state: &AppState, id: Uuid, rec: &mut Rec) {
    let frames = rec.take_over();
    if frames.is_empty() { return; }
    rec.playing = Some(rec.gen);
    debug!(uuid=%id, frames = frames.len(), "parrot over ended, playing back");
    state.parrot.push(Job::Private { id, gen: rec.gen, frames });
}

/// A call ended (router): a recorded group over is played back; an
/// individual call's recording and playback end with it.
pub fn on_call_end(state: &AppState, id: Uuid) {
    let Some(rec) = state.parrot.recs.lock().ok().and_then(|mut r| r.remove(&id)) else { return };
    if let Kind::Group { gssi } = rec.kind {
        let talker = rec.talker;
        let frames = rec.frames;
        if !frames.is_empty() {
            debug!(uuid=%id, gssi, talker, frames = frames.len(), "parrot group over ended, playing back");
            state.parrot.push(Job::Group { gssi, frames, talker });
        }
    }
}

/// Overs that ended without `CALL_SIMPLEX_IDLE`.
fn sweep(state: &AppState, now: Instant) {
    let Ok(mut recs) = state.parrot.recs.lock() else { return };
    for (id, rec) in recs.iter_mut() {
        let quiet = rec.last.is_some_and(|l| now.duration_since(l).as_millis() as u64 >= OVER_GAP_MS);
        if rec.kind == Kind::Simplex && rec.playing.is_none() && quiet {
            end_over(state, *id, rec);
        }
    }
}

/// The parrot's worker: playbacks, echoes and the over sweep.
pub async fn run(state: Arc<AppState>) {
    let Some(mut rx) = state.parrot.jobs_rx.lock().ok().and_then(|mut r| r.take()) else { return };
    let mut echoes: VecDeque<(Instant, Uuid, Vec<u8>)> = VecDeque::new();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        let next_echo = echoes.front().map(|e| tokio::time::Instant::from_std(e.0));
        tokio::select! {
            job = rx.recv() => {
                let Some(job) = job else { return };
                match job {
                    Job::Group { gssi, frames, talker } => { tokio::spawn(play_group(state.clone(), gssi, frames, talker)); }
                    Job::Private { id, gen, frames } => { tokio::spawn(play_private(state.clone(), id, gen, frames)); }
                    Job::Echo { at, id, raw } => echoes.push_back((at, id, raw)),
                    Job::Hangup { id } => {
                        info!(uuid=%id, "parrot ends a duplex call (maximum length)");
                        crate::router::end_call(&state, PARROT_OWNER, id, protocol::build_call_cause(CALL_RELEASE, &id, 0)).await;
                    }
                }
            }
            _ = async { tokio::time::sleep_until(next_echo.unwrap()).await }, if next_echo.is_some() => {
                let now = Instant::now();
                while echoes.front().is_some_and(|e| e.0 <= now) {
                    let (_, id, raw) = echoes.pop_front().unwrap();
                    send_to_caller(&state, id, &raw).await;
                }
            }
            _ = tick.tick() => sweep(&state, Instant::now()),
        }
    }
}

/// Sends a packet to the radio side of an individual call with the parrot
/// (wherever the call's owner is now). `false` when the call is gone.
async fn send_to_caller(state: &AppState, id: Uuid, raw: &[u8]) -> bool {
    let inner = state.inner.read().await;
    let Some(call) = inner.calls.get(&id).filter(|c| c.peers.contains(&PARROT_OWNER)) else { return false };
    call.touch();
    let Some(client) = inner.clients.get(&call.owner) else { return false };
    let _ = client.tx.send(raw.to_vec());
    true
}

fn still_playing(state: &AppState, id: Uuid, gen: u64) -> bool {
    state.parrot.recs.lock().is_ok_and(|r| r.get(&id).is_some_and(|rec| rec.playing == Some(gen) && rec.gen == gen))
}

async fn play_private(state: Arc<AppState>, id: Uuid, gen: u64, frames: Vec<(u64, Vec<u8>)>) {
    tokio::time::sleep(state.parrot.delay()).await;
    if !still_playing(&state, id, gen) { return; }
    if !send_to_caller(&state, id, &build_floor(CALL_SIMPLEX_GRANTED, &id, 0)).await { return; }
    tokio::time::sleep(Duration::from_millis(GRANT_LEAD_MS)).await;
    let start = tokio::time::Instant::now();
    for (at, raw) in &frames {
        tokio::time::sleep_until(start + Duration::from_millis(*at)).await;
        if !still_playing(&state, id, gen) { return; }
        if !send_to_caller(&state, id, raw).await { return; }
    }
    tokio::time::sleep(state.parrot.tail()).await;
    if !still_playing(&state, id, gen) { return; }
    send_to_caller(&state, id, &build_floor(CALL_SIMPLEX_IDLE, &id, 1)).await;
    if let Ok(mut r) = state.parrot.recs.lock() {
        if let Some(rec) = r.get_mut(&id).filter(|rec| rec.playing == Some(gen)) { rec.playing = None; }
    }
    info!(uuid=%id, frames = frames.len(), "parrot played an over back");
}

/// Whether the parrot's own playback `id` still holds the call.
async fn group_call_alive(state: &AppState, id: Uuid) -> bool {
    state.inner.read().await.calls.get(&id).is_some_and(|c| c.owner == PARROT_OWNER)
}

async fn play_group(state: Arc<AppState>, gssi: u32, frames: Vec<(u64, Vec<u8>)>, talker: u32) {
    tokio::time::sleep(state.parrot.delay()).await;
    let Some(issi) = state.parrot.config().active_issi() else { return };
    if state.parrot.config().active_gssi() != Some(gssi) { return; }
    // Someone talks on the group: the playback waits for the floor.
    let deadline = Instant::now() + GROUP_BUSY_WAIT;
    while state.inner.read().await.group_floor.contains_key(&gssi) {
        if Instant::now() >= deadline {
            warn!(gssi, talker, "parrot playback dropped: the group stayed busy");
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let id = Uuid::new_v4();
    crate::router::handle_packet(state.clone(), PARROT_OWNER, protocol::build_group_tx(&id, issi, gssi, 0)).await;
    if !group_call_alive(&state, id).await {
        warn!(gssi, talker, "parrot playback not started (group floor taken)");
        return;
    }
    let start = tokio::time::Instant::now();
    for (at, raw) in &frames {
        tokio::time::sleep_until(start + Duration::from_millis(*at)).await;
        // Pre-empted by a call of higher priority: stop.
        if !group_call_alive(&state, id).await { return; }
        crate::router::handle_packet(state.clone(), PARROT_OWNER, with_id(raw, &id)).await;
    }
    tokio::time::sleep(state.parrot.tail()).await;
    if !group_call_alive(&state, id).await { return; }
    crate::router::handle_packet(state.clone(), PARROT_OWNER, protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
    info!(uuid=%id, gssi, talker, frames = frames.len(), "parrot played a group over back");
}

/// `GET /api/parrot` (admins): `[parrot]` as it is right now.
pub async fn api_get(State(state): State<Arc<AppState>>) -> Json<ParrotConfig> {
    Json(state.parrot.config())
}

/// `POST /api/parrot/set` (admins): `[parrot]`, applied at once and written to
/// the config file. While it is on, its group is added to the allowed
/// talkgroups (so managed talkgroups admit it) and both numbers get the name
/// "Parrot" in the Directory unless they have a name already.
pub async fn api_set(State(state): State<Arc<AppState>>, Json(req): Json<ParrotConfig>) -> Response {
    if let Err(e) = req.validate() { return (StatusCode::BAD_REQUEST, e).into_response(); }
    if let Ok(mut c) = state.parrot.cfg.write() { *c = req; }
    let mut group_added = false;
    if let Some(issi) = req.active_issi() {
        if let Ok(mut n) = state.issi_names.write() { n.entry(issi.to_string()).or_insert_with(|| "Parrot".into()); }
    }
    if let Some(gssi) = req.active_gssi() {
        if let Ok(mut n) = state.gssi_names.write() { n.entry(gssi.to_string()).or_insert_with(|| "Parrot".into()); }
        if let Ok(mut t) = state.talkgroups.write() {
            if !t.allowed.contains(&gssi) { t.allowed.push(gssi); t.allowed.sort_unstable(); group_added = true; }
        }
    }
    if group_added && state.talkgroups_managed() { crate::group_access::refresh(&state).await; }
    warn!(enabled = req.enabled, issi = req.issi, gssi = ?req.active_gssi(), delay_ms = req.delay_ms, tail_ms = req.tail_ms, max_secs = req.max_secs, "parrot settings changed");
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    match crate::dashboard::save_config(&state, &cfg).await {
        Ok(_) => Json(req).into_response(),
        Err(e) => {
            crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("applied, but writing the config file failed: {e}")).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ConnVersion, CALL_GROUP_TX, CALL_SETUP_REJECT, SUB_AFFILIATE, SUB_REGISTER};
    use crate::router::handle_packet;
    use crate::state::Client;

    const PARROT: u32 = 9990;

    fn state_with(cfg: ParrotConfig) -> Arc<AppState> {
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        config.fallback_broadcast_when_no_affiliations = false;
        config.parrot = cfg;
        let state = Arc::new(AppState::new(config, "test.toml".into()).0);
        tokio::spawn(run(state.clone()));
        state
    }

    fn on(delay_ms: u32) -> ParrotConfig {
        // Tail 0 (at least one frame time, as before `tail_ms`) keeps the tests fast.
        ParrotConfig { enabled: true, issi: PARROT, delay_ms, tail_ms: 0, ..Default::default() }
    }

    async fn connect(state: &AppState, mode: ClientMode) -> (ClientId, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = Uuid::new_v4();
        state.inner.write().await.clients.insert(id, Client { tx, mode, version: ConnVersion::V0, version_announced: true, remote_addr: None, connected_at_ms: 0, username: None });
        (id, rx)
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn voice(id: &Uuid, n: u8) -> Vec<u8> {
        protocol::build_traffic_frame(id, &[n; protocol::ACELP_CODED_FRAME_BYTES], &[n; protocol::ACELP_CODED_FRAME_BYTES])
    }

    fn setup(id: &Uuid, caller: u32, to: u32, duplex: bool) -> Vec<u8> {
        let mut p = protocol::build_circular_call_setup(id, caller, to, 0);
        p[61] = duplex as u8;
        p[62] = duplex as u8;
        p
    }

    fn states(packets: &[Vec<u8>]) -> Vec<(u8, u8)> {
        packets.iter().map(|p| (p[0], p[1])).collect()
    }

    fn sleep(ms: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_millis(ms)) }

    /// Ten stations, 25 radios: every radio registered somewhere, the first
    /// 20 in the parrot group.
    async fn network(state: &Arc<AppState>, gssi: u32) -> Vec<(ClientId, mpsc::UnboundedReceiver<Vec<u8>>)> {
        let mut bs = Vec::new();
        for _ in 0..10 { bs.push(connect(state, ClientMode::Basestation).await); }
        for r in 0..25u32 {
            let (id, _) = &bs[(r % 10) as usize];
            let issi = 2_320_000 + r;
            handle_packet(state.clone(), *id, protocol::build_subscriber_message(SUB_REGISTER, issi, &[])).await;
            if r < 20 { handle_packet(state.clone(), *id, protocol::build_subscriber_message(SUB_AFFILIATE, issi, &[gssi])).await; }
        }
        for (_, rx) in bs.iter_mut() { drain(rx); }
        bs
    }

    #[tokio::test]
    async fn simplex_call_is_answered_and_each_over_comes_back_after_the_delay() {
        let state = state_with(on(150));
        let mut bs = network(&state, 1).await;
        let (station, rx) = &mut bs[3];
        let id = Uuid::new_v4();
        handle_packet(state.clone(), *station, setup(&id, 2_320_003, PARROT, false)).await;
        let answer = drain(rx);
        assert_eq!(states(&answer), vec![(protocol::CLASS_CALL_CONTROL, protocol::CALL_SETUP_ACCEPT), (protocol::CLASS_CALL_CONTROL, protocol::CALL_CONNECT_REQUEST)]);
        assert_eq!(answer[1][61], 0, "the answer keeps the call simplex");
        assert_eq!(&answer[1][18..22], &PARROT.to_le_bytes(), "the parrot answers");
        // One over: three frames, then PTT released.
        handle_packet(state.clone(), *station, build_floor(CALL_SIMPLEX_GRANTED, &id, 0)).await;
        for n in 1..=3 { handle_packet(state.clone(), *station, voice(&id, n)).await; sleep(20).await; }
        handle_packet(state.clone(), *station, build_floor(CALL_SIMPLEX_IDLE, &id, 1)).await;
        sleep(80).await;
        assert!(drain(rx).is_empty(), "nothing before the delay");
        sleep(400).await;
        let back = drain(rx);
        let mut want = vec![build_floor(CALL_SIMPLEX_GRANTED, &id, 0)];
        want.extend((1..=3).map(|n| voice(&id, n)));
        want.push(build_floor(CALL_SIMPLEX_IDLE, &id, 1));
        assert_eq!(back, want, "floor, the same frames in order, floor released");
        // The other stations hear nothing of it.
        for (i, (_, other)) in bs.iter_mut().enumerate() { if i != 3 { assert!(drain(other).is_empty()); } }
        // Hanging up ends the call.
        let (station, _) = &bs[3];
        handle_packet(state.clone(), *station, protocol::build_call_cause(CALL_RELEASE, &id, 0)).await;
        assert!(!state.inner.read().await.calls.contains_key(&id));
        assert!(state.parrot.recs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_floor_stays_open_for_the_tail_after_the_last_frame() {
        // The station plays out its buffer after the last frame; releasing the floor at once cut
        // the end of the playback (field test 2026-11-21).
        let state = state_with(ParrotConfig { tail_ms: 500, ..on(0) });
        let (station, mut rx) = connect(&state, ClientMode::Basestation).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), station, setup(&id, 2_320_001, PARROT, false)).await;
        drain(&mut rx);
        handle_packet(state.clone(), station, build_floor(CALL_SIMPLEX_GRANTED, &id, 0)).await;
        handle_packet(state.clone(), station, voice(&id, 1)).await;
        handle_packet(state.clone(), station, build_floor(CALL_SIMPLEX_IDLE, &id, 1)).await;
        sleep(350).await;
        let first = drain(&mut rx);
        assert_eq!(first, vec![build_floor(CALL_SIMPLEX_GRANTED, &id, 0), voice(&id, 1)], "frame played, floor still held");
        sleep(500).await;
        assert_eq!(drain(&mut rx), vec![build_floor(CALL_SIMPLEX_IDLE, &id, 1)], "released after the tail");
    }

    #[tokio::test]
    async fn group_playback_keeps_the_call_for_the_tail() {
        let state = state_with(ParrotConfig { tail_ms: 500, ..on(0) });
        let mut bs = network(&state, PARROT).await;
        let id = Uuid::new_v4();
        let talker_bs = bs[0].0;
        handle_packet(state.clone(), talker_bs, protocol::build_group_tx(&id, 2_320_001, PARROT, 0)).await;
        handle_packet(state.clone(), talker_bs, voice(&id, 1)).await;
        handle_packet(state.clone(), talker_bs, protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
        for (_, rx) in bs.iter_mut() { drain(rx); }
        sleep(300).await;
        let got = drain(&mut bs[1].1);
        assert_eq!(got.len(), 2, "GROUP_TX and the frame, no GROUP_IDLE yet: {got:?}");
        sleep(500).await;
        let got = drain(&mut bs[1].1);
        assert_eq!(got.len(), 1, "GROUP_IDLE after the tail");
        assert_eq!(&got[0][..2], &protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)[..2]);
    }

    #[tokio::test]
    async fn an_over_without_simplex_idle_ends_after_a_quiet_second() {
        let state = state_with(on(0));
        let (station, mut rx) = connect(&state, ClientMode::Basestation).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), station, setup(&id, 2_320_001, PARROT, false)).await;
        drain(&mut rx);
        handle_packet(state.clone(), station, voice(&id, 7)).await;
        sleep(600).await;
        assert!(drain(&mut rx).is_empty());
        sleep(1000).await;
        assert!(drain(&mut rx).contains(&voice(&id, 7)));
    }

    #[tokio::test]
    async fn talking_again_during_the_delay_drops_the_old_over() {
        let state = state_with(on(300));
        let (station, mut rx) = connect(&state, ClientMode::Basestation).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), station, setup(&id, 2_320_001, PARROT, false)).await;
        drain(&mut rx);
        handle_packet(state.clone(), station, voice(&id, 1)).await;
        handle_packet(state.clone(), station, build_floor(CALL_SIMPLEX_IDLE, &id, 1)).await;
        sleep(50).await;
        handle_packet(state.clone(), station, build_floor(CALL_SIMPLEX_GRANTED, &id, 0)).await;
        handle_packet(state.clone(), station, voice(&id, 2)).await;
        handle_packet(state.clone(), station, build_floor(CALL_SIMPLEX_IDLE, &id, 1)).await;
        sleep(700).await;
        let back = drain(&mut rx);
        assert!(!back.contains(&voice(&id, 1)) && back.contains(&voice(&id, 2)));
    }

    #[tokio::test]
    async fn duplex_call_echoes_each_frame_after_the_delay() {
        let state = state_with(on(100));
        let (station, mut rx) = connect(&state, ClientMode::Basestation).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), station, setup(&id, 2_320_001, PARROT, true)).await;
        let answer = drain(&mut rx);
        assert_eq!(answer[1][61], 1, "the answer keeps the call duplex");
        handle_packet(state.clone(), station, voice(&id, 5)).await;
        sleep(30).await;
        assert!(drain(&mut rx).is_empty());
        sleep(200).await;
        assert_eq!(drain(&mut rx), vec![voice(&id, 5)]);
    }

    #[tokio::test]
    async fn duplex_call_ends_after_the_maximum_length() {
        let state = state_with(ParrotConfig { max_secs: 1, ..on(0) });
        let (station, mut rx) = connect(&state, ClientMode::Basestation).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), station, setup(&id, 2_320_001, PARROT, true)).await;
        drain(&mut rx);
        state.parrot.recs.lock().unwrap().get_mut(&id).unwrap().answered_at -= Duration::from_secs(2);
        handle_packet(state.clone(), station, voice(&id, 5)).await;
        sleep(100).await;
        assert!(drain(&mut rx).contains(&protocol::build_call_cause(CALL_RELEASE, &id, 0)));
        assert!(!state.inner.read().await.calls.contains_key(&id));
    }

    #[test]
    fn an_over_is_recorded_up_to_the_maximum_length() {
        let mut rec = Rec::new(Kind::Simplex, 1);
        let t0 = Instant::now();
        for i in 0..100u64 { rec.record(&[i as u8], t0 + Duration::from_millis(i * 60), 1000); }
        assert_eq!(rec.frames.len(), 17, "frames at 0..=960 ms");
        assert!(rec.frames.iter().all(|(at, _)| *at <= 1000));
    }

    #[tokio::test]
    async fn group_over_is_played_back_on_the_group_as_the_parrot() {
        let state = state_with(on(100));
        let mut bs = network(&state, PARROT).await;
        let id = Uuid::new_v4();
        let talker_bs = bs[0].0;
        handle_packet(state.clone(), talker_bs, protocol::build_group_tx(&id, 2_320_000, PARROT, 0)).await;
        for n in 1..=4 { handle_packet(state.clone(), talker_bs, voice(&id, n)).await; sleep(10).await; }
        handle_packet(state.clone(), talker_bs, protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
        for (_, rx) in bs.iter_mut() { drain(rx); }
        sleep(500).await;
        // Radios 0..20 are on every station (r % 10): all ten hear the playback,
        // the talker's own station too.
        for (_, rx) in bs.iter_mut() {
            let got = drain(rx);
            assert_eq!(got.len(), 6, "GROUP_TX, four frames, GROUP_IDLE");
            assert_eq!((got[0][1], &got[0][18..22], &got[0][22..26]), (CALL_GROUP_TX, &PARROT.to_le_bytes()[..], &PARROT.to_le_bytes()[..]));
            let new_id = &got[0][2..18];
            assert_ne!(new_id, id.as_bytes(), "a new call");
            let new_id = Uuid::from_slice(new_id).unwrap();
            assert_eq!(&got[1..5], &(1..=4).map(|n| voice(&new_id, n)).collect::<Vec<_>>()[..]);
            assert_eq!(got[5], protocol::build_call_cause(CALL_GROUP_IDLE, &new_id, 0));
        }
        assert!(state.inner.read().await.calls.is_empty() && state.inner.read().await.group_floor.is_empty());
    }

    #[tokio::test]
    async fn separate_gssi_and_no_group() {
        let cfg = ParrotConfig { separate_gssi: true, gssi: 777, ..on(0) };
        assert_eq!((cfg.active_issi(), cfg.active_gssi()), (Some(PARROT), Some(777)));
        let cfg = ParrotConfig { separate_gssi: true, gssi: 0, ..on(0) };
        assert_eq!(cfg.active_gssi(), None);
        assert_eq!(on(0).active_gssi(), Some(PARROT), "same number by default");
        assert_eq!(ParrotConfig { enabled: false, ..on(0) }.active_issi(), None);
        // An over on the old number is not recorded with a separate GSSI.
        let state = state_with(ParrotConfig { separate_gssi: true, gssi: 777, ..on(50) });
        let mut bs = network(&state, PARROT).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), bs[0].0, protocol::build_group_tx(&id, 2_320_000, PARROT, 0)).await;
        handle_packet(state.clone(), bs[0].0, voice(&id, 1)).await;
        handle_packet(state.clone(), bs[0].0, protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
        for (_, rx) in bs.iter_mut() { drain(rx); }
        sleep(200).await;
        for (_, rx) in bs.iter_mut() { assert!(drain(rx).is_empty()); }
    }

    #[tokio::test]
    async fn switched_off_the_number_is_an_ordinary_unknown_destination() {
        let state = state_with(ParrotConfig { enabled: false, ..on(0) });
        let (station, mut rx) = connect(&state, ClientMode::Basestation).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), station, setup(&id, 2_320_001, PARROT, false)).await;
        assert_eq!(drain(&mut rx), vec![protocol::build_call_cause(CALL_SETUP_REJECT, &id, protocol::CAUSE_CALLED_PARTY_NOT_REACHABLE)]);
    }

    #[tokio::test]
    async fn calls_from_a_peer_server_are_left_to_its_own_parrot() {
        let state = state_with(on(0));
        let (peer, _rx) = connect(&state, ClientMode::Peer).await;
        let id = Uuid::new_v4();
        handle_packet(state.clone(), peer, setup(&id, 2_320_001, PARROT, false)).await;
        assert!(state.parrot.recs.lock().unwrap().is_empty());
        let g = Uuid::new_v4();
        handle_packet(state.clone(), peer, protocol::build_group_tx(&g, 2_320_001, PARROT, 0)).await;
        assert!(state.parrot.recs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_parrot_talks_on_its_group_with_the_allow_list_enforced() {
        let state = state_with(on(0));
        if let Ok(mut a) = state.allowlist.write() { a.enabled = true; }
        let mut bs = network(&state, PARROT).await;
        let id = Uuid::new_v4();
        // A radio on the allow list talks (2320000 admitted).
        if let Ok(mut a) = state.allowlist.write() { a.issis.insert(2_320_000); }
        handle_packet(state.clone(), bs[0].0, protocol::build_group_tx(&id, 2_320_000, PARROT, 0)).await;
        handle_packet(state.clone(), bs[0].0, voice(&id, 1)).await;
        handle_packet(state.clone(), bs[0].0, protocol::build_call_cause(CALL_GROUP_IDLE, &id, 0)).await;
        for (_, rx) in bs.iter_mut() { drain(rx); }
        sleep(200).await;
        assert!(drain(&mut bs[1].1).iter().any(|p| p[1] == CALL_GROUP_TX), "the parrot is not on the allow list but still talks");
    }

    #[test]
    fn validation() {
        assert!(on(0).validate().is_ok());
        assert!(ParrotConfig { issi: 0, ..on(0) }.validate().is_err(), "on needs an ISSI");
        assert!(ParrotConfig { issi: 0x100_0000, ..on(0) }.validate().is_err());
        assert!(ParrotConfig { delay_ms: 60_001, ..on(0) }.validate().is_err());
        assert!(ParrotConfig { max_secs: 0, ..on(0) }.validate().is_err());
        assert!(ParrotConfig { tail_ms: 5_001, ..on(0) }.validate().is_err());
        assert!(ParrotConfig { tail_ms: 5_000, ..on(0) }.validate().is_ok());
        assert!(ParrotConfig { max_secs: 601, ..on(0) }.validate().is_err());
        assert!(ParrotConfig::default().validate().is_ok(), "off without a number");
        assert!(ParrotConfig::default().is_unset());
    }

    #[tokio::test]
    async fn saving_names_both_numbers_and_allows_the_group() {
        let dir = std::env::temp_dir().join(format!("parrot-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brew-server.toml");
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        std::fs::write(&path, "").unwrap();
        let state = Arc::new(AppState::new(config, path.clone()).0);
        if let Ok(mut n) = state.issi_names.write() { n.insert(PARROT.to_string(), "Echo".into()); }
        let r = api_set(State(state.clone()), Json(ParrotConfig { separate_gssi: true, gssi: 777, ..on(500) })).await;
        assert!(r.status().is_success());
        assert!(std::fs::read_to_string(&path).unwrap().contains("[parrot]"), "written to the config file");
        assert_eq!(state.issi_names.read().unwrap().get("9990").map(String::as_str), Some("Echo"), "an existing name stays");
        assert_eq!(state.gssi_names.read().unwrap().get("777").map(String::as_str), Some("Parrot"));
        assert!(state.talkgroups.read().unwrap().allowed.contains(&777));
        assert_eq!(state.config_snapshot().parrot.delay_ms, 500);
        let bad = api_set(State(state.clone()), Json(ParrotConfig { max_secs: 0, ..on(0) })).await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
