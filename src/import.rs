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

/// Flattens Chromium's `Bookmarks` JSON. Bookmarks-bar items get folder paths relative to the bar
/// ("" = on the bar itself); other roots are prefixed with their own name ("Autres favoris/...").
pub fn parse_chromium_bookmarks(json: &str) -> Vec<Bookmark> {
    fn walk(node: &serde_json::Value, folder: &str, out: &mut Vec<Bookmark>) {
        let name = node["name"].as_str().unwrap_or("");
        match node["type"].as_str() {
            Some("url") => {
                let url = node["url"].as_str().unwrap_or("");
                if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
                    out.push(Bookmark { title: name.into(), url: url.into(), folder: folder.into() });
                }
            }
            Some("folder") => {
                let path = if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
                for c in node["children"].as_array().into_iter().flatten() {
                    walk(c, &path, out);
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        todo!()
    }
    out
}

/// Chromium keyword URL -> `%s` template. Google-internal `{google:...}` templates are rejected.
pub fn chromium_search_url(u: &str) -> Option<String> {
    if !u.contains("{searchTerms}") || u.contains("{google:") || !u.starts_with("http") {
        return None;
    }
    Some(u.replace("{searchTerms}", "%s"))
}

/// Every Chromium profile and Firefox-family profile found on this machine.
pub fn sources() -> Vec<Source> {
    let env = |k| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
    let (local, roaming) = (env("LOCALAPPDATA"), env("APPDATA"));
    let mut out = vec![];
    let chromium = [
        ("Brave", local.join(r"BraveSoftware\Brave-Browser\User Data")),
        ("Chrome", local.join(r"Google\Chrome\User Data")),
        ("Edge", local.join(r"Microsoft\Edge\User Data")),
        ("Vivaldi", local.join(r"Vivaldi\User Data")),
        ("Chromium", local.join(r"Chromium\User Data")),
        ("Opera", roaming.join(r"Opera Software\Opera Stable")),
        ("Opera GX", roaming.join(r"Opera Software\Opera GX Stable")),
        ("Yandex", local.join(r"Yandex\YandexBrowser\User Data")),
    ];
    for (name, root) in chromium {
        // Opera keeps a single profile in its root; the others have Default, Profile 1, ...
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("History").exists() || p.join("Bookmarks").exists())
            .collect();
        if root.join("History").exists() {
            dirs.push(root.clone());
        }
        dirs.sort();
        for d in dirs {
            let prof = d.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let label = if d == root || prof == "Default" { name.to_string() } else { format!("{name} ({prof})") };
            out.push(Source { name: label, kind: Kind::Chromium, dir: d });
        }
    }
    let firefox = [
        ("Firefox", roaming.join(r"Mozilla\Firefox\Profiles")),
        ("Zen", roaming.join(r"zen\Profiles")),
        ("LibreWolf", roaming.join(r"librewolf\Profiles")),
        ("Floorp", roaming.join(r"Floorp\Profiles")),
        ("Waterfox", roaming.join(r"Waterfox\Profiles")),
    ];
    for (name, root) in firefox {
        for d in std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()) {
            if d.join("places.sqlite").exists() {
                let prof = d.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(Source { name: format!("{name} ({prof})"), kind: Kind::Firefox, dir: d });
            }
        }
    }
    out
}

/// Opens a copy of a browser database: the browser may hold a lock on the original while it runs.
fn open_copy(src: &Path) -> Option<Connection> {
    let tmp = std::env::temp_dir().join(format!("leirk-import-{}.sqlite", std::process::id()));
    std::fs::copy(src, &tmp).ok()?;
    Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

type Rows = Vec<(String, String, i64, i64)>;

fn query_rows(db: &Connection, sql: &str) -> Rows {
    let Ok(mut st) = db.prepare(sql) else { return vec![] };
    st.query_map([], |r| {
        Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get(2)?, r.get::<_, Option<i64>>(3)?.unwrap_or(0)))
    })
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
}

/// Imports bookmarks, history and (Chromium) search engines. Passwords are not imported:
/// recent Chromium builds encrypt them with app-bound keys only the browser itself can read.
pub fn import(src: &Source, profile: &mut Profile, history: &mut History) -> Report {
    let mut r = Report::default();
    match src.kind {
        Kind::Chromium => {
            if let Ok(json) = std::fs::read_to_string(src.dir.join("Bookmarks")) {
                r.bookmarks = profile.merge_bookmarks(parse_chromium_bookmarks(&json));
            }
            if let Some(db) = open_copy(&src.dir.join("History")) {
                let mut rows = query_rows(&db, "SELECT url,title,visit_count,last_visit_time FROM urls WHERE hidden=0");
                for row in &mut rows {
                    row.3 = chromium_time_to_unix(row.3);
                }
                r.history = history.import(&rows).unwrap_or(0);
            }
            if let Some(db) = open_copy(&src.dir.join("Web Data")) {
                let rows: Vec<(String, String, String)> = db
                    .prepare("SELECT short_name,keyword,url FROM keywords")
                    .and_then(|mut st| st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect())
                    .unwrap_or_default();
                let engines = rows
                    .into_iter()
                    .filter_map(|(name, kw, url)| {
                        Some(Engine { name: if name.is_empty() { kw.clone() } else { name }, keyword: kw, url: chromium_search_url(&url)? })
                    })
                    .collect();
                r.engines = profile.merge_engines(engines);
            }
        }
        Kind::Firefox => {
            if let Some(db) = open_copy(&src.dir.join("places.sqlite")) {
                let marks = query_rows(
                    &db,
                    "SELECT p.url, b.title, 0, 0 FROM moz_bookmarks b JOIN moz_places p ON p.id=b.fk WHERE b.type=1",
                );
                r.bookmarks = profile.merge_bookmarks(
                    marks
                        .into_iter()
                        .filter(|m| m.0.starts_with("http"))
                        .map(|(url, title, _, _)| Bookmark { title, url, folder: "Firefox".into() })
                        .collect(),
                );
                let mut rows = query_rows(
                    &db,
                    "SELECT url,title,visit_count,last_visit_date FROM moz_places WHERE visit_count>0 AND url LIKE 'http%'",
                );
                for row in &mut rows {
                    row.3 /= 1_000_000;
                }
                r.history = history.import(&rows).unwrap_or(0);
            }
        }
    }
    r
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
      "other":{"type":"folder","name":"Autres","children":[{"type":"url","name":"JS","url":"javascript:alert(1)"},{"type":"url","name":"Doc","url":"https://docs.rs/"}]},
      "synced":{"type":"folder","name":"Mobile","children":[]}}}"#;

    #[test]
    fn flattens_bookmarks_with_folders_and_skips_scripts() {
        let b = parse_chromium_bookmarks(BOOKMARKS);
        assert_eq!(b.len(), 3);
        assert_eq!(b[2].folder, "Autres");
        assert_eq!(b[0], Bookmark { title: "Rust".into(), url: "https://rust-lang.org/".into(), folder: "".into() });
        assert_eq!(b[1].folder, "Dev");
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

    /// Reads the real browsers on this machine into throwaway stores: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn imports_installed_browsers() {
        let mut history = History::open(Path::new(":memory:")).unwrap();
        for src in sources() {
            let mut profile = Profile::default();
            let r = import(&src, &mut profile, &mut history);
            println!("{:<28} {:>5} signets {:>7} historique {:>3} moteurs", src.name, r.bookmarks, r.history, r.engines);
        }
    }
}
