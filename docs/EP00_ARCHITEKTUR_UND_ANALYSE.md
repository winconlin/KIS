# EP-00 (M00) — Ist-Analyse und verbindliche Zielarchitektur (ADRs)

Stand: 24.11.2026 · Arbeitspaket: `EP-00` (Meilenstein `M00`) · Status: Entwurf zur unabhängigen Architektur- und Sicherheitsprüfung (keine Produktivfreigabe)

Dieses Dokument bildet die technische und architektonische Grundlage für alle nachfolgenden Enterprise-Arbeitspakete (`EP-01` bis `EP-26`, `DOC-01`–`07`, `ED-01`–`06`). Es stützt sich ausschließlich auf die tatsächliche Analyse des ausführbaren Workspaces unter `crates/`, der Migrationen `migrations/001_init.sql` bis `migrations/011_dienstplan.sql`, der Konfiguration unter `config/`, der Prüfhilfen unter `scripts/` und des dokumentierten Prüfstands ([REVIEW_STATUS.md](REVIEW_STATUS.md)).

Zugehörige EP-00-Artefakte:
- [NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md) — Verbindliches N/H/X-Entscheidungsregister für `E01`–`E24` und `TI01`–`TI15`
- [THREAT_MODEL_VERBUND.md](THREAT_MODEL_VERBUND.md) — Bedrohungsmodell für Klinikverbund, MVZ und zentrale Dienste
- [VERBUND_TESTMODELL.md](VERBUND_TESTMODELL.md) — Synthetisches Verbund-Testmodell und Negativtest-Katalog (`NVT-01`–`NVT-16`)
- [ROADMAP.md](ROADMAP.md) — Abhängigkeitsbasierte Gesamt-Roadmap, Quality Gates `G0`–`G7` und Arbeitspaket-Zerlegung
- [ABNAHMEBERICHT.md](ABNAHMEBERICHT.md) und [abnahme/EP-00.md](abnahme/EP-00.md) — Vorlage und ausgefüllter EP-00-Abnahmebericht

---

## Teil A: Ist-Analyse des bestehenden Systems

### A.1 Workspace-Struktur, Modulgrenzen und Datenbankumfang

Der ausführbare Workspace ([Cargo.toml](../Cargo.toml)) besteht aus 13 Crates:
- **Kern & Komposition:** `kis-core` (Querschnittslogik ohne Fachdomäne), `kis-app` (Komposition, HTTP-Middleware, Admin-/DSGVO-Routen, CLI `kis-admin`).
- **10 Fachmodule + UI:** `kis-org`, `kis-patients`, `kis-doctors`, `kis-appointments`, `kis-cases`, `kis-clinical`, `kis-orders`, `kis-lab`, `kis-nursing`, `kis-medications`, `kis-prescriptions`, `kis-meals`, `kis-roster`, `kis-ui` (Hinweis: `kis-medications` und `kis-prescriptions` sind separate Crates; insgesamt 11 aktive Fachmodule laut `config/module_tables.toml` plus `kis-ui`).
- **Nicht im Build:** Alle Schnittstellen-Entwürfe unter `entwurf/` (`kis-hl7`, `kis-fhir`, `kis-dicom`, `kis-xds`, `kis-gdt`, `kis-medico`, `kis-ti`) sind ausdrücklich kein Ausgangscode und werden weder gebaut noch getestet.

Die Datenbank besteht aus **11 unveränderlichen Migrationen** (`migrations/001_init.sql` bis `migrations/011_dienstplan.sql`, gesichert über `migrations/CHECKSUMS`) mit **49 Tabellen** (verifiziert durch `scripts/check_modules.py`: `49 Tabellen, 11 Fachmodule geprüft, 0 Verstöße`).

| Migration | Schwerpunkt | Zentrale Tabellen |
|---|---|---|
| `001_init.sql` | Basis-Stammdaten | `patients`, `doctors`, `appointments`, `medications` |
| `002_security.sql` | Benutzer, Rollen, Audit, Verschlüsselungsspalten | `users`, `audit_logs`, `patients.*_enc`, `doctors.license_number_enc`, `mfa_secret` |
| `003_audit_hash_chain.sql` | Hash-Kette & Append-Only-Trigger | `audit_logs.prev_hash` / `entry_hash`, Trigger `audit_logs_immutable` |
| `004_sessions_anonymization_audit_guard.sql` | Sitzungen, Anonymisierung | `sessions`, `anonymized_patients`, strengere Audit-Trigger |
| `005_plattform.sql` | Plattform, Rollen als Daten, Org-Struktur, Ereignisse | `roles`, `user_roles`, `settings`, `platform_state`, `org_units`, `rooms`, `beds`, `user_org_units`, `domain_events`, `event_consumers`, `event_failures` |
| `006_faelle.sql` | Stationäre/ambulante Fälle, Behandlungszusammenhang | `cases`, `case_stays`, `case_care_team`, `break_glass_grants` |
| `007_klinische_akte.sql` | Klinische Akte | `case_diagnoses`, `case_procedures`, `clinical_notes`, `case_tasks`, `case_handovers` |
| `008_leistungen_auftraege.sql` | Auftragswesen & Labor | `service_catalog`, `orders`, `lab_results`, `automation_rules` |
| `009_pflege_verordnung.sql` | Pflege & stationäre Verordnung | `vital_signs`, `fluid_balance`, `medication_orders`, `medication_administrations` |
| `010_verpflegung.sql` | Küche & Verpflegung | `diets`, `case_diet_orders` |
| `011_dienstplan.sql` | Personal & Dienstplan | `staff_profiles`, `shift_types`, `shift_assignments`, `staff_absences`, `public_holidays` |

