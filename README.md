# MessagePusher

MessagePusher 是一个可以自行部署的消息推送服务。你的程序只需要调用一个 HTTP 接口，就能把通知发送到邮件、Telegram、企业微信等平台。每个人可以建立多个发送平台，同一条消息会自动投递到所有已启用的平台。

主程序使用 Rust 编写，使用 PostgreSQL 保存用户、平台设置、消息及发送记录。Docker 和普通服务器都能运行主程序。另有一个 JavaScript 消息代理，可以部署到 Vercel 或 Cloudflare Workers，帮助主程序访问网络受限的平台。

## 功能

- 管理员可以创建用户、停用账户、分配权限及重置密码。站点默认关闭公开注册；管理员可以在“站点设置”中开启或关闭注册，新注册账户自动成为普通用户。
- 每个用户可以在“我的账户”中修改登录密码。管理员可以修改站点名称、简介和公告。
- 管理员在“站点设置”中选择向用户开放哪些 Provider 类型。各用户自行创建、编辑和删除个人渠道，并自行填写 Telegram Bot Token、SMTP 账户、企业微信凭据以及代理设置。管理员只管理平台类型的开放状态，不需要代替用户配置渠道。
- 每位用户可以配置多个同类型或不同类型的平台。所有发送都由后台任务并行执行。
- API 在消息加入 PostgreSQL 队列后立即返回消息编号。工作进程通过 `LISTEN/NOTIFY` 在新任务提交后收到通知，并根据数据库中的重试时间安排后续发送。消息持久化在 PostgreSQL，每个平台的发送结果单独记录，暂时失败时最多尝试五次。
- 支持最多五个附件，附件总大小不超过 2 MiB。邮件附件以普通邮件附件的形式发送；Telegram 和企业微信会显示单独的文件消息。
- 同一代理地址和共享密钥下的多个平台，可以在一次请求中并行发送。代理不需要数据库，也不保存用户和消息。
- 支持 Server酱、PushPlus、Bark、ntfy 和 Gotify 的常见调用方式。第三方程序通常只需要替换调用地址和发送密钥。
- 内置响应式中文管理页面，支持手机与电脑使用。可以在页面中管理用户、注册开关、站点信息、推送平台、API 密钥、发送记录与个人密码。

| 平台 | 主程序直接发送 | 经 Vercel / Cloudflare 代理 | 附件 |
| --- | --- | --- | --- |
| Telegram Bot | 支持 | 支持 | 文件 |
| 企业微信自建应用 | 支持 | 支持 | 文件 |
| 企业微信群机器人 | 支持 | 支持 | 文件 |
| SMTP 邮件 | 支持 | 不支持 | MIME 附件 |
| Resend 邮件 API | 支持 | 支持 | 邮件附件 |

## 用 Docker 安装

需要 Docker 和 Docker Compose。

1. 下载仓库，把 `.env.example` 复制为 `.env`。
2. 执行 `openssl rand -base64 32`，把输出填到 `CONFIG_KEY_BASE64`。
3. 执行 `openssl rand -hex 32`，把输出填到 `JWT_SECRET`。
4. 在 `.env` 中填写 `POSTGRES_PASSWORD`、`ADMIN_PASSWORD`、`PUBLIC_BASE_URL`。管理员密码至少 12 个字符。`PUBLIC_BASE_URL` 是外部访问主程序的 HTTPS 地址。
5. 进入仓库目录，执行：

```bash
docker compose up -d --build
```

随后访问 `http://服务器地址:8080`。用 `ADMIN_USERNAME` 和 `ADMIN_PASSWORD` 登录。第一次启动会自动创建数据库表和管理员账号。登录后在“站点设置”中选择是否允许新用户自行注册。网站默认不开放注册。之后更改 `ADMIN_PASSWORD` 环境变量不会直接更改已有管理员的密码。

如果只在内网使用，可以暂时不填写 `PUBLIC_BASE_URL`。Telegram 的订阅 Webhook 和企业微信的网页授权需要公网 HTTPS 地址。

## 在普通服务器上运行

