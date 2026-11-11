//! Dashboard users: who may log in, who is an admin and which pages each one
//! sees (Server settings -> Users).
//!
//! An admin sees every page and may change everything. Any other user only
//! sees the pages ticked for it. "Viewer" is the visitor that is not logged
//! in: with `[dashboard.ui] open = true` it sees the pages ticked for Viewer;
//! with a closed dashboard it sees nothing.
//!
//! The users come from `[dashboard] users` / `admins` until the first change
//! in the dashboard writes `[dashboard] users_file` (JSON, passwords as
//! argon2 hashes). From then on that file is used. Deleting it and restarting
//! brings the users of the config back (the way in again after a lost admin
//! password). Without any user the dashboard has no login and everyone is an
//! admin, as before.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
};

/// Pages that can be ticked for a user or Viewer (the dashboard's page ids).
/// Multi Network, Control, Config file and Server settings are for admins only.
pub const PAGES: &[&str] = &[
    "overview", "calls", "sds", "radios", "issis", "map", "stations", "routing", "roaming", "telemetry",
    "accounts", "sip", "sipcfg", "sms", "tsds", "ha",
];

/// What Viewer and users taken over from the config see: every page that
/// used to be open to anyone who is not an admin.
pub fn default_pages() -> BTreeSet<String> {
    PAGES.iter().filter(|p| **p != "accounts").map(|p| p.to_string()).collect()
}

fn pages_of(list: &[String]) -> BTreeSet<String> {
    list.iter().filter(|p| PAGES.contains(&p.as_str())).cloned().collect()
}

#[derive(Debug, Clone, PartialEq)]
enum Secret {
    /// From `[dashboard] users` (hashed when the file is first written).
    Plain(String),
    /// Argon2 PHC string.
    Hash(String),
}

#[derive(Debug, Clone)]
struct User {
    secret: Secret,
    admin: bool,
    pages: BTreeSet<String>,
}

#[derive(Debug, Default)]
struct Data {
    users: BTreeMap<String, User>,
    viewer: BTreeSet<String>,
    from_file: bool,
}

/// The users file.
#[derive(Serialize, Deserialize)]
struct FileUser {
    password_hash: String,
    admin: bool,
    #[serde(default)]
    pages: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct File {
    users: BTreeMap<String, FileUser>,
    viewer: Vec<String>,
}

/// One user as the dashboard lists it (no password).
#[derive(Debug, Serialize, PartialEq)]
pub struct UserView {
    pub name: String,
    pub admin: bool,
    pub pages: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct UsersView {
    pub users: Vec<UserView>,
    pub viewer: Vec<String>,
    pub pages: &'static [&'static str],
    /// The users come from the users file (else from the config).
    pub from_file: bool,
    pub file: String,
}

/// What a request may do.
#[derive(Debug, Clone, PartialEq)]
pub struct Access {
    /// The logged-in user (`None`: Viewer).
    pub user: Option<String>,
    pub admin: bool,
    /// Pages it sees (an admin: all of them).
    pub pages: BTreeSet<String>,
    /// There are dashboard users (else no login, everyone is an admin).
    pub auth: bool,
}

impl Access {
    pub fn sees(&self, page: &str) -> bool {
        self.admin || self.pages.contains(page)
    }
}

pub struct DashUsers {
    path: PathBuf,
    data: RwLock<Data>,
    /// Credentials checked already (digest of user and password), so HTTP
    /// Basic requests do not run argon2 every time. Cleared on any change.
    ok: Mutex<HashMap<Vec<u8>, String>>,
}

const MAX_CACHE: usize = 64;

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-@".contains(c))
}

fn hash(password: &str) -> Result<String, String> {
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).map_err(|e| e.to_string())?;
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Secret {
    fn matches(&self, password: &str) -> bool {
        match self {
            Secret::Plain(p) => same(p.as_bytes(), password.as_bytes()),
            Secret::Hash(h) => PasswordHash::new(h).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok()),
        }
    }
}

