# EP-00 (`M00`) — Ist-Analyse und verbindliche Zielarchitektur (ADRs)

Stand: 24.11.2026 (konsistenzbereinigte Fassung v2) · Arbeitspaket: `EP-00` (Meilenstein `M00`) · Autor: `google-neon` · Status: **Im Review (unabhängiges Gegenreview `G6` an `hermes` bzw. unabhängige Prüfinstanz zugewiesen; keine Produktivfreigabe)**

Dieses Dokument bildet die technische und architektonische Grundlage für alle nachfolgenden Enterprise-Arbeitspakete (`EP-01` bis `EP-26`, `DOC-01`–`07`, `ED-01`–`06`, `AP-*`). Es stützt sich ausschließlich auf die tatsächliche Prüfung des ausführbaren Workspaces unter `crates/`, der Migrationen `migrations/001_init.sql` bis `migrations/011_dienstplan.sql`, der Konfiguration unter `config/`, der Prüfhilfen unter `scripts/` und des dokumentierten Prüfstands ([REVIEW_STATUS.md](REVIEW_STATUS.md)).

Zugehörige EP-00- und Koordinations-Artefakte:
- [NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md) — Verbindliches N/H/X-Entscheidungsregister für `EP-00`–`EP-07`, `E01`–`E24`, `TI01`–`TI15`, `DOC-01`–`07`, `ED-01`–`06`
- [THREAT_MODEL_VERBUND.md](THREAT_MODEL_VERBUND.md) — Bedrohungsmodell für Klinikverbund, MVZ, zentrale Dienste und klinische Sicherheitsinvarianten
- [VERBUND_TESTMODELL.md](VERBUND_TESTMODELL.md) — Synthetisches Verbund-Testmodell, Durchgängige Abnahmeszenarien 1–8 und Pflicht-Negativtests (`NVT-01`–`NVT-18`)
- [ROADMAP.md](ROADMAP.md) — Verbindliche, mit [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md) abgeglichene Gesamt-Roadmap (`M00`–`M07`), Quality Gates `G0`–`G7` und Feinzerlegung
- [vertraege/README.md](vertraege/README.md) — Implementierungsreife `M01`-Umsetzungsverträge (`EP-01`–`EP-07`, Migrationsentwürfe `012`–`016`) & kanonische synthetische JSON-Fixtures
- [KOORDINATIONSREGISTER.md](KOORDINATIONSREGISTER.md) — Zentrales Aufgaben-, Ressourcen- und Reservierungsregister für Multi-Agenten-Koordination (Spiegelungsvorlage für GitHub-Koordinations-Issue)
- [ABNAHMEBERICHT.md](ABNAHMEBERICHT.md), [abnahme/EP-00.md](abnahme/EP-00.md) und [abnahme/EP-00-konsistenzpruefung.md](abnahme/EP-00-konsistenzpruefung.md)

---

## Teil A: Ist-Analyse des bestehenden Systems

### A.1 Workspace-Struktur, Modulgrenzen und Datenbankumfang

Der ausführbare Workspace ([Cargo.toml](../Cargo.toml)) umfasst exakt **16 Crates**:
- **Kern & Komposition (2 Crates):** `kis-core` (Querschnittslogik ohne Fachdomäne), `kis-app` (Komposition, HTTP-Middleware, Admin-/DSGVO-Routen, CLI `kis-admin`).
- **Kernmodule (3 Crates, nicht abschaltbar, schreiben Kern-Tabellen):** `kis-org`, `kis-patients`, `kis-cases`.
- **Fachmodule (11 Crates gemäß [config/module_tables.toml](../config/module_tables.toml)):** `kis-clinical`, `kis-orders`, `kis-lab`, `kis-nursing`, `kis-prescriptions`, `kis-meals`, `kis-roster`, `kis-doctors`, `kis-appointments`, `kis-medications`, `kis-ui`.
- **Nicht im Build (`exclude = ["entwurf"]`):** Alle älteren Schnittstellen-Entwürfe unter `entwurf/` (`kis-hl7`, `kis-fhir`, `kis-dicom`, `kis-xds`, `kis-gdt`, `kis-medico`, `kis-ti`) sind ausdrücklich kein Ausgangscode und weder gebaut noch geprüft.

Die Datenbank besteht aus **11 unveränderlichen Migrationen** (`migrations/001_init.sql` bis `migrations/011_dienstplan.sql`, gesichert über `migrations/CHECKSUMS`) mit **49 Tabellen** (verifiziert durch `python3 scripts/check_modules.py`: `49 Tabellen, 11 Fachmodule geprüft, 0 Verstöße`):

