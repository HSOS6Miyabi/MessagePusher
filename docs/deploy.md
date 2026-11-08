# 部署、更新和维护

## Docker Compose

1. 下载仓库，复制 `.env.example` 为 `.env`。
2. 在 `.env` 中填写 `POSTGRES_PASSWORD`、`ADMIN_USERNAME`、`ADMIN_PASSWORD`、`JWT_SECRET`、`CONFIG_KEY_BASE64`。
3. 执行 `docker compose up -d --build`。
4. 浏览器打开 `http://你的服务器:8080`，用管理员账户登录。在“站点设置”中填写站点名称和公告，决定是否开放用户自行注册，并选择向用户提供哪些 Provider 类型。各用户登录后自行添加自己的推送渠道。公开注册默认关闭，各个内置 Provider 类型默认开放。
5. 在正式对外使用前，配置反向代理和 HTTPS，然后把 `PUBLIC_BASE_URL` 改为公开的 HTTPS 地址，执行 `docker compose up -d` 使设置生效。

更新程序时，在保存好数据库备份后，拉取仓库新版本，执行 `docker compose up -d --build`。程序启动时会自动执行新增的数据库迁移文件。

数据库保存在 Docker 卷 `postgres_data`，不要在未备份的情况下执行 `docker compose down -v`。

## 普通 Linux、Windows、macOS 服务器

从 GitHub Releases 下载对应系统的程序文件，解压后设置环境变量，运行 `messagepusher`（Windows 使用 `messagepusher.exe`）。程序需要一个可连接的 PostgreSQL 数据库；无须安装 Node.js，也不需要额外运行消息代理。

自行编译时安装 Rust，再执行：

```bash
cargo build --release
```

把 `target/release/messagepusher` 放到服务器上，设置 `BIND_ADDRESS` 等环境变量即可。主程序通常运行在 8080 端口。建议交给 systemd 等服务管理器维持运行。

## 代理部署到 Cloudflare Workers

进入 `relay` 目录，执行：

```bash
npx wrangler login
npx wrangler secret put RELAY_SECRET
npx wrangler deploy
```

按提示输入至少 24 字符的随机密钥。部署成功后复制 HTTPS 地址，填写到主程序对应平台的代理设置中。

Cloudflare Workers 可以通过 HTTPS 向 Telegram、企业微信和 Resend 发请求。它不能使用本项目的 SMTP 发送功能。无需设置数据库。

## 代理部署到 Vercel

1. 在 Vercel 创建项目并导入本 GitHub 仓库。
2. 将项目根目录设为 `relay`，使用默认的 Node.js 函数。
3. 在环境变量中添加 `RELAY_SECRET`，值为至少 24 字符的随机字符串。
4. 部署后用 `https://你的代理域名/healthz` 检查服务是否正常。
5. 在主程序的对应平台设置中填入代理 URL 和相同密钥。

不要把主程序和代理的地址混淆。代理没有用户登录、数据库或管理页面。主程序接收所有外部推送调用，并将需要中转的发送工作交给代理。

## 环境变量

| 名称 | 用途 |
| --- | --- |
| `DATABASE_URL` | PostgreSQL 连接地址，普通服务器必须填写 |
| `POSTGRES_PASSWORD` | 使用 Docker Compose 时，设置 PostgreSQL 密码 |
| `CONFIG_KEY_BASE64` | 32 个随机字节的 Base64 编码，保护平台设置 |
| `JWT_SECRET` | 至少 32 字符的随机字符串，用于签发登录令牌 |
| `ADMIN_USERNAME` | 首次创建管理员时使用的用户名 |
| `ADMIN_PASSWORD` | 首次创建管理员时使用的密码 |
| `PUBLIC_BASE_URL` | 主程序的公开 HTTPS 地址，用于订阅授权 |
| `BIND_ADDRESS` | 监听地址，默认 `0.0.0.0:8080` |
| `RUST_LOG` | 日志详细程度 |
| `RELAY_SECRET` / `RELAY_SECRETS` | 消息代理的签名共享密钥 |

随机字符串可以通过以下命令生成：

```bash
openssl rand -base64 32
openssl rand -hex 32
```

第一条输出给 `CONFIG_KEY_BASE64`。第二条可用于 `JWT_SECRET` 和代理密钥，分别生成不同的值。

## 备份及故障处理

- **不能登录**：检查管理员账户是否在第一次启动时创建。再次设置 `ADMIN_PASSWORD` 不会覆盖数据库里已有用户的密码；管理员可通过管理接口更改密码。
- **消息一直未发送**：在管理页面查看消息详情，检查各个平台的状态和最后一次错误。确认服务器或代理是否能够访问提供商。
- **无法绑定 Telegram**：检查机器人 Token、用户名、Webhook 秘钥以及 `PUBLIC_BASE_URL` 的外网访问情况。接收者需要打开新的订阅链接。
- **企业微信发送失败**：确认应用 Secret、AgentID、接收成员 UserID 和企业微信管理后台的应用可见范围。
- **附件发送失败**：确认文件总大小不超过 2 MiB，并查看对应平台是否允许上传该格式和大小的文件。
- **迁移或恢复数据库**：同时备份 PostgreSQL 数据和 `CONFIG_KEY_BASE64`。这两个内容必须一起保存，否则加密的平台设置无法解密。

主程序当前会持续保存消息内容和附件。请根据使用量检查 PostgreSQL 的磁盘使用情况，并制定适当的数据清理和备份计划。

## 数据库任务与后台工作进程

主程序启动后会连接 PostgreSQL 并订阅消息队列通知。新消息提交后，后台工作进程立即收到数据库通知并领取任务。失败任务按照数据库保存的下次尝试时间唤醒。为应对数据库连接短暂中断，工作进程最长每 60 秒执行一次恢复检查。详情见 [消息队列说明](queue.md)。
