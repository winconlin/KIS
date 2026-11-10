/**
 * KIS — Arbeitsplatz: die tägliche Arbeitsansicht je Einheit.
 *
 * Eine Seite für alle Berufsgruppen. Welche Ansichten erscheinen, hängt
 * von den Rechten des angemeldeten Benutzers ab (/api/v1/me); welche
 * Daten er sieht, entscheidet der Server (Behandlungszusammenhang).
 *
 * Sicherheit: Inhalte werden ausschließlich mit textContent gesetzt
 * (kein innerHTML). Die Seite kommt ohne Inline-Skripte aus (CSP).
 */
(function () {
    'use strict';

    const token = () => localStorage.getItem('kis_access_token');
    const $ = (id) => document.getElementById(id);
    const state = { me: null, unit: null, tab: null, caseId: null };

    /** Ansichten: Bezeichnung, benötigtes Recht (eines genügt), Ladefunktion. */
    const TABS = [
        { id: 'census', label: 'Belegung', needs: ['case.read'], load: loadCensus },
        { id: 'handover', label: 'Übergabe', needs: ['clinical.read', 'nursing.read'], load: loadHandover },
        { id: 'tasks', label: 'Aufgaben', needs: ['clinical.read', 'nursing.read'], load: loadTasks },
        { id: 'worklist', label: 'Arbeitsliste', needs: ['order.process'], load: loadWorklist },
        { id: 'beds', label: 'Betten', needs: ['case.read'], load: loadBeds },
        { id: 'kitchen', label: 'Küche', needs: ['meal.read'], load: loadKitchen },
    ];

    async function api(method, path, body) {
        const res = await fetch(path, {
            method,
            headers: Object.assign(
                { Authorization: 'Bearer ' + token() },
                body ? { 'Content-Type': 'application/json' } : {}
            ),
            body: body ? JSON.stringify(body) : undefined,
        });
        if (res.status === 401) {
            location.href = '/';
            throw new Error('Nicht angemeldet');
        }
        const data = await res.json().catch(() => ({}));
        if (!res.ok) throw new Error(data.error || 'Fehler ' + res.status);
        return data;
    }

    function el(tag, text, className) {
        const node = document.createElement(tag);
        if (text !== undefined && text !== null) node.textContent = String(text);
        if (className) node.className = className;
        return node;
    }

    function say(text) {
        $('message').textContent = text || '';
        $('message').hidden = !text;
    }

    const can = (perms) => perms.some((p) => state.me.permissions.includes(p));
    const date = (v) => (v ? new Date(v).toLocaleDateString('de-DE') : '');
    const dateTime = (v) =>
        v ? new Date(v).toLocaleString('de-DE', { dateStyle: 'short', timeStyle: 'short' }) : '';
    const person = (r) => (r.nameFamily ? r.nameFamily + ', ' + r.nameGiven : '');
    const place = (r) => [r.wardName, r.roomName, r.bedLabel].filter(Boolean).join(' / ');

    /** Tabelle aus Spalten [Überschrift, Zellfunktion]; Zeilenklick optional. */
    function table(rows, columns, onClick) {
        if (!rows.length) return el('p', 'Keine Einträge.');
        const t = el('table');
        const head = t.createTHead().insertRow();
        columns.forEach(([title]) => head.appendChild(el('th', title)));
        const body = t.createTBody();
        rows.forEach((row) => {
            const tr = body.insertRow();
            if (onClick) {
                tr.className = 'ap-click';
                tr.tabIndex = 0;
                tr.addEventListener('click', () => onClick(row));
                tr.addEventListener('keydown', (e) => e.key === 'Enter' && onClick(row));
            }
            columns.forEach(([, cell]) => {
                const td = tr.insertCell();
                const value = cell(row);
                if (value instanceof Node) td.appendChild(value);
                else td.textContent = value === null || value === undefined ? '' : String(value);
            });
        });
        return t;
    }

    function tags(row) {
        const box = el('span');
        const add = (on, text, cls) => on && box.appendChild(el('span', text, 'ap-tag ' + cls));
        add(row.stability === 'instabil', 'instabil', 'ap-warn');
        add(row.stability === 'beobachtung', 'Beobachtung', 'ap-note');
        add(row.fasting, 'nüchtern', 'ap-note');
        add(row.dnr, 'DNR', 'ap-warn');
        add(row.dni, 'DNI', 'ap-warn');
        add(row.isolation, 'Isolation', 'ap-warn');
        add(row.alertCount > 0, row.alertCount + ' Hinweis(e)', 'ap-note');
        return box;
    }

    function show(node) {
        $('list').replaceChildren(node);
    }

    // --- Ansichten -----------------------------------------------------------

    async function loadCensus() {
        const data = await api('GET', `/api/v1/units/${state.unit}/census`);
        show(table(data.items, [
            ['Ort', place],
            ['Patient', person],
            ['Geb.', (r) => date(r.birthDate)],
            ['Fall', (r) => r.caseNumber],
            ['Aufnahme', (r) => date(r.admissionAt)],
            ['Status', tags],
            ['Wenn–dann', (r) => r.contingency],
            ['Entlassung geplant', (r) => date(r.plannedDischarge)],
        ], (r) => openCase(r.caseId)));
    }

    async function loadHandover() {
        const data = await api('GET', `/api/v1/units/${state.unit}/handover`);
        show(table(data.items, [
            ['Ort', place],
            ['Patient', person],
            ['Status', tags],
            ['Diagnose', (r) => r.leadingDiagnosis],
            ['Wenn–dann', (r) => r.contingency],
            ['Offen', (r) => (r.openTasks || []).join(' · ')],
            ['Letzter Verlauf', (r) => r.lastNote],
        ], (r) => openCase(r.caseId)));
    }

    async function loadTasks() {
        const data = await api('GET', `/api/v1/units/${state.unit}/tasks`);
        show(table(data.items, [
            ['Patient', person],
            ['Aufgabe', (r) => r.text],
            ['Zuständig', (r) => r.assigneeLabel],
            ['Fällig', (r) => date(r.dueDate)],
            ['', (r) => actionButton('Erledigt', () =>
                api('POST', `/api/v1/cases/${r.caseId}/tasks/${r.id}/done`, { version: r.version }))],
        ]));
    }

    async function loadWorklist() {
        const data = await api('GET', `/api/v1/units/${state.unit}/worklist`);
        const base = `/api/v1/units/${state.unit}/worklist/`;
        show(table(data.items, [
            ['Dringlichkeit', (r) => r.priority],
            ['Leistung', (r) => r.serviceName],
            ['Ort', place],
            ['Patient', person],
            ['Auftrag', (r) => r.details],
            ['Von', (r) => r.requestingUnitName],
            ['Angefordert', (r) => dateTime(r.requestedAt)],
            ['Status', (r) => r.status],
            ['', (r) => {
                const box = el('span');
                if (r.status === 'angefordert') {
                    box.appendChild(actionButton('Annehmen', () =>
                        api('POST', base + r.id + '/accept', { version: r.version })));
                }
                box.appendChild(actionButton('Erledigt', () => {
                    const resultText = window.prompt('Ergebnis / Befund (leer, wenn keiner nötig):') || undefined;
                    return api('POST', base + r.id + '/complete', { version: r.version, resultText });
                }));
                return box;
            }],
        ]));
    }

    async function loadBeds() {
        const data = await api('GET', `/api/v1/units/${state.unit}/beds`);
        show(table(data.items, [
            ['Station', (r) => r.wardName],
            ['Zimmer', (r) => r.roomName],
            ['Bett', (r) => r.label],
            ['Belegung', (r) => {
                if (r.blockedReason) return el('span', 'gesperrt: ' + r.blockedReason, 'ap-tag ap-warn');
                if (r.caseId) return person(r) + ' (' + r.caseNumber + ')';
                return el('span', 'frei', 'ap-tag ap-ok');
            }],
        ]));
    }

    async function loadKitchen() {
        const data = await api('GET', '/api/v1/meals/production');
        const box = el('div');
        box.appendChild(el('p', `${data.trays} Tabletts, ${data.fasting} nüchtern, ${data.withoutDiet} ohne Kostform`));
        box.appendChild(table(data.totals, [
            ['Station', (r) => r.wardName], ['Kostform', (r) => r.dietName], ['Anzahl', (r) => r.count],
        ]));
        box.appendChild(el('h3', 'Je Bett'));
        box.appendChild(table(data.items, [
            ['Ort', place],
            ['Kostform', (r) => (r.fasting ? 'nüchtern' : r.dietName)],
            ['Hinweis', (r) => r.note],
            ['Nahrungsmittelallergien', (r) => (r.foodAllergies || []).join(', ')],
            ['Isolation', (r) => (r.isolation ? 'ja' : '')],
        ]));
        show(box);
    }

    function actionButton(label, action) {
        const button = el('button', label);
        button.type = 'button';
        button.addEventListener('click', async (event) => {
            event.stopPropagation();
            button.disabled = true;
            try {
                await action();
                say('');
                await refresh();
            } catch (e) {
                say(e.message);
                button.disabled = false;
            }
        });
        return button;
    }

    // --- Fall ----------------------------------------------------------------

    /** Lädt einen Abschnitt; fehlt das Recht oder das Modul, bleibt er leer. */
    async function section(panel, title, path, line) {
        try {
            const data = await api('GET', path);
            const items = data.items || [];
            panel.appendChild(el('h3', `${title} (${items.length})`));
            const list = el('ul');
            items.slice(0, 20).forEach((item) => list.appendChild(el('li', line(item))));
            panel.appendChild(list);
        } catch (e) {
            /* Abschnitt nicht verfügbar: kein Recht oder Modul abgeschaltet */
        }
    }

    async function openCase(caseId) {
        state.caseId = caseId;
        const panel = $('detail');
        panel.hidden = false;
        panel.replaceChildren(el('p', 'Lädt …'));
        const base = `/api/v1/cases/${caseId}`;
        let c;
        try {
            c = await api('GET', base);
        } catch (e) {
            panel.replaceChildren(el('p', e.message));
            return;
        }
        panel.replaceChildren(
            el('h2', `${c.nameFamily}, ${c.nameGiven} — ${c.caseNumber}`),
            el('p', `geb. ${date(c.birthDate)} · ${place(c) || c.departmentName} · seit ${date(c.admissionAt)}`),
            tags(c)
        );
        if (c.alerts.length) {
            panel.appendChild(el('h3', 'Warnhinweise'));
            const list = el('ul');
            c.alerts.forEach((a) => list.appendChild(el('li', `${a.kind}: ${a.text}`, 'ap-warn')));
            panel.appendChild(list);
        }
        await section(panel, 'Diagnosen', base + '/diagnoses', (d) => `${d.icdCode || ''} ${d.text} (${d.kind})`);
        await section(panel, 'Aufgaben', base + '/tasks', (t) => t.text + (t.dueDate ? ' — bis ' + date(t.dueDate) : ''));
        if (can(['task.write'])) {
            panel.appendChild(quickForm('Neue Aufgabe', [['text', 'Aufgabe']], (v) => api('POST', base + '/tasks', v)));
        }
        await section(panel, 'Vitalwerte', base + '/vitals?limit=5', (v) =>
            `${dateTime(v.measuredAt)}: RR ${v.systolic || '–'}/${v.diastolic || '–'}, HF ${v.heartRate || '–'}, ` +
            `T ${v.temperature || '–'}, SpO₂ ${v.spo2 || '–'}`);
        if (can(['nursing.write', 'clinical.write'])) {
            panel.appendChild(quickForm('Vitalwerte', [
                ['systolic', 'RR syst.'], ['diastolic', 'RR diast.'], ['heartRate', 'HF'],
                ['temperature', 'Temp.'], ['spo2', 'SpO₂'],
            ], (v) => api('POST', base + '/vitals', numbers(v))));
        }
        await section(panel, 'Verordnungen', base + '/prescriptions', (p) =>
            `${p.drugName} ${p.dose || ''} ${p.schedule || ''} (${p.status})` +
            (p.therapyDay ? ` — Tag ${p.therapyDay}${p.overdue ? ', überfällig' : ''}` : ''));
        await section(panel, 'Aufträge', base + '/orders', (o) =>
            `${o.serviceName}: ${o.status}` + (o.resultText ? ' — ' + o.resultText : ''));
        await section(panel, 'Verlauf', base + '/notes', (n) =>
            `${dateTime(n.createdAt)} ${n.kind}${n.status === 'entwurf' ? ' (Entwurf)' : ''}: ${n.body}`);
    }

    function numbers(values) {
        const out = {};
        Object.entries(values).forEach(([k, v]) => {
            if (v !== '') out[k] = Number(String(v).replace(',', '.'));
        });
        return out;
    }

    function quickForm(label, fields, submit) {
        const form = el('form');
        form.setAttribute('aria-label', label);
        fields.forEach(([name, placeholder]) => {
            const input = el('input');
            input.name = name;
            input.placeholder = placeholder;
            input.setAttribute('aria-label', placeholder);
            form.appendChild(input);
        });
        const button = el('button', label);
        form.appendChild(button);
        form.addEventListener('submit', async (event) => {
            event.preventDefault();
            const values = {};
            new FormData(form).forEach((v, k) => { values[k] = String(v).trim(); });
            button.disabled = true;
            try {
                await submit(values);
                say('');
                await openCase(state.caseId);
                await refresh();
            } catch (e) {
                say(e.message);
                button.disabled = false;
            }
        });
        return form;
    }

    // --- Rahmen --------------------------------------------------------------

    async function refresh() {
        const tab = TABS.find((t) => t.id === state.tab);
        if (!tab) return;
        if (tab.id !== 'kitchen' && !state.unit) {
            show(el('p', 'Bitte eine Einheit wählen.'));
            return;
        }
        try {
            await tab.load();
        } catch (e) {
            show(el('p', e.message));
        }
    }

    function selectTab(id) {
        state.tab = id;
        $('tabs').querySelectorAll('button').forEach((b) =>
            b.setAttribute('aria-current', String(b.dataset.tab === id)));
        refresh();
    }

    async function start() {
        if (!token()) {
            location.href = '/';
            return;
        }
        state.me = await api('GET', '/api/v1/me');
        $('who').textContent = 'Rolle: ' + state.me.role;

        // Eigene Einheiten; wer hausweit arbeitet, wählt aus allen.
        let units = (await api('GET', '/api/v1/me/units')).items;
        if (!units.length && can(['unit.access_all'])) {
            units = (await api('GET', '/api/v1/org/units').catch(() => ({ items: [] }))).items;
        }
        const select = $('unit');
        units.forEach((u) => {
            const option = el('option', `${u.name} (${u.code})`);
            option.value = u.id;
            select.appendChild(option);
        });
        const preferred = units.find((u) => u.kind === 'station') || units[0];
        if (preferred) {
            select.value = preferred.id;
            state.unit = preferred.id;
        }
        select.addEventListener('change', () => {
            state.unit = select.value;
            $('detail').hidden = true;
            refresh();
        });

        const available = TABS.filter((t) => can(t.needs));
        available.forEach((t) => {
            const button = el('button', t.label);
            button.type = 'button';
            button.dataset.tab = t.id;
            button.addEventListener('click', () => selectTab(t.id));
            $('tabs').appendChild(button);
        });
        // Ohne eigene Einheit mit einer Ansicht beginnen, die keine braucht.
        const first = available.find((t) => state.unit || t.id === 'kitchen') || available[0];
        if (first) selectTab(first.id);
        else show(el('p', 'Für diese Rolle ist hier keine Ansicht eingerichtet.'));
    }

    start().catch((e) => say(e.message));
})();
