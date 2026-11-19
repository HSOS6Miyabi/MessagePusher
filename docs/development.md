# 开发环境、测试与修改指南

本指南面向维护源代码的开发者。首次了解系统结构请先阅读 [系统结构说明](architecture.md)，实际部署与升级参阅 [运维操作手册](operations.md)。

## 1. 开发环境

| 软件 | 用途 |
| --- | --- |
| Rust 与 Cargo（稳定版工具链） | 编译、运行、测试 Rust 主程序 |
| PostgreSQL 17 | 保存数据和执行数据库迁移；其他兼容版本需自行验证 |
| Node.js 22 | 运行消息代理测试、浏览器测试和辅助脚本；消息代理声明最低 Node.js 20 |
| Chromium（通过 Playwright 安装） | 自动操作与截图检查 |
| Docker 与 Docker Compose（可选） | 启动独立测试环境 |
| Git | 版本控制与提交 |

仓库包含 `Cargo.toml`，消息代理的测试命令定义在 `relay/package.json`。代码路径见 [系统结构说明](architecture.md)。

## 2. 在本机启动 Rust 主程序

最简单的方式是使用 Docker Compose：

```bash
cp .env.example .env
# 编辑 .env，为各类密钥设置独立的随机值
docker compose up -d --build
curl -fsS http://127.0.0.1:8080/healthz
```

`.env.example` 中的密钥只是占位文本，不能直接用于启动实际服务。可分别运行 `openssl rand -base64 32` 和 `openssl rand -hex 32` 生成密钥。数据库会使用 Compose 创建的 PostgreSQL 服务。

需要使用 Cargo 调试时，先启动独立 PostgreSQL，准备正确的环境变量，再运行：

```bash
set -a
. ./.env
set +a
cargo run
```

Shell 读取 `.env` 前应检查其中的取值是否适合直接用 Shell 加载。使用 Compose 时，`DATABASE_URL` 会被 Compose 配置覆盖为容器内的 PostgreSQL 地址；在宿主机运行 `cargo run` 时，应使用宿主机可访问的数据库地址。

程序启动时调用 `sqlx::migrate!` 自动执行迁移，之后创建首位管理员（仅当用户表为空）。不要把生产数据库作为开发测试数据库。

## 3. 每次提交需要运行的检查

从仓库根目录执行：

```bash
cargo test --all
node --check web/app.js
node --check scripts/benchmark.mjs
node --check scripts/telegram-live-e2e.mjs
node scripts/check-docs.mjs
```

执行消息代理测试：

```bash
cd relay
npm test
cd ..
```

第一次运行浏览器测试时，安装 Chromium 与 Playwright：

```bash
npm install --no-save --no-package-lock --no-audit --no-fund playwright@1.52.0
npx playwright install chromium
node web/tests/browser.mjs
node web/tests/complete-ui.mjs
```

`web/tests/browser.mjs` 和 `web/tests/complete-ui.mjs` 使用本地模拟 API，检查页面交互和响应式布局，并把截图保存到 `web/screenshots/` 下。建议同时检查 320、390、768、1024 和 1440 像素宽度下的页面。

如果改动了真实数据库或 API 行为，在独立测试环境运行：

```bash
node web/tests/live-backend.mjs
```

此脚本需要正在运行的 Rust 服务、PostgreSQL 及可登录的测试管理员（读取 `ADMIN_USERNAME` 和 `ADMIN_PASSWORD`）。脚本会在测试数据库中创建和删除用户，并提交包含附件的消息；只在可清理的数据环境中执行。

## 4. 负载测试与真实平台测试

本地模拟代理负载测试：

```bash
export MP_BENCH_ADMIN_USERNAME='YOUR_TEST_ADMIN'
export MP_BENCH_ADMIN_PASSWORD='YOUR_TEST_PASSWORD'
node scripts/benchmark.mjs --messages=400 --concurrency=48 --providers=4 --relay-delay-ms=40
```

测试依赖正在运行的主程序及独立 PostgreSQL，需要临时允许本机 HTTP 模拟代理（`ALLOW_HTTP_RELAY=1`）。真实服务器应使用 HTTPS 代理，正式环境不应启用该测试设置。参数和结果解释见 [性能测试](performance.md)。

真实 Telegram 测试脚本为 `scripts/telegram-live-e2e.mjs`，需要提供 `E2E_ADMIN_USERNAME`、`E2E_ADMIN_PASSWORD`、`E2E_BOT_TOKEN`，并按需要设置 `E2E_CHAT_ID`、`E2E_BASE_URL`、`E2E_RELAY_URL`、`E2E_RELAY_SECRET`。脚本会实际发送 Telegram 消息和文件，应使用专门的测试机器人与接收者。它不会替换现有机器人的 Webhook，相关订阅回调通过经过认证的模拟请求验证。详见 [Telegram 使用说明](telegram.md)。

## 5. 修改功能时的顺序

1. 先阅读相关 Rust 模块和现有测试，核实当前接口、数据库字段与调用方式。
2. 确认是否涉及 `src/models.rs` 的输入验证、`src/api.rs` 的用户权限、`src/delivery.rs` 的发送行为或 `relay/core.js` 的代理协议。
3. 修改数据库结构时添加新迁移文件，不能改写已经执行的迁移。
4. 修改前端时同时测试管理员和普通用户，检查浏览器控制台、弹窗、键盘操作、手机布局与附件。
5. 修改签名协议、超时、重试或附件限制时同时测试 Rust 和 JavaScript 两端。
6. 更新用户文档、开发文档及 [维护台账](maintenance-ledger.md)，按 [设计决定](decisions.md) 记录重要方案。
7. 本地运行相关测试，再通过 GitHub Actions 检查对应平台的编译和发布结果。

敏感密钥只通过受保护的测试环境变量传入，不写入测试脚本、截图、日志或提交记录。

## 6. GitHub Actions

- `.github/workflows/ci-release.yml` 运行 Rust 单元测试、代理测试、数据库/API 测试、实际后端浏览器测试以及使用模拟 API 的前端测试，并编译五个平台的程序。
- `.github/workflows/docker.yml` 分别构建 Linux AMD64 和 ARM64 Docker 镜像，并发布多架构镜像。
- 提交到 `main` 后，测试与编译通过的最新提交更新滚动 `edge` Release。推送 `v*` 版本标签时创建对应的版本 Release。
- 浏览器测试截图以 GitHub Actions Artifact 形式保存；这些截图不是生产用户数据。

发布前后的检查、如何确认标签与提交一致，参阅 [运维操作手册](operations.md)。

## 7. 常用检查位置

| 需要检查的内容 | 查看位置 |
| --- | --- |
| HTTP 路由 | `src/main.rs` 的 `create_router` |
| API 权限 | `src/api.rs`、`src/site.rs`、`src/catalog.rs` |
| Provider 配置校验 | `src/models.rs` |
| 数据库结构 | `migrations/` |
| 任务领取、重试与平台 HTTP 调用 | `src/delivery.rs` |
| 消息代理身份验证 | `relay/core.js` |
| 管理页面交互 | `web/app.js` |
| 管理页面布局 | `web/styles.css` |
| 截图与界面交互 | `web/tests/` |
| CI 构建和版本发布 | `.github/workflows/` |

新增代码后请确保用户能通过文档中的路径找到该功能；避免将文档写成只适用于作者本人的操作步骤。
