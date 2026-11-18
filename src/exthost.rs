//! Extension manager: install from the Chrome Web Store, list, remove. WebView2 runs the extensions;
//! their popups open in the side panel since WebView2 has no extension toolbar of its own.
use super::*;
use leirk::extensions::{crx_url, crx_zip, read_info, store_id};

/// Folder names Leirk owns under the extensions root (not user extensions).
const OWN: &[&str] = &["leirk-devtools"];

impl App {
    fn ext_root(&self) -> PathBuf {
        self.dir.join("extensions")
    }

    fn profile7(&self) -> Option<ICoreWebView2Profile7> {
        unsafe { self.ui.webview().cast::<ICoreWebView2_13>().ok()?.Profile().ok()?.cast().ok() }
    }

    /// Installed extensions, joined with the ids WebView2 gave them (needed for chrome-extension:// popups).
    pub(crate) fn ext_list(&self) {
        let root = self.ext_root();
        let folders: Vec<(String, leirk::extensions::Info)> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir() && !OWN.contains(&e.file_name().to_string_lossy().as_ref()))
            .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), read_info(&e.path())?)))
            .collect();
        let Some(profile) = self.profile7() else { return };
        let proxy = self.proxy.clone();
        unsafe {
            let _ = profile.GetBrowserExtensions(&ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, list| {
                let mut live: Vec<(String, String)> = vec![];
                if let Some(list) = list {
                    let mut n = 0;
                    let _ = list.Count(&mut n);
                    for i in 0..n {
                        if let Ok(e) = list.GetValueAtIndex(i) {
                            live.push((pwstr(|p| e.Id(p)), pwstr(|p| e.Name(p))));
                        }
                    }
                }
                let items: Vec<Value> = folders
                    .iter()
                    .map(|(dir, info)| {
                        let id = live.iter().find(|(_, name)| name.eq_ignore_ascii_case(&info.name)).map(|(id, _)| id.clone()).unwrap_or_default();
                        let icon = std::fs::read(root.join(dir).join(&info.icon)).ok().filter(|b| b.starts_with(b"\x89PNG")).map(|b| format!("data:image/png;base64,{}", b64(&b)));
                        json!({"dir": dir, "id": id, "name": info.name, "version": info.version, "popup": info.popup, "icon": icon})
                    })
                    .collect();
                let _ = proxy.send_event(Ev::Exts(Value::Array(items)));
                Ok(())
            })));
        }
    }

    /// Loads an unpacked extension now (no restart) into the default profile.
    pub(crate) fn ext_load(&self, path: PathBuf) {
        let Some(profile) = self.profile7() else { return };
        let proxy = self.proxy.clone();
        unsafe {
            let _ = profile.AddBrowserExtension(
                &HSTRING::from(path.as_os_str()),
                &ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |hr, ext| {
                    let name = ext.map(|e| pwstr(|p| e.Name(p))).unwrap_or_default();
                    let msg = if hr.is_ok() { format!("{name} installée") } else { "L'extension n'a pas pu être chargée par WebView2.".into() };
                    let _ = proxy.send_event(Ev::ExtDone(msg));
                    Ok(())
                })),
            );
        }
    }

    pub(crate) fn ext_command(&mut self, c: &str, v: &Value) -> bool {
        match c {
            "extList" => self.ext_list(),
            "extInstall" => {
                let Some(id) = v.as_str().and_then(store_id) else {
                    self.js("toast('Collez le lien d\\'une extension du Chrome Web Store.')");
                    return true;
                };
                self.js("toast('Téléchargement de l\\'extension…')");
                let (root, proxy) = (self.ext_root(), self.proxy.clone());
                std::thread::spawn(move || {
                    let r = install(&root, &id);
                    let _ = proxy.send_event(match r {
                        Ok(path) => Ev::ExtFetched(path),
                        Err(e) => Ev::ExtDone(e),
                    });
                });
            }
            "extRemove" => {
                let (id, dir) = (v["id"].as_str().unwrap_or("").to_string(), v["dir"].as_str().unwrap_or("").to_string());
                // a direct child folder of the extensions root, never Leirk's own
                if dir.is_empty() || OWN.contains(&dir.as_str()) || !dir.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return true;
                }
                let path = self.ext_root().join(&dir);
                let proxy = self.proxy.clone();
                let Some(profile) = self.profile7() else { return true };
                unsafe {
                    let _ = profile.GetBrowserExtensions(&ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |_, list| {
                        let mut n = 0;
                        if let Some(list) = &list {
                            let _ = list.Count(&mut n);
                            for i in 0..n {
                                let Ok(e) = list.GetValueAtIndex(i) else { continue };
                                if pwstr(|p| e.Id(p)) == id {
                                    let _ = e.Remove(&BrowserExtensionRemoveCompletedHandler::create(Box::new(|_| Ok(()))));
                                }
                            }
                        }
                        let _ = std::fs::remove_dir_all(&path);
                        let _ = proxy.send_event(Ev::ExtDone("Extension retirée".into()));
                        Ok(())
                    })));
                }
            }
            _ => return false,
        }
        true
    }
}

/// Downloads the store's CRX and unpacks it to `<root>/<id>`.
fn install(root: &Path, id: &str) -> std::result::Result<PathBuf, String> {
    use std::os::windows::process::CommandExt;
    let tmp = std::env::temp_dir().join(format!("leirk-{id}.crx"));
    let ok = std::process::Command::new(system_tool("curl.exe"))
        .args(["-sfL", "--proto", "=https", "--proto-redir", "=https", "--max-time", "120", "-o"])
        .arg(&tmp)
        .arg(crx_url(id, &wry::webview_version().unwrap_or_default()))
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|s| s.success());
    let bytes = std::fs::read(&tmp).ok().filter(|_| ok).ok_or("Téléchargement impossible (extension introuvable ou hors ligne).")?;
    let _ = std::fs::remove_file(&tmp);
    let zip = crx_zip(&bytes).ok_or("Le fichier reçu n'est pas une extension Chrome.")?;
    let zip_path = std::env::temp_dir().join(format!("leirk-{id}.zip"));
    std::fs::write(&zip_path, zip).map_err(|e| e.to_string())?;
    let dest = root.join(id);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let unpacked = std::process::Command::new(system_tool("tar.exe"))
        .arg("-xf")
        .arg(&zip_path)
        .arg("-C")
        .arg(&dest)
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(&zip_path);
    // the store's signature lives in the CRX header; unpacked, Chrome would reject a stale metadata folder
    let _ = std::fs::remove_dir_all(dest.join("_metadata"));
    if !unpacked || !dest.join("manifest.json").is_file() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err("Archive d'extension illisible.".into());
    }
    Ok(dest)
}
