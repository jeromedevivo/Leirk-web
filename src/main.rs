#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)] // every COM call inside these fns is a WebView2 FFI call
//! Leirk: a Rust shell over WebView2. Tabs are data; only visible and recently used tabs own a webview.
use leirk::import;
use leirk::omnibox::resolve;
use leirk::shields::{host, Shields};
use leirk::store::{self, Bookmark, History, Profile};
use leirk::tabs::{tile_rects, TabId, Tabs};
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
use windows::core::{w, Interface, BOOL, PWSTR};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE};
use wry::{MemoryUsageLevel, NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder, WebViewExtWindows};

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
    ShieldsReady(Shields),
}

const TICK: Duration = Duration::from_secs(30);
const GAP: f64 = 2.0;

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
        (true, false, false, _) if key == 'K' => "palette",
        (true, false, false, _) if key == 'D' => "bookmark",
        (true, false, false, _) if ('1'..='9').contains(&key) => DIGITS[(vk - '1' as u32) as usize],
        (true, true, false, _) if key == 'N' => "private",
        (true, true, false, _) if key == 'T' => "reopen",
        (true, false, false, 0x09) => "next",
        (true, true, false, 0x09) => "prev",
        (false, false, true, _) if key == 'D' => "focus",
        (false, false, false, 0x71) => "palette", // F2, as in Vivaldi
        (false, false, false, 0x75) => "focus",   // F6
        _ => return None,
    })
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn pwstr(f: impl FnOnce(&mut PWSTR) -> windows::core::Result<()>) -> String {
    let mut p = PWSTR::null();
    if f(&mut p).is_ok() { take_pwstr(p) } else { String::new() }
}

/// Downloads the filter lists Brave ships with (refreshed every 4 days) and builds the engine off the UI thread.
fn load_shields(dir: PathBuf, proxy: EventLoopProxy<Ev>) {
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        let lists = [
            ("easylist", "https://easylist.to/easylist/easylist.txt"),
            ("easyprivacy", "https://easylist.to/easylist/easyprivacy.txt"),
            ("ubo", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt"),
            ("ubo-privacy", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/privacy.txt"),
            ("liste_fr", "https://easylist-downloads.adblockplus.org/liste_fr.txt"),
        ];
        let _ = std::fs::create_dir_all(&dir);
        let mut all = String::new();
        for (name, url) in lists {
            let file = dir.join(format!("{name}.txt"));
            let fresh = std::fs::metadata(&file)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(4 * 86400));
            if !fresh {
                let tmp = file.with_extension("part");
                let ok = std::process::Command::new("curl.exe")
                    .args(["-sfL", "--max-time", "90", "-o"])
                    .arg(&tmp)
                    .arg(url)
                    .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                    .status()
                    .is_ok_and(|s| s.success());
                if ok {
                    let _ = std::fs::rename(&tmp, &file);
                }
            }
            if let Ok(s) = std::fs::read_to_string(&file) {
                all.push_str(&s);
                all.push('\n');
            }
        }
        let _ = proxy.send_event(Ev::ShieldsReady(Shields::new(&all)));
    });
}

struct App {
    window: Window,
    ui: WebView,
    ctx: WebContext,
    proxy: EventLoopProxy<Ev>,
    dir: PathBuf,
    tabs: Tabs,
    views: HashMap<TabId, WebView>,
    shown: HashSet<TabId>,
    profile: Profile,
    history: History,
    shields: Rc<RefCell<Option<Shields>>>,
    blocked: HashMap<TabId, u32>,
    blocked_total: u64,
    closed: Vec<String>,
    loading: HashSet<TabId>,
    top: f64,
    overlay: bool,
    dirty: bool,
}

impl App {
    fn active_blank(&self) -> bool {
        self.tabs.get(self.tabs.active).is_none_or(|t| t.url.is_empty())
    }

    fn logical_size(&self) -> (f64, f64) {
        let s = self.window.inner_size().to_logical::<f64>(self.window.scale_factor());
        (s.width, s.height)
    }

    fn js(&self, code: &str) {
        let _ = self.ui.evaluate_script(code);
    }

