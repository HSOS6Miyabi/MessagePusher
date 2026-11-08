# MessagePusher 使用手册

本手册面向管理员和普通用户，介绍从首次启动到发送通知、查看结果的完整操作。开发者接入已有系统时，请阅读 [API 接入指南](integration.md)。Telegram 的订阅授权和附件发送请参照 [Telegram 使用说明](telegram.md)。

## 1. 系统组成

MessagePusher 接收应用程序发送的事件通知，然后向用户配置的一个或多个推送渠道发送消息。

| 组件 | 作用 | 是否必须 |
| --- | --- | --- |
| Rust 主程序 | 提供管理页面、发送 API、消息记录和后台发送任务 | 必须 |
| PostgreSQL | 保存用户、渠道、消息、附件及发送状态 | 必须 |
| 无状态消息代理 | 在主程序无法访问某个平台时，代替主程序发送消息 | 按需使用 |

主程序可以运行在普通 Linux 服务器或 Docker 容器中。消息代理可运行在 Vercel、Cloudflare Workers 等支持 Fetch API 的环境中。

系统内置的 Provider 类型包括：

| Provider 类型 | 发送目标 | 支持附件 | 支持消息代理 |
| --- | --- | --- | --- |
| `telegram` | Telegram 私聊或群聊 | 文件 | 支持 |
| `wecom_app` | 企业微信自建应用中的成员 | 文件 | 支持 |
| `wecom_webhook` | 企业微信群机器人 | 文件 | 支持 |
| `email_smtp` | 普通 SMTP 邮件 | MIME 邮件附件 | 主程序直接发送 |
| `email_resend` | Resend 邮件 API | 邮件附件 | 支持 |

### 管理员和普通用户的权限

管理员可以开放或关闭网站注册、管理账户、修改站点名称和公告，以及决定用户可以使用哪些 Provider 类型。

每个用户独立管理自己的推送渠道和发送密钥，包括配置 Telegram Bot Token、SMTP 密码、收件人和消息代理。管理员对其他用户的渠道不具有自动查看或修改权限。管理员自身也可以创建渠道。

## 2. 使用 Docker Compose 安装

### 2.1 准备环境

在服务器上安装 Docker、Docker Compose 和 Git，然后执行：

```bash
git clone https://github.com/HSOS6Miyabi/MessagePusher.git
cd MessagePusher
cp .env.example .env
```

编辑 `.env`，填写数据库密码、管理员账户以及随机密钥：

| 环境变量 | 说明 |
| --- | --- |
| `POSTGRES_PASSWORD` | PostgreSQL 数据库密码 |
| `ADMIN_USERNAME` | 首次启动时创建的管理员用户名 |
| `ADMIN_PASSWORD` | 首次启动时创建的管理员密码，长度至少 12 个字符 |
| `CONFIG_KEY_BASE64` | 加密推送渠道凭据的密钥，需要长期备份 |
| `JWT_SECRET` | 签发管理页面登录令牌的密钥 |
| `PUBLIC_BASE_URL` | 用户访问主程序的公开 HTTPS 地址 |
| `HTTP_PORT` | Docker Compose 对外开放的端口，默认 8080 |

生成随机值：

```bash
openssl rand -base64 32
openssl rand -hex 32
```

将第一条命令的结果填入 `CONFIG_KEY_BASE64`，另行生成随机值填入 `JWT_SECRET`。不要重复使用密钥。

如暂时只测试直接发送消息，可以先通过服务器地址访问管理页面。启用 Telegram Webhook 和企业微信网页授权前，需要配置公开 HTTPS 地址。

### 2.2 启动并检查

```bash
docker compose up -d --build
docker compose ps
curl http://127.0.0.1:8080/healthz
```

正常返回：

```json
{"ok":true}
```

浏览器打开 `http://服务器地址:8080`，使用管理员用户名和密码登录。数据库表会在启动时自动创建或更新。

首次启动后，更改环境变量 `ADMIN_PASSWORD` 不会自动覆盖数据库内的管理员密码。可以在管理页面通过“我的账户”修改密码。

### 2.3 配置 HTTPS

正式部署时，建议使用 Nginx、Caddy 或其他反向代理，将公开 HTTPS 请求转发到主程序监听端口。

例如公开地址为 `https://push.example.com`，则：

```dotenv
PUBLIC_BASE_URL=https://push.example.com
```

重启相关容器后，在另一台设备上访问 `https://push.example.com/healthz`，确认能够访问。Telegram 和企业微信的订阅回调需要使用这个地址。

更多关于普通服务器安装、更新、备份及排查问题的方法，请阅读 [部署与维护](deploy.md)。

