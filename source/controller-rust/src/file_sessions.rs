//! File-transfer leases, independent of SSH shell recovery.
use crate::{core::*, ssh};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

pub const RETENTION: i64 = 1800;
#[derive(Clone, Serialize, Deserialize)]
pub struct TransferSession {
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
pub fn load(dir: &std::path::Path) -> Result<HashMap<String, TransferSession>, &'static str> {
    match read_json::<HashMap<String, TransferSession>>(&dir.join("file-sessions.json")) {
        Ok(v) if v.len() <= 32 && v.iter().all(|(k, r)| k == &r.id && valid(k)) => Ok(v),
        Ok(_) => Err("file transfer state invalid"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(_) => Err("file transfer state unavailable"),
    }
}
pub fn valid(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn save(app: &App, i: &Inner) -> ApiResult<()> {
    atomic_json(&app.0.dir.join("file-sessions.json"), &i.file_sessions)
        .map_err(|_| ApiError::internal())
}
pub fn reserve(
    app: &App,
    i: &mut Inner,
    session: &Session,
    node: &str,
    id: &str,
) -> ApiResult<String> {
    if !id.is_empty() && i.file_sessions.contains_key(id) {
        let r = i
            .file_sessions
            .get_mut(id)
            .filter(|r| {
                r.node == node
                    && r.owner == session.handle
                    && r.version == session.version
                    && !r.closing
                    && !r.attached
                    && now() - r.touched <= RETENTION
            })
            .ok_or_else(|| {
                ApiError::new(
                    409,
                    "文件续传记录已过期或正在其他窗口使用，请再次点击连接以新建会话",
                )
            })?;
        r.attached = true;
        r.owner = session.handle.clone();
        r.touched = now();
        save(app, i)?;
        return Ok(id.into());
    }
    if i.file_sessions.len() >= 32 {
        // Empty disconnected workspaces may be evicted; never evict partial uploads.
        i.file_sessions
            .retain(|id, r| r.attached || crate::files::transfer::has_session(app, id));
        save(app, i)?;
    }
    if i.file_sessions.len() >= 32 {
        return Err(ApiError::new(
            429,
            "文件传输工作区数量已满，请关闭不用的终端",
        ));
    }
    let id = token();
    i.file_sessions.insert(
        id.clone(),
        TransferSession {
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
    Ok(id)
}
pub fn detached(app: &App, id: &str) {
    let mut i = app.lock();
    if let Some(r) = i.file_sessions.get_mut(id) {
        r.attached = false;
        r.touched = now();
    }
    let _ = save(app, &i);
}
pub fn ending(app: &App, id: &str) {
    let mut i = app.lock();
    if let Some(r) = i.file_sessions.get_mut(id) {
        r.closing = true;
    }
    let _ = save(app, &i);
}
pub async fn cleanup_one(app: &App, id: &str) -> bool {
    if !crate::files::transfer::has_session(app, id) {
        let mut i = app.lock();
        i.file_sessions.remove(id);
        let _ = save(app, &i);
        return true;
    }
    let info = {
        let i = app.lock();
        i.file_sessions.get(id).and_then(|r| {
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
        let files_clean = crate::files::transfer::remove_session(app, &r.id, &c).await;
        let _ = c
            .disconnect(russh::Disconnect::ByApplication, "terminal ended", "")
            .await;
        if files_clean {
            Ok(())
        } else {
            Err("file cleanup pending")
        }
    };
    if matches!(
        tokio::time::timeout(Duration::from_secs(15), work).await,
        Ok(Ok(()))
    ) {
        let mut i = app.lock();
        i.file_sessions.remove(id);
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
                    .file_sessions
                    .values()
                    .filter(|r| !nodes.contains(&r.node))
                    .map(|r| r.id.clone())
                    .collect();
                for id in missing {
                    i.file_sessions.remove(&id);
                    changed = true;
                }
                for r in i.file_sessions.values_mut() {
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
                i.file_sessions
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
    fn file_session_ids_reject_path_content() {
        assert!(valid(&"a".repeat(64)));
        for s in ["../x", "$(id)", "a;id", ""] {
            assert!(!valid(s));
        }
    }
}
