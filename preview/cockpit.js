const VIEW_LABELS = {
  overview: 'Übersicht & Gates',
  roadmap: 'Roadmap & Koordination',
  contracts: 'M01-Verträge & Doku',
  verbund: 'Verbund-Testmodell',
  handoff: 'Normen & Übergabe',
};

const PLAYBOOK_STEPS = [
  {
    title: '01. Koordinations-Branch ab origin/main anlegen',
    desc: 'Immer auf eigenem Branch ab aktuellem origin/main arbeiten, niemals direkt auf main.',
    lang: 'bash',
    code: [
      'git fetch origin',
      'git checkout -b agent/google-neon/ep00-architecture-coordination origin/main',
      'git rev-parse HEAD',
    ].join('\n'),
  },
  {
    title: '02. Sperrprüfung: crates/ und migrations/ unverändert',
    desc: 'Sicherstellen, dass bis zum G6-Review keine Rust- oder Migrationsdateien berührt wurden.',
    lang: 'bash',
    code: [
      'git status -sb',
      'git diff --stat -- crates/ migrations/',
    ].join('\n'),
  },
  {
    title: '03. Vollständige Gesamtprüfung (G4) & Vertrags-Validator',
    desc: 'Volllauf gegen echte PostgreSQL-16-Instanz ohne Abschwächung.',
    lang: 'bash',
    code: [
      'KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh',
      'python3 scripts/check_vertraege.py',
    ].join('\n'),
  },
  {
    title: '04. Commit, Push, Koordinations-Issue & Draft-PR (G6 an hermes)',
    desc: 'Zentrales Multi-Agenten-Register auf GitHub spiegeln und unabhängiges Gegenreview anfordern.',
    lang: 'bash',
    code: [
      'git add docs/ scripts/check_vertraege.py',
      'git commit -m "docs(ep-00): Zielarchitektur, M01-Vertraege und Koordinationsregister"',
      'git push -u origin agent/google-neon/ep00-architecture-coordination',
      '',
      'gh issue create --repo winconlin/KIS \\',
      '  --title "[Koordination] Zentrales Multi-Agenten-Arbeitspaketregister (EP-00 bis EP-26 / AP-*)" \\',
      '  --body-file docs/KOORDINATIONSREGISTER.md',
      '',
      'gh pr create --repo winconlin/KIS --base main \\',
      '  --head agent/google-neon/ep00-architecture-coordination --draft \\',
      '  --title "EP-00 (M00): Zielarchitektur, M01-Vertraege & Koordinationsregister" \\',
      '  --body-file docs/abnahme/EP-00-konsistenzpruefung.md',
    ].join('\n'),
  },
  {
    title: '05. Nachbesserungs-Issue AP-09b (G6-Befund R-01 zu gemergtem PR #9)',
    desc: 'PR #9 ist auf origin/main (2d0e4479f9) gemerged; R-01 (unbenutzte Dev-Dependency kis-app in crates/kis-doctors/Cargo.toml + Cargo.lock) als AP-09b nachverfolgen.',
    lang: 'bash',
    code: [
      'gh issue create --repo winconlin/KIS \\',
      '  --title "[AP-09b] Unbenutzte Dev-Dependency kis-app aus crates/kis-doctors/Cargo.toml entfernen (G6-Befund R-01 zu PR #9)" \\',
      '  --body-file docs/abnahme/PR-09-review-list-doctors.md',
    ].join('\n'),
  },
];

const state = {
  view: 'overview',
  overview: null,
  fixtures: null,
  checks: null,
  selectedDoc: 'vertraege/README.md',
  docSearch: '',
  roadmapMilestone: 'ALL',
  roadmapPrio: 'ALL',
  roadmapSearch: '',
  tenantFilter: 'ALL',
  verbundSearch: '',
  normClass: 'ALL',
  normSearch: '',
};

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
}

