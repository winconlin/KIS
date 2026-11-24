# Enterprise-Roadmap, Quality Gates (`G0`–`G7`) und Arbeitspaket-Zerlegung

Stand: 24.11.2026 · Maßgebliche Vorgaben: [AGENTS.md](../AGENTS.md), [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md), [TI_COMPLIANCE.md](TI_COMPLIANCE.md), [CLINICAL_WORKFLOWS.md](CLINICAL_WORKFLOWS.md), [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md)

Diese Roadmap strukturiert die schrittweise, abhängigkeitsgesteuerte Umsetzung des Enterprise-KIS von der Architektur-Baseline (`M00`/`EP-00`) bis zur vollständigen Verbund-, MVZ-, Abrechnungs- und TI-Fähigkeit (`M10`). **Native First** ist für alle Meilensteine verbindlich ([NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md)).

---

## 1. Verbindliche Quality Gates (`G0`–`G7`)

Kein Arbeitspaket gilt als abgeschlossen, bevor alle für das Paket anwendbaren Gates nachweislich erfüllt und im [Abnahmebericht](ABNAHMEBERICHT.md) dokumentiert sind.

| Gate | Bezeichnung | Verbindlicher Nachweis |
|---|---|---|
| **G0** | **Auftrag, Grenzen & Native-First** | Task-ID, Abhängigkeiten, betroffene Dateien, ADR-Abgleich und N/H/X-Prüfung ([NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md)) geklärt; keine erfundenen medizinischen Skalen/Grenzwerte/Kataloge. |
| **G1** | **Architektur-, Modul- & Schema-Integrität** | `kis-core` fachfrei; Fachmodule hängen nur von `kis-core` ab; `config/module_tables.toml` konsistent; Migrationen unveränderlich (`migrations/CHECKSUMS`); DB-Integritätsregeln als Constraints. |
| **G2** | **Sicherheit & Mandantenisolation** | Jede Route über `ModuleRoutes` mit `Access`; Mandant/Standort/Zweck/Behandlungszusammenhang geprüft; keine DB-Interna in Fehlern; alle zugeordneten negativen Verbundtests (`NVT-*`) und [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md)-Fälle grün. |
| **G3** | **PostgreSQL-, Funktions- & Fehlerpfad-Tests** | Echte PostgreSQL-Integrationstests (Erfolg, `403` fehlendes Recht, `403` fehlender Behandlungszusammenhang/Mandant, Konflikt-/Ausfallpfade); keine übersprungenen oder mit `#[ignore]` stillgelegten Tests. |
| **G4** | **Gesamtprüfung (`scripts/check.sh`)** | Vollständiger fehlerfreier Durchlauf von `KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh` (Format, Guardrails, Module, Doku, Clippy `-D warnings`, Tests, SQL-Abgleich, Audit-Ausnahmen). |
| **G5** | **Dokumentation & Statuswahrheit** | [FUNKTIONSLANDKARTE.md](FUNKTIONSLANDKARTE.md), [REVIEW_STATUS.md](REVIEW_STATUS.md), [API.md](API.md) (erzeugt), [DATABASE.md](DATABASE.md) und diese Roadmap ohne beschönigte Fertigmeldungen aktualisiert. |
| **G6** | **Unabhängiges Review (Pflicht bei Sicherheit/Kern)** | Unabhängige Prüfung durch nicht-implementierende Person/Instanz für alle Änderungen an Kern, Mandantenmodell, Auth, RBAC, Audit, DSGVO, Invarianten, TI und Migrationen. |
| **G7** | **Fach-, Betriebs- & Regulatorik-Freigabe** | Nur vor Produktiv-/Pilotbetrieb: Fachfreigabe (Klinik/MVZ/Datenschutz/Betriebsrat), Last-/HA-/Restore-Drill, Penetrationstest, ggf. MDR-/gematik-/KBV-/InEK-Nachweis ([TI_COMPLIANCE.md](TI_COMPLIANCE.md)). |

---

## 2. Abhängigkeitsgraph und Parallelisierungsregeln

