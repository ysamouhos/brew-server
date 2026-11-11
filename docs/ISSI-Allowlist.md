# ISSI Allowlist

Run a closed network: only the ISSIs on the allowlist may communicate through
this brew-server, every other radio is barred. It is the counterpart of the
[ISSI blacklist](ISSI-Blacklist.md), which bars a few radios on an otherwise
open network. Manage it on the dashboard under **System -> Server settings -> ISSI allow list** (admin only; classic dashboard: Settings -> ISSI Allowlist) or in
`brew-server.toml`:

```toml
[allowlist]
enabled = true
issis = [4013, 4014, 4015]
```

Changes made on Server settings apply **immediately and without a restart**
(a restart would drop every call). The config file is updated at the same time.
Editing the file by hand, or saving the raw config, restarts the server as any
other config change does.

## Why at the server

A Basestation only sees the radios on its own cell. The allowlist is checked at
the server, so it is one list for the whole network: a radio that is not on it
is barred whichever Basestation it registers on, and admitting it here admits
it everywhere. The Basestations' own registration checks are untouched.

## What an unlisted ISSI cannot do

While `enabled = true`, an ISSI that is not on the list is barred exactly as a
blacklisted one is:

| Traffic | Result |
|---|---|
| Group call transmission (PTT) from it | dropped (not an emergency call) |
| Private call from it | rejected |
| Private call to it | rejected |
| SDS from it, or to it (text, status, ...) | dropped |
| SIP call to it | refused (403) |
| Stored SMS Center messages for it | stay queued until it is admitted |

## What still works

The same exceptions as for the blacklist, for the same reasons:

- **Emergency calls** (priority 15, or from an ISSI with an emergency alarm
  active on a Basestation) are never held back and show in the red ribbon.
- **Ambience listening (SS-AL)** to it from dispatch goes through.
- **LIP position traffic** in both directions, so dispatch can still locate it.
- **Registration.** The radio still registers on its cell and is listed on the
  dashboard, where it can be seen and admitted. To clear it off the cell as
  well, see *Kicking unlisted radios* below.

## The blacklist still applies

An ISSI on the allowlist that is also blacklisted stays barred. The server log
names the list that barred a transmission (`reason = "blacklisted"` or
`reason = "not on the allowlist"`), and the dashboard's emergency ribbon tags a
barred ISSI the same way for both.

## Enabling it

- With `enabled = false` (the default) the list is kept but not enforced: every
  radio may communicate. Build the list while the network runs, then turn
  enforcement on from Server settings; nothing is dropped in between.
- With `enabled = true` and an **empty** list every radio is barred. The server
  says so at startup (`ISSI allowlist enforced with an EMPTY list`) and the
  dashboard shows it ("Enforced, empty", and asks before enforcing an empty list), rather than guessing that an open network
  was meant.
- Every change is logged (`ISSI allowlist changed`, `ISSI allowlist enforcement
  changed`, with who asked).

## Kicking unlisted radios

A barred radio is silent on the network but still registered on its cell. With

```toml
[allowlist]
enabled = true
kick_unknown = true
kick_delay_ms = 2000
```

the server also sends a `KickMs` for it to the Basestation it registered on,
over the control channel, after `kick_delay_ms`. The radio then has to register
again, and is kicked again if it does. Needs `[telemetry]` and `[control]` to be
enabled, as the roaming kick does. If the ISSI is admitted during the delay the
kick is skipped. Off by default: a barred radio then stays where dispatch can
see it.

## API

Server settings uses these admin endpoints, which scripts can use too:

| Method | Path | Effect |
|---|---|---|
| `GET` | `/api/config/allowlist` | `{"enabled": bool, "issis": [...]}` as enforced now |
| `POST` | `/api/config/allowlist/{issi}` | admit an ISSI |
| `DELETE` | `/api/config/allowlist/{issi}` | remove an ISSI |
| `PUT` | `/api/config/allowlist/enabled` | body `{"enabled": true}` or `false` |

All apply at once and write the config file without a restart.