## 3. 登录、用户注册和账户管理

### 3.1 首位管理员

首次启动时由 `ADMIN_USERNAME` 和 `ADMIN_PASSWORD` 创建管理员。管理员可以在左侧“用户管理”页面：

- 创建普通用户或其他管理员。
- 停用或启用账户。
- 重置用户密码、调整账户角色。
- 删除不再需要的用户及其数据。

当用户被停用时，将无法再登录，也不能继续使用已有发送密钥。删除账户前请先备份需要保留的消息记录。

### 3.2 开放用户自行注册

管理员打开“站点设置”，找到“允许用户自行注册”。

公开注册默认关闭。开启后，登录页面会出现“创建账户”；新用户自行填写用户名和密码，新账户自动获得普通用户权限。

用户名可以包含英文字母、数字、下划线及连字符，长度为 3 至 60 个字符；密码长度为 12 至 256 个字符。系统限制每小时最多创建 60 个普通用户账户。

关闭公开注册不会影响已有用户登录，也不会删除他们的推送渠道。

### 3.3 站点信息

管理员可以在“站点设置”中修改网站名称、简介和公告。名称和简介显示在登录界面；公告也会显示在管理页面。

### 3.4 开放 Provider 类型

管理员在“站点设置”中决定是否向用户开放 Telegram、SMTP、Resend 和企业微信等 Provider 类型。内置类型默认开放。

关闭某种类型后，现有用户的相关渠道配置保留。新的消息不会通过该类型发送，等待发送的任务暂时停止；再次开放后，等待中的任务会继续处理。已经开始的外部发送请求可能完成。

### 3.5 修改自己的密码

所有用户都可以进入“我的账户”修改登录密码。需要输入当前密码和新密码。修改成功后，旧登录令牌失效，需要重新登录。

## 4. 创建个人推送渠道

每位用户登录后进入“推送渠道”，点击“添加渠道”，选择网站已开放的 Provider 类型，填写相应的连接信息和接收者地址。

可以添加多个同类型渠道，例如两个 SMTP 邮箱、多个 Telegram 会话或不同企业微信群。创建后可单独启用、暂停、编辑或删除。

### 4.1 Telegram 机器人

在 Telegram 的 `@BotFather` 创建机器人，获取 Bot Token 和机器人用户名。填写：

- Bot Token：用于调用 Telegram API。
- 机器人用户名：例如 `msgpshbot`。
- Webhook 密钥：建议通过 `openssl rand -hex 24` 生成。
- Chat ID：可以填写已知的接收者会话 ID，或者后续通过订阅链接自动绑定。

如果使用订阅链接：先确认 `PUBLIC_BASE_URL` 是公网 HTTPS 地址，再点击“设置 Webhook”“订阅链接”，让接收者打开链接并点击 Start。

完整的订阅授权、群组接入和实际发送步骤见 [Telegram 使用说明](telegram.md)。

### 4.2 SMTP 邮件

选择“SMTP 电子邮件”，填写：

| 配置字段 | 示例 | 说明 |
| --- | --- | --- |
| SMTP 服务器 | `smtp.example.com` | 由邮箱服务提供商提供 |
| 端口 | `465` 或 `587` | 取决于服务商 |
| 加密方式 | `implicit` 或 `starttls` | 465 通常使用隐式 TLS，587 通常使用 STARTTLS |
| 用户名 | `notify@example.com` | SMTP 登录账户 |
| 密码或授权码 | 由服务商提供 | 一般建议使用专用授权码 |
| 发件邮箱 | `notify@example.com` | 邮件的发件地址 |
| 收件邮箱 | `one@example.com` | 可填写多个地址，用逗号分隔 |

SMTP 在 Rust 主程序中直接发送，支持 MIME 附件。若服务器禁止连接 SMTP 端口，可考虑使用支持 HTTPS 的 Resend。

### 4.3 Resend 邮件 API

选择“Resend 邮件 API”，填写 API Key、已验证的发件地址和收件地址。该类型可以直接通过主程序发送，也可以通过消息代理发送。

### 4.4 企业微信自建应用

选择“企业微信自建应用”，填写 CorpID、应用 Secret 和 AgentID。需要先在企业微信管理后台创建应用，设置成员可见范围，并按企业微信的要求配置网页授权可信域名。

用户可以使用“授权链接”完成企业成员 UserID 的绑定，也可以直接填写已知的 UserID。

### 4.5 企业微信群机器人

在目标企业微信群中添加机器人，从 Webhook 地址中提取 `key=` 后面的内容，填写到 MessagePusher 的群机器人配置中。消息会发送到该机器人所在群聊。