function clearNode(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return '—';
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KB`;
}

function formatTime(iso) {
  if (!iso) return '—';
  try {
    return new Date(iso).toLocaleString('de-DE', {
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return iso;
  }
}

function stateClassAndLabel(stateKey) {
  const map = {
    passed: ['state-passed', 'Bestanden'],
    review: ['state-review', 'Offen / Im Review'],
    blocked: ['state-blocked', 'Offen / Blockiert'],
    failed: ['state-blocked', 'Fehlgeschlagen'],
    unavailable: ['state-review', 'Nicht ausführbar (Host nötig)'],
  };
  return map[stateKey] || ['state-neutral', String(stateKey)];
}

function statusTone(statusText) {
  const lower = String(statusText || '').toLowerCase();
  if (lower.includes('blockiert')) return 'state-blocked';
  if (lower.includes('review') || lower.includes('offen') || lower.includes('zugewiesen')) {
    return 'state-review';
  }
  if (lower.includes('freigegeben') || lower.includes('bestanden') || lower.includes('vorhanden')) {
    return 'state-passed';
  }
  return 'state-neutral';
}

async function fetchJson(url) {
  const res = await fetch(url, { headers: { Accept: 'application/json' } });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`HTTP ${res.status} bei ${url}${body ? ` — ${body.slice(0, 120)}` : ''}`);
  }
  return res.json();
}

function showError(msg) {
  const banner = document.getElementById('global-error');
  const text = document.getElementById('global-error-message');
  text.textContent = msg;
  banner.hidden = false;
}

function hideError() {
  document.getElementById('global-error').hidden = true;
}

function createCodeBlock(lang, code) {
  const wrap = el('div', 'code-block-wrap');
  const bar = el('div', 'code-block-bar');
  bar.appendChild(el('span', null, lang || 'text'));

  const copyBtn = el('button', 'copy-code-btn', 'Kopieren');
  copyBtn.type = 'button';
  copyBtn.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(code);
      copyBtn.textContent = 'Kopiert';
      setTimeout(() => {
        copyBtn.textContent = 'Kopieren';
      }, 1500);
    } catch {
      copyBtn.textContent = 'Kopieren fehlgeschlagen';
      setTimeout(() => {
        copyBtn.textContent = 'Kopieren';
      }, 1500);
    }
  });
  bar.appendChild(copyBtn);

  const pre = el('pre');
  const codeNode = el('code', null, code);
  pre.appendChild(codeNode);

  wrap.appendChild(bar);
  wrap.appendChild(pre);
  return wrap;
}

function resolveRelativeDocPath(currentDocPath, href) {
  const cleanHref = href.split('#')[0].split('?')[0];
  if (!cleanHref) return null;
  if (cleanHref.startsWith('/docs/')) return cleanHref.slice('/docs/'.length);
  const baseParts = currentDocPath.split('/').slice(0, -1);
  for (const part of cleanHref.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      baseParts.pop();
    } else {
      baseParts.push(part);
    }
  }
  return baseParts.join('/');
}

function renderInlineMarkdown(container, text, currentDocPath) {
  const tokenRegex = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\))/g;
  let lastIndex = 0;
  let match;

  while ((match = tokenRegex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      container.appendChild(document.createTextNode(text.slice(lastIndex, match.index)));
    }
    const token = match[0];
    if (token.startsWith('`') && token.endsWith('`')) {
      container.appendChild(el('code', null, token.slice(1, -1)));
    } else if (token.startsWith('**') && token.endsWith('**')) {
      container.appendChild(el('strong', null, token.slice(2, -2)));
    } else if (token.startsWith('[')) {
      const linkMatch = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
      if (linkMatch) {
        const [, label, href] = linkMatch;
        const a = el('a', null, label);
        if (/^https?:\/\//i.test(href)) {
          a.href = href;
          a.target = '_blank';
          a.rel = 'noopener noreferrer';
        } else if (href.endsWith('.md') || href.endsWith('.json') || href.includes('.md#')) {
          const target = resolveRelativeDocPath(currentDocPath, href);
          if (target && !target.startsWith('..')) {
            a.href = `#doc=${encodeURIComponent(target)}`;
            a.addEventListener('click', (ev) => {
              ev.preventDefault();
              openDocument(target);
            });
          } else {
            a.href = '#';
            a.addEventListener('click', (ev) => ev.preventDefault());
          }
        } else {
          a.href = '#';
          a.addEventListener('click', (ev) => ev.preventDefault());
        }
        container.appendChild(a);
      }
    }
    lastIndex = tokenRegex.lastIndex;
  }

  if (lastIndex < text.length) {
    container.appendChild(document.createTextNode(text.slice(lastIndex)));
  }
}

