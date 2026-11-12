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
    PAGE_HTML.replace("__VERSION__", VERSION).replace("__SERVERNAME__", &name)
});

pub async fn page() -> Html<&'static str> {
    Html(PAGE.as_str())
}

// The classic pages' embedded form lives in `dashboard::STYLE` (`html.embed`).
const PAGE_HTML: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="light dark">
<title>__SERVERNAME__</title>
<style>
:root{
  --bg:#eceff4;--bg2:#ffffff;--bg3:#f3f5f9;--bg4:#d6dde7;
  --border:#dde3ec;--sep:rgba(20,30,50,0.09);
  --accent:#00876a;--accent2:#1565c0;--warn:#9a5400;--danger:#c0203a;--ok:#16876b;
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
  --accent:#00d4a8;--accent2:#4da6ff;--warn:#ffb224;--danger:#ff4d6d;--ok:#2ec6a6;
  --text:#eef3fb;--text2:#94abc9;--text3:#7f93b5;
  --sidebar:#070a10;--sidebar-border:#161d2c;
  --shadow:0 1px 2px rgba(0,0,0,.45),0 10px 28px -14px rgba(0,0,0,.65);
  --hair:inset 0 1px 0 rgba(255,255,255,.05);
  --on-accent:#04120e;
}}
:root[data-theme=dark]{
  --bg:#090d14;--bg2:#111824;--bg3:#19212f;--bg4:#2b3850;
  --border:#232e40;--sep:rgba(255,255,255,0.07);
  --accent:#00d4a8;--accent2:#4da6ff;--warn:#ffb224;--danger:#ff4d6d;--ok:#2ec6a6;
  --text:#eef3fb;--text2:#94abc9;--text3:#7f93b5;
  --sidebar:#070a10;--sidebar-border:#161d2c;
  --shadow:0 1px 2px rgba(0,0,0,.45),0 10px 28px -14px rgba(0,0,0,.65);
  --hair:inset 0 1px 0 rgba(255,255,255,.05);
  --on-accent:#04120e;
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
.brand-row{display:flex;gap:10px;align-items:center}
.logo{width:34px;height:34px;border-radius:9px;display:grid;place-items:center;color:var(--accent);flex-shrink:0;
  background:linear-gradient(135deg,color-mix(in srgb,var(--accent) 16%,transparent),color-mix(in srgb,var(--accent2) 16%,transparent));
  border:1px solid color-mix(in srgb,var(--accent) 30%,transparent)}
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
.badge:empty{display:none}
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
.btn.danger{color:var(--danger)}
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
.lcard .logo{width:52px;height:52px;border-radius:14px;margin:0 auto 14px;display:grid;place-items:center}
.lcard .logo svg{width:28px;height:28px}
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
    <div class="brand-row">
      <div class="logo" aria-hidden="true"><svg class="i" viewBox="0 0 24 24"><path d="M12 11v10M8 21h8M9.5 21l2.5-10 2.5 10"/><circle cx="12" cy="9" r="1.4"/><path d="M7.8 5.8a6 6 0 0 0 0 6.4M16.2 5.8a6 6 0 0 1 0 6.4M5 3.5a10 10 0 0 0 0 11M19 3.5a10 10 0 0 1 0 11"/></svg></div>
      <div><div class="bname">__SERVERNAME__</div><div class="bsub">brew-server v__VERSION__</div></div>
    </div>
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
  <button class="tab" data-tab="routing"></button>
  <button class="tab" data-tab="more"></button>
</div>

<div class="login" id="login" role="dialog" aria-modal="true" aria-labelledby="login-title">
  <form class="lcard" id="lcard" onsubmit="submitLogin(event)" autocomplete="on">
    <button type="button" class="x" id="login-x" onclick="closeLogin()" title="Close"><svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg><span class="sr">Close</span></button>
    <div class="logo" aria-hidden="true"><svg class="i" viewBox="0 0 24 24"><path d="M12 11v10M8 21h8M9.5 21l2.5-10 2.5 10"/><circle cx="12" cy="9" r="1.4"/><path d="M7.8 5.8a6 6 0 0 0 0 6.4M16.2 5.8a6 6 0 0 1 0 6.4M5 3.5a10 10 0 0 0 0 11M19 3.5a10 10 0 0 1 0 11"/></svg></div>
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
 ctl:'<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2"/>',
 sip:'<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 10h4M7 14h7M16 10h1"/>',
 sipcfg:'<path d="M4 6h10M4 12h6M4 18h10"/><circle cx="17" cy="6" r="2"/><circle cx="13" cy="12" r="2"/><circle cx="17" cy="18" r="2"/>',
 sms:'<path d="M4 6h16v12H4z"/><path d="M4 6l8 7 8-7"/>',
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
 ['telemetry','Telemetry','tele','Infrastructure'],['control','Control','ctl','Infrastructure',null,true],['accounts','Accounts','key','Infrastructure'],
 ['sip','SIP / VoIP','sip','Integrations'],['sipcfg','SIP Config','sipcfg','Integrations'],
 ['sms','SMS Center','sms','Integrations'],['tsds','Telemetry SDS','tsds','Integrations'],
 ['ha','High Availability','ha','System'],['config','Config file','file','System',null,true],['settings','Server settings','set','System',null,true]
];
const PG=Object.fromEntries(PAGES.map(p=>[p[0],p]));
// Pages this login sees (Server settings -> Users): an admin all of them, any other login the ones
// ticked for it, someone not logged in the Viewer pages. The server refuses the rest anyway.
const can=p=>!!PG[p]&&(D.who.admin||(!PG[p][5]&&(D.who.pages||[]).includes(p)));
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

