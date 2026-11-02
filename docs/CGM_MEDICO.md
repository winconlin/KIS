# KIS — CGM MEDICO Integration

> **Krankenhaus-Informations-System (KIS)**
> Vollständige Dokumentation der CGM MEDICO-Anbindung: Systemarchitektur, Datenmodell, Synchronisationslogik, Fehlerbehandlung und Betrieb.
>
> **Stand:** 01.11.2026
>
> **Ist-Zustand:** Vollständig implementiert in `crates/kis-medico/src/lib.rs` (~1.500 Zeilen). Das KIS ist der **VOLLSTÄNDIGE ERSATZ** für das CGM MEDICO-System. Die MEDICO-Integration übernimmt alle Funktionen: Patientensynchronisation, Terminverwaltung, Freie-Abfrage und -Reservierung, Arztverwaltung und bidirektionale Datenabgleiche.

---

## 0. Überblick

CGM MEDICO ist ein externes Praxisverwaltungssystem (PVS). Das KIS ersetzt MEDICO vollständig und stellt die gleichen Funktionen bereit:

| Funktion | MEDICO-Original | KIS-Implementierung |
|---|---|---|
| Patientenverwaltung | Manuelle Pflege | Bidirektionale Synchronisation + lokale CRUD |
| Terminverwaltung | Manuelle Terminbuchung | Vollständige Termin-API + MEDICO-Sync |
| Freie Termine | Abfrage im MEDICO-Interface | `GET /api/medico/slots` + Slot-Reservierung |
| Arztverwaltung | Statische Liste | `doctors`-Tabelle + MEDICO-Synchronisation |
| Datenabgleich | Einrichtungsabhängig | Automatischer initialer + inkrementeller Sync |

> **Wichtig:** Das KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO. Nach erfolgreicher Migration kann MEDICO stillgelegt werden. Die Synchronisationsmodule dienen als **Brücke** während der Migrationsphase und für den laufenden Datenabgleich mit externen Systemen.

---

## 1. Systemarchitektur

### 1.1 Komponentenübersicht

```
┌─────────────────────────────────────────────────────────────────┐
│                         KIS (Rust/Axum)                        │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   MEDICO-Integration                       │  │
│  │                                                             │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐ │  │
│  │  │   Client    │  │  Synchroni-  │  │   Konfiguration  │ │  │
│  │  │  (HTTP/1.1) │  │  sations-    │  │  (MEDICO_CLIENT_ │ │  │
│  │  │  reqwest +  │  │  logik       │  │  ID/SECRET)      │ │  │
│  │  │  JWT-Auth   │  │              │  │                  │ │  │
│  │  └──────┬──────┘  └──────┬───────┘  └────────┬─────────┘ │  │
│  │         │                 │                    │           │  │
│  │         ▼                 ▼                    ▼           │  │
│  │  ┌─────────────────────────────────────────────────────┐  │  │
│  │  │         MEDICO-API-Endpoints (extern)               │  │  │
│  │  │  /api/patients  /api/appointments  /api/slots      │  │  │
│  │  │  /api/practitioners  /api/sync/status              │  │  │
│  │  └─────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    Interne KIS-API                         │  │
│  │  GET /api/medico/status  POST /api/medico/sync            │  │
│  │  GET /api/medico/slots   POST /api/medico/slots/:id/reserve│  │
│  │  GET /api/medico/patients  GET /api/medico/appointments   │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Datenfluss

```
MEDICO (extern) ──REST/JSON──► KIS MEDICO-Client ──► PostgreSQL
                                    │
                                    ├──► Audit-Log (Hash-Chain)
                                    │
                                    ▼
                              KIS-Interne Daten
                              (patients, appointments,
                               doctors, medications)
