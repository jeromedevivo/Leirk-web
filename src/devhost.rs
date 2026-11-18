//! Developer mode (Docker, ports, network, JS errors, technologies, performance, storage, responsive),
//! per-tab memory for hover cards, and full-page capture. Everything here is off unless the user enables it.
use super::*;
use leirk::devtools::{parse_docker, parse_ports};
use leirk::tech::{Db, Signals};
use std::sync::Arc;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// Calls a Chrome DevTools Protocol method on a webview; `done` gets the parsed result (None on failure).
pub(crate) fn cdp(core: &ICoreWebView2, method: &str, params: Value, done: impl FnOnce(Option<Value>) + 'static) {
    unsafe {
        let _ = core.CallDevToolsProtocolMethod(
            &HSTRING::from(method),
            &HSTRING::from(params.to_string()),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, json| {
                done(if hr.is_ok() { serde_json::from_str(&json).ok() } else { None });
                Ok(())
            })),
        );
    }
}

/// Runs a page expression through CDP, awaiting promises, and returns its JSON value.
pub(crate) fn evaluate(core: &ICoreWebView2, expr: &str, done: impl FnOnce(Option<Value>) + 'static) {
    cdp(core, "Runtime.evaluate", json!({"expression": expr, "awaitPromise": true, "returnByValue": true}), move |r| {
        done(r.map(|r| r["result"]["value"].clone()))
    });
}

/// Output of a console command, run without a window.
pub(crate) fn run(cmd: &str, args: &[&str]) -> std::result::Result<String, String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new(cmd).args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    if out.status.success() { Ok(text(&out.stdout) + &text(&out.stderr)) } else { Err(text(&out.stderr)) }
}

/// Private bytes and total CPU time (100 ns units) of a process.
fn proc_usage(pid: u32) -> (u64, u64) {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (0, 0) };
        let mut pmc = PROCESS_MEMORY_COUNTERS_EX::default();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let mem = if K32GetProcessMemoryInfo(h, &mut pmc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, size).as_bool() { pmc.PrivateUsage as u64 } else { 0 };
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let ft = |f: FILETIME| (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64;
        let cpu = if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() { ft(k) + ft(u) } else { 0 };
        let _ = CloseHandle(h);
        (mem, cpu)
    }
}