```text
M00: EP-00 (Ist-Analyse, ADRs, Native-First-Register, Bedrohungsmodell, Testmodell)
  |
  v  [Vertrags- & ADR-Freeze vor M01]
M01: Plattform- & Verbund-Kernverträge (P0-Blocker, streng koordiniert!)
  +-- EP-01 (Mandanten, Rechtsträger, Standorte, MVZ-Kennungen, Config-Vererbung)
  |     |
  |     +--> EP-02 (Enterprise-MPI, Identitätsräume, Merge/Unmerge, Feldverschlüsselung)
  |     |      |
  |     |      +--> EP-03 (Episode, Clinical Case, Encounter, Billing Case, Rechtsträgerwechsel)
  |     |
  |     +--> EP-04 (4-Faktoren-AuthZ, Admin-Trennung, Lese-Audit, Break-Glass, MFA, Retention/Legal Hold)
  |     +--> EP-05 (Klasse-A-Sicherheitsinvarianten vs. Klasse-B-Outbox/Inbox/Quarantäne)
  |     +--> EP-06 (Versioniertes Normen-/Katalog-/Lizenzregister: ICD/OPS/UCUM/LOINC/ATC)
  |     +--> EP-07 (Verbund-Betrieb, Audit-Partitionierung/Anker, Mandanten-Metriken/Backup/HA)
  |
  v  [Erst nach Freeze & grünem G0-G6 von EP-01..EP-07: parallele Fachmodul-Stränge zulässig!]
  |
  +-- Strang A (Klinischer Kern): M02 (EP-08 Aufnahme/Entlass) -> M03 (EP-09, DOC-01..07) -> M04 (EP-10 Medikation/AMTS, EP-11 Pflege)
  +-- Strang B (Akut & Funktionsbereiche): M05 (EP-12, ED-01..06 ZNA) | M06 (EP-13 OP, EP-14 Intensiv) | M07 (EP-15 Labor, EP-16 RIS/PACS, EP-17 Logistik)
  +-- Strang C (MVZ & Sektorübergreifend): M08 (EP-18 MVZ/PVS, EP-19 Therapie/Sozialdienst)
  +-- Strang D (Kodierung, Abrechnung, Personal/Versorgung): M09 (EP-20 DRG/MD, EP-21 Abrechnung, EP-24 Controlling/QM, EP-25 Personal/Küche/Logistik)
  +-- Strang E (Interop, TI & Produktiv-Oberfläche): M10 (EP-22 ISiK/HL7/FHIR/DICOM/XDS/GDT, EP-23 TI01..TI15, EP-26 Enterprise-UI)
```

**Unabhängig bereits parallel zu `M00`/`M01` bearbeitbar (kleine Härtungspakete ohne Eingriff in Mandantenverträge):**
- `AP-01` (Bereinigung `kis-doctors/src/lib.rs`: fehlendes `.bind()` in `list_doctors`, Parametrisierung von `cursor` in `kis-doctors`/`kis-patients`, Überprüfung Modul-Leseausnahmen)
- `AP-02` (Umstellung Browser-Session von `localStorage` auf `HttpOnly`-Cookie + CSRF — koordiniert mit `EP-04a`)

**Strikt seriell / ein Integrator je gemeinsamem Vertrag:**
- Root-Manifest (`Cargo.toml`), `migrations/CHECKSUMS`, `config/module_tables.toml`, `kis-core`-Basisverträge (`TenantContext`, `TreatmentContext`, `EventBus`, `RoleMatrix`).

---

## 3. Feinzerlegung der P0-Blocker (`M01`: `EP-01` bis `EP-07`)

Jedes Teilpaket ist klein genug für einen eigenen Branch, eine einzelne prüfbare Migration (sofern Schema betroffen) und klare PostgreSQL-Abnahmetests.

