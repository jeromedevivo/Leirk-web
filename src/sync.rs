//! Sync through a cloud-synced folder (OneDrive, Google Drive, Dropbox, iCloud, Nextcloud, any folder):
//! each device writes its own file there and merges the others'. No server, no account.
use crate::omnibox::Engine;
use crate::store::Bookmark;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct SyncDoc {
    pub device: String,
    pub updated: i64,
    pub bookmarks: Vec<Bookmark>,
    /// Deleted bookmark URLs with the deletion time, so a removal reaches every device.
    pub deleted: Vec<(String, i64)>,
    pub prefs: Value,
    pub prefs_ts: i64,
    pub engines: Vec<Engine>,
    /// Open tabs (title, url), shown as "tabs from my other devices".
    pub tabs: Vec<(String, String)>,
}

/// Union of every device's bookmarks: for each URL the most recent change wins, deletions included.
pub fn merge_bookmarks(docs: &[&SyncDoc]) -> Vec<Bookmark> {
    todo!()
}

/// The prefs most recently changed on any device.
pub fn newest_prefs<'a>(docs: &[&'a SyncDoc]) -> Option<&'a Value> {
    todo!()
}

/// Cloud folders found on this PC: (provider, folder). `home`/`local` stand for USERPROFILE/LOCALAPPDATA.
pub fn providers(home: &Path, onedrive: Option<PathBuf>, drives: &[PathBuf]) -> Vec<(String, PathBuf)> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(url: &str, title: &str, ts: i64) -> Bookmark {
        Bookmark { title: title.into(), url: url.into(), folder: String::new(), ts }
    }

    #[test]
    fn latest_change_wins_and_deletions_propagate() {
        let a = SyncDoc { bookmarks: vec![bm("https://a/", "A old", 10), bm("https://b/", "B", 10)], ..Default::default() };
        let b = SyncDoc { bookmarks: vec![bm("https://a/", "A new", 20), bm("https://c/", "C", 5)], deleted: vec![("https://b/".into(), 15)], ..Default::default() };
        let c = SyncDoc { bookmarks: vec![bm("https://c/", "C", 30)], deleted: vec![("https://c/".into(), 1)], ..Default::default() };
        let m = merge_bookmarks(&[&a, &b, &c]);
        let titles: Vec<&str> = m.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, vec!["A new", "C"], "b deleted after its last edit; c re-added after an old deletion");
    }

    #[test]
    fn newest_prefs_by_timestamp() {
        let a = SyncDoc { prefs: serde_json::json!({"theme": "dark"}), prefs_ts: 5, ..Default::default() };
        let b = SyncDoc { prefs: serde_json::json!({"theme": "light"}), prefs_ts: 9, ..Default::default() };
        assert_eq!(newest_prefs(&[&a, &b]).unwrap()["theme"], "light");
        assert!(newest_prefs(&[]).is_none());
    }

    #[test]
    fn finds_cloud_folders() {
        let root = std::env::temp_dir().join(format!("leirk-sync-test-{}", std::process::id()));
        let home = root.join("home");
        let gdrive = root.join("G");
        for d in [home.join("Dropbox"), home.join("iCloudDrive"), gdrive.join("Mon Drive"), root.join("OneDrive")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let found = providers(&home, Some(root.join("OneDrive")), &[gdrive.clone(), root.join("missing")]);
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["OneDrive", "Google Drive", "Dropbox", "iCloud Drive"]);
        assert_eq!(found[1].1, gdrive.join("Mon Drive"));
        let _ = std::fs::remove_dir_all(root);
    }
}
