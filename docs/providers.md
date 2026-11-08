# 推送平台配置

站点管理员在“站点设置”中决定开放哪些 Provider 类型，每种类型默认开放。用户登录后在“推送渠道”页面独立创建、编辑、暂停和删除自己的渠道，并填写个人的账户凭据、收件地址及代理信息。各用户的数据相互隔离，管理员只提供可选的平台类型。管理员暂停某种类型时，现有的个人配置会保留；再次开放后可继续使用。

在管理页面选择平台类型，按照界面提示填写参数。每个平台都有一个名称，同一用户可添加多个邮件地址、Telegram 群组或企业微信应用。

## Telegram

```json
{
  "bot_token": "123456789:BOT_TOKEN",
  "bot_username": "MyNotifyBot",
  "webhook_secret": "RANDOM_32_CHARACTER_SECRET"
}
```

通过 @BotFather 创建机器人并取得 Bot Token。平台创建后，先“设置 Webhook”，再生成订阅链接。接收者必须打开链接并点击 Start。链接 15 分钟失效，失效后可重新生成。

如果已经知道聊天 ID，可以在 JSON 里设置 `chat_id`（字符串或整数），跳过链接绑定。群组推送需要先将机器人加入群组，并赋予发送权限。

若服务器访问不了 Telegram，将发送方式设为代理。Webhook 的设置请求也会经代理发出；Telegram 收到的用户消息仍然会发送到主程序的 HTTPS Webhook 地址，因此 `PUBLIC_BASE_URL` 应能从互联网访问。

## 企业微信自建应用

```json
{
  "corp_id": "ww_CORP_ID",
  "corp_secret": "APP_SECRET",
  "agent_id": 1000002
}
```

在企业微信管理后台创建自建应用，获取应用的 Secret 和 AgentID，并将希望接收通知的成员纳入应用可见范围。配置企业微信网页授权可信域名，使用管理页面生成的授权链接，由企业成员在企业微信中打开后，系统会自动保存其 `to_user`。

如果已知要发送的企业微信 UserID，可以直接填入 `"to_user":"zhangsan"`。应用需要拥有给该成员发送消息的权限。企业微信会自行执行接收方、应用可见范围及接口调用次数的限制。

每个附件会先上传为临时素材，然后以文件消息发送。

## 企业微信群机器人

```json
{
  "webhook_key": "GROUP_BOT_KEY"
}
```

在企业微信群里添加群机器人。复制 Webhook 地址，取出 `key=` 后的值放到 `webhook_key`。群机器人会将文本和附件发送到这个群。无需个人订阅授权。

## SMTP 邮件

```json
{
  "host": "smtp.example.com",
  "port": 587,
  "tls": "starttls",
  "username": "notifications@example.com",
  "password": "SMTP_APP_PASSWORD",
  "from": "notifications@example.com",
  "to": ["user1@example.com", "user2@example.com"]
}
```

`tls` 可选 `starttls`、`implicit` 或 `none`，使用时应优先选择安全连接。`port` 根据邮箱服务商的设置填写，常见为 587 或 465。附件通过 MIME 添加在邮件中。SMTP 只能由主程序发送。

某些云主机限制 SMTP 出站端口；遇到这种情况可以使用 Resend 邮件 API，并通过消息代理发送。

## Resend 邮件 API

```json
{
  "api_key": "re_YOUR_API_KEY",
  "from": "Notify <notice@example.com>",
  "to": ["receiver@example.com"]
}
```

需要拥有 Resend API Key，并按 Resend 要求验证发件域名。这个邮件平台使用 HTTPS，主程序和 Vercel / Cloudflare 代理都可发送，支持附件。

## 使用无状态消息代理

选择“通过无状态消息代理发送”，填写：

```json
{
  "mode": "proxy",
  "url": "https://relay.example.com/",
  "secret": "ONE_RANDOM_SECRET_AT_LEAST_24_CHARACTERS"
}
```

同一个代理 URL 和密钥可以配置给多个平台。主程序会合并推送，代理接到请求后并行向多个目标发送，每个平台返回自己的状态。代理只保存环境变量中的共享密钥，不使用数据库。

建议先访问代理的 `/healthz`，检查是否返回 `{"ok":true,"service":"messagepusher-relay"}`。它无需授权即可访问；发送接口需要有效签名。

主程序保存的平台密码、Token 和代理密钥均以加密形式保存在 PostgreSQL。请把代理 URL 和代理密钥当作敏感设置管理，不要使用不可信的代理服务。

用户从创建渠道到完成发送的具体操作，请阅读 [使用手册](guide.md)。Telegram 私聊订阅和 Webhook 说明见 [Telegram 使用说明](telegram.md)。
