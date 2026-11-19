# 系统结构与数据库说明

本文说明 MessagePusher 的主要模块、数据关系和消息处理过程。各模块的实际实现位于仓库列出的代码路径中。

## 1. 程序组成

| 位置 | 作用 |
| --- | --- |
| [src/main.rs](../src/main.rs) | 读取环境变量、连接数据库、执行迁移、启动 HTTP 服务与后台工作进程 |
| [src/api.rs](../src/api.rs) | 登录、用户、发送密钥、个人渠道、消息创建和历史查询 |
| [src/site.rs](../src/site.rs) | 站点设置、注册、个人密码修改、管理员概览 |
| [src/catalog.rs](../src/catalog.rs) | Provider 类型目录及管理员开放状态 |
| [src/models.rs](../src/models.rs) | 消息、附件、渠道、代理请求的数据结构及输入检查 |
| [src/delivery.rs](../src/delivery.rs) | 消息任务领取、并发发送、代理分组、失败重试 |
| [src/security.rs](../src/security.rs) | 密码散列、数据加密、登录令牌及 HMAC 签名 |
| [src/subscribe.rs](../src/subscribe.rs) | Telegram 订阅、Webhook 以及企业微信成员授权 |
| [src/compat.rs](../src/compat.rs) | Server酱、PushPlus、Gotify、ntfy、Bark 兼容入口 |
| [src/error.rs](../src/error.rs) | HTTP API 错误转换 |
| [web/index.html](../web/index.html)、[web/app.js](../web/app.js)、[web/styles.css](../web/styles.css) | 内置中文管理页面、交互与样式 |
| [relay/core.js](../relay/core.js) | 无状态消息代理；[Vercel 入口](../relay/api/index.js) 和 [Cloudflare 入口](../relay/cloudflare.js) 共用此逻辑 |
| [migrations](../migrations) | SQLx 启动时执行的 PostgreSQL 迁移 |

前端页面由 Rust 主程序直接提供静态 HTML、JavaScript 和 CSS，不需要单独启动前端开发服务器。

## 2. 消息提交与发送

1. 客户端访问管理页面，或业务程序使用发送密钥调用 `POST /v1/send`。
2. `src/api.rs` 验证调用者身份、消息格式以及调用者有权使用的渠道。
3. 一次 PostgreSQL 事务向 `events` 写入消息，并为目标渠道向 `deliveries` 批量插入发送任务，同时执行 `NOTIFY messagepusher_delivery`。
4. 事务提交后，API 返回 HTTP 202 和消息 ID。PostgreSQL 向监听进程发送通知。
5. `src/delivery.rs` 使用 `FOR UPDATE SKIP LOCKED` 领取可处理任务，直接调用平台接口，或按代理地址与密钥分组调用消息代理。
6. 任务完成后更新 `deliveries.status`、尝试次数、重试时间及错误说明。管理页面通过 `GET /v1/events` 和 `GET /v1/events/{id}` 查看结果。

后台默认最多有 32 个任务并行执行，单次最多领取 16 个；正在处理的任务具有 75 秒租约，直接发送任务有 55 秒总超时，主程序 HTTP 请求另有 22 秒超时。默认最多发送五次。工作进程同时使用 PostgreSQL 通知、最近任务的下一次处理时间和最长 60 秒的恢复检查。

**发送语义为至少尝试一次。** 在外部平台接受请求后、数据库尚未保存成功结果前发生进程故障，恢复后的重试可能使同一通知重复显示。`Idempotency-Key` 只能防止同一用户重复创建消息任务，无法保证所有外部平台仅接收一次。

参见 [数据库队列说明](queue.md) 和 [性能测试](performance.md)。

## 3. 数据库表关系

| 表 | 主要内容 | 关系和删除行为 |
| --- | --- | --- |
| `users` | 用户、密码散列、角色、启用状态、登录令牌版本 | 用户是个人渠道和消息的所有者 |
| `api_keys` | 发送密钥摘要、显示前缀、撤销时间 | 关联用户；删除用户时级联删除 |
| `providers` | 渠道名称、类型、启用状态、加密配置 | 属于用户；同一用户的渠道名称唯一 |
| `provider_catalog` | 五种内置 Provider 类型、展示名称和开放状态 | 站点管理员只维护类型开放状态 |
| `events` | 消息标题、正文、附件 JSON、可选幂等键 | 属于用户；删除用户时级联删除 |
| `deliveries` | 各渠道独立的发送任务、状态、错误、尝试次数 | 属于事件；删除事件时级联删除；删除渠道后保留渠道名称及历史状态 |
| `subscriptions` | Telegram 或企业微信的一次性订阅代码摘要、到期时间 | 关联用户与渠道；有效期由相应流程控制 |
| `site_settings` | 站点名称、简介、公告、注册开关 | 全站共用的一条配置记录 |

`deliveries.provider_id` 允许为空，删除渠道时设置为空；`deliveries.provider_name` 保存当时的渠道名称，供历史记录展示。

所有迁移按文件名顺序执行：

| 文件 | 主要作用 |
| --- | --- |
| [0001_initial.sql](../migrations/0001_initial.sql) | 用户、发送密钥、渠道、消息、任务、订阅 |
| [0002_delivery_history.sql](../migrations/0002_delivery_history.sql) | 保留渠道删除后的发送历史 |
| [0003_site_settings.sql](../migrations/0003_site_settings.sql) | 站点设置与登录令牌版本 |
| [0004_provider_catalog.sql](../migrations/0004_provider_catalog.sql) | Provider 类型目录 |
| [0005_queue_indexes.sql](../migrations/0005_queue_indexes.sql) | 待发送任务和工作中任务的部分索引 |

新增数据库结构时应创建下一号迁移文件，保留已执行迁移文件的原始内容。迁移需要通过独立数据库测试，并在正式部署前备份生产数据。

## 4. 身份和权限

- 登录使用用户名及密码，密码由 Argon2 处理；管理页面通过 24 小时有效的登录令牌访问需认证接口。
- 发送密钥用于提交消息；密钥创建时只返回一次，数据库只保存 SHA-256 摘要。
- 普通用户只能查看或修改自己的 `providers`、`api_keys` 和 `events`；管理员管理账户及全站设置，同时自身的渠道仍属于个人账户。
- 修改密码、调整用户状态或角色时，程序根据账户状态及登录令牌版本控制之前的会话。
- 渠道配置用 AES-256-GCM 加密保存；详情 API 对凭据字段只返回是否已经设置。

安全边界和运行注意事项参阅 [安全与数据处理](security.md)。

## 5. 新增 Provider 时的修改范围

1. 在 `src/models.rs` 中定义所需配置项及验证规则。
2. 在 `src/delivery.rs` 中加入直接发送实现；若代理支持，还需修改 `relay/core.js` 和代理测试。
3. 根据需要修改 `src/subscribe.rs` 的接收者绑定或授权流程。
4. 新建数据库迁移，更新 `providers.kind` 和 `provider_catalog.kind` 的允许值及展示记录。
5. 在 `web/app.js` 中加入表单字段与展示文本；必要时调整 HTML/CSS。
6. 更新 [平台配置](providers.md)、[API 接入](integration.md)、[使用手册](guide.md) 与 [维护台账](maintenance-ledger.md)。
7. 增加 Rust、代理与浏览器测试，验证普通用户只能管理自己的渠道。

请检查兼容 API 与代理是否仍按原有规则处理消息；SMTP 只能由 Rust 主程序直接发送。
