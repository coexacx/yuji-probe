use crate::{auth, core::*, model::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, time::Duration};
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
    pub offsite: crate::offsite::State,
    pub transfers: HashMap<String, crate::migration::Transfer>,
    pub retired: HashMap<String, NodeSecret>,
    pub removal_names: HashMap<String, String>,
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
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct Input {
    id: String,
    version: String,
}
pub async fn api(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    if c.path == "/api/migrate" {
        return crate::migration::receive(app, c, body).await;
    }
    if c.path.starts_with("/api/admin/ops/backup") || c.path == "/api/admin/ops/restore" {
        return crate::backup::api(app, c, body).await;
    }
    if c.path.starts_with("/api/admin/ops/offsite") {
        return crate::offsite::api(app, c, body).await;
    }
    if matches!(
        c.path.as_str(),
        "/api/admin/ops/policy" | "/api/admin/ops/history"
    ) {
        app.guard(&mut app.lock(), &c, true, c.method != "GET")?;
        return Err(ApiError::new(404, "接口不存在"));
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
pub fn start(app: &App) {
    let app = app.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(5));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {_=app.0.stop.cancelled()=>break,_=timer.tick()=>{}}
            app.lock().expire_nodes();
            crate::backup::scheduled(&app).await;
            crate::offsite::tick(&app).await;
            crate::migration::tick(&app).await;
        }
    });
}
