use crate::{core::*, model::Node, operations::Policy};
use chrono::{Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub minute: Vec<Point>,
    pub quarter: Vec<Point>,
    pub incidents: Vec<Incident>,
    pub last_at: i64,
    pub uptime: u64,
    pub boot: String,
    pub online: bool,
    pub observed: bool,
    pub period: String,
    pub rx: u64,
    pub tx: u64,
    pub previous: HashMap<String, (u64, u64)>,
    pub period_history: Vec<Period>,
    pub policy_revision: String,
    pub minute_at: i64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Period {
    pub start: String,
    pub rx: u64,
    pub tx: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Incident {
    pub at: i64,
    pub online: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Point {
    pub at: i64,
    pub cpu: f64,
    pub memory: f64,
    pub disk: f64,
    pub rx: f64,
    pub tx: f64,
    pub online: bool,
}
pub fn cycle(at: i64, day: u32) -> String {
    let now = Utc.timestamp_opt(at, 0).single().unwrap_or_else(Utc::now);
    let day = day.clamp(1, 28);
    let (year, month) = if now.day() >= day {
        (now.year(), now.month())
    } else if now.month() == 1 {
        (now.year() - 1, 12)
    } else {
        (now.year(), now.month() - 1)
    };
    format!("{year:04}-{month:02}-{day:02}")
}
pub fn update(t: &mut Track, n: &Node, m: &vistart_probe_agent::wire::Metrics, at: i64) {
    let policy = &n.policy;
    let current = cycle(at, policy.billing_day);
    if t.period != current || t.policy_revision != policy.traffic_revision {
        if !t.period.is_empty() {
            t.period_history.push(Period {
                start: t.period.clone(),
                rx: t.rx,
                tx: t.tx,
            });
        }
        if t.period_history.len() > 36 {
            t.period_history.remove(0);
        }
        t.period = current;
        t.rx = 0;
        t.tx = 0;
        t.previous.clear();
        t.policy_revision = policy.traffic_revision.clone();
    }
    let reboot =
        m.uptime < t.uptime || (!m.boot_id.is_empty() && !t.boot.is_empty() && m.boot_id != t.boot);
    let mut next = HashMap::new();
    for nic in &m.network {
        if !included(policy, nic) {
            continue;
        }
        if let Some((rx, tx)) = t.previous.get(&nic.name) {
            t.rx = t.rx.saturating_add(if reboot || nic.rx_bytes < *rx {
                nic.rx_bytes
            } else {
                nic.rx_bytes - *rx
            });
            t.tx = t.tx.saturating_add(if reboot || nic.tx_bytes < *tx {
                nic.tx_bytes
            } else {
                nic.tx_bytes - *tx
            });
        }
        next.insert(nic.name.clone(), (nic.rx_bytes, nic.tx_bytes));
    }
    t.previous = next;
    t.uptime = m.uptime;
    t.boot = m.boot_id.clone();
    if at / 60 > t.minute_at / 60 {
        let network = m.network.iter().filter(|nic| included(policy, nic));
        let mut point = Point {
            at: at / 60 * 60,
            cpu: m.cpu,
            memory: 100.0 * m.memory_used as f64 / m.memory_total.max(1) as f64,
            disk: m
                .volumes
                .iter()
                .map(|d| 100.0 * d.used as f64 / d.total.max(1) as f64)
                .fold(0.0, f64::max),
            online: true,
            ..Default::default()
        };
        for nic in network {
            point.rx += nic.rx_rate.unwrap_or(0.0);
            point.tx += nic.tx_rate.unwrap_or(0.0);
        }
        t.minute.push(point.clone());
        t.minute_at = at;
        t.minute.retain(|v| v.at >= at - 86400);
        if t.quarter.last().is_none_or(|p| p.at / 900 < at / 900) {
            t.quarter.push(point);
        }
        t.quarter.retain(|v| v.at >= at - 30 * 86400);
    }
    status(t, true, at);
}
fn included(p: &Policy, n: &vistart_probe_agent::wire::NetworkInterface) -> bool {
    if !p.interfaces.is_empty() {
        p.interfaces.iter().any(|v| v == &n.name)
    } else {
        !n.r#virtual && n.name != "lo"
    }
}
pub fn status(t: &mut Track, online: bool, at: i64) {
    if !t.observed || t.online != online {
        t.incidents.push(Incident { at, online });
        t.observed = true;
        t.online = online;
        if t.incidents.len() > 2000 {
            t.incidents.remove(0);
        }
    }
    t.last_at = at;
    if !online && at / 60 > t.minute_at / 60 {
        t.minute.push(Point {
            at: at / 60 * 60,
            ..Default::default()
        });
        t.minute_at = at;
        t.minute.retain(|p| p.at >= at - 86400);
        if t.quarter.last().is_none_or(|p| p.at / 900 < at / 900) {
            t.quarter.push(Point {
                at: at / 900 * 900,
                ..Default::default()
            });
        }
        t.quarter.retain(|p| p.at >= at - 30 * 86400);
    }
}
pub fn downtime(t: &Track, start: i64, end: i64) -> i64 {
    let mut state = None;
    let mut cursor = start;
    let mut total = 0;
    for e in &t.incidents {
        if e.at <= start {
            state = Some(e.online);
            continue;
        }
        if e.at > end {
            break;
        }
        if state == Some(false) {
            total += e.at - cursor;
        }
        state = Some(e.online);
        cursor = e.at;
    }
    if state == Some(false) {
        total += end - cursor;
    }
    total.max(0)
}
pub fn load(dir: &std::path::Path) -> HashMap<String, Track> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir.join("history")) else {
        return out;
    };
    for e in entries.flatten().take(200) {
        let path = e.path();
        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !crate::operations::valid_id(id) {
            continue;
        }
        if let Ok(mut t) = read_json::<Track>(&path) {
            if t.minute.len() > 1500
                || t.quarter.len() > 3000
                || t.incidents.len() > 2000
                || t.previous.len() > 64
            {
                continue;
            }
            t.minute.retain(|v| v.at >= now() - 86400);
            t.quarter.retain(|v| v.at >= now() - 30 * 86400);
            out.insert(id.into(), t);
        }
    }
    out
}
// A generation prevents an older asynchronous snapshot from undoing restore/removal.
pub fn persist(
    dir: &std::path::Path,
    gate: &std::sync::Mutex<u64>,
    generation: u64,
    records: std::collections::HashMap<String, Track>,
) {
    let current = gate.lock().unwrap();
    if *current != generation {
        return;
    }
    let path = dir.join("history");
    let _ = std::fs::create_dir_all(&path);
    for (id, track) in records {
        if crate::operations::valid_id(&id) {
            let _ = crate::core::atomic_json(&path.join(format!("{id}.json")), &track);
        }
    }
}
pub fn remove(app: &crate::core::App, id: &str) {
    let mut generation = app.0.history_io.lock().unwrap();
    *generation = generation.wrapping_add(1);
    let _ = std::fs::remove_file(app.0.dir.join("history").join(format!("{id}.json")));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_snapshot_cannot_overwrite_restore_or_recreate_removed_history() {
        let dir = std::env::temp_dir().join(format!("yuji-history-test-{}", crate::core::token()));
        std::fs::create_dir_all(dir.join("history")).unwrap();
        let gate = std::sync::Mutex::new(0);
        let id = "0123456789abcdef0123".to_owned();
        let path = dir.join("history").join(format!("{id}.json"));
        let records = std::collections::HashMap::from([(id, Track::default())]);
        persist(&dir, &gate, 0, records.clone());
        assert!(path.exists());
        *gate.lock().unwrap() = 1;
        std::fs::write(&path, b"restored").unwrap();
        persist(&dir, &gate, 0, records.clone());
        assert_eq!(std::fs::read(&path).unwrap(), b"restored");
        std::fs::remove_file(&path).unwrap();
        persist(&dir, &gate, 0, records);
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn metrics() -> vistart_probe_agent::wire::Metrics {
        serde_json::from_value(serde_json::json!({
            "boot_id":"boot-a","network_available":true,"network":[
                {"name":"eth0","state":"up","default":true,"virtual":false,"rx_bytes":100,"tx_bytes":200,"rx_rate":10.0,"tx_rate":20.0},
                {"name":"docker0","state":"up","default":false,"virtual":true,"rx_bytes":999,"tx_bytes":999,"rx_rate":50.0,"tx_rate":50.0}
            ],"latency_probe":true,"cpu":25.0,"cpu_model":"test","cores":1,"arch":"amd64","system":"Debian",
            "memory_total":1000,"memory_used":400,"swap_total":0,"swap_used":0,"volumes":[],"uptime":100,
            "country":"US","public_ip":"","version":"0.2.0"
        })).unwrap()
    }
    #[test]
    fn traffic_counts_deltas_reboot_and_restored_baseline_without_double_counting() {
        let n = Node::default();
        let mut t = Track::default();
        let mut m = metrics();
        let at = chrono::DateTime::parse_from_rfc3339("2026-09-20T00:00:00Z")
            .unwrap()
            .timestamp();
        update(&mut t, &n, &m, at);
        assert_eq!((t.rx, t.tx), (0, 0));
        m.network[0].rx_bytes = 140;
        m.network[0].tx_bytes = 260;
        m.uptime = 110;
        update(&mut t, &n, &m, at + 10);
        assert_eq!((t.rx, t.tx), (40, 60));
        let raw = serde_json::to_vec(&t).unwrap();
        let mut t: Track = serde_json::from_slice(&raw).unwrap();
        update(&mut t, &n, &m, at + 20);
        assert_eq!((t.rx, t.tx), (40, 60));
        m.boot_id = "boot-b".into();
        m.uptime = 5;
        m.network[0].rx_bytes = 7;
        m.network[0].tx_bytes = 9;
        update(&mut t, &n, &m, at + 30);
        assert_eq!((t.rx, t.tx), (47, 69));
        m.network[0].rx_bytes = 3;
        m.network[0].tx_bytes = 4;
        update(&mut t, &n, &m, at + 40);
        assert_eq!((t.rx, t.tx), (50, 73));
        assert_eq!(t.previous.len(), 1);
    }
    #[test]
    fn traffic_new_cycle_and_interface_policy_start_fresh_baseline() {
        let mut n = Node::default();
        n.policy.billing_day = 15;
        let mut t = Track::default();
        let mut m = metrics();
        let at = chrono::DateTime::parse_from_rfc3339("2026-09-14T23:59:30Z")
            .unwrap()
            .timestamp();
        update(&mut t, &n, &m, at);
        m.network[0].rx_bytes += 25;
        update(&mut t, &n, &m, at + 10);
        assert_eq!(t.rx, 25);
        update(&mut t, &n, &m, at + 40);
        assert_eq!((t.period.as_str(), t.rx), ("2026-09-15", 0));
        assert_eq!(t.period_history[0].rx, 25);
        n.policy.interfaces = vec!["docker0".into()];
        n.policy.traffic_revision = "changed".into();
        update(&mut t, &n, &m, at + 50);
        m.network[1].rx_bytes += 55;
        m.network[0].rx_bytes += 1000;
        update(&mut t, &n, &m, at + 60);
        assert_eq!(t.rx, 55);
        assert_eq!(t.previous.len(), 1);
        n.policy.traffic_mode = "tx".into();
        assert_eq!(crate::operations::used(&n.policy, &t), t.tx);
        n.policy.traffic_mode = "rx".into();
        assert_eq!(crate::operations::used(&n.policy, &t), 55);
    }
    #[test]
    fn cycle_boundaries() {
        assert_eq!(
            cycle(
                chrono::DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
                    .unwrap()
                    .timestamp(),
                15
            ),
            "2025-12-15"
        );
    }
    #[test]
    fn outage_intervals() {
        let t = Track {
            incidents: vec![
                Incident {
                    at: 10,
                    online: true,
                },
                Incident {
                    at: 20,
                    online: false,
                },
                Incident {
                    at: 30,
                    online: true,
                },
            ],
            ..Default::default()
        };
        assert_eq!(downtime(&t, 0, 40), 10);
        assert_eq!(downtime(&t, 25, 28), 3);
    }
}
