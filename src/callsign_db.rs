//! Ham callsign database (`[callsign_db]`): names for radios from a list of
//! `ID;CALLSIGN;...` lines, by default radioid.net's `dmrid.dat`. Amateur
//! TETRA networks give radios their DMR ID as ISSI, so the list names most
//! of them. Directory names (`[issi_names]`) always come first: the
//! dashboard asks this list only for ISSIs without one.
//!
//! The list is fetched on demand (dashboard "Now") or, with `auto`, every day
//! at 00:00 UTC, and kept in `cache` so it is there again after a restart.

use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// radioid.net's list of DMR IDs and call signs.
pub const DEFAULT_URL: &str = "https://radioid.net/static/dmrid.dat";
/// Largest list accepted (dmrid.dat is about 5 MB).
const MAX_BYTES: usize = 64 << 20;
/// A call sign / name from the list is cut to this many characters.
const MAX_NAME: usize = 24;
/// At most this many ISSIs per lookup.
const MAX_LOOKUP: usize = 2000;

/// The loaded list and the state of the last fetch.
#[derive(Default)]
pub struct Db {
    names: RwLock<HashMap<u32, String>>,
    status: Mutex<FetchStatus>,
    running: AtomicBool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FetchStatus {
    /// When the list in use was fetched (ms since 1970), 0 = never.
    pub updated_ms: u64,
    /// The last fetch failed with this.
    pub error: Option<String>,
}

impl Db {
    /// Name for `issi` from the list.
    pub fn get(&self, issi: u32) -> Option<String> {
        self.names.read().ok()?.get(&issi).cloned()
    }

    pub fn len(&self) -> usize {
        self.names.read().map_or(0, |n| n.len())
    }

    fn replace(&self, names: HashMap<u32, String>, updated_ms: u64) {
        if let Ok(mut n) = self.names.write() { *n = names; }
        if let Ok(mut s) = self.status.lock() { *s = FetchStatus { updated_ms, error: None }; }
    }

    fn clear(&self) {
        if let Ok(mut n) = self.names.write() { *n = HashMap::new(); }
    }

    /// Loads the cached list (silently nothing when there is none).
    pub fn load_cache(&self, path: &std::path::Path) {
        let Ok(bytes) = std::fs::read(path) else { return };
        let updated = std::fs::metadata(path).and_then(|m| m.modified()).ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
        let names = parse(&bytes);
        tracing::info!(path = %path.display(), entries = names.len(), "callsign database loaded from the cache");
        self.replace(names, updated);
    }
}

/// `ID;CALLSIGN;...` lines (also `,` or tab separated) -> ISSI -> call sign.
/// IDs outside 1..=16777215 and empty names are skipped.
pub fn parse(bytes: &[u8]) -> HashMap<u32, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = HashMap::new();
    for line in text.lines() {
        let mut f = line.split([';', ',', '\t']);
        let (Some(id), Some(name)) = (f.next(), f.next()) else { continue };
        let Ok(id) = id.trim().parse::<u32>() else { continue };
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
        if (1..=0xFF_FFFF).contains(&id) && !name.is_empty() {
            out.insert(id, name);
        }
    }
    out
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Milliseconds until the next 00:00 UTC.
fn until_midnight_utc(now_ms: u64) -> u64 {
    86_400_000 - now_ms % 86_400_000
}

/// Fetches the list now, keeps it in the cache file and uses it. One fetch at a time.
pub async fn refresh(state: &AppState) -> Result<usize, String> {
    if state.callsign_db.running.swap(true, Ordering::SeqCst) {
        return Err("an update is already running".into());
    }
    let cfg = state.callsign_cfg.read().map(|c| c.clone()).unwrap_or_default();
    let result = async {
        let body = http_get(&cfg.url).await.map_err(|e| format!("{e:#}"))?;
        let names = parse(&body);
        if names.is_empty() {
            return Err("the list has no ID;CALLSIGN lines".to_string());
        }
        if let Err(e) = write_cache(&cfg.cache, &body) {
            tracing::warn!(error = %e, path = %cfg.cache.display(), "cannot write the callsign database cache");
        }
        let n = names.len();
        state.callsign_db.replace(names, now_ms());
        Ok(n)
    }.await;
    match &result {
        Ok(n) => tracing::info!(entries = n, url = %cfg.url, "callsign database updated"),
        Err(e) => {
            tracing::warn!(error = %e, url = %cfg.url, "callsign database update failed");
            if let Ok(mut s) = state.callsign_db.status.lock() { s.error = Some(e.clone()); }
        }
    }
    state.callsign_db.running.store(false, Ordering::SeqCst);
    result
}

fn write_cache(path: &std::path::Path, body: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// With `auto`: fetch every day at 00:00 UTC (and once at the start when
/// switched on but nothing is loaded yet).
pub async fn run(state: Arc<AppState>) {
    let on = |s: &AppState| s.callsign_cfg.read().is_ok_and(|c| c.enabled && c.auto);
    if on(&state) && state.callsign_db.len() == 0 {
        let _ = refresh(&state).await;
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(until_midnight_utc(now_ms()) + 5_000)).await;
        if on(&state) {
            let _ = refresh(&state).await;
        }
    }
}

// ---------------------------------------------------------------- HTTP(S)

struct Url<'a> { tls: bool, host: &'a str, port: u16, path: &'a str }

fn parse_url(url: &str) -> Result<Url<'_>, String> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("https://") { (true, r) }
        else if let Some(r) = url.strip_prefix("http://") { (false, r) }
        else { return Err(format!("'{url}' is not an http:// or https:// URL")) };
    let (hostport, path) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || h.ends_with(']') => (h, p.parse().map_err(|_| format!("bad port in '{url}'"))?),
        _ => (hostport, if tls { 443 } else { 80 }),
    };
    if host.is_empty() { return Err(format!("no host in '{url}'")); }
    Ok(Url { tls, host: host.trim_start_matches('[').trim_end_matches(']'), port, path })
}

trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}

/// Public CA certificates of the system (Debian / the Docker image:
/// `ca-certificates`), or `SSL_CERT_FILE`.
fn tls_connector() -> anyhow::Result<tokio_rustls::TlsConnector> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let path = std::env::var("SSL_CERT_FILE").unwrap_or_else(|_| "/etc/ssl/certs/ca-certificates.crt".into());
    let certs = CertificateDer::pem_file_iter(&path)
        .and_then(|it| it.collect::<Result<Vec<_>, _>>())
        .map_err(|e| anyhow::anyhow!("reading CA certificates {path}: {e} (install ca-certificates)"))?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(certs);
    let cfg = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(cfg)))
}

/// `GET url`, following up to 3 redirects. HTTP/1.1, `Connection: close`,
/// a plain or chunked body of at most `MAX_BYTES`.
pub async fn http_get(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut url = url.trim().to_string();
    for _ in 0..4 {
        let u = parse_url(&url).map_err(|e| anyhow::anyhow!(e))?;
        let tcp = tokio::time::timeout(std::time::Duration::from_secs(20), tokio::net::TcpStream::connect((u.host, u.port))).await
            .map_err(|_| anyhow::anyhow!("connecting to {} timed out", u.host))??;
        let mut stream: Box<dyn Io> = if u.tls {
            let name = rustls::pki_types::ServerName::try_from(u.host.to_string())?;
            Box::new(tls_connector()?.connect(name, tcp).await?)
        } else {
            Box::new(tcp)
        };
        let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: */*\r\nConnection: close\r\n\r\n", u.path, u.host, crate::federation::USER_AGENT);
        stream.write_all(req.as_bytes()).await?;
        let mut buf = Vec::new();
        let read = tokio::time::timeout(std::time::Duration::from_secs(120), async {
            let mut chunk = [0u8; 65536];
            loop {
                match stream.read(&mut chunk).await {
                    Ok(0) => break Ok(()),
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.len() > MAX_BYTES + 65536 { break Err(anyhow::anyhow!("the list is larger than {} MB", MAX_BYTES >> 20)); }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !buf.is_empty() => break Ok(()),
                    Err(e) => break Err(e.into()),
                }
            }
        }).await.map_err(|_| anyhow::anyhow!("download timed out"))?;
        read?;
        let split = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| anyhow::anyhow!("malformed HTTP response"))?;
        let head = String::from_utf8_lossy(&buf[..split]).to_string();
        let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("bad HTTP status line"))?;
        let header = |name: &str| head.lines().skip(1).find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()));
        let body = &buf[split + 4..];
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let loc = header("location").ok_or_else(|| anyhow::anyhow!("redirect without Location"))?;
            url = if loc.starts_with("http://") || loc.starts_with("https://") { loc }
                else { format!("{}://{}:{}{}", if u.tls { "https" } else { "http" }, u.host, u.port, loc) };
            continue;
        }
        if status != 200 {
            anyhow::bail!("HTTP {status} from {}", u.host);
        }
        let body = if header("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) { dechunk(body)? } else { body.to_vec() };
        if body.len() > MAX_BYTES { anyhow::bail!("the list is larger than {} MB", MAX_BYTES >> 20); }
        return Ok(body);
    }
    anyhow::bail!("too many redirects")
}

fn dechunk(mut b: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = b.windows(2).position(|w| w == b"\r\n").ok_or_else(|| anyhow::anyhow!("bad chunked body"))?;
        let size = usize::from_str_radix(String::from_utf8_lossy(&b[..eol]).split(';').next().unwrap_or("").trim(), 16)?;
        b = &b[eol + 2..];
        if size == 0 { return Ok(out); }
        anyhow::ensure!(b.len() >= size, "chunked body cut off");
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
}

// ---------------------------------------------------------------- API

fn status_json(state: &AppState) -> serde_json::Value {
    let c = state.callsign_cfg.read().map(|c| c.clone()).unwrap_or_default();
    let s = state.callsign_db.status.lock().map(|s| s.clone()).unwrap_or_default();
    serde_json::json!({
        "enabled": c.enabled, "url": c.url, "auto": c.auto, "default_url": DEFAULT_URL,
        "entries": state.callsign_db.len(), "updated_ms": s.updated_ms, "error": s.error,
        "running": state.callsign_db.running.load(Ordering::SeqCst),
    })
}

/// `GET /api/callsign-db`: switch, URL, auto, entries, last update / error.
pub async fn status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(status_json(&state))
}

#[derive(Debug, Deserialize)]
pub struct LookupQuery { #[serde(default)] issi: String }

/// `GET /api/callsigns?issi=1,2,3`: call signs from the list for these
/// ISSIs (only the ones it has; empty while switched off).
pub async fn lookup(State(state): State<Arc<AppState>>, Query(q): Query<LookupQuery>) -> Json<BTreeMap<String, String>> {
    let on = state.callsign_cfg.read().is_ok_and(|c| c.enabled);
    let mut out = BTreeMap::new();
    if on {
        for i in q.issi.split(',').take(MAX_LOOKUP).filter_map(|t| t.trim().parse::<u32>().ok()) {
            if let Some(n) = state.callsign_db.get(i) { out.insert(i.to_string(), n); }
        }
    }
    Json(out)
}

#[derive(Debug, Deserialize)]
pub struct SetRequest {
    #[serde(default)] enabled: Option<bool>,
    #[serde(default)] url: Option<String>,
    #[serde(default)] auto: Option<bool>,
}

/// `POST /api/callsign-db/set` (admins): switch, URL (empty = the default),
/// auto. Switching on loads the cache, or fetches the list when there is none.
pub async fn set(State(state): State<Arc<AppState>>, Json(req): Json<SetRequest>) -> Response {
    if let Some(u) = &req.url {
        let u = u.trim();
        if !u.is_empty() && (u.len() > 500 || parse_url(u).is_err()) {
            return (StatusCode::BAD_REQUEST, format!("'{u}' is not an http:// or https:// URL")).into_response();
        }
    }
    let (was, now) = {
        let Ok(mut c) = state.callsign_cfg.write() else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
        let was = c.enabled;
        if let Some(e) = req.enabled { c.enabled = e; }
        if let Some(u) = &req.url { c.url = if u.trim().is_empty() { DEFAULT_URL.into() } else { u.trim().into() }; }
        if let Some(a) = req.auto { c.auto = a; }
        (was, c.clone())
    };
    if now.enabled && !was {
        state.callsign_db.load_cache(&now.cache);
        if state.callsign_db.len() == 0 {
            let st = state.clone();
            tokio::spawn(async move { let _ = refresh(&st).await; });
        }
    } else if !now.enabled && was {
        state.callsign_db.clear();
    }
    let cfg = state.config_snapshot();
    crate::CONFIG_WRITE_APPLIED_LIVE.store(true, Ordering::SeqCst);
    match crate::dashboard::save_config(&state, &cfg).await {
        Ok(_) => Json(status_json(&state)).into_response(),
        Err(e) => {
            crate::CONFIG_WRITE_APPLIED_LIVE.store(false, Ordering::SeqCst);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("applied, but writing the config file failed: {e}")).into_response()
        }
    }
}

/// `POST /api/callsign-db/update` (admins): fetch the list now.
pub async fn update_now(State(state): State<Arc<AppState>>) -> Response {
    if !state.callsign_cfg.read().is_ok_and(|c| c.enabled) {
        return (StatusCode::BAD_REQUEST, "switch the callsign database on first").into_response();
    }
    match refresh(&state).await {
        Ok(_) => Json(status_json(&state)).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CallsignDbConfig;

    #[test]
    fn parses_dmrid_lines() {
        let n = parse(b"2320972;OE4SXM;\r\n2321100;OE1ABC;\nbad;X;\n0;ZERO;\n99999999;BIG;\n2329999;;\n2320001,OE3XYZ,Name\n");
        assert_eq!(n.len(), 3);
        assert_eq!(n[&2320972], "OE4SXM");
        assert_eq!(n[&2320001], "OE3XYZ");
    }

    #[test]
    fn urls_and_midnight() {
        let u = parse_url("https://radioid.net/static/dmrid.dat").unwrap();
        assert!(u.tls && u.host == "radioid.net" && u.port == 443 && u.path == "/static/dmrid.dat");
        let u = parse_url("http://127.0.0.1:8080").unwrap();
        assert!(!u.tls && u.port == 8080 && u.path == "/");
        assert!(parse_url("ftp://x").is_err());
        assert_eq!(until_midnight_utc(86_400_000 * 5 + 1000), 86_400_000 - 1000);
        assert_eq!(dechunk(b"4\r\nabcd\r\n3\r\nefg\r\n0\r\n\r\n").unwrap(), b"abcdefg");
    }

    /// The list is fetched over HTTP (a local server here, with a redirect),
    /// cached, and looked up; Directory names stay first on the dashboard
    /// (it asks only for ISSIs without one). Off: no lookups.
    #[tokio::test]
    async fn fetch_cache_and_lookup() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut req = [0u8; 1024];
                let n = s.read(&mut req).await.unwrap();
                let req = String::from_utf8_lossy(&req[..n]).to_string();
                let resp = if req.starts_with("GET /old ") {
                    "HTTP/1.1 301 Moved\r\nLocation: /dmrid.dat\r\nContent-Length: 0\r\n\r\n".to_string()
                } else {
                    let body: String = (0..25u32).map(|i| format!("{};OE{}T{};\n", 2320000 + i, i % 9 + 1, i)).collect();
                    format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len())
                };
                s.write_all(resp.as_bytes()).await.unwrap();
                s.shutdown().await.ok();
            }
        });
        let cache = std::env::temp_dir().join(format!("brew-cs-{}.dat", uuid::Uuid::new_v4().simple()));
        let mut config = crate::config::Config::default();
        config.storage.enabled = false;
        config.callsign_db = CallsignDbConfig { enabled: true, url: format!("http://{addr}/old"), auto: false, cache: cache.clone() };
        let state = Arc::new(AppState::new(config, std::env::temp_dir().join("brew-cs-test.toml")).0);
        assert_eq!(refresh(&state).await.unwrap(), 25);
        assert_eq!(state.callsign_db.get(2320003).as_deref(), Some("OE4T3"));
        let Json(m) = lookup(State(state.clone()), Query(LookupQuery { issi: "2320003,2320024,1".into() })).await;
        assert_eq!(m.len(), 2, "only ISSIs in the list");
        // Restart: the cache is loaded without fetching.
        let db = Db::default();
        db.load_cache(&cache);
        assert_eq!(db.len(), 25);
        state.callsign_cfg.write().unwrap().enabled = false;
        let Json(m) = lookup(State(state.clone()), Query(LookupQuery { issi: "2320003".into() })).await;
        assert!(m.is_empty(), "switched off: no names");
        std::fs::remove_file(&cache).ok();
    }
}
