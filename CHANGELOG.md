# Changelog

All notable changes to brew-server, newest first.

Version 1.22.14 (RSSI per radio) adds:

- **RSSI per radio.** The Radios page has an RSSI column and the map popup
  shows it: the uplink level (dBFS at the station's SDR, not dBm) the station
  the radio is registered on now last measured, with its age. From
  `SERVICE_RSSI` (FlowStation `[brew] feature_rssi_export`) or the station's
  telemetry link; a reading of a station the radio left is never shown, even
  when that station still reports it. Plain Brew (TetraPack) has no RSSI.

Version 1.22.13 (App as a Basestation) adds:

- **Group access for Basestation logins without cells.** The empty
  `SERVICE_GROUP_ACCESS` request is also taken from a Basestation login (the
  app in Basestation mode), which then gets the verdicts for its own ISSIs.
  Stations that announce `group-access` in `SERVICE_BTS_INFO` still get the
  whole table. Spec: `docs/Brew-Extensions.md` 12.3.
- **DGNA to a station without telemetry.** When the telemetry does not name a
  radio's station, a DGNA from the DGNA & Radio actions page goes to the
  control connection whose login matches the Brew login the radio is
  registered through. See `docs/Routing.md`.

Version 1.22.12 (Group access for apps) adds:

- **Group access for terminal apps.** Terminal-mode logins are offered
  `group-access`. An app that asks with an empty `SERVICE_GROUP_ACCESS` gets
  the access table for its own ISSI (other ISSIs left out of the lists), now
  and on every change, and can grey out groups it may not join or only listen
  to. Apps that do not ask work as before. Spec: `docs/Brew-Extensions.md` 12.3.

Version 1.22.11 (Multi Network for apps) adds:

- **Multi Network for terminal apps.** While Multi Network is on, a
  Terminal-mode login is offered `mni` too. An app that names its home
  network with `SERVICE_MS_NETWORK` is handled like a radio on a station with
  the extension: an app of another network is a visitor here and registered
  at home, an app of this network can listen and talk in groups of a linked
  network. Apps that do not use it work as before.
  Spec: `docs/Brew-Extensions.md` 12b.6.

Version 1.22.10 (Config changes applied live) adds:

- **Fleet Control: config changes applied live.** A station with
  `config-live-v1` (FlowStation live configuration) takes a change it can
  apply without a restart at once and answers `applied_live`; the change log
  now says "applied live without a restart" instead of "station restarts".
  The confirmation window and the automatic rollback work as before. Older
  stations restart as before.

Version 1.22.9 (RF kill switch) adds:

- **Fleet Control: RF kill switch.** A red **RF off** button next to *Power
  options* switches a station's transmitter off with its own Suspend: the
  radio stack stops, the dashboard and Fleet Control stay reachable. The
  station is shown as **Standby · RF off** on the Fleet Control page and with
  the Stand-by marker on the map (no longer as offline); **RF on** starts it
  again. Also a Suspend on the station's own dashboard shows as Standby. Needs
  FlowStation v0.6.7 with `rf-standby-v1`; older stations get no button.

Version 1.22.6 (Networks without a cell) adds:

- **Fleet Control editor: networks without a cell.** A station that announces
  `networks-unused-v1` (FlowStation keeps such a network inactive, no Brew
  link, until a cell selects it) can now save a network no cell uses; the
  editor marks it *unused*. Stations without it still need a cell for every
  network before Apply, as they would refuse the config.

Version 1.22.5 (Logo colours) adds:

- **Dashboard colours from the logo.** The accent colour (buttons, links,
  the active menu item, highlights) is the logo blue instead of teal:
  #1f4fe0 with the darker #1236a8 in light mode, #4d86ff with the lighter
  #8fb0ff in dark mode (a darker blue is hard to read there). Status colours
  (green ok, red alarm) stay as they were.
- **The favicon shows before signing in.** `/favicon.svg` needed a login, so
  on a server with dashboard users the tab icon and the mug in the sign-in
  window stayed empty until signed in. It now loads without a login.
- **Add to home screen.** The dashboard has a web app manifest and app icons
  (the favicon on a dark background, 192, 512 and 180 px for iOS), so a phone
  can add it as an app named "brew-server" (renamable when adding). The manifest and
  icons load without a login.

Version 1.22.4 (brew-server logo) adds:

- **The brew-server logo.** The dashboard sidebar starts with the logo (the
  mug with the honeycomb, its steam the radio waves, and the "brew server"
  wordmark), in its light and dark colours with the dashboard theme; the
  server name and version follow under it. The sign-in window shows the mug,
  and the browser tab shows it as favicon (`/favicon.svg`). The README starts
  with the logo too. Logo by Sebastian OE4SXM; the files are in
  `assets/logo/`.

Version 1.22.3 (Map: Clear map resets everything) adds:

- **"Clear map" resets the whole map.** The button on Settings → Map (was
  "Clear all tracks") only forgot the tracks: every radio that ever sent a
  position stayed at its last one. It now also forgets every radio's last
  position, so the map fills again from what comes in. Stations stay;
  registered radios sit next to their station until they send a position.

Version 1.22.2 (Map: tracks from station reports, visitors under their ISSI) adds:

- **Tracks also for radios whose position the station decodes itself.** A
  LIP that the FlowStation decodes and keeps local only reaches the server
  as a telemetry report ("LIP position: ..."). The map showed it, but the
  track only took positions from the Brew channel, so such a radio had no
  track while an app had one. Telemetry positions now go into the track too;
  one report that arrives both ways is kept once.
- **A visitor is drawn under its home ISSI.** When a station reports a
  visitor's (V)ASSI with its home ISSI (`MsVisitor`), the map shows the
  position, track, station and state under the home ISSI, with "As visitor:
  VASSI n" in the popup, instead of a second marker for the (V)ASSI.

Version 1.22.1 (Map: terminals and offline radios) adds:

- **A terminal stays a terminal on the map after it leaves.** An app often
  registers only to send its position and deregisters seconds later; its
  position was then drawn as a radio. Each stored position now notes
  whether it came from a terminal, and a number not registered here is drawn
  as it was when it sent the position shown.
- **Radios registered nowhere are drawn offline** on the map (grey with a
  dashed ring, "Offline (not registered)" in the popup), at their last
  position.
- **Dashboard loads again.** A missing brace after the map settings card
  (since 1.21.8) stopped the whole dashboard script.

Version 1.22.0 (Parrot, DGNA list, Map rework) rolls up 1.21.4 to 1.21.9.
No functional changes since 1.21.9. In short:

- **Parrot** to test a radio (1.21.8): [docs/Parrot.md](docs/Parrot.md).
- **Assigned by DGNA** list, with Remove and Forget, kept across a restart
  (1.21.9): [docs/Routing.md](docs/Routing.md).
- **Map rework** (1.21.7), with terminals drawn as terminals (1.21.9):
  [docs/Basestation-Locations.md](docs/Basestation-Locations.md).
- **Login type** Auto, Terminal or Bridge, and linked networks that speak
  Brew like a FlowStation (1.21.4, 1.21.5):
  [docs/Multi-Network.md](docs/Multi-Network.md).

Version 1.21.9 (DGNA list) adds:

- **The server remembers the groups given by DGNA**, so they can be taken
  back later: card **Assigned by DGNA** on the DGNA & Radio actions page
  with ISSI, group, station, where it was given (Server with the dashboard
  user, or Station for the station's own dashboard) and when; **Remove**
  sends the DGNA remove to the station the radio is on now, **Forget** drops
  the entry here only. Kept across a restart (`<storage path>.dgna`).
  Stations report each assign and remove over the control channel
  (extension `dgna-report`, offered with `[control] enabled = true`;
  FlowStation v0.5.19 or newer), also those made on the station's own
  dashboard, and send what they know after each connect. Details:
  [docs/Routing.md](docs/Routing.md).
- **Map: terminals are drawn as terminals.** A number registered over a
  Terminal connection (app, console) gets the terminal marker instead of the
  radio, and "(Terminal)" in its popup. Radios behind a login set to
  *Bridge* stay radios; a number set to *Terminal* is a terminal.

Version 1.21.8 (Parrot) adds:

- **Parrot to test a radio** (Settings → Parrot, `[parrot]`, off by
  default, applied live). An individual call to the parrot ISSI is answered
  by the server: in a simplex call each over is played back to the caller
  after the delay (set in milliseconds, `delay_ms`, default 1000); in a
  duplex call everything comes back after the delay (an echo) and the call
  ends after the maximum length. An over on the parrot group is played back
  on the group as the parrot ISSI, so every radio in the group hears it. The
  group has the same number as the ISSI unless *Separate GSSI* is set
  (`separate_gssi`, `gssi`; 0 = no group). Overs are recorded for at most
  `max_secs` (default 60). The floor stays with the parrot for `tail_ms`
  (default 500) after the last played frame, so the station plays out its
  buffer before the end. Saving adds the group to the allowed talkgroups
  and names both numbers "Parrot" in the Directory (an existing name stays).
  Works from every station, nothing new needed on the stations; only calls
  from this server's own connections are answered. Details:
  [docs/Parrot.md](docs/Parrot.md).

Version 1.21.7 (Map rework) adds:

- **New map markers (design set v2).** Stations show their type (TMO
  station, tower TBS, terminal) and status (connected, Fleet Control warning
  or alarm; grey and dashed when offline).
- **Map symbol per login** on the Accounts page (`[bts_locations] icon`:
  `tbs`, `terminal`). A login connected as Terminal is drawn as a terminal
  by default.
- **Radios without a recent position are docked next to their station**, in
  rings at a fixed pixel distance (more than 22: "+N"), and undock when they
  send a position. They dock again after `[map] dock_after_min` (default 30)
  or when they register on another station.
- **Track per radio**: the last `[map] track_points` positions (default 30)
  are kept across a restart (`<storage>.positions`) and shown on a click.
  Settings → Map also has *Guests see tracks* (`[map] guests_see_track`,
  default on): off, visitors who are not signed in get no tracks.
- **The first map view covers the stations** with a static position, also
  before any radio sent one.
Version 1.21.5 (Login type: Auto, Terminal or Bridge) adds:

- **Type per login: Auto, Terminal or Bridge.** The login's dialog on the
  Accounts page, *Change* in the new Type column of Stations & Cells and the
  ISSI dialog of the Directory set how the dashboard lists a login: *Auto*
  (default, as before: Basestation mode = station, Terminal mode = terminal),
  *Terminal* or *Bridge*. A login that connects in Basestation mode but is a
  bridge (e.g. 9100232) can now be listed under Terminals & bridges; it and
  its radios leave the station lists (Stations, header count, map). Stored in
  `[issi_kinds]` (`"terminal"` is stored now too, Auto removes the entry;
  `POST /api/issi-kinds/set` takes `auto`, `terminal`, `bridge`). Dashboard
  only, the server handles the connection as before.

Version 1.21.4 (Linked networks: Brew like a FlowStation) adds:

- **Linked talkgroups and pools: groups of other networks on demand.** A new
  switch *Only on demand* (`on_demand`, per pool or linked talkgroup, default
  off = permanent): the bridge ISSI joins the group of the other network only
  while a radio here is in one of the pool's own groups, and leaves it when
  the last one has left, like a dynamic talkgroup. Permanent groups behave as
  before (their calls always come). The list of linked talkgroups marks them
  "on demand".
- **Multi Network: the link sends registrations like a FlowStation.** Every
  subscriber message to another network now carries the time (seconds and
  nanoseconds since 1970), as a FlowStation sends it; it was 0 (1970). The
  refresh sends a re-registration (REREGISTER) for radios already registered
  there, as a FlowStation does when one of its radios registers again, instead
  of a second REGISTER. Field 2026-11-15..17: TetraPack stopped sending TG 91's
  calls after a while although the bridge ISSI stayed registered. With this
  change (refresh 300 s, rejoin off) TG 91's calls came all night
  (2026-11-17/18, about 2100 calls).
- **Multi Network: registration refresh set per network.** Edit network has
  a *Registration refresh* box: *Every … seconds* (`refresh_seconds`,
  default 300, 0 = off, at least 30) and the switch *Also rejoin the groups*
  (`refresh_rejoin`, default off, greyed out at 0): each refresh then lets one ISSI
  (the bridge ISSI, else the radios in turn) leave its groups there and join
  them again, only while no call crosses the link; all others stay in. Field
  2026-11-16/17: TetraPack stopped sending TG 91's calls while the bridge
  stayed affiliated; leaving and joining the group again brought them back.

Version 1.21.3 (UI Cleanup) adds:

- **Telemetry shows how the stations are doing.** For stations in Fleet
  Control each telemetry card now has **Graphs →** (the graph page, its back
  button returns to Telemetry), the latest CPU load, CPU and SDR temperatures
  and ping, the station's active alarms as chips, and a **Ping** button. The
  condition alarms (unreachable, SDR error, temperature, CPU load, missed
  pings) with their history and the **Auto ping** switch moved here from Fleet
  Control, and the Telemetry menu entry carries their badge. A Fleet Control
  station without a telemetry connection gets a small card of its own. All of
  this needs Fleet Control visibility (acknowledging and ping: management);
  others see the cards as before.
- **Fleet Control is for managing.** Graphs, latest values, Ping and Auto ping
  are gone from its station list. It keeps the change log ALARMs (update,
  restart, config) as **Change log alarms**, with their own badge.
  "Acknowledge all" on either page takes only that page's alarms
  (`POST /api/fleet/alarms/ack` `{"events": true|false}`).
- **Power options on Fleet Control.** The station's Restart button is now
  **Power options**: one dialog with *Restart service*, *Reboot system* and
  the new *Shut down service*; stations without `restart-v1` get the button
  too (restart sent as the older station command).
- **Control is now DGNA & Radio actions.** The page takes the DGNA card from
  Routing and has one **Radio actions** card with a station picker (kick
  radio, clear emergency, live SDS, ambience listening) instead of one card
  per station. Two new rights in Server settings → Users: *DGNA & Radio
  actions: view* and *DGNA & Radio actions: send* (includes view, never for
  Viewer); before, Control and DGNA were for admins only. Stations outside
  Fleet Control keep Restart / Shut down on this page. `#control` links open
  the new page.
- **Server settings: "Graphs and alarms"** (was "Fleet Control graphs and
  alarms"), linking to Telemetry.
- **Telemetry SDS is part of the SDS page.** Its menu entry is gone: the SDS
  page shows Recent SDS and the stations' Telemetry SDS side by side on wide
  screens (one above the other on narrow ones). Each list scrolls on its own,
  the search filters both, Telemetry SDS keeps its Kind filter, and **⤢**
  opens a list on the whole screen with all columns (it follows the live
  refresh). The Telemetry SDS right stays in the users dialog and shows the
  second list; old `#tsds` links open the SDS page.
- `/api/control` (the list of control connections) is no longer open to
  everyone: it needs DGNA & Radio actions or Fleet Control visibility.
- **Radios: the cell (global ID) no longer gets lost.** Deregistrations,
  timeouts and "moved to another station" entries in the registration log
  keep the cell the radio was last seen in, a guest's registration gets the
  cell its station reports for the guest's home ISSI, and **Registered
  radios** falls back to the cell reported on another connection of the same
  station, or to the station's only cell.
- **Registration log kept for `[storage] keep_days` (7 days by default).**
  The Radios page's log no longer holds only the last 50 entries per station
  in memory: every entry (stations and Brew channel) goes to its own file
  `<storage path>.registrations` (JSON lines; the history store is not
  touched, so an older server can still read it), survives a restart and is
  trimmed with the hourly retention. The dashboard gets the newest 2000 and
  shows 1000 in a scrolling list. Admins get **Clear**
  (`POST /api/history/clear` `{"registrations": true}`).
- **Terminals and bridges apart from radios.** The Radios page lists radios
  (on a base station, or of another network) in **Registered radios** and
  numbers logged in as Terminal in **Terminals & bridges**, with their type.
  The Directory dialog of an ISSI sets the type for when it logs in as
  Terminal: *Terminal* (app, console; the default) or *Bridge*
  (`[issi_kinds]`, `GET /api/issi-kinds`, `POST /api/issi-kinds/set`
  `{"issi","kind"}`). Stations & Cells shows the type in its
  **Terminals & bridges** card, and the cell column says "— (bridge)" or
  "— (terminal)".
- **SMS Center: "n radios known" opens the list.** A popup shows the known
  radios with their names (radio directory or callsign list), with a filter.
  `GET /api/sms-center` now also returns `known` (the ISSIs).

Version 1.21.2 adds:

- **Multi Network: a restarted link no longer loses its registrations.**
  Field 2026-11-15: saving a network restarted the TetraPack link, the new
  link registered the radios while the old one was still deregistering them,
  and TetraPack sent nothing more on TG 91 until the radio changed groups.
  Now the new link connects only after the old one has closed (at most 5 s),
  and every link sends its registrations and affiliations there again every
  5 minutes, since Brew has no answer that would show one was lost.
- **Multi Network dialogs: full keyboard on phones.** The fields of "Edit
  network", "New network" and "This server" no longer ask for the numeric
  keypad, which has no comma for the ISSI lists.
- **Dashboard: Radios in the phone tab bar instead of Routing.** Logged in and
  logged out; Routing stays in the More menu.
- **Multi Network: a radio changing between cells of two networks keeps only
  the new cell's groups.** Field 2026-11-15: the MTP went from the 1638 cell
  back to the 9999 cell of the same station and was listed in 91 and in
  TetraPack:91, because a registration on another connection took over the old
  groups, the linked network's group (an alias) included, and the old cell can
  no longer leave it. Now groups of a linked network stay with the connection
  that attached them, and between cells of different networks no group comes
  along (the new cell attaches its own right after registering).
- **Registration log: Multi Network visitors under their home ISSI.** A
  station's telemetry names a visitor by its (V)ASSI (e.g. 15728641); with the
  station's `MsVisitor` event the log now shows the radio's own ISSI, as the
  rest of the server does.

- **Dashboard: Stations & Cells page split.** Stations (Basestation mode, with
  their cells) on top, then Terminals (apps and consoles, no cells), then the
  individual cells. Other connection modes get their own "Network links" card.

- **Dashboard: network names without a login.** Radios' groups of another
  network (Multi Network) showed as `net 2:91` on an open dashboard because
  the names came only from the admin route `/api/networks`. The new public
  route `GET /api/network-names` gives slot and name only (no hosts or
  logins), so they read `<network>:91` for everyone.
- **Fleet Control, phase 7: graphs, ping and alarms.** Stations with
  `metrics-v1` report CPU load, CPU and SDR temperatures, RF state, radios and
  calls every minute (older stations: temperatures from telemetry). Each
  station gets a **Graphs** page (CPU load, temperatures, ping, optionally
  radios and calls) with a gap while it was unreachable; one point per
  sample interval, kept 24 h by default and then overwritten. The server pings
  every online station every 5 min for the ping graph; **Auto ping** on the
  Fleet Control page pings every 5 s while the page is open (not stored). An
  **Alarms** list on the Fleet Control page (unreachable, SDR error, CPU / SDR
  temperature, CPU load, missed pings, and the ALARMs of the change log) with
  **Acknowledge**, a 90-day history and a badge in the sidebar. Server settings
  → **Fleet Control graphs and alarms**: sample interval, keep time, ping
  interval, optional lines, and per alarm on/off, ALARM or warning, limit and
  minutes. The CPU load warning stays off while a station updates (the build
  takes the CPU) and for 5 min after. See `docs/Fleet-Control.md` section 12.
- **Fleet Control, phase 6: templates, bulk changes, neighbour suggestions.**
  Templates hold the settings stations share (Brew server, networks, cell
  reselection, RXLEV_ACCESS_MIN, migration, presence check, Home Mode Display,
  ISSI whitelist, telemetry, update source; never carrier, colour code, LA,
  cells, neighbours or logins), created empty or from a station, applied once.
  **Change config…** on the ticked stations applies a template or one-off
  settings with a preview, rolling (default) or parallel like the remote
  update; a rollback is an ALARM and the run goes on, offline stations get it
  as pending, stations with a pending change or a differing config are
  skipped. The edit page suggests neighbour cells (cells that list this cell,
  cells radios moved to in the last 30 days, cells close by) and marks
  outdated neighbours; the Fleet Control page has a fleet-wide neighbour check
  with **Add missing reciprocal**. The confirmation window for config changes
  is now 90 s and a server setting (60 to 900 s). The change log table scrolls
  after about 10 rows. See docs/Fleet-Control.md section 11.

- **Neighbour tables always show MCC/MNC.** On the fleet edit page the
  neighbour list, the station's own cells and the suggestions show the MCC/MNC
  of every entry (the cell's own network when the entry has none) instead of
  "same".

- **Phones: change log scroll.** The scrollable change log (and every table
  that scrolls) keeps its position on a live update instead of jumping back to
  its start, a scroll inside it pauses live updates like a page scroll, and
  it no longer holds the page scroll at its ends.

- **Fleet config page: order and "Apply server config".** Cards in the order
  Brew link, networks, cells, neighbour cells, cell reselection, Home Mode
  Display, ISSI whitelist, telemetry, control channel, update source. In the
  difference dialog **Apply server config to base station** is now open
  whenever the station is online, also when only settings the page does not
  show differ: the server sends the station's file back to the copy value by
  value (passwords and keys stay as they are on the station).

- **Fleet Control, phases 4/5 (station config).** Fleet Control → **Edit**
  opens one page per station (back arrow) with its Infrastructure settings:
  Brew link, cells (carrier, colour code, LA, network, add / remove), Multi
  Network, cell reselection, neighbours per cell (manual or from the server's
  cell list), telemetry, command channel, update source and the
  config file (read only, passwords and keys removed). **Apply to station**
  writes the change; the station restarts and must be back on this server
  (and on Brew) within 180 s, else it restores its old config by itself;
  **Undo** in between. Offline stations get the change as pending, applied at
  their next login. The server keeps a copy of each station's config; a
  station edited locally shows "Config differs from server copy" and asks to
  import its config or apply the server's. The station is in maintenance
  while the page is open. The config file at the bottom is an editor: only the values that
  changed go to the station (passwords and keys stay hidden). Home Mode
  Display, ISSI whitelist, the home network's whole Brew link and the MCC /
  MNC of neighbours are editable. Multi Network follows the station's own page (the classic network
  becomes "home", every network but home can be removed, a network without
  a cell is caught before Apply). Fleet Control management may change all of it (visibility gets no edit
  page). Needs FlowStation with `config-v1`. See docs/Fleet-Control.md
  section 10.
- **Fleet Control: progress bar for the whole run.** The "Update run" card
  shows the progress of the whole run in %: each station counts the same,
  with its build progress while it builds and its full share once it is
  done (rolling and parallel alike).
- **Fleet Control: the run card goes away after a clean run.** 30 s after a
  run in which every station updated, the "Update run" card is hidden (the
  change log keeps the story). It stays while the run goes and after a run
  with a failed or skipped station, or one that was cancelled.
- **Fleet Control, phase 3 (maintenance and restart).** Per station:
  **Maintenance** puts a solid red banner "work on this base station in
  progress" (with user, minutes left, note) on every page of the station's
  dashboard for 1–240 min; it runs out by itself and is set again when the
  station reconnects. **Restart** restarts the service or reboots the system
  (with confirmation, optional during a call); the change log shows when the
  station is back, and an ALARM if it is not back after 10 min. Needs
  FlowStation with `maintenance-v1` / `restart-v1`. See docs/Fleet-Control.md
  section 9.
- **Fleet Control: live update (RF on) and Clear.** The update dialog has a
  new switch **Live update (RF on)**: the stations install the repository
  and branch of the **Updates repo** (Server settings) like the install
  command in a terminal, stay on the air while they build and are off only
  for the service restart; no check needed, rolling and parallel as before.
  Needs FlowStation with `update-live-v1`. The change log has a **Clear**
  button for admins (one entry "Log cleared by <user>" stays). See
  docs/Fleet-Control.md sections 5.2 and 8.6.
- **Fleet Control, phase 2 (remote update).** On the Fleet Control page,
  **Check for updates** lets every station (or the ticked ones) check its own
  update source; the table shows "Newer: <version>" / "Up to date". **Update
  all** (or **Update** for ticked stations) asks for
  **Rolling** (one after another: the next starts when the previous one is
  back with the new software, so the network is never fully off the air) or
  **Parallel** (all at once), **Update during call**, and how long to wait
  for a call to end (default 10 min). The run goes on on the server with the
  browser closed and after a server restart. Each station shows its stage
  (download, build %, installing, restarting) live. Limits: 45 min to the
  restart, 5 min back after it; a failed station is marked ALARM (red, also
  in the change log) and skipped, the run goes on. A station offline at its
  turn moves to the end once, then it is skipped. **Cancel all** starts no
  further station, **Cancel** takes one station out. Stations always update
  from their own source; new **Updates repo** card (Server settings) sets the
  reference repository and branch, and a station with another source gets
  the hint "⚠ other repo/branch". Needs FlowStation with Fleet Control phase 2
  (`update-v1`). See docs/Fleet-Control.md section 8.
- **Fleet Control, phase 1 (foundation).** Basestations that switch on
  "Fleet Control" (FlowStation, Infrastructure page) can be managed from this
  server over the control channel. Offered automatically while `[control]` is
  on (`X-Brew-Extensions: fleet-control` on the control channel and the Brew
  connection). New page **Fleet Control**: the stations with their software,
  network, features and online / offline state (offline stations stay
  listed), Ping (round trip) and Remove, and a change log kept 90 days
  (switched on / off, new software, removed by whom). A station that switches
  Fleet Control off leaves the list (at once while online, else at its next
  login) with the INFO "Basestation XYZ switched off Fleet Control". New
  rights "Fleet Control visibility" and "Fleet Control management" for
  dashboard users. Stations and log survive restarts in
  `<storage path>.fleet`. Existing control commands do not change. See
  docs/Fleet-Control.md.
- **Multi Network: radios registered there, listed.** On the Multi Network
  page, "N radios registered there" opens a list under the network: ISSI with
  its Directory name, the station / cell here, the groups there and since
  when. `/api/networks` has the same as `exported_detail`. Each registration
  and deregistration there is logged (`multi network: ISSI … registered in
  …`). Brew does not confirm a registration, so this is what this server
  sent; whether the other network took it shows only on its side.
- **Dashboard users** (Server settings -> Dashboard users): any number of
  admins and users, each user with the pages they see; pages without rights
  are left out of the menu. Viewer (not logged in) has its own page ticks,
  which decide what an open dashboard shows. Saving and editing need an admin
  (the login window opens with a note otherwise). Station logins stay visible,
  IP addresses are for admins only. Everyone changes their own password in the
  menu at the top right; admins set other users' passwords. Stored as Argon2
  hashes in `[dashboard] users_file`; the config users are the start of a new
  installation, and deleting the file gets them back when locked out. Accounts
  has an **Authentication required** switch (`[auth] enabled`, live, asks
  before switching off). See docs/Dashboard-Users.md.
- **Dashboard: SIP / VoIP, SIP Config, SMS Center, Telemetry SDS, High
  Availability and Server settings are native pages** in the look of the
  rest (cards, labelled fields, light and dark, phone width) instead of
  frames of the classic pages. SIP extensions, trunks and voice routes are
  added, edited and deleted on SIP Config (admin; read only for others);
  Server settings keeps the ISSI block and allow lists and the static
  station positions. High Availability: settings in a dialog. The classic
  dashboard (`/classic`) is unchanged; only the map is still a frame.
- **Kind of each SDS** from its protocol identifier (ETSI TS 100 392-2
  table 29.21): Text, Position, WAP / data, Multi-part, Security, System,
  App or Other, on Overview, SDS and Telemetry SDS, with a Kind filter
  (hover for the value). Saved with the networks in `<storage path>.nets`;
  `/api/status` has `protocol_id` on SDS entries.
- **Forward LIP per network** (Multi Network, Edit network): off, position
  reports (protocol identifier 0x03, 0x0A, 0x83) do not cross that link in
  either direction; text and other SDS still pass. On by default
  (`forward_lip`). The dialog now says that "Forward unknown targets"
  bypasses the SMS Center.
- **Phones:** a table that scrolls sideways no longer widens the whole page.
- **Fix: talkgroup pools sharing a group.** A call is named by the pool
  that really carries it, not by the first pool its group is listed in: with
  232 in three pools, a call on 2327 shows "TG Pool" (its pool), a call on
  232 itself (heard on all three) "TGP1 + TG Pool + North", on Overview,
  Calls and the Routing page; each pool card shows only its own legs.
- **Fix: networks of calls and SDS survive a restart.** The "→ / ← network"
  of Recent calls and Recent SDS is saved in `<storage path>.nets` next to
  the history file (unchanged, still readable by older versions); before,
  every older entry lost it on a restart.
- **Fix: live updates on phones.** After the first touch the dashboard no
  longer stopped fetching calls, SDS and emergencies (until a reload); the
  scroll handling cleared the live refresh timer by mistake (since 1.21.0).

Version 1.21.1 adds:

- **Do not forward SDS and calls to other networks.** New `never_forward` list
  per network (dashboard: Infrastructure, Edit network, under "Forward
  unknown targets"): no individual call or SDS to or from a listed ISSI
  crosses that network's link, in either direction. A call is refused, an
  SDS goes nowhere (a position in it is still tracked). For an ISSI that
  only this server takes, e.g. a telemetry ISSI that positions are sent to,
  which "forward unknown targets" would otherwise hand to that network.
  Group calls are not affected.

Version 1.21.0 adds:

- **History stays small.** `[storage] keep_days = 7`, `max_calls = 2000`,
  `max_sds = 2000` (0 = no limit): older entries and those over the limit
  are dropped from the history log at start and once an hour (the log is
  rewritten, its format is unchanged). The call / SDS totals keep counting
  them (`<path>.totals`). Admins can clear all calls or all SDS on the Calls
  / SDS pages; the totals stay. Calls and SDS pages: "Show 25 / 50 / 100 /
  500 / All (7 days)". Overview shows the call total under "No active
  calls". HA: a Standby whose copy position no longer fits the rewritten log
  starts over and skips what it has.
- **Ham callsign database** (Directory, Radios tab, folded card, off by
  default): radios without a Directory name are named by their call sign
  from a list of `ID;CALLSIGN` lines, by default radioid.net `dmrid.dat`.
  Directory names always come first. "Now" fetches the list, "Automatic"
  every day at 00:00 UTC; a new list replaces the old one (one cache file,
  nothing else stored). Config `[callsign_db] enabled`, `url`, `auto`,
  `cache`; API `GET /api/callsigns?issi=..`, `GET /api/callsign-db`.
- **Map: names.** The popup of a radio shows its call sign / name from the
  ISSI directory (or the callsign database), e.g. "ISSI 2320972 (OE4SXM)".
- **Phone fixes.** Live updates wait while the page is scrolled (iOS left
  blank areas), fields use 16 px text so iOS no longer zooms in on them,
  double-tap zoom is off and the page does not zoom (`maximum-scale=1`). On
  phones the page itself scrolls now (before, a scrolling area inside a
  fixed-height page: the header could move away and parts stayed blank), so
  the header stays at the top.
- **Sidebar:** the group "Network" is called "Infrastructure".
- **Dashboard clean-up:** Accounts and the registration log on Radios load
  when the page opens (before, up to 5 s of "Loading…"); on phones the page
  titles fit, search fields take the full width and the "?" buttons have a
  larger touch area; a cell without LA or MCC/MNC shows "—"; the pages
  "Configuration" and "Settings" are called "Config file" and "Server
  settings"; the map card is called "Radio positions".
- **Overview: names instead of numbers.** Live calls, recent calls and
  recent SDS show the name or call sign of a radio alone when there is one
  (the ISSI is in the tooltip and on the Calls / SDS pages); a group with a
  name in the Directory likewise (GSSI in the tooltip). A station with a
  name (Accounts, info) shows it after its number: "1000002 (Name)".
- **DGNA: saved talkgroups.** A "Saved TGs…" dropdown next to the GSSI
  field fills in GSSI and name from the groups the server knows.
- **Managed talkgroups.** A card on the Routing page (folds to a thin bar:
  folded while off, open while on) closes the set of talkgroups: only
  allowed groups may be used, i.e. the card's list, the groups of
  talkgroup pools and linked talkgroups, and groups with the access "Only
  these" (limited to their ISSIs anyway). Each group shows its access badges
  ("Only N ISSIs", "N blocked", "RX only"); Edit / Rules opens the Directory
  dialog (name, access, RX only) right on the Routing page. Groups are
  allowed by GSSI with an optional name (written to the Directory) or from the foldable "Saved TGs
  on server" (groups the server knows, including refused ones). Affiliations
  to any other group are not recorded and its group calls are dropped
  (emergency calls pass); radios already on such a group are dropped from
  it. Basestations with `group-access` get `"managed"` and `"listed"` in
  `SERVICE_GROUP_ACCESS` and refuse the attachment on the air (FlowStation
  0.5.6 from this change on); older ones ignore the fields. Off (the
  default) nothing changes and the list is kept. Config `[talkgroups]
  managed`, `allowed`.
- **Directory: hide groups.** A group without a name that radios are on can
  be hidden from the list with ✕ (`[talkgroups] hidden`, display only); the
  filter "Hidden" shows it again. Groups of other networks (`network:GSSI`)
  are no longer listed there as raw numbers.
- **Directory: names for bridge logins.** The Radios (ISSI) list takes
  numbers up to 12 digits, so the Brew login of a bridge (Network mode, such
  as 232099709) can be named; the name shows under the login on Stations &
  Cells. Terminals and base stations are named on the Accounts page. Groups
  stay 24-bit GSSIs.
- **Overview in three pairs.** Live calls | recent calls, stations | radios,
  cell changes | recent SDS, side by side in equal columns; on a phone they
  stack in that order. Recent calls (last 5, pool legs merged) and radios
  (first 6, with cell and group count) are new on the Overview.

Version 1.20.3 adds:

- **Federation peers: `path` sent as written.** The trailing slash of a
  `[[federation.peers]]` path is no longer cut off, so a core that serves
  discovery at `/brew/` and redirects `/brew` to its website (TetraPack) can
  be peered with `path = "/brew/"`; a brew-server answers on both. Discovery
  errors quote the start of the peer's reply. The idea comes from PR #55 by
  Sebastian OE4SXM and David Harcus, MM7FDM, brought onto the current version (that PR is based on
  an older one; redirects are already handled since 1.20.0).

Version 1.20.2 adds:

- **Linked talkgroups on the Routing page.** A simple list of 1:1 links
  "this server · TG ⇄ linked network · TG": one group of this server, both
  ways, to one group of another network of the Multi Network page. Each link
  has its own editor (one GSSI here, the network from the list, one GSSI
  there), an on/off switch and its live status. A link is a talkgroup pool
  with exactly these two groups, saved as `[[talkgroup_pools]]` with
  `linked = true`; the server refuses a link with more groups or one way.
  Links are not listed again under Talkgroup pools. The name is optional;
  without one the list shows only the groups.

- **Stations and cells named the same on every page.** A station is named by
  its Brew login, a cell by its global cell ID `login/cell` (the Brew login
  and the cell number inside the station, 0 = primary). Radios, the
  registration log and DGNA show station and cell in separate columns;
  Brew connections list their cells by global ID; Telemetry shows the local
  cell number with the global ID in brackets; "BTS" columns are now
  "Station". The help ("?") explains that the global cell ID is not an ETSI
  identity, that apps and other terminals have no cells, and that a station
  without `bts-info` is counted but its cells are unknown.
  A station with several Brew connections (one per network its cells
  serve) is listed once under Brew connections, with all its cells, its
  remote addresses and the radios summed; the sidebar counts stations, not
  connections.
  The Overview adds up the radios of a station that has one connection per
  network.

Version 1.20.1 adds:

- **Pool calls in one row.** Recent calls show the legs of a talkgroup pool
  call (same talker, groups of one pool, started within 3 s) as one row with
  the pool's name and its groups, instead of one row per group. Live calls
  still show every group.
  This also works logged out (the network list is admin only), and a bridge
  whose pool is not known joins its legs by GSSI.

Version 1.20.0 adds:

- **Timeslots on the Telemetry page again.** The new dashboard's station cards
  list the station's calls (group or individual, from, to, cell, carrier and
  timeslot, priority) and draw the four timeslots of every carrier in use, busy
  or available, as the classic Basestation Telemetry panel did.
- **One configuration editor.** The Settings page of the new dashboard no
  longer shows the classic raw TOML editor; the configuration is edited on the
  Configuration page. The classic dashboard's Settings page keeps it.
- **Multi Network: bridges to other Brew networks.** Links, bridge pools, individual (duplex) calls and SDS across networks, visitors (radios of other networks on a station, `mni`) and radios of this network migrated onto a cell of a linked network are tested on air (FlowStation, Motorola MTP6650, TetraPack, 2026-11-07). `[multi_network]` lists other networks
  this server connects to itself, each over one outbound connection in plain
  Basestation mode (nothing new needed on the far server). The connection name
  (`user_agent`) is free text, e.g. `TMO-Austria Infra 1.0`. Credentials stay
  on this server. Talkgroup pools can hold groups of those networks
  (`[[talkgroup_pools.remote]]`, `network:GSSI` in the editor): a call is then
  heard across networks with the GSSI translated (TG 9 here, TG 909 there) and
  the talker ISSI passed on 1:1. A bridge ISSI is registered there and
  affiliated to the bridged groups; radios in a bridged group are registered there and affiliated to the group there while they are in it here, like a Basestation does (`register_talkers`, default on). "Register all" (`register_all`) exports every radio of stations with the `mni` capability or listed in `export_stations`. Before registering, a brew-server there is asked whether the ISSI is free (new extension `issi-check`, `SERVICE_ISSI_CHECK` 0x24); for other servers there is a `never_export` list, and a radio there talking with an exported ISSI is reported ("ISSI conflicts" on the Multi Network page). Radios of other networks on a station (visitors, new extension `mni`, `SERVICE_NETWORKS` 0x20 to `SERVICE_CALL_NETWORK` 0x23) are registered in their home network, kept apart from this server's radios, and take part in their home network's group calls. A visitor that then names this server's network (it migrated here, e.g. after its radio was switched from ITSI attach to migration) is deregistered from the network it was registered at, so it is not reachable there and here at once. This server's own group calls are not broadcast (`route_without_affiliations`, the no-affiliation fallback) to a station whose cells all broadcast another network's MNI; it gets a group only where a radio of this network that migrated there is in it, so a group call of a station or terminal without the extension stays in this network. A group call from here that the other network ends at once (TetraPack answers some overs with GROUP_IDLE) keeps its call identifiers until its own end has gone over, so the voice and the end still reach that network and the call no longer hangs there. A radio of this network that migrated onto a cell of a linked network and attaches groups there without an MNI is in that network's groups (TS 100 392-2 clause 14.2.3, the station says so with `SERVICE_GROUP_NETWORK`): it is registered and affiliated there and hears that network's calls on them, instead of being affiliated to this server's group of the same number. An individual call or SDS whose party is here (a visitor that "forward unknown targets" would have sent to its network, or one of this server's radios registered in the visitor's network) is delivered here instead of round the link, where the far server refused it ("party not available"). `export_stations` can also name an app (Terminal) by its login or its ISSI, so "register all" registers it there and radios of that network reach it outside bridged groups. Individual calls and SDS cross networks: to a TSI of another network (the station names it), to and from visitors, and from another network to radios registered there; optional `forward_unknown` per network for targets nobody here takes (loop guard 5 s). Answers in a running call cross as well; call identifiers are translated on each link. Nothing else crosses. New dashboard page
  **Multi Network** (admins): main switch, this server's network and MNI, the other
  networks with link state, edited live without a restart. See
  [docs/Multi-Network.md](docs/Multi-Network.md).
- **Destination network on the dashboard.** Calls and SDS that leave this
  server over a Multi Network link show the network they went to ("→ TP"),
  and those that came in over one the network they came from ("← TP"): live
  calls, Recent calls and Recent SDS, with a Network column when there is
  one to show. In memory only (the history file format is unchanged).
- **Open dashboard fixed.** With `[dashboard.ui] open = true` a visitor who is not
  signed in got the login window every 5 s again (the new page asked the admin-only
  network list on every refresh). It is now asked only for admins.

- **Server name.** `[dashboard] servername` (default `"Tetra Network Monitor"`)
  sets the main dashboard heading and the browser tab title of every
  dashboard page. It belongs in the `[dashboard]` section:

  ```toml
  [dashboard]
  servername = "Tetra Network Monitor"
  ```

- **Carrier registry.** A Basestation that is offered the `bts-info`
  extension (`X-Brew-Extensions` header on the WebSocket upgrade) announces
  its cells (MCC, MNC, carrier, band plan, location area, capabilities) with
  the new `SERVICE_BTS_INFO` message. The server keeps a table of which
  Basestation serves which carrier; duplicate carriers are logged and marked
  on the dashboard (the newest announcement serves).
- **Cells on the Connections page.** Cells are named `<login>/<cell>`; a
  "Cells" panel lists carrier, downlink frequency, location area, MCC/MNC,
  neighbours, capabilities, address and ISSIs per cell. A station with a login
  that went away stays listed as offline until it connects again.
- **Cell directory.** `SERVICE_BTS_INFO` may carry what each cell broadcasts
  (maximum MS power, minimum access level, BS service details, neighbour
  list). With the new `cell-directory` extension the server sends a
  Basestation that asks for it the cells of all other Basestations, online
  and offline, with those values (`SERVICE_CELL_DIRECTORY`, 0x1D), so it can
  name its neighbours, check that both sides list each other and pick a
  neighbour from the list (FlowStation: Roaming page).
- **Handover target by downlink.** When no cell has the exact carrier key of
  a handover request, the target is the cell on the same downlink (network,
  carrier, band, offset): duplex spacing and reverse operation only set the
  uplink, and a neighbour listed without an extension carries the serving
  cell's band plan.
- **Announced handover relay.** Basestations with the `handover-v1`
  capability can prepare an announced cell change on the target Basestation
  through the server (`SERVICE_HO_PREPARE`, `SERVICE_HO_FORWARD_REG`,
  `SERVICE_HO_RESULT`). The target is found by carrier and, when given, its
  location area; when there is none the serving cell is told why (unknown,
  gone, own carrier, timeout). The roaming kick skips radios with a handover
  in flight. Timeout:

  ```toml
  [handover]
  timeout_ms = 2000
  ```

- **Cell of each radio.** With the `ms-cell` extension a Basestation reports
  the cell a radio registered on (`SERVICE_MS_CELL`). Shown in the subscriber
  tables (Connections page and main dashboard, `/api/ms-cells`) and in the
  registration log.
- **Cell Change Log.** The Connections page lists every cell change of a
  radio with from / to cell and how (announced, not announced, within
  station); with the `ms-reselection` extension the Basestation the radio
  left reports the reselection type (undeclared, unannounced, announced
  type 1 / 2 / 3, `SERVICE_MS_RESELECTION`). Kept in memory, 200 entries.
- **Unannounced cell change.** When a radio registers on another Basestation
  without a prepared handover, the Basestation it left gets
  `SERVICE_MS_MOVED` if it asked for it (`ms-moved` capability and
  extension), and an over the radio was talking in there ends, so it can talk
  on from the new Basestation (which must speak `handover-v1`). See
  [docs/Handover.md](docs/Handover.md).
- **Individual calls follow the radio.** When a radio in an individual call
  with a Brew party registers on another Basestation with the `call-restore`
  capability, the call moves there; the old Basestation gets `CALL_RELEASE`
  (cause 14) and lets its leg go, the new one asks for the call when the
  radio restores it (`SERVICE_CALL_RESTORE`). Not restored within 8 s: the
  other party gets `CALL_RELEASE` (cause 19) -- at once when the new
  Basestation reports a plain roaming registration
  (`SERVICE_CALL_NOT_RESTORED`) or the radio sets up a new call. See
  [docs/Handover.md](docs/Handover.md).
- **Calls between two radios survive a station change.** With the new
  `call-handoff` extension (`SERVICE_CALL_HANDOFF`, 0x1E) a local call
  between two radios of one Basestation is handed to the network when one
  radio restores it on another Basestation, and a call between radios on two
  Basestations becomes local again when one radio comes to the other's
  Basestation. See [docs/Handover.md](docs/Handover.md).
- **Redesigned dashboard.** A new dashboard in the style of the FlowStation
  one: sidebar with Monitor, Network, Integrations and System, light and dark
  following the system, a tab bar on phones, status shown with text and not
  by colour alone, help behind "?" buttons. Overview, Calls, SDS, Radios,
  Stations & Cells, Routing, Roaming, Telemetry, Control, Accounts,
  Configuration and HA are native pages; Map, SIP, SMS Center, Telemetry SDS and Settings open the classic
  pages inside it. It is the default. `[dashboard] legacy_ui = true` serves
  the classic dashboard at `/` instead; both stay reachable under `/classic`
  and `/ui`.

  ```toml
  [dashboard]
  legacy_ui = false
  ```

- **DGNA to several radios.** The Routing page assigns a group (with an
  optional name of up to 15 characters) to many radios at once, or takes it
  away again, each through the control link of the Basestation the radio is
  registered on (`POST /api/dgna`, admins). The radios' answers come back
  over telemetry and show per radio (`GET /api/radios`).
- **Talkgroup pools.** `[[talkgroup_pools]]` connects talkgroups inside the
  server: a call on one group is played 1:1 on the others, voice and over end
  included, both ways or only from the first group. A group that already has
  a call is joined when that over ends. Pools are switched on and off,
  edited, added and deleted on the Routing page and apply without a restart
  (`GET /api/pools`, `POST /api/pools/save`). See
  [docs/Routing.md](docs/Routing.md).
- **Login window.** The new dashboard signs in with its own login window
  (Log in button top right, account menu with Log out) and a session cookie
  (`POST /api/login`, `POST /api/logout`; sessions end after 12 hours idle
  and on a restart). A closed dashboard shows only the login window until
  someone signs in. HTTP Basic auth keeps working everywhere, so the classic
  pages and scripts are unchanged.
- **Accounts page.** Adds, changes and deletes the logins Basestations and
  terminals connect with, on Brew (`[auth.users]`), telemetry
  (`[telemetry.users]`) and control (`[control.users]`) at once or each on
  its own. Applies to the next connection without a restart and is written
  to the config file (`GET`/`POST /api/accounts`,
  `DELETE /api/accounts/{username}`, admins).
- **Configuration page.** The whole config file in an editor with a section
  jump. Save checks the file (a mistake is shown with its line and nothing is
  saved) and applies it with the usual restart.
- **Info and position per login.** The Accounts page keeps an info text per
  login (sysop, relay callsign, site) and an optional position, stored as the
  login's `[bts_locations]` entry (`name`, `lat`, `lon`; an entry without a
  position keeps `0` / `0` and is not plotted). A Login / Info switch shows
  the info in place of the number; Telemetry and Stations show it next to
  the login (`GET /api/names`).
- **Directory of ISSI and GSSI names.** Names for radios (`[issi_names]`,
  ISSI = name) and for groups (`[gssi_names]`, GSSI = name), shown next to
  the number on the dashboard pages. The new Directory page (Radios / Groups)
  adds, changes and removes names, imports a pasted list (`ISSI,name` per
  line, `;`, tab or space work too) and exports CSV. Applied without a
  restart and written to the config file (`GET /api/issi-names`,
  `GET /api/gssi-names`, admins: `POST /api/issi-names/set`,
  `POST /api/issi-names/import` and the same under `/api/gssi-names/`).
- **Who may use a group.** Per group an allow list (only the listed ISSIs)
  or a block list (everyone but the listed ISSIs); no list = open. The
  server records an affiliation only for an admitted radio and drops a group
  call from any other (emergency calls always pass), so it neither talks on
  the group across the network nor pulls the group's calls to its station;
  pool copies skip groups that do not admit the talker. Set on the Directory
  page (Groups, Access: Open / Only these / Block these), applied without a
  restart (affiliations the group no longer admits are dropped) and written
  to the config file:

  RX only (switch plus an optional list) makes radios listeners: every radio
  when the list is empty, otherwise only the listed ones. A listener may
  attach and hear the group but gets no floor.

  ```toml
  [group_access."2321"]
  mode = "allow"            # or "block"
  issis = [2320972, 2321100]
  rx_only = true            # optional
  rx_only_issis = [2321100] # optional, empty = everyone listens only
  ```

  With the new `group-access` extension a Basestation that asks for it gets
  the lists (`SERVICE_GROUP_ACCESS`, 0x1F, after its announcement and on
  every change) and refuses the attachment and the request to talk on the
  air itself (FlowStation). A Basestation without it still lets the radio
  attach and talk locally: base Brew has no message to refuse that.
- **Software version.** The Stations page shows the version a Basestation or
  terminal sends in its User-Agent (FlowStation sends its release).
- **Appearance.** System, Light or Dark in the account menu (the sun button
  when signed out); the classic pages shown inside the dashboard follow it.
- **Admin pages open the login window.** Settings, Configuration and the
  other admin pages open the new login window instead of the browser's login
  prompt.
- **Phone layout.** The map no longer scrolls by itself, Telemetry and other
  pages are more compact, the pool groups field opens the full keyboard, the
  tab bar sits lower, the menu opened from More lies above it, and the
  classic dashboard link is in the account menu.
- **HA page.** Changes applied live (blacklist, pools, logins, ISSI and GSSI names, group access) no longer show
  as pending changes.
- **Backward compatible.** Without `[[talkgroup_pools]]` call routing is
  unchanged. All new messages are sent only to Basestations
  that were offered the matching extension; TetraPack, Brandmeister and older
  Basestations see no change. The messages are specified for other
  implementations in [docs/Brew-Extensions.md](docs/Brew-Extensions.md).

Version 1.18.1 adds:

- **ISSI allowlist.** A new `[allowlist]` section and a Settings panel for
  running a closed network: with `enabled = true` only the listed ISSIs may
  communicate through the server, and any other radio is barred exactly as a
  blacklisted one is (no group transmissions, private calls or SDS; SS-AL, LIP
  positions and emergency calls still pass, and it can still register so
  dispatch can see it). Checked at the server for the whole network, whichever
  Basestation the radio registers on; the blacklist still applies on top. The
  list is kept but not enforced with `enabled = false` (the default), so it can
  be built up before the network is closed; an enforced empty list is called
  out at startup and on the Settings page. Edits apply live and are written to
  the config file without a restart. Optional `kick_unknown` also kicks an
  unlisted radio off its Basestation over the control channel. Log lines for a
  dropped transmission now name the list that barred it. See
  [docs/ISSI-Allowlist.md](docs/ISSI-Allowlist.md). Contributed by David
  Harcus MM7FDM.

Version 1.18.0 adds:

- **Late entry for group calls.** A Basestation that affiliates to a group
  (or registers an ISSI that keeps the group) in the middle of an over now
  receives the running call's `GROUP_TX` and its voice from then on, instead
  of hearing nothing until the next over. Sent in the connection's own layout
  (v0/v1) before any voice frame; federation peer links, the talker's own
  connection and connections already in the call are skipped. This also
  covers an announced cell reselection. See [docs/Late-Entry.md](docs/Late-Entry.md).
- **Optional roaming kick.** With `[roaming] kick_old_issi = 1`, a new
  registration drops the ISSI from every other Basestation's list and kicks
  it there over the control channel after `kick_delay_ms`. Off by default.
  See [docs/Roaming.md](docs/Roaming.md).
- **Choppy voice fixed.** `TCP_NODELAY` is set on Brew connections (plain and
  TLS) and outgoing federation links, so Nagle no longer bunches the 60 ms
  voice frames.
- **HA heartbeat ping-pong fixed.** A node sends a heartbeat only on its
  tick or a state change, not in answer to every received one.

Version 1.17.0 adds:

- **Dead Basestation and Terminal connections are dropped.** Only federation
  peer links used to be pinged and timed out, so a Basestation or Terminal
  whose socket died without a close (NAT or proxy expiry, power loss) stayed
  on the Live Connections page for days with no registrations. Every
  connection is now pinged and closed after `keepalive_timeout_seconds` of
  silence (`[federation]` keepalive settings, default 15 s / 45 s; interval 0
  turns it off for all links). Clients must answer WebSocket pings, which
  standard stacks do automatically.

Version 1.16.2 adds:

- **Example configs in `sample/`.** The example `brew-server.toml` moved to
  `sample/brew-server.toml`; the Dockerfile, docker-compose and README point at
  it. Run with your own copy, or `brew-server sample/brew-server.toml`.
- **A 0/0 position is never plotted.** A no-fix report -- including the
  `LIP position: 0.000000, 0.000000` text a FlowStation sends -- no longer
  puts a radio on the map at Null Island (parser, store and map page).
- **Emergency alarms relayed across federation.** An emergency alarm now
  crosses loop-safe federation links (`FED_EMERGENCY`, negotiated with the
  position hello, version 3) and stays on every server -- ribbon, red map
  marker and dispatch consoles -- until it is cleared at its origin, instead
  of vanishing from a federated server when the call ends. Refreshed every
  10 s, dropped after 30 s without one.
- **Block ISSIs from Tetra Dispatch.** A console can block and unblock ISSIs
  over its Brew link (service messages `0x12` / `0x13`). brew-server obeys
  only a console whose Brew username is in the new `[blacklist] console_users`
  (empty by default = view only), applies it live, saves the config without a
  restart, and pushes the list to every console. Needs Tetra Dispatch 1.2.0.
- **Red marker for a radio in an emergency.** On the MS Map a radio with an
  active emergency (alarm or emergency call) turns red, pulses and its popup
  says EMERGENCY; the map note names it too.
- **Emergency list pushed to dispatch consoles.** Every second brew-server
  checks the active emergencies (Basestation alarms plus live emergency
  calls, relayed ones included) and pushes the list to every connected Tetra
  Dispatch console as a `CLASS_SERVICE` message (type `0x11`) whenever it
  changes, and every 5 s while any is active. A console shows the red ribbon
  and marker even for an alarm with no call running. Needs Tetra Dispatch 1.2.0.
- **Emergency calls reach dispatch consoles and every server.** An emergency
  group call is now also pushed to every federation peer, even one that does
  not route the group, so it reaches all brew-servers (loop-safe and plain
  links alike; a ring or mesh prunes the duplicate copies), and to every
  connected Tetra Dispatch console (recognised by its
  `TetraDispatch/` User-Agent), whatever groups it listens to, with its voice
  and end. A call is an emergency when it is at priority 15 *or* its radio has
  an emergency alarm active on a Basestation: FlowStation forwards its radios'
  calls at priority 0, so the alarm is what marks them. For consoles and
  federation peers such a call is forwarded with priority 15, and one already
  running when the alarm arrives is upgraded mid-call. Ordinary calls still
  follow affiliations. Needs Tetra Dispatch 1.2.0.
- **Red emergency ribbon.** Like FlowStation's: a sticky red "EMERGENCY ACTIVE"
  ribbon at the top of the dashboard lists each active emergency -- a
  Basestation emergency alarm (with the called group once its radio is in a
  call), or a live priority-15 call seen on the Brew channel -- and tags a
  blacklisted ISSI.
- **ISSI blacklist.** A new `[blacklist] issis = [...]` list and a Settings
  panel bar an ISSI from communicating through the server: its group
  transmissions are dropped, private calls from or to it are rejected, SDS
  from or to it is dropped and SIP calls to it are refused. Dispatch
  ambience listening (SS-AL) to it and LIP position traffic in both
  directions still work, and it can still register. Edits in Settings apply
  immediately with no process restart. Emergency calls (priority 15, or from
  an ISSI with an active emergency alarm) are never blocked. See the ISSI-Blacklist wiki page
  (`docs/ISSI-Blacklist.md`).

Version 1.16.0 adds:

- **Basestation positions over telemetry, relayed between servers.** A
  Basestation (Bost FlowStation) with `latitude`/`longitude` (and optional
  `site_name`) in its `[telemetry]` section reports a `SiteLocation`
  telemetry event; an unset or 0/0 position is never sent. The `/map` page
  plots it, and loop-safe federation relays it to other brew-servers
  (`FED_BTS`, flooded with path-vector loop protection) -- only to peers
  that announced support (`FED_BTS_HELLO`), never to older or legacy
  links. Relayed positions expire after 2 minutes without a refresh. The
  static `[bts_locations]` table stays as a fallback for any Basestation
  that reports no position. The Basestation re-sends its position every
  minute in case the server missed it. Relaying needs `loop_safe = true` on
  both servers; see the Basestation-Locations wiki page
  (`docs/Basestation-Locations.md`) for setup and troubleshooting.
- **Mobile terminal positions relayed between servers.** The same loop-safe
  links carry each terminal's decoded position (`FED_MS_POS`; the newest fix
  per ISSI wins), so every server's `/map` shows terminals seen by any peer.
  Sent only to peers that announce support (hello version 2); 0/0 is never
  relayed or plotted. Needs `loop_safe = true` on both servers; see the
  Basestation-Locations wiki page.
- **Map icons.** Basestations are drawn as a radio mast (grey when offline)
  and mobile terminals as a handheld radio, instead of the default pin.
- **MCC / MNC in the cell block.** The cell header now reads
  `MCC / MNC / CC / LA`. Needs a FlowStation that sends the new fields;
  older ones show CC / LA only.

Version 1.15.2 adds:

- **Open/read-only dashboard mode with admin Login button.** A new
  `[dashboard.ui]` `open` flag (default `false`). When `ui.open = true`, the
  monitoring dashboard is viewable by anyone without logging in and is
  read-only for anonymous visitors: the Settings link and the Basestation
  Control panel are hidden and their endpoints refuse non-admins. A **Login**
  button in the top-right of every page header lets an admin authenticate
  (and a **Logout** button replaces it once signed in), unlocking editing and
  control for that browser. With `ui.open = false` the previous behavior is
  unchanged -- configured `[dashboard.users]` must log in just to view.
- **Basestation control is now admin-gated.** Issuing control commands
  (`POST /api/control/{id}` -- Ambience Listen, DGNA, kick, restart, etc.)
  now requires an admin credential, the same gate as `/settings`, so the
  control panel is a privileged action rather than available to every
  dashboard viewer.

Version 1.15.0 adds:

- **Ambience listening (experimental).** A new `AmbienceListen { issi, enable }`
  Basestation Control command, with a Start/Stop control on each Basestation
  card in the dashboard. It tells the Basestation to set up an SS-AL call so
  the target radio auto-keys its microphone (with indication on the radio);
  `enable: false` releases it. The wire shape is provisional and only takes
  effect on a Basestation that implements the command and the over-the-air AL
  service.
- **Dispatch-console AL wiring.** A private-call `SETUP_REQUEST` whose service
  byte is the ambience-listening value (as tetra-dispatch's "Ambience listen"
  sends) is now relayed as usual *and* fires the `AmbienceListen` control
  command to the Basestation that hosts the target ISSI (resolved via its Brew
  username), so the console button drives the correct forced-key path. The
  call shows as `ambience` on the dashboard. Best-effort: if the target's
  Basestation has no control session, it stays an ordinary relayed call.

Version 1.14.0 adds:

- **Active/standby high availability.** New `[ha]` section (off by
  default): two brew-servers on one LAN / Layer 2 segment, or on one host,
  share a virtual IP. They elect an Active node over HMAC-signed UDP
  heartbeats: the higher `weight` wins, and `persist = true` keeps a node
  Active even when a higher-weight peer comes back. The Active node holds
  the VIP (`ip addr` + gratuitous ARP; needs `CAP_NET_ADMIN` and
  `CAP_NET_RAW`) and alone runs the Brew, telemetry, control and SIP
  listeners and the background tasks, binding the VIP for listeners set to
  `0.0.0.0`. On failure the Standby takes over within `dead_after_ms`
  (default 2 s), and Basestations reconnect to the same address. A node
  that leaves Active restarts into Standby. Optional `check_gateway` keeps a
  node without an uplink from taking the VIP. See
  [High Availability](https://github.com/ysamouhos/brew-server/wiki/High-Availability).
- **HA replication.** The Standby keeps a live copy of the Active node's
  call/SDS history and SMS Center queue over an authenticated TCP link on
  `heartbeat_port`, so a failover loses neither; history that moves between
  the nodes is never duplicated. A returning higher-weight node takes over
  only once its copy is up to date.
- **High Availability dashboard page** (`/ha`): both nodes' role, weight,
  persist, VIP holder, heartbeat and role-change log, plus a role badge in
  every page header. Admins can switch the Active node, set persist (kept
  in `ha-state.json`, no restart), edit `[ha]` and apply the saved config.
  Make Active, Make Standby, Config default and Apply ask for confirmation,
  naming both nodes and what drops, restarts or takes the VIP. Warnings
  flag unapplied saved changes and a config that differs from the peer's,
  and name the settings involved (never their values); the peer comparison
  ignores `[ha]`, file paths, comments and formatting.
- **No automatic config reload with HA.** With `[ha]` enabled, config file
  changes are applied only from the dashboard's **Apply saved config**,
  which hands an Active node over to its Standby before restarting it.
- **Dashboard `/healthz`.** The dashboard listener answers an
  unauthenticated `/healthz` (`ok`, or `ok active` / `ok standby` with HA),
  for health checks that must pass on both HA nodes.

Version 1.13.0 adds:

- **Federation discovery identifies the peer.** The discovery `GET` of an
  outbound peer link now carries `User-Agent: brew-server/<version>`,
  `X-Brew-Mode: Peer` and `X-Brew-Version`. With `[auth]` enabled the far
  end takes the mode from that request, so between two brew-servers the link
  used to be registered there as a Basestation (no table sync, no relay) and
  federation only worked in one direction; servers that require a
  User-Agent on discovery no longer refuse the link. Inbound, a peer that
  announces its mode and version only on the WebSocket upgrade (brew-server
  1.12 and older) is now recognised too.
- **TLS federation links.** New per-peer `tls` (default `false`): discovery
  over `https://` and the link over `wss://`, for a peer whose `[tls]` is
  enabled -- two servers configured like the example `brew-server.toml`
  could not be linked at all before. The peer certificate is checked against
  `tls_ca_path` (default `/etc/ssl/certs/ca-certificates.crt`) for
  `tls_server_name` (default: the host of `remote_host`), or, for a
  self-signed peer, pinned with `tls_pinned_cert_path`. The Docker image now
  ships `ca-certificates`.
- **Federation keepalive.** Every federation link, dialled or accepted, is
  pinged every `[federation] keepalive_interval_seconds` (default `15`) and
  closed after `keepalive_timeout_seconds` (default `45`) without any frame,
  so a half-open link no longer keeps routes pointing at it; `0` disables.
  Dialling a peer now gives up after 30 s instead of possibly hanging.
- **Unroutable private calls are rejected.** A private `SETUP_REQUEST` whose
  destination is not registered anywhere (and has no SIP route) is answered
  with `CALL_SETUP_REJECT`, cause 3 ("called party not reachable"), so the
  calling radio is released at once instead of waiting out its own timer.
- **Calls end cleanly when a connection drops.** When a Basestation or peer
  link disconnects, the other participants of its calls get the
  `CALL_GROUP_IDLE` / `CALL_RELEASE` a normal hangup would send (cause 14,
  "SwMI requested disconnection"). A group listener dropping no longer ends
  the call for everybody else.
- **No talker names to v0 connections.** `GROUP_TX` and `SETUP_REQUEST`
  forwarded to a connection that announced Brew v0 (`X-Brew-Version: 0`)
  lose the v1 `mnemonic[34]` tail. A connection that announced no version
  still gets them as sent: FlowStation without digest credentials upgrades
  with no `X-Brew-*` header at all, yet shows the talker name. With `[auth]`
  disabled, the mode and version a client announces on its discovery `GET`
  now also apply to its WebSocket upgrade from the same address with the
  same `User-Agent` (FlowStation sends one on both), so other clients behind
  the same NAT are unaffected; the `Peer` mode is never carried over, a peer
  announces it on the upgrade itself.
- **Loop-safe federation (opt-in).** With `[federation] loop_safe = true` on
  both ends, peers negotiate it at connect time (`X-Brew-Federation` /
  `X-Brew-Server-Id`) and exchange path-vector route adverts instead of
  relayed SUB messages, so rings, full meshes and redundant links are safe:
  the newest registration wins network-wide, reached by the shortest
  loop-free path, with failover when a link drops. Older peers keep working
  as tree leaves: a server without it must hang off the mesh by a single
  link. Enable it everywhere before adding redundant links, and keep the
  servers' clocks on NTP.
- **Duplicate calls and SDS across peer links are dropped.** A `GROUP_TX`,
  private `SETUP_REQUEST` or SDS reaching a server a second time over another
  peer link (a ring, a mesh, two links between the same servers) is
  recognised by (uuid, source ISSI) and dropped, instead of taking the call
  over and being forwarded again; a late copy within 5 s of the call ending
  no longer brings it back. A talker change inside a group call (same uuid,
  another ISSI) is still accepted. Basestation traffic is never checked.
  Between loop-safe servers (`loop_safe`) the duplicate's sender is told to
  stop (`FED_PRUNE`), so a redundant link does not carry a second copy of
  the voice stream.
- **Registration fixes.** A `DEREGISTER` from a connection that does not own
  the ISSI is no longer relayed to peers; an ISSI re-registering on another
  connection no longer drops the old connection from groups its other ISSIs
  are still in (all the ISSIs behind a peer link, typically). A registration
  relayed to a peer is now followed by its group affiliations.

Version 1.12.0 adds:

- **Basestation version.** Each Basestation card shows the software version
  next to its IP (build hash on hover), from the `StationVersion` event that
  FlowStation v0.5.2+ sends on every telemetry connect. `/api/telemetry` gains
  `version` and `build`.
- **Per-cell RF metrics.** On multi-cell stations each cell row shows that
  cell's EVM, PAPR and SDR temperature, from the `CellRf` events of the
  additional cells. `/api/telemetry` gains `cell_rf`.
- **Quieter logs for newer Basestations.** `CellRf` is decoded instead of
  being logged as malformed several times a second, and any other unknown
  event type is logged at debug level instead of as a warning.

Version 1.11.0 adds:

- **Basestation cells in the telemetry dashboard.** Each Basestation card
  shows its cells from FlowStation's `CellsSnapshot` telemetry: carriers with
  TX/RX frequencies, colour code / location area, neighbours, registered
  radios, RF state and SDR, and whether the cells are site-linked. On
  multi-cell stations the active-calls table gains a Cell column and
  registered ISSIs show their cell. Telemetry cards are wider (at least
  480 px). `/api/telemetry` gains `cells`, `site_linked` and `ms_cell_out`.
- **Multi-cell telemetry.** The `MsCell` event is decoded instead of being
  logged as a malformed event.
- **Registration list resync.** A station's registration list used to start
  empty on every telemetry reconnect, so radios registered earlier never
  showed. It is now rebuilt from each `CellsSnapshot` (FlowStation v0.5.1+).

Version 1.10.0 adds:

- **SMS Center (store-and-forward SDS).** An individual SDS addressed to an
  ISSI that is offline everywhere on the Brew network used to be dropped
  ("SDS has no registered destination"). It is now stored in a JSON file
  (`[sms_center].path`, default `sms-center.json`) and delivered when that
  ISSI registers again on any Basestation or federation peer: the server
  originates a normal `SHORT_TRANSFER` + `SDS_TRANSFER` from the original
  sender, so the radio sees the real source and its own SDS-TL report goes
  back end-to-end. A message stays queued until the destination Basestation
  answers with `SDS_REPORT`; unanswered attempts are retried
  (`ack_timeout_seconds`, `retry_interval_seconds`, `max_attempts`) until
  `message_ttl_seconds`. If the sender requested an SDS-TL delivery report it
  receives a report on queueing (`stored_report_status`, default 0x00
  "received", because radios show the ETSI 0x22 "stored" status as failed),
  and "validity period expired" (0x48) / "delivery failed" (0x4A) if the
  message is later discarded. By default only ISSIs that have registered
  before are eligible, and LIP positions (PID 0x0A) are never stored. New
  dashboard page `/sms-center` and API `GET /api/sms-center`,
  `DELETE /api/sms-center/{id}` (admins).

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
