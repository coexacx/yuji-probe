use crate::{core::*, model::*};
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::IpAddr;
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct NodeInput {
    group: Option<String>,
    pinned: Option<bool>,
    notes: Option<String>,
    provider_name: Option<String>,
    #[serde(rename = "providerURL")]
    provider_url: Option<String>,
    expires_at: Option<String>,
    notify_renewal: Option<bool>,
    renewal_version: String,
    name: String,
    ip: String,
    port: i64,
    username: String,
    password: String,
    country: String,
    city: String,
    code: String,
    visible: bool,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct RenewalInput {
    action: String,
    version: String,
    expires_at: String,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct SiteInput {
    name: String,
    public: bool,
    refresh_seconds: i64,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CommandInput {
    name: String,
    script: String,
}
pub fn zone() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}
pub fn renewal_day(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&zone()).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
pub fn renewal_due(n: &Node, at: i64) -> bool {
    if !n.notify_renewal || n.demo || n.expires_at.is_empty() {
        return false;
    }
    let Ok(day) = DateTime::parse_from_rfc3339(&n.expires_at) else {
        return false;
    };
    let day = day.with_timezone(&zone()).date_naive();
    let start = zone()
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .timestamp()
        - 3 * 86400;
    at >= start
}
pub fn admin_node(n: &Node) -> Value {
    let mut v = serde_json::to_value(n).expect("node serializable");
    v["renewalDue"] = json!(renewal_due(n, now()));
    v["renewalDay"] = json!(renewal_day(&n.expires_at));
    v
}
fn expiry(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let d = DateTime::parse_from_rfc3339(s).ok()?;
    if !(2000..=2199).contains(&d.year()) {
        return None;
    }
    Some(
        d.with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}
pub fn safe_provider_url(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if s.len() > 2048 || !valid_text(s, 2048) || s.contains(['\\', ' ', '\t', '\r', '\n']) {
        return false;
    }
    reqwest::Url::parse(s).is_ok_and(|u| {
        ["http", "https"].contains(&u.scheme())
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}
fn lease(n: &mut Node, v: &NodeInput, editing: bool) -> ApiResult<()> {
    if let Some(s) = &v.provider_name {
        n.provider_name = s.trim().into();
    }
    if let Some(s) = &v.provider_url {
        n.provider_url = s.trim().into();
    }
    if (!n.provider_name.is_empty() && !valid_text(&n.provider_name, 100))
        || !safe_provider_url(&n.provider_url)
    {
        return Err(ApiError::new(
            400,
            "请检查供应商名称与站点，站点需使用 http:// 或 https://",
        ));
    }
    let old = (n.expires_at.clone(), n.notify_renewal);
    if editing
        && (v.expires_at.is_some() || v.notify_renewal.is_some())
        && !n.expires_at.is_empty()
        && v.renewal_version != n.renewal_version
    {
        return Err(ApiError::new(409, "到期信息已更新，请重新打开编辑页面"));
    }
    if let Some(s) = &v.expires_at {
        n.expires_at = expiry(s).ok_or_else(|| ApiError::new(400, "到期时间格式不正确"))?;
    }
    if let Some(b) = v.notify_renewal {
        n.notify_renewal = b;
    }
    if n.notify_renewal && n.expires_at.is_empty() {
        return Err(ApiError::new(400, "启用续费提醒前请填写到期时间"));
    }
    if old != (n.expires_at.clone(), n.notify_renewal)
        || (!n.expires_at.is_empty() && n.renewal_version.is_empty())
    {
        n.renewal_version = token()[..32].into();
    }
    Ok(())
}
pub fn save_node(
    app: &App,
    i: &mut Inner,
    c: &Context,
    id: &str,
    body: &[u8],
) -> ApiResult<ApiReply> {
    app.guard(i, c, true, true)?;
    let mut v: NodeInput = decode(body)?;
    v.name = v.name.trim().into();
    v.ip = v.ip.trim().into();
    if !valid_text(&v.name, 80)
        || !acceptable_ip(&v.ip)
        || !(1..=65535).contains(&v.port)
        || !username(&v.username)
        || v.password.len() > 256
        || v.password.contains('\0')
    {
        return Err(ApiError::new(400, "请检查服务器名称、SSH IP、端口和用户名"));
    }
    if v.country.is_empty() {
        v.country = "待识别".into()
    }
    if v.city.is_empty() {
        v.city = "待识别".into()
    }
    if v.code.is_empty() {
        v.code = "OTHER".into()
    }
    if (v.code != "OTHER"
        && (v.code.len() != 2
            || !v.code.bytes().all(|b| b.is_ascii_uppercase())
            || country_code(&v.code) == "OTHER"))
        || !valid_text(&v.country, 80)
        || !valid_text(&v.city, 40)
    {
        return Err(ApiError::new(400, "地区格式不正确"));
    }
    if id.is_empty() && i.data.nodes.len() >= 200 {
        return Err(ApiError::new(409, "当前最多管理 200 台服务器"));
    }
    let index = i.data.nodes.iter().position(|n| n.public.id == id);
    if !id.is_empty() && index.is_none() {
        return Err(ApiError::new(404, "服务器不存在"));
    }
    if i.data
        .nodes
        .iter()
        .any(|n| n.public.id != id && n.ip == v.ip && n.port == v.port)
    {
        return Err(ApiError::new(409, "该 SSH 地址已存在，请编辑原节点"));
    }
    if index.is_some_and(|k| i.data.nodes[k].removing) {
        return Err(ApiError::new(409, "节点正在清理"));
    }
    let mut n = if let Some(index) = index {
        i.data.nodes[index].clone()
    } else {
        Node {
            public: PublicNode {
                id: token()[..20].into(),
                pending: true,
                cpu_model: "等待 Agent 上报".into(),
                arch: "待上报".into(),
                system: "待上报".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    };
    let ip = v.ip.parse::<IpAddr>().unwrap().to_string();
    if !id.is_empty()
        && (n.ip != ip || n.port != v.port || n.username != v.username)
        && i.data
            .secrets
            .get(id)
            .is_some_and(|s| !s.ssh_key.is_empty())
    {
        return Err(ApiError::new(
            409,
            "已接入服务器的 SSH 地址不可直接修改，请移除后重新添加",
        ));
    }
    lease(&mut n, &v, !id.is_empty())?;
    if let Some(group) = &v.group {
        let group = group.trim();
        if !group.is_empty() && !valid_text(group, 40) {
            return Err(ApiError::new(400, "分组最多 40 个字符"));
        }
        n.public.group = group.into();
    }
    if let Some(pinned) = v.pinned {
        n.public.pinned = pinned;
    }
    if let Some(notes) = &v.notes {
        if notes.chars().count() > 2000
            || notes
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(ApiError::new(
                400,
                "备注最多 2000 个字符，不能包含特殊控制字符",
            ));
        }
        n.notes = notes.trim().into();
    }
    if id.is_empty() {
        n.public.order = i
            .data
            .nodes
            .iter()
            .map(|n| n.public.order)
            .max()
            .map_or(0, |n| n.saturating_add(1));
    }
    if v.code != "OTHER" {
        v.code = country_code(&v.code);
        v.country = country_name(&v.code);
    }
    n.public.name = v.name;
    n.public.country = v.country;
    n.public.city = v.city;
    n.public.code = v.code.clone();
    n.ip = ip;
    n.port = v.port;
    n.username = v.username;
    n.visible = v.visible;
    n.country_auto = v.code == "OTHER";
    let mut data = i.data.clone();
    if let Some(index) = index {
        data.nodes[index] = n.clone()
    } else {
        data.nodes.push(n.clone())
    }
    app.save_data(i, data)?;
    crate::telegram::invalidate(app, i);
    app.record(
        i,
        if id.is_empty() {
            "node_created"
        } else {
            "node_updated"
        },
        &n.public.name,
    );
    Ok(ApiReply::ok(
        json!({"node":n,"deployed":false,"message":"服务器配置已保存"}),
    ))
}
pub fn delete_node(app: &App, i: &mut Inner, c: &Context, id: &str) -> ApiResult<ApiReply> {
    app.guard(i, c, true, true)?;
    if i.jobs
        .values()
        .any(|j| j.node_id == id && j.state == "running")
    {
        return Err(ApiError::new(409, "请等待部署完成后移除"));
    }
    let n = i
        .data
        .nodes
        .iter()
        .find(|n| n.public.id == id)
        .cloned()
        .ok_or_else(|| ApiError::new(404, "服务器不存在"))?;
    crate::auth::require_elevated(i, c)?;
    if !i.data.secrets.contains_key(id) || !n.public.online || !i.agents.contains_key(id) {
        if let Some(link) = i.agents.remove(id) {
            link.stop.cancel();
        }
        i.tickets.retain(|_, t| t.node_id != id);
        i.ops.transfers.remove(id);
        i.ops.retired.remove(id);
        i.ops.removal_names.remove(id);
        crate::operations::save(app, i)?;

        let mut data = i.data.clone();
        data.nodes.retain(|n| n.public.id != id);
        data.secrets.remove(id);
        app.save_data(i, data)?;
        crate::telegram::invalidate(app, i);
        app.record(i, "node_removed_offline", &n.public.name);
        return Ok(ApiReply::ok(json!({"ok":true})));
    }
    crate::migration::start_manage(app, i, id, "remove")?;
    let mut data = i.data.clone();
    if let Some(n) = data.nodes.iter_mut().find(|n| n.public.id == id) {
        n.removing = true;
        n.visible = false;
        n.deploy_message = "等待远端清除配置".into();
    }
    app.save_data(i, data)?;
    i.tickets.retain(|_, t| t.node_id != id);
    i.ops.removal_names.insert(id.into(), n.public.name.clone());
    crate::operations::save(app, i)?;
    crate::telegram::invalidate(app, i);
    app.record(i, "node_removal_pending", &n.public.name);
    Ok(ApiReply::accepted(json!({"ok":true,"pending":true})))
}
pub fn renew(app: &App, i: &mut Inner, c: &Context, id: &str, body: &[u8]) -> ApiResult<ApiReply> {
    app.guard(i, c, true, true)?;
    let mut v: RenewalInput = decode(body)?;
    if v.action.is_empty() {
        v.action = "renew".into()
    }
    if !["renew", "stop"].contains(&v.action.as_str()) {
        return Err(ApiError::new(400, "续费操作不正确"));
    }
    let index = i
        .data
        .nodes
        .iter()
        .position(|n| n.public.id == id)
        .ok_or_else(|| ApiError::new(404, "服务器不存在"))?;
    let mut n = i.data.nodes[index].clone();
    if v.version.is_empty() || v.version != n.renewal_version || v.expires_at != n.expires_at {
        return Err(ApiError::new(
            409,
            "此到期周期已处理或发生变化，请刷新后查看",
        ));
    }
    if !renewal_due(&n, now()) {
        return Err(ApiError::new(409, "当前没有待处理的续费提醒"));
    }
    let (action, message) = if v.action == "stop" {
        n.notify_renewal = false;
        ("node_renewal_stopped", "已停止该服务器的续费提醒")
    } else {
        let d = DateTime::parse_from_rfc3339(&n.expires_at)
            .map_err(|_| ApiError::new(400, "到期时间不正确"))?
            + chrono::Duration::days(30);
        if d.year() > 2199 {
            return Err(ApiError::new(400, "请在服务器设置中调整到期时间"));
        }
        n.expires_at = d
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        ("node_renewed", "已记录续费，到期时间顺延 30 天")
    };
    n.renewal_version = token()[..32].into();
    let mut data = i.data.clone();
    data.nodes[index] = n.clone();
    app.save_data(i, data)?;
    crate::telegram::invalidate(app, i);
    app.record(i, action, &n.public.name);
    Ok(ApiReply::ok(
        json!({"node":admin_node(&n),"message":message}),
    ))
}
pub fn site(app: &App, i: &mut Inner, c: &Context, body: &[u8]) -> ApiResult<ApiReply> {
    app.guard(i, c, true, true)?;
    let v: SiteInput = decode(body)?;
    let site = Site {
        name: v.name.trim().into(),
        public: v.public,
        refresh_seconds: v.refresh_seconds,
    };
    if !valid_text(&site.name, 60) || !(3..=30).contains(&site.refresh_seconds) {
        return Err(ApiError::new(400, "请检查站点名称和刷新间隔（3–30 秒）"));
    }
    let mut data = i.data.clone();
    data.site = site.clone();
    app.save_data(i, data)?;
    app.record(i, "site_updated", &site.name);
    Ok(ApiReply::ok(json!({"site":site})))
}
pub fn commands(app: &App, i: &mut Inner, c: &Context, body: &[u8]) -> ApiResult<ApiReply> {
    app.guard(i, c, true, c.method != "GET")?;
    if c.method == "GET" {
        return Ok(ApiReply::ok(json!({"commands":i.data.commands})));
    }
    if !["POST", "PATCH", "DELETE"].contains(&c.method.as_str()) {
        return Err(ApiError::new(405, "请求方法不正确"));
    }
    let mut id = c
        .path
        .strip_prefix("/api/admin/commands/")
        .unwrap_or("")
        .to_string();
    let index = i.data.commands.iter().position(|v| v.id == id);
    if c.method != "POST" && index.is_none() {
        return Err(ApiError::new(404, "命令不存在"));
    }
    if c.method == "POST" && (!id.is_empty() || i.data.commands.len() >= 50) {
        return Err(ApiError::new(409, "最多保存 50 条常用命令"));
    }
    let mut v = CommandInput::default();
    if c.method != "DELETE" {
        v = decode(body)?;
        v.name = v.name.trim().into();
        v.script = v.script.replace("\r\n", "\n").trim().into();
        if !valid_text(&v.name, 60)
            || v.script.is_empty()
            || v.script.len() > 8192
            || v.script.contains(['\0', '\x1b'])
        {
            return Err(ApiError::new(
                400,
                "请输入命令名称与脚本，脚本最多 8192 字节",
            ));
        }
    }
    let mut data = i.data.clone();
    let (action, name) = match c.method.as_str() {
        "POST" => {
            id = token()[..20].into();
            let name = v.name.clone();
            data.commands.push(SavedCommand {
                id,
                name: v.name,
                script: v.script,
            });
            ("command_created", name)
        }
        "PATCH" => {
            let name = v.name.clone();
            data.commands[index.unwrap()] = SavedCommand {
                id,
                name: v.name,
                script: v.script,
            };
            ("command_updated", name)
        }
        _ => {
            let name = data.commands.remove(index.unwrap()).name;
            ("command_deleted", name)
        }
    };
    app.save_data(i, data)?;
    app.record(i, action, &name);
    Ok(ApiReply::ok(json!({"commands":i.data.commands})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renewal_calendar_and_urls() {
        let n = Node {
            expires_at: "2026-10-03T23:00:00Z".into(),
            notify_renewal: true,
            ..Default::default()
        };
        assert_eq!(renewal_day(&n.expires_at), "2026-10-04");
        let begin = DateTime::parse_from_rfc3339("2026-10-01T00:00:00+08:00")
            .unwrap()
            .timestamp();
        assert!(!renewal_due(&n, begin - 1));
        assert!(renewal_due(&n, begin));
        assert!(!safe_provider_url("javascript:alert(1)"));
        assert!(!safe_provider_url("https://user:password@example.com"));
        assert!(safe_provider_url("https://example.com/provider"));
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    fn existing_browser_lease_payload() {
        let node:NodeInput=decode(br#"{"name":"node","ip":"192.0.2.1","port":22,"username":"root","visible":false,"providerName":"provider","providerURL":"https://example.com","expiresAt":"2026-10-01T00:00:00Z","notifyRenewal":true,"renewalVersion":"previous","country":"region","city":"","code":"IS"}"#).unwrap();
        assert_eq!(node.provider_url.as_deref(), Some("https://example.com"));
        assert_eq!(node.notify_renewal, Some(true));
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderInput {
    ids: Vec<String>,
}
pub fn reorder(app: &App, i: &mut Inner, c: &Context, body: &[u8]) -> ApiResult<ApiReply> {
    app.guard(i, c, true, true)?;
    let value: OrderInput = decode(body)?;
    let expected: std::collections::HashSet<_> =
        i.data.nodes.iter().map(|n| n.public.id.as_str()).collect();
    let received: std::collections::HashSet<_> = value.ids.iter().map(String::as_str).collect();
    if value.ids.len() != expected.len() || received != expected {
        return Err(ApiError::new(409, "服务器列表已变化，请刷新后重新排序"));
    }
    let mut data = i.data.clone();
    for (order, id) in value.ids.iter().enumerate() {
        data.nodes
            .iter_mut()
            .find(|n| &n.public.id == id)
            .unwrap()
            .public
            .order = order as u32;
    }
    app.save_data(i, data)?;
    app.record(i, "nodes_reordered", "更新服务器显示顺序");
    Ok(ApiReply::ok(json!({"ok":true})))
}
