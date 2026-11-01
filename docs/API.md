# KIS — REST API Referenz

> **Krankenhaus-Informations-System (KIS)**
> Vollständige Referenz der HTTP-API. Endpunkte, Request/Response-Beispiele, Fehlercodes, Authentifizierung und Rate-Limiting.
>
> **Basis-URL:** `http://<host>:8080/api` (Produktion: `https://<host>/api`)
> **Content-Type:** `application/json; charset=utf-8`

---

## 1. Überblick

Die API ist ein RESTful-JSON-Interface, implementiert mit **Axum** (siehe `src/api/mod.rs`). Sie ist unter dem Präfix `/api/` erreichbar. Zusätzlich liegen die FHIR-R4-Endpoints unter `/fhir/` (siehe [INTERFACES.md](INTERFACES.md)).

| Bereich | Präfix | Status |
|---|---|---|
| Core API | `/api/` | Grundgerüst vorhanden |
| FHIR R4 | `/fhir/` | In Entwicklung |
| GUI (HTML) | `/` | Vorhanden (`static/index.html`) |

### Ist-Zustand der Endpunkte (aus `src/api/mod.rs`)

| Methode | Pfad | Handler | Status |
|---|---|---|---|
| `GET` | `/api/health` | `health_check` | ✅ Implementiert (liefert `"OK"`) |
| `GET` | `/api/patients` | `list_patients` | 🔧 Grundgerüst |
| `POST` | `/api/patients` | `create_patient` | 🔧 Grundgerüst |
| `GET` | `/api/patients/:id` | `get_patient` | 🔧 Grundgerüst |
| `GET` | `/api/appointments` | `list_appointments` | 🔧 Grundgerüst |
| `POST` | `/api/appointments` | `create_appointment` | 🔧 Grundgerüst |

> **Hinweis:** Die CRUD-Endpunkte für `doctors` und `medications` sind im aktuellen `create_router()` noch nicht registriert, obwohl die Modelle in `src/models/mod.rs` existieren. Die folgenden Spezifikationen beschreiben den **Soll-Zustand** (Vollständiges API-Design), auf den das Projekt hinsteuert.

---

## 2. Authentifizierung

> **Soll-Zustand.** Die Authentifizierung ist in der aktuellen Codebasis noch nicht implementiert. Für den produktiven Betrieb ist sie **Pflicht** (siehe [SECURITY.md](SECURITY.md) → Zugriffskontrolle).

### 2.1 Bearer-Token (JWT)

Jeder authentifizierungs­pflichtige Request muss den Header tragen:

```
Authorization: Bearer <access_token>
```

### 2.2 Auth-Endpoints (Soll)

| Methode | Pfad | Beschreibung |
|---|---|---|
| `POST` | `/api/auth/login` | Login, liefert Access- + Refresh-Token |
| `POST` | `/api/auth/refresh` | Access-Token erneuern |
| `POST` | `/api/auth/logout` | Session beenden, Token revocieren |
| `GET` | `/api/auth/me` | Aktuelle Benutzer- und Rolleninfo |

**Login-Request:**

```http
POST /api/auth/login
Content-Type: application/json

{
  "username": "dr.mueller",
  "password": "••••••••"
}
```

**Login-Response (200 OK):**

```json
{
  "access_token": "eyJhbGciOiJSUzI1NiIs...",
  "refresh_token": "eyJhbGciOiJSUzI1NiIs...",
  "token_type": "Bearer",
  "expires_in": 900,
  "user": {
    "id": "9f1c2a4e-...",
    "username": "dr.mueller",
    "role": "doctor"
  }
}
```

### 2.3 Offene Endpunkte (ohne Auth)

| Pfad | Begründung |
|---|---|
| `GET /api/health` | Monitoring / Load-Balancer-Health-Check |
| `GET /` (GUI) | Statische HTML-Seite (ohne Daten) |

> **Wichtig:** Alle Endpunkte, die **Patientendaten** zurückgeben oder verändern, erfordern ein gültiges JWT + ausreichende RBAC-Rolle.

---

## 3. Fehlerformat und -codes

