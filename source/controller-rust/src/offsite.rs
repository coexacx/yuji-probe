use crate::{auth, core::*, operations};
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use zeroize::Zeroizing;

const LABEL: &str = "backup:offsite-secret";
const LIMIT: usize = 24 * 1024 * 1024;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Object {
    pub name: String,
    pub key: String,
    pub size: usize,
    pub sha256: String,
    pub at: i64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub enabled: bool,
    pub kind: String,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub prefix: String,
    pub identity: String,
    pub secret: String,
    pub keep: usize,
    pub last: i64,
    pub error: String,
    pub pending: String,
    pub next: i64,
    pub attempts: u32,
    pub objects: Vec<Object>,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct Input {
    enabled: bool,
    kind: String,
    endpoint: String,
    bucket: String,
    region: String,
    prefix: String,
    identity: String,
    secret: String,
    keep: usize,
    name: String,
}
fn err(s: &str) -> ApiError {
    ApiError::new(400, s)
}
fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn valid_name(s: &str) -> bool {
    s.starts_with("yuji-") && s.ends_with(".backup") && segment(s)
}
fn endpoint(s: &str) -> ApiResult<Url> {
    let u = Url::parse(s).map_err(|_| err("请输入有效的 HTTPS 存储地址"))?;
    if s.len() > 1024
        || u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path().split('/').any(|p| !p.is_empty() && !segment(p))
    {
        return Err(err(
            "存储地址须为 HTTPS，且不能包含登录信息、查询参数或特殊路径",
        ));
    }
    Ok(u)
}
fn config_ok(s: &State) -> ApiResult<()> {
    endpoint(&s.endpoint)?;
    if !matches!(s.kind.as_str(), "s3" | "webdav")
        || !(1..=30).contains(&s.keep)
        || s.prefix.len() > 240
        || s.prefix.split('/').count() > 8
        || (!s.prefix.is_empty() && !s.prefix.split('/').all(segment))
        || s.identity.is_empty()
        || s.identity.len() > 200
        || s.identity.chars().any(char::is_control)
        || s.secret.is_empty()
    {
        return Err(err("请检查存储类型、凭据、目录前缀和保留份数"));
    }
    if s.kind == "s3" && (!segment(&s.bucket) || !segment(&s.region) || s.identity.contains(':')) {
        return Err(err("请填写有效的 Bucket、Region 和 Access Key ID"));
    }
    Ok(())
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let a = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_documentation()
                || a[0] == 0
                || a[0] >= 240
                || (a[0] == 100 && (64..=127).contains(&a[1]))
                || (a[0] == 198 && (a[1] == 18 || a[1] == 19))
                || (a[0] == 192 && a[1] == 0 && a[2] == 0))
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let s = v.segments();
            // Only ordinary global unicast; exclude transition and documentation networks.
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] <= 0x01ff || s[1] == 0x0db8))
                && !(s[0] == 0x3fff && (s[1] & 0xf000) == 0)
        }
    }
}
struct Remote {
    client: Client,
    base: Url,
    secret: Zeroizing<String>,
    config: State,
}
impl Remote {
    async fn new(app: &App, s: &State) -> ApiResult<Self> {
        config_ok(s)?;
        let base = endpoint(&s.endpoint)?;
        let host = base.host_str().unwrap().trim_matches(['[', ']']);
        let port = base.port_or_known_default().unwrap_or(443);
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| err("存储地址解析超时"))?
        .map_err(|_| err("存储地址解析失败"))?
        .take(33)
        .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses.iter().any(|a| !public_ip(a.ip()))
        {
            return Err(err("异地存储须使用公网地址，不能访问本机、内网或保留地址"));
        }
        let tls =
            vistart_probe_agent::connection::tls_config().map_err(|_| ApiError::internal())?;
        let client = Client::builder()
            .tls_backend_preconfigured((*tls).clone())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| ApiError::internal())?;
        let raw = app
            .unseal(LABEL, &s.secret)
            .map_err(|_| err("存储凭据无法解密，请重新填写"))?;
        let secret = Zeroizing::new(
            std::str::from_utf8(&raw)
                .map_err(|_| err("存储凭据无效"))?
                .to_owned(),
        );
        Ok(Self {
            client,
            base,
            secret,
            config: s.clone(),
        })
    }
    fn url(&self, key: &str) -> Url {
        let mut u = self.base.clone();
        let path = format!(
            "{}/{}{}",
            u.path().trim_end_matches('/'),
            if self.config.kind == "s3" {
                format!("{}/", self.config.bucket)
            } else {
                String::new()
            },
            key
        );
        u.set_path(&path);
        u
    }
    async fn request(&self, method: Method, key: &str, body: Vec<u8>) -> ApiResult<(u16, Vec<u8>)> {
        let url = self.url(key);
        let mut request = self.client.request(method.clone(), url.clone());
        if self.config.kind == "s3" {
            let date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let hash = hex::encode(Sha256::digest(&body));
            let host = format!(
                "{}{}",
                url.host_str().unwrap_or(""),
                url.port().map(|p| format!(":{p}")).unwrap_or_default()
            );
            let authorization = sign(
                method.as_ref(),
                url.path(),
                &host,
                &date,
                &hash,
                &self.config.region,
                &self.config.identity,
                &self.secret,
            );
            request = request
                .header("x-amz-date", date)
                .header("x-amz-content-sha256", hash)
                .header("Authorization", authorization);
        } else {
            request = request.basic_auth(&self.config.identity, Some(self.secret.as_str()));
        }
        let mut res = request
            .body(body)
            .send()
            .await
            .map_err(|_| err("异地存储连接失败，请检查地址、证书和网络"))?;
        let status = res.status().as_u16();
        let cap = if method == Method::GET {
            LIMIT
        } else {
            16 * 1024
        };
        if res.content_length().is_some_and(|n| n > cap as u64) {
            return Err(err("异地存储响应超过大小限制"));
        }
        let mut output = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(|_| err("异地存储传输中断"))? {
            if output.len() + chunk.len() > cap {
                return Err(err("异地存储响应超过大小限制"));
            }
            output.extend_from_slice(&chunk);
        }
        Ok((status, output))
    }
    async fn prepare(&self) -> ApiResult<()> {
        if self.config.kind == "webdav" && !self.config.prefix.is_empty() {
            let mut part = String::new();
            for p in self.config.prefix.split('/') {
                if !part.is_empty() {
                    part.push('/');
                }
                part.push_str(p);
                let (status, _) = self
                    .request(Method::from_bytes(b"MKCOL").unwrap(), &part, Vec::new())
                    .await?;
                if ![200, 201, 204, 405].contains(&status) {
                    return Err(remote_error(status));
                }
            }
        }
        Ok(())
    }
    async fn put(&self, key: &str, bytes: Vec<u8>) -> ApiResult<()> {
        let (s, _) = self.request(Method::PUT, key, bytes).await?;
        if !(200..300).contains(&s) {
            return Err(remote_error(s));
        }
        Ok(())
    }
    async fn get(&self, key: &str) -> ApiResult<Vec<u8>> {
        let (s, b) = self.request(Method::GET, key, Vec::new()).await?;
        if s != 200 {
            return Err(remote_error(s));
        }
        Ok(b)
    }
    async fn delete(&self, key: &str) -> ApiResult<()> {
        let (s, _) = self.request(Method::DELETE, key, Vec::new()).await?;
        if !(200..300).contains(&s) && s != 404 {
            return Err(remote_error(s));
        }
        Ok(())
    }
}
fn remote_error(status: u16) -> ApiError {
    err(&format!(
        "异地存储返回 HTTP {status}，请检查权限、目录和存储配置"
    ))
}
fn keyed(key: &[u8], value: &str) -> Vec<u8> {
    let mut h =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts arbitrary key sizes");
    h.update(value.as_bytes());
    h.finalize().into_bytes().to_vec()
}
#[allow(clippy::too_many_arguments)]
fn sign(
    method: &str,
    path: &str,
    host: &str,
    date: &str,
    hash: &str,
    region: &str,
    id: &str,
    secret: &str,
) -> String {
    let day = &date[..8];
    let headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical = format!(
        "{method}\n{path}\n\nhost:{host}\nx-amz-content-sha256:{hash}\nx-amz-date:{date}\n\n{headers}\n{hash}"
    );
    let scope = format!("{day}/{region}/s3/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let a = Zeroizing::new(format!("AWS4{secret}"));
    let b = Zeroizing::new(keyed(a.as_bytes(), day));
    let c = Zeroizing::new(keyed(&b, region));
    let d = Zeroizing::new(keyed(&c, "s3"));
    let k = Zeroizing::new(keyed(&d, "aws4_request"));
    format!(
        "AWS4-HMAC-SHA256 Credential={id}/{scope}, SignedHeaders={headers}, Signature={}",
        hex::encode(keyed(&k, &to_sign))
    )
}
fn key(s: &State, name: &str) -> String {
    if s.prefix.is_empty() {
        name.into()
    } else {
        format!("{}/{name}", s.prefix)
    }
}
fn view(s: &State, busy: bool) -> Value {
    json!({"enabled":s.enabled,"kind":s.kind,"endpoint":s.endpoint,"bucket":s.bucket,"region":s.region,
        "prefix":s.prefix,"identity":s.identity,"hasSecret":!s.secret.is_empty(),"keep":s.keep,
        "last":s.last,"error":s.error,"pending":s.pending,"next":s.next,"attempts":s.attempts,"busy":busy,"objects":s.objects})
}
pub async fn api(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let v: Input = if body.is_empty() {
        Input::default()
    } else {
        decode(&body)?
    };
    let action = c.path.strip_prefix("/api/admin/ops/offsite").unwrap_or("");
    let s = {
        let mut i = app.lock();
        app.guard(&mut i, &c, true, c.method != "GET")?;
        if c.method != "GET" {
            auth::require_elevated(&i, &c)?;
        }
        if action.is_empty() && c.method == "GET" {
            return Ok(ApiReply::ok(view(&i.ops.offsite, i.ops.busy)));
        }
        if i.ops.busy {
            return Err(ApiError::new(409, "备份任务正在进行，请稍后重试"));
        }
        if action.is_empty() && c.method == "POST" {
            let mut s = i.ops.offsite.clone();
            if !v.enabled && v.endpoint.trim().is_empty() {
                s.enabled = false;
            } else {
                s.enabled = v.enabled;
                s.kind = v.kind;
                s.endpoint = v.endpoint.trim().trim_end_matches('/').to_owned();
                s.bucket = v.bucket.trim().into();
                s.region = v.region.trim().into();
                s.prefix = v.prefix.trim().trim_matches('/').into();
                s.identity = v.identity.trim().into();
                s.keep = v.keep;
                if !v.secret.is_empty() {
                    if v.secret.len() > 4096 || v.secret.chars().any(char::is_control) {
                        return Err(err("存储密钥长度或格式不正确"));
                    }
                    s.secret = app
                        .seal(LABEL, v.secret.as_bytes())
                        .map_err(|_| ApiError::internal())?;
                }
                config_ok(&s)?;
                let old = &i.ops.offsite;
                if s.endpoint != old.endpoint
                    || s.kind != old.kind
                    || s.bucket != old.bucket
                    || s.prefix != old.prefix
                {
                    s.objects.clear();
                    s.pending.clear();
                    s.last = 0;
                    s.error.clear();
                    s.attempts = 0;
                }
            }
            let old = std::mem::replace(&mut i.ops.offsite, s);
            if let Err(e) = operations::save(&app, &i) {
                i.ops.offsite = old;
                return Err(e);
            }
            app.record(&mut i, "offsite_configured", &c.ip);
            return Ok(ApiReply::ok(view(&i.ops.offsite, false)));
        }
        if c.method != "POST" {
            return Err(ApiError::new(404, "接口不存在"));
        }
        i.request("offsite", 12)?;
        i.ops.offsite.clone()
    };
    match action {
        "/run" => {
            if !s.enabled {
                return Err(err("请先启用异地备份"));
            }
            config_ok(&s)?;
            crate::backup::start_scheduled(&app, true)?;
            Ok(ApiReply::accepted(json!({"ok":true})))
        }
        "/retry" => {
            let mut i = app.lock();
            if !s.enabled || s.pending.is_empty() {
                return Err(err("没有待重试的异地备份"));
            }
            i.ops.offsite.next = 0;
            i.ops.offsite.attempts = 0;
            operations::save(&app, &i)?;
            Ok(ApiReply::accepted(json!({"ok":true})))
        }
        "/test" | "/download" => {
            let object = if action == "/download" {
                Some(
                    s.objects
                        .iter()
                        .find(|o| {
                            o.name == v.name && valid_name(&o.name) && o.key == key(&s, &o.name)
                        })
                        .cloned()
                        .ok_or_else(|| err("备份不在当前存储记录中"))?,
                )
            } else {
                None
            };
            {
                let mut i = app.lock();
                if i.ops.busy {
                    return Err(ApiError::new(409, "备份任务正在进行"));
                }
                i.ops.busy = true;
            }
            // Keep cleanup independent of an HTTP client disconnect.
            let task_app = app.clone();
            let task = tokio::spawn(async move {
                let result = tokio::time::timeout(Duration::from_secs(45), async {
                    let remote = Remote::new(&task_app, &s).await?;
                    if let Some(o) = object {
                        let b = remote.get(&o.key).await?;
                        if b.len() != o.size || hex::encode(Sha256::digest(&b)) != o.sha256 {
                            return Err(err("备份完整性校验失败"));
                        }
                        let backup: Value =
                            serde_json::from_slice(&b).map_err(|_| err("备份格式不正确"))?;
                        Ok(ApiReply::ok(json!({"name":o.name,"backup":backup})))
                    } else {
                        remote.prepare().await?;
                        let k = key(&s, &format!("yuji-check-{}.tmp", &token()[..24]));
                        let content = token().into_bytes();
                        remote.put(&k, content.clone()).await?;
                        let got = remote.get(&k).await;
                        let removed = remote.delete(&k).await;
                        if got? != content {
                            return Err(err("异地存储读回校验失败"));
                        }
                        removed?;
                        Ok(ApiReply::ok(
                            json!({"ok":true,"message":"写入、读取和删除验证通过"}),
                        ))
                    }
                })
                .await
                .unwrap_or_else(|_| Err(err("异地存储操作超时，请检查存储状态后重试")));
                task_app.lock().ops.busy = false;
                result
            });
            let reply = task.await.map_err(|_| ApiError::internal())??;
            app.guard(&mut app.lock(), &c, true, true)?;
            Ok(reply)
        }
        _ => Err(ApiError::new(404, "接口不存在")),
    }
}
pub fn enqueue(i: &mut Inner, name: &str) {
    if i.ops.offsite.enabled {
        i.ops.offsite.pending = name.into();
        i.ops.offsite.next = 0;
        i.ops.offsite.attempts = 0;
    }
}
pub async fn tick(app: &App) {
    let s = {
        let mut i = app.lock();
        let s = &i.ops.offsite;
        if !s.enabled || s.pending.is_empty() || s.attempts >= 5 || s.next > now() || i.ops.busy {
            return;
        }
        let s = s.clone();
        i.ops.busy = true;
        s
    };
    let app = app.clone();
    tokio::spawn(async move {
        let result = async {
            if !valid_name(&s.pending) {
                return Err(err("本地备份名称不正确"));
            }
            let path = app.0.dir.join("backups").join(&s.pending);
            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .map_err(|_| err("本地备份已不可用，请重新生成备份"))?;
            if !metadata.is_file() || metadata.len() > LIMIT as u64 {
                return Err(err("本地备份大小或类型不正确"));
            }
            let content = tokio::fs::read(path)
                .await
                .map_err(|_| err("本地备份读取失败"))?;
            if content.len() > LIMIT {
                return Err(err("本地备份大小超过限制"));
            }
            let remote = Remote::new(&app, &s).await?;
            remote.prepare().await?;
            let object = Object {
                name: s.pending.clone(),
                key: key(&s, &s.pending),
                size: content.len(),
                sha256: hex::encode(Sha256::digest(&content)),
                at: now(),
            };
            remote.put(&object.key, content).await?;
            let received = remote.get(&object.key).await?;
            if received.len() != object.size
                || hex::encode(Sha256::digest(&received)) != object.sha256
            {
                return Err(err("上传完成但读回校验失败"));
            }
            let mut objects = s.objects.clone();
            objects.retain(|o| o.name != object.name);
            objects.push(object);
            objects.sort_by_key(|o| o.at);
            while objects.len() > s.keep.clamp(1, 30) {
                let o = &objects[0];
                if !valid_name(&o.name) || o.key != key(&s, &o.name) {
                    return Err(err("远端备份记录不正确"));
                }
                remote.delete(&o.key).await?;
                objects.remove(0);
            }
            Ok(objects)
        }
        .await;
        let mut i = app.lock();
        i.ops.busy = false;
        let off = &mut i.ops.offsite;
        match result {
            Ok(objects) => {
                off.objects = objects;
                off.last = now();
                off.error.clear();
                off.pending.clear();
                off.attempts = 0;
            }
            Err(e) => {
                off.error = e.message;
                off.attempts = off.attempts.saturating_add(1);
                off.next = now() + 60 * (1i64 << off.attempts.min(5));
            }
        }
        let _ = operations::save(&app, &i);
    });
}
pub fn validate_restore(s: &State) -> bool {
    s.objects.len() <= 30
        && s.secret.len() <= 8192
        && s.endpoint.len() <= 1024
        && s.objects.iter().all(|o| {
            valid_name(&o.name)
                && o.key == key(s, &o.name)
                && o.size <= LIMIT
                && o.sha256.len() == 64
        })
        && (s.pending.is_empty() || valid_name(&s.pending))
        && (!s.enabled || config_ok(s).is_ok())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_internal_endpoints() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.64.1.1",
            "192.0.2.1",
            "198.19.1.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "2001:db8::1",
            "2002:7f00:1::",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
            assert!(public_ip(ip.parse().unwrap()));
        }
        for e in [
            "http://example.com",
            "https://a:b@example.com",
            "https://example.com/?x=1",
            "https://example.com/%2fprivate",
        ] {
            assert!(endpoint(e).is_err());
        }
    }
    #[test]
    fn signing_is_deterministic_and_scoped() {
        let empty = hex::encode(Sha256::digest(b""));
        let v = sign(
            "GET",
            "/test.txt",
            "examplebucket.s3.amazonaws.com",
            "20130524T000000Z",
            &empty,
            "us-east-1",
            "AKID",
            "secret",
        );
        assert!(v.contains("Credential=AKID/20130524/us-east-1/s3/aws4_request"));
        assert_ne!(
            v,
            sign(
                "PUT",
                "/test.txt",
                "examplebucket.s3.amazonaws.com",
                "20130524T000000Z",
                &empty,
                "us-east-1",
                "AKID",
                "secret"
            )
        );
        assert!(valid_name("yuji-123-abcd.backup"));
        assert!(!valid_name("yuji-../escape.backup"));
    }
}
