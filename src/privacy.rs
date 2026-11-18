//! URL privacy rewrites applied to top-level navigations (Brave's query filter and HTTPS upgrade).

/// Query parameters that only exist to track clicks (Brave's query filter list, condensed).
const TRACKING: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "mc_eid", "mc_cid", "yclid", "_openstat",
    "igshid", "igsh", "twclid", "ttclid", "li_fat_id", "mkt_tok", "oly_anon_id", "oly_enc_id", "rb_clickid",
    "s_cid", "vero_conv", "vero_id", "wickedid", "_hsenc", "_hsmi", "__hssc", "__hstc", "__hsfp", "hsCtaTracking",
    "ck_subscriber_id", "srsltid", "si", "ref_src", "ref_url",
];

/// URL without tracking parameters (`utm_*` and the list above), or `None` when nothing changed.
pub fn strip_tracking(url: &str) -> Option<String> {
    let (rest, frag) = match url.split_once('#') {
        Some((r, f)) => (r, Some(f)),
        None => (url, None),
    };
    let (base, query) = rest.split_once('?')?;
    let params: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
    let kept: Vec<&str> = params
        .iter()
        .copied()
        .filter(|p| {
            let name = p.split('=').next().unwrap_or("");
            !name.starts_with("utm_") && !TRACKING.contains(&name)
        })
        .collect();
    if kept.len() == params.len() {
        return None;
    }
    let mut out = base.to_string();
    if !kept.is_empty() {
        out += "?";
        out += &kept.join("&");
    }
    if let Some(f) = frag {
        out += "#";
        out += f;
    }
    Some(out)
}

/// `https://` version of a plain-http URL, except for local hosts that rarely have certificates.
pub fn https_upgrade(url: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let host = crate::shields::host(url);
    let local = host == "localhost"
        || !host.contains('.')
        || host.starts_with('[')
        || host.parse::<std::net::Ipv4Addr>().is_ok()
        || [".local", ".lan", ".internal", ".home", ".test", ".localhost"].iter().any(|s| host.ends_with(s));
    (!local).then(|| format!("https://{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_tracking_params() {
        assert_eq!(
            strip_tracking("https://a.com/p?id=3&utm_source=x&fbclid=y#top").as_deref(),
            Some("https://a.com/p?id=3#top")
        );
        assert_eq!(strip_tracking("https://a.com/?utm_medium=a&gclid=b").as_deref(), Some("https://a.com/"));
        assert_eq!(strip_tracking("https://a.com/?id=3"), None);
        assert_eq!(strip_tracking("https://a.com/"), None);
        // a parameter merely containing a tracking name is kept
        assert_eq!(strip_tracking("https://a.com/?mysi=1"), None);
    }

    #[test]
    fn upgrades_public_http_only() {
        assert_eq!(https_upgrade("http://example.com/a").as_deref(), Some("https://example.com/a"));
        assert_eq!(https_upgrade("https://example.com/"), None);
        assert_eq!(https_upgrade("http://localhost:3000/"), None);
        assert_eq!(https_upgrade("http://192.168.1.10/"), None);
        assert_eq!(https_upgrade("http://nas.local/"), None);
        assert_eq!(https_upgrade("http://router/"), None);
    }
}
