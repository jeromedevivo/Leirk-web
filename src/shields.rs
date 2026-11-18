//! Shields: Brave's own adblock-rust engine with Brave's list catalog, scriptlet resources,
//! redirects, generic and site cosmetic filtering, and per-site opt-out.
use serde_json::Value;
use std::collections::HashSet;

pub struct FilterList {
    pub id: &'static str,
    pub urls: &'static [&'static str],
}

const UBO: &str = "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/";
const BRAVE: &str = "https://raw.githubusercontent.com/brave/adblock-lists/master/";

/// Brave's list catalog (filter_lists/list_catalog.json), desktop entries only. `{ubo}`/`{brave}` expand to the repos above.
pub const LISTS: &[FilterList] = &[
    FilterList {
        id: "default",
        urls: &[
            "{ubo}filters.txt", "{ubo}filters-2020.txt", "{ubo}filters-2021.txt", "{ubo}filters-2022.txt",
            "{ubo}filters-2023.txt", "{ubo}filters-2024.txt", "{ubo}filters-2025.txt", "{ubo}filters-2026.txt",
            "{ubo}filters-general.txt", "{ubo}badware.txt", "{ubo}resource-abuse.txt", "{ubo}unbreak.txt",
            "{ubo}quick-fixes.txt", "{ubo}ubo-link-shorteners.txt",
            "https://easylist.to/easylist/easylist.txt",
            "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-agh-online.txt",
            "{brave}brave-unbreak.txt", "{brave}brave-lists/brave-specific.txt", "{brave}brave-lists/brave-social.txt",
            "{brave}brave-lists/brave-unbreak.txt", "{brave}brave-lists/brave-sugarcoat.txt",
            "{brave}brave-lists/brave-firstparty.txt",
        ],
    },
    FilterList { id: "privacy", urls: &["https://easylist.to/easylist/easyprivacy.txt", "{ubo}privacy.txt"] },
    FilterList {
        id: "cookies",
        urls: &[
            "https://secure.fanboy.co.nz/fanboy-cookiemonster_ubo.txt",
            "{ubo}annoyances-cookies.txt",
            "{brave}brave-lists/brave-cookie-specific.txt",
        ],
    },
    FilterList { id: "annoyances", urls: &["https://secure.fanboy.co.nz/fanboy-annoyance_ubo.txt", "{ubo}annoyances-others.txt"] },
    FilterList { id: "mobile", urls: &["https://secure.fanboy.co.nz/fanboy-mobile-notifications.txt"] },
    FilterList { id: "social", urls: &["https://easylist-downloads.adblockplus.org/fanboy-social.txt"] },
    FilterList {
        id: "fr",
        urls: &["https://filters.adtidy.org/extension/ublock/filters/16.txt", "https://easylist-downloads.adblockplus.org/liste_fr.txt"],
    },
];

/// Extra lists in aggressive mode: AdGuard Base, Tracking Protection and URL Tracking.
const AGGRESSIVE: &[&str] = &[
    "https://filters.adtidy.org/extension/ublock/filters/2.txt",
    "https://filters.adtidy.org/extension/ublock/filters/3.txt",
    "https://filters.adtidy.org/extension/ublock/filters/17.txt",
];

/// Scriptlets and redirect payloads, as Brave ships them.
pub const RESOURCES_URL: &str = "https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json";

/// Every list URL the prefs enable, in order, without duplicates.
pub fn list_urls(prefs: &Value) -> Vec<String> {
    let level = prefs["shields_level"].as_str().unwrap_or("standard");
    let mut out: Vec<String> = vec![];
    if level == "off" {
        return out;
    }
    let mut push = |u: String| {
        if !out.contains(&u) {
            out.push(u);
        }
    };
    for l in LISTS.iter().filter(|l| prefs["lists"][l.id].as_bool().unwrap_or(false)) {
        for u in l.urls {
            push(u.replace("{ubo}", UBO).replace("{brave}", BRAVE));
        }
    }
    if level == "aggressive" {
        AGGRESSIVE.iter().for_each(|u| push(u.to_string()));
    }
    for line in prefs["custom_lists"].as_str().unwrap_or("").lines().map(str::trim) {
        if line.starts_with("https://") {
            push(line.to_string()); // lists can carry scriptlets: never over plain HTTP
        }
    }
    out
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Allow,
    Block,
    /// Serve this `data:` URL instead (a neutral script, pixel...), which keeps pages working.
    Redirect(String),
}

