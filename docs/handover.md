# 维护交接入口

本文供首次接手 MessagePusher 的开发者和运维人员使用。项目功能说明从 [README](../README.md) 开始；本文集中说明接手前需要确认的事项、阅读顺序和职责范围。

## 一、项目组成

MessagePusher 由三个部分组成：

1. **Rust 主程序**：提供管理页面、用户与渠道管理、消息 API、任务调度及直接向平台发送消息。
2. **PostgreSQL**：保存账户、渠道加密配置、发送密钥摘要、消息正文、附件、发送状态以及订阅记录。
3. **可选的 JavaScript 消息代理**：部署在 Vercel 或 Cloudflare Workers，代主程序访问 Telegram、企业微信、Resend。SMTP 由主程序直接发送。

管理员管理用户、站点与 **Provider 类型的开放状态**；每个用户独立创建、编辑、暂停和删除自己的渠道。管理页面默认采用简体中文，目前没有语言切换功能。

## 二、交接阅读顺序

| 阅读顺序 | 文档 | 用途 |
| --- | --- | --- |
| 1 | [使用手册](guide.md) | 熟悉普通用户与管理员的操作 |
| 2 | [系统结构与数据模型](architecture.md) | 确认模块职责、任务处理及数据关系 |
| 3 | [开发指南](development.md) | 安装环境、修改代码、运行测试 |
| 4 | [部署与维护操作手册](operations.md) | 发布前检查、备份、恢复、排查故障 |
| 5 | [安全与数据处理注意事项](security.md) | 密钥、用户数据、网络访问与事件处理 |
| 6 | [维护台账](maintenance-ledger.md) | 现有功能、注意事项、持续维护项目 |
| 7 | [设计决定](decisions.md) | 理解重要实现方式及变更影响 |
| 按需 | [API 接入指南](integration.md)、[推送平台配置](providers.md)、[Telegram](telegram.md)、[队列机制](queue.md)、[性能测试](performance.md) | 排查具体功能 |

代码变更约定见仓库根目录的 [CONTRIBUTING.md](../CONTRIBUTING.md)。

## 三、接手时需要完成的检查

- [ ] 获取 GitHub 仓库访问权限，确认负责维护的分支和发布流程。
- [ ] 确认生产环境使用 Docker Compose、普通服务器程序还是其他部署形式，记录所用版本。
- [ ] 确认 PostgreSQL 的部署位置、版本、容量、备份位置、备份周期、保留期限和恢复负责人。
- [ ] 确认 `CONFIG_KEY_BASE64`、`JWT_SECRET`、数据库密码、各平台代理密钥保存在团队批准的凭据管理系统中，并确认恢复时可以取回。
- [ ] 核对 `PUBLIC_BASE_URL`、域名、HTTPS 证书、反向代理和网络访问策略。
- [ ] 确认 Telegram、企业微信与邮件服务的负责人，以及相应的外部平台管理权限。
- [ ] 检查 [GitHub Actions](https://github.com/HSOS6Miyabi/MessagePusher/actions) 最近一次测试和构建是否成功。
- [ ] 在独立测试环境中完成一次部署、提交消息、查看发送状态和备份恢复演练。
- [ ] 明确谁有权限执行数据库恢复、修改站点设置、撤销密钥与发布版本。
- [ ] 确认运行日志的查看方式、磁盘容量检查方式及发生故障时的联系方式。

涉及实际部署环境的信息应保存在团队内部的交接系统。本仓库只记录字段名称与检查方法，不保存真实密码、密钥、数据库连接字符串或个人联系方式。

## 四、日常维护分工

| 工作 | 建议负责人 | 依据 |
| --- | --- | --- |
| 主程序与数据库迁移 | 后端开发人员 | [开发指南](development.md)、[系统结构](architecture.md) |
| 管理页面和浏览器测试 | 前端开发人员 | [开发指南](development.md) |
| 消息代理部署与配置 | 部署维护人员 | [部署说明](deploy.md)、[安全事项](security.md) |
| PostgreSQL 备份与恢复 | 数据库维护人员 | [操作手册](operations.md) |
| 发布、构建产物检查 | 发布负责人 | [操作手册](operations.md) |
| 平台凭据与权限 | 被授权的系统管理员及各渠道所有者 | [安全事项](security.md) |

实际负责人由接手团队指定。

## 五、修改项目时一并维护哪些文档

| 修改范围 | 必须同步检查 |
| --- | --- |
| 新增或修改 HTTP API | [API 说明](api.md)、[接入指南](integration.md)、相应测试 |
| 新增 Provider、修改授权流程 | [Provider 配置](providers.md)、[使用手册](guide.md)、对应平台文档 |
| 修改数据库、任务分配、重试 | [系统结构](architecture.md)、[队列机制](queue.md)、[维护台账](maintenance-ledger.md) |
| 修改环境变量或部署方式 | [部署说明](deploy.md)、[操作手册](operations.md)、[.env.example](../.env.example) |
| 修改安全策略 | [安全事项](security.md)、[设计决定](decisions.md) |
| 修改 CI 或发布方式 | [开发指南](development.md)、[操作手册](operations.md) |
| 修改管理页面 | 浏览器测试、移动端截图、[使用手册](guide.md) |
| 添加新的维护事项 | [维护台账](maintenance-ledger.md) |

提交前执行 `node scripts/check-docs.mjs`，检查仓库文档的本地链接。

## 六、需要保留的运行信息

- `CONFIG_KEY_BASE64` 丢失后，现有渠道的加密设置无法解密；备份数据库时必须单独安全保存此密钥。
- `ADMIN_PASSWORD` 用于首次创建管理员；之后修改该环境变量不会更改数据库中已有账户的密码。
- 提交 API 返回 HTTP 202 表示已保存消息，实际发送结果需到消息记录查询。
- 消息任务默认最多尝试五次；外部平台已收到消息但程序尚未保存结果时，重试可能使接收者看到重复通知。
- 消息及附件当前没有自动到期删除机制，数据库存储量会持续增长。
- `/healthz` 只确认 HTTP 进程能够响应，不能单独证明 PostgreSQL、后台任务及所有外部平台正常。

具体处理步骤见 [操作手册](operations.md)，持续维护事项见 [维护台账](maintenance-ledger.md)。