| Schicht / Modul | Eigentumstabellen (exakter Ist-Stand aus `config/module_tables.toml`) | Migration(en) |
|---|---|---|
| **Kern** (`kis-core`, `kis-app`, `kis-org`, `kis-patients`, `kis-cases`) | `users`, `sessions`, `roles`, `role_permissions`, `user_roles`, `settings`, `platform_state`, `audit_log`, `audit_logs`, `consents`, `anonymization_keys`, `org_units`, `rooms`, `beds`, `user_org_units`, `domain_events`, `event_consumers`, `event_failures`, `patients`, `patient_alerts`, `cases`, `case_stays`, `case_participants`, `emergency_access_grants` (24 Tabellen) | `001`–`006` |
| `kis-clinical` | `case_diagnoses`, `case_procedures`, `clinical_notes`, `case_tasks` (4 Tabellen) | `007` |
| `kis-orders` | `service_catalog`, `orders`, `automation_rules` (3 Tabellen) | `008` |
| `kis-lab` | `lab_specimens`, `lab_results` (2 Tabellen) | `008` |
| `kis-nursing` | `vital_signs`, `fluid_balance_entries` (2 Tabellen) | `009` |
| `kis-prescriptions` | `prescriptions`, `medication_administrations` (2 Tabellen) | `009` |
| `kis-meals` | `diets`, `case_diets` (2 Tabellen) | `010` |
| `kis-roster` | `staff_members`, `duty_types`, `duty_assignments`, `absences`, `public_holidays`, `staffing_positions`, `staffing_assignments` (7 Tabellen) | `011` |
| `kis-doctors` | `doctors` (1 Tabelle) | `001`, `002` |
| `kis-appointments` | `appointments` (1 Tabelle; dokumentierte Lese-Ausnahme `reads = ["doctors"]`) | `001` |
| `kis-medications` | `medications` (1 Tabelle) | `001` |
| `kis-ui` | keine eigenen Tabellen (0 Tabellen) | — |

*(Hinweis zu `AP-01`: Die Route `GET /api/v1/doctors/:id/schedule` liegt im Bestandscode bereits in `crates/kis-appointments/src/lib.rs` samt Test `crates/kis-appointments/tests/schedule.rs`, und `kis-doctors` hat keine Lese-Ausnahme mehr in `config/module_tables.toml`. Ein formaler CI-Abschlussnachweis für `AP-01` steht in dieser toolchain-freien Umgebung noch aus.)*

### A.2 Bewährte Stärken (unverändert zu erhalten)

1. **Strikte Modul-Isolation:** `kis-core` enthält keine Fachlogik; Fachmodule hängen nur von `kis-core` ab; Schreibgrenzen werden automatisiert per `scripts/check_modules.py` gegen `config/module_tables.toml` erzwungen.
2. **Deny-by-default-Routenvertrag:** Routen entstehen ausschließlich über `ModuleRoutes` mit explizitem `Access` (`crates/kis-core/src/module.rs`, `crates/kis-app/src/middleware.rs`).
3. **Unveränderliche Migrationen & Guardrails:** `migrations/CHECKSUMS` und `scripts/check_guardrails.py` verhindern nachträgliche Migrationsmanipulation, `TODO`/`todo!`/`dbg!` und `#[ignore]` im Produktivcode.
4. **Sitzungs- und Audit-Grundschutz:** Trennung von Access- und Refresh-Token, Argon2id-Passworthashes, persistente Sitzungen mit Rotation/Widerruf, atomares Audit in derselben DB-Transaktion mit Advisory-Lock und Append-Only-Trigger (`crates/kis-core/src/audit.rs`, `auth.rs`).
5. **Getrennte DB-Rollen:** `kis_owner` für Migrationen vs. `kis_app` für Laufzeit (`config/postgres/roles.sql`).

### A.3 Identifizierte Architektur-, Sicherheits- und Code-Lücken (Eingang für `EP-01` bis `EP-07` & Bestandshärtung)

