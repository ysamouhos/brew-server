use crate::{
    config,
    control::{self, ControlCommand, SendError},
    state::AppState,
    telemetry::TelemetryBts,
};
use anyhow::Context;
use axum::{
    extract::{Path, Query, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use axum::extract::Request;
use base64::Engine;
use std::{collections::HashMap, sync::Arc};

/// Server version shown under the "Live" indicator in every page header.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Server name from `[dashboard] servername`, HTML-escaped. Set once at startup,
/// before the first page is built; pages built without it use the default.
static SERVER_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Sets the name shown as page heading and in the browser tab titles.
pub fn set_server_name(name: &str) {
    let _ = SERVER_NAME.set(html_escape(name));
}

pub(crate) fn server_name() -> &'static str {
    SERVER_NAME.get_or_init(|| html_escape(&config::DashboardConfig::default().servername))
}

pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Runs the monitoring dashboard on its own listener (separate from the Brew
/// API). Gated behind optional HTTP Basic auth and optional TLS via the
/// `[dashboard]` config section. Returns immediately if disabled.
pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let cfg = &state.config.dashboard;
    let listen = state.dashboard_bind(cfg.listen);
    if !cfg.enabled {
        return Ok(());
    }

    let app = app(state.clone());

    if cfg.tls.enabled {
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            &cfg.tls.cert_path,
            &cfg.tls.key_path,
        )
        .await
        .with_context(|| {
            format!(
                "loading dashboard TLS cert {} and key {}",
                cfg.tls.cert_path.display(),
                cfg.tls.key_path.display()
            )
        })?;
        tracing::info!(listen=%listen, auth=!cfg.users.is_empty(), tls=true, "dashboard listening (TLS)");
        axum_server::bind_rustls(listen, rustls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(listen).await?;
        tracing::info!(listen=%listen, auth=!cfg.users.is_empty(), tls=false, "dashboard listening");
        axum::serve(listener, app).await?;
    }
    Ok(())
}

/// Every dashboard route with its auth layers.
fn app(state: Arc<AppState>) -> Router {
    // Privileged routes -- viewing/editing server config *and* issuing
    // Basestation control commands -- are split into their own sub-router
    // with an extra require_admin layer, merged into the rest of the
    // dashboard. require_basic below applies to the merged whole and runs
    // first (authenticate, or wave through when the dashboard is open), then
    // require_admin runs for just these routes (authorize) -- see both
    // functions' docs. `/login` lives here too so the header Login button,
    // which just navigates to it, triggers require_admin's auth challenge
    // and then bounces back to wherever Login was pressed.
    let settings_routes = Router::new()
        .route("/login", get(login))
        .route("/api/control/{id}", axum::routing::post(control_command))
        .route("/settings", get(settings_page))
        .route("/api/config/raw", get(config_raw_get).put(config_raw_put))
        .route("/api/config/sip/full", get(sip_config_full))
        .route("/api/config/sip/extensions/{user}", axum::routing::post(upsert_sip_extension).delete(delete_sip_extension))
        .route("/api/config/sip/trunks/{name}", axum::routing::post(upsert_sip_trunk).delete(delete_sip_trunk))
        .route("/api/config/sip/routes", axum::routing::post(upsert_sip_route))
        .route("/api/config/sip/routes/{name}", axum::routing::delete(delete_sip_route))
        .route("/api/config/bts-locations/{username}", axum::routing::post(upsert_bts_location).delete(delete_bts_location))
        .route("/api/config/blacklist", get(blacklist_get))
        .route("/api/config/blacklist/{issi}", axum::routing::post(blacklist_add).delete(blacklist_remove))
        .route("/api/config/allowlist", get(allowlist_get))
        .route("/api/config/allowlist/enabled", axum::routing::put(allowlist_set_enabled))
        .route("/api/config/allowlist/{issi}", axum::routing::post(allowlist_add).delete(allowlist_remove))
        .route("/api/sms-center/{id}", axum::routing::delete(sms_center_delete))
        .route("/api/dgna", axum::routing::post(crate::routing_api::dgna))
        .route("/api/pools/save", axum::routing::post(crate::routing_api::pools_put))
        .route("/api/networks", get(crate::routing_api::networks_get))
        .route("/api/networks/save", axum::routing::post(crate::routing_api::networks_put))
        .route("/api/accounts", get(crate::accounts::list).post(crate::accounts::upsert))
        .route("/api/issi-names/set", axum::routing::post(crate::issi_names::set))
        .route("/api/issi-names/import", axum::routing::post(crate::issi_names::import))
        .route("/api/gssi-names/set", axum::routing::post(crate::issi_names::set_gssi))
        .route("/api/gssi-names/import", axum::routing::post(crate::issi_names::import_gssi))
        .route("/api/group-access/set", axum::routing::post(crate::group_access::set))
        .route("/api/accounts/{username}", axum::routing::delete(crate::accounts::remove))
        .route("/api/ha/active", axum::routing::post(ha_make_active))
        .route("/api/ha/standby", axum::routing::post(ha_make_standby))
        .route("/api/ha/persist", axum::routing::post(ha_set_persist))
        .route("/api/ha/apply", axum::routing::post(ha_apply))
        .route("/api/config/ha", get(ha_config_get).put(ha_config_put))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));

    Router::new()
        .route("/", get(index))
        .route("/classic", get(classic_index))
        .route("/ui", get(crate::dashboard_ui::page))
        .route("/api/radios", get(crate::routing_api::radios))
        .route("/api/names", get(crate::accounts::names))
        .route("/api/issi-names", get(crate::issi_names::list))
        .route("/api/gssi-names", get(crate::issi_names::list_gssi))
        .route("/api/group-access", get(crate::group_access::list))
        .route("/api/pools", get(crate::routing_api::pools_get))
        .route("/calls", get(calls_page))
        .route("/sds", get(sds_page))
        .route("/telemetry-sds", get(telemetry_sds_page))
        .route("/registrations", get(registrations_page))
        .route("/connections", get(connections_page))
        .route("/map", get(map_page))
        .route("/api/status", get(snapshot))
        .route("/api/live", get(live))
        .route("/api/telemetry", get(telemetry_snapshot))
        .route("/api/emergencies", get(emergencies_snapshot))
        .route("/api/rssi", get(brew_rssi_snapshot))
        .route("/api/ms-cells", get(ms_cells_snapshot))
        .route("/api/registrations", get(registration_log))
        .route("/api/connections", get(connections_snapshot))
        .route("/api/positions", get(positions_snapshot))
        .route("/api/bts-locations", get(bts_locations_snapshot))
        .route("/api/control", get(control_list))
        .route("/sip", get(sip_page))
        .route("/sip-config", get(sip_config_page))
        .route("/api/sip", get(sip_snapshot))
        .route("/api/sip/config", get(sip_config))
        .route("/api/whoami", get(whoami))
        .route("/sms-center", get(sms_center_page))
        .route("/ha", get(ha_page))
        .route("/api/ha", get(ha_status))
        .route("/api/sms-center", get(sms_center_snapshot))
        .merge(settings_routes)
        .route_layer(middleware::from_fn_with_state(state.clone(), require_basic))
        // Unauthenticated liveness probe that works on both HA nodes (the
        // Brew listener's /healthz only exists on the Active one).
        .route("/healthz", get(dashboard_healthz))
        .route("/api/login", axum::routing::post(crate::dashboard_auth::login))
        .route("/api/logout", axum::routing::post(crate::dashboard_auth::logout))
        .with_state(state)
}

/// HTTP Basic auth guard for every dashboard route (including the `/api/live`
/// WebSocket upgrade, which browsers authenticate with a normal Authorization
/// header on the handshake). No configured users means auth is disabled.
///
/// When `[dashboard.ui].open` is set, viewing never requires a login: the
/// request is waved through regardless of credentials, so anyone can watch
/// the (read-only) dashboard and an admin logs in on demand via the header
/// Login button. Privileged routes stay protected by `require_admin`
/// underneath. When `open` is false (the default), configured `users` must
/// present a valid credential just to view, the original behavior.
async fn require_basic(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let cfg = &state.config.dashboard;
    if cfg.ui.open {
        return next.run(request).await;
    }
    let users = &cfg.users;
    if users.is_empty() || dashboard_user(users, request.headers()).is_some() {
        return next.run(request).await;
    }
    // The new dashboard's page and `whoami` load without a login: the page
    // then opens its own login window.
    let path = request.uri().path();
    if path == "/ui" || path == "/api/whoami" || (path == "/" && !cfg.legacy_ui) {
        return next.run(request).await;
    }
    if crate::dashboard_auth::from_ui(request.headers()) {
        return (StatusCode::UNAUTHORIZED, "login required").into_response();
    }
    basic_challenge(&cfg.realm)
}

/// Guards the privileged sub-router (see `run`): only usernames listed in
/// `[dashboard].admins` may view/change server config or issue Basestation
/// control commands. An empty `admins` list means "every configured user",
/// matching the pre-existing all-or-nothing behavior; empty `users` means
/// auth is disabled entirely (everyone is an admin).
///
/// A request with a *valid but non-admin* credential gets a real `403` with
/// a short explanation. A request with *no/invalid* credential instead gets
/// a Basic-auth challenge (`401`): in open mode an anonymous visitor reaches
/// here only by following the Settings link or pressing Login, and the
/// challenge is what makes the browser pop its native login dialog. (In
/// closed mode `require_basic` has already authenticated the caller, so the
/// `None` arm isn't reached there.)
async fn require_admin(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let cfg = &state.config.dashboard;
    if cfg.users.is_empty() {
        return next.run(request).await; // auth disabled: everyone is an admin
    }
    match dashboard_user(&cfg.users, request.headers()) {
        Some(user) if cfg.admins.is_empty() || cfg.admins.iter().any(|a| a == &user) => next.run(request).await,
        Some(_) => (StatusCode::FORBIDDEN, "Forbidden: this dashboard user is not listed in [dashboard].admins\n").into_response(),
        None if crate::dashboard_auth::from_ui(request.headers()) => (StatusCode::UNAUTHORIZED, "login required").into_response(),
        None => basic_challenge(&cfg.realm),
    }
}

/// The header Login button's target. It sits behind `require_admin`, so by
/// the time this runs the browser has supplied valid admin credentials (and
/// cached them for the rest of the origin, unlocking `/settings`, the
/// `/api/config/*` editors and Basestation control). It just bounces the
/// user back to wherever they pressed Login. `next` is honored only when
/// it's a local path, so the button can't be turned into an open redirect.
async fn login(Query(q): Query<HashMap<String, String>>) -> Redirect {
    let next = q.get("next").map(String::as_str).unwrap_or("/");
    let safe = if next.starts_with('/') && !next.starts_with("//") { next } else { "/" };
    Redirect::to(safe)
}

/// Returns the authenticated username for `/api/whoami`, plus whether the
/// dashboard is in open/read-only mode and whether a Login button should be
/// offered. Lets the dashboard's own JS swap the header Login/Logout control,
/// hide the Settings link and Basestation control from non-admins, and mark
/// the body read-only -- the real enforcement is `require_admin`/`require_basic`;
/// this is purely a UI convenience.
async fn whoami(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Json<serde_json::Value> {
    let cfg = &state.config.dashboard;
    let username = dashboard_user(&cfg.users, &headers);
    let admin = match &username {
        _ if cfg.users.is_empty() => true, // auth disabled: everyone is effectively admin
        Some(user) => cfg.admins.is_empty() || cfg.admins.iter().any(|a| a == user),
        None => false,
    };
    // A Login button is useful only in open mode, when nobody is signed in
    // yet and there's actually a credential to sign in with.
    let can_login = cfg.ui.open && username.is_none() && !cfg.users.is_empty();
    // Closed dashboard and nobody signed in: the new dashboard shows its
    // login window and nothing else.
    let need_login = !cfg.ui.open && username.is_none() && !cfg.users.is_empty();
    Json(serde_json::json!({ "username": username, "admin": admin, "open": cfg.ui.open, "can_login": can_login, "need_login": need_login }))
}

/// The signed-in dashboard user: HTTP Basic credentials, or the session
/// cookie of the new dashboard's login window.
fn dashboard_user(users: &HashMap<String, String>, headers: &HeaderMap) -> Option<String> {
    basic_username(users, headers).or_else(|| crate::dashboard_auth::session_user(users, headers))
}

/// Validates the request's HTTP Basic credentials against `users` and
/// returns the authenticated username, or `None` if missing/invalid.
fn basic_username(users: &HashMap<String, String>, headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())?;
    let b64 = value.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    if users.get(user).map(|p| p == pass).unwrap_or(false) {
        Some(user.to_string())
    } else {
        None
    }
}

fn basic_challenge(realm: &str) -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&format!("Basic realm=\"{realm}\"")).unwrap(),
    );
    response
}

/// The main dashboard HTML with the shared stylesheet substituted in, built
/// once on first access.
static INDEX_HTML: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| HTML.replace("__STYLE__", STYLE).replace("__VERSION__", VERSION).replace("__SERVERNAME__", server_name()));

/// `/`: the redesigned dashboard, or the classic one with `[dashboard]
/// legacy_ui = true` (the classic one stays at `/classic`, the new one at `/ui`).
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    if state.config.dashboard.legacy_ui { Html(INDEX_HTML.as_str()).into_response() } else { crate::dashboard_ui::page().await.into_response() }
}

pub async fn classic_index() -> Html<&'static str> { Html(INDEX_HTML.as_str()) }

/// Builds a standalone, auto-refreshing log page for one table. `endpoint` is
/// the JSON API the page polls; `extract_js` is a JS expression that, given the
/// parsed response bound to `d`, yields the array of items to paginate;
/// `row_js` renders one item to a `<tr>`; `columns` are the table headers.
fn log_page(title: &str, endpoint: &str, extract_js: &str, row_js: &str, columns: &[&str], per_page: usize, empty_msg: &str) -> String {
    let headers: String = columns.iter().map(|c| format!("<th>{c}</th>")).collect();
    let colspan = columns.len();
    format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>{title} - {name}</title>{style}</head><body><header><h1>{title}</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v{ver}</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a></p>
<section class=panel><table><thead><tr>{headers}</tr></thead><tbody id=log></tbody></table><div class=pager id=log-pager></div></section>
</main><script>
const $=id=>document.getElementById(id);const dt=x=>new Date(x).toLocaleTimeString();const dur=(a,b)=>Math.max(0,Math.floor(((b||Date.now())-a)/1000))+'s';const esc=s=>String(s??'').replace(/[&<>]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;'}}[c]));
const pages={{}};
function renderPaged(bodyId,items,rowFn,perPage,colspan,emptyMsg){{
  const body=$(bodyId),pager=$(bodyId+'-pager');
  const total=items.length,pageCount=Math.max(1,Math.ceil(total/perPage));
  if(pages[bodyId]==null)pages[bodyId]=0;
  if(pages[bodyId]>pageCount-1)pages[bodyId]=pageCount-1;
  if(pages[bodyId]<0)pages[bodyId]=0;
  const page=pages[bodyId],start=page*perPage;
  const slice=items.slice(start,start+perPage);
  body.innerHTML=slice.map(rowFn).join('')||`<tr><td colspan=${{colspan}} class=muted>${{emptyMsg}}</td></tr>`;
  if(pager){{
    if(total<=perPage){{pager.innerHTML='';}}
    else{{
      const from=start+1,to=start+slice.length;
      pager.innerHTML=`<button data-pg="${{bodyId}}" data-dir="-1"${{page<=0?' disabled':''}}>&larr; Prev</button>`+
        `<span class=pginfo>${{from}}\u2013${{to}} of ${{total}} (page ${{page+1}}/${{pageCount}})</span>`+
        `<button data-pg="${{bodyId}}" data-dir="1"${{page>=pageCount-1?' disabled':''}}>Next &rarr;</button>`;
    }}
  }}
}}
let last=[];
document.addEventListener('click',e=>{{const b=e.target.closest('button[data-pg]');if(!b)return;pages['log']=(pages['log']||0)+parseInt(b.getAttribute('data-dir'),10);draw();}});
function draw(){{renderPaged('log',last,x=>{row_js},{per_page},{colspan},'{empty_msg}');}}
async function load(){{try{{const d=await(await fetch('{endpoint}')).json();last={extract_js};draw();$('status').textContent='Live';}}catch(e){{$('status').textContent='Disconnected';}}}}
load();setInterval(load,2000);
</script></body></html>"#,
        title = title, style = STYLE, ver = VERSION, name = server_name(), headers = headers, colspan = colspan,
        row_js = row_js, per_page = per_page, empty_msg = empty_msg,
        endpoint = endpoint, extract_js = extract_js,
    )
}

static CALLS_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| log_page(
    "Recent calls", "/api/status", "d.recent_calls",
    "`<tr><td>${esc(x.kind)}</td><td>${x.source}</td><td>${x.destination}</td><td>${dt(x.started_at_ms)}</td><td>${dur(x.started_at_ms,x.ended_at_ms)}</td><td>${x.voice_frames}</td></tr>`",
    &["Type", "From", "To", "Start", "Duration", "Frames"], 10, "No completed calls",
));

static SDS_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| log_page(
    "Recent SDS", "/api/status", "d.recent_sds",
    "`<tr><td>${dt(x.at_ms)}</td><td>${x.source}</td><td>${x.destination}</td><td>${x.reports}</td><td class=muted>${String(x.uuid).slice(0,8)}</td></tr>`",
    &["Time", "From", "To", "Reports", "UUID"], 10, "No SDS yet",
));

pub(crate) static TELEMETRY_SDS_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| log_page(
    "Telemetry SDS Log", "/api/telemetry",
    "d.flatMap(s=>(s.recent_sds_out||[]).map(x=>({...x,bts:s.id}))).sort((a,b)=>b.at_ms-a.at_ms).slice(0,50)",
    "`<tr><td>${dt(x.at_ms)}</td><td>${esc(x.bts)}</td><td>${esc(x.direction)}</td><td>${x.source_issi}</td><td>${x.dest_issi}${x.is_group?' (grp)':''}</td><td>${x.protocol_id===10?'<span class=\"badge badge-pos\">\\uD83D\\uDCCD Position</span>':`<span class=\"badge badge-sds\">SDS<\\/span> <span class=muted>pid ${x.protocol_id}<\\/span>`}</td><td>${x.protocol_id===10&&!(x.text||'').trim()?'<span class=pos-undec>binary LIP (undecoded)<\\/span>':esc(x.text)}</td></tr>`",
    &["Time", "Station", "Dir", "From", "To", "Type", "Text"], 5, "No telemetry SDS yet",
));

pub(crate) static REGISTRATIONS_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| log_page(
    "Mobile Station Registrations", "/api/registrations",
    "d",
    "`<tr><td>${dt(x.at_ms)}</td><td>${esc(x.bts)}</td><td>${x.issi}</td><td>${x.cell?esc(x.cell)+' <span class=muted>('+x.carrier+')</span>':'<span class=muted>-</span>'}</td><td>${x.kind==='register'?'<span class=\"badge badge-reg-in\">Registered</span>':x.kind==='deregister'?'<span class=\"badge badge-reg-out\">Deregistered</span>':x.kind==='roamed'?'<span class=\"badge badge-reg-out\">Deregistered</span> <span class=muted>(roamed to other BS'+(x.roamed_to?' '+esc(x.roamed_to):'')+')</span>':'<span class=\"badge badge-reg-timeout\">Timed out</span>'}</td></tr>`",
    &["Time", "Station", "ISSI", "Cell (global ID)", "Event"], 15, "No registration events yet",
));

static SMS_CENTER_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| log_page(
    "SMS Center", "/api/sms-center",
    "(d.messages||[]).slice().sort((a,b)=>b.stored_at_ms-a.stored_at_ms)",
    "`<tr><td>${new Date(x.stored_at_ms).toLocaleString()}</td><td>${x.source_issi}</td><td>${x.destination}</td><td>${x.text?esc(x.text):'<span class=muted>'+esc(x.data_hex.slice(0,24))+'</span>'}</td><td>${x.in_flight?'<span class=\"badge badge-reg-timeout\">Delivering</span>':'<span class=\"badge badge-sds\">Waiting</span>'}</td><td>${x.attempts}</td><td>${new Date(x.expires_at_ms).toLocaleString()}</td><td><button class=admin-only onclick=\"if(confirm('Delete this stored message?'))fetch('/api/sms-center/${x.id}',{method:'DELETE'}).then(load)\">Delete</button></td></tr>`",
    &["Stored", "From", "To", "Message", "Status", "Attempts", "Expires", ""], 20, "No stored messages",
));

