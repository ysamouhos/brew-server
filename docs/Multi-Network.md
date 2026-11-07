# Multi Network: bridges to other Brew networks

brew-server can connect to other Brew networks (for example TetraPack or a
neighbouring operator) and bridge talkgroups across them, and it takes radios of those networks on its
stations (visitors, extension `mni`), and carries individual calls and SDS
across networks.

> **Status:** tested on air with TetraPack (FlowStation, Motorola MTP6650,
> 2026-11-07): links to other networks, bridge pools, individual (duplex)
> calls and SDS across networks, visitors (radios of other networks on a
> station, `mni`) and a radio of this network migrated onto a cell of the
> linked network, in that network's groups, and a visitor from a third
> network (901/1234) migrating onto a station.

## How it works

- For each other network this server opens **one outbound connection in
  plain Basestation mode**, with the Basestation login that network gave out.
  The far server needs nothing new: to it this server is one more
  Basestation.
- The connection introduces itself with a **free connection name**
  (`User-Agent`, e.g. `TMO-Austria Infra 1.0`). Empty: `brew-server/<version>`.
- **Credentials stay on this server.** Stations never get them.
- What crosses is set by the **talkgroup pools**: a pool may list groups of
  other networks next to this server's own groups. Such a pool is a bridge: a
  call on one member is played on the others, each as an ordinary group call
  on its own GSSI. GSSIs are translated (TG 9 here, TG 909 there); the
  talker's ISSI is passed on 1:1.
- A **bridge ISSI** (one ISSI of that network) is registered there and
  affiliated to the bridged groups, so that network's server sends their
  calls here. With `bridge_issi = 0` nothing is registered (only for a server
  that routes groups without affiliations).
- **Radios are registered there** (`register_talkers`, default on), as a
  Basestation does for its radios (a server such as TetraPack does not show
  or route a talker it does not know): a radio affiliated here to a group of
  a switched-on bridge pool is registered in that network and affiliated to
  the pool's group there (TG 9 here -> TG 909 there). When it leaves the
  group here (or deregisters, or its station goes), it leaves there and is
  deregistered, except while it is talking. A talker not known that way
  (e.g. from a peer server) is registered before its first over. All of
  them are deregistered when the link is stopped. Switch it off for a link
  to this same server (a loop test): there the registration would move the
  radio off its own station.
- **Register all** (`register_all`, off by default, needs `register_talkers`):
  also every radio of a station with Multi Network (a Basestation announcing
  the `mni` capability, see step 4) or named in `export_stations` (station
  logins) is registered there, in a bridged group or not. An app (Terminal
  mode) is exported only when `export_stations` names its login or its ISSI;
  radios from peers or other networks never. An app not named is registered
  there only while it is in a bridged group or making a call over there
  itself, so a radio of that network reaches it by individual call or SDS
  only then (field check 2026-11-07).
- **ISSI collisions**: a radio is registered there only if that network may
  have its ISSI. A brew-server there (it offers `issi-check`) is asked first
  (`SERVICE_ISSI_CHECK`, docs/Brew-Extensions.md 12a); a taken ISSI is not
  registered and shows up as a conflict. Another server cannot be asked:
  `never_export` lists ISSIs that are never registered there, and when a
  radio there talks with an ISSI this server has registered there, that
  shows up as a conflict too (dashboard: "ISSI conflicts", with a "Never
  export" button).
- **Nothing else crosses**, in either direction: no other registrations, no
  services, no calls on groups that are not bridged. A station cannot send
  into another network by itself.
- A **main switch** (`enabled`, on the Multi Network page next to "This
  server") closes all links at once; settings and pools stay.
- Calls and SDS that cross a link carry their network on the dashboard:
  "→ name" for the network they went to, "← name" for the one they came
  from (live calls, Recent calls, Recent SDS; a call shows the networks of
  its pool legs too). The JSON of `/api/status` has `to_networks` and
  `from_network` on those entries. Kept in memory only, so the history file
  stays readable by older versions; after a restart older entries show no
  network.
- Networks are edited live (dashboard page **Multi Network**, admins only).
  Only a network whose settings changed reconnects; no restart. An empty
  password on save keeps the saved one.

## Visitors (extension `mni`)

- A station with Multi Network (it announces the `mni` capability; FlowStation
  from Multi Network step 7) gets `SERVICE_NETWORKS`: this server's MNI and
  the linked networks with their state, never credentials. It takes a radio
  of such a network by migration (TS 100 392-2 clause 16.4.1.1 b)) and says
  so with `SERVICE_MS_NETWORK` before the radio's `SUB_REGISTER`.
- Such a visitor is kept apart from this server's radios (key: network and
  ISSI), so the same ISSI in both networks works. This server registers it
  in its home network over the link and forwards its group affiliations
  there, so its home network sends the calls of its groups here.
