//! Chromium's own DevTools front end, docked inside the window as in Chrome and Brave
//! (device mode next to the element picker, dock side menu, extension panels such as "Leirk").
//! The front end reports where the inspected page must sit; Leirk moves the tab's view there.
use super::devhost::cdp;
use super::*;
use tao::platform::windows::WindowExtWindows;

pub(crate) struct Inspector {
    pub tab: TabId,
    /// Stops the poller thread when the front end goes away.
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub view: WebView,
    /// Page area inside the front end (CSS px), as reported by `setInspectedPageBounds`.
    pub bounds: Option<(f64, f64, f64, f64)>,
    /// Own top-level window once the user picks "Undock into separate window".
    pub window: Option<Window>,
    /// The front end asked to undock; the event loop creates the window (it owns the window target).
    pub want_window: bool,
    /// Undocked, Chrome shows device mode in a "toolbox" page the front end opens with window.open;
    /// this view is handed to that call and placed over the tab, the page sitting where it says.
    pub toolbox: Option<WebView>,
    /// Holds the toolbox until the front end opens it; empty once it is in use.
    slot: Rc<RefCell<Option<ICoreWebView2>>>,
}

impl Drop for Inspector {
    fn drop(&mut self) {
        self.alive.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Runs in the front end: queues the embedder calls Chrome's browser process would handle.
/// devtools:// pages have no `chrome.webview`, so Leirk collects the queue while DevTools are open.
const HOST_HOOK: &str = r#"(() => {
  if (location.protocol !== "devtools:" || window.top !== window) return;
  const send = (c, v) => (window.__leirkOut ||= []).push({ c, v });
  const patch = () => {
    const h = window.InspectorFrontendHost;
    if (!h || h.__leirk) return !!h;
    h.__leirk = true;
    const bounds = h.setInspectedPageBounds?.bind(h);
    h.setInspectedPageBounds = r => { send("bounds", r); bounds && bounds(r); };
    const docked = h.setIsDocked?.bind(h);
    h.setIsDocked = (d, cb) => { send("docked", d); docked ? docked(d, cb) : cb && cb(); };
    h.closeWindow = () => send("close");
    return true;
  };
  if (!patch()) { const t = setInterval(() => patch() && clearInterval(t), 5); }
})();"#;

impl App {
    /// F12: dock DevTools on the active tab, or close them if they are already there.
    pub(crate) fn toggle_inspector(&mut self) {
        let id = self.tabs.active;
        if self.inspector.as_ref().is_some_and(|i| i.tab == id) {
            return self.close_inspector();
        }
        let Some(view) = self.views.get(&id) else { return };
        if self.debug_port.is_none() {
            return view.open_devtools(); // developer mode off: the separate WebView2 window
        }
        let proxy = self.proxy.clone();
        let Ok(core) = (unsafe { view.controller().CoreWebView2() }) else { return };
        cdp(&core, "Target.getTargetInfo", json!({}), move |r| {
            if let Some(target) = r.and_then(|r| r["targetInfo"]["targetId"].as_str().map(String::from)) {
                let _ = proxy.send_event(Ev::Inspect(id, target));
            }
        });
    }

