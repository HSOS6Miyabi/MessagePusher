// Tests the real Rust application, PostgreSQL queue and Telegram Bot API.
// Credentials must be supplied through environment variables and are never printed.
import { randomBytes } from 'node:crypto';

const root = process.env.E2E_BASE_URL || 'http://127.0.0.1:8080';
const user = process.env.E2E_ADMIN_USERNAME;
const password = process.env.E2E_ADMIN_PASSWORD;
const botToken = process.env.E2E_BOT_TOKEN;
const relayUrl = process.env.E2E_RELAY_URL || '';
const relaySecret = process.env.E2E_RELAY_SECRET || '';
if (!user || !password || !botToken) {
  console.error('Set E2E_ADMIN_USERNAME, E2E_ADMIN_PASSWORD and E2E_BOT_TOKEN.');
  process.exit(2);
}
const telegramBase = 'https://api.telegram.org/bot' + botToken;
const labels = [];
let adminSession = '';
let createdUserId = '';

function assert(value, label) {
  if (!value) throw new Error(label + ' did not succeed');
}
function done(label) {
  labels.push(label);
  console.log('PASS ' + label);
}
async function api(path, method = 'GET', data = null, token = '') {
  const headers = { Accept: 'application/json' };
  if (data !== null) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = 'Bearer ' + token;
  const response = await fetch(new URL(path, root), {
    method, headers, body: data === null ? undefined : JSON.stringify(data)
  });
  const value = await response.json();
  if (!response.ok) throw new Error('MessagePusher HTTP ' + response.status + ' on ' + path);
  return value;
}
async function telegram(method, data = {}) {
  const response = await fetch(telegramBase + '/' + method, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data)
  });
  const result = await response.json();
  assert(result.ok, 'Telegram ' + method);
  return result.result;
}
async function obtainChatId() {
  if (process.env.E2E_CHAT_ID) return process.env.E2E_CHAT_ID;
  const webhook = await telegram('getWebhookInfo');
  if (webhook.url) throw new Error('Set E2E_CHAT_ID when the Bot has an active Webhook');
  const updates = await telegram('getUpdates', { timeout: 0, limit: 100 });
  const message = updates.filter(item => item.message?.chat?.type === 'private').at(-1)?.message;
  assert(message?.chat?.id, 'A private Telegram chat must exist');
  return String(message.chat.id);
}
async function awaitSent(id, session) {
  const end = Date.now() + 25000;
  do {
    const detail = await api('/v1/events/' + id, 'GET', null, session);
    if (detail.deliveries.length > 0 && detail.deliveries.every(item => item.status === 'sent')) return;
    if (detail.deliveries.some(item => item.status === 'failed')) {
      throw new Error('A Telegram delivery was rejected');
    }
    await new Promise(resolve => setTimeout(resolve, 350));
  } while (Date.now() < end);
  throw new Error('Telegram delivery did not finish within the test time limit');
}
async function send(session, sendKey, providerIds, title, files = []) {
  const result = await api('/v1/send', 'POST', {
    title, body: 'MessagePusher integrated Telegram test.',
    provider_ids: providerIds, attachments: files
  }, sendKey);
  assert(result.status === 'queued', 'A message must be accepted into the database');
  await awaitSent(result.id, session);
}
async function test() {
  const chatId = await obtainChatId();
  const identity = await telegram('getMe');
  assert(identity.is_bot === true, 'Telegram bot identity');
  done('Telegram bot identity and recipient');
  assert((await api('/healthz')).ok === true, 'main program health');
  const admin = await api('/v1/login', 'POST', { username: user, password });
  adminSession = admin.token;
  const name = 'tg_e2e_' + randomBytes(5).toString('hex');
  const userPass = randomBytes(20).toString('hex');
  const account = await api('/v1/users', 'POST', {
    username: name, password: userPass, role: 'user'
  }, adminSession);
  createdUserId = account.id;
  const loggedIn = await api('/v1/login', 'POST', { username: name, password: userPass });
  const session = loggedIn.token;
  const own = await api('/v1/me', 'GET', null, session);
  assert(own.role === 'user', 'normal user identity');
  done('User creation and login');
  const direct = await api('/v1/providers', 'POST', {
    name: 'Telegram direct integration',
    kind: 'telegram',
    settings: { bot_token: botToken, chat_id: chatId },
    delivery: { mode: 'direct' }
  }, session);
  const key = await api('/v1/keys', 'POST', { name: 'Telegram E2E test' }, session);
  done('Personal Telegram provider and sending key');
  await send(session, key.key, [direct.id], 'Telegram direct message test');
  done('Rust queue to Telegram text notification');
  await send(session, key.key, [direct.id], 'Telegram file delivery test', [{
    filename: 'messagepusher-test.txt',
    content_type: 'text/plain',
    data_base64: Buffer.from('MessagePusher document verification\n').toString('base64')
  }]);
  done('Rust queue to Telegram attachment');
  const secret = randomBytes(24).toString('hex');
  const provider = await api('/v1/providers', 'POST', {
    name: 'Telegram subscription integration',
    kind: 'telegram',
    settings: { bot_token: botToken, bot_username: identity.username, webhook_secret: secret },
    delivery: { mode: 'direct' }
  }, session);
  const subscription = await api('/v1/providers/' + provider.id + '/telegram/subscribe', 'POST', {}, session);
  const code = new URL(subscription.url).searchParams.get('start');
  assert(code && code.length <= 64, 'Telegram subscription link');
  const path = '/hooks/telegram/' + provider.id;
  const fakeUpdate = { message: { text: '/start ' + code, chat: { type: 'private', id: Number(chatId) } } };
  const rejected = await fetch(new URL(path, root), {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(fakeUpdate)
  });
  assert(rejected.status === 401, 'Webhook must require its secret header');
  const accepted = await fetch(new URL(path, root), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Telegram-Bot-Api-Secret-Token': secret },
    body: JSON.stringify(fakeUpdate)
  });
  assert(accepted.ok, 'Validated Webhook subscription callback');
  const providers = await api('/v1/providers', 'GET', null, session);
  assert(providers.find(item => item.id === provider.id)?.subscribed, 'Chat ID must be stored');
  done('Telegram subscription callback and database binding');
  await send(session, key.key, [provider.id], 'Telegram subscribed channel message test');
  done('Sending through the subscribed provider');
  if (relayUrl && relaySecret) {
    const proxied = await api('/v1/providers', 'POST', {
      name: 'Telegram proxy integration',
      kind: 'telegram',
      settings: { bot_token: botToken, chat_id: chatId },
      delivery: { mode: 'proxy', url: relayUrl, secret: relaySecret }
    }, session);
    await send(session, key.key, [proxied.id], 'Telegram proxy message test');
    done('Signed relay delivery through Telegram');
  }
}
try {
  await test();
  console.log('Telegram end-to-end verification passed: ' + labels.length + ' checks.');
} catch (error) {
  console.error('Verification stopped: ' + error.message);
  process.exitCode = 1;
} finally {
  if (createdUserId && adminSession) {
    try {
      await api('/v1/users/' + createdUserId, 'DELETE', null, adminSession);
      console.log('Temporary test account deleted.');
    } catch {
      console.error('Temporary test account needs manual cleanup.');
      process.exitCode = 1;
    }
  }
}
