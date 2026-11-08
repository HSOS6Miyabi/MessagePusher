# MessagePusher API 接入指南

本指南面向需要从网站、定时任务、监控程序、服务器脚本或其他业务程序发送通知的开发者。所有示例均使用虚构的域名和密钥，请替换为实际值。

## 1. 接入前的准备工作

要发送通知，需要先完成以下操作：

1. 启动 MessagePusher 主程序和 PostgreSQL，确认 `GET /healthz` 返回 `{"ok":true}`。
2. 使用管理员创建普通用户，或者由用户自行注册。
3. 用户登录管理页面，在“推送渠道”中添加至少一个已启用的渠道。
4. 用户在“API 密钥”中创建发送密钥。
5. 应用程序使用该发送密钥调用发送接口。

同一个用户可以配置多个 SMTP、Telegram、企业微信或 Resend 渠道。应用程序通常只需要保存 MessagePusher 的发送密钥，不需要保存 Telegram Bot Token 或邮箱密码。

### 接口地址

假设 MessagePusher 公开地址是：

```text
https://push.example.com
```

建议设置环境变量：

```bash
export MESSAGEPUSHER_URL='https://push.example.com'
export MESSAGEPUSHER_KEY='mpk_YOUR_SEND_KEY'
```

示例中包含的所有 `YOUR_*` 均为需要自行填写的占位符。

## 2. 认证方式

MessagePusher 使用两类凭据：

| 凭据 | 获取方式 | 允许的操作 |
| --- | --- | --- |
| 管理登录令牌 | 调用 `POST /v1/login` | 管理账户、自己的渠道和发送密钥 |
| 发送密钥 | 用户登录后调用 `POST /v1/keys` 或从页面创建 | 提交消息发送请求 |

两个凭据都通过请求头传递：

```http
Authorization: Bearer YOUR_TOKEN
```

不要将管理令牌用于接入普通的监控脚本。发送密钥只显示一次，可以为每个应用创建独立的密钥，方便单独撤销。

### 管理员和普通用户

普通用户只能创建和修改自己的渠道。管理员可以开放或暂停 Provider 类型、管理用户和站点设置，但不会自动获得查看其他用户平台凭据的权限。

## 3. 标准 JSON 发送接口

### 3.1 请求

`POST /v1/send`

请求头：

```http
Authorization: Bearer mpk_YOUR_SEND_KEY
Content-Type: application/json
```

请求体：

```json
{
  "title": "数据库备份完成",
  "body": "数据库备份文件已经生成。",
  "attachments": []
}
```

请求字段说明：

| 字段 | 类型 | 是否必须 | 说明 |
| --- | --- | --- | --- |
| `title` | 字符串 | `title` 和 `body` 至少有一项非空 | 标题，最多 250 个 Unicode 字符 |
| `body` | 字符串 | 同上 | 消息正文，最多 100,000 字节 |
| `provider_ids` | UUID 字符串数组 | 否 | 指定当前用户已启用的渠道，省略则使用所有可用渠道 |
| `attachments` | 数组 | 否 | 附件对象，最多 5 项，文件总大小不超过 2 MiB |

例子：

```bash
curl -X POST "$MESSAGEPUSHER_URL/v1/send" \
  -H "Authorization: Bearer $MESSAGEPUSHER_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"title":"服务器恢复","body":"监控系统检测到服务已恢复正常。"}'
```

### 3.2 成功响应

HTTP 202：

```json
{
  "id": "20dd30d6-bf71-40eb-a866-d82254f98111",
  "status": "queued",
  "status_url": "/v1/events/20dd30d6-bf71-40eb-a866-d82254f98111"
}
```

返回 `queued` 表示消息已写入 PostgreSQL 的持久化任务队列。外部平台的实际发送会在后台完成。请在需要时查询各渠道的发送结果。

### 3.3 指定部分推送渠道

从 `GET /v1/providers` 获取自己配置的渠道 ID，之后可以指定 `provider_ids`：

```json
{
  "title": "网站部署完成",
  "body": "新版本已经可以访问。",
  "provider_ids": [
    "c306178b-d5cf-422f-92a0-b1ca57519bb1",
    "83b9fb9e-5c75-4eab-a3d0-232a96296b35"
  ]
}
```

仅允许指定当前用户已启用、网站管理员已开放的 Provider 类型。若数组为空或者包含不可用的渠道，接口会返回 HTTP 400。

### 3.4 防止重复创建消息

可以使用请求头 `Idempotency-Key`：

```bash
curl -X POST "$MESSAGEPUSHER_URL/v1/send" \
  -H "Authorization: Bearer $MESSAGEPUSHER_KEY" \
  -H 'Idempotency-Key: backup-2026-11-08-001' \
  -H 'Content-Type: application/json' \
  -d '{"title":"备份完成","body":"文件已生成"}'
```

同一用户重复提交相同的 `Idempotency-Key`（长度 1–128 字节），服务器返回第一次创建的消息 ID。不同用户的密钥互不影响。

请为不同业务事件生成不同的值。同一幂等键重复请求时，系统不会重新读取新的正文或目标渠道。