impl DashUsers {
    /// Loads the users file, or takes the users of the config.
    pub fn open(cfg: &crate::config::DashboardConfig) -> Self {
        let path = cfg.users_file.clone();
        let data = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(f) => {
                    tracing::info!(path = %path.display(), users = f.users.len(), "dashboard users loaded");
                    Data {
                        users: f.users.into_iter().map(|(n, u)| (n, User { secret: Secret::Hash(u.password_hash), admin: u.admin, pages: pages_of(&u.pages) })).collect(),
                        viewer: pages_of(&f.viewer),
                        from_file: true,
                    }
                }
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "dashboard users file unreadable; using [dashboard] users");
                    from_config(cfg)
                }
            },
            Err(_) => from_config(cfg),
        };
        Self { path, data: RwLock::new(data), ok: Mutex::new(HashMap::new()) }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Data> {
        self.data.read().unwrap_or_else(|e| e.into_inner())
    }

    /// There are dashboard users (a login exists).
    pub fn enabled(&self) -> bool {
        !self.read().users.is_empty()
    }

    pub fn exists(&self, user: &str) -> bool {
        self.read().users.contains_key(user)
    }

    /// Checks a user's password.
    pub fn check(&self, user: &str, password: &str) -> bool {
        let key = Blake2b512::new().chain_update(user.as_bytes()).chain_update([0]).chain_update(password.as_bytes()).finalize().to_vec();
        if self.ok.lock().unwrap_or_else(|e| e.into_inner()).get(&key).is_some_and(|u| u == user) {
            return true;
        }
        let secret = self.read().users.get(user).map(|u| u.secret.clone());
        let ok = secret.is_some_and(|s| s.matches(password));
        if ok {
            let mut c = self.ok.lock().unwrap_or_else(|e| e.into_inner());
            if c.len() >= MAX_CACHE { c.clear(); }
            c.insert(key, user.to_owned());
        }
        ok
    }

    /// What `user` (`None`: not logged in) may do.
    pub fn access(&self, user: Option<&str>) -> Access {
        let d = self.read();
        if d.users.is_empty() {
            return Access { user: None, admin: true, pages: PAGES.iter().map(|p| p.to_string()).collect(), auth: false };
        }
        match user.and_then(|n| d.users.get(n).map(|u| (n, u))) {
            Some((n, u)) if u.admin => Access { user: Some(n.to_owned()), admin: true, pages: PAGES.iter().map(|p| p.to_string()).collect(), auth: true },
            Some((n, u)) => Access { user: Some(n.to_owned()), admin: false, pages: u.pages.clone(), auth: true },
            None => Access { user: None, admin: false, pages: d.viewer.clone(), auth: true },
        }
    }

    pub fn view(&self) -> UsersView {
        let d = self.read();
        UsersView {
            users: d.users.iter().map(|(n, u)| UserView { name: n.clone(), admin: u.admin, pages: u.pages.iter().cloned().collect() }).collect(),
            viewer: d.viewer.iter().cloned().collect(),
            pages: PAGES,
            from_file: d.from_file,
            file: self.path.display().to_string(),
        }
    }

    /// Adds a user or changes one. `password` empty keeps the current one
    /// (a new user needs one). At least one admin stays.
    pub fn upsert(&self, name: &str, password: &str, admin: bool, pages: &[String]) -> Result<(), String> {
        if !valid_name(name) {
            return Err("User names are 1 to 32 letters, digits or . _ - @".into());
        }
        if !password.is_empty() && password.chars().count() < 6 {
            return Err("Passwords need at least 6 characters".into());
        }
        let mut d = self.data.write().unwrap_or_else(|e| e.into_inner());
        let secret = match (password.is_empty(), d.users.get(name)) {
            (true, Some(u)) => u.secret.clone(),
            (true, None) => return Err("A new user needs a password".into()),
            (false, _) => Secret::Hash(hash(password)?),
        };
        if !admin && !d.users.iter().any(|(n, u)| u.admin && n != name) {
            return Err(if d.users.is_empty() { "The first user has to be an admin".into() } else { "At least one admin has to stay".into() });
        }
        let pages = if admin { BTreeSet::new() } else { pages_of(pages) };
        let mut next = clone_data(&d);
        next.users.insert(name.to_owned(), User { secret, admin, pages });
        self.save(&mut next)?;
        *d = next;
        drop(d);
        self.ok.lock().unwrap_or_else(|e| e.into_inner()).clear();
        tracing::warn!(user = name, admin, "dashboard user changed");
        Ok(())
    }

    /// Removes a user (not the last admin).
    pub fn remove(&self, name: &str) -> Result<(), String> {
        let mut d = self.data.write().unwrap_or_else(|e| e.into_inner());
        let Some(u) = d.users.get(name) else { return Err("No such user".into()) };
        if u.admin && !d.users.iter().any(|(n, o)| o.admin && n != name) {
            return Err("At least one admin has to stay".into());
        }
        let mut next = clone_data(&d);
        next.users.remove(name);
        self.save(&mut next)?;
        *d = next;
        drop(d);
        self.ok.lock().unwrap_or_else(|e| e.into_inner()).clear();
        tracing::warn!(user = name, "dashboard user removed");
        Ok(())
    }

    /// A user changes their own password (the current one is checked).
    pub fn change_password(&self, name: &str, current: &str, new: &str) -> Result<(), String> {
        if !self.check(name, current) {
            return Err("The current password is wrong".into());
        }
        if new.chars().count() < 6 {
            return Err("Passwords need at least 6 characters".into());
        }
        let secret = Secret::Hash(hash(new)?);
        let mut d = self.data.write().unwrap_or_else(|e| e.into_inner());
        if !d.users.contains_key(name) {
            return Err("No such user".into());
        }
        let mut next = clone_data(&d);
        if let Some(u) = next.users.get_mut(name) {
            u.secret = secret;
        }
        self.save(&mut next)?;
        *d = next;
        drop(d);
        self.ok.lock().unwrap_or_else(|e| e.into_inner()).clear();
        tracing::warn!(user = name, "dashboard user changed their password");
        Ok(())
    }

    /// Sets the pages Viewer sees.
    pub fn set_viewer(&self, pages: &[String]) -> Result<(), String> {
        let mut d = self.data.write().unwrap_or_else(|e| e.into_inner());
        let mut next = clone_data(&d);
        next.viewer = pages_of(pages);
        self.save(&mut next)?;
        *d = next;
        tracing::warn!(pages = ?d.viewer, "dashboard viewer pages changed");
        Ok(())
    }

    /// Writes the users file (passwords taken from the config get hashed now).
    fn save(&self, d: &mut Data) -> Result<(), String> {
        for u in d.users.values_mut() {
            if let Secret::Plain(p) = &u.secret {
                u.secret = Secret::Hash(hash(p)?);
            }
        }
        let file = File {
            users: d.users.iter().map(|(n, u)| (n.clone(), FileUser {
                password_hash: match &u.secret { Secret::Hash(h) => h.clone(), Secret::Plain(_) => String::new() },
                admin: u.admin,
                pages: u.pages.iter().cloned().collect(),
            })).collect(),
            viewer: d.viewer.iter().cloned().collect(),
        };
        write_file(&self.path, &serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?)
            .map_err(|e| format!("cannot write {}: {e}", self.path.display()))?;
        d.from_file = true;
        Ok(())
    }
}

