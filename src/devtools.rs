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
    todo!()
}

/// Listening TCP ports from `netstat -ano -p tcp`, named with `tasklist /fo csv /nh`.
pub fn parse_ports(netstat: &str, tasklist: &str) -> Vec<Port> {
    todo!()
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
