/**
 * KIS — Krankenhausinformationssystem
 * API-Client: Fetch-Wrapper für die REST-API
 *
 * Alle API-Aufrufe laufen über diese zentrale Schicht.
 * Handhabt:
 *  - JWT-Access-Token (localStorage)
 *  - Automatischer Token-Refresh bei 401
 *  - Konsistente Fehlerbehandlung
 *  - CSRF-Schutz über Bearer-Token
 */

(function (global) {
    'use strict';

    // Baseline für die API (gleicher Origin, kein CORS-Problem)
    const API_BASE = '/api/v1';

    // Speicher für den Access-Token
    const TOKEN_KEY = 'kis_access_token';
    const REFRESH_KEY = 'kis_refresh_token';

    /**
     * Liest den aktuellen Access-Token aus dem localStorage.
     * @returns {string|null}
     */
    function getAccessToken() {
        try {
            return localStorage.getItem(TOKEN_KEY);
        } catch (e) {
            return null;
        }
    }

    /**
     * Speichert den Access-Token.
     * @param {string} token
     */
    function setAccessToken(token) {
        try {
            if (token) {
                localStorage.setItem(TOKEN_KEY, token);
            } else {
                localStorage.removeItem(TOKEN_KEY);
            }
        } catch (e) {
            // localStorage nicht verfügbar (z. B. InPrivate)
        }
    }

    /**
     * Liest den Refresh-Token.
     * @returns {string|null}
     */
    function getRefreshToken() {
        try {
            return localStorage.getItem(REFRESH_KEY);
        } catch (e) {
            return null;
        }
    }

    /**
     * Speichert den Refresh-Token.
     * @param {string} token
     */
    function setRefreshToken(token) {
        try {
            if (token) {
                localStorage.setItem(REFRESH_KEY, token);
            } else {
                localStorage.removeItem(REFRESH_KEY);
            }
        } catch (e) {
        }
    }

    /**
     * Löscht beide Token (Logout).
     */
    function clearTokens() {
        try {
            localStorage.removeItem(TOKEN_KEY);
            localStorage.removeItem(REFRESH_KEY);
        } catch (e) {
        }
    }

    /**
     * Baut die Request-Header.
     * @param {Object} [extra]
     * @returns {Object}
     */
    function buildHeaders(extra) {
        const headers = {
            'Accept': 'application/json'
        };
        const token = getAccessToken();
        if (token) {
            headers['Authorization'] = 'Bearer ' + token;
        }
        // JSON-Content-Type nur bei Body
        if (extra && extra.body !== undefined) {
            headers['Content-Type'] = 'application/json';
        }
        return Object.assign(headers, extra.headers || {});
    }

    /**
     * Führt einen Fetch-Ausführung durch und parst die Antwort.
     * @param {string} path - relativer Pfad ohne Basis
     * @param {Object} options - Fetch-Optionen
     * @param {boolean} [retry] - intern: bereits gerefresht?
     * @returns {Promise<any>}
     */
    async function request(path, options = {}, retry = false) {
        const url = API_BASE + path;
        const headers = buildHeaders(options);

        let response;
        try {
            response = await fetch(url, Object.assign({}, options, {
                headers: headers,
                credentials: 'same-origin'
            }));
        } catch (networkError) {
            // Netzwerkfehler (Server nicht erreichbar)
            throw new ApiError(0, 'NETWORK_ERROR', 'Server nicht erreichbar. Bitte Verbindung prüfen.');
        }

        // 401 Unauthorized → Token-Refresh versuchen (nur einmal)
        if (response.status === 401 && !retry) {
            const refreshed = await tryRefreshToken();
            if (refreshed) {
                return request(path, options, true);
            }
            // Refresh fehlgeschlagen → Logout
            clearTokens();
            if (typeof global.KISAuth !== 'undefined') {
                global.KISAuth.forceLogout();
            }
            throw new ApiError(401, 'UNAUTHORIZED', 'Sitzung abgelaufen. Bitte erneut anmelden.');
        }

        // Body parsen
        let data = null;
        const contentType = response.headers.get('content-type') || '';
        if (contentType.includes('application/json')) {
            try {
                data = await response.json();
            } catch (e) {
                data = null;
            }
        } else {
            // Nicht-JSON-Antwort (z. B. CSV-Download)
            const text = await response.text();
            data = text;
        }

        // Erfolgs-Status (2xx)
        if (response.ok) {
            // Standardisierte API-Antwort: { data, error, message }
            if (data && typeof data === 'object' && data.error) {
                throw new ApiError(response.status, data.error, data.message || 'Unbekannter Fehler');
            }
            return (data && typeof data === 'object' && 'data' in data) ? data.data : data;
        }

        // Fehler antwort
        let error = 'UNEXPECTED_ERROR';
        let message = 'Unbekannter Fehler';
        if (data && typeof data === 'object') {
            if (data.error) error = data.error;
            if (data.message) message = data.message;
        } else if (typeof data === 'string' && data) {
            message = data;
        }

        // Benutzerfreundliche Fehlermeldungen für bekannte Codes
        message = translateError(response.status, error, message);

        throw new ApiError(response.status, error, message);
    }

    /**
     * Übersetzt bekannte HTTP-Status/Fehler-Codes in deutsche Meldungen.
     * @param {number} status
     * @param {string} code
     * @param {string} fallback
     * @returns {string}
     */
    function translateError(status, code, fallback) {
        if (code === 'VALIDATION_ERROR') return fallback;
        if (code === 'FORBIDDEN') return 'Zugriff verweigert. Benötigte Rolle nicht vorhanden.';
        if (code === 'NOT_FOUND') return 'Ressource nicht gefunden.';
        if (code === 'CONFLICT') return 'Konflikt: Datensatz existiert bereits.';
        if (code === 'RATE_LIMITED') return 'Zu viele Anfragen. Bitte warten Sie einen Moment.';
        if (status === 400) return 'Ungültige Anfrage: ' + fallback;
        if (status === 403) return 'Zugriff verweigert.';
        if (status === 404) return 'Nicht gefunden.';
        if (status === 409) return 'Konflikt mit bestehendem Datensatz.';
        if (status === 429) return 'Zu viele Anfragen. Bitte warten.';
        if (status >= 500) return 'Serverfehler. Bitte später erneut versuchen.';
        return fallback;
    }

    /**
     * Versucht, den Refresh-Token zu tauschen.
     * @returns {Promise<boolean>} true bei Erfolg
     */
    async function tryRefreshToken() {
        const refresh = getRefreshToken();
        if (!refresh) return false;
        try {
            const response = await fetch(API_BASE + '/auth/refresh', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'Accept': 'application/json'
                },
                body: JSON.stringify({ refresh_token: refresh }),
                credentials: 'same-origin'
            });
            if (!response.ok) return false;
            const data = await response.json();
            const payload = (data && data.data) ? data.data : data;
            if (payload.access_token) {
                setAccessToken(payload.access_token);
                if (payload.refresh_token) {
                    setRefreshToken(payload.refresh_token);
                }
                return true;
            }
            return false;
        } catch (e) {
            return false;
        }
    }

    /**
     * ApiError-Klasse: typisierter API-Fehler.
     */
    class ApiError extends Error {
        constructor(status, code, message) {
            super(message);
            this.name = 'ApiError';
            this.status = status;
            this.code = code;
        }
    }

    // --------------------------------------------------------
    // Öffentliche API-Methoden
    // --------------------------------------------------------

    /**
     * GET-Anfrage.
     * @param {string} path
     * @returns {Promise<any>}
     */
    function get(path) {
        return request(path, { method: 'GET' });
    }

    /**
     * POST-Anfrage.
     * @param {string} path
     * @param {Object} body
     * @returns {Promise<any>}
     */
    function post(path, body) {
        return request(path, {
            method: 'POST',
            body: body ? JSON.stringify(body) : undefined
        });
    }

    /**
     * PUT-Anfrage.
     * @param {string} path
     * @param {Object} body
     * @returns {Promise<any>}
     */
    function put(path, body) {
        return request(path, {
            method: 'PUT',
            body: JSON.stringify(body)
        });
    }

    /**
     * PATCH-Anfrage.
     * @param {string} path
     * @param {Object} body
     * @returns {Promise<any>}
     */
    function patch(path, body) {
        return request(path, {
            method: 'PATCH',
            body: JSON.stringify(body)
        });
    }

    /**
     * DELETE-Anfrage.
     * @param {string} path
     * @returns {Promise<any>}
     */
    function del(path) {
        return request(path, { method: 'DELETE' });
    }

    /**
     * POST-Anfrage ohne Body.
     * @param {string} path
     * @returns {Promise<any>}
     */
    function postEmpty(path) {
        return request(path, { method: 'POST' });
    }

    // --------------------------------------------------------
    // Auth-Methoden
    // --------------------------------------------------------

    /**
     * Login: E-Mail + Passwort + optional TOTP.
     * @param {string} email
     * @param {string} password
     * @param {string} [totp]
     * @returns {Promise<{access_token, refresh_token, user}>}
     */
    async function login(email, password, totp) {
        const body = { email: email, password: password };
        if (totp) body.totp_code = totp;
        const response = await fetch(API_BASE + '/auth/login', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Accept': 'application/json'
            },
            body: JSON.stringify(body),
            credentials: 'same-origin'
        });
        const data = response.ok ? await response.json() : (await response.json().catch(() => null));
        if (response.ok) {
            const payload = (data && data.data) ? data.data : data;
            if (payload.access_token) {
                setAccessToken(payload.access_token);
                if (payload.refresh_token) setRefreshToken(payload.refresh_token);
            }
            return payload;
        }
        // MFA erforderlich?
        if (data && data.data && data.data.mfa_required) {
            throw Object.assign(new ApiError(401, 'MFA_REQUIRED', 'MFA-Code erforderlich.'), { mfaRequired: true });
        }
        let code = (data && data.error) || 'LOGIN_FAILED';
        let msg = (data && data.message) || 'Anmeldung fehlgeschlagen.';
        throw new ApiError(response.status, code, msg);
    }

    /**
     * Logout: serverseitige Invalidierung + lokale Token-Löschung.
     * @returns {Promise<void>}
     */
    async function logout() {
        try {
            await postEmpty('/auth/logout');
        } catch (e) {
            // Logout auf dem Server darf fehlschlagen — lokal wird immer gelöscht
        }
        clearTokens();
    }

    /**
     * Lädt die aktuelle Benutzer-Info.
     * @returns {Promise<Object>}
     */
    function me() {
        return get('/auth/me');
    }

    // --------------------------------------------------------
    // Patienten-Methoden
    // --------------------------------------------------------

    /**
     * Listet Patienten mit optionaler Suche und Pagination.
     * @param {Object} [params] - { q, page, per_page, status }
     * @returns {Promise<any>}
     */
    function listPatients(params) {
        const qs = buildQuery(params);
        return get('/patients' + qs);
    }

    /**
     * Holt einen einzelnen Patienten.
     * @param {string} id
     * @returns {Promise<any>}
     */
    function getPatient(id) {
        return get('/patients/' + encodeURIComponent(id));
    }

    /**
     * Aktualisiert einen Patienten.
     * @param {string} id
     * @param {Object} data
     * @returns {Promise<any>}
     */
    function updatePatient(id, data) {
        return put('/patients/' + encodeURIComponent(id), data);
    }

    /**
     * Erstellt einen neuen Patienten.
     * @param {Object} data
     * @returns {Promise<any>}
     */
    function createPatient(data) {
        return post('/patients', data);
    }

    /**
     * Holt den Audit-Trail eines Patienten.
     * @param {string} id
     * @returns {Promise<any>}
     */
    function getPatientAudit(id) {
        return get('/patients/' + encodeURIComponent(id) + '/audit');
    }

    // --------------------------------------------------------
    // Termin-Methoden
    // --------------------------------------------------------

    /**
     * Listet Termine im Zeitraum.
     * @param {Object} [params] - { from, to, page, per_page }
     * @returns {Promise<any>}
     */
    function listAppointments(params) {
        const qs = buildQuery(params);
        return get('/appointments' + qs);
    }

    /**
     * Erstellt einen Termin.
     * @param {Object} data
     * @returns {Promise<any>}
     */
    function createAppointment(data) {
        return post('/appointments', data);
    }

    /**
     * Storniert einen Termin.
     * @param {string} id
     * @returns {Promise<any>}
     */
    function cancelAppointment(id) {
        return post('/appointments/' + encodeURIComponent(id) + '/cancel');
    }

    // --------------------------------------------------------
    // Dashboard-Methoden
    // --------------------------------------------------------

    /**
     * Holt Dashboard-Statistiken.
     * @returns {Promise<any>}
     */
    function getDashboardStats() {
        return get('/dashboard/stats');
    }

    /**
     * Holt den Status aller Module.
     * @returns {Promise<any>}
     */
    function getModuleStatus() {
        return get('/dashboard/modules');
    }

    /**
     * Holt den Audit-Log (letzte Einträge).
     * @param {Object} [params]
     * @returns {Promise<any>}
     */
    function getAuditLog(params) {
        const qs = buildQuery(params);
        return get('/audit' + qs);
    }

    // --------------------------------------------------------
    // Einstellungen-Methoden
    // --------------------------------------------------------

    /**
     * Holt die eigenen Profil-Daten.
     * @returns {Promise<any>}
     */
    function getProfile() {
        return get('/settings/profile');
    }

    /**
     * Aktualisiert das eigene Profil.
     * @param {Object} data
     * @returns {Promise<any>}
     */
    function updateProfile(data) {
        return put('/settings/profile', data);
    }

    /**
     * Generiert einen neuen API-Key.
     * @returns {Promise<any>}
     */
    function generateApiKey() {
        return post('/settings/api-key');
    }

    /**
     * Holt den aktuellen API-Key.
     * @returns {Promise<any>}
     */
    function getApiKey() {
        return get('/settings/api-key');
    }

    /**
     * Revoked den API-Key.
     * @returns {Promise<any>}
     */
    function revokeApiKey() {
        return del('/settings/api-key');
    }

    // --------------------------------------------------------
    // GDT-Methoden
    // --------------------------------------------------------

    /**
     * Listet vorhandene GDT-Exporte.
     * @returns {Promise<any>}
     */
    function listGdtExports() {
        return get('/export/gdt');
    }

    /**
     * Startet einen GDT-Export.
     * @param {Object} [options] - { include_deleted }
     * @returns {Promise<any>}
     */
    function startGdtExport(options) {
        return post('/export/gdt', options || {});
    }

    // --------------------------------------------------------
    // Hilfsfunktionen
    // --------------------------------------------------------

    /**
     * Baut eine Query-String aus einem Objekt.
     * @param {Object} [params]
     * @returns {string}
     */
    function buildQuery(params) {
        if (!params) return '';
        const parts = [];
        Object.keys(params).forEach(function (key) {
            const value = params[key];
            if (value === null || value === undefined || value === '') return;
            parts.push(encodeURIComponent(key) + '=' + encodeURIComponent(value));
        });
        return parts.length ? '?' + parts.join('&') : '';
    }

    // --------------------------------------------------------
    // Öffentliche API
    // --------------------------------------------------------
    global.KISApi = {
        get: get,
        post: post,
        put: put,
        patch: patch,
        del: del,
        postEmpty: postEmpty,
        login: login,
        logout: logout,
        me: me,
        listPatients: listPatients,
        getPatient: getPatient,
        updatePatient: updatePatient,
        createPatient: createPatient,
        getPatientAudit: getPatientAudit,
        listAppointments: listAppointments,
        createAppointment: createAppointment,
        cancelAppointment: cancelAppointment,
        getDashboardStats: getDashboardStats,
        getModuleStatus: getModuleStatus,
        getAuditLog: getAuditLog,
        getProfile: getProfile,
        updateProfile: updateProfile,
        generateApiKey: generateApiKey,
        getApiKey: getApiKey,
        revokeApiKey: revokeApiKey,
        listGdtExports: listGdtExports,
        startGdtExport: startGdtExport,
        getAccessToken: getAccessToken,
        setAccessToken: setAccessToken,
        getRefreshToken: getRefreshToken,
        setRefreshToken: setRefreshToken,
        clearTokens: clearTokens,
        ApiError: ApiError
    };

})(window);
