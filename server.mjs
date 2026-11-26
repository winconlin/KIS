import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const execFileAsync = promisify(execFile);

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PORT = Number.parseInt(process.env.PORT || '3000', 10);
const HOST = '0.0.0.0';
const STATIC_ROOT = path.resolve(__dirname, 'static');
const DOCS_ROOT = path.resolve(__dirname, 'docs');
const PREVIEW_ROOT = path.resolve(__dirname, 'preview');

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

function sendJson(res, statusCode, payload) {
  res.writeHead(statusCode, {
    ...SECURITY_HEADERS,
    'Content-Type': 'application/json; charset=utf-8',
    'Cache-Control': 'no-cache, must-revalidate',
  });
  res.end(JSON.stringify(payload));
}

function sendText(res, statusCode, text, contentType = 'text/plain; charset=utf-8') {
  res.writeHead(statusCode, {
    ...SECURITY_HEADERS,
    'Content-Type': contentType,
    'Cache-Control': 'no-cache, must-revalidate',
  });
  res.end(text);
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
      sendText(res, 404, 'Nicht gefunden');
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
    sendText(res, 404, 'Nicht gefunden');
  }
}

async function pathExists(targetPath) {
  try {
    await fs.access(targetPath);
    return true;
  } catch {
    return false;
  }
}

async function commandExists(cmd) {
  try {
    const { stdout } = await execFileAsync('which', [cmd], { timeout: 3000 });
    return { available: true, detail: stdout.trim() };
  } catch {
    return { available: false, detail: 'nicht installiert' };
  }
}

function classifyDoc(relPath) {
  if (relPath.startsWith('vertraege/')) return 'M01-Umsetzungsverträge & Fixtures';
  if (relPath.startsWith('abnahme/')) return 'Abnahme- & Prüfberichte';
  if (
    [
      'EP00_ARCHITEKTUR_UND_ANALYSE.md',
      'ROADMAP.md',
      'KOORDINATIONSREGISTER.md',
      'NATIVE_FIRST_REGISTER.md',
      'THREAT_MODEL_VERBUND.md',
      'VERBUND_TESTMODELL.md',
      'ABNAHMEBERICHT.md',
      'UEBERGABE_GIT_CI.md',
    ].includes(relPath)
  ) {
    return 'EP-00 Architektur & Koordination';
  }
  if (
    [
      'ENTERPRISE_KIS.md',
      'ENTERPRISE_BACKLOG.md',
      'TI_COMPLIANCE.md',
      'CLINICAL_WORKFLOWS.md',
      'ARBEITSPAKETE.md',
      'SICHERHEITS_ABNAHME.md',
      'MODULE.md',
      'FUNKTIONSLANDKARTE.md',
      'REVIEW_STATUS.md',
    ].includes(relPath)
  ) {
    return 'Verbindliche Grundlagen & Status';
  }
  return 'Betrieb, Schnittstellen & Weitere Doku';
}