| Teilpaket | Ziel & konkreter Umfang | Abhängigkeiten | Hauptdateien / Bereich | Pflicht-Tests & Gates |
|---|---|---|---|---|
| **EP-01a** | **Schema & Kernvertrag Rechtsträger/Standorte:** Neue Migration (`012_...`) für `legal_entities`, `sites`, `org_identifiers` (IK/BSNR/NBSNR/LANR/Telematik-ID), `legal_entity_id` auf `org_units` + Trigger gegen rechtsträgerübergreifende Kanten; `TenantContext` in `kis-core`. | `EP-00` (`ADR-001`) | `migrations/`, `kis-core`, `config/module_tables.toml` | `NVT-01`, `NVT-02`; `G0`–`G6` |
| **EP-01b** | **Mandantenfähiges Org-Modul & Service-Agreements:** Anpassung `kis-org` auf strikte `legal_entity_id`-Filterung, zeitgebundene Org-Kanten, Verwaltung von MVZ-Haupt-/Nebenbetriebsstätten und `service_agreements` für zentrale Dienste. | `EP-01a` | `crates/kis-org`, `crates/kis-app/tests/` | `NVT-01`, `NVT-02`, `NVT-09` (Vorstufe); `G0`–`G6` |
| **EP-01c** | **Mandantenfähige Konfiguration & Policy-Floor:** Vererbung `verbund_default → legal_entity → site/org_unit` für `settings`/`roles`/`platform_state`; unveränderliche Schutzklasse `policy_floor` (z. B. `treatment_context.enforce` nicht abschaltbar). | `EP-01a` | `crates/kis-core/src/platform.rs`, `kis-app/src/admin_platform.rs` | `NVT-04`; `G0`–`G6` |
| **EP-02a** | **MPI-Schema, Identitätsräume & aktive Feldverschlüsselung:** Migration für `mpi_identities`, `patient_records` (`legal_entity_id NOT NULL`), `patient_identifiers` (Namespace/OID, KVNR-Prüfziffer), `birth_date NULL` + `identity_status`; Umstellung `kis-patients` auf aktive AEAD-`*_enc`-Spalten + Blind-Index; Parametrisierung `cursor`. | `EP-01a` (`ADR-002`) | `migrations/`, `crates/kis-patients`, `kis-core/src/security.rs` | DB-Rohinspektionstest, `NVT-05`; `G0`–`G6` |
| **EP-02b** | **Mandantengetrennte Patientensuche & MPI-API:** Strikt getrennte Routen für lokale Patientenakte vs. minimale MPI-Suche (mit Lese-Audit), keine globale `Scope::None`-Patientenliste mehr. | `EP-02a` | `crates/kis-patients`, `crates/kis-app/tests/` | `NVT-05`, `NVT-10` (Suche); `G0`–`G6` |
| **EP-02c** | **Dubletten-Arbeitsliste & auditierter Merge/Unmerge:** `mpi_match_candidates`, `mpi_merge_events`, Notfallpatient (`unknown_emergency`) → Identifikation/Merge, Sperre über Rechtsträgergrenzen und bei laufender Abrechnung/Transfusion. | `EP-02b` | `crates/kis-patients`, `migrations/` | `NVT-06`, Merge/Unmerge-Integritätstest; `G0`–`G6` |
| **EP-03a** | **Fallmodell-Schema:** Migration für `care_episodes`, `clinical_cases` (`care_setting`, `department_id` nur bei stationär Pflicht), `encounters`, `billing_cases`, `legal_entity_id` auf Fall- und Belegungstabellen, `one_open_inpatient` pro `(legal_entity_id, patient_record_id)`. | `EP-01a`, `EP-02a` (`ADR-003`) | `migrations/`, `config/module_tables.toml` | Schema-/Constraint-Tests; `G0`–`G6` |
| **EP-03b** | **Fall-, Episoden- & MVZ-Kontakt-API (`kis-cases`):** Unterstützung für stationär, ZNA, MVZ/KV, Ambulanz, parallele Fälle; strikte Trennung interne Verlegung vs. Rechtsträgerwechsel (Entlassung + Neuaufnahme); Mandantenfilter auf allen Suchrouten (`?caseNumber=`, `/patients/:id/cases`). | `EP-03a` | `crates/kis-cases`, `crates/kis-app/tests/` | `NVT-07`, `PAT-01`, `PAT-02`, `PAT-04`; `G0`–`G6` |
| **EP-03c** | **Migration nachgelagerter Fall-Module auf Mandanten- & Fallvertrag:** Konsistente `legal_entity_id`-Durchreichung in `kis-clinical`, `kis-orders`, `kis-lab`, `kis-nursing`, `kis-prescriptions`, `kis-meals`, `kis-appointments`. | `EP-03b` | betroffene Fachmodule | Durchgängiger Verbund-Krankenhaus-Test (`hospital.rs` erweitert); `G0`–`G6` |
| **EP-04a** | **Admin-Trennung, MFA-Härtung & Session-Sicherheit:** Abschaffung `admin => PermissionSet::all()` (`platform_admin` / `tenant_admin` / `privacy_auditor`), AEAD-Verschlüsselung `mfa_secret`, MFA-Enrollment-Endpunkt, reale Client-IP in `core_routes.rs`, Cookie/CSRF-Vorbereitung (`AP-02`). | `EP-01a`, `EP-01c` (`ADR-004`) | `crates/kis-core/src/rbac.rs`, `auth.rs`, `kis-app` | `NVT-03`, `NVT-04`, MFA-/Session-Tests; `G0`–`G6` |
| **EP-04b** | **4-Faktoren-Zugriffsprüfung & Zentraldienst-Scoping:** Umbau `treatment.rs` & `middleware.rs`: Rechtsträgerprüfung auf `MY_UNITS`, Standort-/Einheitengrenze, `purpose_of_use`, zeitgebundene `service_agreements` für zentrale Dienste (`ZD-LAB`), Prüfung strikt auf Write-Pool. | `EP-01b`, `EP-03b`, `EP-04a` | `crates/kis-core/src/treatment.rs`, `kis-app/src/middleware.rs` | `NVT-08`, `NVT-09`; `G0`–`G6` |
| **EP-04c** | **Lese-Audit & Break-Glass-Governance:** Pflicht-Lese-Audit (Patient, Fall, Akte, Suche) mit Mandant & Zweck (ohne PII-Volldump), Break-Glass-Fristablauf & Nachprüfungs-Arbeitsliste je Rechtsträger. | `EP-04b` | `crates/kis-core/src/audit.rs`, `kis-app` | `NVT-10`, `NVT-11`; `G0`–`G6` |
| **EP-04d** | **Retention-Matrix, Legal Hold & Mandanten-DSGVO:** `retention_policies`, `legal_holds`, mandantengebundener DSGVO-Export über `db.write()` (JSON/CSV/FHIR inkl. Falldaten, `AP-03`), Freitext-Anonymisierung (`AP-04`) ohne fiktives Geburtsdatum. | `EP-03b`, `EP-04b` | `crates/kis-core/src/data_portability.rs`, `data_deletion.rs`, `anonymization.rs`, `kis-app/src/gdpr_routes.rs` | `NVT-12`, `NVT-13`; `G0`–`G6` |
| **EP-05a** | **Klasse-A-Sicherheitsinvarianten:** Trennung atomarer Invarianten von Best-Effort-Events in `kis-core::events`; Umstellung Bettensperre bei Entlassung/Verlegung (`kis-cases`/`kis-orders`) und kritische Laborwerte (`kis-lab`) auf atomaren Sperr-/Quittierungszustand. | `EP-01a`, `EP-03b` (`ADR-005`) | `crates/kis-core/src/events.rs`, `kis-cases`, `kis-orders`, `kis-lab` | `NVT-14`, Fehler-Injektionstest; `G0`–`G6` |
| **EP-05b** | **Mandantenfähige Outbox, Inbox & Quarantäne:** `legal_entity_id`, `idempotency_key`, `schema_version` auf `domain_events`; `event_inbox` & `event_dead_letters`; Pflicht-Audit bei `resolve_event_failure` / Replay. | `EP-05a` | `migrations/`, `crates/kis-core/src/events.rs`, `kis-app/src/admin_platform.rs` | `NVT-15`, Replay-/Idempotenztest; `G0`–`G6` |
| **EP-06a** | **Katalog- & Lizenzregister-Kern:** Migration & Modul für `catalog_sources`, `catalog_versions`, `catalog_entries`, `catalog_mappings` (ICD-10-GM, OPS, UCUM, LOINC, SNOMED CT, ATC/PZN) inkl. Prüfsummen-Import und Stichtags-Abfrage. | `EP-01a` (`ADR-006`) | `migrations/`, neues Modul `kis-catalogs` | Stichtags- & Versions-Tests mit synthetischen Testkatalogen; `G0`–`G6` |
| **EP-06b** | **Anbindung klinischer Module an Stichtags-Kataloge:** Umstellung `kis-clinical` (ICD/OPS), `kis-lab` (UCUM/LOINC) und `kis-prescriptions` (ATC/PZN-Referenz) auf `catalog_version_id`. | `EP-06a`, `EP-03c` | `kis-clinical`, `kis-lab`, `kis-prescriptions` | Historische Kodier-Stabilität; `G0`–`G6` |
| **EP-07a** | **Audit-Partitionierung & externer Prüfanker:** Monatliche Range-Partitionierung von `audit_logs`, mandantenfähige Hash-Kettenprüfung, signierter Anker-Export für WORM-Speicher. | `EP-04c` | `migrations/`, `kis-core/src/audit.rs`, `kis-app/src/bin/kis-admin.rs` | Manipulations- & Anker-Verifikationstest; `G0`–`G6` |
| **EP-07b** | **Verbund-Betrieb, Mandanten-Caching, Backup/Restore & HA:** Mandantenschlüssel-Zwang für Caches/Exporte/Jobs, mandantenfähige Metriken (`/metrics` ohne PII), Backup/Restore-Drill mit Verbund-Daten, Readiness/Liveness-Trennung. | `EP-01c`, `EP-07a` | `kis-app`, `scripts/backup.py`, `tests/test_backup.py` | `NVT-16`, Backup/Restore-Tests; `G0`–`G6` |

