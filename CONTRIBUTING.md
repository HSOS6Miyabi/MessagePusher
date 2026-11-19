# 参与开发和维护

本文供提交源代码、修改部署配置及维护文档的人员使用。项目交接总入口见 [维护交接](docs/handover.md)，开发环境和测试步骤见 [开发指南](docs/development.md)。

## 修改前

1. 阅读 [系统结构](docs/architecture.md)，确认接口、数据表和模块的对应关系。
2. 查看当前 `main` 分支及相关测试，避免覆盖其他人刚刚修改的内容。
3. 涉及数据库时检查全部 `migrations/` 文件；已经应用的迁移不得改写，应创建新的迁移文件。
4. 涉及密钥、用户权限或外部网络访问时先阅读 [安全事项](docs/security.md)。
5. 遇到影响系统整体行为的方案变化，更新 [设计决定](docs/decisions.md) 和 [维护台账](docs/maintenance-ledger.md)。

## 代码与文档一起提交

- API 行为变动：同时修改 [API 文档](docs/api.md)、[接入指南](docs/integration.md) 与相关测试。
- 新增或修改 Provider：同时更新 Rust、可选代理、管理页面以及 [平台配置文档](docs/providers.md)。
- 管理页面改动：分别测试管理员与普通用户、空数据和有数据页面、手机与桌面视口，保存浏览器截图供检查。
- 环境变量、部署、CI 和发布流程改动：同步修改 [部署说明](docs/deploy.md)、[运维手册](docs/operations.md) 以及 `.env.example`。
- 用户界面文字优先使用规范、自然的简体中文。代码标识符、配置项和第三方平台名称保持原文。

## 提交前检查

```bash
cargo test --all
node --check web/app.js
node scripts/check-docs.mjs
cd relay && npm test
```

修改前端时增加以下检查：

```bash
node web/tests/browser.mjs
node web/tests/complete-ui.mjs
```

浏览器脚本依赖 Playwright 和 Chromium。完整安装命令见 [开发指南](docs/development.md)。修改数据库 API 后，应在隔离环境运行 `web/tests/live-backend.mjs`。真实 Telegram 测试需要专用测试机器人，避免对生产接收者产生影响。

提交后查看 GitHub Actions 的测试、五个平台编译结果和 Docker 构建。确认通过后再发布。详细发布流程见 [运维手册](docs/operations.md)。

## 敏感信息处理

不提交 `.env`、数据库备份、真实用户消息或附件、机器人 Token、邮件授权码、发送密钥、JWT 密钥和代理共享密钥。测试示例使用虚构值，生产密钥存储在受保护的凭据管理系统中。
