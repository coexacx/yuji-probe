use crate::{core::*, files, ssh};
use axum::{
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
#[serde(default)]
struct Authorize {
    #[serde(rename = "type")]
    kind: String,
    ticket: String,
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
fn size(cols: i64, rows: i64) -> (u32, u32) {
    (cols.clamp(20, 300) as u32, rows.clamp(5, 120) as u32)
}
async fn notice(out: &mpsc::Sender<WS>, kind: &str, message: &str) {
    let mut v = json!({"type":kind,"message":message});
    if kind == "ready" {
        v["files"] = json!(true);
    }
    let _ = timeout(
        Duration::from_secs(3),
        out.send(WS::Text(v.to_string().into())),
    )
    .await;
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
    stop: CancellationToken,
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.stop.cancel();
        let mut i = self.app.lock();
        if let Some(all) = i.terminals.get_mut(&self.sid) {
            all.remove(&self.id);
        }
        self.app.record(&mut i, "terminal_closed", &self.name);
    }
}
async fn run(app: App, session: Session, pending: OwnedSemaphorePermit, mut ws: WebSocket) {
    let first = match timeout(Duration::from_secs(5), ws.recv()).await {
        Ok(Some(Ok(WS::Text(raw)))) if raw.len() <= 2048 => {
            serde_json::from_str::<Authorize>(&raw).ok()
        }
        _ => None,
    };
    let Some(first) = first.filter(|v| v.kind == "authorize" && v.ticket.len() == 64) else {
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
        if node.removing || (!node.policy.terminal && node.policy.files == "off") {
            return Err(ApiError::new(403, "该节点仅允许监控"));
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
        let stop = app.0.stop.child_token();
        let id = token();
        i.terminals
            .entry(session.id.clone())
            .or_default()
            .insert(id.clone(), stop.clone());
        app.record(&mut i, "terminal_opened", &node.public.name);
        Ok((node, link, secret, stop, id, slot))
    })();
    let (node, link, secret, stop, id, _slot) = match authorized {
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
    let _guard = TerminalGuard {
        app: app.clone(),
        sid: session.id.clone(),
        id,
        name: node.public.name.clone(),
        stop: stop.clone(),
    };
    let client = tokio::select! {_=stop.cancelled()=>return,c=ssh::agent_client(&app,link.clone(),&node.public.id,&node.username,&secret)=>match c{Ok(c)=>c,Err(_)=>{let _=ws.send(WS::Text(json!({"type":"error","message":"SSH 验证失败，请检查节点用户、公钥授权与主机指纹"}).to_string().into())).await;return}}};
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
    let channel = if node.policy.terminal {
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
    } else {
        None
    };
    let (shell_read, shell_write) = if let Some(channel) = channel {
        let (r, w) = channel.split();
        (Some(r), Some(Arc::new(w)))
    } else {
        (None, None)
    };
    let (mut ws_write, mut ws_read) = ws.split();
    let (out, mut send_queue) = mpsc::channel::<WS>(8);
    let credits = Arc::new(Semaphore::new(32));
    let activity = Arc::new(Mutex::new(Instant::now()));
    let probe = Arc::new(Mutex::new(None::<(Vec<u8>, Instant)>));
    let mut tasks = JoinSet::new();
    let check: files::Authorize = {
        let app = app.clone();
        let sid = session.id.clone();
        let version = session.version.clone();
        let node = node.public.id.clone();
        let link = link.clone();
        Arc::new(move || app.lock().terminal_valid(&sid, &version, &node, &link))
    };
    let cancel = stop.clone();
    tasks.spawn(async move{loop{tokio::select!{_=cancel.cancelled()=>break,m=send_queue.recv()=>{let Some(m)=m else{break};if !matches!(timeout(Duration::from_secs(5),ws_write.send(m)).await,Ok(Ok(()))){break}}}}cancel.cancel();});
    let _=out.send(WS::Text(json!({"type":"ready","files":node.policy.files!="off","terminal":node.policy.terminal}).to_string().into())).await;
    let (file_tx, mut file_rx) = mpsc::channel::<files::Request>(2);
    let mut file_service = files::Files::new(
        app.clone(),
        client.clone(),
        node.public.id.clone(),
        node.public.name.clone(),
        check.clone(),
    );
    let file_check = check.clone();
    let file_out = out.clone();
    let file_stop = stop.clone();
    tasks.spawn(async move{loop{tokio::select!{_=file_stop.cancelled()=>break,r=file_rx.recv()=>{let Some(r)=r else{break};if !file_check(){break}let value=file_service.process(&r).await;if file_check(){files::respond(&file_out,&r,value).await;}}}}});
    let read_stop = stop.clone();
    let read_app = app.clone();
    let read_activity = activity.clone();
    let read_credits = credits.clone();
    let read_check = check.clone();
    let read_out = out.clone();
    let read_probe = probe.clone();
    let input = shell_write.clone();
    let node_name = node.public.name.clone();
    tasks.spawn(async move {
        let mut window = Instant::now();
        let (mut frames, mut bytes, mut terminal_bytes) = (0usize, 0usize, 0usize);
        let mut file_window = window;
        let mut file_count = 0;
        loop {
            let msg = tokio::select! {_=read_stop.cancelled()=>break,m=ws_read.next()=>m};
            let Some(Ok(msg)) = msg else { break };
            match msg {
                WS::Pong(bytes) => {
                    let mut p = read_probe.lock().unwrap();
                    if p.as_ref()
                        .is_some_and(|(v, _)| v.as_slice() == bytes.as_ref())
                    {
                        *p = None;
                    }
                    continue;
                }
                WS::Ping(bytes) => {
                    if read_out.send(WS::Pong(bytes)).await.is_err() {
                        break;
                    }
                    continue;
                }
                WS::Close(_) => break,
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
            if frames > 300 || bytes > files::MAX_FRAME || !read_check() {
                break;
            }
            match msg {
                WS::Binary(raw) => {
                    let Some(input) = input.as_ref() else {
                        continue;
                    };
                    terminal_bytes += raw.len();
                    if raw.len() > 16384 || terminal_bytes > 256 * 1024 {
                        break;
                    }
                    *read_activity.lock().unwrap() = Instant::now();
                    if !matches!(
                        timeout(Duration::from_secs(5), input.data(raw.as_ref())).await,
                        Ok(Ok(()))
                    ) {
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
                        file_count += 1;
                        if file_count > 90 {
                            files::respond(
                                &read_out,
                                &r,
                                Err(files::Problem {
                                    code: "rate",
                                    message: "文件操作过于频繁，请稍后重试".into(),
                                }),
                            )
                            .await;
                            continue;
                        }
                        if let Err(e) = files::validate(&r) {
                            files::respond(&read_out, &r, Err(e)).await;
                            continue;
                        }
                        match file_tx.try_send(r) {
                            Ok(_) => *read_activity.lock().unwrap() = Instant::now(),
                            Err(e) => {
                                let r = e.into_inner();
                                files::respond(
                                    &read_out,
                                    &r,
                                    Err(files::Problem {
                                        code: "busy",
                                        message: "已有文件操作正在进行，请稍后重试".into(),
                                    }),
                                )
                                .await;
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
                            let Some(input) = input.as_ref() else {
                                continue;
                            };
                            let (cols, rows) = size(control.cols, control.rows);
                            let _ = input.window_change(cols, rows, 0, 0).await;
                        }
                        "command" => {
                            let Some(input) = input.as_ref() else {
                                continue;
                            };
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
                                *read_activity.lock().unwrap() = Instant::now();
                                let command =
                                    format!("{}\n", script.trim_end_matches(['\r', '\n']));
                                if !matches!(
                                    timeout(Duration::from_secs(5), input.data(command.as_bytes()))
                                        .await,
                                    Ok(Ok(()))
                                ) {
                                    break;
                                }
                            } else {
                                notice(&read_out, "notice", "命令不可用，请重新打开终端").await;
                            }
                        }
                        "close" => break,
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
    if let Some(mut shell_read) = shell_read {
        tasks.spawn(async move {
            let mut total = 0usize;
            let mut count = 0usize;
            let mut window = Instant::now();
            let result = async {
                while let Some(msg) = shell_read.wait().await {
                    match msg {
                        ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                            for chunk in data.chunks(16384) {
                                if total + chunk.len() > 64 * 1024 * 1024 {
                                    return;
                                }
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
                                        Duration::from_secs(5),
                                        output.send(WS::Binary(chunk.to_vec().into()))
                                    )
                                    .await,
                                    Ok(Ok(()))
                                ) {
                                    return;
                                }
                                total += chunk.len();
                                count += chunk.len();
                            }
                        }
                        ChannelMsg::Close => {
                            notice(&output, "closed", "SSH 会话已结束").await;
                            break;
                        }
                        _ => {}
                    }
                }
            };
            tokio::select! {_=output_stop.cancelled()=>{},_=result=>{}}
            output_stop.cancel();
        });
    }
    let watch_stop = stop.clone();
    let watch_out = out.clone();
    tasks.spawn(async move{let started=Instant::now();let mut next_ping=Instant::now()+Duration::from_secs(25);let mut timer=tokio::time::interval(Duration::from_secs(1));loop{tokio::select!{_=watch_stop.cancelled()=>break,_=timer.tick()=>{let idle=activity.lock().unwrap().elapsed()>Duration::from_secs(600);let overdue=probe.lock().unwrap().as_ref().is_some_and(|(_,at)|at.elapsed()>Duration::from_secs(5));if idle||overdue||started.elapsed()>Duration::from_secs(3600)||!check(){notice(&watch_out,"error","终端已超时或管理授权失效").await;break}
if Instant::now()>=next_ping{let value=token().as_bytes()[..32].to_vec();*probe.lock().unwrap()=Some((value.clone(),Instant::now()));if !matches!(timeout(Duration::from_secs(5),watch_out.send(WS::Ping(value.into()))).await,Ok(Ok(()))){break}next_ping=Instant::now()+Duration::from_secs(25);}}}}watch_stop.cancel();});
    stop.cancelled().await;
    if let Some(shell) = shell_write {
        let _ = timeout(Duration::from_secs(1), shell.close()).await;
    }
    let _ = timeout(
        Duration::from_secs(1),
        client.disconnect(russh::Disconnect::ByApplication, "session ended", ""),
    )
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}
