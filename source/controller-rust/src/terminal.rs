use crate::{core::*, file_sessions, files, ssh};
use axum::{
    body::Bytes,
    extract::{
        ConnectInfo, State,
        ws::{Message as WS, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, Method},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use russh::ChannelMsg;
use serde::Deserialize;
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinSet,
    time::timeout,
};
use tokio_util::sync::CancellationToken;
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Authorize {
    #[serde(rename = "transferSession")]
    transfer_session: String,
    #[serde(rename = "type")]
    kind: String,
    ticket: String,
    mode: String,
    cols: i64,
    rows: i64,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Control {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    cols: i64,
    rows: i64,
}
// Bound memory, throughput and unresponsive peers, not useful session output.
const TERMINAL_IO_TIMEOUT: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_secs(25);
const PONG_TIMEOUT: Duration = Duration::from_secs(75);

struct Heartbeat {
    pending: Option<(Vec<u8>, Instant)>,
    next_ping: Instant,
}
impl Heartbeat {
    fn new(at: Instant) -> Self {
        Self {
            pending: None,
            next_ping: at + PING_INTERVAL,
        }
    }
    fn expired(&self, at: Instant) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, sent)| at.duration_since(*sent) >= PONG_TIMEOUT)
    }
    fn ping(&mut self, at: Instant) -> Option<Vec<u8>> {
        if at < self.next_ping || self.expired(at) {
            return None;
        }
        self.next_ping = at + PING_INTERVAL;
        // Retries keep the original challenge and deadline. They never revive a dead peer.
        let (bytes, _) = self
            .pending
            .get_or_insert_with(|| (token().as_bytes()[..32].to_vec(), at));
        Some(bytes.clone())
    }
    fn acknowledge(&mut self, bytes: &[u8], at: Instant) -> bool {
        if self.expired(at)
            || !self
                .pending
                .as_ref()
                .is_some_and(|(expected, _)| expected.as_slice() == bytes)
        {
            return false;
        }
        self.pending = None;
        true
    }
}

enum TerminalInput {
    Data(Bytes),
    Resize(u32, u32),
}

fn retryable(reason: &str) -> bool {
    matches!(
        reason,
        "heartbeat_timeout"
            | "input_stalled"
            | "ssh_input_failed"
            | "ssh_connection_lost"
            | "agent_connection_lost"
            | "browser_write_timeout"
    )
}

type EndReason = Arc<Mutex<Option<&'static str>>>;
fn ended(reason: &EndReason, value: &'static str) {
    reason.lock().unwrap().get_or_insert(value);
}
fn ending_message(reason: &str) -> (&'static str, &'static str) {
    match reason {
        "user_disconnect" | "browser_closed" => ("closed", "SSH 会话已关闭"),
        "ssh_channel_closed" => ("closed", "远端 SSH 会话已结束"),
        "heartbeat_timeout" => ("error", "终端心跳长时间无响应，请检查网络后重新连接"),
        "authorization_lost" => ("error", "管理授权或节点连接已失效，请重新连接"),
        "input_stalled" => ("error", "远端 SSH 长时间未接收输入"),
        "ssh_input_failed" | "ssh_connection_lost" => ("error", "SSH 连接意外中断"),
        "agent_connection_lost" => ("error", "节点连接暂时中断"),
        "input_queue_full" => ("error", "待发送输入过多，连接已关闭"),
        "browser_write_timeout" => ("error", "终端数据发送长时间受阻，请检查网络后重新连接"),
        "invalid_message" => ("error", "终端请求无效或超过速率限制，连接已关闭"),
        _ => ("error", "终端连接已中断，请重新连接"),
    }
}