function renderMarkdownSafe(container, markdown, currentDocPath) {
  clearNode(container);
  const lines = markdown.split(/\r?\n/);
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (!line.trim()) {
      i += 1;
      continue;
    }

    // Fenced code block
    const fence = line.match(/^```(\S*)\s*$/);
    if (fence) {
      const lang = fence[1] || 'text';
      const body = [];
      i += 1;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) {
        body.push(lines[i]);
        i += 1;
      }
      i += 1;
      container.appendChild(createCodeBlock(lang, body.join('\n')));
      continue;
    }

    // Horizontal rule
    if (/^---+\s*$/.test(line.trim())) {
      container.appendChild(el('hr'));
      i += 1;
      continue;
    }

    // Heading
    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      const tag = `h${heading[1].length}`;
      const h = el(tag);
      renderInlineMarkdown(h, heading[2].trim(), currentDocPath);
      container.appendChild(h);
      i += 1;
      continue;
    }

    // Blockquote
    if (line.trim().startsWith('>')) {
      const quoteLines = [];
      while (i < lines.length && lines[i].trim().startsWith('>')) {
        quoteLines.push(lines[i].trim().replace(/^>\s?/, ''));
        i += 1;
      }
      const bq = el('blockquote');
      renderInlineMarkdown(bq, quoteLines.join(' '), currentDocPath);
      container.appendChild(bq);
      continue;
    }

    // Markdown table
    if (
      line.trim().startsWith('|') &&
      i + 1 < lines.length &&
      /^\|[\s:|-]+\|$/.test(lines[i + 1].trim())
    ) {
      const wrap = el('div', 'table-wrap');
      const table = el('table', 'data-table');
      const thead = el('thead');
      const trHead = el('tr');
      const headCells = line.trim().slice(1, -1).split('|').map((c) => c.trim());
      for (const cell of headCells) {
        const th = el('th');
        renderInlineMarkdown(th, cell, currentDocPath);
        trHead.appendChild(th);
      }
      thead.appendChild(trHead);
      table.appendChild(thead);

      const tbody = el('tbody');
      i += 2;
      while (i < lines.length && lines[i].trim().startsWith('|') && lines[i].trim().endsWith('|')) {
        const tr = el('tr');
        const cells = lines[i].trim().slice(1, -1).split('|').map((c) => c.trim());
        for (const cell of cells) {
          const td = el('td');
          renderInlineMarkdown(td, cell, currentDocPath);
          tr.appendChild(td);
        }
        tbody.appendChild(tr);
        i += 1;
      }
      table.appendChild(tbody);
      wrap.appendChild(table);
      container.appendChild(wrap);
      continue;
    }

    // Unordered or ordered list
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\.\s+/.test(line);
      const list = el(ordered ? 'ol' : 'ul');
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) {
        const itemText = lines[i].replace(/^\s*([-*]|\d+\.)\s+/, '');
        const li = el('li');
        renderInlineMarkdown(li, itemText, currentDocPath);
        list.appendChild(li);
        i += 1;
      }
      container.appendChild(list);
      continue;
    }

    // Paragraph
    const paraLines = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^(#{1,4}\s+|```|---+|>\s?|\|)/.test(lines[i].trim()) &&
      !/^\s*([-*]|\d+\.)\s+/.test(lines[i])
    ) {
      paraLines.push(lines[i].trim());
      i += 1;
    }
    const p = el('p');
    renderInlineMarkdown(p, paraLines.join(' '), currentDocPath);
    container.appendChild(p);
  }
}

function renderOverview() {
  const ov = state.overview;
  if (!ov) return;

  document.getElementById('kpi-crates').textContent = `${ov.workspace.cratesCount} Crates`;
  document.getElementById('kpi-tables').textContent =
    `${ov.workspace.modulesCount} Fachmodule · ${ov.workspace.tablesCount} Tabellen · ${ov.workspace.migrationsCount} Migrationen`;
  document.getElementById('kpi-packages').textContent = `${ov.roadmapPackages.length} Pakete`;

  if (state.fixtures?.verbund) {
    const v = state.fixtures.verbund;
    document.getElementById('kpi-nvt').textContent = `${v.negative_tests.length} NVTs`;
    document.getElementById('kpi-nvt-sub').textContent =
      `${v.legal_entities.length} Rechtsträger · ${v.sites.length} Standorte · ${v.users.length} Benutzer`;
  }

  const gatesBody = document.getElementById('gates-tbody');
  clearNode(gatesBody);
  for (const gate of ov.gates) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', gate.id));
    tr.appendChild(el('td', null, gate.title));
    const [cls, lbl] = stateClassAndLabel(gate.state);
    tr.appendChild(el('td', cls, lbl));
    tr.appendChild(el('td', null, gate.note));
    gatesBody.appendChild(tr);
  }

  const toolBody = document.getElementById('toolchain-tbody');
  clearNode(toolBody);
  for (const item of ov.toolchain) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', item.name));
    tr.appendChild(
      el('td', item.available ? 'state-passed' : 'state-blocked', item.available ? 'Verfügbar' : 'Fehlt im Snapshot')
    );
    tr.appendChild(el('td', 'mono', item.detail));
    toolBody.appendChild(tr);
  }

  const decBody = document.getElementById('decisions-tbody');
  clearNode(decBody);
  for (const dec of ov.decisions) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', dec.id));
    tr.appendChild(el('td', null, dec.topic));
    tr.appendChild(el('td', 'mono', dec.packages));
    tr.appendChild(el('td', null, dec.clarification));
    decBody.appendChild(tr);
  }
}

