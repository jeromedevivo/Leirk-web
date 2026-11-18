//! Sync through a cloud-synced folder: this PC writes `<folder>/Leirk/<device>.json` and merges the other devices' files.
use super::*;
use leirk::sync::{merge_bookmarks, newest_prefs, providers, SyncDoc};

/// Settings that describe this PC rather than the user; they never travel.
const LOCAL_ONLY: &[&str] = &["sync_folder", "download_dir", "gpu", "panels", "startup_pages"];

pub(crate) fn device_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

fn sync_dir(folder: &str) -> Option<PathBuf> {
    (!folder.is_empty() && Path::new(folder).is_dir()).then(|| Path::new(folder).join("Leirk"))
}

impl App {
    fn own_doc(&self) -> SyncDoc {
        let mut prefs = self.profile.prefs.clone();
        if let Some(o) = prefs.as_object_mut() {
            LOCAL_ONLY.iter().for_each(|k| drop(o.remove(*k)));
        }
        SyncDoc {
            device: device_name(),
            updated: now() as i64,
            bookmarks: self.profile.bookmarks.clone(),
            deleted: self.profile.deleted.clone(),
            prefs,
            prefs_ts: self.profile.prefs_ts,
            engines: self.profile.engines.clone(),
            tabs: self.tabs.list.iter().filter(|t| !t.private && !t.url.is_empty()).map(|t| (t.title.clone(), t.url.clone())).collect(),
        }
    }

    /// Writes this device's file, then merges every other device's into the profile.
    pub(crate) fn sync_now(&mut self) {
        let Some(dir) = sync_dir(&self.pref_str("sync_folder")) else { return };
        let _ = std::fs::create_dir_all(&dir);
        let me = device_name();
        let own = self.own_doc();
        let mut others: Vec<SyncDoc> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json") && p.file_stem().is_some_and(|s| s.to_string_lossy() != me))
            .filter_map(|p| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<SyncDoc>(&s).ok()))
            .collect();
        others.sort_by_key(|d| std::cmp::Reverse(d.updated));
        // a file in the cloud folder can be written by anyone with access to it: keep web addresses only
        for d in &mut others {
            d.bookmarks.retain(|b| is_web_url(&b.url));
            d.engines.retain(|e| is_web_url(&e.url) && e.url.contains("%s"));
            d.tabs.retain(|(_, u)| is_web_url(u));
        }

        let mut docs: Vec<&SyncDoc> = vec![&own];
        docs.extend(others.iter());
        let merged = merge_bookmarks(&docs);
        if merged != self.profile.bookmarks {
            self.profile.bookmarks = merged;
            self.bookmarks_dirty = true;
        }
        for d in &others {
            for (u, ts) in &d.deleted {
                if !self.profile.deleted.iter().any(|(x, t)| x == u && t >= ts) {
                    self.profile.deleted.retain(|(x, _)| x != u);
                    self.profile.deleted.push((u.clone(), *ts));
                }
            }
            for e in &d.engines {
                if !self.profile.engines.iter().any(|x| x.keyword.eq_ignore_ascii_case(&e.keyword)) {
                    self.profile.engines.push(e.clone());
                }
            }
        }
        // settings: the most recent edit on any device wins, local-only keys stay
        if let Some(remote) = newest_prefs(&others.iter().collect::<Vec<_>>()) {
            let remote_ts = others.iter().find(|d| std::ptr::eq(&d.prefs, remote)).map(|d| d.prefs_ts).unwrap_or(0);
            if remote_ts > self.profile.prefs_ts {
                let mut next = remote.clone();
                for k in LOCAL_ONLY {
                    if let Some(v) = self.profile.prefs.get(*k) {
                        next[*k] = v.clone();
                    }
                }
                let changed: Vec<String> = next.as_object().map(|o| o.iter().filter(|(k, v)| self.profile.prefs.get(*k) != Some(*v)).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                self.profile.prefs = next;
                self.profile.prefs_ts = remote_ts;
                *self.shared.prefs.borrow_mut() = merged_prefs(&self.profile.prefs);
                changed.iter().for_each(|k| self.apply_pref(k));
            }
        }
        self.remote_tabs = others.iter().flat_map(|d| d.tabs.iter().map(move |(t, u)| json!({"device": d.device, "title": t, "url": u}))).collect();
        self.sync_devices = others.iter().map(|d| json!({"device": d.device, "updated": d.updated})).collect();

        let own = self.own_doc();
        if let Ok(text) = serde_json::to_string(&own) {
            let path = dir.join(format!("{me}.json"));
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, path);
            }
        }
        self.last_sync = now();
        let _ = store::save_json(&self.dir.join("profile.json"), &self.profile);
    }

    pub(crate) fn sync_command(&mut self, c: &str, _v: &Value) -> bool {
        match c {
            "syncProviders" => {
                let home = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
                let onedrive = std::env::var_os("OneDrive").or(std::env::var_os("OneDriveConsumer")).map(PathBuf::from);
                let drives: Vec<PathBuf> = ('D'..='Z').map(|c| PathBuf::from(format!("{c}:\\"))).collect();
                let list: Vec<Value> = providers(&home, onedrive, &drives).into_iter().map(|(n, p)| json!({"name": n, "path": p.display().to_string()})).collect();
                self.js(&format!("L.syncProviders({})", Value::Array(list)));
            }
            "syncNow" => {
                self.sync_now();
                self.js("toast('Synchronisation terminée')");
            }
            _ => return false,
        }
        true
    }
}