### A.2 Bewährte Stärken (zu erhalten)

Die bestehende Architektur enthält solide Bausteine, die im Enterprise-Ausbau ausdrücklich erhalten und nicht abgeschwächt werden:
1. **Strikte Modul-Isolation:** `kis-core` enthält keine Fachlogik; Fachmodule hängen nur von `kis-core` ab; Schreibgrenzen werden automatisiert per `scripts/check_modules.py` gegen `config/module_tables.toml` erzwungen.
2. **Deny-by-default-Routenvertrag:** Routen entstehen ausschließlich über `ModuleRoutes` mit explizitem `Access` (`crates/kis-core/src/module.rs`, `crates/kis-app/src/middleware.rs`).
3. **Unveränderliche Migrationen & Guardrails:** `migrations/CHECKSUMS` und `scripts/check_guardrails.py` verhindern nachträgliche Migrationsmanipulation, `TODO`/`todo!`/`dbg!` und `#[ignore]` im Produktivcode.
4. **Sitzungs- und Audit-Grundschutz:** Trennung von Access- und Refresh-Token, Argon2id-Passworthashes, persistente Sitzungen mit Rotation/Widerruf, atomares Audit in derselben DB-Transaktion mit Advisory-Lock und Append-Only-Trigger (`crates/kis-core/src/audit.rs`, `auth.rs`).
5. **Getrennte DB-Rollen:** `kis_owner` für Migrationen vs. `kis_app` für Laufzeit (`config/postgres/roles.sql`).

### A.3 Identifizierte Architektur-, Sicherheits- und Code-Lücken (Eingang für EP-01 bis EP-07)

Die tiefgehende Code- und Schema-Analyse deckt folgende konkrete Blocker auf, die vor dem Aufbau weiterer Fachmodule gelöst werden müssen:

