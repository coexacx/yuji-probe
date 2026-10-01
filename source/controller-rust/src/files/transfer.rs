use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::digest::common::hazmat::{SerializableState, SerializedState};
use std::time::Instant;
pub(super) const MAX_TRANSFER: u64 = 2 * 1024 * 1024 * 1024;
pub(super) const CHUNK: usize = 128 * 1024;

pub(super) struct Transfer {
    id: String,
    sftp: Sftp,
    handle: String,
    path: String,
    upload: bool,
    size: u64,
    offset: u64,
    initial: FileAttributes,
    hash: Sha256,
    touched: Instant,
    temporary: Option<Temporary>,
    _lock: Option<EditLock>,
}
impl Drop for Transfer {
    fn drop(&mut self) {
        if self.handle.is_empty() {
            return;
        }
        let raw = self.sftp.raw.clone();
        let handle = self.handle.clone();
        tokio::spawn(async move {
            let _ = timeout(Duration::from_secs(2), raw.close(handle)).await;
        });
    }
}
pub(super) fn action(s: &str) -> bool {
    matches!(
        s,
        "mkdir"
            | "rename"
            | "upload_start"
            | "upload_chunk"
            | "upload_finish"
            | "transfer_cancel"
            | "download_start"
            | "download_chunk"
            | "download_finish"
            | "transfer_resume"
            | "transfer_list"
            | "transfer_pause"
    )
}
pub(super) fn chunk(s: &str) -> bool {
    matches!(s, "upload_chunk" | "download_chunk" | "download_finish")
}
fn basename(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !matches!(s, "." | "..")
        && !s.contains('/')
        && !s.chars().any(char::is_control)
}
fn writable(path: &str) -> Result<(), Problem> {
    if ["/proc", "/sys", "/dev", "/run"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
    {
        return Err(error("readonly", "系统运行目录不支持修改"));
    }
    Ok(())
}
fn ordinary(a: &FileAttributes) -> Result<u64, Problem> {
    if mode(a) & 0o170000 != 0o100000 {
        return Err(error("unsupported", "仅支持传输普通文件"));
    }
    a.size
        .filter(|v| *v <= MAX_TRANSFER)
        .ok_or_else(|| error("limit", "单个传输文件最多 2 GiB"))
}
async fn missing(c: &Sftp, path: &str) -> Result<(), Problem> {
    match c.raw.lstat(path).await {
        Err(russh_sftp::client::error::Error::Status(s))
            if s.status_code == StatusCode::NoSuchFile =>
        {
            Ok(())
        }
        Ok(_) => Err(error("exists", "同名文件或目录已存在，请更换名称")),
        Err(e) => Err(e.into()),
    }
}
fn lock(files: &Files, path: &str) -> Result<EditLock, Problem> {
    let key = format!("{}:{path}", files.node);
    if !files.app.lock().file_edits.insert(key.clone()) {
        return Err(error("busy", "该路径正在进行其他文件操作"));
    }
    Ok(EditLock {
        app: files.app.clone(),
        key,
    })
}
async fn child(c: &Sftp, parent: &str, name: &str) -> Result<String, Problem> {
    if !basename(name) {
        return Err(error("invalid", "请输入有效文件名，不能包含斜杠或控制字符"));
    }
    let parent = c.real(parent).await?;
    writable(&parent)?;
    let attr = c.raw.lstat(&parent).await?.attrs;
    if mode(&attr) & 0o170000 != 0o040000 {
        return Err(error("invalid", "所选路径不是目录"));
    }
    Ok(format!("{}/{}", parent.trim_end_matches('/'), name))
}
pub(super) async fn cleanup(files: &mut Files) {
    if let Some(mut t) = files.transfer.take() {
        let _ = std::fs::remove_file(checkpoint_path(&files.app, &t.id));
        if !t.handle.is_empty() {
            let _ = timeout(Duration::from_secs(2), t.sftp.raw.close(&t.handle)).await;
            t.handle.clear();
        }
        if let Some(mut temp) = t.temporary.take()
            && matches!(
                timeout(Duration::from_secs(2), temp.raw.remove(&temp.path)).await,
                Ok(Ok(_))
            )
        {
            temp.committed = true;
        }
    }
}
pub(super) async fn expire(files: &mut Files) {
    if files
        .transfer
        .as_ref()
        .is_some_and(|t| t.touched.elapsed() > Duration::from_secs(60))
    {
        park(files).await;
    }
}
pub(super) async fn handle(files: &mut Files, r: &Request) -> Result<Value, Problem> {
    expire(files).await;
    if r.action == "transfer_list" {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(files.app.0.dir.join("transfers")) {
            for entry in entries.flatten().take(128) {
                let id = entry
                    .file_name()
                    .to_string_lossy()
                    .trim_end_matches(".json")
                    .to_string();
                if let Ok(v) = checkpoint_read(files, &id) {
                    list.push(describe(&v));
                }
            }
        }
        return Ok(json!({"transfers":list}));
    }
    if r.action == "transfer_resume" {
        return resume(files, r).await;
    }
    if r.action == "transfer_pause" {
        park(files).await;
        return Ok(json!({"paused":true}));
    }
    if r.action == "transfer_cancel" && files.transfer.as_ref().is_none_or(|t| t.id != r.transfer) {
        let v = checkpoint_read(files, &r.transfer)?;
        if v.upload {
            let c = Sftp::open(&files.client).await?;
            c.raw.remove(&v.temporary).await?;
        }
        let _ = std::fs::remove_file(checkpoint_path(&files.app, &v.id));
        return Ok(json!({"cancelled":true}));
    }
    if matches!(r.action.as_str(), "mkdir" | "rename") {
        let c = Sftp::open(&files.client).await?;
        if r.action == "mkdir" {
            let path = child(&c, &r.path, &r.target).await?;
            let _lock = lock(files, &path)?;
            missing(&c, &path).await?;
            if !(files.authorized)() {
                return Err(error("session", "管理会话已失效"));
            }
            c.raw
                .mkdir(
                    &path,
                    FileAttributes {
                        permissions: Some(0o700),
                        ..Default::default()
                    },
                )
                .await?;
            files
                .app
                .record(&mut files.app.lock(), "file_directory_created", &files.name);
            return Ok(json!({"path":path}));
        }
        if r.path == "/" {
            return Err(error("invalid", "不能重命名根目录"));
        }
        let (parent, name) = r.path.rsplit_once('/').unwrap();
        if !basename(name) {
            return Err(error("invalid", "文件名称不正确"));
        }
        let parent = c.real(if parent.is_empty() { "/" } else { parent }).await?;
        let old = child(&c, &parent, name).await?;
        let new = child(&c, &parent, &r.target).await?;
        if old == new {
            return Ok(json!({"path":new}));
        }
        let _old = lock(files, &old)?;
        let _new = lock(files, &new)?;
        let attrs = c.raw.lstat(&old).await?.attrs;
        if ![0o040000, 0o100000, 0o120000].contains(&(mode(&attrs) & 0o170000)) {
            return Err(error("unsupported", "该类型不支持重命名"));
        }
        missing(&c, &new).await?;
        if !(files.authorized)() {
            return Err(error("session", "管理会话已失效"));
        }
        // Standard SFTP rename rejects existing destinations; unlike posix-rename it must not overwrite.
        c.raw.rename(&old, &new).await?;
        files.snapshots.clear();
        files.order.clear();
        files
            .app
            .record(&mut files.app.lock(), "file_renamed", &files.name);
        return Ok(json!({"path":new}));
    }
    if matches!(r.action.as_str(), "upload_start" | "download_start") {
        if files.transfer.is_some() {
            return Err(error("busy", "当前会话已有文件传输，请完成或取消后继续"));
        }
        let count = std::fs::read_dir(files.app.0.dir.join("transfers"))
            .map(|r| r.take(65).count())
            .unwrap_or(0);
        if count >= 64 {
            return Err(error("limit", "续传队列已满，请清理不用的传输"));
        }
        let c = Sftp::open_limit(&files.client, MAX_TRANSFER as usize + 8 * 1024 * 1024).await?;
        let upload = r.action == "upload_start";
        let id = token();
        let (path, handle, size, initial, temporary, held) = if upload {
            let path = child(&c, &r.path, &r.target).await?;
            let held = lock(files, &path)?;
            missing(&c, &path).await?;
            let parent = path.rsplit_once('/').unwrap().0;
            let temp = format!("{parent}/.probe-upload-{}", &id[..24]);
            let h = c
                .raw
                .open(
                    &temp,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                    FileAttributes {
                        permissions: Some(0o600),
                        ..Default::default()
                    },
                )
                .await?
                .handle;
            let temporary = Temporary {
                raw: c.raw.clone(),
                path: temp,
                committed: false,
            };
            (
                path,
                h,
                r.size,
                FileAttributes::default(),
                Some(temporary),
                Some(held),
            )
        } else {
            let path = c.real(&r.path).await?;
            let a = c.raw.lstat(&path).await?.attrs;
            ordinary(&a)?;
            let h = c
                .raw
                .open(&path, OpenFlags::READ, FileAttributes::default())
                .await?
                .handle;
            let a = c.raw.fstat(&h).await?.attrs;
            let size = match ordinary(&a) {
                Ok(v) => v,
                Err(e) => {
                    let _ = c.raw.close(&h).await;
                    return Err(e);
                }
            };
            (path, h, size, a, None, None)
        };
        let answer = json!({"transfer":id,"path":path,"name":path.rsplit('/').next().unwrap_or("download"),"size":size,"chunkSize":CHUNK});
        files.transfer = Some(Transfer {
            id,
            sftp: c,
            handle,
            path,
            upload,
            size,
            offset: 0,
            initial,
            hash: Sha256::new(),
            touched: Instant::now(),
            temporary,
            _lock: held,
        });
        let v = snapshot(files, files.transfer.as_ref().unwrap());
        if let Err(e) = checkpoint_write(&files.app, &v) {
            cleanup(files).await;
            return Err(e);
        }
        return Ok(answer);
    }
    let t = files
        .transfer
        .as_mut()
        .filter(|t| constant(&t.id, &r.transfer))
        .ok_or_else(|| error("missing", "文件传输已结束或标识不正确"))?;
    t.touched = Instant::now();
    if r.action == "transfer_cancel" {
        cleanup(files).await;
        return Ok(json!({"cancelled":true}));
    }
    if r.action == "download_chunk" && !t.upload && r.offset as u64 == t.offset {
        let v = snapshot(files, files.transfer.as_ref().unwrap());
        checkpoint_write(&files.app, &v)?;
    }
    let t = files.transfer.as_mut().unwrap();
    if r.offset as u64 != t.offset {
        return Err(error("conflict", "传输位置不一致，请重新开始"));
    }
    if r.action == "upload_chunk" {
        if !t.upload {
            return Err(error("invalid", "文件传输方向不正确"));
        }
        let bytes = STANDARD
            .decode(&r.content)
            .map_err(|_| error("invalid", "文件分块编码不正确"))?;
        if bytes.is_empty() || bytes.len() > CHUNK || t.offset + bytes.len() as u64 > t.size {
            return Err(error("limit", "文件分块超过范围"));
        }
        for (i, part) in bytes.chunks(32768).enumerate() {
            t.sftp
                .raw
                .write(&t.handle, t.offset + (i * 32768) as u64, part.to_vec())
                .await?;
        }
        t.hash.update(&bytes);
        t.offset += bytes.len() as u64;
        let reply = json!({"offset":t.offset,"size":t.size,"sha256":hex::encode(t.hash.clone().finalize())});
        let v = snapshot(files, files.transfer.as_ref().unwrap());
        checkpoint_write(&files.app, &v)?;
        return Ok(reply);
    }
    if r.action == "upload_finish" {
        if !t.upload || t.offset != t.size {
            return Err(error("conflict", "文件尚未完整上传"));
        }
        if !(files.authorized)() {
            return Err(error("session", "管理会话已失效"));
        }
        if !constant(&r.revision, &hex::encode(t.hash.clone().finalize())) {
            return Err(error("conflict", "上传校验失败"));
        }
        if !constant(
            &remote_digest(&files.client, &t.temporary.as_ref().unwrap().path, t.size).await?,
            &r.revision,
        ) {
            return Err(error("conflict", "远端文件校验失败"));
        }
        if t.sftp.raw.fstat(&t.handle).await?.attrs.size != Some(t.size) {
            return Err(error("conflict", "远端文件大小不一致"));
        }
        if t.sftp.fsync {
            t.sftp.raw.fsync(&t.handle).await?;
        }
        t.sftp.raw.close(&t.handle).await?;
        t.handle.clear();
        missing(&t.sftp, &t.path).await?;
        let temp = t.temporary.as_mut().unwrap();
        t.sftp.raw.rename(&temp.path, &t.path).await?;
        temp.committed = true;
        let reply =
            json!({"path":t.path,"size":t.size,"sha256":hex::encode(t.hash.clone().finalize())});
        let _ = std::fs::remove_file(checkpoint_path(&files.app, &t.id));
        files.transfer.take();
        files
            .app
            .record(&mut files.app.lock(), "file_uploaded", &files.name);
        return Ok(reply);
    }
    if r.action == "download_finish" {
        if t.upload
            || t.offset != t.size
            || !constant(&r.revision, &hex::encode(t.hash.clone().finalize()))
        {
            return Err(error("conflict", "下载校验失败"));
        }
        cleanup(files).await;
        return Ok(json!({"ok":true}));
    }
    if r.action == "download_chunk" {
        if t.upload {
            return Err(error("invalid", "文件传输方向不正确"));
        }
        let amount = (t.size - t.offset).min(CHUNK as u64) as usize;
        let bytes = if amount == 0 {
            Vec::new()
        } else {
            let b = t
                .sftp
                .raw
                .read(&t.handle, t.offset, amount as u32)
                .await?
                .data
                .to_vec();
            if b.is_empty() || b.len() > amount {
                return Err(error("conflict", "文件读取大小异常"));
            }
            b
        };
        t.hash.update(&bytes);
        t.offset += bytes.len() as u64;
        let done = t.offset == t.size;
        if done {
            let after = t.sftp.raw.fstat(&t.handle).await?.attrs;
            if after.size != t.initial.size
                || after.mtime != t.initial.mtime
                || mode(&after) != mode(&t.initial)
            {
                cleanup(files).await;
                return Err(error("conflict", "文件在下载期间发生变化，请重试"));
            }
        }
        let reply = json!({"content":STANDARD.encode(&bytes),"offset":t.offset,"done":done,"sha256":hex::encode(t.hash.clone().finalize())});
        return Ok(reply);
    }
    Err(error("invalid", "文件传输操作不正确"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filenames_and_runtime_write_boundaries() {
        for name in ["", ".", "..", "a/b", "x\n", "a\0"] {
            assert!(!basename(name));
        }
        for name in ["notes.txt", "中文 文件.txt", "quote'$.txt"] {
            assert!(basename(name));
        }
        for path in ["/proc", "/proc/1", "/dev/shm", "/run/lock", "/sys"] {
            assert!(writable(path).is_err());
        }
        assert!(writable("/var/tmp").is_ok());
        assert!(
            ordinary(&FileAttributes {
                permissions: Some(0o100600),
                size: Some(MAX_TRANSFER + 1),
                ..Default::default()
            })
            .is_err()
        );
    }
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    id: String,
    node: String,
    session: String,
    version: String,
    path: String,
    temporary: String,
    upload: bool,
    size: u64,
    offset: u64,
    mtime: Option<u32>,
    permissions: Option<u32>,
    hash: String,
    state: String,
    touched: i64,
}
fn checkpoint_path(app: &App, id: &str) -> std::path::PathBuf {
    app.0.dir.join("transfers").join(format!("{id}.json"))
}
fn snapshot(files: &Files, t: &Transfer) -> Checkpoint {
    Checkpoint {
        id: t.id.clone(),
        node: files.node.clone(),
        session: files.file_sessions.clone(),
        version: files.app.lock().auth.version.clone(),
        path: t.path.clone(),
        temporary: t
            .temporary
            .as_ref()
            .map(|v| v.path.clone())
            .unwrap_or_default(),
        upload: t.upload,
        size: t.size,
        offset: t.offset,
        mtime: t.initial.mtime,
        permissions: t.initial.permissions,
        hash: hex::encode(t.hash.clone().finalize()),
        state: STANDARD.encode(t.hash.serialize()),
        touched: now(),
    }
}
fn checkpoint_write(app: &App, v: &Checkpoint) -> Result<(), Problem> {
    let dir = app.0.dir.join("transfers");
    std::fs::create_dir_all(&dir).map_err(|_| error("storage", "无法保存续传状态"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| error("storage", "无法保护续传状态"))?;
    atomic_json(&checkpoint_path(app, &v.id), v).map_err(|_| error("storage", "无法保存续传状态"))
}
fn checkpoint_read(files: &Files, id: &str) -> Result<Checkpoint, Problem> {
    if !crate::file_sessions::valid(id) {
        return Err(error("missing", "续传记录不存在"));
    }
    let v: Checkpoint = read_json(&checkpoint_path(&files.app, id))
        .map_err(|_| error("missing", "续传记录不存在"))?;
    if v.node != files.node
        || v.session != files.file_sessions
        || v.version != files.app.lock().auth.version
        || now() - v.touched > 86400
        || !valid_path(&v.path)
        || v.offset > v.size
        || v.size > MAX_TRANSFER
        || v.id != id
    {
        return Err(error("permission", "续传记录已失效或不属于当前会话"));
    }
    if v.upload {
        let expected = format!(
            "{}/.probe-upload-{}",
            v.path.rsplit_once('/').unwrap().0,
            &id[..24]
        );
        if v.temporary != expected {
            return Err(error("invalid", "续传路径不正确"));
        }
    }
    Ok(v)
}
fn describe(v: &Checkpoint) -> Value {
    json!({"transfer":v.id,"path":v.path,"name":v.path.rsplit('/').next().unwrap_or("download"),"upload":v.upload,"size":v.size,"offset":v.offset,"sha256":v.hash,"chunkSize":CHUNK})
}
pub(super) async fn park(files: &mut Files) {
    if let Some(mut t) = files.transfer.take() {
        // Checkpoints already represent acknowledged writes; never delete their partial file.
        if let Some(tmp) = t.temporary.as_mut() {
            tmp.committed = true;
        }
        if !t.handle.is_empty() {
            let _ = timeout(Duration::from_secs(2), t.sftp.raw.close(&t.handle)).await;
            t.handle.clear();
        }
    }
}
async fn remote_digest(client: &ssh::Client, path: &str, length: u64) -> Result<String, Problem> {
    let py = r#"import os,sys,hashlib,stat
fd=os.open(sys.argv[1],os.O_RDONLY|os.O_NOFOLLOW)
with os.fdopen(fd,'rb') as f:
 a=os.fstat(f.fileno());n=int(sys.argv[2])
 if not stat.S_ISREG(a.st_mode) or a.st_size<n:raise RuntimeError('changed')
 h=hashlib.sha256()
 while n:
  b=f.read(min(n,1048576))
  if not b:raise RuntimeError('short read')
  n-=len(b);h.update(b)
 b=os.fstat(f.fileno())
 if (a.st_size,a.st_mtime_ns,a.st_ctime_ns)!=(b.st_size,b.st_mtime_ns,b.st_ctime_ns):raise RuntimeError('changed')
 print(h.hexdigest())
"#;
    let result = ssh::exec(
        client,
        &format!(
            "python3 -c {} {} {}",
            ssh::quote(py),
            ssh::quote(path),
            length
        ),
        &[],
        4096,
    )
    .await
    .map_err(|_| error("conflict", "远端文件校验失败，请重新开始"))?;
    let s = result.trim();
    if !crate::file_sessions::valid(s) {
        return Err(error("conflict", "远端校验结果不正确"));
    }
    Ok(s.into())
}
async fn resume(files: &mut Files, r: &Request) -> Result<Value, Problem> {
    if files.transfer.is_some() {
        park(files).await;
    }
    let v = checkpoint_read(files, &r.transfer)?;
    if !constant(&v.hash, &r.revision) {
        return Err(error(
            "conflict",
            "已传输内容校验不一致，请选择原文件或重新下载",
        ));
    }
    let c = Sftp::open_limit(&files.client, MAX_TRANSFER as usize + 8 * 1024 * 1024).await?;
    let source = if v.upload { &v.temporary } else { &v.path };
    let a = c.raw.lstat(source).await?.attrs;
    ordinary(&a)?;
    if (!v.upload && (a.size != Some(v.size) || a.mtime != v.mtime))
        || a.size.is_none_or(|s| s < v.offset)
    {
        return Err(error("conflict", "远端文件已经变化，请重新开始"));
    }
    if !constant(
        &remote_digest(&files.client, source, v.offset).await?,
        &v.hash,
    ) {
        return Err(error("conflict", "远端已传输内容发生变化，已阻止续传"));
    }
    let held = if v.upload {
        Some(lock(files, &v.path)?)
    } else {
        None
    };
    if v.upload {
        missing(&c, &v.path).await?;
    }
    let handle = c
        .raw
        .open(
            source,
            if v.upload {
                OpenFlags::WRITE
            } else {
                OpenFlags::READ
            },
            FileAttributes::default(),
        )
        .await?
        .handle;
    if v.upload && a.size != Some(v.offset) {
        c.raw
            .fsetstat(
                &handle,
                FileAttributes {
                    size: Some(v.offset),
                    ..Default::default()
                },
            )
            .await?;
    }
    let raw = STANDARD
        .decode(&v.state)
        .map_err(|_| error("invalid", "续传状态损坏"))?;
    let state = SerializedState::<Sha256>::try_from(raw.as_slice())
        .map_err(|_| error("invalid", "续传状态损坏"))?;
    let hash = Sha256::deserialize(&state).map_err(|_| error("invalid", "续传状态损坏"))?;
    if !constant(&hex::encode(hash.clone().finalize()), &v.hash) {
        return Err(error("invalid", "续传校验状态损坏"));
    }
    let temp = if v.upload {
        Some(Temporary {
            raw: c.raw.clone(),
            path: v.temporary.clone(),
            committed: false,
        })
    } else {
        None
    };
    let reply = describe(&v);
    files.transfer = Some(Transfer {
        id: v.id,
        sftp: c,
        handle,
        path: v.path,
        upload: v.upload,
        size: v.size,
        offset: v.offset,
        initial: a,
        hash,
        touched: Instant::now(),
        temporary: temp,
        _lock: held,
    });
    Ok(reply)
}
pub(crate) fn has_session(app: &App, session: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(app.0.dir.join("transfers")) else {
        return false;
    };
    entries
        .flatten()
        .take(128)
        .any(|e| read_json::<Checkpoint>(&e.path()).is_ok_and(|v| v.session == session))
}
pub(crate) async fn remove_session(app: &App, session: &str, client: &ssh::Client) -> bool {
    let entries = match std::fs::read_dir(app.0.dir.join("transfers")) {
        Ok(entries) => entries,
        Err(e) => return e.kind() == std::io::ErrorKind::NotFound,
    };
    let mut cleaned = true;
    for entry in entries.flatten().take(128) {
        let Ok(v) = read_json::<Checkpoint>(&entry.path()) else {
            continue;
        };
        if v.session != session || !crate::file_sessions::valid(&v.id) || !valid_path(&v.path) {
            continue;
        }
        if v.upload {
            let expected = format!(
                "{}/.probe-upload-{}",
                v.path.rsplit_once('/').unwrap().0,
                &v.id[..24]
            );
            if v.temporary != expected {
                cleaned = false;
                continue;
            }
            let removed = async {
                let c = Sftp::open(client).await?;
                match c.raw.remove(&expected).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let problem: Problem = e.into();
                        if problem.code == "missing" {
                            Ok(())
                        } else {
                            Err(problem)
                        }
                    }
                }
            };
            if !matches!(timeout(Duration::from_secs(3), removed).await, Ok(Ok(()))) {
                // Keep ownership metadata until remote deletion succeeds.
                cleaned = false;
                continue;
            }
        }
        if std::fs::remove_file(entry.path()).is_err() {
            cleaned = false;
        }
    }
    cleaned
}

#[cfg(test)]
mod resume_tests {
    use super::*;
    #[test]
    fn persisted_hash_state_roundtrips_at_arbitrary_offsets() {
        let input = vec![0xa5; 3 * CHUNK + 17];
        for cut in [0, 1, 63, 64, 65, CHUNK, CHUNK + 3, input.len()] {
            let mut first = Sha256::new();
            first.update(&input[..cut]);
            let encoded = STANDARD.encode(first.serialize());
            let raw = STANDARD.decode(encoded).unwrap();
            let saved = SerializedState::<Sha256>::try_from(raw.as_slice()).unwrap();
            let mut resumed = Sha256::deserialize(&saved).unwrap();
            resumed.update(&input[cut..]);
            assert_eq!(resumed.finalize(), Sha256::digest(&input));
        }
    }
    #[test]
    fn file_size_and_mode_limits_are_exact() {
        let attrs = FileAttributes {
            permissions: Some(0o100600),
            size: Some(MAX_TRANSFER),
            ..Default::default()
        };
        assert_eq!(ordinary(&attrs).unwrap(), MAX_TRANSFER);
        assert!(
            ordinary(&FileAttributes {
                size: Some(MAX_TRANSFER + 1),
                ..attrs.clone()
            })
            .is_err()
        );
        assert!(
            ordinary(&FileAttributes {
                permissions: Some(0o120600),
                ..attrs
            })
            .is_err()
        );
    }
}
