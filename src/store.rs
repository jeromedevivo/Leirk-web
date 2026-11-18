//! Persistence: profile.json (settings, bookmarks, engines), session.json (tabs) and history.db (SQLite).
use crate::omnibox::{builtin_engines, Engine};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub folder: String,
    /// Last change (unix seconds), so sync can tell which edit is newest.
    #[serde(default)]
    pub ts: i64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Profile {
    pub bookmarks: Vec<Bookmark>,
    pub engines: Vec<Engine>,
    pub default_engine: String,
    /// Max tabs kept alive in memory (visible ones always are).
    pub max_live: usize,
    /// Background tabs are unloaded after this many minutes (0 = never).
    pub idle_minutes: u64,
    pub shields_off: Vec<String>,
    pub vertical_tabs: bool,
    /// Saved settings; read through [`merged_prefs`].
    pub prefs: serde_json::Value,
    pub prefs_ts: i64,
    /// Deleted bookmark URLs with the deletion time, for sync.
    pub deleted: Vec<(String, i64)>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            bookmarks: vec![],
            engines: builtin_engines(),
            default_engine: "br".into(),
            max_live: 4,
            idle_minutes: 15,
            shields_off: vec![],
            vertical_tabs: false,
            prefs: serde_json::json!({}),
            prefs_ts: 0,
            deleted: vec![],
        }
    }
}

/// Every setting's default; the UI renders from the merged result so both sides share one source.
pub const DEFAULTS: &str = include_str!("../ui/defaults.json");

