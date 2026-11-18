//! Side panels: web apps (Teams, Spotify, WhatsApp…) in their own webview, or an installed Windows app's own
//! window docked into Leirk (experimental: Windows does not officially support hosting another app's window).
use super::*;
use tao::platform::windows::WindowExtWindows;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetParent,
    SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, GWL_STYLE, GW_OWNER, SWP_FRAMECHANGED, SWP_NOZORDER, SW_HIDE, SW_SHOW, WS_CAPTION, WS_CHILD,
    WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

/// A foreign window hosted in the panel, with what is needed to give it back untouched.
pub(crate) struct Docked {
    pub hwnd: isize,
    style: isize,
    ex: isize,
    rect: RECT,
}

fn exe_of(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(h);
        if ok { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

/// Visible top-level windows of other apps: (hwnd, title, exe path).
pub(crate) fn app_windows() -> Vec<(isize, String, String)> {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lp.0 as *mut Vec<(isize, String, String)>);
        if IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).is_err() && GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize == 0 {
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut buf);
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if n > 0 && pid != std::process::id() {
                out.push((hwnd.0 as isize, String::from_utf16_lossy(&buf[..n as usize]), exe_of(pid)));
            }
        }
        true.into()
    }
    let mut out: Vec<(isize, String, String)> = vec![];
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize));
    }
    out.retain(|(_, t, e)| !e.is_empty() && t != "Program Manager" && !e.to_lowercase().ends_with("explorer.exe"));
    out
}

fn hwnd(h: isize) -> HWND {
    HWND(h as *mut std::ffi::c_void)
}

impl App {
    fn panel(&self, id: &str) -> Option<Value> {
        self.pref("panels").as_array()?.iter().find(|p| p["id"] == id).cloned()
    }

    /// Places the open panel (web view or docked window) on the side rect the chrome reports; hides the others.
    pub(crate) fn layout_panel(&mut self) {
        let (x, y, w, h) = self.side;
        let open = self.panel_open.clone().unwrap_or_default();
        for (id, v) in &self.panel_views {
            let on = *id == open && w > 0.0;
            let _ = v.set_visible(on);
            if on {
                let _ = v.set_bounds(Rect { position: LogicalPosition::new(x, y).into(), size: LogicalSize::new(w, h).into() });
                if !self.overlay || self.awaiting_backdrop {
                    Self::raise(v.hwnd());
                }
            }
        }
        let sf = self.window.scale_factor();
        for (id, d) in &self.docked {
            unsafe {
                if *id == open && w > 0.0 && IsWindow(Some(hwnd(d.hwnd))).as_bool() {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        hwnd(d.hwnd), None, (x * sf) as i32, (y * sf) as i32, (w * sf) as i32, (h * sf) as i32, SWP_NOZORDER | SWP_FRAMECHANGED,
                    );
                    let _ = ShowWindow(hwnd(d.hwnd), SW_SHOW);
                } else {
                    let _ = ShowWindow(hwnd(d.hwnd), SW_HIDE);
                }
            }
        }
    }

    fn open_web_panel(&mut self, id: &str, url: &str) {
        if !self.panel_views.contains_key(id) {
            let p = self.proxy.clone();
            let built = WebViewBuilder::new_with_web_context(&mut self.ctx)
                .with_additional_browser_args(&self.args)
                .with_url(url)
                .with_visible(false)
                .with_new_window_req_handler(move |u, _| {
                    let _ = p.send_event(Ev::NewTab(u));
                    NewWindowResponse::Deny
                })
                .build_as_child(&self.window);
            let Ok(view) = built else { return };
            unsafe {
                let _ = self.hook_keys(&view.controller());
            }
            self.panel_views.insert(id.to_string(), view);
        }
    }

    /// Docks the first window of the panel's app; starts the app when it is not running.
    fn dock_app(&mut self, id: &str, exe: &str) -> bool {
        if self.docked.contains_key(id) {
            return true;
        }
        let Some((h, _, _)) = app_windows().into_iter().find(|(_, _, e)| e.eq_ignore_ascii_case(exe)) else {
            let _ = std::process::Command::new(exe).spawn();
            self.js("toast(\"L'application démarre : cliquez à nouveau sur son icône dans quelques secondes.\")");
            return false;
        };
        unsafe {
            let w = hwnd(h);
            let style = GetWindowLongPtrW(w, GWL_STYLE);
            let ex = GetWindowLongPtrW(w, GWL_EXSTYLE);
            let mut rect = RECT::default();
            let _ = GetWindowRect(w, &mut rect);
            let strip = (WS_POPUP | WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU).0 as isize;
            SetWindowLongPtrW(w, GWL_STYLE, (style & !strip) | WS_CHILD.0 as isize);
            let _ = SetParent(w, Some(hwnd(self.window.hwnd() as isize)));
            self.docked.insert(id.to_string(), Docked { hwnd: h, style, ex, rect });
        }
        true
    }

    /// Gives a docked window back to the desktop exactly as it was.
    fn undock(&mut self, id: &str) {
        let Some(d) = self.docked.remove(id) else { return };
        unsafe {
            let w = hwnd(d.hwnd);
            if !IsWindow(Some(w)).as_bool() {
                return;
            }
            let _ = SetParent(w, None);
            SetWindowLongPtrW(w, GWL_STYLE, d.style);
            SetWindowLongPtrW(w, GWL_EXSTYLE, d.ex);
            let r = d.rect;
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(w, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_FRAMECHANGED);
            let _ = ShowWindow(w, SW_SHOW);
        }
    }

    /// Must run before Leirk's window goes away: child windows die with their parent.
    pub(crate) fn undock_all(&mut self) {
        let ids: Vec<String> = self.docked.keys().cloned().collect();
        ids.iter().for_each(|id| self.undock(id));
    }

    pub(crate) fn panel_command(&mut self, c: &str, v: &Value) -> bool {
        match c {
            "panelOpen" => {
                let id = v.as_str().unwrap_or("").to_string();
                if id == "dev" {
                    self.panel_open = Some(id);
                } else if let Some(p) = self.panel(&id) {
                    let ok = if p["kind"] == "app" {
                        self.dock_app(&id, p["exe"].as_str().unwrap_or(""))
                    } else {
                        self.open_web_panel(&id, p["url"].as_str().unwrap_or("about:blank"));
                        true
                    };
                    if ok {
                        self.panel_open = Some(id);
                    }
                }
                self.layout();
            }
            "panelClose" => {
                self.panel_open = None;
                self.layout();
            }
            "panelUnload" => {
                let id = v.as_str().unwrap_or("");
                self.panel_views.remove(id);
                self.undock(id);
                if self.panel_open.as_deref() == Some(id) {
                    self.panel_open = None;
                }
                self.layout();
            }
            "panelReload" => {
                if let Some(view) = self.panel_open.as_ref().and_then(|id| self.panel_views.get(id)) {
                    let _ = view.evaluate_script("location.reload()");
                }
            }
            "panelApps" => {
                let list: Vec<Value> = app_windows().into_iter().map(|(_, t, e)| json!({"title": t, "exe": e})).collect();
                self.js(&format!("L.panelApps({})", Value::Array(list)));
            }
            _ => return false,
        }
        true
    }
}
