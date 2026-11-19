// 检查仓库 Markdown 文档中的本地文件链接、章节链接和代码围栏。
import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const skipped = new Set(['.git', 'node_modules', 'target', 'dist', 'backups', '.next', 'screenshots']);
const problems = [];
let linksChecked = 0;
const headingsCache = new Map();

async function listMarkdown(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (skipped.has(entry.name)) continue;
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await listMarkdown(fullPath));
    else if (entry.isFile() && entry.name.endsWith('.md')) result.push(fullPath);
  }
  return result;
}

function visibleLines(content, file) {
  const lines = content.split(/\r?\n/);
  const visible = [];
  let fence = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const marker = line.match(/^\s{0,3}([`~])\1{2,}/);
    if (!fence) {
      if (marker) {
        fence = { char: marker[1], length: marker[0].trim().length, line: i + 1 };
      } else {
        visible.push({ line, number: i + 1 });
      }
    } else if (marker && marker[1] === fence.char && marker[0].trim().length >= fence.length
        && [...line.trim()].every(char => char === fence.char)) {
      fence = null;
    }
  }
  if (fence) problems.push(file + ':' + fence.line + ': 代码围栏没有结束');
  return visible;
}

function slug(text) {
  return text
    .replace(/<[^>]+>/g, '')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[`*_~]/g, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, '')
    .trim().replace(/\s+/g, '-');
}

async function headingIds(filename) {
  if (headingsCache.has(filename)) return headingsCache.get(filename);
  const content = await readFile(filename, 'utf8');
  const counts = new Map();
  const all = new Set();
  for (const { line } of visibleLines(content, path.relative(root, filename))) {
    const match = line.match(/^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/);
    if (!match) continue;
    const name = slug(match[1]);
    const index = counts.get(name) || 0;
    counts.set(name, index + 1);
    all.add(index ? name + '-' + index : name);
  }
  headingsCache.set(filename, all);
  return all;
}

async function inspect(file) {
  const name = path.relative(root, file);
  const content = await readFile(file, 'utf8');
  for (const { line, number } of visibleLines(content, name)) {
    // 代码字面量里的链接示例不属于文档引用。
    const prose = line.replace(/`[^`]*`/g, '');
    for (const match of prose.matchAll(/!?\[[^\]\n]*\]\(([^)\n]+)\)/g)) {
      const raw = match[1].trim().split(/\s+"/)[0].replace(/^<|>$/g, '');
      if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(raw)) continue;
      const [target, fragment] = raw.split('#', 2);
      let decoded;
      try { decoded = decodeURIComponent(target.split('?')[0]); }
      catch { problems.push(name + ':' + number + ': 无法解码链接 ' + raw); continue; }
      const destination = decoded ? path.resolve(path.dirname(file), decoded) : file;
      if (destination !== root && !destination.startsWith(root + path.sep)) {
        problems.push(name + ':' + number + ': 链接超出了仓库目录 ' + raw);
        continue;
      }
      let details;
      try { details = await stat(destination); }
      catch { problems.push(name + ':' + number + ': 本地链接的目标不存在 ' + raw); continue; }
      linksChecked++;
      if (fragment && details.isFile() && destination.endsWith('.md')) {
        let anchor;
        try { anchor = decodeURIComponent(fragment).toLowerCase(); }
        catch { problems.push(name + ':' + number + ': 无法解码章节链接 ' + raw); continue; }
        if (!/^l\d+(?:-l\d+)?$/.test(anchor) && !(await headingIds(destination)).has(anchor)) {
          problems.push(name + ':' + number + ': 找不到目标章节 ' + raw);
        }
      }
    }
  }
}

const docs = await listMarkdown(root);
for (const doc of docs) await inspect(doc);
if (problems.length) {
  for (const problem of problems) console.error(problem);
  console.error('文档检查未通过，共 ' + problems.length + ' 项需要处理。');
  process.exitCode = 1;
} else {
  console.log('文档检查通过：' + docs.length + ' 个 Markdown 文件，' + linksChecked + ' 个本地链接。');
}
