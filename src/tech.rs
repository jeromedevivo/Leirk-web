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
        let mut parts = raw.split("\\;");
        let src = parts.next().unwrap_or("").to_string();
        let version = parts.find_map(|p| p.strip_prefix("version:")).unwrap_or("").to_string();
        Pattern { src, version, re: OnceLock::new() }
    }
    /// `Some(version)` when the pattern matches `text` ("" when no version is extracted).
    fn test(&self, text: &str) -> Option<String> {
        if self.src.is_empty() {
            return Some(String::new());
        }
        // JS-only syntax (lookbehind, backreferences) fails to compile and the pattern is skipped
        let re = self.re.get_or_init(|| RegexBuilder::new(&self.src).case_insensitive(true).size_limit(1 << 20).build().ok());
        let caps = re.as_ref()?.captures(text)?;
        let mut v = self.version.clone();
        for i in (1..caps.len()).rev() {
            v = v.replace(&format!("\\{i}"), caps.get(i).map_or("", |m| m.as_str()));
        }
        Some(v.trim().to_string())
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
        fn list(v: &Value) -> Vec<String> {
            match v {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                _ => vec![],
            }
        }
        fn keyed(v: &Value, lower: bool) -> Vec<(String, Pattern)> {
            let Some(o) = v.as_object() else { return vec![] };
            o.iter()
                .flat_map(|(k, p)| {
                    let k = if lower { k.to_lowercase() } else { k.clone() };
                    let pats = if p.is_string() || p.is_array() { list(p) } else { vec![String::new()] };
                    pats.into_iter().map(move |s| (k.clone(), Pattern::parse(&s)))
                })
                .collect()
        }
        let pats = |v: &Value| list(v).iter().map(|s| Pattern::parse(s)).collect::<Vec<_>>();
        let mut out = vec![];
        for text in techs {
            let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else { continue };
            for (name, t) in map {
                let dom = match &t["dom"] {
                    Value::Object(o) => o.iter().filter(|(_, v)| v.get("exists").is_some()).map(|(k, _)| k.clone()).collect(),
                    v => list(v),
                };
                out.push(Tech {
                    cats: t["cats"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default(),
                    icon: t["icon"].as_str().unwrap_or("").to_string(),
                    website: t["website"].as_str().unwrap_or("").to_string(),
                    headers: keyed(&t["headers"], true),
                    cookies: keyed(&t["cookies"], true),
                    meta: keyed(&t["meta"], true),
                    js: keyed(&t["js"], false),
                    script_src: pats(&t["scriptSrc"]),
                    html: pats(&t["html"]),
                    url: pats(&t["url"]),
                    dom,
                    implies: list(&t["implies"]).iter().map(|s| s.split("\\;").next().unwrap_or("").to_string()).collect(),
                    name,
                });
            }
        }
        let cats = serde_json::from_str::<Value>(cats)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, v)| Some((k.parse().ok()?, v["name"].as_str()?.to_string())))
            .collect();
        Db { techs: out, cats }
    }

    pub fn len(&self) -> usize {
        self.techs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.techs.is_empty()
    }

    /// Every JS global the fingerprints probe, for the page script to look up.
    pub fn js_keys(&self) -> Vec<String> {
        let set: std::collections::BTreeSet<&String> = self.techs.iter().flat_map(|t| t.js.iter().map(|(k, _)| k)).collect();
        set.into_iter().cloned().collect()
    }

    /// Every DOM "exists" selector the fingerprints use.
    pub fn dom_selectors(&self) -> Vec<String> {
        let set: std::collections::BTreeSet<&String> = self.techs.iter().flat_map(|t| t.dom.iter()).collect();
        set.into_iter().cloned().collect()
    }

    /// Detected technologies (with those they imply), sorted by category then name.
    pub fn detect(&self, s: &Signals) -> Vec<Found> {
        let lower = |v: &[(String, String)]| v.iter().map(|(k, x)| (k.to_lowercase(), x.clone())).collect::<Vec<_>>();
        let (headers, meta) = (lower(&s.headers), lower(&s.meta));
        let cookies: HashSet<String> = s.cookies.iter().map(|c| c.to_lowercase()).collect();
        let dom: HashSet<&String> = s.dom.iter().collect();
        let mut hits: BTreeMap<String, String> = BTreeMap::new();
        for t in &self.techs {
            let mut ver: Option<String> = None;
            let mut hit = |v: Option<String>| {
                if let Some(v) = v {
                    if ver.as_ref().is_none_or(|x| x.is_empty()) {
                        ver = Some(v);
                    }
                }
            };
            for (name, p) in &t.headers {
                headers.iter().filter(|(k, _)| k == name).for_each(|(_, v)| hit(p.test(v)));
            }
            for (name, p) in &t.meta {
                meta.iter().filter(|(k, _)| k == name).for_each(|(_, v)| hit(p.test(v)));
            }
            for (name, p) in &t.js {
                s.js.iter().filter(|(k, _)| k == name).for_each(|(_, v)| hit(p.test(v)));
            }
            for (name, _) in &t.cookies {
                if cookies.contains(name) {
                    hit(Some(String::new()));
                }
            }
            for p in &t.script_src {
                s.scripts.iter().for_each(|src| hit(p.test(src)));
            }
            if !s.html.is_empty() {
                t.html.iter().for_each(|p| hit(p.test(&s.html)));
            }
            if !s.url.is_empty() {
                t.url.iter().for_each(|p| hit(p.test(&s.url)));
            }
            if t.dom.iter().any(|d| dom.contains(d)) {
                hit(Some(String::new()));
            }
            if let Some(v) = ver {
                hits.insert(t.name.clone(), v);
            }
        }
        let by_name: HashMap<&str, &Tech> = self.techs.iter().map(|t| (t.name.as_str(), t)).collect();
        let mut queue: Vec<String> = hits.keys().cloned().collect();
        while let Some(n) = queue.pop() {
            for imp in by_name.get(n.as_str()).map(|t| t.implies.clone()).unwrap_or_default() {
                if by_name.contains_key(imp.as_str()) && !hits.contains_key(&imp) {
                    hits.insert(imp.clone(), String::new());
                    queue.push(imp);
                }
            }
        }
        let mut found: Vec<Found> = hits
            .into_iter()
            .filter_map(|(name, version)| {
                let t = by_name.get(name.as_str())?;
                let category = t.cats.first().and_then(|c| self.cats.get(c)).cloned().unwrap_or_default();
                Some(Found { name, version, category, icon: t.icon.clone(), website: t.website.clone() })
            })
            .collect();
        found.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
        found
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
