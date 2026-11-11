# Dashboard users

Who may open the dashboard, which pages each person sees, and who may change
things. Managed on **Server settings -> Dashboard users** (bottom of the page).

## Roles

| Role | Sees | May change |
| --- | --- | --- |
| **Admin** | every page | everything (settings, routing, accounts, users, control) |
| **User** | the pages ticked for them | nothing, only their own password |
| **Viewer** | the pages ticked for Viewer | nothing |

- There can be any number of admins and users. At least one admin always
  stays: the last admin cannot be deleted or turned into a user.
- Turning on **Admin** in the edit dialog ticks every page. An admin always
  sees everything, also after the recovery steps below.
- **Viewer** is everyone who is not logged in. Its ticks decide which pages an
  open dashboard (`[dashboard.ui] open = true`) shows without a login. With a
  closed dashboard (the default) nobody sees anything without logging in.
- Multi Network, Control, Config and Server settings are for admins only and
  cannot be ticked.
- Pages a person may not see are left out of the menu.

Saving and editing are for admins only. When someone who is not an admin
tries it (not logged in, or logged in as a user that only reads), the login
window opens with "For saving or editing please log in with an admin user."

## Station names and addresses

Station logins (Brew account names) are shown to everyone who sees a page
with stations. IP addresses and ports (stations, SIP peers, HA nodes) are
shown to admins only; for everyone else the server leaves them out of its
answers.

## Passwords

- Everyone logged in changes their own password in the menu at the top right
  (**Change password**). The current password is asked for. The other
  browsers signed in with the same user are signed out; this one stays.
- An admin sets another user's password in that user's edit dialog (**New
  password**, empty keeps it). That user's sessions end.
- Passwords need at least 6 characters. Only Argon2 hashes are stored.

## Where users are stored

`[dashboard] users_file` (default `dashboard-users.json`, next to the
server's working directory; in Docker `/var/lib/brew-server`). Readable by the
server's user only (mode 600).

The `[dashboard.users]` and `admins` entries of the config are the users of
a new installation. They are used until the first change on the dashboard;
from then on the users file is used and those config entries are ignored.

```toml
[dashboard]
admins = ["admin"]
users_file = "dashboard-users.json"

[dashboard.users]
"admin" = "change-me-admin"
```

Without any user at all there is no login and everyone is an admin (the old
behaviour); the first user created on the dashboard has to be an admin.

## Locked out: the way back in

If no admin password is known any more, delete the users file and restart.
The users of the config are back, with the passwords written there.

Docker:

```bash
docker exec brew-server rm /var/lib/brew-server/dashboard-users.json
docker restart brew-server
```

Without Docker, delete `dashboard-users.json` in the server's working
directory (or the path in `users_file`) and restart the service.

Users, pages and Viewer ticks made on the dashboard are lost with the file;
create them again afterwards.

## Authentication of stations

**Accounts -> Authentication required** (top of the page, on by default) is
`[auth] enabled`: stations and apps need a Brew account to connect. Switching
it off asks first: any station or app can then connect without a login, and
stations show up without their name. It applies at once and is written to the
config file; no restart.

## API

| Route | Who | |
| --- | --- | --- |
| `GET /api/whoami` | everyone | user, `admin`, `pages`, `allowed` (read routes) |
| `POST /api/password` | logged in | `{"current","new"}`, own password |
| `GET /api/dashboard-users` | admin | users, Viewer pages, file |
| `POST /api/dashboard-users` | admin | `{"name","password","admin","pages"}`, add or change |
| `DELETE /api/dashboard-users/{name}` | admin | remove |
| `POST /api/dashboard-users/viewer` | admin | `{"pages"}` |
| `PUT /api/accounts/auth` | admin | `{"enabled"}`, station authentication |
