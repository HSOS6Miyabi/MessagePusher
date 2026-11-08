import { chromium } from 'playwright';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const shots = path.join(root, 'screenshots', 'complete');
await mkdir(shots, { recursive: true });

const today = '2026-11-08T09:00:00.000Z';
const fixedSite = () => ({
  site_name: 'MessagePusher',
  site_description: '统一管理服务通知，确保重要消息及时送达。',
  announcement: '',
  allow_registration: false
});
let site = fixedSite();
const kinds = [
  { kind: 'telegram', label: 'Telegram 机器人', description: '通过机器人接收消息和文件', enabled: true },
  { kind: 'wecom_app', label: '企业微信自建应用', description: '向企业成员发送通知', enabled: true },
  { kind: 'wecom_webhook', label: '企业微信群机器人', description: '向企业微信群发送消息', enabled: true },
  { kind: 'email_smtp', label: 'SMTP 电子邮件', description: '通过 SMTP 发送邮件和附件', enabled: true },
  { kind: 'email_resend', label: 'Resend 邮件 API', description: '通过 HTTPS API 发送邮件', enabled: true }
];
const users = [
  { id: '00000000-0000-4000-8000-000000000001', username: 'administrator', role: 'admin', active: true, password: 'admin-password-123', created_at: today }
];
const channels = [];
const keys = [];
const messages = [];
let counter = 100;
let sendPayload = null;
const calls = [];
const nextId = () => '00000000-0000-4000-8000-' + String(++counter).padStart(12, '0');
const auth = request => {
  const header = request.headers.authorization || '';
  if (header === 'Bearer admin-session') return users[0];
  if (header.startsWith('Bearer member-session-')) {
    return users.find(user => user.id === header.slice('Bearer member-session-'.length));
  }
  if (header.startsWith('Bearer ')) return users.find(user => user.id === keys.find(key => key.secret === header.slice(7))?.owner);
  return null;
};
const response = (res, code, body) => {
  res.writeHead(code, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
  res.end(JSON.stringify(body));
};
const collect = async req => {
  const chunks = [];
  for await (const part of req) chunks.push(part);
  const raw = Buffer.concat(chunks).toString('utf8');
  return raw ? JSON.parse(raw) : {};
};
const err = (res, status, message) => response(res, status, { error: message });
const catalog = () => kinds.filter(kind => kind.enabled).map(kind => ({ ...kind }));
const details = (channel, id) => {
  const visible = {
    telegram: ['bot_username', 'chat_id'],
    email_smtp: ['host', 'port', 'tls', 'username', 'from', 'to'],
    email_resend: ['from', 'to'],
    wecom_app: ['corp_id', 'agent_id', 'to_user'],
    wecom_webhook: []
  }[channel.kind];
  return {
    id, name: channel.name, kind: channel.kind, enabled: channel.enabled,
    settings: Object.fromEntries(Object.entries(channel.settings).filter(([key]) => visible.includes(key))),
    stored_secrets: Object.keys(channel.settings).filter(key => ['bot_token', 'password', 'corp_secret', 'webhook_key', 'api_key', 'webhook_secret'].includes(key)),
    delivery: channel.delivery.mode === 'proxy'
      ? { mode: 'proxy', url: channel.delivery.url, secret_saved: true }
      : { mode: 'direct' }
  };
};
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://localhost');
    const pathname = url.pathname;
    const method = req.method;
    if (method === 'GET' && ['/', '/assets/styles.css', '/assets/app.js'].includes(pathname)) {
      const source = { '/': 'index.html', '/assets/styles.css': 'styles.css', '/assets/app.js': 'app.js' }[pathname];
      const content = await readFile(path.join(root, source));
      res.writeHead(200, { 'content-type': source.endsWith('html') ? 'text/html' : source.endsWith('css') ? 'text/css' : 'application/javascript' });
      res.end(content);
      return;
    }
    if (method === 'GET' && pathname === '/v1/site') return response(res, 200, site);
    if (method === 'GET' && pathname === '/v1/provider-catalog') return response(res, 200, catalog());
    if (method === 'POST' && pathname === '/v1/login') {
      const input = await collect(req);
      const user = users.find(u => u.username === input.username && u.password === input.password && u.active);
      if (!user) return err(res, 401, 'Authentication required');
      return response(res, 200, { token: user.role === 'admin' ? 'admin-session' : 'member-session-' + user.id });
    }
    if (method === 'POST' && pathname === '/v1/register') {
      if (!site.allow_registration) return err(res, 403, 'Permission denied');
      const input = await collect(req);
      if (users.some(u => u.username === input.username)) return err(res, 400, 'username already exists');
      const user = { id: nextId(), username: input.username, password: input.password, role: 'user', active: true, created_at: today };
      users.push(user);
      return response(res, 201, { token: 'member-session-' + user.id, role: 'user', id: user.id });
    }
    const current = auth(req);
    if (!current || !current.active) return err(res, 401, 'Authentication required');
    if (method === 'GET' && pathname === '/v1/me') {
      return response(res, 200, { id: current.id, username: current.username, role: current.role, active: true });
    }
    if (method === 'POST' && pathname === '/v1/me/password') {
      const input = await collect(req);
      if (current.password !== input.current_password) return err(res, 400, 'current password is incorrect');
      current.password = input.new_password;
      return response(res, 200, { changed: true, login_required: true });
    }
    if (method === 'GET' && pathname === '/v1/providers') {
      return response(res, 200, channels.filter(ch => ch.owner === current.id).map(ch => ({
        id: ch.id, name: ch.name, kind: ch.kind, enabled: ch.enabled,
        subscribed: Boolean(ch.settings.chat_id || ch.settings.to_user) || !['telegram', 'wecom_app'].includes(ch.kind),
        available: kinds.find(k => k.kind === ch.kind)?.enabled ?? false,
        delivery: ch.delivery.mode, created_at: ch.created_at
      })));
    }
    if (method === 'POST' && pathname === '/v1/providers') {
      const input = await collect(req);
      if (!kinds.find(k => k.kind === input.kind)?.enabled) return err(res, 400, 'Provider is not available');
      if (!input.name || !input.kind) return err(res, 400, 'Missing provider data');
      const channel = { id: nextId(), owner: current.id, ...input, created_at: today };
      channels.push(channel);
      calls.push({ type: 'provider-created', kind: channel.kind });
      return response(res, 201, { id: channel.id });
    }
    const providerMatch = pathname.match(/^\/v1\/providers\/([^/]+)$/);
    if (providerMatch) {
      const channel = channels.find(ch => ch.id === providerMatch[1] && ch.owner === current.id);
      if (!channel) return err(res, 404, 'Not found');
      if (method === 'GET') return response(res, 200, details(channel, channel.id));
      if (method === 'PATCH') {
        const patch = await collect(req);
        if (patch.name !== undefined) channel.name = patch.name;
        if (patch.enabled !== undefined) channel.enabled = patch.enabled;
        if (patch.settings) {
          for (const [key, value] of Object.entries(patch.settings)) {
            if (value === null) delete channel.settings[key];
            else channel.settings[key] = value;
          }
        }
        if (patch.delivery) {
          if (patch.delivery.mode === 'direct') channel.delivery = { mode: 'direct' };
          else channel.delivery = { ...channel.delivery, ...patch.delivery };
        }
        calls.push({ type: 'provider-updated', id: channel.id, patch });
        return response(res, 200, { updated: true, id: channel.id });
      }
      if (method === 'DELETE') {
        channels.splice(channels.indexOf(channel), 1);
        return response(res, 200, { deleted: true });
      }
    }
    const subscribeMatch = pathname.match(/^\/v1\/providers\/([^/]+)\/(telegram\/webhook|telegram\/subscribe|wecom\/subscribe)$/);
    if (subscribeMatch && method === 'POST') {
      const ch = channels.find(ch => ch.id === subscribeMatch[1] && ch.owner === current.id);
      if (!ch) return err(res, 404, 'Not found');
      if (subscribeMatch[2].endsWith('webhook')) {
        calls.push({ type: 'webhook-set', id: ch.id });
        return response(res, 200, { configured: true });
      }
      const url = subscribeMatch[2].startsWith('telegram')
        ? 'https://t.me/example_notify_bot?start=subscription_123456789'
        : 'https://open.weixin.qq.com/connect/oauth2/authorize?appid=abc&state=xyz';
      return response(res, 200, { url, expires_in_seconds: 900 });
    }
    if (method === 'GET' && pathname === '/v1/keys') {
      return response(res, 200, keys.filter(k => k.owner === current.id).map(k => ({
        id: k.id, name: k.name, prefix: k.secret.slice(0, 10), created_at: k.created_at
      })));
    }
    if (method === 'POST' && pathname === '/v1/keys') {
      const input = await collect(req);
      const secret = (input.format === 'serverchan' ? 'SCT' : 'mpk_') + 'test_' + String(counter + 1);
      const key = { id: nextId(), owner: current.id, name: input.name, secret, created_at: today };
      keys.push(key);
      return response(res, 201, { id: key.id, key: secret });
    }
    const keyMatch = pathname.match(/^\/v1\/keys\/([^/]+)$/);
    if (keyMatch && method === 'DELETE') {
      const key = keys.find(k => k.id === keyMatch[1] && k.owner === current.id);
      if (!key) return err(res, 404, 'Not found');
      keys.splice(keys.indexOf(key), 1);
      return response(res, 200, { revoked: true });
    }
    if (method === 'POST' && pathname === '/v1/send') {
      const raw = req.headers.authorization?.replace('Bearer ', '');
      const key = keys.find(k => k.secret === raw);
      if (!key) return err(res, 401, 'Authentication required');
      const payload = await collect(req);
      sendPayload = payload;
      const id = nextId();
      const allowed = channels.filter(ch => ch.owner === key.owner && ch.enabled);
      const destinations = payload.provider_ids
        ? allowed.filter(ch => payload.provider_ids.includes(ch.id))
        : allowed;
      messages.unshift({
        id, owner: key.owner, title: payload.title, body: payload.body, attachments: payload.attachments || [],
        created_at: today,
        deliveries: destinations.map(ch => ({ id: nextId(), provider: ch.name, status: 'sent', attempts: 1, error: null, sent_at: today }))
      });
      return response(res, 202, { id, status: 'queued' });
    }
    if (method === 'GET' && pathname === '/v1/events') {
      return response(res, 200, messages.filter(m => m.owner === current.id).map(m => ({
        id: m.id, title: m.title, created_at: m.created_at, total: m.deliveries.length,
        sent: m.deliveries.filter(d => d.status === 'sent').length, failed: 0
      })));
    }
    const eventMatch = pathname.match(/^\/v1\/events\/([^/]+)$/);
    if (method === 'GET' && eventMatch) {
      const event = messages.find(m => m.id === eventMatch[1] && m.owner === current.id);
      if (!event) return err(res, 404, 'Not found');
      return response(res, 200, { ...event, attachments: event.attachments.map(f => ({ filename: f.filename, content_type: f.content_type })) });
    }
    if (current.role !== 'admin') return err(res, 403, 'Permission denied');
    if (method === 'GET' && pathname === '/v1/admin/provider-catalog') return response(res, 200, kinds);
    const kindMatch = pathname.match(/^\/v1\/admin\/provider-catalog\/([^/]+)$/);
    if (kindMatch && method === 'PATCH') {
      const kind = kinds.find(k => k.kind === kindMatch[1]);
      if (!kind) return err(res, 404, 'Not found');
      kind.enabled = (await collect(req)).enabled;
      return response(res, 200, { kind: kind.kind, enabled: kind.enabled });
    }
    if (method === 'GET' && pathname === '/v1/admin/overview') {
      return response(res, 200, { active_users: users.filter(u => u.active).length, enabled_providers: channels.filter(ch => ch.enabled).length, events_last_24h: messages.length, failed_deliveries: 0 });
    }
    if (method === 'PATCH' && pathname === '/v1/admin/site') {
      site = { ...site, ...await collect(req) };
      return response(res, 200, site);
    }
    if (method === 'GET' && pathname === '/v1/users') {
      return response(res, 200, users.map(({ password, ...u }) => u));
    }
    if (method === 'POST' && pathname === '/v1/users') {
      const input = await collect(req);
      if (users.some(u => u.username === input.username)) return err(res, 400, 'username already exists');
      const u = { id: nextId(), active: true, created_at: today, ...input };
      users.push(u);
      return response(res, 201, { id: u.id, role: u.role });
    }
    const userMatch = pathname.match(/^\/v1\/users\/([^/]+)$/);
    if (userMatch) {
      const u = users.find(user => user.id === userMatch[1]);
      if (!u) return err(res, 404, 'Not found');
      if (method === 'PATCH') {
        Object.assign(u, await collect(req));
        return response(res, 200, { updated: true, id: u.id });
      }
      if (method === 'DELETE') {
        users.splice(users.indexOf(u), 1);
        return response(res, 200, { deleted: true });
      }
    }
    return err(res, 404, 'No matching route');
  } catch (error) {
    return err(res, 500, 'Test server: ' + error.message);
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = 'http://127.0.0.1:' + server.address().port;
const issues = [];
let cases = 0;
let browser;
const ok = (label, condition = true) => { assert(condition, label); cases++; };
const screenshot = async (page, name) => {
  // Preserve the page layout without short-lived operation notifications
  // covering the controls being reviewed.
  await page.evaluate(() => document.getElementById('toastArea')?.replaceChildren());
  return page.screenshot({ path: path.join(shots, name + '.png'), fullPage: true, animations: 'disabled' });
};
const expectNoOverflow = async (page, name) => {
  const size = await page.evaluate(() => ({
    scroll: document.documentElement.scrollWidth, viewport: window.innerWidth
  }));
  if (size.scroll > size.viewport + 2) {
    const overflowing = await page.evaluate(() => [...document.querySelectorAll('body *')].map(el => {
      const bounds = el.getBoundingClientRect();
      return { tag: el.tagName, id: el.id, className: typeof el.className === 'string' ? el.className.slice(0,90) : '',
        right: Math.round(bounds.right), width: Math.round(bounds.width), left: Math.round(bounds.left) };
    }).filter(item => item.right > window.innerWidth + 2 && item.width > 0 && item.left >= -1).slice(0,20));
    console.log('RESPONSIVE OVERFLOW', name, JSON.stringify({ size, overflowing }));
    await screenshot(page, '99-overflow-' + name.replaceAll(' ', '-'));
  }
  ok('No horizontal overflow: ' + name, size.scroll <= size.viewport + 2);
};
const gotoView = async (page, view, mobile = false) => {
  if (mobile) await page.locator('#mobileMenu').click();
  await page.locator('.nav-item[data-view="' + view + '"]').click();
  await page.locator('#view-' + view).waitFor({ state: 'visible' });
  ok('Navigation: ' + view);
};
const login = async (page, username = 'administrator', password = 'admin-password-123') => {
  await page.locator('#loginUsername').fill(username);
  await page.locator('#loginPassword').fill(password);
  await page.locator('#loginForm button[type="submit"]').click();
  await page.locator('#appShell').waitFor({ state: 'visible' });
};
const issuesOnPage = page => {
  page.on('pageerror', error => issues.push(error.message));
  page.on('console', event => {
    if (event.type() === 'error') issues.push('Console: ' + event.text());
  });
};
try {
  browser = await chromium.launch({ headless: true });
  const desktop = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  issuesOnPage(desktop);
  await desktop.goto(base, { waitUntil: 'networkidle' });
  ok('Localized connection failure', await desktop.evaluate(() =>
    deliveryErrorMessage('Connection error: Connection refused (os error 111)')
  ) === '连接失败：目标服务器拒绝连接');
  ok('Localized deadline', await desktop.evaluate(() =>
    deliveryErrorMessage('provider deadline exceeded')
  ).startsWith('连接超时'));
  ok('Registration closed by default', await desktop.locator('#registerTab').isHidden());
  await screenshot(desktop, '01-login-desktop');
  await login(desktop);
  await screenshot(desktop, '02-overview-desktop');
  await expectNoOverflow(desktop, 'desktop overview');

  await gotoView(desktop, 'providers');
  await desktop.locator('#addProviderButton').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'visible' });
  await screenshot(desktop, '03-provider-editor-desktop');
  const schemaFields = {
    telegram: 'bot_token', wecom_app: 'corp_id',
    wecom_webhook: 'webhook_key', email_smtp: 'host', email_resend: 'api_key'
  };
  for (const [kind, field] of Object.entries(schemaFields)) {
    await desktop.locator('#providerKind').selectOption(kind);
    ok('Provider field for ' + kind, await desktop.locator('#field-' + field).isVisible());
    if (kind === 'email_smtp') ok('SMTP proxy disabled', await desktop.locator('#providerMode').isDisabled());
  }
  await desktop.locator('#providerKind').selectOption('telegram');
  await desktop.locator('#providerName').fill('团队 Telegram 通知');
  await desktop.locator('#field-bot_token').fill('123456:TEST_TOKEN');
  await desktop.locator('#field-bot_username').fill('example_notify_bot');
  await desktop.locator('#field-webhook_secret').fill('demo_webhook_secret_12345');
  await desktop.locator('#field-chat_id').fill('123456');
  await desktop.locator('#providerDialog button[type="submit"]').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'hidden' });
  await desktop.locator('#providerCards .provider-card').waitFor();
  ok('Telegram created', channels.length === 1);
  await screenshot(desktop, '04-providers-desktop');

  await desktop.locator('#providerCards .provider-card button:has-text("编辑")').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'visible' });
  ok('Saved secret concealed', await desktop.locator('#field-bot_token').inputValue() === '');
  await desktop.locator('#providerName').fill('团队 Telegram（已更新）');
  await desktop.locator('#field-chat_id').fill('654321');
  await desktop.locator('#providerDialog button[type="submit"]').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'hidden' });
  ok('Edit keeps saved token', channels[0].settings.bot_token === '123456:TEST_TOKEN');
  ok('Edit updates chat ID', channels[0].settings.chat_id === '654321');

  await desktop.locator('#providerCards .provider-card button:has-text("暂停")').click();
  await desktop.locator('#providerCards .provider-card .tag.paused').waitFor();
  ok('Pause provider', channels[0].enabled === false);
  await desktop.locator('#providerCards .provider-card button:has-text("启用")').click();
  await desktop.locator('#providerCards .provider-card .tag:not(.paused)').waitFor();
  ok('Enable provider', channels[0].enabled === true);
  await desktop.locator('#providerCards .provider-card button:has-text("设置 Webhook")').click();
  await desktop.locator('#toastArea .toast').last().waitFor();
  ok('Webhook action invoked', calls.some(call => call.type === 'webhook-set'));
  await desktop.locator('#providerCards .provider-card button:has-text("订阅链接")').click();
  await desktop.locator('#linkDialog').waitFor({ state: 'visible' });
  ok('Telegram subscription link', (await desktop.locator('#linkValue').inputValue()).includes('t.me/'));
  await screenshot(desktop, '05-subscription-dialog-desktop');
  await desktop.locator('#linkDialog [data-close]').first().click();

  await desktop.locator('#addProviderButton').click();
  await desktop.locator('#providerKind').selectOption('email_smtp');
  await desktop.locator('#providerName').fill('办公邮箱');
  await desktop.locator('#field-host').fill('smtp.example.com');
  await desktop.locator('#field-port').fill('465');
  await desktop.locator('#field-tls').selectOption('implicit');
  await desktop.locator('#field-from').fill('sender@example.com');
  await desktop.locator('#field-to').fill('one@example.com, two@example.com');
  await screenshot(desktop, '06-smtp-editor-desktop');
  await desktop.locator('#providerDialog button[type="submit"]').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'hidden' });
  ok('SMTP recipients parsed', channels.find(ch => ch.kind === 'email_smtp')?.settings.to.length === 2);

  await desktop.locator('#addProviderButton').click();
  await desktop.locator('#providerKind').selectOption('email_resend');
  await desktop.locator('#providerName').fill('系统邮件 API');
  await desktop.locator('#field-api_key').fill('re_test_key');
  await desktop.locator('#field-from').fill('notice@example.com');
  await desktop.locator('#field-to').fill('owner@example.com');
  await desktop.locator('#providerDialog button[type="submit"]').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'hidden' });
  ok('Resend provider saved', channels.some(ch => ch.kind === 'email_resend'));

  await desktop.locator('#addProviderButton').click();
  await desktop.locator('#providerKind').selectOption('wecom_webhook');
  await desktop.locator('#providerName').fill('企业微信群通知');
  await desktop.locator('#field-webhook_key').fill('wecom-test-key');
  await desktop.locator('#providerDialog button[type="submit"]').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'hidden' });
  ok('WeCom group robot saved', channels.some(ch => ch.kind === 'wecom_webhook'));

  await desktop.locator('#addProviderButton').click();
  await desktop.locator('#providerKind').selectOption('wecom_app');
  await desktop.locator('#providerName').fill('企业微信应用通知');
  await desktop.locator('#field-corp_id').fill('test_enterprise');
  await desktop.locator('#field-corp_secret').fill('secret-value');
  await desktop.locator('#field-agent_id').fill('1000002');
  await desktop.locator('#field-to_user').fill('zhangsan');
  await desktop.locator('#providerDialog button[type="submit"]').click();
  await desktop.locator('#providerDialog').waitFor({ state: 'hidden' });
  ok('WeCom app saved', channels.some(ch => ch.kind === 'wecom_app' && ch.settings.agent_id === 1000002));
  await screenshot(desktop, '06b-all-provider-types-desktop');
  const wecomCard = desktop.locator('#providerCards .provider-card').filter({ hasText: '企业微信应用通知' });
  await wecomCard.locator('button:has-text("授权链接")').click();
  await desktop.locator('#linkDialog').waitFor({ state: 'visible' });
  ok('WeCom authorization link shown', (await desktop.locator('#linkValue').inputValue()).startsWith('https://open.weixin.qq.com'));
  await screenshot(desktop, '06c-wecom-authorization-desktop');
  await desktop.locator('#linkDialog [data-close]').first().click();

  await gotoView(desktop, 'keys');
  await screenshot(desktop, '07-keys-empty-desktop');
  await desktop.locator('#addKeyButton').click();
  await screenshot(desktop, '08-key-dialog-desktop');
  await desktop.locator('#keyName').fill('服务器监控');
  await desktop.locator('#keyDialog button[type="submit"]').click();
  await desktop.locator('#keyDialog').waitFor({ state: 'hidden' });
  ok('Key created', keys.length === 1);
  ok('Key revealed once', await desktop.locator('#keyReveal').isVisible());
  await desktop.evaluate(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: async text => { window.copiedKey = text; } }
    });
  });
  await desktop.locator('#copyKeyButton').click();
  ok('Copy key action', await desktop.evaluate(() => window.copiedKey) === keys[0].secret);
  await screenshot(desktop, '09-key-created-desktop');

  await desktop.locator('#addKeyButton').click();
  await desktop.locator('#keyName').fill('Server酱兼容应用');
  await desktop.locator('#keyFormat').selectOption('serverchan');
  await desktop.locator('#keyDialog button[type="submit"]').click();
  await desktop.locator('#keyDialog').waitFor({ state: 'hidden' });
  ok('Serverchan key created', keys.some(key => key.secret.startsWith('SCT')));

  await gotoView(desktop, 'send');
  await screenshot(desktop, '10-compose-desktop');
  await desktop.locator('#sendTitle').fill('服务器运行日报');
  await desktop.locator('#sendBody').fill('监控结果已生成，请查看附件。');
  await desktop.locator('#sendFiles').setInputFiles({
    name: 'service-report.txt', mimeType: 'text/plain', buffer: Buffer.from('UI attachment check')
  });
  ok('File selected', (await desktop.locator('#fileSelection').textContent()).includes('service-report.txt'));
  await desktop.locator('[name="sendMode"][value="select"]').check();
  await desktop.locator('#sendForm button[type="submit"]').click();
  ok('No target is rejected', messages.length === 0 && (await desktop.locator('#toastArea').textContent()).includes('请至少选择一个推送渠道'));
  await desktop.locator('[name="targetProvider"]').first().check();
  await screenshot(desktop, '11-compose-with-file-desktop');
  await desktop.locator('#sendForm button[type="submit"]').click();
  await desktop.locator('#view-history').waitFor({ state: 'visible' });
  ok('Message submitted', messages.length === 1);
  ok('Attachment Base64 correct', Buffer.from(sendPayload.attachments[0].data_base64, 'base64').toString() === 'UI attachment check');
  ok('Only one recipient selected', sendPayload.provider_ids?.length === 1);
  await screenshot(desktop, '12-message-history-desktop');

  await gotoView(desktop, 'overview');
  await screenshot(desktop, '13-dashboard-with-data-desktop');
  await gotoView(desktop, 'users');
  await screenshot(desktop, '14-users-desktop');
  await desktop.locator('#addUserButton').click();
  await desktop.locator('#manageUsername').fill('invalid username');
  ok('User editor rejects invalid username', await desktop.locator('#manageUsername').evaluate(el => !el.checkValidity()));
  await desktop.locator('#manageUsername').fill('operational_user');
  await desktop.locator('#managePassword').fill('test-user-password-123');
  await screenshot(desktop, '15-user-editor-desktop');
  await desktop.locator('#userDialog button[type="submit"]').click();
  await desktop.locator('#userDialog').waitFor({ state: 'hidden' });
  ok('Administrator creates user', users.some(u => u.username === 'operational_user'));
  const userRow = desktop.locator('#usersList .data-row').filter({ hasText: 'operational_user' });
  await userRow.locator('button:has-text("编辑")').click();
  await desktop.locator('#manageRole').selectOption('admin');
  await desktop.locator('#managePassword').fill('test-new-password-456');
  await desktop.locator('#userDialog button[type="submit"]').click();
  await desktop.locator('#userDialog').waitFor({ state: 'hidden' });
  ok('Administrator edits permissions', users.find(u => u.username === 'operational_user')?.role === 'admin');
  await userRow.locator('button:has-text("停用")').click();
  ok('Administrator disables user', users.find(u => u.username === 'operational_user')?.active === false);

  await gotoView(desktop, 'site');
  await screenshot(desktop, '16-site-settings-desktop');
  await desktop.locator('#siteName').fill('云端通知中心');
  await desktop.locator('#siteAnnouncementInput').fill('今天的服务运行正常。');
  await desktop.locator('#siteForm button[type="submit"]').click();
  ok('Site settings saved', site.site_name === '云端通知中心');
  const smtpType = desktop.locator('#providerCatalogList .catalog-row').filter({ hasText: 'SMTP' });
  await smtpType.locator('label.switch').click();
  await desktop.waitForFunction(() => [...document.querySelectorAll('#providerCatalogList .catalog-row')].some(row => row.textContent.includes('SMTP') && !row.querySelector('input').checked));
  ok('Provider type disabled by administrator', !kinds.find(k => k.kind === 'email_smtp').enabled);
  await screenshot(desktop, '17-site-types-desktop');
  await smtpType.locator('label.switch').click();
  await desktop.waitForFunction(() => [...document.querySelectorAll('#providerCatalogList .catalog-row')].some(row => row.textContent.includes('SMTP') && row.querySelector('input').checked));
  ok('Provider type reenabled', kinds.find(k => k.kind === 'email_smtp').enabled);

  const first = desktop.locator('#usersList .data-row').filter({ hasText: 'operational_user' });
  await gotoView(desktop, 'users');
  await desktop.evaluate(() => { window.confirm = () => true; });
  await first.locator('button:has-text("删除")').click();
  ok('Administrator deletes user', !users.some(u => u.username === 'operational_user'));

  await gotoView(desktop, 'account');
  await screenshot(desktop, '18-account-desktop');
  await desktop.locator('#currentPassword').fill('admin-password-123');
  await desktop.locator('#newPassword').fill('admin-password-999');
  await desktop.locator('#confirmPassword').fill('different-password-123');
  await desktop.locator('#passwordForm button[type="submit"]').click();
  ok('Password mismatch is rejected', await desktop.locator('#appShell').isVisible() && (await desktop.locator('#toastArea').textContent()).includes('两次输入的新密码不一致'));
  await desktop.locator('#confirmPassword').fill('admin-password-999');
  await desktop.locator('#passwordForm button[type="submit"]').click();
  await desktop.locator('#authScreen').waitFor({ state: 'visible' });
  ok('Password change logs out', await desktop.locator('#appShell').isHidden());
  await login(desktop, 'administrator', 'admin-password-999');
  ok('Login after password change');

  await gotoView(desktop, 'keys');
  await desktop.evaluate(() => { window.confirm = () => false; });
  await desktop.locator('#keysList button:has-text("撤销密钥")').first().click();
  ok('Cancelled key revoke preserves key', keys.length === 2);
  await desktop.evaluate(() => { window.confirm = () => true; });
  for (let i = 0; i < 2; i++) {
    await desktop.locator('#keysList button:has-text("撤销密钥")').first().click();
    await desktop.waitForFunction(remaining => document.querySelectorAll('#keysList button').length <= remaining, 1 - i);
  }
  ok('Keys revoked', keys.length === 0);
  await gotoView(desktop, 'providers');
  await desktop.locator('#providerCards .provider-card').first().locator('button:has-text("删除")').click();
  ok('Provider deleted', channels.length === 4);

  const publicPage = await browser.newPage({ viewport: { width: 1200, height: 850 } });
  issuesOnPage(publicPage);
  await publicPage.goto(base, { waitUntil: 'networkidle' });
  ok('Registration still disabled', await publicPage.locator('#registerTab').isHidden());
  await gotoView(desktop, 'site');
  await desktop.locator('#allowRegistration').check({ force: true });
  await desktop.locator('#siteForm button[type="submit"]').click();
  await publicPage.reload({ waitUntil: 'networkidle' });
  ok('Registration enabled', await publicPage.locator('#registerTab').isVisible());
  await publicPage.locator('#registerTab').click();
  await screenshot(publicPage, '19-registration-desktop');
  await publicPage.locator('#registerUsername').fill('registration_draft');
  await publicPage.locator('#registerPassword').fill('register-password-123');
  await publicPage.locator('#registerConfirm').fill('mismatched-password');
  await publicPage.locator('#registerForm button[type="submit"]').click();
  ok('Registration password mismatch is rejected',
    await publicPage.locator('#authScreen').isVisible() &&
    (await publicPage.locator('#toastArea').textContent()).includes('两次输入的密码不一致'));
  await publicPage.locator('#registerUsername').fill('invalid username');
  ok('Registration username validation', await publicPage.locator('#registerUsername').evaluate(el => !el.checkValidity()));
  await publicPage.locator('#registerUsername').fill('self_registered');
  ok('Valid registration username', await publicPage.locator('#registerUsername').evaluate(el => el.checkValidity()));
  await publicPage.locator('#registerPassword').fill('register-password-123');
  await publicPage.locator('#registerConfirm').fill('register-password-123');
  await publicPage.locator('#registerForm button[type="submit"]').click();
  await publicPage.locator('#appShell').waitFor({ state: 'visible' });
  ok('Public registration creates user', users.some(u => u.username === 'self_registered'));
  ok('Regular user cannot see administrator navigation', await publicPage.locator('.nav-item[data-view="site"]').isHidden());
  await gotoView(publicPage, 'providers');
  ok('Other users channels are private', await publicPage.locator('#providerCards .provider-card').count() === 0);
  await screenshot(publicPage, '20-regular-user-desktop');

  const sizes = [
    { width: 320, height: 740, name: 'small-mobile' },
    { width: 390, height: 844, name: 'mobile' },
    { width: 768, height: 1024, name: 'tablet' },
    { width: 1024, height: 800, name: 'small-desktop' }
  ];
  for (const size of sizes) {
    const page = await browser.newPage({ viewport: { width: size.width, height: size.height }, deviceScaleFactor: 1 });
    issuesOnPage(page);
    await page.goto(base, { waitUntil: 'networkidle' });
    await screenshot(page, '21-login-' + size.name);
    await expectNoOverflow(page, size.name + ' login');
    await login(page, 'administrator', 'admin-password-999');
    await expectNoOverflow(page, size.name + ' dashboard');
    await screenshot(page, '22-overview-' + size.name);
    const compact = size.width <= 900;
    for (const view of ['send', 'providers', 'keys', 'history', 'users', 'site', 'account']) {
      await gotoView(page, view, compact);
      await expectNoOverflow(page, size.name + ' ' + view);
      if (['send', 'providers', 'keys', 'history', 'users', 'site'].includes(view)) {
        await screenshot(page, '23-' + view + '-' + size.name);
      }
    }
    if (compact) {
      await page.locator('#mobileMenu').click();
      ok('Sidebar opens on ' + size.name, await page.locator('#sidebar').evaluate(el => el.classList.contains('open')));
      await screenshot(page, '24-navigation-' + size.name);
      await page.locator('#sidebarShade').click({ position: { x: size.width - 10, y: 200 } });
      ok('Sidebar closes on ' + size.name, !(await page.locator('#sidebar').evaluate(el => el.classList.contains('open'))));
      await gotoView(page, 'providers', compact);
    } else {
      await gotoView(page, 'providers');
    }
    await page.locator('#addProviderButton').click();
    await page.locator('#providerKind').selectOption('email_resend');
    await screenshot(page, '25-provider-dialog-' + size.name);
    await expectNoOverflow(page, size.name + ' provider dialog');
    await page.locator('#providerDialog [data-close]').first().click();
    await page.close();
  }

  console.log('BROWSER RUNTIME ISSUE DETAILS', JSON.stringify(issues));
  ok('No browser runtime errors', issues.length === 0);
  const report = {
    checks: cases,
    screenshots: (await import('node:fs/promises')).readdir(shots).then(files => files.filter(f => f.endsWith('.png')).length),
    page_errors: issues,
    api_actions: calls.length
  };
  console.log('UI COMPLETE:', JSON.stringify({ ...report, screenshots: await report.screenshots }));
  await writeFile(path.join(shots, 'results.json'), JSON.stringify({ ...report, screenshots: await report.screenshots }, null, 2));
} finally {
  if (browser) await browser.close();
  server.close();
}
