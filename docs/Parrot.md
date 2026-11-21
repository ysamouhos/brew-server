# Parrot

A number radios call to test themselves: what a radio says is recorded and
sent back after an adjustable delay. Off by default.

## Settings

Server settings → **Parrot** (admins), or `[parrot]` in the config file.
Applied at once, no restart.

| Key | Default | Meaning |
| --- | --- | --- |
| `enabled` | `false` | Switches the parrot on and off. |
| `issi` | `0` | The number radios call (individual call), 1 to 16777215. |
| `separate_gssi` | `false` | Off: the parrot group has the same number as `issi`. On: `gssi` is the group. |
| `gssi` | `0` | The parrot group when `separate_gssi` is on; 0 = no group. |
| `delay_ms` | `1000` | Wait after the over before it is played back, 0 to 60000 ms. |
| `max_secs` | `60` | Longest over recorded, 1 to 600 s. A duplex call is ended after it. |

```toml
[parrot]
enabled = true
issi = 9990
delay_ms = 1000
max_secs = 60
```

ISSIs and GSSIs are separate address spaces in TETRA, so one number can be
both the parrot's ISSI and its group.

Saving with the parrot on adds its group to the allowed talkgroups
(`[talkgroups] allowed`, so managed talkgroups admit it) and names the ISSI
and the GSSI "Parrot" in the Directory (`[issi_names]`, `[gssi_names]`). A
name that is already there stays. Switching the parrot off leaves both.

## Individual call

A radio calls the parrot ISSI. The server answers the call itself
(`CALL_SETUP_ACCEPT`, then `CALL_CONNECT_REQUEST` with the parrot as the
called party, simplex or duplex as the radio set it up).

- **Simplex**: the radio talks; when it lets go of PTT (`CALL_SIMPLEX_IDLE`
  from its station, or one second without voice) the over is played back
  after `delay_ms`: `CALL_SIMPLEX_GRANTED` (the station gives the floor to the
  other party), the voice frames in their original timing, then
  `CALL_SIMPLEX_IDLE`. Talking again before the playback starts drops the old
  over; taking the floor during the playback stops it. The radio ends the call.
- **Duplex**: every voice frame comes back `delay_ms` later (an echo). The
  parrot ends the call (`CALL_RELEASE`) after `max_secs`.

## Group

An over on the parrot group is recorded (up to `max_secs`) and, `delay_ms`
after it ended, played back on the group as a new group call with the parrot
ISSI as the talker, so every radio in the group hears it, the one that
talked too. When someone talks on the group at that moment, the playback
waits for the group to become free (up to 30 s). A call of higher priority
pre-empts it like any group call. The station must route the group to Brew
like any network group.

## Notes

- The parrot runs on the server: it works from every station and needs
  nothing new on the stations or radios. No air-interface behaviour changes;
  the stations use the call control they already have for calls to the
  network.
- Only calls from this server's own connections (stations, terminals) are
  answered. A call that comes over a link from another server is left to the
  parrot there.
- The parrot ISSI and group are not checked against the ISSI block or allow
  list (the parrot is not a radio); the radio that calls it is.
- A radio registered with the parrot ISSI cannot be called while the parrot
  is on: the parrot answers.
- Recordings are kept in memory only, for the over being played back.
