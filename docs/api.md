# HTTP API 使用说明

本说明中的示例地址是 `https://push.example.com`。实际使用时改成自己的主程序地址。

## 登录与管理

第一次启动使用环境变量 `ADMIN_USERNAME` 和 `ADMIN_PASSWORD` 创建管理员。管理员和普通用户都通过 `POST /v1/login` 获取有效期为 24 小时的管理令牌。

```bash
curl -X POST https://push.example.com/v1/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"YOUR_PASSWORD"}'
```

响应示例为 `{"token":"...","expires_in_seconds":86400}`。后续管理接口使用请求头 `Authorization: Bearer 管理令牌`。

| 方法和路径 | 权限 | 用途 |
| --- | --- | --- |
| `GET /v1/site` | 所有人 | 查看站点名称、简介、公告和是否开放注册 |
| `POST /v1/register` | 所有人，需开启注册 | 注册普通用户，字段为 `username`、`password` |
| `PATCH /v1/admin/site` | 管理员 | 设置站点名称、简介、公告及注册开关 |
| `GET /v1/admin/overview` | 管理员 | 查看站点的用户、渠道和消息总量 |
| `GET /v1/me` | 登录用户 | 查看自己的账户 |
| `POST /v1/me/password` | 登录用户 | 通过原密码修改自己的登录密码 |
| `GET /v1/users` | 管理员 | 列出所有用户 |
| `POST /v1/users` | 管理员 | 创建用户，字段为 `username`、`password`、`role` |
| `PATCH /v1/users/{id}` | 管理员 | 修改 `role`、`active` 或 `password` |
| `DELETE /v1/users/{id}` | 管理员 | 删除用户及其数据 |
| `GET /v1/provider-catalog` | 所有人 | 查看当前开放的 Provider 类型 |
| `GET /v1/admin/provider-catalog` | 管理员 | 查看所有内置 Provider 类型及启用状态 |
| `PATCH /v1/admin/provider-catalog/{kind}` | 管理员 | 开放或暂停指定 Provider 类型，参数为 `{"enabled":true}` 或 `{"enabled":false}` |
| `GET /v1/providers` | 登录用户 | 查看自己创建的平台及授权状态 |
| `POST /v1/providers` | 登录用户 | 添加平台 |
| `GET /v1/providers/{id}` | 登录用户 | 查看自己的平台，敏感配置只返回是否已经保存 |
| `PUT /v1/providers/{id}` | 登录用户 | 完整更新平台 |
| `PATCH /v1/providers/{id}` | 登录用户 | 修改平台名称、启用状态或部分配置，未提交的密码保持不变 |
| `DELETE /v1/providers/{id}` | 登录用户 | 删除平台 |
| `GET /v1/keys` | 登录用户 | 列出密钥名称和前缀 |
| `POST /v1/keys` | 登录用户 | 创建发送密钥 |
| `DELETE /v1/keys/{id}` | 登录用户 | 撤销发送密钥 |
| `GET /v1/events` | 登录用户 | 查看最近 50 条消息 |
| `GET /v1/events/{id}` | 登录用户 | 查看各平台的发送状态 |

`POST /v1/keys` 请求体为 `{"name":"服务器监控","format":"standard"}` 或 `{"name":"旧系统通知","format":"serverchan"}`。响应会提供完整密钥一次，之后只能查看开头部分。

### 公开注册与站点设置

公开注册默认关闭。管理员登录后可调用：

```bash
curl -X PATCH https://push.example.com/v1/admin/site \
  -H 'Authorization: Bearer 管理令牌' \
  -H 'Content-Type: application/json' \
  -d '{"site_name":"我的通知中心","site_description":"服务器和网站消息","announcement":"","allow_registration":true}'
```

启用注册后，访问者可以通过管理页面注册，或者使用：

```bash
curl -X POST https://push.example.com/v1/register \
  -H 'Content-Type: application/json' \
  -d '{"username":"newuser","password":"A_STRONG_PASSWORD_123"}'
```

注册成功返回 HTTP 201，包含用户 ID、普通用户权限和登录令牌。用户名只能使用 3–60 个英文字母、数字、下划线或连字符，密码长度为 12–256 个字符。关闭注册后，新注册请求会被拒绝。开放注册时，每小时最多创建 60 个普通用户。

用户修改密码：

```bash
curl -X POST https://push.example.com/v1/me/password \
  -H 'Authorization: Bearer 管理令牌' \
  -H 'Content-Type: application/json' \
  -d '{"current_password":"CURRENT_PASSWORD","new_password":"NEW_STRONG_PASSWORD_123"}'
```

修改成功后需要重新登录。管理员也可以通过 `PATCH /v1/users/{id}` 修改其他用户的密码、角色或启用状态。更改账户信息后，用户需要重新登录。

### Provider 类型和个人渠道

管理员控制系统向用户开放哪些 Provider 类型。调用 `GET /v1/provider-catalog` 即可查看当前可选的类型，例如 `email_smtp`、`telegram` 等。所有内置类型默认开放。

管理员可暂停一个类型，例如：

```bash
curl -X PATCH https://push.example.com/v1/admin/provider-catalog/telegram \
  -H 'Authorization: Bearer 管理令牌' \
  -H 'Content-Type: application/json' \
  -d '{"enabled":false}'
```

关闭后，用户仍保有自己已经配置的 Telegram 渠道，但无法新建该类型的渠道，新的发送任务不会再使用该类型，等待中的任务会暂停。重新开放后，等待中的任务自动继续处理。已经开始发送的请求可能完成。

