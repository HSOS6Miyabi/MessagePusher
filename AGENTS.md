# MessagePusher 仓库维护说明

本文件供进入仓库的开发人员和自动化代码维护工具阅读。

## 开始工作

- 先查看 `README.md`、`docs/handover.md`、`docs/architecture.md` 和相关源代码，确认当前分支与文件内容。
- 用户可能在任务执行期间自行修改文件。继续更改之前重新读取最新内容，不要恢复用户已撤销的修改。
- 修改完成后运行对应测试；涉及 UI 时运行浏览器测试并检查截图；涉及数据库时使用独立测试数据库。
- 完整操作和测试命令参见 `docs/development.md`，发布流程参见 `docs/operations.md`。

## 项目约束

- Rust 主程序与可选 JavaScript 代理分开部署；前端静态资源编译进入 Rust 可执行文件。
- 管理员管理 Provider 类型和用户；个人 Provider 和发送密钥归各自用户管理，必须保持账户隔离。
- PostgreSQL 迁移文件一旦应用，不得修改原有内容；后续结构更新添加新的迁移。
- 发送状态应保持数据库持久化、多实例安全领取、失败重试和按任务记录结果的行为。
- 代理请求使用 HMAC 签名；变更协议时同步修改 Rust、JavaScript 代理及测试。
- SMTP 只由 Rust 主程序直接发送。消息附件与代理请求有大小限制，修改时应同步检查前后端。
- 文档不包含真实密钥、数据库连接凭据、生产消息或用户私人信息。

## 文档与测试

- 更新 API：检查 `docs/api.md`、`docs/integration.md`。
- 更新数据库、工作进程：检查 `docs/architecture.md`、`docs/queue.md`、`docs/maintenance-ledger.md`。
- 更新部署、凭据或发布：检查 `docs/deploy.md`、`docs/operations.md`、`docs/security.md`。
- 更新重要设计方案：检查 `docs/decisions.md`。
- 修改文档后运行 `node scripts/check-docs.mjs`；修改功能后运行相关 Rust、代理或浏览器测试。
- 提交信息清楚说明本次完成的功能或文档范围，避免把内部调试记录写入用户文档。
