import { createReadStream } from 'node:fs';
import { promises as fs } from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = process.env.WEBOS_ROOT || path.resolve(__dirname, '..');
const webRoot = path.join(rootDir, 'web');
const builtInAppsDir = process.env.WEBOS_APPS_DIR || path.join(webRoot, 'apps');
const dataDir = process.env.WEBOS_DATA_DIR || path.join(rootDir, '.data');
const port = Number(process.env.PORT || 8080);

const mimeTypes = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'application/javascript; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
  ['.txt', 'text/plain; charset=utf-8'],
  ['.wasm', 'application/wasm']
]);

async function ensureLayout() {
  await fs.mkdir(path.join(dataDir, 'shared'), { recursive: true });
  await fs.mkdir(path.join(dataDir, 'apps'), { recursive: true });
}

function json(res, statusCode, payload) {
  res.writeHead(statusCode, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(payload, null, 2));
}

function notFound(res) {
  json(res, 404, { error: 'not_found' });
}

function parseBody(req) {
  return new Promise((resolve, reject) => {
    let body = '';
    req.on('data', chunk => { body += chunk.toString('utf8'); });
    req.on('end', () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(error);
      }
    });
    req.on('error', reject);
  });
}

function safeJoin(base, candidate = '.') {
  const resolved = path.resolve(base, candidate.replace(/^\/+/, ''));
  if (!resolved.startsWith(path.resolve(base))) {
    throw new Error('path_escape');
  }
  return resolved;
}

function appPrivateRoot(appId) {
  if (!/^[a-z0-9-]+$/.test(appId)) {
    throw new Error('invalid_app_id');
  }
  return path.join(dataDir, 'apps', appId);
}

async function listApps() {
  const manifests = [];
  const entries = await fs.readdir(builtInAppsDir, { withFileTypes: true }).catch(() => []);
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const manifestPath = path.join(builtInAppsDir, entry.name, 'manifest.json');
    try {
      const raw = await fs.readFile(manifestPath, 'utf8');
      manifests.push(JSON.parse(raw));
    } catch {
      // skip invalid manifests
    }
  }
  return manifests.sort((a, b) => a.name.localeCompare(b.name));
}

async function handleApi(req, res, url) {
  if (url.pathname === '/api/apps' && req.method === 'GET') {
    return json(res, 200, { apps: await listApps() });
  }

  if (url.pathname === '/api/fs/list' && req.method === 'POST') {
    const body = await parseBody(req);
    const target = resolveFileTarget(body);
    await fs.mkdir(target.root, { recursive: true });
    const entries = await fs.readdir(safeJoin(target.root, body.path || '.'), { withFileTypes: true });
    return json(res, 200, {
      entries: entries.map(entry => ({ name: entry.name, kind: entry.isDirectory() ? 'dir' : 'file' }))
    });
  }

  if (url.pathname === '/api/fs/read' && req.method === 'POST') {
    const body = await parseBody(req);
    const target = resolveFileTarget(body);
    const content = await fs.readFile(safeJoin(target.root, body.path), 'utf8');
    return json(res, 200, { content });
  }

  if (url.pathname === '/api/fs/write' && req.method === 'POST') {
    const body = await parseBody(req);
    const target = resolveFileTarget(body);
    const filePath = safeJoin(target.root, body.path);
    await fs.mkdir(path.dirname(filePath), { recursive: true });
    await fs.writeFile(filePath, body.content ?? '', 'utf8');
    return json(res, 200, { ok: true });
  }

  if (url.pathname === '/api/fs/delete' && req.method === 'POST') {
    const body = await parseBody(req);
    const target = resolveFileTarget(body);
    await fs.rm(safeJoin(target.root, body.path), { recursive: true, force: true });
    return json(res, 200, { ok: true });
  }

  if (url.pathname === '/api/fs/mkdir' && req.method === 'POST') {
    const body = await parseBody(req);
    const target = resolveFileTarget(body);
    await fs.mkdir(safeJoin(target.root, body.path), { recursive: true });
    return json(res, 200, { ok: true });
  }

  if (url.pathname === '/api/wasm/execute' && req.method === 'POST') {
    const body = await parseBody(req);
    return json(res, 202, {
      accepted: true,
      runtime: 'rust-wasmtime-bridge',
      program: body.program || null,
      note: 'Hook this endpoint to the runtime binary or daemon for production execution.'
    });
  }

  return notFound(res);
}

function resolveFileTarget(body) {
  if (body.scope === 'shared') {
    return { root: path.join(dataDir, 'shared') };
  }
  if (body.scope === 'app' && body.appId) {
    return { root: appPrivateRoot(body.appId) };
  }
  throw new Error('invalid_scope');
}

async function serveStatic(req, res, url) {
  const requested = url.pathname === '/' ? '/index.html' : url.pathname;
  const filePath = safeJoin(webRoot, requested);
  try {
    const stat = await fs.stat(filePath);
    if (stat.isDirectory()) {
      return serveFile(res, path.join(filePath, 'index.html'));
    }
    return serveFile(res, filePath);
  } catch {
    return notFound(res);
  }
}

function serveFile(res, filePath) {
  const ext = path.extname(filePath);
  res.writeHead(200, { 'Content-Type': mimeTypes.get(ext) || 'application/octet-stream' });
  createReadStream(filePath).pipe(res);
}

async function main() {
  await ensureLayout();
  const server = http.createServer(async (req, res) => {
    try {
      const url = new URL(req.url, `http://${req.headers.host}`);
      if (url.pathname.startsWith('/api/')) {
        return await handleApi(req, res, url);
      }
      return await serveStatic(req, res, url);
    } catch (error) {
      return json(res, 400, { error: error.message || 'bad_request' });
    }
  });

  server.listen(port, '127.0.0.1', () => {
    console.log(`WasmOS backend listening on http://127.0.0.1:${port}`);
  });
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