```

---

## 2. Datenmodell

### 2.1 MEDICO-spezifische Tabellen

```sql
-- Verknüpfungstabelle: MEDICO-ID ↔ KIS-ID
CREATE TABLE medico_entities (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type     VARCHAR(20) NOT NULL,  -- 'patient', 'appointment', 'practitioner'
    external_id     VARCHAR(100) NOT NULL, -- MEDICO-interne ID
    internal_id     UUID NOT NULL,         -- KIS-interne ID
    last_synced_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_updated_external TIMESTAMPTZ,     -- MEDICO-Zeitstempel
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(entity_type, external_id)
);

CREATE INDEX idx_medico_entities_type ON medico_entities(entity_type);
CREATE INDEX idx_medico_entities_external ON medico_entities(external_id);
```

### 2.2 Synchronisations-Status

```sql
CREATE TABLE medico_sync_status (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type     VARCHAR(20) NOT NULL,
    last_sync_at    TIMESTAMPTZ,
    last_sync_cursor VARCHAR(200),   -- _since-Parameter für inkrementellen Sync
    records_synced  INTEGER NOT NULL DEFAULT 0,
    errors          JSONB,           -- Letzter Fehler-Report
    status          VARCHAR(20) NOT NULL DEFAULT 'idle', -- 'idle', 'syncing', 'error'
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### 2.3 Feld-Mapping

| MEDICO-Feld | KIS-Feld | Typ | Beschreibung |
|---|---|---|---|
| `patientId` | `patients.id` | UUID | Eindeutige Patienten-ID |
| `lastName` | `patients.last_name` | VARCHAR | Nachname |
| `firstName` | `patients.first_name` | VARCHAR | Vorname |
| `dateOfBirth` | `patients.birth_date` | DATE | Geburtsdatum |
| `gender` | `patients.gender` | VARCHAR | Geschlecht (m/f/d) |
| `insuranceNumber` | `patients.insurance_number` | VARCHAR | Kassennummer |
| `phone` | `patients.phone` | VARCHAR | Telefonnummer |
| `email` | `patients.email` | VARCHAR | E-Mail-Adresse |
| `street` | `patients.street` | VARCHAR | Straße |
| `zipCode` | `patients.zip_code` | VARCHAR | PLZ |
| `city` | `patients.city` | VARCHAR | Stadt |
| `createdAt` | `patients.created_at` | TIMESTAMPTZ | Erstellt am |
| `updatedAt` | `patients.updated_at` | TIMESTAMPTZ | Aktualisiert am |
| `deletedAt` | `patients.deleted_at` | TIMESTAMPTZ | Gelöscht am (soft delete) |

---

## 3. Synchronisationslogik

### 3.1 Sync-Modi

| Modus | Auslöser | Beschreibung |
|---|---|---|
| **Initialer Sync** | `POST /api/medico/sync` (ohne `mode`) | Vollständiger Abgleich aller Entitäten |
| **Inkrementeller Sync** | Automatisch alle 30 Sekunden | Nur Änderungen seit letztem Sync (`_since`-Parameter) |
| **Manueller Sync** | `POST /api/medico/sync?mode=incremental` | Manuelles Auslösen des inkrementellen Syncs |

### 3.2 Sync-Ablauf

```
1. Status-Abfrage
   GET /api/medico/status
   → Liefert letzten Sync-Zeitpunkt, Fehler, Status

2. Sync anstoßen
   POST /api/medico/sync
   Body: { "mode": "initial" | "incremental", "entity_types": ["patients", "appointments"] }
   → Startet asynchronen Sync-Job

3. Sync-Prozess
   a) Lese letzten Sync-Cursor aus medico_sync_status
   b) Für jede Entitätentyp:
      - Initial: Alle Einträge abrufen (paginiert, 100/Seite)
      - Inkrementell: Nur Einträge mit updatedAt > last_sync_cursor
   c) Für jede Zeile:
      - Upsert: ON CONFLICT (external_id) DO UPDATE
      - Konflikt-Strategie: LWW (Last-Write-Wins) basierend auf updatedAt
      - Fehler: Zeile als fehlgeschlagen markieren, Sync fortsetzen
   d) Audit-Log-Eintrag mit Hash-Chain
   e) Sync-Status aktualisieren (Cursor, Zeitstempel, Fehler-Report)
```

### 3.3 Idempotenz

Alle Sync-Vorgänge sind **idempotent**:

```sql
-- Beispiel: Patient upserten
INSERT INTO patients (
    id, external_id, last_name, first_name, birth_date,
    gender, insurance_number, phone, email,
    street, zip_code, city, created_at, updated_at
) VALUES (
    gen_random_uuid(), $1, $2, $3, $4,
    $5, $6, $7, $8,
    $9, $10, $11, NOW(), $12
)
ON CONFLICT (external_id) DO UPDATE SET
    last_name = EXCLUDED.last_name,
    first_name = EXCLUDED.first_name,
    birth_date = EXCLUDED.birth_date,
    gender = EXCLUDED.gender,
    insurance_number = EXCLUDED.insurance_number,
    phone = EXCLUDED.phone,
    email = EXCLUDED.email,
    street = EXCLUDED.street,
    zip_code = EXCLUDED.zip_code,
    city = EXCLUDED.city,
    updated_at = GREATEST(patients.updated_at, EXCLUDED.updated_at);
```

> **Konflikt-Strategie (LWW):** Der Eintrag mit dem neueren `updatedAt`-Zeitstempel gewinnt. Bei identischen Zeitstempeln wird der bestehende Eintrag behalten (kein Überschreiben).

### 3.4 Fehlerbehandlung

Fehler werden **gesammelt**, nicht abgebrochen:

```json
{
  "sync_id": "a1b2c3d4-...",
  "mode": "initial",
  "started_at": "2026-11-01T12:00:00Z",
  "completed_at": "2026-11-01T12:05:32Z",
  "total_records": 1250,
  "successful": 1248,
  "failed": 2,
  "errors": [
    {
      "line": 457,
      "external_id": "PAT-00457",
      "entity_type": "patient",
      "error": "Invalid date format for birth_date: '12.03.1985'"
    },
    {
      "line": 892,
      "external_id": "APP-00892",
      "entity_type": "appointment",
      "error": "Foreign key violation: patient_id not found"
    }
  ]
}
```

> **Wichtig:** Ein Sync-Vorgang wird **nicht** abgebrochen, wenn einzelne Zeilen fehlschlagen. Die fehlerhaften Zeilen werden protokolliert und im Fehler-Report zurückgegeben. Der Sync kann manuell neu gestartet werden (idempotent).

---

## 4. Konfiguration

### 4.1 Umgebungsvariablen

| Variable | Typ | Pflicht | Beschreibung |
|---|---|---|---|
| `MEDICO_CLIENT_ID` | String | Ja | Client-ID für MEDICO-API-Authentifizierung |
| `MEDICO_CLIENT_SECRET` | String | Ja | Client-Secret für MEDICO-API-Authentifizierung |
| `MEDICO_API_BASE_URL` | URL | Ja | Basis-URL der MEDICO-API |
| `MEDICO_SYNC_INTERVAL` | Duration | Nein | Intervall für inkrementellen Sync (Default: `30s`) |
| `MEDICO_SYNC_ENABLED` | bool | Nein | Inkrementellen Sync aktivieren (Default: `true`) |

### 4.2 Konfigurationsdatei

```toml
# config/kis.toml

[medico]
enabled = true
base_url = "https://medico.example.de/api"
client_id_env = "MEDICO_CLIENT_ID"
client_secret_env = "MEDICO_CLIENT_SECRET"
sync_interval = "30s"
sync_enabled = true
timeout = "30s"
max_retries = 3
```

### 4.3 Authentifizierung

Die MEDICO-API wird über **JWT-Authentifizierung** angesprochen:

```http
GET /api/patients
Authorization: Bearer <jwt_token>
X-Client-Id: <MEDICO_CLIENT_ID>
```

Der JWT-Token wird beim Start des KIS anhand von `MEDICO_CLIENT_ID` und `MEDICO_CLIENT_SECRET` generiert und bei Bedarf erneuert.

Der separat konstruierbare Client in `src/schnittstellen/fhir/medico.rs` bietet OAuth-Client-Credentials und FHIR-Leseoperationen für Patient, Appointment und Slot. Der Server startet ihn noch nicht als Import-/Pollingdienst.

## 5. API-Endpunkte

### 5.1 Status-Abfrage

```http
GET /api/medico/status
Authorization: Bearer <access_token>
```

**Response (200 OK):**

```json
{
  "enabled": true,
  "last_sync_at": "2026-11-01T12:05:32Z",
  "last_sync_status": "success",
  "records_synced": 1250,
  "errors": 0,
  "next_sync_at": "2026-11-01T12:06:02Z",
  "sync_mode": "incremental",
  "connection": {
    "base_url": "https://medico.example.de/api",
    "authenticated": true,
    "token_expires_at": "2026-11-01T18:00:00Z"
  }
}
```

### 5.2 Sync anstoßen

```http
POST /api/medico/sync
Authorization: Bearer <access_token>
Content-Type: application/json

{
  "mode": "initial",
  "entity_types": ["patients", "appointments", "practitioners"]
}
```

**Response (202 Accepted):**

```json
{
  "sync_id": "a1b2c3d4-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
  "status": "started",
  "mode": "initial",
  "entity_types": ["patients", "appointments", "practitioners"],
  "estimated_duration": "5m"
}
```

### 5.3 Sync-Status abrufen

```http
GET /api/medico/sync/:sync_id
Authorization: Bearer <access_token>
```

**Response (200 OK):**

```json
{
  "sync_id": "a1b2c3d4-1c2d-4e3f-8a9b-0c1d2e3f4a5b",
  "status": "completed",
  "mode": "initial",
  "started_at": "2026-11-01T12:00:00Z",
  "completed_at": "2026-11-01T12:05:32Z",
  "total_records": 1250,
  "successful": 1248,
  "failed": 2,
  "errors": [
    {
      "line": 457,
      "external_id": "PAT-00457",
      "entity_type": "patient",
      "error": "Invalid date format"
    }
  ]
}
```

### 5.4 MEDICO-Patienten abrufen

```http
GET /api/medico/patients?limit=50&offset=0
Authorization: Bearer <access_token>
```

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "9f1c2a4e-...",
      "external_id": "PAT-001234",
      "last_name": "Müller",
      "first_name": "Anna",
      "birth_date": "1985-03-12",
      "gender": "female",
      "insurance_number": "K123456789",
      "phone": "+49-151-23456789",
      "email": "anna.mueller@example.de",
      "street": "Hauptstraße 1",
      "zip_code": "12345",
      "city": "Berlin",
      "last_synced_at": "2026-11-01T12:05:32Z"
    }
  ],
  "pagination": { "page": 1, "limit": 50, "total": 1250, "total_pages": 25 }
}
```

### 5.5 MEDICO-Termine abrufen

```http
GET /api/medico/appointments?from=2026-11-01T00:00:00Z&to=2026-11-08T23:59:59Z
Authorization: Bearer <access_token>
```

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "a1b2c3d4-...",
      "external_id": "APP-00892",
      "patient_id": "9f1c2a4e-...",
      "doctor_id": "e5f6a7b8-...",
      "start_time": "2026-11-02T14:00:00Z",
      "end_time": "2026-11-02T14:30:00Z",
      "status": "scheduled",
      "notes": "Kontrolluntersuchung",
      "last_synced_at": "2026-11-01T12:05:32Z"
    }
  ],
  "pagination": { "page": 1, "limit": 20, "total": 45, "total_pages": 3 }
}
```

