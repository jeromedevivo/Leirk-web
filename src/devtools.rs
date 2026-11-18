//! Developer mode back end: parse what Docker and Windows report about containers and listening ports.
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Debug, PartialEq, Default)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    /// Host ports published by the container (clickable as http://localhost:port).
    pub ports: Vec<u16>,
    pub cpu: String,
    pub mem: String,
    /// Docker Compose project and service (empty for a standalone container).
    pub project: String,
    pub service: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Port {
    pub port: u16,
    pub pid: u32,
    pub process: String,
}

/// `docker ps -a --format "{{json .}}"` output, merged with `docker stats --no-stream --format "{{json .}}"`.
pub fn parse_docker(ps: &str, stats: &str) -> Vec<Container> {
    let json = |s: &str| s.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok()).collect::<Vec<_>>();
    let field = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    let usage: HashMap<String, (String, String)> = json(stats)
        .iter()
        .map(|v| (field(v, "ID"), (field(v, "CPUPerc"), field(v, "MemUsage").split(" / ").next().unwrap_or("").to_string())))
        .collect();
    json(ps)
        .iter()
        .map(|v| {
            let id = field(v, "ID");
            let mut ports: Vec<u16> = vec![];
            for p in field(v, "Ports").split(", ") {
                if let Some(port) = p.split("->").next().filter(|_| p.contains("->")).and_then(|h| h.rsplit(':').next()?.parse().ok()) {
                    if !ports.contains(&port) {
                        ports.push(port);
                    }
                }
            }
            let (cpu, mem) = usage.get(&id).cloned().unwrap_or_default();
            let labels = field(v, "Labels");
            let label = |k: &str| labels.split(',').find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).unwrap_or("").to_string();
            let (project, service) = (label("com.docker.compose.project"), label("com.docker.compose.service"));
            Container { name: field(v, "Names"), image: field(v, "Image"), state: field(v, "State"), status: field(v, "Status"), ports, cpu, mem, id, project, service }
        })
        .collect()
}

/// Listening TCP ports from `netstat -ano -p tcp`, named with `tasklist /fo csv /nh`.
pub fn parse_ports(netstat: &str, tasklist: &str) -> Vec<Port> {
    let names: HashMap<u32, String> = tasklist
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim().trim_matches('"').split("\",\"").collect();
            Some((f.get(1)?.parse().ok()?, f.first()?.to_string()))
        })
        .collect();
    let mut out: Vec<Port> = vec![];
    for line in netstat.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 || f[0] != "TCP" || !matches!(f[3], "LISTENING" | "ÉCOUTE" | "ECOUTE") {
            continue;
        }
        let (Some(port), Ok(pid)) = (f[1].rsplit(':').next().and_then(|p| p.parse::<u16>().ok()), f[4].parse::<u32>()) else { continue };
        if !out.iter().any(|p| p.port == port) {
            out.push(Port { port, pid, process: names.get(&pid).cloned().unwrap_or_default() });
        }
    }
    out.sort_by_key(|p| p.port);
    out
}

/// True when localhost:`port` serves a site: an HTTP answer to "/" that is not an error page.
/// Databases do not speak HTTP; app RPC ports and Windows services answer "/" with 404 or 503.
pub fn speaks_http(port: u16) -> bool {
    use std::io::{Read, Write};
    let t = std::time::Duration::from_millis(400);
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, port).into(), t) else { return false };
    let _ = (s.set_read_timeout(Some(t)), s.set_write_timeout(Some(t)));
    let mut buf = [0u8; 12]; // "HTTP/1.1 200"
    if s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").is_err() || s.read_exact(&mut buf).is_err() || !buf.starts_with(b"HTTP/") {
        return false;
    }
    let code: u16 = std::str::from_utf8(&buf[9..12]).ok().and_then(|c| c.parse().ok()).unwrap_or(0);
    (200..400).contains(&code) || code == 401 || code == 403
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_ps_and_stats() {
        let ps = r#"{"ID":"a1b2","Names":"web","Image":"nginx:1.25","State":"running","Status":"Up 2 hours","Ports":"0.0.0.0:8080->80/tcp, :::8080->80/tcp, 0.0.0.0:8443->443/tcp"}
{"ID":"c3d4","Names":"db","Image":"postgres:16","State":"exited","Status":"Exited (0) 3 days ago","Ports":""}
not json"#;
        let stats = r#"{"ID":"a1b2","Name":"web","CPUPerc":"0.12%","MemUsage":"12.5MiB / 7.6GiB"}"#;
        let c = parse_docker(ps, stats);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "web");
        assert_eq!(c[0].ports, vec![8080, 8443], "deduplicated host ports");
        assert_eq!(c[0].cpu, "0.12%");
        assert_eq!(c[0].mem, "12.5MiB");
        assert_eq!(c[1].state, "exited");
        assert!(c[1].ports.is_empty());
    }

    #[test]
    fn compose_project_and_service_from_labels() {
        let ps = r#"{"ID":"e5","Names":"shop-api-1","Image":"node:20","State":"running","Status":"Up","Ports":"","Labels":"com.docker.compose.project=shop,com.docker.compose.service=api,maintainer=x"}
{"ID":"f6","Names":"lonely","Image":"redis","State":"exited","Status":"Exited","Ports":"","Labels":""}"#;
        let c = parse_docker(ps, "");
        assert_eq!((c[0].project.as_str(), c[0].service.as_str()), ("shop", "api"));
        assert_eq!((c[1].project.as_str(), c[1].service.as_str()), ("", ""));
    }

    #[test]
    fn only_ports_that_answer_http_are_web() {
        use std::io::{Read, Write};
        let web = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let raw = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (wp, rp) = (web.local_addr().unwrap().port(), raw.local_addr().unwrap().port());
        std::thread::spawn(move || {
            let (mut s, _) = web.accept().unwrap();
            let _ = s.read(&mut [0u8; 256]);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        });
        std::thread::spawn(move || {
            let (mut s, _) = raw.accept().unwrap();
            let _ = s.write_all(b"\x00\x00\x00\x0aN"); // e.g. a database greeting
        });
        // an app's local RPC answers HTTP too, but 404 on "/": not a site
        let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let ap = api.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = api.accept().unwrap();
            let _ = s.read(&mut [0u8; 256]);
            let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n");
        });
        assert!(speaks_http(wp));
        assert!(!speaks_http(rp));
        assert!(!speaks_http(ap));
    }

    #[test]
    fn listening_ports_with_process_names() {
        let netstat = "
Connexions actives

  Proto  Adresse locale         Adresse distante       État
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1184
  TCP    127.0.0.1:5173         0.0.0.0:0              LISTENING       9001
  TCP    [::]:5173              [::]:0                 LISTENING       9001
  TCP    192.168.1.5:50000      1.2.3.4:443            ESTABLISHED     4242
  TCP    0.0.0.0:5432           0.0.0.0:0              LISTENING       7000
";
        let tasklist = "\"svchost.exe\",\"1184\",\"Services\",\"0\",\"9 000 Ko\"\n\"node.exe\",\"9001\",\"Console\",\"1\",\"80 000 Ko\"\n\"postgres.exe\",\"7000\",\"Services\",\"0\",\"20 000 Ko\"\n";
        let p = parse_ports(netstat, tasklist);
        assert_eq!(
            p,
            vec![
                Port { port: 135, pid: 1184, process: "svchost.exe".into() },
                Port { port: 5173, pid: 9001, process: "node.exe".into() },
                Port { port: 5432, pid: 7000, process: "postgres.exe".into() },
            ]
        );
    }
}
