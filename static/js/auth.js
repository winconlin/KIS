/**
 * KIS — Krankenhausinformationssystem
 * Auth-Modul: Login-Flow, MFA/TOTP, Token-Management
 *
 * Zuständig für:
 *  - Login-Formular-Logik (E-Mail, Passwort, MFA)
 *  - MFA-Phasenwechsel (TOTP-Eingabe)
 *  - Session-Restore (Token-Prüfung beim Start)
 *  - Logout
 *  - Benutzer-Info-Laden und -Anzeigen
 */

(function (global) {
    'use strict';

    // Referenzen auf DOM-Elemente (werden beim Init gesetzt)
    let loginOverlay = null;
    let loginForm = null;
    let loginEmail = null;
    let loginPassword = null;
    let loginTotp = null;
    let mfaGroup = null;
    let submitBtn = null;
    let generalError = null;
    let appEl = null;

    // MFA-Status: ist der TOTP-Code erforderlich?
    let mfaRequired = false;

    // --------------------------------------------------------
    // Initialisierung
    // --------------------------------------------------------

    /**
     * Initialisiert das Auth-Modul: bindet DOM-Elemente und Event-Listener.
     * Muss nach DOM-Loading aufgerufen werden.
     */
    function init() {
        loginOverlay = document.getElementById('login-overlay');
        loginForm = document.getElementById('login-form');
        loginEmail = document.getElementById('login-email');
        loginPassword = document.getElementById('login-password');
        loginTotp = document.getElementById('login-totp');
        mfaGroup = document.getElementById('mfa-group');
        submitBtn = document.getElementById('login-submit');
        generalError = document.getElementById('login-general-error');
        appEl = document.getElementById('app');

        if (!loginForm) return;

        // Submit
        loginForm.addEventListener('submit', handleSubmit);

        // Passwort-Toggle
        const toggle = document.getElementById('password-toggle');
        if (toggle) {
            toggle.addEventListener('click', function () {
                const isPassword = loginPassword.type === 'password';
                loginPassword.type = isPassword ? 'text' : 'password';
                toggle.setAttribute('aria-pressed', String(isPassword));
                toggle.setAttribute('aria-label', isPassword ? 'Passwort verbergen' : 'Passwort anzeigen');
            });
        }

        // TOTP: nur Ziffern zulassen
        if (loginTotp) {
            loginTotp.addEventListener('input', function () {
                loginTotp.value = loginTotp.value.replace(/[^0-9]/g, '').slice(0, 6);
            });
            // Bei 6 Ziffern → automatisch absenden
            loginTotp.addEventListener('input', function () {
                if (loginTotp.value.length === 6 && mfaRequired) {
                    handleSubmit(event);
                }
            });
        }

        // Logout-Button
        const logoutBtn = document.getElementById('logout-btn');
        if (logoutBtn) {
            logoutBtn.addEventListener('click', function () {
                logout();
            });
        }

        // Theme-Toggle
        const themeToggle = document.getElementById('theme-toggle');
        if (themeToggle) {
            themeToggle.addEventListener('click', toggleTheme);
        }

        // Initiales Theme anwenden
        applyStoredTheme();
    }

    // --------------------------------------------------------
    // Login-Flow
    // --------------------------------------------------------

    /**
     * Händelt den Login-Submit.
     * @param {Event} event
     */
    async function handleSubmit(event) {
        if (event) event.preventDefault();

        const email = loginEmail ? loginEmail.value.trim() : '';
        const password = loginPassword ? loginPassword.value : '';
        const totp = (mfaRequired && loginTotp) ? loginTotp.value.trim() : undefined;

        // Clientseitige Validierung
        if (!validateEmail(email)) {
            showFieldError('login-email', 'Bitte gültige E-Mail-Adresse eingeben.');
            return;
        }
        clearFieldError('login-email');
        if (!password) {
            showFieldError('login-password', 'Bitte Passwort eingeben.');
            return;
        }
        clearFieldError('login-password');

        // UI in Ladezustand
        setLoading(true);
        hideGeneralError();

        try {
            const result = await global.KISApi.login(email, password, totp);
            // Login erfolgreich
            clearFieldErrors();
            await onLoginSuccess(result);
        } catch (err) {
            // Fehlerbehandlung
            setLoading(false);
            if (err.code === 'MFA_REQUIRED') {
                // MFA erforderlich → TOTP-Feld anzeigen
                mfaRequired = true;
                showMfaField();
                showGeneralError('Bitte MFA-Code eingeben (6-stellig).');
                if (loginTotp) loginTotp.focus();
            } else if (err.code === 'INVALID_CREDENTIALS') {
                showGeneralError('E-Mail oder Passwort ist falsch.');
            } else if (err.code === 'MFA_INVALID') {
                showGeneralError('MFA-Code ist ungültig. Bitte erneut versuchen.');
                if (loginTotp) {
                    loginTotp.value = '';
                    loginTotp.focus();
                }
            } else if (err.code === 'ACCOUNT_LOCKED') {
                showGeneralError('Konto ist gesperrt. Bitte später erneut versuchen.');
            } else if (err.code === 'NETWORK_ERROR') {
                showGeneralError('Server nicht erreichbar. Bitte Verbindung prüfen.');
            } else {
                showGeneralError(err.message || 'Anmeldung fehlgeschlagen.');
            }
        }
    }

    /**
     * Erfolgreicher Login: Token speichern, App anzeigen, Benutzer laden.
     * @param {Object} result
     */
    async function onLoginSuccess(result) {
        // Token sind bereits via KISApi.login gespeichert

        // App einblenden, Login verbergen
        if (loginOverlay) loginOverlay.hidden = true;
        if (appEl) appEl.hidden = false;

        // Benutzer-Info laden und Sidebar füllen
        try {
            const user = result.user || (await global.KISApi.me());
            renderUserInfo(user);
        } catch (e) {
            // me() fehlgeschlagen → leere Defaults
            renderUserInfo(null);
        }

        // Router starten
        if (typeof global.KISApp !== 'undefined' && global.KISApp.start) {
            global.KISApp.start();
        }

        // Toast
        if (typeof global.KISToast !== 'undefined') {
            global.KISToast.show('Erfolgreich angemeldet.', 'success');
        }
    }

    /**
     * Rendert die Benutzer-Info in der Sidebar.
     * @param {Object|null} user
     */
    function renderUserInfo(user) {
        const nameEl = document.getElementById('user-name');
        const roleEl = document.getElementById('user-role');
        const avatarEl = document.getElementById('user-avatar');

        if (!user) {
            if (nameEl) nameEl.textContent = 'Benutzer';
            if (roleEl) roleEl.textContent = '';
            if (avatarEl) avatarEl.textContent = '?';
            return;
        }

        const fullName = user.full_name || user.display_name || user.name || (user.email ? user.email.split('@')[0] : 'Benutzer');
        const role = user.role || user.role_name || '';

        if (nameEl) nameEl.textContent = fullName;
        if (roleEl) roleEl.textContent = role;
        if (avatarEl) {
            // Initialen als Avatar
            const initials = fullName.split(' ').map(function (p) {
                return p.charAt(0).toUpperCase();
            }).slice(0, 2).join('');
            avatarEl.textContent = initials || '?';
        }

        // Benutzer im Speicher halten
        try {
            localStorage.setItem('kis_user', JSON.stringify(user));
        } catch (e) {
        }
    }

    /**
     * Zeigt das MFA-Feld an.
     */
    function showMfaField() {
        if (mfaGroup) {
            mfaGroup.hidden = false;
        }
    }

    // --------------------------------------------------------
    // Logout
    // --------------------------------------------------------

    /**
     * Logout: serverseitig + lokal, zurück zum Login.
     */
    async function logout() {
        try {
            await global.KISApi.logout();
        } catch (e) {
            // egal
        }
        clearLocalSession();
        showLoginScreen();
        if (typeof global.KISToast !== 'undefined') {
            global.KISToast.show('Erfolgreich abgemeldet.', 'info');
        }
    }

    /**
     * Erzwingt Logout ohne Bestätigung (bei Token-Abgelauf).
     */
    function forceLogout() {
        clearLocalSession();
        showLoginScreen();
    }

    /**
     * Löscht lokale Session-Daten.
     */
    function clearLocalSession() {
        global.KISApi.clearTokens();
        try {
            localStorage.removeItem('kis_user');
        } catch (e) {
        }
        // Router-Reset
        if (typeof global.KISApp !== 'undefined' && global.KISApp.stop) {
            global.KISApp.stop();
        }
    }

    /**
     * Zeigt die Login-Maske.
     */
    function showLoginScreen() {
        if (appEl) appEl.hidden = true;
        if (loginOverlay) {
            loginOverlay.hidden = false;
        }
        // Formular zurücksetzen
        mfaRequired = false;
        if (mfaGroup) mfaGroup.hidden = true;
        if (loginForm) loginForm.reset();
        clearFieldErrors();
        hideGeneralError();
        if (loginEmail) loginEmail.focus();
    }

    // --------------------------------------------------------
    // Session-Restore
    // --------------------------------------------------------

    /**
     * Prüft, ob eine gültige Session vorhanden ist.
     * Falls ja → App anzeigen, sonst Login-Maske.
     * @returns {Promise<void>}
     */
    async function restoreSession() {
        const token = global.KISApi.getAccessToken();
        if (!token) {
            showLoginScreen();
            return;
        }

        // Token-Validität prüfen via /auth/me
        try {
            const user = await global.KISApi.me();
            // Valid → App anzeigen
            if (loginOverlay) loginOverlay.hidden = true;
            if (appEl) appEl.hidden = false;
            renderUserInfo(user);
            if (typeof global.KISApp !== 'undefined' && global.KISApp.start) {
                global.KISApp.start();
            }
        } catch (e) {
            // Token ungültig oder abgelaufen → Login
            clearLocalSession();
            showLoginScreen();
        }
    }

    // --------------------------------------------------------
    // Theme
    // --------------------------------------------------------

    const THEME_KEY = 'kis_theme';

    /**
     * Wechsel zwischen Dark/Light-Theme.
     */
    function toggleTheme() {
        const html = document.documentElement;
        const current = html.getAttribute('data-theme') || 'dark';
        const next = current === 'dark' ? 'light' : 'dark';
        html.setAttribute('data-theme', next);
        try {
            localStorage.setItem(THEME_KEY, next);
        } catch (e) {
        }
    }

    /**
     * Wendet das gespeicherte Theme an.
     */
    function applyStoredTheme() {
        let theme = 'dark';
        try {
            theme = localStorage.getItem(THEME_KEY) || 'dark';
        } catch (e) {
        }
        // System-Einstellung als Fallback
        if (theme === 'system') {
            theme = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
        }
        document.documentElement.setAttribute('data-theme', theme);
    }

    // --------------------------------------------------------
    // Validierung
    // --------------------------------------------------------

    /**
     * Prüft E-Mail-Format (einfache Regex, clientseitig).
     * @param {string} email
     * @returns {boolean}
     */
    function validateEmail(email) {
        if (!email) return false;
        // Einfache, pragmatische E-Mail-Prüfung
        return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email);
    }

    /**
     * Zeigt einen Feldfehler.
     * @param {string} fieldId
     * @param {string} message
     */
    function showFieldError(fieldId, message) {
        const errEl = document.getElementById(fieldId + '-error');
        const inputEl = document.getElementById(fieldId);
        if (errEl) {
            errEl.textContent = message;
            errEl.hidden = false;
        }
        if (inputEl) {
            inputEl.setAttribute('aria-invalid', 'true');
            inputEl.classList.add('input-error');
        }
    }

    /**
     * Entfernt einen Feldfehler.
     * @param {string} fieldId
     */
    function clearFieldError(fieldId) {
        const errEl = document.getElementById(fieldId + '-error');
        const inputEl = document.getElementById(fieldId);
        if (errEl) {
            errEl.textContent = '';
            errEl.hidden = true;
        }
        if (inputEl) {
            inputEl.removeAttribute('aria-invalid');
            inputEl.classList.remove('input-error');
        }
    }

    /**
     * Entfernt alle Feldfehler.
     */
    function clearFieldErrors() {
        ['login-email', 'login-password', 'login-totp'].forEach(clearFieldError);
    }

    /**
     * Zeigt einen allgemeinen Fehler.
     * @param {string} message
     */
    function showGeneralError(message) {
        if (generalError) {
            generalError.textContent = message;
            generalError.hidden = false;
        }
    }

    /**
     * Versteckt den allgemeinen Fehler.
     */
    function hideGeneralError() {
        if (generalError) {
            generalError.textContent = '';
            generalError.hidden = true;
        }
    }

    /**
     * Setzt den Ladezustand des Submit-Buttons.
     * @param {boolean} loading
     */
    function setLoading(loading) {
        if (!submitBtn) return;
        submitBtn.disabled = loading;
        const text = submitBtn.querySelector('.btn-text');
        const spinner = submitBtn.querySelector('.btn-spinner');
        if (text) text.textContent = loading ? 'Anmeldung…' : 'Anmelden';
        if (spinner) spinner.hidden = !loading;
    }

    // --------------------------------------------------------
    // Öffentliche API
    // --------------------------------------------------------
    global.KISAuth = {
        init: init,
        restoreSession: restoreSession,
        logout: logout,
        forceLogout: forceLogout,
        renderUserInfo: renderUserInfo,
        toggleTheme: toggleTheme
    };

})(window);
