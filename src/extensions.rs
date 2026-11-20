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

/// Prepended to every extension's background. Each WebView2 is a Chromium window of its own, so the extension popup
/// (or Leirk's chrome) would be "the current window": point currentWindow/lastFocusedWindow, captureVisibleTab(null)
/// and windows.getCurrent at the last focused window showing a web page, which is what Chrome's tab strip means.
const SHIM: &str = r#"/* leirk-shim 1 */(function(){var c=self.chrome;if(!c||!c.tabs||!c.windows)return;
var web=function(u){return /^(https?|file|ftp):/.test(u||'')},last=null;
var note=function(w){if(w==null||w<0)return;c.tabs.query({windowId:w,active:true},function(t){void c.runtime.lastError;if(t&&t[0]&&web(t[0].url))last=w})};
try{c.windows.onFocusChanged.addListener(note);c.tabs.onActivated.addListener(function(i){note(i.windowId)});c.tabs.onUpdated.addListener(function(id,ch,t){if(t&&t.active&&ch.url)note(t.windowId)})}catch(e){}
var q=c.tabs.query.bind(c.tabs),cap=c.tabs.captureVisibleTab&&c.tabs.captureVisibleTab.bind(c.tabs),get=c.windows.get.bind(c.windows),gc=c.windows.getCurrent.bind(c.windows),gl=c.windows.getLastFocused.bind(c.windows);
var win=function(){return new Promise(function(r){if(last!=null)return r(last);c.windows.getAll({populate:true},function(ws){void c.runtime.lastError;
 var m=(ws||[]).filter(function(w){return (w.tabs||[]).some(function(t){return t.active&&web(t.url)})});var f=m.filter(function(w){return w.focused});r((f[0]||m[0]||{}).id)})})};
var cb=function(p,f){if(typeof f==='function'){p.then(function(v){f(v)});return}return p};
c.tabs.query=function(info,f){info=Object.assign({},info||{});if(!info.currentWindow&&!info.lastFocusedWindow)return cb(new Promise(function(r){q(info,r)}),f);
 delete info.currentWindow;delete info.lastFocusedWindow;return cb(win().then(function(w){if(w!=null)info.windowId=w;return new Promise(function(r){q(info,r)})}),f)};
if(cap)c.tabs.captureVisibleTab=function(w,o,f){if(typeof w!=='number'&&w!=null){f=o;o=w;w=null}if(typeof o==='function'){f=o;o={}}
 return cb((w==null?win():Promise.resolve(w)).then(function(id){return new Promise(function(r){cap(id,o||{},r)})}),f)};
var cur=function(orig){return function(o,f){if(typeof o==='function'){f=o;o={}}return cb(win().then(function(id){return new Promise(function(r){id==null?orig(o||{},r):get(id,o||{},r)})}),f)}};
c.windows.getCurrent=cur(gc);c.windows.getLastFocused=cur(gl);
})();/* /leirk-shim */
"#;

/// Puts [`SHIM`] in front of the extension's background (service worker or MV2 scripts). Idempotent; a newer shim
/// replaces an older one. False when the extension has no background or its files cannot be written.
pub fn patch_background(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("manifest.json")) else { return false };
    let Ok(mut manifest) = serde_json::from_str::<Value>(&text) else { return false };
    let bg = &manifest["background"];
    if let Some(sw) = bg["service_worker"].as_str() {
        let path = dir.join(sw.trim_start_matches('/'));
        let Ok(js) = std::fs::read_to_string(&path) else { return false };
        let rest = js.split_once("/* /leirk-shim */\n").map(|(_, r)| r).unwrap_or(&js);
        let patched = format!("{SHIM}{rest}");
        return patched == js || std::fs::write(&path, patched).is_ok();
    }
    if bg["scripts"].is_array() {
        if std::fs::write(dir.join("leirk-shim.js"), SHIM).is_err() {
            return false;
        }
        let list = manifest["background"]["scripts"].as_array_mut().unwrap();
        if list.first().and_then(Value::as_str) != Some("leirk-shim.js") {
            list.insert(0, Value::from("leirk-shim.js"));
            return std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default()).is_ok();
        }
        return true;
    }
    false
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
    fn background_shim_is_prepended_once() {
        let dir = std::env::temp_dir().join(format!("leirk-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), r#"{"background":{"service_worker":"/bg.js"}}"#).unwrap();
        std::fs::write(dir.join("bg.js"), "work();\n").unwrap();
        assert!(patch_background(&dir) && patch_background(&dir));
        let js = std::fs::read_to_string(dir.join("bg.js")).unwrap();
        assert_eq!(js.matches("leirk-shim 1").count(), 1);
        assert!(js.ends_with("/* /leirk-shim */\nwork();\n"));
        // an older shim is swapped for the current one
        std::fs::write(dir.join("bg.js"), "/* leirk-shim 0 */old();/* /leirk-shim */\nwork();\n").unwrap();
        assert!(patch_background(&dir));
        assert!(!std::fs::read_to_string(dir.join("bg.js")).unwrap().contains("old()"));
        // MV2: a script listed first
        std::fs::write(dir.join("manifest.json"), r#"{"background":{"scripts":["a.js"]}}"#).unwrap();
        assert!(patch_background(&dir) && patch_background(&dir));
        let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(m["background"]["scripts"], serde_json::json!(["leirk-shim.js", "a.js"]));
        std::fs::write(dir.join("manifest.json"), r#"{"name":"no background"}"#).unwrap();
        assert!(!patch_background(&dir));
        let _ = std::fs::remove_dir_all(&dir);
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
