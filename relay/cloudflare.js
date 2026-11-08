import { handle } from './core.js';

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const raw = request.method === 'POST' ? await request.text() : '';
    const result = await handle({
      method: request.method,
      path: url.pathname,
      headers: request.headers,
      raw
    }, env);
    return new Response(JSON.stringify(result.body), {
      status: result.status,
      headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' }
    });
  }
};
