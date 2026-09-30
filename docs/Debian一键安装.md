# Debian 一键安装：从空服务器到 HTTPS 面板

适用于全新 Debian 12 / 13、amd64 / arm64、systemd 服务器。需要 root、可访问 Debian 官方软件源与 GitHub。无需宝塔、数据库或编译器。已有网站、宝塔或本面板的服务器请用[宝塔手动教程](宝塔部署教程.md)，脚本会拒绝覆盖。

## 1. 准备域名

在 DNS 服务商添加 A 记录，指向主控公网 IPv4。若添加 AAAA，也必须指向这台主控的可用公网 IPv6；不能指向旧服务器。首次安装建议关闭 CDN 代理。

云安全组及本机防火墙需允许公网 TCP 80、443。脚本不修改 SSH 配置或关闭防火墙。80 用于证书签发及续期；443 用于 HTTPS/WSS。内部 19281 端口只监听回环，不要开放公网。

将示例换成实际域名：

```sh
getent ahosts probe.example.com
```

结果应与本机公网 IP 相符。HTTP-01 验证由外部证书机构完成，错误解析、不可达的 IPv6、CDN 拦截或阻止 Let's Encrypt 的 CAA 记录都会造成失败。

## 2. 运行

通过系统 SSH 以 root 登录，确认目标服务器：

```sh
cat /etc/os-release
hostname
```

如无 curl：

```sh
apt-get update
apt-get install -y ca-certificates curl
```

下载并运行固定版本：

```sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 https://raw.githubusercontent.com/coexacx/yuji-probe/v0.6.0/install.sh -o /root/yuji-install.sh && bash /root/yuji-install.sh
```

也可以先用 `less /root/yuji-install.sh` 查看脚本，再执行 `bash /root/yuji-install.sh`。

依次填写：

1. 域名；脚本再次提示 DNS 必须指向当前服务器。
2. 同意 Let's Encrypt 证书服务条款。
3. 站点名称。
4. 管理员用户名。
5. 12–72 字节的管理员密码，及一次确认。输入不回显。

可预填域名、证书条款及联系邮箱：

```sh
bash /root/yuji-install.sh --domain probe.example.com --accept-acme-terms --email admin@example.com
```

账号密码仍交互填写，不接受命令行密码参数。不提供邮箱时使用 Certbot 无邮箱注册模式，可日后补充。

## 3. 自动配置内容

- 检查系统、架构、已有站点、安装目录和端口；使用安装锁防止并发执行。
- 安装 Nginx、PHP-FPM、PHP CLI/cURL、Certbot、OpenSSL、Python 等依赖。
- 从本仓库固定版本 Release 下载完整安装包，校验 Ed25519 签名、长度、SHA-256 和解压路径。
- 创建独立系统用户 `yuji-probe` 与私有状态目录，保存 bcrypt 密码哈希。
- 自动生成 Nginx HTTP 验证站点，使用 Certbot webroot 模式申请证书。
- 自动配置 HTTPS/WSS、PHP 独立池、Rust systemd 服务、证书续期及续期后 Nginx 重载。
- 校验 Nginx / PHP 配置并执行 HTTPS 健康检查。

成功后访问输出的 HTTPS 地址，点击右上角设置图标登录。自动安装已经完成账号初始化，不必再次填写网页向导。先启用二步验证，再添加节点；初始看板为空。

## 4. 路径与维护

| 路径 / 服务 | 用途 |
| --- | --- |
| `/opt/yuji-probe` | root 拥有的程序与源码 |
| `/opt/yuji-probe/public` | 唯一公开目录 |
| `/var/lib/yuji-probe` | 私有状态，0700；程序中的 storage 指向这里 |
| `/etc/nginx/sites-available/yuji-probe.conf` | Nginx 配置 |
| `/etc/php/8.x/fpm/pool.d/yuji-probe.conf` | PHP 独立池 |
| `/etc/systemd/system/yuji-probe.service` | 主控服务 |
| `/etc/letsencrypt/live/yuji-probe` | 自动申请的证书 |

检查服务：

```sh
systemctl status yuji-probe --no-pager
nginx -t
systemctl status certbot.timer --no-pager
journalctl -u yuji-probe -n 80 --no-pager
```

续期测试会访问证书机构测试环境：

```sh
certbot renew --cert-name yuji-probe --dry-run
```

## 5. 失败处理

证书失败时先检查 DNS、80 端口、AAAA、CDN 和 CAA；不要关闭 TLS 验证。

创建本站配置后若安装失败，脚本停止本站主控、撤下本站 Nginx/PHP 池配置，但保留软件、程序和私有状态供排查。不会重置密码。重新运行发现已有状态会停止；先检查原因，再按手动教程修复。不要删除含真实节点的 storage。

若 GitHub 返回 404，检查脚本对应的 Release 是否公开且包含安装包和签名清单。下载失败不回退到不受信任镜像或未签名二进制。

## 6. 备份、升级与迁移

备份至少包含 `/var/lib/yuji-probe`、网站配置和 HTTPS 配置，应放在公开目录之外。备份中含认证状态、节点凭据和通知配置。

升级不是再次执行安装脚本。阅读新版本说明，备份，停止本站 `yuji-probe` 服务，替换程序但完整保留 storage 链接和 `/var/lib/yuji-probe`，再启动检查。切换域名还需更新证书、面板 origin 与各节点 WSS 地址。

高级用户可通过 `--cert-file /absolute/fullchain.pem --key-file /absolute/privkey.pem` 使用现有受信任证书。脚本检查证书域名和密钥匹配，外部证书续期由使用者维护；普通安装无需使用。

## 官方参考

- [Certbot webroot 与续期](https://eff-certbot.readthedocs.io/en/stable/using.html)
- [Let's Encrypt HTTP-01](https://letsencrypt.org/docs/challenge-types/#http-01-challenge)
- [证书服务条款](https://letsencrypt.org/repository/)
- [GitHub Release 下载链接](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)