/// Defaults overlaid with the saved prefs (nested objects merge key by key; unknown saved keys survive).
pub fn merged_prefs(saved: &serde_json::Value) -> serde_json::Value {
    fn merge(a: &mut serde_json::Value, b: &serde_json::Value) {
        let (Some(ao), Some(bo)) = (a.as_object_mut(), b.as_object()) else { return };
        for (k, v) in bo {
            match ao.get_mut(k) {
                Some(x) if x.is_object() && v.is_object() => merge(x, v),
                _ => {
                    ao.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let mut d: serde_json::Value = serde_json::from_str(DEFAULTS).expect("ui/defaults.json is valid JSON");
    merge(&mut d, saved);
    d
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Leirk")
}

pub fn load_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Writes via a temp file + rename so a crash never leaves a half-written profile.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(value)?)?;
    std::fs::rename(tmp, path)
}

impl Profile {
    /// Adds bookmarks not already present (by URL); returns how many were added.
    pub fn merge_bookmarks(&mut self, new: Vec<Bookmark>) -> usize {
        let before = self.bookmarks.len();
        for b in new {
            if !self.bookmarks.iter().any(|x| x.url == b.url) {
                self.bookmarks.push(b);
            }
        }
        self.bookmarks.len() - before
    }
    /// Adds engines whose keyword and URL are both new; returns how many were added.
    pub fn merge_engines(&mut self, new: Vec<Engine>) -> usize {
        let before = self.engines.len();
        for e in new {
            if !self.engines.iter().any(|x| x.keyword.eq_ignore_ascii_case(&e.keyword) || x.url == e.url) {
                self.engines.push(e);
            }
        }
        self.engines.len() - before
    }
}

pub struct History {
    db: Connection,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Entry {
    pub url: String,
    pub title: String,
}

impl History {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS h(url TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '',
               visits INTEGER NOT NULL DEFAULT 0, last INTEGER NOT NULL DEFAULT 0);
             CREATE INDEX IF NOT EXISTS h_rank ON h(visits DESC);",
        )?;
        Ok(History { db })
    }

    pub fn visit(&self, url: &str, title: &str, now: i64) -> rusqlite::Result<()> {
        self.db
            .execute(
                "INSERT INTO h(url,title,visits,last) VALUES(?1,?2,1,?3)
                 ON CONFLICT(url) DO UPDATE SET visits=visits+1, last=?3,
                   title=CASE WHEN ?2='' THEN title ELSE ?2 END",
                params![url, title, now],
            )
            .map(|_| ())
    }

    pub fn set_title(&self, url: &str, title: &str) -> rusqlite::Result<()> {
        self.db.execute("UPDATE h SET title=?2 WHERE url=?1 AND ?2<>''", params![url, title]).map(|_| ())
    }

    /// Bulk import (url, title, visits, last); keeps the larger visit count on conflict.
    pub fn import(&mut self, rows: &[(String, String, i64, i64)]) -> rusqlite::Result<usize> {
        let tx = self.db.transaction()?;
        {
            let mut st = tx.prepare(
                "INSERT INTO h(url,title,visits,last) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(url) DO UPDATE SET visits=max(visits,?3), last=max(last,?4),
                   title=CASE WHEN title='' THEN ?2 ELSE title END",
            )?;
            for (u, t, v, l) in rows {
                st.execute(params![u, t, v, l])?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// Entries whose URL or title contains every word of `q`, most visited first.
    pub fn search(&self, q: &str, limit: usize) -> Vec<Entry> {
        let words: Vec<String> = q
            .split_whitespace()
            .map(|w| format!("%{}%", w.replace('!', "!!").replace('%', "!%").replace('_', "!_")))
            .collect();
        let mut sql = String::from("SELECT url,title FROM h WHERE 1");
        for i in 0..words.len() {
            sql += &format!(" AND (url LIKE ?{0} ESCAPE '!' OR title LIKE ?{0} ESCAPE '!')", i + 1);
        }
        sql += &format!(" ORDER BY visits DESC, last DESC LIMIT {limit}");
        let Ok(mut st) = self.db.prepare(&sql) else { return vec![] };
        st.query_map(rusqlite::params_from_iter(words.iter()), |r| Ok(Entry { url: r.get(0)?, title: r.get(1)? }))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn top(&self, limit: usize) -> Vec<Entry> {
        self.search("", limit)
    }

    /// Forgets visits made at or after `since` (unix seconds).
    pub fn clear_since(&self, since: i64) -> rusqlite::Result<()> {
        self.db.execute("DELETE FROM h WHERE last >= ?1", params![since]).map(|_| ())
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.db.execute("DELETE FROM h", []).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> History {
        History::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn visits_rank_and_search() {
        let h = mem();
        h.visit("https://rust-lang.org/", "Rust", 1).unwrap();
        h.visit("https://example.com/", "Example", 2).unwrap();
        h.visit("https://rust-lang.org/", "Rust Programming", 3).unwrap();
        let top = h.top(10);
        assert_eq!(top[0].url, "https://rust-lang.org/");
        assert_eq!(top[0].title, "Rust Programming");
        assert_eq!(h.search("rust prog", 10).len(), 1);
        assert_eq!(h.search("EXAMPLE", 10).len(), 1);
        assert!(h.search("nothing", 10).is_empty());
        assert_eq!(h.search("100%_", 10).len(), 0, "LIKE wildcards are escaped");
    }

    #[test]
    fn empty_title_does_not_erase_known_title() {
        let h = mem();
        h.visit("https://a.com/", "A", 1).unwrap();
        h.visit("https://a.com/", "", 2).unwrap();
        assert_eq!(h.top(1)[0].title, "A");
    }

    #[test]
    fn import_merges() {
        let mut h = mem();
        h.visit("https://a.com/", "A", 1).unwrap();
        let n = h
            .import(&[("https://a.com/".into(), "A".into(), 9, 5), ("https://b.com/".into(), "B".into(), 1, 5)])
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(h.top(1)[0].url, "https://a.com/");
        assert_eq!(h.top(10).len(), 2);
    }

    #[test]
    fn prefs_merge_over_defaults() {
        let saved = serde_json::json!({"theme": "dark", "lists": {"social": true}, "custom_key": 1});
        let m = merged_prefs(&saved);
        assert_eq!(m["theme"], "dark");
        assert_eq!(m["zoom"], 100, "missing keys come from defaults");
        assert_eq!(m["lists"]["social"], true);
        assert_eq!(m["lists"]["default"], true, "nested defaults survive a partial save");
        assert_eq!(m["custom_key"], 1);
    }

    #[test]
    fn profile_merges_without_duplicates() {
        let mut p = Profile::default();
        let b = |u: &str| Bookmark { title: u.into(), url: u.into(), folder: String::new(), ts: 0 };
        assert_eq!(p.merge_bookmarks(vec![b("https://a/"), b("https://a/"), b("https://b/")]), 2);
        assert_eq!(p.merge_bookmarks(vec![b("https://a/")]), 0);
        let n = p.engines.len();
        let dup = p.engines[0].clone();
        let new = Engine { name: "X".into(), keyword: "xx".into(), url: "https://x/?q=%s".into() };
        assert_eq!(p.merge_engines(vec![dup, new]), 1);
        assert_eq!(p.engines.len(), n + 1);
    }
}
