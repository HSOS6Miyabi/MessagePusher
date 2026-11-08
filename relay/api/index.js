import { handle } from '../core.js';

export const config = { api: { bodyParser: false } };

export default async function handler(req, res) {
  let raw = '';
  if (req.method === 'POST') {
    if (typeof req.body === 'string') {
      raw = req.body;
    } else if (req.body && typeof req.body === 'object') {
      raw = JSON.stringify(req.body);
    } else {
      const chunks = [];
      for await (const chunk of req) chunks.push(Buffer.from(chunk));
      raw = Buffer.concat(chunks).toString('utf8');
    }
  }
  const result = await handle({
    method: req.method,
    path: new URL(req.url || '/', 'https://relay.invalid').pathname,
    headers: new Headers(req.headers),
    raw
  }, process.env);
  res.statusCode = result.status;
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.setHeader('Cache-Control', 'no-store');
  res.end(JSON.stringify(result.body));
}
