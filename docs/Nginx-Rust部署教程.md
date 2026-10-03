# Nginx + Rust 部署教程（0.10.0）

本发行版将页面、静态资源、网页安装向导、API、WSS 和业务主控放在一个 Rust 可执行文件里。运行时使用 Nginx 与一个主控进程，不启动 PHP-FPM，不需要数据库、Node.js 或 Rust 编译环境。源码包同时附带 amd64、arm64 二进制，实际只运行对应架构的一个。

两种发行版独立更新：PHP 版为 `v0.10.0`；本版为 `rust-v0.10.0`。两版使用相同 Agent 0.2.2、业务协议与状态格式。不要把另一种发行包直接交给后台升级器。

## 一、全新 Linux 主机一键安装

适用于支持列表中的 Linux + systemd 主机。先把域名 A 记录指向本机；有 AAAA 记录时也须指向本机 IPv6。关闭 CDN 代理，在云安全组放行 TCP 80、443。具体系统见 [系统支持](系统支持.md)。

以 root 登录主控服务器：

```sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 https://raw.githubusercontent.com/coexacx/yuji-probe/rust-v0.10.0/install-rust.sh -o /root/yuji-install-rust.sh
bash /root/yuji-install-rust.sh
```

脚本询问域名、站点名称、管理员用户名、密码和证书条款，安装 Nginx、申请 Let's Encrypt 证书并配置自动续期。管理员密码为 12–72 字节，输入不回显。脚本通过标准输入传递密码，不放入命令行或环境变量。

首次脚本依赖 GitHub HTTPS 与所选版本的可信性；请先阅读再执行。发行包会额外校验 Ed25519 签名、SHA-256、版本、架构和文件大小。

已有网站、宝塔、占用的 80/443/19281 端口或现有安装会使脚本停止，防止覆盖。此时按下一节手动安装。安装所用 Python 3、curl、OpenSSL、Certbot 是部署与维护工具，不是后台常驻 Web 服务。自带有效证书时可用 `--cert-file` 与 `--key-file`；此时由自己维护证书续期。

常用检查：

```sh
systemctl status yuji-probe --no-pager
journalctl -u yuji-probe -n 50 --no-pager
nginx -t
systemctl list-timers yuji-probe-certbot.timer
```

## 二、宝塔或已有 Nginx 的手动安装

以下用 `probe.example.com` 举例，必须替换成自己的域名。使用独立项目目录 `/opt/yuji-probe-rust`，数据目录 `/var/lib/yuji-probe-rust`，内部端口 `19282`。部署第二套面板时不要共用这些目录、用户、服务名或内部端口。

### 1. 准备站点和证书

1. 安装 Nginx；此站点不需要 PHP 或 MySQL。
2. 在宝塔添加站点、绑定域名，PHP 版本选择“纯静态”。站点目录可使用空目录。
3. 申请并启用 SSL，确认域名通过 HTTPS 正常访问。保留宝塔的证书续期配置。
4. 内部端口只绑定 127.0.0.1，云安全组不开放 19282。
5. Debian/Ubuntu 安装维护工具：`apt-get update && apt-get install -y ca-certificates curl unzip python3 openssl`。RPM 系统使用相应 dnf 软件包。

### 2. 下载完整发行包

