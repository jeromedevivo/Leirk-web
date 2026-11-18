#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)] // every COM call inside these fns is a WebView2 FFI call
//! Leirk: a Rust shell over WebView2. Tabs are data; only visible and recently used tabs own a webview.
use leirk::b64::{b64, unb64};
use leirk::calc::calculate;
use leirk::import::{self, favicon_key};
use leirk::omnibox::{resolve, Engine};
use leirk::privacy::{https_upgrade, strip_tracking};
use leirk::shields::{host, list_urls, Shields, Verdict, RESOURCES_URL};
use leirk::store::{self, merged_prefs, Bookmark, History, Profile};
use leirk::tabs::{tile_rects, CloseFocus, Tab, TabId, Tabs};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowBuilderExtWindows;
use tao::window::{ResizeDirection, Window, WindowBuilder};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::*;
use windows::core::{w, Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};
use wry::{
    MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows,
    WebViewExtWindows,
};

mod devapi;
mod devhost;
mod panels;
mod synchost;

enum Ev {
    Ui(String),
    Title(TabId, String),
    Url(TabId, String),
    Loaded(TabId, bool, String),
    NewTab(String),
    Blocked(TabId),
    Key(&'static str),
    Favicon(TabId, String),
    Audio(TabId, bool),
    Focus(TabId),
    Backdrop(String),
    ShieldsReady(Shields, u64, usize),
    Cosmetic(TabId, Vec<String>, Vec<String>),
    Gesture(TabId, String),
    Navigate(TabId, String),
    Quit,
    /// Developer-mode results computed off the UI thread, forwarded to the chrome as `L.dev(kind, data)`.
    Dev(String, Value),
    TechDb(std::sync::Arc<leirk::tech::Db>),
    Thumb(TabId, String),
    Repaint,
    /// A side panel's icon, remembered like a site's icon.
    SiteIcon(String, String),
    /// A command from the DevTools "Leirk" tab: (command, argument, inspected-tab nonce, reply channel).
    DevApi(String, Value, String, std::sync::mpsc::Sender<Value>),
    /// A page announced which DevTools nonce inspects it.
    DevHello(String, TabId),
}

const TICK: Duration = Duration::from_secs(30);
const GAP: f64 = 2.0;
/// Chromium switches wry passes by default; every webview of one environment must use the same set.
const WRY_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

const DIGITS: [&str; 9] = ["tab1", "tab2", "tab3", "tab4", "tab5", "tab6", "tab7", "tab8", "tab9"];

/// Global shortcuts, caught natively so they work whether the page or the chrome has focus.
fn shortcut(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<&'static str> {
    let key = char::from_u32(vk).unwrap_or('\0');
    Some(match (ctrl, shift, alt, vk) {
        (true, false, false, _) if key == 'T' || key == 'N' => "newtab",
        (true, false, false, _) if key == 'W' => "close",
        (true, false, false, _) if key == 'L' => "focus",
        (true, false, false, _) if key == 'K' || key == 'E' => "palette",
        (true, false, false, _) if key == 'D' => "bookmark",
        (true, false, false, _) if key == 'J' => "downloads",
        (true, false, false, _) if ('1'..='9').contains(&key) => DIGITS[(vk - '1' as u32) as usize],
        (true, true, false, _) if key == 'N' => "private",
        (true, true, false, _) if key == 'T' => "reopen",
        (true, true, false, _) if key == 'B' => "toggleBar",
        (true, true, false, _) if key == 'S' => "capture",
        (true, false, false, 0x09) => "next",
        (true, true, false, 0x09) => "prev",
        (true, false, false, 0x22) => "next", // Ctrl+PageDown
        (true, false, false, 0x21) => "prev", // Ctrl+PageUp
        (false, false, true, _) if key == 'D' => "focus",
        (false, false, true, 0x24) => "home", // Alt+Home
        (false, false, false, 0x71) => "palette", // F2, as in Vivaldi
        (false, false, false, 0x75) => "focus",   // F6
        _ => return None,
    })
}

/// Leirk's developer tools ship as a DevTools extension: they appear in F12, never in the regular interface.
fn write_devtools_extension(dir: &Path) -> PathBuf {
    let root = dir.join("extensions");
    let ext = root.join("leirk-devtools");
    let _ = std::fs::create_dir_all(&ext);
    for (name, body) in [
        ("manifest.json", include_str!("../ui/devtools/manifest.json")),
        ("devtools.html", include_str!("../ui/devtools/devtools.html")),
        ("devtools.js", include_str!("../ui/devtools/devtools.js")),
        ("panel.html", include_str!("../ui/devtools/panel.html")),
        ("panel.js", include_str!("../ui/devtools/panel.js")),
        ("panel.css", include_str!("../ui/devtools/panel.css")),
    ] {
        let _ = std::fs::write(ext.join(name), body);
    }
    root
}

/// Window and taskbar icon, decoded from the shipped PNG.
fn app_icon() -> Option<tao::window::Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/icon-64.png"))).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    tao::window::Icon::from_rgba(buf[..info.buffer_size()].to_vec(), info.width, info.height).ok()
}

/// Only web addresses may come from web content or sync files (no file:, javascript:, data: documents).
pub(crate) fn is_web_url(u: &str) -> bool {
    let l = u.trim_start().to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// Icons shown by the chrome: web images or inline images only (a file:// or UNC icon could leak NTLM credentials).
pub(crate) fn is_safe_icon(u: &str) -> bool {
    is_web_url(u) || u.starts_with("data:image/")
}

/// Windows tools by absolute path, so a planted copy elsewhere on PATH is never run.
pub(crate) fn system_tool(exe: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    if exe.eq_ignore_ascii_case("explorer.exe") { root.join(exe) } else { root.join("System32").join(exe) }
}

/// Vivaldi's default gestures (hold right button and draw) plus rocker gestures B/F.
fn gesture(path: &str) -> Option<&'static str> {
    Some(match path {
        "L" | "B" => "back",
        "R" | "F" => "forward",
        "D" => "newtab",
        "DR" => "close",
        "UD" => "reload",
        "DL" => "reopen",
        _ => return None,
    })
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// `data:<mime>;base64,<payload>` -> (mime, bytes): how adblock-rust hands over redirect resources.
fn data_url(u: &str) -> Option<(String, Vec<u8>)> {
    let (head, payload) = u.strip_prefix("data:")?.split_once(',')?;
    let mime = head.strip_suffix(";base64")?;
    Some((mime.to_string(), unb64(payload)?))
}

fn pwstr(f: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut p = PWSTR::null();
    if f(&mut p).is_ok() { take_pwstr(p) } else { String::new() }
}

/// Page script: F2/F6 forwarding, mouse and rocker gestures, generic cosmetic reporting, GPC.
/// Pages can post anything to the host; Rust honours only these message shapes.
const PAGE_JS: &str = r#"(function(){
if(window.__leirk)return;window.__leirk=1;
var top=window.top===window;
if(__GPC__){try{Object.defineProperty(Navigator.prototype,'globalPrivacyControl',{get:function(){return true},configurable:true})}catch(e){}}
var post=function(m){try{window.ipc.postMessage(m)}catch(e){}};
addEventListener('keydown',function(e){if((e.key==='F2'||e.key==='F6')&&!e.ctrlKey&&!e.altKey&&!e.shiftKey){e.preventDefault();post(e.key)}},true);
if(!top)return;
window.__lg=__LG__;window.__rk=__RK__;
var down=false,x=0,y=0,path='',moved=false,left=false,eat=false;
addEventListener('mousedown',function(e){
 if(e.button===2){if(left&&window.__rk){post('g:F');eat=true;e.preventDefault();return}down=true;x=e.clientX;y=e.clientY;path='';moved=false}
 else if(e.button===0){left=true;if(down&&window.__rk){post('g:B');eat=true;down=false;e.preventDefault();e.stopPropagation()}}
},true);
addEventListener('mousemove',function(e){if(!down||!window.__lg)return;var dx=e.clientX-x,dy=e.clientY-y;if(Math.abs(dx)<24&&Math.abs(dy)<24)return;
 var d=Math.abs(dx)>Math.abs(dy)?(dx>0?'R':'L'):(dy>0?'D':'U');if(path.slice(-1)!==d)path+=d;x=e.clientX;y=e.clientY;moved=true},true);
addEventListener('mouseup',function(e){if(e.button===0)left=false;if(e.button===2&&down){down=false;if(moved&&path){post('g:'+path);eat=true}}},true);
addEventListener('contextmenu',function(e){if(eat){e.preventDefault();e.stopPropagation();eat=false}},true);
window.__leirkHide=function(css){var s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s)};
var sc=new Set,si=new Set,qc=[],qi=[],t=0;
function add(el){var l=el.classList;if(l)for(var i=0;i<l.length;i++){var c=l[i];if(!sc.has(c)&&sc.size<20000){sc.add(c);qc.push(c)}}if(el.id&&!si.has(el.id)&&si.size<20000){si.add(el.id);qi.push(el.id)}}
function scan(r){if(r.nodeType!==1)return;add(r);var l=r.querySelectorAll('[class],[id]');for(var i=0;i<l.length;i++)add(l[i])}
function flush(){t=0;if(qc.length||qi.length)post('c:'+JSON.stringify({c:qc.splice(0),i:qi.splice(0)}))}
function sched(){if(!t)t=setTimeout(flush,120)}
function start(){scan(document.documentElement);flush();new MutationObserver(function(ms){for(var i=0;i<ms.length;i++){var n=ms[i].addedNodes;for(var j=0;j<n.length;j++)scan(n[j])}sched()}).observe(document.documentElement,{childList:true,subtree:true})}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',start);else start();
})();"#;