    fn raise_ui(&self) {
        unsafe {
            let _ = SetWindowPos(self.ui.hwnd(), Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        }
    }

    fn layout(&mut self) {
        let (w, h) = self.logical_size();
        let full = self.overlay || self.active_blank();
        let ui_h = if full { h } else { self.top };
        let _ = self.ui.set_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(w, ui_h).into() });
        let visible: Vec<TabId> = if self.active_blank() { vec![] } else { self.tabs.visible() };
        for id in self.shown.clone() {
            if !visible.contains(&id) {
                self.hide(id);
            }
        }
        let rects = tile_rects(visible.len(), w, (h - self.top).max(1.0), GAP);
        for (&id, r) in visible.iter().zip(rects) {
            if !self.views.contains_key(&id) && !self.create_view(id) {
                continue;
            }
            let v = &self.views[&id];
            let _ = v.set_bounds(Rect { position: LogicalPosition::new(r.0, self.top + r.1).into(), size: LogicalSize::new(r.2, r.3).into() });
            if self.shown.insert(id) {
                let _ = v.set_visible(true);
                let _ = v.set_memory_usage_level(MemoryUsageLevel::Normal);
            }
        }
        if full {
            self.raise_ui();
        }
        self.enforce_budget();
    }

    /// Hidden tabs drop to low-memory mode and get suspended (timers and JS stop) until shown again.
    fn hide(&mut self, id: TabId) {
        self.shown.remove(&id);
        let Some(v) = self.views.get(&id) else { return };
        let _ = v.set_visible(false);
        if self.tabs.get(id).is_some_and(|t| t.audible) {
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
        if let Some(t) = self.tabs.get_mut(id) {
            t.loaded = false;
            t.audible = false;
        }
    }

    fn enforce_budget(&mut self) {
        let idle = self.profile.idle_minutes * 60;
        for id in self.tabs.unload_candidates(self.profile.max_live.max(1), idle, now()) {
            self.unload(id);
        }
    }

    fn create_view(&mut self, id: TabId) -> bool {
        let Some(tab) = self.tabs.get(id) else { return false };
        if tab.url.is_empty() {
            return false;
        }
        let (url, private) = (tab.url.clone(), tab.private);
        let (p1, p2, p3, p4) = (self.proxy.clone(), self.proxy.clone(), self.proxy.clone(), self.proxy.clone());
        let built = WebViewBuilder::new_with_web_context(&mut self.ctx)
            .with_url(&url)
            .with_incognito(private)
            .with_visible(false)
            .with_devtools(true)
            .with_document_title_changed_handler(move |t| {
                let _ = p1.send_event(Ev::Title(id, t));
            })
            .with_on_page_load_handler(move |e, u| {
                let _ = p2.send_event(Ev::Loaded(id, matches!(e, PageLoadEvent::Finished), u));
            })
            // WebView2 does not report bare function keys as accelerators, so pages forward F2/F6 themselves.
            // Pages can post anything here; only these two names are honoured.
            .with_initialization_script(
                "addEventListener('keydown',e=>{if((e.key==='F2'||e.key==='F6')&&!e.ctrlKey&&!e.altKey&&!e.shiftKey){e.preventDefault();window.ipc.postMessage(e.key)}},true)",
            )
            .with_ipc_handler(move |req| {
                let key = match req.body().as_str() {
                    "F2" => "palette",
                    "F6" => "focus",
                    _ => return,
                };
                let _ = p4.send_event(Ev::Key(key));
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

    /// Native WebView2 hooks: Shields network filter, shortcuts, favicon, audio, focus, URL changes.
    unsafe fn hook(&self, view: &WebView, id: TabId) -> windows::core::Result<()> {
        let core = view.webview();
        let ctrl = view.controller();
        let env = view.environment();
        let mut tok = Default::default();

        if let Ok(s4) = core.Settings()?.cast::<ICoreWebView2Settings4>() {
            let _ = s4.SetIsPasswordAutosaveEnabled(true);
            let _ = s4.SetIsGeneralAutofillEnabled(true);
        }

        core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
        let (shields, proxy) = (self.shields.clone(), self.proxy.clone());
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                let Ok(guard) = shields.try_borrow() else { return Ok(()) };
                let Some(sh) = guard.as_ref() else { return Ok(()) };
                let url = pwstr(|p| args.Request()?.Uri(p));
                let page = pwstr(|p| sender.Source(p));
                let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                args.ResourceContext(&mut ctx)?;
                let kind = match ctx {
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => {
                        if host(&url) == host(&page) {
                            return Ok(());
                        }
                        "sub_frame"
                    }
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
                    _ => "other",
                };
                if sh.should_block(&url, &page, kind) {
                    let resp = env.CreateWebResourceResponse(None, 403, w!("Blocked by Leirk Shields"), w!(""))?;
                    args.SetResponse(&resp)?;
                    let _ = proxy.send_event(Ev::Blocked(id));
                }
                Ok(())
            })),
            &mut tok,
        )?;

        self.hook_keys(&ctrl)?;

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
                       "loading": self.loading.contains(&t.id)})
            })
            .collect();
        let top_sites = if self.active_blank() { self.history.top(8) } else { vec![] };
        json!({
            "tabs": tabs, "active": self.tabs.active, "tiled": self.tabs.tiled, "url": url,
            "private": active.is_some_and(|t| t.private), "back": back, "forward": fwd,
            "loading": self.loading.contains(&self.tabs.active),
            "shields": {"site": site, "on": !self.profile.shields_off.contains(&site),
                        "count": self.blocked.get(&self.tabs.active).copied().unwrap_or(0),
                        "total": self.blocked_total, "ready": self.shields.borrow().is_some()},
            "bookmarked": !url.is_empty() && self.profile.bookmarks.iter().any(|b| b.url == url),
            "live": self.views.len(), "topSites": top_sites, "maximized": self.window.is_maximized(),
            "settings": {"engines": self.profile.engines, "default": self.profile.default_engine,
                         "maxLive": self.profile.max_live, "idle": self.profile.idle_minutes},
        })
    }

    fn push(&self) {
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

    fn activate(&mut self, id: TabId) {
        self.tabs.activate(id, now());
        self.dirty = true;
        self.layout();
        if let Some(v) = self.views.get(&id) {
            let _ = v.focus();
        }
    }

    fn new_tab(&mut self, url: &str, private: bool, after: Option<TabId>) {
        let id = self.tabs.open(url, private, after);
        self.activate(id);
        if url.is_empty() {
            self.push(); // the field must see the blank tab before it takes focus
            self.command("focus", &Value::Null);
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
        if let Some(t) = self.tabs.get(id) {
            if !t.private && !t.url.is_empty() {
                self.closed.push(t.url.clone());
            }
        }
        self.unload(id);
        self.blocked.remove(&id);
        self.tabs.close(id);
        self.activate(self.tabs.active);
    }

    fn cycle(&mut self, step: isize) {
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
        if open && !self.overlay && !self.active_blank() {
            self.capture_backdrop();
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
        self.js(&format!("L.results({seq},{})", json!({"bookmarks": marks, "history": hist})));
    }

    /// Appends to leirk.log: UI script errors and the commands that preceded them.
    fn log(&self, line: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("leirk.log")) {
            let _ = writeln!(f, "{} {line}", now());
        }
    }

    fn command(&mut self, c: &str, v: &Value) {
        if c != "query" && c != "layout" {
            self.log(&format!("cmd {c} {v}"));
        }
        let target = v.as_u64().unwrap_or(self.tabs.active);
        match c {
            "layout" => {
                self.top = v.as_f64().unwrap_or(80.0);
                self.layout();
            }
            "nav" => {
                let url = resolve(v.as_str().unwrap_or(""), &self.profile.engines, &self.profile.default_engine);
                self.navigate(url);
            }
            "open" => {
                let url = v.as_str().unwrap_or("").to_string();
                if self.active_blank() { self.navigate(url) } else { self.new_tab(&url, false, Some(self.tabs.active)) }
            }
            "newtab" => self.new_tab("", false, None),
            "private" => self.new_tab("", true, None),
            "close" => self.close(target),
            "reopen" => {
                if let Some(u) = self.closed.pop() {
                    self.new_tab(&u, false, None);
                }
            }
            "activate" => self.activate(target),
            "next" => self.cycle(1),
            "prev" => self.cycle(-1),
            "back" => self.with_active(|w| unsafe { w.GoBack() }),
            "forward" => self.with_active(|w| unsafe { w.GoForward() }),
            "reload" => self.with_active(|w| unsafe { w.Reload() }),
            "stop" => self.with_active(|w| unsafe { w.Stop() }),
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
                if let Some(i) = self.profile.shields_off.iter().position(|s| *s == site) {
                    self.profile.shields_off.remove(i);
                } else {
                    self.profile.shields_off.push(site);
                }
                if let Some(sh) = self.shields.borrow_mut().as_mut() {
                    sh.disabled_sites = self.profile.shields_off.iter().cloned().collect();
                }
                self.blocked.remove(&self.tabs.active);
                self.with_active(|w| unsafe { w.Reload() });
            }
            "bookmark" => {
                let Some(t) = self.tabs.get(self.tabs.active) else { return };
                if t.url.is_empty() {
                    return;
                }
                let (url, title) = (t.url.clone(), t.title.clone());
                match self.profile.bookmarks.iter().position(|b| b.url == url) {
                    Some(i) => {
                        self.profile.bookmarks.remove(i);
                    }
                    None => self.profile.bookmarks.push(Bookmark { title, url, folder: String::new() }),
                }
            }
            "overlay" => self.set_overlay(v.as_bool().unwrap_or(false)),
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
                    let r = import::import(s, &mut self.profile, &mut self.history);
                    total.bookmarks += r.bookmarks;
                    total.history += r.history;
                    total.engines += r.engines;
                }
                self.js(&format!("L.imported({})", json!(total)));
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
            "clearHistory" => {
                let _ = self.history.clear();
            }
            "quit" => {
                self.save();
                std::process::exit(0);
            }
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
        self.push();
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
                    self.cosmetic(id, &url);
                }
            }
            Ev::NewTab(url) => {
                let private = self.tabs.get(self.tabs.active).is_some_and(|t| t.private);
                self.new_tab(&url, private, Some(self.tabs.active));
            }
            Ev::Blocked(id) => {
                *self.blocked.entry(id).or_default() += 1;
                self.blocked_total += 1;
                if id != self.tabs.active {
                    return; // background counters need no repaint
                }
            }
            Ev::Favicon(id, f) => {
                if let Some(t) = self.tabs.get_mut(id) {
                    t.favicon = f;
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
            Ev::ShieldsReady(mut sh) => {
                sh.disabled_sites = self.profile.shields_off.iter().cloned().collect();
                *self.shields.borrow_mut() = Some(sh);
            }
        }
        self.push();
    }

    /// Cosmetic filtering: hide site-specific ad containers as the document starts.
    fn cosmetic(&self, id: TabId, url: &str) {
        let Some(v) = self.views.get(&id) else { return };
        let guard = self.shields.borrow();
        let Some(sh) = guard.as_ref() else { return };
        let sel = sh.hide_selectors(url);
        if sel.is_empty() {
            return;
        }
        let css = format!("{}{{display:none!important}}", sel.join(","));
        let _ = v.evaluate_script(&format!(
            "(()=>{{const s=document.createElement('style');s.textContent={};(document.head||document.documentElement).appendChild(s)}})()",
            json!(css)
        ));
    }
}

