# Roaming between Basestations

A TETRA radio that moves to another cell registers there, but it never
deregisters from the cell it left. Without help, the old Basestation keeps
listing the ISSI as registered until its own timeout (if it has one), so the
dashboard shows the same radio on several Basestations at once.

Call and SDS routing in brew-server is not affected by this: there is one
routing entry per ISSI and the newest registration always wins. What goes stale
is the old Basestation's own registration list, which it reports over
telemetry and which the dashboard mirrors.

## Configuration

```toml
[roaming]
kick_old_issi = 1      # 1/true = on, 0/false = off (default)
kick_delay_ms = 2000   # hold-off before the kick is sent
```

Requires `[telemetry]` and `[control]` to be enabled, since the registration
comes in over telemetry and the kick goes out over the control channel.

## What happens

When a Basestation reports `MsRegistration` for an ISSI and
`kick_old_issi` is on:

1. Every other Basestation that still lists that ISSI drops it from its
   dashboard list right away. The registration log shows the entry there as
   **Deregistered (roamed to other BS &lt;id&gt;)**.
2. After `kick_delay_ms`, the server sends `KickMs` for that ISSI to each of
   those Basestations over the control channel, so they release it too.
3. If the radio has registered back on the old Basestation in the meantime
   (ping-pong at a cell border), that kick is skipped and logged as
   `roaming: radio came back to the old BS, kick skipped`.
4. The Basestation's own `MsDeregistration` that confirms the kick is not
   logged a second time.

Server log lines: `subscriber roamed to other BS, kicking old registration`
(with `from`/`to`), plus a warning if the old Basestation refuses the kick or
is not connected on the control channel.

With `kick_old_issi = 0` the behaviour is exactly as before.

## Cell changes between Basestations

Announced cell changes, an unannounced change during a call, the cell change
log and individual calls that follow their radio are described in
[Handover.md](Handover.md). The kick skips a radio whose cell change was
prepared through the server (see there).

## Known limit

The roam-back check runs just before the kick is sent. If a radio returns to
the old Basestation at the very moment the kick is in flight (a window of the
control-channel latency, typically milliseconds, and only after it stayed on
another cell for at least `kick_delay_ms`), the old Basestation can still kick
the fresh registration. The radio then has to register again. Raise
`kick_delay_ms` if radios flip between two cells often.

## Load test

`cargo test roaming_sim -- --nocapture` simulates 10 Basestations and 50
radios. Each fake Basestation answers a kick the way FlowStation does
(`KickMsResponse` plus an `MsDeregistration` on its telemetry).

| Scenario | Moves | Kicks | Wrong kicks | Stale entries | Misplaced radios | Duplicate log rows |
|---|---|---|---|---|---|---|
| `kick_old_issi = 0`, radios move one at a time | 1000 | 0 | 0 | 403 | 0 | 0 |
| `kick_old_issi = 1`, radios move one at a time | 1000 | 1000 | 0 | 0 | 0 | 0 |
| `kick_old_issi = 1`, 10 rounds of all 50 radios ping-ponging A → B → A back to back | 1000 | 500 | – | 0 | 0 | 0 |

- *Stale entries*: registrations listed beyond one per radio.
- *Misplaced radios*: radios missing from the Basestation they are on.
- In the ping-pong run, exactly the kicks for B are sent (the radio is back on
  A, so the kick for A is skipped).

The same simulation covers handovers (`roaming_sim_handover*`): mixed rounds
where every other move is a handover (one kick per unannounced move, none per
handover), a target that does not answer (timeout, then the usual kick), a
target that is offline (no target at once, then the usual kick), and a
handover followed straight by an unannounced move back (the fresh
registration is never kicked).
