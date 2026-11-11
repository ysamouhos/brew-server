//! Dashboard sign-in with a session cookie, next to HTTP Basic auth.
//!
//! The redesigned dashboard signs in through its own login window
//! (`POST /api/login`) and gets a `brew_session` cookie. Basic credentials keep
//! working everywhere (classic pages, scripts). Sessions live in memory: a
//! restart signs everyone out.

use crate::state::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

pub const COOKIE: &str = "brew_session";
/// A session ends this long after its last use.
const IDLE: Duration = Duration::from_secs(12 * 3600);
const MAX_SESSIONS: usize = 256;

/// Header the new dashboard sends with its requests: an unauthenticated one
/// then gets a plain 401 (its login window opens) instead of a Basic
/// challenge, which would make the browser pop its own dialog.
pub const UI_HEADER: &str = "x-brew-ui";

struct Session {
    user: String,
    last: Instant,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn create(user: &str) -> String {
    let token = new_token();
    let mut s = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    s.retain(|_, v| now.duration_since(v.last) < IDLE);
    if s.len() >= MAX_SESSIONS {
        if let Some(oldest) = s.iter().min_by_key(|(_, v)| v.last).map(|(k, _)| k.clone()) {
            s.remove(&oldest);
        }
    }
    s.insert(token.clone(), Session { user: user.to_owned(), last: now });
    token
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(COOKIE).and_then(|r| r.strip_prefix('=')).map(str::to_owned))
}

/// The user of the request's session cookie, if the session is alive and the
/// user still exists (`exists`).
pub fn session_user(exists: impl Fn(&str) -> bool, headers: &HeaderMap) -> Option<String> {
    let token = cookie_token(headers)?;
    let mut s = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let alive = s.get(&token).is_some_and(|v| now.duration_since(v.last) < IDLE && exists(&v.user));
    if !alive {
        s.remove(&token);
        return None;
    }
    let v = s.get_mut(&token)?;
    v.last = now;
    Some(v.user.clone())
}

/// Ends every session of `user` except the one of this request (a new
/// password signs the user out elsewhere).
pub fn end_sessions(user: &str, headers: &HeaderMap) {
    let keep = cookie_token(headers);
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
        .retain(|t, v| v.user != user || keep.as_deref() == Some(t.as_str()));
}

/// Whether the request comes from the new dashboard's own scripts.
pub fn from_ui(headers: &HeaderMap) -> bool {
    headers.contains_key(UI_HEADER)
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

fn cookie(state: &AppState, value: &str, max_age: Option<u64>) -> HeaderValue {
    let secure = if state.config.dashboard.tls.enabled { "; Secure" } else { "" };
    let age = max_age.map(|a| format!("; Max-Age={a}")).unwrap_or_default();
    HeaderValue::from_str(&format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict{secure}{age}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// `POST /api/login`: checks the dashboard user and sets the session cookie.
pub async fn login(State(state): State<Arc<AppState>>, Json(req): Json<LoginRequest>) -> Response {
    let ok = state.dash_users.check(&req.username, &req.password);
    if !ok {
        tracing::warn!(user = %req.username, "dashboard login failed");
        // Slows down guessing; the real protection is a good password.
        tokio::time::sleep(Duration::from_millis(600)).await;
        return (StatusCode::UNAUTHORIZED, "Wrong username or password").into_response();
    }
    let a = state.dash_users.access(Some(&req.username));
    tracing::info!(user = %req.username, admin = a.admin, "dashboard login");
    let token = create(&req.username);
    let mut res = Json(serde_json::json!({ "username": req.username, "admin": a.admin, "pages": a.pages })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie(&state, &token, None));
    res
}

/// `POST /api/logout`: ends the session and clears the cookie.
pub async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie_token(&headers) {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).remove(&token);
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie(&state, "", Some(0)));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cookie(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn a_session_cookie_signs_its_user_in_until_the_user_is_removed() {
        let users = HashMap::from([("admin".to_string(), "pw".to_string())]);
        let token = create("admin");
        let h = with_cookie(&format!("theme=dark; {COOKIE}={token}"));
        let exists = |u: &str| users.contains_key(u);
        assert_eq!(session_user(exists, &h).as_deref(), Some("admin"));
        assert_eq!(session_user(exists, &with_cookie(&format!("{COOKIE}=nope"))), None);
        assert_eq!(session_user(exists, &HeaderMap::new()), None);
        assert_eq!(session_user(|_| false, &h), None, "user gone: session ends");
        assert_eq!(session_user(exists, &h), None, "and stays ended");
    }

    #[test]
    fn a_new_password_ends_the_users_other_sessions() {
        let all = |_: &str| true;
        let (here, there, other) = (create("pwuser"), create("pwuser"), create("pwother"));
        let h = |t: &str| with_cookie(&format!("{COOKIE}={t}"));
        end_sessions("pwuser", &h(&here));
        assert_eq!(session_user(all, &h(&here)).as_deref(), Some("pwuser"), "this session stays");
        assert_eq!(session_user(all, &h(&there)), None, "the other one ends");
        assert_eq!(session_user(all, &h(&other)).as_deref(), Some("pwother"), "other users stay");
        end_sessions("pwuser", &HeaderMap::new());
        assert_eq!(session_user(all, &h(&here)), None, "an admin's change ends them all");
    }
}