| Nr. | Bereich | Konkreter Befund im Bestandscode / Schema | Zielpaket |
|---|---|---|---|
| **G-01** | Mandanten & Rechtsträger | `org_units` (`005_plattform.sql`) kennt keinen eigenständigen Rechtsträger (`legal_entity_id`/`tenant_id`), nur eine einzelne `ik_number` auf `kind = 'einrichtung'`, keine MVZ-/BSNR-/NBSNR-/LANR-/Telematik-ID-Register und keine zeitgebundenen Strukturkanten. `GET /api/v1/org/units` (`kis-org/src/lib.rs`) liefert global alle Einheiten ohne Mandantenfilter. | `EP-01` |
| **G-02** | Globale Singletons | `roles`, `settings`, `platform_state`, `service_catalog`, `automation_rules`, `diets`, `public_holidays` sind globale Tabellen ohne Mandanten-/Standort-Scoping oder Vererbungshierarchie. | `EP-01` |
| **G-03** | Globales Admin-Recht | `RoleMatrix::effective` (`crates/kis-core/src/rbac.rs`) gewährt der Rolle `admin` pauschal `PermissionSet::all()`. Damit hat ein technischer Administrator unbegrenztes klinisches Leserecht — Verstoß gegen [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 2. | `EP-04`, `EP-01` |
| **G-04** | Abschaltbare Schutzregeln | `security.treatment_context.enforce` (`crates/kis-core/src/platform.rs`) kann zur Laufzeit global auf `false` gestellt werden. Rechtliche Mindestschutzregeln dürfen nicht administrativ deaktivierbar sein. | `EP-01`, `EP-04` |
| **G-05** | Rekursion über Rechtsträgergrenzen | `MY_UNITS` in `crates/kis-core/src/treatment.rs` traversiert `org_units` rekursiv nach unten ohne Prüfung auf Rechtsträgergrenzen. Eine Zuordnung auf Ebene `traeger` gewährt Zugriff auf alle untergeordneten Häuser/Rechtsträger. | `EP-01`, `EP-04` |
| **G-06** | Unbeschränkter Patienten-/Fallzugriff | `GET /api/v1/patients`, `GET /api/v1/patients/:id` (`kis-patients/src/lib.rs`), `GET /api/v1/cases?caseNumber=` und `GET /api/v1/patients/:id/cases` (`kis-cases/src/lib.rs`) laufen mit `Scope::None` ohne Mandanten- oder Behandlungszusammenhangsprüfung. | `EP-02`, `EP-03`, `EP-04` |
| **G-07** | MPI & ungenutzte Feldverschlüsselung | `patients` (`001_init.sql`) nutzt globales `external_id TEXT UNIQUE` und `birth_date DATE NOT NULL` (blockiert unbekannte Notfallpatienten ohne fiktive Daten), hat keinen Identitätsraum je Rechtsträger und kein auditiertes Merge/Unmerge. Zudem existieren `insurance_number_enc`, `phone_enc`, `email_enc` (`002_security.sql`), doch `kis-patients` liest und schreibt weiterhin die Klartextspalten. | `EP-02` |
| **G-08** | Fallmodell nur einfach-stationär | `cases` (`006_faelle.sql`) vermischt medizinischen Fall, Abrechnungsfall und Encounter. `department_id` erzwingt `kind = 'fachabteilung'` (blockiert reine MVZ-/Ambulanz-/ZNA-Kontakte). `cases_one_open_inpatient` verhindert legitime gleichzeitige Fälle in getrennten Rechtsträgern. Es fehlt die Trennung zwischen hausinterner Verlegung und rechtsträgerübergreifender Entlassung + Neuaufnahme. | `EP-03` |
| **G-09** | DSGVO, Retention, Read-Replica | `export_patient` (`crates/kis-core/src/data_portability.rs`) liest von `db.read()` (mögliche Replica-Verzögerung) und exportiert global alle Fälle ohne Mandantengrenze; CSV/FHIR lassen Falldaten aus (`AP-03`). `retention_check()` (`data_deletion.rs`) prüft nur `appointments.start_time` und ignoriert `cases`, `clinical_notes`, § 630f BGB und Legal Holds. `anonymization.rs` belässt Fall-Freitexte im Klartext (`AP-04`) und setzt `1900-01-01` als fiktives Geburtsdatum. | `EP-04` |
| **G-10** | Audit, MFA, IP, Browser-Token | Lesezugriffe auf Patienten, Fälle und klinische Akten werden nicht auditiert (nur Mutationen, `audit.read`, `patient.export`). `audit_logs` ist nicht partitioniert und besitzt keinen externen Prüfanker. `users.mfa_secret` liegt im Klartext vor, ein MFA-Einrichtungs-Endpunkt fehlt. `core_routes.rs` übergibt `ip = None` an `auth::login`/`auth::refresh`. Die Web-Oberfläche speichert Tokens im `localStorage` (`AP-02`). | `EP-04`, `EP-07` |
| **G-11** | Best-Effort bei Sicherheitsinvarianten | `EventBus::publish` (`crates/kis-core/src/events.rs`) fängt Fehler aller Abonnenten per Savepoint ab und schreibt sie in `event_failures`, während der Auslöser committet. In `kis-orders/src/lib.rs` läuft die Bettensperre bei Entlassung (`Automation`) genau über diesen Best-Effort-Pfad. Verstoß gegen [AGENTS.md](../AGENTS.md) § 4 (sicherheitskritische Invarianten müssen atomar sein oder in einem expliziten Sperrzustand enden). Zudem fehlt `domain_events` ein Mandantenbezug, ein Inbox-/Idempotenzvertrag und das Audit in `resolve_event_failure` (`admin_platform.rs`). | `EP-05` |
| **G-12** | Kataloge & Kodierung | `case_diagnoses` / `case_procedures` prüfen nur Regex-Formate. Es fehlen versionierte Tabellen für ICD-10-GM, OPS, UCUM, LOINC, SNOMED CT, ATC/PZN sowie ein Lizenz-/Quellenregister. Der Allergie-Abgleich in `kis-prescriptions` ist rein textuell. | `EP-06` |
| **G-13** | Laufzeitfehler & SQL-Stil im Bestandscode | In `crates/kis-doctors/src/lib.rs` (`list_doctors`) erzeugen die Filter `search` und `specialty` Platzhalter `$1`/`$2`, binden die Werte aber nie an `sqlx::query_as` (Laufzeitfehler bei Filteraufruf). Zudem formatieren `list_doctors` und `list_patients` den `cursor`-Parameter per `format!("id > '{}'", cursor)` direkt in den SQL-String statt über gebundene Parameter. | `AP-01` / `EP-02a` |

### A.4 Vorhandene Prüfnachweise und aktuelle Prüfgrenzen

- **Historischer Nachweis (Stand 10.11.2026, [REVIEW_STATUS.md](REVIEW_STATUS.md)):** 100 Rust-Tests (davon 17 gegen PostgreSQL 16), 6 Backup-/Restore-Tests, 276 geprüfte SQL-Anweisungen. Dieser Stand deckt ausschließlich den Ein-Haus-Kern vor den Enterprise-Erweiterungen ab und ist ausdrücklich **kein** Nachweis für Mandantentrennung, MPI, MVZ, TI oder Echtbetrieb.
- **Lokal am 24.11.2026 in dieser Umgebung tatsächlich ausgeführt:** `scripts/check_guardrails.py` (0 Verstöße), `scripts/check_modules.py` (49 Tabellen, 11 Fachmodule, 0 Verstöße), `scripts/check_docs.py` (0 Fehler).
- **In dieser Container-Umgebung nicht ausführbar (offen):** `cargo`/`rustc`, `postgres`/`psql`, `docker`, `age`, `scripts/check_audit_exceptions.py` (fehlende `Cargo.lock`) sowie Git-Branch-/PR-Erstellung (kein `.git`-Verzeichnis im Workspace-Snapshot). Details im [EP-00-Abnahmebericht](abnahme/EP-00.md).

---

## Teil B: Architekturentscheidungen (ADRs)

Die folgenden ADRs legen verbindliche Verträge für `EP-01` bis `EP-07` fest. Implementierungspakete dürfen diese Grenzen nicht aufweichen; Änderungen erfordern eine explizite ADR-Fortschreibung und unabhängige Prüfung.

### ADR-001: Mandanten-, Rechtsträger- und Organisationsmodell (`EP-01`)

- **Status:** Vorgeschlagen (Freigabe vor Beginn `EP-01`)
- **Kontext:** Ein Klinikverbund betreibt rechtlich getrennte Träger (z. B. gGmbH, AG, MVZ-GmbH), mehrere Standorte, Akutkliniken, MVZ-Haupt- und Nebenbetriebsstätten sowie zentrale Dienste. Datenschutzrechtlich (DSGVO Art. 4 Nr. 7, Landeskrankenhausgesetze, SGB V) ist der **Rechtsträger (Verantwortlicher)** die maßgebliche Daten- und Zugriffsgrenze — nicht der übergeordnete Konzern/Verbund.
- **Entscheidung:**
  1. Einführung einer erstklassigen Entität `legal_entities` (Mandant / Rechtsträger) mit unveränderlicher `id`, Rechtsform, Status, Datenschutz-Verantwortlichem und Standard-Aufbewahrungsprofil.
  2. Einführung einer Entität `sites` (Standort / Betriebsstätte) mit Pflicht-Fremdschlüssel `legal_entity_id` und sektorbezogener Kennung (`sector`: `inpatient`, `mvz`, `practice`, `central_service`, `mixed`).
  3. Erweiterung von `org_units` um verpflichtende `legal_entity_id` (und optional `site_id`). Jede Hierarchiekante (`org_unit_edges` oder zeitgebundene Versionierung mit `valid_from` / `valid_to`) darf **niemals** zwei verschiedene `legal_entity_id` über eine normale Organisationsvererbung verbinden (DB-Trigger/Constraint).
  4. Einführung einer versionierten Kennungstabelle `org_identifiers` (`identifier_type` aus Server-Allowlist: `IK`, `BSNR`, `NBSNR`, `LANR`, `TELEMATIK_ID`, `STANDORTNUMMER_293`, `UMSATZSTEUER_ID`, Gültigkeitszeitraum `valid_from`–`valid_to`).
  5. **Zentrale Dienste:** Zusammenarbeit zwischen zwei Rechtsträgern (z. B. Verbund-Zentrallabor oder Radiologie) erfolgt ausschließlich über explizite, zeitgebundene `service_agreements` (Leistungs-/AVV-Verträge mit erlaubtem Zweck und Modul-Scope), niemals über globale Rechtsträger-Zusammenlegung.
  6. **Konfigurationsvererbung:** Dreistufige Hierarchie `verbund_default` → `legal_entity` → `site`/`org_unit` mit unveränderlicher Schutzklasse (`policy_floor`), sodass rechtliche Mindestschutzregeln (Mandantenisolation, Behandlungszusammenhang, Audit-Pflicht, Passwort-/Sitzungsgrenzen) auf keiner Ebene abgeschaltet werden können.
- **Konsequenzen:** Jede fachliche Tabelle mit Personen- oder Fallbezug erhält `legal_entity_id NOT NULL` (direkt oder über verpflichtenden Fremdschlüssel auf eine mandantengebundene Parent-Entität). Alle Abfragen in `kis-core` und Fachmodulen filtern explizit auf den aktiven Mandantenkontext (`TenantContext`).

### ADR-002: Enterprise-MPI und Identitätsräume (`EP-02`)

- **Status:** Vorgeschlagen (Freigabe vor Beginn `EP-02`)
- **Kontext:** In einem Klinikverbund kann derselbe Mensch Patient bei zwei rechtlich getrennten Trägern sein. Ein gemeinsamer Patientenstamm darf **keine** pauschale klinische Quersicht zwischen Rechtsträgern eröffnen. Zugleich müssen Dubletten, unbekannte Notfallpatienten und rechtsträgerlokale Patientennummern (MRN) sauber beherrscht werden.
- **Entscheidung:**
  1. **Zweistufige Trennung:**
     - `mpi_identities` (Verbund-Master-Index, minimaler demografischer Suchindex mit strikt getrenntem Recht `mpi.search` / `mpi.manage` und Auditierung jeder Suche).
     - `patient_records` (rechtsträgerlokale Patientenakte je `legal_entity_id`, besitzt lokale MRN, lokale Versicherungsdaten, Einwilligungen und verknüpft sich optional oder mandantenintern mit einer `mpi_identity_id`).
  2. **Identitätsräume (`patient_identifiers`):** Strukturierte Kennungen mit `(legal_entity_id, namespace_oid, identifier_type, value, valid_from, valid_to, verified_at)` für lokale MRN, KVNR (mit Prüfziffernvalidierung, ohne pauschale Freigabe fremder Rechtsträger), PKV-Nummer, Ausweis-/Fremd-ID. Globale `external_id UNIQUE` wird durch `(legal_entity_id, namespace_oid, value)` abgelöst.
  3. **Unbekannte Notfallpatienten:** `birth_date` wird nullable bzw. begleitet von `identity_status` (`verified`, `unverified`, `unknown_emergency`) und optional `estimated_birth_year` — keine fiktiven Geburtsdaten (`1900-01-01`).
  4. **Dubletten & Merge/Unmerge:** Dedizierte Tabellen `mpi_match_candidates` (Arbeitsliste mit deterministischen/probabilistischen Treffergründen ohne externe Blackbox) und `mpi_merge_events` (reversibler Merge/Unmerge mit vollständiger Referenz-Umhängung, Sperre während laufender Abrechnung/Transfusion und atomarem Audit).
  5. **Verschlüsselung sensibler Felder:** Migration der bisher ungenutzten `*_enc`-Spalten auf aktive AEAD-Feldverschlüsselung in `kis-patients` inkl. Blind-Index (HMAC mit Schlüsselversion) für exakte KVNR-/MRN-Suche, Entfernung von Klartext-Schreibpfaden nach Übergangsmigration.
- **Konsequenzen:** Eine Suche im MPI zeigt außerhalb des eigenen Rechtsträgers ausschließlich minimale Identifikationsmerkmale (und nur bei expliziter Verbund-Berechtigung / Einwilligung / gesetzlicher Grundlage) — niemals Diagnosen, Fälle oder Leistungen fremder Rechtsträger.

### ADR-003: Längsschnittakte, Episode, Encounter, Klinischer Fall und Abrechnungsfall (`EP-03`)

- **Status:** Vorgeschlagen (Freigabe vor Beginn `EP-03`)
- **Kontext:** Das bisherige 1:1-Modell `cases` vermischt medizinischen Verlauf, Aufenthalt und Abrechnung und zwingt jeden Fall in eine stationäre `fachabteilung`. Das verhindert MVZ/KV-Quartalsfälle, BG-/D-Arzt-Fälle, ZNA-Kontakte ohne stationäre Aufnahme, vor-/nachstationäre Ketten, Fallzusammenführungen (DRG) und gleichzeitige Behandlungen in verschiedenen Rechtsträgern.
- **Entscheidung:** Trennung in fünf klar abgegrenzte Entitäten (alle mit `legal_entity_id NOT NULL`):
  1. **`care_episodes` (Längsschnitt-Episode):** Fachlicher Behandlungsanlass über Sektoren innerhalb eines Rechtsträgers (z. B. onkologischer Verlauf, Schwangerschaft, chronische Erkrankung); optionale verbundweite Verknüpfung nur über explizite Einwilligung/Dokumentenaustausch.
  2. **`clinical_cases` (Klinischer Fall):** Medizinischer Fall mit `care_setting` (`inpatient`, `day_clinic`, `emergency`, `outpatient_hospital`, `mvz_kv`, `private_practice`, `pre_inpatient`, `post_inpatient`), optionaler `department_id` (Pflicht nur bei stationär/teilstationär; für MVZ/Ambulanz gilt `site_id`/`org_unit_id`).
  3. **`encounters` (Kontakt / Aufenthalt):** Einzelner Besuch, Ambulanzkontakt, ZNA-Aufenthalt oder stationärer Aufenthaltsabschnitt mit Start/Ende, Status und Standort/Einheit.
  4. **`case_stays` (Betten-/Raumbelegung):** Verfeinerung der bisherigen Tabelle mit Bezug auf `encounter_id` und `clinical_case_id` für Betten-, Behandlungsplatz- und Funktionsplatzbelegung.
  5. **`billing_cases` (Abrechnungsfall):** Eigenständiges Abrechnungsobjekt mit `billing_regime` (`drg`, `pepp`, `kv_ebm`, `goae`, `bg_uv`, `aop`, `hybrid_drg`, `self_pay`, `other`), Verknüpfung zu einem oder mehreren `clinical_cases` (z. B. bei Fallzusammenführung oder vor-/nachstationärer Kopplung) und eigener Statusmaschine (`open`, `coding`, `grouped`, `invoiced`, `contested`, `closed`).
  6. **Rechtsträgerwechsel vs. Verlegung:**
     - Wechsel zwischen Einheiten desselben `legal_entity_id` = interne Verlegung (`case_transfer`).
     - Wechsel in einen anderen `legal_entity_id` = rechtlich zwingende **Entlassung** im abgebenden Rechtsträger + **Neuaufnahme** im aufnehmenden Rechtsträger, ggf. verknüpft über expliziten, auditierten Überleitungsauftrag mit Einwilligung/gesetzlicher Grundlage.
  7. **Parallele Fälle:** Eindeutigkeitsregeln (`one_open_inpatient`) gelten künftig pro `(legal_entity_id, patient_record_id)` — nicht global über alle Rechtsträger.
- **Konsequenzen:** Bestehende API-Routen unter `/api/v1/cases/:case_id/...` bleiben kompatibel über `clinical_cases`, erzwingen aber künftig Mandanten- + Behandlungszusammenhangsprüfung.

### ADR-004: Berechtigung, Zweckbindung, Admin-Trennung, Audit, Retention & Legal Hold (`EP-04`)

- **Status:** Vorgeschlagen (Freigabe vor Beginn `EP-04`)
- **Kontext:** Im Klinikverbund reicht eine globale RBAC-Matrix nicht aus. Datenschutz und Patientensicherheit fordern feingranulare Objektberechtigung, lückenlose Nachvollziehbarkeit von Lesezugriffen, strikte Trennung administrativer von klinischen Rechten und revisionssichere Aufbewahrung.
- **Entscheidung:**
  1. **Vier-Faktoren-Zugriffsprüfung in `kis-core` (Policy Decision Point):** Jede geschützte Anfrage prüft unabhängig:
     1. **Authentifizierung & Sitzungsstatus** auf dem maßgeblichen Schreibstand (`db.write()`, nie verzögerte Read-Replica) inkl. Widerruf und Kontostatus.
     2. **Mandant/Rechtsträger & Standort/Einheit:** Nutzer-Zuordnung im angefragten `legal_entity_id` (oder aktiver `service_agreement` für zentrale Dienste).
     3. **Funktionsrecht & Zweckbindung (`purpose_of_use`):** z. B. `treatment`, `emergency_break_glass`, `billing`, `quality_assurance`, `audit`, `system_admin`, `patient_rights`.
     4. **Behandlungszusammenhang (`TreatmentContext`):** Station/Einheit, Care-Team, aktiver Leistungsauftrag oder zeitbefristeter Break-Glass-Grant — niemals administrativ abschaltbar.
  2. **Admin-Trennung (Least Privilege):** Aufspaltung der bisherigen `admin`-Rolle in:
     - `platform_admin` (technischer Betrieb, Mandantenanlage, Systemzustand — **kein** `patient.read`, **kein** `case.read`, **kein** `clinical.read`),
     - `tenant_admin` (Benutzer/Rollen/Org-Struktur eines einzelnen `legal_entity_id` — **kein** pauschales klinisches Leserecht),
     - `privacy_auditor` (Audit-Prüfung, Break-Glass-Nachkontrolle, DSGVO-Prozesse — kein klinisches Schreibrecht).
     `RoleMatrix::effective` entfernt das hartcodierte `admin => PermissionSet::all()`.
  3. **Audit-Härtung:**
     - Verpflichtendes **Lese-Audit** für Patientensuche, Patientenstammabruf, Fallakte, Dokumente, Exporte und Break-Glass-Zugriffe (mit Zweckbindung und `legal_entity_id`, ohne PII-Volldumps).
     - Monatliche **Partitionierung** von `audit_logs` (PostgreSQL Range Partitioning nach `created_at`) mit pro-Mandant-Prüfkette und **externem kryptografischem Prüfanker** (regelmäßiger signierter Root-Hash-Export in WORM-/externes Zielsystem).
  4. **Authentifizierung & Browser-Sicherheit:**
     - Verschlüsselung von `users.mfa_secret` mit AEAD-Master-Key + verpflichtendes MFA-Enrollment für privilegierte Rollen und Break-Glass-Konten.
     - Erfassung der realen Client-IP (nur vertrauenswürdiger Reverse-Proxy) in `auth::login`/`auth::refresh`.
     - Ablösung von `localStorage`-Tokens durch `HttpOnly; Secure; SameSite=Strict`-Cookies + CSRF-Schutz (`AP-02`).
  5. **Retention, Legal Hold & DSGVO:**
     - Mandanten- und dokumentenartspezifische `retention_policies` (z. B. § 630f BGB, Röntgen-/Strahlenschutzrecht, Transfusionsgesetz — als konfigurierbare Daten mit Quellenverweis, nicht erfunden).
     - Tabelle `legal_holds` (sperrt Löschung/Anonymisierung bei laufenden Verfahren, Gutachten, MD-Prüfungen).
     - DSGVO-Export und Löschprüfung laufen zwingend mandantengebunden gegen `db.write()`, umfassen alle Falldaten in JSON/CSV/FHIR und anonymisieren auch Freitexte bzw. setzen nachvollziehbare Redaktionsmarker ohne fiktive Geburtsdaten.
- **Konsequenzen:** Alle neuen und bestehenden Endpunkte erhalten automatisierte negative Verbundtests (`NVT-01`–`NVT-16` aus [VERBUND_TESTMODELL.md](VERBUND_TESTMODELL.md)).

### ADR-005: Sicherheitskritische Invarianten vs. isolierte Domänenereignisse & Inbox/Outbox (`EP-05`)

- **Status:** Vorgeschlagen (Freigabe vor Beginn `EP-05`)
- **Kontext:** Das aktuelle `EventBus::publish` isoliert sämtliche Folgeaktionen in Savepoints. Für optionale Folgeaktionen (z. B. Küchen-Benachrichtigung, Statistik) ist das korrekt; für sicherheitskritische klinische Invarianten (Bettensperre nach Entlassung/Isolation, kritische Laborwarnung, Transfusionssperre, AMTS-Stopp) ist Best-Effort gefährlich.
- **Entscheidung:** Zwei-Klassen-Vertrag im Kern (`kis-core::events`):
  1. **Klasse A — Sicherheitskritische Invarianten (`InvariantHook` / atomare Transaktion):**
     - Laufen **ohne** Fehler-Schlucken direkt in derselben Datenbanktransaktion wie der Auslöser **oder** überführen das Zielobjekt atomar in einen **explizit sicheren Sperrzustand** (z. B. Bett-Status `cleaning_required` / `blocked_pending_verification` direkt in der Entlassungstransaktion; kritischer Laborbefund in Pflicht-Quittierungsqueue `unacknowledged_critical`).
     - Ein Scheitern der Invariante bricht die auslösende Transaktion ab (Rollback) — niemals stiller Erfolg.
  2. **Klasse B — Optionale / nachgelagerte Domänenereignisse (Transaktionale Outbox + Inbox):**
     - `domain_events` erhält `legal_entity_id NOT NULL`, `event_id` (UUIDv7/v4), `correlation_id`, `idempotency_key`, `schema_version`, `payload_class` (`metadata_only` als Standard, keine ungeschützten PII).
     - Verbraucher implementieren eine `event_inbox`-Tabelle (Idempotenzschlüssel je Consumer), definierte Retry-Strategie (Exponential Backoff), **Quarantäne / Dead-Letter-Queue (`event_dead_letters`)** und auditiertes manuelles Replay (`resolve_event_failure` künftig mit Pflicht-Audit und Begründung).
- **Konsequenzen:** Die Bettensperre bei Entlassung/Verlegung (`kis-cases` ↔ `kis-orders`) wird als erster Referenzfall auf Klasse A (atomarer Sperrzustand + zuverlässige Auftragsanlage) umgestellt.

### ADR-006: Normen-, Terminologie-, Katalog- und Regelwerksversionierung (`EP-06`)

- **Status:** Vorgeschlagen (Freigabe vor Beginn `EP-06`)
- **Kontext:** Medizinische Kodierung (ICD-10-GM, OPS, Alpha-ID), Labor/Beobachtungen (LOINC, UCUM, SNOMED CT), Arzneimittel (ATC, PZN, AMTS) und Abrechnung (EBM, GOÄ, DRG, PEPP, AOP, Hybrid-DRG) ändern sich jährlich bzw. quartalsweise. Fest verdrahtete Listen oder erfundene Grenzwerte sind gemäß [AGENTS.md](../AGENTS.md) § 2 verboten.
- **Entscheidung:**
  1. Einführung eines zentralen, versionierten Katalog-Kerndienstes (`kis-catalogs` bzw. Kernvertrag in `EP-06`) mit Tabellen:
     - `catalog_sources` (Name, Herausgeber, Lizenztyp, Lizenz-Nachweisreferenz, Gültigkeitszeitraum, Import-Prüfsumme SHA-256),
     - `catalog_versions` (Katalog-Typ, Versionskennung z. B. `2026`, `valid_from`, `valid_to`, Status `staged`/`active`/`retired`),
     - `catalog_entries` (Code, System-URI/OID, Anzeigetext, Hierarchie/Parent, Zusatzattribute als validiertes JSONB),
     - `catalog_mappings` (z. B. ICD ↔ Alpha-ID, lokale Laborcodes ↔ LOINC/UCUM).
  2. **Stichtagsbezogene Validierung:** Diagnosen, Prozeduren, Leistungen und Verordnungen referenzieren die zum Behandlungszeitpunkt gültige `catalog_version_id`. Rückwirkende Korrekturen nutzen die historische Katalogversion, nicht den aktuellen Tagesstand.
  3. **Strikte Lizenz- und Quellentrennung:** Amtliche/lizenzierte Kataloge werden über reproduzierbare Import-Pipeline (mit Prüfsummen- und Formatprüfung) geladen. Synthetische Test-Fixtures für CI enthalten ausdrücklich als synthetisch markierte Testcodes und werden nie als amtliche Kataloge ausgegeben.
- **Konsequenzen:** Kein Fachmodul darf medizinische Grenzwerte, Triage-Schwellen oder Abrechnungsregeln erfinden; jede Regel benötigt eine konfigurierbare Katalog-/Regelversion mit Quellenangabe.

### ADR-007: Native-First-Architektur und Kapselung optionaler Koexistenz-Adapter (`EP-00` / alle EP)

- **Status:** Verbindlich (aus [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) § 1a)
- **Kontext:** Ziel ist ein eigenständiges Enterprise-KIS für Klinikverbünde und MVZ, kein Integrationsportal für proprietäre Fremdsoftware.
- **Entscheidung:**
  1. Jede Fähigkeit aus `E01`–`E24` und klinische Kernprozesse (`DOC-01`–`07`, `ED-01`–`06`) werden standardmäßig als **natives KIS-Modul (`N`)** mit eigener Datenhaltung, UI, API, Berechtigungen, Audit und Export geplant und umgesetzt.
  2. Externe Abhängigkeiten sind ausschließlich zulässig, wenn sie im [NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md) begründet als `H` (Hybrid mit unvermeidbarer/zugelassener externer Komponente) oder `X` (unvermeidbar extern) geführt werden.
  3. Schnittstellen- und Migrationsadapter (HL7 v2, FHIR/ISiK, DICOM, XDS, GDT, MEDICO) leben als klar getrennte Adapter-Crates neben den nativen Fachmodulen, schreiben niemals direkt in fremde Modultabellen und gelten niemals als Ersatz für eine fehlende native Implementierung.
- **Konsequenzen:** Jedes Arbeitspaket weist im Abnahmebericht nach, dass keine native Zielimplementierung durch eine bloße Fremdsystem-Verlinkung ersetzt wurde.

---

## Teil C: Offene Fach- und Hausentscheidungen (vor späteren Meilensteinen zu klären)

Gemäß [AGENTS.md](../AGENTS.md) § 2 werden folgende Punkte ausdrücklich **nicht erfunden**, sondern als Entscheidungsbedarf an das Haus/Projektgremium dokumentiert:

| ID | Thema | Betroffenes Paket | Erforderliche Klärung |
|---|---|---|---|
| **DEC-01** | Triage-System in der Notaufnahme | `EP-12` / `ED-02` | Auswahl des lizenzierten Systems (MTS oder ESI) und Lizenzmodell |
| **DEC-02** | Arzneimittel- und AMTS-Datenbasis | `EP-10` / `EP-06` | Lizenzierte Datenquelle (z. B. ABDA/IFAP/MMI/Lauer-Taxe) für PZN, Interaktionen, Dosisgrenzen |
| **DEC-03** | DRG-/PEPP-Grouper-Rechenkern | `EP-20` | Zertifizierter InEK-Grouper (extern/lokal betrieben als `H`) vs. lizenzierte Bibliothek |
| **DEC-04** | TI-Zugang (Konnektor vs. TI-Gateway) | `EP-22` / `EP-23` | Konkreter zugelassener TI-Gateway-/Konnektor-Partner, SMC-B-/HBA-Testkarten und RU-Testumgebung |
| **DEC-05** | Bildarchiv-Strategie (PACS/VNA) | `EP-16` | Eigenbetriebene offene DICOM-Engine (z. B. nativer DICOM-Store/Orthanc-Ausbau als austauschbare Komponente) vs. bestehendes Verbund-PACS |
| **DEC-06** | Aufbewahrungs- & Löschmatrix je Bundesland | `EP-04d` | Rechtsverbindliche Tabelle je Dokumentenart (Landeskrankenhausgesetz, § 630f BGB, StrlSchV, TFG) |
| **DEC-07** | Fachliche Rollenmatrix je Rechtsträger | `EP-04` | Freigabe der Standardrollen-Rechte durch Ärztliche Direktion, Pflegedirektion, Datenschutz und Betriebsrat |