function renderChecks() {
  const data = state.checks;
  if (!data) return;

  document.getElementById('kpi-checks').textContent =
    `${data.passedCount} / ${data.passedCount + data.failedCount} grün`;
  document.getElementById('kpi-checks-sub').textContent =
    `${data.unavailableCount} weitere nur auf Rust-/PG-Host`;
  document.getElementById('checks-timestamp').textContent =
    `Zuletzt geprüft: ${formatTime(data.executedAt)}`;

  const tbody = document.getElementById('checks-tbody');
  clearNode(tbody);
  for (const chk of data.checks) {
    const tr = el('tr');
    tr.appendChild(el('td', null, chk.label));
    tr.appendChild(el('td', 'mono', chk.command));
    const [cls, lbl] = stateClassAndLabel(chk.state);
    tr.appendChild(el('td', cls, lbl));
    tr.appendChild(el('td', 'num', chk.durationMs ? `${chk.durationMs} ms` : '—'));
    const outTd = el('td');
    outTd.appendChild(el('div', 'output-snippet', chk.output));
    tr.appendChild(outTd);
    tbody.appendChild(tr);
  }
}

function renderRoadmap() {
  const ov = state.overview;
  if (!ov) return;

  const assignBody = document.getElementById('assignments-tbody');
  clearNode(assignBody);
  for (const a of ov.assignments) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', a.id));
    const agentTd = el('td');
    agentTd.appendChild(el('span', 'mono', a.agent));
    agentTd.appendChild(el('span', 'cell-secondary', a.role));
    tr.appendChild(agentTd);
    tr.appendChild(el('td', 'mono', a.branch));
    tr.appendChild(el('td', statusTone(a.status), a.status));
    const scopeTd = el('td');
    scopeTd.appendChild(el('span', null, a.scope));
    if (a.blockers && a.blockers !== '—') {
      scopeTd.appendChild(el('span', 'cell-secondary', `Offen: ${a.blockers}`));
    }
    tr.appendChild(scopeTd);
    assignBody.appendChild(tr);
  }

  const query = state.roadmapSearch.trim().toLowerCase();
  const filtered = ov.roadmapPackages.filter((pkg) => {
    if (state.roadmapMilestone !== 'ALL' && pkg.milestone !== state.roadmapMilestone) return false;
    if (state.roadmapPrio !== 'ALL' && !pkg.priority.includes(state.roadmapPrio)) return false;
    if (query) {
      const hay = `${pkg.id} ${pkg.milestone} ${pkg.priority} ${pkg.dependencies} ${pkg.traceability} ${pkg.scope} ${pkg.status}`.toLowerCase();
      if (!hay.includes(query)) return false;
    }
    return true;
  });

  document.getElementById('roadmap-count').textContent =
    `${filtered.length} von ${ov.roadmapPackages.length} Paketen sichtbar`;

  const tbody = document.getElementById('roadmap-tbody');
  const empty = document.getElementById('roadmap-empty');
  clearNode(tbody);
  empty.hidden = filtered.length > 0;

  for (const pkg of filtered) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', pkg.id));
    tr.appendChild(el('td', 'mono', `${pkg.milestone} · ${pkg.priority}`));
    tr.appendChild(el('td', 'mono', pkg.dependencies));
    tr.appendChild(el('td', statusTone(pkg.status), pkg.status));
    const scopeTd = el('td');
    scopeTd.appendChild(el('span', null, pkg.scope));
    if (pkg.traceability) {
      scopeTd.appendChild(el('span', 'cell-secondary mono', pkg.traceability));
    }
    tr.appendChild(scopeTd);

    const actionTd = el('td');
    if (pkg.contractDoc) {
      const btn = el('button', 'btn btn-secondary btn-inline', 'Vertrag öffnen');
      btn.type = 'button';
      btn.addEventListener('click', () => openDocument(pkg.contractDoc));
      actionTd.appendChild(btn);
    } else {
      actionTd.appendChild(el('span', 'muted-note', 'ab M02+'));
    }
    tr.appendChild(actionTd);
    tbody.appendChild(tr);
  }

  const m01Body = document.getElementById('m01-sub-tbody');
  clearNode(m01Body);
  for (const sub of ov.m01Subpackages) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', sub.id));
    tr.appendChild(el('td', 'mono', sub.dependencies));
    const targetTd = el('td');
    targetTd.appendChild(el('span', null, sub.scope));
    targetTd.appendChild(el('span', 'cell-secondary mono', sub.files));
    tr.appendChild(targetTd);
    tr.appendChild(el('td', null, sub.evidence));
    const linkTd = el('td');
    if (sub.contractDoc) {
      const btn = el('button', 'btn btn-secondary btn-inline', 'Vertrag');
      btn.type = 'button';
      btn.addEventListener('click', () => openDocument(sub.contractDoc));
      linkTd.appendChild(btn);
    }
    tr.appendChild(linkTd);
    m01Body.appendChild(tr);
  }

  for (const ap of ov.hardeningPackages) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', ap.id));
    tr.appendChild(el('td', 'mono', ap.dependencies || '—'));
    tr.appendChild(el('td', null, ap.scope));
    tr.appendChild(el('td', statusTone(ap.status), ap.status));
    tr.appendChild(el('td', 'muted-note', ap.parallel));
    m01Body.appendChild(tr);
  }
}