fn size(cols: i64, rows: i64) -> (u32, u32) {
    (cols.clamp(20, 300) as u32, rows.clamp(5, 120) as u32)
}
pub async fn handler(
    State(app): State<App>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    method: Method,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    let c = Context::new(method.as_str(), "/api/terminal", headers, remote);
    let validation = (|| -> ApiResult<_> {
        let mut i = app.lock();
        i.request(&format!("terminal:{}", c.ip), 30)?;
        if c.method != "GET" || c.header("Origin") != app.0.origin {
            return Err(ApiError::new(403, "terminal origin rejected"));
        }
        let session = app.guard(&mut i, &c, true, false)?;
        let slot = app
            .0
            .pending_terminals
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::rate("正在建立的终端连接过多", 5))?;
        Ok((session, slot))
    })();
    let (session, slot) = match validation {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let ws = match upgrade {
        Ok(v) => v,
        Err(_) => return ApiError::new(400, "WebSocket upgrade required").into_response(),
    };
    ws.read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(files::MAX_FRAME * 2)
        .max_message_size(files::MAX_FRAME)
        .max_frame_size(files::MAX_FRAME)
        .on_upgrade(move |ws| run(app, session, slot, ws))
}
struct TerminalGuard {
    app: App,
    sid: String,
    id: String,
    name: String,
    file_sessions: String,
    stop: CancellationToken,
    started: Instant,
    reason: EndReason,
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.stop.cancel();
        if !self.file_sessions.is_empty() {
            file_sessions::detached(&self.app, &self.file_sessions);
        }
        let mut i = self.app.lock();
        if let Some(all) = i.terminals.get_mut(&self.sid) {
            all.remove(&self.id);
        }
        let reason = self.reason.lock().unwrap().unwrap_or("connection_ended");
        self.app.record(
            &mut i,
            "terminal_closed",
            &format!(
                "{} · {} · {}s",
                self.name,
                reason,
                self.started.elapsed().as_secs()
            ),
        );
    }
}
async fn run(app: App, session: Session, pending: OwnedSemaphorePermit, mut ws: WebSocket) {
    let first = match timeout(Duration::from_secs(5), ws.recv()).await {
        Ok(Some(Ok(WS::Text(raw)))) if raw.len() <= 2048 => {
            serde_json::from_str::<Authorize>(&raw).ok()
        }
        _ => None,
    };
    let Some(first) = first.filter(|v| {
        v.kind == "authorize"
            && v.ticket.len() == 64
            && (v.transfer_session.is_empty() || file_sessions::valid(&v.transfer_session))
            && ["", "terminal", "inspect"].contains(&v.mode.as_str())
            && (v.mode != "inspect" || v.transfer_session.is_empty())
    }) else {
        return;
    };
    let authorized = (|| -> ApiResult<_> {
        let mut i = app.lock();
        let ticket = i
            .tickets
            .remove(&first.ticket)
            .filter(|t| t.session_id == session.id && t.expires >= now())
            .ok_or_else(|| ApiError::new(401, "终端授权已过期"))?;
        let valid = i.sessions.get(&session.id).is_some_and(|s| {
            s.auth
                && s.version == session.version
                && i.auth.version == session.version
                && now() - s.created < 28800
                && now() - s.seen < 1800
        });
        if !valid {
            return Err(ApiError::new(401, "终端授权已过期"));
        }
        let slot = app
            .0
            .terminal_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::new(429, "终端会话数量已满"))?;
        let node = i
            .data
            .nodes
            .iter()
            .find(|n| n.public.id == ticket.node_id)
            .cloned()
            .ok_or_else(|| ApiError::new(409, "节点当前未连接"))?;
        if node.removing {
            return Err(ApiError::new(403, "服务器正在移除"));
        }
        let link = i
            .agents
            .get(&ticket.node_id)
            .cloned()
            .ok_or_else(|| ApiError::new(409, "节点当前未连接"))?;
        let secret = i
            .data
            .secrets
            .get(&ticket.node_id)
            .cloned()
            .ok_or_else(|| ApiError::new(409, "节点认证配置不可用"))?;
        let file_sessions_id = if first.mode == "inspect" {
            String::new()
        } else {
            file_sessions::reserve(
                &app,
                &mut i,
                &session,
                &node.public.id,
                &first.transfer_session,
            )?
        };
        let stop = app.0.stop.child_token();
        let id = token();
        i.terminals
            .entry(session.id.clone())
            .or_default()
            .insert(id.clone(), stop.clone());
        app.record(&mut i, "terminal_opened", &node.public.name);
        Ok((node, link, secret, stop, id, slot, file_sessions_id))
    })();
    let (node, link, secret, stop, id, _slot, file_sessions_id) = match authorized {
        Ok(v) => v,
        Err(e) => {
            let _ = ws
                .send(WS::Text(
                    json!({"type":"error","message":e.message})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };
    drop(pending);
    let reason: EndReason = Arc::new(Mutex::new(None));
    let _guard = TerminalGuard {
        app: app.clone(),
        sid: session.id.clone(),
        id,
        name: node.public.name.clone(),
        file_sessions: file_sessions_id.clone(),
        stop: stop.clone(),
        started: Instant::now(),
        reason: reason.clone(),
    };
    let client = tokio::select! {_=stop.cancelled()=>return,c=ssh::agent_client(&app,link.clone(),&node.public.id,&node.username,&secret)=>match c{Ok(c)=>c,Err(_)=>{let _=ws.send(WS::Text(json!({"type":"error","message":"SSH 连接未完成，请检查现有终端数量、节点用户、公钥授权与主机指纹"}).to_string().into())).await;return}}};
    let files_session = file_sessions_id.clone();
    let terminal = async {
        let mut channel = client.channel_open_session().await.map_err(|_| ())?;
        let (cols, rows) = size(first.cols, first.rows);
        channel
            .request_pty(
                true,
                "xterm-256color",
                cols,
                rows,
                0,
                0,
                &[
                    (russh::Pty::ECHO, 1),
                    (russh::Pty::TTY_OP_ISPEED, 14400),
                    (russh::Pty::TTY_OP_OSPEED, 14400),
                ],
            )
            .await
            .map_err(|_| ())?;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => break,
                Some(ChannelMsg::Failure) | None => return Err(()),
                _ => {}
            }
        }
        channel.request_shell(true).await.map_err(|_| ())?;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Success) => break,
                Some(ChannelMsg::Failure) | None => return Err(()),
                _ => {}
            }
        }
        Ok(channel)
    };
    let inspect_only = first.mode == "inspect";
    let channel = if inspect_only {
        None
    } else {
        match timeout(Duration::from_secs(12), terminal).await {
            Ok(Ok(v)) => Some(v),
            _ => {
                let _ = ws
                    .send(WS::Text(
                        json!({"type":"error","message":"无法创建 SSH 终端"})
                            .to_string()
                            .into(),
                    ))
                    .await;
                let _ = client
                    .disconnect(russh::Disconnect::ByApplication, "terminal unavailable", "")
                    .await;
                return;
            }
        }
    };
    let (shell_read, shell_write) = if let Some(channel) = channel {
        let (r, w) = channel.split();
        (Some(r), Some(Arc::new(w)))
    } else {
        (None, None)
    };
    let (mut ws_write, mut ws_read) = ws.split();
    let (out, mut send_queue) = mpsc::channel::<WS>(8);
    let (control_out, mut control_queue) = mpsc::channel::<WS>(4);
    let credits = Arc::new(Semaphore::new(32));
    let probe = Arc::new(Mutex::new(Heartbeat::new(Instant::now())));
    let mut tasks = JoinSet::new();
    let check: files::Authorize = {
        let app = app.clone();
        let sid = session.id.clone();
        let version = session.version.clone();
        let node = node.public.id.clone();
        let link = link.clone();
        let file_sessions_id = file_sessions_id.clone();
        Arc::new(move || {
            let i = app.lock();
            i.terminal_valid(&sid, &version, &node, &link)
                && (file_sessions_id.is_empty()
                    || i.file_sessions
                        .get(&file_sessions_id)
                        .is_some_and(|r| !r.closing))
        })
    };
    let cancel = stop.clone();
    let writer_reason = reason.clone();
    let writer_check = check.clone();
    let mut writer_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    if !writer_check() { ended(&writer_reason, "authorization_lost"); }
                    let why = writer_reason.lock().unwrap().unwrap_or("connection_ended");
                    let (kind, message) = ending_message(why);
                    let _ = timeout(Duration::from_secs(1), async {
                        ws_write.send(WS::Text(json!({"type":kind,"message":message,"reason":why,"retryable":retryable(why)}).to_string().into())).await?;
                        ws_write.send(WS::Close(None)).await
                    }).await;
                    break;
                }
                message = async {
                    tokio::select! {
                        biased;
                        control = control_queue.recv() => control,
                        data = send_queue.recv() => data,
                    }
                } => {
                    let Some(message) = message else { break };
                    let result = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => continue,
                        result = timeout(TERMINAL_IO_TIMEOUT, ws_write.send(message)) => result,
                    };
                    if !matches!(result, Ok(Ok(()))) {
                        ended(&writer_reason, "browser_write_timeout");
                        break;
                    }
                }
            }
        }
        cancel.cancel();
    });
    let _ = out
        .send(WS::Text(
            json!({"type":"ready","files":!inspect_only,"terminal":!inspect_only,"inspection":true,"transferSession":files_session})
                .to_string()
                .into(),
        ))
        .await;
    let (file_tx, mut file_rx) = mpsc::channel::<files::Request>(2);
    let mut file_service = files::Files::new(
        app.clone(),
        client.clone(),
        node.public.id.clone(),
        node.public.name.clone(),
        files_session.clone(),
        check.clone(),
    );
    let file_check = check.clone();
    let file_out = out.clone();
    let file_stop = stop.clone();
    let mut file_task = tokio::spawn(async move {
        let mut clock = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                biased;
                _ = file_stop.cancelled() => break,
                _ = clock.tick() => file_service.expire().await,
                r = file_rx.recv() => {
                    let Some(r) = r else { break };
                    if !file_check() { break; }
                    let value = tokio::select! { biased; _ = file_stop.cancelled() => break, v = file_service.process(&r) => v };
                    if file_check() { files::respond(&file_out, &r, value).await; }
                }
            }
        }
        file_service.cleanup().await;
    });
    // Keep the WebSocket reader available for acknowledgements, heartbeats and
    // cancellation even when the remote SSH receive window temporarily fills.
    // Input remains ordered and bounded; an interrupted write is never replayed.
    let (input_tx, mut input_rx) = mpsc::channel::<TerminalInput>(16);
    if let Some(input) = shell_write.clone() {
        let input_stop = stop.clone();
        let input_check = check.clone();
        let input_reason = reason.clone();
        tasks.spawn(async move {
            loop {
                let message = tokio::select! {
                    biased;
                    _ = input_stop.cancelled() => break,
                    message = input_rx.recv() => match message { Some(m) => m, None => break },
                };
                if !input_check() {
                    ended(&input_reason, "authorization_lost");
                    break;
                }
                let write = async {
                    match message {
                        TerminalInput::Data(bytes) => input.data_bytes(bytes).await,
                        TerminalInput::Resize(cols, rows) => {
                            input.window_change(cols, rows, 0, 0).await
                        }
                    }
                };
                let result = tokio::select! {
                    biased;
                    _ = input_stop.cancelled() => break,
                    result = timeout(TERMINAL_IO_TIMEOUT, write) => result,
                };
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => {
                        ended(&input_reason, "ssh_input_failed");
                        break;
                    }
                    Err(_) => {
                        ended(&input_reason, "input_stalled");
                        break;
                    }
                }
            }
            input_stop.cancel();
        });
    }
    let read_stop = stop.clone();
    let read_app = app.clone();
    let read_session = session.clone();
    let read_link = link.clone();
    let read_node = node.public.id.clone();
    let read_reason = reason.clone();
    let read_credits = credits.clone();
    let read_check = check.clone();
    let read_out = out.clone();
    let read_probe = probe.clone();
    let read_control = control_out.clone();
    let has_input = shell_write.is_some();
    let node_name = node.public.name.clone();
    let read_file_sessions = file_sessions_id.clone();
    tasks.spawn(async move {
        let mut window = Instant::now();
        let (mut frames, mut bytes, mut terminal_bytes) = (0usize, 0usize, 0usize);
        let mut file_window = window;
        let mut file_count = 0;
        let mut control_window = window;
        let mut control_frames = 0usize;
        // Validation errors share a bounded output queue but never wait in the
        // reader. A client flooding requests cannot starve Pong or ACK handling.
        let reject = |request: &files::Request, problem: files::Problem| {
            if read_out.try_send(files::response(request, Err(problem))).is_ok() { true }
            else { ended(&read_reason, "invalid_message"); false }
        };
        loop {
            let msg = tokio::select! {_=read_stop.cancelled()=>break,m=ws_read.next()=>m};
            let Some(Ok(msg)) = msg else {
                ended(&read_reason, "browser_connection_lost");
                break;
            };
            if matches!(&msg, WS::Ping(_) | WS::Pong(_)) {
                if control_window.elapsed() >= Duration::from_secs(1) {
                    control_window = Instant::now();
                    control_frames = 0;
                }
                control_frames += 1;
                if control_frames > 16 {
                    ended(&read_reason, "invalid_message");
                    break;
                }
            }
            match msg {
                WS::Pong(bytes) => {
                    let alive = read_probe
                        .lock()
                        .unwrap()
                        .acknowledge(bytes.as_ref(), Instant::now());
                    if alive {
                        let mut i = read_app.lock();
                        // A live, already authorized terminal counts as session activity.
                        // Never extend absolute expiry or resurrect revoked credentials.
                        if !i.terminal_valid(
                            &read_session.id,
                            &read_session.version,
                            &read_node,
                            &read_link,
                        ) {
                            ended(&read_reason, "authorization_lost");
                            break;
                        }
                        if let Some(s) = i.sessions.get_mut(&read_session.id) {
                            s.seen = now();
                        }
                    }
                    continue;
                }
                WS::Ping(bytes) => {
                    if read_control.try_send(WS::Pong(bytes)).is_err() {
                        ended(&read_reason, "invalid_message");
                        break;
                    }
                    continue;
                }
                WS::Close(_) => {
                    ended(&read_reason, "browser_closed");
                    break;
                }
                _ => {}
            }
            if window.elapsed() >= Duration::from_secs(1) {
                window = Instant::now();
                frames = 0;
                bytes = 0;
                terminal_bytes = 0;
            }
            frames += 1;
            bytes += match &msg {
                WS::Binary(b) => b.len(),
                WS::Text(b) => b.len(),
                _ => 0,
            };
            if !read_check() {
                ended(&read_reason, "authorization_lost");
                break;
            }
            if frames > 300 || bytes > files::MAX_FRAME {
                ended(&read_reason, "invalid_message");
                break;
            }
            match msg {
                WS::Binary(raw) => {
                    if !has_input {
                        continue;
                    }
                    terminal_bytes += raw.len();
                    if raw.len() > 16384 || terminal_bytes > 256 * 1024 {
                        ended(&read_reason, "invalid_message");
                        break;
                    }
                    if let Err(error) = input_tx.try_send(TerminalInput::Data(raw)) {
                        ended(
                            &read_reason,
                            if matches!(error, mpsc::error::TrySendError::Full(_)) {
                                "input_queue_full"
                            } else {
                                "ssh_input_failed"
                            },
                        );
                        break;
                    }
                }
                WS::Text(raw) => {
                    let Ok(control) = serde_json::from_str::<Control>(&raw) else {
                        break;
                    };
                    if control.kind == "file" {
                        let Some(r) = files::parse(raw.as_bytes()) else {
                            continue;
                        };
                        if file_window.elapsed() >= Duration::from_secs(60) {
                            file_window = Instant::now();
                            file_count = 0;
                        }
                        if !files::is_chunk(&r) {
                            file_count += 1;
                        }
                        if inspect_only && !files::is_inspection(&r) {
                            if !reject(&r, files::Problem {
                                    code: "permission",
                                    message: "此连接仅用于查看运行状态".into(),
                                }) { break; }
                            continue;
                        }
                        if file_count > 90 {
                            if !reject(&r, files::Problem {
                                    code: "rate",
                                    message: "文件操作过于频繁，请稍后重试".into(),
                                }) { break; }
                            continue;
                        }
                        if let Err(e) = files::validate(&r) {
                            if !reject(&r, e) { break; }
                            continue;
                        }
                        match file_tx.try_send(r) {
                            Ok(_) => {}
                            Err(e) => {
                                let r = e.into_inner();
                                if !reject(&r, files::Problem {
                                        code: "busy",
                                        message: "已有文件操作正在进行，请稍后重试".into(),
                                    }) { break; }
                            }
                        }
                        continue;
                    }
                    if raw.len() > 256 {
                        break;
                    }
                    match control.kind.as_str() {
                        "ack" => {
                            if read_credits.available_permits() < 32 {
                                read_credits.add_permits(1);
                            }
                        }
                        "resize" => {
                            if !has_input {
                                continue;
                            }
                            let (cols, rows) = size(control.cols, control.rows);
                            if let Err(error) = input_tx.try_send(TerminalInput::Resize(cols, rows))
                            {
                                ended(
                                    &read_reason,
                                    if matches!(error, mpsc::error::TrySendError::Full(_)) {
                                        "input_queue_full"
                                    } else {
                                        "ssh_input_failed"
                                    },
                                );
                                break;
                            }
                        }
                        "command" => {
                            if !has_input {
                                continue;
                            }
                            let script = {
                                let mut i = read_app.lock();
                                let command =
                                    i.data.commands.iter().find(|v| v.id == control.id).cloned();
                                if let Some(command) = command {
                                    read_app.record(
                                        &mut i,
                                        "command_executed",
                                        &format!("{node_name} · {}", command.name),
                                    );
                                    Some(command.script)
                                } else {
                                    None
                                }
                            };
                            if let Some(script) = script {
                                let command =
                                    format!("{}\n", script.trim_end_matches(['\r', '\n']));
                                if command.len() > 16384 {
                                    ended(&read_reason, "invalid_message");
                                    break;
                                }
                                if let Err(error) = input_tx
                                    .try_send(TerminalInput::Data(command.into_bytes().into()))
                                {
                                    ended(
                                        &read_reason,
                                        if matches!(error, mpsc::error::TrySendError::Full(_)) {
                                            "input_queue_full"
                                        } else {
                                            "ssh_input_failed"
                                        },
                                    );
                                    break;
                                }
                            } else {
                                if read_out.try_send(WS::Text(json!({"type":"notice","message":"命令不可用，请重新打开终端"}).to_string().into())).is_err() {
                                    ended(&read_reason, "invalid_message"); break;
                                }
                            }
                        }
                        "close" => {
                            ended(&read_reason, "user_disconnect");
                            file_sessions::ending(&read_app, &read_file_sessions);
                            break;
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        read_stop.cancel();
    });
    let output_stop = stop.clone();
    let output = out.clone();
    let output_app = app.clone();
    let output_file_sessions = file_sessions_id.clone();
    let output_reason = reason.clone();
    let output_client = client.clone();
    let output_link = link.clone();
    if let Some(mut shell_read) = shell_read {
        tasks.spawn(async move {
            let mut count = 0usize;
            let mut window = Instant::now();
            let result = async {
                let mut exited = false;
                while let Some(msg) = shell_read.wait().await {
                    match msg {
                        ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                            for chunk in data.chunks(16384) {
                                if window.elapsed() >= Duration::from_secs(1) {
                                    window = Instant::now();
                                    count = 0;
                                }
                                if count + chunk.len() > 256 * 1024 {
                                    tokio::time::sleep(
                                        Duration::from_secs(1).saturating_sub(window.elapsed()),
                                    )
                                    .await;
                                    window = Instant::now();
                                    count = 0;
                                }
                                let Ok(credit) = credits.acquire().await else {
                                    return;
                                };
                                credit.forget();
                                if !matches!(
                                    timeout(
                                        TERMINAL_IO_TIMEOUT,
                                        output.send(WS::Binary(chunk.to_vec().into()))
                                    )
                                    .await,
                                    Ok(Ok(()))
                                ) {
                                    ended(&output_reason, "browser_write_timeout");
                                    return;
                                }
                                count += chunk.len();
                            }
                        }
                        ChannelMsg::ExitStatus { .. } => exited = true,
                        ChannelMsg::Close => {
                            if exited {
                                file_sessions::ending(&output_app, &output_file_sessions);
                            }
                            let why = if exited {
                                "ssh_channel_closed"
                            } else if output_link.stop.is_cancelled() {
                                "agent_connection_lost"
                            } else if output_client.is_closed() {
                                "ssh_connection_lost"
                            } else {
                                "ssh_channel_closed"
                            };
                            ended(&output_reason, why);
                            break;
                        }
                        _ => {}
                    }
                }
            };
            tokio::select! {
                _ = output_stop.cancelled() => {},
                _ = result => {
                    let why = if output_link.stop.is_cancelled() { "agent_connection_lost" }
                        else if output_client.is_closed() { "ssh_connection_lost" }
                        else { "ssh_channel_closed" };
                    ended(&output_reason, why);
                }
            }
            output_stop.cancel();
        });
    }
    let watch_stop = stop.clone();
    let watch_out = control_out.clone();
    let watch_reason = reason.clone();
    let watch_link = link.clone();
    tasks.spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = watch_stop.cancelled() => break,
                _ = timer.tick() => {
                    if !check() {
                        ended(&watch_reason, if watch_link.stop.is_cancelled() { "agent_connection_lost" } else { "authorization_lost" });
                        break;
                    }
                    let at = Instant::now();
                    let (expired, ping) = {
                        let mut p = probe.lock().unwrap();
                        (p.expired(at), p.ping(at))
                    };
                    if expired {
                        ended(&watch_reason, "heartbeat_timeout");
                        break;
                    }
                    if let Some(value) = ping {
                        match watch_out.try_send(WS::Ping(value.into())) {
                            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {},
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                ended(&watch_reason, "browser_write_timeout");
                                break;
                            },
                        }
                    }
                }
            }
        }
        watch_stop.cancel();
    });
    stop.cancelled().await;
    // Closing the PTY comes first; parked file checkpoints do not keep shells alive.
    if let Some(shell) = shell_write {
        let _ = timeout(Duration::from_secs(1), shell.close()).await;
    }
    if timeout(Duration::from_secs(5), &mut file_task)
        .await
        .is_err()
    {
        file_task.abort();
        let _ = file_task.await;
    }
    let ending = {
        app.lock()
            .file_sessions
            .get(&file_sessions_id)
            .is_some_and(|r| r.closing)
    };
    if ending && files::transfer::remove_session(&app, &files_session, &client).await {
        let mut i = app.lock();
        i.file_sessions.remove(&files_session);
        let _ = file_sessions::save(&app, &i);
    }
    let _ = timeout(
        Duration::from_secs(1),
        client.disconnect(russh::Disconnect::ByApplication, "session ended", ""),
    )
    .await;
    // Give the writer a bounded chance to deliver the real close reason.
    if timeout(Duration::from_secs(2), &mut writer_task)
        .await
        .is_err()
    {
        writer_task.abort();
        let _ = writer_task.await;
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heartbeat_retries_do_not_reset_a_dead_peer_deadline() {
        let now = Instant::now();
        let mut heartbeat = Heartbeat::new(now);
        assert!(heartbeat.ping(now).is_none());
        let first = heartbeat.ping(now + PING_INTERVAL).unwrap();
        assert_eq!(heartbeat.ping(now + PING_INTERVAL * 2).unwrap(), first);
        assert!(!heartbeat.acknowledge(b"incorrect", now + PING_INTERVAL * 2));
        assert!(!heartbeat.expired(now + PING_INTERVAL + PONG_TIMEOUT - Duration::from_secs(1)));
        assert!(heartbeat.expired(now + PING_INTERVAL + PONG_TIMEOUT));
        assert!(!heartbeat.acknowledge(&first, now + PING_INTERVAL + PONG_TIMEOUT));
        assert!(heartbeat.ping(now + PING_INTERVAL + PONG_TIMEOUT).is_none());
    }
    #[test]
    fn healthy_terminal_can_remain_idle_and_tolerates_a_late_pong() {
        let now = Instant::now();
        let mut heartbeat = Heartbeat::new(now);
        let first = heartbeat.ping(now + PING_INTERVAL).unwrap();
        assert!(heartbeat.acknowledge(&first, now + PING_INTERVAL + Duration::from_secs(12)));
        assert!(!heartbeat.acknowledge(&first, now + PING_INTERVAL + Duration::from_secs(13)));
        for second in (50..(24 * 3600)).step_by(25) {
            let at = now + Duration::from_secs(second);
            assert!(!heartbeat.expired(at));
            let challenge = heartbeat.ping(at).unwrap();
            assert!(heartbeat.acknowledge(&challenge, at + Duration::from_secs(1)));
        }
    }
    #[test]
    fn reconnect_is_limited_to_transient_transport_failures() {
        for reason in [
            "user_disconnect",
            "browser_closed",
            "ssh_channel_closed",
            "authorization_lost",
            "invalid_message",
            "input_queue_full",
            "unknown",
        ] {
            assert!(!retryable(reason), "{reason}");
        }
        for reason in [
            "heartbeat_timeout",
            "input_stalled",
            "ssh_input_failed",
            "ssh_connection_lost",
            "agent_connection_lost",
            "browser_write_timeout",
        ] {
            assert!(retryable(reason), "{reason}");
        }
    }
    #[test]
    fn first_close_reason_is_preserved() {
        let reason: EndReason = Arc::new(Mutex::new(None));
        ended(&reason, "authorization_lost");
        ended(&reason, "browser_connection_lost");
        assert_eq!(*reason.lock().unwrap(), Some("authorization_lost"));
    }
    #[test]
    fn removed_shell_resume_protocol_is_rejected() {
        let value = json!({"type":"authorize","ticket":"a".repeat(64),"resume":"b".repeat(64)});
        assert!(serde_json::from_value::<Authorize>(value).is_err());
        let value =
            json!({"type":"authorize","ticket":"a".repeat(64),"transferSession":"b".repeat(64)});
        assert!(serde_json::from_value::<Authorize>(value).is_ok());
    }
}