注意：幂等键防止重复入队。若某个 Provider 已接受消息，但程序在保存发送结果之前意外退出，之后重试时仍可能出现重复发送。这是采用持久化重试时需要考虑的情况。

## 4. 附件发送

### 4.1 JSON 中的 Base64 附件

`attachments` 是对象数组。每个对象必须提供：

| 字段 | 说明 |
| --- | --- |
| `filename` | 文件名，不能包含路径分隔符，最长 120 字节 |
| `content_type` | 文件 MIME 类型，例如 `text/plain` |
| `data_base64` | 文件内容的标准 Base64 编码 |

请求示例：

```json
{
  "title": "运行日志",
  "body": "日志文件已附上。",
  "attachments": [
    {
      "filename": "status.txt",
      "content_type": "text/plain",
      "data_base64": "SGVsbG8="
    }
  ]
}
```

`data_base64` 示例表示内容 `Hello` 的文本文件。请不要把本地文件路径填写为 `data_base64`。

### 4.2 直接上传文件

`POST /v1/send/multipart` 接受 `multipart/form-data`：

```bash
curl -X POST "$MESSAGEPUSHER_URL/v1/send/multipart" \
  -H "Authorization: Bearer $MESSAGEPUSHER_KEY" \
  -F 'title=程序执行报告' \
  -F 'body=请查看报告附件' \
  -F 'files=@./report.txt;type=text/plain'
```

还可以重复使用 `files` 或 `file` 字段上传多个附件。`provider_ids` 可以作为一个包含 UUID 数组的 JSON 字符串表单字段传入。

附件最多五个，总大小不超过 2 MiB。由于 Base64 编码和表单封装需要额外空间，完整 HTTP 请求还有额外的大小限制。代理请求同样需要符合消息代理平台的限制。

### 4.3 各平台的附件表现

- SMTP、Resend：文件作为邮件附件发送。
- Telegram：通过 `sendDocument` 发送，文字和文件可能分别显示。
- 企业微信：先上传临时素材，再发送文件消息。

一个消息包含多个文件时，程序可能需要多次向 Provider 发送请求。如果中途失败后重试，已发送的部分内容可能再次出现。

## 5. 查询消息发送结果

管理员或普通用户使用自己的**管理登录令牌**查询消息，发送密钥不能调用管理接口。

登录：

```bash
curl -X POST "$MESSAGEPUSHER_URL/v1/login" \
  -H 'Content-Type: application/json' \
  -d '{"username":"myuser","password":"YOUR_LOGIN_PASSWORD"}'
```

响应包含 `token` 和 `expires_in_seconds`。令牌默认有效期为 24 小时。

查看最近的消息：

```http
GET /v1/events
Authorization: Bearer MANAGEMENT_TOKEN
```

查看指定消息：

```http
GET /v1/events/{id}
Authorization: Bearer MANAGEMENT_TOKEN
```

消息详情返回标题、正文、附件名称，以及各渠道的 `status`、`attempts`、`last_error` 和 `sent_at` 等信息。

| 状态 | 含义 |
| --- | --- |
| `pending` | 等待首次发送或下次重试 |
| `working` | 后台工作进程正在调用 Provider |
| `sent` | Provider API 确认接收 |
| `failed` | 多次尝试后仍发送失败 |

如果希望程序能够在发送完成后采取后续操作，可以在自己的程序中查询消息状态。MessagePusher 当前没有提供推送到调用方的发送状态回调。

## 6. 常见服务兼容接口

下列接口兼容常用的消息提交参数与部分响应字段。接入时使用 **MessagePusher 创建的发送密钥**。

| 服务风格 | MessagePusher 地址 | 参数 |
| --- | --- | --- |
| Server酱 | `GET/POST /{SEND_KEY}.send` | `title` 或 `text`；`desp` 或 `content` |
| PushPlus | `GET/POST /send` | `token`、`title`、`content` |
| Gotify | `POST /message?token={SEND_KEY}` | `title`、`message` |
| ntfy | `POST /ntfy/{SEND_KEY}` | 请求体是正文，可使用 `Title` 请求头 |
| Bark | `GET /bark/{SEND_KEY}/{TITLE}/{BODY}` | 标题和正文在 URL 路径中 |

### 6.1 Server酱

推荐为这个用途创建 SCT 格式密钥：

```bash
curl -G "$MESSAGEPUSHER_URL/SCT_YOUR_SEND_KEY.send" \
  --data-urlencode 'title=监控告警' \
  --data-urlencode 'desp=主机磁盘空间不足'
```

POST 表单方式：

```bash
curl -X POST "$MESSAGEPUSHER_URL/SCT_YOUR_SEND_KEY.send" \
  --data-urlencode 'title=任务已完成' \
  --data-urlencode 'desp=执行成功'
```