Alle Fehlerantworten folgen einem einheitlichen JSON-Schema:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Feld 'first_name' ist erforderlich und darf nicht leer sein.",
    "details": [
      { "field": "first_name", "issue": "required" }
    ],
    "request_id": "req-1a2b3c4d"
  }
}
```

### 3.1 HTTP-Fehlercodes

| Code | Bedeutung | Typische Ursache |
|---|---|---|
| `200` | OK | Erfolgreiche Anfrage |
| `201` | Created | Resource wurde angelegt (POST) |
| `204` | No Content | Erfolg ohne Body (z. B. DELETE) |
| `400` | Bad Request | Ungültige Eingabe (Validierung) |
| `401` | Unauthorized | Fehlendes / abgelaufenes / ungültiges Token |
| `403` | Forbidden | Token gültig, aber Rolle hat keine Berechtigung |
| `404` | Not Found | Resource existiert nicht |
| `409` | Conflict | Zustandskonflikt (z. B. doppelter Termin) |
| `422` | Unprocessable Entity | Semantisch ungültige Anfrage |
| `429` | Too Many Requests | Rate-Limit überschritten |
| `500` | Internal Server Error | Interner Fehler (unbehandelt) |
| `503` | Service Unavailable | System in Wartung / DB nicht verfügbar |

### 3.2 Fehlertypen (logisch)

| `code` | HTTP | Beschreibung |
|---|---|---|
| `AUTH_MISSING` | 401 | Kein Authorization-Header |
| `AUTH_INVALID` | 401 | Token ungültig oder abgelaufen |
| `AUTH_REVOKED` | 401 | Token wurde revociert |
| `PERMISSION_DENIED` | 403 | Rolle nicht berechtigt für die Aktion |
| `VALIDATION_ERROR` | 400 | Feldvalidierung fehlgeschlagen |
| `NOT_FOUND` | 404 | Resource nicht vorhanden |
| `CONFLICT` | 409 | Zustandskonflikt |
| `RATE_LIMITED` | 429 | Zu viele Anfragen |
| `INTERNAL` | 500 | Unerwarteter Fehler |

---

## 4. Endpunkt-Referenz

### 4.1 Health

#### `GET /api/health`

System-Status prüfen. **Keine Authentifizierung erforderlich.**

**Response (200 OK):**

```
OK
```

> **Ist:** Liefert die statische Zeichenkette `"OK"` (siehe `health_check` in `src/api/mod.rs`).
> **Soll:** JSON mit Details:
> ```json
> { "status": "ok", "version": "0.1.0", "db": "connected", "uptime_s": 3600 }
> ```

---

### 4.2 Patienten

#### `GET /api/patients` — Patientenliste

Liste aller Patienten (oder gefiltert). **Auth: admin, doctor, nurse, reception.**

**Query-Parameter:**

| Parameter | Typ | Beschreibung |
|---|---|---|
| `q` | string | Volltextsuche (Vor-/Nachname) |
| `last_name` | string | Filter nach Nachname (Prefix) |
| `page` | int | Seitennummer (Default 1) |
| `limit` | int | Einträge pro Seite (Default 20, Max 100) |

**Beispiel:**

```http
GET /api/patients?q=müller&limit=10
Authorization: Bearer <token>
```

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
      "first_name": "Anna",
      "last_name": "Müller",
      "birth_date": "1985-03-12",
      "insurance_number": "K123456789",
      "created_at": "2026-01-15T09:30:00Z",
      "updated_at": "2026-01-15T09:30:00Z"
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 10,
    "total": 1,
    "total_pages": 1
  }
}
```

> **Datenschutz:** `insurance_number` wird nur für Rollen `admin` und `reception` zurückgegeben; für `doctor`/`nurse` maskiert (z. B. `K1****89`).

---

#### `POST /api/patients` — Patient anlegen

**Auth: admin, reception.**

**Request-Body:**

```json
{
  "first_name": "Anna",
  "last_name": "Müller",
  "birth_date": "1985-03-12",
  "insurance_number": "K123456789"
}
```

**Validierung:**