fn clone_data(d: &Data) -> Data {
    Data { users: d.users.clone(), viewer: d.viewer.clone(), from_file: d.from_file }
}

fn from_config(cfg: &crate::config::DashboardConfig) -> Data {
    Data {
        users: cfg.users.iter().map(|(n, p)| (n.clone(), User {
            secret: Secret::Plain(p.clone()),
            admin: cfg.admins.is_empty() || cfg.admins.contains(n),
            pages: default_pages(),
        })).collect(),
        viewer: default_pages(),
        from_file: false,
    }
}

/// Writes atomically (temporary file, then rename), readable by the owner only.
fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

#[derive(Deserialize)]
pub struct UpsertRequest {
    pub name: String,
    /// Empty keeps the current password.
    #[serde(default)]
    pub password: String,
    pub admin: bool,
    #[serde(default)]
    pub pages: Vec<String>,
}

#[derive(Deserialize)]
pub struct PasswordRequest {
    pub current: String,
    pub new: String,
}

#[derive(Deserialize)]
pub struct ViewerRequest {
    pub pages: Vec<String>,
}

type St = axum::extract::State<std::sync::Arc<crate::state::AppState>>;

fn answer(state: &crate::state::AppState, r: Result<(), String>) -> axum::response::Response {
    use axum::response::IntoResponse;
    match r {
        Ok(()) => axum::Json(state.dash_users.view()).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, e).into_response(),
    }
}