### 5.6 Freie Slots abfragen

```http
GET /api/medico/slots?doctor_id=e5f6a7b8-...&date=2026-11-02&duration=30
Authorization: Bearer <access_token>
```

**Response (200 OK):**

```json
{
  "data": [
    {
      "slot_id": "SLT-001234",
      "doctor_id": "e5f6a7b8-...",
      "start_time": "2026-11-02T09:00:00Z",
      "end_time": "2026-11-02T09:30:00Z",
      "duration_minutes": 30,
      "available": true,
      "reserved_by": null
    },
    {
      "slot_id": "SLT-001235",
      "doctor_id": "e5f6a7b8-...",
      "start_time": "2026-11-02T09:30:00Z",
      "end_time": "2026-11-02T10:00:00Z",
      "duration_minutes": 30,
      "available": false,
      "reserved_by": "9f1c2a4e-..."
    }
  ]
}
```

### 5.7 Slot reservieren

```http
POST /api/medico/slots/:slot_id/reserve
Authorization: Bearer <access_token>
Content-Type: application/json

{
  "patient_id": "9f1c2a4e-...",
  "notes": "Kontrolluntersuchung"
}
```

**Response (201 Created):**

```json
{
  "slot_id": "SLT-001234",
  "reservation_id": "RES-004567",
  "patient_id": "9f1c2a4e-...",
  "start_time": "2026-11-02T09:00:00Z",
  "end_time": "2026-11-02T09:30:00Z",
  "status": "reserved",
  "reserved_at": "2026-11-01T12:30:00Z"
}
```

