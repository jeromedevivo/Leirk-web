//! Technology detection (Wappalyzer fingerprints, from the open webappanalyzer database).
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

/// What the page exposes; collected by a script in the page plus the main document's response headers.
#[derive(Default, Debug)]
pub struct Signals {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<String>,
    pub meta: Vec<(String, String)>,
    pub scripts: Vec<String>,
    /// JS globals that exist, with their string value ("" when not a string or number).
    pub js: Vec<(String, String)>,
    pub html: String,
    /// CSS selectors from [`Db::dom_selectors`] that matched at least one element.
    pub dom: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    pub version: String,
    pub category: String,
    pub icon: String,
    pub website: String,
}

/// A fingerprint pattern: a regex (compiled on first use) and an optional `\1`-style version template.
struct Pattern {
    src: String,
    version: String,
    re: OnceLock<Option<Regex>>,
}

impl Pattern {
    fn parse(raw: &str) -> Pattern {
        todo!()
    }
    /// `Some(version)` when the pattern matches `text` ("" when no version is extracted).
    fn test(&self, text: &str) -> Option<String> {
        todo!()
    }
}

struct Tech {
    name: String,
    cats: Vec<u64>,
    icon: String,
    website: String,
    headers: Vec<(String, Pattern)>,
    cookies: Vec<(String, Pattern)>,
    meta: Vec<(String, Pattern)>,
    js: Vec<(String, Pattern)>,
    script_src: Vec<Pattern>,
    html: Vec<Pattern>,
    url: Vec<Pattern>,
    dom: Vec<String>,
    implies: Vec<String>,
}

pub struct Db {
    techs: Vec<Tech>,
    cats: HashMap<u64, String>,
}

impl Db {
    /// `techs` are the a.json … z.json files; `cats` is categories.json.
    pub fn parse(techs: &[&str], cats: &str) -> Db {
        todo!()
    }

    pub fn len(&self) -> usize {
        self.techs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.techs.is_empty()
    }

    /// Every JS global the fingerprints probe, for the page script to look up.
    pub fn js_keys(&self) -> Vec<String> {
        todo!()
    }

    /// Every DOM "exists" selector the fingerprints use.
    pub fn dom_selectors(&self) -> Vec<String> {
        todo!()
    }

    /// Detected technologies (with those they imply), sorted by category then name.
    pub fn detect(&self, s: &Signals) -> Vec<Found> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATS: &str = r#"{"1":{"name":"CMS"},"12":{"name":"JavaScript frameworks"},"22":{"name":"Web servers"},"27":{"name":"Programming languages"},"59":{"name":"JavaScript libraries"}}"#;
    const TECHS: &str = r#"{
      "React": {"cats":[12],"icon":"React.svg","js":{"React.version":"^([\\d\\.]+)$\\;version:\\1"},"html":["<[^>]+data-react"]},
      "Nginx": {"cats":[22],"icon":"Nginx.svg","headers":{"Server":"nginx(?:/([\\d.]+))?\\;version:\\1"}},
      "WordPress": {"cats":[1],"icon":"WordPress.svg","meta":{"generator":"^WordPress ?([\\d.]+)?\\;version:\\1"},"implies":"PHP"},
      "PHP": {"cats":[27],"icon":"PHP.svg","cookies":{"PHPSESSID":""}},
      "jQuery": {"cats":[59],"icon":"jQuery.svg","scriptSrc":["jquery(?:-(\\d+\\.\\d+\\.\\d+))[/.-]\\;version:\\1"]},
      "Shopify": {"cats":[1],"icon":"Shopify.svg","url":"^https?//[^/]+\\.myshopify\\.com","dom":{"link[href*='cdn.shopify.com']":{"exists":""}}},
      "Broken": {"cats":[1],"html":"(?<=lookbehind)x"}
    }"#;

    fn db() -> Db {
        Db::parse(&[TECHS], CATS)
    }

    #[test]
    fn patterns_and_versions() {
        let p = Pattern::parse("nginx(?:/([\\d.]+))?\\;version:\\1");
        assert_eq!(p.test("nginx/1.25.3").as_deref(), Some("1.25.3"));
        assert_eq!(p.test("NGINX").as_deref(), Some(""), "case-insensitive, version optional");
        assert_eq!(p.test("apache"), None);
        assert_eq!(Pattern::parse("").test("anything").as_deref(), Some(""), "empty pattern = presence");
        assert_eq!(Pattern::parse("(?<=x)y").test("xy"), None, "unsupported JS regex is skipped, not a panic");
    }

    #[test]
    fn detects_from_every_signal_kind() {
        let s = Signals {
            url: "https://shop.example.com/".into(),
            headers: vec![("server".into(), "nginx/1.25.3".into())],
            cookies: vec!["PHPSESSID".into()],
            meta: vec![("generator".into(), "WordPress 6.4.2".into())],
            scripts: vec!["https://cdn.x.com/jquery-3.7.1.min.js".into()],
            js: vec![("React.version".into(), "18.2.0".into())],
            html: "<div data-reactroot>".into(),
            dom: vec![],
        };
        let found = db().detect(&s);
        let get = |n: &str| found.iter().find(|f| f.name == n).map(|f| f.version.clone());
        assert_eq!(get("React").as_deref(), Some("18.2.0"));
        assert_eq!(get("Nginx").as_deref(), Some("1.25.3"));
        assert_eq!(get("WordPress").as_deref(), Some("6.4.2"));
        assert_eq!(get("jQuery").as_deref(), Some("3.7.1"));
        assert_eq!(get("PHP").as_deref(), Some(""));
        assert!(get("Shopify").is_none());
        assert_eq!(found.iter().find(|f| f.name == "Nginx").unwrap().category, "Web servers");
    }

    #[test]
    fn implies_and_dom() {
        let s = Signals { meta: vec![("generator".into(), "WordPress".into())], dom: vec!["link[href*='cdn.shopify.com']".into()], ..Default::default() };
        let names: Vec<String> = db().detect(&s).into_iter().map(|f| f.name).collect();
        assert!(names.contains(&"PHP".to_string()), "WordPress implies PHP: {names:?}");
        assert!(names.contains(&"Shopify".to_string()));
        assert!(db().js_keys().contains(&"React.version".to_string()));
        assert_eq!(db().dom_selectors(), vec!["link[href*='cdn.shopify.com']".to_string()]);
    }
}