/// Standalone map page. Plots the latest decoded MS positions on an
/// OpenStreetMap base layer using Leaflet (loaded from unpkg CDN). Positions
/// come only from *textual* beacons; the page explains that binary LIP is not
/// yet decoded so an empty map is not mistaken for a bug.
static MAP_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>MS Map - {name}</title>
<link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css"/>
{style}
<style>#map{{height:70vh;border:1px solid #203047;border-radius:12px}}.map-note{{font-size:12px;color:#8fa2b8;margin-top:10px}}.leaflet-popup-content{{color:#0d1826}}.emg-pin{{animation:emgpulse 1.2s ease-in-out infinite}}@keyframes emgpulse{{0%,100%{{box-shadow:0 0 0 0 rgba(229,57,53,.8)}}50%{{box-shadow:0 0 0 14px rgba(229,57,53,0)}}}}.emg-pop{{color:#c62828;font-weight:700}}</style>
</head><body><header><h1>MS MAP</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v{ver}</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a></p>
<section class=panel><h2>Mobile station positions</h2><div id=map></div>
<div class=map-note id=note>Loading positions&hellip;</div>
<div class=map-note>Positions come from decoded LIP (binary short &amp; long reports) and textual beacons over the Brew channel. Radios that beacon but can't be plotted (no GPS fix, or coordinates not relayed to this server) are listed below.</div>
</section>
<section class=panel><h2>Beaconing but not plottable</h2><div id=undecoded class=reg-list><span class=muted>None</span></div>
<div class=map-note>These subscribers sent a position beacon that carried no usable coordinates &mdash; typically no GPS fix yet, or a locally-delivered beacon whose bytes never reach this server.</div>
</section>
</main>
<script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
<script>
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;'}}[c]));
const map=L.map('map').setView([44.43,26.10],5);
L.tileLayer('https://{{s}}.tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png',{{maxZoom:19,attribution:'&copy; OpenStreetMap'}}).addTo(map);
let markers={{}};let btsMarkers={{}};let fitted=false;
// Inline SVG icons (no image fetches): a radio mast with signal arcs for a Basestation, a handheld radio for a mobile terminal.
const pin=(bg,svg)=>`<div style="width:34px;height:34px;border-radius:50%;background:${{bg}};border:2px solid #fff;box-shadow:0 1px 5px rgba(0,0,0,.5);display:flex;align-items:center;justify-content:center">${{svg}}</div>`;
const btsSvg='<svg width=22 height=22 viewBox="0 0 24 24" fill=none stroke=#fff stroke-width=2 stroke-linecap=round stroke-linejoin=round><path d="M12 11v10M8 21h8M9.5 21l2.5-10 2.5 10"/><circle cx=12 cy=9 r=1.6 fill=#fff /><path d="M7.8 5.8a6 6 0 0 0 0 6.4M16.2 5.8a6 6 0 0 1 0 6.4M5 3.5a10 10 0 0 0 0 11M19 3.5a10 10 0 0 1 0 11"/></svg>';
const msSvg='<svg width=20 height=20 viewBox="0 0 24 24" fill=none stroke=#fff stroke-width=2 stroke-linecap=round stroke-linejoin=round><path d="M9 2v4M7 6h10a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z"/><rect x=8.5 y=8.5 width=7 height=4 rx=0.5 /><circle cx=9.5 cy=16 r=0.8 fill=#fff /><circle cx=12 cy=16 r=0.8 fill=#fff /><circle cx=14.5 cy=16 r=0.8 fill=#fff /><circle cx=9.5 cy=19 r=0.8 fill=#fff /><circle cx=12 cy=19 r=0.8 fill=#fff /><circle cx=14.5 cy=19 r=0.8 fill=#fff /></svg>';
const mkIcon=(bg,svg)=>L.divIcon({{html:pin(bg,svg),className:'',iconSize:[34,34],iconAnchor:[17,17],popupAnchor:[0,-18]}});
const btsIcon=mkIcon('#1f6feb',btsSvg),btsIconOff=mkIcon('#7a8696',btsSvg),msIcon=mkIcon('#d9822b',msSvg),msIconEmg=L.divIcon({{html:pin('#e53935',msSvg).replace('<div ','<div class=emg-pin '),className:'',iconSize:[34,34],iconAnchor:[17,17],popupAnchor:[0,-18]}});
async function loadBts(){{
  try{{
    const bts=await(await fetch('/api/bts-locations')).json();
    const seen=new Set();
    bts.forEach(b=>{{
      seen.add(b.id);
      const html=`<b>${{esc(b.name||b.username)}}</b> (Basestation)<br>${{b.lat.toFixed(5)}}, ${{b.lon.toFixed(5)}}<br>IP: ${{esc(b.ip||(b.origin?'via server '+b.origin.slice(0,8):'not connected'))}}<br>Source: ${{esc(b.source)}}<br>${{b.connected?'<span style="color:#2a7">connected</span>':'<span style="color:#a55">offline</span>'}}`;
      if(btsMarkers[b.id]){{btsMarkers[b.id].setLatLng([b.lat,b.lon]).setIcon(b.connected?btsIcon:btsIconOff).setPopupContent(html);}}
      else{{btsMarkers[b.id]=L.marker([b.lat,b.lon],{{icon:b.connected?btsIcon:btsIconOff}}).addTo(map).bindPopup(html);}}
    }});
    Object.keys(btsMarkers).forEach(k=>{{if(!seen.has(k)){{map.removeLayer(btsMarkers[k]);delete btsMarkers[k];}}}});
  }}catch(e){{}}
}}
async function load(){{
  try{{
    const fixes=(await(await fetch('/api/positions')).json()).filter(f=>f.lat!==0||f.lon!==0);
    $('status').textContent='Live';
    const seen=new Set();
    fixes.forEach(f=>{{
      seen.add(f.issi);
      const when=new Date(f.at_ms).toLocaleString();
      const html=`${{f.emergency?'<div class=emg-pop>&#128680; EMERGENCY</div>':''}}<b>ISSI ${{f.issi}}</b><br>${{f.lat.toFixed(5)}}, ${{f.lon.toFixed(5)}}<br>Station: ${{f.bts}}<br>${{when}}<br><span style="color:#555">${{(f.source_text||'').replace(/[<>&]/g,'')}}</span>`;
      const icon=f.emergency?msIconEmg:msIcon,z=f.emergency?10000:0;
      if(markers[f.issi]){{markers[f.issi].setLatLng([f.lat,f.lon]).setIcon(icon).setZIndexOffset(z).setPopupContent(html);}}
      else{{markers[f.issi]=L.marker([f.lat,f.lon],{{icon,zIndexOffset:z}}).addTo(map).bindPopup(html);}}
    }});
    Object.keys(markers).forEach(k=>{{if(!seen.has(Number(k))){{map.removeLayer(markers[k]);delete markers[k];}}}});
    const emg=fixes.filter(f=>f.emergency).map(f=>f.issi);
    $('note').textContent=(fixes.length?`${{fixes.length}} station(s) positioned.`:'No decodable position beacons received yet.')+(emg.length?' EMERGENCY: ISSI '+emg.join(', ')+'.':'');
    $('note').style.color=emg.length?'#ff6b6b':'';
    if(!fitted&&fixes.length){{fitted=true;map.fitBounds(fixes.map(f=>[f.lat,f.lon]),{{padding:[40,40],maxZoom:13}});}}
  }}catch(e){{$('status').textContent='Disconnected';}}
  loadBts();
  // Undecodable beacons (beaconing but not plottable), from telemetry.
  try{{
    const stations=await(await fetch('/api/telemetry')).json();
    const byIssi={{}};
    stations.forEach(s=>(s.undecoded_beacons_out||[]).forEach(u=>{{
      const prev=byIssi[u.issi];
      if(!prev||u.at_ms>prev.at_ms)byIssi[u.issi]=u;
    }}));
    const plotted=new Set(Object.keys(markers).map(Number));
    const list=Object.values(byIssi).filter(u=>!plotted.has(u.issi)).sort((a,b)=>b.at_ms-a.at_ms);
    $('undecoded').innerHTML=list.length?list.map(u=>`<span class=reg-issi title="${{esc(u.reason)}} \u2014 ${{u.count}} beacon(s)">${{u.issi}} <span class=muted>${{new Date(u.at_ms).toLocaleTimeString()}}</span></span>`).join(''):'<span class=muted>None</span>';
  }}catch(e){{}}
}}
load();setInterval(load,3000);
</script></body></html>"#, style = STYLE, ver = VERSION, name = server_name()));

/// SIP live panel: registrations, trunks and active calls, polled from
/// /api/sip every 2s. Renders a clear "disabled" notice when SIP is off.
static SIP_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>SIP / VoIP - {name}</title>{style}</head><body><header><h1>SIP / VoIP</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v{ver}</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a> &nbsp;·&nbsp; <a class=backlink href="/sip-config">SIP configuration &rarr;</a></p>
<div class=banner id=disabled-banner>SIP subsystem is disabled. Enable it in the <code>[sip]</code> section of the config file.</div>
<section class=cards>
<div class=card><div class=muted>Listen</div><div class=n id=listen style="font-size:16px">-</div></div>
<div class=card><div class=muted>Registrations</div><div class=n id=nreg>-</div></div>
<div class=card><div class=muted>Trunks up</div><div class=n id=ntrunk>-</div></div>
<div class=card><div class=muted>Active calls</div><div class=n id=nactive>-</div></div>
<div class=card><div class=muted>Total calls</div><div class=n id=ntotal>-</div></div>
<div class=card><div class=muted>Realm</div><div class=n id=realm style="font-size:16px">-</div></div>
</section>
<section class=panel><h2>Extension registrations</h2><table><thead><tr><th>AOR</th><th>Contact</th><th>Source</th><th>User-Agent</th><th>Auth</th><th>Expires in</th></tr></thead><tbody id=regs></tbody></table></section>
<section class=panel><h2>Trunks</h2><table><thead><tr><th>Name</th><th>Direction</th><th>Remote</th><th>Status</th><th>Peer</th><th>Detail</th><th>Calls</th></tr></thead><tbody id=trunks></tbody></table></section>
<section class=panel><h2>Active calls</h2><table><thead><tr><th>From</th><th>To</th><th>State</th><th>Duration</th><th>RTP A/B</th><th>Call-ID</th></tr></thead><tbody id=calls></tbody></table></section>
</main><script>
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;'}}[c]));
const now=()=>Date.now();
const dur=(a,b)=>{{if(!a)return'-';return Math.max(0,Math.floor(((b||now())-a)/1000))+'s';}};
const legName=e=>{{if(!e)return'-';switch(e.type){{case'sip_extension':return'ext '+esc(e.aor);case'sip_trunk':return'trunk '+esc(e.trunk)+(e.number?(' /'+esc(e.number)):'');case'brew_private':return'ISSI '+e.issi;case'brew_group':return'GSSI '+e.gssi;case'sip_external':return esc(e.uri);default:return esc(JSON.stringify(e));}}}};
function badge(s){{const m={{up:'health-ok',registering:'health-degraded',failed:'health-critical',down:'health-unknown'}};return`<span class="pill ${{m[s]||'health-unknown'}}">${{esc(s)}}</span>`;}}
async function load(){{
  try{{
    const d=await(await fetch('/api/sip')).json();
    $('status').textContent='Live';
    $('disabled-banner').style.display=d.enabled?'none':'block';
    $('listen').textContent=d.listen||'-';
    $('realm').textContent=d.realm||'-';
    $('nreg').textContent=d.registrations.length;
    $('ntrunk').textContent=d.trunks.filter(t=>t.status==='up').length+'/'+d.trunks.length;
    $('nactive').textContent=d.active_calls.length;
    $('ntotal').textContent=d.total_calls;
    $('regs').innerHTML=d.registrations.map(r=>`<tr><td>${{esc(r.aor)}}</td><td class=muted>${{esc(r.contact)}}</td><td>${{esc(r.source)}}</td><td class=muted>${{esc(r.user_agent)}}</td><td>${{r.authenticated?'<span class="pill health-ok">yes</span>':'<span class="pill health-unknown">no</span>'}}</td><td>${{Math.max(0,Math.floor((r.expires_at_ms-now())/1000))}}s</td></tr>`).join('')||'<tr><td colspan=6 class=muted>No registrations</td></tr>';
    $('trunks').innerHTML=d.trunks.map(t=>`<tr><td>${{esc(t.name)}}</td><td>${{esc(t.direction)}}</td><td>${{esc(t.remote_host)}}</td><td>${{badge(t.status)}}</td><td class=muted>${{esc(t.peer_addr||'-')}}</td><td class=muted>${{esc(t.detail)}}</td><td>${{t.active_calls}}</td></tr>`).join('')||'<tr><td colspan=7 class=muted>No trunks provisioned</td></tr>';
    $('calls').innerHTML=d.active_calls.map(c=>`<tr><td>${{legName(c.from)}}</td><td>${{legName(c.to)}}</td><td>${{esc(c.state)}}</td><td>${{dur(c.answered_at_ms||c.started_at_ms)}}</td><td class=muted>${{c.rtp_a_port||'-'}}/${{c.rtp_b_port||'-'}}</td><td class=muted>${{esc(String(c.call_id).slice(0,18))}}</td></tr>`).join('')||'<tr><td colspan=6 class=muted>No active calls</td></tr>';
  }}catch(e){{$('status').textContent='Disconnected';}}
}}
load();setInterval(load,2000);
</script></body></html>"#, style = STYLE, ver = VERSION, name = server_name()));

/// SIP configuration screen: a read-only view of the provisioned extensions,
/// trunks and voice routes from the config file, plus an inline explanation
/// that edits are made in the TOML (which the server hot-reloads).
static SIP_CONFIG_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>SIP Config - {name}</title>{style}</head><body><header><h1>SIP CONFIGURATION</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v{ver}</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a> &nbsp;·&nbsp; <a class=backlink href="/sip">SIP live panel &rarr;</a></p>
<div class=banner id=disabled-banner>SIP subsystem is disabled. Set <code>enabled = true</code> under <code>[sip]</code>.</div>
<section class=panel><h2>General</h2><table><tbody id=general></tbody></table>
<p class=map-note style="color:#8fa2b8;font-size:12px">This screen is read-only. Edit extensions, trunks and routes on the <a class=backlink id=settings-link href="/settings">Settings</a> page, or directly in the server's TOML config file (the running process watches the file and restarts to apply changes). Passwords are never shown here.</p>
</section>
<section class=panel><h2>Extensions</h2><table><thead><tr><th>User</th><th>Display name</th><th>ISSI</th><th>Outbound</th><th>Password</th></tr></thead><tbody id=exts></tbody></table></section>
<section class=panel><h2>Trunks</h2><table><thead><tr><th>Name</th><th>Direction</th><th>Remote host</th><th>Username</th><th>Realm</th><th>Reg interval</th><th>Enabled</th><th>Password</th></tr></thead><tbody id=trunks></tbody></table></section>
<section class=panel><h2>Voice routes</h2><table><thead><tr><th>#</th><th>Name</th><th>Match</th><th>Strip prefix</th><th>From</th><th>To</th><th>Enabled</th></tr></thead><tbody id=routes></tbody></table>
<p class=map-note style="color:#8fa2b8;font-size:12px">Routes are evaluated top to bottom; the first enabled route whose match pattern (and optional <em>from</em> restriction) matches the dialled destination wins. Endpoints: <code>ext:USER</code>, <code>trunk:NAME[/NUMBER]</code>, <code>issi:N</code> (Brew private), <code>group:N</code> (Brew group). <code>strip_prefix</code> removes a leading literal from the dialled string before it reaches an empty-number trunk destination (e.g. a "9" outside-line prefix).</p>
</section>
<style>#general td:first-child{{color:#8fa2b8;width:220px}}</style>
</main><script>
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;'}}[c]));
const yn=b=>b?'<span class="pill health-ok">yes</span>':'<span class="pill health-unknown">no</span>';
const pw=b=>b?'<span class="pill health-ok">set</span>':'<span class="pill health-critical">none</span>';
async function load(){{
  try{{
    const d=await(await fetch('/api/sip/config')).json();
    $('status').textContent='Live';
    $('disabled-banner').style.display=d.enabled?'none':'block';
    $('general').innerHTML=[
      ['Enabled',yn(d.enabled)],
      ['Listen',esc(d.listen)],
      ['Advertised host',esc(d.advertised_host||'(socket local address)')],
      ['Realm',esc(d.realm)],
      ['RTP port range',esc(d.rtp_port_min)+' - '+esc(d.rtp_port_max)],
      ['Registration TTL',esc(d.registration_ttl_seconds)+'s'],
    ].map(r=>`<tr><td>${{r[0]}}</td><td>${{r[1]}}</td></tr>`).join('');
    $('exts').innerHTML=d.extensions.map(e=>`<tr><td>${{esc(e.user)}}</td><td>${{esc(e.display_name||'-')}}</td><td>${{e.issi||'-'}}</td><td>${{yn(e.allow_outbound)}}</td><td>${{pw(e.has_password)}}</td></tr>`).join('')||'<tr><td colspan=5 class=muted>No extensions provisioned</td></tr>';
    $('trunks').innerHTML=d.trunks.map(t=>`<tr><td>${{esc(t.name)}}</td><td>${{esc(t.direction)}}</td><td>${{esc(t.remote_host||'-')}}</td><td>${{esc(t.username)}}</td><td class=muted>${{esc(t.realm||'-')}}</td><td>${{esc(t.register_interval_seconds)}}s</td><td>${{yn(t.enabled)}}</td><td>${{pw(t.has_password)}}</td></tr>`).join('')||'<tr><td colspan=8 class=muted>No trunks provisioned</td></tr>';
    $('routes').innerHTML=d.routes.map((r,i)=>`<tr><td class=muted>${{i+1}}</td><td>${{esc(r.name||'-')}}</td><td><code>${{esc(r.match_pattern)}}</code></td><td class=muted>${{r.strip_prefix?esc(r.strip_prefix):'-'}}</td><td>${{esc(r.from||'any')}}</td><td>${{esc(r.to||'-')}}</td><td>${{yn(r.enabled)}}</td></tr>`).join('')||'<tr><td colspan=7 class=muted>No routes configured</td></tr>';
  }}catch(e){{$('status').textContent='Disconnected';}}
}}
load();setInterval(load,5000);
</script></body></html>"#, style = STYLE, ver = VERSION, name = server_name()));

/// Live "who's connected now" page: Brew connections, registered mobile
/// stations, and SIP registrations/trunks. Distinct from `/registrations`,
/// which is a historical event log (registers/deregisters over time), not a
/// current-state snapshot.
static CONNECTIONS_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>Connections - {name}</title>{style}</head><body><header><h1>LIVE CONNECTIONS</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v{ver}</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a> &nbsp;·&nbsp; <a class=backlink href="/registrations">Registration event log &rarr;</a></p>
<p class=map-note style="color:#8fa2b8;font-size:12px">Who is connected and registered right now, not a history of events. Refreshes every 5s.</p>

<section class=panel><h2>Brew Connections<span class=backlink id=bc-count></span></h2><table><thead><tr><th>ID</th><th>Station</th><th>Mode</th><th>Version</th><th>Remote address</th><th>Connected</th><th>Registered ISSIs</th><th>Cells</th></tr></thead><tbody id=brew-clients></tbody></table></section>

<section class=panel><h2>Cells<span class=backlink id=cell-count></span></h2><table><thead><tr><th>Cell</th><th>Carrier</th><th>Downlink</th><th>LA</th><th>MCC / MNC</th><th>Neighbours</th><th>Capabilities</th><th>Basestation address</th><th>Connected</th><th>Registered ISSIs</th></tr></thead><tbody id=cells></tbody></table><p class=map-note style="color:#8fa2b8;font-size:12px">Cells the Basestations announce (FlowStation with the <code>bts-info</code> extension). Named <i>login/cell</i>: the Brew login of the Basestation and the cell's number within it (0 = primary). On the air a cell is identified by MCC, MNC, carrier and band plan; the name is only a label here. Registered ISSIs are counted per Basestation. Neighbours are the cells a cell broadcasts, named when the server knows them; a station that went away stays listed as <i>offline</i> until it connects again (stations with a login only).</p></section>

<section class=panel><h2>Registered Subscribers<span class=backlink id=ms-count></span></h2><table><thead><tr><th>ISSI</th><th>Registered via</th><th>Basestation</th><th>Cell</th><th>Basestation address</th><th>Groups</th></tr></thead><tbody id=mobile-stations></tbody></table></section>

<section class=panel><h2>Cell Change Log<span class=backlink id=roam-count></span></h2><table><thead><tr><th>Time</th><th>ISSI</th><th>From</th><th>To</th><th>How</th></tr></thead><tbody id=roaming-log></tbody></table><p class=map-note style="color:#8fa2b8;font-size:12px">The last cell changes of the radios (cell reselection, TS 100 392-2 clause 18.3.4.7). When the Basestation the radio left reports it (FlowStation with the <code>ms-reselection</code> extension), the category is shown: <i>undeclared</i> = no call running, <i>unannounced</i> = left a call without telling the cell, <i>announced type 1 / 2 / 3</i> = told the cell first (U-PREPARE) and got D-NEW-CELL; <i>prepared here</i> = the target was prepared through this server. Otherwise: <i>announced</i> = announced reselection prepared through this server, <i>not announced</i> = the radio registered on another Basestation without a preparation here (any other category), <i>within station</i> = another cell of a multi-cell station. A radio only registers when it enters another location area, so moves within one location area show up only once the Basestation reports the cell. Cells show once the Basestation reports them (FlowStation with the <code>ms-cell</code> extension); otherwise only the station. Kept in memory, newest first.</p></section>

<section class=panel><h2>SIP Registrations<span class=backlink id=sip-count></span></h2><div class=banner id=sip-disabled-banner>SIP subsystem is disabled.</div><table><thead><tr><th>AOR</th><th>Contact</th><th>Source</th><th>User-Agent</th><th>Registered</th><th>Expires</th><th>Auth</th></tr></thead><tbody id=sip-regs></tbody></table></section>