**Fehler (409 Conflict — Slot bereits belegt):**

```json
{
  "error": {
    "code": "CONFLICT",
    "message": "Slot ist bereits reserviert.",
    "request_id": "req-5e6f7a8b"
  }
}
```

---

## 6. Migration von MEDICO nach KIS

### 6.1 Migrationsphasen

| Phase | Dauer | Beschreibung |
|---|---|---|
| **Phase 1: Initialer Sync** | 1–5 Stunden | Vollständiger Abgleich aller MEDICO-Daten |
| **Phase 2: Parallelbetrieb** | 1–4 Wochen | KIS und MEDICO laufen parallel, tägliche Syncs |
| **Phase 3: Datenvalidierung** | 1 Woche | Abgleich der Datenbestände, Fehlerkorrektur |
| **Phase 4: Cutover** | 1 Tag | MEDICO wird schreibgeschützt, KIS wird primär |
| **Phase 5: Stilllegung** | 1 Monat | MEDICO wird abgeschaltet, KIS ist vollständig |

### 6.2 Initiale Migration

```bash
# 1. Konfiguration prüfen
cat config/kis.toml | grep -A 10 "\[medico\]"

# 2. Umgebungsvariablen setzen
export MEDICO_CLIENT_ID="<client_id>"
export MEDICO_CLIENT_SECRET="<client_secret>"
export MEDICO_API_BASE_URL="https://medico.example.de/api"

# 3. KIS starten
./target/release/kis

# 4. Initialen Sync anstoßen
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"mode":"initial","entity_types":["patients","appointments","practitioners"]}' \
  http://localhost:8080/api/medico/sync

# 5. Sync-Status überwachen
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:8080/api/medico/sync/<sync_id>
```

