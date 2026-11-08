// Local, repeatable end-to-end performance test. Never contacts real providers.
// Start MessagePusher with ALLOW_HTTP_RELAY=1 in an isolated test environment.
import { createServer } from 'node:http';
import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { performance } from 'node:perf_hooks';

const args = Object.fromEntries(process.argv.slice(2).map(arg => {
  const parts = arg.replace(/^--/, '').split('=');
  return [parts[0], parts.slice(1).join('=')];
}));
function positiveInt(name, fallback, maximum) {
  const raw = args[name] || String(fallback);
  const number = Number(raw);
  if (!Number.isInteger(number) || number < 1 || number > maximum) {
    throw new Error('--' + name + ' must be an integer from 1 to ' + maximum);
  }
  return number;
}
const count = positiveInt('messages', 400, 5000);
const concurrency = positiveInt('concurrency', 48, 200);
const providers = positiveInt('providers', 4, 12);
const delayMs = Number(args['relay-delay-ms'] ?? '40');
if (!Number.isFinite(delayMs) || delayMs < 0 || delayMs > 3000) {
  throw new Error('Invalid --relay-delay-ms');
}
const apiUrl = process.env.MP_BENCH_URL || 'http://127.0.0.1:8080';
const username = process.env.MP_BENCH_ADMIN_USERNAME;
const password = process.env.MP_BENCH_ADMIN_PASSWORD;
if (!username || !password) {
  console.error('Set MP_BENCH_ADMIN_USERNAME and MP_BENCH_ADMIN_PASSWORD.');
  process.exit(2);
}
const shared = randomBytes(32).toString('hex');
const relay = { requests: 0, items: 0, active: 0, peak: 0, invalid: 0 };
let server;
let adminToken = '';
let testUserId = '';

function percentile(values, fraction) {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return Number(sorted[Math.min(sorted.length - 1, Math.ceil(fraction * sorted.length) - 1)].toFixed(2));
}
function failure(message) { throw new Error(message); }

async function api(path, method = 'GET', data = null, token = '') {
  const headers = { Accept: 'application/json' };
  if (data !== null) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = 'Bearer ' + token;
  const response = await fetch(new URL(path, apiUrl), {
    method,
    headers,
    body: data === null ? undefined : JSON.stringify(data),
    signal: AbortSignal.timeout(15000)
  });
  const result = await response.json();
  if (!response.ok) failure('MessagePusher API ' + path + ': HTTP ' + response.status);
  return result;
}

function startFakeRelay() {
  server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) {
      chunks.push(chunk);
    }
    const data = Buffer.concat(chunks);
    const timestamp = req.headers['x-mp-timestamp'] || '';
    const signature = req.headers['x-mp-signature'] || '';
    const expected = createHmac('sha256', shared)
      .update(timestamp + '.').update(data).digest('hex');
    const validTime = /^\d{10}$/.test(timestamp) &&
      Math.abs(Date.now() / 1000 - Number(timestamp)) < 120;
    const validSignature = /^[0-9a-f]{64}$/.test(signature) &&
      timingSafeEqual(Buffer.from(signature), Buffer.from(expected));
    if (!validTime || !validSignature) {
      relay.invalid++;
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'Invalid signature' }));
      return;
    }

    try {
      const payload = JSON.parse(data.toString('utf8'));
      if (!Array.isArray(payload.items) || !payload.items.length) failure('Invalid batch');
      relay.requests++;
      relay.items += payload.items.length;
      relay.active++;
      relay.peak = Math.max(relay.peak, relay.active);
      await new Promise(resolve => setTimeout(resolve, delayMs));
      relay.active--;
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        results: payload.items.map(item => ({ id: item.id, ok: true }))
      }));
    } catch {
      relay.active = Math.max(0, relay.active - 1);
      res.writeHead(400, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'Invalid relay request' }));
    }
  });
  return new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
}

async function runConcurrent(total, limit, callback) {
  let index = 0;
  await Promise.all(Array.from({ length: Math.min(total, limit) }, async () => {
    for (;;) {
      const next = index++;
      if (next >= total) return;
      await callback(next);
    }
  }));
}

