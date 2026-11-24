import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PORT = Number.parseInt(process.env.PORT || '3000', 10);
const HOST = '0.0.0.0';
const STATIC_ROOT = path.resolve(__dirname, 'static');
const DOCS_ROOT = path.resolve(__dirname, 'docs');

const CONTENT_TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.woff': 'font/woff',
  '.ttf': 'font/ttf',
};

const SECURITY_HEADERS = {
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
  'Content-Security-Policy':
    "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'",
};

function escapeHtml(text) {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function safeResolve(rootDir, relativePath) {
  if (!relativePath || relativePath.includes('\0') || relativePath.includes('\\')) {
    return null;
  }
  const normalized = path.posix.normalize('/' + relativePath).replace(/^\/+/, '');
  const resolved = path.resolve(rootDir, normalized);
  if (resolved !== rootDir && !resolved.startsWith(rootDir + path.sep)) {
    return null;
  }
  return resolved;
}

async function serveFile(res, filePath) {
  try {
    const stat = await fs.stat(filePath);
    if (!stat.isFile()) {
      res.writeHead(404, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
      res.end('Nicht gefunden');
      return;
    }
    const ext = path.extname(filePath).toLowerCase();
    const contentType = CONTENT_TYPES[ext] || 'application/octet-stream';
    const data = await fs.readFile(filePath);
    res.writeHead(200, {
      ...SECURITY_HEADERS,
      'Content-Type': contentType,
      'Cache-Control': 'no-cache, must-revalidate',
    });
    res.end(data);
  } catch {
    res.writeHead(404, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('Nicht gefunden');
  }
}

async function listDocsIndex(res) {
  try {
    const entries = await fs.readdir(DOCS_ROOT, { withFileTypes: true });
    const mdFiles = entries
      .filter((e) => e.isFile() && e.name.endsWith('.md'))
      .map((e) => e.name)
      .sort();

    let abnahmeFiles = [];
    try {
      const sub = await fs.readdir(path.join(DOCS_ROOT, 'abnahme'), { withFileTypes: true });
      abnahmeFiles = sub
        .filter((e) => e.isFile() && e.name.endsWith('.md'))
        .map((e) => `abnahme/${e.name}`)
        .sort();
    } catch {
      abnahmeFiles = [];
    }

    const allFiles = [...mdFiles, ...abnahmeFiles];
    const items = allFiles
      .map((name) => `<li><a href="/docs/${encodeURI(name)}">${escapeHtml(name)}</a></li>`)
      .join('\n');

    const html = `<!DOCTYPE html>
<html lang="de" data-theme="dark">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>KIS — Dokumentation</title>
  <link rel="stylesheet" href="/static/css/style.css">
</head>
<body class="docs-body">
  <header class="topbar">
    <h1 class="topbar-title">KIS — Architektur- &amp; EP-00-Dokumentation</h1>
    <div class="topbar-actions"><a href="/" class="btn btn-primary">Zur KIS-Oberfläche</a></div>
  </header>
  <main class="content">
    <p>Schreibgeschützte Vorschau der Markdown-Dokumentation aus <code>docs/</code>.</p>
    <ul>${items}</ul>
  </main>
</body>
</html>`;

    res.writeHead(200, {
      ...SECURITY_HEADERS,
      'Content-Type': 'text/html; charset=utf-8',
      'Cache-Control': 'no-cache, must-revalidate',
    });
    res.end(html);
  } catch {
    res.writeHead(500, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('Interner Fehler beim Lesen von docs/');
  }
}

async function serveMarkdownDoc(res, relPath) {
  const resolved = safeResolve(DOCS_ROOT, relPath);
  if (!resolved || !resolved.endsWith('.md')) {
    res.writeHead(400, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('Ungültiger Pfad');
    return;
  }
  try {
    const raw = await fs.readFile(resolved, 'utf8');
    const html = `<!DOCTYPE html>
<html lang="de" data-theme="dark">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>KIS — ${escapeHtml(relPath)}</title>
  <link rel="stylesheet" href="/static/css/style.css">
</head>
<body>
  <header class="topbar">
    <h1 class="topbar-title">${escapeHtml(relPath)}</h1>
    <div class="topbar-actions">
      <a href="/docs" class="btn">Alle Dokumente</a>
      <a href="/" class="btn btn-primary">KIS-Oberfläche</a>
    </div>
  </header>
  <main class="content">
    <pre>${escapeHtml(raw)}</pre>
  </main>
</body>
</html>`;
    res.writeHead(200, {
      ...SECURITY_HEADERS,
      'Content-Type': 'text/html; charset=utf-8',
      'Cache-Control': 'no-cache, must-revalidate',
    });
    res.end(html);
  } catch {
    res.writeHead(404, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('Dokument nicht gefunden');
  }
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url || '/', `http://${req.headers.host || 'localhost'}`);
  const pathname = decodeURIComponent(url.pathname);

  if (pathname === '/health' || pathname === '/api/health') {
    res.writeHead(200, { ...SECURITY_HEADERS, 'Content-Type': 'application/json; charset=utf-8' });
    res.end(
      JSON.stringify({
        status: 'ok',
        mode: 'static-preview',
        backend: 'unavailable',
        note: 'Statischer Vorschau-Server für static/ und docs/. Für Live-API ist kis-app mit PostgreSQL erforderlich.',
      })
    );
    return;
  }

  if (pathname.startsWith('/api/')) {
    res.writeHead(503, { ...SECURITY_HEADERS, 'Content-Type': 'application/json; charset=utf-8' });
    res.end(
      JSON.stringify({
        error: 'BACKEND_UNAVAILABLE',
        message:
          'Das Rust-/PostgreSQL-Backend (kis-app) läuft in dieser Vorschau-Umgebung nicht. Es werden keine Scheindaten ausgeliefert.',
      })
    );
    return;
  }

  if (pathname === '/' || pathname === '/index.html') {
    await serveFile(res, path.join(STATIC_ROOT, 'index.html'));
    return;
  }

  if (pathname === '/arbeitsplatz' || pathname === '/arbeitsplatz.html') {
    await serveFile(res, path.join(STATIC_ROOT, 'arbeitsplatz.html'));
    return;
  }

  if (pathname.startsWith('/static/')) {
    const rel = pathname.slice('/static/'.length);
    const resolved = safeResolve(STATIC_ROOT, rel);
    if (!resolved) {
      res.writeHead(400, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
      res.end('Ungültiger Pfad');
      return;
    }
    await serveFile(res, resolved);
    return;
  }

  if (pathname === '/docs' || pathname === '/docs/') {
    await listDocsIndex(res);
    return;
  }

  if (pathname.startsWith('/docs/')) {
    const rel = pathname.slice('/docs/'.length);
    await serveMarkdownDoc(res, rel);
    return;
  }

  res.writeHead(404, { ...SECURITY_HEADERS, 'Content-Type': 'text/plain; charset=utf-8' });
  res.end('Nicht gefunden');
});

server.listen(PORT, HOST, () => {
  console.log(`KIS static preview server listening on http://${HOST}:${PORT}`);
});
