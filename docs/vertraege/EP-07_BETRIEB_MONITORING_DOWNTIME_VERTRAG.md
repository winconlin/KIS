# M01-Umsetzungsvertrag: `EP-07` — Betrieb, Monitoring (`AP-05`), Schlüsselrotation, Mandanten-Caching, HA/DR, Downtime & Verbund-Lasttest (`AP-06`)

Stand: 25.11.2026 · Autor: `google-neon` (Architektur & Koordination) · Basis: [ROADMAP.md](../ROADMAP.md) (`EP-07a`, `EP-07b`), [THREAT_MODEL_VERBUND.md](../THREAT_MODEL_VERBUND.md) (`T16`, `T24`), [SICHERHEITS_ABNAHME.md](../SICHERHEITS_ABNAHME.md) (`B-01`–`B-07`, `C-01`–`C-04`), [SCALING.md](../SCALING.md) · Status: **Spezifiziert (Umsetzung nach `EP-01c`, `EP-04c`, `EP-05b`)**

---

## 1. Paketabgrenzung (`EP-07a` & `EP-07b`)

| Teilpaket | Exklusiver Schreibbereich | Abhängigkeiten | Pflicht-Nachweise |
|---|---|---|---|
| **`EP-07a`** | `crates/kis-app/src/core_routes.rs` (`/api/v1/metrics`), `crates/kis-app/src/bin/kis-admin.rs` (`reencrypt`), `crates/kis-core/src/security.rs`, `crates/kis-core/src/cache.rs` (neu), `.github/workflows/` (SBOM) | `EP-01c`, `EP-04c`, `EP-05b` | `NVT-16` (Mandanten-Cache-Isolation), `B-07` (keine PII in Metriken/Logs), `C-01`–`C-04` (Container/SBOM), Schlüsselrotations-Integrationstest |
| **`EP-07b`** | `scripts/backup.py` (Verbund-Restore-Drill), `scripts/loadtest.py` (`AP-06`), `docs/SCALING.md`, `docs/OPERATION.md` | `EP-07a` | `B-01`–`B-06` (Backup/Restore/RTO/RPO), Szenario 5 (WAN-Unterbrechung/Ausfallbetrieb) & Szenario 7 (1 000 Betten / mehrere Standorte & MVZ, `EXPLAIN`-Nachweis) |

---

## 2. Verbindliche Technische Verträge

### 2.1 Monitoring ohne PII (`GET /api/v1/metrics`, `AP-05`)
- **Zugriffsregel:** `Access::Permission(Permission::MetricsRead)` oder eigenes Monitoring-Token — **niemals** unauthentifiziert öffentlich (`Scope::TenantOnly` / System-Scope).
- **Erlaubte Labels (Server-Allowlist):** `method`, `route_template` (niemals konkrete `:case_id` oder `:patient_id`!), `status_class` (`2xx`/`4xx`/`5xx`), `legal_entity_code` (nur aggregierte Zähler, kein Personenbezug), `db_pool` (`write`/`read`), `outbox_topic`.
- **Verbotene Inhalte (`B-07`):** Keine Patientennamen, Geburtsdaten, Fallnummern, KVNR, Diagnosen, Freitexte, Tokens oder SQL-Strings in Metriken oder Tracing-Logs.

### 2.2 Mandanten-Präfixzwang für Caches, Berichte & Hintergrundjobs (`T16`, `NVT-16`)
- Jeder Cache-Eintrag und jeder Hintergrundjob verwendet den verpflichtenden Typ `TenantCacheKey { legal_entity_id: Uuid, namespace: &'static str, key: String }` — der Aufrufer kann keinen Schlüssel ohne `legal_entity_id` erzeugen.
- Hintergrundjobs (Export, Brief-Rendering, Katalog-Validierung, BI-Aggregation) prüfen vor Auslieferung erneut `job.legal_entity_id == caller.tenant_context.legal_entity_id`.

### 2.3 Schlüsselrotation (`key_version` & `kis-admin reencrypt`)
- `kis-core::security::Security` verwaltet einen aktiven Schreibschlüssel (`current_key_version`) und eine geordnete Liste älterer Leseschlüssel (nur zum Entschlüsseln während der Rotation).
- `kis-admin reencrypt --from-version <N> --to-version <N+1> --batch-size 500` verarbeitet `patients`, `patient_identifiers`, `patient_contacts`, `case_coverages` und `users.mfa_secret_enc` idempotent in Batches mit optimistischem Locking (`version`), schreibt Fortschritts-Audit und prüft per Stichprobe die erfolgreiche Entschlüsselung vor Abschaltung von Schlüssel `N`.

### 2.4 Ausfallbetrieb / Downtime-Read-Only-Modus (`T24`, Szenario 5)
- Definierter Betriebszustand `downtime_read_only`:
  - Schreibt ein lokaler Standort wegen WAN-/DB-Störung nicht mehr auf den Primärstandort, bleiben lokale Notfall-Stammdaten/Stationslisten/Medikationspläne nur über explizit signierte, verschlüsselte, zeitlich eng befristete Read-Only-Snapshots einsehbar (niemals ungeschützte Klartext-Exports auf Clients!).
  - Jeder Rücklauf (Reconciliation nach Wiederherstellung) markiert nachgetragene Ereignisse eindeutig mit `event_time_utc` (echte Behandlungszeit) vs. `recorded_time_utc` (Nachtragszeit) und verhindert Überschreiben neuerer Primärstände über `version`-Check.

### 2.5 Verbund-Lasttest (`scripts/loadtest.py`, `AP-06`)
- Pflicht-Zielprofil aus [ENTERPRISE_BACKLOG.md](../ENTERPRISE_BACKLOG.md) & [VERBUND_TESTMODELL.md](../VERBUND_TESTMODELL.md):
  - **1 000 Betten** verteilt auf mehrere Kliniken (`KL-N1`, `KL-N2`, `KL-S1`, `KL-S2`) plus parallele **MVZ-Sprechstunden** (`MVZ-N1`, `MVZ-N2`, `MVZ-S1`) und **Zentrallabor-Aufträge** (`ZD-LAB`).
  - Automatisierte `EXPLAIN (ANALYZE, BUFFERS)`-Auswertung für kritische Abfragen (`check_case`, `MY_UNITS`, Stationsliste, Patientensuche, Outbox-Polling, Audit-Chain-Insert) — keine unerwarteten `Seq Scan`-Engpässe auf Großtabellen.
  - Messbericht wird unter `docs/SCALING.md` mit realer Hardware-Angabe und Commit-SHA dokumentiert (niemals geschätzte Zahlen).