<section class=panel><h2>SIP Trunks</h2><table><thead><tr><th>Name</th><th>Direction</th><th>Status</th><th>Remote host</th><th>Active calls</th></tr></thead><tbody id=sip-trunks></tbody></table></section>
</main><script>
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;'}}[c]));
const yn=b=>b?'<span class="pill health-ok">yes</span>':'<span class="pill health-unknown">no</span>';
const dt=x=>x?new Date(x).toLocaleString():'-';
const ago=x=>{{if(!x)return '-';const s=Math.max(0,Math.floor((Date.now()-x)/1000));if(s<60)return s+'s ago';if(s<3600)return Math.floor(s/60)+'m ago';return Math.floor(s/3600)+'h '+Math.floor((s%3600)/60)+'m ago';}};
// Cells a Basestation announced (SERVICE_BTS_INFO): main carrier and location area; a carrier
// another connection announces too is flagged, and marked standby where the other one serves it.
const dupTag=x=>x.duplicate?` <span style="color:#e55" title="carrier announced by more than one Basestation">duplicate${{x.serving?'':', standby'}}</span>`:'';
const cellsCell=c=>(c.cells||[]).map(x=>`${{esc(x.name)}} <span class=muted>${{x.carrier}} · LA ${{x.location_area}}</span>`+dupTag(x)).join('<br>')||'<span class=muted>-</span>';
const mhz=hz=>hz?(hz/1e6).toFixed(5).replace(/0+$/,'').replace(/\.$/,'')+' MHz':'-';
async function load(){{
  try{{
    const d=await(await fetch('/api/connections')).json();
    $('status').textContent='Live';
    const dups=(d.duplicate_carriers||[]).length;
    $('bc-count').textContent=' ('+d.brew_clients.length+')'+(dups?' · '+dups+' duplicate carrier'+(dups>1?'s':''):'');
    $('ms-count').textContent=' ('+d.mobile_stations.length+')';
    $('brew-clients').innerHTML=d.brew_clients.map(c=>`<tr><td class=muted>${{esc(c.id).slice(0,8)}}</td><td>${{esc(c.station)}}</td><td>${{esc(c.mode)}}</td><td>${{c.version}}</td><td>${{esc(c.remote_addr||'-')}}</td><td>${{ago(c.connected_at_ms)}}</td><td>${{c.registered_issis}}</td><td>${{cellsCell(c)}}</td></tr>`).join('')||'<tr><td colspan=8 class=muted>No Brew connections</td></tr>';
    $('cell-count').textContent=' ('+(d.cells||[]).length+')';
    $('cells').innerHTML=(d.cells||[]).map(x=>`<tr${{x.online===false?' class=muted':''}}><td><b>${{esc(x.name)}}</b>${{dupTag(x)}}${{x.online===false?' <span class=muted>offline</span>':''}}</td><td>${{x.carrier}}</td><td>${{mhz(x.downlink_hz)}}</td><td>${{x.location_area}}</td><td>${{x.mcc}} / ${{x.mnc}}</td><td>${{x.neighbours?(x.neighbours.map(esc).join(', ')||'none'):'<span class=muted>-</span>'}}</td><td>${{(x.capabilities||[]).map(esc).join(', ')||'<span class=muted>-</span>'}}</td><td>${{x.online===false?'-':esc(x.remote_addr||'-')}}</td><td>${{x.online===false?(x.last_seen_ms?'gone '+ago(x.last_seen_ms):'-'):ago(x.connected_at_ms)}}</td><td>${{x.online===false?'-':x.registered_issis}}</td></tr>`).join('')||'<tr><td colspan=10 class=muted>No cells announced</td></tr>';
    const place=p=>p.cell?`${{esc(p.cell)}} <span class=muted>(${{p.carrier}})</span>`:esc(p.station);
    const how={{handover:'<span class="badge badge-reg-in">announced</span>',reselection:'<span class="badge badge-reg-out">not announced</span>',within_station:'<span class=badge>within station</span>'}};
    const resel={{undeclared:['badge-reg-out','undeclared'],unannounced:['badge-reg-out','unannounced'],announced_type_1:['badge-reg-in','announced type 1'],announced_type_2:['badge-reg-in','announced type 2'],announced_type_3:['badge-reg-in','announced type 3']}};
    const where={{handover:'prepared here',within_station:'within station'}};
    const howOf=r=>{{const t=resel[r.reselection];if(!t)return how[r.kind]||esc(r.kind);return `<span class="badge ${{t[0]}}">${{t[1]}}</span>`+(where[r.kind]?` <span class=muted>${{where[r.kind]}}</span>`:'');}};
    $('roam-count').textContent=' ('+(d.roaming_log||[]).length+')';
    $('roaming-log').innerHTML=(d.roaming_log||[]).map(r=>`<tr><td>${{new Date(r.at_ms).toLocaleTimeString()}}</td><td>${{r.issi}}</td><td>${{place(r.from)}}</td><td>${{place(r.to)}}</td><td>${{howOf(r)}}</td></tr>`).join('')||'<tr><td colspan=5 class=muted>No cell changes yet</td></tr>';
    $('mobile-stations').innerHTML=d.mobile_stations.map(m=>`<tr><td>${{m.issi}}</td><td>${{esc(m.mode)}}</td><td>${{esc(m.basestation_name||String(m.basestation_id).slice(0,8))}}</td><td>${{m.cell?`${{esc(m.cell.name)}} <span class=muted>(${{m.cell.carrier}})</span>`:'<span class=muted>-</span>'}}</td><td>${{esc(m.basestation_addr||'-')}}</td><td>${{(m.groups||[]).join(', ')||'-'}}</td></tr>`).join('')||'<tr><td colspan=6 class=muted>No registered subscribers</td></tr>';
    $('sip-disabled-banner').style.display=d.sip.enabled?'none':'block';
    $('sip-count').textContent=' ('+d.sip.registrations.length+')';
    $('sip-regs').innerHTML=d.sip.registrations.map(r=>`<tr><td>${{esc(r.aor)}}</td><td class=muted>${{esc(r.contact)}}</td><td>${{esc(r.source)}}</td><td class=muted>${{esc(r.user_agent||'-')}}</td><td>${{dt(r.registered_at_ms)}}</td><td>${{dt(r.expires_at_ms)}}</td><td>${{yn(r.authenticated)}}</td></tr>`).join('')||'<tr><td colspan=7 class=muted>No SIP registrations</td></tr>';
    $('sip-trunks').innerHTML=d.sip.trunks.map(t=>`<tr><td>${{esc(t.name)}}</td><td>${{esc(t.direction)}}</td><td>${{esc(t.status)}}</td><td class=muted>${{esc(t.remote_host||'-')}}</td><td>${{t.active_calls}}</td></tr>`).join('')||'<tr><td colspan=5 class=muted>No SIP trunks configured</td></tr>';
  }}catch(e){{$('status').textContent='Disconnected';}}
}}
load();setInterval(load,5000);
</script></body></html>"#, style = STYLE, ver = VERSION, name = server_name()));

static SETTINGS_HTML: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>Settings - {name}</title>{style}</head><body><header><h1>SETTINGS</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v{ver}</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a> &nbsp;·&nbsp; <a class=backlink href="/sip-config">SIP Config (read-only view) &rarr;</a></p>
<div class=banner id=save-banner></div>
<p class=map-note style="color:#8fa2b8;font-size:12px">Every save here writes the server's TOML config file and the process restarts within a couple seconds to apply it (the same mechanism as hand-editing the file). A brief connection drop across the restart is expected.</p>

<section class=panel><h2>SIP Extensions</h2><table><thead><tr><th>User (AOR)</th><th>Display name</th><th>ISSI</th><th>Password</th><th>Outbound</th><th></th></tr></thead><tbody id=exts></tbody></table>
<div class=ctl-row><input id=ext-user placeholder="user (e.g. 1001)"><input id=ext-name placeholder="display name"><input id=ext-issi placeholder="ISSI" type=number><input id=ext-pass placeholder="password"><label><input id=ext-out type=checkbox checked> outbound</label><button onclick="saveExt()">Add / Update</button></div>
</section>

<section class=panel><h2>SIP Trunks</h2><table><thead><tr><th>Name</th><th>Direction</th><th>Remote host</th><th>Username</th><th>Password</th><th>Realm</th><th>Reg interval</th><th>Enabled</th><th></th></tr></thead><tbody id=trunks></tbody></table>
<div class=ctl-row><input id=tr-name placeholder="name"><select id=tr-dir><option value=outbound>outbound</option><option value=inbound>inbound</option><option value=peer>peer</option></select><input id=tr-host placeholder="remote host:port"><input id=tr-user placeholder="username"><input id=tr-pass placeholder="password"><input id=tr-realm placeholder="realm"><input id=tr-interval placeholder="reg interval s" type=number value=300><label><input id=tr-en type=checkbox checked> enabled</label><button onclick="saveTrunk()">Add / Update</button></div>
</section>

<section class=panel><h2>Voice routes</h2><table><thead><tr><th>Name</th><th>Match</th><th>Strip prefix</th><th>From</th><th>To</th><th>Enabled</th><th></th></tr></thead><tbody id=routes></tbody></table>
<div class=ctl-row><input id=rt-name placeholder="name"><input id=rt-match placeholder="match pattern, e.g. 9*"><input id=rt-strip placeholder="strip prefix, e.g. 9" style="width:110px"><input id=rt-from placeholder="from (optional): ext:USER | trunk:NAME | issi:N | group:N"><input id=rt-to placeholder="to: ext:USER | trunk:NAME[/NUMBER] | issi:N | group:N"><label><input id=rt-en type=checkbox checked> enabled</label><button onclick="saveRoute()">Add / Update</button></div>
<p class=map-note style="color:#8fa2b8;font-size:12px">Endpoint shorthand: <code>ext:USER</code>, <code>trunk:NAME</code> or <code>trunk:NAME/NUMBER</code>, <code>issi:N</code> (Brew private), <code>group:N</code> (Brew group). Matching runs against the full dialled string (e.g. a PSTN call from a mobile terminal dialling "9" + 10 digits arrives as dialled string "9XXXXXXXXXX"); <code>strip_prefix</code> removes a leading literal (e.g. "9") only from what's handed to an empty-number <code>trunk:NAME</code> destination, so the trunk dials the bare 10 digits. Updating a route matches by name and keeps its position; a new name appends to the end (reorder via the raw editor below).</p>
</section>

<section class=panel><h2>ISSI Blacklist</h2><p class=muted>A blacklisted ISSI cannot transmit to groups, make or receive private calls, or send or receive SDS through this server. Dispatch ambience listening (SS-AL) to it and LIP position traffic still work. It can still register. Changes apply immediately, without a restart.</p><table><thead><tr><th>ISSI</th><th></th></tr></thead><tbody id=blk-list></tbody></table>
<div class=ctl-row><input id=blk-issi placeholder="ISSI to block, e.g. 4013" type=number min=1><button onclick="blockIssi()">Block</button></div></section>
<section class=panel><h2>ISSI Allowlist</h2><p class=muted>The closed-network counterpart of the blacklist: while enforced, only the ISSIs listed here may transmit to groups, make or receive private calls, or send or receive SDS through this server. Any other radio is barred the same way a blacklisted one is (SS-AL, LIP positions and emergency calls still pass, and it can still register). The blacklist still applies on top. Enforced here for the whole network, whichever Basestation the radio registers on. Changes apply immediately, without a restart.</p>
<p id=al-state class=muted></p>
<div class=ctl-row><label><input id=al-enabled type=checkbox onchange="setAllowlistEnabled(this.checked)"> enforce the allowlist</label></div>
<table><thead><tr><th>ISSI</th><th></th></tr></thead><tbody id=al-list></tbody></table>
<div class=ctl-row><input id=al-issi placeholder="ISSI to admit, e.g. 4013" type=number min=1><button onclick="allowIssi()">Admit</button></div></section>
<section class=panel><h2>Basestation Locations (static fallback)</h2><p class=muted>Positions reported by Basestations over telemetry, or relayed by other servers, take precedence. This table lists the configured entries.</p><table><thead><tr><th>Username (auth)</th><th>Name</th><th>Latitude</th><th>Longitude</th><th></th></tr></thead><tbody id=bts-locs></tbody></table>
<div class=ctl-row><input id=bl-user placeholder="Brew username, e.g. 1000001"><input id=bl-name placeholder="Basestation name"><input id=bl-lat placeholder="latitude" type=number step=any><input id=bl-lon placeholder="longitude" type=number step=any><button onclick="saveBtsLoc()">Add / Update</button></div>
<p class=map-note style="color:#8fa2b8;font-size:12px">Keyed by the same numeric username the Basestation authenticates with under <code>[auth.users]</code>, so it's matched automatically to whichever live connection logs in as that identity. Shown on the <a class=backlink href="/map">MS Map</a> alongside mobile-station positions.</p>
</section>