function renderDocList() {
  const ov = state.overview;
  if (!ov) return;

  const query = state.docSearch.trim().toLowerCase();
  const filtered = ov.documents.filter((doc) => {
    if (!query) return true;
    return `${doc.path} ${doc.title} ${doc.group}`.toLowerCase().includes(query);
  });

  const groupsEl = document.getElementById('doc-groups');
  const emptyEl = document.getElementById('doc-list-empty');
  clearNode(groupsEl);
  emptyEl.hidden = filtered.length > 0;

  const order = [
    'M01-Umsetzungsverträge & Fixtures',
    'EP-00 Architektur & Koordination',
    'Abnahme- & Prüfberichte',
    'Verbindliche Grundlagen & Status',
    'Betrieb, Schnittstellen & Weitere Doku',
  ];

  for (const groupName of order) {
    const items = filtered.filter((d) => d.group === groupName);
    if (!items.length) continue;

    const section = el('div');
    section.appendChild(el('div', 'doc-group-title', `${groupName} (${items.length})`));
    const list = el('div', 'doc-group-items');

    for (const doc of items) {
      const btn = el('button', `doc-item-btn${doc.path === state.selectedDoc ? ' is-active' : ''}`);
      btn.type = 'button';
      btn.appendChild(el('span', 'doc-item-title', doc.title));
      btn.appendChild(el('span', 'doc-item-path', doc.path));
      btn.addEventListener('click', () => loadDocument(doc.path, true));
      list.appendChild(btn);
    }

    section.appendChild(list);
    groupsEl.appendChild(section);
  }
}

async function loadDocument(relPath, updateHash = false) {
  state.selectedDoc = relPath;
  renderDocList();

  const pathEl = document.getElementById('doc-current-path');
  const sizeEl = document.getElementById('doc-current-size');
  const rawLink = document.getElementById('doc-raw-link');
  const bodyEl = document.getElementById('doc-reader-body');

  pathEl.textContent = relPath;
  sizeEl.textContent = 'Lädt…';
  rawLink.href = `/docs/${encodeURI(relPath)}?raw=1`;
  clearNode(bodyEl);
  bodyEl.appendChild(el('p', 'muted-note', 'Dokument wird geladen…'));

  if (updateHash) {
    window.history.replaceState(null, '', `#doc=${encodeURIComponent(relPath)}`);
  }

  try {
    const doc = await fetchJson(`/preview-data/doc?path=${encodeURIComponent(relPath)}`);
    sizeEl.textContent = `${formatBytes(doc.sizeBytes)} · aktualisiert ${formatTime(doc.updatedAt)}`;
    if (doc.kind === 'json') {
      clearNode(bodyEl);
      bodyEl.appendChild(el('h1', null, doc.path));
      bodyEl.appendChild(createCodeBlock('json', doc.content));
    } else {
      renderMarkdownSafe(bodyEl, doc.content, doc.path);
    }
  } catch (err) {
    clearNode(bodyEl);
    bodyEl.appendChild(el('p', 'state-blocked', `Dokument konnte nicht geladen werden: ${err.message}`));
  }
}

