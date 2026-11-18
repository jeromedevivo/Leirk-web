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
            Container { name: field(v, "Names"), image: field(v, "Image"), state: field(v, "State"), status: field(v, "Status"), ports, cpu, mem, id }
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
