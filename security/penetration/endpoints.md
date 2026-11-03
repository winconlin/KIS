# KIS — Testbare Endpunkte

**Stand:** 03.11.2026
**Zweck:** Liste aller testbaren Endpunkte mit Risikobewertung für Penetration-Tests

---

## Authentifizierung

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/auth/login` | POST | **Critical** | Brute-Force, SQL-Injection, Auth-Bypass, Rate Limiting |
| `/api/v1/auth/refresh` | POST | **High** | Token-Reuse, Token-Expiry, Refresh-Rotation |
| `/api/v1/auth/logout` | POST | **Medium** | Token-Widerruf, Session-Invalidation |

### Details

**`POST /api/v1/auth/login`**
- **Body:** `{"username": "...", "password": "..."}`
- **Risiken:**
  - Brute-Force (Rate Limiting prüfen)
  - SQL-Injection in Username/Password
  - Auth-Bypass (Header Injection)
  - Timing Attack (User Enumeration)
- **Erwartete Antwort:**
  - 200: `{"access_token": "...", "refresh_token": "..."}`
  - 401: `{"error": "invalid credentials"}`
  - 429: `{"error": "rate limit exceeded"}`

**`POST /api/v1/auth/refresh`**
- **Body:** `{"refresh_token": "..."}`
- **Risiken:**
  - Token-Reuse (nach Rotation)
  - Token-Expiry (nach 24h)
  - Refresh-Token-Compromittierung
- **Erwartete Antwort:**
  - 200: Neues Token-Paar
  - 401: `{"error": "invalid refresh token"}`

---

## Patienten

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/patients` | GET | **High** | IDOR, Mass Assignment, Info Leak |
| `/api/v1/patients` | POST | **High** | RBAC, Mass Assignment, Injection |
| `/api/v1/patients/{id}` | GET | **Critical** | IDOR, Auth Bypass |
| `/api/v1/patients/{id}` | PUT | **High** | RBAC, Mass Assignment, Injection |
| `/api/v1/patients/{id}` | DELETE | **Critical** | RBAC, Injection |
| `/api/v1/patients/{id}/documents` | GET | **High** | IDOR, Auth Bypass |

### Details

**`GET /api/v1/patients/{id}`**
- **Risiken:**
  - IDOR (Anderer Patient ohne Berechtigung)
  - Auth Bypass (Header Injection)
  - Info Leak (Sensible Daten ohne Berechtigung)
- **Tests:**
  1. Mit Viewer-Token: Andere Patienten-ID
  2. Mit Nurse-Token: Nicht-eigene Patienten
  3. Ohne Token: Unauthenticated Access
  4. Mit Header Injection: `X-Forwarded-User: admin`
- **Erwartete Antwort:**
  - 200: Patientendaten (nur mit Berechtigung)
  - 401: Unauthenticated
  - 403: Forbidden
  - 404: Not Found

**`POST /api/v1/patients`**
- **Body:** `{"name": "...", "address": "...", "birth_date": "...", "insurance_no": "..."}`
- **Risiken:**
  - RBAC (Viewer/Pflege dürfen nicht anlegen)
  - Mass Assignment (Sensible Felder setzen)
  - SQL-Injection in Name/Address
  - Validation (Pflichtfelder, Format)
- **Tests:**
  1. Mit Viewer-Token: POST
  2. Mit Pflege-Token: POST
  3. Mit Admin-Token: POST + sensible Felder
  4. SQL-Injection in Name: `test'--`
- **Erwartete Antwort:**
  - 201: Created (nur mit Berechtigung)
  - 401: Unauthenticated
  - 403: Forbidden
  - 422: Validation Error

---

## Termine

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/appointments` | GET | **Medium** | IDOR, Filter Injection |
| `/api/v1/appointments` | POST | **Medium** | RBAC, Mass Assignment |
| `/api/v1/appointments/{id}` | GET | **Medium** | IDOR |
| `/api/v1/appointments/{id}` | PUT | **Medium** | RBAC, Injection |
| `/api/v1/appointments/{id}` | DELETE | **High** | RBAC |

### Details

**`GET /api/v1/appointments`**
- **Query-Params:** `?patient_id=...&date=...&status=...`
- **Risiken:**
  - IDOR (Termine anderer Patienten)
  - Filter Injection (SQL in Query-Params)
  - Mass Assignment (Sensible Felder)
- **Tests:**
  1. Mit Nurse-Token: Termine anderer Patienten
  2. SQL-Injection in patient_id: `1' OR '1'='1`
  3. Date-Format Injection: `2026-11-03'--`
- **Erwartete Antwort:**
  - 200: Terminliste (nur eigene/erlaubte)
  - 401: Unauthenticated
  - 403: Forbidden
  - 500: SQL Error (bei Injection)

---

