//! Shields: Brave's own adblock-rust engine with Brave's list catalog, scriptlet resources,
//! redirects, generic and site cosmetic filtering, and per-site opt-out.
use adblock::lists::{FilterSet, ParseOptions};
use adblock::resources::{PermissionMask, Resource};
use serde_json::Value;
use std::collections::HashSet;

/// uBlock Origin's scriptlets and redirect payloads, built by scripts/ubo-resources.mjs. Brave's published
/// resources.json only carries Brave's extras, so without these no `+js(...)` filter (YouTube's included) would run.
const UBO_RESOURCES: &str = include_str!("../assets/ubo-resources.json");
/// The permission uBO's `trusted-*` scriptlets require (see the script above).
const TRUSTED: PermissionMask = PermissionMask::from_bits(1);

/// Procedural cosmetic filters (`:has-text`, `:upward`, `:remove()`…), applied in the page like Brave's content script.
/// `__P__` is replaced by the JSON list adblock-rust returns; filters with an unknown operator are skipped, never guessed.
const PROCEDURAL_JS: &str = r#"var P=__P__;
var R=function(a){var m=/^\/(.+)\/([imsu]*)$/.exec(a);if(m)return new RegExp(m[1],m[2]);return{test:function(s){return String(s).indexOf(a)>=0}}};
var css=function(n,a){var i=a.indexOf(':');var p=a.slice(0,i).trim(),v=a.slice(i+1).trim();return R(v).test(getComputedStyle(n)[p])};
var step=function(ns,o){var r=[];for(var k=0;k<ns.length;k++){var n=ns[k],a=o.arg;switch(o.type){
case'css-selector':var q=n===document?document.querySelectorAll(a):n.querySelectorAll(/^[>+~]/.test(a)?':scope'+a:':scope '+a);for(var j=0;j<q.length;j++)r.push(q[j]);break;
case'has-text':if(R(a).test(n.textContent))r.push(n);break;
case'min-text-length':if(n.textContent.length>=+a)r.push(n);break;
case'matches-css':if(css(n,a))r.push(n);break;
case'matches-path':if(R(a).test(location.pathname+location.search))r.push(n);break;
case'matches-attr':var m=/^"?([^"=]+)"?(?:="?(.*?)"?)?$/.exec(a);if(m){var at=[].slice.call(n.attributes||[]);if(at.some(function(x){return R(m[1]).test(x.name)&&(m[2]===undefined||R(m[2]).test(x.value))}))r.push(n)}break;
case'upward':var u=n;if(/^\d+$/.test(a)){for(var t=+a;t>0&&u;t--)u=u.parentElement}else u=n.parentElement&&n.parentElement.closest(a);if(u)r.push(u);break;
case'xpath':var x=document.evaluate(a,n,null,7,null);for(var j=0;j<x.snapshotLength;j++)r.push(x.snapshotItem(j));break;
default:return null}}return r};
var apply=function(){P.forEach(function(f){try{var ns=[document];for(var i=0;i<f.selector.length&&ns;i++)ns=step(ns,f.selector[i]);if(!ns)return;var a=f.action||{};
ns.forEach(function(n){if(n===document)return;switch(a.type){case'remove':n.remove();break;case'style':n.style.cssText+=';'+a.arg;break;
case'remove-attr':n.removeAttribute(a.arg);break;case'remove-class':n.classList.remove(a.arg);break;default:n.style.setProperty('display','none','important')}})}catch(e){}})};
var due=0;var later=function(){if(!due)due=setTimeout(function(){due=0;apply()},150)};
document.addEventListener('DOMContentLoaded',function(){apply();new MutationObserver(later).observe(document.documentElement,{childList:true,subtree:true})});"#;

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

/// Is `url` one of Leirk's own lists (trusted, like uBO's and Brave's defaults), not one the user added?
pub fn is_builtin(url: &str) -> bool {
    let expand = |u: &str| u.replace("{ubo}", UBO).replace("{brave}", BRAVE);
    LISTS.iter().flat_map(|l| l.urls).map(|u| expand(u)).chain(AGGRESSIVE.iter().map(|u| u.to_string())).any(|u| u == url)
}

