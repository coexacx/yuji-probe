# 羽迹探针 · PHP 部署版

本仓库维护 Nginx + PHP-FPM + Rust 主控与 Agent 的服务器监控面板，支持浏览器 SSH、SFTP 和 Telegram 通知，无需数据库。

**Nginx + Rust 单二进制版已迁至独立仓库：[coexacx/yuji-probe-rust](https://github.com/coexacx/yuji-probe-rust)**。新安装、下载和后续更新请使用新仓库；已有 Rust 0.10.0 用户参照 [迁移步骤](https://github.com/coexacx/yuji-probe-rust/blob/main/docs/仓库迁移-0.10.1.md)。

[下载发行版](https://github.com/coexacx/yuji-probe/releases) · [Linux 一键安装](docs/Linux一键安装.md) · [宝塔手动部署](docs/宝塔部署教程.md) · [构建源码](ops/BUILD.md)

- 五套内置主题：默认、纯透明液态玻璃、简笔画、纯二次元、四季；支持上传、预览和导出自定义主题包，含背景图、明暗配色、局部样式和站点图标。四季采用连续动画，每季一分钟。详见[主题与外观](docs/主题与外观.md)及[主题开发文档](docs/主题开发.md)。

## 功能

- 公开看板：全球国家与旗帜、CPU、内存、已启用 Swap、磁盘、网卡实时速率与累计上下行流量。
- 管理后台：自动部署节点、供应商与到期日、聚合续费提醒、Telegram 上下线通知、TOTP 二步验证和常用命令。
- 浏览器 SSH / SFTP：通过经过身份认证的 Agent WSS 通道连接，支持手机快捷键（Tab / Esc / Ctrl / 方向键）、粘贴、终端查找、目录浏览、最多 4 个标签、文件上传下载、新建文件与目录、重命名、UTF-8 文件编辑与并发修改检查；保存时不备份原文件。
- 看板整理：分组筛选、置顶、拖动排序与管理员私有备注；只读查看当前进程、systemd 服务和日志。
- 运维：加密自动备份、S3 / WebDAV 自动异地保存、跨域名迁移、一次性恢复码、设备撤销与 Telegram 状态、续费提醒。
- 版本管理：签名升级、健康检查、失败恢复与手动回退。见[运维与恢复](docs/运维与恢复.md)。
- 续费管理：按自然月、季度、年或自定义天数续期，可填写实际到期日；记录每期金额，分币种展示月/年预算及近期到期支出。
- 管理员填写节点 SSH 信息后，主控从本仓库 Releases 下载 Agent、校验签名后部署；节点不需要编译环境。

详见[功能与异地备份配置](docs/功能与异地备份.md)。异地存储默认关闭，需填写自己的存储凭据。

## 两种部署方式

| 发行版 | 运行环境 | 安装 |
| --- | --- | --- |
| Nginx + Rust | Nginx + 一个 Rust 二进制，无 PHP | [独立仓库](https://github.com/coexacx/yuji-probe-rust) · [手动/宝塔教程](https://github.com/coexacx/yuji-probe-rust/blob/main/docs/Nginx-Rust部署教程.md) |
| Nginx + PHP + Rust | 保留原有 PHP 网关与 Rust 主控 | 以下原版安装方法 |

两个项目分别维护源码、Release、安装脚本和更新源。本仓库的 PHP 安装方法与 v* 更新通道继续保留。

## Linux 自动安装

适合全新的 Debian、Ubuntu、Rocky Linux、AlmaLinux、CentOS Stream、Fedora，支持 amd64 / arm64、systemd。具体发行版本见[系统支持列表](docs/系统支持.md)。已有宝塔或网站的服务器请使用手动教程。

**先将域名 A 记录解析到主控服务器公网 IPv4；若设置 AAAA，也必须指向本机 IPv6。关闭 CDN 代理，放行 TCP 80、443。**

在 root 的 SSH 终端执行：

```sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 https://raw.githubusercontent.com/coexacx/yuji-probe/v0.10.0/install.sh -o /root/yuji-install.sh && bash /root/yuji-install.sh
```

脚本询问域名、站点名称、管理员用户名与密码，随后自动安装 Nginx、PHP-FPM，申请 Let's Encrypt 证书，配置 HTTPS/WSS 和证书续期，并启动主控。密码输入不回显，不放入命令行、环境变量或明文配置。

Debian / Ubuntu 未安装 curl 时先执行 `apt-get update && apt-get install -y ca-certificates curl`；RPM 系统使用 `dnf install -y ca-certificates curl`。安装器使用 root 权限；执行前可以查看下载的脚本。初始脚本的信任来自 GitHub HTTPS 和所选版本；后续安装包及 Agent 另有 Ed25519 签名校验。

## 宝塔手动部署

从 [v0.10.0 Release](https://github.com/coexacx/yuji-probe/releases/tag/v0.10.0) 下载 **yuji-probe-panel-0.10.0.zip**。它包含 PHP、前端源码、Rust 源码和两种架构的主控二进制。

解压到独立站点目录，安装 Nginx 和 PHP 8.0+（新部署建议 PHP 8.4），运行目录设为 `public/`。按[手把手教程](docs/宝塔部署教程.md)配置 HTTPS、WSS 与目录权限，再使用服务器内的一次性链接打开网页安装向导。

GitHub 的 Code → Download ZIP 和自动生成的 Source code 压缩包**不含预编译主控**，用于源码开发。完整安装包不含预置管理员、真实节点或运行密钥。

## 下载与版本

| 组件 | 版本 | 发布位置 |
| --- | --- | --- |
| 界面 / 主控 | 0.10.0 | 本仓库 v0.10.0 Release |
| Rust Agent | 0.2.2 | 同一 Release 的 amd64 / arm64 二进制 |

主控锁定本次 Release 的 Agent 清单地址，支持 GitHub 的 HTTPS 下载跳转，只接受 GitHub 发布域名。下载后核对签名、版本、架构、文件名、长度和 SHA-256；失败即终止部署。无需 GitHub 账号或 Token，不依赖原私有下载站点。

源码沿用原来的 crate / 二进制 / Agent 服务名称，以保持协议和迁移兼容。项目按 MIT 开源；第三方依赖许可证见 `THIRD-PARTY-NOTICES/`。签名私钥不在仓库或发布包中。

## 维护与边界

- HTTPS/WSS、节点独立凭据、SSH 主机指纹固定、CSRF/Origin 校验、登录限速及 TOTP。
- `storage/` 必须位于网站公开目录之外。备份放在网站之外，升级必须完整保留此目录。
- 浏览器终端具有所配置 SSH 用户的权限。只向受信任的管理员开放后台，并开启二步验证。
- 当前有一个上游 RSA 依赖告警。现有部署使用 Ed25519 私钥，受影响的 RSA 私钥操作不在当前调用路径；这不等于依赖告警已经修复。详见 [安全说明](docs/RUST-SECURITY.md)。
- 0.5.0 对公开看板使用共享序列化快照，减少重复 JSON 构造。吞吐与资源占用以同条件 200 节点 / 200 客户端复测报告为准，保留旧版报告用于对照。

[0.10.0 功能与验收](docs/验收-0.10.0.md)；[0.7.1 主题验收](docs/验收-0.7.1.md)；[0.6.0 功能验收](docs/验收-0.6.0.md)；[0.5.0 性能对照报告](docs/验收-0.5.0.md)注明实际测试与尚未验证的范围。ARM64 编译支持与真机验收是两回事。

终端在授权及网络有效时支持持续连接；异常断线五分钟内可恢复同一 Shell，主动关闭会结束会话。文件队列与低内存续传、增强编辑器和一次性命令接入，见[操作说明](docs/终端与文件操作.md)。
