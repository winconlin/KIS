# KIS — API

Diese Referenz beschreibt, was der Server tatsächlich registriert. Die
Routentabelle unten wird aus dem laufenden System erzeugt
(`GET /api/v1/routes`); ein Test stellt sicher, dass sie stimmt.

## Grundregeln

- **Anmeldung:** `POST /api/v1/auth/login` mit `{"email": "<Benutzername
  oder E-Mail>", "password": "…", "totp": "…"}` liefert `access_token`
  (15 Minuten) und `refresh_token`. Jede weitere Anfrage trägt
  `Authorization: Bearer <access_token>`. `POST /api/v1/auth/refresh`
  tauscht das Refresh-Token gegen ein neues Paar (das alte ist danach
  ungültig).
- **Rechte:** Jede Route verlangt ein Recht (Spalte „Recht“). Welche
  Rechte ein Benutzer hat, bestimmen seine Rollen; Rollen sind Daten des
  Hauses (`/api/v1/admin/roles`). Unbekannte Pfade unter `/api/` sind nur
  für die Rolle `admin` erreichbar.
- **Behandlungszusammenhang:** Routen mit `:case_id` verlangen zusätzlich
  Zugriff auf den Fall, Routen mit `:unit_id` die Zuordnung zur Einheit.
  Fehlt er: 403 mit `code` `no_treatment_context` bzw.
  `not_member_of_unit`. Notfallzugriff: `POST /api/v1/emergency-access`
  mit `{"caseId": "…", "reason": "…"}` (mindestens 10 Zeichen, wird
  auditiert und nachträglich geprüft).
- **Felder** heißen in Anfragen und Antworten `camelCase`. Zeitpunkte sind
  ISO 8601 in UTC, Kalendertage `JJJJ-MM-TT`.
- **Listen** der neuen Module antworten mit `{"count": n, "items": […]}`.
- **Gleichzeitige Änderungen:** Datensätze tragen `version`. Wer ändert,
  sendet die gelesene Version mit; ist sie veraltet, antwortet der Server
  mit 409.
- **Fehler** haben immer die Form
  `{"success": false, "error": "<Text>", "status": <Code>}`.

| Status | Bedeutung |
|---|---|
| 400 | Eingabe ungültig (auch: Plausibilitätsregel der Datenbank verletzt) |
| 401 | nicht angemeldet, Token ungültig oder abgelaufen |
| 403 | Recht fehlt, kein Behandlungszusammenhang oder keine Zuordnung zur Einheit |
| 404 | Datensatz nicht vorhanden |
| 409 | Konflikt: veraltete Version, Überschneidung, unzulässiger Zustand |
| 429 | zu viele Anfragen (`Retry-After` beachten) |
| 500 | interner Fehler; Einzelheiten nur im Server-Log |

## Routen

