//! The redesigned dashboard, served at `/` (unless `[dashboard] legacy_ui =
//! true`) and always at `/ui`. One page with a sidebar (FlowStation's layout), light
//! and dark following the system. Its own pages read the same JSON APIs as
//! the classic dashboard. Only the classic map is shown inside it in its
//! embedded form (`?embed=1`, see `EMBED_STYLE`); SIP, SMS Center, Telemetry
//! SDS, HA and Server settings are native pages with the same APIs.

use axum::response::Html;

const VERSION: &str = env!("CARGO_PKG_VERSION");

static PAGE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let name = crate::dashboard::html_escape(crate::dashboard::server_name());
    PAGE_HTML.replace("__VERSION__", VERSION).replace("__SERVERNAME__", &name).replace("__LOGO__", LOGO_SVG.trim())
});

/// The brew-server logo (mug and wordmark) at the top of the sidebar. Light colours are the
/// attributes; the `bl-*` classes take the dark colours in dark mode (see `.blogo`).
const LOGO_SVG: &str = include_str!("../assets/logo/brew-server-logo-ui.svg");

/// Favicon: the mug alone, its steam follows the browser's light / dark mode.
pub const FAVICON_SVG: &str = include_str!("../assets/logo/brew-server-mark.svg");

/// `GET /favicon.svg`.
pub async fn favicon() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml"), (axum::http::header::CACHE_CONTROL, "public, max-age=86400")], FAVICON_SVG)
}

/// Home-screen app name (web app manifest); the person can rename it when adding the app.
const APP_NAME: &str = "brew-server";
/// App icons: the favicon on a dark background, full bleed (iOS and Android round them).
const ICON_192: &[u8] = include_bytes!("../assets/logo/app/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../assets/logo/app/icon-512.png");
const APPLE_TOUCH_ICON: &[u8] = include_bytes!("../assets/logo/app/apple-touch-icon.png");

/// `GET /manifest.webmanifest`: lets phones add the dashboard as an app.
pub async fn manifest() -> impl axum::response::IntoResponse {
    let body = serde_json::json!({
        "name": APP_NAME,
        "short_name": APP_NAME,
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "background_color": "#141416",
        "theme_color": "#141416",
        "icons": [
            {"src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any maskable"},
            {"src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable"}
        ]
    });
    ([(axum::http::header::CONTENT_TYPE, "application/manifest+json"), (axum::http::header::CACHE_CONTROL, "public, max-age=86400")], body.to_string())
}

fn png(bytes: &'static [u8]) -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/png"), (axum::http::header::CACHE_CONTROL, "public, max-age=86400")], bytes)
}

/// `GET /icon-192.png`.
pub async fn icon_192() -> impl axum::response::IntoResponse { png(ICON_192) }
/// `GET /icon-512.png`.
pub async fn icon_512() -> impl axum::response::IntoResponse { png(ICON_512) }
/// `GET /apple-touch-icon.png` (iOS home screen).
pub async fn apple_touch_icon() -> impl axum::response::IntoResponse { png(APPLE_TOUCH_ICON) }

/// Paths of the app icons and the manifest; they load without a login.
pub const APP_ASSETS: &[&str] = &["/manifest.webmanifest", "/icon-192.png", "/icon-512.png", "/apple-touch-icon.png"];

pub async fn page() -> Html<&'static str> {
    Html(PAGE.as_str())
}

// The classic pages' embedded form lives in `dashboard::STYLE` (`html.embed`).
const PAGE_HTML: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="light dark">
<title>__SERVERNAME__</title>
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="manifest" href="/manifest.webmanifest">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<meta name="apple-mobile-web-app-capable" content="yes">
<meta name="mobile-web-app-capable" content="yes">
<meta name="apple-mobile-web-app-title" content="brew-server">
<style>
:root{
  --bg:#eceff4;--bg2:#ffffff;--bg3:#f3f5f9;--bg4:#d6dde7;
  --border:#dde3ec;--sep:rgba(20,30,50,0.09);
  --accent:#1f4fe0;--accent2:#1236a8;--warn:#9a5400;--danger:#c0203a;--ok:#16876b;
  --text:#16202e;--text2:#3d4f66;--text3:#5f7188;
  --sidebar:#ffffff;--sidebar-border:#e3e8ef;
  --shadow:0 1px 3px rgba(20,30,50,.06),0 4px 16px -8px rgba(20,30,50,.10);
  --hair:inset 0 1px 0 rgba(255,255,255,.7);
  --on-accent:#ffffff;
  --mono:ui-monospace,'SF Mono','Cascadia Code',Menlo,Consolas,monospace;
  --sans:-apple-system,BlinkMacSystemFont,'Inter','Segoe UI',system-ui,sans-serif;
}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){
  --bg:#090d14;--bg2:#111824;--bg3:#19212f;--bg4:#2b3850;
  --border:#232e40;--sep:rgba(255,255,255,0.07);
  --accent:#4d86ff;--accent2:#8fb0ff;--warn:#ffb224;--danger:#ff4d6d;--ok:#2ec6a6;
  --text:#eef3fb;--text2:#94abc9;--text3:#7f93b5;
  --sidebar:#070a10;--sidebar-border:#161d2c;
  --shadow:0 1px 2px rgba(0,0,0,.45),0 10px 28px -14px rgba(0,0,0,.65);
  --hair:inset 0 1px 0 rgba(255,255,255,.05);
  --on-accent:#06102a;
}}
:root[data-theme=dark]{
  --bg:#090d14;--bg2:#111824;--bg3:#19212f;--bg4:#2b3850;
  --border:#232e40;--sep:rgba(255,255,255,0.07);
  --accent:#4d86ff;--accent2:#8fb0ff;--warn:#ffb224;--danger:#ff4d6d;--ok:#2ec6a6;
  --text:#eef3fb;--text2:#94abc9;--text3:#7f93b5;
  --sidebar:#070a10;--sidebar-border:#161d2c;
  --shadow:0 1px 2px rgba(0,0,0,.45),0 10px 28px -14px rgba(0,0,0,.65);
  --hair:inset 0 1px 0 rgba(255,255,255,.05);
  --on-accent:#06102a;
}
:root[data-theme=dark]{color-scheme:dark} :root[data-theme=light]{color-scheme:light}
@media (prefers-contrast:more){:root{--text2:var(--text);--text3:var(--text2);--border:var(--text3)}}
*{box-sizing:border-box}
html,body{margin:0;height:100%}
body{background:var(--bg);color:var(--text);font:14px/1.45 var(--sans);display:flex;-webkit-font-smoothing:antialiased}
button{font:inherit;color:inherit}
svg.i{width:18px;height:18px;stroke:currentColor;fill:none;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round;flex-shrink:0}
a{color:var(--accent2);text-decoration:none}
:focus-visible{outline:2px solid var(--accent2);outline-offset:2px}
[hidden]{display:none!important}
.sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0)}

/* Sidebar */
#sb{width:236px;min-width:236px;background:var(--sidebar);border-right:1px solid var(--sidebar-border);display:flex;flex-direction:column;height:100vh;height:100dvh;position:sticky;top:0;z-index:20}
.brand{padding:18px 16px 14px;border-bottom:1px solid var(--sidebar-border);display:flex;flex-direction:column;gap:12px}
.blogo{display:block;width:100%;max-width:196px;height:auto}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]) .bl-w{stroke:#fff} :root:not([data-theme=light]) .bl-t{fill:#fff} :root:not([data-theme=light]) .bl-s{fill:#4d86ff} :root:not([data-theme=light]) .bl-g{fill:#8b95a7}}
:root[data-theme=dark] .bl-w{stroke:#fff} :root[data-theme=dark] .bl-t{fill:#fff} :root[data-theme=dark] .bl-s{fill:#4d86ff} :root[data-theme=dark] .bl-g{fill:#8b95a7}
.bname{font-weight:700;font-size:14px;line-height:1.2;overflow-wrap:anywhere}
.bsub{font:10.5px var(--mono);color:var(--text3);letter-spacing:.04em}
.hw{border:1px solid var(--sidebar-border);border-radius:10px;padding:4px;background:color-mix(in srgb,var(--text) 3%,transparent);box-shadow:var(--hair)}
.hw-row{display:flex;align-items:center;gap:9px;padding:6px 7px;border-radius:7px;cursor:pointer}
.hw-row:hover{background:color-mix(in srgb,var(--text) 4%,transparent)}
.hw-row+.hw-row{box-shadow:inset 0 1px 0 var(--sidebar-border)}
.hw-g{width:22px;height:22px;border-radius:6px;display:grid;place-items:center;color:var(--accent);background:color-mix(in srgb,var(--accent) 13%,transparent)}
.hw-g svg{width:13px;height:13px}
.hw-g.b{color:var(--accent2);background:color-mix(in srgb,var(--accent2) 13%,transparent)}
.hw-k{font:700 9px var(--mono);letter-spacing:.12em;text-transform:uppercase;color:var(--text3)}
.hw-v{font:600 11.5px var(--mono);color:var(--text2)}
.dot{width:8px;height:8px;border-radius:50%;background:var(--ok);margin-left:auto;flex-shrink:0}
.dot.w{background:var(--warn)} .dot.d{background:var(--danger)} .dot.i{background:var(--text3)}
nav{padding:8px 8px 12px;overflow:auto;flex:1}
.grp{font:700 10.5px var(--sans);letter-spacing:.1em;text-transform:uppercase;color:var(--text3);padding:14px 10px 6px}
.nv{display:flex;align-items:center;gap:11px;padding:8px 10px;border-radius:8px;color:var(--text2);font-size:14px;cursor:pointer;min-height:36px;border:0;background:none;width:100%;text-align:left}
.nv svg{color:var(--text3)}
.nv:hover{background:color-mix(in srgb,var(--text) 5%,transparent)}
.nv.on{background:color-mix(in srgb,var(--accent) 13%,transparent);color:var(--accent);font-weight:600}
.nv.on svg{color:var(--accent)}
.badge{margin-left:auto;font:700 10.5px var(--mono);padding:1px 7px;border-radius:99px;background:color-mix(in srgb,var(--accent) 16%,transparent);color:var(--accent)}
.badge.w{background:color-mix(in srgb,var(--warn) 18%,transparent);color:var(--warn)}
.badge.d{background:color-mix(in srgb,var(--danger) 18%,transparent);color:var(--danger)}
.badge:empty{display:none}
.fgleg{display:flex;flex-wrap:wrap;gap:6px 16px;font-size:12.5px;padding-top:0}.fgleg i{display:inline-block;width:10px;height:10px;border-radius:3px;margin-right:5px;vertical-align:-1px}.fgleg i.lim{height:0;border-top:2px dashed var(--danger);border-radius:0;vertical-align:3px}
.fgw{padding:4px 18px 16px 48px}.fgc{position:relative;height:180px;border-left:1px solid var(--border);border-bottom:1px solid var(--border)}.fgc svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
.fgy{position:absolute;right:calc(100% + 6px);transform:translateY(-50%);font:11px var(--mono);color:var(--text3);white-space:nowrap}
.fgxs{position:relative;height:18px}.fgx{position:absolute;top:4px;transform:translateX(-50%);font:11px var(--mono);color:var(--text3);white-space:nowrap}.fgx:first-child{transform:none}.fgx:last-child{transform:translateX(-100%)}
@media (max-width:640px){.fgw{padding:4px 12px 14px 40px}.fgc{height:150px}.fgx:nth-child(even){display:none}}
.lock{margin-left:auto;color:var(--text3);display:none} .lock svg{width:13px;height:13px}
body.ro .lock{display:inline-flex}
.sbfoot{border-top:1px solid var(--sidebar-border);padding:10px 14px 12px;font:11px var(--sans);color:var(--text3);line-height:1.6}
.sbfoot a{color:var(--accent2)}
.sbfoot .acct{display:flex;align-items:center;gap:8px;margin-bottom:6px;color:var(--text2)}
.sbfoot .acct button{margin-left:auto}

/* Main */
main{flex:1;min-width:0;display:flex;flex-direction:column;height:100vh;height:100dvh;overflow:auto}
.top{position:sticky;top:0;z-index:5;display:flex;align-items:center;gap:12px;padding:14px 28px;border-bottom:1px solid var(--sidebar-border);
  background:color-mix(in srgb,var(--bg) 82%,transparent);-webkit-backdrop-filter:blur(14px) saturate(1.4);backdrop-filter:blur(14px) saturate(1.4)}
@media (prefers-reduced-transparency:reduce){.top{background:var(--bg)}}
.top h1{font-size:22px;font-weight:700;margin:0;letter-spacing:-.01em}
.pill{display:inline-flex;align-items:center;gap:6px;font:600 11.5px var(--sans);padding:3px 10px;border-radius:99px;border:1px solid var(--border);color:var(--text2);background:var(--bg2);white-space:nowrap}
.pill i{width:7px;height:7px;border-radius:50%;background:var(--ok)}
.pill.w i{background:var(--warn)} .pill.d i{background:var(--danger)}
.sp{flex:1}
.btn{display:inline-flex;align-items:center;gap:7px;min-height:32px;padding:0 12px;border-radius:8px;border:1px solid var(--border);background:var(--bg2);color:var(--text);font:600 12.5px var(--sans);cursor:pointer;white-space:nowrap}
.btn:hover{border-color:var(--text3)}
.btn svg{width:15px;height:15px}
.btn.pri{background:var(--accent);border-color:var(--accent);color:var(--on-accent)}
.btn.danger{color:var(--danger)}.btn.kill{background:var(--danger);border-color:var(--danger);color:#fff;font-weight:600}
.btn:disabled{opacity:.45;cursor:default}
.btn.ic{width:32px;padding:0;justify-content:center}
.content{padding:22px 28px 40px;display:flex;flex-direction:column;gap:18px;width:100%;max-width:1500px}
.emg{display:none;align-items:center;gap:10px;flex-wrap:wrap;background:var(--danger);color:#fff;padding:10px 28px;font-weight:700}
.emg.on{display:flex}
.emg .c{background:rgba(255,255,255,.2);border-radius:5px;padding:2px 8px;font-weight:600;font-size:13px}

.tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(165px,1fr));gap:12px}
.tile{background:var(--bg2);border:1px solid var(--border);border-radius:12px;padding:14px 16px;box-shadow:var(--shadow),var(--hair);cursor:default}
a.tile{color:inherit;cursor:pointer}
.tile .k{font-size:12.5px;color:var(--text2);display:flex;align-items:center;gap:7px}
.tile .k svg{width:15px;height:15px;color:var(--text3)}
.tile .v{font-size:28px;font-weight:700;letter-spacing:-.02em;margin-top:4px;font-variant-numeric:tabular-nums}
.tile .s{font-size:12.5px;color:var(--text3)}
.tile.live{border-color:color-mix(in srgb,var(--accent) 45%,var(--border))}
.tile.live .v{color:var(--accent)}

.card{background:var(--bg2);border:1px solid var(--border);border-radius:12px;box-shadow:var(--shadow),var(--hair);overflow:hidden}
.ch{display:flex;align-items:center;gap:10px;padding:13px 18px;border-bottom:1px solid var(--sep);flex-wrap:wrap}
.ch h2{margin:0;font-size:15px;font-weight:650}
.ch .cnt{font:600 11.5px var(--mono);color:var(--text3)}
.ch a{font-size:12.5px;font-weight:600}
.help{width:22px;height:22px;border-radius:50%;border:1px solid var(--border);display:grid;place-items:center;color:var(--text3);font:700 11px var(--sans);cursor:pointer;background:none;padding:0}
.help[aria-expanded=true]{color:var(--accent2);border-color:var(--accent2)}
.note{padding:12px 18px;font-size:12.5px;color:var(--text2);background:color-mix(in srgb,var(--accent2) 6%,transparent);border-top:1px solid var(--sep);display:flex;gap:10px}
.note[hidden]{display:none}
.note svg{color:var(--accent2);width:16px;height:16px;margin-top:1px}
.empty{padding:16px 18px;color:var(--text3);font-size:13px}
.grid2{display:grid;grid-template-columns:1.35fr 1fr;gap:18px}
.grid2.ov{grid-template-columns:repeat(2,minmax(0,1fr))}
.tgh{cursor:pointer;user-select:none}.tgh .chev{display:inline-block;width:14px;color:var(--text3);transition:transform .15s}.tgh[aria-expanded=true] .chev{transform:rotate(90deg)}
.card.shut .ch.tgh{border-bottom:0}
.tgadd{display:flex;gap:10px;flex-wrap:wrap;align-items:center;padding:12px 18px;border-bottom:1px solid var(--sep)}
.tgsaved{border-top:1px solid var(--sep)}.tgsaved>summary{padding:12px 18px;cursor:pointer;font-weight:650;font-size:14px;list-style-position:inside}
.tgt{table-layout:fixed}.tgt col.g{width:15%}.tgt col.n{width:37%}.tgt col.w{width:26%}.tgt col.a{width:22%}
.tgt td.tga{text-align:right;white-space:nowrap}.tgc{margin-top:3px;display:flex;flex-wrap:wrap;gap:4px}
@media (max-width:760px){.tgt col.g{width:24%}.tgt col.n{width:40%}.tgt col.a{width:36%}.tgt td.tga{white-space:normal}.tgt td.tga .btn{padding:0 9px}}
@media (prefers-reduced-motion:reduce){.tgh .chev{transition:none}}
.tw{overflow-x:auto;position:relative}
.tw.fllog{max-height:452px;overflow-y:auto}
/* SDS page: network SDS and station telemetry SDS side by side on wide screens, each list scrolls */
.sdsgrid{display:grid;gap:18px;grid-template-columns:minmax(0,1fr)}.sdsgrid>.card{margin:0}
@media (min-width:1280px){.sdsgrid.two{grid-template-columns:minmax(0,1fr) minmax(0,1fr)}}
.tw.sdsl{max-height:62vh;overflow-y:auto}.tw.sdsl th,.sheet.full .tw th{position:sticky;top:0;z-index:1;background:linear-gradient(color-mix(in srgb,var(--text) 2.5%,transparent),color-mix(in srgb,var(--text) 2.5%,transparent)),var(--bg2)}
.sdsgrid.two .sdsdir{display:none}.sdsgrid.two .sdstx{max-width:130px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.sheet.full{width:min(1600px,100%);height:92vh;max-height:92vh;display:flex;flex-direction:column}.sheet.full .sb{flex:1;min-height:0}.sheet.full .tw{flex:1;min-height:0;overflow:auto}.tw.fllog th{position:sticky;top:0;z-index:1;background:linear-gradient(color-mix(in srgb,var(--text) 2.5%,transparent),color-mix(in srgb,var(--text) 2.5%,transparent)),var(--bg2)}
.empty .tot{margin-top:6px;font-size:12.5px}.empty .tot b{font:600 15px var(--mono,ui-monospace,monospace);color:var(--text2)}
table{width:100%;border-collapse:collapse}
th{text-align:left;font:600 11px var(--sans);letter-spacing:.04em;text-transform:uppercase;color:var(--text3);padding:9px 18px;background:color-mix(in srgb,var(--text) 2.5%,transparent);white-space:nowrap}
td{padding:10px 18px;border-top:1px solid var(--sep);font-size:13.5px;vertical-align:middle}
td.m,.m{font-family:var(--mono);font-size:12.5px}
.mut{color:var(--text3)}
.num{font-variant-numeric:tabular-nums}
tr.click{cursor:pointer}
tr.click:hover td{background:color-mix(in srgb,var(--text) 2.5%,transparent)}
tr.off td{color:var(--text3)}
.chip{display:inline-flex;align-items:center;gap:5px;font:600 11.5px var(--sans);padding:2px 8px;border-radius:6px;background:var(--bg3);border:1px solid var(--border);color:var(--text2);white-space:nowrap;margin:1px 0}
.chip.a{color:var(--accent);background:color-mix(in srgb,var(--accent) 12%,transparent);border-color:color-mix(in srgb,var(--accent) 28%,transparent)}
.tsg{display:flex;align-items:center;gap:6px;margin-top:6px;font-size:12px;color:var(--text2)}.tsg .l{min-width:86px}.tss{width:38px;height:24px;border-radius:5px;border:1px solid var(--border);display:inline-flex;align-items:center;justify-content:center;font:600 11px var(--sans);color:var(--text3);background:var(--bg3)}.tss.b{color:var(--accent);background:color-mix(in srgb,var(--accent) 14%,transparent);border-color:color-mix(in srgb,var(--accent) 35%,transparent)}
.chip.b{color:var(--accent2);background:color-mix(in srgb,var(--accent2) 12%,transparent);border-color:color-mix(in srgb,var(--accent2) 28%,transparent)}
.chip.w{color:var(--warn);background:color-mix(in srgb,var(--warn) 12%,transparent);border-color:color-mix(in srgb,var(--warn) 30%,transparent)}
.chip.d{color:var(--danger);background:color-mix(in srgb,var(--danger) 12%,transparent);border-color:color-mix(in srgb,var(--danger) 30%,transparent)}
.chip.i{color:var(--text3)}
.tg{font:600 12px var(--mono);padding:2px 8px;border-radius:6px;background:var(--bg3);border:1px solid var(--border);display:inline-block;margin:1px 0;white-space:nowrap}
.tg.src{color:var(--accent);border-color:color-mix(in srgb,var(--accent) 35%,transparent);background:color-mix(in srgb,var(--accent) 10%,transparent)}
.st{display:inline-flex;align-items:center;gap:7px;font-weight:600;font-size:13px;white-space:nowrap}
.st i{width:8px;height:8px;border-radius:50%;background:var(--ok)}
.st.w i{background:var(--warn)} .st.d i{background:var(--danger)} .st.i i{background:var(--text3)} .st.i{color:var(--text3)}

.row{display:flex;align-items:center;gap:14px;padding:13px 18px;border-top:1px solid var(--sep)}
.row:first-child{border-top:0}
.ric{width:38px;height:38px;border-radius:10px;display:grid;place-items:center;background:color-mix(in srgb,var(--accent) 14%,transparent);color:var(--accent);flex-shrink:0}
.ric.p{background:color-mix(in srgb,var(--accent2) 14%,transparent);color:var(--accent2)}
.ric.e{background:color-mix(in srgb,var(--danger) 14%,transparent);color:var(--danger)}
.ric.s{background:var(--bg3);color:var(--text2);border:1px solid var(--border)}
.rm{flex:1;min-width:0}
.rt{font-weight:650;font-size:14.5px;display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.rs{font-size:12.5px;color:var(--text2);margin-top:2px;display:flex;gap:6px;flex-wrap:wrap;align-items:center}
.rr{text-align:right;font-size:12.5px;color:var(--text2);flex-shrink:0}
.rr b{font:600 13px var(--mono);color:var(--text2)}
.talk{display:inline-flex;align-items:center;gap:6px;color:var(--accent);font-weight:600}
.bars{display:inline-flex;gap:2px;align-items:flex-end;height:12px}
.bars b{width:3px;background:var(--accent);border-radius:1px;animation:eq 1s ease-in-out infinite}
.bars b:nth-child(2){animation-delay:-.3s}.bars b:nth-child(3){animation-delay:-.6s}.bars b:nth-child(4){animation-delay:-.15s}
@keyframes eq{0%,100%{height:4px}50%{height:12px}}
@media (prefers-reduced-motion:reduce){.bars b{animation:none;height:9px}}
.ev{display:flex;gap:10px;padding:10px 18px;border-top:1px solid var(--sep);align-items:center;font-size:13px;flex-wrap:wrap}
.ev:first-child{border-top:0}
.ev time{font:12px var(--mono);color:var(--text3);min-width:62px}
.arrow{color:var(--text3)}
.ml{margin-left:auto}

.bar{display:flex;gap:10px;align-items:center;flex-wrap:wrap}
.seg{display:inline-flex;background:var(--bg3);border:1px solid var(--border);border-radius:9px;padding:2px;flex-wrap:wrap}
.seg button{padding:5px 12px;border-radius:7px;font-size:12.5px;font-weight:600;color:var(--text2);border:0;background:none;cursor:pointer;min-height:28px}
.seg button[aria-pressed=true]{background:var(--bg2);color:var(--text);box-shadow:0 1px 2px rgba(0,0,0,.2)}
input.f,select.f,textarea.f{height:32px;padding:0 10px;border-radius:8px;border:1px solid var(--border);background:var(--bg3);color:var(--text);font:13px var(--sans);min-width:0}
input.f::placeholder,textarea.f::placeholder{color:var(--text3)}
.search{position:relative;display:inline-flex;align-items:center}
.search svg{position:absolute;left:9px;width:15px;height:15px;color:var(--text3);pointer-events:none}
.search input{padding-left:30px;width:240px}
.kv{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:10px 24px;padding:6px 0 10px}
.kv span.l{display:block;font-size:11px;color:var(--text3);text-transform:uppercase;letter-spacing:.04em;font-weight:600}
tr.exp td{background:var(--bg3);border-top:0}

/* Routing */
.tog{width:44px;height:26px;border-radius:13px;background:var(--bg4);position:relative;flex-shrink:0;border:0;cursor:pointer;padding:0}
.tog::after{content:"";position:absolute;top:2px;left:2px;width:22px;height:22px;border-radius:50%;background:#fff;box-shadow:0 1px 3px rgba(0,0,0,.35);transition:left .15s}
.tog[aria-checked=true]{background:var(--ok)} .tog[aria-checked=true]::after{left:20px}
.tog:disabled{opacity:.5;cursor:default}
@media (prefers-reduced-motion:reduce){.tog::after{transition:none}}
.pool .flow{display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin-top:6px}
.pool.offp .rt{color:var(--text3)} .pool.offp .flow{opacity:.6}
@media (max-width:760px){.row.pool{flex-wrap:wrap;align-items:flex-start}.row.pool .rm{flex:1 1 calc(100% - 70px);min-width:0}.row.pool>.btn{margin-left:auto}.row.pool>.btn+.btn{margin-left:0}}
.chk{width:18px;height:18px;accent-color:var(--accent);cursor:pointer}
.selbar{display:flex;gap:10px;align-items:center;padding:12px 18px;border-top:1px solid var(--sep);background:color-mix(in srgb,var(--accent) 6%,transparent);flex-wrap:wrap}
.msg{font-size:12.5px;padding:0 18px 12px}
.msg:empty{display:none}
#cfg-msg{white-space:pre-wrap;font-family:var(--mono,ui-monospace,SFMono-Regular,Menlo,monospace);font-size:12px}
.msg.err{color:var(--danger)} .msg.ok{color:var(--ok)}

/* Sheet (pool editor) */
.scrim{position:fixed;inset:0;background:rgba(0,0,0,.35);display:none;align-items:center;justify-content:center;z-index:50;padding:16px}
.scrim.on{display:flex}
.sheet{background:var(--bg2);border:1px solid var(--border);border-radius:14px;box-shadow:0 28px 64px -22px rgba(0,0,0,.5);width:min(520px,100%);max-height:90vh;overflow:auto}
.sheet h3{margin:0;font-size:17px}
.sheet .sb{padding:16px 20px;display:flex;flex-direction:column;gap:14px}
.sheet label.l{font-size:12px;font-weight:600;color:var(--text2);display:block;margin-bottom:5px}
.sheet .acts{display:flex;gap:10px;justify-content:flex-end;padding:14px 20px;border-top:1px solid var(--sep)}
.sheet input.f{width:100%}
.row2{display:flex;gap:10px}.row2>div{flex:1;min-width:0}

/* Account button (top right) and login window */
.acctbtn{display:inline-flex;align-items:center;gap:8px;min-height:34px;padding:0 10px 0 4px;border-radius:99px;border:1px solid var(--border);background:var(--bg2);color:var(--text);font:600 12.5px var(--sans);cursor:pointer}
.acctbtn:hover{border-color:var(--text3)}
.acctbtn .av{width:26px;height:26px;border-radius:50%;display:grid;place-items:center;background:var(--accent);color:var(--on-accent);font-weight:700;font-size:12px;text-transform:uppercase}
.acctbtn svg{width:14px;height:14px;color:var(--text3)}
.loginbtn{padding:0 14px}
.loginbtn svg{width:16px;height:16px}
.menu{position:absolute;right:28px;top:56px;z-index:40;min-width:220px;background:var(--bg2);border:1px solid var(--border);border-radius:12px;box-shadow:0 18px 48px -16px rgba(0,0,0,.45);padding:6px;display:none}
.menu.on{display:block}
.menu .who{padding:10px 12px 8px;border-bottom:1px solid var(--sep);margin-bottom:4px}
.menu .who b{display:block;font-size:14px}
.menu .who span{font-size:12px;color:var(--text3)}
.menu button{display:flex;width:100%;align-items:center;gap:10px;padding:9px 12px;border:0;background:none;color:var(--text);font:500 13px var(--sans);border-radius:8px;cursor:pointer;text-align:left}
.menu button:hover{background:var(--bg3)}
.menu button svg{width:16px;height:16px;color:var(--text3)}
.login{position:fixed;inset:0;z-index:60;display:none;align-items:center;justify-content:center;padding:16px;
  background:color-mix(in srgb,var(--bg) 55%,transparent);-webkit-backdrop-filter:blur(18px) saturate(1.3);backdrop-filter:blur(18px) saturate(1.3)}
.login.on{display:flex}
.login.solid{background:var(--bg)}
@media (prefers-reduced-transparency:reduce){.login{background:var(--bg)}}
.lcard{width:min(380px,100%);background:var(--bg2);border:1px solid var(--border);border-radius:18px;box-shadow:0 32px 80px -28px rgba(0,0,0,.6);padding:30px 28px 24px;position:relative}
.lcard .lmark{display:block;width:56px;height:56px;margin:0 auto 12px}
.lcard h2{text-align:center;margin:0;font-size:20px;letter-spacing:-.01em}
.lcard .lsub{text-align:center;color:var(--text3);font-size:13px;margin:4px 0 22px}
.lcard label{display:block;font-size:12px;font-weight:600;color:var(--text2);margin:0 0 6px}
.lcard .fld{position:relative;margin-bottom:14px}
.lcard input{width:100%;height:42px;padding:0 12px;border-radius:10px;border:1px solid var(--border);background:var(--bg3);color:var(--text);font:15px var(--sans)}
.lcard input:focus{outline:2px solid color-mix(in srgb,var(--accent) 55%,transparent);outline-offset:1px;border-color:var(--accent)}
.lcard .eye{position:absolute;right:6px;bottom:6px;width:30px;height:30px;border:0;background:none;color:var(--text3);cursor:pointer;border-radius:7px;display:grid;place-items:center}
.lcard .eye svg{width:17px;height:17px}
.lcard .go{width:100%;height:42px;justify-content:center;font-size:14px;margin-top:4px}
.lcard .lnote{font-size:13px;line-height:1.4;padding:9px 11px;border-radius:8px;margin:4px 0 10px;color:var(--warn);background:color-mix(in srgb,var(--warn) 12%,transparent);border:1px solid color-mix(in srgb,var(--warn) 30%,transparent)}
.lcard .lerr{color:var(--danger);font-size:13px;min-height:18px;margin:2px 0 8px;display:flex;gap:6px;align-items:center}
.lcard .lerr svg{width:15px;height:15px;flex-shrink:0}
.lcard .x{position:absolute;right:12px;top:12px;width:32px;height:32px;border:0;background:none;color:var(--text3);border-radius:8px;cursor:pointer;display:grid;place-items:center}
.lcard .x svg{width:16px;height:16px}
.lcard .lfoot{text-align:center;font-size:12px;color:var(--text3);margin-top:16px}
@keyframes shake{20%,60%{transform:translateX(-6px)}40%,80%{transform:translateX(6px)}}
.lcard.bad{animation:shake .35s}
@media (prefers-reduced-motion:reduce){.lcard.bad{animation:none}}
body.locked aside,body.locked main,body.locked .tabbar{visibility:hidden}

/* Accounts and configuration */
.svc{display:flex;gap:8px;flex-wrap:wrap;padding:12px 18px;border-bottom:1px solid var(--sep)}
.yes{color:var(--ok);font-weight:600} .no{color:var(--text3)}
textarea.cfg{width:100%;min-height:62vh;resize:vertical;border:0;border-top:1px solid var(--sep);background:var(--bg3);color:var(--text);font:12.5px/1.55 var(--mono,ui-monospace,SFMono-Regular,Menlo,monospace);padding:14px 18px;tab-size:4;white-space:pre;overflow:auto}
textarea.cfg:focus{outline:none;box-shadow:inset 0 0 0 2px color-mix(in srgb,var(--accent) 45%,transparent)}

iframe.embed{width:100%;border:0;min-height:calc(100vh - 130px);background:transparent}
/* The map sizes itself to the frame: a fixed frame, no growing with it. */
iframe.embed.fixed{min-height:0;height:calc(100dvh - 120px)}
.menu .mob{display:none}
.menu .thm{padding:8px 8px 6px;border-top:1px solid var(--sep);margin-top:4px}
.menu .thm .k{font-size:11px;color:var(--text3);font-weight:600;text-transform:uppercase;letter-spacing:.04em;margin:0 4px 6px}
.menu .thm .seg{display:flex;width:100%}
.menu .thm .seg button{flex:1;display:flex;justify-content:center;gap:5px;align-items:center;padding:6px 4px;font-size:12px;border-radius:7px;width:auto;margin:0}
.menu .thm .seg button svg{width:14px;height:14px}
.menu.anon .who,.menu.anon .acc-only{display:none}
.menu.anon .thm{border-top:0;margin-top:0}
.iconbtn{width:34px;height:34px;padding:0;justify-content:center;border-radius:50%;flex:none}
.kv.tight{grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:8px 18px}
td .sub{display:block;font-size:12px;color:var(--text3);font-weight:400;margin-top:2px;white-space:normal}
.yn{white-space:nowrap}

/* Phone */
.tabbar{display:none}
html{-webkit-text-size-adjust:100%;text-size-adjust:100%}
body{touch-action:manipulation}
@media (max-width:760px){
  /* Phones: the page itself scrolls (one scroller). A scrolling <main> inside a fixed-height body gave iOS two
     nested scrollers: the header moved away and parts of the page stayed blank while scrolling. */
  html{overscroll-behavior-y:none}
  html,body{height:auto;min-height:100%}
  body{display:block}
  main{display:block;height:auto;min-height:100vh;min-height:100dvh;overflow:visible}
  .top{position:sticky;top:0;z-index:25;padding-top:calc(12px + env(safe-area-inset-top));background:var(--bg);-webkit-backdrop-filter:none;backdrop-filter:none}
  .menu{position:fixed;top:calc(60px + env(safe-area-inset-top))}
  body.sbopen{overflow:hidden}
  #sb{padding-top:env(safe-area-inset-top)}
  /* iOS zooms into a field with text under 16px when it gets focus, and stays zoomed. */
  input.f,select.f,textarea.f,textarea.cfg,.sheet input,.sheet textarea,.sheet select{font-size:16px}
  #rochip{display:none}
  #sb{position:fixed;left:0;top:0;transform:translateX(-100%);transition:transform .2s;box-shadow:var(--shadow);z-index:35;padding-bottom:env(safe-area-inset-bottom)}
  body.sbopen #sb{transform:none}
  .top{padding-left:16px;padding-right:16px;padding-bottom:12px}
  .top h1{font-size:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
  .top{gap:8px}
  #acct-name,#acctbtn>svg{display:none}
  .hide-s{display:none!important}
  .content{padding:14px 16px 100px;gap:14px}
  .tiles{grid-template-columns:1fr 1fr;gap:10px}
  .grid2,.grid2.ov{grid-template-columns:1fr}
  th,td{padding-left:12px;padding-right:12px}
  .search input{width:100%}
  .bar>.search{flex:1 1 100%}
  /* Small round buttons keep their look and get a 42px touch area. */
  .help{position:relative}
  .help::after{content:"";position:absolute;inset:-10px}
  .tabbar{display:flex;position:fixed;left:12px;right:12px;bottom:calc(env(safe-area-inset-bottom) - 13px);height:62px;border-radius:31px;z-index:30;
    background:color-mix(in srgb,var(--bg2) 76%,transparent);-webkit-backdrop-filter:blur(20px) saturate(1.6);backdrop-filter:blur(20px) saturate(1.6);border:1px solid var(--border);box-shadow:var(--shadow);justify-content:space-around;align-items:center}
  .tab{display:flex;flex-direction:column;align-items:center;gap:2px;font-size:10.5px;color:var(--text3);font-weight:600;min-width:56px;min-height:44px;justify-content:center;border:0;background:none;cursor:pointer}
  .tab svg{width:22px;height:22px}
  .tab.on{color:var(--accent)}
  .menu{right:16px}
  .menu .mob{display:flex}
  .sbfoot .classic{display:none}
  iframe.embed.fixed{height:calc(100dvh - 150px)}
  /* Telemetry and other detail lists: two columns, smaller type */
  .kv.tight{grid-template-columns:1fr 1fr;gap:6px 14px;padding:4px 0 6px;font-size:13px}
  .kv.tight span.l{font-size:10px}
  .card .ch h2{font-size:14.5px}
  .tlm .chips{padding:0 14px 10px!important;gap:4px!important}
  .tlm .chips .chip{font-size:11px;padding:2px 7px}
  .tlm .kv{padding:10px 14px!important}
  .acc td,.acc th{padding-left:8px;padding-right:8px}
}
@media (max-width:760px) and (prefers-reduced-transparency:reduce){.tabbar{background:var(--bg2)}}
</style>
<script>try{const t=localStorage.getItem('brew-theme');if(t==='light'||t==='dark')document.documentElement.dataset.theme=t;}catch(e){}</script>
</head>
<body>
<aside id="sb" aria-label="Navigation">
  <div class="brand">
    __LOGO__
    <div><div class="bname">__SERVERNAME__</div><div class="bsub">brew-server v__VERSION__</div></div>
    <div class="hw">
      <div class="hw-row" data-go="stations"><div class="hw-g" aria-hidden="true"><svg class="i" viewBox="0 0 24 24"><path d="M4 12h4l3-8 4 16 3-8h2"/></svg></div><div><div class="hw-k">Brew</div><div class="hw-v" id="hw-brew">…</div></div><span class="dot i" id="hw-brew-dot"></span></div>
      <div class="hw-row" data-go="ha"><div class="hw-g b" aria-hidden="true"><svg class="i" viewBox="0 0 24 24"><path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z"/></svg></div><div><div class="hw-k">HA</div><div class="hw-v" id="hw-ha">…</div></div><span class="dot i" id="hw-ha-dot"></span></div>
    </div>
  </div>
  <nav id="nav"></nav>
  <div class="sbfoot">
    <span class="classic"><a href="/classic">Classic dashboard</a><br></span>
    brew-server by ysamouhos<br>
    Redesign + extra features: <a href="https://github.com/sebastianmadl" target="_blank" rel="noopener">OE4SXM</a>
  </div>
</aside>

<main>
  <div class="emg" id="emg" role="alert"><span>EMERGENCY ACTIVE</span><span id="emg-list" style="display:flex;gap:8px;flex-wrap:wrap"></span></div>
  <div class="top">
    <h1 id="title">Overview</h1>
    <span class="pill" id="live"><i></i><span>Live</span></span>
    <span class="sp"></span>
    <span class="pill" id="rochip" hidden title="You can look at everything; changes need an admin login">Read only</span>
    <button class="btn iconbtn" id="themebtn" hidden aria-haspopup="menu" aria-expanded="false" title="Appearance" onclick="toggleMenu(event,true)"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg><span class="sr">Appearance</span></button>
    <button class="btn loginbtn" id="loginbtn" hidden onclick="openLogin(false)"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M9 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h3M14 17l5-5-5-5M19 12H8"/></svg><span>Log in</span></button>
    <button class="acctbtn" id="acctbtn" hidden aria-haspopup="menu" aria-expanded="false" onclick="toggleMenu(event)"><span class="av" id="acct-av"></span><span id="acct-name"></span><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg></button>
  </div>
  <div class="menu" id="menu" role="menu"><div class="who"><b id="menu-name"></b><span id="menu-role"></span></div>
    <button role="menuitem" class="acc-only" onclick="go('accounts');closeMenu()" id="menu-acc"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M17 6l3 3M15 8l2 2"/></svg>Accounts</button>
    <button role="menuitem" class="mob acc-only" onclick="location.href='/classic'"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M9 9v11"/></svg>Classic dashboard</button>
    <button role="menuitem" class="acc-only" onclick="closeMenu();changePw()" id="menu-pw"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>Change password</button>
    <button role="menuitem" class="acc-only" onclick="doLogout()"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3M10 17l-5-5 5-5M5 12h11"/></svg>Log out</button>
    <div class="thm"><div class="k">Appearance</div><div class="seg" role="radiogroup" aria-label="Appearance" id="thm"></div></div>
  </div>
  <div class="content" id="content"></div>
</main>

<div class="tabbar" role="tablist" aria-label="Pages">
  <button class="tab" data-tab="overview"></button>
  <button class="tab" data-tab="calls"></button>
  <button class="tab" data-tab="stations"></button>
  <button class="tab" data-tab="radios"></button>
  <button class="tab" data-tab="more"></button>
</div>

<div class="login" id="login" role="dialog" aria-modal="true" aria-labelledby="login-title">
  <form class="lcard" id="lcard" onsubmit="submitLogin(event)" autocomplete="on">
    <button type="button" class="x" id="login-x" onclick="closeLogin()" title="Close"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg><span class="sr">Close</span></button>
    <img class="lmark" src="/favicon.svg" alt="" width="56" height="56">
    <h2 id="login-title">Sign in</h2>
    <div class="lsub">__SERVERNAME__</div>
    <div class="lnote" id="lnote" role="status" hidden></div>
    <div class="fld"><label for="lu">Username</label><input id="lu" name="username" autocomplete="username" autocapitalize="off" spellcheck="false" required></div>
    <div class="fld"><label for="lp">Password</label><input id="lp" name="password" type="password" autocomplete="current-password" required style="padding-right:42px">
      <button type="button" class="eye" onclick="const p=$('lp');p.type=p.type==='password'?'text':'password';this.setAttribute('aria-pressed',p.type==='text')" aria-pressed="false" title="Show password"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg><span class="sr">Show password</span></button></div>
    <div class="lerr" id="lerr" role="alert"></div>
    <button class="btn pri go" id="lgo" type="submit">Sign in</button>
    <div class="lfoot">brew-server dashboard</div>
  </form>
</div>

<div class="scrim" id="scrim" onclick="if(event.target===this)closeSheet()"><div class="sheet" role="dialog" aria-modal="true" aria-labelledby="sheet-title" id="sheet"></div></div>

<script>
'use strict';
const I={
 home:'<path d="M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6h-6v6H4a1 1 0 0 1-1-1z"/>',
 call:'<path d="M5 4h4l2 5-2.5 1.5a11 11 0 0 0 5 5L15 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 6a2 2 0 0 1 2-2"/>',
 sds:'<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
 radios:'<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0M16 4.5a3.5 3.5 0 0 1 0 7M21.5 20a6.5 6.5 0 0 0-4-6"/>',
 map:'<path d="M9 4L3 6v14l6-2 6 2 6-2V4l-6 2zM9 4v14M15 6v14"/>',
 bts:'<path d="M12 11v10M8 21h8M9.5 21l2.5-10 2.5 10"/><circle cx="12" cy="9" r="1.4"/><path d="M7.8 5.8a6 6 0 0 0 0 6.4M16.2 5.8a6 6 0 0 1 0 6.4"/>',
 routing:'<circle cx="5" cy="6" r="2"/><circle cx="5" cy="18" r="2"/><circle cx="19" cy="12" r="2"/><path d="M7 6h3a4 4 0 0 1 4 4 2 2 0 0 0 2 2h1M7 18h3a4 4 0 0 0 4-4"/>',
 roam:'<path d="M7 4L3 8l4 4M3 8h13a5 5 0 0 1 0 10h-2M17 20l4-4-4-4M21 16H8"/>',
 globe:'<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
 tele:'<path d="M3 12h4l3-7 4 14 3-7h4"/>',
 fleet:'<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><path d="M17.5 14v7M14 17.5h7"/>',
 ctl:'<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2"/>',
 sip:'<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 10h4M7 14h7M16 10h1"/>',
 sipcfg:'<path d="M4 6h10M4 12h6M4 18h10"/><circle cx="17" cy="6" r="2"/><circle cx="13" cy="12" r="2"/><circle cx="17" cy="18" r="2"/>',
 sms:'<path d="M4 6h16v12H4z"/><path d="M4 6l8 7 8-7"/>',
 power:'<path d="M12 3v9"/><path d="M6.3 6.3a8 8 0 1 0 11.4 0"/>',
 tsds:'<path d="M4 4h16v16H4z"/><path d="M8 9h8M8 13h8M8 17h5"/>',
 ha:'<path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z"/>',
 set:'<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 3 14H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.2-2.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 10 3.1V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0 1.2 2.9H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>',
 key:'<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M17 6l3 3M15 8l2 2"/>',
 file:'<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h4"/>',
 book:'<path d="M4 4.5A2.5 2.5 0 0 1 6.5 2H20v17H6.5A2.5 2.5 0 0 0 4 21.5zM4 21.5A2.5 2.5 0 0 1 6.5 19H20v3H6.5"/><path d="M9 7h7M9 11h5"/>',
 trash:'<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>',
 pin:'<path d="M12 21s-7-6.5-7-12a7 7 0 0 1 14 0c0 5.5-7 12-7 12z"/><circle cx="12" cy="9" r="2.5"/>',
 lock:'<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>',
 info:'<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>',
 more:'<circle cx="5" cy="12" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="19" cy="12" r="1.5"/>',
 plus:'<path d="M12 5v14M5 12h14"/>',
 search:'<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>',
 alert:'<path d="M12 3l10 18H2z"/><path d="M12 10v5M12 18v.5"/>'
};
const ic=(n,cls)=>`<svg class="i${cls?' '+cls:''}" viewBox="0 0 24 24" aria-hidden="true">${I[n]}</svg>`;
// Pages: [id, title, icon, group, embedded classic page or null, admin only]
const PAGES=[
 ['overview','Overview','home','Monitor'],['calls','Calls','call','Monitor'],['sds','SDS','sds','Monitor'],
 ['radios','Radios','radios','Monitor'],['issis','Directory','book','Monitor'],['map','Map','map','Monitor','/map'],
 ['stations','Stations & Cells','bts','Infrastructure'],['routing','Routing','routing','Infrastructure'],['roaming','Roaming','roam','Infrastructure'],['multinet','Multi Network','globe','Infrastructure',null,true],
 ['telemetry','Telemetry','tele','Infrastructure'],['radioctl','DGNA & Radio actions','ctl','Infrastructure'],['fleet','Fleet Control','fleet','Infrastructure'],['accounts','Accounts','key','Infrastructure'],
 ['sip','SIP / VoIP','sip','Integrations'],['sipcfg','SIP Config','sipcfg','Integrations'],
 ['sms','SMS Center','sms','Integrations'],
 ['ha','High Availability','ha','System'],['config','Config file','file','System',null,true],['settings','Server settings','set','System',null,true]
];
const PG=Object.fromEntries(PAGES.map(p=>[p[0],p]));
// Telemetry SDS has no menu entry any more: it is the second list on the SDS page. The page id
// stays as the right that shows that list (Server settings -> Users).
PG.tsds=['tsds','Telemetry SDS','tsds','Monitor'];
// Pages this login sees (Server settings -> Users): an admin all of them, any other login the ones
// ticked for it, someone not logged in the Viewer pages. The server refuses the rest anyway.
// The station config page (Fleet Control -> Edit) goes with the Fleet Control page.
const can=p=>!!PG[p]&&(D.who.admin||(!PG[p][5]&&(D.who.pages||[]).includes(p==='fleetcfg'||p==='fleettpl'||p==='fleetgraph'?'fleet':p)));
const canAny=(...ps)=>ps.some(can);
// A read route this login may ask (whoami 'allowed'); asking others would only be refused.
const mayGet=u=>D.who.admin||(D.who.allowed||[]).includes(u.split('?')[0]);
const firstPage=()=>PAGES.map(p=>p[0]).find(can);
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const tm=ms=>ms?new Date(ms).toLocaleTimeString([], {hour:'2-digit',minute:'2-digit',second:'2-digit'}):'';
const tmd=ms=>{if(!ms)return '';const d=new Date(ms),n=new Date();return d.toDateString()===n.toDateString()?tm(ms):d.toLocaleDateString()+' '+tm(ms);};
function dur(ms){const s=Math.max(0,Math.floor(ms/1000));const h=Math.floor(s/3600),m=Math.floor(s%3600/60),x=s%60;return (h?h+':'+String(m).padStart(2,'0'):String(m).padStart(2,'0'))+':'+String(x).padStart(2,'0');}
function ago(ms){if(!ms)return '—';const s=Math.max(0,(Date.now()-ms)/1000);if(s<60)return Math.floor(s)+' s';if(s<3600)return Math.floor(s/60)+' min';if(s<86400)return Math.floor(s/3600)+' h '+Math.floor(s%3600/60)+' min';return Math.floor(s/86400)+' d';}
const mhz=hz=>hz?(hz/1e6).toFixed(5).replace(/0+$/,'').replace(/\.$/,'')+' MHz':'—';
const isToday=ms=>new Date(ms).toDateString()===new Date().toDateString();
// Every request says it comes from this page: without a login the server
// then answers a plain 401 (this page's login window opens) instead of
// making the browser pop its own dialog.
async function api(u,opt){opt=opt||{};opt.headers=Object.assign({'x-brew-ui':'1'},opt.headers||{});opt.cache='no-store';
  const r=await fetch(u,opt);const t=await r.text();
  // Saving and editing are for admins: ask for an admin login, also when a user that only reads is signed in.
  if((r.status===401||r.status===403)&&opt.method&&opt.method!=='GET'){openLogin(false,ADMIN_NOTE);throw new Error(ADMIN_NOTE);}
  if(r.status===401){needAuth();throw new Error('Log in first');}
  if(!r.ok)throw new Error(t||String(r.status));try{return JSON.parse(t)}catch(e){return t}}
const getJ=u=>api(u);
const ADMIN_NOTE='For saving or editing please log in with an admin user.';
const postJ=(u,body)=>api(u,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
function doLogin(){openLogin(false);}
let authAsk=null;
function needAuth(){
  if($('login').classList.contains('on')||authAsk)return;
  authAsk=fetch('/api/whoami',{cache:'no-store',headers:{'x-brew-ui':'1'}}).then(r=>r.json()).then(w=>{D.who=w;openLogin(!!w.need_login);}).catch(()=>{}).finally(()=>{authAsk=null;});
}
function openLogin(must,note){
  closeMenu();const n=$('lnote');n.hidden=!note;n.textContent=note||'';$('login').classList.add('on');$('login').classList.toggle('solid',must);document.body.classList.toggle('locked',must);
  $('login-x').hidden=must;$('lerr').textContent='';$('lgo').disabled=false;setTimeout(()=>($('lu').value?$('lp'):$('lu')).focus(),30);
}
function closeLogin(){if(document.body.classList.contains('locked'))return;$('login').classList.remove('on');}
async function submitLogin(e){
  e.preventDefault();const b=$('lgo');b.disabled=true;b.textContent='Signing in…';$('lerr').textContent='';
  try{const r=await fetch('/api/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:$('lu').value.trim(),password:$('lp').value})});
    if(!r.ok)throw new Error(r.status===401?'Wrong username or password.':(await r.text())||'Could not sign in.');
    $('lp').value='';$('login').classList.remove('on');document.body.classList.remove('locked');
    await refreshSlow();await refreshFast();if(PG[UI.page][4])show(UI.page);else render(true);
  }catch(err){$('lerr').innerHTML=ic('alert')+esc(err.message);const c=$('lcard');c.classList.remove('bad');void c.offsetWidth;c.classList.add('bad');$('lp').select();}
  finally{b.disabled=false;b.textContent='Sign in';}
}
async function doLogout(){
  closeMenu();try{await fetch('/api/logout',{method:'POST'});}catch(e){}
  // Basic credentials the browser still caches (classic pages) are dropped the old way.
  const w=await fetch('/api/whoami',{cache:'no-store',headers:{'x-brew-ui':'1'}}).then(r=>r.json()).catch(()=>({}));
  if(w.username){location.href=location.protocol+'//logout:'+Date.now()+'@'+location.host+location.pathname;return;}
  location.reload();
}
function toggleMenu(e,anon){e.stopPropagation();const m=$('menu'),on=!m.classList.contains('on');m.classList.toggle('anon',!!anon);m.classList.toggle('on',on);
  (anon?$('themebtn'):$('acctbtn')).setAttribute('aria-expanded',on);drawTheme();}
function closeMenu(){$('menu').classList.remove('on');$('acctbtn').setAttribute('aria-expanded','false');$('themebtn').setAttribute('aria-expanded','false');}
// Appearance: follows the system unless the viewer picks light or dark (kept in this browser).
function curTheme(){return document.documentElement.dataset.theme||'system';}
function drawTheme(){const t=curTheme();const sun='<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
  moon='<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/></svg>',auto='<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 1 0 16z" fill="currentColor"/></svg>';
  $('thm').innerHTML=[['system','System',auto],['light','Light',sun],['dark','Dark',moon]].map(([k,l,i])=>`<button role="radio" aria-checked="${t===k}" aria-pressed="${t===k}" onclick="event.stopPropagation();setTheme('${k}')">${i}${l}</button>`).join('');}
function setTheme(t){
  if(t==='system')delete document.documentElement.dataset.theme;else document.documentElement.dataset.theme=t;
  try{if(t==='system')localStorage.removeItem('brew-theme');else localStorage.setItem('brew-theme',t);}catch(e){}
  drawTheme();if(PG[UI.page][4])show(UI.page);
}
document.addEventListener('click',e=>{if(!e.target.closest('#menu'))closeMenu();});

const D={fleet:null,status:null,conn:null,tele:[],pools:[],nets:null,radios:[],who:{admin:false},names:{},issi:{},gssi:{},access:{},tg:{managed:false,hidden:[],listed:[]},ctl:[],emg:[],ha:null,regs:[]};
const UI={page:'overview',open:{},filter:{},sel:new Set(),help:{},msg:{}};
// Messages survive the periodic re-render (kept in UI.msg, drawn by msgDiv).
function setMsg(id,cls,text){UI.msg[id]=[cls,text];const e=$(id);if(e){e.className='msg '+cls;e.textContent=text;}}
function msgDiv(id,style){const m=UI.msg[id]||['',''];return `<div class="msg ${m[0]}" id="${id}" role="status"${style?` style="${style}"`:''}>${esc(m[1])}</div>`;}
let ok=true;

function buildNav(){
  let h='',g='';
  for(const [id,t,icn,grp,emb,adm] of PAGES){
    if(!can(id))continue;
    if(grp!==g){h+=`<div class="grp">${grp}</div>`;g=grp;}
    h+=`<button class="nv" data-go="${id}">${ic(icn)}<span>${t}</span>${adm?`<span class="lock" title="Admins only">${ic('lock')}<span class="sr">admins only</span></span>`:''}<span class="badge${id==='sms'?' w':''}" id="b-${id}"></span></button>`;
  }
  $('nav').innerHTML=h;
  document.querySelectorAll('.tab').forEach(t=>{const id=t.dataset.tab;const p=id==='more'?['more','More','more']:PG[id];t.hidden=id!=='more'&&!can(id);t.innerHTML=ic(p[2])+'<span>'+(id==='stations'?'Stations':p[1])+'</span>';});
  UI.navKey=navKey();
}
document.addEventListener('click',e=>{
  const g=e.target.closest('[data-go]');if(g){go(g.dataset.go);return;}
  const t=e.target.closest('[data-tab]');if(t){if(t.dataset.tab==='more')document.body.classList.toggle('sbopen');else go(t.dataset.tab);return;}
  if(document.body.classList.contains('sbopen')&&!e.target.closest('#sb'))document.body.classList.remove('sbopen');
});
function go(p){if(p==='control')p='radioctl';if(p==='tsds')p='sds';if(!PG[p])p='overview';if(location.hash!=='#'+p)location.hash=p;else show(p);}
window.addEventListener('hashchange',()=>show(location.hash.slice(1)));
const navKey=()=>JSON.stringify([D.who.admin,D.who.pages]);
function show(p){
  if(p==='control')p='radioctl'; // the former Control page
  if(p==='tsds')p='sds'; // Telemetry SDS is a list on the SDS page
  if(!PG[p])p='overview';
  // A page this login does not see: the first one it does (or the login window).
  if(D.whoLoaded&&!can(p)){const f=firstPage();if(f){if(PG[p][5]&&!D.who.admin&&D.who.can_login)openLogin(false);p=f;}}
  if(UI.navKey!==navKey())buildNav();
  UI.page=p;document.body.classList.remove('sbopen');
  const navP=p==='fleetcfg'||p==='fleettpl'?'fleet':p==='fleetgraph'?'telemetry':p;
  document.querySelectorAll('.nv').forEach(n=>n.classList.toggle('on',n.dataset.go===navP));
  document.querySelectorAll('.tab').forEach(n=>n.classList.toggle('on',n.dataset.tab===navP));
  $('title').textContent=PG[p][1];
  document.title=PG[p][1]+' · '+document.querySelector('.bname').textContent;
  const emb=PG[p][4];
  // Admin pages of the classic dashboard: without an admin login the frame
  // would make the browser pop its own dialog, so open the login window.
  if(PG[p][5]&&!D.who.admin&&D.whoLoaded&&!D.who.need_login)openLogin(false);
  if(emb&&PG[p][5]&&!D.who.admin){$('content').innerHTML=`<div class="card"><div class="empty">${esc(PG[p][1])} is for admins. <button class="btn pri" onclick="openLogin(false)">Log in</button></div></div>`;return;}
  if(emb){const fixed=emb==='/map';$('content').innerHTML=`<iframe class="embed${fixed?' fixed':''}" title="${esc(PG[p][1])}" src="${emb}?embed=1${document.documentElement.dataset.theme?'&theme='+document.documentElement.dataset.theme:''}"></iframe>`;if(!fixed)fitFrame();}
  else render(true);
  $('content').parentElement.scrollTop=0;window.scrollTo(0,0);
  if(p==='calls'||p==='sds')refreshFast();
  loadPageData(p,true);
  // Lists only this page shows: ask for them now, not at the next slow refresh (up to 5 s of "Loading…").
  if(p==='radios')getJ('/api/registrations').then(v=>{D.regs=v;if(UI.page===p)render();}).catch(()=>{});
  if(p==='accounts'&&mayGet('/api/accounts'))getJ('/api/accounts').then(v=>{D.acc=v;if(UI.page===p)render();}).catch(()=>{});
  if(mayGet('/api/fleet')&&(p==='fleet'||!D.fleet))getJ('/api/fleet').then(v=>{D.fleet=v;fleetInfo();if(UI.page===p)render();}).catch(()=>{});
}
function fitFrame(){const f=document.querySelector('iframe.embed');if(!f)return;f.onload=()=>{try{const d=f.contentDocument;const fit=()=>{f.style.height=Math.max(window.innerHeight-130,d.documentElement.scrollHeight+20)+'px';};fit();new ResizeObserver(fit).observe(d.body);}catch(e){}};}
function helpBtn(key){return `<button class="help" aria-expanded="${!!UI.help[key]}" aria-controls="h-${key}" onclick="UI.help['${key}']=!UI.help['${key}'];render(true)" title="What is this?">?<span class="sr">Explain</span></button>`;}
function helpNote(key,text){return `<div class="note" id="h-${key}" ${UI.help[key]?'':'hidden'}>${ic('info')}<div>${text}</div></div>`;}

/* ---------- data ---------- */
async function refreshFast(){
  if(document.body.classList.contains('locked'))return;
  try{
    const [st,pools,emg]=await Promise.all([mayGet('/api/status')?getJ(statusUrl()):Promise.resolve(D.status),getJ('/api/pools'),getJ('/api/emergencies')]);
    D.status=st;D.pools=pools;D.emg=emg;ok=true;
  }catch(e){ok=false;}
  $('live').className='pill'+(ok?'':' d');$('live').lastChild.textContent=ok?'Live':'Offline';
  if(['sip','sms','ha'].includes(UI.page)&&can(UI.page))loadPageData(UI.page,false);
  $('emg').classList.toggle('on',D.emg.length>0);
  $('emg-list').innerHTML=D.emg.map(e=>`<span class="c">ISSI ${e.issi}${e.destination!=null?' → '+e.destination:''}${e.bts?' · '+esc(e.bts):''}</span>`).join('');
  render();
}
async function refreshSlow(){
  if(document.body.classList.contains('locked'))return;
  const opt=(u,f)=>mayGet(u)?getJ(u).then(f).catch(()=>{}):Promise.resolve();
  const tasks=[opt('/api/connections',v=>D.conn=v),opt('/api/telemetry',v=>D.tele=v),opt('/api/radios',v=>D.radios=v),
    getJ('/api/ha').then(v=>D.ha=v),getJ('/api/whoami').then(v=>{const was=navKey(),first=!D.whoLoaded;D.who=v;D.whoLoaded=true;
      if(was!==navKey()||first)show(UI.page);else if(first&&PG[UI.page][5]&&!v.admin&&!v.need_login)openLogin(false);
      // Admin-only route: asking it without a login answers 401 and opens the login window,
      // so an open (read-only) dashboard would get it every 5 s. Ask only once whoami says admin.
      if(v.admin)return getJ('/api/networks').then(n=>D.nets=n).catch(()=>{});})];
  if(UI.page==='radios')tasks.push(opt('/api/registrations',v=>D.regs=v));
  if(UI.page==='accounts')tasks.push(opt('/api/accounts',v=>D.acc=v));
  if(UI.page==='radioctl')tasks.push(opt('/api/dgna/assignments',v=>D.dgnaA=v));
  if(UI.page==='sipcfg'&&can('sipcfg'))loadPageData('sipcfg',false);
  tasks.push(getJ('/api/names').then(v=>D.names=v).catch(()=>{}));
  tasks.push(getJ('/api/network-names').then(v=>D.netNames=v).catch(()=>{}));
  tasks.push(getJ('/api/issi-names').then(v=>D.issi=v).catch(()=>{}));
  tasks.push(getJ('/api/issi-kinds').then(v=>D.ikind=v).catch(()=>{}));
  tasks.push(getJ('/api/gssi-names').then(v=>D.gssi=v).catch(()=>{}));
  tasks.push(getJ('/api/group-access').then(v=>D.access=v).catch(()=>{}));
  tasks.push(getJ('/api/talkgroups').then(v=>D.tg=v).catch(()=>{}));
  tasks.push(getJ('/api/callsign-db').then(v=>{if(D.csdb&&(D.csdb.updated_ms!==v.updated_ms||D.csdb.enabled!==v.enabled)){D.cs={};CS_ASKED.clear();}D.csdb=v;}).catch(()=>{}));
  tasks.push(opt('/api/control',v=>D.ctl=v));
  tasks.push(opt('/api/fleet',v=>{D.fleet=v;fleetInfo();}));
  await Promise.allSettled(tasks);
  await csLookup();
  chrome();render();
}
// Ham callsign database: ask the server for the ISSIs on screen that have no Directory name (once each).
const CS_ASKED=new Set();
async function csLookup(){
  if(!(D.csdb&&D.csdb.enabled&&D.csdb.entries))return;
  const want=new Set(),add=i=>{if(i>0&&i<=16777215)want.add(String(i));};
  const st=D.status||{};
  for(const c of [...(st.active_calls||[]),...(st.recent_calls||[])]){add(c.source);if(c.kind!=='group')add(c.destination);}
  for(const x of (st.recent_sds||[])){add(x.source);add(x.destination);}
  for(const m of ((D.conn&&D.conn.mobile_stations)||[]))add(m.issi);
  for(const n of ((D.nets&&D.nets.networks)||[]))for(const i of n.exported||[])add(i);
  for(const r of ((D.conn&&D.conn.roaming_log)||[]))add(r.issi);
  for(const r of (D.regs||[]))add(r.issi);
  for(const i of ((D.smsc&&D.smsc.known)||[]))add(i);
  for(const m of ((D.smsc&&D.smsc.messages)||[])){add(m.source_issi);add(m.destination);}
  const ask=[...want].filter(i=>!(D.issi||{})[i]&&!CS_ASKED.has(i)).slice(0,500);
  if(!ask.length)return;
  try{const m=await getJ('/api/callsigns?issi='+ask.join(','));ask.forEach(i=>CS_ASKED.add(i));D.cs=Object.assign(D.cs||{},m);}catch(e){}
}
function csCard(adm){
  const c=D.csdb;if(!c)return '';
  const open=!!UI.csOpen,when=c.updated_ms?new Date(c.updated_ms).toLocaleString():'never';
  const state=c.running?'updating…':c.enabled?`${c.entries.toLocaleString()} call signs · updated ${esc(when)}`:'';
  return `<div class="card${open?'':' shut'}"><div class="ch tgh" role="button" tabindex="0" aria-expanded="${open}" onclick="UI.csOpen=!${open};render(true)" onkeydown="if(event.key==='Enter'||event.key===' '){event.preventDefault();this.click()}">
    <span class="chev" aria-hidden="true">▸</span><h2>Ham callsign database</h2>${c.enabled?'<span class="chip a">on</span>':'<span class="chip i">off</span>'}${state?`<span class="cnt hide-s">${state}</span>`:''}<span onclick="event.stopPropagation()">${helpBtn('csdb')}</span><span class="sp"></span>
    <label style="display:flex;gap:10px;align-items:center" onclick="event.stopPropagation()"><button class="tog" role="switch" aria-checked="${!!c.enabled}" aria-label="Ham callsign database" ${adm?'':'disabled'} onclick="csSet({enabled:this.getAttribute('aria-checked')!=='true'})"></button></label></div>
   ${helpNote('csdb','Names radios without a Directory name by their call sign from a list of <code>ID;CALLSIGN</code> lines (default: radioid.net <code>dmrid.dat</code>, the DMR IDs that amateur TETRA networks use as ISSIs). <b>Directory names always come first.</b> <i>Now</i> fetches the list, <i>Automatic</i> every day at 00:00 UTC. A new list replaces the old one (one file, <code>[callsign_db] cache</code>); nothing else is stored or collected. Config <code>[callsign_db]</code>, no restart.')}
   ${open?`<div class="tgadd"><input class="f" id="cs-url" value="${esc(c.url)}" placeholder="${esc(c.default_url)}" style="flex:1;min-width:200px" ${adm?'':'disabled'} onkeydown="if(event.key==='Enter')csSet({url:this.value})" onchange="csSet({url:this.value})">
     <button class="btn" ${adm&&c.enabled&&!c.running?'':'disabled'} onclick="csNow()">${c.running?'Updating…':'Now'}</button>
     <label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600"><button class="tog" role="switch" aria-checked="${!!c.auto}" ${adm?'':'disabled'} onclick="csSet({auto:this.getAttribute('aria-checked')!=='true'})"></button>Automatic <span class="mut" style="font-weight:400">(daily 00:00 UTC)</span></label></div>
   <div class="msg ${c.error?'err':'mut'}" style="padding:10px 18px">${c.error?'Last update failed: '+esc(c.error):state}</div>${msgDiv('cs-msg','padding:0 18px 10px')}`:''}</div>`;
}
async function csSet(req){
  try{D.csdb=await postJ('/api/callsign-db/set',req);D.cs={};CS_ASKED.clear();if(req.enabled)UI.csOpen=true;setMsg('cs-msg','ok',req.enabled===false?'Switched off.':'Saved.');}catch(e){setMsg('cs-msg','err',e.message);}
  render(true);if(D.csdb&&D.csdb.running)setTimeout(refreshSlow,4000);
}
async function csNow(){
  D.csdb={...D.csdb,running:true};render(true);
  try{D.csdb=await postJ('/api/callsign-db/update',{});D.cs={};CS_ASKED.clear();await csLookup();setMsg('cs-msg','ok',`${D.csdb.entries.toLocaleString()} call signs loaded.`);}catch(e){setMsg('cs-msg','err',e.message);D.csdb={...D.csdb,running:false};}
  render(true);
}
function chrome(){
  const c=D.conn,st=D.status;
  const bs=c?new Set(c.brew_clients.filter(x=>x.mode==='Basestation'&&!asTerm(x.mode,x.station)).map(x=>x.station)).size:0;
  $('hw-brew').textContent=c?`${bs} station${bs===1?'':'s'}`:'…';
  $('hw-brew-dot').className='dot'+(ok?(bs?'':' w'):' d');
  const ha=D.ha;
  if(ha){$('hw-ha').textContent=ha.enabled?(ha.role[0].toUpperCase()+ha.role.slice(1)+(ha.vip_held?' · VIP held':'')):'Off (single node)';$('hw-ha-dot').className='dot'+(ha.enabled?(ha.role==='active'?'':' w'):' i');}
  const w=D.who;
  document.body.classList.toggle('ro',!w.admin);
  if(w.need_login&&!$('login').classList.contains('on'))openLogin(true);
  $('loginbtn').hidden=!(w.can_login&&!w.username);$('acctbtn').hidden=!w.username;$('themebtn').hidden=!!w.username;$('rochip').hidden=w.admin||!!w.need_login;
  if(w.username){$('acct-av').textContent=w.username[0];$('acct-name').textContent=w.username;$('menu-name').textContent=w.username;
    $('menu-role').textContent=w.admin?'Admin':'User (read only)';$('menu-acc').hidden=!w.admin;}
  // Badges of pages hidden for this user are not in the nav.
  const badge=(id,v)=>{const e=$(id);if(e)e.textContent=v;};
  if(st)badge('b-calls',st.active_calls.filter(x=>!legIds().has(x.uuid)).length||'');
  badge('b-radios',(c&&c.mobile_stations.length)||'');
  badge('b-routing',D.pools.filter(p=>p.enabled).length||'');
  fleetAlarmBadge();
}

/* ---------- pools ---------- */
function legIds(){
  // Leg calls: group calls on a running pool's legs from the same talker.
  const out=new Set();if(!D.status)return out;
  for(const p of D.pools){const l=p.live;if(!l)continue;
    for(const c of D.status.active_calls){if(c.kind==='group'&&c.source===l.talker&&l.legs.includes(c.destination))out.add(c.uuid);}}
  return out;
}
// The pool(s) a live call is heard on: a group in several pools carries the call on all of them,
// shown as one pool named "A + B" with all its legs.
function poolOf(call){const ps=D.pools.filter(p=>p.live&&p.live.gssi===call.destination&&p.live.talker===call.source);
  if(ps.length<2)return ps[0];
  return {name:ps.map(p=>p.name).join(' + '),live:{gssi:call.destination,talker:call.source,legs:[...new Set(ps.flatMap(p=>p.live.legs))],waiting:[...new Set(ps.flatMap(p=>p.live.waiting))]}};}
// The pool a call's group belongs to: one of its own groups, or a group of another network (alias
// above the 24 GSSI bits, slot << 24 | gssi). The network list is only loaded for admins, so without
// it the network name comes from the call itself, and failing that the GSSI alone decides.
function inPool(p,c){const g=c.destination;return g<=16777215?p.groups.includes(g):(p.remote||[]).some(r=>{
  const n=netOfSlot(Math.floor(g/16777216)),name=(n&&n.name)||(c.to_networks||[])[0]||c.from_network;
  return g%16777216===r.gssi&&(!name||name===r.network);});}
const poolsOfGroup=c=>D.pools.filter(p=>inPool(p,c));
// Recent calls: the legs of one pool call (same talker, groups of one pool, started within 3 s)
// become one row naming the pool, so a bridge does not fill the log with a row per group. A bridge
// whose pool is unknown here (same talker, same GSSI on another network) is joined too.
function mergePoolLegs(list){
  const out=[],used=new Set(),low=g=>g%16777216;
  list.forEach((c,i)=>{
    if(used.has(i))return;
    if(c.kind!=='group'){out.push(c);return;}
    // A group can be in several pools: every pool of a collected leg is a candidate, until no
    // further leg (same talker, within 3 s) joins.
    const cand=poolsOfGroup(c);
    const legs=[c];
    for(let grew=true;grew;){grew=false;
      for(let j=i+1;j<list.length&&j<=i+50;j++){const o=list[j];
        if(used.has(j)||o.kind!=='group'||o.source!==c.source||Math.abs(o.started_at_ms-c.started_at_ms)>3000)continue;
        if(cand.length?!cand.some(p=>inPool(p,o)):(o.destination===c.destination||low(o.destination)!==low(c.destination)))continue;
        legs.push(o);used.add(j);grew=true;
        for(const q of poolsOfGroup(o))if(!cand.includes(q))cand.push(q);}}
    if(legs.length<2){out.push(c);return;}
    // The pools that carried it hold at least two of its groups; the others only share a group.
    const real=cand.filter(p=>legs.filter(x=>inPool(p,x)).length>=2);
    const p=real.length?{name:real.map(x=>x.name).join(' + ')}:cand[0];
    const ends=legs.map(x=>x.ended_at_ms).filter(Boolean);
    out.push(Object.assign({},c,{pool:p?p.name:'TG '+low(c.destination),legs:legs.map(x=>x.destination),
      started_at_ms:Math.min(...legs.map(x=>x.started_at_ms)),ended_at_ms:ends.length===legs.length?Math.max(...ends):null,
      voice_frames:Math.max(...legs.map(x=>x.voice_frames)),
      from_network:legs.map(x=>x.from_network).find(Boolean)||null,
      to_networks:[...new Set(legs.flatMap(x=>x.to_networks||[]))]}));
  });
  return out;
}

/* ---------- history (Calls / SDS pages) ---------- */
// How many entries the Calls / SDS pages list (UI.show.calls / .sds): 25, 50, 100, 500 or 'all' (everything kept).
const SHOW_N=[25,50,100,500,'all'];
const showN=k=>{const v=(UI.show||{})[k];return SHOW_N.includes(v)?v:50;};
function statusUrl(){
  // Pool calls have one entry per group: ask for more so the merged list still fills the page.
  if(UI.page==='calls'){const n=showN('calls');return '/api/status?calls='+(n==='all'?'all':n*4);}
  if(UI.page==='sds')return '/api/status?sds='+showN('sds');
  return '/api/status';}
function showSel(k){const st=D.status||{},days=st.keep_days;
  return `<label class="mut" style="font-size:12.5px;display:flex;gap:6px;align-items:center">Show <select class="f" aria-label="How many to show" onchange="UI.show=UI.show||{};UI.show['${k}']=this.value==='all'?'all':+this.value;refreshFast();render(true)">${SHOW_N.map(n=>`<option value="${n}" ${showN(k)===n?'selected':''}>${n==='all'?'All'+(days?` (${days} days)`:''):n}</option>`).join('')}</select></label>`;}
async function clearHistory(k){
  const what=k==='calls'?'every call':'every SDS (with delivery reports and the telemetry SDS log)';
  if(!confirm(`Delete ${what} from the history? The totals stay. This cannot be undone.`))return;
  try{await postJ('/api/history/clear',k==='calls'?{calls:true}:{sds:true});setMsg(k+'-msg','ok',k==='calls'?'Calls deleted.':'SDS deleted.');await refreshFast();}catch(e){setMsg(k+'-msg','err',e.message);}
  render(true);}
async function clearRegLog(){
  if(!confirm('Delete the whole registration log? This cannot be undone.'))return;
  try{await postJ('/api/history/clear',{registrations:true});D.regs=[];setMsg('reg-msg','ok','Registration log deleted.');}catch(e){setMsg('reg-msg','err',e.message);}
  render(true);}
const keptNote=(st,k)=>{const d=st.keep_days;return `${k==='calls'?st.kept_calls:st.kept_sds} kept${d?` · up to ${d} days`:''}`;};

/* ---------- render ---------- */
let lastSig='';
// Phones (iOS Safari): replacing the page while it scrolls leaves blank areas. Live updates wait until the
// finger is off and the scroll has come to rest; a forced render (a click) still happens at once.
// scrollTimer only ever holds a real timer id: clearing a made-up id (such as 1) would stop
// whatever timer has that id, for example the live refresh interval.
let scrollBusy=false,scrollTimer=0,touchOn=false,renderLater=false;
function scrollRest(){clearTimeout(scrollTimer);scrollBusy=true;scrollTimer=setTimeout(()=>{scrollTimer=0;scrollBusy=false;if(!touchOn&&renderLater){renderLater=false;render();}},500);}
(()=>{const m=document.querySelector('main');if(!m)return;
  // Desktop scrolls <main>, phones the page itself.
  m.addEventListener('scroll',scrollRest,{passive:true});window.addEventListener('scroll',scrollRest,{passive:true});
  // Scroll does not bubble: catch inner scrollers too (change log, wide tables).
  document.addEventListener('scroll',scrollRest,{passive:true,capture:true});
  m.addEventListener('touchstart',()=>{touchOn=true;clearTimeout(scrollTimer);scrollTimer=0;scrollBusy=true;},{passive:true});
  const up=()=>{touchOn=false;scrollRest();};m.addEventListener('touchend',up,{passive:true});m.addEventListener('touchcancel',up,{passive:true});})();
function render(force){
  const p=UI.page;if(PG[p][4])return;
  if(!force&&(scrollBusy||touchOn)){renderLater=true;return;}
  // Keep focus and typing alive: pages with inputs only re-render when forced or nothing is focused inside.
  const a=document.activeElement;
  if(!force&&a&&$('content').contains(a)&&(a.tagName==='INPUT'||a.tagName==='SELECT'||a.tagName==='TEXTAREA'))return;
  const fn=R[p];if(!fn)return;
  const html=fn();
  if(force||html!==lastSig){
    // Inner scrollers (change log, wide tables) keep their position across a re-render.
    const c=$('content'),pos=[...c.querySelectorAll('.tw')].map(e=>[e.scrollTop,e.scrollLeft]);
    c.innerHTML=html;lastSig=html;
    c.querySelectorAll('.tw').forEach((e,i)=>{const q=pos[i];if(q&&(q[0]||q[1])){e.scrollTop=q[0];e.scrollLeft=q[1];}});}
}
const R={};
// Other networks a call or SDS crossed (Multi Network): where it went, where it came from.
function netChips(x){
  const to=(x.to_networks||[]).map(n=>`<span class="chip b" title="Sent to network ${esc(n)}">${ic('globe')} → ${esc(n)}</span>`);
  const from=x.from_network?[`<span class="chip i" title="Came from network ${esc(x.from_network)}">${ic('globe')} ← ${esc(x.from_network)}</span>`]:[];
  return from.concat(to).join(' ');
}
const hasNet=list=>list.some(x=>x.from_network||(x.to_networks||[]).length);
// Kind of an SDS from its protocol identifier (ETSI TS 100 392-2 table 29.21).
const SDS_KINDS=[['text','Text','a'],['pos','Position','b'],['data','WAP / data',''],['multi','Multi-part',''],['sec','Security','w'],['sys','System','i'],['app','App',''],['other','Other','i']];
const PID_NAMES={1:'OTAK (end-to-end re-keying)',2:'Simple text messaging',3:'Simple location system',4:'WAP',5:'WCMP',6:'Managed DMO',7:'PIN authentication',8:'End-to-end encrypted',9:'Simple immediate text',10:'Location information protocol (LIP)',11:'Net Assist Protocol 2',12:'Concatenated SDS',13:'DOTAM',14:'Simple AGNSS',130:'Text messaging (SDS-TL)',131:'Location system (SDS-TL)',132:'WAP (SDS-TL)',133:'WCMP (SDS-TL)',134:'Managed DMO (SDS-TL)',136:'End-to-end encrypted (SDS-TL)',137:'Immediate text (SDS-TL)',138:'Message with user data header',140:'Concatenated SDS (SDS-TL)',141:'AGNSS'};
const PID_KIND={2:'text',9:'text',130:'text',137:'text',3:'pos',10:'pos',131:'pos',4:'data',5:'data',132:'data',133:'data',138:'data',12:'multi',140:'multi',1:'sec',7:'sec',8:'sec',136:'sec',6:'sys',134:'sys',11:'sys',13:'sys',14:'sys',141:'sys'};
function sdsKind(p){if(p==null)return null;return PID_KIND[p]||((p>=64&&p<=126)||(p>=192&&p<=254)?'app':'other');}
function sdsChip(p){const k=sdsKind(p);if(!k)return '';const d=SDS_KINDS.find(x=>x[0]===k);const n=PID_NAMES[p]||(k==='app'?'User application':'Reserved');
  return `<span class="chip ${d[2]}" title="Protocol identifier ${p} (0x${p.toString(16).toUpperCase().padStart(2,'0')}): ${n}">${d[1]}</span>`;}
// Filter by kind (only kinds present in the list are offered).
function kindSel(key,pids){const have=new Set(pids.map(sdsKind).filter(Boolean));const v=UI.filter[key]||'all';
  return `<label class="mut" style="font-size:12.5px;display:flex;gap:6px;align-items:center">Kind <select class="f" aria-label="Kind of SDS" onchange="UI.filter['${key}']=this.value;render(true)"><option value="all">All</option>${SDS_KINDS.filter(([k])=>have.has(k)||v===k).map(([k,l])=>`<option value="${k}" ${v===k?'selected':''}>${l}</option>`).join('')}</select></label>`;}
const kindOk=(key,p)=>{const v=UI.filter[key]||'all';return v==='all'||sdsKind(p)===v;};
const callRow=(c,now,short)=>{
  const who=short?issiShort:issiTxt;
  const pool=poolOf(c);
  const grp=c.kind==='group';
  const em=c.priority>=15;
  const legs=pool?pool.live.legs.map(g=>tgChip(g)).join(' '):'';
  const wait=pool&&pool.live.waiting.length?` <span class="mut">waiting: ${pool.live.waiting.join(', ')}</span>`:'';
  return `<div class="row"><div class="ric${em?' e':grp?'':' p'}">${ic(grp?'radios':'call')}</div><div class="rm">
   <div class="rt">${grp?(short&&gName(c.destination)?tgShort(c.destination):'TG '+tgTxt(c.destination)):who(c.source)+' → '+who(c.destination)} <span class="chip ${em?'d':grp?'a':'b'}">${em?'Emergency':grp?'Group':'Individual'}</span>${pool?`<span class="chip">${ic('routing')} ${esc(pool.name)}</span>`:''}${netChips(c)?' '+netChips(c):''}</div>
   <div class="rs">${grp?`<span class="talk"><span class="bars"><b></b><b></b><b></b><b></b></span>${who(c.source)} talking</span>`:''}${pool?`<span class="mut">also on</span> ${legs}${wait}`:''}</div></div>
   <div class="rr"><b>${dur(now-c.started_at_ms)}</b><br>${c.voice_frames} frames · prio ${c.priority}</div></div>`;
};
function stationsList(){
  const c=D.conn;if(!c)return [];
  const byStation={};
  for(const x of c.cells||[]){(byStation[x.station]=byStation[x.station]||{name:x.station,cells:[],online:false,regs:0,addr:'',since:0,gone:0}).cells.push(x);
    const s=byStation[x.station];if(x.online!==false){s.online=true;s.regs=x.registered_issis;s.addr=x.remote_addr;s.since=x.connected_at_ms;}else s.gone=x.last_seen_ms;}
  // A station with cells in several networks has one connection per network: add up its radios.
  for(const b of c.brew_clients.filter(b=>b.mode==='Basestation'&&!asTerm(b.mode,b.station))){const s=byStation[b.station]||(byStation[b.station]={name:b.station,cells:[],online:true,regs:b.registered_issis,addr:b.remote_addr,since:b.connected_at_ms,gone:0});s.online=true;s.regs=(s.conns?s.regs:0)+b.registered_issis;s.conns=(s.conns||0)+1;s.since=b.connected_at_ms;s.addr=s.addr||b.remote_addr;s.ver=b.version;}
  for(const t of D.tele){const m=Object.values(byStation).find(s=>s.addr&&t.ip&&s.addr.startsWith(t.ip+':')&&!s.tele);if(m&&Object.values(byStation).filter(s=>s.addr&&s.addr.startsWith(t.ip+':')).length===1)m.tele=t;}
  return Object.values(byStation).sort((a,b)=>(b.online-a.online)||a.name.localeCompare(b.name));
}
R.overview=()=>{
  const st=D.status,c=D.conn,now=Date.now();
  if(!st)return '<div class="empty">Loading…</div>';
  const legs=legIds();
  const calls=st.active_calls.filter(x=>!legs.has(x.uuid));
  const stations=stationsList();
  const on=stations.filter(s=>s.online).length,off=stations.length-on;
  const cells=c?(c.cells||[]).filter(x=>x.online!==false).length:0;
  const roam=c?c.roaming_log||[]:[];
  const roamToday=roam.filter(r=>isToday(r.at_ms));
  const radios=c?c.mobile_stations.length:0;
  const grpCalls=calls.filter(x=>x.kind==='group').length;
  const sdsToday=st.recent_sds.filter(s=>isToday(s.at_ms)).length;
  const recent=mergePoolLegs(st.recent_calls).slice(0,5);
  const ms=c?[...c.mobile_stations].sort((a,b)=>a.issi-b.issi):[];
  return `<div class="tiles">
   <a class="tile" data-go="stations"><div class="k">${ic('bts')}Stations</div><div class="v">${on}</div><div class="s">${cells} cell${cells===1?'':'s'}${off?' · '+off+' offline':''}</div></a>
   <a class="tile" data-go="radios"><div class="k">${ic('radios')}Radios</div><div class="v">${radios}</div><div class="s">registered now</div></a>
   <a class="tile${calls.length?' live':''}" data-go="calls"><div class="k">${ic('call')}Active calls</div><div class="v">${calls.length}</div><div class="s">${grpCalls} group · ${calls.length-grpCalls} individual</div></a>
   <div class="tile"><div class="k">${ic('radios')}Groups</div><div class="v">${st.groups}</div><div class="s">with affiliations</div></div>
   <a class="tile" data-go="roaming"><div class="k">${ic('roam')}Cell changes</div><div class="v">${roamToday.length}</div><div class="s">today · ${roamToday.filter(r=>r.kind==='handover'||String(r.reselection||'').startsWith('announced')).length} announced</div></a>
   <a class="tile" data-go="sds"><div class="k">${ic('sds')}SDS</div><div class="v">${sdsToday}</div><div class="s">today · ${st.total_sds} total</div></a>
  </div>
  <div class="grid2 ov">
   <div class="card"><div class="ch"><h2>Live calls</h2><span class="cnt">${calls.length}</span><span class="sp"></span><a data-go="calls" href="#calls">All calls ›</a></div>
    ${calls.length?calls.map(x=>callRow(x,now,true)).join(''):`<div class="empty">No active calls<div class="tot"><b>${st.total_calls}</b> calls in total</div></div>`}</div>
   <div class="card"><div class="ch"><h2>Recent calls</h2><span class="sp"></span><a data-go="calls" href="#calls">All calls ›</a></div>
    ${recent.length?recent.map(x=>`<div class="ev"><time>${tm(x.started_at_ms)}</time><b class="m">${issiShort(x.source)}</b><span class="arrow">→</span>${x.pool?`<span class="chip" title="Pool: one call heard on ${x.legs.length} groups">${ic('routing')} ${esc(x.pool)}</span>`:x.kind==='group'?`<span class="tg">${tgShort(x.destination)}</span>`:`<b class="m">${issiShort(x.destination)}</b>`}${netChips(x)?' '+netChips(x):''}<span class="ml m mut">${x.ended_at_ms?dur(x.ended_at_ms-x.started_at_ms):'—'}</span></div>`).join(''):'<div class="empty">No calls yet</div>'}</div>
  </div>
  <div class="grid2 ov">
   <div class="card"><div class="ch"><h2>Stations</h2><span class="cnt">${stations.length}</span>${helpBtn('ovst')}<span class="sp"></span><a data-go="stations" href="#stations">Cells ›</a></div>
    ${helpNote('ovst',CELL_ID_HELP+' Cells are counted only for stations with the <code>bts-info</code> extension; a station without it is counted as a station, its cells are unknown. Apps and other terminals are not stations and have no cells.')}
    ${stations.length?stations.map(s=>`<div class="row"><div class="ric s">${ic('bts')}</div><div class="rm"><div class="rt">${esc(s.name)}${infoOf(s.name)?` <span class="mut" style="font-weight:500">(${esc(infoOf(s.name))})</span>`:''}</div><div class="rs">${s.cells.length?s.cells.length+' cell'+(s.cells.length===1?'':'s')+' · '+s.cells.map(x=>esc(x.name)+' ('+x.carrier+')').join(', '):'cells unknown (no <code>bts-info</code>)'}${s.tele&&s.tele.version?' · FlowStation '+esc(s.tele.version):''}</div></div>
      <div class="rr">${s.online?'<span class="st"><i></i>Online</span>':'<span class="st i"><i></i>Offline</span>'}<br>${s.online?s.regs+' radio'+(s.regs===1?'':'s'):(s.gone?'gone '+ago(s.gone):'')}</div></div>`).join(''):'<div class="empty">No stations connected</div>'}</div>
   <div class="card"><div class="ch"><h2>Radios</h2><span class="cnt">${ms.length}</span><span class="sp"></span><a data-go="radios" href="#radios">All radios ›</a></div>
    ${ms.length?ms.slice(0,6).map(m=>`<div class="ev"><b class="m">${issiTxt(m.issi)}</b>${m.cell&&m.cell.name?`<span class="chip">${esc(m.cell.name)}</span>`:m.basestation_name?`<span class="chip">${esc(m.basestation_name)}</span>`:''}<span class="ml mut">${(m.groups||[]).length?(m.groups||[]).length+' group'+((m.groups||[]).length===1?'':'s'):''}</span></div>`).join('')+(ms.length>6?`<div class="ev"><a data-go="radios" href="#radios" class="mut">+ ${ms.length-6} more</a></div>`:''):'<div class="empty">No radios registered</div>'}</div>
  </div>
  <div class="grid2 ov">
   <div class="card"><div class="ch"><h2>Recent cell changes</h2><span class="sp"></span><a data-go="roaming" href="#roaming">Roaming log ›</a></div>
    ${roam.length?roam.slice(0,5).map(r=>`<div class="ev"><time>${tm(r.at_ms)}</time><b class="m">${issiTxt(r.issi)}</b>${place(r.from)}<span class="arrow">→</span>${place(r.to)}<span class="ml">${howOf(r)}</span></div>`).join(''):'<div class="empty">No cell changes yet</div>'}</div>
   <div class="card"><div class="ch"><h2>Recent SDS</h2><span class="sp"></span><a data-go="sds" href="#sds">All SDS ›</a></div>
    ${st.recent_sds.length?st.recent_sds.slice(0,5).map(s=>`<div class="ev"><time>${tm(s.at_ms)}</time><b class="m">${issiShort(s.source)}</b><span class="arrow">→</span><b class="m">${issiShort(s.destination)}</b>${sdsChip(s.protocol_id)?' '+sdsChip(s.protocol_id):''}${netChips(s)?' '+netChips(s):''}<span class="ml mut">${s.reports?s.reports+' report'+(s.reports===1?'':'s'):''}</span></div>`).join(''):'<div class="empty">No SDS yet</div>'}</div>
  </div>`;
};
const RESEL={undeclared:['i','undeclared'],unannounced:['w','unannounced'],announced_type_1:['a','announced · type 1'],announced_type_2:['a','announced · type 2'],announced_type_3:['a','announced · type 3']};
function howOf(r){const t=RESEL[r.reselection];
  const where={handover:'<span class="chip b">prepared here</span>',within_station:'<span class="chip i">within station</span>'};
  if(!t)return r.kind==='handover'?'<span class="chip a">announced</span>':r.kind==='within_station'?where.within_station:'<span class="chip w">not announced</span>';
  return `<span class="chip ${t[0]}">${t[1]}</span>`+(where[r.kind]&&r.kind!=='within_station'?' '+where[r.kind]:(r.kind==='within_station'?' '+where.within_station:''));}
function place(p){return p.cell?`<span class="chip" title="carrier ${p.carrier??''}">${esc(p.cell)}</span>`:`<span class="chip">${esc(p.station)}</span>`;}

function seg(key,opts){const v=UI.filter[key]||opts[0][0];return `<div class="seg" role="group">${opts.map(([k,l])=>`<button aria-pressed="${v===k}" onclick="UI.filter['${key}']='${k}';render(true)">${l}</button>`).join('')}</div>`;}
function searchBox(key,ph){return `<label class="search">${ic('search')}<span class="sr">${ph}</span><input class="f" type="search" placeholder="${ph}" value="${esc(UI.filter[key]||'')}" oninput="UI.filter['${key}']=this.value;clearTimeout(window._st);window._st=setTimeout(()=>{render(true);const e=document.querySelector('.search input');if(e){e.focus();e.setSelectionRange(e.value.length,e.value.length)}},250)"></label>`;}
const match=(q,...v)=>!q||v.some(x=>String(x??'').toLowerCase().includes(q.toLowerCase()));

R.calls=()=>{
  const st=D.status,now=Date.now();if(!st)return '<div class="empty">Loading…</div>';
  const legs=legIds();const q=UI.filter.calls||'';const k=UI.filter.callk||'all';
  const act=st.active_calls.filter(x=>!legs.has(x.uuid));
  const rec=mergePoolLegs(st.recent_calls).filter(c=>(k==='all'||c.kind===k)&&match(q,c.source,c.destination,c.pool,(c.legs||[]).map(tgTxt).join(' '),c.from_network,(c.to_networks||[]).join(' '),nameOf(c.source),c.kind==='group'?gName(c.destination):nameOf(c.destination),c.kind==='group'&&c.destination>16777215?tgTxt(c.destination):''));
  const nc=hasNet(rec);
  return `<div class="card"><div class="ch"><h2>Live calls</h2><span class="cnt">${act.length}</span></div>${act.length?act.map(x=>callRow(x,now)).join(''):'<div class="empty">No active calls</div>'}</div>
  <div class="bar">${seg('callk',[['all','All'],['group','Group'],['private','Individual']])}${searchBox('calls','ISSI or group…')}<span class="sp"></span>${showSel('calls')}<span class="mut" style="font-size:12.5px">${st.total_calls} calls in total</span>${D.who.admin?`<button class="btn danger" onclick="clearHistory('calls')">${ic('trash')}Clear all calls</button>`:''}</div>
  ${msgDiv('calls-msg','padding:0 4px')}
  <div class="card"><div class="ch"><h2>Recent calls</h2><span class="cnt">${Math.min(rec.length,showN('calls')==='all'?1e9:showN('calls'))}</span><span class="sp"></span><span class="mut" style="font-size:12px">${keptNote(st,'calls')}</span></div><div class="tw"><table><thead><tr><th>Started</th><th>Type</th><th>From</th><th>To</th>${nc?'<th>Network</th>':''}<th>Duration</th><th>Prio</th><th>Voice frames</th></tr></thead><tbody>
   ${rec.slice(0,showN('calls')==='all'?undefined:showN('calls')).map(c=>`<tr><td class="m">${tmd(c.started_at_ms)}</td><td>${c.kind==='group'?'<span class="chip a">Group</span>':'<span class="chip b">Individual</span>'}</td><td>${issiCell(c.source)}</td><td>${c.pool?`<span class="chip" title="Pool: one call heard on ${c.legs.length} groups">${ic('routing')} ${esc(c.pool)}</span><span class="sub">${c.legs.map(g=>g>16777215?tgTxt(g):String(g)).join(' · ')}</span>`:c.kind==='group'?`<span class="m">${c.destination>16777215?tgTxt(c.destination):c.destination}</span>${gName(c.destination)?`<span class="sub">${esc(gName(c.destination))}</span>`:''}`:issiCell(c.destination)}</td>${nc?`<td>${netChips(c)}</td>`:''}<td class="m">${c.ended_at_ms?dur(c.ended_at_ms-c.started_at_ms):'—'}</td><td class="num">${c.priority}</td><td class="num">${c.voice_frames}</td></tr>`).join('')||'<tr><td colspan="${nc?8:7}" class="mut">No calls yet</td></tr>'}
  </tbody></table></div></div>`;
};
// SDS: the network's SDS and, with the Telemetry SDS right, the SDS the stations report over their
// telemetry link, side by side on wide screens. Each list scrolls; ⤢ shows it on the whole screen.
function sdsNetList(){const st=D.status,q=UI.filter.sds||'';
  return st.recent_sds.filter(s=>kindOk('sdsk',s.protocol_id)&&match(q,s.source,s.destination,nameOf(s.source),nameOf(s.destination),s.from_network,(s.to_networks||[]).join(' ')));}
function sdsNetTable(list){const nc=hasNet(list);
  return `<table><thead><tr><th>Time</th><th>Kind</th><th>From</th><th>To</th>${nc?'<th>Network</th>':''}<th>Reports</th></tr></thead><tbody>
  ${list.map(s=>`<tr><td class="m">${tmd(s.at_ms)}</td><td>${sdsChip(s.protocol_id)||'<span class="mut">—</span>'}</td><td>${issiCell(s.source)}</td><td>${issiCell(s.destination)}</td>${nc?`<td>${netChips(s)}</td>`:''}<td class="num">${s.reports}</td></tr>`).join('')||`<tr><td colspan="${nc?6:5}" class="mut">No SDS yet</td></tr>`}</tbody></table>`;}
const sdsTeleOn=()=>can('tsds')&&Array.isArray(D.tele);
function sdsTeleAll(){return D.tele.flatMap(s=>(s.recent_sds_out||[]).map(x=>({...x,bts:s.id}))).sort((a,b)=>b.at_ms-a.at_ms).slice(0,100);}
function sdsTeleList(){const q=UI.filter.sds||'';
  return sdsTeleAll().filter(x=>kindOk('tsdsk',x.protocol_id)&&match(q,x.source_issi,x.dest_issi,nameOf(x.source_issi),nameOf(x.dest_issi),x.bts,x.text));}
function sdsTeleTable(list){
  return `<table><thead><tr><th>Time</th><th>Kind</th><th>Station</th><th class="hide-s sdsdir">Direction</th><th>From</th><th>To</th><th>Text</th></tr></thead><tbody>
   ${list.map(x=>`<tr><td class="m">${tmd(x.at_ms)}</td><td>${sdsChip(x.protocol_id)}</td><td><b>${esc(x.bts)}</b></td><td class="hide-s sdsdir">${esc(x.direction)}</td><td class="m">${issiTxt(x.source_issi)}</td><td class="m">${x.is_group?'TG '+tgTxt(x.dest_issi):issiTxt(x.dest_issi)}</td><td><div class="sdstx" title="${esc(x.text||'')}">${x.protocol_id===10&&!(x.text||'').trim()?'<span class="mut">binary LIP (undecoded)</span>':esc(x.text)}</div></td></tr>`).join('')||'<tr><td colspan="7" class="mut">No telemetry SDS yet</td></tr>'}
   </tbody></table>`;}
const TSDS_HELP='The last SDS each station reports over its telemetry link (FlowStation <code>[telemetry]</code>), newest first, up to 100, with the text. <i>Kind</i> comes from the protocol identifier (ETSI TS 100 392-2 table 29.21; hover for the value): Text, Position (LIP, location system), WAP / data, Multi-part (concatenated), Security (OTAK, PIN, end-to-end encrypted), System (managed DMO, DOTAM, AGNSS), App (user application range) or Other. Binary LIP is shown as undecoded. The search above filters both lists.';
const fullBtn=w=>`<button class="btn" onclick="sdsFull('${w}')" title="Show this list on the whole screen" aria-label="Full screen">⤢</button>`;
R.sds=()=>{
  const st=D.status;if(!st)return '<div class="empty">Loading…</div>';
  const list=sdsNetList(),tele=sdsTeleOn(),tl=tele?sdsTeleList():[];
  if(UI.sdsFull)setTimeout(sdsFullFill,0);
  return `<div class="bar">${searchBox('sds','ISSI, name, text…')}${kindSel('sdsk',st.recent_sds.map(s=>s.protocol_id))}<span class="sp"></span>${showSel('sds')}<span class="mut" style="font-size:12.5px">${st.total_sds} SDS in total</span>${D.who.admin?`<button class="btn danger" onclick="clearHistory('sds')">${ic('trash')}Clear all SDS</button>`:''}</div>
  ${msgDiv('sds-msg','padding:0 4px')}
  <div class="sdsgrid${tele?' two':''}">
  <div class="card"><div class="ch"><h2>Recent SDS</h2><span class="cnt">${list.length}</span><span class="sp"></span><span class="mut" style="font-size:12px">${keptNote(st,'sds')}</span>${fullBtn('net')}</div><div class="tw sdsl">${sdsNetTable(list)}</div></div>
  ${tele?`<div class="card"><div class="ch"><h2>Telemetry SDS</h2><span class="cnt">${tl.length}</span>${helpBtn('tsds')}<span class="sp"></span>${kindSel('tsdsk',sdsTeleAll().map(x=>x.protocol_id))}${fullBtn('tele')}</div>${helpNote('tsds',TSDS_HELP)}<div class="tw sdsl">${sdsTeleTable(tl)}</div></div>`:''}
  </div>`;
};
// One list on the whole screen; it follows the live refresh while open.
function sdsFull(w){UI.sdsFull=w;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${w==='tele'?'Telemetry SDS':'Recent SDS'}</h3><span class="cnt" id="sdsf-n"></span></div><div class="sb"><div class="tw" id="sdsf-t"></div></div><div class="acts"><button class="btn" onclick="closeSheet()">Close</button></div>`;
  $('sheet').classList.add('full');$('scrim').classList.add('on');sdsFullFill();}
function sdsFullFill(){const w=UI.sdsFull,t=$('sdsf-t');if(!w||!t||!D.status)return;
  const l=w==='tele'?(sdsTeleOn()?sdsTeleList():[]):sdsNetList();
  const html=w==='tele'?sdsTeleTable(l):sdsNetTable(l);if(t.dataset.sig===html)return;
  const y=t.scrollTop;t.innerHTML=html;t.dataset.sig=html;t.scrollTop=y;$('sdsf-n').textContent=l.length;}
R.radios=()=>{
  const c=D.conn;if(!c)return '<div class="empty">Loading…</div>';
  const q=UI.filter.radios||'';
  const ms=c.mobile_stations.filter(m=>match(q,m.issi,nameOf(m.issi),m.basestation_name,(m.groups||[]).join(' '),(m.groups||[]).map(gName).join(' '),m.cell&&m.cell.name));
  const regs=(D.regs||[]).filter(r=>match(q,r.issi,r.bts));
  ms.sort((a,b)=>a.issi-b.issi);const isT=m=>asTerm(m.mode,m.basestation_name),radios=ms.filter(m=>!isT(m)),terms=ms.filter(isT);
  return `<div class="bar">${searchBox('radios','ISSI, group or station…')}</div>
  <div class="card"><div class="ch"><h2>Registered radios</h2><span class="cnt">${radios.length}</span>${helpBtn('radios')}</div>${helpNote('radios',CELL_ID_HELP+' <i>Station</i> is the Brew login the radio is registered through. <i>Cell</i> is shown when the station reports cells (<code>bts-info</code>, <code>ms-cell</code>); a station without these extensions is shown without a cell. Numbers logged in as Terminal (apps, consoles, bridges) are listed in their own card below.')}<div class="tw"><table><thead><tr><th>ISSI</th><th>Station</th><th>Cell (global ID)</th><th>Groups</th><th>Via</th><th>Station address</th></tr></thead><tbody>
  ${radios.map(m=>`<tr><td>${issiCell(m.issi,true)}</td><td><b class="m">${esc(m.basestation_name||'—')}</b>${infoOf(m.basestation_name)?`<span class="sub">${esc(infoOf(m.basestation_name))}</span>`:''}</td><td>${cellCell(m.cell&&m.cell.name,m.mode)}</td><td>${(m.groups||[]).map(g=>tgChip(g)).join(' ')||'<span class="mut">—</span>'}</td><td>${esc(m.mode)}</td><td class="m mut">${esc(m.basestation_addr||'')}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No radios registered</td></tr>'}
  </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Terminals &amp; bridges</h2><span class="cnt">${terms.length}</span>${helpBtn('rterms')}</div>${helpNote('rterms','Numbers registered in Terminal mode: apps and consoles (<i>Terminal</i>) and bridges to other systems (<i>Bridge</i>), plus the radios of a Basestation login whose type is set to Terminal or Bridge. The type is set on the Accounts, Stations &amp; Cells or Directory page (Auto: as it logs in). They have no cells.')}<div class="tw"><table><thead><tr><th>Number</th><th>Type</th><th>Login</th><th>Groups</th><th>Address</th></tr></thead><tbody>
  ${terms.map(m=>`<tr><td>${issiCell(m.issi,true)}</td><td>${tKindChip(m.basestation_name,m.issi)}</td><td><b class="m">${esc(m.basestation_name||'—')}</b>${infoOf(m.basestation_name)?`<span class="sub">${esc(infoOf(m.basestation_name))}</span>`:''}</td><td>${(m.groups||[]).map(g=>tgChip(g)).join(' ')||'<span class="mut">—</span>'}</td><td class="m mut">${esc(m.basestation_addr||'')}</td></tr>`).join('')||'<tr><td colspan="5" class="mut">No terminals or bridges registered</td></tr>'}
  </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Registration log</h2><span class="cnt">${regs.length}</span><span class="sp"></span><span class="mut" style="font-size:12px">${D.status&&D.status.keep_days?`kept up to ${D.status.keep_days} days`:'kept across restarts'}${regs.length>1000?' · newest 1000 shown, search for older':''}</span>${D.who.admin?`<button class="btn danger" onclick="clearRegLog()">${ic('trash')}Clear</button>`:''}</div>${msgDiv('reg-msg','padding:0 18px')}<div class="tw sdsl"><table><thead><tr><th>Time</th><th>ISSI</th><th>Event</th><th>Station</th><th>Cell (global ID)</th></tr></thead><tbody>
  ${regs.slice(0,1000).map(r=>`<tr><td class="m">${tmd(r.at_ms)}</td><td>${issiCell(r.issi)}</td><td>${r.kind==='register'?'<span class="chip a">register</span>':r.kind==='timeout'?'<span class="chip w">timeout</span>':`<span class="chip i">${esc(r.kind)}</span>`}</td><td>${esc(r.bts||'')}</td><td>${r.cell?'<span class="chip">'+esc(r.cell)+'</span>':'<span class="mut">—</span>'}</td></tr>`).join('')||'<tr><td colspan="5" class="mut">No registrations yet</td></tr>'}
  </tbody></table></div></div>`;
};
// Name of an ISSI from the directory, if any.
// Directory names first (always!), then the ham callsign database (D.cs, only asked for ISSIs without one).
const nameOf=issi=>(D.issi||{})[String(issi)]||(D.cs||{})[String(issi)]||'';
const gName=g=>(D.gssi||{})[String(g)]||'';
// A group of another network shows as "network:GSSI" (inside the server it
// is an alias above the 24 GSSI bits, see netlink.rs).
const netOfSlot=s=>((D.nets&&D.nets.networks)||D.netNames||[]).find(n=>n.slot===s);
const tgTxt=g=>{if(g>16777215){const n=netOfSlot(Math.floor(g/16777216));return `${esc(n?n.name:'net '+Math.floor(g/16777216))}:${g%16777216}`;}const n=gName(g);return n?`${g} · ${esc(n)}`:String(g);};
// Compact form for the Overview: the group name alone when there is one (the GSSI stays in the tooltip).
const tgShort=g=>{const n=g>16777215?'':gName(g);return n?`<span title="GSSI ${g}">${esc(n)}</span>`:tgTxt(g);};
const tgChip=(g,cls)=>`<span class="tg${cls||''}"${gName(g)?` title="${esc(gName(g))}"`:''}>${tgTxt(g)}</span>`;
// "2320972 · OE4SXM" for running text, number and name stacked for tables.
const issiTxt=issi=>{const n=nameOf(issi);return n?`${issi} · ${esc(n)}`:String(issi);};
// Compact form for the Overview: the name alone when there is one (the ISSI stays in the tooltip).
const issiShort=issi=>{const n=nameOf(issi);return n?`<span title="ISSI ${issi}">${esc(n)}</span>`:String(issi);};
const issiCell=(issi,bold)=>{const n=nameOf(issi);return `${bold?'<b class="m">':'<span class="m">'}${issi}${bold?'</b>':'</span>'}${n?`<span class="sub">${esc(n)}</span>`:''}`;};
// Info text of a login (Accounts page), if any.
// A login's name: its Accounts name ([bts_locations]), else a Directory name (bridge logins up to 12 digits).
const infoOf=login=>(D.names||{})[login]||nameOf(login);
// Station and cell naming, the same on every page: a station is named by its Brew login; a cell by
// its global cell ID <login>/<cell number in the station>. The global ID is a brew-server and
// FlowStation name for Brew and the dashboard, not an ETSI identity.
const CELL_ID_HELP='<b>Station</b> = a base station (FlowStation) connected in Basestation mode, named by its Brew login. <b>Global cell ID</b> = <i>login/cell</i>: the Brew login of the station and the number of the cell inside the station (local cell number, 0 = primary), e.g. 1000002/1. The global cell ID is defined by brew-server and FlowStation for Brew and this dashboard; it is not an ETSI identity. On the air interface a cell is identified by MCC, MNC, main carrier and band plan; the location area (LA) is shown separately.';
// The cell column of a radio: its global cell ID, or why there is none.
// Type of a login or number ([issi_kinds], set on Accounts, Stations and Directory): 'terminal', 'bridge' or
// '' for Auto (as it logs in: Basestation mode = station, Terminal mode = terminal).
const kindOver=n=>{const v=(D.ikind||{})[String(n)];return v==='bridge'||v==='terminal'?v:'';};
// Listed with the terminals and bridges: Terminal mode, or a Basestation login set to Terminal or Bridge.
const asTerm=(mode,login)=>mode==='Terminal'||(mode==='Basestation'&&!!kindOver(login));
// Terminal or bridge: the first of the numbers (login, own number) with a type set, else "terminal".
const tKind=(...ns)=>{for(const n of ns){const v=kindOver(n);if(v)return v;}return 'terminal';};
const tKindChip=(...ns)=>tKind(...ns)==='bridge'?'<span class="chip b">Bridge</span>':'<span class="chip i">Terminal</span>';
// Type column of a login on Stations & Cells: what it is listed as, "set" when overridden, and (admins) Change.
const KIND_LABEL={auto:'Auto',terminal:'Terminal',bridge:'Bridge'};
const kindCell=(login,mode)=>{const o=kindOver(login);
  const chip=asTerm(mode,login)?tKindChip(login):'<span class="chip a">Base station</span>';
  return chip+(o&&mode==='Basestation'?`<span class="sub">set, logs in as Basestation</span>`:'')+(D.who.admin&&/^\d{1,12}$/.test(String(login))?` <button class="btn" style="min-height:26px;padding:0 8px;font-size:12px" onclick="editKind('${esc(login)}','${esc(mode)}')" title="Auto, Terminal or Bridge">Change</button>`:'');};
// The Auto / Terminal / Bridge choice in a dialog (Accounts, Stations, Directory).
const kindSeg=n=>{const cur=kindOver(n)||'auto';
  return `<div><span class="l" style="display:block;font-size:12px;font-weight:600;color:var(--text2)">Type</span><div class="seg" role="radiogroup" id="in-kind" style="margin-top:5px">${['auto','terminal','bridge'].map(v=>`<button type="button" role="radio" data-v="${v}" aria-pressed="${cur===v}" aria-checked="${cur===v}" onclick="document.querySelectorAll('#in-kind button').forEach(b=>{const on=b===this;b.setAttribute('aria-pressed',on);b.setAttribute('aria-checked',on);})">${KIND_LABEL[v]}</button>`).join('')}</div><div class="mut" style="font-size:12px;margin-top:4px"><i>Auto</i>: as it logs in (Basestation = station, Terminal = terminal). <i>Terminal</i> or <i>Bridge</i>: listed under Terminals &amp; bridges on Radios and Stations &amp; Cells, even when it logs in as Basestation. Only changes how the dashboard lists it.</div></div>`;};
const kindPicked=()=>{const b=document.querySelector('#in-kind button[aria-pressed="true"]');return b?b.dataset.v:null;};
// Type of a login from the Stations page.
function editKind(login,mode){
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Type of ${esc(login)}</h3></div><div class="sb">
   <div class="mut" style="font-size:12.5px">Logs in as <b>${esc(mode)}</b>${infoOf(login)?' · '+esc(infoOf(login)):''}.</div>
   ${kindSeg(login)}
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveKind('${esc(login)}')">Save</button></div>`;
  $('scrim').classList.add('on');
}
async function saveKind(login){const m=$('sheet-msg'),kind=kindPicked();
  if(!kind||kind===(kindOver(login)||'auto')){closeSheet();return;}
  m.className='msg mut';m.textContent='Saving…';
  try{D.ikind=await postJ('/api/issi-kinds/set',{issi:login,kind});closeSheet();render(true);}catch(e){m.className='msg err';m.textContent=e.message;}}
const cellCell=(name,mode,...ns)=>name?`<span class="chip">${esc(name)}</span>`:mode==='Terminal'?`<span class="mut" title="Terminals and bridges have no cells">— (${tKind(...ns)})</span>`:'<span class="mut" title="The station reports no cells">—</span>';
// Global cell ID of a station cell seen in telemetry, by its carrier (unique in a network).
const gcidOf=carrier=>{const c=((D.conn&&D.conn.cells)||[]).find(x=>x.online!==false&&x.carrier===carrier);return c?c.name:'';};
// "Bost FlowStation/v0.5.6-55a08812" -> "Bost FlowStation v0.5.6-55a08812".
function software(a){if(!a)return '<span class="mut">—</span>';const m=/^(.+?)\/(\S+)(.*)$/.exec(a);return m?`${esc(m[1])} <span class="m">${esc(m[2])}</span>${esc(m[3])}`:esc(a);}
R.stations=()=>{
  const c=D.conn;if(!c)return '<div class="empty">Loading…</div>';
  const conns=connGroups(c.brew_clients);
  const bs=conns.filter(b=>b.mode==='Basestation'&&!asTerm(b.mode,b.station)),term=conns.filter(b=>asTerm(b.mode,b.station)),netl=conns.filter(b=>b.mode!=='Basestation'&&b.mode!=='Terminal');
  const f=UI.filter.cellf||'all';
  const cells=(c.cells||[]).filter(x=>f==='all'||(f==='on')===(x.online!==false));
  const dups=c.duplicate_carriers||[];
  const caps=x=>(x.capabilities||[]).map(k=>`<span class="chip">${esc(k)}</span>`).join(' ')||'<span class="mut">—</span>';
  return `<div class="bar">${seg('cellf',[['all','All'],['on','Online'],['off','Offline']])}</div>
  ${dups.length?`<div class="card"><div class="note">${ic('alert')}<div><b>Duplicate carriers:</b> ${dups.map(d=>esc(JSON.stringify(d))).join(', ')}. The newest announcement serves.</div></div></div>`:''}
  <div class="card"><div class="ch"><h2>Stations</h2><span class="cnt">${bs.length}</span>${helpBtn('conns')}</div>${helpNote('conns','Base stations connected in Basestation mode, by login. A login whose type is set to Terminal or Bridge (<i>Change</i>, or on the Accounts page) is listed under Terminals &amp; bridges instead. A station that opens several connections (one per cell or per network) is listed once, with all its cells, the radios summed and every remote address. A station without the <code>bts-info</code> extension shows no cells.')}<div class="tw"><table><thead><tr><th>Login</th><th>Type</th><th>Software</th><th>Remote address</th><th>Connected</th><th>Radios</th><th>Cells (global ID)</th></tr></thead><tbody>
  ${bs.map(b=>`<tr><td><b>${esc(b.station)}</b>${infoOf(b.station)?`<span class="sub">${esc(infoOf(b.station))}</span>`:''}${b.n>1?`<span class="sub">${b.n} connections</span>`:''}</td><td>${kindCell(b.station,b.mode)}</td><td>${[...new Set(b.agents)].map(software).join('<br>')}<span class="sub">Brew v${[...new Set(b.versions)].join(', v')}</span></td><td class="m">${[...new Set(b.addrs)].map(esc).join('<br>')||'—'}</td><td>${ago(b.connected_at_ms)}</td><td class="num">${b.registered_issis}</td><td>${b.cells.sort((x,y)=>x.name.localeCompare(y.name,undefined,{numeric:true})).map(x=>`<span class="chip" title="Carrier ${x.carrier}, ${x.mcc}/${x.mnc}">${esc(x.name)}</span>`).join(' ')||'<span class="mut">—</span>'}</td></tr>`).join('')||'<tr><td colspan="7" class="mut">No stations connected</td></tr>'}
  </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Terminals &amp; bridges</h2><span class="cnt">${term.length}</span>${helpBtn('sterms')}</div>${helpNote('sterms','Logins in Terminal mode with their own number: apps and consoles (<i>Terminal</i>) and bridges to other systems (<i>Bridge</i>), and Basestation logins whose type is set to Terminal or Bridge. <i>Change</i> sets the type (Auto: as it logs in), also on the Accounts page. They have no cells.')}<div class="tw"><table><thead><tr><th>Login</th><th>Type</th><th>Software</th><th>Remote address</th><th>Connected</th><th>Radios</th></tr></thead><tbody>
  ${term.map(b=>`<tr><td><b>${esc(b.station)}</b>${infoOf(b.station)?`<span class="sub">${esc(infoOf(b.station))}</span>`:''}${b.n>1?`<span class="sub">${b.n} connections</span>`:''}</td><td>${kindCell(b.station,b.mode)}</td><td>${[...new Set(b.agents)].map(software).join('<br>')}<span class="sub">Brew v${[...new Set(b.versions)].join(', v')}</span></td><td class="m">${[...new Set(b.addrs)].map(esc).join('<br>')||'—'}</td><td>${ago(b.connected_at_ms)}</td><td class="num">${b.registered_issis}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No terminals or bridges connected</td></tr>'}
  </tbody></table></div></div>
  ${netl.length?`<div class="card"><div class="ch"><h2>Network links</h2><span class="cnt">${netl.length}</span></div><div class="tw"><table><thead><tr><th>Login</th><th>Software</th><th>Remote address</th><th>Connected</th><th>Radios</th></tr></thead><tbody>
  ${netl.map(b=>`<tr><td><b>${esc(b.station)}</b>${infoOf(b.station)?`<span class="sub">${esc(infoOf(b.station))}</span>`:''}${b.n>1?`<span class="sub">${b.n} connections</span>`:''}</td><td>${[...new Set(b.agents)].map(software).join('<br>')}<span class="sub">Brew v${[...new Set(b.versions)].join(', v')}</span></td><td class="m">${[...new Set(b.addrs)].map(esc).join('<br>')||'—'}</td><td>${ago(b.connected_at_ms)}</td><td class="num">${b.registered_issis}</td></tr>`).join('')||'<tr><td colspan="5" class="mut">—</td></tr>'}
  </tbody></table></div></div>`:''}
  <div class="card"><div class="ch"><h2>Cells</h2><span class="cnt">${cells.length}</span>${helpBtn('cells')}</div><div class="tw"><table><thead><tr><th>Cell (global ID)</th><th>Status</th><th>Carrier</th><th>Downlink</th><th>LA</th><th>MCC / MNC</th><th>Neighbours</th><th>Radios</th></tr></thead><tbody>
  ${cells.map(x=>{const key='c'+x.name;const open=UI.open[key];const offc=x.online===false;
   const ci=infoOf(String(x.name).split('/')[0]);
   return `<tr class="click${offc?' off':''}" onclick="UI.open['${esc(key)}']=!UI.open['${esc(key)}'];render(true)" aria-expanded="${!!open}"><td><b>${esc(x.name)}</b> ${x.cell===0&&!offc?'<span class="chip i">primary</span>':''}${x.duplicate?' <span class="chip w">duplicate</span>':''}${ci?`<span class="sub">${esc(ci)}</span>`:''}</td>
   <td>${offc?'<span class="st i"><i></i>Offline</span>':'<span class="st"><i></i>Online</span>'}</td><td class="m">${x.carrier}</td><td class="num">${mhz(x.downlink_hz)}</td><td>${x.location_area??'—'}</td><td class="num">${x.mcc!=null&&x.mnc!=null?x.mcc+' / '+x.mnc:'—'}</td>
   <td>${x.neighbours?(x.neighbours.map(n=>`<span class="chip">${esc(n)}</span>`).join(' ')||'<span class="mut">none</span>'):'<span class="mut">—</span>'}</td><td class="num">${offc?'—':x.registered_issis}</td></tr>
   ${open?`<tr class="exp"><td colspan="8"><div class="kv"><div><span class="l">Station address</span>${offc?'—':`<span class="m">${esc(x.remote_addr||'—')}</span>`}</div><div><span class="l">${offc?'Gone':'Connected'}</span>${offc?(x.last_seen_ms?ago(x.last_seen_ms)+' ago':'—'):ago(x.connected_at_ms)}</div>
    <div><span class="l">Extensions</span>${caps(x)}</div><div><span class="l">Access level / MS power</span>${x.rxlev_access_min_dbm!=null?x.rxlev_access_min_dbm+' dBm':'—'} · ${x.ms_txpwr_max_dbm!=null?x.ms_txpwr_max_dbm+' dBm':'—'}</div></div></td></tr>`:''}`;}).join('')||'<tr><td colspan="8" class="mut">No cells announced</td></tr>'}
  </tbody></table></div>${helpNote('cells','Cells the stations announce (FlowStation with the <code>bts-info</code> extension). '+CELL_ID_HELP+' Click a row for details. A station that went away stays listed as offline until it connects again (stations with a login only). Apps and other terminals (Terminal mode) have no cells. A station without the <code>bts-info</code> extension is counted as a station (Overview, Stations), but its cells are not known and not listed here.')}</div>
  ${c.sip&&c.sip.enabled?'':''}`;
};
R.roaming=()=>{
  const c=D.conn;if(!c)return '<div class="empty">Loading…</div>';
  const all=c.roaming_log||[];const f=UI.filter.roamf||'all';const q=UI.filter.roam||'';
  const ann=r=>r.kind==='handover'||String(r.reselection||'').startsWith('announced');
  const list=all.filter(r=>(f==='all'||(f==='ann'&&ann(r))||(f==='unann'&&!ann(r)&&r.kind!=='within_station')||(f==='within'&&r.kind==='within_station'))&&match(q,r.issi,nameOf(r.issi)));
  const today=all.filter(r=>isToday(r.at_ms));
  const t=n=>today.filter(r=>r.reselection==='announced_type_'+n).length;
  return `<div class="bar">${seg('roamf',[['all','All'],['ann','Announced'],['unann','Unannounced'],['within','Within station']])}${searchBox('roam','Filter ISSI…')}<span class="sp"></span><span class="mut hide-s" style="font-size:12.5px">Last ${all.length} of at most 200 changes, in memory</span></div>
  <div class="tiles">
   <div class="tile"><div class="k">Today</div><div class="v">${today.length}</div><div class="s">cell changes</div></div>
   <div class="tile"><div class="k">Announced</div><div class="v">${today.filter(ann).length}</div><div class="s">type 1 · ${t(1)} / type 2 · ${t(2)} / type 3 · ${t(3)}</div></div>
   <div class="tile"><div class="k">Unannounced</div><div class="v">${today.filter(r=>!ann(r)&&r.kind!=='within_station').length}</div><div class="s">left without telling the cell</div></div>
   <div class="tile"><div class="k">Within station</div><div class="v">${today.filter(r=>r.kind==='within_station').length}</div><div class="s">multi-cell stations</div></div>
  </div>
  <div class="card"><div class="ch"><h2>Cell change log</h2><span class="cnt">${list.length}</span>${helpBtn('roam')}</div><div class="tw"><table><thead><tr><th>Time</th><th>ISSI</th><th>From</th><th></th><th>To</th><th>How</th></tr></thead><tbody>
  ${list.map(r=>`<tr><td class="m">${tmd(r.at_ms)}</td><td>${issiCell(r.issi,true)}</td><td>${place(r.from)}</td><td class="arrow">→</td><td>${place(r.to)}</td><td>${howOf(r)}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No cell changes yet</td></tr>'}
  </tbody></table></div>${helpNote('roam','The last cell changes of the radios (cell reselection, TS 100 392-2 clause 18.3.4.7). When the station the radio left reports it (FlowStation with the <code>ms-reselection</code> extension), the category is shown: <i>undeclared</i> = no call running, <i>unannounced</i> = left a call without telling the cell, <i>announced type 1/2/3</i> = told the cell first (U-PREPARE) and got D-NEW-CELL; <i>prepared here</i> = the target was prepared through this server; <i>within station</i> = another cell of a multi-cell station. A radio only registers when it enters another location area, so moves within one location area show only once the station reports the cell.')}</div>`;
};
// A station's calls from its telemetry: who talks to whom on which carrier and timeslot, and a
// grid of the four timeslots (TS1-TS4) of every carrier in use, busy or available.
function tlmCalls(s){
  const calls=Object.values(s.active_calls||{});if(!calls.length)return '';
  const cellOf={};(s.cells||[]).forEach(x=>{cellOf[x.main_carrier]=x.id;if(x.secondary_carrier)cellOf[x.secondary_carrier]=x.id;});
  const multi=(s.cells||[]).length>1;
  const used=new Map();calls.forEach(c=>{if(!used.has(c.carrier_num))used.set(c.carrier_num,new Map());used.get(c.carrier_num).set(c.ts,c);});
  const grid=[...used.keys()].sort((a,b)=>a-b).map(cn=>{const b=used.get(cn);
    return `<div class="tsg"><span class="l">Carrier ${cn}</span>${[1,2,3,4].map(ts=>{const c=b.get(ts);
      return c?`<span class="tss b" title="Carrier ${cn} TS${ts}: ${esc(String(c.source_issi))} → ${c.is_group?'group':'radio'} ${esc(String(c.gssi_or_called))}">TS${ts}</span>`:`<span class="tss" title="Carrier ${cn} TS${ts}: available">TS${ts}</span>`;}).join('')}</div>`;}).join('');
  return `<div class="tw"><table><thead><tr><th>Call</th><th>From</th><th>To</th>${multi?'<th>Cell (local / global ID)</th>':''}<th>Carrier / TS</th><th>Pri</th></tr></thead><tbody>${calls.map(c=>`<tr><td>${c.is_group?'Group':'Individual'}</td><td class="m">${esc(String(c.source_issi))}</td><td class="m">${esc(String(c.gssi_or_called))}</td>${multi?`<td>${cellOf[c.carrier_num]??'—'}${gcidOf(c.carrier_num)?` <span class="mut">(${esc(gcidOf(c.carrier_num))})</span>`:''}</td>`:''}<td class="m">${c.carrier_num} / TS${c.ts}</td><td class="num">${c.priority}</td></tr>`).join('')}</tbody></table></div>
   <div style="padding:4px 18px 14px">${grid}<div class="tsg" style="gap:12px;margin-top:8px"><span><span class="tss b" style="width:12px;height:12px"></span> busy</span><span><span class="tss" style="width:12px;height:12px"></span> available</span></div></div>`;
}
// The Fleet Control station behind a telemetry login (control login or its Brew login, as the
// server's graph sampling matches them); only for logins with Fleet Control visibility.
function tlmFleet(id){const f=D.fleet;if(!f||!can('fleet'))return null;return (f.stations||[]).find(x=>x.id===id)||(f.stations||[]).find(x=>x.brew_login===id)||null;}
// Latest values of a Fleet Control station (CPU load, temperatures, ping) as key/value cells.
function tlmLatest(fx){const f=D.fleet;const l=fx&&fx.online&&f.alarms&&f.alarms.latest&&f.alarms.latest[fx.id];if(!l)return '';const at=` title="Last report ${esc(tmd(l.at_ms))}"`;const h=[];
  if(l.cpu_pct!=null)h.push(`<div${at}><span class="l">CPU load</span>${Math.round(l.cpu_pct)} %</div>`);
  if(l.cpu_temp_c!=null)h.push(`<div${at}><span class="l">CPU temperature</span>${l.cpu_temp_c.toFixed(0)} °C</div>`);
  const sdr=Object.entries(l.sdr||{});if(sdr.length)h.push(`<div${at}><span class="l">SDR temperature${sdr.length>1?' (per cell)':''}</span>${sdr.map(([c,t])=>(sdr.length>1?c+': ':'')+(+t).toFixed(0)+' °C').join(' · ')}</div>`);
  const a=UI.aping&&UI.aping[fx.id];
  if(a&&UI.apingOn)h.push(`<div><span class="l">Ping</span><span class="chip ${a.ok?'a':'d'}" title="Auto ping ${esc(tm(a.at))}">${a.ok?'⟳ '+a.ms+' ms':'⟳ no answer'}</span></div>`);
  else if(l.ping_ms!=null)h.push(`<div${at}><span class="l">Ping</span>${l.ping_ms} ms</div>`);
  return h.join('');}
// Active condition alarms of one station, as chips in its card.
function tlmAlarms(fx){const al=fx&&D.fleet.alarms;if(!al)return '';const act=(al.active||[]).filter(a=>a.station===fx.id&&!faEvent(a));
  return act.map(a=>`<span class="chip ${a.severity==='alarm'?'d':'w'}" title="${esc(a.text)} · ${esc(tmd(a.raised_ms))}">${a.severity==='alarm'?'ALARM':'Warning'}: ${esc(FA_KIND[a.kind]||a.kind)}</span>`).join('');}
// Head of a station card: graphs link and ping for Fleet Control stations.
function tlmHeadFx(fx){if(!fx)return '';const sid='t_'+fx.id.replace(/[^a-zA-Z0-9_-]/g,'_');
  return `${D.fleet.can_manage&&fx.online?`<button class="btn" style="padding:3px 9px;font-size:12px" onclick="tlmPing('${esc(fx.id)}','${sid}')" title="Round-trip check over the control channel">Ping</button>`:''}<button class="btn" style="padding:3px 9px;font-size:12px" onclick="fleetGraph('${esc(fx.id)}')" title="CPU load, temperatures and ping over time">Graphs →</button>`;}
async function tlmPing(id,sid){setMsg(sid,'mut','Sending…');
  try{const r=await postJ('/api/fleet/ping',{id});setMsg(sid,r.ok?'ok':'err',(r.ok?'Ping OK':r.code)+' · '+r.ms+' ms'+(r.ok?'':' · '+r.message));}catch(e){setMsg(sid,'err',e.message);}}
R.telemetry=()=>{
  const t=D.tele||[];const f=can('fleet')?D.fleet:null;
  const lvl=l=>l==='ok'?'':l==='degraded'?' w':' d';
  // Fleet Control stations without a telemetry connection still have graphs and values.
  const seen=new Set(t.map(s=>{const fx=tlmFleet(s.id);return fx&&fx.id;}).filter(Boolean));
  const only=f?(f.stations||[]).filter(x=>!seen.has(x.id)):[];
  const anyOnline=f&&(f.stations||[]).some(x=>x.online);
  const aping=f&&f.can_manage?`<label style="display:flex;gap:8px;align-items:center;font-size:13px;font-weight:600" title="Ping every online Fleet Control station every 5 s while this page is open (not stored)"><button class="tog" role="switch" aria-checked="${!!UI.apingOn}" aria-label="Auto ping" ${anyOnline?'':'disabled'} onclick="fleetAutoPing(!UI.apingOn)"></button>Auto ping</label>`:'';
  const head=`<div class="card"><div class="ch"><h2>Station telemetry</h2><span class="cnt">${t.length}</span>${helpBtn('tlm')}<span class="sp"></span>${aping}</div>${helpNote('tlm','One card per station with a telemetry connection, named by its telemetry login. <i>Cell (local / global ID)</i>: the number of the cell inside the station (local, 0 = primary) and in brackets its global cell ID <i>login/cell</i> as on the other pages. '+CELL_ID_HELP+(f?' Stations in Fleet Control also show their CPU load, temperatures and last ping, their active alarms and <b>Graphs →</b> (the values over time). <b>Auto ping</b> pings every online Fleet Control station every 5 s while this page is open. Graph and alarm settings: Server settings → <i>Graphs and alarms</i>.':''))}</div>`;
  const card=(s)=>{const h=s.health||{};const fx=tlmFleet(s.id);const sid=fx?'t_'+fx.id.replace(/[^a-zA-Z0-9_-]/g,'_'):'';
  const nm=infoOf(s.id)||(s.site_name&&s.site_name!==s.id?s.site_name:'');const al=tlmAlarms(fx);
  return `<div class="card tlm"><div class="ch"><h2>${esc(s.id)}</h2>${nm?`<span class="mut" style="font-size:13px">${esc(nm)}</span>`:''}<span class="sp"></span>${tlmHeadFx(fx)}<span class="st${lvl(h.overall)}"><i></i>${esc(h.overall||'unknown')}</span></div>
   ${fx?msgDiv(sid,'padding:0 18px'):''}
   <div class="kv tight" style="padding:12px 18px"><div><span class="l">Version</span>${esc(s.build||s.version||'—')}</div><div><span class="l">Address</span><span class="m">${esc(s.ip||'—')}</span></div><div><span class="l">Connected</span>${ago(s.connected_at_ms)}</div><div><span class="l">Backhaul</span>${s.backhaul_connected==null?'—':s.backhaul_connected?'connected':'down'}</div>
    <div><span class="l">Radios</span>${s.registration_count}</div><div><span class="l">Calls</span>${Object.keys(s.active_calls||{}).length}</div><div><span class="l">TX EVM</span>${s.evm_pct!=null?s.evm_pct.toFixed(1)+' %':'—'}</div><div><span class="l">Emergencies</span>${(s.emergencies||[]).length||'none'}</div>${tlmLatest(fx)}</div>
   ${(h.domains||[]).length||al?`<div class="chips" style="padding:0 18px 12px;display:flex;gap:6px;flex-wrap:wrap">${al}${(h.domains||[]).map(d=>`<span class="chip ${d.level==='ok'?'a':d.level==='degraded'?'w':'d'}" title="${esc(d.detail)}">${esc(d.domain)}: ${esc(d.detail)}</span>`).join('')}</div>`:''}
   <div class="tw"><table><thead><tr><th title="Local cell number in this station, global cell ID in brackets">Cell (local / global ID)</th><th>Carrier</th><th>LA</th><th>RF</th><th>Radios</th></tr></thead><tbody>${(s.cells||[]).map(x=>`<tr><td>${x.id}${gcidOf(x.main_carrier)?` <span class="mut">(${esc(gcidOf(x.main_carrier))})</span>`:''}${x.primary?' <span class="chip i">primary</span>':''}</td><td class="m">${x.main_carrier}${x.secondary_carrier?' + '+x.secondary_carrier:''}</td><td>${x.location_area??'—'}</td><td><span class="st${x.rf_state==='online'?'':' w'}" title="${esc(x.rf_detail||'')}"><i></i>${esc(x.rf_state||'—')}</span></td><td class="num">${x.registered_radios}</td></tr>`).join('')||'<tr><td colspan="5" class="mut">No cell report yet</td></tr>'}</tbody></table></div>${tlmCalls(s)}</div>`;};
  // A Fleet Control station without telemetry: its values, alarms and graphs only.
  const fcard=(fx)=>{const sid='t_'+fx.id.replace(/[^a-zA-Z0-9_-]/g,'_');const al=tlmAlarms(fx);
  return `<div class="card tlm"><div class="ch"><h2>${esc(fx.label)}</h2>${fx.label!==fx.id?`<span class="mut m" style="font-size:12px">${esc(fx.id)}</span>`:''}<span class="sp"></span>${tlmHeadFx(fx)}${fx.online?'<span class="st"><i></i>Online</span>':`<span class="st d"><i></i>Offline · ${ago(fx.last_seen_ms)}</span>`}</div>
   ${msgDiv(sid,'padding:0 18px')}
   <div class="kv tight" style="padding:12px 18px"><div><span class="l">Version</span>${esc(fx.version||'—')}</div><div><span class="l">Telemetry</span><span class="mut">not connected</span></div>${tlmLatest(fx)}</div>
   ${al?`<div class="chips" style="padding:0 18px 12px;display:flex;gap:6px;flex-wrap:wrap">${al}</div>`:''}</div>`;};
  const alarms=f?fleetAlarmCard(f,false):'';
  if(!t.length&&!only.length)return alarms+'<div class="card"><div class="empty">No station telemetry connections. Stations report here with FlowStation <code>[telemetry]</code>.</div></div>';
  return `${alarms}${head}<div class="grid2" style="grid-template-columns:repeat(auto-fill,minmax(min(420px,100%),1fr))">${t.map(card).join('')}${only.map(fcard).join('')}</div>`;
};
// DGNA to many radios (DGNA & Radio actions page; formerly on Routing). mr: may send.
function dgnaCard(mr){
  const q=UI.filter.dg||'';
  const radios=D.radios.filter(r=>match(q,r.issi,nameOf(r.issi),r.station,r.cell,r.groups.join(' '),r.catalog.map(g=>g.gssi+' '+(g.mnemonic||'')).join(' '),r.groups.map(gName).join(' ')));
  for(const id of [...UI.sel])if(!D.radios.some(r=>r.issi===id&&r.radio))UI.sel.delete(id);
  const sel=UI.sel.size;
  const allSel=radios.filter(r=>r.radio).length>0&&radios.filter(r=>r.radio).every(r=>UI.sel.has(r.issi));
  const dg=r=>{if(!r.dgna)return '<span class="mut">—</span>';const d=r.dgna;const age=r.dgna_at_ms?' · '+ago(r.dgna_at_ms)+' ago':'';
    return `<span class="st${d.accepted?'':' w'}"><i></i>${d.accepted?(d.attach?'assigned ':'removed '):'rejected '}${tgTxt(d.gssi)}</span><span class="mut" style="font-size:12px">${age}</span>${d.accepted||!d.detail?'':`<div class="mut" style="font-size:12px">${esc(d.detail.replace(/^Rejected:\s*/,''))}</div>`}`;};
  const groupsOf=r=>{const cat=new Map(r.catalog.map(g=>[g.gssi,g]));const all=[...new Set([...r.groups,...r.catalog.filter(g=>g.is_attached).map(g=>g.gssi)])].sort((a,b)=>a-b);
    return all.map(g=>{const c=cat.get(g);const nm=c&&c.mnemonic?c.mnemonic:gName(g);return `<span class="tg" title="${esc(nm)}${c&&c.is_dynamic?' (DGNA)':''}">${g}${nm?' · '+esc(nm):''}${c&&c.is_dynamic?' ★':''}</span>`;}).join(' ')||'<span class="mut">—</span>';};
  const f=UI.dgf||(UI.dgf={gssi:'',name:'',assign:true,mode:0});
  return `<div class="card"><div class="ch"><h2>Assign radios to a group (DGNA)</h2>${helpBtn('dgna')}<span class="sp"></span>${searchBox('dg','ISSI, group or station…')}</div>
   ${helpNote('dgna','Dynamic group number assignment (SS-DGNA, ETSI EN 300 392-12-22): the station tells the selected radios to add or remove a group, with an optional name the radio shows. Each command goes to the station the radio is registered on, over its control connection (FlowStation <code>[command]</code>). The answer of each radio shows under <i>Last DGNA</i>. ★ = group assigned by DGNA. Apps and other terminals cannot be reached.')}
   <div class="tw"><table><thead><tr><th style="width:44px"><input type="checkbox" class="chk" aria-label="Select all" ${allSel?'checked':''} ${mr?'':'disabled'} onchange="selAll(this.checked)"></th><th>ISSI</th><th>Station</th><th>Cell (global ID)</th><th>Groups now</th><th>Last DGNA</th></tr></thead><tbody>
   ${radios.map(r=>`<tr${r.radio?'':' class="off"'}><td><input type="checkbox" class="chk" aria-label="Select ${r.issi}" ${UI.sel.has(r.issi)?'checked':''} ${mr&&r.radio?'':'disabled'} onchange="selOne(${r.issi},this.checked)"></td><td>${issiCell(r.issi,true)}</td><td><b class="m">${esc(r.station||'—')}</b></td><td>${r.cell?`<span class="chip">${esc(r.cell)}</span>`:`<span class="mut">${r.radio?'—':'— (terminal)'}</span>`}</td><td>${groupsOf(r)}</td><td>${r.radio?dg(r):'<span class="mut">not a radio, DGNA n/a</span>'}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No radios registered</td></tr>'}
   </tbody></table></div>
   ${mr?`<div class="selbar"><b>${sel} selected</b><span class="arrow">→</span>
    ${dgSaved(f)}<input class="f" id="dg-gssi" placeholder="GSSI" inputmode="numeric" style="width:110px" value="${esc(f.gssi)}" oninput="UI.dgf.gssi=this.value">
    <input class="f" id="dg-name" placeholder="Name (optional)" maxlength="15" style="width:160px" value="${esc(f.name)}" oninput="UI.dgf.name=this.value">
    <div class="seg" role="group" aria-label="Assign or remove"><button aria-pressed="${f.assign}" onclick="UI.dgf.assign=true;render(true)">Assign</button><button aria-pressed="${!f.assign}" onclick="UI.dgf.assign=false;render(true)">Remove</button></div>
    <label class="mut" style="font-size:12.5px">Attachment <select class="f" id="dg-mode" style="max-width:220px" onchange="UI.dgf.mode=+this.value">${['0 · attached permanently','1 · attach on next ITSI attach','2 · not allowed on next ITSI attach','3 · attach on next location update','4 · not attached, radio may request','5 · not attached, radio may not request'].map((t,i)=>`<option value="${i}" ${f.mode===i?'selected':''}>${t}</option>`).join('')}</select></label>
    <span class="sp"></span><button class="btn" onclick="UI.sel.clear();render(true)" ${sel?'':'disabled'}>Clear</button><button class="btn pri" onclick="sendDgna()" ${sel?'':'disabled'}>Send DGNA to ${sel} radio${sel===1?'':'s'}</button></div>
    ${msgDiv('dg-msg')}`:'<div class="empty">Sending DGNA needs the right <i>DGNA &amp; Radio actions: send</i>.</div>'}
  </div>`;
}
// Groups given by DGNA (from here or on a station), kept by the server across a restart, so they
// can be taken back. Stations report them over the control channel (`dgna-report`).
function dgnaAssignCard(mr){
  const q=UI.filter.dga||'';
  const all=D.dgnaA||[];
  const rows=all.filter(a=>match(q,a.issi,nameOf(a.issi),a.gssi,a.name||'',gName(a.gssi),a.station,a.station_name||'',a.by||''));
  const src=a=>a.source==='server'?`Server${a.by?' · '+esc(a.by):''}`:'Station';
  return `<div class="card"><div class="ch"><h2>Assigned by DGNA</h2><span class="cnt">${all.length}</span>${helpBtn('dgna-a')}<span class="sp"></span>${searchBox('dga','ISSI, group or station…')}</div>
   ${helpNote('dgna-a','Every group given to a radio by DGNA, from this page or on a station\'s own dashboard, so it can be taken back later. Stations report each assign and remove over their control connection (FlowStation <code>[command]</code>, extension <code>dgna-report</code>); the list is kept across a restart of this server. <b>Remove</b> sends the DGNA remove to the station the radio is registered on now (only while it is registered). <b>Forget</b> drops the entry here only, nothing goes to the radio (for a radio that no longer has the group).')}
   <div class="tw"><table><thead><tr><th>ISSI</th><th>Group</th><th>Station</th><th>From</th><th>Assigned</th>${mr?'<th></th>':''}</tr></thead><tbody>
   ${rows.map(a=>`<tr><td>${issiCell(a.issi,true)}</td><td><span class="tg">${a.gssi}${a.name?' · '+esc(a.name):(gName(a.gssi)?' · '+esc(gName(a.gssi)):'')}</span></td><td><b class="m">${esc(a.station_name||a.station)}</b>${a.station_name?`<span class="sub">${esc(a.station)}</span>`:''}</td><td>${src(a)}</td><td><span title="${esc(new Date(a.at_ms).toLocaleString())}">${ago(a.at_ms)} ago</span></td>${mr?`<td style="white-space:nowrap"><button class="btn" ${a.registered?'':'disabled title="The radio is not registered here now"'} onclick="dgnaTakeBack(${a.issi},${a.gssi})">Remove</button> <button class="btn danger" onclick="dgnaForget(${a.issi},${a.gssi})">Forget</button></td>`:''}</tr>`).join('')||`<tr><td colspan="${mr?6:5}" class="mut">${all.length?'Nothing matches':'No groups assigned by DGNA'}</td></tr>`}
   </tbody></table></div>${msgDiv('dga-msg')}
  </div>`;
}
async function dgnaTakeBack(issi,gssi){
  if(!confirm(`Remove group ${gssi} from radio ${issi} (DGNA)?`))return;
  setMsg('dga-msg','mut','Sending…');
  try{const res=await postJ('/api/dgna',{issis:[issi],gssi,assign:false,attachment_mode:0});const r=res[0]||{};
    setMsg('dga-msg',r.sent?'ok':'err',r.sent?`Sent to ${r.station}. The entry goes away when the station reports the remove.`:`Not sent: ${r.error||'unknown error'}`);
    await refreshSlow();
  }catch(e){setMsg('dga-msg','err',e.message);}
}
async function dgnaForget(issi,gssi){
  if(!confirm(`Forget group ${gssi} on radio ${issi} here? Nothing is sent to the radio.`))return;
  try{await postJ('/api/dgna/forget',{issi,gssi});D.dgnaA=await getJ('/api/dgna/assignments');setMsg('dga-msg','ok','Forgotten.');render(true);}
  catch(e){setMsg('dga-msg','err',e.message);}
}
// DGNA & Radio actions (formerly Control): DGNA to many radios, and the station commands for one
// radio. Seeing needs the page, sending the right "DGNA & Radio actions: send".
const mayRadio=()=>D.who.admin||(!!D.who.username&&(D.who.pages||[]).includes('radioctl_manage'));
R.radioctl=()=>{
  const mr=mayRadio();const ids=D.ctl||[];
  if(!UI.ra||!ids.includes(UI.ra))UI.ra=ids[0]||'';
  const id=UI.ra,s='ra';
  // Stations outside Fleet Control keep their Restart / Shut down here (Fleet Control has it for the others).
  const inFleet=x=>D.fleet&&(D.fleet.stations||[]).some(f=>f.id===x);
  const fm=D.who.admin||!!(D.fleet&&D.fleet.can_manage);
  const actions=!ids.length?'<div class="empty">No station control connections. Stations connect here with FlowStation <code>[command]</code>.</div>'
   :`<div style="padding:14px 18px;display:flex;flex-direction:column;gap:10px">
    <div class="bar"><label class="mut" style="font-size:12.5px">Station <select class="f" id="ra-st" aria-label="Station" onchange="UI.ra=this.value;render(true)">${ids.map(x=>`<option value="${esc(x)}" ${x===id?'selected':''}>${esc(infoOf(x)&&infoOf(x)!==x?x+' · '+infoOf(x):x)}</option>`).join('')}</select></label></div>
    ${mr?`<div class="bar"><input class="f" id="${s}_kick" placeholder="ISSI" inputmode="numeric" style="width:120px"><button class="btn" onclick="ctl(UI.ra,'${s}',{action:'KickMs',issi:+v('${s}_kick')})">Kick radio</button>
     <input class="f" id="${s}_clr" placeholder="ISSI (0 = all)" inputmode="numeric" style="width:130px"><button class="btn" onclick="ctl(UI.ra,'${s}',{action:'ClearEmergency',issi:+v('${s}_clr')})">Clear emergency</button></div>
    <div class="bar"><input class="f" id="${s}_sds" placeholder="Live SDS text" style="flex:1;min-width:160px"><input class="f" id="${s}_sdsi" placeholder="from ISSI" inputmode="numeric" style="width:110px"><input class="f" id="${s}_sdsr" placeholder="repeat" value="0" inputmode="numeric" style="width:80px">
     <button class="btn" onclick="ctl(UI.ra,'${s}',{action:'AddLiveSds',text:v('${s}_sds'),protocol_id:0,source_issi:+v('${s}_sdsi'),repeat_count:+v('${s}_sdsr')})">Add live SDS</button><button class="btn" onclick="ctl(UI.ra,'${s}',{action:'ClearLiveSds'})">Clear live SDS</button></div>
    <div class="bar"><input class="f" id="${s}_al" placeholder="ISSI" inputmode="numeric" style="width:120px"><button class="btn" onclick="ctl(UI.ra,'${s}',{action:'AmbienceListen',issi:+v('${s}_al'),enable:true})" title="The target radio keys its microphone (with indication on the radio). Needs station support.">Ambience listen</button><button class="btn" onclick="ctl(UI.ra,'${s}',{action:'AmbienceListen',issi:+v('${s}_al'),enable:false})">Stop</button></div>`
     :'<div class="mut" style="font-size:12.5px">Sending needs the right <i>DGNA &amp; Radio actions: send</i>.</div>'}
    ${fm&&!inFleet(id)?`<div class="bar"><span class="mut" style="font-size:12.5px">Not in Fleet Control:</span><button class="btn danger" onclick="if(confirm('Restart the service on '+UI.ra+'?'))ctl(UI.ra,'${s}','RestartService')">Restart service</button><button class="btn danger" onclick="if(confirm('Shut the service down on '+UI.ra+'? It does not come back by itself.'))ctl(UI.ra,'${s}','ShutdownService')">Shut down</button></div>`:''}
    ${msgDiv(s+'_res','padding:0')}</div>`;
  return dgnaCard(mr)+dgnaAssignCard(mr)+`<div class="card"><div class="ch"><h2>Radio actions</h2><span class="cnt">${ids.length}</span>${helpBtn('ract')}</div>${helpNote('ract','Commands for one radio, sent to the station you pick over its control connection (FlowStation <code>[command]</code>): <b>Kick radio</b> takes it off the station, <b>Clear emergency</b> ends its emergency (0 = every one on the station), <b>Live SDS</b> adds or clears the text the station sends out, <b>Ambience listen</b> makes the radio key its microphone (with indication on the radio; needs station support). Restart and shut down of a station: Fleet Control (here only for stations that do not use Fleet Control).')}${actions}</div>`;
};
const v=id=>($(id)||{}).value||'';

/* ---------- Fleet Control (docs/Fleet-Control.md) ---------- */
const FLEET_KIND={fleet_on:['On','a'],fleet_off:['Off','d'],version:['Software','b'],removed:['Removed','w'],update:['Update','b'],update_done:['Updated','a'],update_alarm:['Alarm','d'],update_skip:['Skipped','w'],update_end:['Update run','b'],settings:['Settings','i'],log_clear:['Cleared','i'],maintenance:['Maintenance','w'],restart:['Restart','b'],rf:['RF','w'],restart_alarm:['Alarm','d'],config:['Config','i'],config_alarm:['Alarm','d'],template:['Template','i']};
const FLEET_STAGE={download:'Download',build:'Build',install:'Installing',restart:'Restarting',up_to_date:'Up to date',failed:'Failed'};
if(!UI.fsel)UI.fsel=new Set();
const fleetJobOn=f=>!!(f&&f.job&&!f.job.finished_ms);
const fleetItem=(f,id)=>f&&f.job?(f.job.items||[]).find(i=>i.id===id):null;
// Where a station is in the update run (or its last progress), as a chip with a bar.
function fleetUpd(f,x){
  // Only while a run goes: afterwards the run card keeps the results and the column is free again.
  const live=fleetJobOn(f),it=live?fleetItem(f,x.id):null;
  const bar=(p,cls)=>`<div style="height:4px;border-radius:2px;background:var(--bg4);margin-top:4px;min-width:90px"><div style="height:4px;border-radius:2px;width:${Math.max(0,Math.min(100,p))}%;background:var(${cls||'--accent'})"></div></div>`;
  if(it){const s=it.state;
    if(s.state==='pending')return `<span class="chip i">${live?'Queued':'Not started'}</span>`;
    if(s.state==='starting')return '<span class="chip b">Starting…</span>';
    if(s.state==='waiting_call')return `<span class="chip w" title="Tried again every 10 s">Call in progress · waiting</span>`;
    if(s.state==='running')return `<span class="chip b">${esc(FLEET_STAGE[s.stage]||s.stage)} ${s.percent}%</span>${bar(s.percent)}${s.message?`<div class="mut m" style="font-size:11px;max-width:230px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="${esc(s.message)}">${esc(s.message)}</div>`:''}`;
    if(s.state==='restarting')return `<span class="chip b">Restarting · ${ago(s.since_ms)}</span>${bar(100)}`;
    if(s.state==='done')return `<span class="chip a">Updated · ${esc(s.version)}</span>`;
    if(s.state==='failed')return `<span class="chip d" style="white-space:normal" title="${esc(s.reason)}">ALARM · ${esc(s.reason)}</span>`;
    if(s.state==='skipped')return `<span class="chip w" style="white-space:normal">Skipped · ${esc(s.reason)}</span>`;
  }
  const p=x.progress;
  if(p&&Date.now()-p.at_ms<3600e3&&!['up_to_date','restart'].includes(p.stage))return `<span class="chip ${p.stage==='failed'?'d':'b'}" title="Started on the station">${esc(FLEET_STAGE[p.stage]||p.stage)} ${p.stage==='failed'?'':p.percent+'%'}</span>`;
  return '<span class="mut">—</span>';
}
// Software column: version, result of the last check, hint when the station has another OTA source.
function fleetSw(f,x){
  const c=x.check;let h=`<span class="m">${esc(x.version)}</span>`;
  if(c){
    if(!c.ok)h+=`<div><span class="chip i" title="${esc(c.message)}">Check failed · ${ago(c.at_ms)}</span></div>`;
    else if(c.update_available)h+=`<div><span class="chip a" title="Checked ${esc(tmd(c.at_ms))}">Newer: ${esc(c.latest||'available')}</span></div>`;
    else h+=`<div><span class="chip i" title="Checked ${esc(tmd(c.at_ms))}">Up to date</span></div>`;
    if(x.other_source){const r=f.update_source;h+=`<div><span class="chip w" title="This station updates from ${esc(c.repo)} (${esc(c.branch)}, channel ${esc(c.channel)}). Updates repo on this server: ${esc(r.repo)} (${esc(r.branch)}). The station keeps its own source.">⚠ other repo/branch</span></div>`;}
  }
  return h;
}
R.fleet=()=>{
  const f=D.fleet;if(!f)return loading;
  const st=f.stations||[];const log=f.log||[];const m=f.can_manage;const job=f.job;const live=fleetJobOn(f);
  for(const id of [...UI.fsel])if(!st.some(x=>x.id===id))UI.fsel.delete(id);
  const sel=[...UI.fsel];const upd=st.filter(x=>x.updatable);
  const target=sel.length?st.filter(x=>UI.fsel.has(x.id)&&x.updatable):upd;
  const liveT=sel.length?st.filter(x=>UI.fsel.has(x.id)&&x.live):st.filter(x=>x.live);
  const canUpd=target.length||liveT.length;
  const canCheck=st.some(x=>x.online&&(x.capabilities||[]).includes('update-v1'));
  const q=(UI.filter.fleet||'').toLowerCase();
  const shown=log.filter(e=>!q||(e.label+' '+e.station+' '+e.text+' '+(e.user||'')).toLowerCase().includes(q));
  const acts=m?`<button class="btn" ${canCheck&&!live?'':'disabled'} onclick="fleetCheck()" title="Every station (ticked ones only, if any) checks its own update source">Check for updates</button>
     <button class="btn pri" ${canUpd&&!live?'':'disabled'} onclick="fleetUpdateDlg(null)" title="${canUpd?'':'Check for updates first: only stations with a newer version can be updated (or that can take a live update)'}">${sel.length?'Update ('+Math.max(target.length,liveT.length)+')':'Update all'}</button>
     ${live?'<button class="btn danger" onclick="fleetCancel(null)" title="No further station starts; a station that is updating finishes">Cancel all</button>':''}
     <button class="btn" ${st.some(fleetCfgCap)&&!bulkOn(f)?'':'disabled'} onclick="fleetChangeCfg()" title="Apply a template or settings to every station (ticked ones only, if any)">${sel.length?'Change config ('+st.filter(x=>UI.fsel.has(x.id)&&fleetCfgCap(x)).length+')':'Change config…'}</button>`:'';
  const jobCard=job&&!fleetJobHidden(job,live)?(()=>{const n=k=>job.items.filter(i=>i.state.state===k).length;const o=job.options;
    return `<div class="card"><div class="ch"><h2>Update run</h2>${live?'<span class="chip b">running</span>':job.cancelled?'<span class="chip w">cancelled</span>':'<span class="chip a">finished</span>'}<span class="sp"></span><span class="mut" style="font-size:12px">${esc(tmd(job.started_ms))}${job.user?' · '+esc(job.user):''}</span></div>
     <div class="svc" style="font-size:13px"><b>${o.mode==='rolling'?'Rolling (one after another)':'Parallel (all at once)'}</b>${o.live?` · <b>Live update (RF on)</b> from <span class="m">${esc(o.live.repo)}</span> (${esc(o.live.branch)})`:''} · ${o.during_call?'update during call':'calls interrupted after '+o.interrupt_min+' min'} · ${n('done')} updated, ${n('failed')} failed, ${n('skipped')} skipped${live?', '+(job.items.length-n('done')-n('failed')-n('skipped'))+' to go':''}${job.finished_ms?' · ended '+esc(tmd(job.finished_ms)):''}</div>
     ${(()=>{const p=fleetJobPercent(job);const bad=n('failed')>0;return `<div class="svc" style="display:flex;align-items:center;gap:10px" title="Whole run: every station counts the same; the build counts up to 90 %, the restart up to 95 %"><div style="flex:1;height:8px;border-radius:4px;background:var(--bg4);overflow:hidden"><div style="height:8px;border-radius:4px;width:${p}%;background:var(${bad?'--danger':'--accent'});transition:width .6s"></div></div><b class="m" style="font-size:13px;min-width:42px;text-align:right">${p} %</b></div>`;})()}
     <div class="svc" style="display:flex;flex-wrap:wrap;gap:6px">${job.items.map((i,k)=>{const s=i.state.state;const c={done:'a',failed:'d',skipped:'w',running:'b',restarting:'b',starting:'b',waiting_call:'w'}[s]||'i';return `<span class="chip ${c}" title="${esc(i.state.reason||i.state.stage||s)}">${k+1}. ${esc(i.label)}</span>`;}).join('')}</div></div>`;})():'';
  return fleetAlarmCard(f,true)+jobCard+fleetBulkCard(f)+`<div class="card"><div class="ch"><h2>Stations</h2><span class="cnt">${st.length}</span>${helpBtn('fleet')}<span class="sp"></span>${acts}</div>
   ${helpNote('fleet',`Stations that let this server manage them: FlowStation with the switch <b>Fleet Control</b> on (Infrastructure page). They connect over the control channel (<code>[control]</code>${f.offered?'':', <b>switched off on this server</b>: no station sees Fleet Control'}). A station stays here while it is offline and leaves the list when it switches Fleet Control off, at once while it is online, else at its next login. <b>Remove</b> (offline stations only) takes a station off the list (it comes back with its next login while its switch is on). Ping, the latest values, the condition alarms and the graphs of each station are on the Telemetry page.<br><br><b>Updates</b>: <b>Check for updates</b> lets every station (only the ticked ones, if any) check its own update source; only a station that found a newer version can be updated, and it fetches the newest version again when its turn comes. <b>Update all</b> (or <b>Update (n)</b> for the ticked stations, also for a single one) asks for <i>Parallel</i> (all at once) or <i>Rolling</i> (one after another: the next starts when the previous one is back with the new software, so the network is never fully off the air). The run goes on on the server with the browser closed and after a server restart. A station is off the air while it builds (10–20 min on a Raspberry Pi). Limits: ${f.limits.build_min} min to the restart, ${f.limits.return_min} min back after the restart; a station that fails is marked ALARM and skipped. A station offline at its turn moves to the end once, then it is skipped. A call delays the start until it ends, at most the interrupt time, or not at all with <i>Update during call</i>. <b>Live update (RF on)</b> (in the update dialog, stations with <code>update-live-v1</code>): the station installs the repository and branch of the <i>Updates repo</i> (Server settings) like the install command in a terminal, stays on the air while it builds and is off only for the service restart; no check is needed, and afterwards the station follows that source. <b>Maintenance</b> puts a red banner "work on this base station in progress" on every page of the station's own dashboard for the chosen time (it runs out by itself, also when the browser is closed; set again if the station restarts in between); a live update shows its own banner. <b>Restart</b> restarts the station's service, reboots its system (about 2 min off the air) or shuts the service down; a station not back after 10 min of a restart or reboot is marked ALARM. <b>Cancel all</b> starts no further station; one that is updating finishes. ⚠ marks a station whose update source differs from the <i>Updates repo</i> (Server settings). Rights: <i>Fleet Control visibility</i> sees this page, <i>Fleet Control management</i> may also use the buttons (Server settings → Users).`)}
   ${msgDiv('fl_top','padding:0 18px')}
   ${st.length?`<div class="tw"><table><thead><tr>${m?`<th style="width:28px"><input type="checkbox" class="chk" aria-label="Tick every station" ${sel.length&&sel.length===st.length?'checked':''} onclick="fleetSelAll(this.checked)"></th>`:''}<th>Station</th><th>Status</th><th>Software</th><th>Update</th><th>Network</th><th>Features</th>${m?'<th></th>':''}</tr></thead><tbody>${st.map(x=>{const sid=x.id.replace(/[^a-zA-Z0-9_-]/g,'_');const it=fleetItem(f,x.id);const cancelable=live&&it&&['pending','waiting_call'].includes(it.state.state);return `<tr>
     ${m?`<td><input type="checkbox" class="chk" aria-label="Tick ${esc(x.label)}" ${UI.fsel.has(x.id)?'checked':''} onclick="fleetSel('${esc(x.id)}',this.checked)"></td>`:''}
     <td><b>${esc(x.label)}</b>${x.label!==x.id?` <span class="mut m" style="font-size:12px">${esc(x.id)}</span>`:''}</td>
     <td>${x.online?(x.rf_standby?'<span class="st w" title="RF kill switch / Suspend: transmitter off, control channel up"><i></i>Standby · RF off</span>':'<span class="st"><i></i>Online</span>'):`<span class="st d" title="Last seen ${esc(tmd(x.last_seen_ms))}"><i></i>Offline · ${ago(x.last_seen_ms)}</span>`}${fleetMaintChip(x)}${fleetCfgChip(x)}${x.restart?`<div><span class="chip b" title="Sent ${esc(tmd(x.restart.at_ms))}${x.restart.by?' by '+esc(x.restart.by):''}; ALARM if not back after 10 min">${x.restart.host?'Rebooting':'Restarting'} · ${ago(x.restart.at_ms)}</span></div>`:''}</td>
     <td>${fleetSw(f,x)}</td><td><div style="max-width:230px">${fleetUpd(f,x)}</div></td><td class="m">${x.mcc}/${x.mnc}</td>
     <td>${(x.capabilities||[]).map(c=>`<span class="chip">${esc(c)}</span>`).join(' ')||'<span class="mut">—</span>'}</td>
     ${m?`<td style="text-align:right"><div style="display:flex;flex-wrap:wrap;gap:6px;justify-content:flex-end;align-items:center;min-width:170px;max-width:330px;margin-left:auto">${cancelable?`<button class="btn" onclick="fleetCancel('${esc(x.id)}')" title="Take this station out of the run">Cancel</button>`:''}${fleetHas(x,'maintenance-v1')?(fleetMaintOn(x)?`<button class="btn danger" ${x.online?'':'disabled'} onclick="fleetMaintEnd('${esc(x.id)}','${esc(x.label)}')" title="Remove the red banner on the station">End maintenance</button>`:`<button class="btn" ${x.online?'':'disabled'} onclick="fleetMaintDlg('${esc(x.id)}')" title="Red banner on every page of the station's dashboard: work in progress">Maintenance</button>`):''}${fleetHas(x,'config-v1')?`<button class="btn" onclick="fleetEdit('${esc(x.id)}')" title="Read and change the station's config">${x.config&&x.config.drift?'Resolve…':'Edit'}</button>`:''}${!live&&(fleetHas(x,'restart-v1')||(D.ctl||[]).includes(x.id))?`<button class="btn" ${x.online?'':'disabled'} onclick="fleetRestartDlg('${esc(x.id)}')" title="Restart or shut down the service, or reboot the system">${ic('power')}Power options</button>`:''}${!live&&fleetHas(x,'rf-standby-v1')?(x.rf_standby?`<button class="btn pri" ${x.online?'':'disabled'} onclick="fleetRfOn('${esc(x.id)}','${esc(x.label)}')" title="Start the radio stack again (service restart)">${ic('power')}RF on</button>`:`<button class="btn kill" ${x.online?'':'disabled'} onclick="fleetRfDlg('${esc(x.id)}')" title="RF kill switch: suspend the radio stack, the transmitter goes off">${ic('power')}RF off</button>`):''}${x.online?'':`<button class="btn danger" onclick="fleetRemove('${esc(x.id)}','${esc(x.label)}')">${ic('trash')}Remove</button>`}</div><div>${msgDiv('fl_'+sid,'padding:0;font-size:12px')}</div></td>`:''}</tr>`;}).join('')}</tbody></table></div>`
   :`<div class="empty">No station uses Fleet Control yet. A FlowStation turns it on with the switch <b>Fleet Control</b> on its Infrastructure page${f.offered?'':' (this server offers it only while <code>[control]</code> is on)'}.</div>`}</div>
  ${fleetTplCard(f)}${fleetNbCard(f)}
  <div class="card"><div class="ch"><h2>Change log</h2><span class="cnt">${log.length}</span><span class="sp"></span><input class="f" placeholder="Search" aria-label="Search the change log" value="${esc(UI.filter.fleet||'')}" oninput="UI.filter.fleet=this.value;render(true);const e=document.querySelector('[aria-label=&quot;Search the change log&quot;]');if(e){e.focus();e.setSelectionRange(e.value.length,e.value.length);}" style="width:180px"><span class="mut" style="font-size:12px">kept 90 days</span>${f.is_admin?`<button class="btn danger" ${log.length?'':'disabled'} onclick="fleetClearLog()" title="Empty the change log (admins only)">${ic('trash')}Clear</button>`:''}</div>
   ${shown.length?`<div class="tw fllog"><table><thead><tr><th>Time</th><th>Station</th><th>What</th><th>By</th></tr></thead><tbody>${shown.slice(0,300).map(e=>{const k=FLEET_KIND[e.kind]||[e.kind,'i'];const al=e.kind==='update_alarm'||e.kind==='restart_alarm';return `<tr${al?' style="color:var(--danger);font-weight:600"':''}><td class="m">${esc(tmd(e.at_ms))}</td><td>${esc(e.label)}</td><td><span class="chip ${k[1]}">${esc(k[0])}</span> ${esc(e.text)}</td><td>${e.user?esc(e.user):'<span class="mut">'+(e.station?'station':'server')+'</span>'}</td></tr>`;}).join('')}</tbody></table></div>`:'<div class="empty">Nothing in the last 90 days.</div>'}</div>`;
};
// The run card goes away 30 s after a clean end (the change log keeps the story). It stays while
// the run goes on, and after a run with a failed or skipped station, or one that was cancelled.
// The 30 s count from the server's end time or from when this browser first saw the end, whichever
// is earlier, so a browser clock that is behind does not keep the card.
// Progress of the whole run in % for the bar on the run card: each station counts the same;
// finished (updated, failed, skipped) = 1, building = up to 0.9 of its percent, restarting = 0.95.
function fleetJobPercent(job){
  if(!job.items.length)return 0;
  const part=i=>{const s=i.state;return ['done','failed','skipped'].includes(s.state)?1:s.state==='restarting'?0.95:s.state==='running'?0.9*Math.max(0,Math.min(100,s.percent||0))/100:0;};
  return Math.round(100*job.items.reduce((a,i)=>a+part(i),0)/job.items.length);
}
const FLEET_JOB_HIDE_MS=30000;
function fleetJobHidden(job,live){
  if(live||!job.finished_ms||job.cancelled)return false;
  if(job.items.some(i=>['failed','skipped'].includes(i.state.state)))return false;
  const key=job.started_ms+':'+job.finished_ms;
  if(UI.jobSeen?.key!==key)UI.jobSeen={key,at:Date.now()};
  const age=Math.max(Date.now()-job.finished_ms,Date.now()-UI.jobSeen.at);
  if(age>=FLEET_JOB_HIDE_MS)return true;
  clearTimeout(UI.jobHideT);UI.jobHideT=setTimeout(()=>{if(UI.page==='fleet')render(true);},FLEET_JOB_HIDE_MS-age+50);
  return false;
}
function fleetSel(id,on){if(on)UI.fsel.add(id);else UI.fsel.delete(id);render(true);}
function fleetSelAll(on){UI.fsel.clear();if(on)(D.fleet.stations||[]).forEach(x=>UI.fsel.add(x.id));render(true);}
async function fleetReload(){try{D.fleet=await getJ('/api/fleet');}catch(e){}render(true);}
async function fleetCheck(){
  const ids=[...UI.fsel];setMsg('fl_top','mut','Checking for updates… (each station asks its own repository, up to a minute)');render(true);
  try{const r=await postJ('/api/fleet/check',{ids});const res=r.results||[];const newer=res.filter(x=>x.check.update_available).length,bad=res.filter(x=>!x.check.ok).length;
    setMsg('fl_top',bad?'err':'ok',`${res.length} checked: ${newer} with a newer version${bad?', '+bad+' failed':''}`);}catch(e){setMsg('fl_top','err',e.message);}
  fleetReload();}
// Update dialog: Parallel / Rolling, live update (RF on), update during call, interrupt time. `one`: a single station.
function fleetUpdLists(one,liveOn){
  const st=D.fleet.stations||[];const ok=x=>liveOn?x.live:x.updatable;
  const base=one?st.filter(x=>x.id===one):(UI.fsel.size?st.filter(x=>UI.fsel.has(x.id)):st.filter(ok));
  return {list:base.filter(ok),skip:one?[]:base.filter(x=>!ok(x))};
}
function fleetUpdateDlg(one,liveOn){
  const f=D.fleet;const st=f.stations||[];
  // Keep the choices when the live switch redraws the dialog.
  const prev=liveOn===undefined?null:{mode:(document.querySelector('input[name=fu-mode]:checked')||{}).value,call:$('fu-call')&&$('fu-call').getAttribute('aria-checked')==='true',int:v('fu-int')};
  if(liveOn===undefined)liveOn=!fleetUpdLists(one,false).list.length;
  const canLive=fleetUpdLists(one,true).list.length>0,canNormal=fleetUpdLists(one,false).list.length>0;
  const {list,skip}=fleetUpdLists(one,liveOn);
  const all=one?st.filter(x=>x.id===one):(UI.fsel.size?st.filter(x=>UI.fsel.has(x.id)):[]);
  if(!list.length&&!(canLive||canNormal))return;
  const src=f.update_source;const call=prev?prev.call:false;const mode=prev&&prev.mode?prev.mode:'rolling';
  const radio=(v,label,sub)=>`<label style="display:flex;gap:10px;align-items:flex-start;font-size:13px;cursor:pointer"><input type="radio" name="fu-mode" value="${v}" ${mode===v?'checked':''} style="margin-top:3px;accent-color:var(--accent)"><span><b>${label}</b><span class="mut" style="display:block;font-size:12px">${sub}</span></span></label>`;
  const n=list.length;const title=n===1?esc(list[0].label):(n||all.length||'')+' stations';
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Update ${title}</h3></div><div class="sb">
   ${n>1?`<div style="display:grid;gap:10px">${radio('rolling','Rolling (one after another)','The next station starts when the previous one is back with the new software. The network is never fully off the air.')}${radio('parallel','Parallel (all at once)',liveOn?'Every station builds at the same time (radio stays on), then they all restart.':'Every station builds at the same time: all of them are off the air for the build.')}</div>`:''}
   <label style="display:flex;gap:10px;align-items:flex-start;font-size:13px"><button class="tog" role="switch" id="fu-live" aria-checked="${liveOn}" ${canLive&&canNormal?'':'disabled'} onclick="fleetUpdateDlg(${one?`'${esc(one)}'`:'null'},${!liveOn})"></button><span><b>Live update (RF on)</b><span class="mut" style="display:block;font-size:12px">Update with the repo/branch from Server settings and keep RF on until the service restarts: <span class="m">${esc(src.repo)}</span> (${esc(src.branch)}). No check needed; afterwards the station follows this source.${canLive?'':' No station here supports it yet (needs <code>update-live-v1</code>).'}</span></span></label>
   <label style="display:flex;gap:10px;align-items:center;font-size:13px"><button class="tog" role="switch" id="fu-call" aria-checked="${call}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true');$('fu-int').disabled=this.getAttribute('aria-checked')==='true'"></button> Update during call <span class="mut" style="font-size:12px">(start at once${liveOn?', a running call is cut at the restart':', a running call is cut'})</span></label>
   <div><label class="l" for="fu-int">Interrupt a call after (minutes)</label><input class="f" id="fu-int" type="number" min="0" max="${f.limits.max_interrupt_min}" value="${esc(prev&&prev.int?prev.int:'10')}" ${call?'disabled':''} style="width:110px"><div class="mut" style="font-size:12px;margin-top:4px">While a call is in progress the start waits for it to end, at most this long, then the station updates anyway.</div></div>
   <div class="mut" style="font-size:12.5px">${liveOn?'Each station installs the source above like the install command in a terminal. It <b>stays on the air while it builds</b> and is off only for the service restart (about 2 min).':'Each station updates from its own source and fetches the newest version when its turn comes. It is <b>off the air while it builds</b> (10–20 min on a Raspberry Pi), then restarts.'} The run goes on on the server, also with the browser closed.${skip.length?`<br><br>Not updated (${liveOn?'no live update support':'no newer version found'}): ${skip.map(x=>esc(x.label)).join(', ')}.`:''}</div>
   <div class="mut" style="font-size:12px">${list.map(x=>esc(x.label)).join(' · ')||'No station for this kind of update.'}</div>
   ${msgDiv('fu-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" ${n?'':'disabled'} onclick="fleetStart(${one?`['${esc(one)}']`:'null'},${liveOn})">Start update</button></div>`;
  $('scrim').classList.add('on');
}
async function fleetStart(ids,liveOn){
  const list=ids||(UI.fsel.size?fleetUpdLists(null,liveOn).list.map(x=>x.id):[]);
  const m=document.querySelector('input[name=fu-mode]:checked');
  const body={ids:list,mode:m?m.value:'rolling',live:!!liveOn,during_call:$('fu-call').getAttribute('aria-checked')==='true',interrupt_min:Math.max(0,parseInt(v('fu-int')||'10',10)||0)};
  try{await postJ('/api/fleet/update',body);closeSheet();UI.fsel.clear();setMsg('fl_top','ok','Update run started');fleetReload();}catch(e){setMsg('fu-msg','err',e.message);}
}
async function fleetClearLog(){
  if(!confirm('Clear the whole change log? This cannot be undone. One entry will say who cleared it.'))return;
  try{await postJ('/api/fleet/log/clear',{});fleetReload();}catch(e){alert(e.message);}
}
async function fleetCancel(id){
  if(!confirm(id?'Take this station out of the update run?':'Cancel the update run? No further station starts; a station that is updating finishes.'))return;
  try{await postJ('/api/fleet/update/cancel',id?{id}:{});fleetReload();}catch(e){alert(e.message);}
}
/* ---------- Fleet Control: station config (phases 4/5, docs/Fleet-Control.md 10) ---------- */
// Own page per station (back arrow to Fleet Control). The station's settings as its own
// Infrastructure page shows them; every change is marked, applied in one go (restart, 180 s to
// confirm, else the station rolls back) or kept as pending for an offline station.
PG.fleetcfg=['fleetcfg','Fleet Control','fleet','Infrastructure'];
const FC_RESEL=[['fast_threshold_db','Fast threshold (dB)'],['slow_threshold_db','Slow threshold above fast (dB)'],['fast_hysteresis_db','Fast hysteresis (dB)'],['slow_hysteresis_db','Slow hysteresis (dB)']];
const FC_SVC=[['registration','registration'],['deregistration','deregistration'],['priority_cell','priority cell'],['no_minimum_mode','no minimum mode'],['migration','migration'],['system_wide_services','system wide'],['voice_service','voice'],['circuit_mode_data_service','circuit data'],['sndcp_service','SNDCP'],['aie_service','AIE'],['advanced_link','advanced link']];
const FC_TYPES=['0 · not supported','1 · type 1 (seamless)','2 · type 2','3 · type 3'];
const fcClone=o=>o==null?o:JSON.parse(JSON.stringify(o));
const fcSame=(a,b)=>JSON.stringify(a??null)===JSON.stringify(b??null);
function fcGet(o,p){return p.split('.').reduce((x,k)=>x==null?undefined:x[k],o);}
function fcPut(o,p,v){const ks=p.split('.');let x=o;for(const k of ks.slice(0,-1)){if(x[k]==null)x[k]=/^\d+$/.test(k)?[]:{};x=x[k];}x[ks[ks.length-1]]=v;}
// Open the page for one station.
function fleetEdit(id){
  UI.fc={id,data:null,orig:null,v:null,nb:{cell:0,edit:null,pick:false},add:false,conn:null};
  show('fleetcfg');fcLoad(true);fcSugLoad();
  getJ('/api/connections').then(c=>{if(UI.fc&&UI.fc.id===id){UI.fc.conn=c;if(UI.page==='fleetcfg')render(true);}}).catch(()=>{});
}
async function fcLoad(fresh){
  const fc=UI.fc;if(!fc)return;
  try{const d=await getJ('/api/fleet/config?id='+encodeURIComponent(fc.id));if(UI.fc!==fc)return;
    const was=fc.data&&fc.data.cfg.copy&&fc.data.cfg.copy.hash;
    fc.data=d;const copy=d.cfg.copy;
    // A new copy (first load, confirmed change, import): start editing from it.
    if(fresh||!fc.v||(copy&&copy.hash!==was&&!fcDirty())){fc.raw=null;fc.orig=copy?copy.config:null;fcSortNets(fc.orig);fc.v=fcClone(fc.orig);fc.nb.edit=null;}
    if(fresh){fcHeartbeat(true);if(d.cfg.drift)fcDriftDlg();}
  }catch(e){fc.err=e.message;}
  if(UI.page==='fleetcfg')render(true);
}
// Maintenance while the page is open: a heartbeat every minute, off when the page is left.
function fcHeartbeat(on){
  clearInterval(UI.fcBeat);
  const fc=UI.fc;if(!fc)return;
  if(!on){fetch('/api/fleet/edit',{method:'POST',keepalive:true,headers:{'content-type':'application/json','x-brew-ui':'1'},body:JSON.stringify({id:fc.id,open:false})}).catch(()=>{});return;}
  const beat=()=>postJ('/api/fleet/edit',{id:fc.id,open:true}).catch(()=>{});
  beat();UI.fcBeat=setInterval(()=>{if(UI.page==='fleetcfg'&&UI.fc===fc)beat();},60000);
}
// Leaving the page (other page, closed tab) ends the edit session.
{const show0=show;show=function(p){if(UI.page==='fleetcfg'&&p!=='fleetcfg'&&UI.fc){fcHeartbeat(false);}if(p==='fleetcfg'&&!UI.fc)p='fleet';if(p==='fleettpl'&&!UI.tp)p='fleet';if(p==='fleetgraph'&&!UI.fg)p='telemetry';return show0(p);};}
window.addEventListener('pagehide',()=>{if(UI.page==='fleetcfg'&&UI.fc)fcHeartbeat(false);});
// Follow a running change every 3 s.
setInterval(()=>{const fc=UI.fc;if(UI.page==='fleetcfg'&&fc&&fc.data&&(fc.data.cfg.apply||(!fc.data.cfg.copy&&fc.data.online))&&!$('scrim').classList.contains('on'))fcLoad(false);},3000);

// ---- the changes: request body for SetConfig + one line per setting ----
function fcNetBody(n){return {name:n.name,mcc:n.mcc,mnc:n.mnc,via:n.mode==='via'?(n.via||''):'',brew:n.mode==='core'&&n.brew?{host:n.brew.host||'',port:n.brew.port||443,tls:n.brew.tls!==false,username:n.brew.username,password:n.brew.password||''}:null};}
function fcLinkBody(l){if(!l)return {enabled:false,host:''};const b={enabled:l.enabled!==false,host:l.host||'',port:l.port||443,use_tls:!!l.use_tls,username:l.username||'',password:l.password||''};if('site_name' in l){b.site_name=l.site_name||'';b.latitude=l.latitude??null;b.longitude=l.longitude??null;}return b;}
const fcStrip=o=>{if(o==null||typeof o!=='object')return o;const c=Array.isArray(o)?o.map(fcStrip):{};if(!Array.isArray(o))for(const k in o)if(!['password','password_set','server_check','link_up','configured','extensions','fleet_control'].includes(k))c[k]=fcStrip(o[k]);return c;};
const fcShow=v=>v==null||v===''?'—':typeof v==='boolean'?(v?'on':'off'):typeof v==='object'?JSON.stringify(v):String(v);
function fcDiff(o,v){
  const body={},lines=[];if(!o||!v)return {body,lines};
  const line=(what,a,b)=>lines.push(`${what}: ${fcShow(a)} → ${fcShow(b)}`);
  // Cells (radio, LA, network, neighbours), added and removed cells.
  const cells=[];
  for(const c of v.cells){const oc=o.cells.find(x=>x.id===c.id);if(!oc)continue;
    if((v.cells_remove||[]).includes(c.id))continue;
    const e={id:c.id};const name=`Cell ${c.id}`;
    if(c.main_carrier!==oc.main_carrier){e.main_carrier=c.main_carrier;line(name+' carrier',oc.main_carrier,c.main_carrier);}
    if(c.colour_code!==oc.colour_code){e.colour_code=c.colour_code;line(name+' colour code',oc.colour_code,c.colour_code);}
    const la=c.primary?c.location_area:(c.location_area_own?c.location_area:null),ola=oc.primary?oc.location_area:(oc.location_area_own?oc.location_area:null);
    if(la!==ola){e.location_area=la;line(name+' LA',ola??'as cell 0',la??'as cell 0');}
    if(v.networks_explicit&&c.network!==oc.network&&!(oc.network==='default'&&c.network==='home'&&!o.networks_explicit)){e.network=c.network;line(name+' network',oc.network==='default'&&!o.networks_explicit?'home':oc.network,c.network);}
    const nb=c.own_list||c.primary?c.neighbours:null,onb=oc.own_list||oc.primary?oc.neighbours:null;
    if(!fcSame(nb,onb)){e.neighbours=nb;line(name+' neighbours',(onb||[]).length+' entries',nb==null?'station cells only':nb.length+' entries');}
    if(Object.keys(e).length>1)cells.push(e);}
  if(cells.length)body.cells=cells;
  if((v.cells_remove||[]).length){body.cells_remove=v.cells_remove;v.cells_remove.forEach(id=>lines.push(`Cell ${id}: removed`));}
  if((v.cells_new||[]).length){body.cells_add=v.cells_new.map(c=>({device:c.device,main_carrier:c.main_carrier,colour_code:c.colour_code??null,location_area:c.location_area??null}));v.cells_new.forEach(c=>lines.push(`New cell: carrier ${c.main_carrier} on ${c.device}`));}
  // Reselection and presence.
  if(!fcSame(v.reselect,o.reselect)){body.reselect=v.reselect;FC_RESEL.forEach(([k,l])=>{if(v.reselect[k]!==o.reselect[k])line(l,o.reselect[k],v.reselect[k]);});}
  if(v.rxlev_access_min_dbm!==o.rxlev_access_min_dbm){body.rxlev_access_min_dbm=v.rxlev_access_min_dbm;line('Min. access level (dBm)',o.rxlev_access_min_dbm??'default -110',v.rxlev_access_min_dbm??'default -110');}
  if(v.migration!==o.migration){body.migration=v.migration;line('Migration',o.migration,v.migration);}
  if(v.presence_check_secs!==o.presence_check_secs){body.presence_check_secs=v.presence_check_secs;line('Presence check (s)',o.presence_check_secs,v.presence_check_secs);}
  // Multi Network.
  if(v.multi_network_enabled!==o.multi_network_enabled){body.multi_network_enabled=v.multi_network_enabled;line('Multi Network',o.multi_network_enabled,v.multi_network_enabled);}
  if(v.multi_network_enabled){const nets=[];
    for(const n of v.networks){if(n.converted)continue;const on=o.networks.find(x=>x.name===n.name);
      if(n.removed){if(on){nets.push({name:n.name,remove:true});lines.push(`Network ${n.name}: removed`);}continue;}
      if(!on||!fcSame(fcStrip(n),fcStrip(on))||(n.brew&&n.brew.password)){nets.push(fcNetBody(n));
        if(!on){lines.push(`Network ${n.name}: added (${n.mcc}/${n.mnc})`);continue;}
        const nb=n.brew||{},ob=on.brew||{};let any=false;
        [['mcc',n.mcc,on.mcc],['mnc',n.mnc,on.mnc],['Brew host',nb.host,ob.host],['Brew port',nb.port,ob.port],['Brew login',nb.username,ob.username],['Brew TLS',nb.tls,ob.tls]].forEach(([k,a,b])=>{if(!fcSame(a,b)){line(`Network ${n.name} ${k}`,b,a);any=true;}});
        if(nb.password){lines.push(`Network ${n.name} Brew password: changed`);any=true;}
        if(!any)lines.push(`Network ${n.name}: changed`);}}
    if(nets.length)body.networks=nets;}
  // Home Mode Display and ISSI whitelist: single values of the file (`raw`).
  const raw=[];
  if(v.access){const oh=o.home_mode_display,vh=v.home_mode_display,P=['cell_info','home_mode_display'];
    const clean=h=>Object.fromEntries(Object.entries(h).filter(([,x])=>x!=null&&x!==''));
    if(!fcSame(oh,vh)){
      if(vh==null){raw.push({path:P,remove:true});lines.push('Home mode display: off');}
      else if(oh==null){raw.push({path:P,value:clean(vh)});lines.push(`Home mode display: on ("${vh.text||''}")`);}
      else for(const k of [...new Set(Object.keys(oh).concat(Object.keys(vh)))]){if(fcSame(oh[k],vh[k]))continue;
        raw.push(vh[k]==null||vh[k]===''?{path:P.concat(k),remove:true}:{path:P.concat(k),value:vh[k]});line('Home mode display '+k.replace(/_/g,' '),oh[k],vh[k]);}}
    const oa=o.access,va=v.access;
    if(!fcSame(oa.issi_whitelist,va.issi_whitelist)){raw.push({path:['security','issi_whitelist'],value:va.issi_whitelist});line('ISSI whitelist',oa.issi_whitelist.join(', ')||'empty',va.issi_whitelist.join(', ')||'empty');}
    if(oa.whitelist_mode!==va.whitelist_mode){raw.push({path:['security','whitelist_mode'],value:va.whitelist_mode});line('Whitelist mode',oa.whitelist_mode,va.whitelist_mode);}}
  if(raw.length)body.raw=raw;
  // Links.
  if(!v.networks_explicit&&v.brew&&v.brew.link){const b=v.brew.link,ob=o.brew.link||{};
    if(!fcSame(fcStrip(b),fcStrip(ob))||b.password){body.brew={host:b.host||'',port:b.port||443,tls:b.tls!==false,username:b.username,password:b.password||''};
      ['host','port','username','tls'].forEach(k=>{if(b[k]!==ob[k])line('Brew '+k,ob[k],b[k]);});if(b.password)lines.push('Brew password: changed');}}
  if(!v.networks_explicit&&v.net_info&&!fcSame(v.net_info,o.net_info)){body.net_info=v.net_info;line('MCC / MNC',`${o.net_info.mcc}/${o.net_info.mnc}`,`${v.net_info.mcc}/${v.net_info.mnc}`);}
  for(const [k,l] of [['telemetry','Telemetry'],['command','Control channel']]){
    const a=v[k],b=o[k];
    if(!fcSame(fcStrip(a),fcStrip(b))||(a&&a.password)){body[k]=fcLinkBody(a);
      if(!a)lines.push(l+': removed');else if(!b)lines.push(l+': set up ('+(a.host||'')+')');
      else{for(const f of ['enabled','host','port','use_tls','username','site_name','latitude','longitude'])if(a[f]!==b[f]&&(f in a))line(l+' '+f.replace('_',' '),b[f],a[f]);if(a.password)lines.push(l+' password: changed');}}}
  // Update source.
  if(v.ota&&(v.ota.channel!==o.ota.channel||(v.ota.channel==='custom'&&(v.ota.repo!==o.ota.repo||v.ota.branch!==o.ota.branch)))){
    body.ota={channel:v.ota.channel,repo:v.ota.repo||null,branch:v.ota.branch||null};
    line('Update source',o.ota.channel==='custom'?`${o.ota.repo} (${o.ota.branch})`:o.ota.channel,v.ota.channel==='custom'?`${v.ota.repo} (${v.ota.branch})`:v.ota.channel);}
  return {body,lines};
}
// The config editor (bottom of the page) changed: only what differs goes to the station (the server compares).
function fcRawChanged(){const fc=UI.fc;return !!(fc&&fc.raw!=null&&fc.data&&fc.data.cfg.copy&&fc.raw!==fc.data.cfg.copy.text);}
function fcN(){const fc=UI.fc;return fcDiff(fc.orig,fc.v).lines.length+(fcRawChanged()?1:0);}
function fcDirty(){const fc=UI.fc;return !!(fc&&fc.v&&fc.data&&(Object.keys(fcDiff(fc.orig,fc.v).body).length||fcRawChanged()));}
function fcCount(){const fc=UI.fc;if(!fc||!fc.v)return;const n=fcN();const e=$('fc-count');if(e)e.textContent=n?n+(n===1?' change':' changes'):'no changes';
  ['fc-apply','fc-discard'].forEach(id=>{const b=$(id);if(b)b.disabled=!n;});}
// An input changed: store it, mark it, count again (no redraw, so typing goes on).
function fcIn(el){
  const fc=UI.fc;const p=el.dataset.p,t=el.dataset.t||'str';let val;
  if(t==='bool')val=el.getAttribute('aria-checked')!=='true';
  else if(t==='int'||t==='intn'){const s=el.value.trim();val=s===''?(t==='intn'?null:undefined):parseInt(s,10);if(val!==null&&val!==undefined&&!isFinite(val))val=undefined;}
  else if(t==='num'){const s=el.value.trim();val=s===''?null:parseFloat(s);}
  else val=el.value;
  if(val===undefined&&t==='int')return;
  if(t==='bool')el.setAttribute('aria-checked',val);
  fcPut(fc.v,p,val);
  if(p.endsWith('.location_area')&&/^cells\.\d+\./.test(p)){const c=fcGet(fc.v,p.replace(/\.location_area$/,''));if(c&&!c.primary)c.location_area_own=val!=null;}
  const o=fcGet(fc.orig,p);el.classList.toggle('chg',!fcSame(o,val)&&!(t==='str'&&p.endsWith('password')&&!val));
  if(el.dataset.redraw)render(true);else fcCount();
}
// Field helpers: value from the working copy, marked when it differs from the station's.
function fcF(p,t,extra,w){const fc=UI.fc;const v=fcGet(fc.v,p),o=fcGet(fc.orig,p);const chg=!fcSame(v,o)&&!(p.endsWith('password')&&!v);
  return `<input class="f${chg?' chg':''}" data-p="${p}" data-t="${t||'str'}" value="${esc(v??'')}" oninput="fcIn(this)" style="width:${w||'100%'}" ${extra||''}>`;}
function fcSel(p,opts,t,extra,w){const fc=UI.fc;const v=fcGet(fc.v,p),o=fcGet(fc.orig,p);
  return `<select class="f${fcSame(v,o)?'':' chg'}" data-p="${p}" data-t="${t||'str'}" onchange="fcIn(this)" style="width:${w||'100%'}" ${extra||''}>${opts.map(([k,l])=>`<option value="${esc(k??'')}" ${String(k??'')===String(v??'')?'selected':''}>${esc(l)}</option>`).join('')}</select>`;}
function fcTog(p,extra){const fc=UI.fc;const v=!!fcGet(fc.v,p),o=!!fcGet(fc.orig,p);return `<button class="tog${v===o?'':' chg'}" role="switch" aria-checked="${v}" data-p="${p}" data-t="bool" onclick="fcIn(this)" ${extra||''}></button>`;}
const fcFld=(l,ctl)=>`<div><label class="l">${l}</label>${ctl}</div>`;
const fcGrid=(...f)=>`<div class="fcg">${f.join('')}</div>`;
const fcHelp=t=>`<div class="mut" style="font-size:12.5px;padding:10px 18px 0">${t}</div>`;
const fcCard=(title,acts,body,id)=>`<div class="card"${id?` id="${id}"`:''}><div class="ch"><h2>${title}</h2>${acts||''}</div>${body}</div>`;
const fcDb=(a,b,s)=>{const o=[];for(let x=a;x<=b;x+=s)o.push([x,String(x)]);return o;};

R.fleetcfg=()=>{
  const fc=UI.fc;if(!fc)return loading;
  const d=fc.data;if(!d)return fc.err?`<div class="card"><div class="empty">${esc(fc.err)} <button class="btn" onclick="show('fleet')">Back</button></div></div>`:loading;
  const c=d.cfg,v=fc.v,o=fc.orig;
  const back=`<button class="btn ic" title="Back to Fleet Control" aria-label="Back to Fleet Control" onclick="show('fleet')">‹</button>`;
  const maint=d.maintenance&&d.maintenance.until_ms>Date.now()?`<span class="chip w" title="${d.edit&&d.edit.auto_maintenance?'Set while this page is open, refreshed every minute, off when you leave':'Set by hand'}">Maintenance · ${esc(d.maintenance.by||'')}${d.edit&&d.edit.auto_maintenance?' (editing)':''}</span>`:'';
  const sync=!c.copy?'<span class="chip i">no copy yet</span>':c.drift?'<span class="chip d">differs from the station</span>':`<span class="chip i" title="Config hash; read ${esc(tmd(c.copy.at_ms))}">Config ${esc(c.copy.hash.slice(0,6))} · ${c.reported_hash===c.copy.hash?'in sync':d.online?'checking':'as last seen'}</span>`;
  const busy=!!c.apply;
  const locked=busy||!!c.drift||!d.capable||!v;
  const n=v?fcN():0;
  const head=`<div class="card"><div class="ch" style="gap:10px">${back}<h2 style="font-size:17px">${esc(d.label)} <span class="mut m" style="font-size:12px;font-weight:500">${esc(d.id)}</span></h2>
    ${d.online?'<span class="st"><i></i>Online</span>':`<span class="st d"><i></i>Offline · ${ago(d.last_seen_ms)}</span>`}${maint}${sync}
    <span class="sp"></span>${locked?'':`<span class="mut" id="fc-count" style="font-size:12px">${n?n+(n===1?' change':' changes'):'no changes'}</span>
    ${d.template_sections&&D.fleet&&D.fleet.can_manage?`<button class="btn" onclick="tplOpen(null,UI.fc.data.template_sections,UI.fc.data.label)" title="A template with this station's shared settings (no carrier, LA, cells, neighbours, logins)">Save as template</button>`:''}
    <button class="btn" id="fc-discard" ${n?'':'disabled'} onclick="fcReset()">Discard</button>
    <button class="btn pri" id="fc-apply" ${n?'':'disabled'} onclick="fcApplyDlg()">${d.online?'Apply to station':'Save as pending'}</button>`}</div>
    <div class="note">${ic('info')}<div>${!d.capable?'This station cannot read or change its config from here: it needs FlowStation with Fleet Control config (<code>config-v1</code>).':d.online
      ?`While this page is open the station shows the red maintenance banner (refreshed every minute, off when you leave; it runs out by itself after ${3} min without this page). <b>Apply to station</b> checks the whole config on the station, saves the old one, writes the new one and restarts the service (about 20 s off the air). The change stays when the station is back on this server within ${d.confirm_secs} s (and on Brew, if it has a Brew login); otherwise the station restores the old config and restarts again.`
      :`The station is offline (shown: the server copy from ${c.copy?esc(tmd(c.copy.at_ms)):'—'}). <b>Save as pending</b> keeps the change on the server; at the next login of the station it is applied (after a running call has ended), the station restarts and has ${d.confirm_secs} s to come back, else it restores its old config. If the config on the station changed in the meantime, nothing is applied and the change is shown as a conflict.`}${d.read_error?`<div style="color:var(--danger);margin-top:6px">Could not read the station now: ${esc(d.read_error)}</div>`:''}</div></div>${msgDiv('fc_top','padding:0 18px 12px')}</div>`;
  let cards='';
  if(c.apply){const a=c.apply;const back2=a.back_ms;const left=back2?Math.max(0,d.confirm_secs-Math.floor((Date.now()-back2)/1000)):null;
    cards+=fcCard('Config change running',`<span class="chip b">${a.rollback_by!=null?'undoing':back2?'back · confirming':'station restarting'}</span><span class="sp"></span><span class="mut" style="font-size:12px">${esc(tmd(a.at_ms))}${a.by?' · '+esc(a.by):''}</span>`,
      `<div class="svc" style="font-size:13px">${back2?`The station is back with the new config; the server confirms it as soon as ${d.brew_login?'its Brew link is up':'it answers'}${left!=null?` (it rolls back by itself in ${left} s otherwise)`:''}.`:'The station restarts with the new config (about 20 s). If it does not come back in time it restores the old one by itself.'}</div>
      <div class="svc" style="display:block;font-size:12.5px">${(a.summary||[]).map(l=>`<div>${esc(l)}</div>`).join('')||'<span class="mut">no details</span>'}</div>
      ${a.rollback_by==null&&d.online?`<div style="padding:12px 18px"><button class="btn danger" onclick="fcRollback()">Undo (old config back)</button></div>`:''}`);}
  if(c.pending){const p=c.pending;
    cards+=fcCard('Pending change',`${p.conflict?'<span class="chip d">conflict</span>':'<span class="chip w">waiting for the station</span>'}<span class="sp"></span><span class="mut" style="font-size:12px">saved ${esc(tmd(p.at_ms))}${p.by?' · '+esc(p.by):''}</span>`,
      `<div class="svc" style="display:block;font-size:12.5px">${(p.summary||[]).map(l=>`<div>${esc(l)}</div>`).join('')}</div>${p.message?`<div class="svc" style="font-size:12.5px;color:${p.conflict?'var(--danger)':'var(--text2)'}">${esc(p.message)}</div>`:''}
      <div style="display:flex;gap:8px;padding:12px 18px;flex-wrap:wrap"><button class="btn danger" onclick="fcDiscardPending()">Discard pending change</button>${p.conflict?'<span class="mut" style="font-size:12px;align-self:center">Discard it and make the change again on the station\'s current config.</span>':''}</div>`);}
  if(c.drift)cards+=fcCard('Config changed on the station','<span class="chip d">differs</span>',`<div class="svc" style="font-size:13px">The config on the station is not the server copy. Nothing can be changed from here until you choose.</div><div style="padding:12px 18px"><button class="btn pri" onclick="fcDriftDlg()">Resolve…</button></div>`);
  if(!v)return head+cards+`<div class="card"><div class="empty">${d.online&&d.capable?'Reading the config from the station…':'No copy of this station\'s config on the server yet: it is read at the station\'s next login.'}</div></div>`;
  const dis=locked?'disabled':'';
  const fieldset=html=>`<fieldset class="fcfs" ${dis}>${html}</fieldset>`;
  const net=!v.networks_explicit;
  // Brew link (classic config) and network.
  let brew='';
  if(net&&v.brew&&v.brew.link)brew=fcCard('Brew link',`${!d.online||c.apply?'':v.brew.link_up?'<span class="chip a">connected</span>':'<span class="chip i">not connected</span>'}`,`<div style="padding:12px 18px">${fcGrid(
      fcFld('Brew server (host)',fcF('brew.link.host','str','autocomplete="off" spellcheck="false"')),fcFld('Port',fcF('brew.link.port','int','inputmode="numeric"')),
      fcFld('Login (ISSI)',fcF('brew.link.username','int','inputmode="numeric"')),fcFld('Password',fcF('brew.link.password','str',`type="password" autocomplete="new-password" placeholder="${v.brew.link.password_set?'unchanged':'not set'}"`)),
      fcFld('TLS',`<div style="padding-top:3px">${fcTog('brew.link.tls')}</div>`),fcFld('MCC',fcF('net_info.mcc','int','inputmode="numeric"')),fcFld('MNC',fcF('net_info.mnc','int','inputmode="numeric"')))}</div>`);
  // Cells.
  const netOpts=v.networks.filter(x=>!x.removed).map(x=>[x.name,`${x.name} (${x.mcc}/${x.mnc})`]);
  const prim=v.cells.find(x=>x.primary)||{};
  const rmv=v.cells_remove||[];
  const cellRows=v.cells.map((cl,i)=>{const gone=rmv.includes(cl.id);
    return `<tr${gone?' style="opacity:.45;text-decoration:line-through"':''}><td style="white-space:nowrap"><b>Cell ${cl.id}</b>${cl.primary?' ★':''}</td><td class="m">${esc(cl.global_id||'—')}</td><td class="m mut" style="font-size:12px;max-width:180px;overflow:hidden;text-overflow:ellipsis" title="${esc(cl.device||'')}">${esc(cl.device||'—')}</td>
     <td>${fcF(`cells.${i}.main_carrier`,'int',`inputmode="numeric" ${cl.carrier_fixed?`disabled title="${esc(cl.carrier_fixed)}"`:''}`,'84px')}</td><td>${fcF(`cells.${i}.colour_code`,'int','inputmode="numeric"','64px')}</td>
     <td>${cl.primary?fcF(`cells.${i}.location_area`,'int','inputmode="numeric"','78px'):fcF(`cells.${i}.location_area`,'intn',`inputmode="numeric" placeholder="${esc(prim.location_area)}"`,'78px')}</td>
     <td>${v.networks_explicit?fcSel(`cells.${i}.network`,netOpts,'str','data-redraw="1"','170px'):`<span class="m">${v.mcc}/${v.mnc}</span>`}</td>
     <td style="text-align:right">${cl.primary?'':gone?`<button class="btn" onclick="fcCellRemove(${cl.id},false)">Keep</button>`:`<button class="btn danger" onclick="fcCellRemove(${cl.id},true)">Remove</button>`}</td></tr>`;}).join('')
    +(v.cells_new||[]).map((cl,i)=>`<tr><td><b>New</b> <span class="chip w">added</span></td><td class="mut">—</td><td class="m" style="font-size:12px">${esc(cl.device)}</td><td class="m">${cl.main_carrier}</td><td class="m">${cl.colour_code??'—'}</td><td class="m">${cl.location_area??'—'}</td><td></td><td style="text-align:right"><button class="btn" onclick="fcCellNewDrop(${i})">Drop</button></td></tr>`).join('');
  const cells=fcCard('Cells and location areas',`<span class="cnt">${v.cells.length}</span><span class="sp"></span><button class="btn" ${dis} onclick="UI.fc.add=!UI.fc.add;render(true)">${ic('plus')}Add cell</button>`,
    fcHelp('Same as on the station\'s Infrastructure page. A new carrier moves the cell\'s SDR frequencies with it. Empty LA on an extra cell = the LA of cell 0. Same LA on all cells: radios change cell without registering; own LA per cell: radios register on every change (TS 100 392-2 clause 18.3.4.7.1a).')+
    `<div class="tw"><table><thead><tr><th>Cell</th><th>Global cell id</th><th>SDR</th><th>Carrier</th><th>CC</th><th>LA</th><th>Network</th><th></th></tr></thead><tbody>${cellRows}</tbody></table></div>
    ${fc.add?`<div style="display:flex;gap:8px;flex-wrap:wrap;align-items:flex-end;padding:12px 18px;border-top:1px solid var(--sep)">${fcFld('SDR device',`<input class="f" id="fc-nc-dev" placeholder="driver=plutosdr,uri=ip:192.168.3.1" style="width:260px">`)}${fcFld('Carrier',`<input class="f" id="fc-nc-car" inputmode="numeric" style="width:90px">`)}${fcFld('CC',`<input class="f" id="fc-nc-cc" inputmode="numeric" style="width:70px" placeholder="${esc(prim.colour_code)}">`)}${fcFld('LA',`<input class="f" id="fc-nc-la" inputmode="numeric" style="width:80px" placeholder="${esc(prim.location_area)}">`)}<button class="btn pri" onclick="fcCellAdd()">Add</button></div>`:''}`);
  // Multi Network.
  const mnOn=!!v.multi_network_enabled;
  const nets=fcCard('Networks (Multi Network)',`<span class="sp"></span><span class="mut" style="font-size:12.5px">${mnOn?'On':'Off'}</span>${fcTog('multi_network_enabled','data-redraw="1"')}`,
    fcHelp('A cell broadcasts exactly one network (MCC / MNC, TS 100 392-2 table 18.17); pick it per cell above. Own server: the network\'s own Brew link; via: through another network\'s Brew server; local: inside this station.'+(mnOn?'':' Switching Multi Network off turns the networks back into the classic config.'))+
    (v.networks_explicit||mnOn?`<div class="tw"><table><thead><tr><th>Name</th><th>MCC</th><th>MNC</th><th>Reached</th><th>Brew server</th><th></th></tr></thead><tbody>${v.networks.map((x,i)=>`<tr${x.removed?' style="opacity:.45;text-decoration:line-through"':''}><td><b>${esc(x.name)}</b>${o.networks.some(y=>y.name===x.name)||x.converted?'':' <span class="chip w">new</span>'}${!x.removed&&fcUnserved().includes(x.name)?(fcUnusedOk()?' <span class="chip" title="No cell broadcasts this network: the station keeps it in its config but inactive (no Brew link) until a cell selects it">unused</span>':' <span class="chip d" title="Every network needs a cell: bind one above or remove the network. Stations with networks-unused-v1 keep an unused network instead">no cell</span>'):''}</td>${v.networks_explicit?`<td>${fcF(`networks.${i}.mcc`,'int','inputmode="numeric"','74px')}</td><td>${fcF(`networks.${i}.mnc`,'int','inputmode="numeric"','86px')}</td>`:`<td class="m">${x.mcc}</td><td class="m">${x.mnc}</td>`}
      <td>${x.mode==='core'?'<span class="chip a">own server</span>':x.mode==='via'?`<span class="chip b">via ${esc(x.via)}</span>`:'<span class="chip i">local</span>'}</td><td class="m">${x.mode==='core'&&x.brew?esc((x.brew.host||'')+':'+(x.brew.port||443)):'—'}</td>
      <td style="text-align:right">${x.name===fcPrimaryNet()||x.converted?'':x.removed?`<button class="btn" onclick="fcNetRemove(${i},false)">Keep</button>`:`<button class="btn danger" onclick="fcNetRemove(${i},true)">Remove</button>`}</td></tr>${fcNetBrewRow(x,i)}`).join('')}</tbody></table></div>`:'')+
    (mnOn?`<div style="display:flex;gap:8px;flex-wrap:wrap;align-items:flex-end;padding:12px 18px">${fcFld('Name',`<input class="f" id="fc-nn-name" style="width:120px">`)}${fcFld('MCC',`<input class="f" id="fc-nn-mcc" inputmode="numeric" style="width:80px">`)}${fcFld('MNC',`<input class="f" id="fc-nn-mnc" inputmode="numeric" style="width:90px">`)}
      ${fcFld('Reached',`<select class="f" id="fc-nn-via" style="width:150px" onchange="$('fc-nn-own').style.display=this.value==='@core'?'flex':'none'">${v.networks.filter(x=>!x.removed&&x.mode==='core').map(x=>`<option value="${esc(x.name)}">via ${esc(x.name)}</option>`).join('')}<option value="@core">own server</option><option value="@local">local</option></select>`)}
      <span id="fc-nn-own" style="display:none;gap:8px;flex-wrap:wrap;align-items:flex-end">${fcFld('Brew server',`<input class="f" id="fc-nn-host" style="width:170px">`)}${fcFld('Port',`<input class="f" id="fc-nn-port" inputmode="numeric" value="443" style="width:80px">`)}${fcFld('Login',`<input class="f" id="fc-nn-user" inputmode="numeric" style="width:110px">`)}${fcFld('Password',`<input class="f" id="fc-nn-pass" type="password" autocomplete="new-password" style="width:120px">`)}</span>
      <button class="btn" onclick="fcNetAdd()">${ic('plus')}Add network</button></div>${fcServerNets()}`:''));
  // Reselection.
  const rx=[[null,'default (-110)']].concat(fcDb(-125,-50,5));
  const resel=`<details class="card fcdet"><summary class="ch"><h2>Cell reselection (advanced, all cells)</h2><span class="sp"></span><span class="mut" style="font-size:12px">Show / hide</span></summary><div style="padding:12px 18px">${fcGrid(
    ...FC_RESEL.map(([k,l])=>fcFld(l,fcSel('reselect.'+k,fcDb(0,30,2),'int'))),
    fcFld('Min. access level (dBm)',fcSel('rxlev_access_min_dbm',rx,'intn')),fcFld('Presence check after silence (s)',fcF('presence_check_secs','int','inputmode="numeric"')),
    fcFld('Migration supported',`<div style="padding-top:3px">${fcTog('migration',mnOn?'disabled title="On while Multi Network is on"':'')}</div>`))}</div></details>`;
  // Neighbours.
  const ci=Math.min(fc.nb.cell,v.cells.length-1);const cell=v.cells[ci];
  const own=cell.primary||cell.own_list;const list=own?cell.neighbours||[]:[];
  const scells=(fc.conn&&fc.conn.cells||[]).filter(x=>x.station!==d.brew_login);
  const onServer=nb=>scells.find(x=>x.carrier===nb.main_carrier_number&&(nb.main_carrier_number_extension==null||x.carrier_ext==null||x.carrier_ext===nb.main_carrier_number_extension)&&(nb.location_area==null||x.location_area===nb.location_area)&&(nb.mcc==null||x.mcc===nb.mcc));
  // MCC/MNC a cell broadcasts: its network (Multi Network), else the station's.
  const cellMm=cl=>{const n=v.networks_explicit&&cl?v.networks.find(x=>x.name===cl.network):null;return n?[n.mcc,n.mnc]:[v.mcc,v.mnc];};
  const [cmcc,cmnc]=cellMm(cell);
  const svcTxt=s=>FC_SVC.filter(([k])=>s&&s[k]).map(([,l])=>l).join(', ')||'—';
  const nbRows=list.map((nb,i)=>{const sv=onServer(nb);return `<tr><td class="m">${nb.cell_identifier_ca}</td><td class="m">${nb.main_carrier_number}${nb.main_carrier_number_extension!=null?' / '+nb.main_carrier_number_extension:''}</td><td class="m">${nb.location_area??'—'}</td><td class="m">${nb.mcc??cmcc}/${nb.mnc??cmnc}</td>
    <td>${(()=>{const mk=fcSugMark(cell,nb);return mk?(mk.kind==='la'?`<span class="chip w" style="white-space:normal">${esc(mk.text)}</span> <button class="btn" ${dis} onclick="fcNbFixLa(${i},${mk.location_area})">Use LA ${mk.location_area}</button>`:`<span class="chip d" style="white-space:normal" title="${esc(mk.text)}">outdated · not in the cell directory</span>`):sv?`<span class="chip a">${esc(sv.name)}</span>`:'<span class="chip w">not on the server</span>';})()}</td><td>${esc(FC_TYPES[nb.cell_reselection_types_supported]||'—')}</td><td class="mut" style="font-size:12px;max-width:220px">${esc(svcTxt(nb.bs_service_details))}</td>
    <td style="text-align:right;white-space:nowrap"><button class="btn" ${dis} onclick="fcNbEdit(${i})">Edit</button> <button class="btn danger" ${dis} onclick="fcNbDel(${i})">Remove</button></td></tr>`;}).join('')
    +(cell.siblings||[]).map(s=>`<tr class="mut"><td class="m">${s.cell_identifier_ca}</td><td class="m">${s.main_carrier_number}</td><td class="m">${s.location_area??'—'}</td><td class="m">${(()=>{const sc=v.cells.find(x=>x.main_carrier===s.main_carrier_number);const [a,b]=cellMm(sc||cell);return s.mcc!=null?`${s.mcc}/${s.mnc}`:`${a}/${b}`;})()}</td><td colspan="4" style="font-size:12px">this station's cell (added automatically)</td></tr>`).join('');
  const nb=fcCard('Neighbour cells',`<span class="sp"></span><select class="f" style="min-width:110px" onchange="UI.fc.nb.cell=+this.value;UI.fc.nb.edit=null;UI.fc.nb.pick=false;render(true)">${v.cells.map((x,i)=>`<option value="${i}" ${i===ci?'selected':''}>Cell ${x.id}</option>`).join('')}</select>
      <button class="btn" ${dis} onclick="fcNbNew(null)">${ic('plus')}Add manually</button><button class="btn pri" ${dis} onclick="UI.fc.nb.pick=!UI.fc.nb.pick;UI.fc.nb.edit=null;render(true)">${ic('plus')}Add from server list</button>`,
    fcHelp('What the cell broadcasts in D-NWRK-BROADCAST (at most 7, TS 100 392-2 clause 18.5.17). The station\'s other cells are added automatically.')+
    (cell.primary?'':`<div style="display:flex;align-items:center;gap:10px;padding:10px 18px 4px;font-size:13px"><button class="tog" role="switch" aria-checked="${!!cell.own_list}" ${dis} onclick="fcNbOwn()"></button> Own neighbour list for this cell</div>`)+
    `<div class="tw"><table><thead><tr><th>ID</th><th>Carrier</th><th>LA</th><th>MCC/MNC</th><th>On the server</th><th>Reselection</th><th>Services</th><th></th></tr></thead><tbody>${nbRows||'<tr><td colspan="8" class="mut">No neighbours.</td></tr>'}</tbody></table></div>
    ${fc.nb.pick?fcNbPicker(scells,list,d):''}${fc.nb.edit?fcNbEditor():''}${fcSugBlock(cell,list,dis)}`);
  // Telemetry and control channel.
  const link=(k,title,help)=>{const l=v[k];const ro=false;
    if(!l)return fcCard(title,`<span class="chip i">not set up</span><span class="sp"></span><button class="btn" ${dis||ro?'disabled':''} onclick="fcLinkNew('${k}')">Set up</button>`,fcHelp(help)+'<div style="height:12px"></div>');
    const lk=ro?'disabled':'';
    return fcCard(title,`<span class="sp"></span><span class="mut" style="font-size:12.5px">Connect</span>${fcTog(k+'.enabled',lk)}`,
      fcHelp(help)+`<div style="padding:12px 18px">${fcGrid(fcFld('Server (host)',fcF(k+'.host','str','autocomplete="off" spellcheck="false" '+lk)),fcFld('Port',fcF(k+'.port','int','inputmode="numeric" '+lk)),
      fcFld('Username',fcF(k+'.username','str','autocomplete="off" spellcheck="false" '+lk)),fcFld('Password',fcF(k+'.password','str',`type="password" autocomplete="new-password" placeholder="${l.password_set?'unchanged':'not set'}" ${lk}`)),
      fcFld('TLS (wss://)',`<div style="padding-top:3px">${fcTog(k+'.use_tls',lk)}</div>`),
      ...(k==='telemetry'?[fcFld('Site name',fcF(k+'.site_name')),fcFld('Latitude',fcF(k+'.latitude','num','inputmode="decimal"')),fcFld('Longitude',fcF(k+'.longitude','num','inputmode="decimal"'))]:[]))}</div>`);};
  const tel=link('telemetry','Telemetry','Where the station sends its telemetry (position, health).');
  const cmd=link('command','Remote command / control','The channel this page works over. A wrong value here is caught by the rollback: the station goes back to the old values when it cannot reach this server with the new ones.');
  // Update source.
  const us=D.fleet&&D.fleet.update_source;const ov=v.ota||{};
  const ota=fcCard('Update source',`<span class="sp"></span>${us?`<button class="btn" ${dis} onclick="fcOtaFromServer()" title="Server settings → Updates repo">Use the server's Updates repo</button>`:''}`,`<div style="padding:12px 18px">${fcGrid(
    fcFld('Channel',fcSel('ota.channel',[['installed','installed (as installed)'],['stable','stable'],['beta','beta'],['custom','custom']],'str','data-redraw="1"')),
    ...(ov.channel==='custom'?[fcFld('Repository',fcF('ota.repo','str','spellcheck="false"')),fcFld('Branch',fcF('ota.branch','str','spellcheck="false"'))]:[]))}
    <div class="mut" style="font-size:12px;margin-top:8px">Now: <span class="m">${esc(o.ota.effective_repo||'—')}</span> (${esc(o.ota.effective_branch||'—')})${us?` · Server Updates repo: <span class="m">${esc(us.repo)}</span> (${esc(us.branch)})`:''}</div></div>`);
  const rawChg=fcRawChanged();
  const raw=c.copy?`<details class="card fcdet" ${fc.rawOpen?'open':''} ontoggle="UI.fc.rawOpen=this.open"><summary class="ch"><h2>Config file</h2><span class="chip i">passwords and keys hidden</span>${rawChg?'<span class="chip w">edited</span>':''}<span class="sp"></span><span class="mut" style="font-size:12px">${esc(c.copy.how)} ${esc(tmd(c.copy.at_ms))} · ${esc(c.copy.hash)}</span></summary>
    <div class="mut" style="font-size:12.5px;padding:10px 18px 0">Edit any value here; only the values you change go to the station (comments and layout do not count). Hidden values (***) stay as they are: set passwords in their fields above, keys only on the station.${rawChg?` <a href="#" onclick="event.preventDefault();UI.fc.raw=null;render(true)">Undo edits</a>`:''}</div>
    <textarea class="f m fcraw${rawChg?' chg':''}" id="fc-raw" spellcheck="false" ${locked?'disabled':''} oninput="UI.fc.raw=this.value;this.classList.toggle('chg',fcRawChanged());fcCount()">${esc(fc.raw??c.copy.text)}</textarea></details>`:'';
  const css=`<style>.f.chg,.tog.chg{border-color:var(--warn)!important;box-shadow:0 0 0 2px color-mix(in srgb,var(--warn) 25%,transparent)}.fcg{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px 18px}.fcfs{border:0;margin:0;padding:0;min-width:0;display:grid;gap:18px}.fcdet>summary{cursor:pointer;list-style:none}.fcdet>summary::-webkit-details-marker{display:none}.fcraw{font-family:var(--mono)!important;display:block;box-sizing:border-box;width:calc(100% - 36px);margin:10px 18px 16px;padding:10px 12px;font-size:12px;line-height:1.45;white-space:pre;min-height:420px;resize:vertical;overflow:auto}</style>`;
  // Home Mode Display ([cell_info.home_mode_display]) and ISSI whitelist ([security]); stations with config-v1 from 2026-11-12 on.
  let hmd='',wl='';
  if(v.access){const h=v.home_mode_display;
    hmd=fcCard('Home mode display',`<span class="sp"></span><span class="mut" style="font-size:12.5px">${h?'On':'Off'}</span><button class="tog${fcSame(h==null,o.home_mode_display==null)?'':' chg'}" role="switch" aria-checked="${!!h}" onclick="fcHmdToggle()"></button>`,
      fcHelp('Text the radios show on their home screen, sent as a periodic SDS (protocol 220).')+(h?`<div style="padding:12px 18px">${fcGrid(fcFld('Text',fcF('home_mode_display.text','str','maxlength="64"')),
        fcFld('Sender ISSI',fcF('home_mode_display.source_issi','intn','inputmode="numeric" placeholder="0"')),fcFld('Interval (multiframes, ~1 s)',fcF('home_mode_display.interval_multiframes','intn','inputmode="numeric" placeholder="96"')),
        fcFld('Protocol id',fcF('home_mode_display.protocol_id','intn','inputmode="numeric" placeholder="220"')),fcFld('Coding',fcSel('home_mode_display.text_coding_scheme',[[null,'LATIN (default)'],['LATIN','LATIN (ISO-8859-1)'],['UTF16','UTF16']],'str')))}</div>`:'<div style="height:12px"></div>'));
    const a=v.access,chgL=!fcSame(a.issi_whitelist,o.access.issi_whitelist);
    wl=fcCard('ISSI whitelist','',fcHelp('Which radios may register here. auto: empty list = everyone, else only the list; open: everyone; enforce: only the list (empty = nobody).')+
      `<div style="padding:12px 18px">${fcGrid(fcFld('Mode',fcSel('access.whitelist_mode',[['auto','auto'],['open','open'],['enforce','enforce']],'str')),
        fcFld('ISSIs (comma or space separated)',`<input class="f${chgL?' chg':''}" value="${esc(a.issi_whitelist.join(', '))}" inputmode="numeric" spellcheck="false" onchange="fcWl(this)">`))}</div>`);}
  return css+head+cards+fieldset(brew+nets+cells+nb+resel+hmd+wl+tel+cmd+ota)+raw;
};
// Networks of this server (Multi Network page), to fill in a new network.
function fcServerNets(){const nets=D.nets&&D.nets.networks;if(!nets||!nets.length)return '';
  return `<div class="mut" style="font-size:12px;padding:0 18px 12px">Networks of this server: ${nets.map(n=>`<a href="#" onclick="event.preventDefault();$('fc-nn-name').value='${esc(n.name)}';$('fc-nn-mcc').value='${n.mcc}';$('fc-nn-mnc').value='${n.mnc}'">${esc(n.name)} (${n.mcc}/${n.mnc})</a>`).join(', ')}: click one to fill in.</div>`;}
// A network with its own Brew server (the home network too): the whole link editable, as the classic Brew card.
function fcNetBrewRow(x,i){const v=UI.fc.v;if(!v.networks_explicit||x.removed||x.mode!=='core'||!x.brew)return '';const k=`networks.${i}.brew`;
  return `<tr><td colspan="6" style="padding:4px 18px 14px">${fcGrid(fcFld('Brew server (host)',fcF(k+'.host','str','autocomplete="off" spellcheck="false"')),fcFld('Port',fcF(k+'.port','int','inputmode="numeric"')),
    fcFld('Login (ISSI)',fcF(k+'.username','int','inputmode="numeric"')),fcFld('Password',fcF(k+'.password','str',`type="password" autocomplete="new-password" placeholder="${x.brew.password_set?'unchanged':'not set'}"`)),
    fcFld('TLS',`<div style="padding-top:3px">${fcTog(k+'.tls')}</div>`))}</td></tr>`;}
function fcHmdToggle(){const v=UI.fc.v,o=UI.fc.orig;v.home_mode_display=v.home_mode_display?null:(fcClone(o.home_mode_display)||{text:'',source_issi:16777215,interval_multiframes:96});render(true);}
function fcWl(el){const l=el.value.split(/[\s,;]+/).filter(Boolean).map(x=>parseInt(x,10));
  if(l.some(x=>!(x>=1&&x<=16777215))){setMsg('fc_top','err','ISSI whitelist: numbers 1-16777215');render(true);return;}
  UI.fc.v.access.issi_whitelist=[...new Set(l)];render(true);}
function fcReset(){const fc=UI.fc;fc.v=fcClone(fc.orig);fc.raw=null;fc.nb.edit=null;fc.add=false;render(true);}
function fcCellRemove(id,on){const v=UI.fc.v;v.cells_remove=(v.cells_remove||[]).filter(x=>x!==id);if(on)v.cells_remove.push(id);render(true);}
function fcCellNewDrop(i){UI.fc.v.cells_new.splice(i,1);render(true);}
function fcCellAdd(){const v=UI.fc.v;const num=id=>{const s=$(id).value.trim();return s===''?null:parseInt(s,10);};
  const dev=$('fc-nc-dev').value.trim(),car=num('fc-nc-car');if(!dev||car==null||!(car>=0&&car<4096)){setMsg('fc_top','err','New cell: SDR device and carrier (0-4095) are needed');render(true);return;}
  (v.cells_new=v.cells_new||[]).push({device:dev,main_carrier:car,colour_code:num('fc-nc-cc'),location_area:num('fc-nc-la')});UI.fc.add=false;render(true);}
// Networks follow the station's own page: every network but the primary one can go; cells on a
// removed network fall back to the primary network; a network nobody added any more after a
// classic config was converted turns it back into the classic form.
// The primary network first, then the others by name (as the station lists them).
function fcSortNets(o){if(!o||!o.networks)return;const p=(o.cells.find(c=>c.primary)||o.cells[0]||{}).network;o.networks.sort((a,b)=>(a.name!==p)-(b.name!==p)||a.name.localeCompare(b.name));}
function fcPrimaryNet(){const o=UI.fc.orig,p=o.cells.find(c=>c.primary)||o.cells[0];return p.network==='default'&&!o.networks_explicit?'home':p.network;}
// The station keeps a network no cell uses (inactive) instead of refusing the config.
const fcUnusedOk=()=>((UI.fc&&UI.fc.data&&UI.fc.data.capabilities)||[]).includes('networks-unused-v1');
function fcUnserved(){const v=UI.fc.v;if(!v.networks_explicit)return [];const rm=v.cells_remove||[];const used=v.cells.filter(c=>!rm.includes(c.id)).map(c=>c.network);
  if((v.cells_new||[]).length)used.push(fcPrimaryNet());return v.networks.filter(n=>!n.removed&&!used.includes(n.name)).map(n=>n.name);}
function fcNetRemove(i,on){const v=UI.fc.v,n=v.networks[i];
  if(on&&n.mode==='core'&&!confirm(`Remove network ${n.name}? Its own Brew login in the config is removed too.`))return;
  const isNew=!UI.fc.orig.networks.some(x=>x.name===n.name);
  if(on&&isNew)v.networks.splice(i,1);else n.removed=on;
  if(on){const p=fcPrimaryNet();v.cells.forEach(c=>{if(c.network===n.name)c.network=p;});}
  // Converted on this page and nothing added any more: back to the classic config.
  if(!UI.fc.orig.networks_explicit&&!v.networks.some(x=>!x.converted)){v.networks_explicit=false;v.networks.forEach(x=>{if(x.converted){x.name='default';delete x.converted;}});v.cells.forEach(c=>{if(c.network==='home')c.network='default';});}
  render(true);}
function fcNetAdd(){const v=UI.fc.v;const name=$('fc-nn-name').value.trim(),mcc=parseInt($('fc-nn-mcc').value,10),mnc=parseInt($('fc-nn-mnc').value,10),via=$('fc-nn-via').value;
  if(!/^[A-Za-z0-9_-]+$/.test(name)||!isFinite(mcc)||!isFinite(mnc)){setMsg('fc_top','err','Network: name (letters, digits, _ -), MCC and MNC');render(true);return;}
  if(v.networks.some(x=>x.name===name)){setMsg('fc_top','err','There is a network '+name+' already');render(true);return;}
  const n={name,mcc,mnc,mode:via==='@core'?'core':via==='@local'?'local':'via',via:via.startsWith('@')?null:(via==='default'?'home':via)};
  if(n.mode==='core'){n.brew={host:$('fc-nn-host').value.trim(),port:parseInt($('fc-nn-port').value,10)||443,tls:true,username:parseInt($('fc-nn-user').value,10),password:$('fc-nn-pass').value};}
  // A classic station's network becomes "home" when the first network is added (the station does the same).
  if(!v.networks_explicit){v.networks_explicit=true;v.networks.forEach(x=>{if(o0name(x)){x.name='home';x.converted=true;}});v.cells.forEach(c=>{if(c.network==='default')c.network='home';});}
  v.networks.push(n);render(true);}
const o0name=n=>n.name==='default';
function fcLinkNew(k){UI.fc.v[k]={enabled:true,host:'',port:443,use_tls:true,username:'',password:'',password_set:false,...(k==='telemetry'?{site_name:'',latitude:null,longitude:null}:{})};render(true);}
function fcOtaFromServer(){const us=D.fleet.update_source;UI.fc.v.ota=Object.assign({},UI.fc.v.ota,{channel:'custom',repo:us.repo,branch:us.branch});render(true);}
// Neighbour list of the selected cell.
function fcCellNow(){const fc=UI.fc;return fc.v.cells[Math.min(fc.nb.cell,fc.v.cells.length-1)];}
function fcNbOwn(){const c=fcCellNow();c.own_list=!c.own_list;c.neighbours=c.own_list?(c.neighbours||[]):[];render(true);}
function fcNbFreeId(c){const used=(c.neighbours||[]).map(n=>n.cell_identifier_ca).concat((c.siblings||[]).map(s=>s.cell_identifier_ca));let id=0;while(used.includes(id)&&id<31)id++;return id;}
function fcNbNew(sc){const fc=UI.fc,c=fcCellNow();
  const e={cell_identifier_ca:fcNbFreeId(c),cell_reselection_types_supported:2,neighbor_cell_synchronized:false,cell_load_ca:0,main_carrier_number:'',bs_service_details:Object.assign({},fc.v.service_details)};
  if(sc){e.main_carrier_number=sc.carrier;if(sc.carrier_ext!=null)e.main_carrier_number_extension=sc.carrier_ext;e.location_area=sc.location_area;
    if(sc.mcc!==fc.v.mcc||sc.mnc!==fc.v.mnc){e.mcc=sc.mcc;e.mnc=sc.mnc;}
    if(sc.rxlev_access_min_dbm!=null)e.minimum_rx_access_level=(sc.rxlev_access_min_dbm+125)/5;
    if(sc.ms_txpwr_max_dbm!=null)e.maximum_ms_transmit_power=(sc.ms_txpwr_max_dbm-10)/5;}
  fc.nb.edit={idx:-1,e,from:sc?sc.name:null};fc.nb.pick=false;render(true);}
function fcNbEdit(i){const c=fcCellNow();UI.fc.nb.edit={idx:i,e:fcClone(c.neighbours[i])};UI.fc.nb.pick=false;render(true);}
function fcNbDel(i){const c=fcCellNow();c.neighbours.splice(i,1);if(!c.primary)c.own_list=true;UI.fc.nb.edit=null;render(true);}
function fcNbPicker(scells,list,d){
  const listed=sc=>list.some(nb=>nb.main_carrier_number===sc.carrier&&(nb.location_area==null||nb.location_area===sc.location_area));
  const rows=scells.map((sc,i)=>{const back=(sc.neighbours||[]).some(n=>d.brew_login&&n.startsWith(d.brew_login+'/'));
    return `<tr><td class="m">${esc(sc.name)}</td><td>${esc(sc.station)} ${sc.online===false?'<span class="st d" style="font-size:12px"><i></i></span>':'<span class="st" style="font-size:12px"><i></i></span>'}</td><td class="m">${sc.carrier}</td><td class="m">${sc.location_area}</td><td class="m">${sc.mcc}/${sc.mnc}</td><td>${back?'<span class="chip a">yes</span>':'<span class="chip i">no</span>'}</td>
    <td style="text-align:right">${listed(sc)?'<span class="chip i">listed</span>':`<button class="btn pri" onclick="fcNbNew(UI.fc.pick[${i}])">Add</button>`}</td></tr>`;}).join('');
  UI.fc.pick=scells;
  return `<div style="margin:4px 18px 16px;border:1px solid var(--border);border-radius:10px;background:var(--bg3);overflow:hidden"><div class="ch"><h2 style="font-size:13.5px">Add neighbour from the server list</h2><span class="sp"></span><button class="btn" onclick="UI.fc.nb.pick=false;render(true)">Close</button></div>
    <div class="tw"><table><thead><tr><th>Cell</th><th>Station</th><th>Carrier</th><th>LA</th><th>Network</th><th>Lists ${esc(d.label)}</th><th></th></tr></thead><tbody>${rows||'<tr><td colspan="7" class="mut">No other cells on this server.</td></tr>'}</tbody></table></div>
    <div class="mut" style="font-size:12px;padding:8px 18px 12px">Fills in carrier, LA, MCC/MNC, access level and MS power from what that cell broadcasts (cell directory); edit it afterwards like a manual entry. Both cells should list each other (clause 18.3.4.1).</div></div>`;}
function fcNbEditor(){const ed=UI.fc.nb.edit,e=ed.e;
  const num=(k,l,max,ph)=>fcFld(l,`<input class="f" data-k="${k}" inputmode="numeric" value="${esc(e[k]??'')}" placeholder="${ph||''}" style="width:100%" oninput="fcNbSet(this,${max})">`);
  const sel=(k,l,opts)=>fcFld(l,`<select class="f" data-k="${k}" style="width:100%" onchange="fcNbSet(this,99)">${opts.map(([v,t])=>`<option value="${v??''}" ${String(e[k]??'')===String(v??'')?'selected':''}>${esc(t)}</option>`).join('')}</select>`);
  const rxl=[[null,'not broadcast']].concat([...Array(16).keys()].map(i=>[i,(-125+5*i)+' dBm'])),txp=[[null,'not broadcast']].concat([1,2,3,4,5,6,7].map(i=>[i,(10+5*i)+' dBm']));
  return `<div style="margin:4px 18px 16px;border:1px solid var(--border);border-radius:10px;padding:12px 14px;background:var(--bg3)"><div style="font-weight:650;margin-bottom:10px">${ed.idx<0?'New neighbour':'Neighbour '+e.cell_identifier_ca}${ed.from?` <span class="mut" style="font-weight:500">from ${esc(ed.from)}</span>`:''}</div>
    ${fcGrid(num('cell_identifier_ca','Cell identifier (0-31)',31),num('main_carrier_number','Main carrier (0-4095)',4095),num('main_carrier_number_extension','Carrier extension (optional)',1023),
      sel('cell_reselection_types_supported','Reselection types',FC_TYPES.map((t,i)=>[i,t])),sel('cell_load_ca','Cell load',[[0,'0'],[1,'1'],[2,'2'],[3,'3']]),num('location_area','Location area (optional)',16383),
      num('mcc','MCC (empty = this cell\'s)',1023,UI.fc.v.mcc),num('mnc','MNC (empty = this cell\'s)',16383,UI.fc.v.mnc),sel('minimum_rx_access_level','Min. RX access level',rxl),sel('maximum_ms_transmit_power','Max. MS TX power',txp),
      fcFld('Synchronised with this cell',`<div style="padding-top:3px"><button class="tog" role="switch" aria-checked="${!!e.neighbor_cell_synchronized}" onclick="UI.fc.nb.edit.e.neighbor_cell_synchronized=!UI.fc.nb.edit.e.neighbor_cell_synchronized;this.setAttribute('aria-checked',UI.fc.nb.edit.e.neighbor_cell_synchronized)"></button></div>`))}
    <div class="mut" style="font-size:12px;margin:10px 0 6px">BS service details of the neighbour (table 18.26); missing flags are broadcast as "not supported".</div>
    <div style="display:flex;flex-wrap:wrap;gap:6px 16px;font-size:13px">${FC_SVC.map(([k,l])=>`<label style="white-space:nowrap;cursor:pointer"><input type="checkbox" class="chk" ${e.bs_service_details&&e.bs_service_details[k]?'checked':''} onchange="(UI.fc.nb.edit.e.bs_service_details=UI.fc.nb.edit.e.bs_service_details||{})['${k}']=this.checked"> ${l}</label>`).join('')}</div>
    <div style="display:flex;gap:8px;margin-top:12px"><button class="btn pri" onclick="fcNbApply()">Apply</button><button class="btn" onclick="UI.fc.nb.edit=null;render(true)">Cancel</button></div></div>`;}
function fcNbSet(el,max){const e=UI.fc.nb.edit.e,k=el.dataset.k,s=el.value.trim();if(s===''){delete e[k];return;}const n=parseInt(s,10);if(isFinite(n)&&n>=0&&n<=max)e[k]=n;}
function fcNbApply(){const fc=UI.fc,ed=fc.nb.edit,c=fcCellNow();const e=ed.e;
  if(e.cell_identifier_ca==null||e.main_carrier_number===''||e.main_carrier_number==null){setMsg('fc_top','err','Neighbour: cell identifier and main carrier are needed');render(true);return;}
  const d=e.bs_service_details||{};e.bs_service_details=Object.fromEntries(Object.entries(d).filter(([,on])=>on));
  ['main_carrier_number_extension','location_area','mcc','mnc','minimum_rx_access_level','maximum_ms_transmit_power'].forEach(k=>{if(e[k]==null||e[k]==='')delete e[k];});
  c.neighbours=c.neighbours||[];
  if(ed.idx>=0)c.neighbours[ed.idx]=e;else{if(c.neighbours.length>=7){setMsg('fc_top','err','At most 7 neighbours per cell (D-NWRK-BROADCAST)');render(true);return;}c.neighbours.push(e);}
  if(!c.primary)c.own_list=true;fc.nb.edit=null;render(true);}
// Apply / save as pending, with what changes.
async function fcApplyDlg(){
  const fc=UI.fc,d=fc.data;const lines=fcDiff(fc.orig,fc.v).lines.slice();
  if(fcRawChanged()){try{const r=await postJ('/api/fleet/config/raw_preview',{id:fc.id,text:fc.raw});r.lines.forEach(l=>lines.push('Config file: '+l));}catch(e){setMsg('fc_top','err','Config file: '+e.message);render(true);return;}}
  if(!lines.length){setMsg('fc_top','err','Nothing to change');render(true);return;}
  const un=fcUnusedOk()?[]:fcUnserved();if(un.length){setMsg('fc_top','err',`Every network needs a cell: bind a cell to ${un.join(', ')} or remove it`);render(true);return;}
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${d.online?'Apply to':'Save for'} ${esc(d.label)}</h3></div><div class="sb">
   <div class="tw" style="border:1px solid var(--border);border-radius:8px;max-height:280px;overflow:auto"><table><tbody>${lines.map(l=>`<tr><td style="font-size:12.5px">${esc(l)}</td></tr>`).join('')}</tbody></table></div>
   ${d.online?`<label style="display:flex;gap:10px;align-items:center;font-size:13px"><button class="tog" role="switch" id="fc-call" aria-checked="false" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Apply during a call <span class="mut" style="font-size:12px">(else refused while a call runs)</span></label>
   <div class="mut" style="font-size:12.5px">The station checks the config, restarts its service (about 20 s off the air) and must be back on this server${d.brew_login?' and on Brew':''} within ${d.confirm_secs} s, else it restores the old config by itself.</div>`
   :`<div class="mut" style="font-size:12.5px">The station is offline. The change is kept on the server and applied at the station's next login (after a running call), only if its config has not changed in the meantime.</div>`}
   ${msgDiv('fca-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="fcApply()">${d.online?'Apply now':'Save as pending'}</button></div>`;
  $('scrim').classList.add('on');
}
async function fcApply(resolve){
  const fc=UI.fc,d=fc.data;
  const base=resolve?d.cfg.drift.hash:(d.cfg.copy&&d.cfg.copy.hash);
  const from=resolve?d.cfg.drift.config:fc.orig,to=resolve?d.cfg.copy.config:fc.v;
  const {body,lines}=fcDiff(from,to);
  const call=$('fc-call');
  const req={id:fc.id,base_hash:base,changes:body,summary:lines,during_call:!!(call&&call.getAttribute('aria-checked')==='true'),resolve:!!resolve};
  if(!resolve&&fcRawChanged())req.raw_text=fc.raw;
  try{const r=await postJ('/api/fleet/config/apply',req);
    fc.v=fcClone(fc.orig);fc.raw=null;fc.nb.edit=null;closeSheet();setMsg('fc_top','ok',r.pending?'Saved: applied at the station\'s next login':'Sent: the station restarts with the new config');await fcLoad(false);
  }catch(e){setMsg(resolve?'fcd-msg':'fca-msg','err',e.message);}
}
async function fcRollback(){if(!confirm('Undo the change? The station restores its old config and restarts.'))return;
  try{await postJ('/api/fleet/config/rollback',{id:UI.fc.id});setMsg('fc_top','ok','Undo sent');}catch(e){setMsg('fc_top','err',e.message);}fcLoad(false);}
async function fcDiscardPending(){if(!confirm('Discard the pending change?'))return;
  try{await postJ('/api/fleet/config/discard',{id:UI.fc.id});}catch(e){setMsg('fc_top','err',e.message);}fcLoad(false);}
// The station's config differs from the server copy: import it or apply the copy.
function fcDriftDlg(){
  const fc=UI.fc,d=fc.data,c=d.cfg;if(!c.drift)return;
  const {lines}=c.copy?fcDiff(c.drift.config,c.copy.config,true):{lines:[]};
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${esc(d.label)}: config changed on the station</h3></div><div class="sb">
   <div style="font-size:13px">The config on the station is not the one this server last applied or read (seen ${esc(tmd(c.drift.at_ms))}). Until you choose, nothing on this station can be changed from here.</div>
   <div class="tw" style="border:1px solid var(--border);border-radius:8px;max-height:240px;overflow:auto"><table><thead><tr><th>Station → server copy</th></tr></thead><tbody>${lines.map(l=>`<tr><td style="font-size:12.5px">${esc(l)}</td></tr>`).join('')||'<tr><td class="mut" style="font-size:12.5px">Only settings this page does not show differ (see the config file).</td></tr>'}</tbody></table></div>
   <label style="display:flex;gap:10px;align-items:flex-start;font-size:13px;cursor:pointer"><input type="radio" name="fcd" value="import" checked style="margin-top:3px;accent-color:var(--accent)"><span><b>Import base station config to server</b><span class="mut" style="display:block;font-size:12px">The station's config becomes the server copy. Nothing changes on the station.</span></span></label>
   <label style="display:flex;gap:10px;align-items:flex-start;font-size:13px;cursor:pointer"><input type="radio" name="fcd" value="apply" ${d.online&&c.copy?'':'disabled'} style="margin-top:3px;accent-color:var(--accent)"><span><b>Apply server config to base station</b><span class="mut" style="display:block;font-size:12px">The station goes back to the server copy (the settings above and every other value of the config file; passwords and keys stay as they are on the station): restart, ${d.confirm_secs} s to come back, else rollback.${d.online?'':' Only while the station is online.'}</span></span></label>
   ${msgDiv('fcd-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Later</button><button class="btn pri" onclick="fcDriftGo()">Continue</button></div>`;
  $('scrim').classList.add('on');
}
async function fcDriftGo(){const m=document.querySelector('input[name=fcd]:checked');if(!m)return;
  if(m.value==='apply')return fcApply(true);
  try{await postJ('/api/fleet/config/import',{id:UI.fc.id});closeSheet();setMsg('fc_top','ok','Station config imported');await fcLoad(true);}catch(e){setMsg('fcd-msg','err',e.message);}}
// Fleet list: chip for the config state of a station.
function fleetCfgChip(x){const c=x.config||{};
  if(c.apply)return `<div><span class="chip b" title="Sent ${esc(tmd(c.apply))}">Config change running</span></div>`;
  if(c.drift)return '<div><span class="chip d" title="The config on the station differs from the server copy">Config differs from server copy</span></div>';
  if(c.pending)return `<div><span class="chip ${c.conflict?'d':'w'}" title="${c.conflict?'Not sent: the config on the station changed':'Applied at the next login'}">${c.conflict?'Pending change · conflict':'Pending change · '+c.pending+(c.pending===1?' setting':' settings')}</span></div>`;
  return '';}
// The Fleet Control page follows a running update every 3 s.
setInterval(()=>{if(UI.page==='fleet'&&(fleetJobOn(D.fleet)||(D.fleet&&(D.fleet.stations||[]).some(x=>x.restart||fleetMaintOn(x))))&&!$('scrim').classList.contains('on'))fleetReload();},3000);
const fleetHas=(x,c)=>(x.capabilities||[]).includes(c);
const fleetMaintOn=x=>!!(x.maintenance&&x.maintenance.until_ms>Date.now());
function fleetMaintChip(x){
  if(!fleetMaintOn(x))return '';const m=x.maintenance;const min=Math.max(1,Math.ceil((m.until_ms-Date.now())/60000));
  return `<div><span class="chip d" style="white-space:normal" title="Red banner on the station's dashboard${m.reason?': '+esc(m.reason):''}">Maintenance${m.by?' · '+esc(m.by):''} · ${min} min left</span></div>`;
}
function fleetMaintDlg(id){
  const x=(D.fleet.stations||[]).find(s=>s.id===id);if(!x)return;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Maintenance · ${esc(x.label)}</h3></div><div class="sb">
   <div class="mut" style="font-size:12.5px">Every page of the station's dashboard shows a red banner <b>"Maintenance: work on this base station in progress"</b> with your name, until you end it or the time runs out.</div>
   <div><label class="l" for="fm-min">Duration (minutes)</label><input class="f" id="fm-min" type="number" min="1" max="240" value="30" style="width:110px"></div>
   <div><label class="l" for="fm-reason">Note (shown in the banner, optional)</label><input class="f" id="fm-reason" maxlength="120" placeholder="e.g. new carrier"></div>
   ${msgDiv('fm-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn danger" onclick="fleetMaintGo('${esc(id)}')">Start maintenance</button></div>`;
  $('scrim').classList.add('on');
}
async function fleetMaintGo(id){
  const minutes=Math.max(1,Math.min(240,parseInt(v('fm-min')||'30',10)||30));
  try{await postJ('/api/fleet/maintenance',{id,on:true,minutes,reason:v('fm-reason')||''});closeSheet();fleetReload();}catch(e){setMsg('fm-msg','err',e.message);}
}
async function fleetMaintEnd(id,label){
  if(!confirm('End maintenance on '+label+'? The red banner on the station goes away.'))return;
  try{await postJ('/api/fleet/maintenance',{id,on:false});fleetReload();}catch(e){alert(e.message);}
}
// Restart / shut down (Fleet Control management): restart-v1 stations restart the service or reboot
// over Fleet Control (refused during calls unless allowed); Shut down, and Restart for stations without
// restart-v1, use the older station command (FlowStation [command]).
function fleetRestartDlg(id){
  const x=(D.fleet.stations||[]).find(s=>s.id===id);if(!x)return;
  const v1=fleetHas(x,'restart-v1'),cmd=(D.ctl||[]).includes(id);
  const radio=(v,label,sub,on)=>`<label style="display:flex;gap:10px;align-items:flex-start;font-size:13px;cursor:pointer"><input type="radio" name="fr-kind" value="${v}" ${on?'checked':''} style="margin-top:3px;accent-color:var(--accent)" onchange="$('fr-callrow').hidden=this.value==='shutdown'||!${v1}"><span><b>${label}</b><span class="mut" style="display:block;font-size:12px">${sub}</span></span></label>`;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Power options · ${esc(x.label)}</h3></div><div class="sb">
   <div style="display:grid;gap:10px">${radio('service','Restart service','Restarts FlowStation only (a few seconds to a minute off the air).',true)}${v1?radio('host','Reboot system','Reboots the whole computer (about 2 min off the air).',false):''}${cmd?radio('shutdown','Shut down service','Stops FlowStation. It does not come back by itself: someone has to start it on the station.',false):''}</div>
   <label id="fr-callrow" style="display:flex;gap:10px;align-items:center;font-size:13px" ${v1?'':'hidden'}><button class="tog" role="switch" id="fr-call" aria-checked="false" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Restart during call <span class="mut" style="font-size:12px">(otherwise refused while a call is in progress)</span></label>
   <div class="mut" style="font-size:12.5px">${v1?'The station restarts 5 s after it accepts. Not back after 10 min: marked ALARM in the change log.':'This station has no <code>restart-v1</code> (older FlowStation): the restart goes as a station command, without call protection and without an entry in the change log.'}</div>
   ${msgDiv('fr-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn danger" onclick="fleetRestartGo('${esc(id)}')">Send now</button></div>`;
  $('scrim').classList.add('on');
}
async function fleetRestartGo(id){
  const x=(D.fleet.stations||[]).find(s=>s.id===id);const v1=!!x&&fleetHas(x,'restart-v1');
  const k=document.querySelector('input[name=fr-kind]:checked');const kind=k?k.value:'service';
  const ask={service:'Really restart the service of this station?',host:'Really reboot the whole system of this station?',shutdown:'Really shut the service of this station down? It does not come back by itself.'}[kind];
  if(!confirm(ask))return;
  try{
    if(kind==='shutdown'||!v1)await postJ('/api/control/'+encodeURIComponent(id),{action:kind==='shutdown'?'ShutdownService':'RestartService'});
    else await postJ('/api/fleet/restart',{id,host:kind==='host',during_call:$('fr-call').getAttribute('aria-checked')==='true'});
    closeSheet();setMsg('fl_top','ok',{service:'Service restart sent',host:'Reboot sent',shutdown:'Shut down sent'}[kind]);fleetReload();
  }catch(e){setMsg('fr-msg','err',e.message);}
}
// RF kill switch (rf-standby-v1): the station's own Suspend. The radio stack stops and the
// transmitter goes off; dashboard and control channel stay up, so the station shows as Standby
// here and on the map, and "RF on" starts it again (service restart).
function fleetRfDlg(id){
  const x=(D.fleet.stations||[]).find(s=>s.id===id);if(!x)return;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">RF kill switch · ${esc(x.label)}</h3></div><div class="sb">
   <div style="font-size:13px">Switches the transmitter off with the station's <b>Suspend</b>: the radio stack stops, radios lose the cell. The station stays reachable (dashboard and Fleet Control) and is shown as <b>Standby</b> here and on the map until someone presses <b>RF on</b>.</div>
   <label style="display:flex;gap:10px;align-items:center;font-size:13px"><button class="tog" role="switch" id="frf-call" aria-checked="false" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Also during a call <span class="mut" style="font-size:12px">(otherwise refused while a call is in progress)</span></label>
   <div class="mut" style="font-size:12.5px">The transmitter goes off 2 s after the station accepts.</div>
   ${msgDiv('frf-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn kill" onclick="fleetRfGo('${esc(id)}',true)">${ic('power')}Switch RF off</button></div>`;
  $('scrim').classList.add('on');
}
async function fleetRfOn(id,label){
  if(!confirm('Switch RF on again on '+label+'? The radio stack starts again (service restart, a few seconds to a minute).'))return;
  await fleetRfGo(id,false);
}
async function fleetRfGo(id,standby){
  try{
    await postJ('/api/fleet/rf',{id,standby,during_call:standby&&$('frf-call').getAttribute('aria-checked')==='true'});
    if(standby)closeSheet();setMsg('fl_top','ok',standby?'RF off sent':'RF on sent');fleetReload();
  }catch(e){if(standby)setMsg('frf-msg','err',e.message);else alert(e.message);}
}
async function fleetRemove(id,label){
  if(!confirm('Remove '+label+' from the Fleet Manager? It comes back with its next login while its Fleet Control switch is on.'))return;
  try{await postJ('/api/fleet/remove',{id});D.fleet=await getJ('/api/fleet');render(true);}catch(e){alert(e.message);}}
// INFO popup when a station switched Fleet Control off (once per entry in this browser).
function fleetInfo(){
  const f=D.fleet;if(!f||!can('fleet'))return;
  let seen=null;try{seen=localStorage.getItem('brew-fleet-info');}catch(e){}
  const offs=(f.log||[]).filter(e=>e.kind==='fleet_off');
  const top=offs.reduce((a,e)=>Math.max(a,e.id),0);
  // First visit in this browser: only the last 24 h.
  const fresh=offs.filter(e=>seen!=null?e.id>+seen:Date.now()-e.at_ms<864e5).sort((a,b)=>a.id-b.id);
  try{localStorage.setItem('brew-fleet-info',String(Math.max(top,+(seen||0))));}catch(e){}
  if(!fresh.length||$('scrim').classList.contains('on'))return;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${ic('info')} INFO</h3></div><div class="sb">${fresh.map(e=>`<div><b>${esc(e.text)}</b><div class="mut" style="font-size:12px">${esc(tmd(e.at_ms))}</div></div>`).join('')}</div>
   <div class="acts"><button class="btn" data-go="fleet" onclick="closeSheet()">Fleet Control</button><button class="btn pri" onclick="closeSheet()">OK</button></div>`;
  $('scrim').classList.add('on');
}
async function ctl(id,s,cmd){const body=typeof cmd==='string'?{action:cmd}:cmd;setMsg(s+'_res','mut','Sending…');
  try{const r=await postJ('/api/control/'+encodeURIComponent(id),body);setMsg(s+'_res','ok','Sent'+(r&&r.response?': '+JSON.stringify(r.response):''));}catch(e){setMsg(s+'_res','err',e.message);}}

// One row per login and mode: a station with several Brew connections (one per cell or network) is listed once.
function connGroups(list){const m=new Map();
  for(const b of list||[]){const k=b.mode+'|'+b.station;let g=m.get(k);
    if(!g){g={station:b.station,mode:b.mode,n:0,agents:[],versions:[],addrs:[],connected_at_ms:b.connected_at_ms,registered_issis:0,cells:[]};m.set(k,g);}
    g.n++;g.agents.push(b.agent);g.versions.push(b.version);if(b.remote_addr)g.addrs.push(b.remote_addr);
    g.connected_at_ms=Math.min(g.connected_at_ms,b.connected_at_ms);g.registered_issis+=b.registered_issis;
    for(const x of b.cells||[])if(!g.cells.some(y=>y.name===x.name))g.cells.push(x);}
  return [...m.values()];}
/* ---------- Fleet Control: templates, bulk changes, neighbour suggestions (phase 6, docs/Fleet-Control.md 11) ---------- */
// A template holds settings several stations share; it is applied once (nothing stays bound to it).
// A bulk change applies a template, settings for one run or the missing reciprocal neighbours to
// many stations, parallel or rolling like an update run.
/* ---------- Fleet Control phase 7: alarms, graphs, auto ping ---------- */
const FA_KIND={offline:'Unreachable',sdr_error:'SDR error',cpu_temp:'CPU temperature',sdr_temp:'SDR temperature',cpu_load:'CPU load',ping:'Ping',update_alarm:'Update',restart_alarm:'Restart',config_alarm:'Config'};
const faOpen=a=>a.acked_ms==null;
// A change log ALARM (update, restart, config), shown on Fleet Control; the others are condition alarms (Telemetry).
const faEvent=a=>/_alarm$/.test(a.kind);
const faSev=a=>a.severity==='alarm'?'<span class="chip d">ALARM</span>':'<span class="chip w">Warning</span>';
// Sidebar badge: unacknowledged active alarms (red with an ALARM, else yellow).
// Badges: condition alarms on Telemetry, change log ALARMs (update, restart, config) on Fleet Control.
function fleetAlarmBadge(){const act=(D.fleet&&D.fleet.alarms&&D.fleet.alarms.active)||[];
  for(const [page,ev] of [['telemetry',false],['fleet',true]]){const e=$('b-'+page);if(!e)continue;
   const open=act.filter(a=>faOpen(a)&&faEvent(a)===ev);const red=open.some(a=>a.severity==='alarm');
   e.textContent=open.length||'';e.classList.toggle('d',red);e.classList.toggle('w',open.length>0&&!red);e.title=open.length?open.length+' unacknowledged alarm'+(open.length===1?'':'s'):'';}}
// ev true: the change log ALARMs (Fleet Control page), false: the condition alarms (Telemetry page).
function fleetAlarmCard(f,ev){const al=f.alarms;if(!al)return '';const act=(al.active||[]).filter(a=>faEvent(a)===ev);const hist=(al.history||[]).filter(a=>faEvent(a)===ev);const m=f.can_manage;const open=act.filter(faOpen).length;const k=ev?'e':'c';
  const row=a=>`<tr${a.severity==='alarm'&&faOpen(a)?' style="color:var(--danger);font-weight:600"':''}><td>${faSev(a)}</td><td class="m">${esc(tmd(a.raised_ms))}<div class="mut" style="font-weight:400;font-size:11.5px">${ago(a.raised_ms)}</div></td><td>${esc(a.label)}</td><td><span class="chip">${esc(FA_KIND[a.kind]||a.kind)}</span> ${esc(a.text)}</td><td style="font-size:12.5px;font-weight:400">${a.acked_by?`<span class="mut">Acknowledged by ${esc(a.acked_by)} · ${esc(tmd(a.acked_ms))}</span>`:'<span class="chip w">new</span>'}</td>${m?`<td style="text-align:right">${faOpen(a)?`<button class="btn" onclick="fleetAck(${a.id},${ev})">Acknowledge</button>`:''}</td>`:''}</tr>`;
  const hrow=a=>`<tr><td>${faSev(a)}</td><td class="m">${esc(tmd(a.raised_ms))}</td><td>${esc(a.label)}</td><td><span class="chip">${esc(FA_KIND[a.kind]||a.kind)}</span> ${esc(a.text)}</td><td style="font-size:12.5px"><span class="m">${esc(tmd(a.cleared_ms))}</span><div class="mut">${esc(a.clear_text||'')}${a.acked_by&&!(a.clear_text||'').startsWith('acknowledged')?' · acknowledged by '+esc(a.acked_by):''}</div></td></tr>`;
  return `<div class="card"><div class="ch"><h2>${ev?'Change log alarms':'Alarms'}</h2><span class="cnt">${act.length}</span>${helpBtn('falarm'+k)}<span class="sp"></span>${m&&open?`<button class="btn" onclick="fleetAck(null,${ev})">Acknowledge all (${open})</button>`:''}<button class="btn" aria-expanded="${!!UI['faHist'+k]}" onclick="UI['faHist'+'${k}']=!UI['faHist'+'${k}'];render(true)">History (${hist.length})</button></div>
   ${helpNote('falarm'+k,ev?`ALARMs from the change log of Fleet Control (an update, restart or config change that went wrong). They stay here until someone acknowledges them. <b>Acknowledge</b> (Fleet Control management) marks an alarm with your name and moves it to the history, kept 90 days. The condition alarms of the stations (unreachable, SDR error, temperature, CPU load, missed pings) are on the Telemetry page. Settings: Server settings → <i>Graphs and alarms</i> (admins).`:`Condition alarms of the stations in Fleet Control: unreachable, SDR error, temperature, CPU load, missed pings. An alarm stays here until its cause is gone. <b>Acknowledge</b> (Fleet Control management) marks it with your name; finished alarms go to the history, kept 90 days. ALARMs from the change log (update, restart, config) are on the Fleet Control page. Which alarms there are, their limits and whether they count as ALARM or warning: Server settings → <i>Graphs and alarms</i> (admins).`)}
   ${msgDiv('fa-msg-'+k,'padding:0 18px')}
   ${act.length?`<div class="tw fllog"><table><thead><tr><th>Level</th><th>Since</th><th>Station</th><th>Alarm</th><th>State</th>${m?'<th></th>':''}</tr></thead><tbody>${act.map(row).join('')}</tbody></table></div>`:'<div class="empty">No active alarms.</div>'}
   ${UI['faHist'+k]?(hist.length?`<div class="tw fllog" style="border-top:1px solid var(--border)"><table><thead><tr><th>Level</th><th>Raised</th><th>Station</th><th>Alarm</th><th>Finished</th></tr></thead><tbody>${hist.map(hrow).join('')}</tbody></table></div>`:'<div class="empty" style="border-top:1px solid var(--border)">No finished alarms in the last 90 days.</div>'):''}</div>`;}
async function fleetAck(id,ev){const box='fa-msg-'+(ev?'e':'c');try{const r=await postJ('/api/fleet/alarms/ack',id==null?{events:!!ev}:{id});D.fleet=await getJ('/api/fleet');setMsg(box,'ok',r.acknowledged+' acknowledged');fleetAlarmBadge();render(true);}catch(e){setMsg(box,'err',e.message);}}
// Auto ping: every 5 s every online Fleet Control station, while the Telemetry page is open (not stored).
function fleetAutoPing(on){UI.apingOn=on;UI.aping={};clearInterval(UI.apingT);
  if(on){const run=()=>{if(UI.page!=='telemetry'){fleetAutoPing(false);return;}
    for(const x of (D.fleet.stations||[]).filter(x=>x.online))postJ('/api/fleet/ping',{id:x.id}).then(r=>{UI.aping[x.id]={ok:r.ok,ms:r.ms,at:Date.now()};}).catch(()=>{UI.aping[x.id]={ok:false,at:Date.now()};}).finally(()=>{if(UI.page==='telemetry'&&UI.apingOn&&!$('scrim').classList.contains('on'))render(true);});};
    run();UI.apingT=setInterval(run,5000);}
  render(true);}
// The graph page of one station.
PG.fleetgraph=['fleetgraph','Telemetry','tele','Infrastructure'];
function fleetGraph(id){UI.fg={id,hours:null,data:null,err:null,hover:{},series:{}};show('fleetgraph');fgLoad();}
async function fgLoad(){const g=UI.fg;if(!g)return;
  try{const d=await getJ('/api/fleet/graph?id='+encodeURIComponent(g.id)+'&hours='+(g.hours||Math.min(24,(g.data&&g.data.keep_hours)||24)));if(UI.fg!==g)return;g.data=d;g.err=null;}catch(e){g.err=e.message;}
  if(UI.page==='fleetgraph')render(true);}
setInterval(()=>{if(UI.page==='fleetgraph'&&UI.fg&&!$('scrim').classList.contains('on'))fgLoad();},30000);
function fgRange(h){UI.fg.hours=h;fgLoad();}
const FG_SDR=['#8e5bd6','#e07b00','#00a3a3','#c2185b','#6d8b00','#5c6bc0','#a1887f','#26a69a'];
// One chart: lines break where points are further apart than `gap` ms or a value is missing.
function fgChart(key,title,unit,series,o){const d=UI.fg.data;const lims=(o.limits||(o.limit!=null?[{v:o.limit,name:'Alarm limit'}]:[])).filter(l=>l.v!=null);o.limit=lims.length?Math.max(...lims.map(l=>l.v)):null;const from=d.from_ms,to=d.now_ms;const span=Math.max(1,to-from);
  const vals=series.flatMap(s=>s.pts.filter(p=>p[1]!=null).map(p=>p[1]));
  let lo=o.min!=null?o.min:Math.min(...vals,...lims.map(l=>l.v)),hi=o.max!=null?o.max:Math.max(...vals,o.limit??-Infinity);
  if(!vals.length){lo=o.min??0;hi=o.max??1;}
  if(o.min==null)lo=Math.floor((lo-3)/5)*5;if(o.max==null)hi=Math.ceil((hi+3)/5)*5;if(o.int)hi=Math.max(hi,Math.ceil(Math.max(1,...vals)*1.2));if(hi<=lo)hi=lo+1;
  const X=t=>((t-from)/span*1000).toFixed(1),Y=v=>(200-(v-lo)/(hi-lo)*200).toFixed(1);
  const path=(pts,gap)=>{let s='',prev=null;for(const [t,v] of pts){if(v==null){prev=null;continue;}s+=(prev==null||t-prev>gap?'M':'L')+X(t)+' '+Y(v)+' ';prev=t;}return s;};
  // Colours as style (CSS variables do not work in SVG presentation attributes).
  const lines=series.map(s=>`<path d="${path(s.pts,s.gap)}" fill="none" style="stroke:${s.color}" stroke-width="2" vector-effect="non-scaling-stroke" stroke-linejoin="round"/>`+s.pts.filter((p,i,a)=>p[1]!=null&&(i===0||a[i-1][1]==null||p[0]-a[i-1][0]>s.gap)&&(i===a.length-1||a[i+1][1]==null||a[i+1][0]-p[0]>s.gap)).map(p=>`<line x1="${X(p[0])}" x2="${X(p[0])}" y1="${Y(p[1])}" y2="${Y(p[1])}" style="stroke:${s.color}" stroke-width="5" stroke-linecap="round" vector-effect="non-scaling-stroke"/>`).join('')).join('');
  const lim=lims.filter(l=>l.v>=lo&&l.v<=hi).map(l=>`<line x1="0" x2="1000" y1="${Y(l.v)}" y2="${Y(l.v)}" style="stroke:${l.color||'var(--danger)'}" stroke-dasharray="6 5" stroke-width="1" vector-effect="non-scaling-stroke"/>`).join('');
  const miss=(o.miss||[]).map(t=>`<line x1="${X(t)}" x2="${X(t)}" y1="200" y2="170" style="stroke:var(--danger)" stroke-width="2" vector-effect="non-scaling-stroke"/>`).join('');
  const grid=[0,.25,.5,.75,1].map(f=>`<line x1="0" x2="1000" y1="${200*f}" y2="${200*f}" style="stroke:var(--border)" stroke-width="1" vector-effect="non-scaling-stroke"/>`).join('');
  const ylab=[1,.5,0].map(f=>`<span class="fgy" style="top:${(1-f)*100}%">${+(lo+(hi-lo)*f).toFixed(hi-lo<5?1:0)}</span>`).join('');
  const ticks=6,xl=Array.from({length:ticks+1},(_,i)=>from+span*i/ticks).map((t,i)=>`<span class="fgx" style="left:${i/ticks*100}%">${span>36e5*30?new Date(t).toLocaleDateString([], {day:'2-digit',month:'2-digit'})+' ':''}${new Date(t).toLocaleTimeString([], {hour:'2-digit',minute:'2-digit'})}</span>`).join('');
  const last=s=>{const p=[...s.pts].reverse().find(p=>p[1]!=null);return p?`${+p[1].toFixed(o.int?0:1)} ${unit}`:'—';};
  UI.fg.series[key]={series,unit,int:!!o.int};
  return `<div class="card"><div class="ch"><h2>${title}</h2><span class="sp"></span><span class="mut m" style="font-size:12px" id="fgr-${key}">${fgRead(key)}</span></div>
   <div class="svc fgleg">${series.map(s=>`<span><i style="background:${s.color}"></i>${esc(s.name)} <b class="m">${last(s)}</b></span>`).join('')}${lims.map(l=>`<span><i class="lim" style="border-top-color:${l.color||'var(--danger)'}"></i>${esc(l.name)} ${l.v} ${unit}</span>`).join('')}${o.miss&&o.miss.length?`<span><i style="background:var(--danger);width:3px"></i>${o.miss.length} without answer</span>`:''}</div>
   <div class="fgw"><div class="fgc" onmousemove="fgHover('${key}',event,this)" onmouseleave="fgHover('${key}',null,this)"><svg viewBox="0 0 1000 200" preserveAspectRatio="none" role="img" aria-label="${esc(title)}">${grid}${lim}${lines}${miss}</svg>${ylab}</div><div class="fgxs">${xl}</div></div>
   ${vals.length||(o.miss&&o.miss.length)?'':'<div class="empty" style="padding-top:0">No values in this time.</div>'}</div>`;}
// Readout of the values nearest to the pointer.
function fgRead(key){const g=UI.fg;const hv=g.hover[key];const c=g.series[key];if(!hv||!c)return '';
  const v=c.series.map(s=>{let best=null;for(const p of s.pts)if(p[1]!=null&&(best==null||Math.abs(p[0]-hv.t)<Math.abs(best[0]-hv.t)))best=p;return best&&Math.abs(best[0]-hv.t)<=s.gap?esc(s.name)+' '+(+best[1].toFixed(c.int?0:1))+(c.unit?' '+c.unit:''):null;}).filter(Boolean);
  return esc(tmd(hv.t))+': '+(v.join(' · ')||'no data');}
function fgHover(key,ev,el){const g=UI.fg;if(!g||!g.data)return;if(!ev)delete g.hover[key];else{const r=el.getBoundingClientRect();const f=Math.min(1,Math.max(0,(ev.clientX-r.left)/r.width));g.hover[key]={t:g.data.from_ms+(g.data.now_ms-g.data.from_ms)*f};}
  const e=$('fgr-'+key);if(e)e.innerHTML=fgRead(key);}
R.fleetgraph=()=>{const g=UI.fg;if(!g)return loading;const d=g.data;
  const back=`<button class="btn ic" title="Back to Telemetry" aria-label="Back to Telemetry" onclick="show('telemetry')">‹</button>`;
  if(!d)return g.err?`<div class="card"><div class="empty">${esc(g.err)} <button class="btn" onclick="show('telemetry')">Back</button></div></div>`:loading;
  const step=d.sample_ms*1.5+5000,pgap=Math.max(d.ping_ms,60000)*1.5+5000;const P=d.points||[];
  const cells=[...new Set(P.flatMap(p=>Object.keys(p.s||{})))].sort((a,b)=>a-b);
  const hours=g.hours||Math.min(24,d.keep_hours);
  const ranges=[1,6,24,72,168,d.keep_hours].filter((h,i,a)=>h<=d.keep_hours&&a.indexOf(h)===i);
  const lbl=h=>h===d.keep_hours&&h>24?`All (${h>=48?Math.round(h/24)+' d':h+' h'})`:h>=48?Math.round(h/24)+' d':h+' h';
  const head=`<div class="card"><div class="ch">${back}<h2>Graphs · ${esc(d.label)}</h2>${d.online?'<span class="st"><i></i>Online</span>':`<span class="st d"><i></i>Offline · ${ago(d.last_seen_ms)}</span>`}<span class="sp"></span><div style="display:flex;gap:6px;flex-wrap:wrap" role="group" aria-label="Time range">${ranges.map(h=>`<button class="btn${h===hours?' pri':''}" aria-pressed="${h===hours}" onclick="fgRange(${h})">${lbl(h)}</button>`).join('')}</div></div>
   <div class="svc mut" style="font-size:12.5px">One point every ${d.sample_ms/60000} min (CPU load: mean, temperatures and counts: highest), kept ${d.keep_hours} h, then overwritten. A gap means the station sent nothing (unreachable or switched off). ${d.metrics?'':'<b>This station has no <code>metrics-v1</code></b> (older FlowStation): temperatures, radios and calls come from its telemetry, the CPU load is missing until it is updated.'} Settings: Server settings → Graphs and alarms.</div>
   ${(d.alarms||[]).length?`<div class="svc">${d.alarms.map(a=>`<div>${faSev(a)} ${esc(a.text)} <span class="mut" style="font-size:12px">· ${ago(a.raised_ms)}</span></div>`).join('')}</div>`:''}</div>`;
  let h=head;
  h+=fgChart('cpu','CPU load','%',[{name:'CPU',color:'var(--accent2)',pts:P.map(p=>[p.t,p.c??null]),gap:step}],{min:0,max:100,limit:d.cpu_load_limit});
  const temps=[{name:'CPU',color:'var(--accent)',pts:P.map(p=>[p.t,p.ct??null]),gap:step}].concat(cells.map((c,i)=>({name:cells.length>1?'SDR cell '+c:'SDR',color:FG_SDR[i%FG_SDR.length],pts:P.map(p=>[p.t,(p.s||{})[c]??null]),gap:step})));
  h+=fgChart('temp','Temperatures','°C',temps,{limits:[{v:d.cpu_temp_limit,name:'CPU limit'},{v:d.sdr_temp_limit,name:'SDR limit',color:'var(--warn)'}]});
  const pings=d.pings||[];
  h+=fgChart('ping',`Ping${d.ping_ms?' (server → station every '+d.ping_ms/60000+' min)':' (off in Server settings)'}`,'ms',[{name:'Round trip',color:'var(--warn)',pts:pings.map(p=>[p.t,p.ms??null]),gap:pgap}],{min:0,int:true,miss:pings.filter(p=>p.ms==null).map(p=>p.t)});
  if(d.show_radios)h+=fgChart('radios','Registered radios','',[{name:'Radios',color:'var(--accent)',pts:P.map(p=>[p.t,p.r??null]),gap:step}],{min:0,int:true});
  if(d.show_calls)h+=fgChart('calls','Calls in progress','',[{name:'Calls',color:'var(--accent2)',pts:P.map(p=>[p.t,p.k??null]),gap:step}],{min:0,int:true});
  return h;};
// Server settings: graphs, ping and the alarm rules (admins).
const FM_RULES=[['offline','Station unreachable',null,'Minutes offline'],['sdr_error','SDR error or stack health critical',null,'Minutes'],['cpu_temp','CPU temperature','°C','Minutes'],['sdr_temp','SDR temperature (any cell)','°C','Minutes'],['cpu_load','CPU load','%','Minutes'],['ping','Missed pings','pings in a row',null],['events','ALARMs from the change log (update, restart, config)',null,null]];
function fleetMonSettingsCard(){const f=D.fleet;const s=f&&f.alarms&&f.alarms.settings;if(!s)return '';const r=s.rules;
  const num=(id,val,min,max,step,w)=>`<input class="f" id="${id}" type="number" min="${min}" max="${max}" step="${step||1}" value="${val}" style="width:${w||90}px">`;
  const sw=(id,on,label)=>`<label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600"><button class="tog" role="switch" id="${id}" aria-checked="${!!on}" aria-label="${esc(label)}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button>${label}</label>`;
  return `<div class="card"><div class="ch"><h2>Graphs and alarms</h2>${helpBtn('fmon')}<span class="sp"></span><a data-go="telemetry" href="#telemetry">Telemetry ›</a></div>${helpNote('fmon','Stations with <code>metrics-v1</code> send their CPU load, CPU and SDR temperatures, radios and calls every minute; older stations are sampled from their telemetry (no CPU load). <b>Sample interval</b>: one graph point per this many minutes. <b>Keep</b>: graphs are kept this long, then the oldest points are overwritten (at most 10 080 points per station, so more than 7 days need a longer sample interval). <b>Ping interval</b>: the server pings every online station this often for the ping graph (0 = off). <b>Alarms</b>: each rule can be switched off, counted as ALARM (red) or warning (yellow), and given its limit and how many minutes the condition must last (0 = at once). A rule switched off finishes its active alarms. The CPU load alarm is not raised while a station updates (the build takes the CPU) and for 5 min after.')}
   <div class="svc" style="display:flex;gap:14px;align-items:end;flex-wrap:wrap">
    <div><label class="l" for="fm-sample">Sample interval (min)</label>${num('fm-sample',s.sample_min,1,60)}</div>
    <div><label class="l" for="fm-keep">Keep graphs (h)</label>${num('fm-keep',s.keep_hours,1,720)}</div>
    <div><label class="l" for="fm-ping">Ping interval (min, 0 = off)</label>${num('fm-ping',s.ping_min,0,60)}</div>
   </div>
   <div class="svc" style="display:flex;gap:18px;flex-wrap:wrap"><span class="mut" style="font-size:12.5px;align-self:center">Optional graph lines:</span>${sw('fm-radios',s.show_radios,'Registered radios')}${sw('fm-calls',s.show_calls,'Calls in progress')}</div>
   <div class="tw"><table><thead><tr><th>Alarm</th><th>On</th><th>Level</th><th>Limit</th><th>Minutes</th></tr></thead><tbody>${FM_RULES.map(([k,label,unit,mins])=>{const x=r[k];return `<tr><td><b>${label}</b></td>
     <td><button class="tog" role="switch" id="fm-${k}-on" aria-checked="${!!x.enabled}" aria-label="${esc(label)} on" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button></td>
     <td><select class="f" id="fm-${k}-sev" aria-label="${esc(label)} level"><option value="alarm"${x.severity==='alarm'?' selected':''}>ALARM</option><option value="warning"${x.severity==='warning'?' selected':''}>Warning</option></select></td>
     <td>${unit?`<span style="display:inline-flex;gap:6px;align-items:center">${num('fm-'+k+'-thr',x.threshold,1,120,k==='ping'?1:0.5,80)}<span class="mut" style="font-size:12px">${unit}</span></span>`:'<span class="mut">—</span>'}</td>
     <td>${mins?num('fm-'+k+'-min',x.minutes,k==='offline'?1:0,60,1,80):'<span class="mut">—</span>'}</td></tr>`;}).join('')}</tbody></table></div>
   <div class="svc"><button class="btn pri" onclick="saveFleetMon()">Save</button></div>${msgDiv('fm-msg','padding:0 18px 12px')}</div>`;}
async function saveFleetMon(){const s=JSON.parse(JSON.stringify(D.fleet.alarms.settings));const n=id=>{const e=$(id);return e?Number(e.value):0;};const on=id=>$(id).getAttribute('aria-checked')==='true';
  s.sample_min=n('fm-sample');s.keep_hours=n('fm-keep');s.ping_min=n('fm-ping');s.show_radios=on('fm-radios');s.show_calls=on('fm-calls');
  for(const [k,,unit,mins] of FM_RULES){const x=s.rules[k];x.enabled=on('fm-'+k+'-on');x.severity=$('fm-'+k+'-sev').value;if(unit)x.threshold=n('fm-'+k+'-thr');if(mins)x.minutes=n('fm-'+k+'-min');}
  try{await postJ('/api/fleet/monitor/settings',s);D.fleet=await getJ('/api/fleet');setMsg('fm-msg','ok','Saved');render(true);}catch(e){setMsg('fm-msg','err',e.message);}}
PG.fleettpl=['fleettpl','Fleet Control','fleet','Infrastructure'];
const TPL_SEC=[['brew','Brew server'],['multi_network','Networks (Multi Network)'],['reselect','Cell reselection'],['rxlev_access_min_dbm','Min. access level'],['migration','Migration'],['presence_check_secs','Presence check'],['home_mode_display','Home mode display'],['access','ISSI whitelist'],['telemetry','Telemetry server'],['ota','Update source']];
const tplSecName=k=>(TPL_SEC.find(x=>x[0]===k)||[k,k])[1];
const TPL_DEF={brew:{host:'',port:443,tls:true},multi_network:{enabled:false,networks:[]},reselect:{fast_threshold_db:10,slow_threshold_db:20,fast_hysteresis_db:6,slow_hysteresis_db:10},rxlev_access_min_dbm:null,migration:false,presence_check_secs:30,
  home_mode_display:null,access:{issi_whitelist:[],whitelist_mode:'auto'},telemetry:{enabled:true,host:'',port:443,use_tls:true},ota:{channel:'installed',repo:'',branch:''}};
const BULK_STATE={pending:['Queued','i'],sending:['Sending…','b'],waiting_call:['Call in progress · waiting','w'],applying:['Restarting · confirming','b'],done:['Changed','a'],saved:['Pending (offline)','w'],failed:['ALARM','d'],skipped:['Skipped','w']};
const bulkOn=f=>!!(f&&f.bulk&&!f.bulk.finished_ms);
const fleetCfgCap=x=>(x.capabilities||[]).includes('config-v1');
// Open the template editor: `t` a saved template, or `from` sections of a station (Save as template), or a run's settings (`oneoff`: station ids).
function tplOpen(t,from,fromLabel,oneoff){
  const v=fcClone(TPL_DEF);const src=t?t.sections:(from||{});
  for(const k in src)v[k]=fcClone(src[k]);
  if(v.ota){v.ota.repo=v.ota.repo||'';v.ota.branch=v.ota.branch||'';}
  if(v.multi_network&&!Array.isArray(v.multi_network.networks))v.multi_network.networks=[];
  const on=new Set(Object.keys(src).filter(k=>TPL_SEC.some(x=>x[0]===k)));
  UI.tp={id:t?t.id:null,name:t?t.name:(fromLabel?'From '+fromLabel:''),note:t?t.note||'':'',on,v,from:fromLabel||null,oneoff:oneoff||null,back:UI.page==='fleetcfg'?'fleetcfg':'fleet'};
  show('fleettpl');
}
function tpGet(p){return fcGet(UI.tp.v,p);}
function tpIn(el){const p=el.dataset.p,t=el.dataset.t||'str';let val;
  if(t==='bool'){val=el.getAttribute('aria-checked')!=='true';el.setAttribute('aria-checked',val);}
  else if(t==='int'||t==='intn'){const s=el.value.trim();val=s===''?null:parseInt(s,10);if(val!=null&&!isFinite(val))return;}
  else val=el.value;
  fcPut(UI.tp.v,p,val);if(el.dataset.redraw)render(true);}
function tpF(p,t,extra,w){return `<input class="f" data-p="${p}" data-t="${t||'str'}" value="${esc(tpGet(p)??'')}" oninput="tpIn(this)" style="width:${w||'100%'}" ${extra||''}>`;}
function tpSel(p,opts,t,extra){const v=tpGet(p);return `<select class="f" data-p="${p}" data-t="${t||'str'}" onchange="tpIn(this)" style="width:100%" ${extra||''}>${opts.map(([k,l])=>`<option value="${esc(k??'')}" ${String(k??'')===String(v??'')?'selected':''}>${esc(l)}</option>`).join('')}</select>`;}
function tpTog(p,extra){return `<button class="tog" role="switch" aria-checked="${!!tpGet(p)}" data-p="${p}" data-t="bool" onclick="tpIn(this)" ${extra||''}></button>`;}
function tpOn(k,on){if(on)UI.tp.on.add(k);else UI.tp.on.delete(k);render(true);}
R.fleettpl=()=>{
  const tp=UI.tp;if(!tp)return loading;const f=D.fleet||{};const m=f.can_manage;const v=tp.v;
  const back=`<button class="btn ic" title="Back" aria-label="Back" onclick="show('${tp.back==='fleetcfg'&&UI.fc?'fleetcfg':'fleet'}')">‹</button>`;
  const n=tp.on.size;
  const head=`<div class="card"><div class="ch" style="gap:10px">${back}<h2 style="font-size:17px">${tp.oneoff?'Settings for '+tp.oneoff.length+' station'+(tp.oneoff.length===1?'':'s'):tp.id?'Template':'New template'}</h2>${tp.from?`<span class="chip i">from ${esc(tp.from)}</span>`:''}
    <span class="sp"></span>${m?`<span class="mut" style="font-size:12px">${n} section${n===1?'':'s'}</span>${tp.oneoff?`<button class="btn pri" ${n?'':'disabled'} onclick="tpNext()">Next: preview and start</button>`:`<button class="btn pri" ${n?'':'disabled'} onclick="tpSave()">${tp.id?'Save template':'Create template'}</button>`}`:''}</div>
    ${tp.oneoff?'':`<div style="padding:12px 18px;display:grid;gap:10px;grid-template-columns:minmax(0,1fr) minmax(0,2fr)" class="tpgrid"><div><label class="l" for="tp-name">Name</label><input class="f" id="tp-name" maxlength="60" value="${esc(tp.name)}" oninput="UI.tp.name=this.value" style="width:100%" ${m?'':'disabled'} placeholder="e.g. Standard Austria"></div>
      <div><label class="l" for="tp-note">Note</label><input class="f" id="tp-note" maxlength="300" value="${esc(tp.note)}" oninput="UI.tp.note=this.value" style="width:100%" ${m?'':'disabled'} placeholder="What it is for (optional)"></div></div>`}
    <div class="note">${ic('info')}<div>${tp.oneoff?'Switch on the sections to change; only those are compared with each station and sent where they differ.':'Only the switched-on sections are part of the template. Applied once to the chosen stations: each station gets these values, nothing stays bound to the template.'} Never part of it: carrier, colour code, LA, cells, neighbours, logins and passwords (each station keeps its own).</div></div>${msgDiv('tp_top','padding:0 18px 12px')}</div>`;
  const card=(k,body,help)=>{const on=tp.on.has(k);return `<div class="card${on?'':' tpoff'}"><div class="ch"><h2>${esc(tplSecName(k))}</h2><span class="sp"></span><span class="mut" style="font-size:12.5px">${on?(tp.oneoff?'Change':'In template'):'Not included'}</span><button class="tog" role="switch" aria-checked="${on}" aria-label="Include ${esc(tplSecName(k))}" ${m?'':'disabled'} onclick="tpOn('${k}',${!on})"></button></div>${on?`<fieldset class="fcfs" ${m?'':'disabled'}>${help?fcHelp(help):''}<div style="padding:12px 18px">${body}</div></fieldset>`:''}</div>`;};
  const rs=fcDb(0,30,2);const rx=[[null,'default (-110)']].concat(fcDb(-125,-50,5));
  const nets=(v.multi_network.networks||[]);
  const netRows=nets.map((x,i)=>`<tr><td>${tpF(`multi_network.networks.${i}.name`,'str','spellcheck="false" placeholder="name"')}</td><td style="width:90px">${tpF(`multi_network.networks.${i}.mcc`,'int','inputmode="numeric"')}</td><td style="width:100px">${tpF(`multi_network.networks.${i}.mnc`,'int','inputmode="numeric"')}</td>
    <td style="width:150px">${tpSel(`multi_network.networks.${i}.mode`,[['local','local (no Brew)'],['via','via another network'],['core','own Brew server']],'str','data-redraw="1"')}</td>
    <td>${x.mode==='via'?tpF(`multi_network.networks.${i}.via`,'str','placeholder="network name, e.g. home"'):x.mode==='core'?`<div style="display:flex;gap:6px">${tpF(`multi_network.networks.${i}.host`,'str','placeholder="Brew host"')}${tpF(`multi_network.networks.${i}.port`,'int','inputmode="numeric" placeholder="443"','80px')}</div>`:'<span class="mut">—</span>'}</td>
    <td style="text-align:right"><button class="btn danger" onclick="UI.tp.v.multi_network.networks.splice(${i},1);render(true)">Remove</button></td></tr>`).join('');
  const cards=[
    card('brew',fcGrid(fcFld('Brew server (host)',tpF('brew.host','str','autocomplete="off" spellcheck="false"')),fcFld('Port',tpF('brew.port','int','inputmode="numeric"')),fcFld('TLS',`<div style="padding-top:3px">${tpTog('brew.tls')}</div>`)),'For stations with the classic Brew link; each keeps its Brew login and password. Stations with several networks set their Brew servers per network.'),
    card('multi_network',`<label style="display:flex;gap:10px;align-items:center;font-size:13px">${tpTog('multi_network.enabled','data-redraw="1"')} Multi Network on</label>${v.multi_network.enabled?`<div class="tw" style="margin-top:10px"><table><thead><tr><th>Name</th><th>MCC</th><th>MNC</th><th>Reached</th><th>Via / server</th><th></th></tr></thead><tbody>${netRows||'<tr><td colspan="6" class="mut">No networks: only the switch is set.</td></tr>'}</tbody></table></div><button class="btn" style="margin-top:10px" onclick="UI.tp.v.multi_network.networks.push({name:'',mcc:null,mnc:null,mode:'via',via:'home'});render(true)">${ic('plus')}Add network</button>`:''}`,'Networks are added or changed by name; networks a station has besides these stay. A network with its own Brew server only changes on stations that already have it (its login stays the station\'s).'),
    card('reselect',fcGrid(...FC_RESEL.map(([k,l])=>fcFld(l,tpSel('reselect.'+k,rs,'int')))),'Cell reselection thresholds and hysteresis (TS 100 392-2 clause 18.5.4), for all cells of a station.'),
    card('rxlev_access_min_dbm',fcGrid(fcFld('RXLEV_ACCESS_MIN (dBm)',tpSel('rxlev_access_min_dbm',rx,'intn'))),'Weakest signal a radio may use the cell with (clause 18.5.4; link failure below it).'),
    card('migration',`<label style="display:flex;gap:10px;align-items:center;font-size:13px">${tpTog('migration')} Migration (radios of other networks may register)</label>`),
    card('presence_check_secs',fcGrid(fcFld('Presence check after silence (s, 0 = off)',tpF('presence_check_secs','int','inputmode="numeric"')))),
    card('home_mode_display',`<label style="display:flex;gap:10px;align-items:center;font-size:13px"><button class="tog" role="switch" aria-checked="${v.home_mode_display!=null}" onclick="UI.tp.v.home_mode_display=UI.tp.v.home_mode_display?null:{text:'',source_issi:16777215,interval_multiframes:96};render(true)"></button> Show a text on the radios</label>${v.home_mode_display?`<div style="margin-top:10px">${fcGrid(fcFld('Text',tpF('home_mode_display.text','str','maxlength="30"')),fcFld('Source ISSI',tpF('home_mode_display.source_issi','int','inputmode="numeric"')),fcFld('Every (multiframes)',tpF('home_mode_display.interval_multiframes','int','inputmode="numeric"')))}</div>`:''}`),
    card('access',fcGrid(fcFld('ISSIs (comma or space separated)',`<input class="f" value="${esc((v.access.issi_whitelist||[]).join(', '))}" oninput="UI.tp.v.access.issi_whitelist=this.value.split(/[\\s,;]+/).filter(Boolean).map(x=>parseInt(x,10)).filter(x=>isFinite(x))" style="width:100%" inputmode="numeric">`),fcFld('Mode',tpSel('access.whitelist_mode',[['auto','auto (empty = everyone)'],['open','open (everyone)'],['enforce','enforce (only the list)']]))),'Which radios may register. Replaces the station\'s list.'),
    card('telemetry',fcGrid(fcFld('Connect',`<div style="padding-top:3px">${tpTog('telemetry.enabled')}</div>`),fcFld('Server (host)',tpF('telemetry.host','str','autocomplete="off" spellcheck="false"')),fcFld('Port',tpF('telemetry.port','int','inputmode="numeric"')),fcFld('TLS',`<div style="padding-top:3px">${tpTog('telemetry.use_tls')}</div>`)),'Only stations that have telemetry set up; each keeps its login, site name and position.'),
    card('ota',fcGrid(fcFld('Channel',tpSel('ota.channel',[['installed','installed (as installed)'],['stable','stable'],['beta','beta'],['custom','custom repo / branch']],'str','data-redraw="1"')),...(v.ota.channel==='custom'?[fcFld('Repository',tpF('ota.repo','str','spellcheck="false" placeholder="https://github.com/…"')),fcFld('Branch',tpF('ota.branch','str','spellcheck="false"'))]:[]))+(f.update_source?`<div style="margin-top:10px"><button class="btn" onclick="Object.assign(UI.tp.v.ota,{channel:'custom',repo:D.fleet.update_source.repo,branch:D.fleet.update_source.branch});render(true)">Use the server's Updates repo</button></div>`:'')),
  ];
  return `<style>.tpoff .ch{opacity:.75}@media (max-width:640px){.tpgrid{grid-template-columns:1fr!important}}</style>`+head+cards.join('');
};
function tpSections(){const tp=UI.tp,s={};for(const [k] of TPL_SEC)if(tp.on.has(k))s[k]=fcClone(tp.v[k]);
  if(s.ota&&s.ota.channel!=='custom'){s.ota.repo=null;s.ota.branch=null;}
  if(s.multi_network)s.multi_network.networks=(s.multi_network.networks||[]).filter(n=>n.name);
  return s;}
async function tpSave(){const tp=UI.tp;
  try{const r=await postJ('/api/fleet/templates/save',{id:tp.id,name:tp.name,note:tp.note,sections:tpSections()});tp.id=r.id;D.fleet=await getJ('/api/fleet');setMsg('fl_top','ok',`Template "${tp.name.trim()}" saved`);show('fleet');}catch(e){setMsg('tp_top','err',e.message);render(true);}}
function tpNext(){const tp=UI.tp;const ids=tp.oneoff;show('fleet');fleetBulkDlg({sections:tpSections()},ids);}
async function tplDelete(id){const t=(D.fleet.templates||[]).find(x=>x.id===id);if(!t||!confirm(`Delete the template "${t.name}"? Stations it was applied to keep their settings.`))return;
  try{await postJ('/api/fleet/templates/delete',{id});fleetReload();}catch(e){alert(e.message);}}
// The templates card on the Fleet Control page.
function fleetTplCard(f){const m=f.can_manage;const ts=f.templates||[];
  const rows=ts.map(t=>`<tr><td><b>${esc(t.name)}</b>${t.note?`<div class="mut" style="font-size:12px">${esc(t.note)}</div>`:''}</td><td><div style="display:flex;flex-wrap:wrap;gap:4px">${TPL_SEC.filter(([k])=>k in t.sections).map(([k,l])=>`<span class="chip">${esc(l)}</span>`).join('')}</div></td><td class="mut hide-s" style="font-size:12px;white-space:nowrap">${esc(tmd(t.updated_ms))}${t.by?' · '+esc(t.by):''}</td>
    <td style="text-align:right"><div style="display:flex;gap:6px;justify-content:flex-end;flex-wrap:nowrap">${m?`<button class="btn pri" onclick="fleetBulkDlg({template:${t.id}})">Apply…</button><button class="btn" onclick="tplOpen((D.fleet.templates||[]).find(x=>x.id===${t.id}))">Edit</button><button class="btn danger" onclick="tplDelete(${t.id})">${ic('trash')}</button>`:`<button class="btn" onclick="tplOpen((D.fleet.templates||[]).find(x=>x.id===${t.id}))">View</button>`}</div></td></tr>`).join('');
  return `<div class="card"><div class="ch"><h2>Templates</h2><span class="cnt">${ts.length}</span>${helpBtn('fltpl')}<span class="sp"></span>${m?`<button class="btn" onclick="tplOpen(null)">${ic('plus')}New template</button>`:''}</div>
   ${helpNote('fltpl','Settings several stations share: Brew server, networks, cell reselection, access level, migration, presence check, Home mode display, ISSI whitelist, telemetry server, update source. Never carrier, colour code, LA, cells, neighbours, logins or passwords. <b>Apply…</b> changes the chosen stations once (parallel or rolling, each with its own rollback window); nothing stays bound to the template. A template can also be made from a station: its edit page → <b>Save as template</b>.')}
   ${ts.length?`<div class="tw"><table><thead><tr><th>Name</th><th>Sections</th><th class="hide-s">Changed</th><th></th></tr></thead><tbody>${rows}</tbody></table></div>`:`<div class="empty">No templates yet.${m?' <b>New template</b> or, on a station\'s edit page, <b>Save as template</b>.':''}</div>`}</div>`;}
// The bulk run card (like the update run card).
function fleetBulkCard(f){const r=f.bulk;if(!r)return '';const live=bulkOn(f);
  if(r.finished_ms&&UI.nbcRun!==r.id+':'+r.finished_ms){const first=UI.nbcRun===undefined;UI.nbcRun=r.id+':'+r.finished_ms;if(!first&&UI.nbc)setTimeout(fleetNbLoad,0);}
  if(!live&&r.finished_ms&&!r.cancelled&&!r.items.some(i=>['failed','skipped'].includes(i.state.state))&&Date.now()-r.finished_ms>60000)return '';
  const n=k=>r.items.filter(i=>i.state.state===k).length;const w=r.what;
  const what=w.kind==='reciprocal'?'Reciprocal neighbours':w.name?`Template "${esc(w.name)}"`:'Settings for this run';
  const fin=['done','saved','failed','skipped'];const p=r.items.length?Math.round(100*r.items.reduce((a,i)=>a+(fin.includes(i.state.state)?1:i.state.state==='applying'?0.6:i.state.state==='sending'?0.2:0),0)/r.items.length):0;
  return `<div class="card"><div class="ch"><h2>Bulk config change</h2>${live?'<span class="chip b">running</span>':r.cancelled?'<span class="chip w">cancelled</span>':'<span class="chip a">finished</span>'}<span class="sp"></span><span class="mut" style="font-size:12px">${esc(tmd(r.started_ms))}${r.user?' · '+esc(r.user):''}</span>${live&&f.can_manage?'<button class="btn danger" onclick="fleetBulkCancel()">Cancel</button>':''}</div>
   <div class="svc" style="font-size:13px"><b>${what}</b> · ${r.mode==='rolling'?'Rolling (one after another)':'Parallel (all at once)'} · ${r.during_call?'change during call':'calls interrupted after '+r.interrupt_min+' min'} · ${n('done')} changed, ${n('saved')} pending (offline), ${n('failed')} failed, ${n('skipped')} skipped${live?', '+r.items.filter(i=>!fin.includes(i.state.state)).length+' to go':''}</div>
   <div class="svc" style="display:flex;align-items:center;gap:10px"><div style="flex:1;height:8px;border-radius:4px;background:var(--bg4);overflow:hidden"><div style="height:8px;border-radius:4px;width:${p}%;background:var(${n('failed')?'--danger':'--accent'});transition:width .6s"></div></div><b class="m" style="font-size:13px;min-width:42px;text-align:right">${p} %</b></div>
   <div class="tw"><table><thead><tr><th>#</th><th>Station</th><th>State</th><th>Changes</th></tr></thead><tbody>${r.items.map((i,k)=>{const s=BULK_STATE[i.state.state]||[i.state.state,'i'];const why=i.state.reason;
     return `<tr${i.state.state==='failed'?' style="color:var(--danger)"':''}><td class="m">${k+1}</td><td><b>${esc(i.label)}</b></td><td><span class="chip ${s[1]}" style="white-space:normal">${s[0]}${why?' · '+esc(why):''}</span></td><td style="font-size:12px">${(i.summary||[]).slice(0,6).map(l=>`<div>${esc(l)}</div>`).join('')}${(i.summary||[]).length>6?`<div class="mut">… ${i.summary.length-6} more</div>`:''}${(i.notes||[]).map(l=>`<div class="mut">${esc(l)}</div>`).join('')}</td></tr>`;}).join('')}</tbody></table></div></div>`;}
function fleetChangeCfg(){const f=D.fleet;const st=(f.stations||[]).filter(fleetCfgCap);const ids=UI.fsel.size?[...UI.fsel].filter(id=>st.some(x=>x.id===id)):st.map(x=>x.id);
  if((f.templates||[]).length)fleetBulkDlg({template:f.templates[0].id},ids);else tplOpen(null,null,null,ids);}
async function fleetBulkCancel(){if(!confirm('Cancel the bulk config change? No further station starts; a station that is restarting finishes (with its rollback window).'))return;
  try{await postJ('/api/fleet/bulk/cancel',{});fleetReload();}catch(e){alert(e.message);}}
// Bulk dialog: what (template, settings, reciprocal neighbours), preview per station, parallel / rolling, calls.
function fleetBulkDlg(src,ids){
  const st=(D.fleet.stations||[]).filter(fleetCfgCap);
  const list=ids||(UI.fsel.size?[...UI.fsel].filter(id=>st.some(x=>x.id===id)):st.map(x=>x.id));
  UI.bk={src,ids:list,preview:null,err:null,mode:'rolling',call:false,int:'10'};fleetBulkDraw();fleetBulkPreview();$('scrim').classList.add('on');}
function fleetBulkKeep(){const b=UI.bk;if(!b||!$('bk-int'))return;const m=document.querySelector('input[name=bk-mode]:checked');if(m)b.mode=m.value;b.call=$('bk-call').getAttribute('aria-checked')==='true';b.int=v('bk-int');}
function fleetBulkSrc(val){const b=UI.bk;fleetBulkKeep();b.src=val==='reciprocal'?{reciprocal:true}:val==='sections'?b.src0:{template:+val};b.preview=null;fleetBulkDraw();fleetBulkPreview();}
async function fleetBulkPreview(){const b=UI.bk;if(!b)return;b.err=null;
  try{const r=await postJ('/api/fleet/bulk/preview',Object.assign({ids:b.ids},b.src));if(UI.bk!==b)return;b.preview=r.stations;}catch(e){b.err=e.message;}
  if(UI.bk===b&&$('scrim').classList.contains('on')){fleetBulkKeep();fleetBulkDraw();}}
function fleetBulkDraw(){const b=UI.bk;const f=D.fleet;const ts=f.templates||[];
  if(b.src.sections)b.src0=b.src;
  const cur=b.src.reciprocal?'reciprocal':b.src.sections?'sections':String(b.src.template);
  const opts=ts.map(t=>[String(t.id),'Template: '+t.name]).concat(b.src0?[['sections','Settings for this run']]:[]).concat([['reciprocal','Add missing reciprocal neighbours']]);
  const pv=b.preview;const ok=pv?pv.filter(x=>x.ok):[];const skip=pv?pv.filter(x=>!x.ok):[];
  const radio=(val,label,sub)=>`<label style="display:flex;gap:10px;align-items:flex-start;font-size:13px;cursor:pointer"><input type="radio" name="bk-mode" value="${val}" ${b.mode===val?'checked':''} style="margin-top:3px;accent-color:var(--accent)"><span><b>${label}</b><span class="mut" style="display:block;font-size:12px">${sub}</span></span></label>`;
  const rows=pv?pv.map(x=>`<div style="padding:8px 0;border-top:1px solid var(--sep)"><div style="display:flex;gap:8px;align-items:center"><b style="font-size:13px">${esc(x.label)}</b>${x.online?'<span class="st" style="font-size:12px"><i></i>Online</span>':'<span class="st d" style="font-size:12px"><i></i>Offline</span>'}<span class="sp"></span>${x.ok?(x.online?`<span class="chip b">${x.summary.length} change${x.summary.length===1?'':'s'}</span>`:'<span class="chip w">kept as pending</span>'):`<span class="chip w" style="white-space:normal">Skipped · ${esc(x.reason)}</span>`}</div>
     ${x.ok?`<div style="font-size:12px;margin-top:4px">${x.summary.map(l=>`<div>${esc(l)}</div>`).join('')}${(x.notes||[]).map(l=>`<div class="mut">${esc(l)}</div>`).join('')}</div>`:''}</div>`).join(''):'<div class="mut" style="font-size:12.5px">Comparing with each station…</div>';
  const n=ok.length;const cs=f.confirm_secs||90;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Change config of ${b.ids.length} station${b.ids.length===1?'':'s'}</h3></div><div class="sb">
   <div><label class="l" for="bk-src">What</label><select class="f" id="bk-src" style="width:100%" onchange="fleetBulkSrc(this.value)">${opts.map(([k,l])=>`<option value="${esc(k)}" ${k===cur?'selected':''}>${esc(l)}</option>`).join('')}</select>
    <div style="margin-top:6px;display:flex;gap:8px;flex-wrap:wrap"><button class="btn" onclick="closeSheet();tplOpen(null,null,null,UI.bk.ids)">Edit settings for this run…</button></div></div>
   ${n>1?`<div style="display:grid;gap:10px">${radio('rolling','Rolling (one after another)','The next station starts when the previous one is back with the new config and confirmed (or rolled back). A station that rolls back is an ALARM and the run goes on.')}${radio('parallel','Parallel (all at once)','Every station restarts at the same time (about 20 s off the air each).')}</div>`:''}
   <label style="display:flex;gap:10px;align-items:center;font-size:13px"><button class="tog" role="switch" id="bk-call" aria-checked="${b.call}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true');$('bk-int').disabled=this.getAttribute('aria-checked')==='true'"></button> Change during call <span class="mut" style="font-size:12px">(a running call is cut at the restart)</span></label>
   <div><label class="l" for="bk-int">Interrupt a call after (minutes)</label><input class="f" id="bk-int" type="number" min="0" max="${f.limits.max_interrupt_min}" value="${esc(b.int)}" ${b.call?'disabled':''} style="width:110px"></div>
   <div class="mut" style="font-size:12.5px">Each station checks the change, restarts its service and has <b>${cs} s</b> to come back to this server (and Brew); otherwise it restores its old config by itself. Offline stations get the change as pending, sent at their next login. Stations with a pending change already, a running change or a config that differs from the server copy are skipped.</div>
   ${b.err?`<div class="msg err">${esc(b.err)}</div>`:''}
   <div><div style="display:flex;gap:8px;align-items:center"><b style="font-size:13px">Preview</b><span class="mut" style="font-size:12px">${pv?`${n} to change, ${skip.length} skipped`:''}</span></div>${rows}</div>
   ${msgDiv('bk-msg','padding:0')}</div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" ${n&&!bulkOn(f)?'':'disabled'} title="${bulkOn(f)?'A bulk change is running':''}" onclick="fleetBulkStart()">Start (${n})</button></div>`;}
async function fleetBulkStart(){const b=UI.bk;fleetBulkKeep();const ids=(b.preview||[]).filter(x=>x.ok).map(x=>x.id);
  try{await postJ('/api/fleet/bulk/start',Object.assign({ids,mode:b.mode,during_call:b.call,interrupt_min:Math.max(0,parseInt(b.int||'10',10)||0)},b.src));closeSheet();UI.fsel.clear();setMsg('fl_top','ok','Bulk config change started');fleetReload();}catch(e){setMsg('bk-msg','err',e.message);}}
// Fleet-wide neighbour check: cells that list a station's cell without being listed back, and marked entries.
function fleetNbCard(f){const c=UI.nbc;const m=f.can_manage;
  const body=!UI.nbcOpen?'':!c?'<div class="empty">Checking…</div>':!c.stations.length?`<div class="empty">Every cell lists the cells that list it, and every listed neighbour is in the cell directory (${c.directory_cells} cells).</div>`
    :`<div class="tw"><table><thead><tr><th>Station</th><th>Not listed back</th><th>Marked entries</th></tr></thead><tbody>${c.stations.map(s=>`<tr><td><b>${esc(s.label)}</b>${m?`<div><button class="btn" style="margin-top:4px" onclick="fleetEdit('${esc(s.id)}')">Edit</button></div>`:''}</td><td style="font-size:12.5px">${s.missing.map(x=>`<div>Cell ${x.cell}: <span class="m">${esc(x.from)}</span> lists it ${x.online?'':'<span class="mut">(offline)</span>'}</div>`).join('')||'<span class="mut">—</span>'}</td><td style="font-size:12.5px">${s.marks.map(x=>`<div><span class="chip w">Cell ${x.cell}</span> ${esc(x.text)}</div>`).join('')||'<span class="mut">—</span>'}</td></tr>`).join('')}</tbody></table></div>`;
  const nmiss=c?c.stations.filter(s=>s.missing.length).length:0;
  return `<div class="card"><div class="ch" style="cursor:pointer" onclick="fleetNbToggle()"><h2>Neighbour check</h2>${c&&UI.nbcOpen?`<span class="cnt">${c.stations.length}</span>`:''}${helpBtn('flnb')}<span class="sp"></span>${UI.nbcOpen&&m&&nmiss?`<button class="btn pri" onclick="event.stopPropagation();fleetBulkDlg({reciprocal:true},UI.nbc.stations.filter(s=>s.missing.length).map(s=>s.id))">Add missing reciprocal (${nmiss})</button>`:''}${UI.nbcOpen?`<button class="btn" onclick="event.stopPropagation();fleetNbLoad()">Check again</button>`:''}<span class="mut" style="font-size:12px">${UI.nbcOpen?'Hide':'Show'}</span></div>
   ${helpNote('flnb','From the server\'s cell directory: a cell that broadcasts one of a station\'s cells as neighbour but is not listed back (both should list each other, TS 100 392-2 clause 18.3.4.1), and listed neighbours that are not in the directory any more or broadcast another LA. <b>Add missing reciprocal</b> adds them as a bulk change (at most 7 neighbours per cell, siblings included). On a station\'s edit page the Neighbour cells card also suggests cells radios moved to and cells close by.')}${body}</div>`;}
function fleetNbToggle(){UI.nbcOpen=!UI.nbcOpen;if(UI.nbcOpen&&!UI.nbc)fleetNbLoad();render(true);}
async function fleetNbLoad(){try{UI.nbc=await getJ('/api/fleet/neighbours/check');}catch(e){UI.nbc={stations:[],directory_cells:0};}if(UI.page==='fleet')render(true);}
// Edit page: neighbour suggestions for the cell shown.
async function fcSugLoad(){const fc=UI.fc;if(!fc)return;
  try{const r=await getJ('/api/fleet/neighbours?id='+encodeURIComponent(fc.id)+'&radius_km='+(fc.radius||30));if(UI.fc===fc){fc.sug=r;if(UI.page==='fleetcfg')render(true);}}catch(e){fc.sug={err:e.message};}}
function fcSugCell(cell){const s=UI.fc.sug;return s&&s.cells?s.cells.find(c=>c.id===cell.id):null;}
function fcSugMark(cell,nb){const c=fcSugCell(cell);if(!c)return null;return (c.marks||[]).find(m=>m.carrier===nb.main_carrier_number&&(m.kind==='missing'||nb.location_area!==m.location_area));}
const SUG_WHY={reciprocal:()=>'<span class="chip a" title="That cell broadcasts this one as neighbour (clause 18.3.4.1: both should list each other)">lists this cell</span>',moves:w=>`<span class="chip b" title="Radios changed between these cells (last 30 days)">${w.count} cell change${w.count===1?'':'s'}</span>`,distance:w=>`<span class="chip i">${w.km} km</span>`};
function fcSugBlock(cell,list,dis){const fc=UI.fc;const s=fc.sug;
  if(!s)return '<div class="mut" style="font-size:12.5px;padding:8px 18px 14px">Looking for suggestions…</div>';
  if(s.err)return `<div class="mut" style="font-size:12.5px;padding:8px 18px 14px">No suggestions: ${esc(s.err)}</div>`;
  const c=fcSugCell(cell);const full=list.length+(cell.siblings||[]).length>=7;
  const rows=(c?c.suggestions:[]).filter(x=>!list.some(nb=>nb.main_carrier_number===x.cell.carrier&&(nb.mcc??fc.v.mcc)===x.cell.mcc&&(nb.mnc??fc.v.mnc)===x.cell.mnc));
  return `<div style="margin:6px 18px 16px;border:1px dashed var(--border);border-radius:10px;overflow:hidden"><div class="ch" style="gap:8px"><h2 style="font-size:13.5px">Suggestions</h2><span class="cnt">${rows.length}</span><span class="sp"></span><label class="mut" style="font-size:12px;display:flex;gap:6px;align-items:center">Close by within <input class="f" type="number" min="1" max="500" value="${fc.radius||30}" style="width:70px" onchange="UI.fc.radius=Math.max(1,Math.min(500,+this.value||30));fcSugLoad()"> km</label></div>
   ${rows.length?`<div class="tw"><table><thead><tr><th>Cell</th><th>Carrier</th><th>LA</th><th>MCC/MNC</th><th>Why</th><th></th></tr></thead><tbody>${rows.map(x=>`<tr><td><b class="m">${esc(x.cell.name)}</b> ${x.cell.online?'<span class="st" style="font-size:12px"><i></i></span>':'<span class="st d" style="font-size:12px" title="offline"><i></i></span>'}</td><td class="m">${x.cell.carrier}</td><td class="m">${x.cell.location_area}</td><td class="m">${x.cell.mcc}/${x.cell.mnc}</td><td><div style="display:flex;gap:4px;flex-wrap:wrap">${x.why.map(w=>(SUG_WHY[w.kind]||(()=>''))(w)).join('')}</div></td>
     <td style="text-align:right"><button class="btn pri" ${dis} ${full?'disabled title="7 neighbours at most (siblings included)"':''} onclick="fcSugAdd('${esc(x.cell.name)}')">${ic('plus')}Add</button></td></tr>`).join('')}</tbody></table></div>`:`<div class="mut" style="font-size:12.5px;padding:10px 18px">No suggestions for this cell${s.position?'':' (no position for this station: Accounts or its telemetry)'}.</div>`}
   <div class="mut" style="font-size:12px;padding:8px 18px 12px">From the server's cell directory: cells that list this cell, cells radios moved to or from (30 days) and cells close by. Nothing is added by itself; an added entry is filled in from what that cell broadcasts and can be edited like any other.</div></div>`;}
function fcSugAdd(name){const fc=UI.fc;const c=fcCellNow();const s=fcSugCell(c);const x=s&&s.suggestions.find(y=>y.cell.name===name);if(!x)return;
  if(!c.primary)c.own_list=true;c.neighbours=c.neighbours||[];
  const e=fcClone(x.entry);const used=c.neighbours.map(n=>n.cell_identifier_ca).concat((c.siblings||[]).map(n=>n.cell_identifier_ca));
  if(used.includes(e.cell_identifier_ca)){let id=0;while(used.includes(id)&&id<31)id++;e.cell_identifier_ca=id;}
  c.neighbours.push(e);render(true);}
function fcNbFixLa(i,la){const c=fcCellNow();c.neighbours[i].location_area=la;render(true);}
// Server settings: the confirmation window of config changes (also bulk changes).
function fleetCfgSettingsCard(){const f=D.fleet;if(!f||f.confirm_secs==null)return '';
  return `<div class="card"><div class="ch"><h2>Config changes (Fleet Control)</h2>${helpBtn('cfgwin')}<span class="sp"></span><a data-go="fleet" href="#fleet">Fleet Control ›</a></div>${helpNote('cfgwin','After a config change from Fleet Control (one station or a bulk change) the station restarts and must be back on this server, and on Brew if it has a Brew login, within this time; otherwise it restores its old config by itself. 60 to 900 s, default 90 s.')}
   <div class="svc" style="display:flex;gap:10px;align-items:end;flex-wrap:wrap"><div><label class="l" for="cw-secs">Confirmation window (s)</label><input class="f" id="cw-secs" type="number" min="60" max="900" step="10" value="${f.confirm_secs}" style="width:120px"></div><button class="btn pri" onclick="saveCfgWindow()">Save</button></div>${msgDiv('cw-msg','padding:0 18px 12px')}</div>`;}
async function saveCfgWindow(){try{await postJ('/api/fleet/config/settings',{confirm_secs:parseInt(v('cw-secs'),10)||0});D.fleet=await getJ('/api/fleet');setMsg('cw-msg','ok','Saved');render(true);}catch(e){setMsg('cw-msg','err',e.message);}}
/* ---------- routing ---------- */
R.routing=()=>{
  const adm=D.who.admin;
  const pools=D.pools;
  const live=p=>{const l=p.live;return l?`<span class="talk"><span class="bars"><b></b><b></b><b></b><b></b></span>${issiTxt(l.talker)} talking on ${tgTxt(l.gssi)}</span> · heard on ${l.legs.length} group${l.legs.length===1?'':'s'}${l.waiting.length?` · ${l.waiting.join(', ')} join${l.waiting.length===1?'s':''} after ${l.waiting.length===1?'its':'their'} own over`:''}`:(p.enabled?'Idle':'Switched off');};
  // Linked talkgroups: one group of this server both ways to one group of another network.
  const links=pools.map((p,i)=>[p,i]).filter(([p])=>p.linked);
  const ownNet=(D.nets&&D.nets.name)||'This server';
  const linkRows=links.map(([p,i])=>{const r=(p.remote||[])[0]||{network:'?',gssi:0};const g=p.groups[0];
   return `<tr class="${p.enabled?'':'off'}"><td><button class="tog" role="switch" aria-checked="${p.enabled}" aria-label="Link ${esc(p.name)} on" ${adm?'':'disabled'} onclick="togglePool(${i})"></button></td>
    <td>${p.name===autoLink(p)?'<span class="mut">—</span>':`<b>${esc(p.name)}</b>`}</td><td><span class="mut" style="font-size:12px">${esc(ownNet)}</span><br>${tgChip(g)}</td><td class="arrow" style="font-size:18px">⇄</td>
    <td><span class="mut" style="font-size:12px">${esc(r.network)}</span><br><span class="tg net" title="Group of the network ${esc(r.network)}">${ic('globe')} ${r.gssi}</span>${p.on_demand?' <span class="chip" title="Only while a radio here is in the talkgroup">on demand</span>':''}</td>
    <td>${live(p)}</td><td style="white-space:nowrap">${adm?`<button class="btn" onclick="editLink(${i})">Edit</button> <button class="btn danger" onclick="delPool(${i})">Delete</button>`:''}</td></tr>`;}).join('');
  const poolRows=pools.map((p,i)=>{if(p.linked)return '';const l=p.live;const mem=[...p.groups.map(g=>({t:tgChip(g,(l&&l.gssi===g)?' src':''),g})),...(p.remote||[]).map(r=>({t:`<span class="tg net" title="Group of the network ${esc(r.network)}">${ic('globe')} ${esc(r.network)}:${r.gssi}</span>`}))];
   const groups=mem.map((m,j)=>`${!p.both_ways&&j===0?m.t.replace('class="tg','class="tg src'):m.t}${j===0&&!p.both_ways?' <span class="arrow">→</span>':''}`).join(' ');
   const state=live(p);
   return `<div class="row pool${p.enabled?'':' offp'}"><button class="tog" role="switch" aria-checked="${p.enabled}" aria-label="Pool ${esc(p.name)} on" ${adm?'':'disabled'} onclick="togglePool(${i})"></button>
    <div class="rm"><div class="rt">${esc(p.name)} <span class="chip ${p.enabled?'a':'i'}">${p.enabled?'on':'off'}</span><span class="chip">${p.both_ways?'⇄ both ways':'→ one way'}</span></div>
    <div class="flow">${p.both_ways?groups.replace(/<\/span> <span class="tg/,'</span> <span class="arrow">⇄</span> <span class="tg'):groups}</div><div class="rs" style="margin-top:6px">${state}</div></div>
    ${adm?`<button class="btn" onclick="editPool(${i})">Edit</button><button class="btn danger" onclick="delPool(${i})">Delete</button>`:''}</div>`;}).join('');
  return `<div class="card"><div class="ch"><h2>Linked talkgroups</h2><span class="cnt">${links.length}</span>${helpBtn('links')}<span class="sp"></span>${adm?`<button class="btn pri" onclick="editLink(-1)">${ic('plus')}New link</button>`:''}</div>
   ${helpNote('links','A linked talkgroup connects one group of this server with one group of another network (Multi Network link), both ways: a call on either group is heard on the other, the talker\'s ISSI is passed on 1:1. It works like a talkgroup pool with exactly these two groups and is saved as one (<code>[[talkgroup_pools]]</code> with <code>linked = true</code>). The other network must be set up on the Multi Network page. Changes apply at once, no restart.')}
   ${links.length?`<div class="tw"><table><thead><tr><th style="width:56px">On</th><th>Name</th><th>This server · TG</th><th style="width:30px"></th><th>Linked network · TG</th><th>Status</th><th></th></tr></thead><tbody>${linkRows}</tbody></table></div>`:'<div class="empty">No linked talkgroups yet. A link connects one group here with one group of another network.</div>'}
   ${msgDiv('link-msg')}</div>
  <div class="card"><div class="ch"><h2>Talkgroup pools</h2><span class="cnt">${pools.length-links.length}</span>${helpBtn('pools')}<span class="sp"></span>${adm?`<button class="btn pri" onclick="editPool(-1)">${ic('plus')}New pool</button>`:''}</div>
   ${poolRows.trim()||'<div class="empty">No pools yet. A pool plays the calls of one group on other groups too.</div>'}
   ${helpNote('pools','A pool switches groups together inside this server. A call on a group of the pool is played 1:1 on the other groups, as an ordinary group call on each group\'s own GSSI, so the radios see nothing unusual. <i>Both ways</i>: every group of the pool reaches all others. <i>One way</i>: only the first group is copied, to the rest. A group that has a call of its own waits until that over has ended and then joins. Changes apply at once and are saved to the config file, no restart. Federation peers do not get the copies.')}
   ${msgDiv('pool-msg')}</div>
  ${tgCard(adm,pools)}
  <div class="card"><div class="svc mut" style="font-size:12.5px">DGNA (assign radios to a group) is on <a data-go="radioctl" href="#radioctl">DGNA &amp; Radio actions ›</a></div></div>`;
};
function selAll(on){D.radios.filter(r=>r.radio&&match(UI.filter.dg||'',r.issi,r.station,r.cell,r.groups.join(' '))).forEach(r=>on?UI.sel.add(r.issi):UI.sel.delete(r.issi));render(true);}
function selOne(i,on){on?UI.sel.add(i):UI.sel.delete(i);render(true);}
async function sendDgna(){
  const f=UI.dgf,gssi=+f.gssi;
  if(!(gssi>=1&&gssi<=16777215)){setMsg('dg-msg','err','Enter a GSSI (1 to 16777215).');$('dg-gssi').focus();return;}
  setMsg('dg-msg','mut','Sending…');
  try{const res=await postJ('/api/dgna',{issis:[...UI.sel],gssi,name:f.name||null,assign:f.assign,attachment_mode:f.mode});
    const bad=res.filter(r=>!r.sent);
    setMsg('dg-msg',bad.length?'err':'ok',bad.length?`Sent to ${res.length-bad.length} of ${res.length}. Not sent: `+bad.map(b=>b.issi+' ('+b.error+')').join(', '):`Sent to ${res.length} radio${res.length===1?'':'s'}. Their answers show under Last DGNA.`);
    await refreshSlow();
  }catch(e){setMsg('dg-msg','err',e.message);}
}
async function savePools(list,okText,box='pool-msg'){
  try{await postJ('/api/pools/save',list);D.pools=await getJ('/api/pools');setMsg(box,'ok',okText);render(true);return true;}
  catch(e){setMsg(box,'err',e.message);render(true);return false;}
}
// Name a link gets when its name field is left empty.
const autoLink=p=>{const r=(p.remote||[])[0]||{};return `TG ${p.groups[0]} ⇄ ${r.network}:${r.gssi}`.slice(0,40);};
const plain=p=>({name:p.name,enabled:p.enabled,both_ways:p.both_ways,groups:p.groups,remote:(p.remote||[]).map(r=>({network:r.network,gssi:r.gssi})),linked:!!p.linked,on_demand:!!p.on_demand});
// Editor of a linked talkgroup: one own group, one network, one group there.
function editLink(i){
  const p=i>=0?plain(D.pools[i]):{name:'',enabled:true,both_ways:true,groups:[],remote:[],linked:true};
  const r=p.remote[0]||{network:'',gssi:''};const nets=(D.nets&&D.nets.networks||[]).map(n=>n.name);
  if(r.network&&!nets.includes(r.network))nets.push(r.network);
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${i>=0?'Edit linked talkgroup':'New linked talkgroup'}</h3></div><div class="sb">
   <div><label class="l" for="le-name">Name (optional)</label><input class="f" id="le-name" maxlength="40" value="${p.name===autoLink(p)?'':esc(p.name)}" placeholder="e.g. Austria wide ⇄ TetraPack"><div class="mut" style="font-size:12px;margin-top:4px">Can stay empty.</div></div>
   <div><label class="l" for="le-own">Talkgroup of this server (GSSI)</label><input class="f" id="le-own" inputmode="numeric" value="${p.groups[0]??''}" placeholder="e.g. 9" autocomplete="off"></div>
   <div><label class="l" for="le-net">Linked network (Multi Network)</label>${nets.length?`<select class="f" id="le-net">${nets.map(n=>`<option ${n===r.network?'selected':''}>${esc(n)}</option>`).join('')}</select>`:`<input class="f" id="le-net" value="${esc(r.network)}" placeholder="network name as on the Multi Network page">`}</div>
   <div><label class="l" for="le-tg">Talkgroup in the linked network (GSSI)</label><input class="f" id="le-tg" inputmode="numeric" value="${r.gssi}" placeholder="e.g. 909" autocomplete="off"></div>
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="le-dyn" aria-checked="${!!p.on_demand}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Only on demand: the group of the other network comes only while a radio here is in this talkgroup (off: permanent, its calls always come)</span></label>
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="le-on" aria-checked="${p.enabled}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Switched on</label>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveLink(${i})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('le-name').focus(),30);
}
async function saveLink(i){
  const m=$('sheet-msg'),own=+$('le-own').value.trim(),tg=+$('le-tg').value.trim(),net=$('le-net').value.trim();
  const bad=t=>{m.className='msg err';m.textContent=t;};
  if(!(Number.isInteger(own)&&own>=1&&own<=16777215))return bad('Enter the talkgroup of this server (1 to 16777215).');
  if(!net)return bad('Choose the linked network.');
  if(!(Number.isInteger(tg)&&tg>=1&&tg<=16777215))return bad('Enter the talkgroup in the linked network (1 to 16777215).');
  const p={name:'',groups:[own],remote:[{network:net,gssi:tg}],both_ways:true,enabled:$('le-on').getAttribute('aria-checked')==='true',linked:true,on_demand:$('le-dyn').getAttribute('aria-checked')==='true'};
  p.name=($('le-name').value.trim()||autoLink(p)).slice(0,40);
  if(D.pools.some((q,j)=>j!==i&&q.linked&&q.groups[0]===own&&q.remote[0]&&q.remote[0].network===net&&q.remote[0].gssi===tg))return bad('This link exists already.');
  const l=D.pools.map(plain);if(i>=0)l[i]=p;else l.push(p);
  m.className='msg mut';m.textContent='Saving…';
  try{await postJ('/api/pools/save',l);closeSheet();D.pools=await getJ('/api/pools');setMsg('link-msg','ok',`${p.name} saved.`);render(true);}
  catch(e){bad(e.message);}
}
function togglePool(i){const l=D.pools.map(plain);l[i].enabled=!l[i].enabled;savePools(l,`${l[i].name} switched ${l[i].enabled?'on':'off'}.`,l[i].linked?'link-msg':'pool-msg');}
function delPool(i){const p=D.pools[i];if(!confirm(`Delete ${p.linked?'linked talkgroup':'pool'} "${p.name}"?`))return;const l=D.pools.map(plain);l.splice(i,1);savePools(l,`${p.name} deleted.`,p.linked?'link-msg':'pool-msg');}
function editPool(i){
  const p=i>=0?plain(D.pools[i]):{name:'',enabled:true,both_ways:true,groups:[],remote:[]};
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${i>=0?'Edit pool':'New pool'}</h3></div><div class="sb">
   <div><label class="l" for="pe-name">Name</label><input class="f" id="pe-name" maxlength="40" value="${esc(p.name)}" placeholder="e.g. District North"></div>
   <div><label class="l" for="pe-groups">Groups (GSSI), separated by commas or spaces</label><input class="f" id="pe-groups" value="${p.groups.join(', ')}" placeholder="2321, 2323, 2324, 264" autocomplete="off"></div>
   ${(D.nets&&D.nets.networks.length)||p.remote.length?`<div><label class="l" for="pe-remote">Groups of other networks (Multi Network), as network:GSSI</label><input class="f" id="pe-remote" value="${esc(p.remote.map(r=>r.network+':'+r.gssi).join(', '))}" placeholder="${esc(((D.nets&&D.nets.networks[0])||{name:'TetraPack'}).name)}:909" autocomplete="off"><div class="mut" style="font-size:12px;margin-top:4px">Makes the pool a bridge: a call on one of its groups is heard on the other network's groups and back, the talker's ISSI is passed on 1:1. ${D.nets?'Networks: '+D.nets.networks.map(n=>esc(n.name)).join(', '):''}</div></div>`:''}
   <div><span class="l" style="display:block;font-size:12px;font-weight:600;color:var(--text2)">Direction</span><div class="seg" role="radiogroup" id="pe-dir" style="margin-top:5px"><button role="radio" aria-pressed="${p.both_ways}" aria-checked="${p.both_ways}" onclick="setDir(true)">⇄ Both ways</button><button role="radio" aria-pressed="${!p.both_ways}" aria-checked="${!p.both_ways}" onclick="setDir(false)">→ One way, from the first group</button></div></div>
   ${(D.nets&&D.nets.networks.length)||p.remote.length?`<label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="pe-dyn" aria-checked="${!!p.on_demand}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Only on demand: the groups of other networks come only while a radio here is in one of the groups above (off: permanent, their calls always come)</span></label>`:''}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="pe-on" aria-checked="${p.enabled}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Switched on</label>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="savePool(${i})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('pe-name').focus(),30);
}
function setDir(b){document.querySelectorAll('#pe-dir button').forEach((x,j)=>{const on=(j===0)===b;x.setAttribute('aria-pressed',on);x.setAttribute('aria-checked',on);});}
function closeSheet(){$('scrim').classList.remove('on');$('sheet').classList.remove('full');UI.sdsFull=null;}
document.addEventListener('keydown',e=>{if(e.key==='Escape')closeSheet();});
async function savePool(i){
  const groups=$('pe-groups').value.split(/[\s,;]+/).filter(Boolean).map(Number);
  if(groups.some(g=>!Number.isInteger(g))){$('sheet-msg').className='msg err';$('sheet-msg').textContent='Groups must be numbers.';return;}
  const remote=[];
  for(const t of ($('pe-remote')?$('pe-remote').value:'').split(/[,;]+/).map(x=>x.trim()).filter(Boolean)){const k=t.lastIndexOf(':'),g=+t.slice(k+1);
    if(k<1||!Number.isInteger(g)){$('sheet-msg').className='msg err';$('sheet-msg').textContent=`"${t}" is not network:GSSI.`;return;}remote.push({network:t.slice(0,k).trim(),gssi:g});}
  const p={name:$('pe-name').value.trim(),groups,remote,both_ways:document.querySelector('#pe-dir button').getAttribute('aria-pressed')==='true',enabled:$('pe-on').getAttribute('aria-checked')==='true',on_demand:!!$('pe-dyn')&&$('pe-dyn').getAttribute('aria-checked')==='true'};
  const l=D.pools.map(plain);if(i>=0)l[i]=p;else l.push(p);
  const m=$('sheet-msg');m.className='msg mut';m.textContent='Saving…';
  try{await postJ('/api/pools/save',l);closeSheet();D.pools=await getJ('/api/pools');setMsg('pool-msg','ok',`${p.name} saved.`);render(true);}
  catch(e){m.className='msg err';m.textContent=e.message;}
}

/* ---------- multi network ---------- */
R.multinet=()=>{
  if(!D.who.admin)return `<div class="card"><div class="empty">Multi Network is for admins. <button class="btn" onclick="openLogin(false)">Log in</button></div></div>`;
  const M=D.nets;if(!M)return `<div class="card"><div class="empty">Loading…</div></div>`;
  const mni=(a,b)=>a||b?`${a}/${b}`:'<span class="mut">not set</span>';
  const rows=M.networks.map((n,i)=>{const st=!M.enabled?'<span class="st i"><i></i>Multi Network off</span>':!n.enabled?'<span class="st i"><i></i>switched off</span>':n.connected?`<span class="st"><i></i>connected · ${ago(n.since_ms)}</span>`:`<span class="st w"><i></i>not connected</span>${n.last_error?`<div class="mut" style="font-size:12px">${esc(n.last_error)}</div>`:''}`;
    return `<tr${n.enabled?'':' class="off"'}><td><button class="tog" role="switch" aria-checked="${n.enabled}" aria-label="Network ${esc(n.name)} on" onclick="toggleNet(${i})"></button></td>
     <td><b>${esc(n.name)}</b><div class="mut" style="font-size:12px">MNI ${mni(n.mcc,n.mnc)}</div></td><td>${st}</td>
     <td><code>${esc(n.remote_host)}${esc(n.path)}</code>${n.tls?' <span class="chip">TLS</span>':''}<div class="mut" style="font-size:12px">login ${esc(n.username)||'—'} · as “${esc(n.user_agent||M.default_user_agent)}”</div></td>
     <td>${n.bridge_issi||'<span class="mut">—</span>'}${n.register_talkers&&n.connected?`<div style="font-size:12px;margin-top:2px">${n.exported.length?`<button class="chip a" style="cursor:pointer" aria-expanded="${!!UI.open['net'+i]}" onclick="UI.open['net'+${i}]=!UI.open['net'+${i}];render(true)">${UI.open['net'+i]?'▾':'▸'} ${n.exported.length} radio${n.exported.length===1?'':'s'} registered there</button>`:'<span class="mut">no radios registered there</span>'}<span class="mut">${n.register_all?' (all)':''}${n.checks?' · ISSIs checked':''}</span></div>`:''}${n.visitors.length?`<div class="mut" style="font-size:12px">${n.visitors.length} visitor${n.visitors.length===1?'':'s'} here</div>`:''}</td><td>${n.bridged.map(g=>`<span class="tg">${g}</span>`).join(' ')||'<span class="mut">none</span>'}</td>
     <td style="white-space:nowrap"><button class="btn" onclick="editNet(${i})">Edit</button><button class="btn danger" onclick="delNet(${i})">Delete</button></td></tr>${UI.open['net'+i]&&n.connected&&n.exported.length?exportedRow(n):''}`;}).join('');
  return `<div class="card"><div class="ch"><h2>This server</h2>${helpBtn('mnself')}<span class="sp"></span><label style="display:flex;gap:8px;align-items:center;font-size:13px"><button class="tog" role="switch" aria-checked="${M.enabled}" aria-label="Multi Network on" onclick="toggleMn()"></button>Multi Network ${M.enabled?'on':'off'}</label><button class="btn" onclick="editSelf()">Edit</button></div>
   <div class="row"><div class="rm"><div class="rt">${esc(M.name)||'<span class="mut">no name</span>'}</div><div class="rs">MNI (MCC/MNC) ${mni(M.mcc,M.mnc)}</div></div></div>
   ${helpNote('mnself','The switch closes (off) or opens (on) all links to other networks at once; their settings and the pools stay. The network of this server: the stations connected here belong to it. The MNI is for the overview and for the next steps of Multi Network (radios of other networks on a station).')}</div>
  <div class="card"><div class="ch"><h2>Other networks</h2><span class="cnt">${M.networks.length}</span>${helpBtn('mnnets')}<span class="sp"></span><button class="btn pri" onclick="editNet(-1)">${ic('plus')}New network</button></div>
   ${helpNote('mnnets','This server connects to each network itself, as one Basestation with the login that network gave you, and introduces itself with the connection name you choose (empty: brew-server and its version). The login stays on this server; stations never get it. What crosses is set by the talkgroup pools on the <a data-go="routing" href="#routing">Routing</a> page: put a group of a network into a pool as <code>network:GSSI</code> (e.g. TG 9 here and TetraPack:909 there) and the pool becomes a bridge; the talker\'s ISSI is passed on 1:1. The bridge ISSI is registered in that network and listens to the bridged groups so its server sends their calls here. Changes apply at once, only a changed network reconnects, no restart.')}
   ${M.networks.length?`<div class="tw"><table><thead><tr><th style="width:52px">On</th><th>Network</th><th>Link</th><th>Server</th><th>Bridge ISSI</th><th>Bridged groups</th><th></th></tr></thead><tbody>${rows}</tbody></table></div>`:'<div class="empty">No other networks yet.</div>'}
   ${msgDiv('net-msg')}</div>${visitorCard(M)}${conflictCard(M)}`;
};
// The radios a link registered in that network (expanded under its row). Brew has no answer to a
// registration: this is what this server sent there, not a confirmation from that network.
function exportedRow(n){
  const at=new Map(((D.conn&&D.conn.mobile_stations)||[]).map(m=>[m.issi,m]));
  const l=n.exported_detail||n.exported.map(issi=>({issi,groups:[],since_ms:0}));
  return `<tr class="exp"><td colspan="7"><div class="tw"><table><thead><tr><th>ISSI</th><th>Station here</th><th>Groups in ${esc(n.name)}</th><th>Registered there</th></tr></thead><tbody>${l.map(x=>{const m=at.get(x.issi);
   return `<tr><td>${issiCell(x.issi)}</td><td>${m?`${m.cell&&m.cell.name?`<span class="chip">${esc(m.cell.name)}</span>`:`<b class="m">${esc(m.basestation_name||'—')}</b>`}${infoOf(m.basestation_name)?`<span class="sub">${esc(infoOf(m.basestation_name))}</span>`:''}`:'<span class="mut">—</span>'}</td><td>${x.groups.map(g=>`<span class="tg">${g}</span>`).join(' ')||'<span class="mut">none</span>'}</td><td class="m">${x.since_ms?`${tmd(x.since_ms)} <span class="mut">(${ago(x.since_ms)})</span>`:'—'}</td></tr>`;}).join('')}</tbody></table></div>
   <div class="mut" style="font-size:12px;margin-top:6px">What this server registered in ${esc(n.name)}. Brew does not confirm a registration, so whether that network took it shows only on its side (its dashboard, or a call from there reaching the radio).</div></td></tr>`;
}
// Radios of other networks on this server's stations (extension mni).
function visitorCard(M){
  const l=M.networks.flatMap(n=>n.visitors.map(v=>({...v,net:n.name})));
  if(!l.length)return '';
  return `<div class="card"><div class="ch"><h2>Visitors</h2><span class="cnt">${l.length}</span>${helpBtn('mnvis')}</div>
   ${helpNote('mnvis','Radios of another network on a station of this server. The station says which network a radio comes from (stations with Multi Network, extension mni); this server registers it in its home network with its groups, so its home network\'s group calls reach it here and its calls on those groups go there. A radio with the same ISSI in this network is not affected.')}
   <div class="tw"><table><thead><tr><th>Network</th><th>ISSI</th><th>Station</th><th>Groups there</th><th>Since</th></tr></thead><tbody>${l.map(v=>`<tr><td>${esc(v.net)}</td><td class="m">${v.issi}</td><td>${esc(v.station)}</td><td>${v.groups.map(g=>`<span class="tg">${g}</span>`).join(' ')||'<span class="mut">none</span>'}</td><td class="m">${tmd(v.since_ms)}</td></tr>`).join('')}</tbody></table></div></div>`;
}
// ISSI collisions the links saw (newest first), with a never-export button.
function conflictCard(M){
  const l=M.networks.flatMap((n,i)=>n.conflicts.map(c=>({...c,i,net:n.name,never:n.never_export.includes(c.issi)}))).sort((a,b)=>b.at_ms-a.at_ms);
  if(!l.length)return '';
  return `<div class="card"><div class="ch"><h2>ISSI conflicts</h2><span class="cnt">${l.length}</span>${helpBtn('mnconf')}</div>
   ${helpNote('mnconf','An ISSI of this server that the other network has as well. A brew-server there is asked before a radio is registered (taken: not registered). Other servers cannot be asked: a radio there talking with an ISSI this server has registered there shows up here. Put such an ISSI on the never-export list of that network.')}
   <div class="tw"><table><thead><tr><th>Time</th><th>Network</th><th>ISSI</th><th>What</th><th></th></tr></thead><tbody>${l.slice(0,100).map(c=>`<tr><td class="m">${tmd(c.at_ms)}</td><td>${esc(c.net)}</td><td>${issiCell(c.issi)}</td><td>${esc(c.what)}</td><td>${c.never?'<span class="mut">never exported</span>':`<button class="btn" onclick="neverNet(${c.i},${c.issi})">Never export</button>`}</td></tr>`).join('')}</tbody></table></div></div>`;
}
function neverNet(i,issi){const c=mnPlain();const n=c.networks[i];if(!n.never_export.includes(issi))n.never_export.push(issi);saveNets(c,`${issi} is never registered in ${n.name} now.`);}
const netPlain=n=>({name:n.name,enabled:n.enabled,mcc:n.mcc,mnc:n.mnc,remote_host:n.remote_host,path:n.path,username:n.username,password:'',user_agent:n.user_agent,bridge_issi:n.bridge_issi,register_talkers:n.register_talkers!==false,register_all:!!n.register_all,export_stations:[...(n.export_stations||[])],never_export:[...(n.never_export||[])],never_forward:[...(n.never_forward||[])],forward_unknown:!!n.forward_unknown,forward_lip:n.forward_lip!==false,refresh_seconds:n.refresh_seconds??300,refresh_rejoin:!!n.refresh_rejoin,reconnect_interval_seconds:n.reconnect_interval_seconds,tls:n.tls,tls_server_name:n.tls_server_name,tls_ca_path:n.tls_ca_path,tls_pinned_cert_path:n.tls_pinned_cert_path});
const mnPlain=()=>({enabled:D.nets.enabled,name:D.nets.name,mcc:D.nets.mcc,mnc:D.nets.mnc,networks:D.nets.networks.map(netPlain)});
async function saveNets(cfg,okText,sheet){
  const m=sheet?$('sheet-msg'):null;if(m){m.className='msg mut';m.textContent='Saving…';}
  try{await postJ('/api/networks/save',cfg);if(sheet)closeSheet();D.nets=await getJ('/api/networks');setMsg('net-msg','ok',okText);render(true);return true;}
  catch(e){if(m){m.className='msg err';m.textContent=e.message;}else{setMsg('net-msg','err',e.message);render(true);}return false;}
}
function toggleMn(){const c=mnPlain();c.enabled=!c.enabled;if(!c.enabled&&!confirm('Switch Multi Network off? All links to other networks close; settings and pools stay.'))return;saveNets(c,`Multi Network switched ${c.enabled?'on':'off'}.`);}
function toggleNet(i){const c=mnPlain();c.networks[i].enabled=!c.networks[i].enabled;saveNets(c,`${c.networks[i].name} switched ${c.networks[i].enabled?'on':'off'}.`);}
function delNet(i){const c=mnPlain();const n=c.networks[i];if(!confirm(`Delete network "${n.name}"? Remove its groups from the pools first.`))return;c.networks.splice(i,1);saveNets(c,`${n.name} deleted.`);}
const fld=(id,label,val,extra)=>`<div><label class="l" for="${id}">${label}</label><input class="f" id="${id}" value="${esc(val)}" ${extra||''}></div>`;
function editSelf(){const M=D.nets;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">This server</h3></div><div class="sb">
   ${fld('ms-name','Network name',M.name,'maxlength="40" placeholder="e.g. TMO-Austria"')}
   <div style="display:flex;gap:10px">${fld('ms-mcc','MCC',M.mcc||'','placeholder="901"')}${fld('ms-mnc','MNC',M.mnc||'','placeholder="9999"')}</div>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveSelf()">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('ms-name').focus(),30);}
function saveSelf(){const c=mnPlain();c.name=v('ms-name').trim();c.mcc=+v('ms-mcc')||0;c.mnc=+v('ms-mnc')||0;saveNets(c,'Saved.',true);}
function editNet(i){
  const n=i>=0?netPlain(D.nets.networks[i]):{name:'',enabled:true,mcc:0,mnc:0,remote_host:'',path:'/brew',username:'',password:'',user_agent:'',bridge_issi:0,register_talkers:true,register_all:false,export_stations:[],never_export:[],never_forward:[],forward_unknown:false,forward_lip:true,reconnect_interval_seconds:15,tls:false,tls_server_name:'',tls_ca_path:'/etc/ssl/certs/ca-certificates.crt',tls_pinned_cert_path:''};
  const hasPw=i>=0&&D.nets.networks[i].has_password;
  const hp=splitHost(n.remote_host);
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${i>=0?'Edit network':'New network'}</h3></div><div class="sb">
   ${fld('ne-name','Name (pools refer to it)',n.name,'maxlength="40" placeholder="e.g. TetraPack"'+(i>=0&&D.pools.some(p=>(p.remote||[]).some(r=>r.network===n.name))?' readonly title="Used by a pool"':''))}
   <div style="display:flex;gap:10px">${fld('ne-mcc','MCC',n.mcc||'','placeholder="901"')}${fld('ne-mnc','MNC',n.mnc||'','placeholder="9000"')}</div>
   <div style="display:flex;gap:10px"><div style="flex:3">${fld('ne-host','Server (host)',hp.host,'placeholder="brew.example.net" autocomplete="off"')}</div><div style="flex:1">${fld('ne-port','Port',hp.port,'placeholder="443 / 9000"')}</div></div>
   ${fld('ne-path','Path',n.path,'placeholder="/brew"')}
   <div style="display:flex;gap:10px">${fld('ne-user','Login (Basestation ID there)',n.username,'autocomplete="off"')}${fld('ne-pw','Password',hasPw?'':'','type="password" autocomplete="new-password" placeholder="'+(hasPw?'unchanged':'')+'"')}</div>
   ${fld('ne-ua','Connection name (how this server introduces itself there)',n.user_agent,'maxlength="80" placeholder="'+esc(D.nets.default_user_agent)+'"')}
   ${fld('ne-issi','Bridge ISSI in that network (0 = none)',n.bridge_issi,'')}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-reg" aria-checked="${n.register_talkers}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Register radios there (those in a bridged group here, like a base station)</label>
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-all" aria-checked="${n.register_all}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Register all: also every radio of stations with Multi Network (mni) or listed below</label>
   ${fld('ne-exp','Also export the radios of these stations, and these apps (station or app logins, or app ISSIs; comma separated)',n.export_stations.join(', '),'autocomplete="off"')}
   ${fld('ne-never','Never register these ISSIs there (comma separated)',n.never_export.join(', '),'autocomplete="off"')}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-fwd" aria-checked="${n.forward_unknown}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Forward unknown targets: an individual call or SDS nobody here takes goes to this network<span class="mut" style="display:block;font-size:12px;margin-top:2px">The SMS Center does not keep these: an SDS to a radio not registered here goes there instead of being stored and resent here.</span></span></label>
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-lip" aria-checked="${n.forward_lip}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Forward LIP: position reports cross to and from this network<span class="mut" style="display:block;font-size:12px;margin-top:2px">Off: SDS with a location protocol identifier (0x03, 0x0A, 0x83) stay on this side; text and other SDS still pass.</span></span></label>
   <div style="border:1px solid var(--border);border-radius:10px;padding:10px 12px;display:flex;flex-direction:column;gap:8px">
    <div style="font-size:13px;font-weight:600">Registration refresh</div>
    <div class="mut" style="font-size:12px">Brew has no answer to a registration. So the registrations of the bridge ISSI and the radios are sent to that network again from time to time.</div>
    <div style="display:flex;gap:8px;align-items:center"><label for="ne-refresh" style="font-size:13px">Every</label><input class="f" id="ne-refresh" style="width:90px" value="${n.refresh_seconds??300}" placeholder="300" oninput="neRefreshUi()"><span style="font-size:13px">seconds</span><span class="mut" style="font-size:12px">(0 = off, at least 30)</span></div>
    <label id="ne-rejoin-row" style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-rejoin" aria-checked="${!!n.refresh_rejoin}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Also rejoin the groups<br><span class="mut" style="font-size:12px">At each refresh the bridge ISSI (without one: one radio at a time) leaves its groups there and joins them again, only while no call is running. All others stay in. Use it when that network stops sending a group's calls.</span></span></label>
   </div>
   ${fld('ne-nofwd','Do not forward these ISSIs: no individual call or SDS to or from them crosses to this network (comma separated)',n.never_forward.join(', '),'autocomplete="off"')}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-tls" aria-checked="${n.tls}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> TLS (wss://)</label>
   ${fld('ne-sni','TLS server name (empty: host)',n.tls_server_name)}
   ${fld('ne-pin','Pinned certificate file (self-signed server, empty: off)',n.tls_pinned_cert_path)}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-on" aria-checked="${n.enabled}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Connect</label>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveNet(${i})">Save</button></div>`;
  $('scrim').classList.add('on');neRefreshUi();setTimeout(()=>$('ne-name').focus(),30);
}
// The rejoin switch only acts with a refresh: greyed out at 0.
function neRefreshUi(){const off=v('ne-refresh').trim()==='0',r=$('ne-rejoin-row');if(!r)return;r.style.opacity=off?.45:1;$('ne-rejoin').disabled=off;}
// "host:port", "[v6]:port" or a bare host -> {host, port}.
function splitHost(h){h=h||'';const m=h.match(/^\[([^\]]*)\](?::(\d*))?$/);if(m)return {host:m[1],port:m[2]||''};
  const k=h.lastIndexOf(':');if(k>0&&h.indexOf(':')===k)return {host:h.slice(0,k),port:h.slice(k+1)};return {host:h,port:''};}
function saveNet(i){
  const host=v('ne-host').trim().replace(/^(wss?|https?):\/\//,'').replace(/\/.*$/,''),port=v('ne-port').trim(),m=$('sheet-msg');
  if(host&&!(/^\d+$/.test(port)&&+port>=1&&+port<=65535)){m.className='msg err';m.textContent='Enter the port (1 to 65535), e.g. 443 with TLS or 9000.';$('ne-port').focus();return;}
  const remote_host=host?(host.includes(':')?'['+host+']':host)+':'+port:'';
  const rs=v('ne-refresh').trim(),refresh_seconds=rs===''?300:+rs;
  if(!(Number.isInteger(refresh_seconds)&&(refresh_seconds===0||(refresh_seconds>=30&&refresh_seconds<=86400)))){m.className='msg err';m.textContent='Refresh: 0 (never) or 30 to 86400 seconds.';$('ne-refresh').focus();return;}
  const c=mnPlain();const old=i>=0?c.networks[i]:{reconnect_interval_seconds:15,tls_ca_path:'/etc/ssl/certs/ca-certificates.crt'};
  const n=Object.assign({},old,{name:v('ne-name').trim(),mcc:+v('ne-mcc')||0,mnc:+v('ne-mnc')||0,remote_host,path:v('ne-path').trim()||'/brew',
    username:v('ne-user').trim(),password:v('ne-pw'),user_agent:v('ne-ua').trim(),bridge_issi:+v('ne-issi')||0,register_talkers:$('ne-reg').getAttribute('aria-checked')==='true',register_all:$('ne-all').getAttribute('aria-checked')==='true',
    export_stations:v('ne-exp').split(',').map(x=>x.trim()).filter(Boolean),never_export:v('ne-never').split(/[\s,]+/).filter(Boolean).map(Number),never_forward:v('ne-nofwd').split(/[\s,]+/).filter(Boolean).map(Number),forward_unknown:$('ne-fwd').getAttribute('aria-checked')==='true',forward_lip:$('ne-lip').getAttribute('aria-checked')==='true',
    refresh_seconds,refresh_rejoin:$('ne-rejoin').getAttribute('aria-checked')==='true',
    tls:$('ne-tls').getAttribute('aria-checked')==='true',tls_server_name:v('ne-sni').trim(),tls_pinned_cert_path:v('ne-pin').trim(),
    enabled:$('ne-on').getAttribute('aria-checked')==='true'});
  if(i>=0)c.networks[i]=n;else c.networks.push(n);
  saveNets(c,`${n.name} saved.`,true);
}

/* ---------- accounts ---------- */
const svcChip=(name,s)=>!s?'':`<span class="chip ${!s.enabled?'':s.open?'w':'a'}">${name}: ${!s.enabled?'switched off':s.open?'open, no login needed':'login required'}</span>`;
const yn=b=>b?'<span class="yes yn" title="yes">✓<span class="hide-s"> yes</span><span class="sr">yes</span></span>':'<span class="no yn" title="no">—<span class="hide-s"> no</span><span class="sr">no</span></span>';
R.accounts=()=>{
  const adm=D.who.admin;
  const a=D.acc;
  if(!a)return '<div class="card"><div class="empty">Loading…</div></div>';
  const q=UI.filter.acc||'';
  const byInfo=(UI.filter.accv||'login')==='info';
  const rows=a.accounts.filter(x=>match(q,x.username,x.info));
  return `<div class="card"><div class="ch"><h2>Station and terminal logins</h2><span class="mut">${a.accounts.length}</span>${helpBtn('acc')}<span class="sp"></span>
    ${seg('accv',[['login','Login'],['info','Info']])}
    <div class="search">${ic('search')}<input class="f" placeholder="Login or info…" value="${esc(q)}" oninput="UI.filter.acc=this.value;render(true);const i=document.querySelector('.search input');i.focus();i.setSelectionRange(i.value.length,i.value.length)" aria-label="Filter logins"></div>
    ${adm?`<button class="btn pri" onclick="editAcc(-1)">${ic('plus')}New login</button>`:''}</div>
   ${adm?`<div class="svc" style="justify-content:space-between"><label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600"><button class="tog" role="switch" id="acc-auth" aria-checked="${!a.brew.open}" onclick="setBrewAuth(this.getAttribute('aria-checked')!=='true')"></button> Authentication required</label><span class="mut" style="font-size:12px">${a.brew.open?'<span class="chip d">Off</span> Any station or app connects without a login.':'Stations and apps connect only with a Brew login from this list.'}</span></div>`:''}
   ${helpNote('acc','The logins Basestations and terminals (apps, consoles) use to connect to this server. <b>Brew</b> carries calls and SDS; the login is the station\'s or terminal\'s number (1 to 7 digits). <b>Telemetry</b> and <b>control</b> are the FlowStation links for status reports and commands (DGNA, kick, restart). One login can be on all three. Changes apply to the next connection at once, without a restart, and are written to the config file. Connections that are already up stay until they reconnect. Careful: while the telemetry or control list is empty, that link needs no login; the first login you add there turns the login on, and stations without one are refused from then on.')}
   <div class="svc">${svcChip('Brew',a.brew)}${svcChip('Telemetry',a.telemetry)}${svcChip('Control',a.control)}</div>
   <div class="tw acc"><table><thead><tr><th>${byInfo?'Info':'Login'}</th><th>Brew</th><th>Telemetry</th><th>Control</th><th class="hide-s">Position</th><th></th></tr></thead><tbody>
   ${rows.map(x=>{const i=a.accounts.indexOf(x);const first=byInfo&&x.info?`<b>${esc(x.info)}</b>`:`<b class="m">${esc(x.username)}</b>`;
     const ko=kindOver(x.username);
     return `<tr><td>${first}${ko?` ${tKindChip(x.username)}`:''}${byInfo?(x.info?`<span class="sub m">${esc(x.username)}</span>`:'<span class="sub">no info</span>'):''}</td><td>${yn(x.brew)}</td><td>${yn(x.telemetry)}</td><td>${yn(x.control)}</td><td class="hide-s">${x.lat!=null?`<span class="m" style="font-size:12px">${x.lat.toFixed(4)}, ${x.lon.toFixed(4)}</span>`:'<span class="no">—</span>'}${x.icon?` <span class="chip i">${x.icon==='tbs'?'Tower':'Terminal'}</span>`:''}</td>
     <td style="text-align:right;white-space:nowrap">${adm?`<button class="btn" onclick="editAcc(${i})">Edit</button> <button class="btn danger hide-s" onclick="delAcc(${i})">Delete</button>`:''}</td></tr>`;}).join('')||'<tr><td colspan="6" class="mut">No logins</td></tr>'}
   </tbody></table></div>${msgDiv('acc-msg','padding:12px 18px')}</div>`;
};
function editAcc(i){
  const x=i>=0?D.acc.accounts[i]:{username:'',brew:true,telemetry:true,control:true,info:'',lat:null,lon:null,icon:null};
  const sw=(id,on,label,hint)=>`<label style="display:flex;gap:12px;align-items:center"><button type="button" class="tog" role="switch" id="${id}" aria-checked="${on}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button><span><b style="font-size:13.5px">${label}</b><br><span class="mut" style="font-size:12px">${hint}</span></span></label>`;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${i>=0?'Edit login '+esc(x.username):'New login'}</h3></div><div class="sb">
   <div><label class="l" for="ac-user">Username</label><input class="f" id="ac-user" value="${esc(x.username)}" ${i>=0?'disabled':''} placeholder="e.g. 1000003" autocomplete="off" spellcheck="false"></div>
   <div><label class="l" for="ac-pw">Password</label><div style="display:flex;gap:8px"><input class="f" id="ac-pw" type="text" autocomplete="new-password" spellcheck="false" placeholder="${i>=0?'unchanged':'password'}" style="flex:1"><button type="button" class="btn" onclick="$('ac-pw').value=genPw()">Generate</button></div></div>
   <div><label class="l" for="ac-info">Info (optional)</label><input class="f" id="ac-info" maxlength="60" value="${esc(x.info||'')}" placeholder="e.g. Relay Main Square · Sysop OE4XYZ"></div>
   <div><label class="l" for="ac-pos">Position (optional): latitude, longitude</label><div style="display:flex;gap:8px"><input class="f" id="ac-pos" value="${x.lat!=null?x.lat+', '+x.lon:''}" placeholder="48.2082, 16.3738" autocomplete="off" spellcheck="false" style="flex:1"><button type="button" class="btn" onclick="hereFix()" title="Use the position of this device">${ic('pin')}Here</button></div><div class="mut" style="font-size:12px;margin-top:4px">Shown on the map. Paste it from a map app, or take this device's position.</div></div>
   <div><label class="l" for="ac-icon">Map symbol</label><select class="f" id="ac-icon">${[['tmo','Station (TMO)'],['tbs','Tower (TBS)'],['terminal','Terminal']].map(([v,l])=>`<option value="${v}"${(x.icon||'tmo')===v?' selected':''}>${l}</option>`).join('')}</select><div class="mut" style="font-size:12px;margin-top:4px">How the login is drawn on the map at its position: a TMO station, a tower (TBS), or a terminal (app, console). A login connected as Terminal is drawn as a terminal unless another symbol is set.</div></div>
   ${sw('ac-b',x.brew,'Brew','Calls and SDS. Username = number, 1 to 7 digits')}
   ${sw('ac-t',x.telemetry,'Telemetry','FlowStation status reports')}
   ${sw('ac-c',x.control,'Control','Commands to the station: DGNA, kick, restart')}
   <div class="mut" style="font-size:12px">Applies to the next connection at once, no restart.</div>
   ${kindSeg(x.username)}
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts">${i>=0?`<button class="btn danger" style="margin-right:auto" onclick="closeSheet();delAcc(${i})">${ic('trash')}Delete</button>`:''}<button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveAcc(${i})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>(i>=0?$('ac-pw'):$('ac-user')).focus(),30);
}
function hereFix(){
  const m=$('sheet-msg');
  if(!navigator.geolocation){m.className='msg err';m.textContent='This browser cannot give a position (it needs HTTPS).';return;}
  m.className='msg mut';m.textContent='Getting the position…';
  navigator.geolocation.getCurrentPosition(p=>{$('ac-pos').value=p.coords.latitude.toFixed(5)+', '+p.coords.longitude.toFixed(5);m.textContent='';},
    e=>{m.className='msg err';m.textContent='No position: '+(e.code===1?'permission denied':e.message)+'. Over plain HTTP browsers refuse; paste it instead.';},{enableHighAccuracy:true,timeout:15000});
}
function changePw(){
  const f=(id,l,ac)=>`<div><label class="l" for="${id}">${l}</label><input class="f" id="${id}" type="password" autocomplete="${ac}" spellcheck="false"></div>`;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Change password</h3></div><div class="sb">
   <div class="mut" style="font-size:12.5px">For <b>${esc(D.who.username||'')}</b>. You stay signed in here; other browsers signed in as you are signed out.</div>
   ${f('pw-cur','Current password','current-password')}${f('pw-new','New password (at least 6 characters)','new-password')}${f('pw-rep','New password again','new-password')}
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="savePw()">Change</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('pw-cur').focus(),30);
}
async function savePw(){
  const m=$('sheet-msg'),cur=$('pw-cur').value,nw=$('pw-new').value;
  const bad=t=>{m.className='msg err';m.textContent=t;};
  if(nw.length<6)return bad('The new password needs at least 6 characters.');
  if(nw!==$('pw-rep').value)return bad('The new passwords do not match.');
  m.className='msg mut';m.textContent='Saving…';
  try{
    const r=await fetch('/api/password',{method:'POST',cache:'no-store',headers:{'x-brew-ui':'1','content-type':'application/json'},body:JSON.stringify({current:cur,new:nw})});
    if(!r.ok)return bad((await r.text())||String(r.status));
  }catch(e){return bad(e.message);}
  m.className='msg ok';m.textContent='Password changed.';
  document.querySelector('#sheet .acts').innerHTML='<button class="btn pri" onclick="closeSheet()">Close</button>';
}
function genPw(){const c='ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789';const a=new Uint32Array(16);crypto.getRandomValues(a);return [...a].map(n=>c[n%c.length]).join('');}
const on=id=>$(id).getAttribute('aria-checked')==='true';
async function saveAcc(i){
  const m=$('sheet-msg');
  const pos=$('ac-pos').value.trim();let lat=null,lon=null;
  if(pos){const n=pos.split(/[\s,;]+/).filter(Boolean).map(Number);if(n.length!==2||n.some(v=>!isFinite(v))){m.className='msg err';m.textContent='Position: two numbers, latitude and longitude, e.g. 48.2082, 16.3738.';return;}[lat,lon]=n;}
  const body={username:$('ac-user').value.trim(),password:$('ac-pw').value,brew:on('ac-b'),telemetry:on('ac-t'),control:on('ac-c'),info:$('ac-info').value.trim(),lat,lon,icon:$('ac-icon').value};m.className='msg mut';m.textContent='Saving…';
  // The type (Auto / Terminal / Bridge) is a dashboard setting of the number ([issi_kinds]), saved after the login.
  const kind=kindPicked(),kindChanged=!!kind&&kind!==(kindOver(body.username)||'auto');
  if(kindChanged&&!/^\d{1,12}$/.test(body.username)){m.className='msg err';m.textContent='Type: only for a numeric login (up to 12 digits). Leave it on Auto.';return;}
  try{D.acc=await postJ('/api/accounts',body);
    if(kindChanged)D.ikind=await postJ('/api/issi-kinds/set',{issi:body.username,kind});
    closeSheet();setMsg('acc-msg','ok',`${body.username} saved. It works for the next connection.`);render(true);}
  catch(e){m.className='msg err';m.textContent=e.message;}
}
async function setBrewAuth(on){
  if(!on&&!confirm('Switch authentication off?\n\nAny station or app can then connect to this server without a login, and stations appear without their name (login). Connected stations stay connected.'))return render(true);
  try{D.acc=await api('/api/accounts/auth',{method:'PUT',headers:{'content-type':'application/json'},body:JSON.stringify({enabled:on})});
    setMsg('acc-msg',on?'ok':'err',on?'Authentication on: stations and apps need a login from the next connection.':'Authentication off: any station or app can connect without a login.');}
  catch(e){setMsg('acc-msg','err',e.message);}
  render(true);
}
async function delAcc(i){
  const x=D.acc.accounts[i];if(!confirm(`Delete login "${x.username}" from Brew, telemetry and control?`))return;
  try{D.acc=await api('/api/accounts/'+encodeURIComponent(x.username),{method:'DELETE'});setMsg('acc-msg','ok',`${x.username} deleted.`);}catch(e){setMsg('acc-msg','err',e.message);}
  render(true);
}

/* ---------- directory: ISSI and GSSI names ---------- */
const DIR={issi:{api:'/api/issi-names',what:'ISSI',who:'radio',ex:'2320972',exn:'OE4SXM Sebastian',ex2:'2321100, Relay North',file:'issi-directory.csv'},
  gssi:{api:'/api/gssi-names',what:'GSSI',who:'group',ex:'2321',exn:'District North',ex2:'2322, District South',file:'gssi-directory.csv'}};
const dirK=()=>UI.filter.dirk==='gssi'?'gssi':'issi';
// Badges for the access entry of a group ([group_access]): "Only N ISSIs" / "N blocked" and "RX only"; '' when open.
function accChips(a){if(!a)return '';const n=a.issis.length;
  const join=!n?'':a.mode==='block'?`<span class="chip w" title="${a.issis.join(', ')}">${n} blocked</span>`:`<span class="chip a" title="${a.issis.join(', ')}">Only ${n} ISSI${n===1?'':'s'}</span>`;
  const rx=!a.rx_only?'':a.rx_only_issis.length?`<span class="chip i" title="${a.rx_only_issis.join(', ')}">${a.rx_only_issis.length} RX only</span>`:'<span class="chip i">RX only</span>';
  return [join,rx].filter(Boolean).join(' ');}
const onlyThese=a=>!!(a&&a.mode!=='block'&&a.issis.length);
const dirNames=()=>D[dirK()]||{};
R.issis=()=>{
  const adm=D.who.admin;const K=dirK(),d=DIR[K];const q=UI.filter.issi||'';const f=UI.filter.issif||'all';
  const names=dirNames();const ms=(D.conn&&D.conn.mobile_stations)||[];
  let seen=new Map();
  if(K==='issi')seen=new Map(ms.map(m=>[String(m.issi),m]));
  else{for(const m of ms)for(const g of (m.groups||[])){const k=String(g);seen.set(k,(seen.get(k)||0)+1);}
    for(const c of ((D.status&&D.status.active_calls)||[]))if(c.kind==='group'&&!seen.has(String(c.destination)))seen.set(String(c.destination),0);
    // Groups of other networks (aliases above the 24 GSSI bits) belong to their link, not here.
    for(const k of [...seen.keys()])if(+k>16777215)seen.delete(k);}
  const acc=K==='gssi'?(D.access||{}):{};
  const tg=D.tg||{},hid=new Set(K==='gssi'?(tg.hidden||[]).map(String):[]);
  const keys=[...new Set([...Object.keys(names),...seen.keys(),...Object.keys(acc),...hid])].sort((a,b)=>a-b);
  const plain=k=>!names[k]&&!acc[k];
  const rows=keys.filter(k=>(f==='hidden'?hid.has(k):!(hid.has(k)&&plain(k)))&&(f==='all'||f==='hidden'||(f==='named'&&names[k])||(f==='unnamed'&&!names[k])||(f==='limited'&&acc[k]))&&match(q,k,names[k],...((acc[k]||{}).issis||[])));
  const accCell=k=>accChips(acc[k])||'<span class="mut">Open</span>';
  const where=K==='issi'?((m,k)=>(m?`<span class="st"><i></i>${esc(m.cell?m.cell.name:(m.basestation_name||'registered'))}</span>`:'<span class="st i"><i></i>not registered</span>')+((m&&m.mode==='Terminal')||kindOver(k)?' '+tKindChip(k):''))
    :(n=>n===undefined?'<span class="st i"><i></i>no radios</span>':`<span class="st"><i></i>${n?n+' radio'+(n===1?'':'s'):'call running'}</span>`);
  const help=K==='issi'?'Names for ISSIs, for example the call sign and first name of a radio\'s user. They show next to the ISSI on Overview, Calls, SDS, Radios, Roaming and Routing. The login of a bridge (Brew login in Network mode, up to 12 digits, for example a link to another network) can be named here too; the name shows under the login on Stations &amp; Cells. Terminals (apps) and base stations are named on the Accounts page. Radios registered right now are listed too, so you can name them. Changes apply at once, without a restart, and are written to the config file (<code>[issi_names]</code>).'
    :'Names for groups (GSSI), for example the area or the purpose of a talkgroup. They show next to the group on Overview, Calls, Radios and Routing (a name a radio reports for its group comes first). Groups radios are on right now are listed too, so you can name them.<br><br><b>Access</b> decides who may use a group: <i>Open</i> (everyone), <i>Only these</i> (an allow list: only the listed ISSIs) or <i>Block these</i> (everyone but the listed ISSIs). <b>RX only</b> makes radios listeners: all of them with an empty list, or the listed ones (with <i>Only these</i>, listed listeners missing from the allow list are added to it on save); who may talk is then who is admitted and not listen-only. The server records an affiliation only for an admitted radio and drops a group call from a radio that may not talk (emergency calls always pass). A Basestation that supports it (FlowStation with <code>group-access</code>) gets the lists and refuses the attachment or the request to talk on the air itself; on other Basestations the radio can still attach and talk locally, but not across the network. <br><br>Which groups may be used at all (<b>managed talkgroups</b>) is set on the Routing page; naming a group here does not allow it, but a group with <i>Only these</i> counts as allowed there.<br><br><b>✕</b> hides a group without a name from this page while radios are on it (display only); the filter <i>Hidden</i> shows it again. Changes apply at once, without a restart, and are written to the config file (<code>[gssi_names]</code>, <code>[group_access]</code>, <code>[talkgroups]</code>).';
  return `<div class="bar">${seg('dirk',[['issi','Radios (ISSI)'],['gssi','Groups (GSSI)']])}${seg('issif',[['all','All'],['named','With name'],['unnamed','Without name']].concat(K==='gssi'?[['limited','Limited']].concat(hid.size?[['hidden','Hidden ('+hid.size+')']]:[]):[]))}${searchBox('issi',d.what+' or name…')}<span class="sp"></span>
    ${adm?`<button class="btn" onclick="exportNames()">Export</button><button class="btn" onclick="importNames()">Import list</button><button class="btn pri" onclick="editName('')">${ic('plus')}${K==='gssi'?'Add group':'Add name'}</button>`:''}</div>
  ${K==='issi'?csCard(adm):''}
  <div class="card"><div class="ch"><h2>${K==='issi'?'Radios':'Groups'}</h2><span class="cnt">${Object.keys(names).length} names</span>${helpBtn('dir'+K)}</div>
   ${helpNote('dir'+K,help)}
   <div class="tw"><table><thead><tr><th>${d.what}</th><th>Name</th>${K==='gssi'?'<th>Access</th>':''}<th class="hide-s">Now</th>${adm?'<th></th>':''}</tr></thead><tbody>
   ${rows.slice(0,1000).map(k=>`<tr><td><b class="m">${k}</b></td><td>${names[k]?esc(names[k]):K==='issi'&&(D.cs||{})[k]?`<span class="mut" title="From the ham callsign database (a Directory name comes first)">${esc(D.cs[k])} · database</span>`:'<span class="mut">—</span>'}</td>${K==='gssi'?`<td>${accCell(k)}</td>`:''}<td class="hide-s">${where(seen.get(k),k)}</td>
     ${adm?`<td style="text-align:right;white-space:nowrap">${names[k]?`<button class="btn" onclick="editName('${k}')">Edit</button> <button class="btn danger" onclick="delName('${k}')" aria-label="Delete name">${ic('trash')}<span class="hide-s">Delete</span></button>`:acc[k]?`<button class="btn" onclick="editName('${k}')">Edit</button>`:`<button class="btn" onclick="editName('${k}')">${ic('plus')}Name</button>`}${K==='gssi'&&plain(k)?(hid.has(k)?` <button class="btn" onclick="hideGroup('${k}',false)">Show</button>`:` <button class="btn" onclick="hideGroup('${k}',true)" aria-label="Hide group ${k} from this page" title="Hide from this page">✕</button>`):''}</td>`:''}</tr>`).join('')||`<tr><td colspan="5" class="mut">${keys.length?'Nothing matches':'No names yet'}</td></tr>`}
   </tbody></table></div>${rows.length>1000?'<div class="msg mut" style="padding:10px 18px">First 1000 shown, use the search.</div>':''}${msgDiv('issi-msg','padding:10px 18px')}</div>`;
};
// The Directory dialog; from the Routing page it opens with kind 'gssi' (name, access, RX only).
function editName(k,kind){
  const K=kind||dirK(),d=DIR[K];UI.edK=K;UI.edFrom=kind?'tg-msg':'issi-msg';const n=(D[K]||{})[k]||'';const a=K==='gssi'?(D.access||{})[k]:null;
  const am=a&&a.issis.length?a.mode:'open';
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${K==='gssi'?(k?'Group '+k:'Add group'):(k?(n?'Edit name':'Name for '+k):'Add name')}</h3></div><div class="sb">
   <div><label class="l" for="in-issi">${d.what}</label><input class="f" id="in-issi" value="${k}" ${k?'disabled':''} inputmode="numeric" placeholder="e.g. ${d.ex}"></div>
   <div><label class="l" for="in-name">Name${K==='gssi'?' (optional)':''}</label><input class="f" id="in-name" maxlength="60" value="${esc(n)}" placeholder="e.g. ${esc(d.exn)}"></div>
   ${K==='issi'?kindSeg(k):''}
   ${K==='gssi'?`<div><span class="l" style="display:block;font-size:12px;font-weight:600;color:var(--text2)">Access</span><div class="seg" role="radiogroup" id="in-acc" style="margin-top:5px">${[['open','Open'],['allow','Only these'],['block','Block these']].map(([v,l])=>`<button role="radio" data-v="${v}" aria-pressed="${am===v}" aria-checked="${am===v}" onclick="setAcc('${v}')">${l}</button>`).join('')}</div></div>
   <div id="in-acc-box" ${am==='open'?'hidden':''}><label class="l" for="in-acc-list" id="in-acc-l">${am==='block'?'ISSIs that may not use the group':'ISSIs that may use the group (no one else)'}</label>
   <textarea class="f" id="in-acc-list" rows="4" style="width:100%;height:auto;padding:10px;font:13px var(--mono,ui-monospace,monospace)" placeholder="2320972, 2321100">${a?a.issis.join(', '):''}</textarea>
   <div class="mut" style="font-size:12px;margin-top:4px">Separated by commas, spaces or new lines. An empty list opens the group.</div></div>
   <label style="display:flex;gap:12px;align-items:center"><button type="button" class="tog" role="switch" id="in-rx" aria-checked="${!!(a&&a.rx_only)}" onclick="const on=this.getAttribute('aria-checked')!=='true';this.setAttribute('aria-checked',on);$('in-rx-box').hidden=!on"></button><span><b style="font-size:13.5px">RX only</b><br><span class="mut" style="font-size:12px">Listen only: these radios may not talk on the group</span></span></label>
   <div id="in-rx-box" ${a&&a.rx_only?'':'hidden'}><label class="l" for="in-rx-list">Listen-only ISSIs (empty = everyone)</label>
   <div class="mut" style="font-size:12px;margin:0 0 4px">With <i>Only these</i>, ISSIs missing above are added there when you save (a listener must be allowed on the group).</div>
   <textarea class="f" id="in-rx-list" rows="3" style="width:100%;height:auto;padding:10px;font:13px var(--mono,ui-monospace,monospace)" placeholder="empty = nobody may talk">${a?a.rx_only_issis.join(', '):''}</textarea></div>`:''}
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveName()">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>(k?$('in-name'):$('in-issi')).focus(),30);
}
function setAcc(v){document.querySelectorAll('#in-acc button').forEach(x=>{const on=x.dataset.v===v;x.setAttribute('aria-pressed',on);x.setAttribute('aria-checked',on);});
  $('in-acc-box').hidden=v==='open';$('in-acc-l').textContent=v==='block'?'ISSIs that may not use the group':'ISSIs that may use the group (no one else)';}
async function saveName(){
  const K=UI.edK||dirK();const m=$('sheet-msg');const issi=$('in-issi').value.trim(),name=$('in-name').value.trim();
  let acc=null,added=[];
  if(K==='gssi'){const b=document.querySelector('#in-acc button[aria-pressed="true"]');const v=b?b.dataset.v:'open';
    const rx=$('in-rx').getAttribute('aria-checked')==='true';
    acc={gssi:issi,mode:v==='block'?'block':'allow',issis:v==='open'?'':$('in-acc-list').value,rx_only:rx,rx_only_issis:rx?$('in-rx-list').value:''};
    if(v!=='open'&&!acc.issis.trim()){m.className='msg err';m.textContent='Enter at least one ISSI, or choose Open.';return;}
    // Only these + listen-only ISSIs: a listener must be admitted to listen, so add the missing ones to the allowed ISSIs.
    if(v==='allow'&&rx){const sp=t=>t.split(/[\s,;]+/).filter(Boolean),have=sp(acc.issis),miss=sp(acc.rx_only_issis).filter(i=>!have.includes(i));
      if(miss.length){acc.issis=have.concat(miss).join(', ');added=miss;}}}
  const had=(D[K]||{})[issi]||'';
  const kind=K==='issi'?kindPicked():null,kindChanged=!!kind&&kind!==(kindOver(issi)||'auto');
  if(!name&&kindChanged&&!had){m.className='msg mut';m.textContent='Saving…';
    try{D.ikind=await postJ('/api/issi-kinds/set',{issi,kind});closeSheet();setMsg('issi-msg','ok',`${issi}: ${kind}.`);render(true);}catch(e){m.className='msg err';m.textContent=e.message;}return;}
  if(!name&&!(K==='gssi'&&(had||acc.issis.trim()||acc.rx_only||(D.access||{})[issi]))){m.className='msg err';m.textContent='Enter a name (or delete it in the list).';return;}
  m.className='msg mut';m.textContent='Saving…';
  try{
    if(name||had)D[K]=await postJ(DIR[K].api+'/set',{issi,name});
    if(kindChanged)D.ikind=await postJ('/api/issi-kinds/set',{issi,kind});
    if(acc){D.access=await postJ('/api/group-access/set',acc);D.tg=await getJ('/api/talkgroups').catch(()=>D.tg);}
    closeSheet();setMsg(UI.edFrom||'issi-msg','ok',`${issi} saved.`+(added.length?` ${added.join(', ')} also added to the allowed ISSIs (listen only).`:''));render(true);
  }catch(e){m.className='msg err';m.textContent=e.message;}
}
// Managed talkgroups (Routing page): one card that folds to a thin bar. Off it starts folded (open it to
// prepare the list), on it starts open; either way the header folds it.
// DGNA: every talkgroup saved on the server to pick from (Directory names and access entries, allowed
// talkgroups, pool groups), not only the allowed ones.
function dgSaved(f){
  const names=D.gssi||{},ks=[...new Set([...Object.keys(names),...Object.keys(D.access||{}),...((D.tg&&D.tg.allowed)||[]).map(String),...(D.pools||[]).flatMap(p=>(p.groups||[]).map(String))])].filter(k=>+k>=1&&+k<=16777215).sort((a,b)=>a-b);
  if(!ks.length)return '';
  return `<select class="f" aria-label="Saved talkgroups" style="max-width:220px" onchange="if(this.value){UI.dgf.gssi=this.value;UI.dgf.name=((D.gssi||{})[this.value]||'').slice(0,15);render(true)}"><option value="">Saved TGs…</option>${ks.map(k=>`<option value="${k}" ${String(f.gssi)===k?'selected':''}>${k}${names[k]?' · '+esc(names[k]):''}</option>`).join('')}</select>`;
}
function tgCard(adm,pools){
  const tg=D.tg||{},open=UI.tgOpen??!!tg.managed;
  const names=D.gssi||{},acc=D.access||{},refused=tg.refused||{};
  const allowed=new Set((tg.allowed||[]).map(String));
  const seen=new Map();
  for(const m of ((D.conn&&D.conn.mobile_stations)||[]))for(const g of (m.groups||[]))if(g<=16777215)seen.set(String(g),(seen.get(String(g))||0)+1);
  for(const c of ((D.status&&D.status.active_calls)||[]))if(c.kind==='group'&&c.destination<=16777215&&!seen.has(String(c.destination)))seen.set(String(c.destination),0);
  // Allowed without being on the list: groups of pools / linked talkgroups, and groups with "Only these" (limited anyway).
  const auto=new Map();
  for(const p of (pools||[]))for(const g of (p.groups||[]))if(!allowed.has(String(g))&&!auto.has(String(g))){
    const r=(p.remote||[])[0];auto.set(String(g),`<span class="chip b">${p.linked&&r?`Linked: ${esc(r.network)}:${r.gssi}`:`Pool: ${esc(p.name)}`}</span>`);}
  for(const k of Object.keys(acc))if(onlyThese(acc[k])&&+k>=1&&+k<=16777215&&!allowed.has(k)&&!auto.has(k))auto.set(k,'');
  // Name with the badges (pool / linked, access) under it.
  const cell=(k,extra)=>{const c=[extra||'',accChips(acc[k])].filter(Boolean).join(' ');return c?`${names[k]?esc(names[k])+'<div class="tgc">':'<div class="tgc" style="margin-top:0">'}${c}</div>`:nm(k);};
  const ed=k=>adm?(acc[k]?`<button class="btn" onclick="editName('${k}','gssi')" aria-label="Edit group ${k}">Edit</button>`:`<button class="btn" onclick="editName('${k}','gssi')" aria-label="Access rules for group ${k}" title="Who may use or talk on the group">Rules</button>`):'';
  const now=k=>{const n=seen.get(k);return n===undefined?'<span class="st i"><i></i>no radios</span>':`<span class="st"><i></i>${n?n+' radio'+(n===1?'':'s'):'call running'}</span>`;};
  const nm=k=>names[k]?esc(names[k]):'<span class="mut">—</span>';
  const cols='<colgroup><col class="g"><col class="n"><col class="w hide-s"><col class="a"></colgroup>';
  const rows=[...allowed].sort((a,b)=>a-b).map(k=>`<tr><td><b class="m">${k}</b></td><td>${cell(k)}</td><td class="hide-s">${now(k)}</td><td class="tga">${ed(k)}${adm?` <button class="btn" onclick="disallowTg(${k})" aria-label="Remove ${k} from the allowed talkgroups" title="Remove">✕</button>`:''}</td></tr>`)
    .concat([...auto.keys()].sort((a,b)=>a-b).map(k=>`<tr class="off"><td><b class="m">${k}</b></td><td>${cell(k,auto.get(k))}</td><td class="hide-s">${now(k)}</td><td class="tga">${ed(k)}<div class="mut" style="font-size:12px" title="${auto.get(k)?'Allowed because the group is in a pool or linked talkgroup':'Allowed because only the listed ISSIs may use the group (Only these)'}">automatic</div></td></tr>`));
  const saved=[...new Set([...Object.keys(names),...Object.keys(acc),...seen.keys(),...Object.keys(refused)])].filter(k=>+k>=1&&+k<=16777215&&!allowed.has(k)&&!auto.has(k)).sort((a,b)=>a-b);
  const savedRows=saved.map(k=>`<tr><td><b class="m">${k}</b></td><td>${cell(k,refused[k]?`<span class="chip d">refused · ${refused[k]} radio${refused[k]===1?'':'s'} tried</span>`:'')}</td><td class="hide-s">${now(k)}</td><td class="tga">${acc[k]?ed(k)+' ':''}${adm?`<button class="btn" onclick="allowTg('${k}','')">${ic('plus')}Allow</button>`:''}</td></tr>`).join('');
  const n=allowed.size+auto.size;
  return `<div class="card${open?'':' shut'}"><div class="ch tgh" role="button" tabindex="0" aria-expanded="${open}" onclick="UI.tgOpen=!(${open});render(true)" onkeydown="if(event.key==='Enter'||event.key===' '){event.preventDefault();this.click()}">
    <span class="chev" aria-hidden="true">▸</span><h2>Managed talkgroups</h2><span class="cnt">${n} allowed</span>${tg.managed?'<span class="chip a">on</span>':'<span class="chip i">off</span>'}<span onclick="event.stopPropagation()">${helpBtn('tgm')}</span><span class="sp"></span>
    <label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600" onclick="event.stopPropagation()"><button class="tog" role="switch" aria-checked="${!!tg.managed}" ${adm?'':'disabled'} onclick="setManaged(this.getAttribute('aria-checked')!=='true')"></button><span class="hide-s">Only allowed talkgroups</span></label></div>
   ${helpNote('tgm','<b>Managed talkgroups</b>: a closed set of talkgroups. Switched on, radios may only use the allowed groups: the ones in this list, every group of a talkgroup pool or linked talkgroup, and every group with the access <i>Only these</i> (only the listed ISSIs may use it anyway); these are marked <i>automatic</i>. The badges show the access rules of a group (<i>Only N ISSIs</i>, <i>N blocked</i>, <i>RX only</i>); <i>Edit</i> or <i>Rules</i> opens the same dialog as on the Directory page. The server records no affiliation to any other group and drops its group calls (emergency calls pass); radios already on such a group are dropped from it, and a FlowStation with <code>group-access</code> refuses the attachment on the air. Switched off, every group is open as before and the list is kept, so it can be prepared first. A name given with <i>Allow</i> goes to the Directory. <i>Saved TGs on server</i> lists groups the server knows (with a name or access entry on the Directory page, in use now, or refused) that are not allowed yet. Written to the config file (<code>[talkgroups]</code>), no restart.')}
   ${open?`${adm?`<div class="tgadd"><input class="f" id="tg-gssi" placeholder="GSSI" inputmode="numeric" style="width:110px" onkeydown="if(event.key==='Enter')allowTg($('tg-gssi').value,$('tg-name').value)"><input class="f" id="tg-name" maxlength="60" placeholder="Name (optional, goes to the Directory)" style="flex:1;min-width:160px" onkeydown="if(event.key==='Enter')allowTg($('tg-gssi').value,$('tg-name').value)"><button class="btn pri" onclick="allowTg($('tg-gssi').value,$('tg-name').value)">${ic('plus')}Allow</button></div>`:''}
   ${rows.length?`<div class="tw"><table class="tgt">${cols}<thead><tr><th>GSSI</th><th>Name</th><th class="hide-s">Now</th><th></th></tr></thead><tbody>${rows.join('')}</tbody></table></div>`:`<div class="empty">No talkgroups allowed yet${tg.managed?': radios can use none (except emergency calls)':''}.</div>`}
   <details class="tgsaved" ${UI.tgSaved?'open':''} ontoggle="UI.tgSaved=this.open"><summary>Saved TGs on server <span class="cnt" style="font:600 11.5px var(--mono);color:var(--text3)">${saved.length} not allowed</span></summary>
    ${saved.length?`<div class="tw"><table class="tgt">${cols}<tbody>${savedRows}</tbody></table></div>`:'<div class="empty">Every group the server knows is allowed.</div>'}</details>
   ${msgDiv('tg-msg','padding:10px 18px')}`:''}</div>`;
}
async function setManaged(on){
  const n=((D.tg&&D.tg.listed)||[]).length;
  if(on&&!confirm(`Managed talkgroups: radios may only use the ${n} allowed group${n===1?'':'s'} (this list, the groups of pools and linked talkgroups, and groups with Only these). Radios on any other group are dropped from it. Switch on?`))return;
  try{D.tg=await postJ('/api/talkgroups/set',{managed:on});UI.tgOpen=undefined;setMsg('tg-msg','ok',on?'Managed talkgroups on.':'Managed talkgroups off: every group is open again, the list is kept.');}catch(e){setMsg('tg-msg','err',e.message);}
  render(true);
}
async function allowTg(g,name){
  g=String(g).trim();if(!g)return;
  try{D.tg=await postJ('/api/talkgroups/set',{allow:g,name:name||''});if(name)D.gssi=await getJ('/api/gssi-names');
    const a=$('tg-gssi'),b=$('tg-name');if(a)a.value='';if(b)b.value='';setMsg('tg-msg','ok',`${g} allowed.`);}catch(e){setMsg('tg-msg','err',e.message);}
  render(true);
}
async function disallowTg(g){
  if(D.tg&&D.tg.managed&&!confirm(`Remove ${g} from the allowed talkgroups? Radios on it are dropped from it.`))return;
  try{D.tg=await postJ('/api/talkgroups/set',{disallow:g});setMsg('tg-msg','ok',`${g} no longer allowed.`);}catch(e){setMsg('tg-msg','err',e.message);}
  render(true);
}
async function hideGroup(k,hide){
  try{D.tg=await postJ('/api/talkgroups/set',hide?{hide:+k}:{unhide:+k});if(!(D.tg.hidden||[]).length&&UI.filter.issif==='hidden')UI.filter.issif='all';setMsg('issi-msg','ok',hide?`${k} hidden. Filter "Hidden" shows it again.`:`${k} shown again.`);}catch(e){setMsg('issi-msg','err',e.message);}
  render(true);
}
async function delName(k){
  const K=dirK();if(!confirm(`Delete the name of ${k}?`))return;
  try{D[K]=await postJ(DIR[K].api+'/set',{issi:k,name:''});setMsg('issi-msg','ok',`Name of ${k} deleted.`);}catch(e){setMsg('issi-msg','err',e.message);}
  render(true);
}
function importNames(){
  const d=DIR[dirK()];
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Import list</h3></div><div class="sb">
   <div><label class="l" for="in-list">One ${d.who} per line: ${d.what}, then the name (comma, semicolon, tab or space between). A CSV export from a spreadsheet works.</label>
   <textarea class="f" id="in-list" rows="9" style="width:100%;height:auto;padding:10px;font:13px var(--mono,ui-monospace,monospace)" placeholder="${d.ex}, ${esc(d.exn)}&#10;${esc(d.ex2)}"></textarea></div>
   <label style="display:flex;gap:12px;align-items:center"><button type="button" class="tog" role="switch" id="in-replace" aria-checked="false" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button><span><b style="font-size:13.5px">Replace the whole list</b><br><span class="mut" style="font-size:12px">Off: adds the names and overwrites the ones listed</span></span></label>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="doImport()">Import</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('in-list').focus(),30);
}
async function doImport(){
  const K=dirK();const m=$('sheet-msg');const replace=$('in-replace').getAttribute('aria-checked')==='true';
  if(replace&&!confirm(`Replace all ${DIR[K].what} names with this list?`))return;
  m.className='msg mut';m.textContent='Importing…';
  try{const before=Object.keys(D[K]||{}).length;D[K]=await postJ(DIR[K].api+'/import',{text:$('in-list').value,replace});closeSheet();
    setMsg('issi-msg','ok',`Imported. The list has ${Object.keys(D[K]).length} names (before ${before}).`);render(true);}
  catch(e){m.className='msg err';m.textContent=e.message;}
}
function exportNames(){
  const K=dirK(),d=DIR[K];
  const rows=Object.entries(D[K]||{}).sort((a,b)=>a[0]-b[0]).map(([k,v])=>k+','+(/[",]/.test(v)?'"'+v.replace(/"/g,'""')+'"':v));
  const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([d.what+',Name\n'+rows.join('\n')+'\n'],{type:'text/csv'}));a.download=d.file;a.click();setTimeout(()=>URL.revokeObjectURL(a.href),1000);
}

/* ---------- configuration ---------- */
R.config=()=>{
  if(!D.who.admin)return `<div class="card"><div class="empty">The configuration is for admins. <button class="btn" onclick="openLogin(false)">Log in</button></div></div>`;
  if(UI.raw==null){if(!UI.rawLoading)loadRaw();return '<div class="card"><div class="empty">Loading…</div></div>';}
  const secs=[...UI.raw.matchAll(/^\s*(\[\[?[^\]\n]+\]\]?)/gm)].map(m=>m[1]);
  return `<div class="card"><div class="ch"><h2>Full configuration</h2><span class="mut">brew-server.toml</span>${helpBtn('cfg')}<span class="sp"></span>
    <select class="f" onchange="jumpSec(this.value);this.value=''" aria-label="Jump to section"><option value="">Jump to section…</option>${[...new Set(secs)].map(x=>`<option>${esc(x)}</option>`).join('')}</select>
    <button class="btn" onclick="if(!UI.rawDirty||confirm('Throw away your changes and load the file again?'))loadRaw()">Reload</button>
    <button class="btn pri" onclick="saveRaw()">Save and apply</button></div>
   ${helpNote('cfg','Every setting of the server. <b>Save and apply</b> checks the text first; a mistake is shown and nothing is saved. Then the server restarts within a few seconds to apply it: running calls drop and stations reconnect by themselves, and you sign in again. Logins (Accounts), talkgroup pools (Routing) and the ISSI blacklist change without a restart on their own pages. With high availability on, apply the file from the High Availability page.')}
   ${msgDiv('cfg-msg','padding:10px 18px')}
   <textarea class="cfg" id="cfg" spellcheck="false" autocapitalize="off" aria-label="Configuration file" oninput="UI.raw=this.value;UI.rawDirty=true">${esc(UI.raw)}</textarea></div>`;
};
async function loadRaw(){
  UI.rawLoading=true;
  try{UI.raw=await getJ('/api/config/raw');UI.rawDirty=false;setMsg('cfg-msg','','');}catch(e){UI.raw=null;setMsg('cfg-msg','err',e.message);}
  UI.rawLoading=false;render(true);
}
function jumpSec(sec){
  const t=$('cfg');if(!t||!sec)return;const i=t.value.search(new RegExp('^\\s*'+sec.replace(/[.*+?^${}()|[\]\\]/g,'\\$&'),'m'));if(i<0)return;
  t.focus();t.setSelectionRange(i,i);const line=t.value.slice(0,i).split('\n').length-1;t.scrollTop=Math.max(0,line*parseFloat(getComputedStyle(t).lineHeight)-20);
}
async function saveRaw(){
  const text=$('cfg')?$('cfg').value:UI.raw;
  if(!confirm('Save and apply? The server restarts to apply it; running calls drop.'))return;
  setMsg('cfg-msg','mut','Checking and saving…');
  try{const r=await api('/api/config/raw',{method:'PUT',body:text});UI.rawDirty=false;
    setMsg('cfg-msg','ok',(r&&r.note?r.note[0].toUpperCase()+r.note.slice(1):'Saved')+'.');
    if(!(D.ha&&D.ha.enabled))waitRestart();
  }catch(e){setMsg('cfg-msg','err',e.message);}
}
async function waitRestart(){
  // The process restarts a moment after the file changed: wait until it is gone and back.
  let down=false;
  for(let n=0;n<60;n++){await new Promise(r=>setTimeout(r,1000));
    const up=await fetch('/healthz',{cache:'no-store'}).then(r=>r.ok).catch(()=>false);
    if(!up){down=true;setMsg('cfg-msg','mut','Restarting…');continue;}
    if(down||n>=8){setMsg('cfg-msg','ok','Applied: the server is back with the new configuration.');UI.raw=null;
      const w=await fetch('/api/whoami',{cache:'no-store',headers:{'x-brew-ui':'1'}}).then(r=>r.json()).catch(()=>null);
      if(w){D.who=w;if(w.need_login||(!w.username&&!w.admin))openLogin(!!w.need_login);}
      render(true);return;}
  }
}

/* ---------- SIP, SMS Center, High Availability, Server settings (native, were classic frames) ---------- */
// Page data that only these pages need: asked when the page opens and with the live refresh.
function loadPageData(p,first){
  const t=[];const set=(k,u)=>t.push(getJ(u).then(v=>{D[k]=v;}).catch(e=>{D[k+'Err']=e.message;}));
  if(p==='sip')set('sip','/api/sip');
  if(p==='sipcfg'){set('sipc','/api/sip/config');if(first&&D.who.admin)set('sipFull','/api/config/sip/full');}
  if(p==='sms')set('smsc','/api/sms-center');
  if(p==='ha'){set('ha','/api/ha');if(first&&D.who.admin)set('haCfg','/api/config/ha');}
  if(p==='settings'&&first&&D.who.admin){set('dusers','/api/dashboard-users');set('blk','/api/config/blacklist');set('allow','/api/config/allowlist');set('btsLocs','/api/bts-locations?config_only=1');set('mapSet','/api/map');set('parrot','/api/parrot');}
  if(t.length)Promise.all(t).then(()=>{if(UI.page===p)render(first);});
}
const adminOnly=what=>`<div class="card"><div class="empty">${what} is for admins. <button class="btn" onclick="openLogin(false)">Log in</button></div></div>`;
const loading='<div class="card"><div class="empty">Loading…</div></div>';
const kvRow=(k,v)=>`<div><span class="l">${k}</span>${v}</div>`;
const pwChip=b=>b?'<span class="chip a">set</span>':'<span class="chip d">none</span>';
const sipState=s=>{const m={up:'',registering:' w',failed:' d',down:' i'};return `<span class="st${m[s]??' i'}"><i></i>${esc(s)}</span>`;};
function sipLeg(e){if(!e)return '—';switch(e.type){case 'sip_extension':return 'ext '+esc(e.aor);case 'sip_trunk':return 'trunk '+esc(e.trunk)+(e.number?' / '+esc(e.number):'');
  case 'brew_private':return 'ISSI '+issiTxt(e.issi);case 'brew_group':return 'TG '+tgTxt(e.gssi);case 'sip_external':return esc(e.uri);default:return esc(JSON.stringify(e));}}
const sipOff=(d,how)=>d&&!d.enabled?`<div class="card"><div class="note">${ic('info')}<div>The SIP gateway is switched off. ${how}</div></div></div>`:'';

R.sip=()=>{
  const d=D.sip;if(!d)return loading;
  const up=d.trunks.filter(t=>t.status==='up').length,now=Date.now();
  return `${sipOff(d,'Turn it on with <code>[sip] enabled = true</code> on the <a data-go="config" href="#config">Config file</a> page.')}
  <div class="tiles">
   <div class="tile"><div class="k">${ic('sip')}Listen</div><div class="v m" style="font-size:17px">${esc(d.listen||'—')}</div><div class="s">realm ${esc(d.realm||'—')}</div></div>
   <div class="tile"><div class="k">${ic('radios')}Registrations</div><div class="v">${d.registrations.length}</div><div class="s">SIP extensions</div></div>
   <div class="tile"><div class="k">${ic('routing')}Trunks up</div><div class="v">${up} / ${d.trunks.length}</div><div class="s">provisioned</div></div>
   <div class="tile${d.active_calls.length?' live':''}"><div class="k">${ic('call')}Active calls</div><div class="v">${d.active_calls.length}</div><div class="s">${d.total_calls} in total</div></div>
  </div>
  <div class="card"><div class="ch"><h2>Extension registrations</h2><span class="cnt">${d.registrations.length}</span>${helpBtn('sipreg')}<span class="sp"></span><a data-go="sipcfg" href="#sipcfg">SIP Config ›</a></div>${helpNote('sipreg','SIP phones and softphones registered to this server as an extension (address of record, AOR). <i>Auth</i>: the registration passed digest authentication. Extensions, trunks and voice routes are set on the <a data-go="sipcfg" href="#sipcfg">SIP Config</a> page.')}
   <div class="tw"><table><thead><tr><th>AOR</th><th>Contact</th><th class="hide-s">Source</th><th class="hide-s">User-Agent</th><th>Auth</th><th>Expires in</th></tr></thead><tbody>
   ${d.registrations.map(r=>`<tr><td class="m"><b>${esc(r.aor)}</b></td><td class="m mut" style="font-size:12px">${esc(r.contact)}</td><td class="m hide-s">${esc(r.source)}</td><td class="mut hide-s">${esc(r.user_agent)}</td><td>${yn(r.authenticated)}</td><td class="num">${Math.max(0,Math.floor((r.expires_at_ms-now)/1000))} s</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No registrations</td></tr>'}
   </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Trunks</h2><span class="cnt">${d.trunks.length}</span></div>
   <div class="tw"><table><thead><tr><th>Name</th><th>Direction</th><th class="hide-s">Remote</th><th>Status</th><th class="hide-s">Peer</th><th class="hide-s">Detail</th><th>Calls</th></tr></thead><tbody>
   ${d.trunks.map(t=>`<tr><td><b>${esc(t.name)}</b></td><td>${esc(t.direction)}</td><td class="m hide-s">${esc(t.remote_host||'—')}</td><td>${sipState(t.status)}</td><td class="m mut hide-s">${esc(t.peer_addr||'—')}</td><td class="mut hide-s">${esc(t.detail||'')}</td><td class="num">${t.active_calls}</td></tr>`).join('')||'<tr><td colspan="7" class="mut">No trunks provisioned</td></tr>'}
   </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Active calls</h2><span class="cnt">${d.active_calls.length}</span></div>
   <div class="tw"><table><thead><tr><th>From</th><th>To</th><th>State</th><th>Duration</th><th class="hide-s">RTP A / B</th><th class="hide-s">Call-ID</th></tr></thead><tbody>
   ${d.active_calls.map(c=>`<tr><td>${sipLeg(c.from)}</td><td>${sipLeg(c.to)}</td><td>${esc(c.state)}</td><td class="num">${dur(now-(c.answered_at_ms||c.started_at_ms))}</td><td class="m mut hide-s">${c.rtp_a_port||'—'} / ${c.rtp_b_port||'—'}</td><td class="m mut hide-s" style="font-size:12px">${esc(String(c.call_id).slice(0,18))}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No active calls</td></tr>'}
   </tbody></table></div></div>`;
};

// SIP Config: read-only for everyone; an admin adds, edits and deletes extensions, trunks and routes here.
// Endpoints as written in the forms: ext:USER, trunk:NAME[/NUMBER], issi:N, group:N.
function sipEp(s){s=(s||'').trim();if(!s)return null;const i=s.indexOf(':');if(i<0)return undefined;const k=s.slice(0,i),r=s.slice(i+1);
  if(k==='ext'&&r)return {kind:'sip_extension',user:r};
  if(k==='trunk'&&r){const [trunk,number]=r.split('/');return {kind:'sip_trunk',trunk,number:number||''};}
  if(k==='issi'&&+r>0)return {kind:'brew_private',issi:+r};
  if(k==='group'&&+r>0)return {kind:'brew_group',gssi:+r};
  return undefined;}
function sipEpTxt(ep){if(!ep)return '';if(ep.kind==='sip_extension')return 'ext:'+ep.user;if(ep.kind==='sip_trunk')return ep.number?`trunk:${ep.trunk}/${ep.number}`:'trunk:'+ep.trunk;
  if(ep.kind==='brew_private')return 'issi:'+ep.issi;if(ep.kind==='brew_group')return 'group:'+ep.gssi;return '';}
R.sipcfg=()=>{
  const d=D.sipc;if(!d)return loading;
  const adm=D.who.admin,F=D.sipFull;
  const act=(kind,key)=>adm&&F?`<td style="text-align:right;white-space:nowrap"><button class="btn" onclick="sipEdit('${kind}',${esc(JSON.stringify(key))})">Edit</button> <button class="btn danger hide-s" onclick="sipDel('${kind}',${esc(JSON.stringify(key))})">Delete</button></td>`:'';
  const addBtn=kind=>adm&&F?`<span class="sp"></span><button class="btn pri" onclick="sipEdit('${kind}',null)">${ic('plus')}Add</button>`:'';
  const restart='Saving writes the config file and restarts the server within a few seconds (Brew and SIP connections drop briefly and reconnect).';
  return `${sipOff(d,'Set <code>[sip] enabled = true</code> on the <a data-go="config" href="#config">Config file</a> page.')}
  <div class="card"><div class="ch"><h2>General</h2>${helpBtn('sipgen')}<span class="sp"></span><a data-go="sip" href="#sip">SIP / VoIP ›</a></div>${helpNote('sipgen',(adm?'Extensions, trunks and voice routes are edited on this page. '+restart:'Read only. An admin edits extensions, trunks and voice routes here (log in).')+' The general settings are in <code>[sip]</code> on the Config file page. Passwords are never shown.')}
   <div class="kv" style="padding:12px 18px">${kvRow('Enabled',yn(d.enabled))}${kvRow('Listen',`<span class="m">${esc(d.listen)}</span>`)}${kvRow('Advertised host',`<span class="m">${esc(d.advertised_host||'local socket address')}</span>`)}${kvRow('Realm',esc(d.realm))}${kvRow('RTP ports',`<span class="m">${esc(d.rtp_port_min)}–${esc(d.rtp_port_max)}</span>`)}${kvRow('Registration TTL',esc(d.registration_ttl_seconds)+' s')}</div>
   ${msgDiv('sip-msg')}</div>
  <div class="card"><div class="ch"><h2>Extensions</h2><span class="cnt">${d.extensions.length}</span>${addBtn('ext')}</div>
   <div class="tw"><table><thead><tr><th>User</th><th>Display name</th><th>ISSI</th><th>Outbound</th><th class="hide-s">Password</th>${adm&&F?'<th></th>':''}</tr></thead><tbody>
   ${d.extensions.map(e=>`<tr><td class="m"><b>${esc(e.user)}</b></td><td>${esc(e.display_name||'—')}</td><td class="m">${e.issi?issiTxt(e.issi):'—'}</td><td>${yn(e.allow_outbound)}</td><td class="hide-s">${pwChip(e.has_password)}</td>${act('ext',e.user)}</tr>`).join('')||`<tr><td colspan="6" class="mut">No extensions provisioned</td></tr>`}
   </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Trunks</h2><span class="cnt">${d.trunks.length}</span>${addBtn('trunk')}</div>
   <div class="tw"><table><thead><tr><th>Name</th><th>Direction</th><th class="hide-s">Remote host</th><th class="hide-s">Username</th><th class="hide-s">Realm</th><th class="hide-s">Registration</th><th>Enabled</th><th class="hide-s">Password</th>${adm&&F?'<th></th>':''}</tr></thead><tbody>
   ${d.trunks.map(t=>`<tr><td><b>${esc(t.name)}</b></td><td>${esc(t.direction)}</td><td class="m hide-s">${esc(t.remote_host||'—')}</td><td class="m hide-s">${esc(t.username||'—')}</td><td class="mut hide-s">${esc(t.realm||'—')}</td><td class="num hide-s">every ${esc(t.register_interval_seconds)} s</td><td>${yn(t.enabled)}</td><td class="hide-s">${pwChip(t.has_password)}</td>${act('trunk',t.name)}</tr>`).join('')||'<tr><td colspan="9" class="mut">No trunks provisioned</td></tr>'}
   </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Voice routes</h2><span class="cnt">${d.routes.length}</span>${helpBtn('siprt')}${addBtn('route')}</div>${helpNote('siprt','Routes are checked from top to bottom; the first enabled route whose match pattern (and optional <i>from</i>) fits the dialled number wins. Endpoints: <code>ext:USER</code> (SIP extension), <code>trunk:NAME</code> or <code>trunk:NAME/NUMBER</code>, <code>issi:N</code> (individual call to a radio), <code>group:N</code> (group call). <i>Strip prefix</i> removes a leading literal (e.g. the outside line digit "9") only from the number handed to a <code>trunk:NAME</code> without a number. Editing keeps a route\'s position; a new route goes to the end (reorder it on the Config file page).')}
   <div class="tw"><table><thead><tr><th>#</th><th>Name</th><th>Match</th><th class="hide-s">Strip prefix</th><th>From</th><th>To</th><th>Enabled</th>${adm&&F?'<th></th>':''}</tr></thead><tbody>
   ${d.routes.map((r,i)=>`<tr><td class="mut num">${i+1}</td><td><b>${esc(r.name||'—')}</b></td><td><code>${esc(r.match_pattern)}</code></td><td class="m mut hide-s">${esc(r.strip_prefix||'—')}</td><td class="m">${esc(r.from||'any')}</td><td class="m">${esc(r.to||'—')}</td><td>${yn(r.enabled)}</td>${act('route',r.name)}</tr>`).join('')||'<tr><td colspan="8" class="mut">No voice routes</td></tr>'}
   </tbody></table></div></div>`;
};
const sel=(id,label,val,opts)=>`<div><label class="l" for="${id}">${label}</label><select class="f" id="${id}" style="width:100%">${opts.map(o=>`<option value="${o}"${o===val?' selected':''}>${o}</option>`).join('')}</select></div>`;
const swRow=(id,on,label,hint)=>`<label style="display:flex;gap:12px;align-items:center"><button type="button" class="tog" role="switch" id="${id}" aria-checked="${!!on}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button><span><b style="font-size:13.5px">${label}</b>${hint?`<br><span class="mut" style="font-size:12px">${hint}</span>`:''}</span></label>`;
function sipEdit(kind,key){
  const F=D.sipFull||{extensions:{},trunks:{},routes:[]};const isNew=key==null;let body='',title='';
  if(kind==='ext'){const e=isNew?{display_name:'',issi:0,allow_outbound:true,password:''}:F.extensions[key]||{};title=isNew?'New extension':'Extension '+esc(key);
    body=`${fld('se-user','User (extension number)',isNew?'':key,(isNew?'':'disabled ')+'placeholder="e.g. 1001" autocomplete="off"')}${fld('se-name','Display name',e.display_name||'','placeholder="optional"')}
     ${fld('se-issi','ISSI (radio this extension stands for)',e.issi||'','inputmode="numeric" placeholder="optional"')}${fld('se-pw','Password',isNew?'':(e.password||''),'type="password" autocomplete="new-password" spellcheck="false"')}
     ${swRow('se-out',e.allow_outbound!==false,'Outbound calls','May call out through trunks')}`;}
  else if(kind==='trunk'){const t=isNew?{direction:'outbound',remote_host:'',username:'',password:'',realm:'',register_interval_seconds:300,enabled:true}:F.trunks[key]||{};title=isNew?'New trunk':'Trunk '+esc(key);
    body=`${fld('st-name','Name',isNew?'':key,(isNew?'':'disabled ')+'placeholder="e.g. provider" autocomplete="off"')}${sel('st-dir','Direction',t.direction||'outbound',['outbound','inbound','peer'])}
     ${fld('st-host','Remote host (host:port)',t.remote_host||'','placeholder="sip.example.net:5060" autocomplete="off" spellcheck="false"')}
     <div class="row2">${fld('st-user','Username',t.username||'','autocomplete="off" spellcheck="false"')}${fld('st-pw','Password',isNew?'':(t.password||''),'type="password" autocomplete="new-password" spellcheck="false"')}</div>
     <div class="row2">${fld('st-realm','Realm',t.realm||'','placeholder="optional"')}${fld('st-int','Registration interval (s)',t.register_interval_seconds||300,'inputmode="numeric"')}</div>
     ${swRow('st-en',t.enabled!==false,'Enabled','')}`;}
  else{const r=isNew?{name:'',match_pattern:'',strip_prefix:'',from:null,to:null,enabled:true}:(F.routes||[]).find(x=>x.name===key)||{};title=isNew?'New voice route':'Voice route '+esc(key);
    body=`${fld('sr-name','Name',r.name||'',(isNew?'':'disabled ')+'placeholder="e.g. outside line" autocomplete="off"')}
     <div class="row2">${fld('sr-match','Match pattern',r.match_pattern||'','placeholder="e.g. 9*" spellcheck="false"')}${fld('sr-strip','Strip prefix',r.strip_prefix||'','placeholder="e.g. 9" spellcheck="false"')}</div>
     ${fld('sr-from','From (optional)',sipEpTxt(r.from),'placeholder="any, or ext:1001 / trunk:NAME / issi:N / group:N" spellcheck="false"')}
     ${fld('sr-to','To',sipEpTxt(r.to),'placeholder="ext:1001 / trunk:NAME[/NUMBER] / issi:N / group:N" spellcheck="false"')}
     ${swRow('sr-en',r.enabled!==false,'Enabled','')}`;}
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${title}</h3></div><div class="sb">${body}
   <div class="mut" style="font-size:12px">Saving writes the config file and restarts the server within a few seconds.</div><div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts">${isNew?'':`<button class="btn danger" style="margin-right:auto" onclick="closeSheet();sipDel('${kind}',${esc(JSON.stringify(key))})">${ic('trash')}Delete</button>`}<button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="sipSave('${kind}',${esc(JSON.stringify(key))})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>{const f=$('sheet').querySelector('input.f:not([disabled])');if(f)f.focus();},30);
}
async function sipSave(kind,key){
  const m=$('sheet-msg'),v=id=>$(id).value.trim(),bad=t=>{m.className='msg err';m.textContent=t;};
  let url,body;
  if(kind==='ext'){const user=key??v('se-user');if(!user)return bad('The extension needs a user (number).');
    url='/api/config/sip/extensions/'+encodeURIComponent(user);body={password:$('se-pw').value,display_name:v('se-name'),issi:parseInt(v('se-issi'),10)||0,allow_outbound:on('se-out')};}
  else if(kind==='trunk'){const name=key??v('st-name');if(!name)return bad('The trunk needs a name.');
    url='/api/config/sip/trunks/'+encodeURIComponent(name);body={direction:$('st-dir').value,remote_host:v('st-host'),username:v('st-user'),password:$('st-pw').value,realm:v('st-realm'),register_interval_seconds:parseInt(v('st-int'),10)||300,enabled:on('st-en')};}
  else{const name=key??v('sr-name');if(!name)return bad('The route needs a name.');
    const from=sipEp(v('sr-from')),to=sipEp(v('sr-to'));
    if(from===undefined)return bad('From: write ext:USER, trunk:NAME, issi:N or group:N, or leave it empty.');
    if(!to)return bad('To: write ext:USER, trunk:NAME[/NUMBER], issi:N or group:N.');
    url='/api/config/sip/routes';body={name,match_pattern:v('sr-match')||'*',strip_prefix:v('sr-strip'),from,to,enabled:on('sr-en')};}
  m.className='msg mut';m.textContent='Saving…';
  try{await postJ(url,body);closeSheet();sipSaved();}catch(e){bad(e.message);}
}
async function sipDel(kind,key){
  const what=kind==='ext'?'extension':kind==='trunk'?'trunk':'voice route';
  if(!confirm(`Delete ${what} "${key}"? The server restarts within a few seconds to apply it.`))return;
  const path=kind==='ext'?'extensions':kind==='trunk'?'trunks':'routes';
  try{await api(`/api/config/sip/${path}/${encodeURIComponent(key)}`,{method:'DELETE'});sipSaved();}catch(e){setMsg('sip-msg','err',e.message);render(true);}
}
function sipSaved(){setMsg('sip-msg','ok','Saved. The server restarts to apply it; this page updates once it is back.');render(true);
  setTimeout(()=>{getJ('/api/config/sip/full').then(v=>{D.sipFull=v;}).catch(()=>{});loadPageData('sipcfg',false);},4000);}

R.sms=()=>{
  const d=D.smsc;if(!d)return loading;
  const adm=D.who.admin,list=(d.messages||[]).slice().sort((a,b)=>b.stored_at_ms-a.stored_at_ms);
  return `${!d.enabled?`<div class="card"><div class="note">${ic('info')}<div>The SMS Center is switched off (<code>[sms_center]</code> on the Config file page).</div></div></div>`:''}
  <div class="card"><div class="ch"><h2>Stored messages</h2><span class="cnt">${list.length}</span>${helpBtn('smsc')}<span class="sp"></span><button class="btn" style="font-size:12.5px" onclick="smsKnown()" title="Show the radios the SMS Center knows">${d.known_issis} radios known</button></div>${helpNote('smsc','SDS to a radio that is not reachable are kept here and delivered when it registers again, until they expire. <i>Delivering</i>: a delivery attempt is running. Admins can delete a stored message.')}
   <div class="tw"><table><thead><tr><th>Stored</th><th>From</th><th>To</th><th>Message</th><th>Status</th><th class="hide-s">Attempts</th><th class="hide-s">Expires</th>${adm?'<th></th>':''}</tr></thead><tbody>
   ${list.map(x=>`<tr><td class="m">${tmd(x.stored_at_ms)}</td><td class="m">${issiTxt(x.source_issi)}</td><td class="m">${issiTxt(x.destination)}</td><td>${x.text?esc(x.text):`<span class="m mut">${esc(String(x.data_hex||'').slice(0,24))}</span>`}</td><td>${x.in_flight?'<span class="chip w">Delivering</span>':'<span class="chip">Waiting</span>'}</td><td class="num hide-s">${x.attempts}</td><td class="m hide-s">${tmd(x.expires_at_ms)}</td>${adm?`<td style="text-align:right"><button class="btn danger" onclick="smsDel('${esc(x.id)}')">Delete</button></td>`:''}</tr>`).join('')||`<tr><td colspan="8" class="mut">No stored messages</td></tr>`}
   </tbody></table></div>${msgDiv('sms-msg','padding:12px 18px')}</div>`;
};
// The radios the SMS Center has seen register, with their names where the server has one.
function smsKnown(){
  const d=D.smsc||{},l=(d.known||[]).slice().sort((a,b)=>{const na=nameOf(a)||'',nb=nameOf(b)||'';return na&&!nb?-1:nb&&!na?1:na.localeCompare(nb)||a-b;});
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">Known radios</h3><span class="cnt">${l.length}</span></div><div class="sb">
   <div class="mut" style="font-size:12.5px">Radios that registered at least once since the SMS Center keeps its file. Names come from the radio directory or the callsign list, where one is stored.</div>
   <input class="f" id="smk-q" placeholder="Filter by ISSI or name" autocomplete="off" oninput="smsKnownFilter()">
   <div class="tw" style="max-height:55vh;overflow:auto"><table><thead><tr><th>ISSI</th><th>Name</th></tr></thead><tbody id="smk-b">
   ${l.map(i=>`<tr data-k="${esc((i+' '+(nameOf(i)||'')).toLowerCase())}"><td class="m">${i}</td><td>${nameOf(i)?esc(nameOf(i)):'<span class="mut">–</span>'}</td></tr>`).join('')||'<tr><td colspan="2" class="mut">No radios known yet</td></tr>'}
   </tbody></table></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Close</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('smk-q').focus(),30);
}
function smsKnownFilter(){const q=$('smk-q').value.trim().toLowerCase();document.querySelectorAll('#smk-b tr[data-k]').forEach(r=>{r.style.display=!q||r.dataset.k.includes(q)?'':'none';});}
async function smsDel(id){if(!confirm('Delete this stored message? It will not be delivered.'))return;
  try{await api('/api/sms-center/'+id,{method:'DELETE'});setMsg('sms-msg','ok','Deleted.');}catch(e){setMsg('sms-msg','err',e.message);}loadPageData('sms',false);}


// High Availability: both nodes, controls and the [ha] settings (admin), role changes.
const haRole=r=>{const m={active:'a',standby:'b',down:'d',starting:'w'};return `<span class="chip ${m[r]||'i'}">${esc(String(r||'unknown'))}</span>`;};
function haRepl(d){const r=d.replication;if(!r)return '—';if(!r.needed)return '<span class="mut">nothing to copy</span>';
  if(d.role==='active')return r.connected?'serving the Standby':'<b>no Standby connected</b>';
  const a=r.last_rx_ms_ago!=null?` <span class="mut">(${Math.round(r.last_rx_ms_ago/1000)} s ago)</span>`:'';
  return (r.synced?'in sync':r.connected?'<b>catching up</b>':'<b>not connected</b>')+a;}
const HA_F=[['enabled','sw','HA enabled'],['node_name','t','Node name'],['weight','n','Weight'],['persist','sw','Persist'],['vip','t','Virtual IP (a.b.c.d/nn)'],['vip_interface','t','Interface'],['real_ip','t','This node\'s real IP'],['peer_ip','t','Peer\'s real IP'],
  ['heartbeat_port','n','Heartbeat port'],['heartbeat_interval_ms','n','Heartbeat interval (ms)'],['dead_after_ms','n','Peer dead after (ms)'],['shared_secret','pw','Shared secret'],['check_gateway','t','Gateway check IP (optional)']];
R.ha=()=>{
  const d=D.ha;if(!d)return loading;
  const adm=D.who.admin,p=d.peer;
  const warn=[];const list=a=>a&&a.length?': '+a.map(x=>`<code>${esc(x)}</code>`).join(', '):'';
  if(d.pending_changes)warn.push('The config file has saved changes that are not applied yet'+list(d.pending_diff));
  if(d.peer_config_differs)warn.push('The peer runs a different configuration'+(d.peer_config_diff&&d.peer_config_diff.length?list(d.peer_config_diff):' (details follow within a few seconds)'));
  if(d.enabled&&!d.gateway_ok)warn.push('Gateway check failing: this node cannot become Active.');
  const node=(title,chip,rows)=>`<div class="card"><div class="ch"><h2>${title}</h2><span class="sp"></span>${chip}</div><div class="kv tight" style="padding:12px 18px">${rows.map(([k,v])=>kvRow(k,v)).join('')}</div></div>`;
  const nodes=d.enabled?`<div class="grid2">${node('This node: '+esc(d.node),haRole(d.role),[['Weight',d.weight],['Persist',`${d.persist?'on':'off'} <span class="mut">(${d.persist_source==='override'?'dashboard':'config'})</span>`],['Manual hold',d.hold?'yes':'no'],
     ['Virtual IP',`<span class="m">${esc(d.vip)}</span> ${d.vip_held?'<b>held here</b>':'<span class="mut">not held</span>'}`],['Gateway check',d.gateway_ok?'ok':'<b>failing</b>'],['Handing over to',esc(d.handover_to||'—')],
     ['Replication',haRepl(d)],['Records from peer',d.replication&&d.replication.needed?`${d.replication.records_received} <span class="mut">(+${d.replication.sms_snapshots_received} SMS queue copies)</span>`:'—']])}
    ${node('Peer'+(p?': '+esc(p.node):''),p?(p.alive?haRole(p.role):haRole('down')):'<span class="chip i">never heard</span>',p?[['Address',`<span class="m">${esc(p.ip)}</span>`],['Weight',p.weight],['Persist',p.persist?'on':'off'],['Manual hold',p.hold?'yes':'no'],['Copy up to date',p.synced?'yes':'no'],['Last heartbeat',(p.last_seen_ms_ago/1000).toFixed(1)+' s ago']]:[['Status','no heartbeat received yet']])}</div>`
    :`<div class="card"><div class="empty">HA is not enabled on this node.${adm?' Fill in the settings below and switch <i>HA enabled</i> on; the node restarts to apply it.':' An admin can turn it on here.'}</div></div>`;
  const ctl=adm&&d.enabled?`<div class="card"><div class="ch"><h2>Controls</h2>${helpBtn('hactl')}</div>${helpNote('hactl','<i>Make Active</i> / <i>Make Standby</i> hand the virtual IP over to the other node, which then keeps it (manual hold) until it leaves Active. <i>Persist</i> keeps the Active node Active even when a peer with a higher weight comes back; the buttons override the config value until <i>Config default</i>, and each of them ends a manual hold. While HA is enabled, configuration changes take effect only through <i>Apply saved config</i>: an Active node hands over to the Standby first, then restarts. A node that leaves Active restarts into Standby and its sessions reconnect to the new Active node through the virtual IP. The Standby keeps a copy of the call and SDS history and the SMS Center queue; a returning node with a higher weight takes over only once its copy is up to date (manual switches do not wait).')}
    <div class="tgadd"><button class="btn" ${d.role!=='standby'?'disabled':''} onclick="haAct('active')">Make Active</button><button class="btn" ${d.role!=='active'?'disabled':''} onclick="haAct('standby')">Make Standby</button>
     <span class="mut" style="font-size:12.5px">Persist</span><div class="seg" role="group"><button onclick="haPersist(true)" aria-pressed="${d.persist_source==='override'&&d.persist}">On</button><button onclick="haPersist(false)" aria-pressed="${d.persist_source==='override'&&!d.persist}">Off</button><button onclick="haPersist(null)" aria-pressed="${d.persist_source!=='override'}">Config default</button></div>
     <span class="sp"></span><button class="btn pri" onclick="haApply(false)">Apply saved config</button></div>${msgDiv('ha-act','padding:12px 18px')}</div>`:'';
  const c=D.haCfg;
  const form=adm&&c?`<div class="card"><div class="ch"><h2>Settings</h2>${helpBtn('hacfg')}<span class="sp"></span><button class="btn" onclick="haEditCfg()">Edit</button></div>${helpNote('hacfg','Both nodes need the same virtual IP, heartbeat port and shared secret; node name, weight and the two real IPs are per node (swap them on the other node). Saving writes <code>[ha]</code> to the config file only; apply it with <i>Apply saved config</i>.')}
    <div class="kv tight" style="padding:12px 18px">${HA_F.map(([k,t,l])=>kvRow(l,t==='sw'?(c[k]?'on':'off'):t==='pw'?(c[k]?'set':'<span class="no">none</span>'):`<span class="m">${esc(c[k]??'')||'—'}</span>`)).join('')}</div>${msgDiv('ha-cfg','padding:0 18px 12px')}</div>`:'';
  return `${warn.length?`<div class="card"><div class="note" style="border-top:0">${ic('alert')}<div>${warn.join('<br>')}</div></div></div>`:''}${nodes}${ctl}${form}
   <div class="card"><div class="ch"><h2>Role changes</h2><span class="cnt">${(d.transitions||[]).length}</span></div><div class="tw"><table><thead><tr><th>Time</th><th>From</th><th>To</th><th>Reason</th></tr></thead><tbody>
   ${(d.transitions||[]).map(t=>`<tr><td class="m">${tmd(t.ts_ms)}</td><td>${haRole(t.from)}</td><td>${haRole(t.to)}</td><td>${esc(t.reason)}</td></tr>`).join('')||'<tr><td colspan="4" class="mut">No role changes yet</td></tr>'}
   </tbody></table></div></div>`;
};
const haMe=()=>D.ha?D.ha.node:'this node',haPeer=()=>D.ha&&D.ha.peer?D.ha.peer.node:'the peer';
async function haPost(url,body,msg){try{const r=await postJ(url,body);setMsg(msg,'ok',r&&typeof r.result==='string'?r.result:'Done.');}catch(e){setMsg(msg,'err',e.message);}loadPageData('ha',false);}
function haAct(which){const d=D.ha||{},p=d.peer||{},r=d.replication||{};
  const repl=(who,ok)=>ok?'':`\n\nWarning: the copy of the call history and SMS Center queue on ${who} is not up to date; anything not copied yet stays behind.`;
  const msg=which==='active'
    ?`Make ${haMe()} Active?\n\n${haPeer()} hands the virtual IP over to ${haMe()}. Basestations, peers and SIP calls on ${haPeer()} are dropped and reconnect here; ${haPeer()} restarts into Standby.\n\n${haMe()} then keeps the virtual IP (manual hold) until it leaves Active or a Persist button is pressed.`+repl(haMe(),!r.needed||r.synced)
    :`Make ${haMe()} Standby?\n\n${haPeer()} takes over the virtual IP. Basestations, peers and SIP calls on ${haMe()} are dropped and reconnect to ${haPeer()}; ${haMe()} restarts into Standby.\n\n${haPeer()} then keeps the virtual IP (manual hold) until it leaves Active or a Persist button is pressed.`+repl(haPeer(),p.synced!==false);
  if(confirm(msg))haPost('/api/ha/'+which,{},'ha-act');}
function haPersist(v){
  if(v===null){const c=D.haCfg?(D.haCfg.persist?'on':'off'):'the config file value';
    if(!confirm(`Reset Persist on ${haMe()} to the config default (${c})?\n\nThis removes the dashboard override and ends any manual hold. If Persist ends up off and ${haPeer()} has the higher weight, ${haPeer()} takes over the virtual IP once its copy of the data is up to date, and sessions on ${haMe()} reconnect there.`))return;}
  haPost('/api/ha/persist',{persist:v},'ha-act');}
async function haApply(force){
  const d=D.ha||{},changes=(d.pending_diff||[]).join(', ');
  const what=changes?`Changes to apply: ${changes}`:'No setting changes are pending; this only restarts the node.';
  const how=d.role==='active'?`${haMe()} is Active: it hands the virtual IP over to ${haPeer()} first, then restarts. Sessions on ${haMe()} reconnect to ${haPeer()}.`:`${haMe()} is ${d.role||'not Active'}: it restarts; service on ${haPeer()} is not affected.`;
  if(!force&&!confirm(`Apply the saved config on ${haMe()}?\n\n${what}\n\n${how}`))return;
  const r=await fetch('/api/ha/apply',{method:'POST',headers:{'content-type':'application/json','x-brew-ui':'1'},body:JSON.stringify({force})});const t=await r.text();
  if(!r.ok&&r.status===409&&!force){if(confirm(t+'\n\nRestart anyway?'))return haApply(true);}
  let m=t;try{m=JSON.parse(t).result;}catch(e){}setMsg('ha-act',r.ok?'ok':'err',r.ok?String(m):t);render(true);}
function haEditCfg(){const c=D.haCfg||{};
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">High Availability settings</h3></div><div class="sb">
   ${HA_F.map(([k,t,l])=>t==='sw'?swRow('ha-'+k,c[k],l,''):fld('ha-'+k,l,c[k]??'',(t==='n'?'inputmode="numeric" ':'')+(t==='pw'?'type="password" autocomplete="new-password" ':'autocomplete="off" ')+'spellcheck="false"')).join('')}
   <div class="mut" style="font-size:12px">Saving writes <code>[ha]</code> to the config file; apply it with "Apply saved config".</div><div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="haSave()">Save</button></div>`;
  $('scrim').classList.add('on');}
async function haSave(){
  const c=Object.assign({},D.haCfg||{}),m=$('sheet-msg');
  for(const [k,t] of HA_F){c[k]=t==='sw'?on('ha-'+k):t==='n'?Number($('ha-'+k).value):$('ha-'+k).value.trim();}
  m.className='msg mut';m.textContent='Saving…';
  try{await api('/api/config/ha',{method:'PUT',headers:{'content-type':'application/json'},body:JSON.stringify(c)});D.haCfg=c;closeSheet();setMsg('ha-cfg','ok','Saved to the config file. Apply it with "Apply saved config".');render(true);}
  catch(e){m.className='msg err';m.textContent=e.message;}}

// Server settings: ISSI block list and allow list (live), static station positions.
R.settings=()=>{
  if(!D.who.admin)return adminOnly('Server settings');
  const b=D.blk,a=D.allow,l=D.btsLocs;if(!b||!a||!l)return loading;
  const issiRows=(list,btn,fn,empty)=>list.map(i=>`<tr><td class="m"><b>${issiTxt(i)}</b></td><td style="text-align:right"><button class="btn" onclick="${fn}(${i})">${btn}</button></td></tr>`).join('')||`<tr><td colspan="2" class="mut">${empty}</td></tr>`;
  const alState=a.enabled?(a.issis.length?`<span class="chip a">Enforced</span> ${a.issis.length} ISSI${a.issis.length===1?'':'s'} admitted, every other radio is barred.`:'<span class="chip d">Enforced, empty</span> Every radio is barred until one is admitted.'):'<span class="chip i">Not enforced</span> The list is kept, every radio may communicate.';
  return `<div class="card"><div class="ch"><h2>ISSI block list</h2><span class="cnt">${b.length}</span>${helpBtn('blk')}</div>${helpNote('blk','A blocked ISSI cannot transmit to groups, make or receive individual calls, or send or receive SDS through this server. Ambience listening (SS-AL) and LIP position reports still work, and it can still register. Applies at once, no restart (<code>[blacklist]</code>).')}
    <div class="tgadd"><input class="f" id="blk-issi" placeholder="ISSI to block, e.g. 4013" inputmode="numeric" style="flex:1;min-width:160px" onkeydown="if(event.key==='Enter')issiList('blacklist','POST',this.value)"><button class="btn pri" onclick="issiList('blacklist','POST',$('blk-issi').value)">${ic('plus')}Block</button></div>
    <div class="tw"><table><tbody>${issiRows(b,'Unblock','unblockIssi','No ISSI is blocked')}</tbody></table></div>${msgDiv('blk-msg','padding:12px 18px')}</div>
   <div class="card"><div class="ch"><h2>ISSI allow list</h2><span class="cnt">${a.issis.length}</span>${helpBtn('alw')}<span class="sp"></span><label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600">Enforce<button class="tog" role="switch" aria-checked="${!!a.enabled}" aria-label="Enforce the allow list" onclick="allowEnforce(this.getAttribute('aria-checked')!=='true')"></button></label></div>${helpNote('alw','The closed-network counterpart of the block list: while enforced, only the listed ISSIs may transmit to groups, make or receive individual calls, or send or receive SDS through this server, whichever station they are on. Any other radio is barred like a blocked one (SS-AL, LIP and emergency calls still pass, and it can still register). The block list applies on top. Applies at once, no restart.')}
    <div class="svc">${alState}</div>
    <div class="tgadd"><input class="f" id="al-issi" placeholder="ISSI to admit, e.g. 4013" inputmode="numeric" style="flex:1;min-width:160px" onkeydown="if(event.key==='Enter')issiList('allowlist','POST',this.value)"><button class="btn pri" onclick="issiList('allowlist','POST',$('al-issi').value)">${ic('plus')}Admit</button></div>
    <div class="tw"><table><tbody>${issiRows(a.issis,'Remove','disallowIssi','No ISSI is on the allow list')}</tbody></table></div>${msgDiv('al-msg','padding:12px 18px')}</div>
   <div class="card"><div class="ch"><h2>Station positions (static)</h2><span class="cnt">${l.length}</span>${helpBtn('btsl')}<span class="sp"></span><a data-go="accounts" href="#accounts">Accounts ›</a></div>${helpNote('btsl','Configured names and positions of stations by Brew login (<code>[bts_locations]</code>), shown on the map. A position a station reports over telemetry, or one relayed by another server, comes first. Usually set together with the login on the <a data-go="accounts" href="#accounts">Accounts</a> page. Saving restarts the server.')}
    <div class="tw"><table><thead><tr><th>Login</th><th>Name</th><th>Position</th><th></th></tr></thead><tbody>
    ${l.map(x=>`<tr><td class="m"><b>${esc(x.username)}</b></td><td>${esc(x.name)}</td><td class="m" style="font-size:12px">${(+x.lat).toFixed(4)}, ${(+x.lon).toFixed(4)}</td><td style="text-align:right"><button class="btn danger" onclick="btsLocDel('${esc(x.username)}')">Delete</button></td></tr>`).join('')||'<tr><td colspan="4" class="mut">No static station positions</td></tr>'}
    </tbody></table></div>${msgDiv('btsl-msg','padding:12px 18px')}</div>
   ${mapSettingsCard()}
   ${parrotCard()}
   ${updatesRepoCard()}
   ${fleetCfgSettingsCard()}
   ${fleetMonSettingsCard()}
   ${usersCard()}`;
};
// Server settings: the map's radio layer ([map], live, no restart).
function mapSettingsCard(){
  const m=D.mapSet;if(!m||!m.settings)return '';const c=m.settings;
  return `<div class="card"><div class="ch"><h2>Map</h2>${helpBtn('mapset')}<span class="sp"></span><a data-go="map" href="#map">Map ›</a></div>${helpNote('mapset','<i>Positions per radio</i>: how many of a radio\'s last positions are kept (0 to 500); clicking a radio on the map shows them as a track. They survive a restart. <i>Dock after</i>: a radio whose last position is older than this is shown next to the station it is registered on again (0 = only when it registers on another station). A radio without any position is always shown next to its station. <i>Guests see tracks</i>: off, visitors who are not signed in see only the current positions. Applies at once, no restart (<code>[map]</code>).')}
   <div class="svc" style="display:grid;gap:10px;grid-template-columns:minmax(0,1fr) minmax(0,1fr) auto;align-items:end">
    <div><label class="l" for="map-pts">Positions per radio</label><input class="f" id="map-pts" type="number" min="0" max="500" value="${c.track_points}" style="width:100%"></div>
    <div><label class="l" for="map-dock">Dock after (minutes)</label><input class="f" id="map-dock" type="number" min="0" max="10080" value="${c.dock_after_min}" style="width:100%"></div>
    <button class="btn pri" onclick="saveMapSettings()">Save</button></div>
   <div class="svc"><label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600"><button class="tog" role="switch" id="map-guest" aria-checked="${c.guests_see_track!==false}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Guests see tracks</label><span class="mut" style="font-size:12px">Visitors who are not signed in</span></div>
   <div class="svc"><button class="btn danger" onclick="clearMapTracks()">Clear map</button><span class="mut" style="font-size:12px">Forgets all tracks and every radio's last position. Registered radios stay next to their station until they send a position again.</span></div>${msgDiv('map-msg','padding:0 18px 12px')}</div>`;
}
async function saveMapSettings(){
  try{const c=await postJ('/api/map/settings',{track_points:Number(v('map-pts')),dock_after_min:Number(v('map-dock')),guests_see_track:$('map-guest').getAttribute('aria-checked')==='true'});D.mapSet={...D.mapSet,settings:c};setMsg('map-msg','ok','Saved');render(true);}catch(e){setMsg('map-msg','err',e.message);}
}
async function clearMapTracks(){
  if(!confirm('Clear the map?\n\nForgets all tracks and every radio\'s last position. Stations stay.'))return;
  try{await api('/api/map/clear',{method:'POST'});setMsg('map-msg','ok','Map cleared.');}catch(e){setMsg('map-msg','err',e.message);}
}
// Server settings: the parrot ([parrot], live, no restart).
function parrotCard(){
  const p=D.parrot;if(!p)return '';
  const tg=(id,on,lbl,extra)=>`<label style="display:flex;gap:10px;align-items:center;font-size:13px;font-weight:600"><button class="tog" role="switch" id="${id}" aria-checked="${!!on}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true');${extra||''}"></button> ${lbl}</label>`;
  return `<div class="card"><div class="ch"><h2>Parrot</h2>${helpBtn('parrot')}<span class="sp"></span>${tg('par-on',p.enabled,'On')}</div>${helpNote('parrot','A number radios call to test themselves. <i>Individual call</i> to the ISSI: the server answers; in a simplex call each over comes back after the delay, in a duplex call everything comes back after the delay (an echo) and the call ends after the maximum length. <i>Group</i>: an over on the parrot group is played back on the group, as the parrot ISSI, so every radio in the group hears it. The group has the same number as the ISSI unless <i>Separate GSSI</i> is on (GSSI empty or 0 = no group). An over is recorded for at most the maximum length. On saving, the group is added to the allowed talkgroups and both numbers are named Parrot in the Directory (an existing name stays). <i>Tail</i>: how long the floor stays with the parrot after the last played frame, so the station plays out its buffer before the end is cut (default 500 ms). Stations need nothing new: the group must be routed to Brew on them like any network group. Applies at once, no restart (<code>[parrot]</code>).')}
   <div class="svc" style="display:grid;gap:10px;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));align-items:end">
    <div><label class="l" for="par-issi">ISSI</label><input class="f" id="par-issi" type="number" min="1" max="16777215" inputmode="numeric" value="${p.issi||''}" placeholder="e.g. 9990" style="width:100%"></div>
    <div><label class="l" for="par-gssi">GSSI</label><input class="f" id="par-gssi" type="number" min="0" max="16777215" inputmode="numeric" value="${p.separate_gssi?(p.gssi||''):''}" placeholder="${p.separate_gssi?'0 = no group':'same as ISSI'}" ${p.separate_gssi?'':'disabled'} style="width:100%"></div>
    <div><label class="l" for="par-delay">Delay (ms)</label><input class="f" id="par-delay" type="number" min="0" max="60000" step="100" value="${p.delay_ms}" style="width:100%"></div>
    <div><label class="l" for="par-tail">Tail (ms)</label><input class="f" id="par-tail" type="number" min="0" max="5000" step="100" value="${p.tail_ms??500}" style="width:100%"></div>
    <div><label class="l" for="par-max">Maximum length (s)</label><input class="f" id="par-max" type="number" min="1" max="600" value="${p.max_secs}" style="width:100%"></div>
   </div>
   <div class="svc" style="display:flex;gap:14px;align-items:center;flex-wrap:wrap">${tg('par-sep',p.separate_gssi,'Separate GSSI',"parrotSep()")}<span class="sp" style="flex:1"></span><button class="btn pri" onclick="saveParrot()">Save</button></div>${msgDiv('par-msg','padding:0 18px 12px')}</div>`;
}
function parrotSep(){
  const on=$('par-sep').getAttribute('aria-checked')==='true',g=$('par-gssi');
  g.disabled=!on;g.placeholder=on?'0 = no group':'same as ISSI';if(!on)g.value='';
}
async function saveParrot(){
  const on=x=>$(x).getAttribute('aria-checked')==='true';
  try{const c=await postJ('/api/parrot/set',{enabled:on('par-on'),issi:Number(v('par-issi'))||0,separate_gssi:on('par-sep'),gssi:Number(v('par-gssi'))||0,delay_ms:Number(v('par-delay')),tail_ms:Number(v('par-tail')),max_secs:Number(v('par-max'))});D.parrot=c;setMsg('par-msg','ok','Saved');render(true);}catch(e){setMsg('par-msg','err',e.message);}
}
// Server settings: the reference source for Fleet Control updates (hint only, stations keep their own).
function updatesRepoCard(){
  const f=D.fleet;if(!f||!f.update_settings)return '';
  const s=f.update_settings,r=f.update_source;
  return `<div class="card"><div class="ch"><h2>Updates repo</h2>${helpBtn('updrepo')}<span class="sp"></span><a data-go="fleet" href="#fleet">Fleet Control ›</a></div>${helpNote('updrepo','Repository and branch the Fleet Control page compares the stations with: a station that updates from another source gets the hint ⚠ other repo/branch. Empty: the product repository, branch main. The stations keep updating from their own source; this setting does not change them.')}
   <div class="svc" style="display:grid;gap:10px;grid-template-columns:minmax(0,2fr) minmax(0,1fr) auto;align-items:end">
    <div><label class="l" for="ur-repo">Repository</label><input class="f" id="ur-repo" value="${esc(s.repo)}" placeholder="${esc('https://github.com/Aitorrio/bost-flowstation')}" autocomplete="off" spellcheck="false" autocapitalize="off" style="width:100%"></div>
    <div><label class="l" for="ur-branch">Branch</label><input class="f" id="ur-branch" value="${esc(s.branch)}" placeholder="main" autocomplete="off" spellcheck="false" autocapitalize="off" style="width:100%"></div>
    <button class="btn pri" onclick="saveUpdatesRepo()">Save</button></div>
   <div class="svc mut" style="font-size:12.5px">In effect: <span class="m">${esc(r.repo)}</span> (${esc(r.branch)})</div>${msgDiv('ur-msg','padding:0 18px 12px')}</div>`;
}
async function saveUpdatesRepo(){
  try{await postJ('/api/fleet/settings',{repo:v('ur-repo').trim(),branch:v('ur-branch').trim()});D.fleet=await getJ('/api/fleet');setMsg('ur-msg','ok','Saved');render(true);}catch(e){setMsg('ur-msg','err',e.message);}
}
// Dashboard users (Server settings, admins): who logs in, who is admin, which pages each one sees.
const pageName=id=>id==='fleet_manage'?'Fleet Control management':id==='radioctl_manage'?'DGNA & Radio actions: send':PG[id]?PG[id][1]:id;
const pageChips=(list,all)=>all?'<span class="chip a">Every page · edits</span>':(list.length?list.map(p=>`<span class="chip">${esc(pageName(p))}</span>`).join(' '):'<span class="mut">no page</span>');
function usersCard(){
  const u=D.dusers;if(!u)return loading;
  const adminPages=PAGES.filter(p=>p[5]).map(p=>p[1]).join(', ');
  return `<div class="card"><div class="ch"><h2>Dashboard users</h2><span class="cnt">${u.users.length}</span>${helpBtn('dusr')}<span class="sp"></span><button class="btn pri" onclick="editUser(-1)">${ic('plus')}New user</button></div>
   ${helpNote('dusr',`Who may log in to this dashboard. An <b>admin</b> sees every page and may change everything. Any other user only sees the pages ticked for it and changes nothing. ${esc(adminPages)} are for admins only. <b>Viewer</b> is whoever is not logged in: with the open dashboard (<code>[dashboard.ui] open = true</code>) it sees the pages ticked for Viewer, with a closed dashboard nothing. Passwords are stored as hashes in <code>${esc(u.file)}</code>. Until the first change here the users come from <code>[dashboard] users</code> / <code>admins</code> in the config; to get back in after losing the admin password, delete that file and restart: the users of the config are back.`)}
   ${!D.who.auth?`<div class="svc"><span class="chip w">No users</span> There is no login: anyone who opens this dashboard is an admin. Add an admin user.</div>`:''}
   ${D.who.auth&&!u.from_file?`<div class="svc mut" style="font-size:12.5px">From the config (<code>[dashboard] users</code>) until the first change here.</div>`:''}
   <div class="tw"><table><thead><tr><th>User</th><th>Sees</th><th></th></tr></thead><tbody>
   ${u.users.map((x,i)=>`<tr><td><b class="m">${esc(x.name)}</b>${x.admin?' <span class="chip a">Admin</span>':''}${x.name===D.who.username?' <span class="mut" style="font-size:12px">(you)</span>':''}</td><td>${pageChips(x.pages,x.admin)}</td><td style="text-align:right;white-space:nowrap"><button class="btn" onclick="editUser(${i})">Edit</button> <button class="btn danger hide-s" onclick="delUser(${i})">Delete</button></td></tr>`).join('')||'<tr><td colspan="3" class="mut">No users</td></tr>'}
   <tr><td><b>Viewer</b><span class="sub">not logged in · ${D.who.open?'open dashboard':'closed dashboard: sees nothing'}</span></td><td>${pageChips(u.viewer,false)}</td><td style="text-align:right"><button class="btn" onclick="editUser('viewer')">Edit</button></td></tr>
   </tbody></table></div>${msgDiv('dusr-msg','padding:12px 18px')}</div>`;
}
function editUser(i){
  const u=D.dusers,viewer=i==='viewer';
  const x=viewer?{name:'Viewer',admin:false,pages:u.viewer}:i>=0?u.users[i]:{name:'',admin:false,pages:u.viewer};
  const box=(id,label)=>`<label style="display:flex;gap:8px;align-items:center;font-size:13px"><input type="checkbox" class="chk du-pg" value="${id}" ${x.admin||x.pages.includes(id)?'checked':''} ${x.admin?'disabled':''}> ${esc(label)}</label>`;
  // Fleet Control and DGNA & Radio actions: seeing the page, and acting there (includes seeing; not for Viewer).
  const boxes=PAGES.filter(p=>!p[5]).map(p=>box(p[0],p[0]==='fleet'?'Fleet Control visibility':p[0]==='radioctl'?'DGNA & Radio actions: view':p[1])).join('')+box('tsds','Telemetry SDS (list on the SDS page)')+(viewer?'':box('fleet_manage','Fleet Control management')+box('radioctl_manage','DGNA & Radio actions: send'));
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${viewer?'Viewer (not logged in)':i>=0?'Edit user '+esc(x.name):'New user'}</h3></div><div class="sb">
   ${viewer?`<div class="mut" style="font-size:12.5px">What anyone sees without logging in${D.who.open?'':' once the dashboard is open (<code>[dashboard.ui] open = true</code>; it is closed now, so nobody sees anything without logging in)'}.</div>`:`
   <div><label class="l" for="du-name">User name</label><input class="f" id="du-name" value="${esc(x.name)}" ${i>=0?'disabled':''} placeholder="e.g. oe4sxm" autocomplete="off" spellcheck="false" autocapitalize="off"></div>
   <div><label class="l" for="du-pw">${i>=0?'New password (empty: unchanged; signs the user out elsewhere)':'Password'}</label><div style="display:flex;gap:8px"><input class="f" id="du-pw" type="text" autocomplete="new-password" spellcheck="false" placeholder="${i>=0?'unchanged':'at least 6 characters'}" style="flex:1"><button type="button" class="btn" onclick="$('du-pw').value=genPw()">Generate</button></div></div>
   <label style="display:flex;gap:12px;align-items:center"><button type="button" class="tog" role="switch" id="du-admin" aria-checked="${x.admin}" onclick="duAdmin(this)"></button><span><b style="font-size:13.5px">Admin</b><br><span class="mut" style="font-size:12px">Every page, and may change everything</span></span></label>`}
   <div><span class="l" style="display:block">Pages ${viewer?'Viewer':'this user'} sees</span><div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:8px;margin-top:6px">${boxes}</div>
   <div class="mut" style="font-size:12px;margin-top:6px">${esc(PAGES.filter(p=>p[5]).map(p=>p[1]).join(', '))}: admins only. Station addresses are shown to admins only.</div></div>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts">${!viewer&&i>=0?`<button class="btn danger" style="margin-right:auto" onclick="closeSheet();delUser(${i})">${ic('trash')}Delete</button>`:''}<button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveUser(${viewer?"'viewer'":i})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>{const f=$(i>=0?'du-pw':'du-name');if(f)f.focus();},30);
}
function duAdmin(b){const on=b.getAttribute('aria-checked')!=='true';b.setAttribute('aria-checked',on);
  document.querySelectorAll('.du-pg').forEach(c=>{c.disabled=on;if(on)c.checked=true;});}
async function saveUser(i){
  const m=$('sheet-msg');const pages=[...document.querySelectorAll('.du-pg')].filter(c=>c.checked).map(c=>c.value);
  m.className='msg mut';m.textContent='Saving…';
  try{
    if(i==='viewer'){D.dusers=await postJ('/api/dashboard-users/viewer',{pages});closeSheet();setMsg('dusr-msg','ok','Viewer pages saved.');}
    else{const name=$('du-name').value.trim(),first=!D.who.auth;
      D.dusers=await postJ('/api/dashboard-users',{name,password:$('du-pw').value,admin:on('du-admin'),pages});closeSheet();
      setMsg('dusr-msg','ok',`${name} saved.${first?' The dashboard now needs a login.':''}`);
      if(first){await refreshSlow();openLogin(false);}}
    await refreshSlow();
  }catch(e){m.className='msg err';m.textContent=e.message;return;}
  render(true);
}
async function delUser(i){
  const x=D.dusers.users[i];if(!confirm(`Delete dashboard user "${x.name}"? It is logged out at once.`))return;
  try{D.dusers=await api('/api/dashboard-users/'+encodeURIComponent(x.name),{method:'DELETE'});setMsg('dusr-msg','ok',`${x.name} deleted.`);}catch(e){setMsg('dusr-msg','err',e.message);}
  render(true);
}
async function issiList(which,method,v){const i=parseInt(String(v).trim(),10);const msg=which==='blacklist'?'blk-msg':'al-msg';
  if(!(i>0&&i<=16777215)){setMsg(msg,'err','An ISSI is a number from 1 to 16777215.');render(true);return;}
  try{await api(`/api/config/${which}/${i}`,{method});setMsg(msg,'ok','Applied at once.');}catch(e){setMsg(msg,'err',e.message);}
  [D.blk,D.allow]=await Promise.all([getJ('/api/config/blacklist'),getJ('/api/config/allowlist')]);render(true);}
const unblockIssi=i=>issiList('blacklist','DELETE',i),disallowIssi=i=>issiList('allowlist','DELETE',i);
async function allowEnforce(v){
  if(v&&D.allow&&!D.allow.issis.length&&!confirm('The allow list is empty: enforcing it bars every radio until one is admitted. Enforce anyway?'))return;
  try{await api('/api/config/allowlist/enabled',{method:'PUT',headers:{'content-type':'application/json'},body:JSON.stringify({enabled:v})});setMsg('al-msg','ok','Applied at once.');}catch(e){setMsg('al-msg','err',e.message);}
  D.allow=await getJ('/api/config/allowlist');render(true);}
async function btsLocDel(u){if(!confirm(`Delete the static position of ${u}? The server restarts to apply it.`))return;
  try{await api('/api/config/bts-locations/'+encodeURIComponent(u),{method:'DELETE'});setMsg('btsl-msg','ok','Deleted. The server restarts to apply it.');D.btsLocs=D.btsLocs.filter(x=>x.username!==u);}catch(e){setMsg('btsl-msg','err',e.message);}render(true);}

buildNav();
show(location.hash.slice(1)||'overview');
refreshFast();refreshSlow();
setInterval(refreshFast,2000);setInterval(refreshSlow,5000);
setInterval(()=>{if(UI.page==='overview'||UI.page==='calls')render();},1000);
</script>
</body></html>"##;

#[cfg(test)]
mod tests {
    use super::PAGE_HTML;

    #[test]
    fn sidebar_starts_with_the_logo_and_the_tab_has_the_favicon() {
        let page = super::PAGE.as_str();
        assert!(!page.contains("__LOGO__"));
        let brand = page.find(r#"<div class="brand">"#).expect("brand");
        let logo = page.find(r#"<svg class="blogo""#).expect("logo inlined");
        let name = page[brand..].find(r#"<div class="bname">"#).expect("name") + brand;
        assert!(brand < logo && logo < name, "logo first, then the server name");
        assert!(page.contains(r#"<link rel="icon" href="/favicon.svg" type="image/svg+xml">"#));
        assert!(page.contains(r#"<img class="lmark" src="/favicon.svg""#), "sign-in window shows the mug");
    }

    #[test]
    fn logo_files_carry_no_metadata_and_the_favicon_follows_dark_mode() {
        for svg in [super::LOGO_SVG, super::FAVICON_SVG] {
            assert!(svg.trim_start().starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
            assert!(!svg.contains("<metadata"));
        }
        assert!(super::FAVICON_SVG.contains("prefers-color-scheme:dark"));
        for c in ["bl-w", "bl-t", "bl-s", "bl-g"] {
            assert!(super::LOGO_SVG.contains(&format!(r#"class="{c}""#)), "{c} themable");
        }
    }

    #[tokio::test]
    async fn app_manifest_and_icons_are_served() {
        use axum::response::IntoResponse;
        let page = super::PAGE.as_str();
        assert!(page.contains(r#"<link rel="manifest" href="/manifest.webmanifest">"#));
        assert!(page.contains(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#));
        let r = super::manifest().await.into_response();
        assert_eq!(r.headers()["content-type"], "application/manifest+json");
        let body = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap();
        let m: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(m["name"], super::APP_NAME);
        assert_eq!(m["display"], "standalone");
        assert_eq!(m["icons"].as_array().unwrap().len(), 2);
        for (bytes, size) in [(super::ICON_192, 192u32), (super::ICON_512, 512), (super::APPLE_TOUCH_ICON, 180)] {
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), size, "width");
            assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), size, "height");
        }
        let r = super::icon_512().await.into_response();
        assert_eq!(r.headers()["content-type"], "image/png");
    }

    #[tokio::test]
    async fn favicon_is_served_as_svg() {
        use axum::response::IntoResponse;
        let r = super::favicon().await.into_response();
        assert_eq!(r.headers()[axum::http::header::CONTENT_TYPE], "image/svg+xml");
    }

    /// The polls must not ask admin-only routes for a visitor who is not signed in: their 401
    /// opens the login window, so an open (read-only) dashboard would show it every 5 s.
    #[test]
    fn fleet_run_card_has_a_whole_run_bar() {
        let start = PAGE_HTML.find("function fleetJobPercent(").expect("fleetJobPercent");
        let body = &PAGE_HTML[start..start + PAGE_HTML[start..].find("\n}\n").expect("end")];
        assert!(body.contains("['done','failed','skipped'].includes(s.state.state)") || body.contains("['done','failed','skipped'].includes(s.state)"));
        assert!(body.contains("s.state==='restarting'?0.95") && body.contains("s.state==='running'?0.9*"));
        assert!(PAGE_HTML.contains("const p=fleetJobPercent(job);"), "the run card shows it");
    }

    #[test]
    fn fleet_run_card_hides_only_after_a_clean_end() {
        let start = PAGE_HTML.find("function fleetJobHidden(").expect("fleetJobHidden");
        let body = &PAGE_HTML[start..start + PAGE_HTML[start..].find("\n}\n").expect("end")];
        assert!(body.contains("if(live||!job.finished_ms||job.cancelled)return false;"), "stays while running or after a cancel");
        assert!(body.contains("['failed','skipped'].includes(i.state.state)"), "stays after a failed or skipped station");
        assert!(PAGE_HTML.contains("const FLEET_JOB_HIDE_MS=30000;"));
        assert!(PAGE_HTML.contains("const jobCard=job&&!fleetJobHidden(job,live)?"), "the card uses it");
    }

    /// Each login picks its map symbol (station, tower, terminal) and sends it with the save;
    /// the map's track length and dock time are set on the Settings page.
    #[test]
    fn accounts_pick_a_map_symbol_and_map_settings_are_editable() {
        let edit = &PAGE_HTML[PAGE_HTML.find("function editAcc(").expect("editAcc")..];
        let edit = &edit[..edit.find("\n}\n").expect("end")];
        assert!(edit.contains("id=\"ac-icon\"") && edit.contains("['tbs','Tower (TBS)']") && edit.contains("['terminal','Terminal']"));
        assert!(PAGE_HTML.contains("lat,lon,icon:$('ac-icon').value}"), "saved with the login");
        assert!(PAGE_HTML.contains("function mapSettingsCard(") && PAGE_HTML.contains("/api/map/settings"));
        assert!(PAGE_HTML.contains("id=\"map-guest\"") && PAGE_HTML.contains("guests_see_track:$('map-guest')"), "guests see tracks: a switch");
        // The function ends before the next card's comment (a missing brace broke the whole script).
        assert!(PAGE_HTML.contains("setMsg('map-msg','err',e.message);}\n}\n// Server settings: the parrot"), "clearMapTracks is closed");
    }

    /// The parrot is set on the Settings page (on/off, ISSI, separate GSSI, delay, length).
    #[test]
    fn parrot_settings_are_editable() {
        assert!(PAGE_HTML.contains("function parrotCard(") && PAGE_HTML.contains("/api/parrot/set") && PAGE_HTML.contains("Separate GSSI"));
        assert!(PAGE_HTML.contains("set('parrot','/api/parrot')"), "loaded with the Settings page");
        assert!(PAGE_HTML.contains(r#"id="par-tail""#) && PAGE_HTML.contains("tail_ms:Number(v('par-tail'))"), "the tail is set on the card");
    }

    #[test]
    fn polls_ask_admin_routes_only_for_admins() {
        let start = PAGE_HTML.find("async function refreshSlow(){").expect("refreshSlow");
        let body = &PAGE_HTML[start..];
        let body = &body[..body.find("\n}\n").expect("end of refreshSlow")];
        for route in ["'/api/networks'", "'/api/accounts'", "'/api/control'", "'/api/config/"] {
            for line in body.lines().filter(|l| l.contains(&format!("getJ({route}"))) {
                assert!(line.contains(".admin"), "{route} polled without an admin check: {line}");
            }
        }
    }

    /// Stations and cells are named the same way on every page: a station by its Brew login, a
    /// cell by its global cell ID (login/cell), with telemetry adding the local cell number; the
    /// help says that the global ID is not an ETSI identity and that terminals have no cells.
    #[test]
    fn stations_and_cells_are_named_the_same_everywhere() {
        assert!(!PAGE_HTML.contains("Station / cell"), "station and cell in their own columns");
        assert!(PAGE_HTML.contains("<th>Cell (global ID)</th>"));
        assert!(PAGE_HTML.contains("Cell (local / global ID)"), "telemetry: local number and global ID");
        assert!(PAGE_HTML.contains("it is not an ETSI identity"));
        assert!(PAGE_HTML.contains("Apps and other terminals (Terminal mode) have no cells"), "cells help");
        assert!(PAGE_HTML.contains("a station without it is counted as a station, its cells are unknown"));
        assert!(PAGE_HTML.contains("bs.map(b=>") && PAGE_HTML.contains("term.map(b=>") && PAGE_HTML.contains("function connGroups("), "one row per station, not per connection");
        let st = PAGE_HTML.find("<h2>Stations</h2><span class=\"cnt\">${bs.length}").expect("stations card");
        let te = PAGE_HTML.find("<h2>Terminals &amp; bridges</h2><span class=\"cnt\">${term.length}").expect("terminals card");
        let ce = PAGE_HTML[te..].find("<h2>Cells</h2>").expect("cells card after terminals");
        assert!(st < te && ce > 0, "Stations & Cells page: stations, then terminals, then cells");
        for page in [&*crate::dashboard::TELEMETRY_SDS_HTML, &*crate::dashboard::REGISTRATIONS_HTML] {
            assert!(!page.contains("<th>BTS</th>"), "Station, not BTS");
        }
    }

    /// Linked talkgroups have their own list and editor on the Routing page and
    /// are saved as pools with `linked`.
    #[test]
    fn routing_page_lists_linked_talkgroups() {
        assert!(PAGE_HTML.contains("<h2>Linked talkgroups</h2>"));
        assert!(PAGE_HTML.contains("function editLink(") && PAGE_HTML.contains("function saveLink("));
        assert!(PAGE_HTML.contains("id=\"le-name\""), "a link has a name field");
        assert!(PAGE_HTML.contains("linked:!!p.linked"), "the flag survives every save of the pool list");
        assert!(PAGE_HTML.contains("if(p.linked)return '';"), "links are not listed again as pools");
    }

    /// Recent calls show one row per pool call, naming the pool, instead of a row per group.
    #[test]
    fn recent_calls_merge_the_legs_of_a_pool_call() {
        let start = PAGE_HTML.find("R.calls=()=>{").expect("R.calls");
        let body = &PAGE_HTML[start..start + PAGE_HTML[start..].find("\n};\n").expect("end")];
        assert!(body.contains("mergePoolLegs(st.recent_calls)"), "recent calls merged");
        assert!(body.contains("c.pool?"), "pool name in the To column");
        // Without the admin-only network list the alias network comes from the call.
        let pg = &PAGE_HTML[PAGE_HTML.find("function inPool(p,c){").expect("inPool")..];
        assert!(pg[..pg.find("\n}").unwrap_or(pg.len()).max(400).min(pg.len())].contains("(c.to_networks||[])[0]||c.from_network"), "works logged out");
        let f = &PAGE_HTML[PAGE_HTML.find("function mergePoolLegs(list){").expect("mergePoolLegs")..start];
        // A group in several pools: the pools holding at least two of the call's groups name it.
        for needle in ["o.source!==c.source", "3000", "!cand.some(p=>inPool(p,o))", "low(o.destination)!==low(c.destination)", "legs.filter(x=>inPool(p,x)).length>=2"] {
            assert!(f.contains(needle), "{needle}");
        }
    }

    /// A station's telemetry card lists its calls with carrier and timeslot and draws the four
    /// timeslots of every carrier in use, as the classic Basestation Telemetry panel did.
    #[test]
    fn telemetry_shows_calls_and_timeslots() {
        let start = PAGE_HTML.find("R.telemetry=()=>{").expect("R.telemetry");
        let body = &PAGE_HTML[start..start + PAGE_HTML[start..].find("\n};\n").expect("end")];
        assert!(body.contains("${tlmCalls(s)}"), "calls on the card");
        let f = &PAGE_HTML[PAGE_HTML.find("function tlmCalls(s){").expect("tlmCalls")..start];
        for needle in ["Carrier / TS", "[1,2,3,4]", "c.carrier_num", "c.ts", "available"] {
            assert!(f.contains(needle), "{needle}");
        }
    }

    /// The Overview shows three pairs side by side, in this order (stacked the same way on a
    /// phone): live calls | recent calls, stations | radios, cell changes | recent SDS.
    #[test]
    fn overview_pairs_in_order() {
        let start = PAGE_HTML.find("R.overview=()=>{").expect("overview");
        let body = &PAGE_HTML[start..];
        let body = &body[..body.find("\n};\n").expect("end of overview")];
        let mut at = 0;
        for h in ["Live calls", "Recent calls", "Stations</h2>", "Radios</h2>", "Recent cell changes", "Recent SDS"] {
            let i = body[at..].find(h).unwrap_or_else(|| panic!("{h} missing or out of order"));
            at += i + h.len();
        }
        assert_eq!(body.matches("<div class=\"grid2 ov\">").count(), 3);
        assert!(PAGE_HTML.contains(".grid2,.grid2.ov{grid-template-columns:1fr}"), "phone stacks the pairs");
    }

    /// Managed talkgroups live on the Routing page in a card that folds to a
    /// bar (folded while off, open while on); the Directory only names groups.
    #[test]
    fn managed_talkgroups_card_is_on_the_routing_page() {
        let routing = &PAGE_HTML[PAGE_HTML.find("R.routing=()=>{").unwrap()..];
        let routing = &routing[..routing.find("\n};\n").unwrap()];
        assert!(routing.contains("${tgCard(adm,pools)}"));
        assert!(PAGE_HTML.contains("const tg=D.tg||{},open=UI.tgOpen??!!tg.managed;"), "folded while off, open while on");
        assert!(PAGE_HTML.contains("Saved TGs on server"));
        let dir = &PAGE_HTML[PAGE_HTML.find("R.issis=()=>{").unwrap()..];
        let dir = &dir[..dir.find("\n};\n").unwrap()];
        assert!(!dir.contains("setManaged") && !dir.contains("Refused"), "no managed switch on the Directory page");
    }

    #[test]
    fn dgna_offers_the_saved_talkgroups() {
        assert!(PAGE_HTML.contains("${dgSaved(f)}<input class=\"f\" id=\"dg-gssi\""));
        assert!(PAGE_HTML.contains("maximum-scale=1"), "no zoom (test, Sebastian)");
        assert!(PAGE_HTML.contains("main{display:block;height:auto;min-height:100vh;min-height:100dvh;overflow:visible}"), "phones: the page scrolls, not <main>");
        assert!(PAGE_HTML.contains(",'Infrastructure'],") && !PAGE_HTML.contains(",'Network'],"), "sidebar group renamed");
    }

    #[test]
    fn callsign_database_names_come_after_the_directory() {
        assert!(PAGE_HTML.contains("const nameOf=issi=>(D.issi||{})[String(issi)]||(D.cs||{})[String(issi)]||'';"), "Directory name first");
        assert!(PAGE_HTML.contains("filter(i=>!(D.issi||{})[i]&&!CS_ASKED.has(i))"), "asked only for ISSIs without a Directory name");
        assert!(PAGE_HTML.contains("${K==='issi'?csCard(adm):''}"), "card on the Directory radios tab");
        assert!(PAGE_HTML.contains("const open=!!UI.csOpen"), "closed by default");
    }

    #[test]
    fn history_pages_choose_how_many_and_clear() {
        assert!(PAGE_HTML.contains("const SHOW_N=[25,50,100,500,'all'];"));
        for page in ["R.calls=()=>{", "R.sds=()=>{"] {
            let body = &PAGE_HTML[PAGE_HTML.find(page).unwrap()..];
            let body = &body[..body.find("\n};\n").unwrap()];
            assert!(body.contains("showSel(") && body.contains("clearHistory("), "{page}: show selector and Clear button");
            assert!(body.contains("D.who.admin?"), "{page}: Clear only for admins");
        }
        assert!(PAGE_HTML.contains("calls in total</div></div>`}"), "Overview: total under No active calls");
        assert!(PAGE_HTML.contains("if(!force&&(scrollBusy||touchOn)){renderLater=true;return;}"), "no live re-render while scrolling");
        // A made-up timer id (scrollBusy=1) cleared with clearTimeout stopped the live refresh
        // interval that had id 1: after one touch the page never updated again.
        assert!(!PAGE_HTML.contains("scrollBusy=1"), "no fake timer id");
        for clear in PAGE_HTML.match_indices("clearTimeout(").map(|(i, _)| &PAGE_HTML[i..]) {
            assert!(!clear.starts_with("clearTimeout(scrollBusy)"), "scrollBusy is a flag, not a timer id");
        }
    }

    #[test]
    fn routing_talkgroups_show_access_badges_and_edit() {
        let card = &PAGE_HTML[PAGE_HTML.find("function tgCard(").unwrap()..];
        let card = &card[..card.find("\n}\n").unwrap()];
        assert!(card.contains("accChips(acc[k])"), "badges of the access entry");
        assert!(card.contains("onlyThese(acc[k])"), "Only these groups are allowed automatically");
        assert!(card.contains("editName('${k}','gssi')") && card.contains(">Rules</button>"), "Edit / Rules opens the Directory dialog");
        assert!(PAGE_HTML.contains("function editName(k,kind){"));
        assert!(PAGE_HTML.contains("if(v==='allow'&&rx){"), "listen-only ISSIs are added to an allow list");
    }

    /// The former classic sub-pages are native pages now: no frame, a renderer each, the SIP
    /// editing on SIP Config (admin) with the same API as the classic Settings page. Only the
    /// map stays a frame.
    #[test]
    fn sds_kind_from_the_protocol_identifier() {
        // ETSI TS 100 392-2 table 29.21: text, location, WAP/WCMP and user data header.
        for (pid, kind) in [(2, "text"), (9, "text"), (130, "text"), (137, "text"), (3, "pos"), (10, "pos"), (131, "pos"), (4, "data"), (5, "data"), (132, "data"), (133, "data"), (138, "data"), (12, "multi"), (140, "multi"), (1, "sec"), (8, "sec"), (136, "sec")] {
            assert!(PAGE_HTML.contains(&format!("{pid}:'{kind}'")), "pid {pid} is {kind}");
        }
        assert!(PAGE_HTML.contains("(p>=64&&p<=126)||(p>=192&&p<=254)?'app':'other'"), "user application ranges");
        // The SDS page (network and station telemetry lists) shows and filters the kind.
        let body = &PAGE_HTML[PAGE_HTML.find("function sdsNetList(){").unwrap()..];
        let body = &body[..body.find("R.sds=()=>{").unwrap()];
        assert_eq!(body.matches("sdsChip(").count(), 2, "both lists show the kind");
        assert_eq!(body.matches("kindOk(").count(), 2, "both lists filter it");
        assert!(PAGE_HTML.contains("${sdsChip(s.protocol_id)?' '+sdsChip(s.protocol_id):''}"), "Overview shows the kind");
    }

    #[test]
    fn fleet_management_edits_every_config_section() {
        // Sebastian 2026-11-12: Fleet Control management changes everything, the control channel
        // included; only clearing the change log is for admins.
        let page = &PAGE_HTML[PAGE_HTML.find("function fcDiff(o,v){").unwrap()..];
        let page = &page[..page.find("function fleetCfgChip(").unwrap()];
        assert!(!page.contains("admin"), "no admin-only part on the config page");
        assert!(PAGE_HTML.contains("${f.is_admin?`<button class=\"btn danger\""), "Clear stays admins only");
    }

    #[test]
    fn config_page_follows_the_station_on_networks() {
        // As FlowStation's Infrastructure page: the classic network becomes "home" when the first
        // network is added (the station creates it, the page does not send it), every network
        // but the primary one can be removed, and a network without a cell is caught before Apply
        // unless the station keeps unused networks.
        assert!(PAGE_HTML.contains("if(n.converted)continue;"));
        assert!(PAGE_HTML.contains("x.name===fcPrimaryNet()||x.converted?'':"));
        assert!(PAGE_HTML.contains("const un=fcUnusedOk()?[]:fcUnserved();if(un.length)"));
        // A station with networks-unused-v1 keeps a network without a cell: "unused", no block.
        assert!(PAGE_HTML.contains("includes('networks-unused-v1')"));
        assert!(PAGE_HTML.contains("via==='default'?'home':via"));
        // The config editor sends its text; the server turns it into single-value patches.
        assert!(PAGE_HTML.contains("if(!resolve&&fcRawChanged())req.raw_text=fc.raw;"));
        assert!(PAGE_HTML.contains("'/api/fleet/config/raw_preview'"));
        assert!(PAGE_HTML.contains("path:['security','issi_whitelist']"));
        assert!(PAGE_HTML.contains("vh=v.home_mode_display,P=['cell_info','home_mode_display']"));
    }

    #[test]
    fn config_page_order_and_resolve() {
        // Sebastian 2026-11-12: Brew link, networks, cells, neighbours, reselection, Home Mode
        // Display, whitelist, telemetry, control, update source.
        assert!(PAGE_HTML.contains("fieldset(brew+nets+cells+nb+resel+hmd+wl+tel+cmd+ota)+raw"));
        // "Apply server config" is open whenever the station is online with a copy, also when
        // only settings the page does not show differ (the server sends those as raw patches).
        assert!(PAGE_HTML.contains(r#"value="apply" ${d.online&&c.copy?'':'disabled'}"#));
    }

    #[test]
    fn inner_scrollers_keep_their_position() {
        // Sebastian 2026-11-12 (phone): the scrollable change log jumped back to its start on
        // every live update and caught the page scroll.
        assert!(PAGE_HTML.contains("c.querySelectorAll('.tw').forEach((e,i)=>{const q=pos[i];"));
        assert!(PAGE_HTML.contains("document.addEventListener('scroll',scrollRest,{passive:true,capture:true});"));
        assert!(!PAGE_HTML.contains("fllog{max-height:452px;overflow-y:auto;overscroll-behavior:contain}"));
    }

    #[test]
    fn neighbour_tables_always_show_mcc_mnc() {
        // Sebastian 2026-11-12: always show MCC/MNC, never "same" (the cell's own network when the
        // entry has none).
        assert!(!PAGE_HTML.contains(r#"<span class="mut">same</span>"#));
        assert!(PAGE_HTML.contains("${nb.mcc??cmcc}/${nb.mnc??cmnc}</td>"));
        assert!(PAGE_HTML.contains("<th>Cell</th><th>Carrier</th><th>LA</th><th>MCC/MNC</th><th>Why</th>"));
    }

    #[test]
    fn former_classic_pages_are_native() {
        let pages = &PAGE_HTML[PAGE_HTML.find("const PAGES=[").unwrap()..];
        let pages = &pages[..pages.find("];").unwrap()];
        for id in ["sip", "sipcfg", "sms", "ha", "settings"] {
            let entry = &pages[pages.find(&format!("['{id}',")).unwrap()..];
            let entry = &entry[..entry.find(']').unwrap()];
            assert!(!entry.contains("'/"), "{id} is not a frame any more: {entry}");
            assert!(PAGE_HTML.contains(&format!("R.{id}=()=>{{")), "{id} has a renderer");
        }
        assert!(pages.contains("['map','Map','map','Monitor','/map']"), "the map stays a frame");
        assert!(pages.contains("['settings','Server settings','set','System',null,true]"), "Server settings stays admin only");
        for route in ["/api/config/sip/extensions/", "/api/config/sip/trunks/", "'/api/config/sip/routes'", "`/api/config/${which}/${i}`", "/api/config/allowlist/enabled", "/api/config/ha", "/api/ha/apply", "/api/sms-center/"] {
            assert!(PAGE_HTML.contains(route), "{route} used");
        }
        // Admin-only data is asked only with an admin login.
        let load = &PAGE_HTML[PAGE_HTML.find("function loadPageData(p,first){").unwrap()..];
        let load = &load[..load.find("\n}\n").unwrap()];
        for line in load.lines().filter(|l| l.contains("/api/config/")) {
            assert!(line.contains("D.who.admin"), "admin route without admin check: {line}");
        }
        // A table that scrolls sideways keeps its screen-reader text inside: no page-wide overflow on phones.
        assert!(PAGE_HTML.contains(".tw{overflow-x:auto;position:relative}"));
    }

    /// UI cleanup: Telemetry watches (graphs, values, condition alarms, ping), Fleet Control
    /// manages (restart and shut down together), DGNA & Radio actions replaces Control and takes
    /// DGNA from Routing.
    #[test]
    fn terminals_and_bridges_are_told_apart() {
        assert!(PAGE_HTML.contains("const isT=m=>asTerm(m.mode,m.basestation_name),radios=ms.filter(m=>!isT(m)),terms=ms.filter(isT);"), "Radios page: two lists");
        assert!(PAGE_HTML.contains("<h2>Terminals &amp; bridges</h2><span class=\"cnt\">${terms.length}"));
        assert!(PAGE_HTML.contains("<td>${kindCell(b.station,b.mode)}</td>"), "Stations & Cells shows the type");
        assert!(PAGE_HTML.contains("— (${tKind(...ns)})"), "the cell column names it");
        assert!(PAGE_HTML.contains("postJ('/api/issi-kinds/set',{issi,kind})"), "set in the Directory dialog");
        assert!(PAGE_HTML.contains("getJ('/api/issi-kinds')"));
    }

    /// Type per login: Auto (as it logs in), Terminal or Bridge. A Basestation login set to
    /// Terminal or Bridge leaves the station list (Stations, header count, map) and is listed,
    /// with its radios, under Terminals & bridges. Set on Accounts, Stations and Directory.
    #[test]
    fn a_login_type_overrides_where_it_is_listed() {
        assert!(PAGE_HTML.contains("const asTerm=(mode,login)=>mode==='Terminal'||(mode==='Basestation'&&!!kindOver(login));"));
        assert!(PAGE_HTML.contains("return v==='bridge'||v==='terminal'?v:'';"), "anything else is Auto");
        assert!(PAGE_HTML.contains("const bs=conns.filter(b=>b.mode==='Basestation'&&!asTerm(b.mode,b.station)),term=conns.filter(b=>asTerm(b.mode,b.station)),netl="), "Stations page");
        assert!(PAGE_HTML.contains("c.brew_clients.filter(x=>x.mode==='Basestation'&&!asTerm(x.mode,x.station))"), "header count");
        assert!(PAGE_HTML.contains("c.brew_clients.filter(b=>b.mode==='Basestation'&&!asTerm(b.mode,b.station))"), "map and overview stations");
        assert!(PAGE_HTML.contains("['auto','terminal','bridge'].map(v=>"), "three choices");
        assert!(PAGE_HTML.contains("   ${kindSeg(x.username)}"), "Accounts dialog");
        assert!(PAGE_HTML.contains("if(kindChanged)D.ikind=await postJ('/api/issi-kinds/set',{issi:body.username,kind});"), "saved with the login");
        assert!(PAGE_HTML.contains("onclick=\"editKind('${esc(login)}','${esc(mode)}')\""), "Change on the Stations page");
        assert!(PAGE_HTML.contains("D.ikind=await postJ('/api/issi-kinds/set',{issi:login,kind})"));
        assert!(PAGE_HTML.contains("   ${K==='issi'?kindSeg(k):''}"), "Directory dialog");
        assert!(!PAGE_HTML.contains("When it logs in as Terminal"));
    }

    #[test]
    fn registration_log_scrolls_and_admins_can_clear_it() {
        assert!(PAGE_HTML.contains("<h2>Registration log</h2>"));
        assert!(PAGE_HTML.contains(r#"onclick="clearRegLog()""#) && PAGE_HTML.contains("postJ('/api/history/clear',{registrations:true})"));
        assert!(PAGE_HTML.contains("${regs.slice(0,1000).map(r=>"));
    }

    #[test]
    fn telemetry_sds_is_a_second_list_on_the_sds_page() {
        let pages = &PAGE_HTML[PAGE_HTML.find("const PAGES=[").unwrap()..];
        let pages = &pages[..pages.find("];").unwrap()];
        assert!(!pages.contains("'tsds'"), "no menu entry of its own");
        assert!(PAGE_HTML.contains("PG.tsds=['tsds','Telemetry SDS','tsds','Monitor'];"), "the right stays");
        assert!(PAGE_HTML.contains("box('tsds','Telemetry SDS (list on the SDS page)')"), "and can be ticked");
        assert!(PAGE_HTML.contains("if(p==='tsds')p='sds';"), "old links land on the SDS page");
        assert!(!PAGE_HTML.contains("R.tsds=()=>{"));
        let body = &PAGE_HTML[PAGE_HTML.find("R.sds=()=>{").unwrap()..];
        let body = &body[..body.find("\n};").unwrap()];
        assert!(body.contains("sdsTeleOn()") && body.contains("kindSel('tsdsk'"), "the station list with its own kind filter");
        assert!(body.contains("fullBtn('net')") && body.contains("fullBtn('tele')"), "both lists open on the whole screen");
        assert!(PAGE_HTML.contains("@media (min-width:1280px){.sdsgrid.two{grid-template-columns:minmax(0,1fr) minmax(0,1fr)}}"), "side by side on wide screens");
        assert!(PAGE_HTML.contains(".tw.sdsl{max-height:62vh;overflow-y:auto}"), "each list scrolls");
    }

    #[test]
    fn dgna_assignments_card_on_the_radio_actions_page() {
        assert!(PAGE_HTML.contains("return dgnaCard(mr)+dgnaAssignCard(mr)+"));
        assert!(PAGE_HTML.contains("if(UI.page==='radioctl')tasks.push(opt('/api/dgna/assignments',v=>D.dgnaA=v));"));
        assert!(PAGE_HTML.contains("postJ('/api/dgna',{issis:[issi],gssi,assign:false,attachment_mode:0})"), "Remove sends the DGNA remove");
        assert!(PAGE_HTML.contains("postJ('/api/dgna/forget',{issi,gssi})"));
    }

    #[test]
    fn sms_center_known_radios_open_a_named_list() {
        assert!(PAGE_HTML.contains(r#"onclick="smsKnown()""#), "the count is a button");
        assert!(PAGE_HTML.contains("function smsKnown(){"));
        assert!(PAGE_HTML.contains("for(const i of ((D.smsc&&D.smsc.known)||[]))add(i);"), "call signs are looked up for them");
    }

    #[test]
    fn pages_after_the_ui_cleanup() {
        let page = |name: &str| {
            let a = PAGE_HTML.find(&format!("\nR.{name}=()=>{{")).unwrap_or_else(|| panic!("R.{name}"));
            let rest = &PAGE_HTML[a + 1..];
            &rest[..rest[1..].find("\nR.").map_or(rest.len(), |b| b + 1)]
        };
        let pages = &PAGE_HTML[PAGE_HTML.find("const PAGES=[").unwrap()..];
        let pages = &pages[..pages.find("];").unwrap()];
        assert!(!pages.contains("['control'"), "Control is gone");
        assert!(pages.contains("['telemetry','Telemetry','tele','Infrastructure'],['radioctl','DGNA & Radio actions'"));
        let tele = page("telemetry");
        assert!(tele.contains("fleetAlarmCard(f,false)") && tele.contains("tlmHeadFx(fx)") && tele.contains("tlmLatest(fx)") && tele.contains("fleetAutoPing"));
        assert!(PAGE_HTML.contains("onclick=\"fleetGraph('${esc(fx.id)}')\""), "graphs link at the telemetry card");
        let fleet = page("fleet");
        assert!(fleet.contains("fleetAlarmCard(f,true)"), "change log ALARMs stay on Fleet Control");
        assert!(!fleet.contains("fleetGraph(") && !fleet.contains("fleetAutoPing") && !fleet.contains("/api/fleet/ping"), "graphs and ping moved to Telemetry");
        assert!(PAGE_HTML.contains("'Shut down service'") && PAGE_HTML.contains("action:kind==='shutdown'?'ShutdownService':'RestartService'"));
        assert!(PAGE_HTML.contains("${ic('power')}Power options</button>"), "Fleet Control button is Power options");
        assert!(PAGE_HTML.contains("show('telemetry')\">‹</button>"), "the graph page goes back to Telemetry");
        let routing = page("routing");
        assert!(!routing.contains("Assign radios to a group (DGNA)") && routing.contains("data-go=\"radioctl\""));
        let radio = page("radioctl");
        assert!(radio.contains("dgnaCard(mr)") && radio.contains("KickMs") && radio.contains("AmbienceListen") && radio.contains("mayRadio()"));
        assert!(PAGE_HTML.contains("box('radioctl_manage','DGNA & Radio actions: send')"), "own right to send");
        assert!(PAGE_HTML.contains("<h2>Graphs and alarms</h2>") && !PAGE_HTML.contains("Fleet Control graphs and alarms"));
    }
}