用户自行创建和编辑个人渠道，平台密码和收件人均由用户自己填写。用户只能查询、修改、删除自己创建的渠道，其他用户和管理员的账户身份不会自动获得查看这些渠道的权限。只有 Provider 类型的开放状态由管理员管理。

### 添加平台的格式

```json
{
  "name": "个人 Telegram",
  "kind": "telegram",
  "enabled": true,
  "settings": {
    "bot_token": "123456:BOT_TOKEN",
    "chat_id": "123456789"
  },
  "delivery": { "mode": "direct" }
}
```

改为代理发送时，将 `delivery` 改为：

```json
{
  "mode": "proxy",
  "url": "https://your-relay.example.com/",
  "secret": "SAME_SECRET_AS_RELAY_SECRET"
}
```

平台详细参数见 [各平台配置](providers.md)。使用 PUT 更新平台时应提交完整的新参数。使用 PATCH 时，只需提交需要更改的字段，例如 `{"enabled":false}` 或 `{"settings":{"to":["new@example.com"]}}`。已保存的密码和 Token 不会通过查询接口返回，未提交的敏感字段会保留。

## 发送消息

### JSON

`POST /v1/send` 使用发送密钥作为 Bearer Token。

```json
{
  "title": "备份完成",
  "body": "备份文件已成功生成",
  "provider_ids": ["9aa027b6-e5c7-4080-984d-b6b6d7eae6b0"],
  "attachments": [
    {
      "filename": "log.txt",
      "content_type": "text/plain",
      "data_base64": "SGVsbG8="
    }
  ]
}
```

`provider_ids` 可省略，省略时发送到该用户全部已启用的平台。消息标题最多 250 个字符，正文最多 100,000 字节。最多五个附件、总计不超过 2 MiB。文件名不能包含文件路径分隔符。

成功返回 HTTP 202：

```json
{
  "id": "d049cf07-fc5a-4164-927f-0e3061a7f70e",
  "status": "queued",
  "status_url": "/v1/events/d049cf07-fc5a-4164-927f-0e3061a7f70e"
}
```

可传 `Idempotency-Key` 请求头（1–128 字节），同一用户使用相同的值重复提交时返回第一次创建的消息编号，避免重试时重复创建消息。

### 上传真实文件

`POST /v1/send/multipart` 支持 `multipart/form-data`，字段为 `title`、`body`、重复的 `file` 或 `files`，也可以包含 JSON 数组字符串 `provider_ids`。

```bash
curl -X POST https://push.example.com/v1/send/multipart \
  -H 'Authorization: Bearer mpk_YOUR_KEY' \
  -F 'title=日志文件' \
  -F 'body=附件见下方' \
  -F 'files=@/tmp/service.log;type=text/plain'
```

## 与常见推送服务兼容的接口

这些地址都调用当前用户已启用的推送平台。将原有程序的 API 地址改为这里的地址，并填写由本服务创建的发送密钥即可。

| 风格 | 方法和地址 | 主要参数 |
| --- | --- | --- |
| Server酱 Turbo | `GET/POST /{KEY}.send` | `title` 或 `text`；`desp` 或 `content` |
| PushPlus | `GET/POST /send` | `token`、`title`、`content` |
| Bark | `GET /bark/{KEY}/{TITLE}/{BODY}` | 标题和正文写在 URL 路径 |
| ntfy | `POST /ntfy/{KEY}` | 请求体直接是正文，`Title` 请求头为标题 |
| Gotify | `POST /message?token={KEY}` | `title`、`message`，可使用 JSON 或表单 |

Server酱示例：

```bash
curl 'https://push.example.com/SCT_YOUR_KEY.send?title=测试&desp=你好'
```

PushPlus 示例：

```bash
curl -X POST https://push.example.com/send \
  -H 'Content-Type: application/json' \
  -d '{"token":"mpk_YOUR_KEY","title":"测试","content":"已完成"}'
```

ntfy 示例：

```bash
curl -X POST https://push.example.com/ntfy/mpk_YOUR_KEY \
  -H 'Title: 监控提醒' -d '网站出现异常'
```

Gotify 示例：

```bash
curl -X POST 'https://push.example.com/message?token=mpk_YOUR_KEY' \
  -d 'title=备份完成' -d 'message=一切正常'
```

以上接口兼容常见的消息内容参数及响应结构。原有服务提供的高级功能，例如 PushPlus 群组、Bark 音效或 ntfy 订阅管理，不在本项目当前接口的范围内。返回的成功状态说明主程序已接受消息。最终发送结果请通过管理页面查询。

### 签约和订阅

Telegram 个人接收者：

1. 建立 Telegram 平台并填写 `bot_token`、`bot_username`、`webhook_secret`。
2. 调用 `POST /v1/providers/{id}/telegram/webhook` 注册机器人 Webhook。
3. 调用 `POST /v1/providers/{id}/telegram/subscribe` 获取十五分钟有效的 URL。
4. 由接收者打开 URL、与机器人开始对话。主程序收到 `/start` 后保存 chat_id。

企业微信自建应用：

1. 配置企业 CorpID、应用 Secret、AgentID 和可信的网页授权域名。
2. 调用 `POST /v1/providers/{id}/wecom/subscribe` 获取十五分钟有效的 URL。
3. 企业成员在企业微信内打开 URL。授权结果返回 `/hooks/wecom`，主程序识别成员的 UserID 并保存。

两个授权接口都需要管理登录令牌，并且仅能操作自己创建的平台。已经知道接收者 ID 的情况下，可以直接配置 `chat_id` 或 `to_user`。

更完整的请求和响应格式、附件示例及兼容接口示例，请参阅 [API 接入指南](integration.md)。数据库任务的执行和重试方式见 [消息队列说明](queue.md)。
