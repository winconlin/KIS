# KIS — HTTP-API

Stand: 06.11.2026 · Basis lokal `http://127.0.0.1:8080` · JSON (`application/json`)

Diese Referenz beschreibt nur Routen, die `kis-app` tatsächlich registriert. Die Berechtigungen stammen aus [`access_policy.rs`](../crates/kis-core/src/access_policy.rs); dort nicht aufgeführte `/api/`-Routen erfordern die Admin-Berechtigung.

## Authentifizierung

| Methode | Pfad | Zugriff | Beschreibung |
|---|---|---|---|
| POST | `/api/v1/auth/login` | öffentlich | `{"email": "<benutzername oder e-mail>", "password": "…", "totp_code": "123456"?}` |
| POST | `/api/v1/auth/refresh` | öffentlich | `{"refresh_token": "…"}` — rotiert Access- **und** Refresh-Token |
| POST | `/api/v1/auth/logout` | angemeldet | widerruft die aktuelle Sitzung |
| GET | `/api/v1/auth/sessions` | angemeldet | eigene aktive Sitzungen |
| POST | `/api/v1/auth/sessions/:id/revoke` | angemeldet | eigene Sitzung; Admin: beliebige |

Antwort auf Login/Refresh:

```json
{
  "access_token": "…",
  "refresh_token": "…",
  "tokenType": "Bearer",
  "expiresIn": 900,
  "sessionId": "8f0c…",
  "user": { "id": "1d41…", "role": "arzt" }
}
```

Regeln:

- Anfragen senden `Authorization: Bearer <access_token>`. Refresh-Tokens werden an allen anderen Routen abgelehnt.
- Nach einem Refresh sind das alte Access- und Refresh-Token ungültig. Bei paralleler Verwendung desselben Refresh-Tokens gelingt genau eine Anfrage.
- Sitzungen enden nach `security.refresh_expiry` (Standard 24 h) oder nach 30 Minuten ohne Aktivität.
- Rolle und Kontostatus werden bei jeder Anfrage aus der Datenbank gelesen; Änderungen wirken sofort.
- Nach `security.max_login_attempts` Fehlversuchen ist das Konto für `security.lockout_duration` Sekunden gesperrt. Falsches Passwort, unbekanntes Konto und Sperre ergeben dieselbe Antwort (`401 Ungültige Anmeldedaten`).
- Rate-Limit je Client-IP: `/api/v1/auth/*` 10/min, sonst 100/min (konfigurierbar). Überschreitung: `429` mit `Retry-After`.

## Fachmodule

| Pfad | GET | POST | PUT | DELETE |
|---|---|---|---|---|
| `/api/v1/patients` | `patient.read` | `patient.create` | — | — |
| `/api/v1/patients/:id` | `patient.read` | — | `patient.write` | `patient.delete` (Soft-Delete) |
| `/api/v1/patients/:id/history` | `audit.read` | — | — | — |
| `/api/v1/doctors`, `/api/v1/doctors/:id`, `…/:id/schedule` | `doctor.read` | `doctor.write` | `doctor.write` | `doctor.write` |
| `/api/v1/appointments`, `…/slots`, `…/:doctor_id/calendar`, `…/:id` | `appointment.read` | `appointment.book` (anlegen) | `appointment.write` | `appointment.cancel` |
| `/api/v1/appointments/:id/book` | — | `appointment.book` | — | — |
| `/api/v1/appointments/:id/cancel` | — | `appointment.cancel` | — | — |
| `/api/v1/medications`, `…/:id`, `…/patient/:patient_id` | `medication.read` | `medication.write` | `medication.write` | `medication.write` |

- Updates verlangen die aktuelle `version` (Optimistic Locking); veraltete Version → `409`.
- Überlappender Termin desselben Arztes → `409` (auch bei gleichzeitigen Anfragen).
- `GET /api/v1/patients?active=false` liefert gelöschte Datensätze nur Rollen mit `patient.delete`.
- Felder in camelCase, z. B. `nameFamily`, `birthDate`, `allergies` (Array).

## DSGVO

| Methode | Pfad | Zugriff | Beschreibung |
|---|---|---|---|
| GET | `/api/v1/patients/:id/export?format=json\|csv\|fhir` | `data.export` | Datenauskunft/-übertragbarkeit (Art. 15/20) |
| POST | `/api/v1/patients/:id/hard-delete` | Admin | `{"confirmation": "<Familienname>"}`, unwiderruflich |
| POST | `/api/v1/admin/deletion/check` | Admin | Patienten mit abgelaufener Aufbewahrungsfrist |
| POST | `/api/v1/admin/anonymization/patients/:id` | Admin | `{"mode": "anonymize"}` (irreversibel) oder `{"mode": "pseudonymize"}` |
| POST | `/api/v1/admin/anonymization/check` | Admin | fristbasierte Anonymisierung soft-gelöschter Patienten |
| GET | `/api/v1/admin/anonymization/log?limit=N` | Admin | Nachweise |

Export- und Löschvorgänge werden protokolliert, ohne Inhalte im Audit-Trail abzulegen (Export: SHA-256 der ausgegebenen Daten).

## Betrieb und Audit

| Methode | Pfad | Zugriff |
|---|---|---|
| GET | `/health`, `/api/v1/health` | öffentlich, Antwort `ok` |
| GET | `/api/v1/status` | angemeldet |
| GET | `/api/v1/health/db`, `/replica`, `/cache` | `config.read` |
| GET | `/api/v1/audit/:entity_type/:entity_id` | `audit.read`; der Zugriff selbst wird protokolliert |

## Rollen

| Rolle | Berechtigungen |
|---|---|
| admin | alle |
| arzt | patient.read/write/create, doctor.read, appointment.read/write/book/cancel, medication.read/write, document.read |
| pflege | patient.read, appointment.read, medication.read |
| rezeption | patient.read/create, appointment.book/cancel |
| viewer | patient.read, appointment.read |
| auditor | audit.read |
| llm_agent | patient.read, appointment.read, medication.read, document.read |

## Fehler

```json
{ "success": false, "error": "Keine Berechtigung für diese Aktion", "status": 403 }
```

`401` fehlende/ungültige Anmeldung, `403` fehlende Berechtigung, `404`, `409` Konflikt, `429` Rate-Limit, `500` mit generischer Meldung (Details nur im Server-Log).

Alle Antworten tragen `X-Content-Type-Options`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer` und eine strikte `Content-Security-Policy`; API-Antworten zusätzlich `Cache-Control: no-store`.

## Nicht registriert

FHIR-, HL7-, DICOM-, XDS-, GDT-, MEDICO- und TI-Endpunkte sind nicht erreichbar. Ihre Beschreibungen in [INTERFACES.md](INTERFACES.md) und [CGM_MEDICO.md](CGM_MEDICO.md) sind Zielbild.