到 [rust-v0.10.0 Release](https://github.com/coexacx/yuji-probe/releases/tag/rust-v0.10.0) 下载 `yuji-probe-rust-0.10.0.zip` 及同名 `.sha256` 文件。不要下载 GitHub 自动生成的 Source code 包：它没有预编译二进制。

```sh
install -d -m 700 /root/yuji-rust-install
cd /root/yuji-rust-install
curl -fLO --proto '=https' --proto-redir '=https' https://github.com/coexacx/yuji-probe/releases/download/rust-v0.10.0/yuji-probe-rust-0.10.0.zip
curl -fLO --proto '=https' --proto-redir '=https' https://github.com/coexacx/yuji-probe/releases/download/rust-v0.10.0/yuji-probe-rust-0.10.0.zip.sha256
sha256sum -c yuji-probe-rust-0.10.0.zip.sha256
unzip yuji-probe-rust-0.10.0.zip
test ! -e /opt/yuji-probe-rust
mv yuji-probe-rust-0.10.0 /opt/yuji-probe-rust
```

校验和用于核对下载内容；完整的签名校验由一键安装器和后台升级器执行。手动部署只从本仓库受信任的 Release 获取包和校验文件。

确认结构：

```text
/opt/yuji-probe-rust/
  bin/probe-linux-amd64
  bin/probe-linux-arm64
  ops/templates/nginx-rust.conf
  ops/templates/yuji-probe-rust.service
  source/                       # 源码，运行无需编译
  docs/
```

### 3. 创建服务用户与私有数据目录

```sh
useradd --system --user-group --home-dir /var/lib/yuji-probe-rust --shell /usr/sbin/nologin yuji-probe-rust
install -d -m 700 -o yuji-probe-rust -g yuji-probe-rust /var/lib/yuji-probe-rust
chown -R root:root /opt/yuji-probe-rust
chmod 755 /opt/yuji-probe-rust/bin/probe-linux-*
```

如果用户名或目录已存在，先确认是否为已有安装，不要覆盖。程序目录由 root 持有，服务用户只写私有数据目录。Nginx 不直接公开源码、二进制或数据目录。

### 4. 创建 systemd 服务

查看 `uname -m`：x86_64 使用 amd64；aarch64 使用 arm64。

创建 `/etc/systemd/system/yuji-probe-rust.service`。下面以 amd64 为例，替换域名；ARM 服务器把程序名改为 `probe-linux-arm64`：

```ini
[Unit]
Description=Yuji Probe Nginx + Rust
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=yuji-probe-rust
Group=yuji-probe-rust
WorkingDirectory=/opt/yuji-probe-rust
ExecStart=/opt/yuji-probe-rust/bin/probe-linux-amd64 -state /var/lib/yuji-probe-rust/control -listen 127.0.0.1:19282 -origin https://probe.example.com -web
Restart=always
RestartSec=3
TimeoutStopSec=10
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/yuji-probe-rust
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
CapabilityBoundingSet=
LockPersonality=true
MemoryMax=512M
TasksMax=128

[Install]
WantedBy=multi-user.target
```

`MemoryMax` 是保护上限，不是预留或实际占用。根据规模与备份/主题处理峰值调整，资源比较见验收报告。

```sh
systemctl daemon-reload
systemctl enable --now yuji-probe-rust
systemctl status yuji-probe-rust --no-pager
```

首次启动会生成私有安装链接；还没有创建任何管理员。程序不会在日志中输出密码或安装密钥。

### 5. 配置本网站 Nginx 反向代理

在宝塔该站点配置的 **HTTPS server 块**中，删除本网站原有的 PHP、伪静态和冲突的 `location /`。保留域名、证书路径、SSL 设置和证书验证相关配置。加入：

```nginx
client_max_body_size 24m;
client_header_timeout 10s;
client_body_timeout 10s;
send_timeout 15s;
# 不把私有安装链接的查询参数写入访问日志；后台另有操作审计。
access_log off;

location = /_internal/health { return 404; }
location ~ ^/(?:app|bin|storage|source|docs|ops|vendor|node_modules)(?:/|$) { return 404; }
location ~ /\. { return 404; }

location ~ ^/api/(?:agent|terminal)$ {
    proxy_pass http://127.0.0.1:19282;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 185s;
    proxy_send_timeout 30s;
    proxy_buffering off;
}
location = /api/enroll/claim {
    proxy_pass http://127.0.0.1:19282;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Probe-Gateway "";
    proxy_read_timeout 185s;
    proxy_buffering off;
}
location / {
    proxy_pass http://127.0.0.1:19282;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Probe-Gateway "";
    proxy_set_header Connection "";
    proxy_read_timeout 65s;
    proxy_send_timeout 30s;
}
```

如果使用非标准 HTTPS 端口，需要在 `-origin` 带上端口，并把 Host 设置改为 `$http_host`。常规 443 使用上面的配置即可。不要直接把来自互联网的 X-Real-IP 或 X-Probe-Gateway 透传给主控。

宝塔保存前会校验配置；手工管理的 Nginx 使用 `nginx -t` 检查通过后仅 reload。宝塔可能使用 `/www/server/nginx/sbin/nginx`。不要停止其他站点的 Nginx 或 PHP 服务。

启用 SELinux 的系统使用发行包内 `ops/templates/yuji-probe.cil`，将内部监听端口标记为 `yuji_probe_port_t`，程序、状态与证书设置对应标签；保持 enforcing。自定义目录与端口时参照 `install-rust.sh` 的 SELinux 段调整，不直接关闭 SELinux。

### 6. 使用网页安装向导

在服务器终端运行：

```sh
cat /var/lib/yuji-probe-rust/control/setup-link.txt
```

复制输出的完整 HTTPS 链接到自己的浏览器。该链接含安装所有权凭据，不发给他人。填写站点名称、管理员用户名与密码，提交后跳转登录页面。首次访问普通域名只显示安装尚未完成，不能抢先创建管理员。

安装完成后，链接文件会删除，安装入口锁定。管理员密码保存为 bcrypt 哈希。访问域名、登录、启用二步验证，再添加服务器。无需数据库配置。

### 7. 启用后台签名更新与回退

以 root 执行：

```sh
python3 /opt/yuji-probe-rust/ops/update-panel.py --configure --name rust-main --root /opt/yuji-probe-rust --state /var/lib/yuji-probe-rust/control --service yuji-probe-rust.service --origin https://probe.example.com --listen 127.0.0.1:19282 --distribution rust
```

版本管理只检查 `rust-v*` 的正式发行版。管理员确认更新后，独立 root 更新服务下载并校验签名，保存上一版与私有状态，更新、重启并做健康检查；失败自动恢复，也可手动回退。不是发现新版后无人值守自动升级。

## 三、从现有 PHP 版迁移

建议先在独立测试站验证。迁移本网站时：

1. 在后台生成加密备份并下载到站点之外，同时保留旧程序与旧 Nginx 配置。
2. 暂停管理员操作，结束终端和传输。只停止探针自己的旧主控/专用 PHP 池，保持其他网站服务运行。
3. 把旧 `storage/control` **完整复制**到新私有状态目录，保留 `app.key`、认证配置、节点配置、加密密钥与 SSH 指纹。不可只复制 nodes.json，也不运行新安装向导覆盖原配置。
4. 将数据目录所有者改为新的服务用户，目录 700、文件 600。旧副本保留在只有 root 可读的位置。
5. 用新 Rust 服务以相同 `-origin` 启动，修改该域名的 Nginx 代理端口，检查后 reload。
6. 验证原管理员登录、所有节点上线、SSH/文件操作、通知和备份。主控重启会使旧登录过期，需重新登录。
7. 重新用 `--distribution rust` 注册这一实例的更新器。停用旧实例更新监听，避免两个更新器同时操作同一状态。
8. 验收后再清理本网站不再使用的 PHP 池。不要停掉其他网站正在使用的 PHP-FPM。

同域名且完整保留状态时，Agent 不需要重新录入节点密钥。若同时更换域名，使用面板的加密备份恢复/迁移功能按其提示重新配置 Agent，不只修改 Nginx 域名。

需要回到旧 PHP 版时，停止新服务，恢复迁移前的程序、状态副本和该站点 Nginx 配置，再启动旧的专用服务。不要让两套主控同时写同一个数据目录。

## 四、终端行为与故障排查

Agent 需更新到 0.2.2，安装/更新时会准备 tmux。SSH 终端、目录浏览/编辑、文件传输各用按需建立的独立 SSH 连接，仍通过经认证的 WSS Agent 通道传输。

异常断线后原 Shell 最多保留五分钟，只允许原管理登录恢复；主动断开、关闭终端标签、退出登录会结束该 Shell。主控重启、远端重启或远端主动退出不能恢复。命令不会自动重发。详见 [终端与文件操作](终端与文件操作.md)。

- 502：检查 systemd 服务、内部监听端口、目录所有权与日志。
- 421：检查 `-origin`、访问域名、HTTPS 端口和 Nginx Host 头一致。
- 安装提示私有链接：在服务器读取 setup-link.txt；已安装时不要删除 auth.json 强行重开安装。
- WebSocket 失败：检查对应 location 的 Upgrade/Connection 头、代理超时和 HTTPS 证书。
- 无法建立可恢复终端：先更新 Agent 并确认目标服务器已安装 tmux；检查 SSH 指纹与用户权限。
- 升级失败：查看版本管理提示和 `journalctl -u yuji-probe-update-rust-main`，签名或校验失败时不要跳过校验。

本版不要求打开新的公网管理端口。Nginx 接受 443，Rust 主控只监听回环地址，Agent 主动连接主控 WSS。