需要 PostgreSQL 17（或兼容版本）和 Rust 工具链。可以使用 [Releases](https://github.com/HSOS6Miyabi/MessagePusher/releases) 下载与你的服务器系统匹配的可执行文件。

为主程序设置 `DATABASE_URL`、`CONFIG_KEY_BASE64`、`JWT_SECRET`、`ADMIN_USERNAME` 和 `ADMIN_PASSWORD`，然后运行：

```bash
cargo run --release
```

如果下载的是编译完成的二进制文件，直接运行 `./messagepusher` 即可。数据库表结构会在启动时自动更新。建议使用 systemd 管理进程，并使用 Nginx、Caddy 或其他反向代理启用 HTTPS。详见 [部署说明](docs/deploy.md)。

## 添加推送平台

登录管理页面后，在“推送渠道”中创建自己的接收渠道。系统默认开放 SMTP、Telegram、企业微信和 Resend 等 Provider 类型；站点管理员可在“站点设置”中按类型关闭或重新开放。每个人都只会看到自己的渠道和凭据。普通用户可以直接编辑自己的平台配置与接收地址，无需管理员操作。关闭某个 Provider 类型后，已有配置仍保留，该类型的发送会暂停，重新开放后恢复处理待发送消息。

以 Telegram 为例：

```json
{
  "bot_token": "123456:YOUR_BOT_TOKEN",
  "bot_username": "YourNotificationBot",
  "webhook_secret": "REPLACE_WITH_A_RANDOM_SECRET"
}
```

如果已经知道接收者的 `chat_id`，可以直接添加到上面的 JSON 中。首次私聊接收时，先点击“设置 Webhook”，再点击“获取订阅链接”，让接收消息的人在 Telegram 中打开链接并启动机器人。Telegram 要求用户先与机器人建立会话，机器人才能给这位用户发私聊消息。

企业微信自建应用需要企业的 CorpID、应用 Secret 和 AgentID。接收者应在企业微信里打开授权链接，同时管理员需要把该成员纳入应用的可见范围。企业微信群机器人可以直接使用群机器人 Webhook 的 `key`，无需个人授权。

若主程序无法访问 Telegram 或企业微信，添加平台时将发送方式改为 `proxy`，填写代理的 HTTPS 地址和共享密钥。每个经过代理的 Telegram 或企业微信配置，依然是一个独立的推送平台。更多示例见 [平台设置](docs/providers.md)。

## 调用 API

先在管理页面创建发送密钥。标准密钥以 `mpk_` 开头，Server酱格式密钥以 `SCT` 开头。两种格式都可使用标准 JSON API。

### 标准接口

```bash
curl -X POST 'https://push.example.com/v1/send' \
  -H 'Authorization: Bearer mpk_YOUR_KEY' \
  -H 'Content-Type: application/json' \
  -d '{
    "title": "备份完成",
    "body": "数据库备份已完成。",
    "attachments": [
      {
        "filename": "backup-log.txt",
        "content_type": "text/plain",
        "data_base64": "SGVsbG8sIHdvcmxkIQ=="
      }
    ]
  }'
```

成功加入发送队列后返回 HTTP 202，以及一个消息 `id`。无需附件时，可以省略 `attachments`。也可以把文件作为 `file` / `files` 字段，发送 `multipart/form-data` 到 `/v1/send/multipart`。用可选的 `provider_ids` 数组选择本次需要发送到哪些平台。

### Server酱兼容接口

```bash
curl -X POST 'https://push.example.com/SCT_YOUR_SENDKEY.send' \
  -d 'title=监控告警' \
  --data-urlencode 'desp=主机 CPU 使用率过高'
```

也支持 GET：

```text
https://push.example.com/SCT_YOUR_SENDKEY.send?title=Hello&desp=World
```

`title` 和 `text` 都可以作为标题参数。`desp` 和 `content` 都可以作为正文参数。返回 JSON 中 `code=0` 表示消息已经成功加入本服务的发送队列。实际发送结果请在管理页面查看。

其他兼容接口的参数示例见 [API 说明](docs/api.md)。这些地址都使用在 MessagePusher 中创建的发送密钥。

## 部署无状态消息代理

代理代码位于 `relay/`，使用 JavaScript Fetch API。一个代理实例可同时发送多个 Telegram、企业微信和 Resend 邮件请求。

- **Cloudflare Workers**：进入 `relay/`，执行 `npx wrangler deploy`，然后用 `npx wrangler secret put RELAY_SECRET` 设置共享密钥。
- **Vercel**：将此仓库导入 Vercel，项目根目录设为 `relay`，在项目环境变量中填写 `RELAY_SECRET` 并部署。

代理密钥至少 24 个字符。主程序中平台的 `delivery.secret` 必须与代理环境变量一致。代理也接受逗号分隔的 `RELAY_SECRETS`，方便在更换密钥期间同时接受新旧密钥。

代理只接受带有时间戳和 HMAC-SHA256 签名的请求。代理发往外部服务的地址在程序中固定，无法通过 API 指定任意 HTTP 目的地。

Vercel Functions 限制每次请求体最大 4.5 MB。本程序为代理设置了更低的 3.4 MB 限制，因此合并发送时会自动拆成多次请求。文件大小和消息长度的限制可在源代码中查看，部署者可以按实际需要调整。

## 用户、数据与安全

管理员可以管理用户、站点信息和注册开关；普通用户只能使用自己的推送平台和密钥。公开注册默认关闭，开启后每小时最多创建 60 个普通用户账户。管理员或用户修改密码后，之前的登录令牌将失效。发送密钥仅在创建时显示一次，数据库中保存其 SHA-256 摘要。平台凭据使用 AES-256-GCM 加密后存入 PostgreSQL。务必长期保留 `CONFIG_KEY_BASE64`，否则已有平台设置将无法解密。

管理页面登录令牌有效期为 24 小时。请在公网使用 HTTPS，不要将 `.env` 上传到 GitHub，也不要在聊天或日志中公开机器人 Token、邮箱密码及发送密钥。

附件会以 Base64 格式保存在 PostgreSQL 的消息记录中，直到对应消息被删除。请为数据库预留足够的磁盘空间并定期备份。保密程度较高的文件请自行评估是否适合发送到第三方平台。

## 编译与发布

GitHub Actions 在提交到 `main` 时执行 Rust 测试、代理测试及数据库与 API 检查。通过后会构建 Linux x86_64、Linux ARM64、Windows x86_64、macOS Intel 和 macOS Apple Silicon 可执行文件，并上传到滚动更新的 `edge` Release。

推送版本标签（例如 `v0.1.0`）时，会生成相应版本的 Release。另有工作流编译 Linux AMD64/ARM64 Docker 镜像，推送到 GitHub Container Registry。

- [完整使用手册](docs/guide.md)
- [API 接入指南](docs/integration.md)
- [Telegram 使用说明](docs/telegram.md)
- [PostgreSQL 消息队列说明](docs/queue.md)
- [API 接口列表](docs/api.md)
- [各平台配置](docs/providers.md)
- [详细部署说明](docs/deploy.md)
