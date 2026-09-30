use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::time::Instant;
pub(super) const MAX_TRANSFER: u64 = 128 * 1024 * 1024;
pub(super) const CHUNK: usize = 64 * 1024;

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
    )
}
pub(super) fn chunk(s: &str) -> bool {
    matches!(s, "upload_chunk" | "download_chunk")
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
        .ok_or_else(|| error("limit", "单个传输文件最多 128 MiB"))
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
        cleanup(files).await;
    }
}
pub(super) async fn handle(files: &mut Files, r: &Request) -> Result<Value, Problem> {
    expire(files).await;
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
        let c = Sftp::open_limit(&files.client, MAX_TRANSFER as usize + 2 * 1024 * 1024).await?;
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
        return Ok(json!({"offset":t.offset,"size":t.size}));
    }
    if r.action == "upload_finish" {
        if !t.upload || t.offset != t.size {
            return Err(error("conflict", "文件尚未完整上传"));
        }
        if !(files.authorized)() {
            return Err(error("session", "管理会话已失效"));
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
        files.transfer.take();
        files
            .app
            .record(&mut files.app.lock(), "file_uploaded", &files.name);
        return Ok(reply);
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
        let reply = json!({"content":STANDARD.encode(&bytes),"offset":t.offset,"done":done,"sha256":if done {hex::encode(t.hash.clone().finalize())}else{String::new()}});
        if done {
            cleanup(files).await;
            files
                .app
                .record(&mut files.app.lock(), "file_downloaded", &files.name);
        }
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