<!-- ROUTEN-ANFANG (erzeugt, nicht von Hand ändern) -->
### Kern (Anmeldung, Verwaltung, Datenschutz) (`kern`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| POST | `/api/v1/admin/anonymization/check` | admin |  |
| GET | `/api/v1/admin/anonymization/log` | admin |  |
| POST | `/api/v1/admin/anonymization/patients/:id` | admin |  |
| POST | `/api/v1/admin/deletion/check` | admin |  |
| GET | `/api/v1/admin/emergency-access` | audit.read |  |
| POST | `/api/v1/admin/emergency-access/:id/review` | audit.read |  |
| GET | `/api/v1/admin/event-failures` | admin |  |
| POST | `/api/v1/admin/event-failures/:id/resolve` | admin |  |
| GET | `/api/v1/admin/permissions` | admin |  |
| GET | `/api/v1/admin/roles` | admin |  |
| DELETE | `/api/v1/admin/roles/:key` | admin |  |
| PUT | `/api/v1/admin/roles/:key` | admin |  |
| GET | `/api/v1/admin/settings` | admin |  |
| PUT | `/api/v1/admin/settings/:key` | admin |  |
| GET | `/api/v1/admin/users` | admin |  |
| POST | `/api/v1/admin/users` | admin |  |
| PUT | `/api/v1/admin/users/:id` | admin |  |
| POST | `/api/v1/admin/users/:id/password` | admin |  |
| PUT | `/api/v1/admin/users/:id/roles` | admin |  |
| PUT | `/api/v1/admin/users/:id/units` | admin |  |
| GET | `/api/v1/audit/:entity_type/:entity_id` | audit.read |  |
| POST | `/api/v1/auth/login` | öffentlich |  |
| POST | `/api/v1/auth/logout` | angemeldet |  |
| POST | `/api/v1/auth/refresh` | öffentlich |  |
| GET | `/api/v1/auth/sessions` | angemeldet |  |
| POST | `/api/v1/auth/sessions/:id/revoke` | angemeldet |  |
| POST | `/api/v1/emergency-access` | access.break_glass |  |
| GET | `/api/v1/health` | öffentlich |  |
| GET | `/api/v1/health/db` | config.read |  |
| GET | `/api/v1/interfaces/events` | interface.consume |  |
| POST | `/api/v1/interfaces/events/ack` | interface.consume |  |
| GET | `/api/v1/me` | angemeldet |  |
| GET | `/api/v1/modules` | angemeldet |  |
| GET | `/api/v1/patients/:id/export` | data.export |  |
| POST | `/api/v1/patients/:id/hard-delete` | admin |  |
| GET | `/api/v1/routes` | angemeldet |  |
| GET | `/api/v1/status` | angemeldet |  |
| GET | `/health` | öffentlich |  |

### Organisation (`org`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/me/units` | angemeldet |  |
| POST | `/api/v1/org/beds` | org.write |  |
| PUT | `/api/v1/org/beds/:id` | org.write |  |
| POST | `/api/v1/org/rooms` | org.write |  |
| GET | `/api/v1/org/units` | org.read |  |
| POST | `/api/v1/org/units` | org.write |  |
| PUT | `/api/v1/org/units/:id` | org.write |  |
| GET | `/api/v1/org/wards/:ward_id/rooms` | org.read |  |

### Patientenstamm (`patients`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/patients` | patient.read |  |
| POST | `/api/v1/patients` | patient.create |  |
| DELETE | `/api/v1/patients/:id` | patient.delete |  |
| GET | `/api/v1/patients/:id` | patient.read |  |
| PUT | `/api/v1/patients/:id` | patient.write |  |
| GET | `/api/v1/patients/:id/alerts` | patient.read |  |
| POST | `/api/v1/patients/:id/alerts` | patient.write oder clinical.write oder nursing.write |  |
| POST | `/api/v1/patients/:id/alerts/:alert_id/end` | patient.write oder clinical.write oder nursing.write |  |
| GET | `/api/v1/patients/:id/history` | audit.read |  |

### Fälle und Belegung (`cases`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases` | case.read |  |
| POST | `/api/v1/cases` | case.admit |  |
| GET | `/api/v1/cases/:case_id` | case.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/cancel` | case.admit | Behandlungszusammenhang |
| PUT | `/api/v1/cases/:case_id/clinical-status` | clinical.write oder nursing.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/discharge` | case.admit | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/stays` | case.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/transfer` | case.transfer | Behandlungszusammenhang |
| GET | `/api/v1/patients/:id/cases` | case.read |  |
| GET | `/api/v1/units/:unit_id/beds` | case.read | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/census` | case.read | Zuordnung zur Einheit |

### Klinische Akte (`clinical`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases/:case_id/diagnoses` | clinical.read oder nursing.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/diagnoses` | clinical.write oder coding.write | Behandlungszusammenhang |
| DELETE | `/api/v1/cases/:case_id/diagnoses/:id` | clinical.write oder coding.write | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/notes` | clinical.read oder nursing.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/notes` | clinical.write oder nursing.write | Behandlungszusammenhang |
| DELETE | `/api/v1/cases/:case_id/notes/:id` | clinical.write oder nursing.write | Behandlungszusammenhang |
| PUT | `/api/v1/cases/:case_id/notes/:id` | clinical.write oder nursing.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/notes/:id/amend` | clinical.write oder nursing.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/notes/:id/sign` | clinical.write oder nursing.write | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/procedures` | clinical.read oder nursing.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/procedures` | clinical.write oder coding.write | Behandlungszusammenhang |
| DELETE | `/api/v1/cases/:case_id/procedures/:id` | clinical.write oder coding.write | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/tasks` | clinical.read oder nursing.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/tasks` | task.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/tasks/:id/done` | task.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/tasks/:id/reopen` | task.write | Behandlungszusammenhang |
| GET | `/api/v1/units/:unit_id/handover` | clinical.read oder nursing.read | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/tasks` | clinical.read oder nursing.read | Zuordnung zur Einheit |