/// Cache file for a list URL (lists are refreshed every 4 days).
fn cache_name(url: &str) -> String {
    let s: String = url.trim_start_matches("https://").chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect();
    let tail: String = s.chars().rev().take(110).collect::<Vec<_>>().into_iter().rev().collect();
    format!("list-{tail}") // the prefix rules out "." and ".." as file names
}

fn fetch(url: &str, dir: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let file = dir.join(cache_name(url));
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(4 * 86400));
    if !fresh {
        let tmp = file.with_extension("part");
        let ok = std::process::Command::new(system_tool("curl.exe"))
            // HTTPS only, redirects included: a filter list can carry scriptlets, so it must not be swappable on the wire
            .args(["-sfL", "--proto", "=https", "--proto-redir", "=https", "--compressed", "--max-time", "90", "-o"])
            .arg(&tmp)
            .arg(url)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    std::fs::read_to_string(&file).ok() // a stale copy beats no protection when offline
}

/// Downloads Brave's lists and scriptlet resources in parallel and builds the engine off the UI thread.
fn load_shields(dir: PathBuf, urls: Vec<String>, custom: String, generation: u64, proxy: EventLoopProxy<Ev>) {
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        let texts: Vec<Option<String>> = urls
            .chunks(8)
            .flat_map(|chunk| {
                std::thread::scope(|s| {
                    let hs: Vec<_> = chunk.iter().map(|u| s.spawn(|| fetch(u, &dir))).collect();
                    hs.into_iter().map(|h| h.join().ok().flatten()).collect::<Vec<_>>()
                })
            })
            .collect();
        let loaded = texts.iter().filter(|t| t.is_some()).count();
        let mut all: String = texts.into_iter().flatten().collect::<Vec<_>>().join("\n");
        all.push('\n');
        all.push_str(&custom);
        let res = fetch(RESOURCES_URL, &dir).unwrap_or_default();
        let _ = proxy.send_event(Ev::ShieldsReady(Shields::new(&all).with_resources(&res), generation, loaded));
    });
}

/// State the WebView2 callbacks read while the event loop runs.
struct Shared {
    prefs: RefCell<Value>,
    shields: RefCell<Option<Shields>>,
    doc_scripts: RefCell<HashMap<TabId, String>>,
    upgraded: RefCell<HashMap<TabId, String>>,
    http_only: RefCell<HashSet<String>>,
    /// Developer mode: per-tab request log, JS errors, main document headers.
    netlog: RefCell<HashMap<TabId, std::collections::VecDeque<Value>>>,
    errors: RefCell<HashMap<TabId, Vec<Value>>>,
    doc_headers: RefCell<HashMap<TabId, Vec<(String, String)>>>,
}

impl Shared {
    fn on(&self, k: &str) -> bool {
        self.prefs.borrow()[k].as_bool().unwrap_or(false)
    }
    fn noscript(&self, url: &str) -> bool {
        let h = host(url);
        self.prefs.borrow()["noscript_sites"].as_array().is_some_and(|a| a.iter().any(|s| s == h))
    }
}

fn resource_kind(ctx: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> &'static str {
    match ctx {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "sub_frame",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        _ => "other",
    }
}

struct App {
    window: Window,
    ui: WebView,
    ctx: WebContext,
    args: String,
    proxy: EventLoopProxy<Ev>,
    dir: PathBuf,
    tabs: Tabs,
    views: HashMap<TabId, WebView>,
    shown: HashSet<TabId>,
    profile: Profile,
    history: History,
    shared: Rc<Shared>,
    shields_gen: u64,
    lists_loaded: usize,
    blocked: HashMap<TabId, u32>,
    blocked_total: u64,
    closed: Vec<String>,
    loading: HashSet<TabId>,
    content: (f64, f64, f64, f64),
    overlay: bool,
    /// Overlay opened, waiting for the page snapshot before covering the page.
    awaiting_backdrop: bool,
    dirty: bool,
    bookmarks_dirty: bool,
    /// Site icons by host (imported data: URLs or icon URLs seen while browsing).
    favicons: HashMap<String, String>,
    favicons_dirty: bool,
    restart_needed: bool,
    quitting: bool,
    tech_db: Option<std::sync::Arc<leirk::tech::Db>>,
    tech_pending: bool,
    cpu_prev: Option<(u64, Instant)>,
    thumbs: HashMap<TabId, String>,
    /// Side panel rect (logical px) and what it shows: "dev" or a panel id.
    side: (f64, f64, f64, f64),
    panel_open: Option<String>,
    panel_views: HashMap<String, WebView>,
    docked: HashMap<String, panels::Docked>,
    remote_tabs: Vec<Value>,
    sync_devices: Vec<Value>,
    last_sync: u64,
    last_gesture: Instant,
    /// The tab the DevTools request is about (None: the active tab).
    dev_target: Option<TabId>,
    dev_nonces: HashMap<String, TabId>,
    /// Developer results waiting for the DevTools tab to collect them.
    outbox: Vec<Value>,
}

impl App {
    fn pref(&self, k: &str) -> Value {
        self.shared.prefs.borrow()[k].clone()
    }
    fn pref_on(&self, k: &str) -> bool {
        self.shared.on(k)
    }
    fn pref_str(&self, k: &str) -> String {
        self.pref(k).as_str().unwrap_or("").to_string()
    }
    fn scale(&self) -> f64 {
        self.pref("ui_scale").as_f64().unwrap_or(100.0).clamp(75.0, 150.0) / 100.0
    }

    fn active_blank(&self) -> bool {
        self.tabs.get(self.tabs.active).is_none_or(|t| t.url.is_empty())
    }

    fn js(&self, code: &str) {
        let _ = self.ui.evaluate_script(code);
    }