| Nr. | Bereich | Konkreter Befund im Bestandscode / Schema | Zugeordnetes Zielpaket |
|---|---|---|---|
| **G-01** | Mandanten & Rechtsträger | `org_units` (`005_plattform.sql`) kennt keinen eigenständigen Rechtsträger (`legal_entity_id`/`tenant_id`), keine Betriebsgesellschaft, keine virtuellen Einheiten/Kostenstellen/Geräte, nur eine einzelne `ik_number` auf `kind = 'einrichtung'`, keine versionierten MVZ-/BSNR-/NBSNR-/LANR-/Telematik-ID-Register und keine zeitgebundenen, zyklenfreien Strukturkanten. `GET /api/v1/org/units` (`kis-org/src/lib.rs`) liefert global alle Einheiten ohne Mandantenfilter. | `EP-01` (`EP-01a`/`01b`) |
| **G-02** | Globale Singletons | `roles`, `settings`, `platform_state`, `service_catalog`, `automation_rules`, `diets`, `public_holidays` sind globale Tabellen ohne Mandanten-/Standort-Scoping oder Vererbungshierarchie. | `EP-01` (`EP-01c`) |
| **G-03** | Globales Admin-Recht | `RoleMatrix::effective` (`crates/kis-core/src/rbac.rs`) gewährt der Rolle `admin` pauschal `PermissionSet::all()`. Damit hat ein technischer Administrator unbegrenztes klinisches Leserecht — Verstoß gegen [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 2. | `EP-04` (`EP-04a`), `EP-01` |
| **G-04** | Abschaltbare Schutzregeln | `security.treatment_context.enforce` (`crates/kis-core/src/platform.rs`) kann zur Laufzeit global auf `false` gestellt werden. Rechtliche Mindestschutzregeln dürfen nicht administrativ deaktivierbar sein. | `EP-01` (`EP-01c`), `EP-04` (`EP-04a`) |
| **G-05** | Rekursion über Rechtsträgergrenzen | `MY_UNITS` in `crates/kis-core/src/treatment.rs` traversiert `org_units` rekursiv nach unten ohne Prüfung auf Rechtsträgergrenzen. Eine Zuordnung auf Ebene `traeger` gewährt Zugriff auf alle untergeordneten Häuser/Rechtsträger. | `EP-01` (`EP-01a`), `EP-04` (`EP-04b`) |
| **G-06** | Unbeschränkter Patienten-/Fallzugriff | `GET /api/v1/patients`, `GET /api/v1/patients/:id` (`kis-patients/src/lib.rs`), `GET /api/v1/cases?caseNumber=` und `GET /api/v1/patients/:id/cases` (`kis-cases/src/lib.rs`) laufen mit `Scope::None` ohne Mandanten- oder Behandlungszusammenhangsprüfung. | `EP-02`, `EP-03`, `EP-04` |
| **G-07** | MPI & ungenutzte Feldverschlüsselung | `patients` (`001_init.sql`) nutzt globales `external_id TEXT UNIQUE` und `birth_date DATE NOT NULL` (blockiert unbekannte Notfallpatienten und Neugeborene ohne fiktive Daten), hat keine Identitätsräume je Rechtsträger und kein auditiertes Merge/Unmerge. Zudem existieren `insurance_number_enc`, `phone_enc`, `email_enc` (`002_security.sql`), doch `kis-patients` liest und schreibt weiterhin die Klartextspalten. | `EP-02` (`EP-02a`/`02b`/`02c`) |
| **G-08** | Fallmodell nur einfach-stationär | `cases` (`006_faelle.sql`) vermischt medizinischen Fall, Abrechnungsfall und Encounter. `department_id` erzwingt `kind = 'fachabteilung'` (blockiert reine MVZ-/Ambulanz-/ZNA-Kontakte ohne Bett). `cases_one_open_inpatient` verhindert legitime gleichzeitige Fälle in getrennten Rechtsträgern. Es fehlen klinische Doppelbelegungskonflikte, Rechtsträgerwechsel (Entlassung + Neuaufnahme vs. Verlegung), Belegarzt/Shared Care und Kostenträger (`AP-13`). | `EP-03` (`EP-03a`/`03b`/`03c`) |
| **G-09** | DSGVO, Retention, Read-Replica | `export_patient` (`crates/kis-core/src/data_portability.rs`) liest von `db.read()` (mögliche Replica-Verzögerung) und exportiert global alle Fälle ohne Mandantengrenze; CSV/FHIR lassen Falldaten aus (`AP-03`). `retention_check()` (`data_deletion.rs`) prüft nur `appointments.start_time` und ignoriert `cases`, `clinical_notes`, § 630f BGB und Legal Holds. `anonymization.rs` belässt Fall-Freitexte im Klartext (`AP-04`) und setzt `1900-01-01` als fiktives Geburtsdatum. | `EP-04` (`EP-04d`, `AP-03`/`04`) |
| **G-10** | Audit, MFA, IP, Browser-Token | Lesezugriffe auf Patienten, Fälle und klinische Akten werden nicht auditiert (nur Mutationen, `audit.read`, `patient.export`). `audit_logs` ist nicht partitioniert und besitzt keinen externen Prüfanker. `users.mfa_secret` liegt im Klartext vor, ein MFA-Einrichtungs-Endpunkt fehlt. `core_routes.rs` übergibt `ip = None` an `auth::login`/`auth::refresh`. Die Web-Oberfläche speichert Tokens im `localStorage` (`AP-02`). | `EP-04` (`EP-04a`/`04c`), `EP-07`, `AP-02` |
| **G-11** | Best-Effort bei Sicherheitsinvarianten | `EventBus::publish` (`crates/kis-core/src/events.rs`) fängt Fehler aller Abonnenten per Savepoint ab und schreibt sie in `event_failures`, während der Auslöser committet. In `kis-orders/src/lib.rs` läuft die Bettensperre bei Entlassung (`Automation`) genau über diesen Best-Effort-Pfad. Verstoß gegen [AGENTS.md](../AGENTS.md) § 4 und [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 4. Zudem fehlt `domain_events` ein Mandantenbezug, ein Inbox-/Idempotenzvertrag, Reconciliation, Out-of-order-Behandlung und das Audit in `resolve_event_failure` (`admin_platform.rs`). | `EP-05` (`EP-05a`/`05b`) |
| **G-12** | Kataloge, Einheiten & Kodierung | `case_diagnoses` / `case_procedures` prüfen nur Regex-Formate. Es fehlen versionierte Tabellen für ICD-10-GM, OPS, UCUM, LOINC, SNOMED CT, ATC/PZN, oBDS sowie ein maschinenlesbares Normen-/Lizenzregister ([TI_COMPLIANCE.md](TI_COMPLIANCE.md)). Der Allergie-Abgleich in `kis-prescriptions` ist rein textuell. | `EP-06` (`EP-06a`/`06b`, `AP-14`) |
| **G-13** | Laufzeitfehler & SQL-Stil im Bestandscode | In `crates/kis-doctors/src/lib.rs` (`list_doctors`) erzeugen die Filter `search` und `specialty` Platzhalter `$1`/`$2`, binden die Werte aber nie an `sqlx::query_as` (Laufzeitfehler `500` bei Filteraufruf). Zudem formatieren `list_doctors` und `list_patients` den `cursor`-Parameter per `format!("id > '{}'", cursor)` direkt in den SQL-String statt über gebundene Parameter. | **`AP-09`** (unabhängige Bestandskorrektur; nicht mit `AP-01` vermischen!) |
| **G-14** | Ungewartete Abhängigkeiten | Laut `docs/ARBEITSPAKETE.md` (`AP-08`) meldete `cargo audit` `chrono` (`RUSTSEC-2026-0543`) und `rustls-pemfile` (`RUSTSEC-2025-0134`) als unmaintained. Evaluierung/Ablösung ist offen und in der Rust-Umgebung mit `Cargo.lock` zu prüfen. | **`AP-08`** (unabhängige Wartung/Härtung) |

### A.4 Vorhandene Prüfnachweise und aktuelle Prüfgrenzen

- **Historischer Nachweis (Stand 10.11.2026, [REVIEW_STATUS.md](REVIEW_STATUS.md)):** 100 Rust-Tests (davon 17 gegen PostgreSQL 16), 6 Backup-/Restore-Tests, 276 geprüfte SQL-Anweisungen. Dieser Stand deckt ausschließlich den Ein-Haus-Kern vor den Enterprise-Erweiterungen ab und ist **kein** Nachweis für Mandantentrennung, MPI, MVZ, TI oder Echtbetrieb.
- **Lokal am 24.11.2026 tatsächlich ausgeführt:** `scripts/check_guardrails.py` (0 Verstöße), `scripts/check_modules.py` (49 Tabellen, 11 Fachmodule, 0 Verstöße), `scripts/check_docs.py` (0 Fehler).
- **In dieser Container-Umgebung nicht ausführbar (offen):** `cargo`/`rustc`, `postgres`/`psql`, `docker`, `age`, `scripts/check_audit_exceptions.py` (fehlende `Cargo.lock`), `gh` (nicht installiert) sowie Git-Branch-/PR-Erstellung (kein `.git`-Verzeichnis im Workspace-Snapshot). Details siehe [abnahme/EP-00.md](abnahme/EP-00.md) und [abnahme/EP-00-konsistenzpruefung.md](abnahme/EP-00-konsistenzpruefung.md).

---

## Teil B: Architekturentscheidungen (ADRs)

Die folgenden ADRs legen verbindliche Verträge für `EP-01` bis `EP-07` fest. Sie setzen alle Anforderungen aus [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 1a, § 2, § 4 und [TI_COMPLIANCE.md](TI_COMPLIANCE.md) um. Änderungen erfordern eine explizite ADR-Fortschreibung und unabhängige Prüfung (`G6`).

### ADR-001: Mandanten-, Rechtsträger- und Organisationsmodell (`EP-01`)

- **Status:** Vorgeschlagen (Gegenreview `G6` durch `hermes` / unabhängige Instanz vor Start `EP-01a`)
- **Kontext:** Ein Klinikverbund umfasst Träger/Konzern, rechtlich getrennte Verantwortliche/Mandanten, Betriebsgesellschaften, Einrichtungen, Standorte, Fachabteilungen, MVZ, Haupt- und Nebenbetriebsstätten, Praxen, Stationen, Funktionsstellen, virtuelle Einheiten, Kostenstellen, Räume, Betten, Geräte und zentrale Dienste. Datenschutzrechtlich ist der **rechtliche Verantwortliche (`legal_entity`)** die Daten- und Zugriffsgrenze — das Organigramm ersetzt niemals eine Datenschutzgrenze.
- **Entscheidung:**
  1. **Klare Entitätstrennung:**
     - `organization_groups` (optionaler Träger/Konzern — rein organisatorische Klammer, gewährt **niemals** Datenzugriff).
     - `legal_entities` (rechtlicher Verantwortlicher / Mandant, inkl. optionaler `operating_company` / Betriebsgesellschaft, Rechtsform, Datenschutz-Verantwortlicher, Standard-Aufbewahrungsprofil).
     - `sites` (Standort / Betriebsstätte mit Pflicht-Fremdschlüssel `legal_entity_id` und Sektor `inpatient`, `mvz`, `practice`, `central_service`, `mixed`).
     - `org_units` erweitert um `legal_entity_id NOT NULL`, optional `site_id`, differenzierte Typen (`facility`, `department`, `mvz_main`, `mvz_branch`, `practice`, `ward`, `functional_unit`, `virtual_unit`, `cost_center`, `admin`, `supply`, `other`), sowie getrennte Ressourcen-Tabellen (`rooms`, `beds`, `devices`).
  2. **Zeitgebundene, zyklenfreie Hierarchie:** Kanten (`org_unit_edges` mit `valid_from` / `valid_to`) dürfen **niemals** Zyklen bilden und **niemals** Einheiten unterschiedlicher `legal_entity_id` über eine normale Vererbung verbinden (PostgreSQL-Trigger/Constraint).
  3. **Versionierte institutionelle Kennungen (`org_identifiers`):** `identifier_type` aus Server-Allowlist (`IK`, `STANDORTNUMMER_293`, `BSNR`, `NBSNR`, `LANR`, `TELEMATIK_ID`, `UMSATZSTEUER_ID`, weitere amtliche Register-IDs) mit `issuing_authority`, `valid_from`, `valid_to`, `legal_entity_id`. Organisatorische, medizinische, abrechnende und TI-Identität werden ausdrücklich getrennt geführt.
  4. **Mehrfach-Arbeitsverhältnisse & Rotationen (`user_org_assignments`):** Eine Person kann zeitgebundene Rollen und Arbeitsverhältnisse an mehreren Standorten/Einheiten oder Rechtsträgern halten (Vertreter, Konsile, Rufdienste, rotierende Mitarbeitende), jeweils mit explizitem aktivem Mandanten-/Standortkontext pro Anfrage (`TenantContext`).
  5. **Zentrale Dienste (`service_agreements`):** Zusammenarbeit zwischen zwei Rechtsträgern erfolgt ausschließlich über explizite, zeitgebundene Leistungs-/AVV-Verträge mit erlaubtem Zweck und Modul-Scope, niemals über globale Rechtsträger-Zusammenlegung.
  6. **Konfigurationsvererbung & Policy-Floor:** Hierarchie `verbund_default` → `legal_entity` → `site` → `org_unit` mit Gültigkeitszeiträumen, expliziten Überschreibungen und unveränderlicher Schutzklasse (`policy_floor`), sodass rechtliche Mindestschutzregeln (Mandantenisolation, Behandlungszusammenhang, Audit-Pflicht, Passwort-/Sitzungsgrenzen) auf keiner Ebene abschaltbar sind.
- **Konsequenzen:** Jede fachliche Tabelle mit Personen- oder Fallbezug führt `legal_entity_id NOT NULL` (direkt oder über verpflichtenden mandantengebundenen Parent-FK). Alle Abfragen, Suchen, Exporte, Caches und Jobs prüfen den serverseitig ermittelten `TenantContext`.

### ADR-002: Enterprise-MPI, Identitätsräume und Kontakte (`EP-02`, inkl. `AP-12`)

- **Status:** Vorgeschlagen (Gegenreview `G6` vor Start `EP-02a`)
- **Kontext:** Ein gemeinsamer Patientenstamm darf keine pauschale Quersicht zwischen Rechtsträgern eröffnen. Gleichzeitig müssen Dubletten, unbekannte Notfallpatienten, Neugeborene, lokale Patientenaktennummern (MRN), Einweiser/Angehörige (`AP-12`) und rechtssichere Zusammenführungen beherrscht werden.
- **Entscheidung:**
  1. **Zweistufige Trennung & optional getrennte Identitätsräume:**
     - `mpi_identities` (minimaler demografischer Master-Index mit eigenem Recht `mpi.search`/`mpi.manage` und Pflicht-Leseaudit). Betriebsoption `mpi.mode` unterstützt `isolated_per_legal_entity` (vollständig getrennte Identitätsräume je Rechtsträger) sowie `federated_with_legal_basis` (mandantenübergreifender Abgleich nur auf dokumentierter Rechtsgrundlage/Einwilligung).
     - `patient_records` (rechtsträgerlokale Patientenakte je `legal_entity_id` mit lokaler MRN, lokalen Daten und optionalem Verweis auf `mpi_identity_id`).
  2. **Identifikator-Namensräume & Provenienz (`patient_identifiers`):** `(legal_entity_id, namespace_oid, identifier_type, value, source_system, verified_at, valid_from, valid_to)` für lokale MRN, KVNR (nur soweit zulässig, mit Prüfziffer), PKV-Nummer, Fremd-IDs. Globales `external_id UNIQUE` entfällt.
  3. **Temporäre Identitäten:** `birth_date` wird nullable (`SICHERHEITS_ABNAHME` `D-05`); `identity_status` unterscheidet `verified`, `unverified`, `unknown_emergency` (unbekannter Notfallpatient) und `newborn_provisional` (Neugeborenes mit optionalem Mutterbezug) — niemals fiktive Geburtsdaten (`1900-01-01`).
  4. **Dubletten-Arbeitsliste & reversibler Merge/Unmerge:** `mpi_match_candidates` (kein automatischer Merge allein nach Name + Geburtsdatum!) und `mpi_merge_events` (geprüfter Merge/Unmerge mit vollständiger Provenienz, Sperre über Rechtsträgergrenzen sowie bei laufender Transfusion/OP/Abrechnung, konkurrierende Merges per Lock serialisiert).
  5. **Kontakte & Einweiser (`AP-12` integriert in `EP-02`):** `patient_contacts` je `legal_entity_id` (Angehörige, gesetzliche Betreuung/Vertretung, Hausarzt, Einweiser, Auskunftsberechtigung).
  6. **Aktive Feldverschlüsselung:** Umstellung von `kis-patients` auf AEAD-Verschlüsselung (`*_enc` inkl. `key_version` für Rotation) plus schlüsselversionierten Blind-Index für exakte Suche; Ablösung ungeschützter Klartext-Schreibpfade.
- **Konsequenzen:** MPI-Suchen offenbaren niemals klinische Fälle, Diagnosen oder Leistungen fremder Rechtsträger.

### ADR-003: Sektorenübergreifendes Fall-, Episoden-, Aufenthalts- und Abrechnungsmodell (`EP-03`, inkl. `AP-13`)

- **Status:** Vorgeschlagen (Gegenreview `G6` vor Start `EP-03a`)
- **Kontext:** Das bisherige 1:1-Modell `cases` blockiert MVZ/KV-Quartalsfälle, ambulante Kontakte ohne Bett, parallele Fälle in getrennten Standorten/Rechtsträgern, Belegärzte, Shared Care und die Trennung zwischen klinischem Verlauf und Abrechnung.
- **Entscheidung:** Klare Trennung in sechs koordinierte Kernentitäten (alle mit `legal_entity_id NOT NULL`):
  1. **`care_episodes` (Längsschnitt-Episode):** Fachlicher Behandlungsanlass über Sektoren innerhalb eines Rechtsträgers (z. B. onkologischer Verlauf, Schwangerschaft); optionale Verknüpfung über Rechtsträgergrenzen nur über expliziten, auditierten Überleitungs-/Dokumentenaustausch mit Rechtsgrundlage.
  2. **`clinical_cases` (medizinischer Fall):** Mit `care_setting` (`inpatient`, `day_clinic`, `emergency`, `outpatient_hospital`, `mvz_kv`, `private_practice`, `pre_inpatient`, `post_inpatient`, `rehab`, `aop`, `asv`), `department_id` nur bei stationär/teilstationär Pflicht (ambulante/MVZ-/ZNA-Kontakte brauchen kein Bett und keine stationäre Fachabteilung).
  3. **`encounters` (Kontakt / Aufenthaltsabschnitt):** Einzelner Besuch, Sprechstundenkontakt, ZNA-Kontakt oder stationärer Abschnitt mit Start/Ende, Status, Standort/Einheit und Behandlerrolle (inkl. `attending`, `consulting`, `belegarzt`, `shared_care`, `external_partner`).
  4. **`case_stays` (Betten-/Platzbelegung):** Zuordnung zu `encounter_id` und `clinical_case_id`; **klinische Doppelbelegung** (zwei gleichzeitige physische Bett-/Platzbelegungen derselben Person oder zweier Personen auf demselben Bett) wird als eigener Konflikt/Constraint unabhängig von parallelen Abrechnungsfällen behandelt.
  5. **`billing_cases` (abrechnungsrechtlicher Fall):** Eigenständiges Objekt mit `billing_regime` (`drg`, `pepp`, `kv_ebm`, `goae`, `bg_uv`, `aop`, `asv`, `hybrid_drg`, `self_pay`, `other`), n:m-Verknüpfung zu `clinical_cases` (Fallzusammenführung, vor-/nachstationär) und eigener Statusmaschine.
  6. **Kostenträger & Versicherungsverhältnis (`AP-13` integriert in `EP-03`):** Mandantenfähige Tabellen `payers` (IK-Formatprüfung) und `case_coverages` (höchstens ein Hauptkostenträger zur selben Zeit je Abrechnungsfall).
  7. **Übergänge & Parallelfälle:**
     - Interne Verlegung innerhalb desselben `legal_entity_id` erhält Fall-/Episoden-ID und Historie.
     - Wechsel zwischen zwei `legal_entity_id` ist zwingend **Entlassung + Neuaufnahme** (optional verknüpft über auditierten Überweisungs-/Überleitungsdatensatz).
     - Überweisung, Rücküberweisung und Konsil werden als eigene, nachvollziehbare Übergangsdatensätze mit erhaltener ID-Historie modelliert.
     - Parallele Fälle über verschiedene Standorte/Sektoren oder getrennte Rechtsträger sind kontrolliert zulässig (`one_open_inpatient` pro `(legal_entity_id, patient_record_id)` statt global).
- **Konsequenzen:** Bestehende `/api/v1/cases/:case_id/...`-Routen bleiben über `clinical_cases` kompatibel, erzwingen aber Mandanten- und Behandlungszusammenhangsprüfung.

### ADR-004: IAM/SSO, Objektberechtigung, Admin-Trennung, Audit, Retention & Legal Hold (`EP-04`, inkl. `AP-02`/`03`/`04`)

- **Status:** Vorgeschlagen (Gegenreview `G6` vor Start `EP-04a`)
- **Kontext:** Mandantenfähiger Verbundbetrieb erfordert serverseitige Objektberechtigung auf **jeder** Datenfläche (API, Suche, Listen, Export, FHIR, Anhänge/OCR, Cache, Jobs, Outbox, Logs, BI), strikte Trennung von Technik-/HR-/Finanz-/Forschungs- und Klinikrechten sowie lückenlose Nachvollziehbarkeit.
- **Entscheidung:**
  1. **Vier-Faktoren-Zugriffsprüfung (`PolicyDecisionPoint` in `kis-core`):** Jede geschützte Anfrage prüft auf dem maßgeblichen Schreibstand (`db.write()`, `SICHERHEITS_ABNAHME` `A-14`):
     1. Authentifizierung, Sitzungswiderruf und Kontostatus,
     2. Mandant/Rechtsträger & Standort/Einheit (oder aktiver `service_agreement`),
     3. Funktionsrecht & Zweckbindung (`purpose_of_use`: `treatment`, `emergency_break_glass`, `billing`, `quality_assurance`, `research_pseudonymized`, `hr`, `finance`, `audit`, `system_admin`, `patient_rights`),
     4. Behandlungszusammenhang (`TreatmentContext`, niemals abschaltbar).
  2. **Rollen- und Domänentrennung:** Abschaffung von `admin => PermissionSet::all()`. Getrennte Rollenfamilien für `platform_admin` (Technik, kein klinisches Leserecht), `tenant_admin` (Mandanten-Stammdaten, kein pauschales klinisches Leserecht), `privacy_auditor`, `hr_manager` (keine klinischen Daten), `finance_clerk` (keine unberechtigten klinischen Freitexte), `research_analyst` (nur genehmigte pseudonymisierte Zone).
  3. **IAM/SSO & Session-Härtung:** Lokale Authentifizierung bleibt voll funktionsfähig; optionaler OIDC-/SAML-/LDAP-Adapter mit strikter Mandantenzuordnung. AEAD-Verschlüsselung von `users.mfa_secret`, vollständiger MFA-Enrollment-/Recovery-Flow, reale Client-IP-Auswertung (nur vertrauenswürdige Proxy-Grenze, `F-03`), Browser-Session auf `HttpOnly; Secure; SameSite=Strict`-Cookie + CSRF (`AP-02`), Schlüsselrotation (`key_version`).
  4. **Audit & externer Prüfanker:** Verpflichtendes **Lese-Audit** für Stammdatensuche, Patientenabruf, Fallakte, Dokumente/Anhänge/OCR, Exporte und Break-Glass (mit `legal_entity_id` und `purpose_of_use`, ohne PII-Volldump). Break-Glass eng befristet, begründungspflichtig und mit eskalierender Nachprüfungs-Queue je Rechtsträger. Monatliche Partitionierung von `audit_logs` und signierter externer Prüfanker (WORM) gegen DB-Admin-Manipulation (`L-01`–`L-09`).
  5. **Retention, Legal Hold & DSGVO:** Konfigurierbare `retention_policies` (nach Rechtsgrundlage/Dokumentart/Bundesland, `DEC-06`), `legal_holds` gegen vorzeitige Löschung, mandantengebundener DSGVO-Export über `db.write()` in JSON, CSV und FHIR inkl. aller Falldaten (`AP-03`) sowie abgestimmte Freitext-Anonymisierung (`AP-04`, `DEC-08`) ohne fiktive Geburtsdaten.
- **Konsequenzen:** Jede neue Datenfläche benötigt negative Verbundtests (`NVT-01`–`NVT-18`) und die anwendbaren Nachweise aus [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md).

### ADR-005: Sicherheitskritische Invarianten vs. transaktionale Outbox/Inbox (`EP-05`)

- **Status:** Vorgeschlagen (Gegenreview `G6` vor Start `EP-05a`)
- **Kontext:** Das aktuelle `EventBus::publish` isoliert sämtliche Abonnenten im Savepoint. Sicherheitskritische klinische Invarianten dürfen jedoch niemals best-effort laufen, und das Abschalten eines optionalen Moduls darf keine unverzichtbare Schutzfunktion still beseitigen.
- **Entscheidung:** Zwei-Klassen-Vertrag in `kis-core::events`:
  1. **Klasse A — Sicherheitskritische Invarianten:** Laufen atomar in derselben DB-Transaktion wie der Auslöser oder überführen das Zielobjekt atomar in einen expliziten **sicheren Sperrzustand** (z. B. Bettensperre `cleaning_required` bei Entlassung/Isolation; kritischer Laborwert in Pflicht-Quittierungsqueue; Allergie-/AMTS-/Transfusionssperre; Entlassung+Neuaufnahme bei Rechtsträgerwechsel; Pflicht-Audit). Scheitern führt zu Rollback oder sicherem Blockierzustand — niemals stillem Erfolg. Modulabschaltung darf Klasse-A-Schutzfunktionen nicht deaktivieren (Start-/Konfigurationsprüfung).
  2. **Klasse B — Optionale Domänenereignisse & Schnittstellen (Outbox/Inbox):** `domain_events` mit `legal_entity_id NOT NULL`, `event_id`, `correlation_id`, `idempotency_key`, `schema_version`, `payload_class` (`metadata_only` als Standard). Verbraucher nutzen eine persistente `event_inbox` (Deduplizierung, Out-of-order-Erkennung, keine falsche Behauptung von „exactly-once über Netze“), Retry mit Backoff, Quarantäne/Dead-Letter (`event_dead_letters`), Reconciliation-Abgleich und auditiertes Replay (`resolve_event_failure` mit Pflicht-Audit und Begründung).
- **Konsequenzen:** Crash-, Retry-, Duplikat-, Out-of-order- und mandantenübergreifende Sperrtests werden verpflichtender Bestandteil von `EP-05`.

### ADR-006: Normen-, Terminologie-, Katalog- und Zeit-/Einheitenvertrag (`EP-06`, inkl. `AP-14`)

- **Status:** Vorgeschlagen (Gegenreview `G6` vor Start `EP-06a`)
- **Kontext:** Amtliche Kataloge (ICD-10-GM, OPS, Alpha-ID, UCUM, LOINC, SNOMED CT, ATC/PZN, oBDS, EBM/GOÄ/DRG/PEPP/AOP/Hybrid-DRG) sowie TI-/FHIR-Profile ändern sich regelmäßig und unterliegen teils Lizenzen. Zudem verlangt [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 4 strikte Zeit-, Einheiten- und Statussemantik.
- **Entscheidung:**
  1. **Versioniertes Katalog- & Normenregister (`kis-catalogs`):** Tabellen `catalog_sources` (Herausgeber, URL, Lizenztyp, Lizenzreferenz, SHA-256-Prüfsumme), `catalog_versions` (`valid_from`, `valid_to`, Status `staged`/`active`/`retired`, Ablaufprüfung alter Kataloge), `catalog_entries`, `catalog_mappings` sowie das maschinenlesbare **Normen-/Compliance-Register** (`normative_requirements`: Quelle/URL, Version, Gültigkeit, Verpflichtungsgrundlage, Sektor, Rolle, Status, Test-ID, Nachweis, Owner, nächstes Reviewdatum gemäß [TI_COMPLIANCE.md](TI_COMPLIANCE.md)).
  2. **Stichtagsbezogene Bindung:** Diagnosen, Prozeduren, Laborwerte, Verordnungen und Abrechnungspositionen referenzieren die zum Ereigniszeitpunkt gültige `catalog_version_id`.
  3. **Einheiten-, Zeit- und Statusvertrag (Kernstandard für alle klinischen Module):**
     - Zeitstempel speichern `UTC` + lokale Zeitzone und trennen `event_time`, `recorded_time` und `released_time`.
     - Messwerte speichern Originaleinheit **und** normierte UCUM-Einheit.
     - Klinische Ergebnisse trennen verbindlich `preliminary`, `final`, `corrected`, `cancelled` (keine stille Gleichsetzung).
  4. **Gepinnte FHIR-/Profilpakete & synthetische CI-Fixtures:** Keine erfundenen medizinischen Codes, Grenzwerte oder Abrechnungsregeln im Code; CI verwendet klar als synthetisch markierte Testkataloge.
- **Konsequenzen:** `EP-06` kann nach Freeze von `EP-00` weitgehend parallel zu `EP-01` starten (mandantenbezogene Katalog-Überschreibungen docken an `EP-01c` an).

### ADR-007: Native-First-Architektur und Kapselung optionaler Koexistenz-Adapter (`EP-00` / alle EP)

- **Status:** Verbindlich (aus [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 1a)
- **Entscheidung:**
  1. Alle Fähigkeiten aus `E01`–`E24` sowie `DOC-01`–`07` und `ED-01`–`06` werden grundsätzlich als **native KIS-Module (`N`)** mit eigener Datenhaltung, Fachlogik, UI, API, Rechten, Audit, Korrektur, Ausfallverhalten und Export umgesetzt.
  2. `H`- und `X`-Grenzen sind ausschließlich im [NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md) mit Begründung, Quelle, nativer Abgrenzung, Exit-Plan, Verantwortlichem und Prüfdatum zulässig.
  3. Optionale Adapter (`EP-08` HL7/FHIR/DICOM/IHE/xDT, `EP-25` Migrations-/Koexistenzadapter, `EP-21` Nextcloud/SSO) bleiben strikt gekapselt, schreiben nie direkt in fremde Modultabellen und ersetzen niemals eine offene native Zielimplementierung.

---

## Teil C: Offene Fach-, Rechts- und Hausentscheidungen (nicht erfinden!)

| ID | Thema | Betroffenes Paket | Erforderliche Klärung vor Implementierung |
|---|---|---|---|
| **DEC-01** | Triage-System in der Notaufnahme | `EP-16` (`ED-02`, `AP-18`) | Auswahl und Lizenzierung des Einschätzungssystems (MTS oder ESI) je Haus |
| **DEC-02** | Arzneimittel- und AMTS-Datenbasis | `EP-13`, `EP-06` | Lizenzierte Datenquelle (z. B. ABDA/IFAP/MMI/Lauer-Taxe) für PZN, Interaktionen, Dosis-/Nieren-/Pädiatrie-Regeln |
| **DEC-03** | DRG-/PEPP-Grouper-Strategie | `EP-11` | Zertifizierter InEK-Grouper als `H`-Rechenkern vs. langfristig eigene zertifizierte Implementierung (`ENTERPRISE_KIS.md` § 1a) |
| **DEC-04** | TI-Zugang (Konnektor vs. TI-Gateway) & RU-Testkarten | `EP-09` (`TI01`–`TI15`) | Zugelassener TI-Gateway-/Konnektor-Partner, SMC-B-/eHBA-Testkarten und Zugang zur gematik-Referenzumgebung (RU) |
| **DEC-05** | Bildarchiv-/Viewer-Komponenten & diagnostische Zulassung | `EP-15` (`E07`) | Auswahl selbst betriebener offener DICOM-Bibliotheken/Codecs für natives RIS/PACS sowie MDR-Abgrenzung diagnostischer Viewer |
| **DEC-06** | Aufbewahrungs- & Löschmatrix je Bundesland | `EP-04d` | Rechtsverbindliche Fristentabelle je Dokumentart (§ 630f BGB, Landeskrankenhausgesetz, StrlSchV, TFG) |
| **DEC-07** | Fachliche Rollenmatrix & Betriebsrats-/Datenschutzfreigabe | `EP-04`, `EP-26` | Verbindliche Rechtezuordnung für klinische, MVZ-, HR-, Finanz- und Forschungsrollen je Rechtsträger |
| **DEC-08** | Umgang mit Fall-Freitexten bei Anonymisierung (`AP-04`) | `EP-04d` (`AP-04`) | Entscheidung: löschen, schwärzen oder Fall vom Patientenstamm trennen? |
| **DEC-09** | Medizinprodukt- & AI-Act-Einordnung je Modul | `EP-13`, `EP-14`, `EP-15`, `EP-16`, `EP-23`, `EP-26` | Zweckbestimmung, MDR/IVDR Rule 11, IEC 62304/ISO 14971 und EU-AI-Act-Einstufung vor klinischer Nutzung |
