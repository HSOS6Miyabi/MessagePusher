# 部署维护、备份和故障处理操作手册

本手册供实际运行 MessagePusher 的维护人员使用。首次安装步骤见 [部署说明](deploy.md)，运行设计见 [系统结构](architecture.md)，密钥管理见 [安全注意事项](security.md)。

## 1. 生产部署前检查

- 确认 PostgreSQL 数据库可以连接，数据库卷具有持久存储空间。
- 用独立的随机值配置 `CONFIG_KEY_BASE64`（Base64 解码后必须为 32 字节）、`JWT_SECRET`（至少 32 字符）、数据库密码及管理员初始密码。
- 配置公开 HTTPS 入口，`PUBLIC_BASE_URL` 填写用户实际访问的主程序地址；Telegram Webhook 和企业微信网页授权需要正确的公开地址。
- 确认反向代理能够转发 API、`/hooks/telegram/{id}` 和 `/hooks/wecom`。
- 确认正式环境未启用 `ALLOW_HTTP_RELAY=1`。为出站访问第三方平台与代理配置必要的网络规则。
- 确认谁保管数据库备份、主加密密钥、代理共享密钥和其他平台账户的恢复权限。
- 按照业务量设置 `WORKER_CONCURRENCY`（默认 32）和 `DATABASE_MAX_CONNECTIONS`（默认 24）。多个主程序实例的 PostgreSQL 连接总数应低于数据库允许的连接数，并留有管理连接。
- 记录当前发布版本、数据库容量与可用空间，并确定日志留存和异常告警方式。

## 2. 启动与日常检查

Docker Compose 部署，从含有 `compose.yaml` 的目录执行：

```bash
docker compose ps
docker compose up -d --build
docker compose ps
docker compose logs --tail=100 messagepusher
curl -fsS http://127.0.0.1:8080/healthz
```

`/healthz` 返回 `{"ok":true}` 只说明 HTTP 服务能够响应。还应检查数据库连接、待发送任务及实际平台消息。查看队列状态：

```bash
docker compose exec -T postgres psql -U messagepusher -d messagepusher \
  -c "SELECT status, count(*) FROM deliveries GROUP BY status ORDER BY status;"
```

检查待发送及到期重试任务的数量：

```bash
docker compose exec -T postgres psql -U messagepusher -d messagepusher \
  -c "SELECT count(*) FROM deliveries WHERE status='pending' AND next_attempt_at<=now();"
```

`pending` 有时代表等待下一次重试；请结合 `next_attempt_at` 和渠道的启用状态判断。`working` 超过租约时间后，后台工作进程可以重新领取。

如果采用普通服务器程序，应查看对应的 systemd、Windows 服务管理器或进程管理工具中的启动状态和日志。

## 3. 备份 PostgreSQL

在生产环境建立自动备份任务，并限制备份文件的访问权限。以下命令适用于当前仓库的 Docker Compose 部署，从仓库根目录执行：

```bash
mkdir -p backups
chmod 700 backups
docker compose exec -T postgres pg_dump -U messagepusher -d messagepusher -Fc \
  > "backups/messagepusher-$(date +%Y%m%d-%H%M%S).dump"
```

命令生成自定义格式的 PostgreSQL 数据库备份。检查输出文件存在且大小合理，并使用独立测试数据库定期恢复验证。请根据业务要求制定备份频率、异地保存、保留期限与恢复时间目标。

**数据库备份不会保存进程环境变量。** 必须另外在受保护的凭据管理系统中保存与数据库配套的 `CONFIG_KEY_BASE64`。同时妥善保管 `JWT_SECRET`、数据库登录信息和各平台代理密钥。不要把真实备份、`.env`、密钥或用户附件提交到 Git。

## 4. 在独立数据库验证恢复

下列示例会创建一个新数据库 `messagepusher_restore_check`，不会覆盖正在运行的 `messagepusher` 数据库。在执行前确认数据库名称没有被其他业务使用，并将 `BACKUP_FILE` 改成实际备份文件路径。

```bash
BACKUP_FILE='backups/messagepusher-YYYYMMDD-HHMMSS.dump'
docker compose exec -T postgres createdb -U messagepusher -O messagepusher messagepusher_restore_check
docker compose exec -T postgres pg_restore -U messagepusher \
  --exit-on-error --no-owner --no-acl \
  -d messagepusher_restore_check < "$BACKUP_FILE"
docker compose exec -T postgres psql -U messagepusher -d messagepusher_restore_check \
  -c 'SELECT count(*) AS user_count FROM users;'
docker compose exec -T postgres psql -U messagepusher -d messagepusher_restore_check \
  -c 'SELECT version, success FROM _sqlx_migrations ORDER BY version;'
```

接着在隔离网络内，以恢复后的数据库和对应的 `CONFIG_KEY_BASE64` 启动测试实例，验证能登录、读取个人渠道、查看发送历史以及创建测试消息。测试实例应避免误向真实接收者发送消息。