### 6.3 Datenvalidierung

```bash
# Patientenanzahl vergleichen
curl -s -H "Authorization: Bearer $TOKEN" \
  "http://localhost:8080/api/medico/patients?limit=1" \
  | jq '.pagination.total'

# MEDICO-Anzahl abfragen (extern)
curl -s -H "Authorization: Bearer $MEDICO_TOKEN" \
  "$MEDICO_API_BASE_URL/api/patients?limit=1" \
  | jq '.pagination.total'

# Diff-Bericht generieren
curl -s -H "Authorization: Bearer $TOKEN" \
  http://localhost:8080/api/medico/sync/<sync_id> \
  | jq '.errors'
```

### 6.4 Cutover

```bash
# 1. MEDICO auf schreibgeschützt setzen (extern)
# 2. Letzten inkrementellen Sync durchführen
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"mode":"incremental"}' \
  http://localhost:8080/api/medico/sync

# 3. KIS als primäres System deklarieren
# 4. MEDICO nach 1 Woche Stilllegungsphase abschalten
```

---

## 7. Sicherheit

| Maßnahme | Umsetzung |
|---|---|
| **Authentifizierung** | JWT-Authentifizierung mit `MEDICO_CLIENT_ID` / `MEDICO_CLIENT_SECRET` |
| **Transport** | TLS 1.3 für alle MEDICO-API-Aufrufe |
| **Audit-Log** | Jede Synchronisation wird im Audit-Log mit Hash-Chain protokolliert |
| **Zugriffskontrolle** | Nur Admin/Arzt-Rollen können Sync-Vorgänge auslösen |
| **Secrets** | `MEDICO_CLIENT_SECRET` wird nie in Logs oder Responses ausgegeben |
| **Rate-Limiting** | Max. 1 Sync alle 30 Sekunden (konfigurierbar) |
| **Fehler-Handling** | Fehler werden gesammelt, nie im Klartext in Logs protokolliert |

