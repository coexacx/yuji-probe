use crate::{auth, core::*, history, model::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, time::Duration};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Policy {
    pub terminal: bool,
    pub files: String,
    pub maintenance_until: i64,
    pub cpu: f64,
    pub memory: f64,
    pub disk: f64,
    pub sustained_seconds: i64,
    pub repeat_seconds: i64,
    pub billing_day: u32,
    pub quota_bytes: u64,
    pub traffic_percent: f64,
    pub traffic_mode: String,
    pub interfaces: Vec<String>,
    pub traffic_revision: String,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            terminal: true,
            files: "write".into(),
            maintenance_until: 0,
            cpu: 0.,
            memory: 0.,
            disk: 0.,
            sustained_seconds: 300,
            repeat_seconds: 3600,
            billing_day: 1,
            quota_bytes: 0,
            traffic_percent: 80.,
            traffic_mode: "both".into(),
            interfaces: vec![],
            traffic_revision: String::new(),
        }
    }
}
pub fn default_policy() -> Policy {
    Policy::default()
}
impl Policy {
    pub fn validate(&self) -> bool {
        [self.cpu, self.memory, self.disk, self.traffic_percent]
            .iter()
            .all(|x| x.is_finite() && *x >= 0. && *x <= 100.)
            && (10..=3600).contains(&self.sustained_seconds)
            && (300..=86400).contains(&self.repeat_seconds)
            && (1..=28).contains(&self.billing_day)
            && ["rx", "tx", "both"].contains(&self.traffic_mode.as_str())
            && ["off", "read", "write"].contains(&self.files.as_str())
            && self.interfaces.len() <= 64
            && self.interfaces.iter().all(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            })
            && self.maintenance_until >= 0
            && self.maintenance_until <= now() + 90 * 86400
            && self.quota_bytes < 1 << 60
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Alarm {
    pub since: i64,
    pub sent: i64,
    pub active: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSchedule {
    pub every_hours: u32,
    pub keep: usize,
    pub sealed_password: String,
    pub last: i64,
    pub error: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub schedule: BackupSchedule,
    pub alarms: HashMap<String, Alarm>,
    pub transfers: HashMap<String, crate::migration::Transfer>,
    pub retired: HashMap<String, NodeSecret>,
    pub removal_names: HashMap<String, String>,
    #[serde(skip)]
    pub tracks: HashMap<String, history::Track>,
    #[serde(skip)]
    pub busy: bool,
}
pub fn load(dir: &Path) -> Result<State, &'static str> {
    let mut state = match read_json(&dir.join("operations.json")) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
        Err(_) => return Err("operations state invalid"),
    };
    for t in state.transfers.values_mut() {
        if t.state == "running" {
            t.state = "retry".into();
            t.next = 0;
        }
    }
    state.tracks = history::load(dir);
    Ok(state)
}
pub fn save(app: &App, i: &Inner) -> ApiResult<()> {
    atomic_json(&app.0.dir.join("operations.json"), &i.ops).map_err(|_| ApiError::internal())
}
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
pub fn notify_login(app: &App, i: &mut Inner, ip: &str) {
    crate::telegram::queue_notice(app, i, &format!("管理员登录 · {ip}"), "security");
}
pub fn used(p: &Policy, t: &history::Track) -> u64 {
    match p.traffic_mode.as_str() {
        "rx" => t.rx,
        "tx" => t.tx,
        _ => t.rx.saturating_add(t.tx),
    }
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct Input {
    id: String,
    days: i64,
    policy: Option<Policy>,
    version: String,
}
pub async fn api(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    if c.path == "/api/migrate" {
        return crate::migration::receive(app, c, body).await;
    }
    if c.path.starts_with("/api/admin/ops/backup") || c.path == "/api/admin/ops/restore" {
        return crate::backup::api(app, c, body).await;
    }
    let v: Input = if body.is_empty() {
        Input::default()
    } else {
        decode(&body)?
    };
    if c.path == "/api/admin/ops/update-check" && c.method == "GET" {
        {
            let mut i = app.lock();
            app.guard(&mut i, &c, true, false)?;
            i.request("update-check", 10)?;
        }
        return crate::backup::check_release(&app).await;
    }
    let mut i = app.lock();
    app.guard(&mut i, &c, true, c.method != "GET")?;
    let action = c.path.strip_prefix("/api/admin/ops/").unwrap_or("");
    match (action, c.method.as_str()) {
        ("sessions", "GET") => {
            let rows:Vec<_>=i.sessions.values().filter(|s|s.auth&&now()-s.seen<=1800&&now()-s.created<28800)
 .map(|s|json!({"id":s.handle,"current":s.id==c.sid,"source":s.source,"device":s.device,"created":s.created,"seen":s.seen,"terminals":i.terminals.get(&s.id).map_or(0,|t|t.len())})).collect();
            Ok(ApiReply::ok(json!({"sessions":rows})))
        }
        ("sessions/revoke", "POST") => {
            auth::require_elevated(&i, &c)?;
            let ids: Vec<_> = i
                .sessions
                .values()
                .filter(|s| s.auth && ((v.id == "others" && s.id != c.sid) || s.handle == v.id))
                .map(|s| s.id.clone())
                .collect();
            for id in ids {
                i.revoke(&id);
            }
            app.record(&mut i, "sessions_revoked", &c.ip);
            Ok(ApiReply::ok(json!({"ok":true})))
        }
        ("recovery", "POST") => {
            auth::require_elevated(&i, &c)?;
            if i.auth.mfa.is_empty() {
                return Err(ApiError::new(409, "请先开启二步验证"));
            }
            let mut a = i.auth.clone();
            let codes = auth::new_recovery(&mut a);
            auth::save_auth(&app, &mut i, a)?;
            app.record(&mut i, "recovery_regenerated", &c.ip);
            Ok(ApiReply::ok(json!({"codes":codes})))
        }
        ("policy", "POST") => {
            auth::require_elevated(&i, &c)?;
            let mut policy = v.policy.ok_or_else(|| ApiError::new(400, "缺少节点策略"))?;
            if !policy.validate() {
                return Err(ApiError::new(400, "策略参数不正确"));
            }
            let mut data = i.data.clone();
            let n = data
                .nodes
                .iter_mut()
                .find(|n| n.public.id == v.id && !n.removing)
                .ok_or_else(|| ApiError::new(404, "节点不存在"))?;
            if n.policy.billing_day != policy.billing_day
                || n.policy.interfaces != policy.interfaces
            {
                policy.traffic_revision = token();
            } else {
                policy.traffic_revision = n.policy.traffic_revision.clone();
            }
            n.policy = policy;
            let name = n.public.name.clone();
            app.save_data(&mut i, data)?;
            // An existing shell can otherwise bypass newly narrowed file permissions.
            for terminals in i.terminals.values() {
                for stop in terminals.values() {
                    stop.cancel();
                }
            }
            crate::telegram::invalidate(&app, &mut i);
            app.record(&mut i, "node_policy_updated", &name);
            Ok(ApiReply::ok(json!({"ok":true})))
        }
        ("history", "POST") => {
            if ![1, 7, 30].contains(&v.days) || !i.data.nodes.iter().any(|n| n.public.id == v.id) {
                return Err(ApiError::new(400, "请指定节点与 1/7/30 天范围"));
            }
            let t = i.ops.tracks.get(&v.id).cloned().unwrap_or_default();
            let n = i.data.nodes.iter().find(|n| n.public.id == v.id).unwrap();
            let start = now() - v.days * 86400;
            let points: Vec<_> = if v.days == 1 { &t.minute } else { &t.quarter }
                .iter()
                .filter(|p| p.at >= start)
                .collect();
            Ok(ApiReply::ok(
                json!({"points":points,"incidents":t.incidents,"downtimeSeconds":history::downtime(&t,start,now()),"period":t.period,"rx":t.rx,"tx":t.tx,"used":used(&n.policy,&t),"quota":n.policy.quota_bytes,"periods":t.period_history,"policy":n.policy}),
            ))
        }
        ("status", "GET") => Ok(ApiReply::ok(
            json!({"version":VERSION,"agentVersion":crate::deploy::AGENT_VERSION,"transfers":i.ops.transfers.iter().map(|(id,t)|json!({"id":id,"state":t.state,"message":t.message,"target":t.target})).collect::<Vec<_>>(),"removals":i.ops.removal_names,"updater":app.0.dir.join("updater-enabled").exists()}),
        )),
        ("agent-update", "POST") | ("agent-rollback", "POST") => {
            auth::require_elevated(&i, &c)?;
            crate::migration::start_manage(
                &app,
                &mut i,
                &v.id,
                if action == "agent-update" {
                    "upgrade"
                } else {
                    "rollback"
                },
            )?;
            Ok(ApiReply::accepted(json!({"ok":true})))
        }
        ("panel-update", "POST") | ("panel-rollback", "POST") => {
            auth::require_elevated(&i, &c)?;
            if !app.0.dir.join("updater-enabled").exists() {
                return Err(ApiError::new(409, "请先按部署教程安装专用更新服务"));
            }
            if action == "panel-update" && !crate::backup::valid_version(&v.version) {
                return Err(ApiError::new(400, "版本号不正确"));
            }
            atomic_json(&app.0.dir.join("update-request.json"),&json!({"action":if action=="panel-update"{"update"}else{"rollback"},"version":v.version,"at":now()})).map_err(|_|ApiError::internal())?;
            app.record(&mut i, "panel_update_requested", &v.version);
            Ok(ApiReply::accepted(json!({"ok":true})))
        }
        ("update-status", "GET") => Ok(ApiReply::ok(
            read_json::<Value>(&app.0.dir.join("update-result.json"))
                .unwrap_or(json!({"state":"idle"})),
        )),
        ("migration/retry", "POST") => {
            auth::require_elevated(&i, &c)?;
            if let Some(t) = i.ops.transfers.get_mut(&v.id) {
                t.next = 0;
            }
            save(&app, &i)?;
            Ok(ApiReply::ok(json!({"ok":true})))
        }
        _ => Err(ApiError::new(404, "接口不存在")),
    }
}
fn alarms(app: &App, i: &mut Inner, at: i64) {
    let mut notices = Vec::new();
    for n in &i.data.nodes {
        if n.demo || n.removing {
            continue;
        }
        let p = &n.policy;
        let t = i.ops.tracks.entry(n.public.id.clone()).or_default();
        history::status(t, n.public.online, at);
        if p.maintenance_until > at {
            let prefix = format!("{}:", n.public.id);
            i.ops.alarms.retain(|key, _| !key.starts_with(&prefix));
            continue;
        }
        if !n.public.online {
            let prefix = format!("{}:", n.public.id);
            for (key, alarm) in &mut i.ops.alarms {
                if key.starts_with(&prefix) && !alarm.active {
                    alarm.since = 0;
                }
            }
            continue;
        }
        let memory = 100. * n.public.memory.used.unwrap_or(0.) / n.public.memory.total.max(1.);
        let disk = n
            .public
            .disks
            .iter()
            .map(|d| 100. * d.used.unwrap_or(0.) / d.total.max(1.))
            .fold(0., f64::max);
        let traffic = if p.quota_bytes > 0 {
            100. * used(p, t) as f64 / p.quota_bytes as f64
        } else {
            0.
        };
        for (kind, value, threshold) in [
            ("CPU", n.public.cpu.unwrap_or(0.), p.cpu),
            ("内存", memory, p.memory),
            ("磁盘", disk, p.disk),
            (
                "流量",
                traffic,
                if p.quota_bytes > 0 {
                    p.traffic_percent
                } else {
                    0.
                },
            ),
        ] {
            let key = format!("{}:{kind}", n.public.id);
            let a = i.ops.alarms.entry(key).or_default();
            let high = threshold > 0. && value >= threshold;
            if high {
                if a.since == 0 {
                    a.since = at;
                }
                if at - a.since >= p.sustained_seconds
                    && (a.sent == 0 || at - a.sent >= p.repeat_seconds)
                {
                    a.active = true;
                    a.sent = at;
                    notices.push(format!(
                        "{} · {} {:.1}% ≥ {:.1}%",
                        n.public.name, kind, value, threshold
                    ));
                }
            } else if !a.active || threshold == 0. || value <= threshold - (threshold * 0.1).min(5.)
            {
                if a.active && threshold > 0. {
                    notices.push(format!(
                        "{} · {}已恢复（{:.1}%）",
                        n.public.name, kind, value
                    ));
                }
                *a = Alarm::default();
            }
        }
    }
    for chunk in notices.chunks(20) {
        crate::telegram::queue_notice(app, i, &chunk.join("\n"), "resource");
    }
}
pub fn start(app: &App) {
    let app = app.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(5));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut persisted = 0;
        loop {
            tokio::select! {_=app.0.stop.cancelled()=>break,_=timer.tick()=>{}}
            let at = now();
            let records = {
                let mut i = app.lock();
                i.expire_nodes();
                alarms(&app, &mut i, at);
                if at - persisted >= 60 {
                    persisted = at;
                    let _ = save(&app, &i);
                    Some((*app.0.history_io.lock().unwrap(), i.ops.tracks.clone()))
                } else {
                    None
                }
            };
            if let Some((generation, records)) = records {
                let writer = app.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    history::persist(&writer.0.dir, &writer.0.history_io, generation, records);
                })
                .await;
            }
            crate::backup::scheduled(&app).await;
            crate::migration::tick(&app).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sustained_alerts_group_recover_repeat_and_respect_maintenance() {
        let dir = std::env::temp_dir().join(format!("yuji-alarm-{}", token()));
        std::fs::create_dir_all(&dir).unwrap();
        atomic_json(&dir.join("auth.json"), &Auth::default()).unwrap();
        atomic_json(
            &dir.join("nodes.json"),
            &Data {
                schema: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let app = App::new(dir.clone(), "https://example.com".into(), false).unwrap();
        {
            let mut i = app.lock();
            i.telegram.config.enabled = true;
            i.telegram.config.token = "test-only-no-network".into();
            for id in ["a", "b"] {
                let mut n = Node::default();
                n.public.id = id.into();
                n.public.name = format!("test-{id}");
                n.public.online = true;
                n.public.cpu = Some(90.);
                n.policy.cpu = 80.;
                n.policy.sustained_seconds = 10;
                n.policy.repeat_seconds = 300;
                i.data.nodes.push(n);
            }
            alarms(&app, &mut i, 100);
            alarms(&app, &mut i, 109);
            assert!(i.telegram.queue.is_empty());
            alarms(&app, &mut i, 110);
            assert_eq!(i.telegram.queue.len(), 1);
            assert!(i.telegram.queue[0].name.contains("test-a"));
            assert!(i.telegram.queue[0].name.contains("test-b"));
            alarms(&app, &mut i, 409);
            assert_eq!(i.telegram.queue.len(), 1);
            alarms(&app, &mut i, 410);
            assert_eq!(i.telegram.queue.len(), 2);
            for n in &mut i.data.nodes {
                n.public.cpu = Some(70.);
            }
            alarms(&app, &mut i, 415);
            assert_eq!(i.telegram.queue.len(), 3);
            assert!(i.telegram.queue[2].name.contains("已恢复"));
            for n in &mut i.data.nodes {
                n.public.cpu = Some(90.);
                n.policy.maintenance_until = 500;
            }
            alarms(&app, &mut i, 420);
            alarms(&app, &mut i, 499);
            assert!(i.ops.alarms.is_empty());
            alarms(&app, &mut i, 500);
            alarms(&app, &mut i, 509);
            assert_eq!(i.telegram.queue.len(), 3);
            alarms(&app, &mut i, 510);
            assert_eq!(i.telegram.queue.len(), 4);
            for n in &mut i.data.nodes {
                n.public.cpu = Some(70.);
            }
            alarms(&app, &mut i, 520);
            for n in &mut i.data.nodes {
                n.public.cpu = Some(90.);
            }
            alarms(&app, &mut i, 530);
            for n in &mut i.data.nodes {
                n.public.online = false;
            }
            alarms(&app, &mut i, 539);
            for n in &mut i.data.nodes {
                n.public.online = true;
            }
            alarms(&app, &mut i, 550);
            assert_eq!(i.telegram.queue.len(), 5);
            alarms(&app, &mut i, 559);
            assert_eq!(i.telegram.queue.len(), 5);
            alarms(&app, &mut i, 560);
            assert_eq!(i.telegram.queue.len(), 6);
        }
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
