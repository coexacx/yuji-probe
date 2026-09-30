# 羽迹探针

轻量的服务器监控面板。Rust 主控与 Agent，透明玻璃界面，支持浏览器 SSH、SFTP 和 Telegram 通知。无需数据库。

[下载发行版](https://github.com/coexacx/yuji-probe/releases) · [Debian 一键安装](docs/Debian一键安装.md) · [宝塔手动部署](docs/宝塔部署教程.md) · [构建源码](ops/BUILD.md)

## 功能

- 公开看板：全球国家与旗帜、CPU、内存、已启用 Swap、磁盘、网卡实时速率与累计上下行流量。
- 管理后台：自动部署节点、供应商与到期日、聚合续费提醒、Telegram 上下线通知、TOTP 二步验证和常用命令。
- 浏览器 SSH / SFTP：通过经过身份认证的 Agent WSS 通道连接，支持移动端粘贴、目录浏览、UTF-8 文件编辑与并发修改检查；保存时不备份原文件。
- 管理员填写节点 SSH 信息后，主控从本仓库 Releases 下载 Agent、校验签名后部署；节点不需要编译环境。

## Debian 自动安装

适合全新的 Debian 12 / 13、amd64 / arm64、systemd 服务器。已有宝塔或网站的服务器请使用手动教程。

**先将域名 A 记录解析到主控服务器公网 IPv4；若设置 AAAA，也必须指向本机 IPv6。关闭 CDN 代理，放行 TCP 80、443。**

在 root 的 SSH 终端执行：

```sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 https://raw.githubusercontent.com/coexacx/yuji-probe/v0.4.1/install.sh -o /root/yuji-install.sh && bash /root/yuji-install.sh
```

脚本询问域名、站点名称、管理员用户名与密码，随后自动安装 Nginx、PHP-FPM，申请 Let's Encrypt 证书，配置 HTTPS/WSS 和证书续期，并启动主控。密码输入不回显，不放入命令行、环境变量或明文配置。

未安装 curl 时先执行 `apt-get update && apt-get install -y ca-certificates curl`。安装器使用 root 权限；执行前可以查看下载的脚本。初始脚本的信任来自 GitHub HTTPS 和所选版本；后续安装包及 Agent 另有 Ed25519 签名校验。

## 宝塔手动部署

从 [v0.4.1 Release](https://github.com/coexacx/yuji-probe/releases/tag/v0.4.1) 下载 **yuji-probe-panel-0.4.1.zip**。它包含 PHP、前端源码、Rust 源码和两种架构的主控二进制。

解压到独立站点目录，安装 Nginx 和 PHP 8.0+（新部署建议 PHP 8.4），运行目录设为 `public/`。按[手把手教程](docs/宝塔部署教程.md)配置 HTTPS、WSS 与目录权限，再使用服务器内的一次性链接打开网页安装向导。

GitHub 的 Code → Download ZIP 和自动生成的 Source code 压缩包**不含预编译主控**，用于源码开发。完整安装包不含预置管理员、真实节点或运行密钥。

## 下载与版本

| 组件 | 版本 | 发布位置 |
| --- | --- | --- |
| 界面 / 主控 | 0.4.1 | 本仓库 v0.4.1 Release |
| Rust Agent | 0.1.3-rust.1 | 同一 Release 的 amd64 / arm64 二进制 |

主控锁定本次 Release 的 Agent 清单地址，支持 GitHub 的 HTTPS 下载跳转，只接受 GitHub 发布域名。下载后核对签名、版本、架构、文件名、长度和 SHA-256；失败即终止部署。无需 GitHub 账号或 Token，不依赖原私有下载站点。

源码沿用原来的 crate / 二进制 / Agent 服务名称，以保持协议和迁移兼容。项目按 MIT 开源；第三方依赖许可证见 `THIRD-PARTY-NOTICES/`。签名私钥不在仓库或发布包中。

## 维护与边界

- HTTPS/WSS、节点独立凭据、SSH 主机指纹固定、CSRF/Origin 校验、登录限速及 TOTP。
- `storage/` 必须位于网站公开目录之外。备份放在网站之外，升级必须完整保留此目录。
- 浏览器终端具有所配置 SSH 用户的权限。只向受信任的管理员开放后台，并开启二步验证。
- 当前有一个上游 RSA 依赖告警。现有部署使用 Ed25519 私钥，受影响的 RSA 私钥操作不在当前调用路径；这不等于依赖告警已经修复。详见 [安全说明](docs/RUST-SECURITY.md)。
- 旧版 200 节点 / 200 客户端压测中，Rust 主控内存更低，但 HTTP 吞吐低于 Go；本版只修改分发和安装，不宣称已解决该性能差距。见 [实测报告](docs/vistart-probe-200-node-benchmark-20260930.md)。

[GitHub 版验收记录](docs/GITHUB-RELEASE-TESTS.md)分别注明实际测试与尚未验证的范围。ARM64 编译支持与真机验收是两回事。
