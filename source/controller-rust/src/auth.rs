use crate::{core::*, model::Auth};
use chrono::DateTime;
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::json;
use sha1::Sha1;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Login {
    username: String,
    password: String,
    code: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Password {
    current: String,
    new: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Mfa {
    password: String,
    code: String,
}
pub fn totp(secret: &str, counter: i64) -> Option<String> {
    let key = BASE32_NOPAD.decode(secret.as_bytes()).ok()?;
    if key.len() != 20 || counter < 0 {
        return None;
    }
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(&key).ok()?;
    mac.update(&(counter as u64).to_be_bytes());
    let sum = mac.finalize().into_bytes();
    let off = usize::from(sum[19] & 15);
    let number = u32::from_be_bytes(sum[off..off + 4].try_into().ok()?) & 0x7fffffff;
    Some(format!("{:06}", number % 1_000_000))
}
pub fn verify_totp(secret: &str, code: &str, at: i64, last: i64) -> Option<i64> {
    if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    [at / 30, at / 30 - 1, at / 30 + 1]
        .into_iter()
        .find(|&step| step > last && totp(secret, step).is_some_and(|value| constant(&value, code)))
}
fn reserve(app: &App, i: &mut Inner, key: &str, limit: u32, global: bool) -> ApiResult<()> {
    let t = now();
    i.limits.sources.retain(|_, a| a.until.timestamp() > t);
    if i.limits.sources.len() > 8192 {
        return Err(ApiError::new(503, "服务繁忙"));
    }
    let a = i.limits.sources.entry(key.into()).or_default();
    if a.until.timestamp() <= t {
        *a = Attempt {
            count: 0,
            until: DateTime::from_timestamp(t + 900, 0).unwrap(),
        };
    }
    if global && i.limits.global.until.timestamp() <= t {
        i.limits.global = Attempt {
            count: 0,
            until: DateTime::from_timestamp(t + 900, 0).unwrap(),
        };
    }
    if a.count >= limit || (global && i.limits.global.count >= 24) {
        return Err(ApiError::rate("验证次数过多，请 15 分钟后重试", 900));
    }
    a.count += 1;
    if global {
        i.limits.global.count += 1;
    }
    app.persist_limits(i)
}
async fn password_matches(hash: String, password: String) -> bool {
    if password.len() > 72 {
        return false;
    }
    tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash).unwrap_or(false))
        .await
        .unwrap_or(false)
}
fn save_auth(app: &App, i: &mut Inner, auth: Auth) -> ApiResult<()> {
    atomic_json(&app.0.dir.join("auth.json"), &auth).map_err(|_| ApiError::internal())?;
    i.auth = auth;
    Ok(())
}
fn login_mfa(app: &App, i: &mut Inner, code: &str) -> bool {
    if i.auth.mfa.is_empty() {
        return true;
    }
    let Ok(raw) = app.unseal("admin:mfa", &i.auth.mfa) else {
        return false;
    };
    let Ok(secret) = std::str::from_utf8(&raw) else {
        return false;
    };
    let Some(counter) = verify_totp(secret, code, now(), i.auth.mfa_last) else {
        return false;
    };
    let mut auth = i.auth.clone();
    auth.mfa_last = counter;
    save_auth(app, i, auth).is_ok()
}
fn invalidate(i: &mut Inner) {
    let ids: Vec<_> = i.sessions.keys().cloned().collect();
    for id in ids {
        i.revoke(&id);
    }
}
pub async fn login(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let mut v: Login = decode(&body)?;
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, false, true)?;
        reserve(&app, &mut i, &c.ip, 8, true)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if v.password.len() > 72 || v.username.len() > 80 {
        v.password = "invalid".into();
    }
    let matched = password_matches(hash, v.password).await;
    let mut i = app.lock();
    if !matched
        || v.username != i.auth.username
        || version != i.auth.version
        || !login_mfa(&app, &mut i, &v.code)
    {
        app.record(&mut i, "login_failed", &c.ip);
        return Err(ApiError::new(401, "用户名、密码或验证码不正确"));
    }
    if !i.session(&c.sid).is_some_and(|s| s.id == x.id) {
        return Err(ApiError::new(401, "登录页面已过期，请刷新"));
    }
    i.limits.sources.remove(&c.ip);
    i.limits.global = Attempt::default();
    app.persist_limits(&mut i)?;
    let x = i.new_session(&x.id, true)?;
    let name = i.auth.username.clone();
    app.record(&mut i, "login", &name);
    Ok(ApiReply::session(i.info(&x), &x.id))
}
pub async fn password(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    let v: Password = decode(&body)?;
    if !valid_password(&v.new) {
        return Err(ApiError::new(400, "新密码需为 12–72 字节"));
    }
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, true, true)?;
        reserve(&app, &mut i, "password:account", 5, false)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if !password_matches(hash, v.current).await {
        return Err(ApiError::new(400, "当前密码不正确"));
    }
    let hash = tokio::task::spawn_blocking(move || bcrypt::hash(v.new, 12))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())?;
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if i.auth.version != version {
        return Err(ApiError::new(409, "账户设置已变化，请重新登录"));
    }
    let mut auth = i.auth.clone();
    auth.hash = hash;
    auth.version = token();
    save_auth(&app, &mut i, auth)?;
    invalidate(&mut i);
    let x = i.new_session(&x.id, true)?;
    let name = i.auth.username.clone();
    app.record(&mut i, "password_changed", &name);
    Ok(ApiReply::session(i.info(&x), &x.id))
}
pub async fn mfa(app: App, c: Context, body: Vec<u8>) -> ApiResult<ApiReply> {
    if c.method != "POST" {
        return Err(ApiError::new(405, "请求方法不正确"));
    }
    let v: Mfa = decode(&body)?;
    let action = c.path.strip_prefix("/api/admin/mfa/").unwrap_or("");
    if !["setup", "enable", "disable"].contains(&action) {
        return Err(ApiError::new(404, "接口不存在"));
    }
    let _slot = app
        .0
        .login_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate("请稍后再试", 1))?;
    let (x, hash, version) = {
        let mut i = app.lock();
        let x = app.guard(&mut i, &c, true, true)?;
        reserve(&app, &mut i, "mfa:account", 5, false)?;
        (x, i.auth.hash.clone(), i.auth.version.clone())
    };
    if action != "enable" && !password_matches(hash, v.password).await {
        return Err(ApiError::new(400, "当前密码或验证码不正确"));
    }
    let mut i = app.lock();
    app.guard(&mut i, &c, true, true)?;
    if i.auth.version != version {
        return Err(ApiError::new(409, "账户设置已变化，请重新登录"));
    }
    if action == "setup" {
        if !i.auth.mfa.is_empty() {
            return Err(ApiError::new(409, "二步验证已经启用"));
        }
        let mut bytes = [0; 20];
        getrandom::fill(&mut bytes).map_err(|_| ApiError::internal())?;
        let secret = BASE32_NOPAD.encode(&bytes);
        let s = i
            .sessions
            .get_mut(&x.id)
            .ok_or_else(|| ApiError::new(401, "登录已过期"))?;
        s.mfa_pending = secret.clone();
        s.mfa_expires = now() + 300;
        return Ok(ApiReply::ok(json!({"secret":secret})));
    }
    let mut auth = i.auth.clone();
    if action == "enable" {
        if !auth.mfa.is_empty() || x.mfa_pending.is_empty() || now() > x.mfa_expires {
            return Err(ApiError::new(400, "绑定已过期，请重新开始"));
        }
        let counter = verify_totp(&x.mfa_pending, &v.code, now(), 0)
            .ok_or_else(|| ApiError::new(400, "验证码不正确，请检查验证器时间"))?;
        auth.mfa = app
            .seal("admin:mfa", x.mfa_pending.as_bytes())
            .map_err(|_| ApiError::internal())?;
        auth.mfa_last = counter;
    } else {
        if auth.mfa.is_empty() {
            return Err(ApiError::new(409, "二步验证尚未启用"));
        }
        let raw = app
            .unseal("admin:mfa", &auth.mfa)
            .map_err(|_| ApiError::internal())?;
        let secret = std::str::from_utf8(&raw).map_err(|_| ApiError::internal())?;
        verify_totp(secret, &v.code, now(), auth.mfa_last)
            .ok_or_else(|| ApiError::new(400, "当前密码或验证码不正确"))?;
        auth.mfa.clear();
        auth.mfa_last = 0;
    }
    auth.version = token();
    save_auth(&app, &mut i, auth)?;
    invalidate(&mut i);
    i.limits.sources.remove("mfa:account");
    app.persist_limits(&mut i)?;
    let x = i.new_session(&x.id, true)?;
    let name = i.auth.username.clone();
    app.record(&mut i, &format!("mfa_{action}"), &name);
    Ok(ApiReply::session(i.info(&x), &x.id))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc_totp_and_replay() {
        let secret = BASE32_NOPAD.encode(b"12345678901234567890");
        assert_eq!(totp(&secret, 1).as_deref(), Some("287082"));
        assert_eq!(verify_totp(&secret, "287082", 59, 0), Some(1));
        assert_eq!(verify_totp(&secret, "287082", 59, 1), None);
        assert_eq!(verify_totp(&secret, "28a082", 59, 0), None);
    }
    #[test]
    fn strict_login_payload() {
        assert!(decode::<Login>(br#"{"username":"admin","extra":true}"#).is_err());
        assert!(decode::<Login>(br#"{} {}"#).is_err());
    }
}
