# Fleet Control

Fleet Control lets brew-server (the SwMI) manage the Basestations (FlowStation)
connected to it: see which stations allow it, their software and state, and
later change their configuration, update and restart them. It runs over the
existing Basestation control channel (WebSocket, subprotocol
`bluestation-control-v1`, `[control]`), so it works behind NAT and needs no
new port.

This document is the protocol and behaviour reference for phase 1
(foundation), phase 2 (remote update, section 8), phase 3 (maintenance and
restart, section 9), phases 4/5 (reading and changing the config,
section 10), phase 6 (templates, bulk changes, neighbour suggestions,
section 11) and phase 7 (graphs, ping and alarms, section 12). Later phases add
commands to the same scheme; each new command is announced as a capability
first, so older stations and servers keep working.

## 1. Principles

- **Opt-in per station.** A station is managed only when its local
  administrator switches Fleet Control on (FlowStation: Infrastructure page,
  under the Brew line, "Fleet Control — Station management by SwMI
  (BrewServer)", default off). Other stations on the same server are not
  touched.
- **Backwards compatible.** The server offers Fleet Control with
  `X-Brew-Extensions`. A station that sees no offer sends nothing new; a
  server that does not know the messages never gets them. Unknown messages
  are ignored on both sides.
- **Existing commands are independent.** Kick, DGNA, SDS, live SDS, emergency
  clear, ambience listening, service restart and shutdown work as before,
  whatever the switch says. Only Fleet Control commands need the switch.
- **The station stays in charge of its configuration.** (From phase 5:) the
  server sends change requests, the station checks them like its own
  dashboard and goes back to the previous configuration if it loses the
  server afterwards.

## 2. Negotiation

### 2.1 Server offer

brew-server adds `fleet-control` to `X-Brew-Extensions` whenever `[control]
enabled = true` (no extra setting):

- in the HTTP 101 of the **control channel** (this is the offer the station
  acts on: it sends `StationHello` only on a control connection whose 101
  names `fleet-control`);
- in the HTTP 101 of the **Brew connection** of a Basestation (so the station
  can show its switch even before its control channel is set up; the switch
  then opens a dialog for the control channel).

```
X-Brew-Extensions: fleet-control, dgna-report
```

`dgna-report` (DGNA groups reported by the station over the control
channel) is not part of Fleet Control and does not need its switch; see
`docs/Routing.md`, "Groups assigned by DGNA".

Same header syntax as the Brew extensions (`docs/Brew-Extensions.md` section
3.1): comma separated, trimmed, unknown names ignored.

### 2.2 Station capabilities

The station lists the Fleet Control commands it understands in
`StationHello.capabilities` while its switch is on (empty while off). The
server sends a Fleet Control command only to a station whose last hello
listed its capability.

| Capability | Commands | Phase |
| --- | --- | --- |
| `fleet-v1` | `FleetPing` | 1 |
| `update-v1` | `CheckUpdate`, `StartUpdate` (answers `UpdateInfo`, `CommandResult`; reports `UpdateProgress`) | 2 |
| `update-live-v1` | `StartUpdate` with `source` (live update, radio kept on; section 8.6) | 2 |
| `maintenance-v1` | `SetMaintenance` (red banner on the station's dashboard; section 9.1) | 3 |
| `restart-v1` | `RestartStation` (service restart or system reboot; section 9.2) | 3 |

Later phases add for example `config-v1`.

## 3. Messages

The control channel carries JSON in binary WebSocket frames, serde's
externally tagged form: a unit variant is a bare string (`"RestartService"`),
any other is `{"<Name>": {fields}}`. Server → station frames are commands,
station → server frames are responses.

### 3.1 `StationHello` (station → server)

Sent right after the control channel connects (when the server offered
`fleet-control`) and again every time the local switch changes, also when it
goes off.

```json
{"StationHello": {
  "protocol": 1,
  "version": "v0.5.9-8d7a363",
  "brew_login": "1000001",
  "mcc": 232,
  "mnc": 20,
  "fleet_control": true,
  "capabilities": ["fleet-v1"]
}}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `protocol` | u32 | Fleet Control protocol version, `1` |
| `version` | string | station software (`STACK_VERSION`) |
| `brew_login` | string or null | the station's Brew login (its number on the Brew server); null without Brew |
| `mcc`, `mnc` | u16 | network of the primary cell |
| `fleet_control` | bool | the local switch |
| `capabilities` | string list | Fleet Control commands it understands (section 2.2), empty while the switch is off |

A server must accept missing `brew_login`, `mcc`, `mnc`, `capabilities`
(defaults) and ignore unknown fields.

### 3.2 `CommandResult` (station → server)

The general answer to every Fleet Control command, correlated by `handle`.

```json
{"CommandResult": {"handle": 17, "ok": false, "code": "fleet_control_off", "message": "Fleet Control is switched off on this station"}}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `handle` | u32 | the command's handle |
| `ok` | bool | done |
| `code` | string | machine-readable outcome (below) |
| `message` | string | short English text for the dashboard |

Codes in phase 1: `ok`, `fleet_control_off` (the switch is off; the station
refuses every Fleet Control command). Phase 2 adds `busy` (an update is
already running), `call_active` (a call is in progress) and `unavailable`
(the station cannot do it, e.g. no OTA without its dashboard). Later phases
add codes such as `invalid_value`.

### 3.3 `FleetPing` (server → station)

Round-trip check (capability `fleet-v1`).

```json
{"FleetPing": {"handle": 17}}
```

Answer: `CommandResult` with `ok: true, code: "ok"` while the switch is on,
else `ok: false, code: "fleet_control_off"`. The station's control worker
answers it itself; it never reaches the radio stack.

## 4. Station behaviour (FlowStation)

- Switch: `[command] fleet_control = true|false` (default false). The
  dashboard changes it at run time: the config file is written and the
  running station uses the new value at once (no restart); the control worker
  sends a new `StationHello` within about a second.
- Control channel not configured (no `[command]`, or `enabled = false`) when
  the switch is turned on: a dialog asks for server (IP or DNS name, never
  pre-filled), port (443), TLS (on), user (pre-filled with the Brew login) and
  password (empty = take the Brew password, on the station; the password never
  goes to the browser). Saving writes `[command]` with `fleet_control = true`
  and restarts the service, because the control channel starts at boot.
- Sidebar (under Health): field "Fleet Control", green "on" / red "off",
  shown only while the server offers Fleet Control.
- Code: FlowStation `net_control/fleet.rs` (messages, answers),
  `net_control/worker.rs` (hello, switch changes), `net_dashboard/fleet.rs`
  (switch, dialog), documented in FlowStation `Docs/fleet-control.md`.

## 5. Server behaviour (brew-server)

### 5.1 Fleet Manager

Dashboard page **Fleet Control** (Infrastructure). Stations are keyed by their
**control login** (the user the station logs in with on the control channel;
kept identical to its Brew login). A station that shares a control login with
another station is seen as one.

- A hello with `fleet_control: true` adds or updates the station (software,
  network, capabilities). The name shown is the station's name on this server
  (Accounts), else its Brew number, else the control login.
- A station whose control channel drops stays listed as **offline** with the
  time it was last seen.
- A hello with `fleet_control: false` removes a listed station: at once when
  it switches off while online, else at its next login. The change log gets
  `Basestation <name> switched off Fleet Control`, and the dashboard shows
  this as an INFO popup (once per entry and browser; on the first visit only
  entries of the last 24 h).
- **Remove** (Fleet Control management, offline stations only; online ones show **Ping**) takes a station off the list; it comes
  back with its next hello while its switch is on.
- **Ping** (Fleet Control management) sends `FleetPing` and shows the result
  and the round-trip time.

### 5.2 Change log

Every change is logged with time, station, who (dashboard user, or the station
itself) and text, and kept **90 days**:

| Kind | When |
| --- | --- |
| `fleet_on` | a station switched Fleet Control on (first hello with the switch on) |
| `fleet_off` | a listed station switched Fleet Control off |
| `version` | a listed station came back with other software |
| `removed` | a user removed a station from the list |
| `update` | update run started / cancelled, a station started, waits for a call, restarts, moved to the end of the queue (phase 2) |
| `update_done` | a station is back with the new software |
| `update_alarm` | a station failed (shown red): refused, build failed, 45 min build limit, not back 5 min after the restart, back with the old software |
| `update_skip` | a station was not updated: up to date, offline twice, cancelled |
| `update_end` | the run is over, with the counts |
| `settings` | an admin changed "Updates repo" |
| `log_clear` | an admin cleared the change log (the only entry left after **Clear**) |
| `maintenance` | maintenance on (minutes, note) / off / ended (time up) (phase 3) |
| `restart` | service restart / system reboot sent; station back after it (with the seconds) (phase 3) |
| `restart_alarm` | a station not back 10 min after a restart / reboot (shown red) |

Phases 3–7 add configuration changes and restarts.

**Clear** (admins only, next to the search field) empties the change log after
a confirmation; one entry "Log cleared by <user>" stays.

Stations and log are kept in `<[storage] path>.fleet` (JSON, rewritten through
a temporary file) and survive restarts; without `[storage] enabled` they are
kept in memory only.

### 5.3 Rights

Dashboard users (Server settings → Users), next to the page ticks:

| Right | Page id | Allows |
| --- | --- | --- |
| Fleet Control visibility | `fleet` | see the Fleet Manager and the change log |
| Fleet Control management | `fleet_manage` | visibility plus every button (Remove, Check for updates, Update, Cancel, Maintenance, Restart / Shut down, config; on the Telemetry page Ping, Auto ping and acknowledging alarms) |

Admins have both. Viewer (not logged in) may get visibility, never
management. Neither right is given to users taken over from the config.

### 5.4 HTTP API

| Route | Right | Body | Answer |
| --- | --- | --- | --- |
| `GET /api/fleet` | visibility | – | `{offered, protocol, can_manage, stations: [...], log: [...]}` |
| `POST /api/fleet/ping` | management | `{"id": "<control login>"}` | `{ok, code, message, ms}`; 404 not connected, 504 no answer in 5 s |
| `POST /api/fleet/remove` | management | `{"id": "<control login>"}` | `{ok: true}`; 404 not listed |
| `POST /api/fleet/check` | management | `{"ids": [...]}` (empty: all) | `{results: [{id, check}]}` (section 8.3) |
| `POST /api/fleet/update` | management | `{"ids": [...], "mode": "parallel"\|"rolling", "during_call": bool, "interrupt_min": n, "live": bool}` | `{ok, job}`; 409 a run is going or no station; 400 live update without an https Updates repo |
| `POST /api/fleet/update/cancel` | management | `{}` (whole run) or `{"id": "<control login>"}` | `{ok: true}`; 404 nothing to cancel |
| `POST /api/fleet/settings` | admin | `{"repo": "...", "branch": "..."}` | `{ok: true}`; 400 not a git URL / branch |
| `POST /api/fleet/log/clear` | admin | `{}` | `{ok: true}` |
| `POST /api/fleet/maintenance` | management | `{"id", "on": bool, "minutes": n (default 30, 1–240), "reason": "..."}` | `{ok: true}`; 400 no `maintenance-v1`; 404 not connected; 409 refused by the station; 504 no answer |
| `POST /api/fleet/restart` | management | `{"id", "host": bool, "during_call": bool}` | `{ok: true}`; 400 no `restart-v1`; 404 / 409 / 504 as above |

A station in `stations`: `id`, `label`, `brew_login`, `version`, `mcc`, `mnc`,
`protocol`, `capabilities`, `online`, `first_seen_ms`, `last_seen_ms`, and
from phase 2 `check` (last update check or null), `progress` (last
`UpdateProgress` or null), `updatable`, `other_source`, `live` (online with
`update-live-v1`), from phase 3 `maintenance` (`{by, reason, since_ms,
until_ms}` or null) and `restart` (`{host, by, at_ms}` while a restarted
station is not back, or null). A log entry: `id`,
`at_ms`, `station`, `label`, `user` (null = the station or the server),
`kind`, `text`. Phase 2 adds `is_admin`, `job` (the current or last run, or
null), `update_settings`, `update_source` (in effect) and `limits` to
`GET /api/fleet`.

Code: `src/fleet.rs` (Fleet Manager, log, API), `src/control.rs` (messages),
`src/fsnet.rs` (offer on the control channel), `src/server.rs` (offer on the
Brew connection), `src/dash_users.rs` (rights), `src/dashboard_ui.rs` (page).

## 6. Backwards compatibility

| Server | Station | Result |
| --- | --- | --- |
| old (no offer) | new | the station sends no hello; switch hidden; everything as before |
| new | old | no hello arrives; the station is not in the Fleet Manager; existing commands work |
| new, `[control]` off | new | not offered; switch hidden |
| new | new, switch off | hello with `fleet_control: false`; the station is not listed |

An old server that got a hello anyway (a third-party station sending it
without the offer) logs `dropping malformed control response` and carries on;
nothing breaks.

## 7. Security

- Use TLS for the control channel (`wss://`, port 443 by default in the
  dialog).
- Passwords are never sent back to a browser; an empty password field means
  "unchanged" (or "take the Brew password" in the dialog).
- Key material (K, SCK, CCK) never travels through the configuration
  commands.

## 8. Remote update (phase 2, `update-v1`)

### 8.1 Principles

- **The station keeps its own update source.** Each station updates from its
  own OTA source (FlowStation `[dashboard] ota_channel`: by default the
  repository and branch it was installed from, forks included). The server
  only says "check" and "update"; it never chooses a repository or branch.
  Moving a station to another source is a configuration change (phase 5, by
  hand per station: "apply OTA settings from the server"), never automatic.
- **Only after a check.** A station can be updated only when its last check
  (`CheckUpdate`), made with the software it runs now, found a newer version.
  When its turn comes it fetches the newest version again, so a rolling run
  always installs what is newest at that moment.
- **The station is off the air while it builds.** FlowStation's OTA frees the
  SDR for the build (10–20 min on a Raspberry Pi), then installs and
  restarts. This is why rolling runs exist.

### 8.2 Messages

Server → station (only to stations whose hello listed `update-v1`):

```json
{"CheckUpdate": {"handle": 9}}
{"StartUpdate": {"handle": 10, "during_call": false}}
```

Station → server:

```json
{"UpdateInfo": {"handle": 9, "ok": true, "current": "v0.5.9-8d7a363", "latest": "v0.6.0-1a2b3c4", "update_available": true,
  "channel": "installed", "repo": "https://github.com/sebastianmadl/bost-flowstation", "branch": "fleet-control", "message": "newer version available"}}
{"CommandResult": {"handle": 10, "ok": true, "code": "ok", "message": "Update started"}}
{"UpdateProgress": {"stage": "build", "percent": 42, "message": "   Compiling tetra-entities v0.1.0", "version": "v0.5.9-8d7a363", "elapsed_secs": 312}}
```

| Message | Fields |
| --- | --- |
| `CheckUpdate` | `handle`. The station checks its own source, fresh (no cache); it asks GitHub, so the answer can take some seconds (the server waits up to 60 s) |
| `UpdateInfo` | `handle`, `ok` (the check reached the repository), `current`, `latest` (or null), `update_available`, `channel` (`installed` / `stable` / `beta` / `custom`), `repo` (never with credentials), `branch`, `message` |
| `StartUpdate` | `handle`, `during_call` (default false: refuse with `call_active` while a call is in progress). Answer `CommandResult`: `ok`, `busy`, `call_active`, `unavailable`, `fleet_control_off` |
| `UpdateProgress` | `stage`: `download`, `build`, `install`, `restart`, `up_to_date`, `failed`; `percent` 0–100 (estimated in `build`); `message` (last log line); `version` (running software, the old one until the restart); `elapsed_secs`. Sent on every change of stage or percentage, at least every 10 s while running, also for updates started on the station's own dashboard |

The end of an update is the next `StationHello` after the restart: other
`version` = updated.

### 8.3 Check for updates

**Check for updates** (Fleet Control page) sends `CheckUpdate` to every
online station with `update-v1` (only the ticked ones, if any) and stores each
answer with the station (`check`). The table shows "Newer: <version>", "Up to
date" or "Check failed". A check is dropped when the station comes back with
other software.

**Updates repo** (Server settings, admins): repository URL and branch, free to
choose (e.g. a fork with `handover`); empty = `https://github.com/Aitorrio/bost-flowstation`,
`main`. It is a reference only: a station whose checked source differs (other
repository, compared without case, `.git`, trailing slash or credentials, or
another branch) gets the hint **⚠ other repo/branch** with both sources in the
tooltip.

### 8.4 Update run

**Update all** (nothing ticked) or **Update (n)** (ticked stations; tick one
for a single station) opens a dialog. There is no Update button per station;
a station in a running rollout has **Cancel** to take it out of the queue.

| Field | Meaning |
| --- | --- |
| Rolling (one after another) | the next station starts only when the previous one is back with the new software (or failed / skipped); the network is never fully off the air |
| Parallel (all at once) | every station starts at once |
| Update during call | start at once, a running call is cut |
| Interrupt a call after (min) | without "Update during call": wait at most this long for a call to end (default 10, at most 120), then start anyway |

One run at a time. The run lives on the server (`fleet_update.rs`, driven
every 2 s by `fleet::run_updates`): it goes on with the browser closed and
survives a server restart (saved in `<storage path>.fleet`; a start whose
answer never came is sent again after the restart).

Per station:

| State | Next |
| --- | --- |
| up to date at the start (no newer version found) | skipped at once |
| offline (or no answer) when its turn comes | moved to the end of the queue once; offline again: skipped |
| `call_active` | tried again every 10 s; after the interrupt time with `during_call: true` |
| started (`ok`, or `busy`: an update was already running there and is followed) | follows `UpdateProgress` |
| no `restart` within **45 min** of the start (build limit) | ALARM, skipped |
| `restart` | waits for the station; not back within **5 min**: ALARM, skipped |
| back with other software | updated |
| back with the same software after the restart | ALARM, skipped |
| `up_to_date` | skipped |
| `failed`, or any other refusal (`unavailable`, `fleet_control_off`) | ALARM, skipped |

A failed station never stops the run: it is marked red in the table and in
the change log and the next station starts.

**Cancel all** (shown next to Update while a run goes) starts no further
station; one that is updating finishes. **Cancel** on a station takes it out
of the run while it has not started yet (queued or waiting for a call).

The Fleet Control page shows the run (mode, counts, the queue with each
station's state) and, while the run goes, per station the stage with a
progress bar, refreshed every 3 s. After the run the Update column is empty
again; the run card keeps the results. A bar on the run card shows the whole
run in %: every station has the same share (100 / number of stations), a
station that builds counts its build % up to 90 % of its share, a restarting
one 95 %, an updated, failed or skipped one its full share. That is the same
for rolling (one share after the other) and parallel (the average of all
stations). After a clean run (every station
updated, nothing failed or skipped, not cancelled) the card goes away 30 s
after the end; the change log keeps the story. After a failed, skipped or
cancelled run it stays until the next run.

### 8.5 Backwards compatibility

| Server | Station | Result |
| --- | --- | --- |
| phase 1 | phase 2 | the server ignores `update-v1`; no update commands are sent; `UpdateProgress` frames are dropped as unknown (logged, nothing breaks) |
| phase 2 | phase 1 | no `update-v1` in the hello: the station is never checked or updated (not counted in Update all) |
| phase 2 | phase 2, no dashboard | no `update-v1`, same as above |

### 8.6 Live update (`update-live-v1`)

**Live update (RF on)** in the update dialog (shown for stations that list
`update-live-v1`): the server sends the repository and branch of its
**Updates repo** with `StartUpdate`:

```json
{"StartUpdate": {"handle": 12, "during_call": false, "source": {"repo": "https://github.com/sebastianmadl/bost-flowstation", "branch": "fleet-control"}}}
```

The station runs its installer with them, the same as
`sudo BOST_REPO=… BOST_BRANCH=… bash contrib/install/install-bost.sh` in a
terminal, and **stays on the air while it builds**; only the service restart
at the end takes it off the air (about 2 min). Afterwards the station's own
update source is that repository and branch.

- Rolling and parallel, call wait, limits, alarms and Cancel work as in 8.4.
- No check is needed first (the station may change source); every ticked
  station (or, with nothing ticked, every online station with
  `update-live-v1`) takes part.
- Back with the same software after the restart counts as done
  ("live update finished, software unchanged"), not as an alarm.
- Only a plain `https://` repository without credentials and a plain branch
  name are sent (400 otherwise; the station checks again and answers
  `invalid_source`). An ssh Updates repo cannot be used for live updates.
- `source` is sent only to stations that list `update-live-v1`: an older
  station would ignore the field and run a normal OTA (radio off).

Code: `src/fleet_update.rs` (run state, pure), `src/fleet.rs` (checks,
settings, API, driver), `src/control.rs` (messages), `src/dashboard_ui.rs`
(page, dialog, Server settings card). Station side: FlowStation
`net_dashboard/ota_remote.rs`, `net_control/fleet.rs`, `Docs/fleet-control.md`.

## 9. Maintenance and restart (phase 3)

### 9.1 Maintenance (`maintenance-v1`)

```json
{"SetMaintenance": {"handle": 3, "on": true, "minutes": 30, "by": "oe4sxm", "reason": "new carrier"}}
```

Answer `CommandResult` (`ok`, `fleet_control_off`). While the flag is on,
every page of the station's dashboard shows a solid red banner under the top
bar: "Maintenance: work on this base station in progress", with `by`, the
minutes left and `reason`. **Maintenance** on a station (Fleet Control page,
management) asks for the duration (default 30 min, 1–240) and a note; **End
maintenance** switches it off. The flag runs out by itself on both sides, so a
closed browser never leaves it on. The server keeps it in the Fleet file and
sends it again (with the time left) when the station connects again, e.g.
after a restart. The table shows a red chip "Maintenance · user · n min left".

The config edit page (section 10) sets it automatically while it is open and
refreshes it while it stays open.

A live update (8.6) shows its own banner on the station ("Live system update:
RF on!") without this flag.

### 9.2 Restart (`restart-v1`)

```json
{"RestartStation": {"handle": 4, "host": true, "during_call": false}}
```

Answer `CommandResult`: `ok` (the station restarts its service, `host` false,
or reboots the computer, `host` true, 5 s later), `busy` (an update is
running), `call_active` (a call is in progress and `during_call` is false),
`fleet_control_off`. **Power options** on a station opens a dialog: *Restart
service* / *Reboot system* / *Shut down service*, *Restart during call*, then
a confirmation. *Shut down service*, and Restart on a station without
`restart-v1`, go as the older station commands `ShutdownService` /
`RestartService` (`POST /api/control/{id}`, no call protection, no change log
entry); the station does not come back from a shut down by itself. The
change log records who sent it and, from the next `StationHello`, "back after
the reboot (n s)"; a station not back within **10 min** gets an ALARM
(`restart_alarm`). The station needs no sudo rule: FlowStation's service runs
as root.

### 9.3 RF kill switch (`rf-standby-v1`)

```json
{"SetRfStandby": {"handle": 6, "standby": true, "during_call": false}}
```

Uses the station's own **Suspend** (FlowStation dashboard, power menu): the
radio stack stops and the transmitter goes off, while the process, its
dashboard and the control channel stay up. `standby` false starts the stack
again (a service restart: the SDR can only be reopened by a fresh process).
Answer `CommandResult`: `ok` (off 2 s later, or the service starts again; also
when the station already is in the asked state), `busy` (an update is
running), `call_active` (switching off while a call is in progress and
`during_call` is false), `fleet_control_off`.

The station reports the state as `rf_standby` (true / false) in every
`StationHello` and sends a new hello when it changes, also after a Suspend on
its own dashboard; its `FleetMetrics` report `rf: "standby"` for every cell
meanwhile, so no SDR alarm is raised. Because the control channel stays up,
the station is not "offline": the Fleet Manager shows **Standby · RF off** in
the status column, and the map draws its marker with the Stand-by status
(half yellow dot top left, marker not grey) and "standby (RF off)" in the
popup, also for users without Fleet Control rights. When the control channel
drops as well, the station is offline as usual.

Dashboard: a red **RF off** button next to *Power options* (Fleet Control
management, station online, `rf-standby-v1`) opens a dialog with *Also during
a call* and *Switch RF off*; a station in standby has a **RF on** button
instead (confirmation, then the service starts). API `POST /api/fleet/rf`
`{"id", "standby", "during_call"}`. The change log (kind `rf`) records who
switched it and, from the hellos, "RF off (standby)" / "RF on again".

### 9.4 Backwards compatibility

A station without `maintenance-v1` / `restart-v1` / `rf-standby-v1` gets no
buttons and the API refuses with 400; a phase 2 server never sends the new
commands. An older server ignores `rf_standby` in the hello (unknown fields
are skipped); an older station leaves it out, which the server reads as not
in standby.

## 10. Station config (phases 4/5, `config-v1`)

The server reads each station's config, keeps a copy, and changes it with a
safety net: the station writes the new config, restarts and must be back on
this server (and on Brew, if it has a Brew login) within **90 s** (Server
settings → Config changes, 60 to 900 s), else it
restores the old config by itself. Keys (`[security]`, TEA/SCK) are never
read or written; passwords are never sent to the server.

### 10.1 Messages

The hello carries the config state (absent from older stations):

```json
{"StationHello": {"...": "...", "config_hash": "211602a72878a9a4", "config_pending": 170}}
```

`config_hash`: md5 (16 hex) of the parsed config, re-serialized, without
`[command] fleet_control` (comments and layout do not count).
`config_pending`: seconds left of a change waiting for confirmation, absent
when none waits. The station sends a new hello when either changes (checked
every 5 s), so an edit on the station shows up on the server.

```json
{"GetConfig": {"handle": 5}}
{"SetConfig": {"handle": 6, "base_hash": "211602a72878a9a4", "changes": {"cells": [{"id": 0, "location_area": 2}]}, "confirm_secs": 90, "during_call": false, "by": "admin"}}
{"ConfirmConfig": {"handle": 7}}
{"RollbackConfig": {"handle": 8}}
```

- `GetConfig` → `ConfigInfo {handle, ok, code, message, hash, config, text,
  pending}`: `config` is the station's Infrastructure view as JSON (cells, LA,
  colour code, networks / Multi Network, neighbours per cell, reselection,
  rxlev, migration, presence check, Brew link, telemetry, command channel, OTA
  source) without passwords (`password_set` only); `text` is the config file
  with passwords and keys replaced by `***`.
- `SetConfig` → `CommandResult`: `ok` (written, restart in 3 s), `conflict`
  (the config is not `base_hash` any more), `invalid` (the result does not
  give a valid config, unknown keys are refused), `call_active`, `busy` (an
  update runs or a change already waits), `fleet_control_off`. `changes`
  holds only the changed sections; an empty password means "unchanged". The
  station validates the whole config like its own dashboard does, saves the
  old file as `<config>.fleet-backup` and notes the change in
  `<config>.fleet-pending.json` before it writes the new one.
- A station that announces `config-live-v1` (FlowStation live reload) applies
  a change it can take without a restart at once and answers `SetConfig` with
  `ok: true` and code `applied_live`; the window then runs from that moment
  and the station sends its hello with the new hash and `config_pending` on
  the same connection, so the server confirms as after a restart (nothing
  else changes on the server; older servers work the same way). A change that
  needs a restart is answered `ok` as before.
- `ConfirmConfig` → `CommandResult`: `ok`, `not_pending`, `not_restarted`
  (the process that wrote the change has not restarted yet: the hello it
  re-announces before the restart already says "pending"; the server waits
  for the hello after the restart).
- `RollbackConfig` → `CommandResult`: `ok` (old config back, restart),
  `not_pending`.
- `raw` in `changes`: `[{path: [key | index, ..], value?, remove?}]`, single
  values of the config file, applied last on the station. The server builds
  them from its config editor (it compares the edited text with the copy's
  text, so comments and layout do not count) and from the Home Mode Display
  and ISSI whitelist cards. Paths through a password or key name and hidden
  values (`***`) are refused on both sides.

The window runs from the restart (`written + 3 s + confirm_secs`, at least
20 s after the boot). A watchdog armed at boot restores the backup and
restarts when it runs out, so a change that cuts the station off (wrong
server, login or network) undoes itself.

### 10.2 Server behaviour

- **Copy.** At the first hello with `config-v1` and whenever the reported hash
  differs from the copy, the server reads the config. The copy is kept in the
  Fleet file and shown when the station is offline.
- **Difference.** A station whose config differs from the copy (edited on the
  station) gets the chip "Config differs from server copy" and the alarm
  `config_alarm` in the change log. Its edit page shows a dialog: **Import base
  station config to server** (the station's config becomes the copy) or
  **Apply server config to base station** (the station goes back to the copy,
  passwords stay as they are on the station). Nothing is editable before that.
- **Apply.** An online station gets `SetConfig` at once; when it is back with
  `config_pending` and its Brew client (by Brew login) is connected, the
  server sends `ConfirmConfig` (retried every 10 s). **Undo** in the running
  card sends `RollbackConfig`. A station back on its old hash means a
  rollback: ALARM in the change log (or "undone" after Undo).
- **Pending (offline).** For an offline station the change is kept as
  pending (one per station) and sent at its next login, once no call runs and
  only if the station's config is still `base_hash`; otherwise it is marked
  as conflict and not sent.
- **Rights.** Fleet Control management changes every section, the command
  channel (the channel Fleet Control itself works over) included; a wrong
  value there is caught by the rollback. Fleet Control visibility gets no
  edit page. Clearing the change log stays for admins.
- **Maintenance.** While the edit page is open it sends a heartbeat every
  minute; the station is in maintenance for 3 min per heartbeat ("editing
  config"), off again when the page is left. Maintenance set by hand stays.
- **Log.** Each change is logged with who, what (one line per setting, old →
  new, passwords only "changed") and the outcome.

### 10.3 Dashboard

Fleet Control → **Edit** (or **Resolve…** when the config differs) opens one
page per station with a back arrow: Brew link, cells (carrier, colour code,
LA, network; add / remove), Multi Network, cell reselection, neighbours per
cell (manual or picked from the server's cell directory), telemetry, command
channel, Home Mode Display, ISSI whitelist, update source and the config
file (editable; passwords and keys hidden, only changed values are sent).
Networks with their own Brew server (home included) show their whole link. A network no cell uses
is marked *unused* and can be saved when the station announces `networks-unused-v1` (it keeps the
network inactive); for other stations it blocks Apply (*no cell*), as they refuse such a config. Changed fields are marked; **Apply to station** (online) or
**Save as pending** (offline) shows the list of changes first.

### 10.4 HTTP API (Fleet Control management)

| Route | Body | Does |
| --- | --- | --- |
| `GET /api/fleet/config?id=` | | station, copy, difference, pending, running change (reads fresh when online) |
| `POST /api/fleet/config/apply` | `{id, base_hash, changes, summary, during_call, resolve, raw_text?}` | send or keep as pending |
| `POST /api/fleet/config/discard` | `{id}` | drop the pending change |
| `POST /api/fleet/config/import` | `{id}` | the station's config becomes the copy |
| `POST /api/fleet/config/rollback` | `{id}` | Undo a running change |
| `POST /api/fleet/config/raw_preview` | `{id, text}` | lines the edited config file would change |
| `POST /api/fleet/edit` | `{id, open}` | edit page heartbeat / closed |

`GET /api/fleet` gives each station a small `config` summary (`apply`,
`drift`, `pending`, `conflict`, `copy`).

### 10.5 Backwards compatibility

New options need both a FlowStation and a server update: the station
validates every key and refuses unknown ones. A station without `config-v1`
gets no Edit button; an older server ignores the new hello fields and never
sends the new commands.

## 11. Templates, bulk changes and neighbour suggestions (phase 6)

Server side only; it uses the `config-v1` commands of section 10, so every
station with `config-v1` works without a FlowStation update.

### 11.1 Confirmation window

The window a station has to come back after a change (section 10) is a server
setting: **Server settings → Config changes (Fleet Control)**, default 90 s,
60 to 900 s (admin). It is sent as `confirm_secs` with every `SetConfig`,
single or bulk.

### 11.2 Templates

A template holds only settings stations share: Brew server (host, port, TLS,
MCC/MNC), networks (Multi Network), cell reselection, RXLEV_ACCESS_MIN,
migration, presence check, Home Mode Display, ISSI whitelist, telemetry and
update source. Never part of it: carrier, colour code, LA, cells, neighbours,
logins and passwords; each station keeps its own (the server fills in the
station's username, site name and position where a section needs them).

- Each section has its own switch; only switched-on sections are applied.
- Created empty (**New template**) or from a station (**Save as template** on
  its edit page); editable afterwards. At most 50.
- Applied once: the station gets the values, nothing stays bound to the
  template, a later template edit changes no station.
- Per station the server builds the changes against its config copy; a
  section is left out with a note when it cannot apply (for example the Brew
  section on a station that uses Multi Network, or telemetry on a station
  without a telemetry login). Home Mode Display and the whitelist go as `raw`
  values.

### 11.3 Bulk config change

Fleet Control → tick stations → **Change config…** → a template, settings
for this run only (**Edit settings for this run…**) or the missing reciprocal
neighbours → preview → **Start**. One bulk run at a time.

- Preview per station: the changes (old → new), "kept as pending" or the
  reason it is skipped (nothing to change, a pending change already, a change
  running, config differs from the server copy, no `config-v1`).
- **Rolling** (default): one station after another; the next starts when the
  previous one is confirmed or rolled back. A rollback is an ALARM in the run
  and in the change log, and the run goes on. **Parallel**: all at once.
- Calls as with the remote update: wait for the call to end, or cut it after
  *n* minutes, or change during the call.
- Offline stations get the change as pending (section 10.2); it is sent at
  the next login.
- **Cancel** starts no further station; a station that is restarting
  finishes with its own rollback window.
- The run survives a server restart (states sent or waiting are re-checked
  from the stations' last outcome).

### 11.4 Neighbour suggestions and check

From the cell directory (section 5, `cell-directory`), per cell of a station:

- **lists this cell**: a cell that broadcasts this cell as neighbour without
  being listed back (both should list each other, TS 100 392-2 clause
  18.3.4.1);
- **radios moved**: cells radios changed to or from, counted from the roaming
  log over the last 30 days (kept in the Fleet file);
- **distance**: cells within a radius (default 30 km) of the station's
  position (static station positions, else the station's telemetry position).

Suggestions are never applied by themselves. **Add** fills the entry from what
the neighbour cell broadcasts (carrier, LA, services, rxlev, power) and it can
be edited like any other; at most 7 neighbours per cell, siblings included.
Listed neighbours that are not in the directory any more are marked
"outdated", ones whose LA differs from what the cell broadcasts get "LA
differs" with a button to take it over.

**Neighbour check** on the Fleet Control page lists, for the whole fleet, the
cells not listed back and the marked entries; **Add missing reciprocal** runs
them as a bulk change (11.3).

### 11.5 Rights

Fleet Control visibility sees templates, the bulk run and the neighbour check;
Fleet Control management creates, edits and deletes templates and starts or
cancels bulk changes. The confirmation window is an admin setting.

### 11.6 HTTP API

| Route | Right | Body / query | Does |
| --- | --- | --- | --- |
| `GET /api/fleet/templates` | visibility | | templates |
| `POST /api/fleet/templates/save` | management | `{id?, name, note, sections}` | create or replace |
| `POST /api/fleet/templates/delete` | management | `{id}` | delete |
| `POST /api/fleet/bulk/preview` | management | `{ids, template? \| sections? \| reciprocal}` | per station changes or skip reason |
| `POST /api/fleet/bulk/start` | management | preview body + `{mode: rolling\|parallel, during_call, interrupt_min}` | start the run |
| `POST /api/fleet/bulk/cancel` | management | | cancel the run |
| `GET /api/fleet/neighbours?id=&radius_km=` | visibility | | suggestions and marks per cell |
| `GET /api/fleet/neighbours/check` | visibility | | fleet-wide check |
| `POST /api/fleet/config/settings` | admin | `{confirm_secs}` | confirmation window |

`GET /api/fleet` also gives `templates`, `bulk` (the last run) and
`confirm_secs`.

## 12. Graphs, ping and alarms (phase 7, `metrics-v1`)

### 12.1 Metrics from the station

A station with `metrics-v1` sends, without being asked, 5 s after the connect
(or after its switch went on) and then every 60 s:

```json
{"FleetMetrics": {"interval_secs": 60, "cpu_pct": 17.4, "cpu_temp_c": 52.1,
  "cells": [{"cell": 0, "rf": "online", "sdr_temp_c": 44.3},
            {"cell": 1, "rf": "error", "sdr_temp_c": null, "detail": "SoapySDR: no device found"}],
  "radios": 3, "calls": 1, "health": "ok"}}
```

`cpu_pct`: CPU load, mean since the previous report (%). `cpu_temp_c`: CPU
temperature (°C) or null. Per cell: `rf` `online` / `off` / `error` /
`starting`, `detail` (error text), `sdr_temp_c` (°C) or null. `radios`:
registered radios, `calls`: calls in progress, `health`: stack health `ok` /
`degraded` / `critical`. No answer. The server takes it only from listed
stations (switch on).

A station without `metrics-v1` (older FlowStation) is sampled once a minute
from its telemetry, if it has a telemetry login (matched by control login or
Brew login): CPU temperature (the temperature sensor whose name has `cpu`,
`soc`, `core`, `package` or `k10temp`, else the first one), SDR temperatures,
registered radios, calls, health. Its CPU load stays empty.

### 12.2 Graphs

The server folds the reports into one point per **sample interval** (Server
settings, 1–60 min, default 1): CPU load as the mean, temperatures and counts
as the highest value. Points are kept for **Keep graphs** (1–720 h, default
24 h), then overwritten (ring buffer); at most 10 080 points per station, so
more than 7 days need a longer interval. While a station sends nothing
(unreachable, switched off) no point is written: its graph shows a gap there
(the page breaks a line where two points are more than 1.5 intervals apart).

**Ping**: every **ping interval** (Server settings, 0–60 min, default 5, 0 =
off) the server sends `FleetPing` (section 3.3) to every online station with
`fleet-v1` and stores the round trip (server ↔ station, ms) or "no answer"
(5 s). **Auto ping** on the Telemetry page (management) asks
`POST /api/fleet/ping` for every online station every 5 s while the page is
open; these are shown at the station only, not stored and not logged.

Optional lines (Server settings): registered radios, calls in progress.

The Telemetry page shows, for every Fleet Control station (matched to its
telemetry login by control login or Brew login, as the sampling does), the
latest CPU load, temperatures and ping, its active alarms and **Graphs →**; a
Fleet Control station without telemetry gets a card of its own. The graph page
(**Graphs →**, visibility; its back button returns to Telemetry) shows CPU load with the
alarm limit, CPU and SDR temperatures (one line per cell) with their limits,
ping (unanswered pings as red marks), and the optional lines, for 1 h, 6 h,
24 h, 3 d, 7 d or the whole keep time; it reloads every 30 s.

### 12.3 Alarms

| Rule | Condition | Default | Ends when |
| --- | --- | --- | --- |
| Station unreachable | control channel down for *minutes* (counted from the server start at the earliest) | ALARM, 2 min | back online |
| SDR error | a cell's `rf` is `error`, or `health` is `critical`, for *minutes* | ALARM, 0 | all cells ok and health not critical |
| CPU temperature | ≥ *limit* for *minutes* | ALARM, 80 °C, 0 | below limit − 2 °C |
| SDR temperature | any cell ≥ *limit* for *minutes* (one alarm per cell) | ALARM, 70 °C, 0 | below limit − 2 °C |
| CPU load | ≥ *limit* for *minutes*; not while an OTA update runs (its last `UpdateProgress` is a running step not older than 30 min, or `restart` / `failed` / `up_to_date` not older than 5 min: the build takes the CPU), the minutes count from its end | warning, 90 %, 5 min | below limit − 5 % |
| Missed pings | *limit* server pings in a row unanswered while connected | warning, 3 | a ping is answered |
| Change log ALARMs | an `update_alarm`, `restart_alarm` or `config_alarm` entry | ALARM | acknowledged |

Every rule can be switched off, set to ALARM (red) or warning (yellow), and
given its limit and minutes in Server settings → **Graphs and alarms**
(admins). Switching a rule off finishes its active alarms ("rule
switched off by …"). Metric rules are only checked with a report not older
than 3 min. A station that leaves Fleet Control finishes its alarms and drops
its graph. On the very first start the change log ALARMs already in the log
are not raised.

An alarm is active until it ends; **Acknowledge** (management, one or all)
marks it with the user and time; a change log ALARM ends by being
acknowledged. Finished alarms are kept 90 days as history. The **Telemetry**
page shows the active condition alarms (ALARM first, newest first) and the
last finished ones, the **Fleet Control** page the change log ALARMs the same
way (*Change log alarms*); each page's sidebar badge counts its unacknowledged
ones (red with an ALARM). **Acknowledge all** takes only that page's alarms
(`{"events": false}` / `{"events": true}`).

Files next to the history log: `<storage>.fleet-alarms` (settings, alarms;
written on every change) and `<storage>.fleet-metrics` (graphs; written every
5 min, so a restart loses at most 5 min of points).

### 12.4 HTTP API

| Route | Right | Body / query | Does |
| --- | --- | --- | --- |
| `GET /api/fleet/graph?id=&hours=` | visibility | | points `{t, c, ct, s: {cell: °C}, r, k}` and pings `{t, ms}` from `hours` back (default: whole keep time), settings, limits, active alarms of the station |
| `POST /api/fleet/alarms/ack` | management | `{id}`, `{}` (all), `{"events": false}` (all condition alarms) or `{"events": true}` (all change log ALARMs) | acknowledge |
| `POST /api/fleet/monitor/settings` | admin | `{sample_min, keep_hours, ping_min, show_radios, show_calls, rules: {offline, sdr_error, cpu_temp, sdr_temp, cpu_load, ping, events: {enabled, severity: "alarm"\|"warning", threshold, minutes}}}` | settings; 400 with the reason when a value is out of range |

`GET /api/fleet` also gives `alarms: {active, history, settings, latest}`
(`latest`: last report per station with `cpu_pct`, `cpu_temp_c`, `sdr`,
`radios`, `calls`, `ping_ms`).

### 12.5 Backwards compatibility

`FleetMetrics` goes only to a server that offered `fleet-control` and only
while the switch is on, so older servers never see it. A server older than
phase 7 would drop it as an unknown response (warning in its log). A station
without `metrics-v1` keeps working; its graphs come from telemetry, or stay
empty. Code: `src/fleet_monitor.rs`.