/// `GET /api/dashboard-users` (admins).
pub async fn list(axum::extract::State(state): St) -> axum::Json<UsersView> {
    axum::Json(state.dash_users.view())
}

/// `POST /api/dashboard-users` (admins): adds or changes a user. A new
/// password signs the user out of their other sessions.
pub async fn upsert(axum::extract::State(state): St, headers: axum::http::HeaderMap, axum::Json(req): axum::Json<UpsertRequest>) -> axum::response::Response {
    let s = state.clone();
    let name = req.name.trim().to_owned();
    let new_password = !req.password.is_empty();
    let n = name.clone();
    let r = tokio::task::spawn_blocking(move || s.dash_users.upsert(&n, &req.password, req.admin, &req.pages))
        .await.unwrap_or_else(|e| Err(e.to_string()));
    if r.is_ok() && new_password {
        crate::dashboard_auth::end_sessions(&name, &headers);
    }
    answer(&state, r)
}

/// `POST /api/password` (every signed-in user): changes the own password.
/// The user's other sessions end; this one stays.
pub async fn change_password(axum::extract::State(state): St, headers: axum::http::HeaderMap, axum::Json(req): axum::Json<PasswordRequest>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(user) = crate::dashboard::access(&state, &headers).user else {
        return (axum::http::StatusCode::UNAUTHORIZED, "login required").into_response();
    };
    let s = state.clone();
    let u = user.clone();
    let r = tokio::task::spawn_blocking(move || s.dash_users.change_password(&u, &req.current, &req.new))
        .await.unwrap_or_else(|e| Err(e.to_string()));
    match r {
        Ok(()) => {
            crate::dashboard_auth::end_sessions(&user, &headers);
            axum::http::StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, e).into_response(),
    }
}

/// `DELETE /api/dashboard-users/{name}` (admins).
pub async fn remove(axum::extract::State(state): St, axum::extract::Path(name): axum::extract::Path<String>) -> axum::response::Response {
    let r = state.dash_users.remove(&name);
    answer(&state, r)
}

