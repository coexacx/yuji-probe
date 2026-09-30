#!/usr/bin/env bash
# 羽迹探针：用于全新 Debian 12/13。已有宝塔请使用 docs/宝塔部署教程.md。
set +x
set -Eeuo pipefail
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
umask 077
readonly YUJI_VERSION=0.4.1
readonly YUJI_RELEASE_BASE=https://github.com/coexacx/yuji-probe/releases/download/v0.4.1
readonly YUJI_PUBLIC_KEY=o8+DdHbo82V7fxJEIiEhe5AK/frR91Fz5vjf/pDAnts=
yuji_domain='' yuji_email='' yuji_accept_terms=0 yuji_cert='' yuji_key=''
yuji_work='' yuji_changes=0 yuji_success=0 yuji_php_version=''
usage() {
    cat <<'HELP'
羽迹探针 Debian 一键安装
用法：bash install.sh [--domain probe.example.com] [--accept-acme-terms] [--email admin@example.com]
      bash install.sh --domain probe.example.com --cert-file /path/fullchain.pem --key-file /path/privkey.pem
无参数运行会询问域名、站点名称、管理员用户名、密码及证书服务条款。
域名 A 记录必须已经指向本机；存在 AAAA 记录时也必须指向本机 IPv6。
需要 root、Debian 12/13、systemd、空闲 80/443/19281 端口；已建站或宝塔主机请用手动教程。
自动安装 Nginx、PHP-FPM，申请 Let's Encrypt 证书并启用 HTTPS 与自动续期。
不会覆盖已安装的面板或已有网站；不会修改 SSH 或防火墙。
HELP
}
die() { printf '错误：%s\n' "$*" >&2; exit 1; }
note() { printf '%s\n' "$*"; }
cleanup() {
    local rc=$?
    trap - EXIT
    unset yuji_password yuji_repeat
    if (( yuji_success == 0 && yuji_changes == 1 )); then
        systemctl disable --now yuji-probe.service >/dev/null 2>&1 || true
        rm -f /etc/systemd/system/yuji-probe.service /etc/nginx/sites-enabled/yuji-probe.conf /etc/nginx/sites-available/yuji-probe.conf
        if [[ -n "$yuji_php_version" ]]; then
            rm -f "/etc/php/$yuji_php_version/fpm/pool.d/yuji-probe.conf"
            systemctl reload "php$yuji_php_version-fpm" >/dev/null 2>&1 || true
        fi
        systemctl daemon-reload >/dev/null 2>&1 || true
        if nginx -t >/dev/null 2>&1; then systemctl reload nginx >/dev/null 2>&1 || true; fi
        note '安装未完成，本站入口已撤下。已安装的软件包及 /opt/yuji-probe、/var/lib/yuji-probe 保留供排查；不要覆盖已有状态。'
    fi
    if [[ -n "$yuji_work" && "$yuji_work" == /var/tmp/yuji-probe-install.* ]]; then rm -rf -- "$yuji_work"; fi
    exit "$rc"
}
trap cleanup EXIT
trap 'printf "安装在第 %s 行中止，请查看上方错误。\n" "$LINENO" >&2' ERR
while (( $# )); do
    case "$1" in
        --domain|--email|--cert-file|--key-file)
            (( $# >= 2 )) || die "$1 缺少参数"
            case "$1" in
                --domain) yuji_domain=$2 ;;
                --email) yuji_email=$2 ;;
                --cert-file) yuji_cert=$2 ;;
                --key-file) yuji_key=$2 ;;
            esac
            shift 2 ;;
        --accept-acme-terms) yuji_accept_terms=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "未知参数：$1" ;;
    esac
done
(( EUID == 0 )) || die '请使用 root 运行。'
[[ -f /etc/os-release ]] || die '无法识别系统。'
# shellcheck disable=SC1091
. /etc/os-release
[[ ${ID:-} == debian && ${VERSION_ID:-} =~ ^(12|13)$ ]] || die '自动安装目前只支持 Debian 12、13。'
[[ -d /run/systemd/system ]] || die '需要使用 systemd 的完整 Debian 系统。'
case "$(uname -m)" in
    x86_64) yuji_arch=amd64 ;;
    aarch64|arm64) yuji_arch=arm64 ;;
    *) die '只支持 amd64、arm64 架构。' ;;