function openDocument(relPath) {
  setActiveView('contracts', false);
  loadDocument(relPath, true);
}

function renderVerbund() {
  const v = state.fixtures?.verbund;
  if (!v) return;

  const q = state.verbundSearch.trim().toLowerCase();
  const tf = state.tenantFilter;

  const matchTenant = (leCode, siteCode, unitCode) => {
    if (tf === 'ALL') return true;
    if (tf === 'CENTRAL') {
      return siteCode === 'ZD-LAB' || unitCode === 'ZD-LAB-UNIT' || leCode === null;
    }
    return leCode === tf;
  };

  const units = v.org_units.filter((u) => {
    if (!matchTenant(u.legal_entity_code, u.site_code, u.code)) return false;
    if (!q) return true;
    return `${u.code} ${u.site_code} ${u.legal_entity_code} ${u.kind} ${u.parent_code || ''}`.toLowerCase().includes(q);
  });

  document.getElementById('verbund-units-count').textContent = `${units.length} von ${v.org_units.length} Einheiten`;
  const unitsBody = document.getElementById('verbund-units-tbody');
  clearNode(unitsBody);
  for (const u of units) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', u.code));
    tr.appendChild(el('td', 'mono', `${u.site_code} · ${u.legal_entity_code}`));
    tr.appendChild(el('td', null, u.care_level ? `${u.kind} (${u.care_level})` : u.kind));
    tr.appendChild(el('td', 'mono', u.parent_code || '—'));
    unitsBody.appendChild(tr);
  }

  const users = v.users.filter((u) => {
    const isCentral = u.code === 'U-PLAT' || u.code === 'U-LAB-Z';
    if (tf === 'CENTRAL' && !isCentral) return false;
    if ((tf === 'RT-NORD' || tf === 'RT-SUED') && u.legal_entity_code !== tf) return false;
    if (!q) return true;
    return `${u.code} ${u.email} ${u.legal_entity_code || 'plattform'} ${u.roles.join(' ')} ${u.unit_codes.join(' ')}`.toLowerCase().includes(q);
  });

  document.getElementById('verbund-users-count').textContent =
    `${users.length} Benutzer · ${v.patients.length} Patientenszenarien`;
  const usersBody = document.getElementById('verbund-users-tbody');
  clearNode(usersBody);
  for (const u of users) {
    const tr = el('tr');
    const codeTd = el('td');
    codeTd.appendChild(el('span', 'mono', u.code));
    codeTd.appendChild(el('span', 'cell-secondary mono', u.email));
    tr.appendChild(codeTd);
    tr.appendChild(el('td', 'mono', u.legal_entity_code || 'Plattform'));
    tr.appendChild(el('td', 'mono', u.roles.join(', ')));
    tr.appendChild(el('td', 'mono', u.unit_codes.length ? u.unit_codes.join(', ') : '—'));
    usersBody.appendChild(tr);
  }

  const nvts = v.negative_tests.filter((n) => {
    if (q) {
      const hay = `${n.id} ${n.package} ${n.actors.join(' ')} ${n.patient || ''} ${n.summary}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });

  document.getElementById('nvt-count').textContent = `${nvts.length} von ${v.negative_tests.length} Negativtests`;
  const nvtBody = document.getElementById('nvt-tbody');
  clearNode(nvtBody);
  for (const n of nvts) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', n.id));
    tr.appendChild(el('td', 'mono', n.package));
    tr.appendChild(el('td', 'mono', n.actors.join(', ')));
    tr.appendChild(el('td', 'mono', n.patient || '—'));
    tr.appendChild(el('td', 'mono', n.expected_status.join(' / ')));
    tr.appendChild(el('td', null, n.summary));
    nvtBody.appendChild(tr);
  }

  const emptyEl = document.getElementById('verbund-empty');
  emptyEl.hidden = units.length > 0 || users.length > 0 || nvts.length > 0;
}

function renderHandoff() {
  const playbookContainer = document.getElementById('playbook-commands');
  if (!playbookContainer.childElementCount) {
    for (const step of PLAYBOOK_STEPS) {
      const block = el('div', 'playbook-step');
      const head = el('div', 'playbook-step-head');
      head.appendChild(el('h3', null, step.title));
      block.appendChild(head);
      block.appendChild(el('p', null, step.desc));
      block.appendChild(createCodeBlock(step.lang, step.code));
      playbookContainer.appendChild(block);
    }
  }

  const reqs = state.fixtures?.normative?.requirements || [];
  const q = state.normSearch.trim().toLowerCase();
  const filtered = reqs.filter((r) => {
    if (state.normClass !== 'ALL' && r.native_class !== state.normClass) return false;
    if (q) {
      const hay = `${r.requirement_code} ${r.title} ${r.source_authority} ${r.legal_obligation} ${r.sector} ${r.status}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });

  document.getElementById('norm-count').textContent = `${filtered.length} von ${reqs.length} Einträgen`;
  const tbody = document.getElementById('norm-tbody');
  const empty = document.getElementById('norm-empty');
  clearNode(tbody);
  empty.hidden = filtered.length > 0;

  for (const r of filtered) {
    const tr = el('tr');
    tr.appendChild(el('td', 'mono', r.requirement_code));
    const titleTd = el('td');
    titleTd.appendChild(el('span', null, r.title));
    titleTd.appendChild(el('span', 'cell-secondary', `${r.source_authority} · ${r.spec_version}`));
    tr.appendChild(titleTd);
    tr.appendChild(el('td', 'mono', `${r.native_class} · ${r.sector}`));
    tr.appendChild(el('td', null, r.legal_obligation));
    tr.appendChild(el('td', statusTone(r.status), r.status));
    tr.appendChild(el('td', 'mono', r.next_review_date));
    tbody.appendChild(tr);
  }
}