/// `POST /api/dashboard-users/viewer` (admins): the pages Viewer sees.
pub async fn viewer(axum::extract::State(state): St, axum::Json(req): axum::Json<ViewerRequest>) -> axum::response::Response {
    let s = state.clone();
    let r = tokio::task::spawn_blocking(move || s.dash_users.set_viewer(&req.pages)).await.unwrap_or_else(|e| Err(e.to_string()));
    answer(&state, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(file: &Path) -> crate::config::DashboardConfig {
        let mut c = crate::config::DashboardConfig::default();
        c.users.insert("admin".into(), "secret1".into());
        c.users.insert("bob".into(), "secret2".into());
        c.admins = vec!["admin".into()];
        c.users_file = file.to_path_buf();
        c
    }

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!("brew-dash-users-{}.json", uuid::Uuid::new_v4().simple()))
    }

    #[test]
    fn config_users_until_the_first_change_then_the_file() {
        let path = tmp();
        let u = DashUsers::open(&cfg(&path));
        assert!(u.enabled() && !path.exists(), "nothing written before a change");
        assert!(u.check("admin", "secret1") && !u.check("admin", "nope") && !u.check("eve", "secret1"));
        assert!(u.access(Some("admin")).admin);
        let bob = u.access(Some("bob"));
        assert!(!bob.admin && bob.sees("calls") && !bob.sees("accounts"), "config users see what the open dashboard showed");
        assert_eq!(u.access(None), Access { user: None, admin: false, pages: default_pages(), auth: true });

        // Two admins, a viewer that only sees the map.
        u.upsert("carol", "carolpw", true, &[]).unwrap();
        u.upsert("bob", "", false, &["calls".into(), "settings".into(), "nonsense".into()]).unwrap();
        u.set_viewer(&["map".into()]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("secret1") && !text.contains("carolpw") && text.contains("$argon2"), "only hashes in the file");
        assert_eq!(u.access(Some("bob")).pages, BTreeSet::from(["calls".to_string()]), "admin pages cannot be ticked");
        assert_eq!(u.access(None).pages, BTreeSet::from(["map".to_string()]));

        // A restart reads the file, not the config.
        let mut c = cfg(&path);
        c.users.insert("admin".into(), "changed-in-config".into());
        let u = DashUsers::open(&c);
        assert!(u.view().from_file && u.check("admin", "secret1") && u.check("carol", "carolpw") && u.check("bob", "secret2"));
        assert!(!u.check("admin", "changed-in-config"));
        assert!(u.access(Some("carol")).admin);

        // The way back in: delete the file, restart, the config users are back.
        std::fs::remove_file(&path).unwrap();
        let u = DashUsers::open(&c);
        assert!(!u.view().from_file && u.check("admin", "changed-in-config") && u.access(Some("admin")).admin);
        assert!(!u.exists("carol"));
    }

    #[test]
    fn at_least_one_admin_stays() {
        let path = tmp();
        let u = DashUsers::open(&cfg(&path));
        assert!(u.remove("admin").is_err(), "the only admin");
        assert!(u.upsert("admin", "", false, &[]).is_err(), "nor downgraded");
        u.upsert("second", "secondpw", true, &[]).unwrap();
        u.upsert("admin", "", false, &["map".into()]).unwrap();
        assert!(u.remove("second").is_err(), "now second is the only admin");
        u.remove("bob").unwrap();
        assert!(!u.check("bob", "secret2"), "removed user cannot log in");
        assert!(u.upsert("x", "", false, &[]).is_err(), "a new user needs a password");
        assert!(u.upsert("x", "12345", false, &[]).is_err(), "too short");
        assert!(u.upsert("bad name", "123456", false, &[]).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn users_change_their_own_password() {
        let path = tmp();
        let u = DashUsers::open(&cfg(&path));
        assert!(u.check("bob", "secret2"), "fills the check cache");
        assert!(u.change_password("bob", "wrong", "newsecret").is_err(), "current password checked");
        assert!(u.change_password("bob", "secret2", "short").is_err(), "too short");
        u.change_password("bob", "secret2", "newsecret").unwrap();
        assert!(!u.check("bob", "secret2") && u.check("bob", "newsecret"), "old one gone, cache cleared");
        assert!(u.check("admin", "secret1"), "others untouched");
        assert!(!u.access(Some("bob")).admin, "rights untouched");
        let u = DashUsers::open(&cfg(&path));
        assert!(u.check("bob", "newsecret"), "saved to the file");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn without_users_there_is_no_login_and_the_first_user_is_an_admin() {
        let path = tmp();
        let mut c = crate::config::DashboardConfig::default();
        c.users_file = path.clone();
        let u = DashUsers::open(&c);
        let a = u.access(None);
        assert!(!a.auth && a.admin && a.sees("settings"));
        assert!(u.upsert("bob", "bobbobbob", false, &[]).is_err());
        u.upsert("admin", "adminpw", true, &[]).unwrap();
        assert!(u.enabled() && !u.access(None).admin);
        std::fs::remove_file(&path).ok();
    }
}