/// Local date-time for file names (UTC; good enough to sort captures).
fn stamp(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let (z, rem) = (days + 719_468, secs % 86400);
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y}-{m:02}-{d:02}_{:02}{:02}{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

/// Decodes PNG chunks of equal width and stacks them into one PNG file.
fn stitch(chunks: &[Vec<u8>], path: &Path) -> std::result::Result<(u32, u32), String> {
    let mut width = 0;
    let mut rows: Vec<u8> = vec![];
    let mut height = 0;
    for c in chunks {
        let mut dec = png::Decoder::new(std::io::Cursor::new(c));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = dec.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        if width != 0 && info.width != width {
            return Err("largeurs différentes".into());
        }
        width = info.width;
        height += info.height;
        let px = &buf[..info.buffer_size()];
        match info.color_type {
            png::ColorType::Rgba => rows.extend_from_slice(px),
            png::ColorType::Rgb => px.chunks(3).for_each(|p| rows.extend_from_slice(&[p[0], p[1], p[2], 255])),
            other => return Err(format!("format {other:?}")),
        }
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().and_then(|mut w| w.write_image_data(&rows)).map_err(|e| e.to_string())?;
    Ok((width, height))
}

const TECH_BASE: &str = "https://raw.githubusercontent.com/enthec/webappanalyzer/main/src/";

impl Shared {
    /// A developer module is active (developer mode on and this module kept).
    pub(crate) fn dev(&self, module: &str) -> bool {
        let p = self.prefs.borrow();
        p["dev_mode"].as_bool().unwrap_or(false) && p["dev_modules"][module].as_bool().unwrap_or(true)
    }
}

impl App {
    fn dev_send(&self, kind: &str, data: Value) {
        self.js(&format!("L.dev({},{})", json!(kind), data));
    }

    fn active_core(&self) -> Option<ICoreWebView2> {
        self.views.get(&self.tabs.active).map(|v| v.webview())
    }

    /// Per-webview developer hooks: response log and JS errors (only when their module is on).
    pub(crate) unsafe fn dev_hook(&self, core: &ICoreWebView2, id: TabId) -> windows::core::Result<()> {
        let mut tok = Default::default();
        if let Ok(c2) = core.cast::<ICoreWebView2_2>() {
            let shared = self.shared.clone();
            c2.add_WebResourceResponseReceived(
                &WebResourceResponseReceivedEventHandler::create(Box::new(move |sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    let (net, tech) = (shared.dev("network"), shared.dev("tech"));
                    if !net && !tech {
                        return Ok(());
                    }
                    let url = pwstr(|p| args.Request()?.Uri(p));
                    let resp = args.Response()?;
                    let mut status = 0;
                    resp.StatusCode(&mut status)?;
                    let mut headers: Vec<(String, String)> = vec![];
                    if let Ok(it) = resp.Headers().and_then(|h| h.GetIterator()) {
                        let mut has = BOOL::default();
                        while it.HasCurrentHeader(&mut has).is_ok() && has.as_bool() {
                            let (mut n, mut v) = (PWSTR::null(), PWSTR::null());
                            if it.GetCurrentHeader(&mut n, &mut v).is_ok() {
                                headers.push((take_pwstr(n), take_pwstr(v)));
                            }
                            let _ = it.MoveNext(&mut has);
                        }
                    }
                    if tech && sender.is_some_and(|s| pwstr(|p| s.Source(p)) == url) {
                        shared.doc_headers.borrow_mut().insert(id, headers.clone());
                    }
                    if net {
                        let size = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<u64>().ok());
                        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                        if let Some(log) = shared.netlog.borrow_mut().get_mut(&id) {
                            if let Some(e) = log.iter_mut().rev().find(|e| e["u"] == url.as_str() && e["s"] == 0) {
                                e["s"] = json!(status);
                                e["z"] = json!(size);
                                e["d"] = json!(now_ms.saturating_sub(e["ts"].as_u64().unwrap_or(now_ms)));
                            }
                        }
                    }
                    Ok(())
                })),
                &mut tok,
            )?;
        }
        if self.shared.dev("errors") {
            self.watch_errors(core, id);
        }
        Ok(())
    }

    /// Records uncaught exceptions and console.error calls (CDP Runtime domain).
    pub(crate) fn watch_errors(&self, core: &ICoreWebView2, id: TabId) {
        cdp(core, "Runtime.enable", json!({}), |_| {});
        for event in ["Runtime.exceptionThrown", "Runtime.consoleAPICalled"] {
            let (shared, proxy) = (self.shared.clone(), self.proxy.clone());
            unsafe {
                let Ok(rx) = core.GetDevToolsProtocolEventReceiver(&HSTRING::from(event)) else { continue };
                let mut tok = Default::default();
                let _ = rx.add_DevToolsProtocolEventReceived(
                    &DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let Ok(v) = serde_json::from_str::<Value>(&pwstr(|p| args.ParameterObjectAsJson(p))) else { return Ok(()) };
                        let entry = if v["exceptionDetails"].is_object() {
                            let d = &v["exceptionDetails"];
                            json!({"msg": d["exception"]["description"].as_str().or(d["text"].as_str()).unwrap_or("Erreur"),
                                   "src": format!("{}:{}", d["url"].as_str().unwrap_or(""), d["lineNumber"].as_i64().unwrap_or(0) + 1)})
                        } else if v["type"] == "error" {
                            let msg: Vec<String> = v["args"].as_array().into_iter().flatten()
                                .map(|a| a["value"].as_str().map(String::from).or(a["description"].as_str().map(String::from)).unwrap_or_default())
                                .collect();
                            json!({"msg": msg.join(" "), "src": "console.error"})
                        } else {
                            return Ok(());
                        };
                        let mut errs = shared.errors.borrow_mut();
                        let list = errs.entry(id).or_default();
                        if list.len() < 200 {
                            list.push(entry);
                        }
                        let _ = proxy.send_event(Ev::Repaint);
                        Ok(())
                    })),
                    &mut tok,
                );
            }
        }
    }

    /// Private memory of each loaded tab: renderer processes mapped to tabs through their frames.
    pub(crate) fn tab_memory(&self, done: impl FnOnce(HashMap<TabId, u64>) + 'static) {
        let mut frames: HashMap<u32, TabId> = HashMap::new();
        for (id, v) in &self.views {
            unsafe {
                if let Ok(w20) = v.webview().cast::<ICoreWebView2_20>() {
                    let mut f = 0u32;
                    if w20.FrameId(&mut f).is_ok() {
                        frames.insert(f, *id);
                    }
                }
            }
        }
        let Ok(env13) = self.ui.environment().cast::<ICoreWebView2Environment13>() else { return done(HashMap::new()) };
        unsafe {
            let _ = env13.GetProcessExtendedInfos(&GetProcessExtendedInfosCompletedHandler::create(Box::new(move |hr, coll| {
                let mut out: HashMap<TabId, u64> = HashMap::new();
                if let (true, Some(coll)) = (hr.is_ok(), coll) {
                    let mut n = 0u32;
                    coll.Count(&mut n)?;
                    for i in 0..n {
                        let info = coll.GetValueAtIndex(i)?;
                        let p = info.ProcessInfo()?;
                        let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
                        p.Kind(&mut kind)?;
                        if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER {
                            continue;
                        }
                        let mut pid = 0i32;
                        p.ProcessId(&mut pid)?;
                        let mut tabs = HashSet::new();
                        // the iterator does not keep its collection alive: hold it for the whole loop
                        let frame_infos = info.AssociatedFrameInfos()?;
                        let it = frame_infos.GetIterator()?;
                        let mut has = BOOL::default();
                        while it.HasCurrent(&mut has).is_ok() && has.as_bool() {
                            // a tab's main frame carries the tab's frame id (cross-site iframes in other processes are not counted)
                            if let Ok(f2) = it.GetCurrent().and_then(|f| f.cast::<ICoreWebView2FrameInfo2>()) {
                                let mut fid = 0u32;
                                if f2.FrameId(&mut fid).is_ok() {
                                    if let Some(t) = frames.get(&fid) {
                                        tabs.insert(*t);
                                    }
                                }
                            }
                            let _ = it.MoveNext(&mut has);
                        }
                        let mem = proc_usage(pid as u32).0;
                        for t in tabs {
                            *out.entry(t).or_default() += mem;
                        }
                    }
                }
                done(out);
                Ok(())
            })));
        }
    }

    /// Thumbnail of a tab as it leaves the screen, for hover cards.
    pub(crate) fn capture_thumb(&self, id: TabId) {
        let Some(v) = self.views.get(&id) else { return };
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
                    let (len, ptr) = (GlobalSize(hg), GlobalLock(hg) as *const u8);
                    if !ptr.is_null() && len > 0 {
                        let _ = proxy.send_event(Ev::Thumb(id, b64(std::slice::from_raw_parts(ptr, len))));
                    }
                    let _ = GlobalUnlock(hg);
                    Ok(())
                })),
            );
        }
    }

    /// Full-page capture that defeats lazy loading and virtualised lists: scroll through the page, then make the
    /// viewport as tall as the document so every element counts as visible, and capture it in stitched slices.
    pub(crate) fn capture_full(&self) {
        let Some(core) = self.active_core() else { return self.js("toast('Ouvrez une page à capturer.')") };
        let host = host(&self.tabs.get(self.tabs.active).map(|t| t.url.clone()).unwrap_or_default()).replace('.', "-");
        let proxy = self.proxy.clone();
        self.js("toast('Capture en cours… la page défile pour tout charger.')");
        const PRELOAD: &str = "(async()=>{const d=document.scrollingElement||document.documentElement;const w=t=>new Promise(r=>setTimeout(r,t));let y=0;for(let i=0;i<120&&y<d.scrollHeight;i++){y+=innerHeight*0.85;scrollTo(0,y);await w(140)}scrollTo(0,0);await w(400);return 1})()";
        let c1 = core.clone();
        evaluate(&core, PRELOAD, move |_| {
            let c2 = c1.clone();
            cdp(&c1, "Page.getLayoutMetrics", json!({}), move |m| {
                let Some(m) = m else { return };
                let w = m["cssLayoutViewport"]["clientWidth"].as_f64().unwrap_or(1280.0).round();
                let h = m["cssContentSize"]["height"].as_f64().unwrap_or(1000.0).round().clamp(1.0, 30000.0);
                let c3 = c2.clone();
                cdp(&c2, "Emulation.setDeviceMetricsOverride", json!({"width": w, "height": h, "deviceScaleFactor": 0, "mobile": false}), move |_| {
                    // let the page re-lay out and lazy content appear with everything "in view"
                    let c4 = c3.clone();
                    evaluate(&c3, "new Promise(r=>setTimeout(()=>r(document.documentElement.scrollHeight),900))", move |nh| {
                        let h = nh.and_then(|v| v.as_f64()).unwrap_or(h).clamp(1.0, 30000.0);
                        let chunks = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
                        shoot(c4, w, h, 0.0, chunks, host, proxy);
                    });
                });
            });
        });

        fn shoot(core: ICoreWebView2, w: f64, h: f64, y: f64, chunks: Rc<RefCell<Vec<Vec<u8>>>>, host: String, proxy: EventLoopProxy<Ev>) {
            const SLICE: f64 = 4000.0;
            if y >= h {
                cdp(&core, "Emulation.clearDeviceMetricsOverride", json!({}), |_| {});
                let parts = std::mem::take(&mut *chunks.borrow_mut());
                std::thread::spawn(move || {
                    let dir = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Pictures").join("Leirk");
                    let _ = std::fs::create_dir_all(&dir);
                    let path = dir.join(format!("{host}_{}.png", stamp(now())));
                    let msg = match stitch(&parts, &path) {
                        Ok((pw, ph)) => json!({"ok": true, "path": path.display().to_string(), "w": pw, "h": ph}),
                        Err(e) => json!({"ok": false, "error": e}),
                    };
                    let _ = proxy.send_event(Ev::Dev("capture".into(), msg));
                });
                return;
            }
            let clip = json!({"x": 0, "y": y, "width": w, "height": (h - y).min(SLICE), "scale": 1});
            let c2 = core.clone();
            cdp(&core, "Page.captureScreenshot", json!({"format": "png", "captureBeyondViewport": true, "fromSurface": true, "clip": clip}), move |r| {
                match r.and_then(|r| r["data"].as_str().and_then(unb64)) {
                    Some(png) => chunks.borrow_mut().push(png),
                    None => {
                        cdp(&c2, "Emulation.clearDeviceMetricsOverride", json!({}), |_| {});
                        let _ = proxy.send_event(Ev::Dev("capture".into(), json!({"ok": false, "error": "capture refusée par la page"})));
                        return;
                    }
                }
                shoot(c2, w, h, y + SLICE, chunks, host, proxy);
            });
        }
    }

    /// Fingerprint database, downloaded once (refreshed weekly) and parsed off the UI thread.
    fn load_tech_db(&self) {
        let (dir, proxy) = (self.dir.join("tech"), self.proxy.clone());
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            let names: Vec<String> = std::iter::once("_".to_string()).chain(('a'..='z').map(|c| c.to_string())).collect();
            let texts: Vec<String> = std::thread::scope(|s| {
                let hs: Vec<_> = names.iter().map(|n| { let d = &dir; s.spawn(move || fetch(&format!("{TECH_BASE}technologies/{n}.json"), d)) }).collect();
                hs.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
            });
            let cats = fetch(&format!("{TECH_BASE}categories.json"), &dir).unwrap_or_default();
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            let _ = proxy.send_event(Ev::TechDb(Arc::new(Db::parse(&refs, &cats))));
        });
    }

    /// Collects the page's signals and runs detection in a worker thread.
    pub(crate) fn tech_detect(&mut self) {
        let Some(db) = self.tech_db.clone() else {
            self.tech_pending = true;
            self.dev_send("tech", json!({"loading": true}));
            return self.load_tech_db();
        };
        let Some(core) = self.active_core() else { return self.dev_send("tech", json!({"found": []})) };
        let url = self.tabs.get(self.tabs.active).map(|t| t.url.clone()).unwrap_or_default();
        let headers = self.shared.doc_headers.borrow().get(&self.tabs.active).cloned().unwrap_or_default();
        let expr = format!(
            "(()=>{{const K={},D={};const js=[];for(const k of K){{try{{let o=window;for(const p of k.split('.')){{if(o==null)throw 0;o=o[p]}}if(o!==undefined&&o!==null)js.push([k,(typeof o==='string'||typeof o==='number')?String(o):''])}}catch(e){{}}}}\
             const dom=[];for(const s of D){{try{{if(document.querySelector(s))dom.push(s)}}catch(e){{}}}}\
             return{{meta:[...document.querySelectorAll('meta')].map(m=>[m.getAttribute('name')||m.getAttribute('property')||'',m.getAttribute('content')||'']).filter(x=>x[0]),\
             scripts:[...document.scripts].map(s=>s.src).filter(Boolean),js,dom,cookies:document.cookie.split(';').map(c=>c.split('=')[0].trim()).filter(Boolean),\
             html:document.documentElement.outerHTML.slice(0,300000)}}}})()",
            json!(db.js_keys()),
            json!(db.dom_selectors())
        );
        let proxy = self.proxy.clone();
        evaluate(&core, &expr, move |v| {
            let v = v.unwrap_or_default();
            let pairs = |k: &str| -> Vec<(String, String)> {
                v[k].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str().unwrap_or("").to_string()))).collect()
            };
            let strings = |k: &str| -> Vec<String> { v[k].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect() };
            let s = Signals { url, headers, cookies: strings("cookies"), meta: pairs("meta"), scripts: strings("scripts"), js: pairs("js"), html: v["html"].as_str().unwrap_or("").to_string(), dom: strings("dom") };
            std::thread::spawn(move || {
                let _ = proxy.send_event(Ev::Dev("tech".into(), json!({"found": db.detect(&s), "known": db.len()})));
            });
        });
    }

    /// RAM and CPU of every loaded tab, plus page metrics and Web Vitals of the active one.
    fn perf(&mut self) {
        let proxy = self.proxy.clone();
        let titles: HashMap<TabId, String> = self.tabs.list.iter().map(|t| (t.id, if t.title.is_empty() { t.url.clone() } else { t.title.clone() })).collect();
        self.tab_memory(move |mem| {
            let mut rows: Vec<Value> = mem.iter().map(|(id, b)| json!({"id": id, "title": titles.get(id), "mb": b / 1_048_576})).collect();
            rows.sort_by_key(|r| std::cmp::Reverse(r["mb"].as_u64().unwrap_or(0)));
            let _ = proxy.send_event(Ev::Dev("perfTabs".into(), Value::Array(rows)));
        });
        let (mem, cpu) = proc_usage(std::process::id());
        let prev = self.cpu_prev.replace((cpu, Instant::now()));
        let cpu_pct = prev.map(|(c, t)| (cpu.saturating_sub(c)) as f64 / 1e4 / t.elapsed().as_millis().max(1) as f64 * 100.0).unwrap_or(0.0);
        self.dev_send("perfApp", json!({"mb": mem / 1_048_576, "cpu": (cpu_pct * 10.0).round() / 10.0}));
        let Some(core) = self.active_core() else { return };
        let proxy = self.proxy.clone();
        let c2 = core.clone();
        cdp(&core, "Performance.enable", json!({}), move |_| {
            cdp(&c2, "Performance.getMetrics", json!({}), move |m| {
                let metrics: serde_json::Map<String, Value> = m.and_then(|m| m["metrics"].as_array().cloned()).unwrap_or_default().into_iter()
                    .filter_map(|x| Some((x["name"].as_str()?.to_string(), x["value"].clone()))).collect();
                let _ = proxy.send_event(Ev::Dev("perfMetrics".into(), Value::Object(metrics)));
            });
        });
        const VITALS: &str = "(async()=>{const get=t=>new Promise(r=>{try{new PerformanceObserver((l,o)=>{o.disconnect();r(l.getEntries())}).observe({type:t,buffered:true});setTimeout(()=>r([]),300)}catch(e){r([])}});\
            const lcp=(await get('largest-contentful-paint')).pop();const cls=(await get('layout-shift')).filter(e=>!e.hadRecentInput).reduce((a,e)=>a+e.value,0);\
            const nav=performance.getEntriesByType('navigation')[0]||{};const fcp=performance.getEntriesByName('first-contentful-paint')[0];\
            return{lcp:lcp?Math.round(lcp.startTime):null,cls:Math.round(cls*1000)/1000,fcp:fcp?Math.round(fcp.startTime):null,ttfb:nav.responseStart?Math.round(nav.responseStart):null,\
            load:nav.loadEventEnd?Math.round(nav.loadEventEnd):null,nodes:document.getElementsByTagName('*').length,transfer:performance.getEntriesByType('resource').reduce((a,e)=>a+(e.transferSize||0),0)}})()";
        let proxy = self.proxy.clone();
        evaluate(&core, VITALS, move |v| {
            let _ = proxy.send_event(Ev::Dev("vitals".into(), v.unwrap_or_default()));
        });
    }

    /// Developer-mode commands from the chrome; returns false for anything else.
    pub(crate) fn dev_command(&mut self, c: &str, v: &Value) -> bool {
        let proxy = self.proxy.clone();
        let core = self.active_core();
        let url = self.tabs.get(self.tabs.active).map(|t| t.url.clone()).unwrap_or_default();
        match c {
            "devDocker" => {
                std::thread::spawn(move || {
                    let msg = match run("docker", &["ps", "-a", "--format", "{{json .}}"]) {
                        Ok(ps) => {
                            let stats = run("docker", &["stats", "--no-stream", "--format", "{{json .}}"]).unwrap_or_default();
                            json!({"containers": parse_docker(&ps, &stats)})
                        }
                        Err(e) => json!({"error": if e.is_empty() { "Docker n'est pas installé.".to_string() } else { e.lines().next().unwrap_or("").to_string() }}),
                    };
                    let _ = proxy.send_event(Ev::Dev("docker".into(), msg));
                });
            }
            "devDockerAct" => {
                let (id, act) = (v["id"].as_str().unwrap_or("").to_string(), v["act"].as_str().unwrap_or("").to_string());
                if ["start", "stop", "restart"].contains(&act.as_str()) && id.chars().all(|c| c.is_ascii_alphanumeric()) {
                    std::thread::spawn(move || {
                        let r = run("docker", &[&act, &id]);
                        let _ = proxy.send_event(Ev::Dev("dockerAct".into(), json!({"ok": r.is_ok(), "error": r.err()})));
                    });
                }
            }
            "devDockerLogs" => {
                let id = v.as_str().unwrap_or("").to_string();
                if id.chars().all(|c| c.is_ascii_alphanumeric()) {
                    std::thread::spawn(move || {
                        let text = run("docker", &["logs", "--tail", "300", "--timestamps", &id]).unwrap_or_else(|e| e);
                        let _ = proxy.send_event(Ev::Dev("logs".into(), json!({"id": id, "text": text})));
                    });
                }
            }
            "devPorts" => {
                std::thread::spawn(move || {
                    let ports = parse_ports(&run("netstat", &["-ano", "-p", "tcp"]).unwrap_or_default(), &run("tasklist", &["/fo", "csv", "/nh"]).unwrap_or_default());
                    let _ = proxy.send_event(Ev::Dev("ports".into(), json!(ports)));
                });
            }
            "devNet" => {
                let log: Vec<Value> = self.shared.netlog.borrow().get(&self.tabs.active).map(|l| l.iter().cloned().collect()).unwrap_or_default();
                self.dev_send("network", Value::Array(log));
            }
            "devNetClear" => drop(self.shared.netlog.borrow_mut().remove(&self.tabs.active)),
            "devReplay" => {
                if let Some(view) = self.views.get(&self.tabs.active) {
                    let js = format!("fetch({},{{method:{},credentials:'include'}}).then(r=>console.log('[Leirk] rejoué',r.status,r.url))", json!(v["u"]), json!(v["m"]));
                    let _ = view.evaluate_script(&js);
                }
            }
            "devErrors" => {
                let errs = self.shared.errors.borrow().get(&self.tabs.active).cloned().unwrap_or_default();
                self.dev_send("errors", Value::Array(errs));
            }
            "devErrorsClear" => drop(self.shared.errors.borrow_mut().remove(&self.tabs.active)),
            "devTech" => self.tech_detect(),
            "devPerf" => self.perf(),
            "devCookies" => {
                if let Some(core) = core {
                    cdp(&core, "Network.getCookies", json!({"urls": [url]}), move |r| {
                        let _ = proxy.send_event(Ev::Dev("cookies".into(), r.map(|r| r["cookies"].clone()).unwrap_or(json!([]))));
                    });
                }
            }
            "devCookieDel" => {
                if let Some(core) = core {
                    cdp(&core, "Network.deleteCookies", json!({"name": v["name"], "domain": v["domain"], "path": v["path"]}), |_| {});
                }
            }
            "devCookieSet" => {
                if let Some(core) = core {
                    cdp(&core, "Network.setCookie", json!({"name": v["name"], "value": v["value"], "domain": v["domain"], "path": v["path"], "url": url}), |_| {});
                }
            }
            "devStorage" => {
                if let Some(core) = core {
                    evaluate(&core, "(()=>{try{return{local:Object.entries(localStorage),session:Object.entries(sessionStorage)}}catch(e){return{local:[],session:[]}}})()", move |r| {
                        let _ = proxy.send_event(Ev::Dev("storage".into(), r.unwrap_or_default()));
                    });
                }
            }
            "devStorageSet" | "devStorageDel" => {
                if let Some(view) = self.views.get(&self.tabs.active) {
                    let area = if v["area"] == "session" { "sessionStorage" } else { "localStorage" };
                    let js = if c == "devStorageSet" { format!("{area}.setItem({},{})", json!(v["k"]), json!(v["v"])) } else { format!("{area}.removeItem({})", json!(v["k"])) };
                    let _ = view.evaluate_script(&js);
                }
            }
            "devDevice" => {
                if let Some(core) = core {
                    if v.is_null() {
                        cdp(&core, "Emulation.clearDeviceMetricsOverride", json!({}), |_| {});
                        cdp(&core, "Emulation.setTouchEmulationEnabled", json!({"enabled": false}), |_| {});
                    } else {
                        let mobile = v["mobile"].as_bool().unwrap_or(false);
                        cdp(&core, "Emulation.setDeviceMetricsOverride", json!({"width": v["w"], "height": v["h"], "deviceScaleFactor": v["dpr"], "mobile": mobile}), |_| {});
                        cdp(&core, "Emulation.setTouchEmulationEnabled", json!({"enabled": mobile, "maxTouchPoints": 5}), |_| {});
                    }
                }
            }
            "devThrottle" => {
                if let Some(core) = core {
                    let (off, lat, down, up) = match v.as_str().unwrap_or("") {
                        "offline" => (true, 0, 0, 0),
                        "3g" => (false, 300, 50_000, 50_000),
                        "4g" => (false, 70, 1_200_000, 600_000),
                        _ => (false, 0, -1, -1),
                    };
                    let c2 = core.clone();
                    cdp(&core, "Network.enable", json!({}), move |_| {
                        cdp(&c2, "Network.emulateNetworkConditions", json!({"offline": off, "latency": lat, "downloadThroughput": down, "uploadThroughput": up}), |_| {});
                    });
                }
            }
            "devErrorsWatch" => {
                for (id, view) in &self.views {
                    self.watch_errors(&view.webview(), *id);
                }
            }
            "capture" => self.capture_full(),
            "tabInfo" => {
                let id = v.as_u64().unwrap_or(0);
                let thumb = self.thumbs.get(&id).cloned();
                let loaded = self.views.contains_key(&id);
                if !loaded {
                    self.dev_send("tabInfo", json!({"id": id, "mb": 0, "thumb": thumb}));
                } else {
                    self.tab_memory(move |mem| {
                        let _ = proxy.send_event(Ev::Dev("tabInfo".into(), json!({"id": id, "mb": mem.get(&id).map(|b| b / 1_048_576), "thumb": thumb})));
                    });
                }
            }
            _ => return false,
        }
        true
    }
}