| Feld | Regel |
|---|---|
| `first_name` | Erforderlich, 1–100 Zeichen |
| `last_name` | Erforderlich, 1–100 Zeichen |
| `birth_date` | Optional, `YYYY-MM-DD`, ≤ heute |
| `insurance_number` | Optional, max. 50 Zeichen |

**Response (201 Created):**

```json
{
  "id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
  "first_name": "Anna",
  "last_name": "Müller",
  "birth_date": "1985-03-12",
  "insurance_number": "K123456789",
  "created_at": "2026-11-01T12:00:00Z",
  "updated_at": "2026-11-01T12:00:00Z"
}
```

**Fehler-Beispiel (400):**

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Ungültige Eingaben",
    "details": [
      { "field": "first_name", "issue": "required" }
    ],
    "request_id": "req-1a2b3c4d"
  }
}
```

---

#### `GET /api/patients/:id` — Einzelpatienten abrufen

**Auth: admin, doctor, nurse, reception.**

```http
GET /api/patients/9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab
Authorization: Bearer <token>
```

**Response (200 OK):**

```json
{
  "id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
  "first_name": "Anna",
  "last_name": "Müller",
  "birth_date": "1985-03-12",
  "insurance_number": "K123456789",
  "created_at": "2026-01-15T09:30:00Z",
  "updated_at": "2026-01-15T09:30:00Z",
  "appointments": [
    { "id": "...", "start_time": "2026-11-02T14:00:00Z", "status": "scheduled" }
  ],
  "medications": [
    { "id": "...", "name": "Ibuprofen", "dosage": "600 mg 3x/Tag" }
  ]
}
```

> **Soll:** Rückgabe mit verknüpften Terminen und Medikationen (eager loading).
> **Fehler (404):** `NOT_FOUND`, wenn die UUID nicht existiert.

---

#### `PUT /api/patients/:id` — Patient aktualisieren *(Soll)*

**Auth: admin, reception.**

**Request-Body:** (teilweise Update möglich)

```json
{
  "last_name": "Müller-Schmidt",
  "insurance_number": "K987654321"
}
```

**Response (200 OK):** aktualisiertes Objekt.

---

#### `DELETE /api/patients/:id` — Patient löschen *(Soll)*

**Auth: admin.**

> **Datenschutz-Hinweis:** Löschung ist nur mit Begründung und wird im Audit-Log protokolliert (`patient.delete`). Alternativ: Anonymisierung statt Löschung.

**Response (204 No Content).**

---

### 4.3 Termine (Appointments)

#### `GET /api/appointments` — Terminliste

**Auth: admin, doctor, nurse, reception.**

**Query-Parameter:**

| Parameter | Typ | Beschreibung |
|---|---|---|
| `patient_id` | UUID | Filter nach Patient |
| `doctor_id` | UUID | Filter nach Arzt |
| `from` | ISO-8601 | Zeitraum ab |
| `to` | ISO-8601 | Zeitraum bis |
| `status` | enum | `scheduled`, `confirmed`, `completed`, `cancelled`, `noshow` |
| `page` / `limit` | int | Paginierung |

**Beispiel:**

```http
GET /api/appointments?from=2026-11-01T00:00:00Z&to=2026-11-08T23:59:59Z&status=scheduled
Authorization: Bearer <token>
```

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "a1b2c3d4-...",
      "patient_id": "9f1c2a4e-...",
      "doctor_id": "e5f6a7b8-...",
      "start_time": "2026-11-02T14:00:00Z",
      "end_time": "2026-11-02T14:30:00Z",
      "status": "scheduled",
      "notes": "Kontrolluntersuchung"
    }
  ],
  "pagination": { "page": 1, "limit": 20, "total": 1, "total_pages": 1 }
}
```

> **Status-Enum** (aus `src/models/mod.rs`, `AppointmentStatus`, `rename_all = "lowercase"`):
> `scheduled` · `confirmed` · `completed` · `cancelled` · `noshow`

---

#### `POST /api/appointments` — Termin anlegen

**Auth: admin, doctor, nurse, reception.**

**Request-Body:**

```json
{
  "patient_id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
  "doctor_id": "e5f6a7b8-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
  "start_time": "2026-11-02T14:00:00Z",
  "end_time": "2026-11-02T14:30:00Z",
  "status": "scheduled",
  "notes": "Kontrolluntersuchung"
}
```

