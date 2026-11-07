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
block list and RX only, `[group_access]`), and **Configuration** edits the whole config
file. Sign in with the Log in button top right; HTTP Basic auth keeps working
too. To keep the classic dashboard at `/`:

```toml
[dashboard]
legacy_ui = true
```

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

- [Roaming](docs/Roaming.md) -- kick stale registrations on old Basestations
- [Handover](docs/Handover.md) -- radios keep group and individual calls when they move between Basestations
- [Brew extensions](docs/Brew-Extensions.md) -- the protocol behind it, for other server and Basestation implementations
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
- Sebastian OE4SXM ([sebastianmadl](https://github.com/sebastianmadl)) -- late entry, carrier registry, handover and individual calls between Basestations, the Brew extensions, dashboard redesign, Routing page and talkgroup pools, login and accounts pages, ISSI and GSSI directory, group access and RX only (`group-access`), Multi Network (bridges to other networks, visitors, `issi-check`, `mni`)
- David Harcus MM7FDM
