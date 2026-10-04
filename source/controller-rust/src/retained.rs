//! Short-lived remote shells, owned by exactly one authenticated login.
use crate::{core::*, ssh};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;

pub const RETENTION: i64 = 300;
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
    #[serde(skip)]
    pub ready: bool,
    #[serde(skip)]
    pub reconnecting: bool,
    #[serde(skip)]
    pub stop: Option<CancellationToken>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
}
pub fn valid(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn load(dir: &std::path::Path) -> Result<HashMap<String, Retained>, &'static str> {
    let mut records = match read_json::<HashMap<String, Retained>>(&dir.join("shell-leases.json")) {
        Ok(v) if v.len() <= 8 && v.iter().all(|(k, r)| k == &r.id && valid(k)) => v,
        Ok(_) => return Err("shell lease state invalid"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(_) => return Err("shell lease state unavailable"),
    };
    // Login sessions are intentionally not persisted across controller restarts.
    // Queue orphan cleanup instead of accepting an old shell under a new login.
    for r in records.values_mut() {
        r.closing = true;
    }
    Ok(records)
}
pub fn save(app: &App, i: &Inner) -> ApiResult<()> {
    atomic_json(&app.0.dir.join("shell-leases.json"), &i.retained).map_err(|_| ApiError::internal())
}
fn owner(r: &Retained, session: &Session, node: &str, at: i64) -> bool {
    r.owner == session.handle
        && r.version == session.version
        && r.node == node
        && !r.closing
        && (r.attached || at - r.touched <= RETENTION)
}
pub fn reserve(
    app: &App,
    i: &mut Inner,
    session: &Session,
    node: &str,
    id: &str,
    stop: &CancellationToken,
) -> ApiResult<(String, bool)> {
    if !id.is_empty() {
        let r = i
            .retained
            .get_mut(id)
            .filter(|r| owner(r, session, node, now()))
            .ok_or_else(|| ApiError::new(410, "原 SSH 会话已结束或过期，请新建连接"))?;
        if r.attached {
            // A browser can discover the broken route before its old TCP socket times out.
            // Only the same login may cancel it; wait for full cleanup before reattaching.
            r.reconnecting = true;
            if let Some(stop) = &r.stop {
                stop.cancel();
            }
            return Err(ApiError::new(409, "正在释放旧连接，请稍后恢复原会话"));
        }
        r.attached = true;
        r.ready = false;
        r.reconnecting = false;
        r.stop = Some(stop.clone());
        if let Err(error) = save(app, i) {
            if let Some(r) = i.retained.get_mut(id) {
                r.attached = false;
                r.stop = None;
            }
            return Err(error);
        }
        return Ok((id.into(), false));
    }
    if i.retained.len() >= 8 || i.retained.values().filter(|r| r.node == node).count() >= 4 {
        return Err(ApiError::new(429, "终端会话数量已满，请关闭不用的终端"));
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
            ready: false,
            reconnecting: false,
            stop: Some(stop.clone()),
        },
    );
    if let Err(error) = save(app, i) {
        i.retained.remove(&id);
        return Err(error);
    }
    Ok((id, true))
}
pub fn ready(app: &App, id: &str) {
    let mut i = app.lock();
    if let Some(r) = i.retained.get_mut(id) {
        r.ready = true;
        r.touched = now();
    }
    let _ = save(app, &i);
}
pub fn release(app: &App, id: &str, reason: &str) {
    let mut i = app.lock();
    if let Some(r) = i.retained.get_mut(id) {
        let recoverable = matches!(
            reason,
            "browser_connection_lost"
                | "heartbeat_timeout"
                | "input_stalled"
                | "ssh_input_failed"
                | "ssh_connection_lost"
                | "agent_connection_lost"
                | "browser_write_timeout"
        );
        if !recoverable && !r.reconnecting {
            r.closing = true;
        }
        if r.ready {
            r.touched = now();
        }
        r.attached = false;
        r.ready = false;
        r.reconnecting = false;
        r.stop = None;
    }
    let _ = save(app, &i);
}
pub fn api(app: &App, i: &mut Inner, c: &Context, body: &[u8]) -> ApiResult<ApiReply> {
    let s = app.guard(i, c, true, true)?;
    let v: Input = decode(body)?;
    if !valid(&v.id) {
        return Err(ApiError::new(400, "会话标识不正确"));
    }
    let r = i
        .retained
        .get_mut(&v.id)
        .filter(|r| r.owner == s.handle && r.version == s.version)
        .ok_or_else(|| ApiError::new(404, "会话不存在"))?;
    r.closing = true;
    if let Some(stop) = &r.stop {
        stop.cancel();
    }
    save(app, i)?;
    Ok(ApiReply::ok(json!({"ok":true})))
}
pub fn prefix(app: &App) -> String {
    format!(
        "tmux -L yuji2-{} -f /dev/null",
        &hex::encode(Sha256::digest(app.0.master.as_slice()))[..20]
    )
}
fn name(id: &str) -> String {
    ssh::quote(&format!("yuji_{id}"))
}
pub fn attach_command(app: &App, id: &str, cols: u32, rows: u32) -> String {
    assert!(valid(id));

    // One atomic tmux command queue: subscribe, resize, capture, then stream.
    // The -C client forwards original PTY output instead of lossy screen redraws.
    let target = ssh::quote(&format!("yuji_{id}:0.0"));
    let state = ssh::quote(
        "YUJI_STATE #{pane_id} #{cursor_x} #{cursor_y} #{?alternate_saved_x,#{alternate_saved_x},0} #{?alternate_saved_y,#{alternate_saved_y},0} #{?alternate_on,1,0} #{?cursor_flag,1,0} #{?insert_flag,1,0} #{?wrap_flag,1,0} #{?keypad_cursor_flag,1,0} #{?keypad_flag,1,0} #{?mouse_standard_flag,1,0} #{?mouse_button_flag,1,0} #{?mouse_all_flag,1,0} #{?mouse_utf8_flag,1,0} #{?mouse_sgr_flag,1,0} 1",
    );
    format!(
        "{} -C attach-session -d -t {} ';' refresh-client -C {cols},{rows} ';' capture-pane -p -e -C -S -{} -t {target} ';' capture-pane -a -q -p -e -C -S -{} -t {target} ';' capture-pane -p -P -C -t {target} ';' display-message -p -t {target} {state}",
        prefix(app),
        name(id),
        crate::tmux_stream::HISTORY,
        crate::tmux_stream::HISTORY
    )
}
pub async fn prepare(
    app: &App,
    client: &ssh::Client,
    id: &str,
    fresh: bool,
    cols: u32,
    rows: u32,
) -> Result<(), &'static str> {
    if !valid(id) {
        return Err("invalid shell lease");
    }
    let base = prefix(app);
    let target = name(id);
    let command = if fresh {
        // One small watchdog per shell. Detached shells expire even if the
        // controller never comes back. No files, user tmux sockets or SSH config are touched.
        // run-shell expands tmux formats once before starting the job. Escape
        // the hash so display-message evaluates attachment on each iteration.
        let watch = format!(
            "probe_idle=0; while probe_attached=$({base} display-message -p -t {target} '##{{session_attached}}' 2>/dev/null); do case \"$probe_attached\" in 0) probe_idle=$((probe_idle+5));; *) probe_idle=0;; esac; if [ \"$probe_idle\" -gt {RETENTION} ]; then {base} kill-session -t {target} 2>/dev/null; break; fi; sleep 5; done"
        );
        let hook = format!("run-shell -b {}", ssh::quote(&watch));
        format!(
            "command -v tmux >/dev/null || exit 73; {base} start-server ';' set-option -g history-limit 5000 ';' new-session -d -s {} -x {cols} -y {rows} && {base} set-option -t {target} status off && {base} set-option -t {target} prefix None && {base} set-option -t {target} destroy-unattached off && {base} {hook}",
            ssh::quote(&format!("yuji_{id}"))
        )
    } else {
        format!("{base} has-session -t {target}")
    };
    tokio::time::timeout(
        Duration::from_secs(12),
        ssh::exec(client, &command, &[], 4096),
    )
    .await
    .map_err(|_| "shell preparation timeout")??;
    Ok(())
}
pub async fn cleanup_client(app: &App, id: &str, client: &ssh::Client) -> bool {
    if !valid(id) {
        return false;
    }
    let base = prefix(app);
    let target = name(id);
    let command = format!(
        "{base} kill-session -t {target} 2>/dev/null || ! {base} has-session -t {target} 2>/dev/null"
    );
    if matches!(
        tokio::time::timeout(
            Duration::from_secs(4),
            ssh::exec(client, &command, &[], 4096)
        )
        .await,
        Ok(Ok(_))
    ) {
        let mut i = app.lock();
        if i.retained.get(id).is_some_and(|r| r.closing && !r.attached) {
            i.retained.remove(id);
            let _ = save(app, &i);
        }
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
            let work = {
                let mut i = app.lock();
                let version = i.auth.version.clone();
                let owners: std::collections::HashSet<_> = i
                    .sessions
                    .values()
                    .filter(|s| {
                        s.auth
                            && s.version == version
                            && now() - s.created < 28800
                            && now() - s.seen < 1800
                    })
                    .map(|s| s.handle.clone())
                    .collect();
                let nodes: std::collections::HashSet<_> = i
                    .data
                    .nodes
                    .iter()
                    .filter(|n| !n.removing)
                    .map(|n| n.public.id.clone())
                    .collect();
                i.retained.retain(|_, r| nodes.contains(&r.node));
                let mut changed = false;
                for r in i.retained.values_mut() {
                    if r.attached && r.ready && !r.closing && now() - r.touched >= 60 {
                        r.touched = now();
                        changed = true;
                    }
                    if r.version != version
                        || !owners.contains(&r.owner)
                        || (!r.attached && now() - r.touched > RETENTION)
                    {
                        if !r.closing {
                            changed = true;
                        }
                        r.closing = true;
                        if let Some(stop) = &r.stop {
                            stop.cancel();
                        }
                    }
                }
                if changed {
                    let _ = save(&app, &i);
                }
                i.retained
                    .values()
                    .filter(|r| r.closing && !r.attached)
                    .filter_map(|r| {
                        Some((
                            r.id.clone(),
                            i.data.nodes.iter().find(|n| n.public.id == r.node)?.clone(),
                            i.data.secrets.get(&r.node)?.clone(),
                            i.agents.get(&r.node)?.clone(),
                        ))
                    })
                    .collect::<Vec<_>>()
            };
            for (id, node, secret, link) in work {
                let action = async {
                    if let Ok(client) =
                        ssh::agent_client(&app, link, &node.public.id, &node.username, &secret)
                            .await
                    {
                        cleanup_client(&app, &id, &client).await;
                        let _ = client
                            .disconnect(russh::Disconnect::ByApplication, "shell cleanup", "")
                            .await;
                    }
                };
                let _ = tokio::time::timeout(Duration::from_secs(12), action).await;
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_tokens_reject_shell_syntax() {
        assert!(valid(&"a".repeat(64)));
        for value in ["", "../x", "$(id)", "a;id", &"A".repeat(64)] {
            assert!(!valid(value));
        }
    }
    #[test]
    fn recovery_requires_original_login_node_and_version() {
        let r = Retained {
            id: "a".repeat(64),
            node: "node".into(),
            owner: "owner".into(),
            version: "v".into(),
            touched: 100,
            created: 90,
            closing: false,
            attached: false,
            ready: false,
            reconnecting: false,
            stop: None,
        };
        let s = Session {
            id: "cookie".into(),
            csrf: String::new(),
            auth: true,
            version: "v".into(),
            created: 90,
            seen: 100,
            source: String::new(),
            device: String::new(),
            handle: "owner".into(),
            elevated: 0,
            mfa_pending: String::new(),
            mfa_expires: 0,
        };
        assert!(owner(&r, &s, "node", 400));
        assert!(!owner(&r, &s, "node", 401));
        assert!(!owner(
            &r,
            &Session {
                handle: "other".into(),
                ..s.clone()
            },
            "node",
            100
        ));
        assert!(!owner(
            &r,
            &Session {
                version: "revoked".into(),
                ..s.clone()
            },
            "node",
            100
        ));
        assert!(!owner(&r, &s, "other", 100));
        assert!(!owner(&Retained { closing: true, ..r }, &s, "node", 100));
    }
}