**Validierung:**

| Feld | Regel |
|---|---|
| `patient_id` | Erforderlich, gültige UUID, muss existieren |
| `doctor_id` | Optional, gültige UUID |
| `start_time` | Erforderlich, ISO-8601, > jetzt |
| `end_time` | Erforderlich, > `start_time` |
| `status` | Optional, Default `scheduled` |
| `notes` | Optional, max. 2000 Zeichen |

**Response (201 Created):** vollständiges Termin-Objekt.

**Fehler-Beispiel (409 — Terminüberschneidung):**

```json
{
  "error": {
    "code": "CONFLICT",
    "message": "Der Arzt hat in diesem Zeitraum bereits einen Termin.",
    "request_id": "req-5e6f7a8b"
  }
}
```

---

#### `PUT /api/appointments/:id` — Termin aktualisieren *(Soll)*

**Auth: admin, doctor, nurse, reception.**

Statuswechsel (z. B. `scheduled` → `confirmed` → `completed`/`cancelled`/`noshow`) ist hier über `status` möglich.

---

#### `DELETE /api/appointments/:id` — Termin stornieren *(Soll)*

**Auth: admin, doctor, reception.**

> **Hinweis:** Stornierung setzt `status = "cancelled"` (soft delete), statt die Zeile physisch zu entfernen — Audit-Nachvollziehbarkeit.

**Response (204 No Content).**

---

### 4.4 Ärzte (Doctors) *(Soll)*

#### `GET /api/doctors` — Arztlst

**Auth: admin, doctor, reception.**

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "e5f6a7b8-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
      "name": "Dr. med. Thomas Weber",
      "specialty": "Kardiologie",
      "license_number": "A-123456-BY"
    }
  ]
}
```

#### `POST /api/doctors` — Arzt anlegen

**Auth: admin.**

```json
{
  "name": "Dr. med. Thomas Weber",
  "specialty": "Kardiologie",
  "license_number": "A-123456-BY"
}
```

**Response (201 Created).**

#### `GET /api/doctors/:id` — Einzelarzt

**Auth: admin, doctor, reception.**

#### `PUT /api/doctors/:id` — Arzt aktualisieren *(Soll)*

**Auth: admin.**

#### `DELETE /api/doctors/:id` — Arzt entfernen *(Soll)*

**Auth: admin.**

---

### 4.5 Medikamente (Medications) *(Soll)*

#### `GET /api/medications` — Medikationsliste

**Auth: admin, doctor, nurse.**

**Query-Parameter:**

| Parameter | Typ | Beschreibung |
|---|---|---|
| `patient_id` | UUID | Filter nach Patient |
| `active` | bool | Nur laufende Medikationen (Default `true`) |
| `page` / `limit` | int | Paginierung |

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "b2c3d4e5-...",
      "patient_id": "9f1c2a4e-...",
      "name": "Ibuprofen",
      "dosage": "600 mg 3x/Tag",
      "start_date": "2026-11-01T00:00:00Z",
      "end_date": "2026-11-15T00:00:00Z",
      "prescriber_id": "e5f6a7b8-..."
    }
  ],
  "pagination": { "page": 1, "limit": 20, "total": 1, "total_pages": 1 }
}
```

#### `POST /api/medications` — Medikament verschreiben

**Auth: admin, doctor.**

```json
{
  "patient_id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
  "name": "Ibuprofen",
  "dosage": "600 mg 3x/Tag",
  "start_date": "2026-11-01T00:00:00Z",
  "end_date": "2026-11-15T00:00:00Z",
  "prescriber_id": "e5f6a7b8-1c2d-4e3f-8a9b-0c1d2e3f4a5b"
}
```

**Validierung:**

| Feld | Regel |
|---|---|
| `patient_id` | Erforderlich, gültige UUID |
| `name` | Erforderlich, 1–200 Zeichen |
| `dosage` | Erforderlich, 1–200 Zeichen |
| `start_date` | Erforderlich, ISO-8601 |
| `end_date` | Optional, ≥ `start_date` |
| `prescriber_id` | Optional, gültige UUID (Arzt) |