function setActiveView(viewName, updateHash = true) {
  if (!VIEW_LABELS[viewName]) viewName = 'overview';
  state.view = viewName;

  for (const key of Object.keys(VIEW_LABELS)) {
    const panel = document.getElementById(`view-${key}`);
    if (panel) panel.hidden = key !== viewName;
  }

  for (const btn of document.querySelectorAll('.nav-item')) {
    btn.classList.toggle('is-active', btn.dataset.view === viewName);
  }

  document.getElementById('breadcrumb-current').textContent = VIEW_LABELS[viewName];

  // Mobile sidebar schließen
  document.getElementById('sidebar').classList.remove('is-open');
  document.getElementById('sidebar-backdrop').hidden = true;
  document.getElementById('menu-toggle').setAttribute('aria-expanded', 'false');

  if (updateHash) {
    window.history.replaceState(null, '', `#${viewName}`);
  }
}

async function refreshChecks(force = false) {
  const btn = document.getElementById('run-checks-btn');
  const tbody = document.getElementById('checks-tbody');
  btn.disabled = true;
  btn.textContent = 'Prüft…';
  clearNode(tbody);
  const tr = el('tr');
  const td = el('td', 'skeleton-row', 'Führe lokale Python-Prüfungen aus…');
  td.colSpan = 5;
  tr.appendChild(td);
  tbody.appendChild(tr);

  try {
    state.checks = await fetchJson(`/preview-data/checks${force ? '?refresh=1' : ''}`);
    renderChecks();
  } catch (err) {
    showError(`Prüflauf fehlgeschlagen: ${err.message}`);
  } finally {
    btn.disabled = false;
    btn.textContent = 'Lokale Prüfungen ausführen';
  }
}

