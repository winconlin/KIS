/**
 * KIS — Krankenhausinformationssystem
 * Haupt-SPA: Hash-Router, Views, Toast-System, Modal-System
 *
 * Views:
 *  - #/dashboard
 *  - #/patients
 *  - #/patients/:id
 *  - #/appointments
 *  - #/settings
 */

(function (global) {
    'use strict';

    // --------------------------------------------------------
    // Toast-System
    // --------------------------------------------------------
    const KISToast = (function () {
        let container = null;

        function init() {
            container = document.getElementById('toast-container');
        }

        /**
         * Zeigt eine Toast-Meldung.
         * @param {string} message
         * @param {'success'|'error'|'warning'|'info'} [type]
         * @param {string} [title]
         * @param {number} [duration] - ms, default 5000
         */
        function show(message, type, title, duration) {
            if (!container) container = document.getElementById('toast-container');
            if (!container) return;

            type = type || 'info';
            duration = duration || 5000;

            const icons = {
                success: '<svg class="toast-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>',
                error: '<svg class="toast-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>',
                warning: '<svg class="toast-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>',
                info: '<svg class="toast-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>'
            };

            const titles = {
                success: 'Erfolg',
                error: 'Fehler',
                warning: 'Warnung',
                info: 'Information'
            };

            const el = document.createElement('div');
            el.className = 'toast ' + type;
            el.setAttribute('role', 'alert');
            el.innerHTML =
                (icons[type] || icons.info) +
                '<div class="toast-content">' +
                '<div class="toast-title">' + escapeHtml(title || titles[type]) + '</div>' +
                '<div class="toast-message">' + escapeHtml(message) + '</div>' +
                '</div>' +
                '<button class="toast-close" aria-label="Schließen">' +
                '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>' +
                '</button>';

            const closeBtn = el.querySelector('.toast-close');
            closeBtn.addEventListener('click', function () {
                dismiss(el);
            });

            container.appendChild(el);

            // Auto-Dismiss
            setTimeout(function () {
                dismiss(el);
            }, duration);
        }

        function dismiss(el) {
            if (!el.parentNode) return;
            el.style.animation = 'slideOut 0.3s ease forwards';
            el.addEventListener('animationend', function () {
                if (el.parentNode) el.parentNode.removeChild(el);
            });
        }

        return { init: init, show: show };
    })();

    // --------------------------------------------------------
    // Modal-System
    // --------------------------------------------------------
    const KISModal = (function () {
        let container = null;

        function init() {
            container = document.getElementById('modal-container');
        }

        /**
         * Öffnet einen Modal mit dem gegebenen HTML-Inhalt.
         * @param {string} title
         * @param {string} bodyHtml
         * @param {Object} [options] - { footerHtml, onOpen }
         * @returns {HTMLElement} das Modal-Element
         */
        function open(title, bodyHtml, options) {
            options = options || {};
            if (!container) container = document.getElementById('modal-container');
            if (!container) return null;

            // Vorherigen Inhalt leeren
            container.innerHTML = '';
            container.setAttribute('aria-hidden', 'false');

            const modal = document.createElement('div');
            modal.className = 'modal';
            modal.setAttribute('role', 'dialog');
            modal.setAttribute('aria-modal', 'true');
            modal.setAttribute('aria-labelledby', 'modal-title');

            const footer = options.footerHtml
                ? '<div class="modal-footer">' + options.footerHtml + '</div>'
                : '<div class="modal-footer"><button class="btn btn-secondary" data-modal-close>Abbrechen</button></div>';

            modal.innerHTML =
                '<div class="modal-header">' +
                '<h2 class="modal-title" id="modal-title">' + escapeHtml(title) + '</h2>' +
                '<button class="btn-icon" data-modal-close aria-label="Schließen">' +
                '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>' +
                '</button>' +
                '</div>' +
                '<div class="modal-body">' + bodyHtml + '</div>' +
                footer;

            container.appendChild(modal);

            // Close-Buttons
            modal.querySelectorAll('[data-modal-close]').forEach(function (btn) {
                btn.addEventListener('click', close);
            });

            // ESC schließt
            document.addEventListener('keydown', escHandler);

            // Overlay-Klick schließt
            container.addEventListener('click', function backdropClick(e) {
                if (e.target === container) close();
            });

            if (typeof options.onOpen === 'function') {
                options.onOpen(modal);
            }

            // Fokussierung
            const firstInput = modal.querySelector('input, select, textarea, button');
            if (firstInput) firstInput.focus();

            return modal;
        }

        function close() {
            if (!container) return;
            container.innerHTML = '';
            container.setAttribute('aria-hidden', 'true');
            document.removeEventListener('keydown', escHandler);
        }

        function escHandler(e) {
            if (e.key === 'Escape') close();
        }

        return { init: init, open: open, close: close };
    })();

    // --------------------------------------------------------
    // HTML-Hilfsfunktionen
    // --------------------------------------------------------

    /**
     * Escapt HTML-Sonderzeichen.
     * @param {string} str
     * @returns {string}
     */
    function escapeHtml(str) {
        if (str === null || str === undefined) return '';
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#039;');
    }

    /**
     * Formatiert ein Datum (ISO → DD.MM.YYYY).
     * @param {string} iso
     * @returns {string}
     */
    function formatDate(iso) {
        if (!iso) return '—';
        try {
            const d = new Date(iso);
            if (isNaN(d.getTime())) return iso;
            return d.toLocaleDateString('de-DE', { year: 'numeric', month: '2-digit', day: '2-digit' });
        } catch (e) {
            return iso;
        }
    }

    /**
     * Formatiert Datum + Zeit.
     * @param {string} iso
     * @returns {string}
     */
    function formatDateTime(iso) {
        if (!iso) return '—';
        try {
            const d = new Date(iso);
            if (isNaN(d.getTime())) return iso;
            return d.toLocaleString('de-DE', {
                year: 'numeric', month: '2-digit', day: '2-digit',
                hour: '2-digit', minute: '2-digit'
            });
        } catch (e) {
            return iso;
        }
    }

    /**
     * Formatiert eine Uhrzeit.
     * @param {string} iso
     * @returns {string}
     */
    function formatTime(iso) {
        if (!iso) return '—';
        try {
            const d = new Date(iso);
            if (isNaN(d.getTime())) return iso;
            return d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
        } catch (e) {
            return iso;
        }
    }

    // --------------------------------------------------------
    // Router
    // --------------------------------------------------------

    const Router = (function () {
        let contentEl = null;
        let topbarTitle = null;
        let currentRoute = null;
        let routes = {};

        function init() {
            contentEl = document.getElementById('content');
            topbarTitle = document.getElementById('topbar-title');

            // Routen registrieren
            routes = {
                'dashboard': { view: renderDashboard, title: 'Dashboard' },
                'patients': { view: renderPatients, title: 'Patienten' },
                'patient-detail': { view: renderPatientDetail, title: 'Patienten-Detail' },
                'appointments': { view: renderAppointments, title: 'Termine' },
                'settings': { view: renderSettings, title: 'Einstellungen' }
            };

            window.addEventListener('hashchange', handleRouteChange);

            // Sidebar-Links
            document.querySelectorAll('.sidebar-link').forEach(function (link) {
                link.addEventListener('click', function (e) {
                    // Hash wird automatisch gesetzt, nur Active-State aktualisieren
                    setActiveLink(this);
                });
            });
        }

        /**
         * Startet den Router (erste Route).
         */
        function start() {
            init();
            handleRouteChange();
        }

        function stop() {
            // Aufräum-Logik
        }

        /**
         * Analysiert den Hash und rendert die passende View.
         */
        function handleRouteChange() {
            const hash = window.location.hash || '#/dashboard';
            const path = hash.replace(/^#/, '');

            // Pfadsegmente
            const segments = path.split('/').filter(Boolean); // z. B. ['patients', '123']

            let routeName = null;
            let params = {};

            if (segments.length === 0) {
                routeName = 'dashboard';
            } else if (segments[0] === 'patients') {
                if (segments.length >= 2) {
                    routeName = 'patient-detail';
                    params.id = decodeURIComponent(segments[1]);
                } else {
                    routeName = 'patients';
                }
            } else if (segments[0] === 'appointments') {
                routeName = 'appointments';
            } else if (segments[0] === 'settings') {
                routeName = 'settings';
            } else {
                routeName = 'dashboard';
            }

            // Title aktualisieren
            const route = routes[routeName];
            if (route && topbarTitle) {
                topbarTitle.textContent = route.title;
            }

            // Sidebar-Active
            const activeRoute = (routeName === 'patient-detail') ? 'patients' : routeName;
            document.querySelectorAll('.sidebar-link').forEach(function (link) {
                const linkRoute = link.getAttribute('data-route');
                if (linkRoute === activeRoute) {
                    link.classList.add('active');
                } else {
                    link.classList.remove('active');
                }
            });

            // Ladezustand anzeigen
            showLoading();

            // View rendern (asynchron)
            if (route) {
                Promise.resolve(route.view(params)).then(function (html) {
                    if (contentEl) {
                        contentEl.innerHTML = html;
                        bindViewEvents(routeName, params);
                    }
                }).catch(function (err) {
                    console.error('Render-Fehler:', err);
                    if (contentEl) {
                        contentEl.innerHTML = renderErrorState(err);
                    }
                });
            }

            currentRoute = routeName;
        }

        function setActiveLink(link) {
            // Nur visuelle Markierung — der Hash-Change übernimmt den Rest
        }

        function showLoading() {
            if (contentEl) {
                contentEl.innerHTML =
                    '<div class="loading">' +
                    '<div class="loading-spinner"></div>' +
                    '<span class="loading-text">Laden…</span>' +
                    '</div>';
            }
        }

        // --------------------------------------------------------
        // View-Event-Binding (delegiert)
        // --------------------------------------------------------
        function bindViewEvents(routeName, params) {
            if (!contentEl) return;
            switch (routeName) {
                case 'patients':
                    bindPatientsEvents();
                    break;
                case 'patient-detail':
                    bindPatientDetailEvents(params);
                    break;
                case 'appointments':
                    bindAppointmentsEvents(params);
                    break;
                case 'settings':
                    bindSettingsEvents();
                    break;
                case 'dashboard':
                    // Keine besonderen Events
                    break;
            }
        }

        // --------------------------------------------------------
        // Öffentliche API
        // --------------------------------------------------------
        return {
            init: init,
            start: start,
            stop: stop,
            navigate: function (hash) {
                window.location.hash = hash;
            }
        };
    })();

    // --------------------------------------------------------
    // View: Dashboard
    // --------------------------------------------------------
    async function renderDashboard() {
        let statsHtml = '';
        let modulesHtml = '';
        let auditHtml = '';

        // Paralleles Laden von Stats, Modulen und Audit
        try {
            const stats = await global.KISApi.getDashboardStats();
            statsHtml = renderStats(stats);
        } catch (e) {
            statsHtml = renderCardGridFallback('Statistiken nicht verfügbar.');
        }

        try {
            const modules = await global.KISApi.getModuleStatus();
            modulesHtml = renderModuleStatus(modules);
        } catch (e) {
            modulesHtml = '<div class="empty-state"><div class="empty-state-title">Module-Status nicht verfügbar</div></div>';
        }

        try {
            const audit = await global.KISApi.getAuditLog({ limit: 8 });
            auditHtml = renderAuditLog(audit);
        } catch (e) {
            auditHtml = '<div class="empty-state"><div class="empty-state-title">Audit-Log nicht verfügbar</div></div>';
        }

        return '' +
            '<div class="card-grid">' + statsHtml + '</div>' +
            '<div class="detail-grid">' +
            '<div class="card">' +
            '<div class="card-header"><h2 class="card-title">Module-Status</h2></div>' +
            modulesHtml +
            '</div>' +
            '<div class="card">' +
            '<div class="card-header"><h2 class="card-title">Letzte Audit-Einträge</h2></div>' +
            auditHtml +
            '</div>' +
            '</div>';
    }

    function renderStats(stats) {
        if (!stats) return renderCardGridFallback('Keine Daten.');
        // stats: { patients_total, patients_active, appointments_today, appointments_week, gdt_last_export, medico_sync_status }
        const cards = [];

        const items = [
            { label: 'Gesamt-Patienten', value: stats.patients_total, sub: stats.patients_active + ' aktiv', icon: 'users', color: 'accent' },
            { label: 'Termine heute', value: stats.appointments_today, sub: stats.appointments_week + ' diese Woche', icon: 'calendar', color: 'info' },
            { label: 'Letzter GDT-Export', value: stats.gdt_last_export ? formatDate(stats.gdt_last_export) : '—', sub: 'GDT-Schnittstelle', icon: 'file', color: 'success' },
            { label: 'MEDICO-Sync', value: stats.medico_sync_status || '—', sub: 'CGM MEDICO Migration', icon: 'sync', color: 'warning' }
        ];

        items.forEach(function (item) {
            cards.push(
                '<div class="card">' +
                '<div class="card-header">' +
                '<span class="card-title">' + escapeHtml(item.label) + '</span>' +
                '</div>' +
                '<div class="card-value">' + escapeHtml(String(item.value)) + '</div>' +
                (item.sub ? '<div class="card-sub">' + escapeHtml(item.sub) + '</div>' : '') +
                '</div>'
            );
        });

        return cards.join('');
    }

    function renderCardGridFallback(msg) {
        return '<div class="card"><div class="card-sub">' + escapeHtml(msg) + '</div></div>';
    }

    function renderModuleStatus(modules) {
        if (!modules) return '<div class="empty-state"><div class="empty-state-title">Keine Daten</div></div>';

        // modules: Array oder Object
        let list = [];
        if (Array.isArray(modules)) {
            list = modules;
        } else if (modules.modules) {
            list = modules.modules;
        } else {
            list = Object.keys(modules).map(function (key) {
                return { name: key, status: modules[key] };
            });
        }

        return '<div class="module-status-list">' + list.map(function (m) {
            const status = (m.status || 'unknown').toLowerCase();
            const statusClass = (status === 'online' || status === 'active' || status === 'ok') ? 'online'
                : (status === 'warning' || status === 'degraded') ? 'warning' : 'offline';
            const statusLabel = statusClass === 'online' ? 'Online' : statusClass === 'warning' ? 'Eingeschränkt' : 'Offline';
            return '' +
                '<div class="module-status-item">' +
                '<div class="module-status-icon ' + statusClass + '">' +
                '<span class="status-dot ' + statusClass + '"></span>' +
                '</div>' +
                '<div class="module-status-info">' +
                '<div class="module-status-name">' + escapeHtml(m.name || m.module || 'Unbekannt') + '</div>' +
                '<div class="module-status-desc">' + escapeHtml(statusLabel) + '</div>' +
                '</div>' +
                '</div>';
        }).join('') + '</div>';
    }

    function renderAuditLog(audit) {
        if (!audit) return '<div class="empty-state"><div class="empty-state-title">Keine Einträge</div></div>';

        let entries = [];
        if (Array.isArray(audit)) entries = audit;
        else if (audit.entries) entries = audit.entries;
        else if (audit.data) entries = audit.data;

        if (!entries.length) {
            return '<div class="empty-state"><div class="empty-state-icon">📋</div><div class="empty-state-title">Keine Audit-Einträge</div></div>';
        }

        return '<ul class="audit-trail">' + entries.map(function (a) {
            const action = a.action || a.type || 'Unbekannt';
            const actor = a.actor || a.user || 'System';
            const when = a.timestamp || a.created_at || '';
            const target = a.target || a.entity || '';
            return '' +
                '<li class="audit-item">' +
                '<div class="audit-icon">📝</div>' +
                '<div class="audit-content">' +
                '<div class="audit-action">' + escapeHtml(action) + (target ? ' — ' + escapeHtml(target) : '') + '</div>' +
                '<div class="audit-meta">' + escapeHtml(actor) + ' · ' + escapeHtml(formatDateTime(when)) + '</div>' +
                '</div>' +
                '</li>';
        }).join('') + '</ul>';
    }

    function renderErrorState(err) {
        return '<div class="empty-state">' +
            '<div class="empty-state-icon">⚠️</div>' +
            '<div class="empty-state-title">Fehler beim Laden</div>' +
            '<div class="empty-state-text">' + escapeHtml(err.message || 'Unbekannter Fehler') + '</div>' +
            '</div>';
    }

    // --------------------------------------------------------
    // View: Patienten-Liste
    // --------------------------------------------------------
    let patientState = { page: 1, per_page: 20, q: '', status: '' };

    async function renderPatients(params) {
        let data;
        try {
            data = await global.KISApi.listPatients({
                q: patientState.q,
                page: patientState.page,
                per_page: patientState.per_page,
                status: patientState.status || undefined
            });
        } catch (e) {
            return renderErrorState(e);
        }

        const patients = extractList(data, 'patients');
        const total = extractTotal(data, patients.length);
        const pages = Math.max(1, Math.ceil(total / patientState.per_page));

        return '' +
            '<div class="search-bar">' +
            '<div class="search-input-wrapper">' +
            '<svg class="search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>' +
            '<input type="search" class="form-input" id="patient-search" placeholder="Patienten suchen (Name, ID)…" ' +
            'value="' + escapeHtml(patientState.q) + '" aria-label="Patienten suchen">' +
            '</div>' +
            '<div class="filter-group">' +
            '<label class="filter-label" for="patient-status-filter">Status:</label>' +
            '<select class="form-select" id="patient-status-filter" aria-label="Status-Filter">' +
            '<option value="">Alle</option>' +
            '<option value="active"' + (patientState.status === 'active' ? ' selected' : '') + '>Aktiv</option>' +
            '<option value="inactive"' + (patientState.status === 'inactive' ? ' selected' : '') + '>Inaktiv</option>' +
            '</select>' +
            '</div>' +
            '<button class="btn btn-primary" id="patient-new-btn">+ Neuer Patient</button>' +
            '</div>' +
            '<div class="table-wrapper">' +
            (patients.length ? renderPatientsTable(patients) : renderEmptyState('Keine Patienten gefunden', 'Passen Sie die Suche an oder legen Sie einen neuen Patienten an.')) +
            '</div>' +
            renderPagination(pages, patientState.page, total, 'patient');
    }

    function extractList(data, key) {
        if (!data) return [];
        if (Array.isArray(data)) return data;
        if (data.patients) return data.patients;
        if (data.items) return data.items;
        if (data.data) return extractList(data.data, key);
        return [];
    }

    function extractTotal(data, fallback) {
        if (!data) return fallback;
        if (typeof data === 'object' && !Array.isArray(data)) {
            if (data.total !== undefined) return data.total;
            if (data.total_count !== undefined) return data.total_count;
            if (data.count !== undefined) return data.count;
            if (data.meta && data.meta.total !== undefined) return data.meta.total;
        }
        return fallback;
    }

    function renderPatientsTable(patients) {
        const rows = patients.map(function (p) {
            const id = p.id || p.patient_id;
            const name = (p.last_name || p.nachname || '') + ' ' + (p.first_name || p.vorname || '');
            const dob = p.birth_date || p.geburtsdatum || '';
            const gender = p.gender || p.geschlecht || '';
            const status = p.status || 'active';
            const statusBadge = status === 'active'
                ? '<span class="badge badge-success">Aktiv</span>'
                : '<span class="badge badge-neutral">Inaktiv</span>';
            return '' +
                '<tr class="clickable" data-patient-id="' + escapeHtml(String(id)) + '">' +
                '<td>' + escapeHtml(String(id)) + '</td>' +
                '<td><strong>' + escapeHtml(name || '—') + '</strong></td>' +
                '<td>' + escapeHtml(formatDate(dob)) + '</td>' +
                '<td>' + escapeHtml(gender) + '</td>' +
                '<td>' + statusBadge + '</td>' +
                '</tr>';
        }).join('');

        return '' +
            '<table class="table" aria-label="Patienten-Liste">' +
            '<thead>' +
            '<tr>' +
            '<th scope="col">ID</th>' +
            '<th scope="col">Name</th>' +
            '<th scope="col">Geburtsdatum</th>' +
            '<th scope="col">Geschlecht</th>' +
            '<th scope="col">Status</th>' +
            '</tr>' +
            '</thead>' +
            '<tbody>' + rows + '</tbody>' +
            '</table>';
    }

    function renderEmptyState(title, text) {
        return '<div class="empty-state">' +
            '<div class="empty-state-icon">🔍</div>' +
            '<div class="empty-state-title">' + escapeHtml(title) + '</div>' +
            (text ? '<div class="empty-state-text">' + escapeHtml(text) + '</div>' : '') +
            '</div>';
    }

    function renderPagination(pages, current, total, prefix) {
        if (pages <= 1) return '';
        const from = (current - 1) * 20 + 1;
        const to = Math.min(current * 20, total);

        // Seiten-Buttons (max 7)
        const start = Math.max(1, current - 3);
        const end = Math.min(pages, current + 3);
        let pageButtons = '';
        for (let i = start; i <= end; i++) {
            pageButtons += '<button class="pagination-btn' + (i === current ? ' active' : '') + '" data-page="' + i + '">' + i + '</button>';
        }

        return '' +
            '<div class="pagination">' +
            '<span class="pagination-info">' + from + '–' + to + ' von ' + total + ' Einträgen</span>' +
            '<div class="pagination-controls">' +
            '<button class="pagination-btn" data-page="' + Math.max(1, current - 1) + '" ' + (current <= 1 ? 'disabled' : '') + '>‹ Zurück</button>' +
            pageButtons +
            '<button class="pagination-btn" data-page="' + Math.min(pages, current + 1) + '" ' + (current >= pages ? 'disabled' : '') + '>Weiter ›</button>' +
            '</div>' +
            '</div>';
    }

    function bindPatientsEvents() {
        const searchInput = document.getElementById('patient-search');
        const statusFilter = document.getElementById('patient-status-filter');
        const newBtn = document.getElementById('patient-new-btn');
        const tbody = document.querySelector('#content tbody');
        const pagination = document.querySelector('#content .pagination-controls');

        if (searchInput) {
            let debounce;
            searchInput.addEventListener('input', function () {
                clearTimeout(debounce);
                debounce = setTimeout(function () {
                    patientState.q = searchInput.value.trim();
                    patientState.page = 1;
                    Router.handleRouteChangePublic();
                }, 400);
            });
        }

        if (statusFilter) {
            statusFilter.addEventListener('change', function () {
                patientState.status = statusFilter.value;
                patientState.page = 1;
                Router.handleRouteChangePublic();
            });
        }

        if (newBtn) {
            newBtn.addEventListener('click', function () {
                openPatientModal(null);
            });
        }

        if (tbody) {
            tbody.addEventListener('click', function (e) {
                const row = e.target.closest('tr[data-patient-id]');
                if (row) {
                    const id = row.getAttribute('data-patient-id');
                    Router.navigate('#/patients/' + encodeURIComponent(id));
                }
            });
        }

        if (pagination) {
            pagination.addEventListener('click', function (e) {
                const btn = e.target.closest('button[data-page]');
                if (btn && !btn.disabled) {
                    patientState.page = parseInt(btn.getAttribute('data-page'), 10);
                    Router.handleRouteChangePublic();
                }
            });
        }
    }

    // --------------------------------------------------------
    // Patient-Modal (Anlegen/Bearbeiten)
    // --------------------------------------------------------
    function openPatientModal(patient) {
        const isEdit = !!patient;
        const title = isEdit ? 'Patient bearbeiten' : 'Neuer Patient';

        const body = '' +
            '<form id="patient-form">' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-lastname">Nachname *</label>' +
            '<input type="text" class="form-input" id="pf-lastname" name="last_name" value="' + escapeHtml(patient ? patient.last_name || patient.nachname || '' : '') + '" required>' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-firstname">Vorname *</label>' +
            '<input type="text" class="form-input" id="pf-firstname" name="first_name" value="' + escapeHtml(patient ? patient.first_name || patient.vorname || '' : '') + '" required>' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-dob">Geburtsdatum</label>' +
            '<input type="date" class="form-input" id="pf-dob" name="birth_date" value="' + escapeHtml(patient ? (patient.birth_date || patient.geburtsdatum || '').slice(0, 10) : '') + '">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-gender">Geschlecht</label>' +
            '<select class="form-select" id="pf-gender" name="gender">' +
            '<option value="">Unbekannt</option>' +
            '<option value="m"' + (patient && (patient.gender === 'm' || patient.geschlecht === 'm') ? ' selected' : '') + '>Männlich</option>' +
            '<option value="w"' + (patient && (patient.gender === 'w' || patient.geschlecht === 'w') ? ' selected' : '') + '>Weiblich</option>' +
            '<option value="x"' + (patient && (patient.gender === 'x' || patient.geschlecht === 'x') ? ' selected' : '') + '>Divers</option>' +
            '</select>' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-phone">Telefon</label>' +
            '<input type="tel" class="form-input" id="pf-phone" name="phone" value="' + escapeHtml(patient ? patient.phone || patient.telefon || '' : '') + '">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-email">E-Mail</label>' +
            '<input type="email" class="form-input" id="pf-email" name="email" value="' + escapeHtml(patient ? patient.email || patient.e_mail || '' : '') + '">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-street">Straße</label>' +
            '<input type="text" class="form-input" id="pf-street" name="street" value="' + escapeHtml(patient ? patient.street || patient.strasse || patient.straße || '' : '') + '">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-zip">PLZ</label>' +
            '<input type="text" class="form-input" id="pf-zip" name="zip" maxlength="5" value="' + escapeHtml(patient ? patient.zip || patient.plz || '' : '') + '">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="pf-city">Stadt</label>' +
            '<input type="text" class="form-input" id="pf-city" name="city" value="' + escapeHtml(patient ? patient.city || patient.stadt || '' : '') + '">' +
            '</div>' +
            '</form>';

        const footer = '' +
            '<button class="btn btn-secondary" data-modal-close>Abbrechen</button>' +
            '<button class="btn btn-primary" id="patient-save-btn">' + (isEdit ? 'Speichern' : 'Anlegen') + '</button>';

        KISModal.open(title, body, {
            footerHtml: footer,
            onOpen: function (modal) {
                const saveBtn = modal.querySelector('#patient-save-btn');
                saveBtn.addEventListener('click', function () {
                    savePatient(modal, isEdit ? patient : null);
                });
                // Form-Submit
                const form = modal.querySelector('#patient-form');
                if (form) {
                    form.addEventListener('submit', function (e) {
                        e.preventDefault();
                        savePatient(modal, isEdit ? patient : null);
                    });
                }
            }
        });
    }

    async function savePatient(modal, existing) {
        const form = modal.querySelector('#patient-form');
        if (!form) return;

        const data = {
            last_name: form.last_name.value.trim(),
            first_name: form.first_name.value.trim(),
            birth_date: form.birth_date.value || null,
            gender: form.gender.value || null,
            phone: form.phone.value.trim() || null,
            email: form.email.value.trim() || null,
            street: form.street.value.trim() || null,
            zip: form.zip.value.trim() || null,
            city: form.city.value.trim() || null
        };

        const saveBtn = modal.querySelector('#patient-save-btn');
        if (saveBtn) saveBtn.disabled = true;

        try {
            if (existing) {
                await global.KISApi.updatePatient(existing.id, data);
                KISToast.show('Patient erfolgreich aktualisiert.', 'success');
            } else {
                await global.KISApi.createPatient(data);
                KISToast.show('Patient erfolgreich angelegt.', 'success');
            }
            KISModal.close();
            Router.handleRouteChangePublic();
        } catch (e) {
            KISToast.show(e.message || 'Speichern fehlgeschlagen.', 'error');
            if (saveBtn) saveBtn.disabled = false;
        }
    }

    // --------------------------------------------------------
    // View: Patienten-Detail
    // --------------------------------------------------------
    async function renderPatientDetail(params) {
        const id = params.id;
        if (!id) return renderErrorState(new Error('Keine Patient-ID angegeben.'));

        let patient;
        try {
            patient = await global.KISApi.getPatient(id);
        } catch (e) {
            return renderErrorState(e);
        }

        if (!patient) {
            return renderEmptyState('Patient nicht gefunden', 'Der angeforderte Patient existiert nicht oder wurde gelöscht.');
        }

        const name = (patient.last_name || patient.nachname || '') + ' ' + (patient.first_name || patient.vorname || '');
        const dob = patient.birth_date || patient.geburtsdatum || '';
        const gender = patient.gender || patient.geschlecht || '';
        const status = patient.status || 'active';
        const statusBadge = status === 'active'
            ? '<span class="badge badge-success">Aktiv</span>'
            : '<span class="badge badge-neutral">Inaktiv</span>';

        // Alter berechnen
        let age = '—';
        if (dob) {
            try {
                const d = new Date(dob);
                const now = new Date();
                age = (now.getFullYear() - d.getFullYear()) + ' Jahre';
            } catch (e) {
            }
        }

        const body = '' +
            '<div class="detail-header">' +
            '<div>' +
            '<h2 class="detail-title">' + escapeHtml(name || 'Unbekannt') + '</h2>' +
            '<div class="detail-subtitle">ID: ' + escapeHtml(String(id)) + ' · ' + statusBadge + '</div>' +
            '</div>' +
            '<div class="detail-actions">' +
            '<button class="btn btn-secondary" id="patient-edit-btn">✏️ Bearbeiten</button>' +
            '<a class="btn btn-secondary" href="#/patients">← Zurück</a>' +
            '</div>' +
            '</div>' +

            '<div class="tabs" role="tablist">' +
            '<button class="tab active" role="tab" data-tab="stammdaten" aria-selected="true">Stammdaten</button>' +
            '<button class="tab" role="tab" data-tab="termine" aria-selected="false">Termine</button>' +
            '<button class="tab" role="tab" data-tab="medikamente" aria-selected="false">Medikamente</button>' +
            '<button class="tab" role="tab" data-tab="audit" aria-selected="false">Audit-Trail</button>' +
            '</div>' +

            '<div class="tab-content active" id="tab-stammdaten" role="tabpanel">' +
            '<div class="detail-grid">' +
            '<div class="card">' +
            '<div class="card-header"><h3 class="card-title">Personendaten</h3></div>' +
            detailField('Nachname', patient.last_name || patient.nachname) +
            detailField('Vorname', patient.first_name || patient.vorname) +
            detailField('Geburtsdatum', formatDate(dob)) +
            detailField('Alter', age) +
            detailField('Geschlecht', gender) +
            '</div>' +
            '<div class="card">' +
            '<div class="card-header"><h3 class="card-title">Kontaktdaten & Adresse</h3></div>' +
            detailField('Telefon', patient.phone || patient.telefon) +
            detailField('E-Mail', patient.email || patient.e_mail) +
            detailField('Straße', patient.street || patient.strasse || patient.straße) +
            detailField('PLZ', patient.zip || patient.plz) +
            detailField('Stadt', patient.city || patient.stadt) +
            detailField('Versicherten-Nr.', patient.insurance_number || patient.versichertennummer) +
            '</div>' +
            '</div>' +
            '</div>' +

            '<div class="tab-content" id="tab-termine" role="tabpanel">' +
            '<div id="patient-appointments" class="loading"><div class="loading-spinner"></div></div>' +
            '</div>' +

            '<div class="tab-content" id="tab-medikamente" role="tabpanel">' +
            '<div id="patient-medications" class="loading"><div class="loading-spinner"></div></div>' +
            '</div>' +

            '<div class="tab-content" id="tab-audit" role="tabpanel">' +
            '<div id="patient-audit" class="loading"><div class="loading-spinner"></div></div>' +
            '</div>';

        return body;
    }

    function detailField(label, value) {
        return '<div class="detail-field">' +
            '<span class="detail-field-label">' + escapeHtml(label) + '</span>' +
            '<span class="detail-field-value">' + escapeHtml(value || '—') + '</span>' +
            '</div>';
    }

    function bindPatientDetailEvents(params) {
        const id = params.id;

        // Tabs
        const tabs = document.querySelectorAll('#content .tab');
        tabs.forEach(function (tab) {
            tab.addEventListener('click', function () {
                const tabName = tab.getAttribute('data-tab');
                // Active-Tab setzen
                tabs.forEach(function (t) {
                    t.classList.remove('active');
                    t.setAttribute('aria-selected', 'false');
                });
                tab.classList.add('active');
                tab.setAttribute('aria-selected', 'true');
                // Content anzeigen
                document.querySelectorAll('#content .tab-content').forEach(function (c) {
                    c.classList.remove('active');
                });
                const content = document.getElementById('tab-' + tabName);
                if (content) content.classList.add('active');
            });
        });

        // Bearbeiten-Button
        const editBtn = document.getElementById('patient-edit-btn');
        if (editBtn) {
            editBtn.addEventListener('click', function () {
                // Patient-Daten laden und Modal öffnen
                global.KISApi.getPatient(id).then(function (patient) {
                    openPatientModal(patient);
                }).catch(function (e) {
                    KISToast.show(e.message, 'error');
                });
            });
        }

        // Lazy-Loading der Tabs
        loadPatientTab('termine', id);
    }

    async function loadPatientTab(tabName, id) {
        const container = document.getElementById('patient-' + tabName);
        if (!container) return;

        try {
            if (tabName === 'termine') {
                const appts = await global.KISApi.listAppointments({ patient_id: id, limit: 20 });
                const list = Array.isArray(appts) ? appts : (appts.items || appts.appointments || []);
                container.innerHTML = list.length
                    ? renderAppointmentsList(list)
                    : renderEmptyState('Keine Termine', 'Dieser Patient hat keine Termine.');
            } else if (tabName === 'medikamente') {
                // Medikation über Patient-Endpunkt
                const data = await global.KISApi.get('/patients/' + encodeURIComponent(id) + '/medications');
                const meds = Array.isArray(data) ? data : (data.medications || data.items || []);
                container.innerHTML = meds.length
                    ? renderMedicationsList(meds)
                    : renderEmptyState('Keine Medikamente', 'Keine Medikation verordnet.');
            } else if (tabName === 'audit') {
                const audit = await global.KISApi.getPatientAudit(id);
                const entries = Array.isArray(audit) ? audit : (audit.entries || audit.items || []);
                container.innerHTML = '<ul class="audit-trail">' + entries.map(function (a) {
                    const action = a.action || a.type || 'Unbekannt';
                    const actor = a.actor || a.user || 'System';
                    const when = a.timestamp || a.created_at || '';
                    return '' +
                        '<li class="audit-item">' +
                        '<div class="audit-icon">📝</div>' +
                        '<div class="audit-content">' +
                        '<div class="audit-action">' + escapeHtml(action) + '</div>' +
                        '<div class="audit-meta">' + escapeHtml(actor) + ' · ' + escapeHtml(formatDateTime(when)) + '</div>' +
                        '</div>' +
                        '</li>';
                }).join('') + '</ul>';
            }
        } catch (e) {
            container.innerHTML = renderErrorState(e);
        }
    }

    function renderAppointmentsList(appts) {
        return '<table class="table" aria-label="Termine">' +
            '<thead><tr><th>Zeitpunkt</th><th>Art</th><th>Behandler</th><th>Status</th></tr></thead>' +
            '<tbody>' + appts.map(function (a) {
                const when = a.start || a.start_time || a.start_time_iso || a.created_at || '';
                const type = a.type || a.reason || 'Termin';
                const practitioner = a.practitioner || a.doctor || '—';
                const status = a.status || 'geplant';
                const badge = status === 'storniert'
                    ? '<span class="badge badge-danger">Storniert</span>'
                    : (status === 'abgeschlossen' ? '<span class="badge badge-success">Abgeschlossen</span>'
                    : '<span class="badge badge-info">Geplant</span>');
                return '<tr>' +
                    '<td>' + escapeHtml(formatDateTime(when)) + '</td>' +
                    '<td>' + escapeHtml(type) + '</td>' +
                    '<td>' + escapeHtml(practitioner) + '</td>' +
                    '<td>' + badge + '</td>' +
                    '</tr>';
            }).join('') + '</tbody></table>';
    }

    function renderMedicationsList(meds) {
        return '<table class="table" aria-label="Medikamente">' +
            '<thead><tr><th>Medikament</th><th>Dosierung</th><th>Verordnet am</th><th>Status</th></tr></thead>' +
            '<tbody>' + meds.map(function (m) {
                const name = m.name || m.medication || 'Unbekannt';
                const dosage = m.dosage || m.dose || '—';
                const prescribed = m.prescribed_at || m.started_at || '';
                const status = m.status || 'aktiv';
                const badge = status === 'aktiv'
                    ? '<span class="badge badge-success">Aktiv</span>'
                    : '<span class="badge badge-neutral">' + escapeHtml(status) + '</span>';
                return '<tr>' +
                    '<td>' + escapeHtml(name) + '</td>' +
                    '<td>' + escapeHtml(dosage) + '</td>' +
                    '<td>' + escapeHtml(formatDate(prescribed)) + '</td>' +
                    '<td>' + badge + '</td>' +
                    '</tr>';
            }).join('') + '</tbody></table>';
    }

    // --------------------------------------------------------
    // View: Termine (Kalender)
    // --------------------------------------------------------
    let calendarState = {
        year: new Date().getFullYear(),
        month: new Date().getMonth(), // 0-based
        view: 'month', // 'month' | 'day'
        selectedDate: null
    };

    async function renderAppointments(params) {
        const now = new Date();
        const year = calendarState.year;
        const month = calendarState.month;
        const monthNames = ['Januar', 'Februar', 'März', 'April', 'Mai', 'Juni', 'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember'];

        const from = new Date(year, month, 1);
        const lastDay = new Date(year, month + 1, 0).getDate();
        const to = new Date(year, month + 1, 0);

        // Termine laden
        let appts = [];
        try {
            const data = await global.KISApi.listAppointments({
                from: from.toISOString(),
                to: to.toISOString(),
                limit: 200
            });
            appts = extractList(data, 'appointments');
        } catch (e) {
            // Kalender ohne Termine rendern
        }

        const monthHtml = renderMonthCalendar(year, month, appts);
        const dayHtml = calendarState.view === 'day' && calendarState.selectedDate
            ? renderDayView(calendarState.selectedDate, appts)
            : '';

        return '' +
            '<div class="search-bar">' +
            '<div class="filter-group">' +
            '<button class="btn btn-secondary btn-sm" id="cal-prev">‹</button>' +
            '<span class="filter-label" id="cal-title" style="min-width:180px;text-align:center;font-weight:600;">' +
            monthNames[month] + ' ' + year +
            '</span>' +
            '<button class="btn btn-secondary btn-sm" id="cal-next">›</button>' +
            '<button class="btn btn-secondary btn-sm" id="cal-today">Heute</button>' +
            '</div>' +
            '<div class="filter-group">' +
            '<label class="filter-label">Ansicht:</label>' +
            '<select class="form-select" id="cal-view" aria-label="Kalender-Ansicht">' +
            '<option value="month"' + (calendarState.view === 'month' ? ' selected' : '') + '>Monat</option>' +
            '<option value="day"' + (calendarState.view === 'day' ? ' selected' : '') + '>Tag</option>' +
            '</select>' +
            '</div>' +
            '<button class="btn btn-primary" id="appt-new-btn">+ Neuer Termin</button>' +
            '</div>' +
            (calendarState.view === 'month' ? monthHtml : dayHtml);
    }

    function renderMonthCalendar(year, month, appts) {
        const firstDay = new Date(year, month, 1).getDay(); // 0 = So
        const daysInMonth = new Date(year, month + 1, 0).getDate();
        const daysInPrev = new Date(year, month, 0).getDate();

        const today = new Date();
        const isCurrentMonth = (today.getFullYear() === year && today.getMonth() === month);

        // Termine nach Tag gruppieren
        const byDay = {};
        appts.forEach(function (a) {
            const d = a.start || a.start_time || a.start_time_iso;
            if (!d) return;
            const dt = new Date(d);
            const key = dt.getFullYear() + '-' + dt.getMonth() + '-' + dt.getDate();
            if (!byDay[key]) byDay[key] = [];
            byDay[key].push(a);
        });

        let cells = '';
        const totalCells = Math.ceil((firstDay + daysInMonth) / 7) * 7;

        for (let i = 0; i < totalCells; i++) {
            const dayNum = i - firstDay + 1;
            let dayDate;
            let otherMonth = false;

            if (dayNum < 1) {
                dayDate = new Date(year, month - 1, daysInPrev + dayNum);
                otherMonth = true;
            } else if (dayNum > daysInMonth) {
                dayDate = new Date(year, month + 1, dayNum - daysInMonth);
                otherMonth = true;
            } else {
                dayDate = new Date(year, month, dayNum);
            }

            const isToday = isCurrentMonth && dayDate.getDate() === today.getDate() && dayDate.getMonth() === today.getMonth();
            const isSelected = calendarState.selectedDate &&
                calendarState.selectedDate.toDateString() === dayDate.toDateString();

            const key = dayDate.getFullYear() + '-' + dayDate.getMonth() + '-' + dayDate.getDate();
            const dayAppts = byDay[key] || [];
            const eventsHtml = dayAppts.slice(0, 3).map(function (a) {
                const label = a.type || a.reason || 'Termin';
                return '<div class="calendar-event" title="' + escapeHtml(label) + '">' +
                    escapeHtml(formatTime(a.start || a.start_time)) + ' ' + escapeHtml(label) + '</div>';
            }).join('') + (dayAppts.length > 3 ? '<div class="calendar-event">+' + (dayAppts.length - 3) + ' weitere</div>' : '');

            cells += '' +
                '<div class="calendar-day' + (otherMonth ? ' other-month' : '') +
                (isToday ? ' today' : '') +
                (isSelected ? ' selected' : '') + '" ' +
                'data-date="' + dayDate.toISOString() + '" ' +
                'role="button" tabindex="0" ' +
                'aria-label="' + dayDate.toLocaleDateString('de-DE') + (dayAppts.length ? ', ' + dayAppts.length + ' Termine' : '') + '">' +
                '<div class="calendar-day-num">' + dayDate.getDate() + '</div>' +
                '<div class="calendar-events">' + eventsHtml + '</div>' +
                '</div>';
        }

        const dayHeaders = ['So', 'Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa'].map(function (d) {
            return '<div class="calendar-day-header">' + d + '</div>';
        }).join('');

        return '' +
            '<div class="calendar">' +
            '<div class="calendar-grid">' +
            dayHeaders +
            cells +
            '</div>' +
            '</div>';
    }

    function renderDayView(date, appts) {
        const dayAppts = appts.filter(function (a) {
            const d = a.start || a.start_time || a.start_time_iso;
            if (!d) return false;
            return new Date(d).toDateString() === date.toDateString();
        }).sort(function (a, b) {
            return new Date(a.start || a.start_time) - new Date(b.start || b.start_time);
        });

        const header = '' +
            '<div class="day-view-header">' +
            '<h2 class="day-view-title">' + date.toLocaleDateString('de-DE', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' }) + '</h2>' +
            '<button class="btn btn-secondary btn-sm" id="day-back-month">← Monatsansicht</button>' +
            '</div>';

        if (!dayAppts.length) {
            return '<div class="day-view">' + header +
                renderEmptyState('Keine Termine', 'Für diesen Tag sind keine Termine geplant.') +
                '</div>';
        }

        return '<div class="day-view">' + header +
            '<div class="timeline">' + dayAppts.map(function (a) {
                const start = a.start || a.start_time || '';
                const end = a.end || a.end_time || '';
                const type = a.type || a.reason || 'Termin';
                const practitioner = a.practitioner || a.doctor || '—';
                const status = a.status || 'geplant';
                const badge = status === 'storniert'
                    ? '<span class="badge badge-danger">Storniert</span>'
                    : (status === 'abgeschlossen' ? '<span class="badge badge-success">Abgeschlossen</span>'
                    : '<span class="badge badge-info">Geplant</span>');
                return '' +
                    '<div class="timeline-item" style="margin-top:' + (parseInt((new Date(start) || new Date()).getMinutes()) / 60 * 20) + 'px">' +
                    '<div class="timeline-time">' + escapeHtml(formatTime(start)) + (end ? ' – ' + escapeHtml(formatTime(end)) : '') + '</div>' +
                    '<div class="timeline-item-title">' + escapeHtml(type) + ' ' + badge + '</div>' +
                    '<div class="timeline-item-meta">' + escapeHtml(practitioner) + '</div>' +
                    '</div>';
            }).join('') + '</div>' +
            '</div>';
    }

    function bindAppointmentsEvents(params) {
        const prevBtn = document.getElementById('cal-prev');
        const nextBtn = document.getElementById('cal-next');
        const todayBtn = document.getElementById('cal-today');
        const viewSelect = document.getElementById('cal-view');
        const newBtn = document.getElementById('appt-new-btn');
        const dayBackBtn = document.getElementById('day-back-month');
        const calendarGrid = document.querySelector('#content .calendar-grid');

        if (prevBtn) {
            prevBtn.addEventListener('click', function () {
                calendarState.month--;
                if (calendarState.month < 0) { calendarState.month = 11; calendarState.year--; }
                Router.handleRouteChangePublic();
            });
        }
        if (nextBtn) {
            nextBtn.addEventListener('click', function () {
                calendarState.month++;
                if (calendarState.month > 11) { calendarState.month = 0; calendarState.year++; }
                Router.handleRouteChangePublic();
            });
        }
        if (todayBtn) {
            todayBtn.addEventListener('click', function () {
                const now = new Date();
                calendarState.year = now.getFullYear();
                calendarState.month = now.getMonth();
                calendarState.selectedDate = now;
                Router.handleRouteChangePublic();
            });
        }
        if (viewSelect) {
            viewSelect.addEventListener('change', function () {
                calendarState.view = viewSelect.value;
                if (calendarState.view === 'day' && !calendarState.selectedDate) {
                    calendarState.selectedDate = new Date();
                }
                Router.handleRouteChangePublic();
            });
        }
        if (newBtn) {
            newBtn.addEventListener('click', function () {
                openAppointmentModal();
            });
        }
        if (dayBackBtn) {
            dayBackBtn.addEventListener('click', function () {
                calendarState.view = 'month';
                Router.handleRouteChangePublic();
            });
        }
        if (calendarGrid) {
            calendarGrid.addEventListener('click', function (e) {
                const day = e.target.closest('.calendar-day');
                if (day) {
                    const dateStr = day.getAttribute('data-date');
                    calendarState.selectedDate = new Date(dateStr);
                    if (calendarState.view === 'month') {
                        // In Monatsansicht: Tag-View öffnen
                        calendarState.view = 'day';
                    }
                    Router.handleRouteChangePublic();
                }
            });
        }
    }

    function openAppointmentModal() {
        const now = new Date();
        const tomorrow = new Date(now);
        tomorrow.setDate(tomorrow.getDate() + 1);
        tomorrow.setHours(9, 0, 0, 0);

        const body = '' +
            '<form id="appt-form">' +
            '<div class="form-group">' +
            '<label class="form-label" for="af-patient">Patient *</label>' +
            '<select class="form-select" id="af-patient" name="patient_id" required>' +
            '<option value="">Bitte wählen…</option>' +
            '<option value="__search">🔍 Patient suchen…</option>' +
            '</select>' +
            '<span class="form-hint">Patientenliste wird bei Auswahl geladen.</span>' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="af-type">Terminart *</label>' +
            '<input type="text" class="form-input" id="af-type" name="type" placeholder="z. B. Beratung, OP, Nachsorge" required>' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="af-start">Start *</label>' +
            '<input type="datetime-local" class="form-input" id="af-start" name="start" value="' +
            tomorrow.toISOString().slice(0, 16) + '" required>' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="af-end">Ende</label>' +
            '<input type="datetime-local" class="form-input" id="af-end" name="end">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="af-practitioner">Behandler</label>' +
            '<input type="text" class="form-input" id="af-practitioner" name="practitioner" placeholder="Name des Behandlers">' +
            '</div>' +
            '<div class="form-group">' +
            '<label class="form-label" for="af-notes">Notizen</label>' +
            '<textarea class="form-input" id="af-notes" name="notes" rows="3"></textarea>' +
            '</div>' +
            '</form>';

        const footer = '' +
            '<button class="btn btn-secondary" data-modal-close>Abbrechen</button>' +
            '<button class="btn btn-primary" id="appt-save-btn">Termin anlegen</button>';

        KISModal.open('Neuer Termin', body, {
            footerHtml: footer,
            onOpen: function (modal) {
                // Patienten laden und füllen
                const patientSelect = modal.querySelector('#af-patient');
                global.KISApi.listPatients({ limit: 100 }).then(function (data) {
                    const patients = extractList(data, 'patients');
                    patients.forEach(function (p) {
                        const id = p.id || p.patient_id;
                        const name = (p.last_name || p.nachname || '') + ', ' + (p.first_name || p.vorname || '');
                        const opt = document.createElement('option');
                        opt.value = id;
                        opt.textContent = name + ' (' + id + ')';
                        patientSelect.appendChild(opt);
                    });
                }).catch(function () {
                    // leere Liste
                });

                const saveBtn = modal.querySelector('#appt-save-btn');
                saveBtn.addEventListener('click', function () {
                    saveAppointment(modal);
                });
                const form = modal.querySelector('#appt-form');
                if (form) {
                    form.addEventListener('submit', function (e) {
                        e.preventDefault();
                        saveAppointment(modal);
                    });
                }
            }
        });
    }

    async function saveAppointment(modal) {
        const form = modal.querySelector('#appt-form');
        if (!form) return;

        const data = {
            patient_id: form.patient_id.value || null,
            type: form.type.value.trim(),
            start: form.start.value ? new Date(form.start.value).toISOString() : null,
            end: form.end.value ? new Date(form.end.value).toISOString() : null,
            practitioner: form.practitioner.value.trim() || null,
            notes: form.notes.value.trim() || null
        };

        if (!data.patient_id) {
            KISToast.show('Bitte einen Patienten wählen.', 'warning');
            return;
        }
        if (!data.start) {
            KISToast.show('Bitte einen Startzeitpunkt wählen.', 'warning');
            return;
        }

        const saveBtn = modal.querySelector('#appt-save-btn');
        if (saveBtn) saveBtn.disabled = true;

        try {
            await global.KISApi.createAppointment(data);
            KISToast.show('Termin erfolgreich angelegt.', 'success');
            KISModal.close();
            Router.handleRouteChangePublic();
        } catch (e) {
            KISToast.show(e.message || 'Termin konnte nicht angelegt werden.', 'error');
            if (saveBtn) saveBtn.disabled = false;
        }
    }

    // --------------------------------------------------------
    // View: Einstellungen
    // --------------------------------------------------------
    async function renderSettings() {
        let profile = {};
        let apiKey = '';
        try {
            profile = await global.KISApi.getProfile();
        } catch (e) {
            // leer
        }
        try {
            const keyData = await global.KISApi.getApiKey();
            apiKey = keyData.key || keyData.api_key || (typeof keyData === 'string' ? keyData : '');
        } catch (e) {
            // kein Key
        }

        const currentTheme = document.documentElement.getAttribute('data-theme') || 'dark';

        return '' +
            '<div class="settings-section">' +
            '<h3 class="settings-section-title">Profil</h3>' +
            '<div class="settings-row">' +
            '<div class="settings-row-info">' +
            '<div class="settings-row-label">Anzeigename</div>' +
            '<div class="settings-row-description">' + escapeHtml(profile.full_name || profile.display_name || '—') + '</div>' +
            '</div>' +
            '<div class="settings-row-control">' +
            '<button class="btn btn-secondary btn-sm" id="edit-profile-btn">Bearbeiten</button>' +
            '</div>' +
            '</div>' +
            '<div class="settings-row">' +
            '<div class="settings-row-info">' +
            '<div class="settings-row-label">E-Mail</div>' +
            '<div class="settings-row-description">' + escapeHtml(profile.email || '—') + '</div>' +
            '</div>' +
            '</div>' +
            '<div class="settings-row">' +
            '<div class="settings-row-info">' +
            '<div class="settings-row-label">Rolle</div>' +
            '<div class="settings-row-description">' + escapeHtml(profile.role || profile.role_name || '—') + '</div>' +
            '</div>' +
            '</div>' +
            '</div>' +

            '<div class="settings-section">' +
            '<h3 class="settings-section-title">Erscheinungsbild</h3>' +
            '<div class="settings-row">' +
            '<div class="settings-row-info">' +
            '<div class="settings-row-label">Theme</div>' +
            '<div class="settings-row-description">Farbschema der Oberfläche wählen.</div>' +
            '</div>' +
            '<div class="settings-row-control">' +
            '<div class="theme-switch">' +
            '<button class="theme-option' + (currentTheme === 'dark' ? ' active' : '') + '" data-theme-option="dark">🌙 Dark</button>' +
            '<button class="theme-option' + (currentTheme === 'light' ? ' active' : '') + '" data-theme-option="light">☀️ Light</button>' +
            '</div>' +
            '</div>' +
            '</div>' +
            '</div>' +

            '<div class="settings-section">' +
            '<h3 class="settings-section-title">API-Schlüssel</h3>' +
            '<div class="settings-row">' +
            '<div class="settings-row-info">' +
            '<div class="settings-row-label">Aktueller Schlüssel</div>' +
            '<div class="settings-row-description">Für den Zugriff auf die KIS-REST-API.</div>' +
            '</div>' +
            '</div>' +
            '</div>' +
            '<div class="settings-row">' +
            '<div class="settings-row-info">' +
            '<div class="api-key-display">' +
            '<div class="api-key-value" id="api-key-value">' + escapeHtml(apiKey || 'Kein Schlüssel vorhanden') + '</div>' +
            '<div class="api-key-actions">' +
            '<button class="btn btn-secondary btn-sm" id="api-copy-btn" aria-label="Schlüssel kopieren">Kopieren</button>' +
            '<button class="btn btn-secondary btn-sm" id="api-revoke-btn" aria-label="Schlüssel widerrufen">Widerrufen</button>' +
            '<button class="btn btn-primary btn-sm" id="api-generate-btn">Neu generieren</button>' +
            '</div>' +
            '</div>' +
            '</div>' +
            '</div>' +
            '</div>';
    }

    function bindSettingsEvents() {
        const editProfileBtn = document.getElementById('edit-profile-btn');
        const themeOptions = document.querySelectorAll('.theme-option');
        const copyBtn = document.getElementById('api-copy-btn');
        const revokeBtn = document.getElementById('api-revoke-btn');
        const generateBtn = document.getElementById('api-generate-btn');

        if (editProfileBtn) {
            editProfileBtn.addEventListener('click', openProfileModal);
        }

        themeOptions.forEach(function (btn) {
            btn.addEventListener('click', function () {
                const theme = btn.getAttribute('data-theme-option');
                document.documentElement.setAttribute('data-theme', theme);
                try {
                    localStorage.setItem('kis_theme', theme);
                } catch (e) {
                }
                // Active-State
                themeOptions.forEach(function (b) { b.classList.remove('active'); });
                btn.classList.add('active');
            });
        });

        if (copyBtn) {
            copyBtn.addEventListener('click', function () {
                const valueEl = document.getElementById('api-key-value');
                const value = valueEl ? valueEl.textContent.trim() : '';
                if (!value || value === 'Kein Schlüssel vorhanden') return;
                copyToClipboard(value);
                KISToast.show('API-Schlüssel kopiert.', 'success');
            });
        }

        if (revokeBtn) {
            revokeBtn.addEventListener('click', function () {
                KISModal.open('API-Schlüssel widerrufen',
                    '<p>Sind Sie sicher? Alle Anwendungen, die diesen Schlüssel verwenden, verlieren den Zugang.</p>',
                    {
                        footerHtml: '' +
                            '<button class="btn btn-secondary" data-modal-close>Abbrechen</button>' +
                            '<button class="btn btn-danger" id="api-revoke-confirm">Widerrufen</button>',
                        onOpen: function (modal) {
                            modal.querySelector('#api-revoke-confirm').addEventListener('click', function () {
                                global.KISApi.revokeApiKey().then(function () {
                                    KISToast.show('API-Schlüssel widerrufen.', 'info');
                                    KISModal.close();
                                    Router.handleRouteChangePublic();
                                }).catch(function (e) {
                                    KISToast.show(e.message, 'error');
                                });
                            });
                        }
                    }
                );
            });
        }

        if (generateBtn) {
            generateBtn.addEventListener('click', function () {
                global.KISApi.generateApiKey().then(function (data) {
                    const key = data.key || data.api_key || (typeof data === 'string' ? data : '');
                    const valueEl = document.getElementById('api-key-value');
                    if (valueEl) valueEl.textContent = key;
                    KISToast.show('Neuer API-Schlüssel generiert.', 'success');
                    // Warnung: Schlüssel nur einmal sichtbar
                    setTimeout(function () {
                        KISToast.show('Hinweis: Der Schlüssel wird nur einmal angezeigt.', 'warning');
                    }, 1000);
                }).catch(function (e) {
                    KISToast.show(e.message, 'error');
                });
            });
        }
    }

    function openProfileModal() {
        global.KISApi.getProfile().then(function (profile) {
            const body = '' +
                '<form id="profile-form">' +
                '<div class="form-group">' +
                '<label class="form-label" for="pf-name">Anzeigename</label>' +
                '<input type="text" class="form-input" id="pf-name" name="full_name" value="' + escapeHtml(profile.full_name || profile.display_name || '') + '">' +
                '</div>' +
                '<div class="form-group">' +
                '<label class="form-label" for="pf-email">E-Mail</label>' +
                '<input type="email" class="form-input" id="pf-email" name="email" value="' + escapeHtml(profile.email || '') + '">' +
                '</div>' +
                '</form>';

            KISModal.open('Profil bearbeiten', body, {
                footerHtml: '' +
                    '<button class="btn btn-secondary" data-modal-close>Abbrechen</button>' +
                    '<button class="btn btn-primary" id="profile-save-btn">Speichern</button>',
                onOpen: function (modal) {
                    modal.querySelector('#profile-save-btn').addEventListener('click', function () {
                        const form = modal.querySelector('#profile-form');
                        const data = {
                            full_name: form.full_name.value.trim(),
                            email: form.email.value.trim()
                        };
                        global.KISApi.updateProfile(data).then(function () {
                            KISToast.show('Profil aktualisiert.', 'success');
                            KISModal.close();
                            Router.handleRouteChangePublic();
                        }).catch(function (e) {
                            KISToast.show(e.message, 'error');
                        });
                    });
                }
            });
        }).catch(function (e) {
            KISToast.show(e.message, 'error');
        });
    }

    /**
     * Kopiert Text in die Zwischenablage.
     * @param {string} text
     */
    function copyToClipboard(text) {
        if (navigator.clipboard && navigator.clipboard.writeText) {
            navigator.clipboard.writeText(text).catch(function () {
                fallbackCopy(text);
            });
        } else {
            fallbackCopy(text);
        }
    }

    function fallbackCopy(text) {
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        try {
            document.execCommand('copy');
        } catch (e) {
        }
        document.body.removeChild(ta);
    }

    // --------------------------------------------------------
    // App-Init
    // --------------------------------------------------------

    /**
     * Startet die Anwendung (wird von KISAuth aufgerufen).
     */
    function start() {
        KISToast.init();
        KISModal.init();
        Router.start();
    }

    /**
     * Stoppt die Anwendung (Logout).
     */
    function stop() {
        Router.stop();
    }

    // Öffentliche API
    global.KISApp = {
        start: start,
        stop: stop,
        navigate: function (hash) {
            Router.navigate(hash);
        }
    };

    // Router handleRouteChangePublic für interne Aufrufe
    // (Router.start() wird erst nach Login aufgerufen;
    //  davor müssen View-Reloads ohne hashchange möglich sein)
    let _routerStarted = false;
    const _origStart = Router.start;
    Router.start = function () {
        if (_routerStarted) {
            // Bereits gestartet → nur handleRouteChange
            handleRouteChange();
            return;
        }
        _routerStarted = true;
        return _origStart();
    };
    Router.handleRouteChangePublic = function () {
        if (!_routerStarted) return;
        handleRouteChange();
    };

})(window);
