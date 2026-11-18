//! Chrome extensions: Web Store ids and links, CRX packages, manifests.
use serde_json::Value;
use std::path::Path;

/// The 32-letter (a–p) id from a bare id or a Chrome Web Store link.
pub fn store_id(input: &str) -> Option<String> {
    let input = input.trim();
    let is_id = |s: &str| s.len() == 32 && s.bytes().all(|b| (b'a'..=b'p').contains(&b));
    if is_id(input) {
        return Some(input.to_string());
    }
    let host_ok = input.contains("chromewebstore.google.com/") || input.contains("chrome.google.com/webstore/");
    host_ok.then(|| input.split(['/', '?', '#']).find(|p| is_id(p)).map(String::from)).flatten()
}

/// Google's update endpoint, which redirects to the signed CRX of the latest version compatible with
/// `engine` (the Chromium version; the store answers 204 No Content to versions it considers too old).
pub fn crx_url(id: &str, engine: &str) -> String {
    let v = if engine.split('.').next().and_then(|m| m.parse::<u32>().ok()).is_some() { engine } else { "150.0.0.0" };
    format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion={v}&acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc")
}

/// The zip archive inside a CRX (versions 2 and 3), or None when the bytes are not a CRX.
pub fn crx_zip(crx: &[u8]) -> Option<&[u8]> {
    let u32_at = |i: usize| crx.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if crx.get(..4)? != b"Cr24" {
        return None;
    }
    let start = match u32_at(4)? {
        2 => 16 + u32_at(8)? + u32_at(12)?,
        3 => 12 + u32_at(8)?,
        _ => return None,
    };
    crx.get(start..).filter(|z| z.starts_with(b"PK"))
}

/// Display information read from an unpacked extension folder.
#[derive(Debug, PartialEq, Default)]
pub struct Info {
    pub name: String,
    pub version: String,
    /// Popup page relative to the folder ("" when the extension has none).
    pub popup: String,
    /// Largest declared icon, relative to the folder.
    pub icon: String,
}

pub fn read_info(dir: &Path) -> Option<Info> {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    let action = if manifest["action"].is_object() { &manifest["action"] } else { &manifest["browser_action"] };
    let icon = [&manifest["icons"], &action["default_icon"]]
        .iter()
        .filter_map(|v| v.as_object())
        .flat_map(|o| o.iter())
        .max_by_key(|(size, _)| size.parse::<u32>().unwrap_or(0))
        .and_then(|(_, p)| p.as_str())
        .unwrap_or("")
        .trim_start_matches('/')
        .to_string();
    let name = localize(dir, &manifest, manifest["name"].as_str().unwrap_or(""));
    Some(Info { name, version: manifest["version"].as_str().unwrap_or("").into(), popup: action["default_popup"].as_str().unwrap_or("").trim_start_matches('/').into(), icon })
}

/// Resolves "__MSG_key__" names from the extension's default locale.
fn localize(dir: &Path, manifest: &Value, text: &str) -> String {
    let Some(key) = text.strip_prefix("__MSG_").and_then(|t| t.strip_suffix("__")) else { return text.to_string() };
    let locale = manifest["default_locale"].as_str().unwrap_or("en");
    ["fr", locale]
        .iter()
        .filter_map(|l| std::fs::read_to_string(dir.join("_locales").join(l).join("messages.json")).ok())
        .filter_map(|s| serde_json::from_str::<Value>(&s).ok())
        .find_map(|m| m.as_object()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).and_then(|(_, v)| v["message"].as_str().map(String::from)))
        .unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BITWARDEN: &str = "nngceckbapebfimnlniiiahkandclblb";

    #[test]
    fn ids_from_store_links_or_bare() {
        assert_eq!(store_id(BITWARDEN).as_deref(), Some(BITWARDEN));
        let link = format!("https://chromewebstore.google.com/detail/bitwarden-password-manager/{BITWARDEN}?hl=fr");
        assert_eq!(store_id(&link).as_deref(), Some(BITWARDEN));
        assert_eq!(store_id(&format!("https://chrome.google.com/webstore/detail/x/{BITWARDEN}")).as_deref(), Some(BITWARDEN));
        assert_eq!(store_id(&format!("https://evil.example/{BITWARDEN}")), None, "only the store's own links");
        assert_eq!(store_id("not an id"), None);
        let url = crx_url(BITWARDEN, "157.0.4341.46");
        assert!(url.starts_with("https://clients2.google.com/") && url.contains(BITWARDEN) && url.contains("prodversion=157.0.4341.46"));
        assert!(crx_url(BITWARDEN, "").contains("prodversion=150.0.0.0"), "a recent version when the engine's is unknown");
    }

    #[test]
    fn zip_inside_crx2_and_crx3() {
        let zip = b"PK\x03\x04rest";
        let mut crx3 = b"Cr24".to_vec();
        crx3.extend(3u32.to_le_bytes());
        crx3.extend(5u32.to_le_bytes());
        crx3.extend(b"HEADR");
        crx3.extend(zip);
        assert_eq!(crx_zip(&crx3), Some(&zip[..]));
        let mut crx2 = b"Cr24".to_vec();
        crx2.extend(2u32.to_le_bytes());
        crx2.extend(2u32.to_le_bytes());
        crx2.extend(3u32.to_le_bytes());
        crx2.extend(b"KKSSS");
        crx2.extend(zip);
        assert_eq!(crx_zip(&crx2), Some(&zip[..]));
        assert_eq!(crx_zip(b"PK\x03\x04plain zip"), None);
        assert_eq!(crx_zip(b"Cr24\x03\x00\x00\x00\xff\xff\xff\x7f"), None, "header longer than the file");
    }

    #[test]
    fn manifest_info_with_localized_name() {
        let dir = std::env::temp_dir().join(format!("leirk-ext-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("_locales/en")).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"name":"__MSG_extName__","version":"2026.1","default_locale":"en","icons":{"16":"i16.png","128":"/images/i128.png"},"action":{"default_popup":"popup/index.html"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("_locales/en/messages.json"), r#"{"extName":{"message":"Bitwarden Password Manager"}}"#).unwrap();
        let info = read_info(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(info, Info { name: "Bitwarden Password Manager".into(), version: "2026.1".into(), popup: "popup/index.html".into(), icon: "images/i128.png".into() });
    }
}