async function main() {
  await startFakeRelay();
  const relayUrl = 'http://127.0.0.1:' + server.address().port + '/';
  const logged = await api('/v1/login', 'POST', { username, password });
  adminToken = logged.token;
  const user = 'bench_' + randomBytes(6).toString('hex');
  const userPassword = randomBytes(24).toString('hex');
  const created = await api('/v1/users', 'POST', {
    username: user, password: userPassword, role: 'user'
  }, adminToken);
  testUserId = created.id;
  const account = await api('/v1/login', 'POST', {
    username: user, password: userPassword
  });
  const token = account.token;

  const providerIds = [];
  for (let i = 0; i < providers; i++) {
    const item = await api('/v1/providers', 'POST', {
      name: 'Performance test ' + i, kind: 'telegram',
      settings: { bot_token: '123456:FAKE_NOT_USED', chat_id: '123456' },
      delivery: { mode: 'proxy', url: relayUrl, secret: shared }
    }, token);
    providerIds.push(item.id);
  }

  const key = await api('/v1/keys', 'POST', { name: 'Performance test key' }, token);
  const ids = new Array(count);
  const latencies = new Array(count);
  const start = performance.now();
  await runConcurrent(count, concurrency, async index => {
    const began = performance.now();
    const result = await api('/v1/send', 'POST', {
      title: 'Performance message ' + index,
      body: 'MessagePusher isolated performance test.',
      provider_ids: providerIds
    }, key.key);
    if (result.status !== 'queued') failure('Unexpected queue response');
    ids[index] = result.id;
    latencies[index] = performance.now() - began;
  });
  const queued = performance.now();
  const remaining = new Set(ids);
  const deadline = Date.now() + 90000;

  while (remaining.size && Date.now() < deadline) {
    const sample = [...remaining];
    await runConcurrent(sample.length, Math.min(40, concurrency), async i => {
      const detail = await api('/v1/events/' + sample[i], 'GET', null, token);
      if (detail.deliveries.length !== providers) {
        failure('Missing delivery records');
      }
      if (detail.deliveries.every(item => item.status === 'sent')) {
        remaining.delete(sample[i]);
      } else if (detail.deliveries.some(item => item.status === 'failed')) {
        failure('An isolated relay delivery was rejected');
      }
    });
    if (remaining.size) await new Promise(resolve => setTimeout(resolve, 300));
  }

  if (remaining.size) failure(remaining.size + ' messages remained unfinished');
  if (relay.invalid !== 0) failure('Invalid relay request signatures');
  if (relay.items !== count * providers) {
    failure('Incorrect relay item count');
  }

  const completed = performance.now();
  const secondsQueued = (queued - start) / 1000;
  const secondsDelivered = (completed - start) / 1000;
  console.log(JSON.stringify({
    messages: count,
    destinations_per_message: providers,
    deliveries: count * providers,
    client_concurrency: concurrency,
    simulated_relay_delay_ms: delayMs,
    enqueue_duration_seconds: Number(secondsQueued.toFixed(3)),
    enqueue_messages_per_second: Number((count / secondsQueued).toFixed(2)),
    enqueue_latency_ms_p50: percentile(latencies, 0.5),
    enqueue_latency_ms_p95: percentile(latencies, 0.95),
    completion_duration_seconds: Number(secondsDelivered.toFixed(3)),
    verified_deliveries_per_second: Number((count * providers / secondsDelivered).toFixed(2)),
    relay_http_requests: relay.requests,
    average_deliveries_per_relay_request: Number((relay.items / relay.requests).toFixed(2)),
    concurrent_relay_requests_peak: relay.peak,
    successful_deliveries: count * providers
  }, null, 2));
}
try {
  await main();
} catch (error) {
  console.error('Benchmark failed: ' + error.message);
  process.exitCode = 1;
} finally {
  if (testUserId && adminToken) {
    try {
      await api('/v1/users/' + testUserId, 'DELETE', null, adminToken);
    } catch {
      console.error('Temporary benchmark account needs manual cleanup.');
      process.exitCode = 1;
    }
  }
  if (server) await new Promise(resolve => server.close(resolve));
}