**Response (201 Created).**

#### `PUT /api/medications/:id` — Medikation anpassen *(Soll)*

**Auth: admin, doctor.** (z. B. Dosis-Änderung, Beendigung)

#### `DELETE /api/medications/:id` — Medikation beenden *(Soll)*

**Auth: admin, doctor.**

> **Hinweis:** Beendigung setzt `end_date = jetzt` (soft), nicht physische Löschung.

---

## 5. Rate-Limiting

> **Soll-Zustand.** Rate-Limiting ist in der aktuellen Codebasis noch nicht implementiert. Für Produktion **Pflicht** (siehe [SECURITY.md](SECURITY.md) → DoS).

| Regel | Wert |
|---|---|
| Standard (pro IP) | 600 Requests / Minute |
| Auth-Endpoints (pro IP) | 20 Requests / Minute |
| Schreib-Endpoints (pro Benutzer) | 300 Requests / Minute |
| Burst | 100 Requests |
| Algorithmus | Token-Bucket |

**Rate-Limit-Header:**

```
X-RateLimit-Limit: 600
X-RateLimit-Remaining: 598
X-RateLimit-Reset: 1730500000
```

**Response bei Überschreitung (429):**

```json
{
  "error": {
    "code": "RATE_LIMITED",
    "message": "Zu viele Anfragen. Bitte warten Sie.",
    "retry_after": 30
  }
}
```

Header: `Retry-After: 30`

---

## 6. Konventionen

### 6.1 Identifikatoren

- Alle `id`-Felder sind **UUID v4** (RFC 4122), seralisiert als lowercase-hex-String.
- Zeitstempel sind **UTC** im Format **ISO 8601** mit `Z`-Suffix (z. B. `2026-11-01T12:00:00Z`).

### 6.2 Paginierung

Standard: Offset-basiert via `page` + `limit`. Response enthält `pagination`-Objekt:

```json
{
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 145,
    "total_pages": 8
  }
}
```

### 6.3 CORS

Aktuell `CorsLayer::permissive()` in `src/api/mod.rs`. Für Produktion auf konkrete Origin(s) einschränken (siehe [CONFIGURATION.md](CONFIGURATION.md) → CORS).

### 6.4 Idempotenz

- `POST`-Endpunkte unterstützen den optionalen Header `Idempotency-Key` (UUID), um doppelte Verarbeitung zu vermeiden.
- `PUT`/`DELETE` sind idempotent.

---

## 7. curl-Beispiele

```bash
BASE=http://localhost:8080/api
TOKEN="<access_token>"

# Health-Check (ohne Auth)
curl -fsS $BASE/health

# Patientenliste
curl -fsS -H "Authorization: Bearer $TOKEN" "$BASE/patients?limit=10"

# Patient anlegen
curl -fsS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"first_name":"Anna","last_name":"Müller","birth_date":"1985-03-12"}' \
  $BASE/patients

# Termin anlegen
curl -fsS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"patient_id":"9f1c2a4e-...","start_time":"2026-11-02T14:00:00Z","end_time":"2026-11-02T14:30:00Z"}' \
  $BASE/appointments

# Medikament verschreiben
curl -fsS -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"patient_id":"9f1c2a4e-...","name":"Ibuprofen","dosage":"600 mg 3x/Tag","start_date":"2026-11-01T00:00:00Z"}' \
  $BASE/medications
```

---

## 8. Versionierung und Kompatibilität

- Die API ist unter `/api/` ohne Versions-Suffix.
- **Bis v1.0:** breaking changes können in Minor-Releases auftreten; Änderungen werden im Changelog dokumentiert.
- **Ab v1.0:** Semver-Prinzip; breaking changes nur in Major-Releases (`/api/v2/` als Parallelbetrieb).
- Deprecation-Header für zurückgestellte Endpunkte: `Deprecation: true` + `Sunset: <date>`.

---

**Verwandte Dokumente:** [SECURITY.md](SECURITY.md) · [INTERFACES.md](INTERFACES.md) · [DATABASE.md](DATABASE.md)