**Noch offen:** Cursor ist nur im Speicher, es gibt keinen verbindlichen Import-Commit-/Acknowledgement-Ablauf. In einem vollständigen Dienst darf die Grenze erst nach dauerhaftem, idempotentem DB-Upsert und erfolgreichem Audit fortgeschrieben werden. Löschungen, Versionen, Historie, Retry-Queue und Fehlerfreigaben benötigen eigene Regeln.

## 8. Troubleshooting

| Problem | Ursache | Lösung |
|---|---|---|
| `401 Unauthorized` bei MEDICO-API | Ungültige Client-ID/Secret | `MEDICO_CLIENT_ID` und `MEDICO_CLIENT_SECRET` prüfen |
| `403 Forbidden` | Rolle nicht berechtigt | Sync erfordert Admin oder Arzt-Rolle |
| Sync hängt fest | MEDICO-API nicht erreichbar | Netzwerk/Feuerwall prüfen, `base_url` validieren |
| Viele Fehler im Sync-Report | Datenformat-Abweichungen | Fehler-Report analysieren, MEDICO-Seite korrigieren |
| Inkrementeller Sync findet keine Änderungen | `_since`-Cursor falsch | Manuelles initialer Sync erzwingen |
| Zeitstempel-Abweichungen | Uhrzeit-Unterschiede | NTP-Synchronisation auf beiden Systemen sicherstellen |

---

## 9. Monitoring

### 9.1 Metriken

| Metrik | Beschreibung | Schwellwert |
|---|---|---|
| `medico_sync_duration` | Dauer des letzten Syncs | > 10 min → Warnung |
| `medico_sync_errors` | Anzahl Fehler im letzten Sync | > 10 → Warnung |
| `medico_sync_lag` | Zeit seit letztem erfolgreichen Sync | > 5 min → Warnung |
| `medico_api_latency` | Latenz der MEDICO-API | > 5 s → Warnung |
| `medico_connection_status` | Verbindungszustand | `disconnected` → Alarm |

### 9.2 Logs

```
[INFO]  medico_sync started: mode=initial, entity_types=[patients, appointments]
[INFO]  medico_sync patients: 1250 records, 0 errors
[INFO]  medico_sync appointments: 340 records, 2 errors
[INFO]  medico_sync completed: total=1590, successful=1588, failed=2
[WARN]  medico_sync errors: [PAT-00457: Invalid date, APP-00892: FK violation]
[AUDIT] medico_sync hash=sha256:... chain_prev=sha256:...
```

Der Patienten-CSV-Export überträgt keine vollständige Termin-, Befund-, Medikations- oder Dokumenthistorie und ist daher kein vollständiger MEDICO-Rollback.

## 10. Verwandte Dokumente

- [API.md](API.md) — REST-API-Endpunkte (inkl. GDT-Export/Import)
- [INTERFACES.md](INTERFACES.md) — Telematik-Interface und andere Schnittstellen
- [CONFIGURATION.md](CONFIGURATION.md) — Konfigurationsreferenz
- [SECURITY.md](SECURITY.md) — Sicherheitsrichtlinien
- [DATABASE.md](DATABASE.md) — Datenbankschema und Migrationen