esac
command -v flock >/dev/null || die '缺少 util-linux 的 flock。'
exec 9>/run/yuji-probe-install.lock
flock -n 9 || die '已有安装程序正在运行。'
for yuji_path in /www/server/panel /opt/yuji-probe /var/lib/yuji-probe /etc/yuji-probe /etc/systemd/system/yuji-probe.service /etc/nginx/sites-available/yuji-probe.conf; do
    [[ ! -e "$yuji_path" && ! -L "$yuji_path" ]] || die "检测到 $yuji_path；为保护已有站点，请使用手动部署/升级教程。"
done
getent passwd yuji-probe >/dev/null && die 'yuji-probe 系统用户已存在，请先检查已有安装。'
if command -v ss >/dev/null; then
    [[ -z "$(ss -H -ltn '( sport = :80 or sport = :443 or sport = :19281 )')" ]] || die '80、443 或 19281 端口已被使用，请采用手动部署教程。'
fi
if [[ -d /etc/nginx/sites-enabled ]]; then
    for yuji_path in /etc/nginx/sites-enabled/*; do
        [[ ! -e "$yuji_path" && ! -L "$yuji_path" ]] && continue
        die '检测到已有 Nginx 站点配置，请采用手动部署教程。'
    done
fi
for yuji_service in nginx apache2; do
    if systemctl is-active --quiet "$yuji_service"; then die "已有 $yuji_service 正在运行，请采用手动部署教程。"; fi
done
exec 3<>/dev/tty || die '请在可交互的 SSH 终端中运行。'
note '域名必须先完成 DNS 解析：A 记录指向本机公网 IPv4；若有 AAAA 记录，也必须指向本机 IPv6。'
note '请暂时关闭 CDN 代理，并在云安全组及防火墙放行 TCP 80、443。'
if [[ -z "$yuji_domain" ]]; then read -r -u 3 -p '面板域名（如 probe.example.com）：' yuji_domain; fi
yuji_domain=${yuji_domain,,}
[[ ${#yuji_domain} -le 253 && "$yuji_domain" == *.* && "$yuji_domain" != *[!a-z0-9.-]* ]] || die '请填写有效域名，不带协议、路径或端口。'
IFS='.' read -r -a yuji_labels <<< "$yuji_domain"
for yuji_label in "${yuji_labels[@]}"; do
    [[ "$yuji_label" =~ ^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$ ]] || die '域名标签格式不正确。'
done
[[ "$yuji_domain" != *. && "${yuji_labels[-1]}" =~ ^[a-z] ]] || die '请使用域名，不使用 IP 地址。'
if [[ -n "$yuji_email" ]]; then
    [[ "$yuji_email" =~ ^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$ ]] || die '邮件地址格式不正确。'
fi
if [[ -n "$yuji_cert" || -n "$yuji_key" ]]; then
    [[ "$yuji_cert" == /* && "$yuji_key" == /* && -r "$yuji_cert" && -r "$yuji_key" ]] || die '证书和私钥必须成对提供绝对路径。'
else
    if (( ! yuji_accept_terms )); then
        note "将向 Let's Encrypt 申请域名证书，自动续期。服务条款：https://letsencrypt.org/repository/"
        read -r -u 3 -p '同意证书服务条款并继续？[y/N]：' yuji_reply
        [[ "$yuji_reply" == y || "$yuji_reply" == Y ]] || die '未同意证书服务条款，安装已停止。'
    fi
fi
read -r -u 3 -p '站点名称：' yuji_name
[[ -n "$yuji_name" && ${#yuji_name} -le 180 && ! "$yuji_name" =~ [[:cntrl:]] ]] || die '站点名称不能为空或包含控制字符。'
read -r -u 3 -p '管理员用户名：' yuji_user
[[ "$yuji_user" =~ ^[a-zA-Z_][a-zA-Z0-9_.-]{0,31}$ ]] || die '用户名须以字母/下划线开头，最多 32 个字符。'
read -r -s -u 3 -p '管理员密码（12–72 字节，输入不显示）：' yuji_password
printf '\n' >&3
read -r -s -u 3 -p '再次输入密码：' yuji_repeat
printf '\n' >&3
[[ "$yuji_password" == "$yuji_repeat" ]] || die '两次密码不一致。'
yuji_bytes=$(printf '%s' "$yuji_password" | wc -c)
(( yuji_bytes >= 12 && yuji_bytes <= 72 )) || die '密码长度必须为 12–72 字节。'
unset yuji_repeat
note '正在安装 Debian 官方软件源中的运行依赖……'
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl openssl python3 nginx php-fpm php-cli php-curl certbot
yuji_php_version=$(php -r 'echo PHP_MAJOR_VERSION,".",PHP_MINOR_VERSION;')
[[ "$yuji_php_version" =~ ^8\.[0-9]+$ && -d "/etc/php/$yuji_php_version/fpm/pool.d" ]] || die 'PHP-FPM 版本不符合要求。'
php -r 'exit(extension_loaded("curl") && extension_loaded("openssl") && extension_loaded("session") && function_exists("proc_open") ? 0 : 1);' || die 'PHP 运行环境不完整。'
yuji_work=$(mktemp -d /var/tmp/yuji-probe-install.XXXXXXXX)
note '正在从 GitHub 下载并验证发行包签名……'
timeout 240 python3 - "$YUJI_RELEASE_BASE" "$YUJI_PUBLIC_KEY" "$yuji_work" "$YUJI_VERSION" <<'PYDOWNLOAD'
import base64, hashlib, json, pathlib, ssl, stat, subprocess, sys, urllib.parse, urllib.request, zipfile
base, key, work, version = sys.argv[1:]
root = pathlib.Path(work)
prefix = urllib.parse.urlsplit(base).path + "/"
def allowed(url):
    u = urllib.parse.urlsplit(url)
    if u.scheme != "https" or u.port not in (None, 443) or u.username or u.password or u.fragment:
        return False
    return ((u.hostname == "github.com" and u.path.startswith(prefix) and not u.query)
        or (u.hostname == "release-assets.githubusercontent.com" and u.path.startswith("/github-production-release-asset/"))
        or (u.hostname == "objects.githubusercontent.com" and u.path.startswith("/github-production-release-asset-2e65be/")))
class Redirect(urllib.request.HTTPRedirectHandler):
    max_redirections = 4
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not allowed(newurl): raise RuntimeError("下载被引导至非 GitHub HTTPS 发布地址")
        return super().redirect_request(req, fp, code, msg, headers, newurl)
client = urllib.request.build_opener(urllib.request.ProxyHandler({}), Redirect(), urllib.request.HTTPSHandler(context=ssl.create_default_context()))
def download(name, limit):
    url = base + "/" + name
    if not allowed(url): raise RuntimeError("无效的 GitHub 发布地址")
    with client.open(urllib.request.Request(url, headers={"User-Agent":"Yuji-Probe-Installer/"+version}), timeout=25) as response:
        if response.status != 200: raise RuntimeError("下载失败")
        raw = response.read(limit + 1)
    if len(raw) > limit: raise RuntimeError("发布文件超出限制")
    return raw
envelope = json.loads(download("panel-stable.json", 65536))
payload = base64.b64decode(envelope["payload"], validate=True)
signature = base64.b64decode(envelope["signature"], validate=True)
public = base64.b64decode(key, validate=True)
if len(public) != 32 or len(signature) != 64: raise RuntimeError("无效签名")
(root/"payload").write_bytes(payload)
(root/"signature").write_bytes(signature)
(root/"public.der").write_bytes(bytes.fromhex("302a300506032b6570032100")+public)
subprocess.run(["openssl","pkeyutl","-verify","-pubin","-keyform","DER","-inkey",str(root/"public.der"),"-rawin","-in",str(root/"payload"),"-sigfile",str(root/"signature")],check=True,stdout=subprocess.DEVNULL)
manifest=json.loads(payload)
entry=manifest["files"]["panel"]
name="yuji-probe-panel-"+version+".zip"
if manifest["version"] != version or entry["name"] != name or not 1024 <= entry["size"] <= 128*1024*1024:
    raise RuntimeError("发行版本或文件名称不匹配")
data=download(name, entry["size"])
if len(data) != entry["size"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
    raise RuntimeError("发行包哈希或大小不匹配")
archive=root/name
archive.write_bytes(data)
prefix="yuji-probe-panel-"+version+"/"
with zipfile.ZipFile(archive) as z:
    if len(z.infolist()) > 10000 or sum(i.file_size for i in z.infolist()) > 256*1024*1024:
        raise RuntimeError("发行包解压大小超出限制")
    seen=set()
    for i in z.infolist():
        name=i.filename
        path=pathlib.PurePosixPath(name)
        kind=stat.S_IFMT(i.external_attr >> 16)
        if not name.startswith(prefix) or "\\" in name or path.is_absolute() or ".." in path.parts or name in seen or kind not in (0,stat.S_IFREG,stat.S_IFDIR):
            raise RuntimeError("发行包包含无效路径或特殊文件")
        seen.add(name)
    z.extractall(root/"unpacked")
package=root/"unpacked"/prefix.rstrip("/")
for required in ["public/index.php","app/bootstrap.php","ops/install-cli.php","bin/probe-linux-amd64","bin/probe-linux-arm64","ops/templates/nginx.conf"]:
    if not (package/required).is_file(): raise RuntimeError("发行包缺少必要文件")
print("发行包 Ed25519 签名、SHA-256、大小与解压路径校验通过。")
PYDOWNLOAD
if [[ -n "$yuji_cert" ]]; then
    openssl x509 -in "$yuji_cert" -noout -checkhost "$yuji_domain" -checkend 86400 >/dev/null || die '证书域名不匹配或即将过期。'
    yuji_cert_pub=$(openssl x509 -in "$yuji_cert" -pubkey -noout | openssl pkey -pubin -outform DER | sha256sum)
    yuji_key_pub=$(openssl pkey -in "$yuji_key" -passin pass: -pubout -outform DER | sha256sum)
    [[ "$yuji_cert_pub" == "$yuji_key_pub" ]] || die '证书与私钥不匹配。'
fi
yuji_changes=1
install -d -m 0755 /opt/yuji-probe
cp -a "$yuji_work/unpacked/yuji-probe-panel-$YUJI_VERSION/." /opt/yuji-probe/
find /opt/yuji-probe -type d -exec chmod 0755 {} +
find /opt/yuji-probe -type f -exec chmod 0644 {} +
chmod 0755 /opt/yuji-probe/bin/probe-linux-*
rm -f /opt/yuji-probe/storage/.gitkeep
rmdir /opt/yuji-probe/storage
useradd --system --user-group --home-dir /var/lib/yuji-probe --shell /usr/sbin/nologin yuji-probe
install -d -m 0700 -o yuji-probe -g yuji-probe /var/lib/yuji-probe /var/lib/yuji-probe/sessions
ln -s /var/lib/yuji-probe /opt/yuji-probe/storage
install -d -m 0755 /var/lib/yuji-probe-acme /etc/yuji-probe /etc/yuji-probe/tls
install -m 0644 /opt/yuji-probe/ops/templates/php-pool.conf "/etc/php/$yuji_php_version/fpm/pool.d/yuji-probe.conf"
printf '%s\0' "$yuji_name" "$yuji_user" "$yuji_password" "$yuji_domain" | runuser -u yuji-probe -- php /opt/yuji-probe/ops/install-cli.php
unset yuji_password
yuji_nginx=/etc/nginx/sites-available/yuji-probe.conf
cat > "$yuji_nginx" <<NGINXHTTP
server {
    listen 80;
    listen [::]:80;
    server_name $yuji_domain;
    server_tokens off;
    location ^~ /.well-known/acme-challenge/ { root /var/lib/yuji-probe-acme; default_type text/plain; }
    location / { return 503; }
}
NGINXHTTP
chmod 0644 "$yuji_nginx"
ln -s "$yuji_nginx" /etc/nginx/sites-enabled/yuji-probe.conf
nginx -t
systemctl enable --now nginx
systemctl reload nginx
if [[ -n "$yuji_cert" ]]; then
    install -m 0644 "$yuji_cert" /etc/yuji-probe/tls/fullchain.pem
    install -m 0600 "$yuji_key" /etc/yuji-probe/tls/privkey.pem
else
    note '正在申请 HTTPS 证书；验证方将检查本域名的 DNS 和公网 80 端口……'
    yuji_cert_args=(certonly --non-interactive --agree-tos --webroot -w /var/lib/yuji-probe-acme --cert-name yuji-probe -d "$yuji_domain")
    if [[ -n "$yuji_email" ]]; then yuji_cert_args+=(--email "$yuji_email"); else yuji_cert_args+=(--register-unsafely-without-email); fi
    if ! certbot "${yuji_cert_args[@]}"; then
        die '证书申请失败：请检查 A/AAAA 是否指向本机、80 端口是否开放、CDN 代理和 DNS CAA 记录。'
    fi
    ln -s /etc/letsencrypt/live/yuji-probe/fullchain.pem /etc/yuji-probe/tls/fullchain.pem
    ln -s /etc/letsencrypt/live/yuji-probe/privkey.pem /etc/yuji-probe/tls/privkey.pem
    install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy
    printf '%s\n' '#!/bin/sh' 'set -eu' '/usr/sbin/nginx -t -q' '/usr/bin/systemctl reload nginx' > /etc/letsencrypt/renewal-hooks/deploy/yuji-probe-nginx
    chmod 0700 /etc/letsencrypt/renewal-hooks/deploy/yuji-probe-nginx
    systemctl enable --now certbot.timer
fi
sed "s/__DOMAIN__/$yuji_domain/g" /opt/yuji-probe/ops/templates/nginx.conf > "$yuji_nginx"
sed -e "s/__DOMAIN__/$yuji_domain/g" -e "s/__ARCH__/$yuji_arch/g" /opt/yuji-probe/ops/templates/yuji-probe.service > /etc/systemd/system/yuji-probe.service
chmod 0644 "$yuji_nginx" /etc/systemd/system/yuji-probe.service
nginx -t
"php-fpm$yuji_php_version" -t
systemctl daemon-reload
systemctl enable --now "php$yuji_php_version-fpm" yuji-probe
systemctl reload "php$yuji_php_version-fpm"
systemctl reload nginx
yuji_healthy=0
for ((yuji_i=0; yuji_i<20; yuji_i++)); do
    if curl --silent --show-error --fail --noproxy '*' --connect-timeout 2 --max-time 5 --resolve "$yuji_domain:443:127.0.0.1" "https://$yuji_domain/" -o /dev/null; then yuji_healthy=1; break; fi
    sleep 1
done
(( yuji_healthy == 1 )) || die 'HTTPS 健康检查失败，请查看 systemctl status yuji-probe 和本站 Nginx 日志。'
systemctl is-active --quiet yuji-probe || die '主控服务未启动。'
yuji_success=1
printf '\n安装完成。访问：https://%s/\n管理员：%s\n密码为刚才设置的值，不会另存明文。\n' "$yuji_domain" "$yuji_user"
if [[ -n "$yuji_cert" ]]; then note '使用的是您提供的证书；请自行维护该证书续期。'; else note 'HTTPS 证书自动续期已启用。'; fi
note '请登录后启用二步验证，再添加监控服务器。'