### Auftragswesen (`orders`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases/:case_id/orders` | order.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/orders` | order.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/orders/:id/cancel` | order.write | Behandlungszusammenhang |
| GET | `/api/v1/catalog/automation-rules` | catalog.write |  |
| POST | `/api/v1/catalog/automation-rules` | catalog.write |  |
| DELETE | `/api/v1/catalog/automation-rules/:id` | catalog.write |  |
| GET | `/api/v1/catalog/services` | angemeldet |  |
| POST | `/api/v1/catalog/services` | catalog.write |  |
| PUT | `/api/v1/catalog/services/:id` | catalog.write |  |
| POST | `/api/v1/units/:unit_id/requests` | order.write | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/worklist` | order.process | Zuordnung zur Einheit |
| POST | `/api/v1/units/:unit_id/worklist/:id/accept` | order.process | Zuordnung zur Einheit |
| POST | `/api/v1/units/:unit_id/worklist/:id/complete` | order.process | Zuordnung zur Einheit |
| POST | `/api/v1/units/:unit_id/worklist/:id/reject` | order.process | Zuordnung zur Einheit |

### Labor (`lab`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases/:case_id/lab/cumulative` | lab.read oder clinical.read | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/lab/specimens` | lab.read oder clinical.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/lab/specimens` | lab.write | Behandlungszusammenhang |
| GET | `/api/v1/units/:unit_id/lab/critical` | lab.read oder clinical.read | Zuordnung zur Einheit |

### Pflege (`nursing`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases/:case_id/fluid-balance` | nursing.read oder clinical.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/fluid-balance` | nursing.write oder clinical.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/fluid-balance/:id/void` | nursing.write oder clinical.write | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/vitals` | nursing.read oder clinical.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/vitals` | nursing.write oder clinical.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/vitals/:id/void` | nursing.write oder clinical.write | Behandlungszusammenhang |

### Stationäre Verordnung (`prescriptions`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases/:case_id/administrations` | prescription.read | Behandlungszusammenhang |
| GET | `/api/v1/cases/:case_id/prescriptions` | prescription.read | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/prescriptions` | prescription.write | Behandlungszusammenhang |
| POST | `/api/v1/cases/:case_id/prescriptions/:id/administrations` | medication.administer | Behandlungszusammenhang |
| PUT | `/api/v1/cases/:case_id/prescriptions/:id/status` | prescription.write | Behandlungszusammenhang |

### Verpflegung (`meals`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/cases/:case_id/diet` | nursing.read oder clinical.read | Behandlungszusammenhang |
| PUT | `/api/v1/cases/:case_id/diet` | meal.write | Behandlungszusammenhang |
| GET | `/api/v1/meals/diets` | angemeldet |  |
| POST | `/api/v1/meals/diets` | catalog.write |  |
| PUT | `/api/v1/meals/diets/:id` | catalog.write |  |
| GET | `/api/v1/meals/production` | meal.read |  |

