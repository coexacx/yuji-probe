//! Remote tmux sessions are scoped to this panel and authenticated administration.
use crate::{core::*, ssh};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};

pub const RETENTION: i64 = 1800;
#[derive(Clone, Serialize, Deserialize)]
pub struct Retained {
    pub id: String,
    pub node: String,
    pub owner: String,
    pub version: String,
    pub touched: i64,
    pub created: i64,
    #[serde(default)]
    pub closing: bool,
    #[serde(skip)]
    pub attached: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
}
pub fn load(dir: &std::path::Path) -> Result<HashMap<String, Retained>, &'static str> {
    match read_json::<HashMap<String, Retained>>(&dir.join("terminal-sessions.json")) {
        Ok(v) if v.len() <= 32 && v.iter().all(|(k, r)| k == &r.id && valid(k)) => Ok(v),
        Ok(_) => Err("terminal state invalid"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(_) => Err("terminal state unavailable"),
    }
}
pub fn valid(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn save(app: &App, i: &Inner) -> ApiResult<()> {
    atomic_json(&app.0.dir.join("terminal-sessions.json"), &i.retained)
        .map_err(|_| ApiError::internal())
}
pub fn prefix(app: &App) -> String {
    // Stable across domain migration, private to this panel installation.
    format!(
        "tmux -L yuji-{} -f /dev/null",
        &hex::encode(Sha256::digest(app.0.master.as_slice()))[..20]
    )
}
pub fn attach_command(app: &App, id: &str) -> String {
    debug_assert!(valid(id));
    format!(
        "{} attach-session -t {}",
        prefix(app),
        ssh::quote(&format!("yuji_{id}"))
    )
}
pub async fn prepare(
    app: &App,
    client: &ssh::Client,
    id: &str,
    fresh: bool,
) -> Result<bool, &'static str> {
    if fresh
        && !matches!(
            tokio::time::timeout(
                Duration::from_secs(5),
                ssh::exec(client, "command -v tmux", &[], 4096)
            )
            .await,
            Ok(Ok(_))
        )
    {
        return Ok(false);
    }
    let base = prefix(app);
    let name = ssh::quote(&format!("yuji_{id}"));
    let command = if fresh {
        format!(
            "command -v tmux >/dev/null || exit 73; {base} new-session -d -s {name} -x 100 -y 30 && {base} set-option -t {name} status off && {base} set-option -t {name} history-limit 2000 && {base} set-option -t {name} destroy-unattached off"
        )
    } else {
        format!("{base} has-session -t {name}")
    };
    tokio::time::timeout(
        Duration::from_secs(10),
        ssh::exec(client, &command, &[], 4096),
    )
    .await
    .map_err(|_| "terminal prepare timeout")??;
    Ok(true)
}
pub fn reserve(
    app: &App,
    i: &mut Inner,
    session: &Session,
    node: &str,
    id: &str,
) -> ApiResult<(String, bool)> {
    if !id.is_empty() {
        let r = i
            .retained
            .get_mut(id)
            .filter(|r| {
                r.node == node
                    && r.version == session.version
                    && !r.closing
                    && !r.attached
                    && now() - r.touched <= RETENTION
            })
            .ok_or_else(|| ApiError::new(409, "会话已结束、过期或正在其他窗口使用"))?;
        r.attached = true;
        r.owner = session.handle.clone();
        r.touched = now();
        save(app, i)?;
        return Ok((id.into(), false));
    }
    if i.retained.len() >= 8 {
        return Err(ApiError::new(429, "最多保留 8 个终端，请先结束不用的会话"));
    }
    let id = token();
    i.retained.insert(
        id.clone(),
        Retained {
            id: id.clone(),
            node: node.into(),
            owner: session.handle.clone(),
            version: session.version.clone(),
            touched: now(),
            created: now(),
            closing: false,
            attached: true,
        },
    );
    save(app, i)?;
    Ok((id, true))
}
pub fn detached(app: &App, id: &str) {
    let mut i = app.lock();
    if let Some(r) = i.retained.get_mut(id) {
        r.attached = false;
        r.touched = now();
    }
    let _ = save(app, &i);
}
pub fn ending(app: &App, id: &str) {
    let mut i = app.lock();
    if let Some(r) = i.retained.get_mut(id) {
        r.closing = true;
    }
    let _ = save(app, &i);
}
pub fn api(app: &App, i: &mut Inner, c: &Context, body: &[u8]) -> ApiResult<ApiReply> {
    let s = app.guard(i, c, true, c.method != "GET")?;
    if c.method == "GET" {
        let list:Vec<_>=i.retained.values().filter(|r|r.version==s.version&&!r.closing&&(r.attached||now()-r.touched<=RETENTION)).map(|r|json!({"id":r.id,"node":r.node,"attached":r.attached,"created":r.created,"expires":if r.attached {0}else{r.touched+RETENTION}})).collect();
        return Ok(ApiReply::ok(
            json!({"sessions":list,"retentionSeconds":RETENTION}),
        ));
    }
    if c.method != "POST" {
        return Err(ApiError::new(405, "请求方法不正确"));
    }
    let v: Input = decode(body)?;
    let r = i
        .retained
        .get_mut(&v.id)
        .filter(|r| r.version == s.version)
        .ok_or_else(|| ApiError::new(404, "会话不存在"))?;
    r.closing = true;
    save(app, i)?;
    Ok(ApiReply::ok(json!({"ok":true})))
}
pub async fn cleanup_one(app: &App, id: &str) -> bool {
    let info = {
        let i = app.lock();
        i.retained.get(id).and_then(|r| {
            Some((
                r.clone(),
                i.data.nodes.iter().find(|n| n.public.id == r.node)?.clone(),
                i.data.secrets.get(&r.node)?.clone(),
                i.agents.get(&r.node)?.clone(),
            ))
        })
    };
    let Some((r, node, secret, link)) = info else {
        return false;
    };
    let work = async {
        let c = ssh::agent_client(app, link, &node.public.id, &node.username, &secret).await?;
        let command = format!(
            "{} kill-session -t {} 2>/dev/null || ! {} has-session -t {} 2>/dev/null",
            prefix(app),
            ssh::quote(&format!("yuji_{}", r.id)),
            prefix(app),
            ssh::quote(&format!("yuji_{}", r.id))
        );
        let files_clean = crate::files::transfer::remove_session(app, &r.id, &c).await;
        let result = ssh::exec(&c, &command, &[], 4096).await;
        let _ = c
            .disconnect(russh::Disconnect::ByApplication, "terminal ended", "")
            .await;
        result.and({
            if files_clean {
                Ok(())
            } else {
                Err("file cleanup pending")
            }
        })
    };
    if matches!(
        tokio::time::timeout(Duration::from_secs(15), work).await,
        Ok(Ok(()))
    ) {
        let mut i = app.lock();
        i.retained.remove(id);
        let _ = save(app, &i);
        true
    } else {
        false
    }
}
pub fn start(app: &App) {
    let app = app.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {_=app.0.stop.cancelled()=>break,_=tick.tick()=>{}}
            let ids = {
                let mut i = app.lock();
                let version = i.auth.version.clone();
                let mut changed = false;
                let nodes: std::collections::HashSet<String> =
                    i.data.nodes.iter().map(|n| n.public.id.clone()).collect();
                let missing: Vec<_> = i
                    .retained
                    .values()
                    .filter(|r| !nodes.contains(&r.node))
                    .map(|r| r.id.clone())
                    .collect();
                for id in missing {
                    i.retained.remove(&id);
                    changed = true;
                }
                for r in i.retained.values_mut() {
                    if r.attached && !r.closing && now() - r.touched >= 60 {
                        r.touched = now();
                        changed = true;
                    }
                    if r.version != version || (!r.attached && now() - r.touched > RETENTION) {
                        if !r.closing {
                            changed = true;
                        }
                        r.closing = true;
                    }
                }
                if changed {
                    let _ = save(&app, &i);
                }
                i.retained
                    .values()
                    .filter(|r| r.closing && !r.attached)
                    .map(|r| r.id.clone())
                    .collect::<Vec<_>>()
            };
            for id in ids {
                cleanup_one(&app, &id).await;
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_ids_reject_shell_content() {
        assert!(valid(&"a".repeat(64)));
        for s in ["../x", "$(id)", "a;id", ""] {
            assert!(!valid(s));
        }
    }
}
