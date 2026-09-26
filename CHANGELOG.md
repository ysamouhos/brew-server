# Changelog

All notable changes to brew-server, newest first.

Version 1.9.1 adds:

- **Call inactivity timeout.** New top-level `call_inactivity_timeout_seconds`
  (default `60`, `0` disables), shared by Brew and SIP calls. A Brew call with
  no voice/DTMF frames or call control for that long is ended like a normal
  hangup (`CALL_GROUP_IDLE`/`CALL_RELEASE`); an answered SIP call that
  receives no RTP for that long is torn down like a BYE. Fixes group calls
  whose `GROUP_IDLE` was lost lingering on the dashboard (duration counting
  up) until the 4h `max_call_duration_seconds` sweep. SIP-bridged Brew calls
  are judged on the RTP side only. Both call sweeps now run every 5s.

Version 1.9 adds:

- **Logged-in username and a logout control on every dashboard page.**
  Next to the "Live" status indicator, the header now shows the
  authenticated dashboard user's name (via `/api/whoami`, already added for
  the privileged-users feature) and a Logout button. HTTP Basic auth has no
  real server-side session to end, so "logout" is the standard best-effort
  trick: navigating to the current page with bogus embedded credentials
  (`//logout:<timestamp>@host/path`) so the browser discards its cached
  valid ones and re-prompts on the next request. Both elements stay hidden
  when no credential is present (auth disabled, or not yet logged in).

Version 1.8 adds:

- **Privileged dashboard users.** New `[dashboard].admins` config: a list of
  usernames (a subset of `[dashboard.users]`'s keys) allowed to view or edit
  `/settings` and its `/api/config/*` endpoints. Every other authenticated
  dashboard user keeps full read access to the rest of the dashboard, just
  not that page (a real `403`, not a blank/hidden page) or its APIs. Leaving
  `admins` empty keeps the previous all-or-nothing behavior. Implemented as
  a second `require_admin` middleware layer wrapping just the settings
  sub-router, applied inside the existing `require_basic` layer so it only
  ever runs after a request is already authenticated. New `/api/whoami`
  endpoint lets the dashboard's own JS hide the Settings nav link for
  non-admins (a UI convenience only; the real enforcement is server-side).
  Note this does *not* cover the Control panel (kick/DGNA/restart/stop),
  which is unaffected and stays available to every authenticated user.

Version 1.7 adds:

- **Fixed the actual root cause of garbled/choppy/silent SIP<->Brew audio:
  wrong `FRAME_TRAFFIC_CHANNEL` wire format.** This server always sent (and
  parsed) one raw 18-byte ACELP subframe per Brew voice message, no header.
  A real Basestation doesn't speak that: confirmed against FlowStation's
  `net_brew::entity::handle_voice_frame`/`handle_ul_voice`, every
  `FRAME_TRAFFIC_CHANNEL` payload is 36 bytes -- 1 "STE" header byte (`0x00`
  = normal speech) followed by 35 bytes packing *two* 137-bit ACELP
  subframes (60ms) back-to-back, MSB-first, with a single 6-bit pad at the
  very end (not two independently 7-bit-padded subframes concatenated). A
  real Basestation silently discards anything shorter
  (`data.len() < 36 -> drop with a warning, never reaching the radio`),
  which is exactly why PSTN->ISSI audio counted as sent by this server's own
  metrics (added in the diagnostics below) but was never heard: every frame
  was rejected on arrival. In the other direction, a real Basestation's
  genuine 36-byte STE frames were misread as one corrupt 18-byte subframe
  each (missing the header-byte offset and losing more than half the real
  bits) -- garbled audio, and undercounted at exactly half the true 33.3
  frames/sec speech rate, matching the "choppy" symptom precisely. New
  `protocol::pack_ste_voice_payload`/`unpack_ste_voice_payload` do the exact
  bit-level (re)packing FlowStation's own encoder/decoder use;
  `build_traffic_frame` now takes two subframes and produces a real 36-byte
  STE payload; `transcode::task`'s ACELP-side ticker moved from 30ms/1
  subframe to 60ms/2 subframes to match.
- **Transcoder diagnostics.** `transcode::task` now logs a per-call summary
  every 5 seconds at the default log level: `rtp_in`/`rtp_out` (the SIP/PSTN
  leg), `acelp_in`/`acelp_out` (the Brew/ISSI leg), an `*_underflow` count
  for each (a paced tick that fired with too little buffered audio to emit --
  starvation, not corruption), and the current buffered-sample depth on each
  side. This is what surfaced the wire-format bug above: PSTN->ISSI counters
  looked perfectly healthy (frames generated and queued continuously, zero
  underflow) even though the user heard nothing, proving the fault was past
  this task's own output, not within it -- and the ISSI->PSTN counters
  showing exactly half the expected frame rate pointed straight at a framing
  mismatch rather than packet loss.

Version 1.6 adds:

- **Transcoder diagnostics.** `transcode::task` now logs a per-call summary
  every 5 seconds at the default log level: `rtp_in`/`rtp_out` (the SIP/PSTN
  leg), `acelp_in`/`acelp_out` (the Brew/ISSI leg), an `*_underflow` count
  for each (a paced tick that fired with too little buffered audio to emit --
  starvation, not corruption), and the current buffered-sample depth on each
  side. The 1.5 pacing fix resolved a confirmed RTP-timing bug, but garbled/
  choppy/silent audio reports persisted after it -- these counters exist to
  tell the next report apart at a glance: packet loss upstream (`*_in` stops
  incrementing), this task's own buffer starving (rising `*_underflow` with
  low buffered-sample counts), or a healthy transcoder feeding into a problem
  further down the pipeline (both directions' counts look normal).

Version 1.5 adds:

- **Fixed bursty/unpaced transcoder output: the real cause of garbled and
  missing SIP<->Brew audio.** Diagnosed from a live production `tcpdump`
  capture: outbound RTP packets frequently went out in ~60us-apart pairs
  instead of a steady 20ms cadence, even though inbound audio arrived cleanly
  paced. Root cause: `transcode::task::spawn`'s pump decoded and immediately
  emitted output the instant a buffer crossed a full frame's worth of
  samples, draining it to completion in a `while` loop with no real-time
  gap between iterations. ACELP's 240-sample/30ms frame and RTP's
  160-sample/20ms packet share no common multiple shorter than 480 samples,
  so roughly every third RTP packet (or every other ACELP frame) completed
  *two* output units back-to-back. G.711/SIP jitter buffers mostly tolerate
  that; a real TETRA Basestation's downlink traffic channel is locked to a
  rigid TDMA slot schedule and does not -- frames delivered off that cadence
  are dropped or garbled on the radio side. Decoding (on arrival, still
  event-driven/bursty as the network delivers it) is now fully decoupled
  from emission (on two dedicated tickers, a 20ms one for RTP and a 30ms one
  for ACELP, each emitting at most one unit per tick regardless of how much
  piled up in between) via the existing sample buffers acting as a proper
  jitter absorber between the two paced clocks.

Version 1.4 adds:

- **Fixed the persistent SIP<->Brew one-way/garbled audio: advertised host
  was literally "0.0.0.0".** With the common default config
  (`sip.listen = "0.0.0.0:PORT"`, `sip.advertised_host` unset), the fallback
  used `local.ip()` from the bound socket's own address -- which for a
  wildcard bind is literally the string `"0.0.0.0"`, not a real interface
  address. Every SDP body this server generated (both its own outbound
  INVITE offers and its answers to inbound INVITEs) therefore advertised
  `c=IN IP4 0.0.0.0`/`o=... IN IP4 0.0.0.0`: unroutable, and some SIP stacks
  read `c=0.0.0.0` as RFC 3264 5.1's "this stream is on hold" and never send
  media there at all -- explaining reports of one whole call direction
  (PSTN->ISSI) being totally silent while the other (ISSI->PSTN) limped
  along on symmetric-RTP latching alone. `sip::transport::run` now detects a
  real outbound-facing local IP (via a UDP "connect" to a public address --
  no packet is sent, it just asks the OS routing table which local
  interface/IP would be used) whenever the bind address is unspecified,
  instead of ever advertising the wildcard. Set `sip.advertised_host`
  explicitly (still the most reliable option, especially behind NAT) to skip
  this detection entirely.

Version 1.3 adds:

- **Basestation locations on the MS map.** New `[bts_locations]` config,
  keyed by the same numeric Brew username each Basestation authenticates
  with under `[auth.users]` -- so an entry automatically matches whichever
  live connection logs in as that identity, with no separate ID scheme.
  Each entry (`name`, `lat`, `lon`) is editable from the `/settings`
  dashboard page (or the raw-TOML editor) and shown as its own marker on
  `/map` (new `/api/bts-locations`, merging the fixed config location with
  live connection state), with a popup showing the Basestation's name,
  coordinates, live IP address and connect status. Threading the
  authenticated username through to the connection required carrying it from
  Digest verification (`server::verify_digest`) through the auth-session
  handshake to `Client.username`, which previously only tracked the
  connection's mode/version/remote address.

Version 1.2 adds:

- **APRS forwarding for mobile-station LIP positions.** New `[aprs]` config
  (`enabled`, `server` — an APRS-IS host:port, `callsign`/`passcode` — this
  server's own APRS-IS login, `symbol_table`/`symbol_code`, `comment`,
  `object_name_prefix`, `min_report_interval_seconds` rate limit,
  `reconnect_interval_seconds`). When enabled, every LIP fix decoded from
  Brew SDS traffic (`router::handle_sds_header`/`handle_sds_transfer`, the
  same decode path that already feeds the dashboard's MS map) is also queued
  to a new `aprs` module, which maintains a reconnecting TCP link to
  APRS-IS and reports each ISSI as its own APRS *object*
  (`;OBJECTNAME*DDHHMMz...`) under this server's single login — the same
  technique real DMR/D-STAR-to-APRS gateways use, so no per-radio APRS
  callsign/passcode is needed. Position queuing is decoupled via a channel so
  a slow or unreachable APRS-IS server never blocks call/SDS routing.
- **Fixed garbled SIP->ISSI audio caused by unfiltered RTP.** The
  transcoder's RTP receive loop decoded every incoming UDP datagram's bytes
  after a fixed 12-byte header as G.711 audio, regardless of the packet's
  actual payload type and without accounting for an optional CSRC list or
  header extension. Anything else sharing the port — comfort noise, RFC 2833
  DTMF events, or a packet with CSRC/extension data — got its bytes decoded
  as if they were audio samples, corrupting the PCM handed to the ACELP
  encoder. Now the payload offset is computed from the real CSRC
  count/extension bit, and any packet whose payload type doesn't match the
  negotiated codec is dropped instead of decoded.

Version 1.1 adds:

- **Server-to-server federation.** Multiple brew-server instances can now be
  linked (chain or star topology) so calls, SDS and subscriber/group
  registrations reach a remote site's Basestations and mobile stations. A
  peer link connects and authenticates exactly like a Basestation does, over
  the same Brew WebSocket protocol, tagged `X-Brew-Mode: Peer` (new
  `[[federation.peers]]` config, dialled outbound with reconnect; an inbound
  link needs no matching config, just Basestation-style auth). Registrations
  propagate peer to peer automatically — each server relays what it learns to
  its *other* peers (split-horizon, safe for any loop-free topology) — so
  private/group call routing and SDS forwarding across servers need no
  federation-specific routing code at all: they already resolve a
  destination via the same `inner.subscribers`/`inner.group_clients` tables
  used for local routing, which now include remote entries. A newly
  (re)connected peer gets a full snapshot of everything this server currently
  knows, in both directions, so it isn't blind to registrations that predate
  the link.
- **DTMF forwarding.** Some real clients (e.g. nexus-bs, a FlowStation-derived
  Basestation) send in-call DTMF as a Brew `FRAME_DTMF` frame (one ASCII
  digit per frame) — outside this server's original protocol coverage, and
  previously silently dropped. It now routes like a voice frame to every
  other Brew-side call participant, and for a SIP-bridged call the
  transcoder converts it to RFC 4733 (formerly 2833) telephone-event RTP
  instead of dropping it there too.
- **Per-ISSI RSSI from the main Brew channel.** Some real clients also send
  `CLASS_SERVICE` type `0x10` (`{"issi":N,"rssi_dbfs":F}`) — previously
  parsed but unconditionally ignored. It's now stored and exposed at
  `/api/rssi`, merged into the dashboard's existing "MS RSSI" column
  alongside the Basestation Telemetry channel's own per-station RSSI.

Version 1.0 adds:

- **ACELP<->G.711 media transcoder for SIP<->Brew calls.** SIP legs are
  steered to G.711 (PCMU/PCMA); Brew traffic frames carry ACELP. A new
  `transcode` module vendors the ETSI EN 300 395-2 reference TETRA codec
  (`third_party/tetra-codec/`, compiled via `build.rs`) alongside a pure-Rust
  G.711 implementation, and a bidirectional pump (`transcode::task`) bridges
  RTP and Brew traffic frames in both directions, so PSTN/SIP calls to and
  from a mobile terminal actually carry audio, not just signalling.
- **Complete Brew<->SIP private-call accept/ring/answer handshake.**
  Previously the bridge answered SIP `INVITE`s with `200 OK` immediately and
  never reacted to the ISSI's `SETUP_ACCEPT`/`ALERT`/`CONNECT_REQUEST` —
  callers got no ringback, and pressing accept on a mobile terminal did
  nothing. Now: `SETUP_ACCEPT`/`ALERT` -> SIP `180 Ringing`; `CONNECT_REQUEST`
  (accept pressed) -> `CALL_CONNECT_CONFIRM` back to the ISSI *and* SIP
  `200 OK` together; `SETUP_REJECT`/`RELEASE` before answer -> SIP `486` and
  teardown. The reverse direction (Brew->SIP) sends `CALL_SETUP_ACCEPT`
  immediately and drives `CALL_ALERT`/`CALL_CONNECT_CONFIRM` from Asterisk's
  own `180`/`200` responses, plus the SIP `ACK` a `200 OK` to our own
  outbound `INVITE` requires (previously missing — Asterisk would keep
  retransmitting the `200` and drop the dialog). Fixed along the way:
  `CALL_CONNECT_CONFIRM` needs a 2-byte grant/permission payload, not an
  empty one (real clients reject it outright otherwise); and the three
  pre-built SIP responses (`180`/`200`/`486`) now share one dialog `To`-tag
  instead of each independently generating its own, which previously caused
  a `BYE` built from the wrong tag to get `481`'d by the peer.
- **Route a mobile terminal's PSTN-style dialled number to SIP.** A terminal
  dialling a non-ISSI number (e.g. "9" + a 10-digit PSTN number) arrives with
  `destination = 0` and the digits in the Brew `CircularCall`'s ASCII
  `number` field, not `destination` — previously ignored entirely. That field
  is now used as the dialled string for `[[sip.routes]]` matching when
  present, and a new `strip_prefix` route field removes a leading literal
  (e.g. the outside-line "9") before it reaches an empty-`number` SIP trunk
  destination.
- **Dashboard settings editor.** A new `/settings` page can add/update/delete
  SIP extensions, trunks and voice routes, plus a raw-TOML editor covering
  every other setting. Saves validate then write atomically to the running
  process's config file, reusing the existing config-watcher restart-to-apply
  mechanism — no new hot-reload path needed.
- **Live connections page.** `/connections` (JSON at `/api/connections`)
  shows who is connected/registered *right now*: Brew connections (mode,
  protocol version, remote address, connect time), registered subscribers
  (both Terminal-mode MS and Basestation-gateway registrations, matching the
  main dashboard's panel), and SIP registrations/trunks. Distinct from
  `/registrations`, which is a historical event log.
- **Max call duration limits.** New `max_call_duration_seconds` (Brew
  station/private/group calls) and `sip.max_call_duration_seconds` (SIP
  calls) config settings force-end a call once it has run too long, the same
  way a normal hangup would (`CALL_RELEASE`/`CALL_GROUP_IDLE` or a SIP `BYE`,
  not a silent kill). Default 4 hours; `0` disables.
- **Server version shown on every dashboard page**, under the live-status
  indicator.
- Renamed `BlueStation`/`FlowStation` references throughout (code, UI, docs)
  to a single consistent `Basestation`/`Basestations`, matching the existing
  `ClientMode::Basestation`. The two WebSocket subprotocol identifiers real
  hardware negotiates with (`bluestation-control-v1`,
  `bluestation-telemetry-v2`) are deliberately left unchanged — they're wire
  compatibility strings, not display text. Also renamed the main dashboard's
  "Logs" panel to "Menu".
- **Fixed a misconfigured `sip.advertised_host` producing malformed SDP.** If
  `advertised_host` is accidentally set to `host:port` instead of a bare
  host (it's written verbatim into the SDP `c=`/`o=` lines, which never
  carry a port), the port is now stripped with a warning instead of silently
  emitting SDP that peers like Asterisk reject.

Version 0.8 adds:

- **Persistent telemetry SDS log.** SDS entries observed on a Basestation
  Telemetry channel (`SdsLog`) are now also appended to the same append-only
  history log used for calls/SDS, tagged with the reporting station, so the
  Telemetry SDS Log survives a server restart instead of resetting with the
  BTS's live in-memory state. Replayed on startup like the rest of `[storage]`
  history, and readable with the same `brew-history` tool (new `SdsTelemetry`
  record type).

Version 0.7 adds:

- **Persistent history.** Completed calls and SDS are written to an append-only
  binary log (`bincode`-framed, crash-safe on read) and replayed on startup, so
  call/SDS history and counters survive restarts. Configured under `[storage]`
  (`enabled`, `path`); it keeps everything with no rotation. A torn trailing
  record from a hard crash is detected and skipped. Read the log with the
  bundled `brew-history` tool: `brew-history brew-history.bin` for readable text,
  or `brew-history brew-history.bin --json` to pipe into `jq`.

- **Position mapping.** SDS position beacons are decoded to latitude/longitude,
  tracked per subscriber ISSI, and plotted on a new `/map` page (Leaflet +
  OpenStreetMap); a `/api/positions` endpoint exposes the latest fixes. Two
  sources are supported: **binary TETRA LIP** short location reports (ETSI TS
  100 392-18), decoded from the raw SDS relayed over the Brew channel, and
  **textual** beacons (APRS, decimal degrees, Maidenhead). No Basestation change
  is required — the LIP payload is decoded in `handle_sds_transfer` from the SDS
  that the Brew channel already relays. See "Position mapping" below.

Version 0.6 adds:

- **Brew protocol version 1 support.** The server advertises and negotiates the
  protocol version via the `X-Brew-Version` header on the discovery GET
  (responding `426 Upgrade Required` for versions it does not implement). Because
  real clients (e.g. Basestation) send no version header on the WebSocket
  handshake, the version is tracked **per connection** and resolved *lazily from
  message content*, defaulting to v0 and promoting to v1 once a v1-shaped
  call-control message is seen. The v1 SS-TPI `mnemonic[34]` talking-party name
  is parsed on `GROUP_TX`/`SETUP_REQUEST` (ETSI EN 300 392-9), and the
  `X-Brew-Mode` header (`Terminal`/`Basestation`) is tracked per client.
- **Dashboard control-panel fix.** The Basestation Control panel no longer wipes
  operator input: it reconciles station cards incrementally instead of rebuilding
  the DOM on every refresh, and reconnects its WebSocket in the background rather
  than reloading the page.
- **Paginated logs.** Recent calls, Recent SDS and the Telemetry SDS Log are
  paginated (10, 10 and 5 rows per page respectively) and have moved off the main
  dashboard onto their own linked pages: `/calls`, `/sds`, and `/telemetry-sds`.
- **Timeslot occupancy graphic.** Each Basestation telemetry card shows a small
  per-carrier TS1-TS4 grid indicating which timeslots are busy vs. available.
- **Registered-subscribers frame.** A dashboard panel lists which subscriber
  ISSIs are registered on each connected Basestation.

Version 0.5 adds:

- The monitoring dashboard now runs on its **own listener/port** (`[dashboard]`,
  default `:9003`), separate from the Brew protocol API. The Brew listener
  (`:9000`) serves only `/brew` and `/healthz`.
- Optional HTTP **Basic** authentication for the dashboard (`[dashboard.users]`).
- Optional native **TLS/HTTPS** for the dashboard (`[dashboard.tls]`), so it can
  be reached over `https://` / `wss://` independently of the Brew `[tls]` block.

Version 0.4 adds:

- Optional Basestation Telemetry ingestion channel (registrations, calls with
  carrier/timeslot, RF/DSP quality, SDR/host health, SDS log, emergency alarms),
  surfaced on the dashboard with an emergency-alarm banner.
- Optional Basestation Control channel (Kick MS, DGNA assign/deassign, live SDS
  add/delete/clear, clear emergency, restart/stop the service), with a per-station
  command panel on the dashboard.

Version 0.3 adds:

- TLS Support for https:// and wss://

Version 0.2 adds:

- HTTP Digest authentication compatible with Basestation's current WebSocket transport (MD5 + qop=auth).
- Single-use authenticated WebSocket session URLs returned by the discovery GET.
- Subscriber registration and talkgroup affiliation routing.
- Group speech routing with priority-based floor pre-emption.
- SDS routing using `SHORT_TRANSFER` + `SDS_TRANSFER`, and reverse `SDS_REPORT` delivery.
- Experimental private/simplex call routing for Brew call states 4..13.