<section class=panel id=raw-panel><h2>Full configuration (raw TOML)</h2>
<p class=map-note style="color:#8fa2b8;font-size:12px">Every setting lives here, including ones with no form above (listen addresses, TLS, dashboard/auth/telemetry/control users, storage, call-routing flags). Loads the live config; Save validates it before writing anything.</p>
<textarea id=raw style="width:100%;min-height:420px;background:#0d1826;color:#e7edf5;border:1px solid #203047;border-radius:8px;padding:12px;font-family:ui-monospace,monospace;font-size:12px"></textarea>
<div class=ctl-row><button onclick="loadRaw()">Reload from server</button><button onclick="saveRaw()">Save</button><span id=raw-result class=ctl-result></span></div>
</section>
</main><script>
const $=id=>document.getElementById(id);
// Inside the new dashboard (a frame of its Settings page) the configuration is edited on its
// own Configuration page, so the raw editor here is not shown twice.
if(window.self!==window.top){{$('raw-panel').style.display='none';}}
const esc=s=>String(s??'').replace(/[&<>]/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;'}}[c]));
const yn=b=>b?'<span class="pill health-ok">yes</span>':'<span class="pill health-unknown">no</span>';
function banner(ok,msg){{const b=$('save-banner');b.style.display='block';b.style.background=ok?'#173822':'#3a1414';b.style.borderColor=ok?'#245c37':'#f2545b';b.style.color=ok?'#52d273':'#ffb4b8';b.textContent=msg;setTimeout(()=>{{b.style.display='none'}},6000);}}
async function api(method,url,body){{
  const r=await fetch(url,{{method,headers:body!==undefined?{{'Content-Type':'application/json'}}:undefined,body:body!==undefined?JSON.stringify(body):undefined}});
  const t=await r.text();
  if(!r.ok){{banner(false,'Failed: '+t);throw new Error(t);}}
  banner(true,'Saved. Restarting to apply…');
  return t;
}}
function endpoint(s){{
  if(!s)return null;
  const [kind,rest]=s.split(':');
  if(kind==='ext')return {{kind:'sip_extension',user:rest}};
  if(kind==='trunk'){{const[trunk,number]=rest.split('/');return {{kind:'sip_trunk',trunk,number:number||''}};}}
  if(kind==='issi')return {{kind:'brew_private',issi:parseInt(rest,10)}};
  if(kind==='group')return {{kind:'brew_group',gssi:parseInt(rest,10)}};
  return null;
}}
function describe(ep){{
  if(!ep)return '';
  if(ep.kind==='sip_extension')return 'ext:'+ep.user;
  if(ep.kind==='sip_trunk')return ep.number?`trunk:${{ep.trunk}}/${{ep.number}}`:'trunk:'+ep.trunk;
  if(ep.kind==='brew_private')return 'issi:'+ep.issi;
  if(ep.kind==='brew_group')return 'group:'+ep.gssi;
  return '';
}}
async function loadSip(){{
  const d=await(await fetch('/api/config/sip/full')).json();
  $('exts').innerHTML=Object.entries(d.extensions).map(([user,e])=>`<tr><td>${{esc(user)}}</td><td>${{esc(e.display_name||'-')}}</td><td>${{e.issi||'-'}}</td><td class=muted>${{e.password?'•'.repeat(8):'(none)'}}</td><td>${{yn(e.allow_outbound)}}</td><td><button onclick="delExt('${{esc(user)}}')">Delete</button></td></tr>`).join('')||'<tr><td colspan=6 class=muted>No extensions provisioned</td></tr>';
  $('trunks').innerHTML=Object.entries(d.trunks).map(([name,t])=>`<tr><td>${{esc(name)}}</td><td>${{esc(t.direction)}}</td><td>${{esc(t.remote_host||'-')}}</td><td>${{esc(t.username)}}</td><td class=muted>${{t.password?'•'.repeat(8):'(none)'}}</td><td class=muted>${{esc(t.realm||'-')}}</td><td>${{esc(t.register_interval_seconds)}}s</td><td>${{yn(t.enabled)}}</td><td><button onclick="delTrunk('${{esc(name)}}')">Delete</button></td></tr>`).join('')||'<tr><td colspan=9 class=muted>No trunks provisioned</td></tr>';
  $('routes').innerHTML=d.routes.map(r=>`<tr><td>${{esc(r.name||'-')}}</td><td><code>${{esc(r.match_pattern)}}</code></td><td class=muted>${{r.strip_prefix?esc(r.strip_prefix):'-'}}</td><td>${{esc(describe(r.from))||'any'}}</td><td>${{esc(describe(r.to))}}</td><td>${{yn(r.enabled)}}</td><td><button onclick="delRoute('${{esc(r.name)}}')">Delete</button></td></tr>`).join('')||'<tr><td colspan=7 class=muted>No routes configured</td></tr>';
}}
async function saveExt(){{
  const user=$('ext-user').value.trim(); if(!user)return;
  await api('POST',`/api/config/sip/extensions/${{encodeURIComponent(user)}}`,{{
    password:$('ext-pass').value, display_name:$('ext-name').value,
    issi:parseInt($('ext-issi').value,10)||0, allow_outbound:$('ext-out').checked,
  }});
  loadSip();
}}
async function delExt(user){{ await api('DELETE',`/api/config/sip/extensions/${{encodeURIComponent(user)}}`); loadSip(); }}
async function saveTrunk(){{
  const name=$('tr-name').value.trim(); if(!name)return;
  await api('POST',`/api/config/sip/trunks/${{encodeURIComponent(name)}}`,{{
    direction:$('tr-dir').value, remote_host:$('tr-host').value, username:$('tr-user').value,
    password:$('tr-pass').value, realm:$('tr-realm').value,
    register_interval_seconds:parseInt($('tr-interval').value,10)||300, enabled:$('tr-en').checked,
  }});
  loadSip();
}}
async function delTrunk(name){{ await api('DELETE',`/api/config/sip/trunks/${{encodeURIComponent(name)}}`); loadSip(); }}
async function saveRoute(){{
  const name=$('rt-name').value.trim(); if(!name)return;
  await api('POST','/api/config/sip/routes',{{
    name, match_pattern:$('rt-match').value||'*', strip_prefix:$('rt-strip').value.trim(),
    from:endpoint($('rt-from').value.trim()), to:endpoint($('rt-to').value.trim()),
    enabled:$('rt-en').checked,
  }});
  loadSip();
}}
async function delRoute(name){{ await api('DELETE',`/api/config/sip/routes/${{encodeURIComponent(name)}}`); loadSip(); }}
async function loadBlk(){{
  const d=await(await fetch('/api/config/blacklist')).json();
  $('blk-list').innerHTML=d.map(i=>`<tr><td>${{i}}</td><td><button onclick="unblockIssi(${{i}})">Unblock</button></td></tr>`).join('')||'<tr><td colspan=2 class=muted>No ISSI is blacklisted</td></tr>';
}}
async function blkApi(method,issi){{
  const r=await fetch(`/api/config/blacklist/${{issi}}`,{{method}});
  const t=await r.text();
  if(!r.ok){{banner(false,'Failed: '+t);return;}}
  banner(true,'Applied immediately.');
  loadBlk();
}}
function blockIssi(){{const v=parseInt($('blk-issi').value,10); if(!v)return; $('blk-issi').value=''; blkApi('POST',v);}}
function unblockIssi(i){{blkApi('DELETE',i);}}
async function loadAl(){{
  const d=await(await fetch('/api/config/allowlist')).json();
  $('al-enabled').checked=d.enabled;
  $('al-state').textContent=d.enabled?(d.issis.length?`Enforced: ${{d.issis.length}} ISSI${{d.issis.length==1?'':'s'}} admitted, every other radio is barred.`:'Enforced with an EMPTY list: every radio is barred until one is admitted.'):'Not enforced: the list is kept but every radio may communicate.';
  $('al-list').innerHTML=d.issis.map(i=>`<tr><td>${{i}}</td><td><button onclick="disallowIssi(${{i}})">Remove</button></td></tr>`).join('')||'<tr><td colspan=2 class=muted>No ISSI is on the allowlist</td></tr>';
}}
async function alApi(method,url,body){{
  const r=await fetch(url,{{method,headers:body?{{'Content-Type':'application/json'}}:{{}},body:body?JSON.stringify(body):undefined}});
  const t=await r.text();
  if(!r.ok){{banner(false,'Failed: '+t);loadAl();return;}}
  banner(true,'Applied immediately.');
  loadAl();
}}
function allowIssi(){{const v=parseInt($('al-issi').value,10); if(!v)return; $('al-issi').value=''; alApi('POST',`/api/config/allowlist/${{v}}`);}}
function disallowIssi(i){{alApi('DELETE',`/api/config/allowlist/${{i}}`);}}
function setAllowlistEnabled(enabled){{alApi('PUT','/api/config/allowlist/enabled',{{enabled}});}}
async function loadBtsLocs(){{
  const d=(await(await fetch('/api/bts-locations?config_only=1')).json());
  $('bts-locs').innerHTML=d.map(b=>`<tr><td>${{esc(b.username)}}</td><td>${{esc(b.name)}}</td><td>${{b.lat}}</td><td>${{b.lon}}</td><td><button onclick="delBtsLoc('${{esc(b.username)}}')">Delete</button></td></tr>`).join('')||'<tr><td colspan=5 class=muted>No Basestation locations configured</td></tr>';
}}
async function saveBtsLoc(){{
  const username=$('bl-user').value.trim(); if(!username)return;
  await api('POST',`/api/config/bts-locations/${{encodeURIComponent(username)}}`,{{
    name:$('bl-name').value, lat:parseFloat($('bl-lat').value)||0, lon:parseFloat($('bl-lon').value)||0,
  }});
  loadBtsLocs();
}}
async function delBtsLoc(username){{ await api('DELETE',`/api/config/bts-locations/${{encodeURIComponent(username)}}`); loadBtsLocs(); }}
async function loadRaw(){{ $('raw').value=await(await fetch('/api/config/raw')).text(); }}
async function saveRaw(){{
  const r=await fetch('/api/config/raw',{{method:'PUT',body:$('raw').value}});
  const t=await r.text();
  if(!r.ok){{banner(false,'Failed: '+t);return;}}
  banner(true,'Saved. Restarting to apply…');
}}
loadSip();loadRaw();loadBtsLocs();loadBlk();loadAl();
</script></body></html>"#, style = STYLE, ver = VERSION, name = server_name()));

pub async fn calls_page() -> Html<&'static str> { Html(CALLS_HTML.as_str()) }
pub async fn sds_page() -> Html<&'static str> { Html(SDS_HTML.as_str()) }
pub async fn telemetry_sds_page() -> Html<&'static str> { Html(TELEMETRY_SDS_HTML.as_str()) }
pub async fn registrations_page() -> Html<&'static str> { Html(REGISTRATIONS_HTML.as_str()) }
pub async fn connections_page() -> Html<&'static str> { Html(CONNECTIONS_HTML.as_str()) }
pub async fn snapshot(State(state): State<Arc<AppState>>) -> Json<crate::monitor::Snapshot> { let i=state.inner.read().await; let counts=(i.basestation_count(),i.ms_registration_count(),i.group_clients.len()); drop(i); Json(state.monitor.snapshot(counts.0,counts.1,counts.2).await) }
pub async fn live(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> impl IntoResponse { ws.on_upgrade(move |s| live_socket(state,s)) }
async fn live_socket(state: Arc<AppState>, mut socket: WebSocket) { let mut rx=state.monitor.subscribe(); while let Ok(ev)=rx.recv().await { if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.is_err(){break;} } }

/// Active emergencies for the red ribbon: Basestation alarms from telemetry and
/// live emergency calls seen on the Brew channel (see `emergency`). The
/// blacklist never holds an emergency call back.
pub async fn emergencies_snapshot(State(state): State<Arc<AppState>>) -> Json<Vec<crate::emergency::Emergency>> {
    Json(crate::emergency::snapshot(&state).await)
}

pub async fn telemetry_snapshot(State(state): State<Arc<AppState>>) -> Json<Vec<TelemetryBts>> {
    Json(state.telemetry.read().await.snapshot())
}

/// Per-ISSI RSSI reported directly on the main Brew channel (`SERVICE_RSSI`),
/// as `[issi, rssi_dbfs]` pairs -- separate from the per-Basestation
/// `ms_rssi_out` in `/api/telemetry`, which comes from the Basestation
/// Telemetry WebSocket instead. The dashboard merges both into one "MS RSSI"
/// column.
pub async fn brew_rssi_snapshot(State(state): State<Arc<AppState>>) -> Json<Vec<(u32, f32)>> {
    let t = state.telemetry.read().await;
    Json(t.brew_ms_rssi.iter().map(|(issi, dbfs)| (*issi, *dbfs)).collect())
}

/// The cell a registered subscriber is on, as its station reported it
/// (`SERVICE_MS_CELL`): `{"name":"<login>/<cell>","cell":N,"carrier":N}`.
fn ms_cell_json(inner: &crate::state::Inner, issi: u32, client: crate::state::ClientId) -> Option<serde_json::Value> {
    let cell = inner.carriers.ms_cell(issi, client)?;
    let station = crate::carriers::station_name(client, inner.clients.get(&client).and_then(|c| c.username.as_deref()));
    Some(serde_json::json!({
        "name": crate::carriers::cell_name(&station, cell.cell),
        "cell": cell.cell,
        "carrier": cell.key.carrier,
    }))
}

/// `/api/ms-cells`: every registered subscriber whose cell is known, as
/// `[issi, cell]` pairs (see `ms_cell_json`), for the main dashboard's
/// subscriber list.
pub async fn ms_cells_snapshot(State(state): State<Arc<AppState>>) -> Json<Vec<(u32, serde_json::Value)>> {
    let inner = state.inner.read().await;
    let mut cells: Vec<_> = inner.subscribers.iter()
        .filter_map(|(issi, s)| ms_cell_json(&inner, *issi, s.client_id).map(|c| (*issi, c)))
        .collect();
    cells.sort_unstable_by_key(|(issi, _)| *issi);
    Json(cells)
}

pub async fn registration_log(State(state): State<Arc<AppState>>) -> Json<Vec<crate::telemetry::RegLogRow>> {
    Json(state.telemetry.read().await.registration_log())
}

pub async fn positions_snapshot(State(state): State<Arc<AppState>>) -> Json<Vec<crate::telemetry::PositionFix>> {
    let mut fixes = state.telemetry.read().await.positions();
    // Positions other servers relayed, merged by newest fix per ISSI.
    let inner = state.inner.read().await;
    for (issi, e) in inner.fed.ms_pos.iter().filter(|(_, e)| !e.path.is_empty()) {
        if !crate::fedroute::valid_position(e.lat, e.lon) { continue; }
        let origin = e.path.last().map(|o| crate::fedroute::format_server_id(*o)).unwrap_or_default();
        let fix = crate::telemetry::PositionFix {
            issi: *issi, lat: e.lat, lon: e.lon, at_ms: e.at_ms,
            bts: format!("{} @ {}", e.station, &origin[..origin.len().min(8)]),
            source_text: format!("relayed by federation from server {origin}"), emergency: false,
        };
        match fixes.iter_mut().find(|f| f.issi == *issi) {
            Some(f) if f.at_ms >= fix.at_ms => {}
            Some(f) => *f = fix,
            None => fixes.push(fix),
        }
    }
    fixes.sort_unstable_by(|a, b| b.at_ms.cmp(&a.at_ms));
    drop(inner);
    // Radios with an active emergency are drawn red on the map.
    let emergencies: std::collections::HashSet<u32> = crate::emergency::snapshot(&state).await.iter().map(|e| e.issi).collect();
    for f in &mut fixes { f.emergency = emergencies.contains(&f.issi); }
    Json(fixes)
}

pub async fn map_page() -> Html<&'static str> { Html(MAP_HTML.as_str()) }
pub async fn sms_center_page() -> Html<&'static str> { Html(SMS_CENTER_HTML.as_str()) }
pub async fn sms_center_snapshot(State(state): State<Arc<AppState>>) -> Json<crate::sms_center::Snapshot> { Json(state.sms_center.snapshot()) }
/// Admin only: discard one stored message.
pub async fn sms_center_delete(State(state): State<Arc<AppState>>, Path(id): Path<uuid::Uuid>) -> StatusCode {
    if state.sms_center.delete(id) { StatusCode::NO_CONTENT } else { StatusCode::NOT_FOUND }
}

#[derive(serde::Serialize)]
pub struct BtsLocation {
    /// Unique marker id (the identity, prefixed by the origin server for a remote one).
    pub id: String,
    pub username: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub ip: Option<String>,
    pub connected: bool,
    /// Where the position came from: "telemetry" (reported by the Basestation),
    /// "federation" (relayed by another brew-server) or "config" (static fallback).
    pub source: &'static str,
    /// Origin server id (hex) for a "federation" entry.
    pub origin: Option<String>,
}

/// Basestation positions for the map, best source first: what a Basestation
/// reports over telemetry, what other brew-servers relay, and the static
/// `[bts_locations]` as a fallback for any identity not covered by those.
/// (0, 0) is an unconfigured/default entry (Null Island), not a real fix --
/// same convention the LIP decoder uses for MS positions (see position.rs) --
/// and is never plotted.
pub async fn bts_locations_snapshot(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Json<Vec<BtsLocation>> {
    // `?config_only=1`: just the static entries (the Settings table), all of them.
    let config_only = q.contains_key("config_only");
    let mut out: Vec<BtsLocation> = Vec::new();
    let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();

    for st in state.telemetry.read().await.stations.values().filter(|_| !config_only) {
        let Some((lat, lon)) = st.site_location.filter(|(la, lo)| crate::fedroute::valid_position(*la, *lo)) else { continue };
        covered.insert(st.id.clone());
        out.push(BtsLocation {
            id: st.id.clone(), username: st.id.clone(),
            name: st.site_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| st.id.clone()),
            lat, lon, ip: st.ip.clone(), connected: true, source: "telemetry", origin: None,
        });
    }

    let inner = state.inner.read().await;
    let now = crate::telemetry::now_ms();
    for ((origin, key), b) in inner.fed.bts_remote.iter().filter(|_| !config_only) {
        if !b.online || now.saturating_sub(b.seen_ms) >= crate::fedroute::BTS_STALE_MS { continue; }
        let origin = crate::fedroute::format_server_id(*origin);
        out.push(BtsLocation {
            id: format!("{origin}/{key}"), username: key.clone(),
            name: if b.name.is_empty() { key.clone() } else { b.name.clone() },
            lat: b.lat, lon: b.lon, ip: None, connected: true, source: "federation", origin: Some(origin),
        });
    }

    let locations = state.locations.read().map(|l| l.clone()).unwrap_or_default();
    for (username, loc) in &locations {
        if covered.contains(username) || !crate::fedroute::valid_position(loc.lat, loc.lon) { continue; }
        let live = inner.clients.values().find(|c| c.username.as_deref() == Some(username.as_str()));
        out.push(BtsLocation {
            id: username.clone(), username: username.clone(), name: loc.name.clone(),
            lat: loc.lat, lon: loc.lon,
            ip: live.and_then(|c| c.remote_addr).map(|a| a.ip().to_string()),
            connected: live.is_some(), source: "config", origin: None,
        });
    }
    Json(out)
}

/// JSON snapshot of the SIP subsystem for the live panel. Returns an object
/// with `enabled=false` when SIP is not running, so the page can render a clear
/// disabled state rather than erroring.
/// Live "who's connected right now" snapshot: Brew connections (Basestations
/// and any direct Terminal/brew mobile clients), registered subscribers
/// (every ISSI in `inner.subscribers`, whether registered directly by a
/// Terminal-mode MS or on its behalf by a Basestation gateway -- matches
/// what the main dashboard's "Registered Subscribers" panel shows), and SIP
/// registrations/trunks. Unlike `/api/registrations` (a historical event
/// log), this reflects only what is connected/registered at this instant.
/// The neighbours `cell` reported, named after the directory cell they match
/// (network, carrier, band and offset, LA; an absent LA is the reporting cell's
/// own, TS 100 392-2 table 18.64 note 2), else `carrier N`; `None` when the
/// station did not report its neighbours.
fn neighbour_names(directory: &[crate::carriers::DirectoryEntry], cell: &crate::carriers::Cell) -> Option<Vec<String>> {
    let list = cell.extra.neighbours.as_ref()?;
    Some(list.iter().map(|n| {
        directory.iter()
            .filter(|e| e.cell.key.same_downlink(&crate::carriers::CarrierKey { mcc: n.mcc, mnc: n.mnc, carrier: n.carrier, carrier_ext: n.carrier_ext }))
            .find(|e| n.location_area.unwrap_or(cell.location_area) == e.cell.location_area)
            .map(|e| crate::carriers::cell_name(&e.station, e.cell.cell))
            .unwrap_or_else(|| format!("carrier {}", n.carrier))
    }).collect())
}

pub async fn connections_snapshot(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let (brew_clients, mobile_stations, cells, duplicate_carriers, handovers_pending, roaming_log) = {
        let inner = state.inner.read().await;
        let station = |id: crate::state::ClientId| {
            crate::carriers::station_name(id, inner.clients.get(&id).and_then(|c| c.username.as_deref()))
        };
        let issi_count = |id: crate::state::ClientId| inner.subscribers.values().filter(|s| s.client_id == id).count();
        let brew_clients: Vec<_> = inner.clients.iter().map(|(id, c)| {
            let issi_count = issi_count(*id);
            let info = inner.carriers.info(*id);
            let cells: Vec<_> = info.map(|i| i.cells.iter().map(|cell| serde_json::json!({
                "name": crate::carriers::cell_name(&station(*id), cell.cell),
                "mcc": cell.key.mcc,
                "mnc": cell.key.mnc,
                "carrier": cell.key.carrier,
                "carrier_ext": cell.key.carrier_ext,
                "location_area": cell.location_area,
                "duplicate": inner.carriers.is_duplicate(&cell.key),
                "serving": inner.carriers.owner(&cell.key) == Some(*id),
            })).collect()).unwrap_or_default();
            serde_json::json!({
                "id": id.to_string(),
                "station": station(*id),
                "mode": c.mode.as_str(),
                "version": c.version.as_u8(),
                "agent": inner.client_agents.get(id),
                "remote_addr": c.remote_addr.map(|a| a.to_string()),
                "connected_at_ms": c.connected_at_ms,
                "registered_issis": issi_count,
                "cells": cells,
                "capabilities": info.map(|i| i.capabilities.clone()).unwrap_or_default(),
            })
        }).collect();
        let mobile_stations: Vec<_> = inner.subscribers.iter()
            .map(|(issi, s)| {
                let basestation = inner.clients.get(&s.client_id);
                serde_json::json!({
                    "issi": issi,
                    "mode": s.mode.as_str(),
                    "basestation_id": s.client_id.to_string(),
                    "basestation_name": station(s.client_id),
                    "basestation_addr": basestation.and_then(|c| c.remote_addr).map(|a| a.to_string()),
                    // The cell the station reported the radio on (`SERVICE_MS_CELL`).
                    "cell": ms_cell_json(&inner, *issi, s.client_id),
                    "groups": s.groups.iter().copied().collect::<Vec<_>>(),
                })
            }).collect();
        // Every announced cell, one row each: the overview of the radio network.
        let directory = inner.carriers.directory(station);
        let mut cells: Vec<_> = inner.carriers.announcements().flat_map(|(id, info)| {
            let client = inner.clients.get(&id);
            let station = station(id);
            let issis = issi_count(id);
            info.cells.iter().map(move |cell| (station.clone(), cell, id, client, issis, &info.capabilities))
        }).map(|(station, cell, id, client, issis, capabilities)| serde_json::json!({
            "name": crate::carriers::cell_name(&station, cell.cell),
            "station": station,
            "cell": cell.cell,
            "mcc": cell.key.mcc,
            "mnc": cell.key.mnc,
            "carrier": cell.key.carrier,
            "carrier_ext": cell.key.carrier_ext,
            "downlink_hz": cell.key.downlink_hz(),
            "location_area": cell.location_area,
            "capabilities": capabilities,
            "basestation_id": id.to_string(),
            "remote_addr": client.and_then(|c| c.remote_addr).map(|a| a.to_string()),
            "connected_at_ms": client.map(|c| c.connected_at_ms),
            "registered_issis": issis,
            "duplicate": inner.carriers.is_duplicate(&cell.key),
            "serving": inner.carriers.owner(&cell.key) == Some(id),
            "online": true,
            "rxlev_access_min_dbm": cell.extra.rxlev_access_min_dbm,
            "ms_txpwr_max_dbm": cell.extra.ms_txpwr_max_dbm,
            "neighbours": neighbour_names(&directory, cell),
        })).collect();
        // Cells of stations with a login that went away (cell directory).
        cells.extend(directory.iter().filter(|e| e.client.is_none()).map(|e| serde_json::json!({
            "name": crate::carriers::cell_name(&e.station, e.cell.cell),
            "station": e.station,
            "cell": e.cell.cell,
            "mcc": e.cell.key.mcc,
            "mnc": e.cell.key.mnc,
            "carrier": e.cell.key.carrier,
            "carrier_ext": e.cell.key.carrier_ext,
            "downlink_hz": e.cell.key.downlink_hz(),
            "location_area": e.cell.location_area,
            "capabilities": [],
            "registered_issis": 0,
            "duplicate": false,
            "serving": false,
            "online": false,
            "last_seen_ms": e.last_seen_ms,
            "rxlev_access_min_dbm": e.cell.extra.rxlev_access_min_dbm,
            "ms_txpwr_max_dbm": e.cell.extra.ms_txpwr_max_dbm,
            "neighbours": neighbour_names(&directory, &e.cell),
        })));
        cells.sort_by(|a, b| (a["station"].as_str(), a["cell"].as_u64()).cmp(&(b["station"].as_str(), b["cell"].as_u64())));
        (brew_clients, mobile_stations, cells, inner.carriers.duplicates(), inner.handovers.pending_count(), inner.roaming_log.newest_first())
    };

    let sip = match state.sip_snapshot().await {
        Some(snap) => serde_json::json!({
            "enabled": snap.enabled,
            "registrations": snap.registrations,
            "trunks": snap.trunks,
        }),
        None => serde_json::json!({ "enabled": false, "registrations": [], "trunks": [] }),
    };

    Json(serde_json::json!({
        "brew_clients": brew_clients,
        "mobile_stations": mobile_stations,
        "cells": cells,
        "duplicate_carriers": duplicate_carriers,
        "handovers_pending": handovers_pending,
        "roaming_log": roaming_log,
        "sip": sip,
    }))
}

pub async fn sip_snapshot(State(state): State<Arc<AppState>>) -> Response {
    match state.sip_snapshot().await {
        Some(snap) => Json(snap).into_response(),
        None => Json(serde_json::json!({
            "enabled": false,
            "listen": "",
            "realm": "",
            "registrations": [],
            "trunks": [],
            "active_calls": [],
            "total_calls": 0,
            "total_registrations": 0,
        })).into_response(),
    }
}

/// JSON view of the *provisioned* SIP configuration (extensions, trunks,
/// routes) as loaded from the config file. Passwords are redacted. This backs
/// the read-only config screen; edits are made in the TOML file, which the
/// running process watches and reloads.
pub async fn sip_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let sip = &state.config.sip;
    let extensions: Vec<_> = sip.extensions.iter().map(|(user, e)| serde_json::json!({
        "user": user,
        "display_name": e.display_name,
        "issi": e.issi,
        "allow_outbound": e.allow_outbound,
        "has_password": !e.password.is_empty(),
    })).collect();
    let trunks: Vec<_> = sip.trunks.iter().map(|(name, t)| serde_json::json!({
        "name": name,
        "direction": format!("{:?}", t.direction).to_lowercase(),
        "remote_host": t.remote_host,
        "username": if t.username.is_empty() { name.clone() } else { t.username.clone() },
        "realm": t.realm,
        "register_interval_seconds": t.register_interval_seconds,
        "enabled": t.enabled,
        "has_password": !t.password.is_empty(),
    })).collect();
    let routes: Vec<_> = sip.routes.iter().map(|r| serde_json::json!({
        "name": r.name,
        "match_pattern": r.match_pattern,
        "strip_prefix": r.strip_prefix,
        "from": r.from.as_ref().map(describe_endpoint),
        "to": r.to.as_ref().map(describe_endpoint),
        "enabled": r.enabled,
    })).collect();
    Json(serde_json::json!({
        "enabled": sip.enabled,
        "listen": sip.listen.to_string(),
        "advertised_host": sip.advertised_host,
        "realm": sip.realm,
        "rtp_port_min": sip.rtp_port_min,
        "rtp_port_max": sip.rtp_port_max,
        "registration_ttl_seconds": sip.registration_ttl_seconds,
        "extensions": extensions,
        "trunks": trunks,
        "routes": routes,
    }))
}

/// Renders a route endpoint config as a short human string for the config page.
fn describe_endpoint(ep: &crate::config::RouteEndpoint) -> String {
    use crate::config::RouteEndpoint::*;
    match ep {
        SipExtension { user } => format!("ext:{user}"),
        SipTrunk { trunk, number } if number.is_empty() => format!("trunk:{trunk}"),
        SipTrunk { trunk, number } => format!("trunk:{trunk}/{number}"),
        BrewPrivate { issi } => format!("issi:{issi}"),
        BrewGroup { gssi } => format!("group:{gssi}"),
    }
}

pub async fn sip_page() -> Html<&'static str> { Html(SIP_HTML.as_str()) }
pub async fn sip_config_page() -> Html<&'static str> { Html(SIP_CONFIG_HTML.as_str()) }
pub async fn settings_page() -> Html<&'static str> { Html(SETTINGS_HTML.as_str()) }

/// Re-serializes `cfg` to TOML, round-trip-validates it by parsing it back
/// (belt and braces: catches anything `to_toml_pretty` itself can't express),
/// and writes it to the process's config file. The already-running
/// `config_watcher` picks up the mtime change within ~2s and restarts the
/// process so the new config takes effect; this function does not restart
/// anything itself.
pub(crate) async fn save_config(state: &Arc<AppState>, cfg: &config::Config) -> anyhow::Result<()> {
    let text = cfg.to_toml_pretty()?;
    config::Config::parse(&text)?;
    config::Config::save_atomic(&state.config_path, &text)?;
    Ok(())
}

/// Applies `edit` to a clone of the live config and saves it. Used by every
/// structured (non-raw-editor) settings endpoint below so they share one
/// clone/edit/validate/write/report path.
/// What a successful config save tells the admin: with HA the file is only
/// applied from the /ha page (no automatic restart, see `main`).
fn saved_note(state: &AppState) -> serde_json::Value {
    let note = if state.config.ha.enabled {
        "written to the config file; HA is enabled, so apply it from the High Availability page"
    } else {
        "written to the config file; the process restarts within a couple seconds to apply it"
    };
    serde_json::json!({ "saved": true, "note": note })
}

async fn mutate_and_save(
    state: &Arc<AppState>,
    edit: impl FnOnce(&mut config::Config),
) -> Response {
    let mut cfg = state.config_snapshot();
    edit(&mut cfg);
    match save_config(state, &cfg).await {
        Ok(()) => Json(saved_note(state)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

/// Unredacted JSON view of `[sip]` (real passwords included, unlike
/// `sip_config`'s summary), so the settings editor can prefill edit forms
/// with actual current values instead of a "set/none" placeholder. Safe here
/// for the same reason `config_raw_get` is: every route on this router sits
/// behind `require_basic`.
pub async fn sip_config_full(State(state): State<Arc<AppState>>) -> Json<crate::config::SipConfig> {
    Json(state.config.sip.clone())
}

/// Full config as TOML text, for the raw editor. Unlike `sip_config`'s
/// redacted summary, this includes real secrets (trunk/extension passwords,
/// dashboard/auth user passwords) — acceptable because every route on this
/// router already sits behind `require_basic`, the same gate protecting the
/// rest of the admin surface.
pub async fn config_raw_get(State(state): State<Arc<AppState>>) -> Response {
    match state.config_snapshot().to_toml_pretty() {
        Ok(text) => (StatusCode::OK, text).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Validates and saves a full replacement config submitted as raw TOML text
/// (the editor's "Save" button). This is the only path that can touch every
/// setting, including ones with no dedicated form (listen addresses, TLS,
/// dashboard/auth/telemetry/control users, storage, call-routing flags).
pub async fn config_raw_put(State(state): State<Arc<AppState>>, body: String) -> Response {
    let cfg = match config::Config::parse(&body) {
        Ok(cfg) => cfg,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("invalid config: {e:#}")).into_response(),
    };
    match save_config(&state, &cfg).await {
        Ok(()) => Json(saved_note(&state)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn upsert_sip_extension(
    State(state): State<Arc<AppState>>,
    Path(user): Path<String>,
    Json(ext): Json<crate::config::SipExtensionConfig>,
) -> Response {
    mutate_and_save(&state, |cfg| { cfg.sip.extensions.insert(user, ext); }).await
}

pub async fn delete_sip_extension(State(state): State<Arc<AppState>>, Path(user): Path<String>) -> Response {
    mutate_and_save(&state, |cfg| { cfg.sip.extensions.remove(&user); }).await
}

pub async fn upsert_sip_trunk(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(trunk): Json<crate::config::SipTrunkConfig>,
) -> Response {
    mutate_and_save(&state, |cfg| { cfg.sip.trunks.insert(name, trunk); }).await
}

pub async fn delete_sip_trunk(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    mutate_and_save(&state, |cfg| { cfg.sip.trunks.remove(&name); }).await
}

/// The ISSI blacklist as enforced now.
pub async fn blacklist_get(State(state): State<Arc<AppState>>) -> Json<Vec<u32>> {
    Json(state.blocked_list())
}

/// Blocks an ISSI. Applied to the running server at once and written to the
/// config file without a restart (a restart would drop every call).
pub async fn blacklist_add(State(state): State<Arc<AppState>>, Path(issi): Path<u32>) -> Response {
    set_blacklisted(&state, issi, true).await
}

pub async fn blacklist_remove(State(state): State<Arc<AppState>>, Path(issi): Path<u32>) -> Response {
    set_blacklisted(&state, issi, false).await
}

async fn set_blacklisted(state: &Arc<AppState>, issi: u32, blocked: bool) -> Response {
    match crate::blacklist::apply(state, issi, blocked, "the dashboard").await {
        Ok(()) => Json(serde_json::json!({ "saved": true, "note": "applied immediately; saved to the config file without a restart" })).into_response(),
        Err(crate::blacklist::ApplyError::Invalid) => (StatusCode::BAD_REQUEST, "ISSI must be 1-16777215").into_response(),
        Err(crate::blacklist::ApplyError::Save(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("applied, but saving the config file failed: {e}")).into_response(),
    }
}

/// The ISSI allowlist as enforced now: `{"enabled": bool, "issis": [...]}`.
pub async fn allowlist_get(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let (enabled, issis) = state.allowlist_snapshot();
    Json(serde_json::json!({ "enabled": enabled, "issis": issis }))
}

/// Admits an ISSI. Applied to the running server at once and written to the
/// config file without a restart (a restart would drop every call).
pub async fn allowlist_add(State(state): State<Arc<AppState>>, Path(issi): Path<u32>) -> Response {
    allowlist_result(crate::allowlist::apply_issi(&state, issi, true, "the dashboard").await)
}

pub async fn allowlist_remove(State(state): State<Arc<AppState>>, Path(issi): Path<u32>) -> Response {
    allowlist_result(crate::allowlist::apply_issi(&state, issi, false, "the dashboard").await)
}

#[derive(serde::Deserialize)]
pub struct AllowlistEnabled {
    pub enabled: bool,
}

/// Turns enforcement of the allowlist on or off, live.
pub async fn allowlist_set_enabled(State(state): State<Arc<AppState>>, Json(body): Json<AllowlistEnabled>) -> Response {
    allowlist_result(crate::allowlist::apply_enabled(&state, body.enabled, "the dashboard").await)
}

fn allowlist_result(result: Result<(), crate::allowlist::ApplyError>) -> Response {
    match result {
        Ok(()) => Json(serde_json::json!({ "saved": true, "note": "applied immediately; saved to the config file without a restart" })).into_response(),
        Err(crate::allowlist::ApplyError::Invalid) => (StatusCode::BAD_REQUEST, "ISSI must be 1-16777215").into_response(),
        Err(crate::allowlist::ApplyError::Save(e)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("applied, but saving the config file failed: {e}")).into_response(),
    }
}

/// Adds or replaces a `[bts_locations]` entry, keyed by the same numeric Brew
/// username the Basestation authenticates with under `[auth.users]`.
pub async fn upsert_bts_location(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    Json(loc): Json<crate::config::BtsLocationConfig>,
) -> Response {
    mutate_and_save(&state, |cfg| { cfg.bts_locations.insert(username, loc); }).await
}

pub async fn delete_bts_location(State(state): State<Arc<AppState>>, Path(username): Path<String>) -> Response {
    mutate_and_save(&state, |cfg| { cfg.bts_locations.remove(&username); }).await
}

/// Adds or replaces (matched by `name`) a voice route. Routes are order-
/// sensitive (`SipConfig::routes` is evaluated top to bottom), so an update
/// keeps the existing position and only a new name appends at the end;
/// reordering is left to the raw editor.
pub async fn upsert_sip_route(
    State(state): State<Arc<AppState>>,
    Json(route): Json<crate::config::VoiceRouteConfig>,
) -> Response {
    mutate_and_save(&state, |cfg| {
        match cfg.sip.routes.iter_mut().find(|r| r.name == route.name) {
            Some(existing) => *existing = route,
            None => cfg.sip.routes.push(route),
        }
    }).await
}

pub async fn delete_sip_route(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    mutate_and_save(&state, |cfg| { cfg.sip.routes.retain(|r| r.name != name); }).await
}

pub async fn control_list(State(state): State<Arc<AppState>>) -> Json<Vec<String>> {
    Json(state.control.read().await.connected_ids())
}

pub async fn control_command(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(command): Json<ControlCommand>,
) -> Response {
    match control::send_command(&state, &id, command).await {
        Ok(response) => Json(serde_json::json!({"sent": true, "response": response})).into_response(),
        Err(SendError::NotConnected) => (StatusCode::NOT_FOUND, "control BTS not connected").into_response(),
        Err(SendError::Timeout) => (StatusCode::GATEWAY_TIMEOUT, "no response from BTS").into_response(),
    }
}

/// Shared CSS for the dashboard and its sub-pages, so the standalone log pages
/// match the main dashboard exactly.
const STYLE: &str = r#"<style>
:root{font-family:Inter,system-ui,sans-serif;color:#e7edf5;background:#09111c}*{box-sizing:border-box}body{margin:0}header{padding:22px 28px;border-bottom:1px solid #203047;display:flex;justify-content:space-between;align-items:center}h1{font-size:20px;margin:0}h1.srvname{text-transform:uppercase}.muted{color:#8fa2b8}.wrap{padding:24px;max-width:1500px;margin:auto}.cards{display:grid;grid-template-columns:repeat(6,1fr);gap:12px}.card,.panel{background:#101b2a;border:1px solid #203047;border-radius:12px}.card{padding:16px}.n{font-size:28px;font-weight:700;margin-top:6px}.panel{margin-top:16px;padding:18px}h2{font-size:14px;text-transform:uppercase;letter-spacing:.08em;color:#8fa2b8;margin:0 0 14px}table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:10px;border-bottom:1px solid #1c2a3c;font-size:13px}th{color:#8fa2b8}.pill{padding:3px 8px;border-radius:99px;background:#203047}.live{display:inline-block;width:8px;height:8px;border-radius:50%;background:#52d273;margin-right:7px}.hdr-status{display:flex;flex-direction:column;align-items:flex-end;gap:2px}.ver{font-size:11px;color:#8fa2b8}.hdr-user{display:flex;align-items:center;gap:8px;font-size:12px;color:#cfe0f2}#logout-btn,#login-btn{display:none;background:#203047;color:#e7edf5;border:1px solid #2c405c;border-radius:6px;padding:3px 9px;font-size:11px;cursor:pointer}#logout-btn:hover,#login-btn:hover{background:#2c405c}body.not-admin .admin-only{display:none!important}@media(max-width:900px){.cards{grid-template-columns:repeat(2,1fr)}.wrap{padding:12px}}
.health-ok{background:#173822;color:#52d273}.health-degraded{background:#3a2f12;color:#e8b93d}.health-critical{background:#3a1414;color:#f2545b}.health-unknown{background:#203047;color:#8fa2b8}
.bts-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:12px}.bts-grid.wide{grid-template-columns:repeat(auto-fill,minmax(min(480px,100%),1fr))}.bts-card{min-width:0}.bts-card{background:#0d1826;border:1px solid #203047;border-radius:10px;padding:14px}.bts-card h3{margin:0;font-size:15px}.bts-meta{font-size:12px;margin-top:4px}.bts-card table{margin-top:10px}.bts-card th,.bts-card td{padding:6px;font-size:12px}
.emg-ribbon{display:none;position:sticky;top:0;z-index:50;align-items:center;gap:10px;flex-wrap:wrap;background:#c62828;color:#fff;padding:10px 24px;font-weight:700;border-bottom:2px solid #ff8a80}.emg-ribbon.on{display:flex}.emg-dot{width:10px;height:10px;border-radius:50%;background:#fff;animation:emgb 1.6s ease-in-out infinite}.emg-chip{background:rgba(255,255,255,.2);border-radius:4px;padding:2px 8px;font-weight:600;font-size:13px}@keyframes emgb{50%{opacity:.3}}
.banner{display:none;background:#3a1414;border:1px solid #f2545b;color:#ffb4b8;padding:12px 18px;border-radius:10px;margin-bottom:16px;font-weight:600}
.ctl-row{display:flex;gap:6px;align-items:center;margin-top:8px;flex-wrap:wrap}.ctl-row input{background:#0d1826;border:1px solid #203047;color:#e7edf5;border-radius:6px;padding:5px 8px;font-size:12px;width:auto}.ctl-row label{font-size:12px;display:flex;align-items:center;gap:4px}.ctl-row button{background:#203047;color:#e7edf5;border:1px solid #2c405c;border-radius:6px;padding:5px 10px;font-size:12px;cursor:pointer}.ctl-row button:hover{background:#2c405c}.ctl-result{font-size:12px;margin-top:8px;word-break:break-all}
.pager{display:flex;align-items:center;gap:10px;margin-top:12px;font-size:12px;color:#8fa2b8}.pager button{background:#203047;color:#e7edf5;border:1px solid #2c405c;border-radius:6px;padding:4px 10px;font-size:12px;cursor:pointer}.pager button:hover:not(:disabled){background:#2c405c}.pager button:disabled{opacity:.4;cursor:default}.pager .pginfo{min-width:120px}
.ts-wrap{margin-top:10px}.ts-carrier{display:flex;align-items:center;gap:6px;margin-top:5px}.ts-carrier .lbl{font-size:11px;color:#8fa2b8;min-width:64px}.ts-slots{display:flex;gap:4px}.ts-slot{width:34px;height:22px;border-radius:4px;border:1px solid #203047;display:flex;align-items:center;justify-content:center;font-size:10px;font-weight:600}.ts-free{background:#0d1826;color:#3f5a78}.ts-busy{background:#173822;color:#52d273;border-color:#245c37}.ts-legend{display:flex;gap:12px;font-size:11px;color:#8fa2b8;margin-top:6px}.ts-legend span{display:inline-flex;align-items:center;gap:4px}.ts-dot{width:10px;height:10px;border-radius:2px;display:inline-block}
.cell-row{margin-top:8px;padding-top:8px;border-top:1px solid #203047;font-size:12px;min-width:0}.cell-head{color:#cfe0f2;line-height:1.5;overflow-wrap:anywhere}.cell-dev{display:inline-block;max-width:140px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;vertical-align:bottom;color:#8fa2b8;font-family:ui-monospace,monospace}.cell-carriers{display:flex;flex-wrap:wrap;gap:5px;margin-top:5px}.cell-carrier{background:#0a1320;border:1px solid #203047;border-radius:5px;padding:2px 7px;font-family:ui-monospace,monospace;color:#8fa2b8;white-space:nowrap}.cell-carrier b{color:#cfe0f2;font-weight:600}
.reg-list{display:flex;flex-wrap:wrap;gap:5px;margin-top:8px}.reg-issi{background:#0d1826;border:1px solid #203047;border-radius:5px;padding:3px 7px;font-size:12px;font-family:ui-monospace,monospace;color:#cfe0f2}.reg-count{font-size:12px;color:#8fa2b8}
.navlinks{display:flex;gap:12px;flex-wrap:wrap}.navlink{display:block;background:#0d1826;border:1px solid #203047;border-radius:10px;padding:14px 18px;color:#cfe0f2;text-decoration:none;font-size:14px;font-weight:600;transition:background .1s}.navlink:hover{background:#16273c;border-color:#2c405c}.navlink .sub{display:block;font-size:12px;font-weight:400;color:#8fa2b8;margin-top:4px}
.backlink{color:#8fa2b8;text-decoration:none;font-size:13px}.backlink:hover{color:#cfe0f2}
h2 .backlink{text-transform:none;letter-spacing:normal;margin-left:8px}
.badge{display:inline-block;padding:2px 7px;border-radius:99px;font-size:11px;font-weight:600}.badge-pos{background:#123047;color:#5cc0f2;border:1px solid #1d4a66}.badge-sds{background:#203047;color:#8fa2b8}.pos-undec{color:#8fa2b8;font-style:italic}
.badge-reg-in{background:#173822;color:#52d273;border:1px solid #245c37}.badge-reg-out{background:#203047;color:#8fa2b8;border:1px solid #2c405c}.badge-reg-timeout{background:#3a2f12;color:#e8b93d;border:1px solid #5c4a1d}
.ha-active{background:#173822;color:#52d273;border:1px solid #245c37}.ha-standby{background:#123047;color:#5cc0f2;border:1px solid #1d4a66}.ha-init{background:#203047;color:#8fa2b8}.ha-fault,.ha-down{background:#3a1414;color:#f2545b;border:1px solid #5c1d1d}a.ha-hdr{text-decoration:none;font-size:11px}
/* Embedded in the new dashboard (?embed=1): no header, its colours. */
html.embed{--e-bg:#eceff4;--e-bg2:#ffffff;--e-bg3:#f3f5f9;--e-border:#dde3ec;--e-sep:rgba(20,30,50,.09);--e-text:#16202e;--e-text2:#3d4f66;--e-text3:#5f7188;--e-accent:#00876a;--e-link:#1565c0}
@media (prefers-color-scheme:dark){html.embed:not([data-theme=light]){--e-bg:#090d14;--e-bg2:#111824;--e-bg3:#19212f;--e-border:#232e40;--e-sep:rgba(255,255,255,.07);--e-text:#eef3fb;--e-text2:#94abc9;--e-text3:#7f93b5;--e-accent:#00d4a8;--e-link:#4da6ff}}
html.embed[data-theme=dark]{--e-bg:#090d14;--e-bg2:#111824;--e-bg3:#19212f;--e-border:#232e40;--e-sep:rgba(255,255,255,.07);--e-text:#eef3fb;--e-text2:#94abc9;--e-text3:#7f93b5;--e-accent:#00d4a8;--e-link:#4da6ff}
html.embed #map{height:calc(100vh - 80px)!important}
html.embed,html.embed body{background:var(--e-bg)!important;color:var(--e-text)!important;font-family:-apple-system,BlinkMacSystemFont,Inter,'Segoe UI',system-ui,sans-serif}
html.embed header,html.embed .backlink,html.embed p:has(> .backlink),html.embed .emg-ribbon{display:none!important}
html.embed .wrap{padding:0 2px 24px;max-width:none}
html.embed .panel,html.embed .card,html.embed .bts-card,html.embed .navlink{background:var(--e-bg2)!important;border-color:var(--e-border)!important;color:var(--e-text)!important;box-shadow:0 1px 3px rgba(20,30,50,.08)}
html.embed .panel{margin-top:14px}
html.embed h2{color:var(--e-text)!important;font-size:15px;text-transform:none;letter-spacing:0;font-weight:650}
html.embed th{color:var(--e-text3)!important;font-size:11px;text-transform:uppercase;letter-spacing:.04em}
html.embed th,html.embed td{border-bottom-color:var(--e-sep)!important}
html.embed .muted,html.embed .ver,html.embed .pager{color:var(--e-text2)!important}
html.embed a{color:var(--e-link)}
html.embed input,html.embed select,html.embed textarea{background:var(--e-bg3)!important;color:var(--e-text)!important;border:1px solid var(--e-border)!important;border-radius:8px;padding:6px 9px;font:inherit}
html.embed button,html.embed .pager button,html.embed .ctl-row button{background:var(--e-bg2)!important;color:var(--e-text)!important;border:1px solid var(--e-border)!important;border-radius:8px;padding:6px 12px;font-weight:600;cursor:pointer}
html.embed .pill,html.embed .reg-issi,html.embed .cell-carrier{background:var(--e-bg3)!important;color:var(--e-text2)!important;border-color:var(--e-border)!important}
</style><script>
if(new URLSearchParams(location.search).has('embed')){document.documentElement.classList.add('embed');const t=new URLSearchParams(location.search).get('theme');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t;}
// Admin login/logout, shared by every page header. The dashboard can be
// read-only to anonymous visitors (open mode); logging in only unlocks
// config editing and Basestation control. doLogin() navigates to the
// admin-gated /login, which makes the browser prompt and then bounces back
// here; doLogout() is a best-effort HTTP-Basic "logout" (there's no
// server-side session to end) that reloads the current page unauthenticated.
function doLogin(){location.href='/login?next='+encodeURIComponent(location.pathname+location.search);}
function doLogout(){location.href=location.protocol+'//logout:'+Date.now()+'@'+location.host+location.pathname;}
document.addEventListener('DOMContentLoaded',()=>{const h=document.querySelector('.hdr-status');if(!h)return;
// Start read-only (CSS hides .admin-only controls) so nothing privileged
// flashes before /api/whoami answers; reveal only once confirmed admin.
document.body.classList.add('not-admin');
// Then populate login state everywhere: hide the Settings link from
// non-admins and show the username + Logout when signed in, or a Login
// button when the dashboard is open and nobody's signed in. Server-side
// guards are the real enforcement; this is just the UI.
fetch('/api/whoami').then(r=>r.json()).then(w=>{
  if(w.admin){document.body.classList.remove('not-admin');document.body.classList.add('is-admin');}
  const sl=document.getElementById('settings-link');if(sl&&!w.admin)sl.style.display='none';
  const who=document.getElementById('whoami'),lin=document.getElementById('login-btn'),lout=document.getElementById('logout-btn');
  if(w.username){if(who)who.textContent=w.username;if(lout)lout.style.display='inline-block';}
  else if(w.can_login){if(lin)lin.style.display='inline-block';}
}).catch(()=>{});
// HA role badge in every page header (only when [ha] is enabled).
const tick=()=>fetch('/api/ha').then(r=>r.json()).then(d=>{let a=document.getElementById('ha-hdr');if(!d.enabled){if(a)a.remove();return;}
if(!a){a=document.createElement('a');a.id='ha-hdr';a.href='/ha';h.insertBefore(a,h.querySelector('.ver'));}
a.className='badge ha-hdr ha-'+d.role;a.textContent='HA '+d.node+': '+d.role.toUpperCase();}).catch(()=>{});
tick();setInterval(tick,5000);});
</script>"#;

const HTML: &str = r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>__SERVERNAME__</title>__STYLE__</head><body><div class=emg-ribbon id=emergency-banner role=alert><span class=emg-dot></span><span>EMERGENCY ACTIVE</span><span id=emergency-list style="display:flex;flex-wrap:wrap;gap:8px"></span></div><header><h1 class=srvname>__SERVERNAME__</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v__VERSION__</div></div></header><main class=wrap>
<section class=cards><div class=card><div class=muted>Basestations</div><div class=n id=bs>-</div></div><div class=card><div class=muted>Subscribers</div><div class=n id=subs>-</div></div><div class=card><div class=muted>Groups</div><div class=n id=groups>-</div></div><div class=card><div class=muted>Active calls</div><div class=n id=active>-</div></div><div class=card><div class=muted>Total calls</div><div class=n id=calls>-</div></div><div class=card><div class=muted>SDS</div><div class=n id=sds>-</div></div></section><section class=panel><h2>Live calls</h2><table><thead><tr><th>Type</th><th>From</th><th>To</th><th>Priority</th><th>Duration</th><th>Voice frames</th><th>MS RSSI</th><th>UUID</th></tr></thead><tbody id=livecalls></tbody></table></section><section class=panel><h2>Menu</h2><div class=navlinks><a class=navlink href="/calls">Recent calls<span class=sub>Completed call history</span></a><a class=navlink href="/sds">Recent SDS<span class=sub>Short data messages</span></a><a class=navlink href="/telemetry-sds">Telemetry SDS Log<span class=sub>Per-Basestation SDS stream</span></a><a class=navlink href="/map">MS Map<span class=sub>Plot positioned mobiles</span></a><a class=navlink href="/sms-center">SMS Center<span class=sub>Messages stored for offline radios</span></a><a class=navlink href="/connections">Live Connections<span class=sub>Who's connected now: Brew, MS &amp; SIP</span></a><a class=navlink href="/sip">SIP / VoIP<span class=sub>Registrations, trunks &amp; calls</span></a><a class=navlink href="/sip-config">SIP Config<span class=sub>Extensions, trunks &amp; routes</span></a><a class=navlink href="/ha">High Availability<span class=sub>Active/standby role, VIP &amp; failover</span></a><a class=navlink id=settings-link href="/settings">Settings<span class=sub>Edit &amp; save server configuration</span></a></div></section>
<section class=panel><h2>Basestation Telemetry</h2><div class="bts-grid wide" id=telemetry-stations></div></section>
<section class=panel><h2>Registered Subscribers <a class=backlink href="/registrations">(view registration log &rarr;)</a></h2><div class=bts-grid id=registrations></div></section>
<section class="panel admin-only"><h2>Basestation Control</h2><div class=bts-grid id=control-stations></div></section>
</main><script>
let snap=null;let tsnap=null;let brssi={};let bcell={};const $=id=>document.getElementById(id);const dt=x=>new Date(x).toLocaleTimeString();const dur=(a,b)=>Math.max(0,Math.floor(((b||Date.now())-a)/1000))+'s';const esc=s=>String(s??'').replace(/[&<>]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;'}[c]));
function render(s){snap=s;$('bs').textContent=s.connected_basestations;$('subs').textContent=s.subscribers;$('groups').textContent=s.groups;$('active').textContent=s.active_calls.length;$('calls').textContent=s.total_calls;$('sds').textContent=s.total_sds;
// Build an ISSI -> RSSI (dBFS) lookup from the latest telemetry snapshot so each
// live call can show its mobile station's received signal strength. RSSI is not
// SNR; it is the real per-MS metric Basestation reports.
const rssiByIssi={};(tsnap||[]).forEach(st=>(st.ms_rssi_out||[]).forEach(([issi,dbfs])=>{rssiByIssi[issi]=dbfs;}));
Object.assign(rssiByIssi,brssi); // per-ISSI RSSI reported directly on the main Brew channel (SERVICE_RSSI)
const rssiCell=issi=>rssiByIssi[issi]!=null?`${rssiByIssi[issi].toFixed(1)} dBFS`:'<span class=muted>&ndash;</span>';
$('livecalls').innerHTML=s.active_calls.map(c=>`<tr><td><span class=pill>${c.kind}</span></td><td>${c.source}</td><td>${c.destination}</td><td>${c.priority}</td><td>${dur(c.started_at_ms)}</td><td>${c.voice_frames}</td><td>${rssiCell(c.source)}</td><td class=muted>${c.uuid.slice(0,8)}</td></tr>`).join('')||'<tr><td colspan=8 class=muted>No active calls</td></tr>';}
async function refresh(){try{render(await(await fetch('/api/status')).json())}catch(e){$('status').textContent='Disconnected'}}
function healthPill(level){const cls=level==='ok'?'health-ok':level==='degraded'?'health-degraded':level==='critical'?'health-critical':'health-unknown';return `<span class="pill ${cls}">${level||'unknown'}</span>`;}
// Build a small timeslot occupancy grid from the station's active calls. TETRA
// carriers are TDMA with 4 timeslots each (TS1-TS4). A slot is "busy" when an
// active call occupies that carrier/timeslot; every other slot on a carrier
// that is in use is shown as "available". Only carriers that appear in at least
// one active call are drawn (we cannot know the full carrier plan otherwise).
const TS_PER_CARRIER=4;
function tsGrid(calls){
  if(!calls.length)return '';
  // Map carrier_num -> {ts -> speaker issi/dest} for busy slots.
  const carriers=new Map();
  calls.forEach(c=>{
    if(!carriers.has(c.carrier_num))carriers.set(c.carrier_num,new Map());
    carriers.get(c.carrier_num).set(c.ts,c);
  });
  const rows=[...carriers.keys()].sort((a,b)=>a-b).map(cn=>{
    const busy=carriers.get(cn);
    const slots=[];
    for(let ts=1;ts<=TS_PER_CARRIER;ts++){
      const c=busy.get(ts);
      if(c){
        const who=c.is_group?('grp '+c.gssi_or_called):('to '+c.gssi_or_called);
        slots.push(`<div class="ts-slot ts-busy" title="Carrier ${cn} TS${ts}: ${esc(String(c.source_issi))} \u2192 ${esc(who)}">TS${ts}</div>`);
      }else{
        slots.push(`<div class="ts-slot ts-free" title="Carrier ${cn} TS${ts}: available">TS${ts}</div>`);
      }
    }
    return `<div class=ts-carrier><span class=lbl>Carrier ${cn}</span><div class=ts-slots>${slots.join('')}</div></div>`;
  }).join('');
  return `<div class=ts-wrap>${rows}<div class=ts-legend><span><span class="ts-dot" style="background:#173822;border:1px solid #245c37"></span>busy</span><span><span class="ts-dot" style="background:#0d1826;border:1px solid #203047"></span>available</span></div></div>`;
}
const mhz=hz=>(hz/1e6).toFixed(4);
const rfPill=st=>{const c={online:'#5fd08a',starting:'#e0b050',error:'#ff6b6b',offline:'#8fa2b8'}[st]||'#8fa2b8';return `<span style="color:${c}">${esc(st||'unknown')}</span>`;};
// Cells of one Basestation, from its CellsSnapshot telemetry: a summary line
// per cell, then one chip per carrier (compact enough for the card width).
function cellsTable(s){
  const cells=s.cells||[];
  if(!cells.length)return '';
  const linked=s.site_linked==null?'':` &middot; ${s.site_linked?'site-linked':'independent'}`;
  const rows=cells.map(c=>{
    const carriers=(c.carriers||[]).map(k=>`<span class=cell-carrier title="Carrier ${k.carrier_num}: TX ${mhz(k.tx_freq_hz)} MHz, RX ${mhz(k.rx_freq_hz)} MHz"><b>${k.carrier_num}</b> TX ${mhz(k.tx_freq_hz)} &middot; RX ${mhz(k.rx_freq_hz)}</span>`).join('');
    const dev=c.device?`<span class=cell-dev title="${esc(c.device)}">${esc(c.device)}</span>`:'';
    // This cell's own SDR metrics: the primary's arrive untagged, the others' via CellRf.
    const rf=c.primary?{q:s.last_tx_quality,h:s.last_sdr_health}:(x=>({q:x&&x.tx_quality,h:x&&x.sdr_health}))((s.cell_rf||{})[c.id]);
    const rfm=[rf.q?`EVM ${rf.q.evm_pct.toFixed(2)}%`:'',rf.q?`PAPR ${rf.q.papr_db.toFixed(1)}dB`:'',
      rf.h&&rf.h.temperature_c!=null?`SDR ${rf.h.temperature_c.toFixed(1)}&deg;C`:''].filter(Boolean).join(' &middot; ');
    return `<div class=cell-row>
      <div class=cell-head><b>Cell ${c.id}</b>${c.primary?' <span class=muted>primary</span>':''} &middot; <span title="${esc(c.rf_detail||'')}">${rfPill(c.rf_state)}</span> &middot; ${c.mcc!=null&&c.mnc!=null?'MCC '+c.mcc+' / MNC '+c.mnc+' / ':''}CC ${c.colour_code} / LA ${c.location_area} &middot; ${c.neighbours} nbr &middot; ${c.registered_radios} radio(s)${dev?' &middot; '+dev:''}</div>
      ${rfm?`<div class="bts-meta muted" title="This cell's transmit EVM / PAPR and SDR temperature">${rfm}</div>`:''}
      <div class=cell-carriers>${carriers}</div>
    </div>`;
  }).join('');
  return `<div class="bts-meta muted" style="margin-top:10px">${cells.length} cell(s)${linked}</div>${rows}`;
}
function renderTelemetry(stations){
  tsnap=stations;
  $('telemetry-stations').innerHTML=stations.length?stations.map(s=>{
    const calls=Object.values(s.active_calls||{});
    const cellOfCarrier={};(s.cells||[]).forEach(c=>(c.carriers||[]).forEach(k=>{cellOfCarrier[k.carrier_num]=c.id;}));
    const multi=(s.cells||[]).length>1;
    const backhaul=s.backhaul_connected===true?'up':s.backhaul_connected===false?'down':'unknown';
    const q=s.last_tx_quality,sdr=s.last_sdr_health;
    const ipLabel=(s.ip||s.version)?` <span class=muted style="font-weight:400">${s.ip?`- ${esc(s.ip)}`:''}${s.version?` <span title="${esc(s.build||s.version)}">&middot; ${esc(s.version)}</span>`:''}</span>`:'';
    const evm=(s.evm_pct!=null)?`EVM ${s.evm_pct.toFixed(2)}%`:(q?`EVM ${q.evm_pct.toFixed(2)}%`:'');
    const rssi=(s.rssi_dbfs!=null)?`RSSI ${s.rssi_dbfs.toFixed(1)} dBFS`:'';
    const sig=[evm,rssi].filter(Boolean).join(' &middot; ');
    return `<div class=bts-card>
      <div style="display:flex;justify-content:space-between;align-items:center"><h3>${esc(s.id)}${ipLabel}</h3>${healthPill(s.health&&s.health.overall)}</div>
      <div class="bts-meta muted">Backhaul ${backhaul} &middot; ${s.registration_count} registered &middot; ${calls.length} active call(s)</div>
      ${sig?`<div class="bts-meta muted" title="EVM is transmit error-vector magnitude (SNR proxy); RSSI is received signal strength — neither is a true SNR">${sig}${q?` &middot; PAPR ${q.papr_db.toFixed(1)}dB`:''}${sdr&&sdr.temperature_c!=null?` &middot; SDR ${sdr.temperature_c.toFixed(1)}&deg;C`:''}</div>`:''}
      ${calls.length?`<table><thead><tr><th>Type</th><th>From</th><th>To</th>${multi?'<th>Cell</th>':''}<th>Carrier/TS</th><th>Pri</th></tr></thead><tbody>${calls.map(c=>`<tr><td>${c.is_group?'Group':'Private'}</td><td>${c.source_issi}</td><td>${c.gssi_or_called}</td>${multi?`<td>${cellOfCarrier[c.carrier_num]??'&ndash;'}</td>`:''}<td>${c.carrier_num}/${c.ts}</td><td>${c.priority}</td></tr>`).join('')}</tbody></table>`:''}
      ${cellsTable(s)}
      ${tsGrid(calls)}
    </div>`;
  }).join(''):'<div class=muted>No Basestation telemetry connections</div>';
  // Registered subscribers, grouped by Basestation. Shows which ISSIs are
  // currently registered on each connected station.
  $('registrations').innerHTML=stations.length?stations.map(s=>{
    const issis=s.registrations_list||[];
    const cellOf={};(s.ms_cell_out||[]).forEach(([i,c])=>{cellOf[i]=c;});
    // The cell from the Brew connection (name and carrier) when the station
    // reports it, else the cell number from telemetry (multi-cell stations).
    const cellTag=i=>bcell[i]?` <span class=muted>${esc(bcell[i].name)} &middot; ${bcell[i].carrier}</span>`:cellOf[i]!=null?` <span class=muted>cell ${cellOf[i]}</span>`:'';
    const chips=issis.length?`<div class=reg-list>${issis.map(i=>`<span class=reg-issi>${esc(String(i))}${cellTag(i)}</span>`).join('')}</div>`:'<div class="bts-meta muted" style="margin-top:8px">No subscribers registered</div>';
    return `<div class=bts-card><div style="display:flex;justify-content:space-between;align-items:center"><h3>${esc(s.id)}</h3><span class=reg-count>${issis.length} registered</span></div>${chips}</div>`;
  }).join(''):'<div class=muted>No Basestation telemetry connections</div>';
}
async function refreshEmergencies(){
  try{
    const list=await(await fetch('/api/emergencies')).json();
    const b=$('emergency-banner');
    b.classList.toggle('on',list.length>0);
    $('emergency-list').innerHTML=list.map(e=>`<span class=emg-chip>ISSI ${e.issi}${e.destination!=null?' &rarr; '+e.destination+' &middot; emergency call':''}${e.bts?' &middot; '+esc(e.bts):''}${e.blacklisted?' &middot; blacklisted':''}</span>`).join('');
  }catch(e){}
}
async function refreshTelemetry(){try{renderTelemetry(await(await fetch('/api/telemetry')).json())}catch(e){}}
async function refreshMsCells(){try{const pairs=await(await fetch('/api/ms-cells')).json();bcell={};pairs.forEach(([issi,c])=>{bcell[issi]=c;});if(tsnap)renderTelemetry(tsnap);}catch(e){}}
async function refreshBrewRssi(){try{const pairs=await(await fetch('/api/rssi')).json();brssi={};pairs.forEach(([issi,dbfs])=>{brssi[issi]=dbfs;});if(snap)render(snap);}catch(e){}}
function ctlSafeId(id){return 'ctl_'+id.replace(/[^a-zA-Z0-9_-]/g,'_');}
function jsq(s){return String(s).replace(/\\/g,'\\\\').replace(/'/g,"\\'");}
function ctlCardHtml(id){
  const s=ctlSafeId(id);
  return `<div class=ctl-row><input id="${s}_kick_issi" placeholder="ISSI" size=8><button onclick="ctlKick('${jsq(id)}','${s}')">Kick MS</button></div>
      <div class=ctl-row><input id="${s}_clr_issi" placeholder="ISSI (0=all)" size=8><button onclick="ctlClearEmergency('${jsq(id)}','${s}')">Clear emergency</button></div>
      <div class=ctl-row><input id="${s}_dgna_issi" placeholder="ISSI" size=6><input id="${s}_dgna_gssi" placeholder="GSSI" size=6><input id="${s}_dgna_mode" placeholder="mode" size=3 value=0><label><input type=checkbox id="${s}_dgna_attach" checked>attach</label><button onclick="ctlDgna('${jsq(id)}','${s}')">DGNA</button></div>
      <div class=ctl-row><input id="${s}_sds_text" placeholder="live SDS text"><input id="${s}_sds_issi" placeholder="src ISSI" size=8><input id="${s}_sds_repeat" placeholder="repeat" size=4 value=0><button onclick="ctlAddLiveSds('${jsq(id)}','${s}')">Add live SDS</button><button onclick="ctlClearLiveSds('${jsq(id)}','${s}')">Clear all</button></div>
      <div class=ctl-row><input id="${s}_raw_src" placeholder="src ISSI" size=8><input id="${s}_raw_dest" placeholder="dest ISSI/GSSI" size=8><label><input type=checkbox id="${s}_raw_grp">group</label><input id="${s}_raw_len" placeholder="len bits" size=6><input id="${s}_raw_hex" placeholder="payload hex"><button onclick="ctlSendSds('${jsq(id)}','${s}')">Send raw SDS</button></div>
      <div class=ctl-row><input id="${s}_al_issi" placeholder="ISSI" size=8><button onclick="ctlAmbience('${jsq(id)}','${s}',true)" title="Ambience listening: target radio auto-keys its mic (with indication on the radio). Needs Basestation AL support.">Ambience listen</button><button onclick="ctlAmbience('${jsq(id)}','${s}',false)">Stop AL</button></div>
      <div class=ctl-row><button onclick="ctlRestart('${jsq(id)}')">Restart service</button><button onclick="ctlShutdown('${jsq(id)}')">Shutdown service</button></div>
      <div class="ctl-result muted" id="${s}_result"></div>`;
}
// Incrementally reconcile the control cards against the connected station list.
// We never rebuild an existing card, so user input and focus in its fields are
// preserved across the periodic refresh (this was the bug: a full innerHTML
// rewrite every few seconds wiped whatever the operator was typing).
function renderControl(ids){
  const host=$('control-stations');
  const wanted=new Set(ids);
  if(!ids.length){host.innerHTML='<div class="muted ctl-empty">No Basestation control connections</div>';return;}
  // Clear the "no connections" placeholder before adding the first real card.
  const ph=host.querySelector('.ctl-empty');
  if(ph)ph.remove();
  // Remove cards for stations that are no longer connected.
  Array.from(host.querySelectorAll('.bts-card[data-ctl-id]')).forEach(card=>{
    if(!wanted.has(card.getAttribute('data-ctl-id')))card.remove();
  });
  // Add cards for newly connected stations, preserving existing ones untouched.
  const existing=new Set(Array.from(host.querySelectorAll('.bts-card[data-ctl-id]')).map(c=>c.getAttribute('data-ctl-id')));
  ids.forEach(id=>{
    if(existing.has(id))return;
    const card=document.createElement('div');
    card.className='bts-card';
    card.setAttribute('data-ctl-id',id);
    card.innerHTML=`<h3>${esc(id)}</h3>`+ctlCardHtml(id);
    host.appendChild(card);
  });
}
async function refreshControl(){try{renderControl(await(await fetch('/api/control')).json())}catch(e){}}
async function ctlSend(id,body,resultId){
  try{
    const r=await fetch('/api/control/'+encodeURIComponent(id),{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    const text=await r.text();
    if(resultId) $(resultId).textContent=(r.ok?'OK: ':'Error: ')+text;
  }catch(e){ if(resultId) $(resultId).textContent='Error: '+e; }
}
function hexToBytes(hex){hex=(hex||'').replace(/\s+/g,'');const out=[];for(let i=0;i<hex.length-1;i+=2)out.push(parseInt(hex.substr(i,2),16)||0);return out;}
function ctlKick(id,s){ctlSend(id,{action:'KickMs',issi:Number($(s+'_kick_issi').value||0)},s+'_result');}
function ctlClearEmergency(id,s){ctlSend(id,{action:'ClearEmergency',issi:Number($(s+'_clr_issi').value||0)},s+'_result');}
function ctlDgna(id,s){ctlSend(id,{action:'Dgna',issi:Number($(s+'_dgna_issi').value||0),gssi:Number($(s+'_dgna_gssi').value||0),mnemonic:null,attachment_mode:Number($(s+'_dgna_mode').value||0),attach:$(s+'_dgna_attach').checked},s+'_result');}
function ctlAddLiveSds(id,s){ctlSend(id,{action:'AddLiveSds',text:$(s+'_sds_text').value,protocol_id:10,source_issi:Number($(s+'_sds_issi').value||0),repeat_count:Number($(s+'_sds_repeat').value||0)},s+'_result');}
function ctlClearLiveSds(id,s){ctlSend(id,{action:'ClearLiveSds'},s+'_result');}
function ctlSendSds(id,s){const payload=hexToBytes($(s+'_raw_hex').value);ctlSend(id,{action:'SendSds',source_ssi:Number($(s+'_raw_src').value||0),dest_ssi:Number($(s+'_raw_dest').value||0),dest_is_group:$(s+'_raw_grp').checked,len_bits:Number($(s+'_raw_len').value||payload.length*8),payload},s+'_result');}
function ctlAmbience(id,s,enable){const issi=Number($(s+'_al_issi').value||0);if(!issi){$(s+'_result').textContent='Enter an ISSI';return;}ctlSend(id,{action:'AmbienceListen',issi,enable},s+'_result');}
function ctlRestart(id){if(confirm('Restart Basestation service on '+id+'? This disconnects it.'))ctlSend(id,{action:'RestartService'},null);}
function ctlShutdown(id){if(confirm('Shutdown Basestation service on '+id+'? This stops the BTS process.'))ctlSend(id,{action:'ShutdownService'},null);}
refresh();refreshTelemetry();refreshEmergencies();refreshControl();refreshBrewRssi();refreshMsCells();setInterval(refreshEmergencies,2000);setInterval(refresh,2000);setInterval(refreshTelemetry,2000);setInterval(refreshControl,3000);setInterval(refreshBrewRssi,5000);setInterval(refreshMsCells,3000);
// Keep a live WebSocket for push updates, but never reload the page on drop:
// a reload would wipe anything the operator is typing in the control panel.
// Instead we reconnect in the background and fall back to the polling above.
function connectLive(){
  let ws=new WebSocket((location.protocol==='https:'?'wss://':'ws://')+location.host+'/api/live');
  ws.onopen=()=>{$('status').textContent='Live';};
  ws.onmessage=()=>{refresh();refreshTelemetry();refreshEmergencies();refreshControl();};
  ws.onclose=()=>{$('status').textContent='Reconnecting';setTimeout(connectLive,3000);};
  ws.onerror=()=>{try{ws.close();}catch(e){}};
}
connectLive();
</script></body></html>"#;

// ---------------------------------------------------------------------------
// High availability (`ha`): status for everyone, controls for admins.

/// `/api/ha`: the HA task's status plus whether the config file on disk
/// differs from the one this process runs (saved but not applied yet).
pub async fn ha_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let status = state.ha.status.borrow().clone();
    // A file that no longer loads also counts as pending (Apply refuses it).
    let on_disk = config::Config::load(&state.config_path);
    let mut v = serde_json::to_value(&status).unwrap_or_default();
    match &on_disk {
        Ok(saved) => {
            // Setting names only, never values (this endpoint is not
            // admin-only and settings include passwords). Includes [ha].
            // Live edits (blacklist, talkgroup pools) are applied already,
            // so the running config is compared with them in.
            let diff = crate::ha::config_diff(
                &crate::ha::config_sections(&state.config_snapshot(), true),
                &crate::ha::config_sections(saved, true),
            );
            v["pending_changes"] = (!diff.is_empty()).into();
            v["pending_diff"] = diff.into();
        }
        Err(_) => {
            v["pending_changes"] = true.into();
            v["pending_diff"] = vec!["(the saved file does not load)"].into();
        }
    }
    let peer_differs = status.peer.as_ref().is_some_and(|p| p.config_hash != status.config_hash);
    v["peer_config_differs"] = peer_differs.into();
    v["peer_config_diff"] = match (peer_differs, state.ha.peer_config_sections.lock().unwrap().as_ref()) {
        (true, Some(peer)) => crate::ha::config_diff(&state.ha.config_sections, peer).into(),
        _ => serde_json::Value::Array(vec![]),
    };
    Json(v)
}

fn ha_reply<T: serde::Serialize>(r: Result<T, String>) -> Response {
    match r {
        Ok(v) => Json(serde_json::json!({ "ok": true, "result": v })).into_response(),
        Err(e) => (StatusCode::CONFLICT, e).into_response(),
    }
}

pub async fn ha_make_active(State(state): State<Arc<AppState>>) -> Response {
    ha_reply(state.ha.command(crate::ha::HaCommand::MakeActive).await)
}

pub async fn ha_make_standby(State(state): State<Arc<AppState>>) -> Response {
    ha_reply(state.ha.command(crate::ha::HaCommand::MakeStandby).await)
}

#[derive(serde::Deserialize)]
pub struct PersistBody {
    /// `null` clears the dashboard override (back to `[ha] persist`).
    persist: Option<bool>,
}

pub async fn ha_set_persist(State(state): State<Arc<AppState>>, Json(body): Json<PersistBody>) -> Response {
    ha_reply(state.ha.command(|tx| crate::ha::HaCommand::SetPersist(body.persist, tx)).await)
}

#[derive(serde::Deserialize)]
pub struct ApplyBody {
    #[serde(default)]
    force: bool,
}

/// Restarts this node to apply the saved config file, handing over first
/// when it is Active (see `HaCommand::Apply`). Refuses a file that would not
/// load, so a bad edit never takes a node down.
pub async fn ha_apply(State(state): State<Arc<AppState>>, Json(body): Json<ApplyBody>) -> Response {
    if let Err(e) = config::Config::load(&state.config_path) {
        return (StatusCode::BAD_REQUEST, format!("the saved config does not load, not applying: {e:#}")).into_response();
    }
    ha_reply(state.ha.command(|reply| crate::ha::HaCommand::Apply { force: body.force, reply }).await)
}

/// `[ha]` as saved in the config file (which may be ahead of the running
/// config until applied), for the /ha settings form.
pub async fn ha_config_get(State(state): State<Arc<AppState>>) -> Response {
    match config::Config::load(&state.config_path) {
        Ok(cfg) => Json(cfg.ha).into_response(),
        Err(_) => Json(state.config.ha.clone()).into_response(),
    }
}

/// Saves `[ha]` into the config file, keeping the rest of the file as saved.
/// A changed `persist` also clears the dashboard override, so the saved
/// value is the one in effect.
pub async fn ha_config_put(State(state): State<Arc<AppState>>, Json(ha): Json<config::HaConfig>) -> Response {
    if let Err(e) = ha.validate() {
        return (StatusCode::BAD_REQUEST, format!("{e:#}")).into_response();
    }
    let mut cfg = match config::Config::load(&state.config_path) {
        Ok(cfg) => cfg,
        Err(_) => state.config.clone(),
    };
    let persist_changed = cfg.ha.persist != ha.persist;
    cfg.ha = ha;
    if let Err(e) = save_config(&state, &cfg).await {
        return (StatusCode::BAD_REQUEST, format!("{e:#}")).into_response();
    }
    if persist_changed && state.config.ha.enabled {
        let _ = state.ha.command(|tx| crate::ha::HaCommand::SetPersist(None, tx)).await;
    }
    Json(saved_note(&state)).into_response()
}

/// `ok active` / `ok standby` / ... (or plain `ok` without HA): this process
/// is up and serving its dashboard, whatever its role.
async fn dashboard_healthz(State(state): State<Arc<AppState>>) -> String {
    if state.config.ha.enabled {
        let role = serde_json::to_value(state.ha.status.borrow().role).unwrap_or_default();
        format!("ok {}\n", role.as_str().unwrap_or("unknown"))
    } else {
        "ok\n".into()
    }
}

pub async fn ha_page() -> Html<&'static str> { Html(HA_HTML.as_str()) }

static HA_HTML: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| HA_PAGE.replace("__STYLE__", STYLE).replace("__VERSION__", VERSION).replace("__SERVERNAME__", server_name()));

const HA_PAGE: &str = r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>High Availability - __SERVERNAME__</title>__STYLE__</head><body><header><h1>HIGH AVAILABILITY</h1><div class=hdr-status><span class=live></span><span id=status>Live</span><div class=hdr-user><span id=whoami></span><button id=login-btn onclick="doLogin()">Login</button><button id=logout-btn onclick="doLogout()">Logout</button></div><div class=ver>v__VERSION__</div></div></header><main class=wrap>
<p><a class=backlink href="/">&larr; Back to dashboard</a></p>
<div class=banner id=ha-banner></div>
<div id=ha-off class=panel style="display:none"><p class=muted>HA is not enabled on this node. Fill in the form below and set <b>enabled</b> to turn it on (the node restarts to apply it).</p></div>
<section class=bts-grid id=ha-nodes></section>
<section class=panel id=ha-actions style="display:none"><h2>Controls</h2>
<div class=ctl-row><button id=btn-active onclick="act('active')">Make Active</button><button id=btn-standby onclick="act('standby')">Make Standby</button>
<span class=muted style="margin-left:12px">Persist:</span><button onclick="persist(true)">On</button><button onclick="persist(false)">Off</button><button onclick="persist(null)">Config default</button>
<span class=muted style="margin-left:12px">Config:</span><button id=btn-apply onclick="applyCfg(false)">Apply saved config</button></div>
<div class=ctl-result id=act-result></div>
<p class=map-note style="color:#8fa2b8;font-size:12px">Make Active / Make Standby hand the VIP over to the other node, which then holds it (manual hold) until it leaves Active. Persist keeps the Active node Active even when a higher-weight peer comes back; the buttons override the config value until "Config default", and any of them ends a manual hold. While HA is enabled, config changes (here, on Settings, or in the file) take effect only through <b>Apply saved config</b>: an Active node hands over to the Standby first, then restarts. A node that leaves Active restarts into Standby, dropping its sessions so they reconnect to the new Active node through the VIP. The Standby keeps a copy of the Active node's call/SDS history and SMS Center queue; a returning higher-weight node takes over only once its copy is up to date (manual switches do not wait).</p>
</section>
<section class=panel id=ha-form style="display:none"><h2>Settings</h2>
<div class=ctl-row><label><input id=f-enabled type=checkbox> enabled</label><label>node name <input id=f-node_name></label><label>weight <input id=f-weight type=number min=0></label><label><input id=f-persist type=checkbox> persist</label></div>
<div class=ctl-row><label>VIP (a.b.c.d/nn) <input id=f-vip></label><label>interface <input id=f-vip_interface></label><label>this node's real IP <input id=f-real_ip></label><label>peer real IP <input id=f-peer_ip></label></div>
<div class=ctl-row><label>heartbeat port <input id=f-heartbeat_port type=number></label><label>interval ms <input id=f-heartbeat_interval_ms type=number></label><label>dead after ms <input id=f-dead_after_ms type=number></label><label>shared secret <input id=f-shared_secret type=password></label><label>gateway check IP <input id=f-check_gateway placeholder="optional"></label></div>
<div class=ctl-row><button onclick="saveCfg()">Save</button></div>
<div class=ctl-result id=cfg-result></div>
<p class=map-note style="color:#8fa2b8;font-size:12px">Both nodes need the same VIP, heartbeat port and secret; node name, weight and the two real IPs are per node (swap real/peer IP on the other node). Saving writes the config file only; apply it with the button above.</p>
</section>
<section class=panel><h2>Role changes</h2><table><thead><tr><th>Time</th><th>From</th><th>To</th><th>Reason</th></tr></thead><tbody id=log></tbody></table></section>
</main><script>
const $=id=>document.getElementById(id);const esc=s=>String(s??'').replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const yn=b=>b?'yes':'no';
const roleBadge=r=>`<span class="badge ha-${r}">${esc(r).toUpperCase()}</span>`;
let admin=false,formLoaded=false,last=null;
function replText(d){const r=d.replication;if(!r)return '-';if(!r.needed)return '<span class=muted>nothing to replicate</span>';
  if(d.role==='active')return r.connected?'serving the Standby':'<b>no Standby connected</b>';
  const ago=r.last_rx_ms_ago!=null?` <span class=muted>(${(r.last_rx_ms_ago/1000).toFixed(0)}s ago)</span>`:'';
  return (r.synced?'in sync':r.connected?'<b>catching up</b>':'<b>not connected</b>')+ago;}
function node(title,rows){return `<div class=bts-card><h3>${title}</h3><table>${rows.map(([k,v])=>`<tr><th>${k}</th><td>${v}</td></tr>`).join('')}</table></div>`;}
async function load(){
  try{
    const d=await (await fetch('/api/ha')).json();
    last=d;
    $('status').textContent='Live';
    $('ha-off').style.display=d.enabled?'none':'block';
    $('ha-actions').style.display=admin&&d.enabled?'block':'none';
    const b=[];
    const list=a=>a&&a.length?': '+a.map(x=>`<code>${esc(x)}</code>`).join(', '):'';
    if(d.pending_changes)b.push('The config file has saved changes that are not applied yet'+list(d.pending_diff));
    if(d.peer_config_differs)b.push('The peer runs a different config'+(d.peer_config_diff&&d.peer_config_diff.length?list(d.peer_config_diff):' (details arrive within a few seconds)'));
    if(d.enabled&&!d.gateway_ok)b.push('Gateway check failing: this node cannot become Active.');
    $('ha-banner').style.display=b.length?'block':'none';$('ha-banner').innerHTML=b.join('<br>');
    if(d.enabled){
      const p=d.peer;
      $('ha-nodes').innerHTML=node(`This node: ${esc(d.node)} ${roleBadge(d.role)}`,[
        ['Weight',d.weight],['Persist',`${yn(d.persist)} <span class=muted>(${d.persist_source==='override'?'dashboard override':'config'})</span>`],
        ['Manual hold',yn(d.hold)],['VIP',`${esc(d.vip)} ${d.vip_held?'<b>held here</b>':'<span class=muted>not held</span>'}`],
        ['Gateway check',d.gateway_ok?'ok':'<b>failing</b>'],['Handing over to',esc(d.handover_to||'-')],
        ['Replication',replText(d)],['Records from peer',d.replication&&d.replication.needed?`${d.replication.records_received} (+${d.replication.sms_snapshots_received} SMS queue snapshots)`:'-']])
      +node(p?`Peer: ${esc(p.node)} ${p.alive?roleBadge(p.role):roleBadge('down')}`:'Peer: never heard',p?[
        ['Address',esc(p.ip)],['Weight',p.weight],['Persist',yn(p.persist)],['Manual hold',yn(p.hold)],['Replica up to date',yn(p.synced)],
        ['Last heartbeat',`${(p.last_seen_ms_ago/1000).toFixed(1)}s ago`]]:[['Status','no heartbeat received yet']]);
      $('btn-active').disabled=d.role!=='standby';$('btn-standby').disabled=d.role!=='active';
    }
    $('log').innerHTML=(d.transitions||[]).map(t=>`<tr><td>${new Date(t.ts_ms).toLocaleString()}</td><td>${roleBadge(t.from)}</td><td>${roleBadge(t.to)}</td><td>${esc(t.reason)}</td></tr>`).join('')||'<tr><td colspan=4 class=muted>No role changes yet</td></tr>';
  }catch(e){$('status').textContent='Disconnected';}
}
async function post(url,body,out){
  const r=await fetch(url,{method:url.includes('/config/')?'PUT':'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  const t=await r.text();let msg=t;try{const j=JSON.parse(t);msg=j.note||(typeof j.result==='string'?j.result:'done');}catch(e){}
  $(out).textContent=(r.ok?'':'Error: ')+msg;return r.ok;
}
// Confirmation texts name both nodes and spell out the consequences.
const me=()=>last?last.node:'this node';
const peerName=()=>last&&last.peer?last.peer.node:'the peer';
const replNote=(who,ok)=>ok?'':`\n\nWarning: ${who}'s copy of the call history and SMS Center queue is not up to date; anything not copied yet stays behind.`;
async function act(which){
  const d=last||{},p=d.peer||{},r=d.replication||{};
  const msg=which==='active'
    ?`Make ${me()} Active?\n\n${peerName()} hands the VIP over to ${me()}. Basestations, peers and SIP calls on ${peerName()} are dropped and reconnect here; ${peerName()} restarts into Standby.\n\n${me()} then keeps the VIP (manual hold) until it leaves Active or a Persist button is pressed.`+replNote(me(),!r.needed||r.synced)
    :`Make ${me()} Standby?\n\n${peerName()} takes over the VIP. Basestations, peers and SIP calls on ${me()} are dropped and reconnect to ${peerName()}; ${me()} restarts into Standby.\n\n${peerName()} then keeps the VIP (manual hold) until it leaves Active or a Persist button is pressed.`+replNote(peerName(),p.synced!==false);
  if(!confirm(msg))return;
  await post('/api/ha/'+which,{},'act-result');load();
}
async function persist(v){
  if(v===null){
    const c=window._haCfg?(window._haCfg.persist?'on':'off'):'the config file value';
    if(!confirm(`Reset Persist on ${me()} to the config default (${c})?\n\nThis removes the dashboard override and ends any manual hold. If persist ends up off and ${peerName()} has the higher weight, ${peerName()} takes over the VIP once its copy of the data is up to date, and sessions on ${me()} reconnect there.`))return;
  }
  await post('/api/ha/persist',{persist:v},'act-result');load();
}
async function applyCfg(force){
  const d=last||{},changes=(d.pending_diff||[]).join(', ');
  const what=changes?`Changes to apply: ${changes}`:'No setting changes are pending; this only restarts the node.';
  const how=d.role==='active'
    ?`${me()} is Active: it hands the VIP over to ${peerName()} first, then restarts. Sessions on ${me()} reconnect to ${peerName()}.`
    :`${me()} is ${d.role||'not Active'}: it restarts; service on ${peerName()} is not affected.`;
  if(!force&&!confirm(`Apply the saved config on ${me()}?\n\n${what}\n\n${how}`))return;
  const r=await fetch('/api/ha/apply',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({force})});
  const t=await r.text();
  if(!r.ok&&r.status===409&&!force){if(confirm(t+'\n\nRestart anyway?'))return applyCfg(true);}
  try{$('act-result').textContent=r.ok?JSON.parse(t).result:'Error: '+t;}catch(e){$('act-result').textContent=t;}
}
const fields=['enabled','node_name','weight','persist','vip','vip_interface','real_ip','peer_ip','heartbeat_port','heartbeat_interval_ms','dead_after_ms','shared_secret','check_gateway'];
async function loadCfg(){
  const c=await (await fetch('/api/config/ha')).json();
  for(const f of fields){const el=$('f-'+f);if(el.type==='checkbox')el.checked=!!c[f];else el.value=c[f]??'';}
  c.state_path=c.state_path;window._haCfg=c;
}
async function saveCfg(){
  const c=Object.assign({},window._haCfg||{});
  for(const f of fields){const el=$('f-'+f);c[f]=el.type==='checkbox'?el.checked:el.type==='number'?Number(el.value):el.value.trim();}
  await post('/api/config/ha',c,'cfg-result');load();
}
fetch('/api/whoami').then(r=>r.json()).then(w=>{admin=!!w.admin;if(admin){$('ha-form').style.display='block';loadCfg();}load();}).catch(()=>load());
setInterval(load,2000);
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// In the new dashboard's frame the classic raw TOML editor is hidden: the configuration is
    /// edited on the Configuration page there. Opened on its own (classic dashboard) it stays.
    #[test]
    fn settings_hides_the_raw_editor_inside_the_new_dashboard() {
        let h = SETTINGS_HTML.as_str();
        assert!(h.contains("<section class=panel id=raw-panel><h2>Full configuration (raw TOML)"));
        assert!(h.contains("if(window.self!==window.top){$('raw-panel').style.display='none';}"));
    }

    #[test]
    fn connections_page_shows_the_connections_before_the_cells() {
        let at = |h: &str| CONNECTIONS_HTML.find(h).unwrap_or_else(|| panic!("{h} missing"));
        assert!(at("<h2>Brew Connections") < at("<h2>Cells"));
    }

    #[tokio::test]
    async fn ms_cells_lists_registered_subscribers_on_their_cell() {
        use crate::protocol::{self, ConnVersion, SUB_REGISTER};
        let state = AppState::for_test();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (bs, other) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        for (id, user) in [(bs, "2320997"), (other, "2320998")] {
            state.inner.write().await.clients.insert(id, crate::state::Client {
                tx: tx.clone(), mode: crate::state::ClientMode::Basestation, version: ConnVersion::V0,
                version_announced: false, remote_addr: None, connected_at_ms: 0, username: Some(user.into()),
            });
        }
        let packet = |p: Vec<u8>, from| crate::router::handle_packet(state.clone(), from, p);
        packet(protocol::build_service(protocol::SERVICE_BTS_INFO,
            r#"{"mcc":232,"mnc":5,"cells":[{"cell":0,"carrier":1528,"carrier_ext":268,"location_area":1},{"cell":3,"carrier":1530,"carrier_ext":268,"location_area":2}]}"#), bs).await;
        for issi in [7, 8, 9] {
            packet(protocol::build_subscriber_message(SUB_REGISTER, issi, &[]), bs).await;
        }
        packet(protocol::build_service(protocol::SERVICE_MS_CELL, r#"{"issi":7,"carrier":1530,"carrier_ext":268}"#), bs).await;
        packet(protocol::build_service(protocol::SERVICE_MS_CELL, r#"{"issi":9,"carrier":1528,"carrier_ext":268}"#), bs).await;
        // 9 moves to another station that reports no cell: its old cell no longer counts.
        packet(protocol::build_subscriber_message(SUB_REGISTER, 9, &[]), other).await;
        let Json(cells) = ms_cells_snapshot(State(state.clone())).await;
        assert_eq!(cells, vec![(7, serde_json::json!({ "name": "2320997/3", "cell": 3, "carrier": 1530 }))]);
        let h = INDEX_HTML.as_str();
        assert!(h.contains("/api/ms-cells") && h.contains("bcell[i]"), "main page shows the cell per subscriber");
        // 9's move is in the roaming log on the Connections page.
        let Json(conn) = connections_snapshot(State(state.clone())).await;
        assert_eq!(conn["roaming_log"][0]["issi"], 9);
        assert_eq!(conn["roaming_log"][0]["kind"], "reselection");
        assert!(conn["roaming_log"][0]["reselection"].is_null(), "the station did not say how");
        assert_eq!(conn["roaming_log"][0]["from"]["cell"], "2320997/0");
        assert_eq!(conn["roaming_log"][0]["to"]["station"], "2320998");
        assert!(CONNECTIONS_HTML.contains("id=roaming-log"));
        assert!(REGISTRATIONS_HTML.contains("<th>Cell (global ID)</th>") && REGISTRATIONS_HTML.contains("x.cell"));
    }

    #[test]
    fn server_name_is_the_heading_and_every_tab_title() {
        let name = server_name();
        assert_eq!(name, "Tetra Network Monitor", "default without [dashboard] servername");
        assert!(INDEX_HTML.contains("<title>Tetra Network Monitor</title>"));
        assert!(INDEX_HTML.contains("<h1 class=srvname>Tetra Network Monitor</h1>"));
        for (page, html) in [
            ("calls", CALLS_HTML.as_str()),
            ("map", MAP_HTML.as_str()),
            ("sip", SIP_HTML.as_str()),
            ("sip-config", SIP_CONFIG_HTML.as_str()),
            ("connections", CONNECTIONS_HTML.as_str()),
            ("settings", SETTINGS_HTML.as_str()),
            ("ha", HA_HTML.as_str()),
        ] {
            assert!(html.contains(" - Tetra Network Monitor</title>"), "{page}: tab title");
            assert!(!html.contains("__SERVERNAME__") && !html.contains("TETRA Network"), "{page}: no old name");
        }
    }

    #[test]
    fn server_name_is_escaped() {
        assert_eq!(html_escape(r#"A&B <x> "y""#), "A&amp;B &lt;x&gt; &quot;y&quot;");
    }

    #[test]
    fn index_has_style_and_navlinks_no_removed_tables() {
        let h = INDEX_HTML.as_str();
        assert!(!h.contains("__STYLE__"), "style placeholder must be substituted");
        assert!(h.contains("<style>"), "style present");
        // nav links to the three sub-pages
        assert!(h.contains("href=\"/calls\""));
        assert!(h.contains("href=\"/sds\""));
        assert!(h.contains("href=\"/telemetry-sds\""));
        assert!(h.contains("href=\"/registrations\""));
        assert!(h.contains("href=\"/sms-center\""));
        // the three moved tables must be gone from the main page
        assert!(!h.contains("id=sdstable"), "recent SDS table removed from index");
        assert!(!h.contains("id=history"), "recent calls table removed from index");
        assert!(!h.contains("id=telemetry-sds"), "telemetry SDS table removed from index");
    }

    #[test]
    fn subpages_build_and_contain_expected_bits() {
        for (name, html, endpoint) in [
            ("calls", CALLS_HTML.as_str(), "/api/status"),
            ("sds", SDS_HTML.as_str(), "/api/status"),
            ("telemetry", TELEMETRY_SDS_HTML.as_str(), "/api/telemetry"),
            ("registrations", REGISTRATIONS_HTML.as_str(), "/api/registrations"),
            ("sms_center", SMS_CENTER_HTML.as_str(), "/api/sms-center"),
        ] {
            assert!(!html.contains("__STYLE__"), "{name}: style substituted");
            assert!(html.contains("id=log"), "{name}: log table body present");
            assert!(html.contains("id=log-pager"), "{name}: pager present");
            assert!(html.contains(endpoint), "{name}: polls {endpoint}");
            assert!(html.contains("Back to dashboard"), "{name}: back link present");
            // write out the embedded script for external JS syntax checking
            let script = html.split("<script>").nth(1).unwrap().split("</script>").next().unwrap();
            std::fs::write(format!("/tmp/subpage_{name}.js"), script).unwrap();
        }
    }

    #[test]
    fn map_page_builds() {
        let h = MAP_HTML.as_str();
        assert!(!h.contains("__STYLE__"), "style substituted");
        assert!(h.contains("/api/positions"), "map polls positions api");
        assert!(h.contains("leaflet"), "leaflet loaded");
        assert!(h.contains("Back to dashboard"), "back link present");
        // OSM tile template must survive the format! escaping as literal braces
        assert!(h.contains("{s}.tile.openstreetmap.org/{z}/{x}/{y}"), "tile template intact");
        let script = h.rsplit("<script>").next().unwrap().split("</script>").next().unwrap();
        std::fs::write("/tmp/map_page.js", script).unwrap();
    }

    #[test]
    fn index_links_to_map() {
        assert!(INDEX_HTML.as_str().contains("href=\"/map\""), "dashboard links to /map");
    }

    /// Every dashboard page must show who's logged in (or nothing, when
    /// auth is disabled/no credential was sent) and offer a logout control
    /// next to the live-status indicator -- pins the header markup, the
    /// doLogout() best-effort HTTP-Basic "logout" (there's no real
    /// server-side session to end), and the /api/whoami populate call
    /// across every page, not just the index.
    #[test]
    fn every_page_has_whoami_and_logout_in_its_header() {
        let pages: &[(&str, &str)] = &[
            ("index", INDEX_HTML.as_str()),
            ("map", MAP_HTML.as_str()),
            ("sip", SIP_HTML.as_str()),
            ("sip-config", SIP_CONFIG_HTML.as_str()),
            ("connections", CONNECTIONS_HTML.as_str()),
            ("settings", SETTINGS_HTML.as_str()),
            ("ha", HA_HTML.as_str()),
        ];
        for (name, html) in pages {
            assert!(html.contains("id=whoami") || html.contains("id=\"whoami\""), "{name}: whoami span present");
            assert!(html.contains("id=logout-btn") || html.contains("id=\"logout-btn\""), "{name}: logout button present");
            assert!(html.contains("doLogout()"), "{name}: doLogout wired up");
            assert!(html.contains("/api/whoami"), "{name}: polls /api/whoami");
        }
        // log_page-templated pages (calls/sds/telemetry-sds/registrations)
        // share one function; a single instance stands in for all four.
        let calls_script = CALLS_HTML.as_str();
        assert!(calls_script.contains("id=whoami") || calls_script.contains("id=\"whoami\""));
        assert!(calls_script.contains("doLogout()"));
    }

    #[tokio::test]
    async fn connections_show_each_basestations_cells_and_duplicates() {
        use crate::carriers::{BtsInfo, CarrierKey, Cell};
        use crate::state::{Client, ClientMode};
        let state = crate::state::AppState::for_test();
        let cell = |carrier| Cell { key: CarrierKey { mcc: 232, mnc: 5, carrier, carrier_ext: 268 }, location_area: 7, cell: 0, ..Default::default() };
        let (bs1, bs2, bs3) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        {
            let mut inner = state.inner.write().await;
            for id in [bs1, bs2, bs3] {
                let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
                let username = (id == bs1).then(|| "2320997".to_string());
                inner.clients.insert(id, Client { tx, mode: ClientMode::Basestation, version: crate::protocol::ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username });
            }
            inner.carriers.announce(bs1, BtsInfo { cells: vec![cell(1528), Cell { cell: 3, ..cell(1532) }], capabilities: vec![] });
            inner.carriers.announce(bs2, BtsInfo { cells: vec![cell(1530)], capabilities: vec!["handover-v1".into()] });
            inner.carriers.announce(bs3, BtsInfo { cells: vec![cell(1530)], capabilities: vec![] });
        }
        let Json(out) = connections_snapshot(State(state)).await;
        let client = |id: uuid::Uuid| out["brew_clients"].as_array().unwrap().iter().find(|c| c["id"] == id.to_string()).unwrap().clone();
        let one = client(bs1);
        assert_eq!(one["cells"][0]["carrier"], 1528);
        assert_eq!(one["cells"][0]["location_area"], 7);
        assert_eq!(one["cells"][0]["duplicate"], false);
        assert_eq!(client(bs2)["capabilities"][0], "handover-v1");
        assert_eq!((client(bs2)["cells"][0]["duplicate"].clone(), client(bs2)["cells"][0]["serving"].clone()), (true.into(), false.into()));
        assert_eq!((client(bs3)["cells"][0]["duplicate"].clone(), client(bs3)["cells"][0]["serving"].clone()), (true.into(), true.into()));
        assert_eq!(one["station"], "2320997");
        assert_eq!(one["cells"][1]["name"], "2320997/3");
        // The overview: one row per cell, sorted by station and cell number.
        let rows = out["cells"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        let mine: Vec<_> = rows.iter().filter(|r| r["station"] == "2320997").collect();
        assert_eq!((mine[0]["name"].as_str(), mine[1]["name"].as_str()), (Some("2320997/0"), Some("2320997/3")));
        assert_eq!(mine[1]["carrier"], 1532);
        // carrier_ext 268: band 4, no offset (duplex spacing 6).
        assert_eq!(mine[0]["downlink_hz"], 438_200_000u64);
        assert_eq!(mine[0]["location_area"], 7);
        let unnamed = crate::carriers::station_name(bs2, None);
        assert!(rows.iter().any(|r| r["name"] == format!("{unnamed}/0") && r["capabilities"][0] == "handover-v1"),
            "without a login the station is named after its connection");
        assert_eq!(out["duplicate_carriers"].as_array().unwrap().len(), 1);
        assert_eq!(out["duplicate_carriers"][0]["carrier"], 1530);
    }

    #[tokio::test]
    async fn cells_overview_names_neighbours_and_keeps_offline_stations() {
        use crate::carriers::{BtsInfo, CarrierKey, Cell, CellExtra, NeighbourRef};
        use crate::state::{Client, ClientMode};
        let state = AppState::for_test();
        let (bs1, bs2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        let key = |carrier| CarrierKey { mcc: 232, mnc: 5, carrier, carrier_ext: 268 };
        let near = |carrier| NeighbourRef { mcc: 232, mnc: 5, carrier, carrier_ext: 268, location_area: None };
        {
            let mut inner = state.inner.write().await;
            for (id, login) in [(bs1, "1000001"), (bs2, "1000002")] {
                let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
                inner.clients.insert(id, Client { tx, mode: ClientMode::Basestation, version: crate::protocol::ConnVersion::V0, version_announced: false, remote_addr: None, connected_at_ms: 0, username: Some(login.into()) });
                inner.carriers.set_login(id, login);
            }
            // Without an LA a neighbour is in the reporting cell's LA (2), so the second entry is
            // not 1000002/0 (LA 1).
            let extra = CellExtra { neighbours: Some(vec![NeighbourRef { location_area: Some(1), ..near(1530) }, near(1530), near(1600)]), rxlev_access_min_dbm: Some(-100), ..Default::default() };
            inner.carriers.announce(bs1, BtsInfo { cells: vec![Cell { key: key(1528), location_area: 2, cell: 0, extra }], capabilities: vec![] });
            inner.carriers.announce(bs2, BtsInfo { cells: vec![Cell { key: key(1530), location_area: 1, cell: 0, ..Default::default() }], capabilities: vec![] });
        }
        state.cleanup_client(bs2).await;
        let Json(out) = connections_snapshot(State(state)).await;
        let rows = out["cells"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "1000001/0");
        assert_eq!(rows[0]["neighbours"], serde_json::json!(["1000002/0", "carrier 1530", "carrier 1600"]));
        assert_eq!(rows[0]["rxlev_access_min_dbm"], -100);
        assert_eq!((rows[1]["name"].as_str(), rows[1]["online"].as_bool()), (Some("1000002/0"), Some(false)));
        assert!(rows[1]["neighbours"].is_null(), "not reported");
    }

    #[tokio::test]
    async fn bts_locations_snapshot_omits_unconfigured_zero_entries() {
        let mut config = crate::config::Config::default();
        config.bts_locations.insert("1000001".into(), crate::config::BtsLocationConfig {
            name: "Athens HQ".into(), lat: 37.9917, lon: 23.7640,
        });
        config.bts_locations.insert("1000002".into(), crate::config::BtsLocationConfig {
            name: "Unconfigured".into(), lat: 0.0, lon: 0.0,
        });
        let (state, _rx) = crate::state::AppState::new(config, std::path::PathBuf::from("test.toml"));
        let Json(out) = bts_locations_snapshot(State(std::sync::Arc::new(state)), axum::extract::Query(HashMap::new())).await;
        assert_eq!(out.len(), 1, "the (0,0) entry must not be plotted");
        assert_eq!(out[0].username, "1000001");
    }

    #[tokio::test]
    async fn bts_locations_prefer_telemetry_over_static_config() {
        let mut config = crate::config::Config::default();
        config.bts_locations.insert("1000001".into(), crate::config::BtsLocationConfig { name: "Static".into(), lat: 1.0, lon: 2.0 });
        config.bts_locations.insert("1000002".into(), crate::config::BtsLocationConfig { name: "Fallback".into(), lat: 3.0, lon: 4.0 });
        let (state, _rx) = crate::state::AppState::new(config, std::path::PathBuf::from("test.toml"));
        let state = std::sync::Arc::new(state);
        {
            let mut t = state.telemetry.write().await;
            t.add_test_station("1000001", Some((37.9917, 23.764)), "Reported");
            t.add_test_station("1000003", None, "No location"); // reports none: not plotted
        }
        let Json(out) = bts_locations_snapshot(State(state.clone()), axum::extract::Query(HashMap::new())).await;
        let by = |id: &str| out.iter().find(|b| b.id == id).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!((by("1000001").source, by("1000001").lat), ("telemetry", 37.9917));
        assert_eq!(by("1000002").source, "config");
        let Json(cfg) = bts_locations_snapshot(State(state), axum::extract::Query(HashMap::from([("config_only".into(), "1".into())]))).await;
        assert_eq!(cfg.len(), 2, "settings table lists every static entry");
    }

    #[tokio::test]
    async fn positions_include_relayed_fixes_newest_wins() {
        let (state, _rx) = crate::state::AppState::new(crate::config::Config::default(), std::path::PathBuf::from("test.toml"));
        let state = std::sync::Arc::new(state);
        state.telemetry.write().await.record_sds_position(4013, 1.0, 2.0, 100, "local".into());
        {
            let mut inner = state.inner.write().await;
            let e = |lat, at_ms| crate::fedroute::MsEntry { lat, lon: 5.0, at_ms, station: "bts9".into(), path: vec![7] };
            inner.fed.ms_pos.insert(4013, e(3.0, 200)); // newer than the local one
            inner.fed.ms_pos.insert(4014, e(4.0, 50));  // only known remotely
            inner.fed.ms_pos.insert(4015, crate::fedroute::MsEntry { lat: 9.0, lon: 9.0, at_ms: 1, station: "x".into(), path: vec![] }); // local origin: already in telemetry
        }
        let Json(fixes) = positions_snapshot(State(state)).await;
        assert_eq!(fixes.len(), 2);
        assert_eq!((fixes[0].issi, fixes[0].lat), (4013, 3.0));
        assert!(fixes[1].issi == 4014 && fixes[1].bts.starts_with("bts9 @ "));
    }

    #[tokio::test]
    async fn blacklist_api_applies_live_and_writes_the_config_without_restart() {
        let path = std::env::temp_dir().join(format!("brew-blacklist-{}.toml", uuid::Uuid::new_v4()));
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.sms_center.enabled = false;
        let (state, _rx) = crate::state::AppState::new(config, path.clone());
        let state = std::sync::Arc::new(state);

        let resp = blacklist_add(State(state.clone()), Path(4013)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(state.is_blocked(4013), "applied to the running server at once");
        assert!(crate::CONFIG_WRITE_APPLIED_LIVE.swap(false, std::sync::atomic::Ordering::SeqCst), "watcher told not to restart");
        let saved = crate::config::Config::load(path.to_str().unwrap()).unwrap();
        assert_eq!(saved.blacklist.issis, vec![4013]);
        assert_eq!(blacklist_get(State(state.clone())).await.0, vec![4013]);

        // Another settings edit keeps the live list instead of reverting it.
        let resp = upsert_bts_location(State(state.clone()), Path("1".into()), Json(crate::config::BtsLocationConfig { name: "x".into(), lat: 1.0, lon: 2.0 })).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(crate::config::Config::load(path.to_str().unwrap()).unwrap().blacklist.issis, vec![4013]);

        assert_eq!(blacklist_remove(State(state.clone()), Path(4013)).await.status(), StatusCode::OK);
        assert!(!state.is_blocked(4013));
        assert_eq!(blacklist_add(State(state), Path(0)).await.status(), StatusCode::BAD_REQUEST);
        crate::CONFIG_WRITE_APPLIED_LIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn positions_of_radios_in_an_emergency_are_flagged() {
        let (state, _rx) = crate::state::AppState::new(crate::config::Config::default(), std::path::PathBuf::from("test.toml"));
        let state = std::sync::Arc::new(state);
        {
            let mut t = state.telemetry.write().await;
            t.record_sds_position(4013, 1.0, 2.0, 100, "a".into());
            t.record_sds_position(4014, 3.0, 4.0, 100, "b".into());
            t.add_test_station("bts2", None, "bts2");
            t.stations.get_mut("bts2").unwrap().emergencies.insert(4013);
        }
        let Json(fixes) = positions_snapshot(State(state)).await;
        let by = |i: u32| fixes.iter().find(|f| f.issi == i).unwrap();
        assert!(by(4013).emergency && !by(4014).emergency);
    }

    #[test]
    fn map_page_renders_with_marker_icons() {
        let html = MAP_HTML.as_str();
        assert!(html.contains("const btsIcon=") && html.contains("msIconEmg") && html.contains("f.emergency") && !html.contains("{{"));
    }

    fn basic_auth_header(user: &str, pass: &str) -> HeaderMap {
        let creds = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Basic {creds}")).unwrap());
        headers
    }

    #[test]
    fn basic_username_accepts_matching_credentials() {
        let mut users = HashMap::new();
        users.insert("alice".to_string(), "secret".to_string());
        let headers = basic_auth_header("alice", "secret");
        assert_eq!(basic_username(&users, &headers), Some("alice".to_string()));
    }

    #[test]
    fn basic_username_rejects_wrong_password() {
        let mut users = HashMap::new();
        users.insert("alice".to_string(), "secret".to_string());
        let headers = basic_auth_header("alice", "wrong");
        assert_eq!(basic_username(&users, &headers), None);
    }

    #[test]
    fn basic_username_rejects_missing_header() {
        let users: HashMap<String, String> = HashMap::new();
        assert_eq!(basic_username(&users, &HeaderMap::new()), None);
    }

    /// Pins the privileged-user feature this test module's name suggests:
    /// a user authenticated but not listed in [dashboard].admins must not
    /// be treated as an admin by /api/whoami, while a listed one must.
    #[tokio::test]
    async fn whoami_reports_admin_only_for_listed_users() {
        let mut config = crate::config::Config::default();
        config.dashboard.users.insert("alice".into(), "secret".into());
        config.dashboard.users.insert("bob".into(), "secret2".into());
        config.dashboard.admins = vec!["alice".into()];
        let (state, _rx) = crate::state::AppState::new(config, std::path::PathBuf::from("test.toml"));
        let state = std::sync::Arc::new(state);

        let Json(alice) = whoami(State(state.clone()), basic_auth_header("alice", "secret")).await;
        assert_eq!(alice["admin"], serde_json::json!(true));
        assert_eq!(alice["username"], serde_json::json!("alice"));

        let Json(bob) = whoami(State(state.clone()), basic_auth_header("bob", "secret2")).await;
        assert_eq!(bob["admin"], serde_json::json!(false));

        let Json(nobody) = whoami(State(state), HeaderMap::new()).await;
        assert_eq!(nobody["admin"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn whoami_treats_empty_admins_list_as_everyone_admin() {
        let mut config = crate::config::Config::default();
        config.dashboard.users.insert("alice".into(), "secret".into());
        // admins left empty: pre-existing all-or-nothing behavior.
        let (state, _rx) = crate::state::AppState::new(config, std::path::PathBuf::from("test.toml"));
        let Json(alice) = whoami(State(std::sync::Arc::new(state)), basic_auth_header("alice", "secret")).await;
        assert_eq!(alice["admin"], serde_json::json!(true));
    }

    /// Open (read-only) mode: an anonymous visitor is not an admin but is
    /// offered a Login button, while a signed-in admin is an admin and isn't.
    #[tokio::test]
    async fn whoami_open_mode_offers_login_to_anonymous_only() {
        let mut config = crate::config::Config::default();
        config.dashboard.ui.open = true;
        config.dashboard.users.insert("alice".into(), "secret".into());
        config.dashboard.admins = vec!["alice".into()];
        let (state, _rx) = crate::state::AppState::new(config, std::path::PathBuf::from("test.toml"));
        let state = std::sync::Arc::new(state);

        let Json(anon) = whoami(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(anon["admin"], serde_json::json!(false));
        assert_eq!(anon["open"], serde_json::json!(true));
        assert_eq!(anon["can_login"], serde_json::json!(true));

        let Json(alice) = whoami(State(state), basic_auth_header("alice", "secret")).await;
        assert_eq!(alice["admin"], serde_json::json!(true));
        assert_eq!(alice["can_login"], serde_json::json!(false));
    }

    /// Closed mode (the default) never advertises a Login button -- viewing
    /// already requires a credential, so the browser's own prompt handles it.
    #[tokio::test]
    async fn whoami_closed_mode_hides_login_button() {
        let mut config = crate::config::Config::default();
        config.dashboard.users.insert("alice".into(), "secret".into());
        let (state, _rx) = crate::state::AppState::new(config, std::path::PathBuf::from("test.toml"));
        let Json(anon) = whoami(State(std::sync::Arc::new(state)), HeaderMap::new()).await;
        assert_eq!(anon["open"], serde_json::json!(false));
        assert_eq!(anon["can_login"], serde_json::json!(false));
    }

    /// `/login` returns to a local `next`, but refuses an off-site one so the
    /// Login button can't be bent into an open redirect.
    #[tokio::test]
    async fn login_redirects_only_to_local_paths() {
        let loc = |m: &HeaderMap| m[header::LOCATION].to_str().unwrap().to_string();

        let local = login(Query(HashMap::from([("next".into(), "/ha".into())]))).await.into_response();
        assert_eq!(loc(local.headers()), "/ha");

        let protocol_relative = login(Query(HashMap::from([("next".into(), "//evil.example/".into())]))).await.into_response();
        assert_eq!(loc(protocol_relative.headers()), "/");

        let absolute = login(Query(HashMap::from([("next".into(), "https://evil.example/".into())]))).await.into_response();
        assert_eq!(loc(absolute.headers()), "/");

        let missing = login(Query(HashMap::new())).await.into_response();
        assert_eq!(loc(missing.headers()), "/");
    }

    /// One raw HTTP/1.1 request to the test server; returns the response head and body.
    async fn http(addr: std::net::SocketAddr, req: String) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        c.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn login_window_session_works_next_to_basic_auth() {
        let mut c = config::Config::default();
        c.storage.enabled = false;
        c.sms_center.enabled = false;
        c.dashboard.users = HashMap::from([("admin".into(), "pw".into()), ("viewer".into(), "v".into())]);
        c.dashboard.admins = vec!["admin".into()];
        let state = Arc::new(AppState::new(c, "test.toml".into()).0);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
        let get = |path: &str, extra: &str| format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n{extra}\r\n");

        // The new page and whoami load without a login; whoami asks for one.
        assert!(http(addr, get("/", "")).await.starts_with("HTTP/1.1 200"));
        let who = http(addr, get("/api/whoami", "")).await;
        assert!(who.contains("\"need_login\":true"), "{who}");
        // Data needs a login: the browser's dialog for classic pages, a plain 401 for the new page.
        let classic = http(addr, get("/api/status", "")).await;
        assert!(classic.starts_with("HTTP/1.1 401") && classic.to_lowercase().contains("www-authenticate"), "{classic}");
        let ui = http(addr, get("/api/status", "x-brew-ui: 1\r\n")).await;
        assert!(ui.starts_with("HTTP/1.1 401") && !ui.to_lowercase().contains("www-authenticate"), "{ui}");
        // Basic still works.
        let b64 = base64::engine::general_purpose::STANDARD.encode("admin:pw");
        assert!(http(addr, get("/api/status", &format!("Authorization: Basic {b64}\r\n"))).await.starts_with("HTTP/1.1 200"));

        // Wrong password: no cookie.
        let post = |body: &str| format!("POST /api/login HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let bad = http(addr, post(r#"{"username":"admin","password":"no"}"#)).await;
        assert!(bad.starts_with("HTTP/1.1 401") && !bad.to_lowercase().contains("set-cookie"), "{bad}");
        // Viewer: signed in, but not for admin routes.
        let cookie_of = |r: &str| r.lines().find(|l| l.to_lowercase().starts_with("set-cookie:")).unwrap()
            .split_once(':').unwrap().1.trim().split(';').next().unwrap().to_string();
        let v = http(addr, post(r#"{"username":"viewer","password":"v"}"#)).await;
        let vc = cookie_of(&v);
        assert!(http(addr, get("/api/status", &format!("Cookie: {vc}\r\nx-brew-ui: 1\r\n"))).await.starts_with("HTTP/1.1 200"));
        assert!(http(addr, get("/api/accounts", &format!("Cookie: {vc}\r\nx-brew-ui: 1\r\n"))).await.starts_with("HTTP/1.1 403"));
        // Admin: admin routes, then logout ends the session.
        let a = http(addr, post(r#"{"username":"admin","password":"pw"}"#)).await;
        assert!(a.contains("\"admin\":true"), "{a}");
        let ac = cookie_of(&a);
        assert!(http(addr, get("/api/accounts", &format!("Cookie: {ac}\r\nx-brew-ui: 1\r\n"))).await.starts_with("HTTP/1.1 200"));
        let out = format!("POST /api/logout HTTP/1.1\r\nHost: x\r\nConnection: close\r\nCookie: {ac}\r\nContent-Length: 0\r\n\r\n");
        assert!(http(addr, out).await.starts_with("HTTP/1.1 204"));
        assert!(http(addr, get("/api/accounts", &format!("Cookie: {ac}\r\nx-brew-ui: 1\r\n"))).await.starts_with("HTTP/1.1 401"));
    }
}

