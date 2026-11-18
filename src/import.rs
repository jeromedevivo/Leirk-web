//! Import bookmarks, history and search engines from every installed Chromium or Firefox browser.
use crate::omnibox::Engine;
use crate::store::{Bookmark, History, Profile};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub enum Kind {
    Chromium,
    Firefox,
}

#[derive(Serialize, Clone, Debug)]
pub struct Source {
    pub name: String,
    pub kind: Kind,
    /// Profile directory (holds Bookmarks/History, or places.sqlite).
    pub dir: PathBuf,
}

#[derive(Serialize, Debug, Default)]
pub struct Report {
    pub bookmarks: usize,
    pub history: usize,
    pub engines: usize,
}

/// Flattens Chromium's `Bookmarks` JSON; `folder` is the slash-joined path below the root.
pub fn parse_chromium_bookmarks(json: &str) -> Vec<Bookmark> {
    todo!()
}

/// Chromium keyword URL -> `%s` template. Google-internal `{google:...}` templates are rejected.
pub fn chromium_search_url(u: &str) -> Option<String> {
    todo!()
}

/// Chromium stores times as microseconds since 1601-01-01; Firefox as microseconds since 1970.
pub fn chromium_time_to_unix(t: i64) -> i64 {
    t / 1_000_000 - 11_644_473_600
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOKMARKS: &str = r#"{"roots":{
      "bookmark_bar":{"type":"folder","name":"Barre","children":[
        {"type":"url","name":"Rust","url":"https://rust-lang.org/"},
        {"type":"folder","name":"Dev","children":[{"type":"url","name":"GH","url":"https://github.com/"}]}]},
      "other":{"type":"folder","name":"Autres","children":[{"type":"url","name":"JS","url":"javascript:alert(1)"}]},
      "synced":{"type":"folder","name":"Mobile","children":[]}}}"#;

    #[test]
    fn flattens_bookmarks_with_folders_and_skips_scripts() {
        let b = parse_chromium_bookmarks(BOOKMARKS);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], Bookmark { title: "Rust".into(), url: "https://rust-lang.org/".into(), folder: "Barre".into() });
        assert_eq!(b[1].folder, "Barre/Dev");
        assert!(parse_chromium_bookmarks("not json").is_empty());
    }

    #[test]
    fn converts_search_templates() {
        assert_eq!(chromium_search_url("https://x.com/?q={searchTerms}").as_deref(), Some("https://x.com/?q=%s"));
        assert_eq!(chromium_search_url("{google:baseURL}search?q={searchTerms}"), None);
        assert_eq!(chromium_search_url("https://x.com/?q={searchTerms}&{google:RLZ}"), None);
        assert_eq!(chromium_search_url("https://x.com/home"), None);
    }

    #[test]
    fn time_conversion() {
        assert_eq!(chromium_time_to_unix(11_644_473_600 * 1_000_000), 0);
    }
}