fn main() {
    let dir = store::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Leirk")
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(LogicalSize::new(520.0, 360.0))
        .build(&event_loop)
        .expect("window");

    let mut ctx = WebContext::new(Some(dir.join("WebView2")));
    let p = proxy.clone();
    let ui = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_html(include_str!("../ui/index.html"))
        .with_ipc_handler(move |req| {
            let _ = p.send_event(Ev::Ui(req.into_body()));
        })
        .with_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(1280.0, 820.0).into() })
        .build_as_child(&window)
        .expect("chrome webview");

    let mut tabs: Tabs = store::load_json(&dir.join("session.json"));
    if let Some(arg) = std::env::args().nth(1) {
        let url = if Path::new(&arg).exists() { format!("file:///{}", arg.replace('\\', "/")) } else { arg };
        tabs.active = tabs.open(&url, false, None);
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
        proxy: proxy.clone(),
        dir: dir.clone(),
        tabs,
        views: HashMap::new(),
        shown: HashSet::new(),
        profile: store::load_json(&dir.join("profile.json")),
        history,
        shields: Rc::new(RefCell::new(None)),
        blocked: HashMap::new(),
        blocked_total: stats["blocked"].as_u64().unwrap_or(0),
        closed: vec![],
        loading: HashSet::new(),
        top: 80.0,
        overlay: false,
        dirty: false,
    };
    unsafe {
        let _ = app.hook_keys(&app.ui.controller());
    }
    app.tabs.activate(app.tabs.active, now());
    let _ = app.ui.focus();
    load_shields(dir.join("lists"), proxy);

    event_loop.run(move |event, _, flow| match event {
        Event::NewEvents(StartCause::Init) => {
            *flow = ControlFlow::WaitUntil(Instant::now() + TICK);
            app.layout();
        }
        Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
            *flow = ControlFlow::WaitUntil(Instant::now() + TICK);
            app.enforce_budget();
            if app.dirty {
                app.save();
            }
            app.push();
        }
        Event::UserEvent(ev) => app.event(ev),
        Event::WindowEvent { event: WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }, .. } => {
            app.layout();
            app.push();
        }
        Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
            app.save();
            *flow = ControlFlow::Exit;
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn shortcuts() {
        assert_eq!(shortcut('T' as u32, true, false, false), Some("newtab"));
        assert_eq!(shortcut('T' as u32, true, true, false), Some("reopen"));
        assert_eq!(shortcut('3' as u32, true, false, false), Some("tab3"));
        assert_eq!(shortcut(0x71, false, false, false), Some("palette"));
        assert_eq!(shortcut('T' as u32, false, false, false), None);
    }
}