const D={status:null,conn:null,tele:[],pools:[],nets:null,radios:[],who:{admin:false},names:{},issi:{},gssi:{},access:{},tg:{managed:false,hidden:[],listed:[]},ctl:[],emg:[],ha:null,regs:[]};
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
function go(p){if(!PG[p])p='overview';if(location.hash!=='#'+p)location.hash=p;else show(p);}
window.addEventListener('hashchange',()=>show(location.hash.slice(1)));
const navKey=()=>JSON.stringify([D.who.admin,D.who.pages]);
function show(p){
  if(!PG[p])p='overview';
  // A page this login does not see: the first one it does (or the login window).
  if(D.whoLoaded&&!can(p)){const f=firstPage();if(f){if(PG[p][5]&&!D.who.admin&&D.who.can_login)openLogin(false);p=f;}}
  if(UI.navKey!==navKey())buildNav();
  UI.page=p;document.body.classList.remove('sbopen');
  document.querySelectorAll('.nv').forEach(n=>n.classList.toggle('on',n.dataset.go===p));
  document.querySelectorAll('.tab').forEach(n=>n.classList.toggle('on',n.dataset.tab===p));
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
  if(UI.page==='sipcfg'&&can('sipcfg'))loadPageData('sipcfg',false);
  tasks.push(getJ('/api/names').then(v=>D.names=v).catch(()=>{}));
  tasks.push(getJ('/api/issi-names').then(v=>D.issi=v).catch(()=>{}));
  tasks.push(getJ('/api/gssi-names').then(v=>D.gssi=v).catch(()=>{}));
  tasks.push(getJ('/api/group-access').then(v=>D.access=v).catch(()=>{}));
  tasks.push(getJ('/api/talkgroups').then(v=>D.tg=v).catch(()=>{}));
  tasks.push(getJ('/api/callsign-db').then(v=>{if(D.csdb&&(D.csdb.updated_ms!==v.updated_ms||D.csdb.enabled!==v.enabled)){D.cs={};CS_ASKED.clear();}D.csdb=v;}).catch(()=>{}));
  if(D.who.admin)tasks.push(getJ('/api/control').then(v=>D.ctl=v).catch(()=>{}));
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
  const bs=c?new Set(c.brew_clients.filter(x=>x.mode==='Basestation').map(x=>x.station)).size:0;
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
  if(force||html!==lastSig){$('content').innerHTML=html;lastSig=html;}
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
  for(const b of c.brew_clients.filter(b=>b.mode==='Basestation')){const s=byStation[b.station]||(byStation[b.station]={name:b.station,cells:[],online:true,regs:b.registered_issis,addr:b.remote_addr,since:b.connected_at_ms,gone:0});s.online=true;s.regs=(s.conns?s.regs:0)+b.registered_issis;s.conns=(s.conns||0)+1;s.since=b.connected_at_ms;s.addr=s.addr||b.remote_addr;s.ver=b.version;}
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
R.sds=()=>{
  const st=D.status;if(!st)return '<div class="empty">Loading…</div>';
  const q=UI.filter.sds||'';const list=st.recent_sds.filter(s=>kindOk('sdsk',s.protocol_id)&&match(q,s.source,s.destination,nameOf(s.source),nameOf(s.destination),s.from_network,(s.to_networks||[]).join(' ')));
  const nc=hasNet(list);
  return `<div class="bar">${searchBox('sds','ISSI…')}${kindSel('sdsk',st.recent_sds.map(s=>s.protocol_id))}<span class="sp"></span>${showSel('sds')}<span class="mut" style="font-size:12.5px">${st.total_sds} SDS in total</span>${D.who.admin?`<button class="btn danger" onclick="clearHistory('sds')">${ic('trash')}Clear all SDS</button>`:''}</div>
  ${msgDiv('sds-msg','padding:0 4px')}
  <div class="card"><div class="ch"><h2>Recent SDS</h2><span class="cnt">${list.length}</span><span class="sp"></span><span class="mut" style="font-size:12px">${keptNote(st,'sds')}</span></div><div class="tw"><table><thead><tr><th>Time</th><th>Kind</th><th>From</th><th>To</th>${nc?'<th>Network</th>':''}<th>Reports</th></tr></thead><tbody>
  ${list.map(s=>`<tr><td class="m">${tmd(s.at_ms)}</td><td>${sdsChip(s.protocol_id)||'<span class="mut">—</span>'}</td><td>${issiCell(s.source)}</td><td>${issiCell(s.destination)}</td>${nc?`<td>${netChips(s)}</td>`:''}<td class="num">${s.reports}</td></tr>`).join('')||'<tr><td colspan="${nc?6:5}" class="mut">No SDS yet</td></tr>'}</tbody></table></div></div>`;
};
R.radios=()=>{
  const c=D.conn;if(!c)return '<div class="empty">Loading…</div>';
  const q=UI.filter.radios||'';
  const ms=c.mobile_stations.filter(m=>match(q,m.issi,nameOf(m.issi),m.basestation_name,(m.groups||[]).join(' '),(m.groups||[]).map(gName).join(' '),m.cell&&m.cell.name));
  const regs=(D.regs||[]).filter(r=>match(q,r.issi,r.bts));
  return `<div class="bar">${searchBox('radios','ISSI, group or station…')}</div>
  <div class="card"><div class="ch"><h2>Registered radios</h2><span class="cnt">${ms.length}</span>${helpBtn('radios')}</div>${helpNote('radios',CELL_ID_HELP+' <i>Station</i> is the Brew login the radio is registered through. <i>Cell</i> is shown when the station reports cells (<code>bts-info</code>, <code>ms-cell</code>). Apps and other terminals (Terminal mode) have no cells; a station without these extensions is shown without a cell.')}<div class="tw"><table><thead><tr><th>ISSI</th><th>Station</th><th>Cell (global ID)</th><th>Groups</th><th>Via</th><th>Station address</th></tr></thead><tbody>
  ${ms.sort((a,b)=>a.issi-b.issi).map(m=>`<tr><td>${issiCell(m.issi,true)}</td><td><b class="m">${esc(m.basestation_name||'—')}</b>${infoOf(m.basestation_name)?`<span class="sub">${esc(infoOf(m.basestation_name))}</span>`:''}</td><td>${cellCell(m.cell&&m.cell.name,m.mode)}</td><td>${(m.groups||[]).map(g=>tgChip(g)).join(' ')||'<span class="mut">—</span>'}</td><td>${esc(m.mode)}</td><td class="m mut">${esc(m.basestation_addr||'')}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No radios registered</td></tr>'}
  </tbody></table></div></div>
  <div class="card"><div class="ch"><h2>Registration log</h2><span class="cnt">${regs.length}</span></div><div class="tw"><table><thead><tr><th>Time</th><th>ISSI</th><th>Event</th><th>Station</th><th>Cell (global ID)</th></tr></thead><tbody>
  ${regs.slice(0,300).map(r=>`<tr><td class="m">${tmd(r.at_ms)}</td><td>${issiCell(r.issi)}</td><td>${r.kind==='register'?'<span class="chip a">register</span>':r.kind==='timeout'?'<span class="chip w">timeout</span>':`<span class="chip i">${esc(r.kind)}</span>`}</td><td>${esc(r.bts||'')}</td><td>${r.cell?'<span class="chip">'+esc(r.cell)+'</span>':'<span class="mut">—</span>'}</td></tr>`).join('')||'<tr><td colspan="5" class="mut">No registrations yet</td></tr>'}
  </tbody></table></div></div>`;
};
// Name of an ISSI from the directory, if any.
// Directory names first (always!), then the ham callsign database (D.cs, only asked for ISSIs without one).
const nameOf=issi=>(D.issi||{})[String(issi)]||(D.cs||{})[String(issi)]||'';
const gName=g=>(D.gssi||{})[String(g)]||'';
// A group of another network shows as "network:GSSI" (inside the server it
// is an alias above the 24 GSSI bits, see netlink.rs).
const netOfSlot=s=>((D.nets&&D.nets.networks)||[]).find(n=>n.slot===s);
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
const cellCell=(name,mode)=>name?`<span class="chip">${esc(name)}</span>`:mode==='Terminal'?'<span class="mut" title="Apps and other terminals have no cells">— (terminal)</span>':'<span class="mut" title="The station reports no cells">—</span>';
// Global cell ID of a station cell seen in telemetry, by its carrier (unique in a network).
const gcidOf=carrier=>{const c=((D.conn&&D.conn.cells)||[]).find(x=>x.online!==false&&x.carrier===carrier);return c?c.name:'';};
// "Bost FlowStation/v0.5.6-55a08812" -> "Bost FlowStation v0.5.6-55a08812".
function software(a){if(!a)return '<span class="mut">—</span>';const m=/^(.+?)\/(\S+)(.*)$/.exec(a);return m?`${esc(m[1])} <span class="m">${esc(m[2])}</span>${esc(m[3])}`:esc(a);}
R.stations=()=>{
  const c=D.conn;if(!c)return '<div class="empty">Loading…</div>';
  const conns=connGroups(c.brew_clients);
  const f=UI.filter.cellf||'all';
  const cells=(c.cells||[]).filter(x=>f==='all'||(f==='on')===(x.online!==false));
  const dups=c.duplicate_carriers||[];
  const caps=x=>(x.capabilities||[]).map(k=>`<span class="chip">${esc(k)}</span>`).join(' ')||'<span class="mut">—</span>';
  return `<div class="bar">${seg('cellf',[['all','All'],['on','Online'],['off','Offline']])}</div>
  ${dups.length?`<div class="card"><div class="note">${ic('alert')}<div><b>Duplicate carriers:</b> ${dups.map(d=>esc(JSON.stringify(d))).join(', ')}. The newest announcement serves.</div></div></div>`:''}
  <div class="card"><div class="ch"><h2>Cells</h2><span class="cnt">${cells.length}</span>${helpBtn('cells')}</div><div class="tw"><table><thead><tr><th>Cell (global ID)</th><th>Status</th><th>Carrier</th><th>Downlink</th><th>LA</th><th>MCC / MNC</th><th>Neighbours</th><th>Radios</th></tr></thead><tbody>
  ${cells.map(x=>{const key='c'+x.name;const open=UI.open[key];const offc=x.online===false;
   const ci=infoOf(String(x.name).split('/')[0]);
   return `<tr class="click${offc?' off':''}" onclick="UI.open['${esc(key)}']=!UI.open['${esc(key)}'];render(true)" aria-expanded="${!!open}"><td><b>${esc(x.name)}</b> ${x.cell===0&&!offc?'<span class="chip i">primary</span>':''}${x.duplicate?' <span class="chip w">duplicate</span>':''}${ci?`<span class="sub">${esc(ci)}</span>`:''}</td>
   <td>${offc?'<span class="st i"><i></i>Offline</span>':'<span class="st"><i></i>Online</span>'}</td><td class="m">${x.carrier}</td><td class="num">${mhz(x.downlink_hz)}</td><td>${x.location_area??'—'}</td><td class="num">${x.mcc!=null&&x.mnc!=null?x.mcc+' / '+x.mnc:'—'}</td>
   <td>${x.neighbours?(x.neighbours.map(n=>`<span class="chip">${esc(n)}</span>`).join(' ')||'<span class="mut">none</span>'):'<span class="mut">—</span>'}</td><td class="num">${offc?'—':x.registered_issis}</td></tr>
   ${open?`<tr class="exp"><td colspan="8"><div class="kv"><div><span class="l">Station address</span>${offc?'—':`<span class="m">${esc(x.remote_addr||'—')}</span>`}</div><div><span class="l">${offc?'Gone':'Connected'}</span>${offc?(x.last_seen_ms?ago(x.last_seen_ms)+' ago':'—'):ago(x.connected_at_ms)}</div>
    <div><span class="l">Extensions</span>${caps(x)}</div><div><span class="l">Access level / MS power</span>${x.rxlev_access_min_dbm!=null?x.rxlev_access_min_dbm+' dBm':'—'} · ${x.ms_txpwr_max_dbm!=null?x.ms_txpwr_max_dbm+' dBm':'—'}</div></div></td></tr>`:''}`;}).join('')||'<tr><td colspan="8" class="mut">No cells announced</td></tr>'}
  </tbody></table></div>${helpNote('cells','Cells the stations announce (FlowStation with the <code>bts-info</code> extension). '+CELL_ID_HELP+' Click a row for details. A station that went away stays listed as offline until it connects again (stations with a login only). Apps and other terminals (Terminal mode) have no cells. A station without the <code>bts-info</code> extension is counted as a station (Overview, Brew connections), but its cells are not known and not listed here.')}</div>
  <div class="card"><div class="ch"><h2>Brew connections</h2><span class="cnt">${conns.length}</span>${helpBtn('conns')}</div>${helpNote('conns','Every Brew connection, by login: stations (Basestation mode), apps and other terminals (Terminal mode, no cells) and this server\'s own links to other networks (Network mode). A station that opens several connections (one per cell or per network) is listed once, with all its cells, the radios summed and every remote address. A station without the <code>bts-info</code> extension shows no cells.')}<div class="tw"><table><thead><tr><th>Login</th><th>Mode</th><th>Software</th><th>Remote address</th><th>Connected</th><th>Radios</th><th>Cells (global ID)</th></tr></thead><tbody>
  ${conns.map(b=>`<tr><td><b>${esc(b.station)}</b>${infoOf(b.station)?`<span class="sub">${esc(infoOf(b.station))}</span>`:''}${b.n>1?`<span class="sub">${b.n} connections</span>`:''}</td><td>${esc(b.mode)}</td><td>${[...new Set(b.agents)].map(software).join('<br>')}<span class="sub">Brew v${[...new Set(b.versions)].join(', v')}</span></td><td class="m">${[...new Set(b.addrs)].map(esc).join('<br>')||'—'}</td><td>${ago(b.connected_at_ms)}</td><td class="num">${b.registered_issis}</td><td>${b.cells.sort((x,y)=>x.name.localeCompare(y.name,undefined,{numeric:true})).map(x=>`<span class="chip" title="Carrier ${x.carrier}, ${x.mcc}/${x.mnc}">${esc(x.name)}</span>`).join(' ')||`<span class="mut">${b.mode==='Terminal'?'— (terminal)':'—'}</span>`}</td></tr>`).join('')||'<tr><td colspan="7" class="mut">No Brew connections</td></tr>'}
  </tbody></table></div></div>
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
R.telemetry=()=>{
  const t=D.tele||[];
  const lvl=l=>l==='ok'?'':l==='degraded'?' w':' d';
  return t.length?`<div class="card"><div class="ch"><h2>Station telemetry</h2><span class="cnt">${t.length}</span>${helpBtn('tlm')}</div>${helpNote('tlm','One card per station with a telemetry connection, named by its telemetry login. <i>Cell (local / global ID)</i>: the number of the cell inside the station (local, 0 = primary) and in brackets its global cell ID <i>login/cell</i> as on the other pages. '+CELL_ID_HELP)}</div><div class="grid2" style="grid-template-columns:repeat(auto-fill,minmax(min(420px,100%),1fr))">${t.map(s=>{const h=s.health||{};
  const nm=infoOf(s.id)||(s.site_name&&s.site_name!==s.id?s.site_name:'');
  return `<div class="card tlm"><div class="ch"><h2>${esc(s.id)}</h2>${nm?`<span class="mut" style="font-size:13px">${esc(nm)}</span>`:''}<span class="sp"></span><span class="st${lvl(h.overall)}"><i></i>${esc(h.overall||'unknown')}</span></div>
   <div class="kv tight" style="padding:12px 18px"><div><span class="l">Version</span>${esc(s.build||s.version||'—')}</div><div><span class="l">Address</span><span class="m">${esc(s.ip||'—')}</span></div><div><span class="l">Connected</span>${ago(s.connected_at_ms)}</div><div><span class="l">Backhaul</span>${s.backhaul_connected==null?'—':s.backhaul_connected?'connected':'down'}</div>
    <div><span class="l">Radios</span>${s.registration_count}</div><div><span class="l">Calls</span>${Object.keys(s.active_calls||{}).length}</div><div><span class="l">TX EVM</span>${s.evm_pct!=null?s.evm_pct.toFixed(1)+' %':'—'}</div><div><span class="l">Emergencies</span>${(s.emergencies||[]).length||'none'}</div></div>
   ${(h.domains||[]).length?`<div class="chips" style="padding:0 18px 12px;display:flex;gap:6px;flex-wrap:wrap">${h.domains.map(d=>`<span class="chip ${d.level==='ok'?'a':d.level==='degraded'?'w':'d'}" title="${esc(d.detail)}">${esc(d.domain)}: ${esc(d.detail)}</span>`).join('')}</div>`:''}
   <div class="tw"><table><thead><tr><th title="Local cell number in this station, global cell ID in brackets">Cell (local / global ID)</th><th>Carrier</th><th>LA</th><th>RF</th><th>Radios</th></tr></thead><tbody>${(s.cells||[]).map(x=>`<tr><td>${x.id}${gcidOf(x.main_carrier)?` <span class="mut">(${esc(gcidOf(x.main_carrier))})</span>`:''}${x.primary?' <span class="chip i">primary</span>':''}</td><td class="m">${x.main_carrier}${x.secondary_carrier?' + '+x.secondary_carrier:''}</td><td>${x.location_area??'—'}</td><td><span class="st${x.rf_state==='online'?'':' w'}" title="${esc(x.rf_detail||'')}"><i></i>${esc(x.rf_state||'—')}</span></td><td class="num">${x.registered_radios}</td></tr>`).join('')||'<tr><td colspan="5" class="mut">No cell report yet</td></tr>'}</tbody></table></div>${tlmCalls(s)}</div>`;}).join('')}</div>`
  :'<div class="card"><div class="empty">No station telemetry connections. Stations report here with FlowStation <code>[telemetry]</code>.</div></div>';
};
R.control=()=>{
  if(!D.who.admin)return `<div class="card"><div class="empty">Station control is for admins. ${D.who.can_login?'<button class="btn" onclick="doLogin()">Log in</button>':''}</div></div>`;
  const ids=D.ctl||[];
  if(!ids.length)return '<div class="card"><div class="empty">No station control connections. Stations connect here with FlowStation <code>[command]</code>.</div></div>';
  return ids.map(id=>{const s=id.replace(/[^a-zA-Z0-9_-]/g,'_');return `<div class="card"><div class="ch"><h2>${esc(id)}</h2><span class="st"><i></i>Connected</span></div>
   <div style="padding:14px 18px;display:flex;flex-direction:column;gap:10px">
    <div class="bar"><input class="f" id="${s}_kick" placeholder="ISSI" inputmode="numeric" style="width:120px"><button class="btn" onclick="ctl('${esc(id)}','${s}',{action:'KickMs',issi:+v('${s}_kick')})">Kick radio</button>
     <input class="f" id="${s}_clr" placeholder="ISSI (0 = all)" inputmode="numeric" style="width:130px"><button class="btn" onclick="ctl('${esc(id)}','${s}',{action:'ClearEmergency',issi:+v('${s}_clr')})">Clear emergency</button></div>
    <div class="bar"><input class="f" id="${s}_sds" placeholder="Live SDS text" style="flex:1;min-width:160px"><input class="f" id="${s}_sdsi" placeholder="from ISSI" inputmode="numeric" style="width:110px"><input class="f" id="${s}_sdsr" placeholder="repeat" value="0" inputmode="numeric" style="width:80px">
     <button class="btn" onclick="ctl('${esc(id)}','${s}',{action:'AddLiveSds',text:v('${s}_sds'),protocol_id:0,source_issi:+v('${s}_sdsi'),repeat_count:+v('${s}_sdsr')})">Add live SDS</button><button class="btn" onclick="ctl('${esc(id)}','${s}',{action:'ClearLiveSds'})">Clear live SDS</button></div>
    <div class="bar"><input class="f" id="${s}_al" placeholder="ISSI" inputmode="numeric" style="width:120px"><button class="btn" onclick="ctl('${esc(id)}','${s}',{action:'AmbienceListen',issi:+v('${s}_al'),enable:true})" title="The target radio keys its microphone (with indication on the radio). Needs station support.">Ambience listen</button><button class="btn" onclick="ctl('${esc(id)}','${s}',{action:'AmbienceListen',issi:+v('${s}_al'),enable:false})">Stop</button>
     <span class="sp"></span><button class="btn danger" onclick="if(confirm('Restart the service on ${esc(id)}?'))ctl('${esc(id)}','${s}','RestartService')">Restart service</button><button class="btn danger" onclick="if(confirm('Shut the service down on ${esc(id)}? It does not come back by itself.'))ctl('${esc(id)}','${s}','ShutdownService')">Shut down</button></div>
    ${msgDiv(s+'_res','padding:0')}<div class="mut" style="font-size:12px">DGNA for several radios at once: <a data-go="routing" href="#routing">Routing</a>.</div></div></div>`;}).join('');
};
const v=id=>($(id)||{}).value||'';
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
    <td><span class="mut" style="font-size:12px">${esc(r.network)}</span><br><span class="tg net" title="Group of the network ${esc(r.network)}">${ic('globe')} ${r.gssi}</span></td>
    <td>${live(p)}</td><td style="white-space:nowrap">${adm?`<button class="btn" onclick="editLink(${i})">Edit</button> <button class="btn danger" onclick="delPool(${i})">Delete</button>`:''}</td></tr>`;}).join('');
  const poolRows=pools.map((p,i)=>{if(p.linked)return '';const l=p.live;const mem=[...p.groups.map(g=>({t:tgChip(g,(l&&l.gssi===g)?' src':''),g})),...(p.remote||[]).map(r=>({t:`<span class="tg net" title="Group of the network ${esc(r.network)}">${ic('globe')} ${esc(r.network)}:${r.gssi}</span>`}))];
   const groups=mem.map((m,j)=>`${!p.both_ways&&j===0?m.t.replace('class="tg','class="tg src'):m.t}${j===0&&!p.both_ways?' <span class="arrow">→</span>':''}`).join(' ');
   const state=live(p);
   return `<div class="row pool${p.enabled?'':' offp'}"><button class="tog" role="switch" aria-checked="${p.enabled}" aria-label="Pool ${esc(p.name)} on" ${adm?'':'disabled'} onclick="togglePool(${i})"></button>
    <div class="rm"><div class="rt">${esc(p.name)} <span class="chip ${p.enabled?'a':'i'}">${p.enabled?'on':'off'}</span><span class="chip">${p.both_ways?'⇄ both ways':'→ one way'}</span></div>
    <div class="flow">${p.both_ways?groups.replace(/<\/span> <span class="tg/,'</span> <span class="arrow">⇄</span> <span class="tg'):groups}</div><div class="rs" style="margin-top:6px">${state}</div></div>
    ${adm?`<button class="btn" onclick="editPool(${i})">Edit</button><button class="btn danger" onclick="delPool(${i})">Delete</button>`:''}</div>`;}).join('');
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
  return `<div class="card"><div class="ch"><h2>Linked talkgroups</h2><span class="cnt">${links.length}</span>${helpBtn('links')}<span class="sp"></span>${adm?`<button class="btn pri" onclick="editLink(-1)">${ic('plus')}New link</button>`:''}</div>
   ${helpNote('links','A linked talkgroup connects one group of this server with one group of another network (Multi Network link), both ways: a call on either group is heard on the other, the talker\'s ISSI is passed on 1:1. It works like a talkgroup pool with exactly these two groups and is saved as one (<code>[[talkgroup_pools]]</code> with <code>linked = true</code>). The other network must be set up on the Multi Network page. Changes apply at once, no restart.')}
   ${links.length?`<div class="tw"><table><thead><tr><th style="width:56px">On</th><th>Name</th><th>This server · TG</th><th style="width:30px"></th><th>Linked network · TG</th><th>Status</th><th></th></tr></thead><tbody>${linkRows}</tbody></table></div>`:'<div class="empty">No linked talkgroups yet. A link connects one group here with one group of another network.</div>'}
   ${msgDiv('link-msg')}</div>
  <div class="card"><div class="ch"><h2>Talkgroup pools</h2><span class="cnt">${pools.length-links.length}</span>${helpBtn('pools')}<span class="sp"></span>${adm?`<button class="btn pri" onclick="editPool(-1)">${ic('plus')}New pool</button>`:''}</div>
   ${poolRows.trim()||'<div class="empty">No pools yet. A pool plays the calls of one group on other groups too.</div>'}
   ${helpNote('pools','A pool switches groups together inside this server. A call on a group of the pool is played 1:1 on the other groups, as an ordinary group call on each group\'s own GSSI, so the radios see nothing unusual. <i>Both ways</i>: every group of the pool reaches all others. <i>One way</i>: only the first group is copied, to the rest. A group that has a call of its own waits until that over has ended and then joins. Changes apply at once and are saved to the config file, no restart. Federation peers do not get the copies.')}
   ${msgDiv('pool-msg')}</div>
  ${tgCard(adm,pools)}
  <div class="card"><div class="ch"><h2>Assign radios to a group (DGNA)</h2>${helpBtn('dgna')}<span class="sp"></span>${searchBox('dg','ISSI, group or station…')}</div>
   ${helpNote('dgna','Dynamic group number assignment (SS-DGNA, ETSI EN 300 392-12-22): the station tells the selected radios to add or remove a group, with an optional name the radio shows. Each command goes to the station the radio is registered on, over its control connection (FlowStation <code>[command]</code>). The answer of each radio shows under <i>Last DGNA</i>. ★ = group assigned by DGNA. Apps and other terminals cannot be reached.')}
   <div class="tw"><table><thead><tr><th style="width:44px"><input type="checkbox" class="chk" aria-label="Select all" ${allSel?'checked':''} ${adm?'':'disabled'} onchange="selAll(this.checked)"></th><th>ISSI</th><th>Station</th><th>Cell (global ID)</th><th>Groups now</th><th>Last DGNA</th></tr></thead><tbody>
   ${radios.map(r=>`<tr${r.radio?'':' class="off"'}><td><input type="checkbox" class="chk" aria-label="Select ${r.issi}" ${UI.sel.has(r.issi)?'checked':''} ${adm&&r.radio?'':'disabled'} onchange="selOne(${r.issi},this.checked)"></td><td>${issiCell(r.issi,true)}</td><td><b class="m">${esc(r.station||'—')}</b></td><td>${r.cell?`<span class="chip">${esc(r.cell)}</span>`:`<span class="mut">${r.radio?'—':'— (terminal)'}</span>`}</td><td>${groupsOf(r)}</td><td>${r.radio?dg(r):'<span class="mut">not a radio, DGNA n/a</span>'}</td></tr>`).join('')||'<tr><td colspan="6" class="mut">No radios registered</td></tr>'}
   </tbody></table></div>
   ${adm?`<div class="selbar"><b>${sel} selected</b><span class="arrow">→</span>
    ${dgSaved(f)}<input class="f" id="dg-gssi" placeholder="GSSI" inputmode="numeric" style="width:110px" value="${esc(f.gssi)}" oninput="UI.dgf.gssi=this.value">
    <input class="f" id="dg-name" placeholder="Name (optional)" maxlength="15" style="width:160px" value="${esc(f.name)}" oninput="UI.dgf.name=this.value">
    <div class="seg" role="group" aria-label="Assign or remove"><button aria-pressed="${f.assign}" onclick="UI.dgf.assign=true;render(true)">Assign</button><button aria-pressed="${!f.assign}" onclick="UI.dgf.assign=false;render(true)">Remove</button></div>
    <label class="mut" style="font-size:12.5px">Attachment <select class="f" id="dg-mode" style="max-width:220px" onchange="UI.dgf.mode=+this.value">${['0 · attached permanently','1 · attach on next ITSI attach','2 · not allowed on next ITSI attach','3 · attach on next location update','4 · not attached, radio may request','5 · not attached, radio may not request'].map((t,i)=>`<option value="${i}" ${f.mode===i?'selected':''}>${t}</option>`).join('')}</select></label>
    <span class="sp"></span><button class="btn" onclick="UI.sel.clear();render(true)" ${sel?'':'disabled'}>Clear</button><button class="btn pri" onclick="sendDgna()" ${sel?'':'disabled'}>Send DGNA to ${sel} radio${sel===1?'':'s'}</button></div>
    ${msgDiv('dg-msg')}`:'<div class="empty">Log in as an admin to send DGNA.</div>'}
  </div>`;
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
const plain=p=>({name:p.name,enabled:p.enabled,both_ways:p.both_ways,groups:p.groups,remote:(p.remote||[]).map(r=>({network:r.network,gssi:r.gssi})),linked:!!p.linked});
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
  const p={name:'',groups:[own],remote:[{network:net,gssi:tg}],both_ways:true,enabled:$('le-on').getAttribute('aria-checked')==='true',linked:true};
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
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="pe-on" aria-checked="${p.enabled}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Switched on</label>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="savePool(${i})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('pe-name').focus(),30);
}
function setDir(b){document.querySelectorAll('#pe-dir button').forEach((x,j)=>{const on=(j===0)===b;x.setAttribute('aria-pressed',on);x.setAttribute('aria-checked',on);});}
function closeSheet(){$('scrim').classList.remove('on');}
document.addEventListener('keydown',e=>{if(e.key==='Escape')closeSheet();});
async function savePool(i){
  const groups=$('pe-groups').value.split(/[\s,;]+/).filter(Boolean).map(Number);
  if(groups.some(g=>!Number.isInteger(g))){$('sheet-msg').className='msg err';$('sheet-msg').textContent='Groups must be numbers.';return;}
  const remote=[];
  for(const t of ($('pe-remote')?$('pe-remote').value:'').split(/[,;]+/).map(x=>x.trim()).filter(Boolean)){const k=t.lastIndexOf(':'),g=+t.slice(k+1);
    if(k<1||!Number.isInteger(g)){$('sheet-msg').className='msg err';$('sheet-msg').textContent=`"${t}" is not network:GSSI.`;return;}remote.push({network:t.slice(0,k).trim(),gssi:g});}
  const p={name:$('pe-name').value.trim(),groups,remote,both_ways:document.querySelector('#pe-dir button').getAttribute('aria-pressed')==='true',enabled:$('pe-on').getAttribute('aria-checked')==='true'};
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
const netPlain=n=>({name:n.name,enabled:n.enabled,mcc:n.mcc,mnc:n.mnc,remote_host:n.remote_host,path:n.path,username:n.username,password:'',user_agent:n.user_agent,bridge_issi:n.bridge_issi,register_talkers:n.register_talkers!==false,register_all:!!n.register_all,export_stations:[...(n.export_stations||[])],never_export:[...(n.never_export||[])],never_forward:[...(n.never_forward||[])],forward_unknown:!!n.forward_unknown,forward_lip:n.forward_lip!==false,reconnect_interval_seconds:n.reconnect_interval_seconds,tls:n.tls,tls_server_name:n.tls_server_name,tls_ca_path:n.tls_ca_path,tls_pinned_cert_path:n.tls_pinned_cert_path});
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
   <div style="display:flex;gap:10px">${fld('ms-mcc','MCC',M.mcc||'','inputmode="numeric" placeholder="901"')}${fld('ms-mnc','MNC',M.mnc||'','inputmode="numeric" placeholder="9999"')}</div>
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
   <div style="display:flex;gap:10px">${fld('ne-mcc','MCC',n.mcc||'','inputmode="numeric" placeholder="901"')}${fld('ne-mnc','MNC',n.mnc||'','inputmode="numeric" placeholder="9000"')}</div>
   <div style="display:flex;gap:10px"><div style="flex:3">${fld('ne-host','Server (host)',hp.host,'placeholder="brew.example.net" autocomplete="off"')}</div><div style="flex:1">${fld('ne-port','Port',hp.port,'inputmode="numeric" placeholder="443 / 9000"')}</div></div>
   ${fld('ne-path','Path',n.path,'placeholder="/brew"')}
   <div style="display:flex;gap:10px">${fld('ne-user','Login (Basestation ID there)',n.username,'autocomplete="off"')}${fld('ne-pw','Password',hasPw?'':'','type="password" autocomplete="new-password" placeholder="'+(hasPw?'unchanged':'')+'"')}</div>
   ${fld('ne-ua','Connection name (how this server introduces itself there)',n.user_agent,'maxlength="80" placeholder="'+esc(D.nets.default_user_agent)+'"')}
   ${fld('ne-issi','Bridge ISSI in that network (0 = none)',n.bridge_issi,'inputmode="numeric"')}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-reg" aria-checked="${n.register_talkers}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Register radios there (those in a bridged group here, like a base station)</label>
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-all" aria-checked="${n.register_all}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Register all: also every radio of stations with Multi Network (mni) or listed below</label>
   ${fld('ne-exp','Also export the radios of these stations, and these apps (station or app logins, or app ISSIs; comma separated)',n.export_stations.join(', '),'autocomplete="off"')}
   ${fld('ne-never','Never register these ISSIs there (comma separated)',n.never_export.join(', '),'autocomplete="off" inputmode="numeric"')}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-fwd" aria-checked="${n.forward_unknown}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Forward unknown targets: an individual call or SDS nobody here takes goes to this network<span class="mut" style="display:block;font-size:12px;margin-top:2px">The SMS Center does not keep these: an SDS to a radio not registered here goes there instead of being stored and resent here.</span></span></label>
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-lip" aria-checked="${n.forward_lip}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> <span>Forward LIP: position reports cross to and from this network<span class="mut" style="display:block;font-size:12px;margin-top:2px">Off: SDS with a location protocol identifier (0x03, 0x0A, 0x83) stay on this side; text and other SDS still pass.</span></span></label>
   ${fld('ne-nofwd','Do not forward these ISSIs: no individual call or SDS to or from them crosses to this network (comma separated)',n.never_forward.join(', '),'autocomplete="off" inputmode="numeric"')}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-tls" aria-checked="${n.tls}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> TLS (wss://)</label>
   ${fld('ne-sni','TLS server name (empty: host)',n.tls_server_name)}
   ${fld('ne-pin','Pinned certificate file (self-signed server, empty: off)',n.tls_pinned_cert_path)}
   <label style="display:flex;gap:10px;align-items:center"><button class="tog" role="switch" id="ne-on" aria-checked="${n.enabled}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button> Connect</label>
   <div class="msg" id="sheet-msg" style="padding:0"></div></div>
   <div class="acts"><button class="btn" onclick="closeSheet()">Cancel</button><button class="btn pri" onclick="saveNet(${i})">Save</button></div>`;
  $('scrim').classList.add('on');setTimeout(()=>$('ne-name').focus(),30);
}
// "host:port", "[v6]:port" or a bare host -> {host, port}.
function splitHost(h){h=h||'';const m=h.match(/^\[([^\]]*)\](?::(\d*))?$/);if(m)return {host:m[1],port:m[2]||''};
  const k=h.lastIndexOf(':');if(k>0&&h.indexOf(':')===k)return {host:h.slice(0,k),port:h.slice(k+1)};return {host:h,port:''};}
function saveNet(i){
  const host=v('ne-host').trim().replace(/^(wss?|https?):\/\//,'').replace(/\/.*$/,''),port=v('ne-port').trim(),m=$('sheet-msg');
  if(host&&!(/^\d+$/.test(port)&&+port>=1&&+port<=65535)){m.className='msg err';m.textContent='Enter the port (1 to 65535), e.g. 443 with TLS or 9000.';$('ne-port').focus();return;}
  const remote_host=host?(host.includes(':')?'['+host+']':host)+':'+port:'';
  const c=mnPlain();const old=i>=0?c.networks[i]:{reconnect_interval_seconds:15,tls_ca_path:'/etc/ssl/certs/ca-certificates.crt'};
  const n=Object.assign({},old,{name:v('ne-name').trim(),mcc:+v('ne-mcc')||0,mnc:+v('ne-mnc')||0,remote_host,path:v('ne-path').trim()||'/brew',
    username:v('ne-user').trim(),password:v('ne-pw'),user_agent:v('ne-ua').trim(),bridge_issi:+v('ne-issi')||0,register_talkers:$('ne-reg').getAttribute('aria-checked')==='true',register_all:$('ne-all').getAttribute('aria-checked')==='true',
    export_stations:v('ne-exp').split(',').map(x=>x.trim()).filter(Boolean),never_export:v('ne-never').split(/[\s,]+/).filter(Boolean).map(Number),never_forward:v('ne-nofwd').split(/[\s,]+/).filter(Boolean).map(Number),forward_unknown:$('ne-fwd').getAttribute('aria-checked')==='true',forward_lip:$('ne-lip').getAttribute('aria-checked')==='true',
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
     return `<tr><td>${first}${byInfo?(x.info?`<span class="sub m">${esc(x.username)}</span>`:'<span class="sub">no info</span>'):''}</td><td>${yn(x.brew)}</td><td>${yn(x.telemetry)}</td><td>${yn(x.control)}</td><td class="hide-s">${x.lat!=null?`<span class="m" style="font-size:12px">${x.lat.toFixed(4)}, ${x.lon.toFixed(4)}</span>`:'<span class="no">—</span>'}</td>
     <td style="text-align:right;white-space:nowrap">${adm?`<button class="btn" onclick="editAcc(${i})">Edit</button> <button class="btn danger hide-s" onclick="delAcc(${i})">Delete</button>`:''}</td></tr>`;}).join('')||'<tr><td colspan="6" class="mut">No logins</td></tr>'}
   </tbody></table></div>${msgDiv('acc-msg','padding:12px 18px')}</div>`;
};
function editAcc(i){
  const x=i>=0?D.acc.accounts[i]:{username:'',brew:true,telemetry:true,control:true,info:'',lat:null,lon:null};
  const sw=(id,on,label,hint)=>`<label style="display:flex;gap:12px;align-items:center"><button type="button" class="tog" role="switch" id="${id}" aria-checked="${on}" onclick="this.setAttribute('aria-checked',this.getAttribute('aria-checked')!=='true')"></button><span><b style="font-size:13.5px">${label}</b><br><span class="mut" style="font-size:12px">${hint}</span></span></label>`;
  $('sheet').innerHTML=`<div class="ch"><h3 id="sheet-title">${i>=0?'Edit login '+esc(x.username):'New login'}</h3></div><div class="sb">
   <div><label class="l" for="ac-user">Username</label><input class="f" id="ac-user" value="${esc(x.username)}" ${i>=0?'disabled':''} placeholder="e.g. 1000003" autocomplete="off" spellcheck="false"></div>
   <div><label class="l" for="ac-pw">Password</label><div style="display:flex;gap:8px"><input class="f" id="ac-pw" type="text" autocomplete="new-password" spellcheck="false" placeholder="${i>=0?'unchanged':'password'}" style="flex:1"><button type="button" class="btn" onclick="$('ac-pw').value=genPw()">Generate</button></div></div>
   <div><label class="l" for="ac-info">Info (optional)</label><input class="f" id="ac-info" maxlength="60" value="${esc(x.info||'')}" placeholder="e.g. Relay Main Square · Sysop OE4XYZ"></div>
   <div><label class="l" for="ac-pos">Position (optional): latitude, longitude</label><div style="display:flex;gap:8px"><input class="f" id="ac-pos" value="${x.lat!=null?x.lat+', '+x.lon:''}" placeholder="48.2082, 16.3738" autocomplete="off" spellcheck="false" style="flex:1"><button type="button" class="btn" onclick="hereFix()" title="Use the position of this device">${ic('pin')}Here</button></div><div class="mut" style="font-size:12px;margin-top:4px">Shown on the map. Paste it from a map app, or take this device's position.</div></div>
   ${sw('ac-b',x.brew,'Brew','Calls and SDS. Username = number, 1 to 7 digits')}
   ${sw('ac-t',x.telemetry,'Telemetry','FlowStation status reports')}
   ${sw('ac-c',x.control,'Control','Commands to the station: DGNA, kick, restart')}
   <div class="mut" style="font-size:12px">Applies to the next connection at once, no restart.</div>
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
  const body={username:$('ac-user').value.trim(),password:$('ac-pw').value,brew:on('ac-b'),telemetry:on('ac-t'),control:on('ac-c'),info:$('ac-info').value.trim(),lat,lon};m.className='msg mut';m.textContent='Saving…';
  try{D.acc=await postJ('/api/accounts',body);closeSheet();setMsg('acc-msg','ok',`${body.username} saved. It works for the next connection.`);render(true);}
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
  const where=K==='issi'?(m=>m?`<span class="st"><i></i>${esc(m.cell?m.cell.name:(m.basestation_name||'registered'))}</span>`:'<span class="st i"><i></i>not registered</span>')
    :(n=>n===undefined?'<span class="st i"><i></i>no radios</span>':`<span class="st"><i></i>${n?n+' radio'+(n===1?'':'s'):'call running'}</span>`);
  const help=K==='issi'?'Names for ISSIs, for example the call sign and first name of a radio\'s user. They show next to the ISSI on Overview, Calls, SDS, Radios, Roaming and Routing. The login of a bridge (Brew login in Network mode, up to 12 digits, for example a link to another network) can be named here too; the name shows under the login on Stations &amp; Cells. Terminals (apps) and base stations are named on the Accounts page. Radios registered right now are listed too, so you can name them. Changes apply at once, without a restart, and are written to the config file (<code>[issi_names]</code>).'
    :'Names for groups (GSSI), for example the area or the purpose of a talkgroup. They show next to the group on Overview, Calls, Radios and Routing (a name a radio reports for its group comes first). Groups radios are on right now are listed too, so you can name them.<br><br><b>Access</b> decides who may use a group: <i>Open</i> (everyone), <i>Only these</i> (an allow list: only the listed ISSIs) or <i>Block these</i> (everyone but the listed ISSIs). <b>RX only</b> makes radios listeners: all of them with an empty list, or the listed ones (with <i>Only these</i>, listed listeners missing from the allow list are added to it on save); who may talk is then who is admitted and not listen-only. The server records an affiliation only for an admitted radio and drops a group call from a radio that may not talk (emergency calls always pass). A Basestation that supports it (FlowStation with <code>group-access</code>) gets the lists and refuses the attachment or the request to talk on the air itself; on other Basestations the radio can still attach and talk locally, but not across the network. <br><br>Which groups may be used at all (<b>managed talkgroups</b>) is set on the Routing page; naming a group here does not allow it, but a group with <i>Only these</i> counts as allowed there.<br><br><b>✕</b> hides a group without a name from this page while radios are on it (display only); the filter <i>Hidden</i> shows it again. Changes apply at once, without a restart, and are written to the config file (<code>[gssi_names]</code>, <code>[group_access]</code>, <code>[talkgroups]</code>).';
  return `<div class="bar">${seg('dirk',[['issi','Radios (ISSI)'],['gssi','Groups (GSSI)']])}${seg('issif',[['all','All'],['named','With name'],['unnamed','Without name']].concat(K==='gssi'?[['limited','Limited']].concat(hid.size?[['hidden','Hidden ('+hid.size+')']]:[]):[]))}${searchBox('issi',d.what+' or name…')}<span class="sp"></span>
    ${adm?`<button class="btn" onclick="exportNames()">Export</button><button class="btn" onclick="importNames()">Import list</button><button class="btn pri" onclick="editName('')">${ic('plus')}${K==='gssi'?'Add group':'Add name'}</button>`:''}</div>
  ${K==='issi'?csCard(adm):''}
  <div class="card"><div class="ch"><h2>${K==='issi'?'Radios':'Groups'}</h2><span class="cnt">${Object.keys(names).length} names</span>${helpBtn('dir'+K)}</div>
   ${helpNote('dir'+K,help)}
   <div class="tw"><table><thead><tr><th>${d.what}</th><th>Name</th>${K==='gssi'?'<th>Access</th>':''}<th class="hide-s">Now</th>${adm?'<th></th>':''}</tr></thead><tbody>
   ${rows.slice(0,1000).map(k=>`<tr><td><b class="m">${k}</b></td><td>${names[k]?esc(names[k]):K==='issi'&&(D.cs||{})[k]?`<span class="mut" title="From the ham callsign database (a Directory name comes first)">${esc(D.cs[k])} · database</span>`:'<span class="mut">—</span>'}</td>${K==='gssi'?`<td>${accCell(k)}</td>`:''}<td class="hide-s">${where(seen.get(k))}</td>
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
  if(!name&&!(K==='gssi'&&(had||acc.issis.trim()||acc.rx_only||(D.access||{})[issi]))){m.className='msg err';m.textContent='Enter a name (or delete it in the list).';return;}
  m.className='msg mut';m.textContent='Saving…';
  try{
    if(name||had)D[K]=await postJ(DIR[K].api+'/set',{issi,name});
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

/* ---------- SIP, SMS Center, Telemetry SDS, High Availability, Server settings (native, were classic frames) ---------- */
// Page data that only these pages need: asked when the page opens and with the live refresh.
function loadPageData(p,first){
  const t=[];const set=(k,u)=>t.push(getJ(u).then(v=>{D[k]=v;}).catch(e=>{D[k+'Err']=e.message;}));
  if(p==='sip')set('sip','/api/sip');
  if(p==='sipcfg'){set('sipc','/api/sip/config');if(first&&D.who.admin)set('sipFull','/api/config/sip/full');}
  if(p==='sms')set('smsc','/api/sms-center');
  if(p==='tsds'&&first)set('tele','/api/telemetry');
  if(p==='ha'){set('ha','/api/ha');if(first&&D.who.admin)set('haCfg','/api/config/ha');}
  if(p==='settings'&&first&&D.who.admin){set('dusers','/api/dashboard-users');set('blk','/api/config/blacklist');set('allow','/api/config/allowlist');set('btsLocs','/api/bts-locations?config_only=1');}
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
  <div class="card"><div class="ch"><h2>Stored messages</h2><span class="cnt">${list.length}</span>${helpBtn('smsc')}<span class="sp"></span><span class="mut" style="font-size:12.5px">${d.known_issis} radios known</span></div>${helpNote('smsc','SDS to a radio that is not reachable are kept here and delivered when it registers again, until they expire. <i>Delivering</i>: a delivery attempt is running. Admins can delete a stored message.')}
   <div class="tw"><table><thead><tr><th>Stored</th><th>From</th><th>To</th><th>Message</th><th>Status</th><th class="hide-s">Attempts</th><th class="hide-s">Expires</th>${adm?'<th></th>':''}</tr></thead><tbody>
   ${list.map(x=>`<tr><td class="m">${tmd(x.stored_at_ms)}</td><td class="m">${issiTxt(x.source_issi)}</td><td class="m">${issiTxt(x.destination)}</td><td>${x.text?esc(x.text):`<span class="m mut">${esc(String(x.data_hex||'').slice(0,24))}</span>`}</td><td>${x.in_flight?'<span class="chip w">Delivering</span>':'<span class="chip">Waiting</span>'}</td><td class="num hide-s">${x.attempts}</td><td class="m hide-s">${tmd(x.expires_at_ms)}</td>${adm?`<td style="text-align:right"><button class="btn danger" onclick="smsDel('${esc(x.id)}')">Delete</button></td>`:''}</tr>`).join('')||`<tr><td colspan="8" class="mut">No stored messages</td></tr>`}
   </tbody></table></div>${msgDiv('sms-msg','padding:12px 18px')}</div>`;
};
async function smsDel(id){if(!confirm('Delete this stored message? It will not be delivered.'))return;
  try{await api('/api/sms-center/'+id,{method:'DELETE'});setMsg('sms-msg','ok','Deleted.');}catch(e){setMsg('sms-msg','err',e.message);}loadPageData('sms',false);}

R.tsds=()=>{
  const t=D.tele;if(!t)return loading;
  const all=t.flatMap(s=>(s.recent_sds_out||[]).map(x=>({...x,bts:s.id}))).sort((a,b)=>b.at_ms-a.at_ms).slice(0,100);
  const list=all.filter(x=>kindOk('tsdsk',x.protocol_id));
  return `<div class="bar"><span class="sp"></span>${kindSel('tsdsk',all.map(x=>x.protocol_id))}</div><div class="card"><div class="ch"><h2>Telemetry SDS</h2><span class="cnt">${list.length}</span>${helpBtn('tsds')}</div>${helpNote('tsds','The last SDS each station reports over its telemetry link (FlowStation <code>[telemetry]</code>), newest first, up to 100. <i>Kind</i> comes from the protocol identifier (ETSI TS 100 392-2 table 29.21; hover for the value): Text, Position (LIP, location system), WAP / data, Multi-part (concatenated), Security (OTAK, PIN, end-to-end encrypted), System (managed DMO, DOTAM, AGNSS), App (user application range) or Other. Binary LIP is shown as undecoded.')}
   <div class="tw"><table><thead><tr><th>Time</th><th>Kind</th><th>Station</th><th class="hide-s">Direction</th><th>From</th><th>To</th><th>Text</th></tr></thead><tbody>
   ${list.map(x=>`<tr><td class="m">${tmd(x.at_ms)}</td><td>${sdsChip(x.protocol_id)}</td><td><b>${esc(x.bts)}</b></td><td class="hide-s">${esc(x.direction)}</td><td class="m">${issiTxt(x.source_issi)}</td><td class="m">${x.is_group?'TG '+tgTxt(x.dest_issi):issiTxt(x.dest_issi)}</td><td>${x.protocol_id===10&&!(x.text||'').trim()?'<span class="mut">binary LIP (undecoded)</span>':esc(x.text)}</td></tr>`).join('')||'<tr><td colspan="7" class="mut">No telemetry SDS yet</td></tr>'}
   </tbody></table></div></div>`;
};

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
   ${usersCard()}`;
};
// Dashboard users (Server settings, admins): who logs in, who is admin, which pages each one sees.
const pageName=id=>PG[id]?PG[id][1]:id;
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
  const boxes=PAGES.filter(p=>!p[5]).map(p=>`<label style="display:flex;gap:8px;align-items:center;font-size:13px"><input type="checkbox" class="chk du-pg" value="${p[0]}" ${x.admin||x.pages.includes(p[0])?'checked':''} ${x.admin?'disabled':''}> ${esc(p[1])}</label>`).join('');
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

    /// The polls must not ask admin-only routes for a visitor who is not signed in: their 401
    /// opens the login window, so an open (read-only) dashboard would show it every 5 s.
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
        assert!(PAGE_HTML.contains("Apps and other terminals (Terminal mode) have no cells"));
        assert!(PAGE_HTML.contains("a station without it is counted as a station, its cells are unknown"));
        assert!(PAGE_HTML.contains("conns.map(b=>") && PAGE_HTML.contains("function connGroups("), "one Brew connections row per station, not per connection");
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
        for page in ["R.sds=()=>{", "R.tsds=()=>{"] {
            let body = &PAGE_HTML[PAGE_HTML.find(page).unwrap()..];
            let body = &body[..body.find("\n};").unwrap()];
            assert!(body.contains("sdsChip(") && body.contains("kindSel(") && body.contains("kindOk("), "{page} shows and filters the kind");
        }
        assert!(PAGE_HTML.contains("${sdsChip(s.protocol_id)?' '+sdsChip(s.protocol_id):''}"), "Overview shows the kind");
    }

    #[test]
    fn former_classic_pages_are_native() {
        let pages = &PAGE_HTML[PAGE_HTML.find("const PAGES=[").unwrap()..];
        let pages = &pages[..pages.find("];").unwrap()];
        for id in ["sip", "sipcfg", "sms", "tsds", "ha", "settings"] {
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
}