响应示例：

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "pushid": "20dd30d6-bf71-40eb-a866-d82254f98111",
    "queued": true
  }
}
```

`code=0` 代表消息加入 MessagePusher 的发送队列。实际发送结果需通过管理页面查看。Server酱的其他高级参数，例如频道控制、通知类型等，并未全部实现。

### 6.2 PushPlus

```bash
curl -X POST "$MESSAGEPUSHER_URL/send" \
  -H 'Content-Type: application/json' \
  -d '{"token":"mpk_YOUR_SEND_KEY","title":"服务通知","content":"服务已恢复"}'
```

### 6.3 Gotify

```bash
curl -X POST "$MESSAGEPUSHER_URL/message?token=mpk_YOUR_SEND_KEY" \
  -d 'title=备份完成' \
  -d 'message=备份文件已保存'
```

### 6.4 ntfy

```bash
curl -X POST "$MESSAGEPUSHER_URL/ntfy/mpk_YOUR_SEND_KEY" \
  -H 'Title: 监控通知' \
  --data '网站服务已恢复'
```

### 6.5 Bark

```text
GET /bark/{SEND_KEY}/{TITLE}/{BODY}
```

使用时需要对标题、正文中的非 ASCII 字符和特殊字符进行 URL 编码。

兼容接口针对消息提交场景设计，部分原服务的订阅管理、音效、优先级或群组功能不在当前实现范围内。

## 7. 用户和平台管理 API

这些接口使用管理令牌。普通用户只能管理自己的账户信息、发送密钥和推送渠道。

| 方法与地址 | 用途 |
| --- | --- |
| `GET /v1/site` | 获取网站名称、简介、公告和注册状态 |
| `POST /v1/register` | 注册普通用户，需要管理员开放注册 |
| `POST /v1/login` | 登录 |
| `GET /v1/me` | 查看当前账户 |
| `POST /v1/me/password` | 修改自己的密码 |
| `GET /v1/provider-catalog` | 查看当前开放的 Provider 类型 |
| `GET /v1/providers` | 列出自己的推送渠道 |
| `POST /v1/providers` | 创建推送渠道 |
| `GET /v1/providers/{id}` | 获取渠道可编辑的非敏感配置 |
| `PATCH /v1/providers/{id}` | 修改部分配置，其他字段保留 |
| `PUT /v1/providers/{id}` | 完整替换渠道配置 |
| `DELETE /v1/providers/{id}` | 删除自己的渠道 |
| `GET /v1/keys` | 查看自己的密钥名称和前缀 |
| `POST /v1/keys` | 创建发送密钥 |
| `DELETE /v1/keys/{id}` | 撤销发送密钥 |

管理员专属接口：

| 方法与地址 | 用途 |
| --- | --- |
| `GET /v1/users` | 列出用户 |
| `POST /v1/users` | 创建用户 |
| `PATCH /v1/users/{id}` | 修改用户角色、状态或密码 |
| `DELETE /v1/users/{id}` | 删除用户 |
| `PATCH /v1/admin/site` | 修改站点信息和注册开关 |
| `GET /v1/admin/provider-catalog` | 查看全部 Provider 类型 |
| `PATCH /v1/admin/provider-catalog/{kind}` | 开放或关闭指定 Provider 类型 |
| `GET /v1/admin/overview` | 查看站点统计信息 |

用户创建渠道时，需指定 `name`、`kind`、`settings` 和 `delivery`。各 Provider 的字段见 [平台配置说明](providers.md)。

管理员关闭一个 Provider 类型时，用户已有配置保留，新的发送任务不会使用该类型，等待中的任务暂停。

## 8. 通过消息代理发送

消息代理没有数据库，也不提供普通用户登录。主程序把已经选定的目标渠道、所需凭据、消息内容和附件发送给代理，再由代理调用外部 Provider。

代理请求通过时间戳与 HMAC-SHA256 签名验证，包含以下请求头：

```http
X-MP-Timestamp: UNIX_SECONDS
X-MP-Signature: HMAC_SHA256_HEX
Content-Type: application/json
```

一般应用开发者无需直接调用代理。只需要在管理页面为自己的渠道配置代理 HTTPS 地址和 `RELAY_SECRET`，仍然向主程序的 `/v1/send` 提交消息。

代理部署操作见 [使用手册](guide.md#5-通过消息代理发送)。

## 9. 接入建议

- 将每个业务系统的发送密钥存放在环境变量或秘密管理服务中。
- 调用 HTTP API 时设置合理的连接和请求超时。
- 对可能重试的业务事件传入稳定的 `Idempotency-Key`。
- 接收到 HTTP 202 后保存返回的消息 ID，需要确认发送结果时查询该 ID。
- 只有在目标 Provider 确认接收后，消息状态才会变为 `sent`。
- 不要把 Telegram Bot Token、SMTP 密码或代理密钥直接放进业务程序的前端代码。
- 减少含敏感个人信息的正文和附件，按需要制定数据库备份和消息保留方案。

## 10. 进一步阅读

- [使用手册](guide.md)：从安装到使用管理页面。
- [Telegram 使用说明](telegram.md)：机器人、Webhook、订阅绑定和文件发送。
- [数据库消息队列说明](queue.md)：并行发送、失败重试、通知唤醒和故障恢复。
- [平台配置说明](providers.md)：各 Provider 的配置字段。