---

## 4. Übersicht der Folge-Meilensteine (`M02`–`M10`)

Die Feinzerlegung jedes Folge-Meilensteins wird vor dessen Start gegen die eingefrorenen Verträge aus `M01` finalisiert; die groben Teilpakete sind hier verbindlich zugeordnet:

| Meilenstein | EP-/Workflow-Pakete | Kernliefergegenstände (Native First) | Voraussetzung |
|---|---|---|---|
| **M02** | `EP-08` | Patientenadministration, Kostenträger/Versicherung, Einweiser/Angehörige, Wahlleistungen, Belegungsplanung, Entlassmanagement (§ 39 Abs. 1a SGB V) | `M01` grün |
| **M03** | `EP-09`, `DOC-01`–`07` | Native klinische Dokumentation, Arztbrief-Redaktion, Textbausteine, strukturiertes DMS mit Versionierung, mehrstufige Freigabe/Signatur, Korrektur/Nachtrag, PDF/A | `M01`, `M02` |
| **M04** | `EP-10`, `EP-11` | Native Medikation/Closed-Loop/BTM/BMP (mit `DEC-02`-Datenbasis als `H`), Pflegeplanung, Maßnahmen, Wunden, Assessments, PPR 2.0/PpUGV | `M01`, `EP-06b`, `DEC-02` |
| **M05** | `EP-12`, `ED-01`–`06` | Native Notaufnahme/ZNA: Anmeldung/Voranmeldung, unbekannter Patient, Ersteinschätzung (`DEC-01` als `H`), Behandlungsplatz-Board, Re-Assessment, stationäre Überleitung | `M01`, `M03`, `DEC-01` |
| **M06** | `EP-13`, `EP-14` | OP-Planung, WHO-Checklisten, OP-/Anästhesiedoku, Implantate; Intensivkurve/PDMS-Kern (Geräteanbindung `H`, MDR-Prüfung) | `M01`, `M03`, `M04` |
| **M07** | `EP-15`, `EP-16`, `EP-17` | Labor-Eigenbetrieb/Probenlogistik/Zweistufen-Validation, RIS-Kern & PACS/DICOMweb-Anbindung (`DEC-05`), Termin-/Transport-/Funktionslogistik | `M01`, `EP-05`, `DEC-05` |
| **M08** | `EP-18`, `EP-19` | Vollständiges natives MVZ-/PVS-Modul (BSNR/NBSNR/LANR, Scheine/Quartale, Verordnungen/AU/Überweisung), Therapie, Sozialdienst & Case Management | `M01`, `M02`, `M03` |
| **M09** | `EP-20`, `EP-21`, `EP-24`, `EP-25` | Kodierarbeitsplatz, DRG/PEPP-Grouper-Anbindung (`DEC-03`), MD-Verfahren; Abrechnung (§ 301, KV/EBM, GOÄ, BG, AOP, Hybrid-DRG); Controlling/QM/Meldewesen; Dienstplan- & Küchenausbau | `M01`, `M06`, `M08`, `DEC-03` |
| **M10** | `EP-22`, `EP-23`, `EP-26` | Interoperabilität (ISiK, ISiP, FHIR, HL7 v2, DICOM, XDS, GDT als gekapselte Module), TI (`TI01`–`TI15`, `DEC-04`), vollständige rollen-/mandantenfähige Enterprise-Weboberfläche & Barrierefreiheit | `M01`–`M09`, `DEC-04` |