正式恢复时先停止访问原数据库的应用实例，在经审批的恢复方案中选择已验证的数据副本，并把应用的 `DATABASE_URL` 指向恢复数据库。当前 `compose.yaml` 的数据库名称写在服务环境变量中，切换数据库名称时需要修改该配置并重新创建主程序容器。保留原数据库副本，直至核实恢复结果。

## 5. 升级与发布前后检查

升级前：

1. 记录 `git rev-parse HEAD`、当前 GitHub Release 或容器镜像摘要。
2. 检查 [GitHub Actions](https://github.com/HSOS6Miyabi/MessagePusher/actions) 的 Rust、数据库、浏览器、跨平台编译和 Docker 工作是否通过。
3. 阅读新迁移文件，确认应用版本与数据库结构的兼容性。
4. 备份 PostgreSQL，验证 `CONFIG_KEY_BASE64` 可以从安全存储中恢复。
5. 预先安排能够恢复到先前版本的应用程序或镜像。

从源代码部署的 Docker Compose 实例可以执行：

```bash
git pull --ff-only
docker compose up -d --build
docker compose ps
docker compose logs --tail=150 messagepusher
```

升级后检查：`/healthz`、登录页面、用户与渠道列表、创建测试消息、发送结果、数据库错误数量及后台任务状态。使用真实 Provider 的测试要选择专用接收者。

**应用程序可以回退到已保存的版本；已执行的 SQLx 迁移不会自动撤销。** 如果旧程序不兼容新的数据库结构，需要按事先验证的恢复方案处理数据库。不要在生产数据库上直接删除 `_sqlx_migrations` 记录或修改旧迁移文件。

GitHub Actions 的发布方式：

- `.github/workflows/ci-release.yml`：通过测试与五个平台编译后，提交到 `main` 的当前版本更新 `edge` Release；`v*` 标签创建对应版本发布。
- `.github/workflows/docker.yml`：构建 Linux AMD64/ARM64，并发布 `ghcr.io/hsos6miyabi/messagepusher:edge`；`v*` 标签还会发布对应版本和 `latest`。
- 浏览器截图保存在相关 GitHub Actions 运行的 Artifact 中。
- 请核实 `main`、`edge` 标签与镜像发布时间对应的提交版本。两个工作流分别运行，需分别检查结果。

## 6. 常见故障的检查顺序

| 现象 | 优先检查 | 处理 |
| --- | --- | --- |
| 服务无法启动 | 主程序日志、`DATABASE_URL`、数据库状态、`CONFIG_KEY_BASE64` 长度及 JWT 密钥 | 修正配置，确认迁移成功后重启 |
| 网站可以访问但消息没有发送 | `deliveries` 状态、`next_attempt_at`、渠道启用状态及 Provider 类型开放状态 | 检查工作进程、出站网络及外部平台账户 |
| 多条消息处于 `working` | 租约、日志、数据库连接与正在执行的请求 | 排查网络或任务进程；过期任务会按租约机制重新领取 |
| Telegram 订阅失败 | `PUBLIC_BASE_URL`、Webhook 可访问性、机器人 Token 与订阅链接是否过期 | 重新生成订阅链接，检查平台回调与请求认证 |
| 企业微信授权失败 | CorpID、Secret、AgentID、接收者可见范围及 HTTPS 回调 | 核查企业微信管理端与主程序设置 |
| SMTP 无法连接 | 主程序网络、服务器地址、端口、TLS 模式、账户授权 | 在相同出站网络条件下检查 SMTP 连接 |
| 代理 HTTP 401 | 代理和个人渠道所填的共享密钥、服务器时钟 | 核实 HMAC 配置及双方时间偏差 |
| 磁盘空间持续下降 | `events.attachments` 和历史消息数量、PostgreSQL 卷占用 | 增加存储，制定经过测试的数据保留方案 |
| 管理员无法登录 | 账户是否存在、密码是否被修改、令牌是否失效 | 由已有授权管理员重设密码或按恢复流程处理 |

查看日志时注意隐藏个人信息、请求正文、完整 URL 中可能包含的密钥及平台错误详情。

## 7. 数据保留和紧急处置

当前主程序没有自动到期删除消息和附件的后台任务。清理历史数据前必须明确数据保留期限、是否需要保留发送记录以及受影响的外键关系，并在测试数据库验证删除操作。

若怀疑凭据泄露，应限制受影响的接口访问，撤销相关发送密钥，按平台规则更换机器人 Token、SMTP 授权码或代理密钥，然后核查日志与已发送消息。`CONFIG_KEY_BASE64` 的更换需要同时处理现有加密配置，不能直接替换环境变量。

持续维护事项见 [维护台账](maintenance-ledger.md)；更多安全说明见 [安全事项](security.md)。