## Medikamente

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/medications` | GET | **Medium** | IDOR, Filter Injection |
| `/api/v1/medications` | POST | **Medium** | RBAC, Injection |
| `/api/v1/medications/{id}` | GET | **Medium** | IDOR |
| `/api/v1/medications/{id}` | PUT | **Medium** | RBAC, Injection |

### Details

**`GET /api/v1/medications`**
- **Query-Params:** `?patient_id=...&date=...`
- **Risiken:**
  - IDOR (Medikamente anderer Patienten)
  - Filter Injection
  - Info Leak (Sensible Medikamentendaten)
- **Tests:**
  1. Mit Pflege-Token: Medikamente anderer Patienten
  2. SQL-Injection in patient_id
  3. Ohne Token: Unauthenticated Access
- **Erwartete Antwort:**
  - 200: Medikamentenliste (nur eigene/erlaubte)
  - 401: Unauthenticated
  - 403: Forbidden

---

## Dokumente

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/documents` | GET | **High** | IDOR, Auth Bypass |
| `/api/v1/documents` | POST | **High** | RBAC, File Upload |
| `/api/v1/documents/{id}` | GET | **Critical** | IDOR, File Download |
| `/api/v1/documents/{id}` | DELETE | **High** | RBAC |

### Details

**`GET /api/v1/documents/{id}`**
- **Risiken:**
  - IDOR (Dokumente anderer Patienten)
  - Auth Bypass
  - File Download (sensible Dokumente)
  - Path Traversal (`../../etc/passwd`)
- **Tests:**
  1. Mit Viewer-Token: Andere Dokument-ID
  2. Path Traversal: `../../etc/passwd`
  3. Ohne Token: Unauthenticated Access
  4. Mit Header Injection: `X-Forwarded-User: admin`
- **Erwartete Antwort:**
  - 200: Dokument (nur mit Berechtigung)
  - 401: Unauthenticated
  - 403: Forbidden
  - 404: Not Found

**`POST /api/v1/documents`**
- **Body:** `multipart/form-data` (Datei + Metadata)
- **Risiken:**
  - RBAC (Pflege darf nicht hochladen)
  - File Upload (schädliche Dateien)
  - MIME-Type Validation
  - File Size Limit
- **Tests:**
  1. Mit Pflege-Token: Upload
  2. Schädliche Datei: `malware.pdf` (mit Script)
  3. MIME-Type Fälschung: `script.js` als `.pdf`
  4. Große Datei: 100MB
- **Erwartete Antwort:**
  - 201: Created (nur mit Berechtigung)
  - 401: Unauthenticated
  - 403: Forbidden
  - 413: File Too Large
  - 422: Invalid File Type

---

## Administration

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/admin/config` | GET | **Critical** | RBAC, Info Leak |
| `/api/v1/admin/config` | PUT | **Critical** | RBAC, Injection |
| `/api/v1/admin/users` | GET | **Critical** | RBAC, Info Leak |
| `/api/v1/admin/users` | POST | **Critical** | RBAC, Mass Assignment |
| `/api/v1/admin/users/{id}` | PUT | **Critical** | RBAC, Injection |
| `/api/v1/admin/users/{id}` | DELETE | **Critical** | RBAC |

### Details

**`GET /api/v1/admin/config`**
- **Risiken:**
  - RBAC (Nur Admin)
  - Info Leak (Sensible Konfigurationsdaten)
  - Injection
- **Tests:**
  1. Mit Doctor-Token: GET
  2. Mit Nurse-Token: GET
  3. Ohne Token: GET
  4. Mit Header Injection: `X-Role: admin`
- **Erwartete Antwort:**
  - 200: Config (nur Admin)
  - 401: Unauthenticated
  - 403: Forbidden

**`POST /api/v1/admin/users`**
- **Body:** `{"username": "...", "email": "...", "role": "admin", "password": "..."}`
- **Risiken:**
  - RBAC (Nur Admin)
  - Mass Assignment (Role-Eskalation)
  - Injection
  - Weak Password
- **Tests:**
  1. Mit Doctor-Token: POST
  2. Mit Nurse-Token: POST
  3. Role-Eskalation: `{"role": "admin"}` mit Viewer-Token
  4. Schwaches Passwort: `password123`
- **Erwartete Antwort:**
  - 201: Created (nur Admin)
  - 401: Unauthenticated
  - 403: Forbidden
  - 422: Validation Error

---

## Audit-Log

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/api/v1/audit` | GET | **High** | RBAC, Info Leak |
| `/api/v1/audit/export` | GET | **High** | RBAC, Data Exfiltration |

### Details

**`GET /api/v1/audit`**
- **Query-Params:** `?from=...&to=...&user_id=...&action=...`
- **Risiken:**
  - RBAC (Nur Auditor/Admin)
  - Info Leak (Sensible Audit-Daten)
  - Data Exfiltration (Große Mengen)
- **Tests:**
  1. Mit Doctor-Token: GET
  2. Mit Nurse-Token: GET
  3. Mit Auditor-Token: GET
  4. Große Menge: `?from=2020-01-01&to=2026-12-31`