function applyHashRoute() {
  const hash = window.location.hash.replace(/^#/, '');
  if (hash.startsWith('doc=')) {
    const rel = decodeURIComponent(hash.slice(4));
    setActiveView('contracts', false);
    loadDocument(rel, false);
    return;
  }
  if (VIEW_LABELS[hash]) {
    setActiveView(hash, false);
  } else {
    setActiveView('overview', false);
  }
}

async function init() {
  hideError();
  try {
    const [overview, fixtures] = await Promise.all([
      fetchJson('/preview-data/overview'),
      fetchJson('/preview-data/fixtures'),
    ]);
    state.overview = overview;
    state.fixtures = fixtures;

    renderOverview();
    renderRoadmap();
    renderDocList();
    renderVerbund();
    renderHandoff();

    applyHashRoute();
    if (state.view !== 'contracts') {
      loadDocument(state.selectedDoc, false);
    }
    await refreshChecks(false);
  } catch (err) {
    showError(err.message);
  }
}

function bindEvents() {
  for (const btn of document.querySelectorAll('.nav-item')) {
    btn.addEventListener('click', () => setActiveView(btn.dataset.view, true));
  }

  document.getElementById('retry-load-btn').addEventListener('click', init);
  document.getElementById('run-checks-btn').addEventListener('click', () => refreshChecks(true));

  const menuBtn = document.getElementById('menu-toggle');
  const sidebar = document.getElementById('sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  menuBtn.addEventListener('click', () => {
    const open = !sidebar.classList.contains('is-open');
    sidebar.classList.toggle('is-open', open);
    backdrop.hidden = !open;
    menuBtn.setAttribute('aria-expanded', String(open));
  });
  backdrop.addEventListener('click', () => {
    sidebar.classList.remove('is-open');
    backdrop.hidden = true;
    menuBtn.setAttribute('aria-expanded', 'false');
  });

  // Roadmap filters
  for (const btn of document.querySelectorAll('#milestone-filter .seg-btn')) {
    btn.addEventListener('click', () => {
      state.roadmapMilestone = btn.dataset.milestone;
      for (const b of document.querySelectorAll('#milestone-filter .seg-btn')) {
        b.classList.toggle('is-active', b === btn);
      }
      renderRoadmap();
    });
  }

  for (const btn of document.querySelectorAll('#prio-filter .seg-btn')) {
    btn.addEventListener('click', () => {
      state.roadmapPrio = btn.dataset.prio;
      for (const b of document.querySelectorAll('#prio-filter .seg-btn')) {
        b.classList.toggle('is-active', b === btn);
      }
      renderRoadmap();
    });
  }

  document.getElementById('roadmap-search').addEventListener('input', (ev) => {
    state.roadmapSearch = ev.target.value;
    renderRoadmap();
  });

  document.getElementById('reset-roadmap-btn').addEventListener('click', () => {
    state.roadmapMilestone = 'ALL';
    state.roadmapPrio = 'ALL';
    state.roadmapSearch = '';
    document.getElementById('roadmap-search').value = '';
    for (const b of document.querySelectorAll('#milestone-filter .seg-btn')) {
      b.classList.toggle('is-active', b.dataset.milestone === 'ALL');
    }
    for (const b of document.querySelectorAll('#prio-filter .seg-btn')) {
      b.classList.toggle('is-active', b.dataset.prio === 'ALL');
    }
    renderRoadmap();
  });

  // Docs search
  document.getElementById('doc-search').addEventListener('input', (ev) => {
    state.docSearch = ev.target.value;
    renderDocList();
  });

  document.getElementById('reset-doc-search-btn').addEventListener('click', () => {
    state.docSearch = '';
    document.getElementById('doc-search').value = '';
    renderDocList();
  });

  // Verbund filters
  for (const btn of document.querySelectorAll('#tenant-filter .seg-btn')) {
    btn.addEventListener('click', () => {
      state.tenantFilter = btn.dataset.tenant;
      for (const b of document.querySelectorAll('#tenant-filter .seg-btn')) {
        b.classList.toggle('is-active', b === btn);
      }
      renderVerbund();
    });
  }

  document.getElementById('verbund-search').addEventListener('input', (ev) => {
    state.verbundSearch = ev.target.value;
    renderVerbund();
  });

  document.getElementById('reset-verbund-btn').addEventListener('click', () => {
    state.tenantFilter = 'ALL';
    state.verbundSearch = '';
    document.getElementById('verbund-search').value = '';
    for (const b of document.querySelectorAll('#tenant-filter .seg-btn')) {
      b.classList.toggle('is-active', b.dataset.tenant === 'ALL');
    }
    renderVerbund();
  });

  document.getElementById('open-fixture-doc-btn').addEventListener('click', () => {
    openDocument('vertraege/verbund_test_fixture.json');
  });

  // Normative filters & Playbook link
  for (const btn of document.querySelectorAll('#norm-class-filter .seg-btn')) {
    btn.addEventListener('click', () => {
      state.normClass = btn.dataset.class;
      for (const b of document.querySelectorAll('#norm-class-filter .seg-btn')) {
        b.classList.toggle('is-active', b === btn);
      }
      renderHandoff();
    });
  }

  document.getElementById('norm-search').addEventListener('input', (ev) => {
    state.normSearch = ev.target.value;
    renderHandoff();
  });

  document.getElementById('reset-norm-btn').addEventListener('click', () => {
    state.normClass = 'ALL';
    state.normSearch = '';
    document.getElementById('norm-search').value = '';
    for (const b of document.querySelectorAll('#norm-class-filter .seg-btn')) {
      b.classList.toggle('is-active', b.dataset.class === 'ALL');
    }
    renderHandoff();
  });

  document.getElementById('open-playbook-btn').addEventListener('click', () => {
    openDocument('UEBERGABE_GIT_CI.md');
  });

  window.addEventListener('hashchange', applyHashRoute);
}

bindEvents();
init();