### 4.6 编辑或删除渠道

在“推送渠道”页面点击“编辑”，修改接收地址、平台配置和发送方式。已保存的密码与 Token 不会以明文返回，敏感字段留空可保留原值。

“暂停”会阻止这个渠道继续接收新消息。“删除”会移除渠道配置，历史消息仍可显示曾使用的渠道名称。

## 5. 通过消息代理发送

部分服务器无法直接访问 Telegram 或企业微信。在 Vercel 或 Cloudflare Workers 部署消息代理后，可为某个个人渠道选择“通过无状态消息代理发送”。

### 5.1 Cloudflare Workers

在仓库的 `relay` 目录执行：

```bash
npx wrangler login
npx wrangler secret put RELAY_SECRET
npx wrangler deploy
```

`RELAY_SECRET` 应为至少 24 个字符的随机字符串。将部署得到的 HTTPS 地址和密钥填写到需要使用代理的个人推送渠道中。

### 5.2 Vercel

在 Vercel 导入仓库，将项目根目录设为 `relay`。设置环境变量 `RELAY_SECRET` 后部署。访问代理的 `/healthz` 路径，应返回 `{"ok":true,"service":"messagepusher-relay"}`。

消息代理不保存账户或发送记录，主程序负责保存消息并对发送失败的任务进行重试。同一代理地址和密钥下的多个渠道可以合并为一次请求，代理并行向不同 Provider 发送。

## 6. 创建发送密钥

用户进入“API 密钥”，点击“创建密钥”。可以选择：

- 标准格式：以 `mpk_` 开头，适合直接调用 JSON API。
- Server酱兼容格式：以 `SCT` 开头，适合需要兼容 Server酱的程序。

两种密钥都能调用 MessagePusher 的标准发送接口。完整密钥只会在创建时显示一次。建议为每个应用创建独立的发送密钥，便于撤销。

## 7. 发送一条通知

### 7.1 从管理页面发送

在“发送消息”页面填写发送密钥、标题、正文和附件。可以向所有已启用渠道发送，也可以指定部分渠道。

附件最多五个，总大小不超过 2 MiB。Telegram 将附件作为文件发送，SMTP 和 Resend 会以邮件附件形式发送。

### 7.2 从命令行发送

```bash
curl -X POST 'https://push.example.com/v1/send' \
  -H 'Authorization: Bearer mpk_YOUR_SEND_KEY' \
  -H 'Content-Type: application/json' \
  -d '{"title":"备份完成","body":"数据库备份已生成"}'
```

成功提交会返回 HTTP 202，以及消息编号和 `queued` 状态。这个状态表示消息已写入数据库并等待发送。实际发送结果可在管理页面查看。

需要从其他程序接入、发送附件或指定渠道，请阅读 [API 接入指南](integration.md)。

## 8. 查看发送结果

在“消息记录”中可以看到最近的消息，点击“查看详情”可以查看每个渠道的发送状态：

| 状态 | 含义 |
| --- | --- |
| `pending` | 等待发送或等待下次重试 |
| `working` | 后台任务正在发送 |
| `sent` | Provider 接口确认接受该消息 |
| `failed` | 多次尝试后仍然失败 |

一个渠道的发送失败不会阻止其他渠道发送。程序对临时失败的任务进行延迟重试，最多尝试五次。

## 9. 数据保存及注意事项

用户、平台配置、消息正文、附件和发送状态保存在 PostgreSQL。平台凭据使用 AES-256-GCM 加密，发送密钥在数据库中以摘要形式保存。

请定期备份数据库及 `CONFIG_KEY_BASE64`。附件以 Base64 数据形式随消息保存在 PostgreSQL 中，数据量会随着历史消息增加。处理含有个人信息或保密内容的附件时，请遵守相关数据保护要求。

MessagePusher 的发送接口成功返回，表示消息已进入持久化队列。外部 Provider 可能因为网络错误、接收者拒绝或平台限额而延迟或拒绝接收消息；请通过消息详情确认最终结果。

## 10. 其他资料

- [API 接入指南](integration.md)：认证方式、请求格式、附件、兼容接口和调用示例。
- [Telegram 使用说明](telegram.md)：创建 Bot、订阅绑定、Webhook、代理发送和问题排查。
- [数据库消息队列说明](queue.md)：通知唤醒、并发发送、失败重试和重启恢复。
- [部署与维护](deploy.md)：普通服务器部署、升级、备份和环境变量。
- [Provider 配置示例](providers.md)：各类型平台的设置字段。