async function collectDocsList(dir = DOCS_ROOT, prefix = '') {
  const results = [];
  const entries = await fs.readdir(dir, { withFileTypes: true });
  for (const entry of entries) {
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const sub = await collectDocsList(full, rel);
      results.push(...sub);
    } else if (entry.isFile() && (entry.name.endsWith('.md') || entry.name.endsWith('.json'))) {
      const stat = await fs.stat(full);
      let title = entry.name;
      if (entry.name.endsWith('.md')) {
        const text = await fs.readFile(full, 'utf8');
        const heading = text.match(/^#\s+(.+)$/m);
        if (heading) title = heading[1].replace(/`/g, '').trim();
      }
      results.push({
        path: rel,
        name: entry.name,
        title,
        kind: entry.name.endsWith('.json') ? 'json' : 'markdown',
        group: classifyDoc(rel),
        sizeBytes: stat.size,
        updatedAt: stat.mtime.toISOString(),
      });
    }
  }
  return results.sort((a, b) => a.path.localeCompare(b.path));
}

function parseMarkdownTableRows(markdown, sectionHeadingPattern) {
  const lines = markdown.split(/\r?\n/);
  let inSection = !sectionHeadingPattern;
  const tables = [];
  let current = [];

  for (const line of lines) {
    if (sectionHeadingPattern && /^##+\s+/.test(line)) {
      if (inSection && current.length) {
        tables.push(current);
        current = [];
      }
      inSection = sectionHeadingPattern.test(line);
      continue;
    }
    if (!inSection) continue;
    if (line.trim().startsWith('|') && line.trim().endsWith('|')) {
      current.push(line.trim());
    } else if (current.length) {
      tables.push(current);
      current = [];
    }
  }
  if (current.length) tables.push(current);

  const parsed = [];
  for (const table of tables) {
    if (table.length < 3) continue;
    const rows = table.slice(2).map((row) =>
      row
        .slice(1, -1)
        .split('|')
        .map((cell) => cell.trim())
    );
    parsed.push(...rows);
  }
  return parsed;
}

function cleanMdInline(text) {
  return String(text || '')
    .replace(/\*\*/g, '')
    .replace(/`/g, '')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .trim();
}

function contractForPackage(pkgId) {
  if (pkgId.startsWith('EP-01')) return 'vertraege/EP-01_MANDANTEN_UND_ORG_VERTRAG.md';
  if (pkgId.startsWith('EP-02') || pkgId.startsWith('EP-03')) {
    return 'vertraege/EP-02_EP-03_MPI_UND_FALL_VERTRAG.md';
  }
  if (pkgId.startsWith('EP-04') || pkgId.startsWith('EP-05')) {
    return 'vertraege/EP-04_EP-05_AUTHZ_AUDIT_UND_INVARIANTEN_VERTRAG.md';
  }
  if (pkgId.startsWith('EP-06')) return 'vertraege/EP-06_KATALOG_NORMEN_UND_ZEITVERTRAG.md';
  if (pkgId.startsWith('EP-07')) return 'vertraege/EP-07_BETRIEB_MONITORING_DOWNTIME_VERTRAG.md';
  if (pkgId === 'EP-00') return 'EP00_ARCHITEKTUR_UND_ANALYSE.md';
  return null;
}

async function readWorkspaceStats() {
  let cratesCount = 0;
  let migrationsCount = 0;
  let modulesCount = 0;
  let tablesCount = 0;

  try {
    const cargo = await fs.readFile(path.join(__dirname, 'Cargo.toml'), 'utf8');
    const membersMatch = cargo.match(/members\s*=\s*\[([\s\S]*?)\]/);
    if (membersMatch) {
      cratesCount = (membersMatch[1].match(/"crates\/[^"]+"/g) || []).length;
    }
  } catch {
    cratesCount = 0;
  }

  try {
    const migFiles = await fs.readdir(path.join(__dirname, 'migrations'));
    migrationsCount = migFiles.filter((f) => /^\d{3}_.+\.sql$/.test(f)).length;
  } catch {
    migrationsCount = 0;
  }

  try {
    const modToml = await fs.readFile(path.join(__dirname, 'config', 'module_tables.toml'), 'utf8');
    modulesCount = (modToml.match(/^\[modules\.[^\]]+\]/gm) || []).length;
    const tableBlocks = modToml.match(/tables\s*=\s*\[([\s\S]*?)\]/g) || [];
    for (const block of tableBlocks) {
      tablesCount += (block.match(/"[^"]+"/g) || []).length;
    }
  } catch {
    modulesCount = 0;
    tablesCount = 0;
  }

  return { cratesCount, modulesCount, tablesCount, migrationsCount };
}

async function buildOverviewPayload() {
  const [workspace, documents, roadmapMd, koordMd, ep00Md] = await Promise.all([
    readWorkspaceStats(),
    collectDocsList(),
    fs.readFile(path.join(DOCS_ROOT, 'ROADMAP.md'), 'utf8'),
    fs.readFile(path.join(DOCS_ROOT, 'KOORDINATIONSREGISTER.md'), 'utf8'),
    fs.readFile(path.join(DOCS_ROOT, 'EP00_ARCHITEKTUR_UND_ANALYSE.md'), 'utf8'),
  ]);

  const m01Rows = parseMarkdownTableRows(roadmapMd, /^## 3\. Feinzerlegung/);
  const m01Subpackages = m01Rows.map((cols) => {
    const id = cleanMdInline(cols[0]);
    return {
      id,
      milestone: 'M01',
      priority: 'P0',
      scope: cleanMdInline(cols[1]),
      dependencies: cleanMdInline(cols[2]),
      files: cleanMdInline(cols[3]),
      evidence: cleanMdInline(cols[4]),
      status: id === 'EP-04d' ? 'Blockiert (DEC-06/08)' : 'Spezifiziert',
      contractDoc: contractForPackage(id),
    };
  });

  const epFollowRows = parseMarkdownTableRows(roadmapMd, /^## 4\. Mapping aller Folgepakete/);
  const epFollow = epFollowRows.map((cols) => {
    const id = cleanMdInline(cols[0]);
    return {
      id,
      priority: cleanMdInline(cols[1]),
      milestone: cleanMdInline(cols[2]),
      dependencies: cleanMdInline(cols[3]),
      traceability: cleanMdInline(cols[4]),
      scope: cleanMdInline(cols[5]),
      status: ['EP-12', 'EP-16', 'EP-21', 'EP-23'].includes(id) ? 'Spezifiziert (Teilbereich)' : 'Geplant',
      contractDoc: contractForPackage(id),
    };
  });

  const roadmapPackages = [
    {
      id: 'EP-00',
      priority: 'P0',
      milestone: 'M00',
      dependencies: '—',
      traceability: 'ADR-001–007, NVT-01–18',
      scope: 'Ist-Analyse, ADRs, M01-Umsetzungsverträge, Native-First-Register, Bedrohungs- & Testmodell, Koordinationsregister',
      status: 'Im Review (G6 offen)',
      contractDoc: 'EP00_ARCHITEKTUR_UND_ANALYSE.md',
    },
    {
      id: 'EP-01',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-00 (G6)',
      traceability: 'E24, ADR-001, NVT-01/02/04',
      scope: 'Mandanten, Rechtsträger, Standorte, MVZ-Kennungen, zyklenfreie Hierarchie, Scoped Config & Policy-Floor',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-01'),
    },
    {
      id: 'EP-02',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-01',
      traceability: 'E01, AP-12, ADR-002, NVT-05/06',
      scope: 'Enterprise-MPI, Identitätsräume, aktive AEAD-Verschlüsselung, Kontakte, Dubletten & Merge/Unmerge',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-02'),
    },
    {
      id: 'EP-03',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-01, EP-02',
      traceability: 'E01/E02, AP-13, ADR-003, NVT-07',
      scope: 'Episoden, klinische Fälle, Encounters, MVZ ohne Bett, Doppelbelegungs-Constraint, Abrechnungsfälle & Kostenträger',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-03'),
    },
    {
      id: 'EP-04',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-01, EP-03',
      traceability: 'AP-02/03/04, ADR-004, NVT-03/08–13',
      scope: 'Admin-/Domänentrennung, 4-Faktoren-AuthZ, Lese-Audit, WORM-Prüfanker, Retention & Legal Hold',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-04'),
    },
    {
      id: 'EP-05',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-01, EP-03, EP-04',
      traceability: 'ADR-005, NVT-14/15',
      scope: 'Klasse-A-Sicherheitsinvarianten (Bettensperre, kritische Werte), mandantenfähige Outbox/Inbox & Quarantäne',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-05'),
    },
    {
      id: 'EP-06',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-00 (G6)',
      traceability: 'AP-14, TI15, ADR-006',
      scope: 'Normen-, Terminologie-, Katalog- & Lizenzregister (kis-catalogs) sowie klinischer Zeit-/UCUM-/Statusvertrag',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-06'),
    },
    {
      id: 'EP-07',
      priority: 'P0',
      milestone: 'M01',
      dependencies: 'EP-01, EP-04, EP-05',
      traceability: 'AP-05/06, NVT-16',
      scope: 'PII-freies Monitoring, Schlüsselrotation, Mandanten-Caching, HA/DR, Downtime-Read-Only & Verbund-Lasttest',
      status: 'Spezifiziert',
      contractDoc: contractForPackage('EP-07'),
    },
    ...epFollow,
  ];

  const assignmentRows = parseMarkdownTableRows(koordMd, /^## 3\. Aktive Agenten-Zuweisungen/);
  const assignments = assignmentRows.map((cols) => ({
    id: cleanMdInline(cols[0]),
    scope: cleanMdInline(cols[1]),
    agent: cleanMdInline(cols[2]),
    role: cleanMdInline(cols[3]),
    branch: cleanMdInline(cols[5]),
    migrations: cleanMdInline(cols[7]),
    status: cleanMdInline(cols[9]),
    blockers: cleanMdInline(cols[11]),
    updatedAt: cleanMdInline(cols[12]),
  }));

  const apRows = parseMarkdownTableRows(koordMd, /^### 4\.1 Unabhängige Bestandshärtung/);
  const hardeningPackages = apRows.map((cols) => ({
    id: cleanMdInline(cols[0]),
    scope: cleanMdInline(cols[1]),
    dependencies: cleanMdInline(cols[2]),
    migration: cleanMdInline(cols[3]),
    status: cleanMdInline(cols[4]),
    parallel: cleanMdInline(cols[5]),
  }));

  const decRows = parseMarkdownTableRows(ep00Md, /^## Teil C: Offene Fach-/);
  const decisions = decRows.map((cols) => ({
    id: cleanMdInline(cols[0]),
    topic: cleanMdInline(cols[1]),
    packages: cleanMdInline(cols[2]),
    clarification: cleanMdInline(cols[3]),
  }));

  const gates = [
    { id: 'G0', title: 'Auftrag, Koordination & Native-First', state: 'passed', note: 'EP-00, M01-Verträge & Koordinationsregister konsistent erstellt' },
    { id: 'G1', title: 'Architektur-, Modul- & Schema-Integrität', state: 'passed', note: 'check_guardrails.py & check_modules.py = 0 Verstöße; crates/ & migrations/ gesperrt' },
    { id: 'G2', title: 'Sicherheit & Mandantenisolation', state: 'review', note: 'Bedrohungsmodell (T01–T24) & NVT-01–18 spezifiziert; Laufzeit-Tests ab EP-01' },
    { id: 'G3', title: 'PostgreSQL-, Funktions- & Ausfalltests', state: 'blocked', note: 'PostgreSQL-16-Host erforderlich (nicht in diesem Snapshot installiert)' },
    { id: 'G4', title: 'Gesamtprüfung scripts/check.sh & CI', state: 'blocked', note: 'Rust-Toolchain, Cargo.lock & PostgreSQL für Volllauf erforderlich' },
    { id: 'G5', title: 'Dokumentation & Statuswahrheit', state: 'passed', note: 'check_docs.py & check_vertraege.py = 0 Fehler' },
    { id: 'G6', title: 'Unabhängiges Gegenreview (kein Selbst-Review)', state: 'review', note: 'Verbindlich an hermes (EP-00-REV) / unabhängige Instanz zugewiesen' },
    { id: 'G7', title: 'Fach-, Betriebs- & Regulatorik-Freigabe', state: 'blocked', note: 'Offen (DEC-01–09, DPIA, Pentest, gematik RU / KBV / MDR)' },
  ];

  const toolNames = ['python3', 'node', 'git', 'gh', 'cargo', 'rustc', 'psql', 'postgres', 'docker', 'age'];
  const cmdChecks = await Promise.all(
    toolNames.map(async (name) => ({ name, ...(await commandExists(name)) }))
  );
  const [hasGitDir, hasCargoLock] = await Promise.all([
    pathExists(path.join(__dirname, '.git')),
    pathExists(path.join(__dirname, 'Cargo.lock')),
  ]);
  const toolchain = [
    ...cmdChecks,
    { name: '.git-Verzeichnis', available: hasGitDir, detail: hasGitDir ? 'vorhanden' : 'Snapshot ohne .git' },
    { name: 'Cargo.lock', available: hasCargoLock, detail: hasCargoLock ? 'vorhanden' : 'fehlt im Snapshot' },
  ];

  return {
    generatedAt: new Date().toISOString(),
    coordinator: 'google-neon',
    workspace,
    toolchain,
    gates,
    assignments,
    hardeningPackages,
    roadmapPackages,
    m01Subpackages,
    decisions,
    documents,
  };
}

let cachedChecks = null;
let cachedChecksAt = 0;

async function runSingleCheck(id, label, command, args) {
  const started = Date.now();
  try {
    const { stdout, stderr } = await execFileAsync(command, args, {
      cwd: __dirname,
      timeout: 15000,
    });
    const combined = [stdout.trim(), stderr.trim()].filter(Boolean).join('\n');
    return {
      id,
      label,
      command: `${command} ${args.join(' ')}`,
      state: 'passed',
      exitCode: 0,
      durationMs: Date.now() - started,
      output: combined || 'OK',
    };
  } catch (err) {
    const combined = [err.stdout?.trim(), err.stderr?.trim(), err.message?.trim()]
      .filter(Boolean)
      .join('\n');
    return {
      id,
      label,
      command: `${command} ${args.join(' ')}`,
      state: 'failed',
      exitCode: typeof err.code === 'number' ? err.code : 1,
      durationMs: Date.now() - started,
      output: combined,
    };
  }
}

async function runLocalChecks(forceRefresh = false) {
  if (!forceRefresh && cachedChecks && Date.now() - cachedChecksAt < 10_000) {
    return cachedChecks;
  }

  const results = await Promise.all([
    runSingleCheck('guardrails', 'Leitplanken & Migrations-Prüfsummen', 'python3', ['scripts/check_guardrails.py']),
    runSingleCheck('modules', 'Modulgrenzen & Tabelleneigentum', 'python3', ['scripts/check_modules.py']),
    runSingleCheck('docs', 'Dokumentation, Links & Codeblöcke', 'python3', ['scripts/check_docs.py']),
    runSingleCheck('vertraege', 'M01-Verträge & JSON-Fixture-Integrität', 'python3', ['scripts/check_vertraege.py']),
    runSingleCheck('server_syntax', 'Vorschau-Server & Cockpit-Syntax', 'node', ['--check', 'server.mjs']),
  ]);

  const unavailable = [
    {
      id: 'audit_exceptions',
      label: 'Audit-Ausnahmen (scripts/check_audit_exceptions.py)',
      command: 'python3 scripts/check_audit_exceptions.py',
      state: 'unavailable',
      exitCode: null,
      durationMs: 0,
      output: 'Nicht ausführbar im Snapshot: Cargo.lock fehlt (läuft auf Git-/Rust-Host).',
    },
    {
      id: 'full_check_sh',
      label: 'Gesamtprüfung (scripts/check.sh: Rust, Clippy, PG-Tests, SQL-Abgleich)',
      command: 'KIS_TEST_DATABASE_URL=... scripts/check.sh',
      state: 'unavailable',
      exitCode: null,
      durationMs: 0,
      output: 'Nicht ausführbar im Snapshot: cargo, rustc und PostgreSQL 16 sind nicht installiert.',
    },
  ];

  const payload = {
    executedAt: new Date().toISOString(),
    passedCount: results.filter((r) => r.state === 'passed').length,
    failedCount: results.filter((r) => r.state === 'failed').length,
    unavailableCount: unavailable.length,
    checks: [...results, ...unavailable],
  };

  cachedChecks = payload;
  cachedChecksAt = Date.now();
  return payload;
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url || '/', `http://${req.headers.host || 'localhost'}`);
  let pathname;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    sendText(res, 400, 'Ungültige URL');
    return;
  }

  if (pathname === '/health' || pathname === '/api/health') {
    sendJson(res, 200, {
      status: 'ok',
      mode: 'readonly-architecture-preview',
      backend: 'unavailable',
      note: 'Schreibgeschützter Vorschau-Server für Architektur-Cockpit (docs/) und statische KIS-Oberfläche (static/). Klinische Endpunkte (/api/*) liefern ehrlich 503.',
    });
    return;
  }

  if (pathname.startsWith('/api/')) {
    sendJson(res, 503, {
      error: 'BACKEND_UNAVAILABLE',
      message:
        'Das Rust-/PostgreSQL-Backend (kis-app) läuft in dieser Vorschau-Umgebung nicht. Es werden keine Scheindaten ausgeliefert.',
    });
    return;
  }

  if (pathname.startsWith('/preview-data/')) {
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      sendJson(res, 405, { error: 'METHOD_NOT_ALLOWED', message: 'Vorschau-Endpunkte sind strikt schreibgeschützt (nur GET).' });
      return;
    }

    try {
      if (pathname === '/preview-data/overview') {
        const overview = await buildOverviewPayload();
        sendJson(res, 200, overview);
        return;
      }

      if (pathname === '/preview-data/fixtures') {
        const [verbundRaw, normativeRaw] = await Promise.all([
          fs.readFile(path.join(DOCS_ROOT, 'vertraege', 'verbund_test_fixture.json'), 'utf8'),
          fs.readFile(path.join(DOCS_ROOT, 'vertraege', 'normative_requirements_seed.json'), 'utf8'),
        ]);
        sendJson(res, 200, {
          verbund: JSON.parse(verbundRaw),
          normative: JSON.parse(normativeRaw),
        });
        return;
      }

      if (pathname === '/preview-data/doc') {
        const relPath = url.searchParams.get('path') || '';
        const resolved = safeResolve(DOCS_ROOT, relPath);
        if (!resolved || !(resolved.endsWith('.md') || resolved.endsWith('.json'))) {
          sendJson(res, 400, { error: 'INVALID_PATH', message: 'Nur .md- und .json-Dateien unter docs/ sind zulässig.' });
          return;
        }
        const stat = await fs.stat(resolved);
        if (!stat.isFile()) {
          sendJson(res, 404, { error: 'NOT_FOUND', message: 'Dokument nicht gefunden.' });
          return;
        }
        const content = await fs.readFile(resolved, 'utf8');
        const heading = content.match(/^#\s+(.+)$/m);
        sendJson(res, 200, {
          path: relPath,
          title: heading ? heading[1].replace(/`/g, '').trim() : path.basename(relPath),
          kind: resolved.endsWith('.json') ? 'json' : 'markdown',
          sizeBytes: stat.size,
          updatedAt: stat.mtime.toISOString(),
          content,
        });
        return;
      }

      if (pathname === '/preview-data/checks') {
        const refresh = url.searchParams.get('refresh') === '1';
        const checks = await runLocalChecks(refresh);
        sendJson(res, 200, checks);
        return;
      }

      sendJson(res, 404, { error: 'NOT_FOUND', message: 'Unbekannter Vorschau-Endpunkt.' });
    } catch (err) {
      sendJson(res, 500, { error: 'PREVIEW_READ_ERROR', message: 'Fehler beim Lesen der lokalen Projektdaten.' });
    }
    return;
  }

  if (pathname === '/' || pathname === '/cockpit' || pathname === '/docs' || pathname === '/docs/') {
    await serveFile(res, path.join(PREVIEW_ROOT, 'cockpit.html'));
    return;
  }

  if (pathname.startsWith('/docs/')) {
    const rel = pathname.slice('/docs/'.length);
    const resolved = safeResolve(DOCS_ROOT, rel);
    if (!resolved || !(resolved.endsWith('.md') || resolved.endsWith('.json'))) {
      sendText(res, 400, 'Ungültiger Dokumentpfad');
      return;
    }
    if (url.searchParams.get('raw') === '1') {
      await serveFile(res, resolved);
      return;
    }
    res.writeHead(302, {
      ...SECURITY_HEADERS,
      Location: `/docs#doc=${encodeURIComponent(rel)}`,
    });
    res.end();
    return;
  }

  if (pathname.startsWith('/preview/')) {
    const rel = pathname.slice('/preview/'.length);
    const resolved = safeResolve(PREVIEW_ROOT, rel);
    if (!resolved) {
      sendText(res, 400, 'Ungültiger Pfad');
      return;
    }
    await serveFile(res, resolved);
    return;
  }

  if (pathname === '/kis' || pathname === '/index.html') {
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
      sendText(res, 400, 'Ungültiger Pfad');
      return;
    }
    await serveFile(res, resolved);
    return;
  }

  sendText(res, 404, 'Nicht gefunden');
});

server.listen(PORT, HOST, () => {
  console.log(`KIS architecture cockpit & preview server listening on http://${HOST}:${PORT}`);
});
