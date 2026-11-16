# brew-server

Experimental Rust Brew core for linking two or more MidnightBlue Basestation or Flowstation TETRA base stations.

Reference spec from https://wiki.tetrapack.online/tetra/specifications/brew/

Related Projects: 

- Tetra BTS - https://github.com/Aitorrio/bost-flowstation

- Tetra Dispatch - https://github.com/ysamouhos/tetra-dispatch



**What's new:** see [CHANGELOG.md](CHANGELOG.md)

**Configuration and feature docs:** see the [wiki](https://github.com/ysamouhos/brew-server/wiki)

## Server name

The dashboard heading and the browser tab titles use the server name
(default "Tetra Network Monitor"). Set your own in the `[dashboard]` section
of the config:

```toml
[dashboard]
servername = "Tetra Network Monitor"
```

## Dashboard

The dashboard (port 9003) has a sidebar layout that follows the system's light
or dark mode (or a fixed one, from the account menu). Its **Routing** page
sends DGNA to several radios at once and manages talkgroup pools, which
connect talkgroups inside the server without a restart
([docs/Routing.md](docs/Routing.md)). **Accounts** adds the logins of
Basestations and terminals (Brew, telemetry, control) without a restart, with
an info text and position per login, the **Directory** keeps names for ISSIs
and groups (`[issi_names]`, `[gssi_names]`) and who may use a group (allow or
block list and RX only, `[group_access]`), and **Config file** edits the whole config
file. **Managed talkgroups** on the Routing page close the set of groups that
may be used (`[talkgroups]`). The optional **ham callsign database** names
radios without a Directory name from radioid.net `dmrid.dat`, fetched on
demand or daily (`[callsign_db]`; Directory names always come first). The
call and SDS history keeps 7 days and at most 2000 entries each by default
(`[storage] keep_days`, `max_calls`, `max_sds`), admins can clear it, and the
totals keep counting. Sign in with the Log in button top right; HTTP Basic auth keeps working
too. To keep the classic dashboard at `/`:

```toml
[dashboard]
legacy_ui = true
```

## Fleet Control

The **Fleet Control** page manages the Basestations (FlowStation) over the
existing control channel (`[control]`, WebSocket, works behind NAT, no new
port). A station takes part only after its own administrator switches Fleet
Control on; the others are not touched. Every function is announced as a
capability, so older stations and servers keep working.

- **Overview**: each managed station with software version, cells, RF state,
  health, and whether a newer version is on its update source.
- **Remote and live update**: check and update one, several or all stations,
  as a run with progress (one by one, in groups or all at once). A normal
  update keeps each station on its own repository and branch; a live update
  builds from the Updates repo of the server settings while the radio stays
  on.
- **Maintenance banner and restart**, from the server.
- **Station config**: read and change cells, neighbours, reselection values,
  Brew, telemetry, update source and single values of the config file (never
  passwords or keys), with a confirmation window, templates, bulk changes and
  neighbour suggestions.
- **Graphs and alarms**: CPU load, temperatures, ping, radios and calls as 24 h
  graphs; alarms (unreachable, SDR error, temperature, CPU load, missed pings)
  with acknowledge and a 90-day history.
- **Change log and rights**: every action is logged with who did it; the rights
  "Fleet Control visibility" and "Fleet Control management" decide who sees and
  who acts.

Protocol and details: [docs/Fleet-Control.md](docs/Fleet-Control.md).

## Run directly

Requires a Rust toolchain and a C compiler (the vendored ACELP codec in
`third_party/tetra-codec/` is compiled by `build.rs`).

```bash
cargo run --release -- sample/brew-server.toml   # the example; copy and edit it for your own
```

## Run via Docker

```bash
docker compose up --build
```

## Health check

```bash
curl http://127.0.0.1:9000/healthz
```

With `[ha]` enabled, the Brew `/healthz` answers only on the Active node; the
dashboard's `/healthz` (port 9003) answers on both, with the node's role.

## More documentation

- [Dashboard users](docs/Dashboard-Users.md) -- admins, users and Viewer, per-page rights, own passwords, the way back in when locked out
- [Roaming](docs/Roaming.md) -- kick stale registrations on old Basestations
- [Handover](docs/Handover.md) -- radios keep group and individual calls when they move between Basestations
- [Brew extensions](docs/Brew-Extensions.md) -- the protocol behind it, for other server and Basestation implementations
- [Fleet Control](docs/Fleet-Control.md) -- manage the Basestations from the server over the control channel (stations opt in; protocol, Fleet Manager, rights, change log)
- [Late entry](docs/Late-Entry.md) -- join running group calls mid-over
- [Routing](docs/Routing.md) -- DGNA to several radios, talkgroup pools
- [Multi Network](docs/Multi-Network.md) -- connect to other Brew networks and bridge talkgroups across them (tested on air with TetraPack, including visitors and migration)
- [ISSI blacklist](docs/ISSI-Blacklist.md)
- [ISSI allowlist](docs/ISSI-Allowlist.md) -- run a closed network: only listed ISSIs may communicate
- [Basestation locations](docs/Basestation-Locations.md)

## High availability

Two brew-servers can run as an active/standby pair sharing a virtual IP on
one LAN. See [docs/High-Availability.md](docs/High-Availability.md).

## Credits

- Joaquin EA5GVK
- Sebastian OE4SXM ([sebastianmadl](https://github.com/sebastianmadl)) -- late entry, carrier registry, handover and individual calls between Basestations, the Brew extensions, dashboard redesign, Routing page and talkgroup pools, login and accounts pages, ISSI and GSSI directory, group access and RX only (`group-access`), Multi Network (bridges to other networks, visitors, `issi-check`, `mni`), Fleet Control (managing the Basestations from the server)
- David Harcus MM7FDM