    pub(crate) fn open_inspector(&mut self, id: TabId, target: String) {
        let Some(port) = self.debug_port else { return };
        if !target.chars().all(|c| c.is_ascii_alphanumeric()) {
            return;
        }
        self.inspector = None;
        let url = format!("devtools://devtools/bundled/devtools_app.html?ws=127.0.0.1:{port}/devtools/page/{target}&can_dock=true");
        let p = self.proxy.clone();
        let slot: Rc<RefCell<Option<ICoreWebView2>>> = Rc::default();
        let free = slot.clone();
        let keep = slot.clone();
        let built = WebViewBuilder::new_with_web_context(&mut self.ctx)
            .with_additional_browser_args(&self.args)
            .with_browser_extensions_enabled(true)
            .with_url(&url)
            .with_visible(false)
            .with_initialization_script(HOST_HOOK)
            .with_new_window_req_handler(move |u, _| match free.borrow_mut().take() {
                // device_mode_emulation_frame.html (toolbox.html in older Chromium)
                Some(webview) if ["device_mode_emulation_frame.html", "toolbox.html"].iter().any(|f| u.starts_with(&format!("devtools://devtools/bundled/{f}"))) => {
                    NewWindowResponse::Create { webview }
                }
                _ => NewWindowResponse::Deny,
            })
            .build_as_child(&self.window);
        if let Ok(view) = built {
            // same environment as the front end, never navigated: WebView2 requires both for window.open targets
            let env = unsafe { view.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_2>()).and_then(|c| c.Environment()) };
            let toolbox = env.ok().and_then(|env| {
                WebViewBuilder::new_with_web_context(&mut self.ctx)
                    .with_environment(env)
                    .with_visible(false)
                    .build_as_child(&self.window)
                    .ok()
            });
            if let Some(t) = &toolbox {
                *slot.borrow_mut() = unsafe { t.controller().CoreWebView2() }.ok();
            }
            let _ = unsafe { self.hook_keys(&view.controller()) }; // F12 inside DevTools closes them
            let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let flag = alive.clone();
            std::thread::spawn(move || {
                while flag.load(std::sync::atomic::Ordering::Relaxed) && p.send_event(Ev::InspectorPoll).is_ok() {
                    std::thread::sleep(Duration::from_millis(150));
                }
            });
            self.inspector = Some(Inspector { tab: id, alive, view, bounds: None, window: None, want_window: false, toolbox, slot: keep });
            self.layout();
        }
    }

    pub(crate) fn close_inspector(&mut self) {
        self.inspector = None;
        self.layout();
    }

    /// Collects the front end's queued calls.
    pub(crate) fn poll_inspector(&mut self) {
        let Some(i) = &self.inspector else { return };
        let p = self.proxy.clone();
        let _ = i.view.evaluate_script_with_callback("JSON.stringify((window.__leirkOut || []).splice(0))", move |out| {
            // the result arrives JSON-encoded twice (string of a JSON array)
            let list: Vec<Value> = serde_json::from_str::<String>(&out).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            for m in list {
                let _ = p.send_event(Ev::InspectorHost(m.to_string()));
            }
        });
    }

    /// Calls from the front end: page area, dock side, close button.
    pub(crate) fn inspector_host(&mut self, body: &str) {
        let Ok(m) = serde_json::from_str::<Value>(body) else { return };
        match m["c"].as_str().unwrap_or("") {
            "bounds" => {
                let f = |k: &str| m["v"][k].as_f64().unwrap_or(0.0).max(0.0);
                if let Some(i) = &mut self.inspector {
                    i.bounds = Some((f("x"), f("y"), f("width"), f("height")));
                }
                self.layout();
            }
            "close" => self.close_inspector(),
            "docked" => {
                let Some(i) = &mut self.inspector else { return };
                if m["v"] == false {
                    i.want_window = i.window.is_none();
                } else if let Some(w) = i.window.take() {
                    // a dock side picked from the separate window: back into Leirk
                    let _ = i.view.reparent(self.window.hwnd());
                    drop(w);
                    self.layout();
                }
            }
            _ => {}
        }
    }

    /// Moves the front end into its own window, as Chrome's "Undock into separate window".
    pub(crate) fn undock_inspector(&mut self, window: Window) {
        let Some(i) = &mut self.inspector else { return };
        i.want_window = false;
        let _ = i.view.reparent(window.hwnd());
        i.window = Some(window);
        if i.slot.borrow().is_some() {
            // opened already undocked: Chrome's front end only opens its device-mode frame on a dock change
            let _ = i.view.evaluate_script("window.open(location.href.replace('devtools_app.html', 'device_mode_emulation_frame.html'))");
        }
        self.fit_inspector_window();
        self.layout();
    }

    /// The separate window was resized: the front end fills it.
    pub(crate) fn fit_inspector_window(&self) {
        let Some((i, w)) = self.inspector.as_ref().and_then(|i| Some((i, i.window.as_ref()?))) else { return };
        let s = w.inner_size().to_logical::<f64>(w.scale_factor());
        let _ = i.view.set_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(s.width, s.height).into() });
        let _ = i.view.set_visible(true);
    }

    pub(crate) fn is_inspector_window(&self, id: tao::window::WindowId) -> bool {
        self.inspector.as_ref().and_then(|i| i.window.as_ref()).is_some_and(|w| w.id() == id)
    }

    /// Places the front end over the tab's rect and the page inside it; returns the page rect.
    pub(crate) fn layout_inspector(&mut self, id: TabId, r: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        let Some(i) = self.inspector.as_ref().filter(|i| i.tab == id) else { return r };
        if i.window.is_some() {
            // undocked: the device-mode frame covers the tab and the page sits where the front end says
            let (Some(t), Some((x, y, w, h)), true) = (&i.toolbox, i.bounds, i.slot.borrow().is_none()) else { return r };
            let _ = t.set_bounds(Rect { position: LogicalPosition::new(r.0, r.1).into(), size: LogicalSize::new(r.2, r.3).into() });
            let _ = t.set_visible(true);
            if !self.overlay || self.awaiting_backdrop {
                Self::raise(t.hwnd());
            }
            return (r.0 + x, r.1 + y, w.min(r.2 - x).max(1.0), h.min(r.3 - y).max(1.0));
        }
        if let Some(t) = &i.toolbox {
            let _ = t.set_visible(false);
        }
        let _ = i.view.set_bounds(Rect { position: LogicalPosition::new(r.0, r.1).into(), size: LogicalSize::new(r.2, r.3).into() });
        let _ = i.view.set_visible(true);
        if !self.overlay || self.awaiting_backdrop {
            Self::raise(i.view.hwnd());
        }
        match i.bounds {
            Some((x, y, w, h)) => (r.0 + x, r.1 + y, w.min(r.2 - x).max(1.0), h.min(r.3 - y).max(1.0)),
            None => (r.0, r.1, 1.0, 1.0), // until the front end has laid itself out
        }
    }

    /// Hides the front end when its tab is off screen and drops it when the tab is gone.
    pub(crate) fn park_inspector(&mut self, visible: &[TabId]) {
        let Some(i) = &self.inspector else { return };
        if self.tabs.get(i.tab).is_none() {
            self.inspector = None;
        } else if !visible.contains(&i.tab) && i.window.is_none() {
            let _ = i.view.set_visible(false);
        }
    }
}
