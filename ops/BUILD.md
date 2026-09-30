# Rust 源码构建

普通宝塔安装使用完整 Release 安装包 bin/ 中的静态二进制（Git 源码树不携带二进制），无需 Rust、Cargo、Go、Node.js 或数据库。主控与 Agent 均为 Rust，PHP 8.0+ 负责安装向导和 HTTP 网关，Nginx 负责 TLS/WSS。

## 开发构建

在 Debian/Ubuntu 构建机安装 rust-toolchain.toml 指定的 Rust 工具链、musl-tools、gcc-aarch64-linux-gnu、对应交叉 libc 头文件。通过 rustup 安装 x86_64-unknown-linux-musl 和 aarch64-unknown-linux-musl 目标，再在项目根目录执行：

```sh
./ops/build-backend.sh ./build
```

构建使用 Cargo.lock 锁定依赖，先运行格式检查、单元测试和 Clippy，再构建两种架构。正式版不要启用 `test-deployment`；该功能只为隔离测试服务改名，避免与生产 Agent 冲突。默认输出 `probe-linux-amd64`、`probe-linux-arm64` 与两个带版本号的 Agent。

控制器 CLI 与旧安装器兼容：`-state`、`-listen`、`-origin`、`-php-gateway`、`-daemon`。内部监听强制回环地址。`--version` 显示组件版本。

## 依赖扫描

分别在 `source/agent-rust` 和 `source/controller-rust` 执行 `cargo audit --json`，保留原始输出。主控 RSA 依赖的上游告警与可达性判断见 `docs/RUST-SECURITY.md`；不要过滤扫描输出或把已知告警称为零告警。发现新的告警需重新评审再发布。

## 前端

```sh
cd source/frontend-tools
npm ci --ignore-scripts
cd ..
node build.mjs
```

将生成的 `source/public/assets/` 复制到项目 `public/assets/`，将 `source/public/index.html` 复制为 `app/view.html`。先发布带内容哈希的资源，再替换模板。国家数据位于 `source/controller-rust/assets/countries.json`，旗帜与终端组件均自托管。

## Agent 发布源

主控从 https://github.com/coexacx/yuji-probe/releases/download/v0.4.1/ 获取 `stable.json`，先用内嵌 Ed25519 公钥验证签名，再验证精确版本、架构、文件名、长度与 SHA-256。校验失败会终止部署，不执行远端二进制。

发布下载仅允许固定仓库 HTTPS 与 GitHub 资产 CDN，最多跟随 4 次重定向。其他 HTTP 客户端仍不跟随重定向。

自建发布源时，同时修改安装器和主控的下载地址、跳转白名单及版本；修改 `source/controller-rust/src/deploy.rs` 的地址，替换 `assets/release-public.txt` 的公钥，并用自己的离线私钥签署发布清单。签名私钥不得放入源码包或网站目录。普通使用者无需自己编译或签名。
