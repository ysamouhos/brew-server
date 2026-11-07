# Cell changes between Basestations (handover)

A radio that moves from one Basestation to another should keep its call. In
TETRA the radio decides when it changes cells (cell reselection, ETSI TS 100
392-2 clause 18.3.4); the network cannot hand it over. What the network can do
is be ready for it: prepare the new cell when the radio announces the change,
tell the old cell when it went, and let the call go on from the new cell.
"Handover" in brew-server only names that help between Basestations.

With two or more Basestations that speak the extensions below (FlowStation,
`handover` work), brew-server:

- relays an **announced** cell change (U-PREPARE on the old cell) to the
  Basestation of the target cell, so the radio arrives registered and the
  group call is already there;
- tells the old Basestation when a radio changed **unannounced**, and ends an
  over the radio was talking in there, so it can talk on from the new cell;
- moves **individual calls** with the radio, so it can restore them on the new
  Basestation;
- shows where every radio is and how it changed cells (Connections page).

Group calls need no help for listeners: a radio that registers on the new
Basestation joins the running over there by [late entry](Late-Entry.md).

The messages between Basestation and server are specified in
[Brew-Extensions.md](Brew-Extensions.md), for anyone who wants to build them
into another server or Basestation.

## Requirements

- brew-server 1.20.0 or newer and Basestations that speak the extensions:
  [Bost FlowStation](https://github.com/Aitorrio/bost-flowstation) 0.5.6 or
  newer (when released).
- Every Basestation connects in Basestation mode and is offered the
  extensions on the WebSocket upgrade (`X-Brew-Extensions`); brew-server
  always offers them. A Basestation uses only the ones it knows.
- Each cell has its own main carrier. Carriers can be reused across a
  network if the location areas differ (see "Target lookup").
- On the radio side the Basestations list each other as neighbour cells
  (FlowStation: `[[cell_info.neighbor_cells_ca]]`), otherwise the radio has no
  neighbour to announce. With the `cell-directory` extension the server sends
  each Basestation the other Basestations' cells with what they broadcast;
  FlowStation's Roaming page uses it to pick neighbours and to warn when two
  cells do not list each other or the levels in an entry differ.
- TetraPack, Brandmeister and Basestations without the extensions are not
  affected: they get none of the new messages and behave as before.

## Configuration

```toml
[handover]
timeout_ms = 2000      # how long the target Basestation has to answer
```

Keep it below the serving Basestation's own fallback (FlowStation: 3 s) and
well below the radio's T370 of 5 s (clause 18.6.2). Nothing else to configure.

## Announced cell change

In announced reselection the radio tells its serving cell first (U-PREPARE,
answered with D-NEW-CELL or D-PREPARE-FAIL, clause 18.3.4.7). When it names
the target cell and the target is served by another Basestation:

1. The serving Basestation sends the request to the server, with the
   registration for the new cell if the radio included one (forward
   registration).
2. The server finds the target Basestation by carrier and passes the request
   on. The target registers the radio (which moves the ISSI to it on Brew and
   brings it into its group calls by late entry) and answers.
3. The server relays the answer; the serving Basestation sends D-NEW-CELL
   with the target's registration answer, and the radio goes.

If the target does not answer within `timeout_ms`, or there is no target, the
serving Basestation is told at once and answers the radio as without the
extension. A target that went away during the request makes the serving
Basestation answer D-PREPARE-FAIL, so the radio is not sent to a cell that is
gone.

A radio that announces without naming a cell (type 3, clause 18.3.4.7.4)
needs no server: the old Basestation answers D-NEW-CELL itself.

Log lines: `handover: request forwarded to the target Basestation`,
`handover: result relayed to the serving Basestation` (with `ms`),
`handover: no target` (with `reason`), `handover: target did not answer in
time`, `handover: target Basestation went away`.

### Target lookup

The target is the Basestation that announced the cell's network (MCC, MNC),
carrier and band plan. The serving Basestation may also name the target's
location area (from its neighbour entry): carriers are reused across a
network, so the server then only picks a Basestation whose cell on that
carrier is in that location area. A "no target" answer carries a `reason`:
`gone` (the target Basestation went away), `unknown`, `own_cell`,
`not_capable` or `no_network`.

### Roaming kick

With the [roaming kick](Roaming.md) on, a radio whose change was prepared
through the server is not kicked: its registration on the new Basestation
came with the handover, and the old Basestation lets it go by itself. The kick
is skipped while the handover is in flight and for `kick_delay_ms +
[handover] timeout_ms` after it succeeded (log line `roaming: radio was handed
over, kick skipped`). A handover that fails, times out or finds no target does
not count, so the unannounced move that follows is kicked as usual.

The guard is per ISSI, not per Basestation: if the radio moves on again
without preparation within that window (ping-pong back to the old cell right
after the handover), the kick of the cell it just left is skipped too, and
that Basestation keeps its stale entry until its own timeout.

## Unannounced cell change

Most radios change cells unannounced, a listener always (clause 18.3.4.7.1),
and then the Basestation they left does not know that they went. Two things
change, both only between Basestations that opted in:

- **The old Basestation is told.** If it announced the `ms-moved`
  capability, it gets `SERVICE_MS_MOVED` and stops serving the radio without
  deregistering it (log line `handover: radio left unprepared, old
  Basestation told`). It no longer keeps a traffic channel open for a group
  nobody there listens to.
- **A talker's over there ends.** If the radio was the talker of a group call
  that the old Basestation owns and the new Basestation speaks `handover-v1`,
  the over ends at once: the listeners get `GROUP_IDLE`, the group floor is
  free, and the radio's next `GROUP_TX` from the new Basestation goes through
  (log line `handover: talker left its Basestation, ending its over there`).
  Before, the floor stayed taken until the old Basestation noticed the silent
  uplink, and the radio's PTT on the new cell was denied.

A prepared handover is not reported this way.

## Individual calls

A radio in an individual call with a Brew party (a terminal, an app, a radio
on another Basestation) that changes to another Basestation restores the call
there with U-CALL RESTORE (clause 14.5.1.2.4). The new Basestation never saw
the call, so the server moves it, between Basestations that announced the
`call-restore` capability:

- **At the registration.** Every individual call of the radio that is up on
  the old Basestation moves to the new one (log line `call restore:
  individual call moved with the radio`). The old Basestation gets
  `CALL_RELEASE` with cause 14 (SwMI requested disconnection) and releases
  its leg without ending the call; anything it still sends for the call is
  ignored. Calls bridged to SIP are not moved.
- **At the restore.** The new Basestation asks for the call and gets it back
  with who holds a simplex floor (log lines `call restore: radio restores its
  call here` / `no call of this radio to restore`). The radio gets its own
  channel on the new cell; duplex calls talk on both ways after the break.
- **Not restored.** If the radio does not restore the call within 8 s (the
  radio gives up after T306, at most 6 s, clause 14.6), the other party gets
  `CALL_RELEASE` with cause 19, "call restoration of the other user failed"
  (log line `call restore: the radio did not restore the call, ending it`).
- **Given up.** The call ends the same way at once when the radio shows it
  has no call left: the new Basestation reports a plain roaming registration
  (a radio with a circuit mode call registers with "service restoration
  roaming", clause 16.4 a)), or the radio sets up a new call from there (log
  line `call restore: the radio gave up the call, ending it`). The other
  party can call back at once.

A Basestation without the capability keeps its calls as before: the old
Basestation ends the call when the radio's uplink stays silent.

### Calls between two radios

Between Basestations that also announced `call-handoff`, a call between two
radios survives a station change in both directions:

- **Both on one Basestation, one leaves.** The server never saw this local
  call. When the radio restores it on the new Basestation, the server asks
  the Basestation of the other radio for it (`SERVICE_CALL_HANDOFF`, log line
  `call restore: asking the peer's Basestation for its local call`). That
  one hands it over under a new call UUID; the call then runs between the two
  Basestations (log line `call handoff: a local call now runs between two
  Basestations`). The radio that stayed notices nothing; a talker that left
  loses its floor at the break, as ETSI says. No answer within 1.5 s: the
  restore fails as before (log line `call restore: no handoff answer, no
  call`). A duplex local call whose leg goes silent is held 8 s on that
  Basestation so the request can still find it.
- **On two Basestations, one comes to the other.** At the registration the
  old Basestation lets its leg go as above; the new one, where the other
  radio is, joins both legs into a local call when the radio restores, and
  ends the Brew call. Not restored within 8 s: the other radio's call ends
  as above.

Multi-cell FlowStations do not announce `call-handoff` yet. Between the cells of one multi-cell FlowStation such calls survive a cell change without the server: the FlowStation handles them itself.

## What the dashboard shows

- **Connections page, "Cells":** every cell of every Basestation, named
  `<login>/<cell>`, with carrier, downlink frequency, location area,
  MCC/MNC, neighbours, capabilities, address and ISSIs. A carrier claimed by
  two Basestations is flagged; the newest announcement serves it. Neighbours
  are the cells a cell broadcasts, named when the server knows them. A
  station with a login that went away stays listed as offline.
- **Cell per radio:** subscriber tables on the Connections page and the main
  dashboard ("Registered Subscribers", `/api/ms-cells`), and the cell of each
  entry in the registration log.
- **Cell Change Log** (Connections page, last 200 changes, in memory,
  `/api/connections` `roaming_log`): every change with from / to cell and how.
  If the Basestation the radio left reports it (`ms-reselection`), the
  category of cell reselection (clause 18.3.4.7.0):

  | Shown | Meaning |
  |---|---|
  | undeclared | no call running on the old cell |
  | unannounced | left a call without telling the old cell |
  | announced type 1 | told the old cell (U-PREPARE), got D-NEW-CELL "follow MAC channel change" |
  | announced type 2 | told the old cell, got D-NEW-CELL "change channel immediately" |
  | announced type 3 | told the old cell without naming a cell, got D-NEW-CELL |

  "prepared here" is added when the target was prepared through this server.
  Without the report the column shows "announced" / "not announced". The
  report and the move may arrive in either order, up to 10 s apart.
- `/api/connections` also has `handovers_pending`.

## Field results

Tested with two FlowStation Basestations (Pluto+ and SX1255, carriers 1528 /
1530) and a Motorola MTP6650 between October and November 2026:

- Listeners and idle radios change unannounced; the group call goes on after
  a gap of 0.5 to 1.5 s.
- A talker holding PTT keeps the floor on the new Basestation.
- Duplex and simplex individual calls are restored on the new Basestation;
  a call the radio gave up (link lost at the cell edge) ends for the other
  party at once.
- The MTP6650 never announced a change with a target cell (its background
  scan of the neighbour never showed), so the prepared path (forward
  registration, types 1 and 2) is covered by the unit tests and the
  simulation only.
- Not yet tested on air: a multi-cell Basestation (several cells on one
  FlowStation) as the source or target of a move between Basestations, and
  calls between two radios across Basestations (`call-handoff`).

## Tests

`cargo test handover`, `cargo test call_restore` and `cargo test roaming_sim`
cover the relay (timeouts, target gone or unknown, reused carriers, capability
gate), the unannounced cases and individual call moves; `roaming_sim` runs 10
Basestations and 50 radios.
