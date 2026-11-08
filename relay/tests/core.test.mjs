import test from 'node:test';
import assert from 'node:assert/strict';
import { handle } from '../core.js';

const secret = 'long-relay-secret-1234567890123456789';
const headers = new Headers();
const item = (id, kind, settings, attachments = []) => ({
  id, kind, settings, title: 'Deploy complete', body: 'All checks passed', attachments
});
async function signed(raw, timestamp = String(Math.floor(Date.now() / 1000))) {
  const key = await crypto.subtle.importKey('raw', new TextEncoder().encode(secret),
    { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const signature = new Uint8Array(await crypto.subtle.sign('HMAC', key,
    new TextEncoder().encode(timestamp + '.' + raw)));
  const hex = Array.from(signature, b => b.toString(16).padStart(2, '0')).join('');
  return new Headers({ 'x-mp-timestamp': timestamp, 'x-mp-signature': hex });
}
async function request(items, overrides = {}) {
  const raw = JSON.stringify({ items });
  return handle({ method: 'POST', path: '/', headers: await signed(raw), raw, ...overrides }, { RELAY_SECRET: secret });
}
test('health does not require credentials', async () => {
  const response = await handle({ method: 'GET', path: '/healthz', headers, raw: '' }, {});
  assert.equal(response.status, 200);
  assert.equal(response.body.ok, true);
});
test('reject invalid and expired signatures', async () => {
  const response = await handle({ method: 'POST', path: '/', headers, raw: '{"items":[]}' }, { RELAY_SECRET: secret });
  assert.equal(response.status, 401);
  const raw = '{"items":[]}';
  const stale = await handle({ method: 'POST', path: '/', headers: await signed(raw, '1500000000'), raw }, { RELAY_SECRET: secret });
  assert.equal(stale.status, 401);
});
test('multiple independent providers run and results remain separated', async () => {
  const seen = [];
  const before = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    seen.push([String(url), init?.method || 'GET']);
    if (String(url).includes('sendMessage')) return Response.json({ ok: true });
    if (String(url).includes('gettoken')) return Response.json({ errcode: 0, access_token: 'abc' });
    if (String(url).includes('message/send')) return Response.json({ errcode: 0 });
    return Response.json({ id: 'email-id' });
  };
  try {
    const response = await request([
      item('a', 'telegram', { bot_token: '123:abc_DEF', chat_id: '456' }),
      item('b', 'email_resend', { api_key: 're_test', from: 'me@example.com', to: ['you@example.com'] }),
      item('c', 'wecom_app', { corp_id: 'corp', corp_secret: 'secret', agent_id: 123, to_user: 'zhang' })
    ]);
    assert.equal(response.status, 200);
    assert.deepEqual(response.body.results.map(r => r.ok), [true, true, true]);
    assert.equal(seen.length, 4);
    assert.ok(seen.some(([url]) => url.includes('api.resend.com')));
  } finally {
    globalThis.fetch = before;
  }
});
test('attachment is sent using multipart/form-data', async () => {
  const before = globalThis.fetch;
  let hasDocument = false;
  globalThis.fetch = async (url, init) => {
    if (String(url).includes('sendDocument')) {
      hasDocument = init.body instanceof FormData && init.body.has('document');
    }
    return Response.json({ ok: true });
  };
  try {
    const response = await request([item('file', 'telegram',
      { bot_token: '123:abc_DEF', chat_id: '456' },
      [{ filename: 'log.txt', content_type: 'text/plain', data_base64: 'dGVzdA==' }])]);
    assert.equal(response.body.results[0].ok, true);
    assert.equal(hasDocument, true);
  } finally {
    globalThis.fetch = before;
  }
});
test('one provider failure does not block others', async () => {
  const before = globalThis.fetch;
  globalThis.fetch = async (url) =>
    String(url).includes('sendMessage') ? Response.json({ ok: false, error_code: 403 }) : Response.json({ id: 'email-id' });
  try {
    const result = await request([
      item('a', 'telegram', { bot_token: '123:abc_DEF', chat_id: '456' }),
      item('b', 'email_resend', { api_key: 're_test', from: 'me@example.com', to: 'you@example.com' })
    ]);
    assert.deepEqual(result.body.results.map(r => r.ok), [false, true]);
  } finally { globalThis.fetch = before; }
});

test('Telegram webhook configuration registers message updates with a valid secret', async () => {
  const original = globalThis.fetch;
  let captured;
  globalThis.fetch = async (url, options) => {
    captured = { url: String(url), body: JSON.parse(options.body) };
    return Response.json({ ok: true, result: true });
  };
  try {
    const result = await request([item('webhook', 'telegram_webhook_setup',
      { bot_token: '123:abc_DEF', webhook_secret: 'secure_webhook_12345678' })].map(x => ({
      ...x, title: 'https://push.example.com/hooks/telegram/test'
    })));
    assert.equal(result.body.results[0].ok, true);
    assert.ok(captured.url.endsWith('/setWebhook'));
    assert.deepEqual(captured.body.allowed_updates, ['message']);
    assert.equal(captured.body.secret_token, 'secure_webhook_12345678');
  } finally { globalThis.fetch = original; }
});
test('Telegram webhook setup rejects invalid secret before making network requests', async () => {
  const result = await request([{...item('webhook', 'telegram_webhook_setup',
    {bot_token:'123:abc_DEF',webhook_secret:'bad/value'}),
    title:'https://push.example.com/hooks/telegram/test'}]);
  assert.equal(result.body.results[0].ok, false);
  assert.match(result.body.results[0].error,/secret/i);
});
