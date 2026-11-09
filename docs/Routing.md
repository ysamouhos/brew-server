# Routing: DGNA and talkgroup pools

The **Routing** page of the dashboard (new UI, sidebar **Infrastructure -> Routing**)
does two things:

- assign a group to several radios at once with DGNA, and
- connect talkgroups inside the server (talkgroup pools).

Everyone logged in sees the page. Changing anything needs an admin login.

## DGNA to several radios

The table lists every radio registered on this server: ISSI, station and
cell, the groups Brew knows for it, the groups the station reported (with
names) and the last DGNA answer.

1. Tick the radios (or the box in the header for all of them).
2. Enter the **GSSI** and, if you like, a **group name** (up to 15
   characters; the radio shows it).
3. Choose **Assign** or **Remove**, and the attachment mode (0 to 5; leave it
   at 0 unless you need another one).
4. Press **Send**.

Each radio's command goes over the control channel to the Basestation it
registered on last. That needs:

- the Basestation's `[telemetry]` link, so the server knows which station the
  radio is on, and
- its `[control]` link, which carries the command.

With only one control connection the command goes there. A radio that cannot
be reached shows the reason right away; the radio's own answer arrives over
telemetry and shows in the **Last DGNA** column (for example "assigned 2329"
or "rejected 2329" with the station's reason).

DGNA reaches radios only. App and console terminals are listed but cannot be
selected.

## Talkgroup pools

A pool connects talkgroups: a call on one of them is played 1:1 on the others,
with voice and the end of each over.

```toml
[[talkgroup_pools]]
name = "Region East"
enabled = true
both_ways = true              # false: only calls on the first group are copied
groups = [2321, 2323, 2324, 2329, 264]
```

- **Name:** shown on the page and on the calls. Up to 40 characters, unique.
- **Groups:** at least two, each 1 to 16777215, no group twice.
- **Direction:** `both_ways = true` copies a call on any of the groups to all
  the others. `both_ways = false` copies only calls on the **first** group to
  the others; calls on the others stay on their own group.
- **On/off:** `enabled = false` keeps the pool but does nothing.

On the page every pool has a switch, **Edit** and **Delete**; **New pool**
adds one. A change applies at once and is written to the config file without
a restart. Calls that are already running keep their copies until they end.

### Behaviour

- The copy of a call ("leg") uses its own call identifier on each target
  group, with the talker's ISSI and priority. Stations on a target group see
  an ordinary group call.
- A target group that already has its own call is not interrupted. The pool
  waits until that over ends and joins it then; the page shows the group as
  "waiting".
- A leg never starts further legs, so pools that share a group do not loop.
  A group in two pools gets the copy once.
- When the talker's over ends, all its legs end too.
- Pool legs are not sent to federation peers; the original call goes there as
  before.
- Without `[[talkgroup_pools]]` nothing changes: the server routes as it did.
- A pool can also hold groups of other Brew networks (`remote`): a bridge
  across networks. See [Multi-Network.md](Multi-Network.md).

## Choosing the dashboard

The redesigned dashboard is the default. To keep the classic one at `/`:

```toml
[dashboard]
legacy_ui = true
```

Both stay reachable either way: the classic dashboard under `/classic`, the
new one under `/ui`.

## Logins and the config file

Two more pages of the new dashboard, both for admins:

- **Infrastructure -> Accounts:** the logins Basestations and terminals connect with.
  One login can be on Brew (the station's or terminal's number, 1 to 7
  digits), telemetry and control at once. Changes apply to the next
  connection without a restart and are written to the config file;
  connections already up stay until they reconnect. Adding the first
  telemetry or control login turns the login on for that listener: stations
  without one are refused from then on. Each login can have an info text
  (sysop, relay callsign) and a position; both are the login's
  `[bts_locations]` entry, so the position is the map's static fallback.
  The Login / Info switch shows the info in place of the number.
- **Monitor -> Directory:** names for radios (ISSI) and groups (GSSI), shown
  next to the number on every page; Radios / Groups switches the list. A
  name a radio reports for one of its groups comes before the directory's. Add one by one or import a pasted list, one ISSI and name per
  line (`2320972, OE4SXM Sebastian`; `;`, tab or space work too, `#` lines
  and a header line are skipped). A wrong line stops the import and nothing
  changes. Stored in the config file:

  ```toml
  [issi_names]
  "2320972" = "OE4SXM Sebastian"

  [gssi_names]
  "2321" = "District North"
  ```

  **Access** (Groups list) decides who may use a group: *Open* (no list),
  *Only these* (allow list: only the listed ISSIs) or *Block these* (everyone
  but the listed ISSIs). One list per group, the switch decides what it
  means. The server records an affiliation only for an admitted radio and
  drops a group call from any other; emergency calls always pass. A radio
  that is not admitted therefore neither talks on the group across the
  network nor pulls the group's calls to its station, and a pool does not
  copy its calls to that group. Changing the list drops the affiliations it
  no longer admits; opening it again takes effect when the radios affiliate
  again.

  **RX only** makes radios listeners on the group: with the list empty every
  radio, otherwise only the listed ISSIs. A listener is admitted (it may
  attach and hears the calls) but the server drops its group calls, so it
  gets no floor. Talking needs both: admitted by Access and not listen-only.

  A Basestation that announces the `group-access` capability (FlowStation)
  gets the lists live and refuses on the air itself: the attachment of a
  group the radio may not use, the call set-up and the request to talk of a
  radio that may not talk (see [Brew-Extensions.md](Brew-Extensions.md),
  section 12). On any other Basestation the radio can still attach and talk
  locally (base Brew has no message to refuse it), and it hears a call that
  reaches its station for other members.

  ```toml
  [group_access."2321"]
  mode = "allow"            # or "block"
  issis = [2320972, 2321100]
  rx_only = true            # optional
  rx_only_issis = [2321100] # optional, empty = everyone listens only
  ```

  Unnamed groups radios are on can be hidden from the Directory with ✕
  (display only, filter "Hidden", `[talkgroups] hidden`).
- **Managed talkgroups** (Routing page, a card that folds to a thin bar:
  folded while off, open while on, foldable either way): a closed set of
  groups. Switched on, radios may only use allowed groups: the ones in the
  card's list, every group of a talkgroup pool or linked talkgroup, and
  every group with the access *Only these* (only its ISSIs may use it
  anyway); these are listed as *automatic*. Each row shows the access
  badges of the group (*Only N ISSIs*, *N blocked*, *RX only*), and *Edit*
  (group with rules) or *Rules* (open group) opens the same dialog as on the
  Directory page. The server records no affiliation to any other
  group and drops its group calls (emergency calls pass); radios already on
  such a group are dropped from it, and a FlowStation with `group-access`
  refuses the attachment on the air. Switched off, every group is open and
  the list is kept, so it can be prepared first. A group is allowed by GSSI
  with an optional name (written to the Directory), or with *+ Allow* from
  *Saved TGs on server*: the groups the server knows (a Directory name or
  access entry, in use now, or refused since the start) that are not allowed
  yet. A Directory name, a block list or RX only alone does not allow a
  group.

  ```toml
  [talkgroups]
  managed = true
  allowed = [9, 232, 2321]
  hidden = [4000]   # hidden from the Directory page
  ```
- **System -> Config file:** the whole config file. Save checks it and
  applies it with a restart (running calls drop, stations reconnect, you sign
  in again).
