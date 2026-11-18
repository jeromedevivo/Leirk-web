//! Shields: Brave's own adblock-rust engine, with per-site opt-out.
use std::collections::HashSet;

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

    /// `kind` uses adblock request types: "script", "image", "stylesheet", "xmlhttprequest", "sub_frame", "other"...
    pub fn should_block(&self, url: &str, page_url: &str, kind: &str) -> bool {
        if self.disabled_sites.contains(host(page_url)) {
            return false;
        }
        adblock::request::Request::new(url, page_url, kind, "GET")
            .map(|r| self.engine.check_network_request(&r).should_block())
            .unwrap_or(false)
    }

    /// Site-specific CSS selectors to hide on `url` (cosmetic filtering).
    pub fn hide_selectors(&self, url: &str) -> Vec<String> {
        if self.disabled_sites.contains(host(url)) {
            return vec![];
        }
        self.engine.url_cosmetic_resources(url).hide_selectors.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[test]
    fn cosmetic_selectors() {
        let s = Shields::new(RULES);
        assert!(s.hide_selectors("https://news.com/article").contains(&".ad-banner".to_string()));
        assert!(s.hide_selectors("https://other.com/").is_empty());
    }
}
