//! Persistence: profile.json (settings, bookmarks, engines), session.json (tabs) and history.db (SQLite).
use crate::omnibox::{builtin_engines, Engine};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub folder: String,
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
        }
    }
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
        todo!()
    }
    /// Adds engines whose keyword and URL are both new; returns how many were added.
    pub fn merge_engines(&mut self, new: Vec<Engine>) -> usize {
        todo!()
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
        todo!()
    }

    /// Bulk import (url, title, visits, last); keeps the larger visit count on conflict.
    pub fn import(&mut self, rows: &[(String, String, i64, i64)]) -> rusqlite::Result<usize> {
        todo!()
    }

    /// Entries whose URL or title contains every word of `q`, most visited first.
    pub fn search(&self, q: &str, limit: usize) -> Vec<Entry> {
        todo!()
    }

    pub fn top(&self, limit: usize) -> Vec<Entry> {
        self.search("", limit)
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
    fn profile_merges_without_duplicates() {
        let mut p = Profile::default();
        let b = |u: &str| Bookmark { title: u.into(), url: u.into(), folder: String::new() };
        assert_eq!(p.merge_bookmarks(vec![b("https://a/"), b("https://a/"), b("https://b/")]), 2);
        assert_eq!(p.merge_bookmarks(vec![b("https://a/")]), 0);
        let n = p.engines.len();
        let dup = p.engines[0].clone();
        let new = Engine { name: "X".into(), keyword: "xx".into(), url: "https://x/?q=%s".into() };
        assert_eq!(p.merge_engines(vec![dup, new]), 1);
        assert_eq!(p.engines.len(), n + 1);
    }
}
