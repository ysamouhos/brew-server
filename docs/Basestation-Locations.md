# Basestation Locations

The `/map` page plots every Basestation next to the mobile terminals (see
[Map markers](#map-markers) and [Radios on the map](#radios-on-the-map)). A
Basestation's position comes from, in order of precedence:

1. **The Basestation itself**, over telemetry (recommended).
2. **Another brew-server**, relayed over federation.
3. **`[bts_locations]`** in `brew-server.toml`, a static fallback for any
   Basestation that reports no position.

A Basestation covered by 1 or 2 never also shows its static entry.

## Reporting the position from Bost FlowStation

Add the position to the `[telemetry]` section of the FlowStation config:

```toml
[telemetry]
host = "brew.example.com"
port = 443
# ...
site_name = "Athens Hill"   # optional, shown on the map
latitude  = 37.9917         # decimal degrees, -90..90
longitude = 23.7640         # decimal degrees, -180..180
```

- Set both `latitude` and `longitude`, or neither. Values out of range are
  rejected when the config loads.
- **Nothing is sent when the position is unset or `0.0` / `0.0`.** The empty
  position is never transmitted or plotted.
- The position is sent when it changes and **re-sent every 60 seconds**, in
  case the server missed it. After a telemetry reconnect the marker can take up
  to a minute to appear.
- The same telemetry link also carries MCC / MNC, shown in the cell header
  (`MCC 202 / MNC 1 / CC 1 / LA 2`) on the dashboard. Older FlowStation builds
  do not send them and the header shows only CC / LA.

The map matches a Basestation to its static entry by the telemetry login name,
so for the fallback to be replaced cleanly it must equal the Brew username.

## Relay between brew-servers

With `[federation] loop_safe = true`, a brew-server passes the positions of its
own Basestations, and those it learned, on to its federation peers:

- **Both servers must set `loop_safe = true`** in `[federation]`, including a
  server that only accepts the link (it may keep `enabled = false`) -- see
  [Troubleshooting](#troubleshooting-positions-not-relayed). With it set on
  only one end the link stays a plain one and no positions cross it.
- Positions travel as `FED_BTS` messages. A link first sends `FED_BTS_HELLO`;
  positions go **only to peers that announced support**. Older peers and
  legacy (non loop-safe) links receive nothing.
- Every advert carries the servers it crossed, and a server never accepts one
  that went through it, so rings and meshes cannot loop. Each server forwards a
  given advert at most once.
- The origin refreshes a live position at least every 30 seconds of telemetry
  traffic and withdraws it when the Basestation's telemetry disconnects.
- A learned position not refreshed for 2 minutes is dropped, which is how a
  lost link or a dead origin server is noticed.
- A newly connected peer is sent everything currently known.
- Not replicated to an HA Standby: it rebuilds from reconnecting Basestations
  and peer refreshes after a failover.

## Static fallback

```toml
[bts_locations."1000001"]   # the Brew username the Basestation logs in as
name = "Athens HQ"
lat = 37.9917
lon = 23.7640
```

Also editable under Settings and on the dashboard's Accounts page (info,
position and map symbol per login). A `0` / `0` entry is treated as not
configured and is not plotted.

`icon` picks the map symbol of the login (Accounts page, *Map symbol*):

| `icon` | Marker |
| --- | --- |
| unset or `"tmo"` | TMO station (blue mast) |
| `"tbs"` | Tower, TBS (slate) |
| `"terminal"` | Terminal (purple headset): app or console |

```toml
[bts_locations."1000006"]
name = "Tower Kahlenberg"
lat = 48.2766
lon = 16.3335
icon = "tbs"
```

A login connected in Terminal mode is drawn as a terminal unless another
symbol is set.

## Map markers

Markers follow the map design set v2: the colour and glyph give the type, a
small badge adds the status.

- **Status, top left** (stations): green check = connected, orange triangle =
  active Fleet Control warning, red cross = active Fleet Control alarm (only for
  dashboard users who see Fleet Control). Offline: the whole marker grey with a
  dashed ring. Terminals get no status badge.

## Radios on the map

- A radio with a recent position is drawn at it. A radio without one (no
  position, one older than `dock_after_min`, or one sent before it registered
  on another station) is drawn **docked** next to the station it is registered
  on: at a fixed pixel distance in rings around the station (8, then 14, then a
  "+N" bubble), so it stays beside it at every zoom and never covers it.
- A radio registered nowhere is drawn offline (grey) at its last position.
  With `hide_offline_after_h` it is left off the map once that position is
  older than the limit; a new position or registration brings it back, an
  emergency is always shown. 0 (the default) keeps it until a restart of the
  server or *Clear map*.
- Clicking a radio shows its last positions as a track; clicking the map hides
  it. The positions survive a restart (`<storage>.positions`, part of the
  backup).
- Settings → Map (admins), or in the config:

```toml
[map]
track_points = 30     # positions kept per radio, 0..500 (0 = none)
dock_after_min = 30   # dock again after this many minutes without a position, 0..10080 (0 = never)
guests_see_track = true  # off: visitors who are not signed in see only the current positions
hide_offline_after_h = 0 # leave offline radios off the map after this many hours, 0..720 (0 = never)
```

## Troubleshooting: positions not relayed

A Basestation position shows on the server it reports to but not on its peer:

1. **`loop_safe` on both servers.** The dialling server (`[[federation.peers]]`)
   and the accepting one both need:

   ```toml
   [federation]
   loop_safe = true
   ```

   The accepting side can keep `enabled = false`. Restart after changing it.
   Switching a link to loop-safe also changes how subscriber registrations
   are exchanged (route adverts instead of plain subscriber messages), so
   check calls between the sites afterwards.
2. **Same build on both.** A server without this feature ignores the messages.
3. **Check the logs.**
   - `federation: peer link established` with an empty `loop_safe_neighbour`
     means a plain link (see 1).
   - `federation: peer supports Basestation positions; synced` on each side
     means the relay is active.
   - Debug log on the origin: `Basestation position not relayed: no peer link
     announced support`.
4. **The Basestation must report a position** (`latitude` / `longitude` in its
   `[telemetry]`, not 0 / 0) and its telemetry must be connected.
5. **Mobile terminal positions** need both servers on a build that relays them
   (hello version 2). A server on the earlier build still gets Basestation
   positions but no mobile ones.

## Mobile terminal positions

The same loop-safe links also relay the position of each mobile terminal
(ISSI) that a server decodes -- LIP reports and textual beacons -- so the
`/map` page on every server shows terminals seen by any of them. A relayed
marker's popup shows the reporting station and the origin server.

- Needs `loop_safe = true` on both servers, like Basestation positions.
- The newest fix per ISSI wins network-wide, so keep the servers on NTP. Each
  server forwards a fix at most once; rings and meshes cannot loop.
- A 0 / 0 position is never relayed or plotted.
- A position decoded from an SDS that another server forwarded is not
  re-advertised: the server that received it from the radio already did.
- A newly connected peer is sent every position currently known.
- Relayed terminal positions are kept in memory only and are not replicated to
  an HA Standby; they rebuild as terminals beacon and peers reconnect.