- A group call of the home network on such a group reaches the stations with
  a visitor in it, as that network's GSSI with `SERVICE_CALL_NETWORK` before
  it. A visitor talking on a home group (station sends `SERVICE_CALL_NETWORK`)
  goes to its home network, to the other visitors in the group and into a
  talkgroup pool that bridges the group.
- Moving between this server's stations is invisible to the home network.
  Deregistration, or the station going away, deregisters it at home. When a
  link comes back, every visitor is registered there again.
- A radio a station names first with another network and then with this
  server's network (it registered there by ITSI attach, then migrated here)
  leaves the other network: the server deregisters it there.
- A station whose announced cells (`SERVICE_BTS_INFO`) all broadcast another
  network's MNI and that has the `mni` capability (a cell of TetraPack served
  through this server) is a station of that network: this server's own group
  calls are not broadcast to it (`route_without_affiliations` or the
  no-affiliation fallback). It gets a group of this network only where a radio
  of this network that migrated onto it is affiliated. So a group call from a
  station or terminal without the extension stays in this network. Stations
  without `mni` or without cells announced, and servers without their own MNI
  set, are routed as before.
- The dashboard lists the visitors per network ("Visitors" on the Multi
  Network page).
- A radio of this server's network migrated onto a cell of a linked network
  that attaches a group there without an MNI is in that network's group (TS
  100 392-2 clause 14.2.3). The station names it with `SERVICE_GROUP_NETWORK`
  before the affiliation; the server keeps the affiliation as that network's
  group (not this server's group of the same number), registers and
  affiliates the radio there, and that network's calls on the group reach the
  station (with `SERVICE_CALL_NETWORK`; the cell plays them on the GSSI).
- Not yet: a visitor in a group of a third network (`SERVICE_GROUP_NETWORK`
  is logged only).

## Individual calls and SDS across networks

Delivered here first; to another network only when the target is known
there or carries another network's MNI.

| Target | Goes to |
| --- | --- |
| TSI of another network (the station names the called network, `SERVICE_CALL_NETWORK` `called`) | a visitor of that network here; one of this server's radios the link registered there: its station here; else over the link to that network (refused while it is down); the caller is registered there first like a talker |
| ISSI without a network, registered here | here, as before (a visitor dialling one means this network) |
| ISSI without a network, not registered here | as before (SIP, SMS Center, refused); with `forward_unknown` on a network: a visitor of that network here with this ISSI, else once to that network |
| from network X to a radio of ours the link registered there | its station |
| from network X to a visitor from X | its station, with `SERVICE_CALL_NETWORK` (both parties' networks) before it |
| from network X to anything else | refused |

`forward_unknown` (per network, off by default) never sends back over the
link a call came in on, and the same calling / called pair a second time
within 5 s counts as a loop and is refused. Such an SDS is not stored by the
SMS Center. A party that is here (a visitor, or one of this server's radios
registered there) is never sent round the link: the far server would send
it back over the same connection, which a Brew server does not do.

A visitor that dials a bare ISSI reaches a party of its own network (the
cell's network, TS 100 392-2 clause 14.2.3); to reach a radio or the app of
this server's network it has to dial the full TSI.

Switching networks by cell: with one radio and one home identity, the cell
it uses decides the network it talks in (field check 2026-11-07, home
901/9999, this server linked to TetraPack): on a 901/9999 cell it is this
server's radio; on a 901/1638 cell it migrates and its groups are
TetraPack / Brandmeister groups over the link (or it is a TetraPack radio
there with ITSI attach). Bridge pools work in both cases.
- `mni` is offered only while Multi Network is on and has networks.

The protocol is in [Brew-Extensions.md](Brew-Extensions.md) section 12b.

## Config

```toml
[multi_network]
enabled = true            # main switch: false closes every link (default on)
name = "TMO-Austria"      # this server's network, for the dashboard
mcc = 901                 # its MNI (MCC 0..1023, MNC 0..16383), 0 = not set
mnc = 9999

[[multi_network.networks]]
name = "TetraPack"        # pools refer to the network by this name
enabled = true
mcc = 901
mnc = 9000
remote_host = "brew.example.net:443"
path = "/brew/"           # TetraPack: /brew/ (trailing slash matters)
username = "1234567"      # Basestation login there
password = "secret"
user_agent = "TMO-Austria Infra 1.0"
bridge_issi = 9000001     # ISSI of the bridge in that network, 0 = none
register_talkers = true   # register radios in bridged groups there
register_all = false      # also all radios of mni stations / export_stations
export_stations = ["bs-wien", "2320997"]   # station logins for register_all; apps by login or ISSI
never_export = [2320002]  # ISSIs that network has itself
forward_unknown = false   # individual calls / SDS nobody here takes go there
reconnect_interval_seconds = 15
tls = true                # wss://, settings as for [[federation.peers]]
# tls_ca_path, tls_server_name, tls_pinned_cert_path

[[talkgroup_pools]]
name = "Bridge TP"
groups = [9]              # this server's groups
[[talkgroup_pools.remote]]
network = "TetraPack"     # groups of other networks
gssi = 909
```

On the dashboard the pool editor (Routing page) takes these groups as
`network:GSSI`, e.g. `TetraPack:909`.

With `both_ways = false` the first member is the source: the first of
`groups`, or the first `remote` group when `groups` is empty (for example to
listen to a group of another network on several groups here).

## Behaviour

- **Answers cross too:** a radio that answers in a call it hears (a new
  over on that call, as Brew stations send it) is heard on every group of
  the pool, on both networks. When the station of the last talker ends its
  call, the call ends everywhere, as with any Brew group call.
- A call crossing a link gets its own call identifier on the other side, so
  a copy that comes back (e.g. a test with a server linked to itself) is
  never mixed up with the call it came from.
- A group call from here that the other network ends at once (TetraPack
  answers some overs with GROUP_IDLE) keeps its identifiers until this
  side's own end has gone over, so the rest of the over and the end still
  reach that network instead of leaving the call hanging there.
- The pool rules apply unchanged (one talker per group, a group with its own
  call joins after it, legs end with the call).
- The discovery path is used exactly as configured (TetraPack needs
  `/brew/`; `/brew` is redirected to its web page there). A redirect on the
  same server is followed; an answer that is not a WebSocket path is refused
  with a hint.
- The keepalive of the links is `[federation] keepalive_interval_seconds` /
  `keepalive_timeout_seconds`.
- A network that is used by a pool cannot be deleted or renamed; remove its
  groups from the pools first.
- Stations announce the `mni` capability only from step 7 on (FlowStation);
  until then "register all" exports the stations listed in `export_stations`.

## Standards

Brew is not an ETSI interface, and the ETSI inter-system interface (ISI,
EN 300 392-3) is not used: none of this is defined by ETSI. The only ETSI
values used are the identity sizes: GSSI 24 bits (TS 100 392-2 V3.10.1 clause
16.10.28, Table 16.60), MCC 10 bits and MNC 14 bits (clause 18.4.2.1, Table
18.17). Inside the server a group of another network is addressed by an alias
above the 24 GSSI bits (`slot << 24 | gssi`), so it can never collide with a
real group; stations never see it.