---

## 5. Aktueller Fortschritt (ehrlicher Prüfstand: 24.11.2026)

| Meilenstein / Paket | Status | Tatsächlicher Nachweis | Offene Gates |
|---|---|---|---|
| **M00 / `EP-00`** | **Entwurf abgeschlossen — wartet auf unabhängiges Review** | Ist-Analyse, ADRs 001–007, Native-First-Register, Bedrohungsmodell, Verbund-Testmodell, Roadmap & Abnahmebericht erstellt; `check_guardrails.py`, `check_modules.py`, `check_docs.py` lokal 0 Fehler ([abnahme/EP-00.md](abnahme/EP-00.md)) | `G4` vollständig (Rust/PG/CI), `G6` unabhängiges Review, Git-Branch/PR-Integration |
| **M01 / `EP-01`–`EP-07`** | **offen** (zerlegt in `EP-01a`–`EP-07b`) | Noch nicht implementiert | `G0`–`G6` |
| **M02–M10 / `EP-08`–`EP-26`** | **offen** | Spezifiziert; Umsetzung erst nach Freeze von `M01` | `G0`–`G7` |
| **Unabhängige Härtung (`AP-01`, `AP-02`)** | **offen** | Konkreter Laufzeitfehler in `kis-doctors/src/lib.rs` (`list_doctors`) in `EP-00` identifiziert | Umsetzung + `G0`–`G6` in Rust/PG-Umgebung |