- **Erwartete Antwort:**
  - 200: Audit-Log (nur Auditor/Admin)
  - 401: Unauthenticated
  - 403: Forbidden
  - 429: Rate Limit

**`GET /api/v1/audit/export`**
- **Query-Params:** `?format=csv|json&from=...&to=...`
- **Risiken:**
  - RBAC (Nur Auditor/Admin)
  - Data Exfiltration (Komplettes Audit-Log)
  - DDoS (Große Exporte)
- **Tests:**
  1. Mit Doctor-Token: GET
  2. Mit Auditor-Token: GET (großer Zeitraum)
  3. Format-Wechsel: `?format=csv`
- **Erwartete Antwort:**
  - 200: Export (nur Auditor/Admin)
  - 401: Unauthenticated
  - 403: Forbidden
  - 504: Timeout (bei großer Menge)

---

## Health & Metrics

| Endpunkt | Methode | Risiko | Tests |
|----------|---------|--------|-------|
| `/health` | GET | **Low** | Info Leak |
| `/metrics` | GET | **Medium** | Info Leak |
| `/api/v1` | GET | **Low** | Info Leak |

### Details

**`GET /health`**
- **Risiken:**
  - Info Leak (Versionsinformationen)
- **Tests:**
  1. GET ohne Token
  2. Response-Header prüfen
- **Erwartete Antwort:**
  - 200: `{"status": "ok"}`
  - Keine sensiblen Informationen

**`GET /metrics`**
- **Risiken:**
  - Info Leak (Performance-Metriken)
- **Tests:**
  1. GET ohne Token
  2. Response-Header prüfen
- **Erwartete Antwort:**
  - 200: Metriken (nur interne Daten)
  - Keine sensiblen Informationen

---

## TLS & Netzwerk

| Test | Risiko | Beschreibung |
|------|--------|--------------|
| **TLS 1.3** | **Medium** | Protokoll-Version prüfen (TLS 1.0/1.1/1.2/1.3) |
| **HSTS** | **Medium** | `Strict-Transport-Security` Header prüfen |
| **Zertifikat** | **Medium** | Gültigkeit, Chain, SANs |
| **Key Exchange** | **Low** | RSA vs. ECDHE |
| **Cipher Suite** | **Low** | Starke vs. schwache Cipher |
| **CORS** | **Medium** | `Access-Control-Allow-Origin` Header |
| **Rate Limiting** | **High** | 100 API-Anfragen/Minute testen |
| **DoS** | **High** | Lasttest (1000 Requests/Minute) |

### Details

**TLS 1.3**
- **Tests:**
  1. `openssl s_client -connect kis-test.local:443`
  2. `nmap --script ssl-enum-ciphers -p 443 kis-test.local`
  3. `testssl.sh kis-test.local`
- **Erwartet:**
  - TLS 1.3 (min. TLS 1.2)
  - Starke Cipher Suites (AES-256-GCM, ChaCha20)
  - Gültiges Zertifikat (1 Jahr, CA-Chain)

**HSTS**
- **Tests:**
  1. `curl -I https://kis-test.local`
  2. Response-Header prüfen
- **Erwartet:**
  - `Strict-Transport-Security: max-age=31536000; includeSubDomains; preload`

**CORS**
- **Tests:**
  1. `curl -H "Origin: https://evil.com" -I https://kis-test.local`
  2. Response-Header prüfen
- **Erwartet:**
  - Keine `Access-Control-Allow-Origin` Header
  - Oder: `Access-Control-Allow-Origin: https://kis-test.local`

**Rate Limiting**
- **Tests:**
  1. 100 Requests in 1 Minute (erlaubt)
  2. 101+ Requests in 1 Minute (blockiert)
  3. Response-Code 429 prüfen
- **Erwartet:**
  - 100 Requests: 200
  - 101+ Requests: 429

---

## Zusammenfassung

| Risiko | Anzahl | Endpunkte |
|--------|--------|-----------|
| **Critical** | 5 | `/api/v1/patients/{id}` (GET/DELETE), `/api/v1/admin/config` (GET/PUT), `/api/v1/admin/users` (POST) |
| **High** | 12 | `/api/v1/auth/login`, `/api/v1/patients` (GET/POST), `/api/v1/patients/{id}` (PUT), `/api/v1/appointments/{id}` (DELETE), `/api/v1/documents` (GET/POST), `/api/v1/documents/{id}` (GET/DELETE), `/api/v1/audit` (GET), `/api/v1/audit/export`, Rate Limiting, DoS |
| **Medium** | 10 | `/api/v1/auth/refresh`, `/api/v1/auth/logout`, `/api/v1/appointments` (GET/POST), `/api/v1/appointments/{id}` (GET/PUT), `/api/v1/medications` (GET/POST), `/api/v1/medications/{id}` (GET/PUT), `/metrics`, CORS, TLS, HSTS |
| **Low** | 4 | `/health`, `/api/v1`, Key Exchange, Cipher Suite |

**Total: 31 testbare Endpunkte**
