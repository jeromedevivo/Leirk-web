//! Bridge between the "Leirk" DevTools panel (an extension page) and the browser.
//! A loopback HTTP endpoint on a random port, guarded by a 128-bit secret only the extension's own files contain.
//! Web pages cannot read extension files, so they cannot learn the token; requests also have to come from an
//! extension origin and name the loopback host (no DNS rebinding).
use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

pub(crate) struct DevApi {
    pub port: u16,
    pub token: String,
}

/// 128 random bits from the OS generator, hex-encoded.
fn secret() -> String {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    let mut buf = [0u8; 16];
    let ok = unsafe { BCryptGenRandom(None, &mut buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG).is_ok() };
    assert!(ok, "system RNG unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time comparison, so the token cannot be guessed byte by byte from response timing.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct Request {
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(s: &mut TcpStream) -> Option<Request> {
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let method = lines.next()?.split(' ').next()?.to_string();
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if len > 64 * 1024 {
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Some(Request { method, headers, body })
}

fn respond(s: &mut TcpStream, status: &str, origin: &str, body: &str) {
    let cors = if origin.is_empty() {
        String::new()
    } else {
        format!("Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Headers: content-type, x-leirk-token\r\nAccess-Control-Allow-Methods: POST\r\nVary: Origin\r\n")
    };
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

impl DevApi {
    /// Starts the endpoint; every accepted command is handed to the event loop, which replies with the outbox.
    pub(crate) fn start(proxy: EventLoopProxy<Ev>) -> Option<DevApi> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
        let port = listener.local_addr().ok()?.port();
        let token = secret();
        let (tok, host) = (token.clone(), format!("127.0.0.1:{port}"));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (proxy, tok, host) = (proxy.clone(), tok.clone(), host.clone());
                std::thread::spawn(move || handle(stream, &proxy, &tok, &host));
            }
        });
        Some(DevApi { port, token })
    }
}

fn handle(mut s: TcpStream, proxy: &EventLoopProxy<Ev>, token: &str, host: &str) {
    let Some(req) = read_request(&mut s) else { return };
    let h = |k: &str| req.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap_or("");
    let origin = h("origin");
    // only the DevTools extension, only on the loopback name it was given
    if !origin.starts_with("chrome-extension://") || h("host") != host {
        return respond(&mut s, "403 Forbidden", "", "{}");
    }
    if req.method == "OPTIONS" {
        return respond(&mut s, "204 No Content", origin, "");
    }
    if req.method != "POST" || !same(h("x-leirk-token"), token) {
        return respond(&mut s, "403 Forbidden", origin, "{}");
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else { return respond(&mut s, "400 Bad Request", origin, "{}") };
    let (tx, rx) = mpsc::channel();
    let cmd = msg["c"].as_str().unwrap_or("poll").to_string();
    let nonce = msg["tab"].as_str().unwrap_or("").to_string();
    if proxy.send_event(Ev::DevApi(cmd, msg["v"].clone(), nonce, tx)).is_err() {
        return respond(&mut s, "503 Service Unavailable", origin, "{}");
    }
    let reply = rx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| json!({"outbox": []}));
    respond(&mut s, "200 OK", origin, &reply.to_string());
}

/// Writes the token and port next to the panel files; only extension pages can load them.
pub(crate) fn write_config(ext_dir: &Path, api: &DevApi) {
    let _ = std::fs::write(ext_dir.join("config.js"), format!("const LEIRK = {{ port: {}, token: \"{}\" }};\n", api.port, api.token));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_random_and_compared_in_constant_time() {
        let (a, b) = (secret(), secret());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(same(&a, &a.clone()));
        assert!(!same(&a, &b) && !same(&a, &a[..31]));
    }
}