    fn raise(hwnd: windows::Win32::Foundation::HWND) {
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    /// The chrome webview fills the window; page webviews sit on top of the content rect the chrome reports.
    fn layout(&mut self) {
        let s = self.window.inner_size().to_logical::<f64>(self.window.scale_factor());
        let _ = self.ui.set_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(s.width, s.height).into() });
        let visible: Vec<TabId> = if self.active_blank() { vec![] } else { self.tabs.visible() };
        for id in self.shown.clone() {
            if !visible.contains(&id) {
                self.hide(id);
            }
        }
        let (cx, cy, cw, ch) = self.content;
        let rects = tile_rects(visible.len(), cw.max(1.0), ch.max(1.0), GAP);
        for (&id, r) in visible.iter().zip(rects) {
            if !self.views.contains_key(&id) && !self.create_view(id) {
                continue;
            }
            let v = &self.views[&id];
            let _ = v.set_bounds(Rect { position: LogicalPosition::new(cx + r.0, cy + r.1).into(), size: LogicalSize::new(r.2, r.3).into() });
            if self.shown.insert(id) {
                let _ = v.set_visible(true);
                let _ = v.set_memory_usage_level(MemoryUsageLevel::Normal);
            }
            if !self.overlay || self.awaiting_backdrop {
                Self::raise(v.hwnd());
            }
        }
        self.layout_panel();
        if self.overlay && !self.awaiting_backdrop {
            Self::raise(self.ui.hwnd());
        }
        self.enforce_budget();
    }

    /// Hidden tabs drop to low-memory mode and get suspended (timers and JS stop) until shown again.
    fn hide(&mut self, id: TabId) {
        self.shown.remove(&id);
        if !self.views.contains_key(&id) {
            return;
        }
        self.capture_thumb(id);
        let Some(v) = self.views.get(&id) else { return };
        let _ = v.set_visible(false);
        if self.tabs.get(id).is_some_and(|t| t.audible) || !self.pref_on("suspend_bg") {
            return;
        }
        let _ = v.set_memory_usage_level(MemoryUsageLevel::Low);
        unsafe {
            if let Ok(w3) = v.webview().cast::<ICoreWebView2_3>() {
                let _ = w3.TrySuspend(&TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(()))));
            }
        }
    }

    fn unload(&mut self, id: TabId) {
        self.shown.remove(&id);
        self.views.remove(&id); // dropping the WebView closes its renderer
        self.loading.remove(&id);
        self.shared.doc_scripts.borrow_mut().remove(&id);
        if let Some(t) = self.tabs.get_mut(id) {
            t.loaded = false;
            t.audible = false;
        }
    }

    fn enforce_budget(&mut self) {
        let idle = self.profile.idle_minutes * 60;
        let keep_audible = self.pref_on("keep_audible");
        let never: Vec<String> = self.pref("never_unload").as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let protect = |t: &Tab| (keep_audible && t.audible) || never.iter().any(|h| host(&t.url).ends_with(h.as_str()));
        for id in self.tabs.unload_candidates(self.profile.max_live.max(1), idle, now(), &protect) {
            self.unload(id);
        }
    }

    fn create_view(&mut self, id: TabId) -> bool {
        let Some(tab) = self.tabs.get(id) else { return false };
        if tab.url.is_empty() {
            return false;
        }
        let container = tab.container.as_ref().filter(|_| !tab.private).map(|n| n.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(32).collect::<String>()).filter(|n| !n.is_empty());
        let (url, private) = (tab.url.clone(), tab.private);
        let (p1, p2, p3, p4) = (self.proxy.clone(), self.proxy.clone(), self.proxy.clone(), self.proxy.clone());
        let page_js = PAGE_JS
            .replace("__GPC__", if self.pref_on("gpc") { "true" } else { "false" })
            .replace("__LG__", if self.pref_on("gestures") { "true" } else { "false" })
            .replace("__RK__", if self.pref_on("rocker") { "true" } else { "false" });
        let mut builder = WebViewBuilder::new_with_web_context(&mut self.ctx);
        if let Some(name) = container {
            builder = builder.with_profile_name(format!("leirk-{name}")); // cookies and storage isolated per container
        }
        let built = builder
            .with_additional_browser_args(&self.args)
            .with_browser_extensions_enabled(true)
            .with_url(&url)
            .with_incognito(private)
            .with_visible(false)
            .with_devtools(true)
            .with_initialization_script(&page_js)
            .with_ipc_handler(move |req| {
                let body = req.body();
                let ev = match body.as_str() {
                    "F2" => Ev::Key("palette"),
                    "F6" => Ev::Key("focus"),
                    b if b.starts_with("g:") && b.len() < 12 => Ev::Gesture(id, b[2..].to_string()),
                    b if b.starts_with("devhello:") && b.len() < 80 => Ev::DevHello(b[9..].to_string(), id),
                    b if b.starts_with("c:") => {
                        let Ok(v) = serde_json::from_str::<Value>(&b[2..]) else { return };
                        let list = |k: &str| -> Vec<String> {
                            v[k].as_array().map(|a| a.iter().take(5000).filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
                        };
                        Ev::Cosmetic(id, list("c"), list("i"))
                    }
                    _ => return,
                };
                let _ = p4.send_event(ev);
            })
            .with_document_title_changed_handler(move |t| {
                let _ = p1.send_event(Ev::Title(id, t));
            })
            .with_on_page_load_handler(move |e, u| {
                let _ = p2.send_event(Ev::Loaded(id, matches!(e, PageLoadEvent::Finished), u));
            })
            .with_new_window_req_handler(move |u, _| {
                let _ = p3.send_event(Ev::NewTab(u));
                NewWindowResponse::Deny
            })
            .build_as_child(&self.window);
        let Ok(view) = built else { return false };
        unsafe {
            let _ = self.hook(&view, id);
        }
        self.views.insert(id, view);
        if let Some(t) = self.tabs.get_mut(id) {
            t.loaded = true;
        }
        true
    }

    /// Native WebView2 hooks: Shields, privacy rewrites, shortcuts, favicon, audio, focus, URL changes.
    unsafe fn hook(&self, view: &WebView, id: TabId) -> windows::core::Result<()> {
        let core = view.webview();
        let ctrl = view.controller();
        let env = view.environment();
        let mut tok = Default::default();

        let settings = core.Settings()?;
        let _ = settings.SetIsScriptEnabled(!self.shared.noscript(&self.tabs.get(id).map(|t| t.url.clone()).unwrap_or_default()));
        if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
            let _ = s4.SetIsPasswordAutosaveEnabled(self.pref_on("passwords"));
            let _ = s4.SetIsGeneralAutofillEnabled(self.pref_on("autofill"));
        }
        let _ = ctrl.SetZoomFactor(self.pref("zoom").as_f64().unwrap_or(100.0) / 100.0);
        self.apply_profile(&core);

        // service workers too, where the runtime supports it
        match core.cast::<ICoreWebView2_22>() {
            Ok(c22) => c22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
        }
        let (shared, proxy) = (self.shared.clone(), self.proxy.clone());
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let req = args.Request()?;
                let url = pwstr(|p| req.Uri(p));
                let page = pwstr(|p| sender.Source(p));
                let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                args.ResourceContext(&mut ctx)?;
                if ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT && host(&url) == host(&page) {
                    return Ok(()); // the page itself
                }
                let verdict = if ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING && shared.on("block_pings") {
                    Verdict::Block
                } else {
                    let Ok(guard) = shared.shields.try_borrow() else { return Ok(()) };
                    match guard.as_ref() {
                        Some(sh) => sh.check(&url, &page, resource_kind(ctx)),
                        None => Verdict::Allow,
                    }
                };
                if shared.dev("network") {
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                    let mut logs = shared.netlog.borrow_mut();
                    let log = logs.entry(id).or_default();
                    if log.len() >= 400 {
                        log.pop_front();
                    }
                    log.push_back(json!({"u": url, "m": pwstr(|p| req.Method(p)), "t": resource_kind(ctx), "b": verdict != Verdict::Allow, "s": 0, "z": null, "d": null, "ts": ts}));
                }
                match verdict {
                    Verdict::Allow => {
                        if shared.on("gpc") {
                            let _ = req.Headers()?.SetHeader(w!("Sec-GPC"), w!("1"));
                        }
                        return Ok(());
                    }
                    Verdict::Redirect(data) => match data_url(&data) {
                        Some((mime, bytes)) => {
                            let stream = SHCreateMemStream(Some(&bytes));
                            let headers = HSTRING::from(format!("Content-Type: {mime}\r\nAccess-Control-Allow-Origin: *"));
                            let resp = env.CreateWebResourceResponse(stream.as_ref(), 200, w!("OK"), &headers)?;
                            args.SetResponse(&resp)?;
                        }
                        None => args.SetResponse(&env.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?)?,
                    },
                    Verdict::Block => args.SetResponse(&env.CreateWebResourceResponse(None, 403, w!("Blocked by Leirk Shields"), w!(""))?)?,
                }
                let _ = proxy.send_event(Ev::Blocked(id));
                Ok(())
            })),
            &mut tok,
        )?;

        // Top-level navigations: query filter, HTTPS upgrade, per-site scripts, scriptlets + site CSS at document start.
        let (shared, proxy) = (self.shared.clone(), self.proxy.clone());
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let uri = pwstr(|p| args.Uri(p));
                if shared.on("strip_params") {
                    if let Some(clean) = strip_tracking(&uri) {
                        args.SetCancel(true)?;
                        let _ = proxy.send_event(Ev::Navigate(id, clean));
                        return Ok(());
                    }
                }
                if shared.on("https_upgrade") && !shared.http_only.borrow().contains(host(&uri)) {
                    if let Some(secure) = https_upgrade(&uri) {
                        args.SetCancel(true)?;
                        shared.upgraded.borrow_mut().insert(id, secure.clone());
                        let _ = proxy.send_event(Ev::Navigate(id, secure));
                        return Ok(());
                    }
                }
                let _ = sender.Settings()?.SetIsScriptEnabled(!shared.noscript(&uri));
                if let Some(old) = shared.doc_scripts.borrow_mut().remove(&id) {
                    let _ = sender.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(old));
                }
                let script = shared.shields.try_borrow().ok().and_then(|g| g.as_ref().and_then(|s| s.document_script(&uri)));
                if let Some(js) = script {
                    let shared2 = shared.clone();
                    let _ = sender.AddScriptToExecuteOnDocumentCreated(
                        &HSTRING::from(js),
                        &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |hr, sid| {
                            if hr.is_ok() {
                                shared2.doc_scripts.borrow_mut().insert(id, sid);
                            }
                            Ok(())
                        })),
                    );
                }
                Ok(())
            })),
            &mut tok,
        )?;

        // HTTPS upgrade fallback: a site without HTTPS goes back to http and is remembered for the session.
        let (shared, proxy) = (self.shared.clone(), self.proxy.clone());
        core.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut ok = BOOL::default();
                args.IsSuccess(&mut ok)?;
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                args.WebErrorStatus(&mut status)?;
                let up = shared.upgraded.borrow_mut().remove(&id);
                if let Some(secure) = up {
                    if !ok.as_bool() && status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                        shared.http_only.borrow_mut().insert(host(&secure).to_string());
                        let _ = proxy.send_event(Ev::Navigate(id, secure.replacen("https://", "http://", 1)));
                    }
                }
                Ok(())
            })),
            &mut tok,
        )?;

        self.hook_keys(&ctrl)?;
        self.dev_hook(&core, id)?;

        let proxy = self.proxy.clone();
        core.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(s) = sender {
                    let _ = proxy.send_event(Ev::Url(id, pwstr(|p| s.Source(p))));
                }
                Ok(())
            })),
            &mut tok,
        )?;

        if let Ok(w15) = core.cast::<ICoreWebView2_15>() {
            let proxy = self.proxy.clone();
            w15.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                    if let Some(w) = sender.and_then(|s| s.cast::<ICoreWebView2_15>().ok()) {
                        let _ = proxy.send_event(Ev::Favicon(id, pwstr(|p| w.FaviconUri(p))));
                    }
                    Ok(())
                })),
                &mut tok,
            )?;
        }

        if let Ok(w8) = core.cast::<ICoreWebView2_8>() {
            let proxy = self.proxy.clone();
            w8.add_IsDocumentPlayingAudioChanged(
                &IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
                    if let Some(w) = sender.and_then(|s| s.cast::<ICoreWebView2_8>().ok()) {
                        let mut b = BOOL::default();
                        let _ = w.IsDocumentPlayingAudio(&mut b);
                        let _ = proxy.send_event(Ev::Audio(id, b.as_bool()));
                    }
                    Ok(())
                })),
                &mut tok,
            )?;
        }

        let proxy = self.proxy.clone();
        ctrl.add_GotFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                let _ = proxy.send_event(Ev::Focus(id));
                Ok(())
            })),
            &mut tok,
        )?;
        Ok(())
    }

    unsafe fn hook_keys(&self, ctrl: &ICoreWebView2Controller) -> windows::core::Result<()> {
        let proxy = self.proxy.clone();
        let mut tok = Default::default();
        ctrl.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                args.KeyEventKind(&mut kind)?;
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
                    return Ok(());
                }
                let mut vk = 0u32;
                args.VirtualKey(&mut vk)?;
                if let Some(cmd) = shortcut(vk, key_down(VK_CONTROL), key_down(VK_SHIFT), key_down(VK_MENU)) {
                    args.SetHandled(true)?;
                    let _ = proxy.send_event(Ev::Key(cmd));
                }
                Ok(())
            })),
            &mut tok,
        )
    }

    /// Profile-wide settings shared by every webview: tracking prevention and the download folder.
    unsafe fn apply_profile(&self, core: &ICoreWebView2) {
        let Ok(profile) = core.cast::<ICoreWebView2_13>().and_then(|c| c.Profile()) else { return };
        let level = match self.pref_str("tracking").as_str() {
            "off" => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
            "basic" => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC,
            "strict" => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
            _ => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
        };
        if let Ok(p3) = profile.cast::<ICoreWebView2Profile3>() {
            let _ = p3.SetPreferredTrackingPreventionLevel(level);
        }
        let dl = self.pref_str("download_dir");
        if !dl.is_empty() && Path::new(&dl).is_dir() {
            let _ = profile.SetDefaultDownloadFolderPath(&HSTRING::from(dl));
        }
    }

    /// Clears browsing data in WebView2's profile (any webview reaches it; the chrome one always exists).
    fn clear_data(&mut self, since: Option<u64>, kinds: &[&str], done: Option<EventLoopProxy<Ev>>) {
        if kinds.contains(&"history") {
            let _ = match since {
                Some(s) => self.history.clear_since(s as i64),
                None => self.history.clear(),
            };
            self.closed.clear();
        }
        let mut mask = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
        for k in kinds {
            mask.0 |= match *k {
                "cookies" => COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE.0,
                "cache" => COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0 | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE.0,
                "downloads" => COREWEBVIEW2_BROWSING_DATA_KINDS_DOWNLOAD_HISTORY.0,
                "autofill" => COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL.0,
                "passwords" => COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE.0,
                _ => 0,
            };
        }
        let finish = move || {
            if let Some(p) = &done {
                let _ = p.send_event(Ev::Quit);
            }
        };
        if mask.0 == 0 {
            return finish();
        }
        unsafe {
            let Ok(p2) = self.ui.webview().cast::<ICoreWebView2_13>().and_then(|c| c.Profile()).and_then(|p| p.cast::<ICoreWebView2Profile2>()) else {
                return finish();
            };
            let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |_| {
                finish();
                Ok(())
            }));
            let _ = match since {
                Some(s) => p2.ClearBrowsingDataInTimeRange(mask, s as f64, now() as f64 + 60.0, &handler),
                None => p2.ClearBrowsingData(mask, &handler),
            };
        }
    }

    /// Captures the active page as a JPEG so the open sheet can blur it behind itself.
    fn capture_backdrop(&self) {
        let Some(v) = self.views.get(&self.tabs.active) else { return };
        let proxy = self.proxy.clone();
        unsafe {
            let Ok(stream) = CreateStreamOnHGlobal(Default::default(), true) else { return };
            let s2 = stream.clone();
            let _ = v.webview().CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
                &stream,
                &CapturePreviewCompletedHandler::create(Box::new(move |hr| {
                    hr?;
                    let hg = GetHGlobalFromStream(&s2)?;
                    let len = GlobalSize(hg);
                    let ptr = GlobalLock(hg) as *const u8;
                    if !ptr.is_null() && len > 0 {
                        let data = std::slice::from_raw_parts(ptr, len);
                        let _ = proxy.send_event(Ev::Backdrop(b64(data)));
                    }
                    let _ = GlobalUnlock(hg);
                    Ok(())
                })),
            );
        }
    }

    fn state(&self) -> Value {
        let active = self.tabs.get(self.tabs.active);
        let url = active.map(|t| t.url.clone()).unwrap_or_default();
        let site = host(&url).to_string();
        let (mut back, mut fwd) = (false, false);
        if let Some(v) = self.views.get(&self.tabs.active) {
            let (mut b, mut f) = (BOOL::default(), BOOL::default());
            unsafe {
                let _ = v.webview().CanGoBack(&mut b);
                let _ = v.webview().CanGoForward(&mut f);
            }
            (back, fwd) = (b.as_bool(), f.as_bool());
        }
        let tabs: Vec<Value> = self
            .tabs
            .list
            .iter()
            .map(|t| {
                json!({"id": t.id, "title": t.title, "url": t.url, "favicon": t.favicon, "stack": t.stack,
                       "loaded": t.loaded, "private": t.private, "pinned": t.pinned, "audible": t.audible,
                       "loading": self.loading.contains(&t.id), "container": t.container,
                       "errors": self.shared.errors.borrow().get(&t.id).map_or(0, |e| e.len())})
            })
            .collect();
        let top_sites = if self.active_blank() { self.history.top(8) } else { vec![] };
        let off = self.profile.shields_off.contains(&site);
        json!({
            "tabs": tabs, "active": self.tabs.active, "tiled": self.tabs.tiled, "url": url,
            "private": active.is_some_and(|t| t.private), "back": back, "forward": fwd,
            "loading": self.loading.contains(&self.tabs.active),
            "shields": {"site": site, "on": !off && self.pref_str("shields_level") != "off",
                        "count": self.blocked.get(&self.tabs.active).copied().unwrap_or(0),
                        "total": self.blocked_total, "ready": self.shared.shields.borrow().is_some(),
                        "lists": self.lists_loaded, "noscript": self.shared.noscript(&url)},
            "bookmarked": !url.is_empty() && self.profile.bookmarks.iter().any(|b| b.url == url),
            "live": self.views.len(), "topSites": top_sites, "maximized": self.window.is_maximized(),
            "engines": self.profile.engines, "defaultEngine": self.profile.default_engine,
            "maxLive": self.profile.max_live, "idle": self.profile.idle_minutes,
            "prefs": *self.shared.prefs.borrow(), "restart": self.restart_needed,
            "lists": leirk::shields::LISTS.iter().map(|l| l.id).collect::<Vec<_>>(),
            "panelOpen": self.panel_open, "docked": self.docked.keys().collect::<Vec<_>>(),
            "remoteTabs": self.remote_tabs, "syncDevices": self.sync_devices, "lastSync": self.last_sync, "device": synchost::device_name(),
        })
    }

    fn push(&mut self) {
        if self.favicons_dirty {
            self.favicons_dirty = false;
            let _ = store::save_json(&self.dir.join("favicons.json"), &self.favicons);
            self.js(&format!("L.favicons({})", json!(self.favicons)));
        }
        if self.bookmarks_dirty {
            self.bookmarks_dirty = false;
            self.js(&format!("L.bookmarks({})", json!(self.profile.bookmarks)));
        }
        self.js(&format!("L.state({})", self.state()));
    }

    fn save(&mut self) {
        let _ = std::fs::create_dir_all(&self.dir);
        // private tabs never touch the disk
        let mut session = serde_json::to_value(&self.tabs).unwrap_or_default();
        session["list"] = json!(self.tabs.list.iter().filter(|t| !t.private).collect::<Vec<_>>());
        let _ = store::save_json(&self.dir.join("session.json"), &session);
        let _ = store::save_json(&self.dir.join("profile.json"), &self.profile);
        let _ = store::save_json(&self.dir.join("stats.json"), &json!({"blocked": self.blocked_total}));
        self.dirty = false;
    }

    fn reload_shields(&mut self) {
        self.shields_gen += 1;
        let prefs = self.shared.prefs.borrow().clone();
        let custom = prefs["custom_rules"].as_str().unwrap_or("").to_string();
        load_shields(self.dir.join("lists"), list_urls(&prefs), custom, self.shields_gen, self.proxy.clone());
    }

    fn activate(&mut self, id: TabId) {
        self.tabs.activate(id, now());
        self.dirty = true;
        self.layout();
        if let Some(v) = self.views.get(&id) {
            let _ = v.focus();
        }
    }

    fn new_tab(&mut self, url: &str, private: bool, background: bool) {
        let after = (self.pref_str("new_tab_pos") == "right").then_some(self.tabs.active);
        let id = self.tabs.open(url, private, after);
        if background {
            return;
        }
        self.activate(id);
        if url.is_empty() {
            self.push(); // the field must see the blank tab before it takes focus
            self.command("focus", &Value::Null);
        }
    }

    fn navigate_tab(&mut self, id: TabId, url: String) {
        if let Some(t) = self.tabs.get_mut(id) {
            t.url = url.clone();
        }
        if let Some(v) = self.views.get(&id) {
            let _ = v.load_url(&url);
        }
    }

    fn navigate(&mut self, url: String) {
        if url.is_empty() {
            return;
        }
        let id = self.tabs.active;
        self.overlay = false;
        if let Some(t) = self.tabs.get_mut(id) {
            t.url = url.clone();
            t.title = url.clone();
        }
        self.blocked.remove(&id);
        if let Some(v) = self.views.get(&id) {
            let _ = v.load_url(&url);
        }
        self.layout();
        if let Some(v) = self.views.get(&id) {
            let _ = v.focus();
        }
        self.dirty = true;
    }

    fn close(&mut self, id: TabId) {
        if self.tabs.list.len() == 1 && self.pref_on("close_last_quits") {
            return self.quit();
        }
        if let Some(t) = self.tabs.get(id) {
            if !t.private && !t.url.is_empty() {
                self.closed.push(t.url.clone());
            }
        }
        self.unload(id);
        self.blocked.remove(&id);
        let focus = match self.pref_str("close_activate").as_str() {
            "left" => CloseFocus::Left,
            "right" => CloseFocus::Right,
            _ => CloseFocus::Recent,
        };
        self.tabs.close_with(id, focus);
        self.activate(self.tabs.active);
    }

    fn cycle(&mut self, step: isize) {
        if step == 1 && self.pref_on("mru") {
            if let Some(id) = self.tabs.recent_other() {
                return self.activate(id);
            }
        }
        let n = self.tabs.list.len() as isize;
        let i = self.tabs.list.iter().position(|t| t.id == self.tabs.active).unwrap_or(0) as isize;
        let id = self.tabs.list[(i + step).rem_euclid(n) as usize].id;
        self.activate(id);
    }

    fn with_active(&self, f: impl FnOnce(&ICoreWebView2) -> windows::core::Result<()>) {
        if let Some(v) = self.views.get(&self.tabs.active) {
            let _ = f(&v.webview());
        }
    }

    fn set_overlay(&mut self, open: bool) {
        self.awaiting_backdrop = false;
        if open && !self.overlay && !self.active_blank() && self.views.contains_key(&self.tabs.active) {
            self.capture_backdrop();
            self.awaiting_backdrop = true; // the chrome sends "raise" once the snapshot is painted
        }
        self.overlay = open;
        self.layout();
        if !open {
            if let Some(v) = self.views.get(&self.tabs.active) {
                let _ = v.focus();
            }
        }
    }

    fn query(&self, q: &str, seq: u64) {
        let ql = q.to_lowercase();
        let marks: Vec<&Bookmark> = self
            .profile
            .bookmarks
            .iter()
            .filter(|b| ql.split_whitespace().all(|w| b.title.to_lowercase().contains(w) || b.url.to_lowercase().contains(w)))
            .take(6)
            .collect();
        let hist = self.history.search(q, 8);
        let calc = if self.pref_on("calculator") { calculate(q) } else { None };
        self.js(&format!("L.results({seq},{})", json!({"bookmarks": marks, "history": hist, "calc": calc})));
    }

    fn log(&self, line: &str) {
        use std::io::Write;
        let path = self.dir.join("leirk.log");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
            let _ = std::fs::rename(&path, path.with_extension("old.log"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{} {line}", now());
        }
    }

    /// Settings with an effect beyond the UI apply here as soon as they change.
    fn apply_pref(&mut self, k: &str) {
        match k {
            "shields_level" | "lists" | "custom_lists" | "custom_rules" => self.reload_shields(),
            "gestures" | "rocker" => {
                let js = format!("window.__lg={};window.__rk={};", self.pref_on("gestures"), self.pref_on("rocker"));
                self.views.values().for_each(|v| drop(v.evaluate_script(&js)));
            }
            "zoom" => {
                let z = self.pref("zoom").as_f64().unwrap_or(100.0) / 100.0;
                self.views.values().for_each(|v| unsafe { drop(v.controller().SetZoomFactor(z)) });
            }
            "tracking" | "download_dir" => unsafe { self.apply_profile(&self.ui.webview()) },
            "ui_scale" => drop(self.ui.zoom(self.scale())),
            "autoplay_block" | "gpu" => self.restart_needed = true,
            "passwords" | "autofill" => {
                for v in self.views.values() {
                    unsafe {
                        if let Ok(s4) = v.webview().Settings().and_then(|s| s.cast::<ICoreWebView2Settings4>()) {
                            let _ = s4.SetIsPasswordAutosaveEnabled(self.pref_on("passwords"));
                            let _ = s4.SetIsGeneralAutofillEnabled(self.pref_on("autofill"));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Registers Leirk as a browser for the current user, then opens Windows' default-apps page on it.
    fn register_default_browser(&self) {
        let Ok(exe) = std::env::current_exe() else { return };
        let exe = exe.display().to_string();
        let cmd = format!("\"{exe}\" \"%1\"");
        let cap = r"HKCU\Software\Clients\StartMenuInternet\Leirk\Capabilities";
        let reg: &[&[&str]] = &[
            &["add", r"HKCU\Software\Classes\LeirkHTML", "/ve", "/d", "Leirk HTML", "/f"],
            &["add", r"HKCU\Software\Classes\LeirkHTML\shell\open\command", "/ve", "/d", &cmd, "/f"],
            &["add", r"HKCU\Software\Clients\StartMenuInternet\Leirk", "/ve", "/d", "Leirk", "/f"],
            &["add", r"HKCU\Software\Clients\StartMenuInternet\Leirk\shell\open\command", "/ve", "/d", &cmd, "/f"],
            &["add", cap, "/v", "ApplicationName", "/d", "Leirk", "/f"],
            &["add", cap, "/v", "ApplicationDescription", "/d", "Navigateur léger", "/f"],
            &["add", &format!(r"{cap}\URLAssociations"), "/v", "http", "/d", "LeirkHTML", "/f"],
            &["add", &format!(r"{cap}\URLAssociations"), "/v", "https", "/d", "LeirkHTML", "/f"],
            &["add", &format!(r"{cap}\FileAssociations"), "/v", ".html", "/d", "LeirkHTML", "/f"],
            &["add", &format!(r"{cap}\FileAssociations"), "/v", ".htm", "/d", "LeirkHTML", "/f"],
            &["add", r"HKCU\Software\RegisteredApplications", "/v", "Leirk", "/d", r"Software\Clients\StartMenuInternet\Leirk\Capabilities", "/f"],
        ];
        use std::os::windows::process::CommandExt;
        for args in reg {
            let _ = std::process::Command::new(system_tool("reg.exe")).args(*args).creation_flags(0x0800_0000).status();
        }
        let _ = std::process::Command::new(system_tool("explorer.exe")).arg("ms-settings:defaultapps?registeredAppUser=Leirk").spawn();
    }

    fn quit(&mut self) {
        self.undock_all();
        self.save();
        self.sync_now();
        let flags = self.pref("clear_on_exit");
        let kinds: Vec<&str> = ["history", "cookies", "cache"].into_iter().filter(|k| flags[*k].as_bool().unwrap_or(false)).collect();
        if kinds.is_empty() || self.quitting {
            std::process::exit(0);
        }
        self.quitting = true;
        let _ = self.window.set_visible(false);
        self.clear_data(None, &kinds, Some(self.proxy.clone()));
    }

    fn command(&mut self, c: &str, v: &Value) {
        if !matches!(c, "query" | "layout" | "log" | "raise" | "tabInfo") {
            self.log(&format!("cmd {c}")); // never the arguments: they carry URLs, searches and cookie values
        }
        if self.dev_command(c, v) || self.panel_command(c, v) || self.sync_command(c, v) {
            return;
        }
        let target = v.as_u64().unwrap_or(self.tabs.active);
        match c {
            "openContainer" => {
                let name = v["name"].as_str().unwrap_or("").to_string();
                let url = v["url"].as_str().map(String::from).unwrap_or_else(|| self.tabs.get(self.tabs.active).map(|t| t.url.clone()).unwrap_or_default());
                let after = (self.pref_str("new_tab_pos") == "right").then_some(self.tabs.active);
                let id = self.tabs.open(&url, false, after);
                if let Some(t) = self.tabs.get_mut(id) {
                    t.container = Some(name).filter(|n| !n.is_empty());
                }
                self.activate(id);
            }
            "layout" => {
                let s = self.scale();
                let f = |k: &str| v[k].as_f64().unwrap_or(0.0) * s;
                self.content = (f("x"), f("y"), f("w"), f("h"));
                self.side = (f("sx"), f("sy"), f("sw"), f("sh"));
                self.layout();
                return;
            }
            "nav" => {
                let private = self.tabs.get(self.tabs.active).is_some_and(|t| t.private);
                let engine = if private { self.pref_str("engine_private") } else { self.profile.default_engine.clone() };
                let url = resolve(v.as_str().unwrap_or(""), &self.profile.engines, &engine);
                self.navigate(url);
            }
            "open" | "openBg" | "openPrivate" => {
                let url = v.as_str().unwrap_or("").to_string();
                if c == "open" && self.active_blank() {
                    self.navigate(url)
                } else {
                    self.new_tab(&url, c == "openPrivate", c == "openBg")
                }
            }
            "newtab" => self.new_tab("", false, false),
            "private" => self.new_tab("", true, false),
            "home" => {
                let home = self.pref_str("home");
                if home.is_empty() { self.navigate_blank() } else { self.navigate(home) }
            }
            "close" => self.close(target),
            "reopen" => {
                if let Some(u) = self.closed.pop() {
                    self.new_tab(&u, false, false);
                }
            }
            "activate" => self.activate(target),
            "next" => self.cycle(1),
            "prev" => self.cycle(-1),
            "back" => self.with_active(|w| unsafe { w.GoBack() }),
            "forward" => self.with_active(|w| unsafe { w.GoForward() }),
            "reload" => self.with_active(|w| unsafe { w.Reload() }),
            "stop" => self.with_active(|w| unsafe { w.Stop() }),
            "downloads" => self.with_active(|w| unsafe { w.cast::<ICoreWebView2_9>()?.OpenDefaultDownloadDialog() }),
            "stack" => {
                if let (Some(a), Some(b)) = (v["id"].as_u64(), v["onto"].as_u64()) {
                    self.tabs.stack(a, b);
                }
            }
            "unstack" => self.tabs.unstack(target),
            "move" => {
                if let (Some(a), Some(i)) = (v["id"].as_u64(), v["index"].as_u64()) {
                    self.tabs.move_to(a, i as usize);
                }
            }
            "tile" => {
                let ids: Vec<TabId> = v.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                self.tabs.tile(&ids);
                if let Some(&first) = self.tabs.tiled.first() {
                    if !self.tabs.tiled.contains(&self.tabs.active) {
                        self.tabs.activate(first, now());
                    }
                }
                self.layout();
            }
            "untile" => {
                self.tabs.tile(&[]);
                self.layout();
            }
            "unload" => {
                if !self.tabs.visible().contains(&target) {
                    self.unload(target);
                }
            }
            "unloadOthers" => {
                let vis = self.tabs.visible();
                let ids: Vec<_> = self.views.keys().copied().filter(|i| !vis.contains(i)).collect();
                for i in ids {
                    self.unload(i);
                }
            }
            "pin" => {
                if let Some(t) = self.tabs.get_mut(target) {
                    t.pinned = !t.pinned;
                }
            }
            "shields" => {
                let site = host(&self.tabs.get(self.tabs.active).map(|t| t.url.clone()).unwrap_or_default()).to_string();
                if site.is_empty() {
                    return;
                }
                match self.profile.shields_off.iter().position(|s| *s == site) {
                    Some(i) => drop(self.profile.shields_off.remove(i)),
                    None => self.profile.shields_off.push(site),
                }
                if let Some(sh) = self.shared.shields.borrow_mut().as_mut() {
                    sh.disabled_sites = self.profile.shields_off.iter().cloned().collect();
                }
                self.blocked.remove(&self.tabs.active);
                self.with_active(|w| unsafe { w.Reload() });
            }
            "noscript" => {
                let site = host(&self.tabs.get(self.tabs.active).map(|t| t.url.clone()).unwrap_or_default()).to_string();
                let mut list: Vec<Value> = self.pref("noscript_sites").as_array().cloned().unwrap_or_default();
                match list.iter().position(|s| *s == site) {
                    Some(i) => drop(list.remove(i)),
                    None if !site.is_empty() => list.push(json!(site)),
                    None => {}
                }
                self.set_pref("noscript_sites", json!(list));
                self.with_active(|w| unsafe { w.Reload() });
            }
            "bookmark" => {
                let Some(t) = self.tabs.get(self.tabs.active) else { return };
                if t.url.is_empty() {
                    return;
                }
                let (url, title) = (t.url.clone(), t.title.clone());
                if !self.profile.bookmarks.iter().any(|b| b.url == url) {
                    self.profile.bookmarks.push(Bookmark { title, url, folder: String::new(), ts: now() as i64 });
                    self.bookmarks_dirty = true;
                }
                self.push();
                return self.js("L.bookmarkAdded()");
            }
            "bmEdit" => {
                let url = v["url"].as_str().unwrap_or("");
                if let Some(b) = self.profile.bookmarks.iter_mut().find(|b| b.url == url) {
                    if let Some(t) = v["title"].as_str() {
                        b.title = t.to_string();
                    }
                    if let Some(f) = v["folder"].as_str() {
                        b.folder = f.trim_matches('/').to_string();
                    }
                    b.ts = now() as i64;
                }
                self.bookmarks_dirty = true;
            }
            "bmRemove" => {
                let url = v.as_str().unwrap_or("");
                self.profile.bookmarks.retain(|b| b.url != url);
                self.profile.deleted.push((url.to_string(), now() as i64));
                self.bookmarks_dirty = true;
            }
            "bmFolderRemove" => {
                let f = v.as_str().unwrap_or("").to_string();
                let gone: Vec<String> = self.profile.bookmarks.iter().filter(|b| b.folder == f || b.folder.starts_with(&format!("{f}/"))).map(|b| b.url.clone()).collect();
                self.profile.bookmarks.retain(|b| !gone.contains(&b.url));
                self.profile.deleted.extend(gone.into_iter().map(|u| (u, now() as i64)));
                self.bookmarks_dirty = true;
            }
            "toggleBar" => {
                let next = if self.pref_str("bookmarks_bar") == "always" { "never" } else { "always" };
                self.set_pref("bookmarks_bar", json!(next));
            }
            "overlay" => return self.set_overlay(v.as_bool().unwrap_or(false)),
            "raise" => {
                if self.overlay && self.awaiting_backdrop {
                    self.awaiting_backdrop = false;
                    self.layout();
                }
                return;
            }
            "query" => return self.query(v["q"].as_str().unwrap_or(""), v["seq"].as_u64().unwrap_or(0)),
            "drag" => return drop(self.window.drag_window()),
            "resize" => return drop(self.window.drag_resize_window(ResizeDirection::North)),
            "min" => return self.window.set_minimized(true),
            "max" => self.window.set_maximized(!self.window.is_maximized()),
            "devtools" => {
                if let Some(v) = self.views.get(&self.tabs.active) {
                    v.open_devtools();
                }
            }
            "imports" => {
                let list: Vec<Value> = import::sources().iter().map(|s| json!({"name": s.name})).collect();
                return self.js(&format!("L.imports({})", Value::Array(list)));
            }
            "import" => {
                let sources = import::sources();
                let picked: Vec<_> = match v.as_u64() {
                    Some(i) => sources.into_iter().skip(i as usize).take(1).collect(),
                    None => sources,
                };
                let mut total = import::Report::default();
                for s in &picked {
                    let r = import::import(s, &mut self.profile, &mut self.history, &mut self.favicons);
                    total.bookmarks += r.bookmarks;
                    total.history += r.history;
                    total.engines += r.engines;
                }
                self.bookmarks_dirty = true;
                self.favicons_dirty = true;
                self.js(&format!("L.imported({})", json!(total)));
            }
            "pref" => {
                let Some(k) = v["k"].as_str() else { return };
                self.set_pref(k, v["v"].clone());
            }
            "resetPrefs" => {
                self.profile.prefs = json!({});
                *self.shared.prefs.borrow_mut() = merged_prefs(&self.profile.prefs);
                let _ = self.ui.zoom(self.scale());
                self.reload_shields();
            }
            "setting" => {
                match v["k"].as_str() {
                    Some("default") => self.profile.default_engine = v["v"].as_str().unwrap_or("br").into(),
                    Some("maxLive") => self.profile.max_live = v["v"].as_u64().unwrap_or(4).clamp(1, 32) as usize,
                    Some("idle") => self.profile.idle_minutes = v["v"].as_u64().unwrap_or(15),
                    _ => {}
                }
                self.enforce_budget();
            }
            "engineAdd" => {
                let e: Option<Engine> = serde_json::from_value(v.clone()).ok();
                if let Some(e) = e.filter(|e| e.url.contains("%s") && is_web_url(&e.url) && !e.keyword.trim().is_empty()) {
                    self.profile.engines.retain(|x| !x.keyword.eq_ignore_ascii_case(&e.keyword));
                    self.profile.engines.push(e);
                }
            }
            "engineRemove" => {
                let k = v.as_str().unwrap_or("");
                if k != self.profile.default_engine {
                    self.profile.engines.retain(|e| e.keyword != k);
                }
            }
            "clearData" => {
                let since = v["since"].as_u64().filter(|&s| s > 0).map(|s| now().saturating_sub(s));
                let kinds: Vec<String> = v["kinds"].as_array().map(|a| a.iter().filter_map(|k| k.as_str().map(String::from)).collect()).unwrap_or_default();
                let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
                self.clear_data(since, &kinds, None);
            }
            "clearHistory" => {
                let _ = self.history.clear();
            }
            "defaultBrowser" => self.register_default_browser(),
            "openFolder" => {
                let dir = match v.as_str() {
                    Some("downloads") => {
                        let d = self.pref_str("download_dir");
                        if d.is_empty() { std::env::var("USERPROFILE").map(|p| format!("{p}\\Downloads")).unwrap_or_default() } else { d }
                    }
                    _ => self.dir.display().to_string(),
                };
                if !dir.is_empty() && !dir.starts_with('/') {
                    let _ = std::process::Command::new(system_tool("explorer.exe")).arg(dir).spawn();
                }
            }
            "quit" => return self.quit(),
            k if k.starts_with("tab") => {
                let n: usize = k[3..].parse().unwrap_or(1);
                let last = self.tabs.list.len() - 1;
                // Ctrl+9 is always the last tab, as in every browser
                let i = if n == 9 { last } else { (n - 1).min(last) };
                let id = self.tabs.list[i].id;
                self.activate(id);
            }
            "log" => return self.log(&format!("ui {}", v.as_str().unwrap_or(""))),
            "focus" | "palette" => {
                let _ = self.ui.focus();
                return self.js(if c == "focus" { "L.focus()" } else { "L.palette()" });
            }
            _ => return,
        }
        self.dirty = true;
        // user data is written at once: an import or a new bookmark must survive a crash
        if matches!(c, "import" | "bookmark" | "bmEdit" | "bmRemove" | "bmFolderRemove" | "pref" | "setting" | "engineAdd" | "engineRemove" | "resetPrefs") {
            self.save();
            if c != "pref" || v["k"] == "sync_folder" {
                self.sync_now();
            }
        }
        self.push();
    }

    fn navigate_blank(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.tabs.active) {
            t.url.clear();
            t.title.clear();
        }
        let id = self.tabs.active;
        self.unload(id);
        self.layout();
        self.command("focus", &Value::Null);
    }

    fn set_pref(&mut self, k: &str, v: Value) {
        if !self.profile.prefs.is_object() {
            self.profile.prefs = json!({});
        }
        self.profile.prefs[k] = v;
        self.profile.prefs_ts = now() as i64;
        *self.shared.prefs.borrow_mut() = merged_prefs(&self.profile.prefs);
        self.apply_pref(k);
        self.dirty = true;
    }

    fn event(&mut self, ev: Ev) {
        match ev {
            Ev::Ui(msg) => {
                if let Ok(m) = serde_json::from_str::<Value>(&msg) {
                    let c = m["c"].as_str().unwrap_or("").to_string();
                    self.command(&c, &m["v"]);
                }
                return;
            }
            Ev::Key(k) => return self.command(k, &Value::Null),
            Ev::Gesture(from, path) => {
                // any page can post this message: honour it only from the visible tab, at human speed
                if !self.tabs.visible().contains(&from) || self.last_gesture.elapsed() < Duration::from_millis(300) {
                    return;
                }
                self.last_gesture = Instant::now();
                let rocker = path == "B" || path == "F";
                let on = if rocker { self.pref_on("rocker") } else { self.pref_on("gestures") };
                if let (true, Some(cmd)) = (on, gesture(&path)) {
                    self.command(cmd, &Value::Null);
                }
                return;
            }
            Ev::Navigate(id, url) => return self.navigate_tab(id, url),
            Ev::DevHello(nonce, id) => {
                if self.dev_nonces.len() >= 64 {
                    self.dev_nonces.clear();
                }
                self.dev_nonces.insert(nonce, id);
                return;
            }
            Ev::DevApi(c, v, nonce, reply) => {
                self.dev_target = self.dev_nonces.get(&nonce).copied().filter(|t| self.tabs.get(*t).is_some());
                let dev = self.pref_on("dev_mode");
                if dev {
                    match c.as_str() {
                        "devJs" => {
                            // toggle scripts for the inspected tab's site
                            let (prev, target) = (self.tabs.active, self.dev_tab());
                            self.tabs.active = target;
                            self.command("noscript", &Value::Null);
                            self.tabs.active = prev;
                            self.push();
                        }
                        "open" => {
                            if let Some(u) = v.as_str().filter(|u| is_web_url(u)) {
                                self.new_tab(u, false, false);
                            }
                        }
                        c if c.starts_with("dev") => drop(self.dev_command(c, &v)),
                        _ => {}
                    }
                }
                let url = self.tabs.get(self.dev_tab()).map(|t| t.url.clone()).unwrap_or_default();
                let prefs = self.shared.prefs.borrow().clone();
                let _ = reply.send(json!({"outbox": std::mem::take(&mut self.outbox), "dev": dev, "modules": prefs["dev_modules"],
                    "envs": prefs["dev_envs"], "noscript": self.shared.noscript(&url), "url": url}));
                self.dev_target = None;
                return;
            }
            Ev::Dev(kind, data) => {
                if !matches!(kind.as_str(), "capture" | "tabInfo") {
                    if kind == "dockerAct" {
                        return self.command("devDocker", &Value::Null);
                    }
                    return self.dev_out(&kind, data);
                }
                if kind == "dockerAct" {
                    return self.command("devDocker", &Value::Null);
                }
                return self.js(&format!("L.dev({},{})", json!(kind), data));
            }
            Ev::TechDb(db) => {
                self.tech_db = Some(db);
                if std::mem::take(&mut self.tech_pending) {
                    self.tech_detect();
                }
                return;
            }
            Ev::Thumb(id, data) => {
                if self.thumbs.len() >= 40 && !self.thumbs.contains_key(&id) {
                    let views = &self.views;
                    if let Some(k) = self.thumbs.keys().find(|k| !views.contains_key(k)).or(self.thumbs.keys().next()).copied() {
                        self.thumbs.remove(&k);
                    }
                }
                self.thumbs.insert(id, format!("data:image/jpeg;base64,{data}"));
                return;
            }
            Ev::Repaint => {}
            Ev::SiteIcon(key, uri) => {
                if !key.is_empty() && is_safe_icon(&uri) && !self.favicons.get(&key).is_some_and(|v| v.starts_with("data:")) {
                    self.favicons.insert(key, uri);
                    self.favicons_dirty = true;
                }
            }
            Ev::Quit => std::process::exit(0),
            Ev::Cosmetic(id, classes, ids) => {
                let (Some(v), Some(t)) = (self.views.get(&id), self.tabs.get(id)) else { return };
                let sels = self.shared.shields.borrow().as_ref().map(|s| s.generic_selectors(&t.url, &classes, &ids)).unwrap_or_default();
                if !sels.is_empty() {
                    let css = format!("{}{{display:none!important}}", sels.join(","));
                    let _ = v.evaluate_script(&format!("window.__leirkHide&&window.__leirkHide({})", json!(css)));
                }
                return;
            }
            Ev::Title(id, title) => {
                let Some(t) = self.tabs.get_mut(id) else { return };
                t.title = title.clone();
                if !t.private {
                    let _ = self.history.set_title(&t.url, &title);
                }
                self.dirty = true;
            }
            Ev::Url(id, url) => {
                if let Some(t) = self.tabs.get_mut(id) {
                    if t.url != url && !url.is_empty() {
                        t.url = url;
                        self.dirty = true;
                    }
                }
            }
            Ev::Loaded(id, done, url) => {
                if done {
                    self.loading.remove(&id);
                    if let Some(t) = self.tabs.get(id) {
                        if !t.private && url.starts_with("http") {
                            let _ = self.history.visit(&url, &t.title, now() as i64);
                        }
                    }
                } else {
                    self.loading.insert(id);
                    self.blocked.remove(&id);
                    // a new document starts: what the developer tools collected belongs to the old one
                    self.shared.errors.borrow_mut().remove(&id);
                    self.shared.netlog.borrow_mut().remove(&id);
                    self.shared.doc_headers.borrow_mut().remove(&id);
                }
            }
            Ev::NewTab(url) => {
                if !is_web_url(&url) && url != "about:blank" {
                    return self.log("blocked a page opening a non-web address in a new tab");
                }
                let private = self.tabs.get(self.tabs.active).is_some_and(|t| t.private);
                let bg = self.pref_on("links_bg");
                self.new_tab(&url, private, bg);
            }
            Ev::Blocked(id) => {
                *self.blocked.entry(id).or_default() += 1;
                self.blocked_total += 1;
                if id != self.tabs.active {
                    return; // background counters need no repaint
                }
            }
            Ev::Favicon(id, f) => {
                let Some(t) = self.tabs.get_mut(id) else { return };
                let f = if is_safe_icon(&f) { f } else { String::new() };
                t.favicon = f.clone();
                let key = favicon_key(&t.url);
                // remember the site's icon for bookmarks and history; imported data: icons work offline, keep them
                if !t.private && !f.is_empty() && !key.is_empty() && !self.favicons.get(&key).is_some_and(|v| v.starts_with("data:")) {
                    if self.favicons.insert(key, f).is_none() {
                        self.favicons_dirty = true;
                    }
                }
            }
            Ev::Audio(id, on) => {
                if let Some(t) = self.tabs.get_mut(id) {
                    t.audible = on;
                }
            }
            Ev::Focus(id) => {
                if id == self.tabs.active || !self.tabs.tiled.contains(&id) {
                    return;
                }
                self.tabs.activate(id, now());
            }
            Ev::Backdrop(data) => return self.js(&format!("L.backdrop('data:image/jpeg;base64,{data}')")),
            Ev::ShieldsReady(mut sh, generation, loaded) => {
                if generation != self.shields_gen {
                    return; // a newer list selection is already loading
                }
                sh.disabled_sites = self.profile.shields_off.iter().cloned().collect();
                *self.shared.shields.borrow_mut() = Some(sh);
                self.lists_loaded = loaded;
            }
        }
        self.push();
    }
}

fn main() {
    let dir = store::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let log = dir.join("leirk.log");
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(f, "{} PANIC {info}", now());
        }
    }));
    let mut profile: Profile = store::load_json(&dir.join("profile.json"));
    let prefs = merged_prefs(&profile.prefs);
    if !profile.prefs.is_object() {
        profile.prefs = json!({});
    }

    // Engine switches are fixed for the process lifetime; changing them asks for a restart.
    let mut args = WRY_ARGS.to_string();
    if prefs["autoplay_block"].as_bool().unwrap_or(true) {
        args += " --autoplay-policy=document-user-activation-required";
    }
    if !prefs["gpu"].as_bool().unwrap_or(true) {
        args += " --disable-gpu --disable-gpu-compositing";
    }

    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Leirk")
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(LogicalSize::new(520.0, 360.0))
        .with_window_icon(app_icon())
        .build(&event_loop)
        .expect("window");

    let mut ctx = WebContext::new(Some(dir.join("WebView2")));
    let extensions = write_devtools_extension(&dir);
    if let Some(api) = devapi::DevApi::start(proxy.clone()) {
        devapi::write_config(&extensions.join("leirk-devtools"), &api);
    }
    let p = proxy.clone();
    let ui = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_additional_browser_args(&args)
        .with_browser_extensions_enabled(true)
        .with_extensions_path(extensions)
        .with_html(
            include_str!("../ui/index.html")
                .replace("<!--EXTRA-->", concat!("<script>", include_str!("../ui/extra.js"), "</script>"))
                .replace("__LOGO__", &format!("data:image/png;base64,{}", b64(include_bytes!("../assets/icon-128.png")))),
        )
        .with_ipc_handler(move |req| {
            let _ = p.send_event(Ev::Ui(req.into_body()));
        })
        .with_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(1280.0, 820.0).into() })
        .build_as_child(&window)
        .expect("chrome webview");

    let mut tabs: Tabs = store::load_json(&dir.join("session.json"));
    match prefs["startup"].as_str() {
        Some("ntp") => tabs = Tabs::default(),
        Some("pages") => {
            tabs = Tabs::default();
            for u in prefs["startup_pages"].as_str().unwrap_or("").lines().map(str::trim).filter(|u| !u.is_empty()) {
                tabs.open(&resolve(u, &profile.engines, &profile.default_engine), false, None);
            }
            if let Some(t) = tabs.list.first() {
                tabs.active = t.id;
            }
        }
        _ => {}
    }
    if let Some(arg) = std::env::args().nth(1).filter(|a| !a.starts_with('-')) {
        let url = if Path::new(&arg).is_file() { Some(format!("file:///{}", arg.replace('\\', "/"))) } else { Some(arg).filter(|a| is_web_url(a)) };
        if let Some(url) = url {
            tabs.active = tabs.open(&url, false, None);
        }
    }
    if tabs.list.is_empty() {
        tabs.active = tabs.open("", false, None);
    }
    if tabs.get(tabs.active).is_none() {
        tabs.active = tabs.list[0].id;
    }
    let stats: Value = store::load_json(&dir.join("stats.json"));
    let history = History::open(&dir.join("history.db")).unwrap_or_else(|_| History::open(Path::new(":memory:")).expect("sqlite"));

    let mut app = App {
        window,
        ui,
        ctx,
        args,
        proxy: proxy.clone(),
        dir: dir.clone(),
        tabs,
        views: HashMap::new(),
        shown: HashSet::new(),
        profile,
        history,
        shared: Rc::new(Shared {
            prefs: RefCell::new(prefs),
            shields: RefCell::new(None),
            doc_scripts: RefCell::new(HashMap::new()),
            upgraded: RefCell::new(HashMap::new()),
            http_only: RefCell::new(HashSet::new()),
            netlog: RefCell::new(HashMap::new()),
            errors: RefCell::new(HashMap::new()),
            doc_headers: RefCell::new(HashMap::new()),
        }),
        shields_gen: 0,
        lists_loaded: 0,
        blocked: HashMap::new(),
        blocked_total: stats["blocked"].as_u64().unwrap_or(0),
        closed: vec![],
        loading: HashSet::new(),
        content: (0.0, 120.0, 1280.0, 700.0),
        overlay: false,
        awaiting_backdrop: false,
        dirty: false,
        bookmarks_dirty: true,
        favicons: store::load_json(&dir.join("favicons.json")),
        favicons_dirty: true,
        restart_needed: false,
        quitting: false,
        tech_db: None,
        tech_pending: false,
        cpu_prev: None,
        thumbs: HashMap::new(),
        side: (0.0, 0.0, 0.0, 0.0),
        panel_open: None,
        panel_views: HashMap::new(),
        docked: HashMap::new(),
        remote_tabs: vec![],
        sync_devices: vec![],
        last_sync: 0,
        last_gesture: Instant::now(),
        dev_target: None,
        dev_nonces: HashMap::new(),
        outbox: vec![],
    };
    unsafe {
        let _ = app.hook_keys(&app.ui.controller());
        app.apply_profile(&app.ui.webview());
    }
    let _ = app.ui.zoom(app.scale());
    // bookmarks imported before icons were: fetch theirs once from the installed browsers
    if app.favicons.is_empty() && !app.profile.bookmarks.is_empty() {
        let hosts: HashSet<String> = app.profile.bookmarks.iter().map(|b| favicon_key(&b.url)).collect();
        for src in import::sources() {
            import::import_favicons(&src, hosts.clone(), &mut app.favicons);
        }
    }
    app.tabs.activate(app.tabs.active, now());
    let _ = app.ui.focus();
    app.reload_shields();
    app.sync_now();

    event_loop.run(move |event, _, flow| match event {
        Event::NewEvents(StartCause::Init) => {
            *flow = ControlFlow::WaitUntil(Instant::now() + TICK);
            app.layout();
        }
        Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
            *flow = ControlFlow::WaitUntil(Instant::now() + TICK);
            if app.quitting {
                std::process::exit(0); // clearing data took too long; leave anyway
            }
            app.enforce_budget();
            if app.dirty {
                app.save();
            }
            if now().saturating_sub(app.last_sync) >= 120 {
                app.sync_now();
            }
            app.push();
        }
        Event::UserEvent(ev) => app.event(ev),
        Event::WindowEvent { event: WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }, .. } => {
            app.layout();
            app.push();
        }
        Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => app.quit(),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        for s in [&b""[..], b"f", b"fo", b"foo", b"(function(){})();"] {
            assert_eq!(unb64(&b64(s)).as_deref(), Some(s));
        }
        assert_eq!(unb64("**"), None);
    }

    #[test]
    fn data_urls_from_redirects() {
        let (mime, bytes) = data_url("data:application/javascript;base64,KGZ1bmN0aW9uKCl7fSkoKTs=").unwrap();
        assert_eq!(mime, "application/javascript");
        assert_eq!(bytes, b"(function(){})();");
        assert_eq!(data_url("data:text/plain,hello"), None);
        assert_eq!(data_url("https://x"), None);
    }

    #[test]
    fn shortcuts() {
        assert_eq!(shortcut('T' as u32, true, false, false), Some("newtab"));
        assert_eq!(shortcut('T' as u32, true, true, false), Some("reopen"));
        assert_eq!(shortcut('B' as u32, true, true, false), Some("toggleBar"));
        assert_eq!(shortcut('3' as u32, true, false, false), Some("tab3"));
        assert_eq!(shortcut(0x71, false, false, false), Some("palette"));
        assert_eq!(shortcut('T' as u32, false, false, false), None);
    }

    #[test]
    fn gestures_follow_vivaldi_defaults() {
        assert_eq!(gesture("L"), Some("back"));
        assert_eq!(gesture("B"), Some("back"));
        assert_eq!(gesture("DR"), Some("close"));
        assert_eq!(gesture("UD"), Some("reload"));
        assert_eq!(gesture("RLRL"), None);
    }

    #[test]
    fn list_cache_names_are_file_safe() {
        let n = cache_name("https://raw.githubusercontent.com/a/b/filters-2024.txt?x=1");
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'), "{n}");
        assert!(n.len() <= 120);
        assert_eq!(cache_name("https://.."), "list-..", "never a bare . or .. path component");
    }

    #[test]
    fn only_web_addresses_from_untrusted_sources() {
        assert!(is_web_url("https://a.com/") && is_web_url("HTTP://a.com"));
        for bad in ["file:///C:/x.html", "javascript:alert(1)", "data:text/html,x", r"\host\share", "about:blank"] {
            assert!(!is_web_url(bad), "{bad}");
        }
        assert!(is_safe_icon("data:image/png;base64,AA") && is_safe_icon("https://a.com/f.ico"));
        assert!(!is_safe_icon("file://attacker/share/x.ico") && !is_safe_icon("data:text/html,<b>"));
    }
}
