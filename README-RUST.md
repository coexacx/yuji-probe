# 羽迹探针 · Nginx + Rust

页面、安装向导、API、WSS 与主控业务均包含在预编译 Rust 二进制中。Nginx 提供 HTTPS 与反向代理，无需 PHP-FPM、Node.js、数据库或 Rust 编译环境。

- 完整安装包：GitHub Release rust-v0.10.0 的 yuji-probe-rust-0.10.0.zip。
- 原 PHP 部署版继续发布为 v0.10.0，两版共用业务代码与 Agent。
- 全新 Linux 主机运行本发行包的 install-rust.sh；使用宝塔或已有网站请按 [部署教程](docs/Nginx-Rust部署教程.md) 配置独立服务及站点。
- 一键脚本会要求域名已解析到服务器，安装 Nginx、申请 HTTPS 证书并启用续期。
- 手动部署以 -web 参数启动，首次运行后通过私有状态目录的 setup-link.txt 完成网页安装。
- 状态目录只允许服务用户读取；不要放进 Nginx 公开目录。
- 异常断线的 SSH 会话保留 5 分钟，只允许原管理登录恢复。主动断开、关闭标签、退出登录会结束会话。主控重启后需要重新登录，旧会话进入清理流程。
- 文件浏览与传输使用独立、按需建立的 SSH 连接。被控需更新到 Agent 0.2.2；安装与更新流程自动准备 tmux。


全新主机一键安装（先把域名解析到该服务器）：

```sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 https://raw.githubusercontent.com/coexacx/yuji-probe/rust-v0.10.0/install-rust.sh -o /root/yuji-install-rust.sh
bash /root/yuji-install-rust.sh
```

[完整部署教程](docs/Nginx-Rust部署教程.md) · [功能、安全与性能验收](docs/验收-0.10.0.md) · [已知安全边界](docs/RUST-SECURITY.md)

已安装的 Agent 先升级至 0.2.2，再使用可恢复终端与三个独立 SSH 通道。主控自身不承担被控服务器的 SSH 服务端角色。