### Dienstplan (`roster`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/roster/absences` | roster.read |  |
| POST | `/api/v1/roster/absences` | roster.write |  |
| DELETE | `/api/v1/roster/absences/:id` | roster.write |  |
| GET | `/api/v1/roster/holidays` | roster.read |  |
| DELETE | `/api/v1/roster/holidays/:date` | roster.write |  |
| PUT | `/api/v1/roster/holidays/:date` | roster.write |  |
| GET | `/api/v1/roster/my-duties.ics` | angemeldet |  |
| GET | `/api/v1/roster/staff` | roster.read |  |
| POST | `/api/v1/roster/staff` | roster.write |  |
| PUT | `/api/v1/roster/staff/:id` | roster.write |  |
| PUT | `/api/v1/units/:unit_id/roster/assignments` | roster.write | Zuordnung zur Einheit |
| DELETE | `/api/v1/units/:unit_id/roster/assignments/:id` | roster.write | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/roster/duty-types` | roster.read | Zuordnung zur Einheit |
| POST | `/api/v1/units/:unit_id/roster/duty-types` | roster.write | Zuordnung zur Einheit |
| PUT | `/api/v1/units/:unit_id/roster/duty-types/:id` | roster.write | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/roster/plan` | roster.read | Zuordnung zur Einheit |
| POST | `/api/v1/units/:unit_id/roster/positions` | roster.write | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/roster/staffing` | roster.read | Zuordnung zur Einheit |
| PUT | `/api/v1/units/:unit_id/roster/staffing` | roster.write | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/roster/timesheet.csv` | roster.read | Zuordnung zur Einheit |
| GET | `/api/v1/units/:unit_id/roster/validate` | roster.read | Zuordnung zur Einheit |

### Ärzte-Stammdaten (`doctors`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/doctors` | doctor.read |  |
| POST | `/api/v1/doctors` | doctor.write |  |
| DELETE | `/api/v1/doctors/:id` | doctor.write |  |
| GET | `/api/v1/doctors/:id` | doctor.read |  |
| PUT | `/api/v1/doctors/:id` | doctor.write |  |
| GET | `/api/v1/doctors/:id/schedule` | doctor.read |  |

### Terminplanung (`appointments`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/appointments` | appointment.read |  |
| POST | `/api/v1/appointments` | appointment.book |  |
| GET | `/api/v1/appointments/:doctor_id/calendar` | appointment.read |  |
| DELETE | `/api/v1/appointments/:id` | appointment.cancel |  |
| GET | `/api/v1/appointments/:id` | appointment.read |  |
| PUT | `/api/v1/appointments/:id` | appointment.write |  |
| POST | `/api/v1/appointments/:id/book` | appointment.book |  |
| POST | `/api/v1/appointments/:id/cancel` | appointment.cancel |  |
| GET | `/api/v1/appointments/slots` | appointment.read |  |

### Dauermedikation (`medications`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/api/v1/medications` | medication.read |  |
| POST | `/api/v1/medications` | medication.write |  |
| DELETE | `/api/v1/medications/:id` | medication.write |  |
| GET | `/api/v1/medications/:id` | medication.read |  |
| PUT | `/api/v1/medications/:id` | medication.write |  |
| GET | `/api/v1/medications/patient/:patient_id` | medication.read |  |

### Weboberfläche (`ui`)

| Methode | Pfad | Recht | Zusätzlich geprüft |
|---|---|---|---|
| GET | `/` | öffentlich |  |
| GET | `/static/*path` | öffentlich |  |

<!-- ROUTEN-ENDE -->

## Eingaben und Antworten im Einzelnen

Die Felder jeder Route stehen als Struktur mit Kommentar direkt am Handler
im Quelltext des Moduls (`crates/kis-<modul>/src/`); die Tests unter
`crates/kis-app/tests/` zeigen vollständige Aufrufe mit Antworten.

## Ereignisse für Schnittstellen

Schnittstellenkonten (Recht `interface.consume`) lesen fachliche
Ereignisse in der Reihenfolge ihres Entstehens:

```text
GET  /api/v1/interfaces/events?consumer=<name>&limit=100
POST /api/v1/interfaces/events/ack   {"consumer": "<name>", "upTo": <id>}
```

Jeder Konsument hat einen eigenen Lesezeiger. Ereignisse enthalten nur
Schlüssel und Codes, keine Personendaten; Themen und Nutzlast stehen in
[MODULE.md](MODULE.md#der-vertrag-des-kerns).
