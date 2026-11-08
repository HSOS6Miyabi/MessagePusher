const encoder = new TextEncoder();

function sendResult(status, body) { return { status, body }; }
function getStr(object, key, required = true) {
  const value = object?.[key];
  if (typeof value === 'string' && value.length > 0) return value;
  if (!required) return '';
  throw new Error('Missing provider setting: ' + key);
}
function configText(settings, key) {
  const value = settings?.[key];
  return typeof value === 'number' ? String(value) : getStr(settings, key);
}
function checkKey(key) {
  if (!/^[0-9]+:[A-Za-z0-9_-]+$/.test(key)) throw new Error('Invalid Telegram bot token');
  return key;
}
function bytesFromBase64(content) {
  if (typeof content !== 'string' || !/^[A-Za-z0-9+/]*={0,2}$/.test(content)) throw new Error('Invalid attachment encoding');
  const binary = atob(content);
  const output = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) output[i] = binary.charCodeAt(i);
  return output;
}
function checkItems(items) {
  if (!Array.isArray(items) || items.length === 0 || items.length > 50) throw new Error('Expected 1 to 50 items');
  for (const item of items) {
    if (!item || typeof item.id !== 'string' || typeof item.kind !== 'string'
      || !item.settings || typeof item.settings !== 'object' || typeof item.title !== 'string'
      || typeof item.body !== 'string' || !Array.isArray(item.attachments) || item.attachments.length > 5) {
      throw new Error('Invalid delivery item');
    }
    let total = 0;
    for (const file of item.attachments) {
      if (!file || typeof file.filename !== 'string' || !file.filename ||
        file.filename.length > 120 || /[/\\\x00]/.test(file.filename) ||
        typeof file.content_type !== 'string' || typeof file.data_base64 !== 'string') {
        throw new Error('Invalid attachment');
      }
      total += bytesFromBase64(file.data_base64).length;
    }
    if (total > 2 * 1024 * 1024) throw new Error('Attachments exceed 2 MiB');
  }
}
async function signature(secret, raw, timestamp) {
  const key = await crypto.subtle.importKey('raw', encoder.encode(secret),
    { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  const digest = new Uint8Array(await crypto.subtle.sign('HMAC', key, encoder.encode(timestamp + '.' + raw)));
  return Array.from(digest, b => b.toString(16).padStart(2, '0')).join('');
}
function constantEqual(a, b) {
  if (typeof a !== 'string' || typeof b !== 'string' || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}
async function authorized(headers, raw, env) {
  const timestamp = headers.get('x-mp-timestamp');
  const digest = headers.get('x-mp-signature');
  if (!timestamp || !/^\d{10}$/.test(timestamp) || !digest || !/^[a-f0-9]{64}$/.test(digest)) return false;
  if (Math.abs(Date.now() / 1000 - Number(timestamp)) > 120) return false;
  const secrets = String(env.RELAY_SECRETS || env.RELAY_SECRET || '').split(',')
    .map(x => x.trim()).filter(x => x.length >= 24);
  if (!secrets.length) return false;
  let match = false;
  for (const secret of secrets) {
    const expected = await signature(secret, raw, timestamp);
    match = constantEqual(expected, digest) || match;
  }
  return match;
}
async function checked(url, options, kind = 'ok') {
  const response = await fetch(url, options);
  if (!response.ok) throw new Error('Provider HTTP ' + response.status);
  const data = await response.json();
  if (kind === 'ok' && data.ok !== true) throw new Error('Telegram API error ' + (data.error_code || 'unknown'));
  if (kind === 'errcode' && data.errcode !== 0) throw new Error('WeCom API error ' + (data.errcode ?? 'unknown'));
  return data;
}
function deadline() { return AbortSignal.timeout(24000); }
async function telegram(item, signal) {
  const token = checkKey(getStr(item.settings, 'bot_token'));
  const chatId = configText(item.settings, 'chat_id');
  const text = (item.title ? item.title + '\n\n' : '') + item.body;
  const base = 'https://api.telegram.org/bot' + token;
  const characters = Array.from(text);
  for (let offset = 0; offset < characters.length; offset += 4000) {
    await checked(base + '/sendMessage', {
      method: 'POST', signal, headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ chat_id: chatId, text: characters.slice(offset, offset + 4000).join('') })
    });
  }
  for (const file of item.attachments) {
    const form = new FormData();
    form.set('chat_id', chatId);
    form.set('document', new Blob([bytesFromBase64(file.data_base64)], { type: file.content_type }), file.filename);
    await checked(base + '/sendDocument', { method: 'POST', body: form, signal });
  }
}
async function telegramWebhook(item, signal) {
  const token = checkKey(getStr(item.settings, 'bot_token'));
  const url = new URL(item.title);
  if (url.protocol !== 'https:') throw new Error('Telegram webhook must be HTTPS');
  const secret = getStr(item.settings, 'webhook_secret');
  if (secret.length < 16 || secret.length > 256 || !/^[A-Za-z0-9_-]+$/.test(secret)) {
    throw new Error('Invalid Telegram webhook secret');
  }
  await checked('https://api.telegram.org/bot' + token + '/setWebhook', {
    method: 'POST', signal, headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ url: url.toString(), secret_token: secret, allowed_updates: ['message'] })
  });
}
function wecomKey(settings) { return getStr(settings, 'webhook_key'); }
async function wecomToken(settings, signal) {
  const params = new URLSearchParams({ corpid: getStr(settings, 'corp_id'), corpsecret: getStr(settings, 'corp_secret') });
  const json = await checked('https://qyapi.weixin.qq.com/cgi-bin/gettoken?' + params, { signal }, 'errcode');
  return getStr(json, 'access_token');
}
async function wecomUpload(url, file, signal) {
  const form = new FormData();
  form.set('media', new Blob([bytesFromBase64(file.data_base64)], { type: file.content_type }), file.filename);
  const json = await checked(url, { method: 'POST', body: form, signal }, 'errcode');
  return getStr(json, 'media_id');
}
function utf8Chunks(text, maximum) {
  const chunks = [];
  let part = '';
  let length = 0;
  for (const char of text) {
    const bytes = encoder.encode(char).length;
    if (length + bytes > maximum && part) {
      chunks.push(part);
      part = '';
      length = 0;
    }
    part += char;
    length += bytes;
  }
  if (part) chunks.push(part);
  return chunks;
}
async function wecomApp(item, signal) {
  const token = await wecomToken(item.settings, signal);
  const recipient = getStr(item.settings, 'to_user');
  const agentId = Number(configText(item.settings, 'agent_id'));
  if (!Number.isSafeInteger(agentId) || agentId <= 0) throw new Error('Invalid WeCom agent_id');
  const sendUrl = 'https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token=' + encodeURIComponent(token);
  for (const part of utf8Chunks(item.title + '\n' + item.body, 1900)) {
    await checked(sendUrl, {
      method: 'POST', signal, headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ touser: recipient, agentid: agentId, msgtype: 'text', text: { content: part } })
    }, 'errcode');
  }
  for (const file of item.attachments) {
    const uploadUrl = 'https://qyapi.weixin.qq.com/cgi-bin/media/upload?access_token=' + encodeURIComponent(token) + '&type=file';
    const mediaId = await wecomUpload(uploadUrl, file, signal);
    await checked(sendUrl, {
      method: 'POST', signal, headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ touser: recipient, agentid: agentId, msgtype: 'file', file: { media_id: mediaId } })
    }, 'errcode');
  }
}
async function wecomWebhook(item, signal) {
  const key = wecomKey(item.settings);
  const url = 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=' + encodeURIComponent(key);
  for (const part of utf8Chunks(item.title + '\n' + item.body, 1900)) {
    await checked(url, {
      method: 'POST', signal, headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ msgtype: 'text', text: { content: part } })
    }, 'errcode');
  }
  for (const file of item.attachments) {
    const mediaId = await wecomUpload('https://qyapi.weixin.qq.com/cgi-bin/webhook/upload_media?key=' +
      encodeURIComponent(key) + '&type=file', file, signal);
    await checked(url, {
      method: 'POST', signal, headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ msgtype: 'file', file: { media_id: mediaId } })
    }, 'errcode');
  }
}
function recipients(settings) {
  const to = settings.to;
  const list = typeof to === 'string' ? [to] : to;
  if (!Array.isArray(list) || list.length === 0 || list.length > 30 || list.some(x => typeof x !== 'string' || !x.includes('@'))) {
    throw new Error('Invalid email recipients');
  }
  return list;
}
async function resend(item, signal) {
  const response = await fetch('https://api.resend.com/emails', {
    method: 'POST', signal,
    headers: { authorization: 'Bearer ' + getStr(item.settings, 'api_key'), 'content-type': 'application/json' },
    body: JSON.stringify({
      from: getStr(item.settings, 'from'), to: recipients(item.settings),
      subject: item.title || 'Notification', text: item.body || ' ',
      attachments: item.attachments.map(file => ({
        filename: file.filename, content: file.data_base64, content_type: file.content_type
      }))
    })
  });
  if (!response.ok) throw new Error('Resend HTTP ' + response.status);
}
async function deliver(item) {
  const signal = deadline();
  switch (item.kind) {
    case 'telegram': return telegram(item, signal);
    case 'telegram_webhook_setup': return telegramWebhook(item, signal);
    case 'wecom_app': return wecomApp(item, signal);
    case 'wecom_webhook': return wecomWebhook(item, signal);
    case 'email_resend': return resend(item, signal);
    default: throw new Error('Unsupported relay provider: ' + item.kind);
  }
}
export async function handle(request, env) {
  if (request.method === 'GET' && (request.path === '/healthz' || request.path === '/api/index' || request.path === '/')) {
    return sendResult(200, { ok: true, service: 'messagepusher-relay' });
  }
  if (request.method !== 'POST' || !['/', '/api/index'].includes(request.path)) {
    return sendResult(404, { error: 'Not found' });
  }
  if (encoder.encode(request.raw).length > 3_500_000) return sendResult(413, { error: 'Request too large' });
  if (!await authorized(request.headers, request.raw, env)) return sendResult(401, { error: 'Invalid relay signature' });
  let body;
  try {
    body = JSON.parse(request.raw);
    checkItems(body.items);
  } catch (error) {
    return sendResult(400, { error: String(error.message || 'Invalid request') });
  }
  const results = await Promise.all(body.items.map(async item => {
    try {
      await deliver(item);
      return { id: item.id, ok: true };
    } catch (error) {
      // Never include the secret provider request URL or credential in the error response.
      const description = String(error.message || 'Provider request failed').slice(0, 160);
      return { id: item.id, ok: false, error: description };
    }
  }));
  return sendResult(200, { results });
}
