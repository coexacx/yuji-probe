use super::*;
use std::time::Instant;
const PROCESS_SCRIPT: &str = r#"export LC_ALL=C
printf 'META '; getconf CLK_TCK
printf 'PAGE '; getconf PAGESIZE
awk '/^MemTotal:/ {print "MEM " $2}' /proc/meminfo
probe_count=0
for probe_stat in /proc/[0-9]*/stat; do
  [ "$probe_count" -lt 4096 ] || break
  IFS= read -r probe_line < "$probe_stat" 2>/dev/null || continue
  printf '%s\n' "$probe_line"
  probe_count=$((probe_count + 1))
done
"#;
#[derive(Default)]
pub(super) struct Inspector {
    previous: HashMap<(u32, u64), u64>,
    sampled: Option<Instant>,
}
pub(super) fn action(s: &str) -> bool {
    matches!(s, "processes" | "services" | "service_logs")
}
fn valid_unit(unit: &str) -> bool {
    !unit.starts_with('-')
        && unit.ends_with(".service")
        && unit.len() <= 200
        && unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._@:-".contains(&b))
}
#[derive(Debug)]
struct Process {
    pid: u32,
    start: u64,
    ticks: u64,
    rss: u64,
    name: String,
}
fn process(line: &str, page: u64) -> Option<Process> {
    let (pid, tail) = line.split_once(" (")?;
    let end = tail.rfind(") ")?;
    let name = &tail[..end];
    let fields: Vec<_> = tail[end + 2..].split_ascii_whitespace().collect();
    if fields.len() < 22 || name.chars().any(char::is_control) {
        return None;
    }
    let rss = fields[21].parse::<i64>().ok()?.max(0) as u64;
    Some(Process {
        pid: pid.parse().ok()?,
        start: fields[19].parse().ok()?,
        ticks: fields[11]
            .parse::<u64>()
            .ok()?
            .saturating_add(fields[12].parse().ok()?),
        rss: rss.saturating_mul(page),
        name: name.chars().take(100).collect(),
    })
}
impl Inspector {
    pub(super) async fn handle(
        &mut self,
        client: &ssh::Client,
        r: &Request,
    ) -> Result<Value, Problem> {
        if r.action == "processes" {
            if !["", "cpu", "memory"].contains(&r.target.as_str()) {
                return Err(error("invalid", "进程排序方式不正确"));
            }
            if self
                .sampled
                .is_some_and(|t| t.elapsed() < Duration::from_millis(900))
            {
                return Err(error("rate", "请稍后刷新进程信息"));
            }
            let raw = ssh::exec(client, "/bin/sh -s", PROCESS_SCRIPT.as_bytes(), 1024 * 1024)
                .await
                .map_err(|_| error("unavailable", "无法读取进程信息，请检查当前 SSH 用户权限"))?;
            let mut hz = 100.;
            let mut page = 4096;
            let mut memory = 0u64;
            let mut rows = Vec::new();
            let mut next = HashMap::new();
            let at = Instant::now();
            let elapsed = self.sampled.map(|t| at.duration_since(t).as_secs_f64());
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix("META ") {
                    hz = v
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| *v > 0. && *v < 100000.)
                        .unwrap_or(100.);
                    continue;
                }
                if let Some(v) = line.strip_prefix("PAGE ") {
                    page = v
                        .trim()
                        .parse::<u64>()
                        .ok()
                        .filter(|v| *v > 0 && *v <= 65536)
                        .unwrap_or(4096);
                    continue;
                }
                if let Some(v) = line.strip_prefix("MEM ") {
                    memory = v.trim().parse::<u64>().unwrap_or(0).saturating_mul(1024);
                    continue;
                }
                if let Some(p) = process(line, page) {
                    if next.len() >= 4096 {
                        break;
                    }
                    let cpu = elapsed.and_then(|seconds| {
                        self.previous
                            .get(&(p.pid, p.start))
                            .filter(|ticks| p.ticks >= **ticks)
                            .map(|ticks| {
                                ((p.ticks - *ticks) as f64 / hz / seconds * 100.).min(100000.)
                            })
                    });
                    next.insert((p.pid, p.start), p.ticks);
                    rows.push(json!({"pid":p.pid,"name":p.name,"cpu":cpu,"memory":p.rss,"memoryPercent":if memory>0 {p.rss as f64/memory as f64*100.}else{0.}}));
                }
            }
            self.previous = next;
            self.sampled = Some(at);
            rows.sort_by(|a, b| {
                if r.target == "memory" {
                    b["memory"].as_u64().cmp(&a["memory"].as_u64())
                } else {
                    b["cpu"]
                        .as_f64()
                        .unwrap_or(0.)
                        .total_cmp(&a["cpu"].as_f64().unwrap_or(0.))
                        .then(b["memory"].as_u64().cmp(&a["memory"].as_u64()))
                }
            });
            let total = rows.len();
            rows.truncate(100);
            return Ok(
                json!({"processes":rows,"sampled":elapsed.is_some(),"total":total,"at":now(),"limited":total>=4096}),
            );
        }
        if r.action == "services" {
            let raw=ssh::exec(client,"LC_ALL=C SYSTEMD_COLORS=0 /usr/bin/systemctl list-units --type=service --all --no-legend --no-pager --plain",&[],256*1024).await
                .map_err(|_|error("unavailable","无法读取 systemd 服务，请检查系统支持与 SSH 用户权限"))?;
            let rows:Vec<_>=raw.lines().filter_map(|line|{
                let mut fields=line.split_whitespace();
                let unit=fields.next()?;if !valid_unit(unit){return None;}
                Some(json!({"unit":unit,"load":fields.next()?,"active":fields.next()?,"state":fields.next()?,"description":fields.collect::<Vec<_>>().join(" ").chars().take(200).collect::<String>()}))
            }).take(1000).collect();
            return Ok(json!({"services":rows,"at":now()}));
        }
        if r.action == "service_logs" {
            if !valid_unit(&r.target) {
                return Err(error("invalid", "服务名称不正确"));
            }
            let cmd = format!(
                "LC_ALL=C SYSTEMD_COLORS=0 /usr/bin/journalctl --no-pager --quiet --output=short-iso --lines=100 --unit={}",
                ssh::quote(&r.target)
            );
            let raw = ssh::exec(client, &cmd, &[], 256 * 1024)
                .await
                .map_err(|_| {
                    error(
                        "unavailable",
                        "无法读取日志，可能缺少 journal 权限或日志超过限制",
                    )
                })?;
            return Ok(json!({"unit":r.target,"text":raw,"at":now()}));
        }
        Err(error("invalid", "不支持的运行状态请求"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_names_reject_shell_and_option_injection() {
        for s in ["nginx.service", "ssh@22.service", "docker.service"] {
            assert!(valid_unit(s));
        }
        for s in [
            "--all.service",
            "ssh.service;id",
            "$(id).service",
            "a\n.service",
            "a/../b.service",
        ] {
            assert!(!valid_unit(s));
        }
    }
    #[test]
    fn proc_stat_handles_names_with_spaces_and_parentheses() {
        let mut fields = vec!["0"; 30];
        fields[0] = "S";
        fields[11] = "120";
        fields[12] = "20";
        fields[19] = "100";
        fields[21] = "5";
        let line = format!("42 (test ) name) {}", fields.join(" "));
        let p = process(&line, 4096).unwrap();
        assert_eq!((p.pid, p.start, p.ticks, p.rss), (42, 100, 140, 20480));
        assert_eq!(p.name, "test ) name");
    }
}