/// adblock-rust drops `$popup` filters; keep them, minus that option, for a separate engine that judges window.open targets.
fn popup_rules(text: &str) -> String {
    let mut out = String::new();
    for l in text.lines().map(str::trim) {
        if l.starts_with('!') || ["##", "#@#", "#?#", "#$#", "#%#"].iter().any(|c| l.contains(c)) {
            continue;
        }
        let Some((pat, opts)) = l.rsplit_once('$') else { continue };
        let kept: Vec<&str> = opts.split(',').filter(|o| !matches!(*o, "popup" | "popunder")).collect();
        if pat.is_empty() || kept.len() == opts.split(',').count() {
            continue;
        }
        out.push_str(pat);
        if !kept.is_empty() {
            out.push('$');
            out.push_str(&kept.join(","));
        }
        out.push('\n');
    }
    out
}

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
    popups: adblock::Engine,
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
    /// Untrusted rules only (the user's own).
    pub fn new(rules: &str) -> Self {
        Self::from_lists("", rules)
    }

    /// `trusted` rules may inject `trusted-*` scriptlets, `other` rules may not.
    pub fn from_lists(trusted: &str, other: &str) -> Self {
        let mut set = FilterSet::new(false);
        set.add_filter_list(trusted.to_string(), ParseOptions { permissions: TRUSTED, ..Default::default() });
        set.add_filter_list(other.to_string(), ParseOptions::default());
        let mut engine = adblock::Engine::new_with_filter_set(set);
        engine.use_resources(Self::ubo_resources());
        let popups = adblock::Engine::new_with_list_text(popup_rules(trusted) + &popup_rules(other));
        Shields { engine, popups, disabled_sites: HashSet::new() }
    }

    fn ubo_resources() -> Vec<Resource> {
        serde_json::from_str(UBO_RESOURCES).unwrap_or_default()
    }

    /// Adds Brave's `resources.json` to uBO's resources. Invalid JSON is ignored.
    pub fn with_resources(mut self, json: &str) -> Self {
        if let Ok(r) = serde_json::from_str::<Vec<Resource>>(json) {
            self.engine.use_resources(Self::ubo_resources().into_iter().chain(r));
        }
        self
    }

    /// Should a window.open from `page_url` to `url` be refused (`$popup` filters: popunders, ad tabs)?
    pub fn block_popup(&self, url: &str, page_url: &str) -> bool {
        if self.disabled_sites.contains(host(page_url)) {
            return false;
        }
        let Ok(req) = adblock::request::Request::new(url, page_url, "other", "GET") else { return false };
        self.popups.check_network_request(&req).should_block()
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
        if sels.is_empty() && res.injected_script.is_empty() && res.procedural_actions.is_empty() {
            return None;
        }
        let procedural = if res.procedural_actions.is_empty() {
            String::new()
        } else {
            let list: Vec<Value> = res.procedural_actions.iter().filter_map(|p| serde_json::from_str(p).ok()).collect();
            PROCEDURAL_JS.replace("__P__", &Value::Array(list).to_string())
        };
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
        // every frame: ads are often served in iframes, and Brave runs scriptlets there too
        Some(format!("(function(){{try{{{}}}catch(e){{}}{style}try{{{procedural}}}catch(e){{}}}})();", res.injected_script))
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
    fn procedural_filters_reach_the_page_script() {
        // native ads hidden by text, as uBlock Origin and Brave do
        let s = Shields::new("feed.com##div.post:has-text(Sponsored)\nfeed.com##.promo:remove()");
        let js = s.document_script("https://feed.com/").expect("script for feed.com");
        assert!(js.contains("has-text") && js.contains("Sponsored"), "{js}");
        assert!(js.contains("remove"), "{js}");
        assert!(s.document_script("https://other.com/").is_none());
    }

    #[test]
    fn scriptlets_also_run_in_frames() {
        let s = Shields::new("feed.com##div.post:has-text(Sponsored)");
        let js = s.document_script("https://feed.com/").unwrap();
        assert!(!js.contains("window.top!==window"), "ads often live in iframes: {js}");
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
    fn popup_filters_judge_window_open() {
        let s = Shields::new("||popads.net^$popup,third-party\n||ads.com^\n@@||popads.net/ok$popup");
        assert!(s.block_popup("https://popads.net/x", "https://site.com/"));
        assert!(!s.block_popup("https://popads.net/ok", "https://site.com/"));
        assert!(!s.block_popup("https://ads.com/", "https://site.com/"), "plain filters never block a new tab");
        assert!(!s.block_popup("https://other.com/", "https://site.com/"));
    }

    #[test]
    fn trusted_scriptlets_only_from_builtin_lists() {
        let res = r#"[{"name":"trusted-x.js","aliases":[],"kind":{"mime":"application/javascript"},"content":"ZnVuY3Rpb24gdHJ1c3RlZFgoKXt9","dependencies":[],"permission":1}]"#;
        let js = |s: Shields| s.with_resources(res).document_script("https://yt.com/").unwrap_or_default();
        assert!(js(Shields::from_lists("yt.com##+js(trusted-x)", "")).contains("trustedX"));
        assert!(!js(Shields::from_lists("", "yt.com##+js(trusted-x)")).contains("trustedX"));
        assert!(is_builtin("https://easylist.to/easylist/easylist.txt"));
        assert!(!is_builtin("https://example.com/list.txt"));
    }

    #[test]
    fn ubo_scriptlets_and_redirects_are_bundled() {
        let s = Shields::from_lists("www.youtube.com##+js(trusted-rpfr, '\"adPlacements\"', '\"no_ads\"', player?)\n||ads.com/x.js$script,redirect=noopjs", "");
        let js = s.document_script("https://www.youtube.com/watch?v=x").expect("YouTube scriptlet");
        assert!(js.contains("adPlacements") && js.contains("function"), "trusted uBO scriptlet injected");
        assert!(matches!(s.check("https://ads.com/x.js", "https://site.com/", "script"), Verdict::Redirect(_)));
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