pub struct Shields {
    engine: adblock::Engine,
    pub disabled_sites: HashSet<String>,
}

/// Host of a URL ("" when there is none). `www.` is kept: the site key is the exact host.
pub fn host(url: &str) -> &str {
    let Some((_, rest)) = url.split_once("://") else { return "" };
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let auth = auth.rsplit('@').next().unwrap_or(auth);
    auth.split(':').next().unwrap_or("")
}

impl Shields {
    pub fn new(rules: &str) -> Self {
        Shields { engine: adblock::Engine::new_with_list_text(rules), disabled_sites: HashSet::new() }
    }

    /// Loads Brave's `resources.json` (scriptlets and redirect targets). Invalid JSON is ignored.
    pub fn with_resources(mut self, json: &str) -> Self {
        if let Ok(r) = serde_json::from_str::<Vec<adblock::resources::Resource>>(json) {
            self.engine.use_resources(r);
        }
        self
    }

    /// `kind` uses adblock request types: "script", "image", "stylesheet", "xmlhttprequest", "sub_frame", "other"...
    pub fn check(&self, url: &str, page_url: &str, kind: &str) -> Verdict {
        if self.disabled_sites.contains(host(page_url)) {
            return Verdict::Allow;
        }
        let Ok(req) = adblock::request::Request::new(url, page_url, kind, "GET") else { return Verdict::Allow };
        let r = self.engine.check_network_request(&req);
        match r.redirect {
            Some(data) if r.should_block() => Verdict::Redirect(data),
            _ if r.should_block() => Verdict::Block,
            _ => Verdict::Allow,
        }
    }

    pub fn should_block(&self, url: &str, page_url: &str, kind: &str) -> bool {
        self.check(url, page_url, kind) != Verdict::Allow
    }

    /// Site-specific CSS selectors to hide on `url`.
    pub fn hide_selectors(&self, url: &str) -> Vec<String> {
        if self.disabled_sites.contains(host(url)) {
            return vec![];
        }
        self.engine.url_cosmetic_resources(url).hide_selectors.into_iter().collect()
    }

    /// Script to run at document creation on `url`: its scriptlets plus a style sheet hiding site-specific ads.
    pub fn document_script(&self, url: &str) -> Option<String> {
        if self.disabled_sites.contains(host(url)) {
            return None;
        }
        let res = self.engine.url_cosmetic_resources(url);
        let sels: Vec<&str> = res.hide_selectors.iter().map(String::as_str).collect();
        if sels.is_empty() && res.injected_script.is_empty() {
            return None;
        }
        let style = if sels.is_empty() {
            String::new()
        } else {
            let css = serde_json::to_string(&format!("{}{{display:none!important}}", sels.join(","))).unwrap_or_default();
            // the document element may not exist yet at document creation
            format!(
                "var c={css};var a=function(){{var s=document.createElement('style');s.textContent=c;(document.head||document.documentElement).appendChild(s)}};\
                 if(document.documentElement)a();else new MutationObserver(function(m,o){{if(document.documentElement){{o.disconnect();a()}}}}).observe(document,{{childList:true}});"
            )
        };
        Some(format!("(function(){{if(window.top!==window)return;try{{{}}}catch(e){{}}{style}}})();", res.injected_script))
    }

