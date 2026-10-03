mod inspector;
pub(crate) mod transfer;
use crate::{core::*, ssh};
use axum::extract::ws::Message as WS;
use russh_sftp::{
    client::{Config, RawSftpSession},
    protocol::{FileAttributes, OpenFlags, Packet, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::mpsc,
    time::timeout,
};
pub const MAX_EDIT: usize = 256 * 1024;
pub const MAX_FRAME: usize = 2 * 1024 * 1024;
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Request {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub action: String,
    pub path: String,
    pub revision: String,
    pub content: String,
    pub offset: i64,
    pub target: String,
    pub transfer: String,
    pub size: u64,
}
#[derive(Clone)]
struct Snapshot {
    path: String,
    fingerprint: String,
    revision: String,
    line_ending: String,
    editable: bool,
}
#[derive(Debug)]
pub struct Problem {
    pub code: &'static str,
    pub message: String,
}
fn error(code: &'static str, message: impl Into<String>) -> Problem {
    Problem {
        code,
        message: message.into(),
    }
}
impl From<russh_sftp::client::error::Error> for Problem {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        use russh_sftp::client::error::Error;
        match e {
            Error::Status(s) if s.status_code == StatusCode::PermissionDenied => {
                error("permission", "当前 SSH 用户没有访问权限")
            }
            Error::Status(s) if s.status_code == StatusCode::NoSuchFile => {
                error("missing", "文件或目录不存在，请刷新列表")
            }
            _ => error("unavailable", "文件操作未完成，请检查连接与远端权限"),
        }
    }
}
pub fn valid_path(p: &str) -> bool {
    p.starts_with('/')
        && p.len() <= 4096
        && !p.contains('\0')
        && (p == "/"
            || (!p.ends_with('/')
                && !p[1..]
                    .split('/')
                    .any(|v| v.is_empty() || v == "." || v == "..")))
}
pub fn parse(raw: &[u8]) -> Option<Request> {
    let r: Request = serde_json::from_slice(raw).ok()?;
    if r.kind != "file"
        || r.id.is_empty()
        || r.id.len() > 64
        || !r
            .id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return None;
    }
    Some(r)
}
pub fn validate(r: &Request) -> Result<(), Problem> {
    if r.revision.len() > 128
        || r.action.len() > 32
        || r.content.len() > MAX_EDIT
        || r.offset < 0
        || r.offset > transfer::MAX_TRANSFER as i64
        || r.size > transfer::MAX_TRANSFER
        || r.transfer.len() > 64
        || r.target.len() > 4096
    {
        return Err(error("limit", "在线编辑最多支持 256 KiB 文本"));
    }
    if !valid_path(&r.path)
        || (!["list", "read", "save", "home"].contains(&r.action.as_str())
            && !transfer::action(&r.action)
            && !inspector::action(&r.action))
    {
        return Err(error("invalid", "文件路径或操作不正确"));
    }
    Ok(())
}
pub fn response(r: &Request, value: Result<Value, Problem>) -> WS {
    let data = match value {
        Ok(v) => json!({"type":"file_result","id":r.id,"action":r.action,"ok":true,"data":v}),
        Err(e) => {
            json!({"type":"file_result","id":r.id,"action":r.action,"ok":false,"code":e.code,"error":e.message})
        }
    };
    let raw = serde_json::to_string(&data).unwrap();
    let raw = if raw.len() > MAX_FRAME {
        json!({"type":"file_result","id":r.id,"ok":false,"code":"limit","error":"响应过大，请选择更小的文件或目录"}).to_string()
    } else {
        raw
    };
    WS::Text(raw.into())
}
pub async fn respond(out: &mpsc::Sender<WS>, r: &Request, value: Result<Value, Problem>) {
    let _ = timeout(Duration::from_secs(8), out.send(response(r, value))).await;
}
struct Limited<S> {
    inner: S,
    left: usize,
}
impl<S: AsyncRead + Unpin> AsyncRead for Limited<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.left == 0 {
            return Poll::Ready(Err(std::io::Error::other("SFTP byte limit")));
        }
        let size = buf.remaining().min(self.left);
        let (result, filled) = {
            let mut limited = ReadBuf::new(buf.initialize_unfilled_to(size));
            let result = Pin::new(&mut self.inner).poll_read(cx, &mut limited);
            (result, limited.filled().len())
        };
        buf.advance(filled);
        self.left -= filled;
        result
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Limited<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        b: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
struct Sftp {
    raw: Arc<RawSftpSession>,
    rename: bool,
    fsync: bool,
}
impl Sftp {
    async fn open(client: &ssh::Client) -> Result<Self, Problem> {
        Self::open_limit(client, 16 * 1024 * 1024).await
    }
    async fn open_limit(client: &ssh::Client, limit: usize) -> Result<Self, Problem> {
        let channel = client
            .channel_open_session()
            .await
            .map_err(|_| error("unavailable", "SFTP 通道不可用"))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|_| error("unsupported", "该 SSH 服务未启用 SFTP 子系统"))?;
        let raw = Arc::new(RawSftpSession::new_with_config(
            Limited {
                inner: channel.into_stream(),
                left: limit,
            },
            Config {
                max_packet_len: 256 * 1024,
                max_concurrent_reads: 1,
                max_concurrent_writes: 1,
                max_write_packet_len: 32768,
                request_timeout_secs: 10,
            },
        ));
        let v = raw.init().await?;
        if v.version != 3 {
            return Err(error("unsupported", "不支持远端 SFTP 版本"));
        }
        Ok(Self {
            rename: v
                .extensions
                .get("posix-rename@openssh.com")
                .is_some_and(|v| v == "1"),
            fsync: v
                .extensions
                .get("fsync@openssh.com")
                .is_some_and(|v| v == "1"),
            raw,
        })
    }
    async fn real(&self, path: &str) -> Result<String, Problem> {
        let v = self.raw.realpath(path).await?;
        let path = v
            .files
            .first()
            .ok_or_else(|| error("invalid", "远端返回了无效路径"))?
            .filename
            .clone();
        if !valid_path(&path) {
            return Err(error("invalid", "远端返回了无效路径"));
        }
        Ok(path)
    }
    async fn read(
        &self,
        path: &str,
        initial: &FileAttributes,
    ) -> Result<(Vec<u8>, FileAttributes), Problem> {
        regular(initial)?;
        let handle = self
            .raw
            .open(path, OpenFlags::READ, FileAttributes::default())
            .await?
            .handle;
        let before = self.raw.fstat(&handle).await?.attrs;
        regular(&before)?;
        let size = before.size.unwrap() as usize;
        let mut bytes = Vec::with_capacity(size + 1);
        while bytes.len() <= size {
            let count = (size + 1 - bytes.len()).min(32768);
            match self
                .raw
                .read(&handle, bytes.len() as u64, count as u32)
                .await
            {
                Ok(v) => {
                    if v.data.len() > count {
                        return Err(error("limit", "文件响应超过请求范围"));
                    }
                    if v.data.is_empty() {
                        break;
                    }
                    bytes.extend(v.data);
                }
                Err(russh_sftp::client::error::Error::Status(s))
                    if s.status_code == StatusCode::Eof =>
                {
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
        let after = self.raw.fstat(&handle).await?.attrs;
        self.raw.close(handle).await?;
        if bytes.len() > MAX_EDIT {
            return Err(error("limit", "文件超过 256 KiB，暂不支持在线编辑"));
        }
        if before.size != after.size
            || before.mtime != after.mtime
            || after.size != Some(bytes.len() as u64)
        {
            return Err(error("conflict", "文件正在变化，请稍后重新读取"));
        }
        regular(&after)?;
        Ok((bytes, after))
    }
    async fn replace(&self, old: &str, new: &str) -> Result<(), Problem> {
        if !self.rename {
            return Err(error("readonly", "远端不支持安全替换，仅供查看"));
        }
        let mut data = Vec::new();
        for s in [old, new] {
            data.extend((s.len() as u32).to_be_bytes());
            data.extend(s.as_bytes());
        }
        match self.raw.extended("posix-rename@openssh.com", data).await? {
            Packet::Status(s) if s.status_code == StatusCode::Ok => Ok(()),
            Packet::Status(s) => Err(russh_sftp::client::error::Error::Status(s).into()),
            _ => Err(error("unavailable", "文件替换未得到确认")),
        }
    }
}
struct Temporary {
    raw: Arc<RawSftpSession>,
    path: String,
    committed: bool,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let raw = self.raw.clone();
        let path = self.path.clone();
        tokio::spawn(async move {
            let _ = timeout(Duration::from_secs(3), raw.remove(path)).await;
        });
    }
}
fn mode(a: &FileAttributes) -> u32 {
    a.permissions.unwrap_or(0)
}
fn regular(a: &FileAttributes) -> Result<(), Problem> {
    if mode(a) & 0o170000 != 0o100000 {
        return Err(error("unsupported", "仅支持读取和编辑普通文本文件"));
    }
    if a.size.is_none_or(|v| v > MAX_EDIT as u64) {
        return Err(error("limit", "文件超过 256 KiB，暂不支持在线编辑"));
    }
    Ok(())
}
fn text_file(b: &[u8]) -> bool {
    std::str::from_utf8(b).is_ok()
        && b.iter()
            .all(|b| *b != 0 && (*b >= 32 || b"\t\n\r".contains(b)))
}
fn line_ending(b: &[u8]) -> &'static str {
    let crlf = b.windows(2).filter(|p| *p == b"\r\n").count();
    let cr = b.iter().filter(|b| **b == b'\r').count();
    let lf = b.iter().filter(|b| **b == b'\n').count();
    if cr > crlf || (crlf > 0 && crlf < lf) {
        "mixed"
    } else if crlf > 0 {
        "crlf"
    } else {
        "lf"
    }
}
fn fingerprint(a: &FileAttributes, b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(format!(
        "{}:{}:{}:{}:{}:",
        a.size.unwrap_or(0),
        mode(a),
        a.mtime.unwrap_or(0),
        a.uid.unwrap_or(0),
        a.gid.unwrap_or(0)
    ));
    h.update(b);
    hex::encode(h.finalize())
}
fn mode_text(a: &FileAttributes) -> String {
    let p = mode(a);
    let mut s = String::from(match p & 0o170000 {
        0o040000 => "d",
        0o120000 => "L",
        0o100000 => "-",
        _ => "?",
    });
    for shift in [6, 3, 0] {
        for (bit, c) in [(4, 'r'), (2, 'w'), (1, 'x')] {
            s.push(if (p >> shift) & bit != 0 { c } else { '-' })
        }
    }
    s
}
pub type Authorize = Arc<dyn Fn() -> bool + Send + Sync>;
pub struct Files {
    transfer: Option<transfer::Transfer>,
    inspector: inspector::Inspector,
    app: App,
    client: Arc<ssh::Client>,
    node: String,
    name: String,
    file_sessions: String,
    authorized: Authorize,
    snapshots: HashMap<String, Snapshot>,
    order: VecDeque<String>,
}
struct EditLock {
    app: App,
    key: String,
}
impl Drop for EditLock {
    fn drop(&mut self) {
        self.app.lock().file_edits.remove(&self.key);
    }
}
impl Files {
    pub fn new(
        app: App,
        client: Arc<ssh::Client>,
        node: String,
        name: String,
        file_sessions: String,
        authorized: Authorize,
    ) -> Self {
        Self {
            transfer: None,
            inspector: inspector::Inspector::default(),
            app,
            client,
            node,
            name,
            file_sessions,
            authorized,
            snapshots: HashMap::new(),
            order: VecDeque::new(),
        }
    }
    pub async fn process(&mut self, r: &Request) -> Result<Value, Problem> {
        validate(r)?;
        let allowed = {
            let i = self.app.lock();
            i.data
                .nodes
                .iter()
                .find(|n| n.public.id == self.node)
                .is_some_and(|n| !n.removing)
        };
        if !allowed {
            return Err(error("permission", "该节点不允许此文件操作"));
        }
        if !(self.authorized)() {
            return Err(error("session", "管理会话已失效"));
        }
        let duration = if matches!(r.action.as_str(), "transfer_resume" | "upload_finish") {
            120
        } else if r.action == "save" {
            45
        } else {
            20
        };
        match timeout(Duration::from_secs(duration), self.handle(r)).await {
            Ok(v) => v,
            Err(_) => {
                let _ = timeout(
                    Duration::from_secs(1),
                    self.client.disconnect(
                        russh::Disconnect::ByApplication,
                        "operation timeout",
                        "",
                    ),
                )
                .await;
                Err(error("timeout", "文件操作超时，请重新连接"))
            }
        }
    }
    pub async fn cleanup(&mut self) {
        transfer::park(self).await;
    }
    pub async fn expire(&mut self) {
        transfer::expire(self).await;
    }
    async fn handle(&mut self, r: &Request) -> Result<Value, Problem> {
        if inspector::action(&r.action) {
            return self.inspector.handle(&self.client, r).await;
        }
        if transfer::action(&r.action) {
            return transfer::handle(self, r).await;
        }
        let c = Sftp::open(&self.client).await?;
        let canonical = c
            .real(if r.action == "home" { "." } else { &r.path })
            .await?;
        let attr = c.raw.lstat(&canonical).await?.attrs;
        if r.action == "list"
            || r.action == "home"
            || (r.action == "read" && mode(&attr) & 0o170000 == 0o040000)
        {
            return self.list(&c, &canonical, &attr, r.offset as usize).await;
        }
        if r.action == "read" {
            return self.read(&c, &r.path, &canonical, &attr).await;
        }
        self.save(&c, r, &canonical, &attr).await
    }
    async fn list(
        &self,
        c: &Sftp,
        path: &str,
        attr: &FileAttributes,
        mut offset: usize,
    ) -> Result<Value, Problem> {
        if mode(attr) & 0o170000 != 0o040000 {
            return Err(error("invalid", "所选路径不是目录"));
        }
        let handle = c.raw.opendir(path).await?.handle;
        let mut list = Vec::new();
        loop {
            match c.raw.readdir(&handle).await {
                Ok(v) => {
                    if v.files.is_empty() {
                        break;
                    }
                    if list.len() + v.files.len() > 10002 {
                        return Err(error(
                            "limit",
                            "目录超过 10000 项，请直接输入子目录或文件路径",
                        ));
                    }
                    list.extend(
                        v.files
                            .into_iter()
                            .filter(|v| v.filename != "." && v.filename != ".."),
                    );
                    if list.len() > 10000 {
                        return Err(error(
                            "limit",
                            "目录超过 10000 项，请直接输入子目录或文件路径",
                        ));
                    }
                }
                Err(russh_sftp::client::error::Error::Status(s))
                    if s.status_code == StatusCode::Eof =>
                {
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
        c.raw.close(handle).await?;
        list.sort_by(|a, b| {
            let ad = mode(&a.attrs) & 0o170000 == 0o040000;
            let bd = mode(&b.attrs) & 0o170000 == 0o040000;
            bd.cmp(&ad).then(a.filename.cmp(&b.filename))
        });
        let end = (offset + 150).min(list.len());
        offset = offset.min(end);
        let mut out = Vec::new();
        for f in &list[offset..end] {
            if f.filename.contains(['\0', '/']) {
                continue;
            }
            let full = format!("{}/{}", path.trim_end_matches('/'), f.filename);
            let kind = match mode(&f.attrs) & 0o170000 {
                0o040000 => "directory",
                0o120000 => "link",
                0o100000 => "file",
                _ => "other",
            };
            out.push(json!({"name":f.filename,"path":full,"kind":kind,"size":f.attrs.size.unwrap_or(0),"mode":mode_text(&f.attrs),"modified":f.attrs.mtime.unwrap_or(0)}));
        }
        Ok(
            json!({"kind":"directory","path":path,"entries":out,"total":list.len(),"offset":offset,"nextOffset":if end<list.len(){end as i64}else{-1}}),
        )
    }
    fn remember(
        &mut self,
        requested: &str,
        path: &str,
        attr: &FileAttributes,
        bytes: &[u8],
        editable: bool,
    ) -> Snapshot {
        let snapshot = Snapshot {
            path: path.into(),
            fingerprint: fingerprint(attr, bytes),
            revision: token(),
            line_ending: line_ending(bytes).into(),
            editable,
        };
        if !self.snapshots.contains_key(requested) {
            self.order.push_back(requested.into());
        }
        self.snapshots.insert(requested.into(), snapshot.clone());
        if self.order.len() > 16
            && let Some(old) = self.order.pop_front()
        {
            self.snapshots.remove(&old);
        }
        snapshot
    }
    fn view(
        requested: &str,
        path: &str,
        a: &FileAttributes,
        b: &[u8],
        s: &Snapshot,
        reason: &str,
    ) -> Value {
        json!({"kind":"file","path":path,"requestedPath":requested,"content":String::from_utf8_lossy(b),"revision":s.revision,"bytes":b.len(),"mode":mode_text(a),"modified":a.mtime.unwrap_or(0),"readOnly":!s.editable,"reason":reason,"lineEnding":s.line_ending})
    }
    async fn reason(&self, c: &Sftp, path: &str, bytes: &[u8]) -> String {
        if ["/proc", "/sys", "/dev", "/run"]
            .iter()
            .any(|base| path == *base || path.starts_with(&format!("{base}/")))
        {
            return "系统运行目录仅供查看".into();
        }
        if line_ending(bytes) == "mixed" {
            return "文件使用混合换行格式，仅供查看".into();
        }
        if !c.rename {
            return "远端不支持安全替换，仅供查看".into();
        }
        match ssh::exec(
            &self.client,
            &format!("/usr/bin/stat --printf='%h' -- {}", ssh::quote(path)),
            &[],
            4096,
        )
        .await
        {
            Ok(v) => match v.trim().parse::<u32>() {
                Ok(1) => String::new(),
                Ok(_) => "文件存在硬链接，仅供查看以保留链接关系".into(),
                Err(_) => "无法核对文件属性，仅供查看".into(),
            },
            Err(_) => "无法核对文件属性，仅供查看".into(),
        }
    }
    async fn read(
        &mut self,
        c: &Sftp,
        requested: &str,
        path: &str,
        a: &FileAttributes,
    ) -> Result<Value, Problem> {
        let (bytes, attr) = c.read(path, a).await?;
        if !text_file(&bytes) {
            return Err(error("binary", "该文件不是 UTF-8 文本，不能在线编辑"));
        }
        let reason = self.reason(c, path, &bytes).await;
        let snapshot = self.remember(requested, path, &attr, &bytes, reason.is_empty());
        self.app
            .record(&mut self.app.lock(), "file_read", &self.name);
        Ok(Self::view(
            requested, path, &attr, &bytes, &snapshot, &reason,
        ))
    }
    async fn save(
        &mut self,
        c: &Sftp,
        r: &Request,
        path: &str,
        a: &FileAttributes,
    ) -> Result<Value, Problem> {
        let snapshot = self
            .snapshots
            .get(&r.path)
            .filter(|s| !r.revision.is_empty() && s.revision == r.revision && s.path == path)
            .cloned()
            .ok_or_else(|| error("conflict", "文件版本已变化，请重新读取后合并修改"))?;
        if !snapshot.editable {
            return Err(error("readonly", "该文件当前仅供查看"));
        }
        let key = format!("{}\0{path}", self.node);
        if !self.app.lock().file_edits.insert(key.clone()) {
            return Err(error("busy", "其他会话正在保存此文件，请稍后重试"));
        }
        let _lock = EditLock {
            app: self.app.clone(),
            key,
        };
        let (old, attr) = c.read(path, a).await?;
        if fingerprint(&attr, &old) != snapshot.fingerprint {
            return Err(error(
                "conflict",
                "文件已被其他程序修改。当前编辑已保留，请重新读取后合并",
            ));
        }
        let reason = self.reason(c, path, &old).await;
        if !reason.is_empty() {
            return Err(error("readonly", reason));
        }
        if !text_file(r.content.as_bytes()) {
            return Err(error(
                "binary",
                "仅支持 UTF-8 文本，不能写入二进制或控制字符",
            ));
        }
        let content = if snapshot.line_ending == "crlf" {
            r.content
                .replace("\r\n", "\n")
                .replace('\n', "\r\n")
                .into_bytes()
        } else {
            r.content.as_bytes().to_vec()
        };
        if content.len() > MAX_EDIT {
            return Err(error("limit", "在线编辑最多支持 256 KiB 文本"));
        }
        if content == old {
            return Ok(Self::view(&r.path, path, &attr, &old, &snapshot, ""));
        }
        let parent = path.rsplit_once('/').unwrap().0;
        let temporary = format!("{parent}/.probe-edit-{}", &token()[..24]);
        let handle = c
            .raw
            .open(
                &temporary,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                FileAttributes {
                    permissions: Some(0o600),
                    ..Default::default()
                },
            )
            .await?
            .handle;
        let mut cleanup = Temporary {
            raw: c.raw.clone(),
            path: temporary.clone(),
            committed: false,
        };
        let result = async {
            for (index, chunk) in content.chunks(32768).enumerate() {
                c.raw
                    .write(&handle, (index * 32768) as u64, chunk.to_vec())
                    .await?;
            }
            if c.fsync {
                c.raw.fsync(&handle).await?;
            }
            c.raw.close(&handle).await?;
            ssh::exec(
                &self.client,
                &format!(
                    "/usr/bin/cp --attributes-only --preserve=mode,ownership,xattr -- {} {}",
                    ssh::quote(path),
                    ssh::quote(&temporary)
                ),
                &[],
                4096,
            )
            .await
            .map_err(|_| error("metadata", "无法完整保留权限与扩展属性，原文件未修改"))?;
            if c.fsync {
                let h = c
                    .raw
                    .open(&temporary, OpenFlags::WRITE, FileAttributes::default())
                    .await?
                    .handle;
                c.raw.fsync(&h).await?;
                c.raw.close(h).await?;
            }
            if c.real(&r.path).await? != path {
                return Err(error("conflict", "目标路径已变化，原文件未修改"));
            }
            let latest = c.raw.lstat(path).await?.attrs;
            let (bytes, latest) = c.read(path, &latest).await?;
            if fingerprint(&latest, &bytes) != snapshot.fingerprint {
                return Err(error("conflict", "保存期间文件已变化，请重新读取后合并"));
            }
            if !(self.authorized)() {
                return Err(error("session", "管理会话已失效，保存已取消"));
            }
            c.replace(&temporary, path).await?;
            cleanup.committed = true;
            Ok::<(), Problem>(())
        }
        .await;
        if let Err(e) = result {
            let _ = c.raw.remove(&temporary).await;
            return Err(e);
        }
        let updated = c
            .raw
            .stat(path)
            .await
            .map_err(|_| error("verify", "保存已提交，但无法确认文件状态，请重新读取核对"))?
            .attrs;
        let s = self.remember(&r.path, path, &updated, &content, true);
        self.app
            .record(&mut self.app.lock(), "file_saved", &self.name);
        Ok(Self::view(&r.path, path, &updated, &content, &s, ""))
    }
}

pub fn is_transfer(r: &Request) -> bool {
    r.action.starts_with("upload_")
        || r.action.starts_with("download_")
        || r.action.starts_with("transfer_")
}
pub fn is_chunk(r: &Request) -> bool {
    transfer::chunk(&r.action)
}
pub fn is_inspection(r: &Request) -> bool {
    inspector::action(&r.action)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_text_boundaries() {
        for p in ["/a/../b", "/a//b", "/a/", "relative", "/a\0b"] {
            assert!(!valid_path(p));
        }
        assert!(valid_path("/"));
        assert!(valid_path("/root/a file.txt"));
        assert!(!text_file(b"a\0b"));
        assert!(!text_file(&[0xff]));
        assert_eq!(line_ending(b"a\r\nb\n"), "mixed");
        assert_eq!(line_ending(b"a\r\n"), "crlf");
        let a = FileAttributes {
            permissions: Some(0o120777),
            size: Some(1),
            ..Default::default()
        };
        assert!(regular(&a).is_err());
    }
    #[test]
    fn file_request_unknown_fields() {
        assert!(
            parse(br#"{"type":"file","id":"ok","action":"read","path":"/a","extra":true}"#)
                .is_none()
        );
    }
}
