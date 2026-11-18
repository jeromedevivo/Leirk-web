//! Address bar: turns what the user typed into a URL, with every search engine reachable by keyword.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Engine {
    pub name: String,
    pub keyword: String,
    /// Search URL with `%s` where the query goes.
    pub url: String,
}

pub fn builtin_engines() -> Vec<Engine> {
    [
        ("Brave Search", "br", "https://search.brave.com/search?q=%s"),
        ("Google", "g", "https://www.google.com/search?q=%s"),
        ("DuckDuckGo", "d", "https://duckduckgo.com/?q=%s"),
        ("Bing", "b", "https://www.bing.com/search?q=%s"),
        ("Qwant", "q", "https://www.qwant.com/?q=%s"),
        ("Ecosia", "e", "https://www.ecosia.org/search?q=%s"),
        ("Startpage", "sp", "https://www.startpage.com/do/search?q=%s"),
        ("Kagi", "k", "https://kagi.com/search?q=%s"),
        ("Mojeek", "mj", "https://www.mojeek.com/search?q=%s"),
        ("Perplexity", "p", "https://www.perplexity.ai/search?q=%s"),
        ("ChatGPT", "gpt", "https://chatgpt.com/?q=%s"),
        ("Claude", "cl", "https://claude.ai/new?q=%s"),
        ("Yahoo", "y", "https://search.yahoo.com/search?p=%s"),
        ("Yandex", "ya", "https://yandex.com/search/?text=%s"),
        ("Baidu", "bd", "https://www.baidu.com/s?wd=%s"),
        ("Naver", "nv", "https://search.naver.com/search.naver?query=%s"),
        ("Swisscows", "sc", "https://swisscows.com/web?query=%s"),
        ("Lilo", "li", "https://search.lilo.org/?q=%s"),
        ("Wikipédia", "w", "https://fr.wikipedia.org/w/index.php?search=%s"),
        ("YouTube", "yt", "https://www.youtube.com/results?search_query=%s"),
        ("GitHub", "gh", "https://github.com/search?q=%s"),
        ("Reddit", "r", "https://www.reddit.com/search/?q=%s"),
        ("Amazon", "a", "https://www.amazon.fr/s?k=%s"),
        ("Google Maps", "m", "https://www.google.com/maps/search/%s"),
        ("Google Images", "i", "https://www.google.com/search?tbm=isch&q=%s"),
        ("crates.io", "cr", "https://crates.io/search?q=%s"),
        ("MDN", "mdn", "https://developer.mozilla.org/fr/search?q=%s"),
        ("Stack Overflow", "so", "https://stackoverflow.com/search?q=%s"),
    ]
    .into_iter()
    .map(|(n, k, u)| Engine { name: n.into(), keyword: k.into(), url: u.into() })
    .collect()
}

pub fn encode(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn looks_like_url(s: &str) -> Option<&'static str> {
    if s.contains(char::is_whitespace) {
        return None;
    }
    let host_port = s.split(['/', '?', '#']).next().unwrap_or("");
    let host = host_port.split(':').next().unwrap_or("");
    if host == "localhost" || host.parse::<std::net::Ipv4Addr>().is_ok() {
        return Some("http://");
    }
    let tld = host.rsplit('.').next().unwrap_or("");
    let ok = host.contains('.')
        && !host.starts_with('.')
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
        && host.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.');
    ok.then_some("https://")
}

/// What the user typed -> URL to load. `bang`-style `!kw query` and `kw query` both pick an engine.
pub fn resolve(input: &str, engines: &[Engine], default_kw: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    let lower = s.to_ascii_lowercase();
    if ["http://", "https://", "file://", "about:", "data:", "view-source:", "edge://"].iter().any(|p| lower.starts_with(p)) {
        return s.to_string();
    }
    if let Some(scheme) = looks_like_url(s) {
        return format!("{scheme}{s}");
    }
    let search = |e: &Engine, q: &str| e.url.replace("%s", &encode(q));
    if let Some((kw, rest)) = s.split_once(char::is_whitespace) {
        let kw = kw.strip_prefix('!').unwrap_or(kw);
        if let Some(e) = engines.iter().find(|e| e.keyword.eq_ignore_ascii_case(kw)) {
            return search(e, rest.trim());
        }
    }
    let default = engines.iter().find(|e| e.keyword == default_kw).or(engines.first());
    match default {
        Some(e) => search(e, s),
        None => format!("https://search.brave.com/search?q={}", encode(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(s: &str) -> String {
        resolve(s, &builtin_engines(), "br")
    }

    #[test]
    fn urls_pass_through_or_get_https() {
        assert_eq!(r("https://example.com/a?b"), "https://example.com/a?b");
        assert_eq!(r("  example.com  "), "https://example.com");
        assert_eq!(r("sub.example.co.uk/path"), "https://sub.example.co.uk/path");
        assert_eq!(r("localhost:3000"), "http://localhost:3000");
        assert_eq!(r("192.168.1.1"), "http://192.168.1.1");
        assert_eq!(r("about:blank"), "about:blank");
        assert_eq!(r("file:///C:/x.html"), "file:///C:/x.html");
    }

    #[test]
    fn text_goes_to_default_engine() {
        assert_eq!(r("rust lang"), "https://search.brave.com/search?q=rust%20lang");
        assert_eq!(r("version 1.2"), "https://search.brave.com/search?q=version%201.2");
        assert_eq!(r("c++ & co"), "https://search.brave.com/search?q=c%2B%2B%20%26%20co");
        assert_eq!(r("café"), "https://search.brave.com/search?q=caf%C3%A9");
        assert_eq!(r("hello"), "https://search.brave.com/search?q=hello");
    }

    #[test]
    fn keywords_and_bangs_pick_engine() {
        assert_eq!(r("g rust"), "https://www.google.com/search?q=rust");
        assert_eq!(r("!yt lofi"), "https://www.youtube.com/results?search_query=lofi");
        assert_eq!(r("yt"), "https://search.brave.com/search?q=yt");
        assert_eq!(r("!unknown x"), "https://search.brave.com/search?q=%21unknown%20x");
    }

    #[test]
    fn empty_and_unknown_default() {
        assert_eq!(r("   "), "");
        assert_eq!(resolve("hi", &builtin_engines(), "nope"), "https://search.brave.com/search?q=hi");
    }
}