    /// Generic cosmetic filtering: selectors matching the classes and ids the page actually uses.
    pub fn generic_selectors(&self, url: &str, classes: &[String], ids: &[String]) -> Vec<String> {
        if self.disabled_sites.contains(host(url)) {
            return vec![];
        }
        let res = self.engine.url_cosmetic_resources(url);
        if res.generichide {
            return vec![];
        }
        self.engine.hidden_class_id_selectors(classes, ids, &res.exceptions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const RULES: &str = "||ads.example.com^\n||tracker.net^$third-party\nnews.com##.ad-banner\n@@||ads.example.com/ok.js";

    #[test]
    fn host_parsing() {
        assert_eq!(host("https://www.a.com:8080/x?y"), "www.a.com");
        assert_eq!(host("http://b.org"), "b.org");
        assert_eq!(host("about:blank"), "");
    }

    #[test]
    fn blocks_listed_domains_and_honours_exceptions() {
        let s = Shields::new(RULES);
        assert!(s.should_block("https://ads.example.com/x.js", "https://site.com/", "script"));
        assert!(!s.should_block("https://ads.example.com/ok.js", "https://site.com/", "script"));
        assert!(!s.should_block("https://cdn.site.com/app.js", "https://site.com/", "script"));
    }

    #[test]
    fn third_party_option() {
        let s = Shields::new(RULES);
        assert!(s.should_block("https://tracker.net/p.gif", "https://site.com/", "image"));
        assert!(!s.should_block("https://tracker.net/p.gif", "https://tracker.net/", "image"));
    }

    #[test]
    fn per_site_disable() {
        let mut s = Shields::new(RULES);
        s.disabled_sites.insert("site.com".into());
        assert!(!s.should_block("https://ads.example.com/x.js", "https://site.com/a", "script"));
        assert_eq!(s.document_script("https://site.com/"), None);
    }

    #[test]
    fn cosmetic_selectors() {
        let s = Shields::new(RULES);
        assert!(s.hide_selectors("https://news.com/article").contains(&".ad-banner".to_string()));
        assert!(s.hide_selectors("https://other.com/").is_empty());
    }

    fn resources() -> String {
        // base64 of "(function(){})();" and of "window.__s='{{1}}';"
        json!([
            {"name": "noop.js", "aliases": ["noopjs"], "kind": {"mime": "application/javascript"}, "content": "KGZ1bmN0aW9uKCl7fSkoKTs="},
            {"name": "set-flag.js", "kind": "template", "content": "d2luZG93Ll9fcz0ne3sxfX0nOw=="}
        ])
        .to_string()
    }

    #[test]
    fn redirects_to_neutral_resources() {
        let s = Shields::new("||ads.com/ad.js$script,redirect=noop.js").with_resources(&resources());
        match s.check("https://ads.com/ad.js", "https://site.com/", "script") {
            Verdict::Redirect(u) => assert!(u.starts_with("data:application/javascript;base64,"), "{u}"),
            v => panic!("expected redirect, got {v:?}"),
        }
        assert_eq!(Shields::new("x").with_resources("not json").check("https://a.com/", "https://b.com/", "script"), Verdict::Allow);
    }

    #[test]
    fn document_script_carries_scriptlets_and_site_css() {
        let s = Shields::new("x.com##+js(set-flag, on)\nx.com##.banner").with_resources(&resources());
        let js = s.document_script("https://x.com/page").expect("script for x.com");
        assert!(js.contains("window.__s='on'"), "{js}");
        assert!(js.contains(".banner"));
        assert_eq!(s.document_script("https://y.com/"), None);
    }

    #[test]
    fn generic_cosmetics_follow_page_classes() {
        let s = Shields::new("##.ad-slot\n###sponsor\nok.com#@#.ad-slot");
        let got = s.generic_selectors("https://x.com/", &["ad-slot".into(), "nav".into()], &["sponsor".into()]);
        assert!(got.contains(&".ad-slot".to_string()) && got.contains(&"#sponsor".to_string()), "{got:?}");
        assert!(s.generic_selectors("https://ok.com/", &["ad-slot".into()], &[]).is_empty(), "site exception wins");
    }

    #[test]
    fn list_selection_follows_prefs() {
        let defaults: Value = serde_json::from_str(crate::store::DEFAULTS).unwrap();
        let urls = list_urls(&defaults);
        assert!(urls.iter().any(|u| u == "https://easylist.to/easylist/easylist.txt"));
        assert!(urls.iter().any(|u| u.ends_with("uAssets/master/filters/filters-2026.txt")));
        assert!(!urls.iter().any(|u| u.contains("fanboy-social")));
        assert!(!urls.iter().any(|u| u.ends_with("/filters/2.txt")));
        let mut p = defaults.clone();
        p["shields_level"] = json!("aggressive");
        p["custom_lists"] = json!("# my lists\nhttps://example.com/list.txt\n\n");
        let urls = list_urls(&p);
        assert!(urls.iter().any(|u| u.ends_with("/filters/2.txt")));
        assert!(urls.contains(&"https://example.com/list.txt".to_string()));
        p["custom_lists"] = json!("http://insecure.example/list.txt");
        assert!(!list_urls(&p).iter().any(|u| u.starts_with("http://")), "plain-HTTP lists are refused");
        p["shields_level"] = json!("off");
        assert!(list_urls(&p).is_empty());
    }
}
