# Telegram 使用说明

本说明适用于通过 Telegram Bot 向私人会话或群组推送文字与文件的场景。以下示例使用公开机器人用户名 `msgpshbot`，所有 Token 和发送密钥都使用占位符，请填写自己的实际值。

## 一、在 Telegram 创建机器人

打开 [@BotFather](https://t.me/BotFather)，使用 `/newbot` 创建机器人，并保存 BotFather 返回的 Bot Token。机器人用户名通常以 `bot` 结尾。

机器人主页可以设置简介、头像和命令菜单。MessagePusher 发送消息并不依赖这些外观设置。

**保护 Bot Token：** 拥有它的人可以操作你的机器人。请将 Token 填入个人推送渠道的配置，避免公开到代码仓库、截图或文档。如果 Token 曾被公开，应该在 BotFather 中重新生成，并更新 MessagePusher 中的配置。

## 二、确认机器人可以接收私人消息

在 Telegram 中打开机器人主页，例如：

https://t.me/msgpshbot

点击 Start，或发送 `/start`。Telegram 要求用户先与机器人建立会话，机器人才能主动向该用户发送私人消息。

可以通过 Telegram 官方 API 的 `getMe` 检查 Bot Token 是否有效：

```bash
export TELEGRAM_BOT_TOKEN='YOUR_TELEGRAM_BOT_TOKEN'

curl -sS -X POST \
  "https://api.telegram.org/bot$TELEGRAM_BOT_TOKEN/getMe"
```

正常情况下响应中的 `ok` 为 `true`，`result.username` 是你的机器人用户名。

不要将含有 Bot Token 的完整请求地址复制到公共日志或聊天记录中。

## 三、选择接收者绑定方式

### 方式 A：直接填写 Chat ID

如果你已经知道私人会话或群组的 Chat ID，可以直接填写到个人 Telegram 渠道中。这样不需要为这个渠道设置 Webhook。

创建渠道时选择“Telegram 机器人”，填写：

- Bot Token。
- Chat ID。
- 渠道名称，例如“我的私人通知”。

然后点击保存。可以在“发送消息”中选择该渠道发送测试消息。

Chat ID 是 Telegram 会话的数字标识，可以是字符串，也可以是整数。群聊标识可能是负数。

### 方式 B：使用订阅链接自动绑定

适合不希望手动查询 Chat ID 的个人用户。

前提条件：

1. MessagePusher 主程序已经运行。
2. `PUBLIC_BASE_URL` 已设置为主程序公开的 HTTPS 地址。
3. 该 HTTPS 地址可以从互联网访问。
4. 机器人用户名、Bot Token 和 Webhook 密钥已经填写。

建议生成 Webhook 密钥：

```bash
openssl rand -hex 24
```

该密钥支持英文字母、数字、下划线和连字符，长度为 16–256 个字符。

在“推送渠道”保存 Telegram 渠道，依次点击：

1. **设置 Webhook**：MessagePusher 请求 Telegram 把机器人收到的消息转发到主程序。
2. **订阅链接**：MessagePusher 生成一条有效期为 15 分钟的链接。
3. 在 Telegram 中打开该链接，并点击 Start。
4. MessagePusher 验证请求中的 Webhook 密钥和订阅参数，自动保存该接收者的 Chat ID。
5. 回到管理页面刷新渠道列表，确认渠道显示为已绑定。

订阅链接的地址形式类似：

```text
https://t.me/YourBot?start=RANDOM_SUBSCRIPTION_CODE
```

其中 `RANDOM_SUBSCRIPTION_CODE` 由服务器自动生成，无需手动构造。订阅参数仅用于绑定，不是机器人发送密钥。

如果接收者之前已经点击过 Start，仍应使用本次生成的订阅链接重新进入机器人，让机器人收到包含本次订阅参数的 `/start` 消息。

### Webhook 的地址和安全设置

MessagePusher 注册的回调地址为：

```text
https://push.example.com/hooks/telegram/{provider_id}
```

`{provider_id}` 是该用户 Telegram 渠道的 UUID。

Telegram 发送请求时，会包含 `X-Telegram-Bot-Api-Secret-Token` 请求头。主程序校验这个密钥，再根据有效的订阅参数更新渠道。

Telegram 每个机器人同一时间只能设置一个 Webhook。如果在多个渠道中使用同一个 Bot Token，并分别点击“设置 Webhook”，后设置的回调地址会替换先前的地址。需要多个独立的自动订阅入口时，建议使用不同的机器人；已经知道 Chat ID 的渠道可以直接填写该值。

**使用 `getUpdates` 读取机器人消息时，如果机器人已经设置 Webhook，两种方式会冲突。** 完成订阅后，消息会由 Telegram 主动发送给 Webhook，不需要让用户或服务器持续调用 `getUpdates`。

## 四、发送文字消息

首先在 MessagePusher 的“API 密钥”中创建一把发送密钥，然后在“发送消息”中选择 Telegram 渠道。

也可以通过命令行发送：

```bash
export PUSH_BASE_URL='https://push.example.com'
export MESSAGEPUSHER_SEND_KEY='mpk_YOUR_SEND_KEY'

curl -X POST "$PUSH_BASE_URL/v1/send" \
  -H "Authorization: Bearer $MESSAGEPUSHER_SEND_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"title":"服务恢复","body":"网站已恢复正常访问"}'
```

API 返回 HTTP 202 后，可以在“消息记录”查看 Telegram 渠道的发送状态。`sent` 表示 Telegram 的接口已确认接收消息。

Telegram 文本消息存在长度限制。程序会按 Unicode 字符拆分过长的消息并分段发送。

## 五、发送文件附件

MessagePusher 支持每条消息最多五个附件，总大小不超过 2 MiB。Telegram 会通过 `sendDocument` 把附件作为文件发送。

### 使用文件上传接口

```bash
curl -X POST "$PUSH_BASE_URL/v1/send/multipart" \
  -H "Authorization: Bearer $MESSAGEPUSHER_SEND_KEY" \
  -F 'title=运行日志' \
  -F 'body=请查看附件中的日志' \
  -F 'files=@./service.log;type=text/plain'
```

### 使用 JSON 接口

JSON 附件需要将文件编码为 Base64，并提供文件名和 MIME 类型。具体格式见 [API 接入指南](integration.md)。

文件上传后，文字和文件会分别发送。一个消息在发送中途失败并重新尝试时，可能会出现重复文字或文件。对需要严格去重的业务，可以在消息标题或正文中加入业务事件编号，并在接收端进行核对。

## 六、通过消息代理发送 Telegram

如果主程序无法连接 `api.telegram.org`，可以将用户的 Telegram 渠道改为“通过无状态消息代理发送”。

1. 在 Vercel 或 Cloudflare Workers 部署仓库的 `relay` 目录。
2. 在代理环境变量中设置随机的 `RELAY_SECRET`。
3. 在 Telegram 渠道的“发送方式”中选择消息代理。
4. 填入代理的 HTTPS 地址和相同的共享密钥。
5. 保存渠道并发送测试消息。

主程序会用 HMAC-SHA256 对代理请求签名，代理校验签名后调用 Telegram API，并返回每个发送任务的结果。

**消息代理只负责主动调用 Telegram API。** 订阅时，Telegram 发送给 MessagePusher 的 Webhook 仍然需要能从互联网访问主程序的公开 HTTPS 地址。

## 七、检查机器人和 Webhook

检查机器人身份：

```bash
curl -sS -X POST \
  "https://api.telegram.org/bot$TELEGRAM_BOT_TOKEN/getMe"
```

检查 Webhook：

```bash
curl -sS -X POST \
  "https://api.telegram.org/bot$TELEGRAM_BOT_TOKEN/getWebhookInfo"
```

正常配置时，返回的 `result.url` 应当指向 MessagePusher 的 `/hooks/telegram/{provider_id}`。

如果你采用直接填写 Chat ID 的方式，没有配置 Webhook 也可以正常发送消息。

## 八、常见问题

### 点击 Start 后仍显示未绑定

确认点击的是刚生成的订阅链接，且链接未超过 15 分钟。检查 `PUBLIC_BASE_URL` 的 HTTPS 是否可以从外部访问，以及管理页面中的“设置 Webhook”是否返回成功。

如果机器人已经收到消息，但 Webhook 仍未生效，查看 `getWebhookInfo` 返回的 `last_error_message`。确认反向代理允许 Telegram POST 到 `/hooks/telegram/{provider_id}`。

### Telegram 返回 401

通常是 Bot Token 无效、已被重新生成，或错误地使用了其他机器人的 Token。重新核对 BotFather 中最新的 Token。

### Telegram 返回 403

确认接收者已与机器人建立会话，并未屏蔽该机器人。向群组发送时，检查机器人是否已加入群组并具有发送权限。

### Telegram 返回 429

表示 Telegram 对请求进行调用次数限制。请降低发送频率，检查是否同时有其他程序使用相同 Bot Token，并参考 Telegram API 的等待时间信息。

### 代理服务健康检查正常，但不能发送

`/healthz` 只用于检查代理程序是否启动。仍需核对代理共享密钥、Telegram Bot Token、接收者 Chat ID，以及代理能否连接 Telegram 官方 API。

### 发送记录显示 `pending`

说明任务仍在等待发送或等待下一次重试。MessagePusher 保存了下次尝试时间，后台工作进程会按照数据库记录唤醒并继续处理。

## 九、接入自己的应用

通过 [API 接入指南](integration.md) 可以了解 JSON、multipart/form-data、Server酱兼容接口、发送密钥、指定渠道和发送状态查询的完整格式。
