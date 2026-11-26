# Enterprise-Roadmap, Quality Gates (`G0`–`G7`) und Arbeitspaket-Zerlegung

Stand: 24.11.2026 (konsistenzbereinigte Fassung v2 — 1:1 abgeglichen mit [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md), [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md), [TI_COMPLIANCE.md](TI_COMPLIANCE.md), [CLINICAL_WORKFLOWS.md](CLINICAL_WORKFLOWS.md) und [ARBEITSPAKETE.md](ARBEITSPAKETE.md)) · Koordinator: `google-neon`

**Vorrangregel bei IDs:** Die Paketnummern `EP-00` bis `EP-26` aus [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md), die Fähigkeits-IDs `E01`–`E24` aus [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md), die TI-IDs `TI01`–`TI15` aus [TI_COMPLIANCE.md](TI_COMPLIANCE.md), die Workflow-IDs `DOC-01`–`07` / `ED-01`–`06` aus [CLINICAL_WORKFLOWS.md](CLINICAL_WORKFLOWS.md) und die Bestandspakete `AP-01`–`AP-43` aus [ARBEITSPAKETE.md](ARBEITSPAKETE.md) sind verbindlich und unveränderlich. **Native First** gilt für alle Meilensteine ([NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md)). Die aktive Aufgaben- und Ressourcenvergabe führt das [KOORDINATIONSREGISTER.md](KOORDINATIONSREGISTER.md).

---

## 1. Verbindliche Status-Taxonomie und Quality Gates (`G0`–`G7`)

Jedes Paket und jede Fähigkeit unterscheidet strikt:
`Geplant` · `Spezifiziert` · `In Bearbeitung` · `Blockiert` · `Im Review` · `Technisch integriert` · `Lokal getestet` · `Gegenstelle getestet` · `Fachlich abgenommen` · `Extern bestätigt` · `Betriebsfreigegeben`.

Kein Arbeitspaket gilt als abgeschlossen, bevor alle anwendbaren Quality Gates nachweislich erfüllt und im [Abnahmebericht](ABNAHMEBERICHT.md) dokumentiert sind:

| Gate | Bezeichnung | Verbindlicher Nachweis |
|---|---|---|
| **G0** | **Auftrag, Koordination & Native-First** | Eindeutige Reservierung im [KOORDINATIONSREGISTER.md](KOORDINATIONSREGISTER.md) / GitHub-Koordinations-Issue; Abhängigkeiten, Dateibereiche, ADR-Abgleich und N/H/X-Prüfung ([NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md)) geklärt; keine erfundenen medizinischen Skalen, Grenzwerte, Abrechnungsregeln oder Kataloge. |
| **G1** | **Architektur-, Modul- & Schema-Integrität** | `kis-core` fachfrei; Fachmodule hängen nur von `kis-core` ab; `config/module_tables.toml` konsistent; Migrationen unveränderlich (`migrations/CHECKSUMS`); DB-Integritätsregeln als Constraints; `check_guardrails.py` & `check_modules.py` = 0 Verstöße. |
| **G2** | **Sicherheit & Mandantenisolation** | Jede Route über `ModuleRoutes` mit `Access`; Mandant/Rechtsträger, Standort, Zweckbindung und Behandlungszusammenhang getrennt geprüft; keine DB-Interna in Fehlern; alle zugeordneten Negativtests (`NVT-01`–`NVT-18`) und [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md)-Fälle (`A-*`, `D-*`, `P-*`, `L-*`, `F-*`, `B-*`, `C-*`) grün. |
| **G3** | **PostgreSQL-, Funktions-, Konkurrenz- & Ausfalltests** | Echte PostgreSQL-Integrationstests (Erfolg, `403` fehlendes Recht, `403` fehlender Behandlungszusammenhang/Mandant, Konkurrenz-/Ausfall-/Wiederanlaufpfade); keine übersprungenen oder per `#[ignore]` stillgelegten Tests. |
| **G4** | **Gesamtprüfung (`scripts/check.sh`) & CI-Jobs** | Vollständiger fehlerfreier Durchlauf von `KIS_TEST_DATABASE_URL=... scripts/check.sh` sowie der CI-Jobs (`lint`, `test`, `audit`, `docs`, `container`), inkl. `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `cargo audit`, `check_sql.py`, `check_audit_exceptions.py`. |
| **G5** | **Dokumentation & Statuswahrheit** | Paket-Abnahmebericht unter `docs/abnahme/<ID>.md`; abgestimmte Fortschreibung von [FUNKTIONSLANDKARTE.md](FUNKTIONSLANDKARTE.md), [REVIEW_STATUS.md](REVIEW_STATUS.md), [API.md](API.md), [DATABASE.md](DATABASE.md) und [KOORDINATIONSREGISTER.md](KOORDINATIONSREGISTER.md) ohne beschönigte Fertigmeldungen. |
| **G6** | **Unabhängiges Review (Pflicht bei Sicherheit/Kern)** | Unabhängige Prüfung durch eine **nicht-implementierende** Instanz für alle Änderungen an Architekturverträgen, Kern, Mandantenmodell, Auth/RBAC, Audit, DSGVO, Invarianten, Migrationen, Schnittstellen und TI. Bei relevanten Nachänderungen erneut zu prüfen. |
| **G7** | **Fach-, Betriebs- & Regulatorik-Freigabe (`EP-26`)** | Nur vor Pilot-/Produktivbetrieb: klinische Usability-/Fachfreigabe, Datenschutz-Folgenabschätzung (DPIA), Penetrationstest, Last-/HA-/Restore-Drill, MDR/IVDR-/AI-Act-Bewertung sowie erforderliche externe Bestätigungen (gematik/TI-RU, ISiK/ISiP, KBV, InEK/Grouper). |

---

## 2. Abhängigkeitsgraph und Parallelisierungsregeln (exakt nach `ENTERPRISE_BACKLOG.md`)

```text
M00: EP-00 (P0 — Ist-Analyse, ADRs 001-007, Native-First-Register, Threat Model, Testmodell, Koordination)
  |
  v  [Unabhängiges Gegenreview G6 & ADR-Freeze vor M01]
M01: P0-Plattform- & Verbund-Kernverträge (streng koordiniert; nur 1 aktiver Migrations-Schreiber!)
  +-- EP-01 (P0, ab EP-00): Mandanten, Rechtsträger, Standorte, MVZ-Kennungen, Hierarchie, Scoped Config
  |     |
  |     +--> EP-02 (P0, ab EP-01): MPI, Identitätsräume, Merge/Unmerge, temporäre IDs, Kontakte (AP-12)
  |     |      |
  |     |      +--> EP-03 (P0, ab EP-01/02): Episode, Encounter, klinischer & Abrechnungsfall, Kostenträger (AP-13)
  |     |
  |     +--> EP-04 (P0, ab EP-01): IAM/SSO, 4-Faktoren-AuthZ, Break-Glass, Lese-Audit, Anker, Retention/Legal Hold (AP-02/03/04)
  |     +--> EP-05 (P0, ab EP-01/03/04): Klasse-A-Sicherheitsinvarianten, Outbox/Inbox, Idempotenz, Quarantäne, Reconciliation
  |     +--> EP-07 (P0, ab EP-01/04/05): HA, Backup/DR, Monitoring (AP-05/06), Downtime, SBOM, Key-Rotation, Lasttest
  |
  +-- EP-06 (P0, ab EP-00 — parallel zu EP-01 startbar für Kern-Katalogschema):
        Normen-/Terminologie-/Lizenzregister, ICD/OPS (AP-14), UCUM, SNOMED/LOINC, PZN/ATC

  v  [Erst nach Freeze & grünem G0-G6 der jeweils benötigten M01-Verträge: parallele Fachstränge!]

M02 (P1 — Interoperabilität & TI-Fundament):
  +-- EP-08 (ab EP-01..06): Kommunikationsengine, FHIR/ISiK (TI10), HL7 ADT (AP-30) / ORU (AP-31) / ORM..SIU, DICOM/IHE/xDT
  +-- EP-09 (ab EP-01..06): Native TI-Primärsystem-Workflows (TI01-TI15; je Dienst separates Unterpaket + RU-Nachweis)

M03 (P1 — MVZ/PVS & Sektorübergreifende Abrechnung):
  +-- EP-10 (ab EP-03/06/08/09): MVZ/PVS (E02 — Quartal, KV/EBM, Privat, DMP, Formulare, Recall, KBV-Prüfmodule)
  +-- EP-11 (ab EP-03/06/08): Abrechnung & Medizincontrolling (E20 — Kodierung, §301/§21, DRG/PEPP, AOP/ASV/Hybrid-DRG, MD)

M04 (P1 — Klinischer Kern, Dokumentation, Medikation & Diagnostik):
  +-- EP-12 (ab EP-03/04/06): Arzt-/Pflegearbeitsplatz (E01/E03/E04, AP-11/17/40/41, DOC-01..07, Kurve, Pflegeplanung)
  +-- EP-13 (ab EP-03/06/08): Medikation/AMTS/eMAR/BCMA/Apotheke (E05)
  +-- EP-14 (ab EP-03/05/06/08): Natives LIS/POCT-Management & getrennte Blutbank/Transfusion (E06)
  +-- EP-15 (ab EP-03/08): Natives RIS/PACS (E07) & Fachdiagnostik Kardiologie/Lufu/Endoskopie (E08, AP-15)

M05 (P1 — Akutversorgung, Interdisziplinäre Versorgung & Meldewesen):
  +-- EP-16 (ab EP-03/05/08): Native Notaufnahme/Rettungsdienst (E09, ED-01..06, AP-18) & OP/AIMS/PDMS/ICU (E10, AP-19)
  +-- EP-17 (ab EP-03/04/08): Aufklärung/Einwilligung (E11), Therapie/Geriatrie/Palliativ/Entlass (E12), Onkologie/Register (E13)
  +-- EP-24 (ab EP-01..08): Externe Meldewege & Register (E22, TI14 — DEMIS, IRD, oBDS, IQTIG)

M06 (P2 — Logistik, Versorgung, Personal, QM/DMS, Portal, Wissen/BI):
  +-- EP-18 (ab EP-03/05): Transport, Bettenaufbereitung, Küche (AP-16), Technik, Facility, Dosimetrie (E14/E16)
  +-- EP-19 (ab EP-01/05/06): Einkauf, Material, native ERP/Fibu-Grundfunktionen, AEMP, Chargen/Recall (E15)
  +-- EP-20 (ab EP-01/04): Dienstplan (AP-10/42), HR, Zeitwirtschaft, Fortbildung, Recruiting (E17)
  +-- EP-21 (ab EP-01/04/05): QM, CIRS (getrennt), Intranet, DMS/Scan/OCR (DOC-06), Collaboration (E18)
  +-- EP-22 (ab EP-03/04/08/09): Patientenportal, Termine/Recall, Vertreter, PROM/PREM (E21)
  +-- EP-23 (ab EP-04/06/12): Wissens-/SOP-Suche, Diktat/optionale KI (DOC-07, E19), BI/Forschung (E23)

M07 (P1/P0 — Migration, Koexistenz & Release-Freigabeprogramm):
  +-- EP-25 (P1, ab EP-01..08): Migrations-/Koexistenzstrategie (E24 — Dry-Run, Mapping, Reconciliation, Cutover/Rollback)
  +-- EP-26 (P0/P1, alle betroffenen Pakete): Freigabeprogramm (Security-Reviews, Usability, DPIA, MDR/AI-Act, Pentest, G7)
```

**Unabhängig parallel zu `M00`/`M01` bearbeitbare Bestandshärtung (greift nicht in neue Mandantenverträge vor):**
- **`AP-09`** *(neu aus `EP-00`-Befund `G-13`, strikt getrennt von `AP-01`)*: Fehlerbehebung in `crates/kis-doctors/src/lib.rs` (`list_doctors` bindet `$1`/`$2` nicht → Laufzeitfehler `500` bei `?search=`/`?specialty=`) sowie Parametrisierung von `cursor` in `kis-doctors` und `kis-patients` mit PostgreSQL-Regressionstests.
- **`AP-01`**: Route `GET /api/v1/doctors/:id/schedule` liegt im Bestandscode bereits in `crates/kis-appointments/src/lib.rs` samt `tests/schedule.rs`; erfordert lediglich Verifikation via `scripts/check.sh` und Statusabschluss.
- **`AP-02`**, **`AP-05`**, **`AP-06`**, **`AP-08`** (`chrono` / `rustls-pemfile` Evaluierung): parallel ausführbar, sofern zentrale Ressourcen (`Cargo.toml`, `Cargo.lock`, `kis-app`, `kis-core`) über das [KOORDINATIONSREGISTER.md](KOORDINATIONSREGISTER.md) reserviert sind.

---

## 3. Feinzerlegung der P0-Blocker (`M01`: `EP-01` bis `EP-07`)

Nur **ein** aktiver Schreiber darf zeitgleich den gemeinsamen Migrationssatz (`migrations/`, `migrations/CHECKSUMS`, nächste freie Nummer: `012`) bearbeiten. Die implementierungsreifen Schema-, Rust-Trait-, API- und Testverträge für alle nachfolgenden Teilpakete (`EP-01a`–`EP-07b`) inkl. kanonischer synthetischer JSON-Fixtures liegen unter [vertraege/README.md](vertraege/README.md).

| Teilpaket | Ziel & fachlicher Umfang | Abhängigkeiten | Hauptdateien / Bereich | Pflicht-Nachweise & Gates |
|---|---|---|---|---|
| **EP-01a** | **Schema & Kernvertrag Rechtsträger/Standorte:** Migration `012` für `organization_groups`, `legal_entities`, `sites`, `org_identifiers` (IK/Standortnummer/BSNR/NBSNR/LANR/Telematik-ID), `legal_entity_id` auf `org_units` + Trigger für Zyklenfreiheit & Mandantengrenze; `TenantContext` in `kis-core`. | `EP-00` (`ADR-001`, `G6`) | `migrations/012_*`, `kis-core`, `config/module_tables.toml` | `NVT-01`, `NVT-02`; `G0`–`G6` |
| **EP-01b** | **Mandantenfähiges Org-Modul, Mehrfachzuordnungen & Service-Agreements:** `kis-org` mit strikter `legal_entity_id`-Filterung, zeitgebundenen Kanten, MVZ-Haupt-/Nebenbetriebsstätten, `devices`, `user_org_assignments` und `service_agreements` für zentrale Dienste. | `EP-01a` | `crates/kis-org`, `crates/kis-app/tests/` | `NVT-01`, `NVT-02`, `NVT-09` (Vorstufe); `G0`–`G6` |
| **EP-01c** | **Scoped Config, Vererbung & Policy-Floor:** Hierarchie `verbund_default → legal_entity → site → org_unit` für `settings`/`roles`/`platform_state`; unveränderliche Schutzklasse `policy_floor`. | `EP-01a` | `crates/kis-core/src/platform.rs`, `kis-app/src/admin_platform.rs` | `NVT-04`; `G0`–`G6` |
| **EP-02a** | **MPI-Schema, Identitätsräume & aktive AEAD-Feldverschlüsselung:** Migration für `mpi_identities`, `patient_records` (`legal_entity_id NOT NULL`), `patient_identifiers` (Namespace/OID/Provenienz), `birth_date NULL`, `identity_status` (`unknown_emergency`/`newborn_provisional`), `mpi.mode`; Umstellung `kis-patients` auf `*_enc` + `key_version` + Blind-Index. | `EP-01a` (`ADR-002`) | `migrations/`, `crates/kis-patients`, `kis-core/src/security.rs` | DB-Rohinspektionstest, `D-03`, `D-05`, `NVT-05`; `G0`–`G6` |
| **EP-02b** | **Mandantengetrennte Patientensuche, MPI-API & Kontakte (`AP-12`):** Trennung lokale Patientenakte vs. minimale MPI-Suche (mit Lese-Audit), keine globale `Scope::None`-Patientenliste; `patient_contacts` (Angehörige, Betreuung, Hausarzt, Einweiser). | `EP-02a` | `crates/kis-patients`, neues Modul/Tabellen `contacts`, Tests | `NVT-05`, `NVT-10` (Suche); `G0`–`G6` |
| **EP-02c** | **Dubletten-Arbeitsliste & auditierter Merge/Unmerge:** `mpi_match_candidates`, `mpi_merge_events`, Notfall-/Neugeborenen-Identifikation → Merge/Unmerge, Sperre über Rechtsträgergrenzen & bei kritischen Prozessen, Konkurrenz-Lock. | `EP-02b` | `crates/kis-patients`, `migrations/` | `NVT-06`, Merge-Race-Test; `G0`–`G6` |
| **EP-03a** | **Fall-, Episoden-, Abrechnungs- & Kostenträger-Schema (`AP-13`):** Migration für `care_episodes`, `clinical_cases`, `encounters`, `case_stays` (inkl. klinischer Doppelbelegungs-Constraint), `billing_cases`, `payers`, `case_coverages`, `legal_entity_id` auf Falltabellen. | `EP-01a`, `EP-02a` (`ADR-003`) | `migrations/`, `config/module_tables.toml` | Schema-/Constraint-Tests, `D-01`–`D-08`; `G0`–`G6` |
| **EP-03b** | **Sektorübergreifende Fall-API (`kis-cases` & `coverage`):** Stationär, ZNA, MVZ/KV ohne Bett, parallele Fälle, Belegarzt/Shared Care, Überweisung/Konsil, interne Verlegung vs. Rechtsträgerwechsel (Entlassung + Neuaufnahme); Mandantenfilter auf allen Suchrouten. | `EP-03a` | `crates/kis-cases`, `crates/kis-app/tests/` | `NVT-07`, `PAT-01`, `PAT-02`, `PAT-04`; `G0`–`G6` |
| **EP-03c** | **Durchreichung Mandanten- & Fallvertrag in Bestandsmodule:** Konsistentes `legal_entity_id`-Scoping in `kis-clinical`, `kis-orders`, `kis-lab`, `kis-nursing`, `kis-prescriptions`, `kis-meals`, `kis-appointments`, `kis-doctors`, `kis-medications`. | `EP-03b` | betroffene Fachmodule | Erweiterter Verbund-`hospital.rs`-Test; `G0`–`G6` |
| **EP-04a** | **Admin-/Domänentrennung, MFA-Härtung & Session/SSO (`AP-02`):** Abschaffung `admin => PermissionSet::all()`, getrennte Rollenfamilien (`platform_admin`/`tenant_admin`/`privacy_auditor`/`hr`/`finance`), AEAD-`mfa_secret`, MFA-Enrollment, reale Proxy-IP (`F-03`), `HttpOnly`-Cookie + CSRF (`AP-02`), OIDC/SAML-Schnittstelle. | `EP-01a`, `EP-01c` (`ADR-004`) | `crates/kis-core/src/rbac.rs`, `auth.rs`, `kis-app`, `static/js/` | `NVT-03`, `NVT-04`, `A-01`–`A-14`; `G0`–`G6` |
| **EP-04b** | **4-Faktoren-Zugriffsprüfung & Zentraldienst-Scoping:** Umbau `treatment.rs` & `middleware.rs`: Rechtsträgerprüfung auf `MY_UNITS`, Standort-/Einheitengrenze, `purpose_of_use`, zeitgebundene `service_agreements` (`ZD-LAB`), strikt auf `db.write()`. | `EP-01b`, `EP-03b`, `EP-04a` | `crates/kis-core/src/treatment.rs`, `kis-app/src/middleware.rs` | `NVT-08`, `NVT-09`, `A-14`; `G0`–`G6` |
| **EP-04c** | **Lese-Audit, Break-Glass-Governance & externer Prüfanker:** Pflicht-Lese-Audit (mit Mandant & Zweck, ohne PII-Volldump), Break-Glass-Frist & mandantengetrennte Nachprüfung, Audit-Partitionierung & signierter WORM-Prüfanker. | `EP-04b` | `crates/kis-core/src/audit.rs`, `kis-app`, `migrations/` | `NVT-10`, `NVT-11`, `L-01`–`L-09`; `G0`–`G6` |
| **EP-04d** | **Retention-Matrix, Legal Hold & Mandanten-DSGVO (`AP-03`/`04`):** `retention_policies` (`DEC-06`), `legal_holds`, DSGVO-Export über `db.write()` in JSON/CSV/FHIR inkl. Falldaten (`AP-03`), Freitext-Anonymisierung (`AP-04`/`DEC-08`) ohne fiktives Geburtsdatum. | `EP-03b`, `EP-04b`, `DEC-06`, `DEC-08` | `data_portability.rs`, `data_deletion.rs`, `anonymization.rs`, `gdpr_routes.rs` | `NVT-12`, `NVT-13`; `G0`–`G6` |
| **EP-05a** | **Klasse-A-Sicherheitsinvarianten & Modul-Schutzprüfung:** Atomare Invarianten / sicherer Sperrzustand in `kis-core::events`; Umstellung Bettensperre (`kis-cases`/`kis-orders`) und kritische Laborwerte (`kis-lab`); Modulabschaltung darf Klasse-A-Schutz nie deaktivieren. | `EP-01a`, `EP-03b`, `EP-04b` (`ADR-005`) | `crates/kis-core/src/events.rs`, `kis-cases`, `kis-orders`, `kis-lab` | `NVT-14`, Fehler-Injektionstest, Szenario 7; `G0`–`G6` |
| **EP-05b** | **Mandantenfähige Outbox, Inbox, Quarantäne & Reconciliation:** `legal_entity_id`, `idempotency_key`, `schema_version` auf `domain_events`; `event_inbox` (Deduplizierung, Out-of-order), `event_dead_letters`, Reconciliation-Job, Pflicht-Audit bei Replay/`resolve_event_failure`. | `EP-05a` | `migrations/`, `crates/kis-core/src/events.rs`, `admin_platform.rs` | `NVT-15`, Crash-/Retry-/Duplikat-/Out-of-order-Tests; `G0`–`G6` |
| **EP-06a** | **Normen-, Terminologie-, Katalog- & Lizenzregister (`AP-14`):** Migration & Modul `kis-catalogs` für `catalog_sources`, `catalog_versions`, `catalog_entries`, `catalog_mappings`, `normative_requirements`, `kis-admin import-catalog`, Ablaufprüfung alter Kataloge. | `EP-00` (`ADR-006`) | `migrations/`, neues Modul `kis-catalogs`, `kis-admin` | Wiederholbarer Import & Stichtagstest (synthetische Kataloge); `G0`–`G6` |
| **EP-06b** | **Stichtagsbindung, UCUM-Einheiten & Zeit-/Statusvertrag:** Anbindung `kis-clinical` (ICD/OPS), `kis-lab` (UCUM/LOINC), `kis-prescriptions` (ATC/PZN), Durchsetzung `UTC`+lokal und `preliminary/final/corrected/cancelled`. | `EP-06a`, `EP-03c` | `kis-clinical`, `kis-lab`, `kis-prescriptions` | Historische Kodier- & Einheiten-Tests; `G0`–`G6` |
| **EP-07a** | **Monitoring (`AP-05`), Schlüsselrotation, SBOM & Mandanten-Caching:** `GET /api/v1/metrics` ohne PII, `key_version`-Re-Encrypt-Werkzeug, SBOM-/Release-Pipeline, Mandanten-Präfixzwang für Caches/Jobs. | `EP-01c`, `EP-04c`, `EP-05b` | `kis-app`, `kis-core`, `.github/workflows/` | `NVT-16`, `B-07`, `C-01`–`C-04`; `G0`–`G6` |
| **EP-07b** | **HA, Backup/DR, Downtime & Verbund-Lasttest (`AP-06`):** Verbund-Backup/Restore-Drill mit definierten RTO/RPO, WAN-/Failover-Test, Read-Only-Downtime-Modus, `scripts/loadtest.py` (1 000 Betten / mehrere Standorte & MVZ, `EXPLAIN`-Nachweis). | `EP-07a` | `scripts/backup.py`, `scripts/loadtest.py`, `docs/SCALING.md` | `B-01`–`B-07`, Szenario 5 & 7; `G0`–`G6` |

---

## 4. Mapping aller Folgepakete (`EP-08`–`EP-26`, exakt nach `ENTERPRISE_BACKLOG.md`)

| Paket | Prio | Meilenstein | Abhängigkeiten | Traceability (`E*`/`TI*`/`DOC*`/`ED*`/`AP*`) | Kernumfang |
|---|---|---|---|---|---|
| **EP-08** | P1 | `M02` | `EP-01`–`06` | `E24`, `TI10`, `TI11`, `AP-30`, `AP-31` | Native Kommunikationsengine, FHIR R4 / ISiK, HL7 v2 (ADT/ORU/ORM/OML/MDM/SIU), DICOM/DICOMweb, IHE, xDT |
| **EP-09** | P1 | `M02` | `EP-01`–`06`, `DEC-04` | `E22`, `TI01`–`TI15` | Native TI-Primärsystem-Workflows (je Dienst eigenes Unterpaket + RU-Nachweis: Zugang, Identitäten/QES, VSDM, ePA, E-Rezept, KIM, eAU, TIM, NFDM/eMP, TI 2.0) |
| **EP-10** | P1 | `M03` | `EP-03`/`06`/`08`/`09` | `E02`, `TI03`/`05`/`07`/`11`/`13` | Natives MVZ/PVS: Quartal, BSNR/NBSNR/LANR, KV/EBM, Privat, DMP, Formulare, Sprechstunde, Recall, KBV-Prüfmodule |
| **EP-11** | P1 | `M03` | `EP-03`/`06`/`08`, `DEC-03` | `E20` | Stationäre & sektorübergreifende Abrechnung: Kodierung, Faktura, Regelengine, § 301/§ 21, DRG/PEPP (eigene Konformität anstreben / zertifizierter Grouper), AOP/ASV/Hybrid-DRG, MD |
| **EP-12** | P1 | `M04` | `EP-03`/`04`/`06` | `E01`, `E03`, `E04`, `DOC-01`–`05`, `AP-11`/`17`/`40`/`41` | Arzt-/Pflegearbeitsplatz, longitudinaler Verlauf, native Dokumentenplattform, Arztbrief 2.0 & bidirektionale Freigabe-Workflows (`DOC-01`–`05`), Kurve, Pflegeplanung & Assessments |
| **EP-13** | P1 | `M04` | `EP-03`/`06`/`08`, `DEC-02`/`09` | `E05`, `TI05`/`09` | Medikation/AMTS/eMAR/BCMA/Apotheke, validierte Dosis-/Interaktionsprüfung, Unit Dose, BTM, Chargen/Recall |
| **EP-14** | P1 | `M04` | `EP-03`/`05`/`06`/`08`, `DEC-09` | `E06` | Natives LIS/POCT-Management, Geräteadapter, Probenlogistik, kritische Werte (Klasse A), getrennte Blutbank/Transfusion |
| **EP-15** | P1 | `M04` | `EP-03`/`08`, `DEC-05`/`09` | `E07`, `E08`, `AP-15` | Natives RIS/PACS (Speicher, Worklist, Viewer, Archiv) & Fachdiagnostik (Kardiologie, Lufu, Endoskopie u. a.) |
| **EP-16** | P1 | `M05` | `EP-03`/`05`/`08`, `DEC-01`/`09` | `E09`, `E10`, `ED-01`–`06`, `AP-18`/`19` | Native Notaufnahme/Rettungsdienst/INZ/MANV/PLS (`ED-01`–`06`) & OP/AIMS/PDMS/ICU (je Bereich gesonderte Unterpakete) |
| **EP-17** | P1 | `M05` | `EP-03`/`04`/`08` | `E11`, `E12`, `E13` | Aufklärung/Einwilligung, Therapie/Geriatrie/Palliativ/Entlassmanagement, Onkologie/Tumorboard/Fachregister |
| **EP-18** | P2 | `M06` | `EP-03`/`05` | `E14`, `E16`, `AP-16` | Transport, Bettenaufbereitung (Klasse-A-Sperrbett), Küche/Speiseplan, Technik, Facility, Dosimetrie |
| **EP-19** | P2 | `M06` | `EP-01`/`05`/`06` | `E15` | Nativer Einkauf, Material, ERP/Fibu-Grundfunktionen, AEMP, Inventar, Chargen-Traceability bis Patient/OP, getrennte Finanzrechte |
| **EP-20** | P2 | `M06` | `EP-01`/`04` | `E17`, `AP-10`/`42` | Dienstplan (mehrere Slots), HR, Zeitwirtschaft, Fortbildung, Qualifikation, Recruiting, optionaler Payroll-Adapter (keine klinischen Daten in HR) |
| **EP-21** | P2 | `M06` | `EP-01`/`04`/`05` | `E18`, `DOC-06` | Natives QM, getrenntes CIRS, Intranet, DMS/Scan/OCR/Quarantäne (`DOC-06`), Formulare, interne Kommunikation, optionale Nextcloud/SSO-Adapter |
| **EP-22** | P2 | `M06` | `EP-03`/`04`/`08`/`09` | `E21` | Patientenportal, Termine/Recall, Vertreter, sichere Nachrichten, PROM/PREM, Nachsorge |
| **EP-23** | P2 | `M06` | `EP-04`/`06`/`12`, `DEC-09` | `E19`, `E23`, `DOC-07` | Native Wissens-/SOP-Suche, Diktat/optionale KI-Entwürfe (`DOC-07`, Prompt-Injection-Tests), BI & getrennte Forschungs-/Pseudonymisierungszone |
| **EP-24** | P1 | `M05` | `EP-01`–`08` | `E22`, `TI14` | Externe Meldewege & Register (DEMIS, IRD, Organspenderegister, Krebsregister/oBDS, IQTIG), Fristen, Quittungen, Korrektur |
| **EP-25** | P1 | `M07` | `EP-01`–`08` | `E24` | Migrations- und Koexistenzstrategie (synthetischer Dry-Run, Mapping, Reconciliation, Cutover/Rollback, Altarchiv) |
| **EP-26** | P0/P1 | `M07` | alle betroffenen Pakete | `E24`, `AP-43`, `G6`/`G7` | Release-übergreifendes Freigabeprogramm: unabhängige Security-Reviews, Usability, DPIA, MDR/AI-Act, Pentest, Betreiber- & externe Nachweise |

---

## 5. Aktueller Fortschritt (Stand: 24.11.2026)

| Meilenstein / Paket | Status | Tatsächlicher Nachweis | Offene Gates / Nächster Schritt |
|---|---|---|---|
| **M00 / `EP-00`** | **Im Review** (konsistenzbereinigt durch Autor `google-neon`) | Alle EP-00-Dokumente mit `ENTERPRISE_BACKLOG.md`, `ENTERPRISE_KIS.md`, `TI_COMPLIANCE.md` & `CLINICAL_WORKFLOWS.md` abgeglichen; `check_guardrails.py`, `check_modules.py`, `check_docs.py` = 0 Fehler ([abnahme/EP-00-konsistenzpruefung.md](abnahme/EP-00-konsistenzpruefung.md)) | **`G6` unabhängiges Gegenreview durch `hermes`/unabhängige Instanz**; `G4` vollständiges `scripts/check.sh` in Rust/PG-CI; Git-Branch & PR-Erstellung |
| **Bestandshärtung `AP-09`** | **Geplant / frei für parallele Bearbeitung (`hermes`)** | Konkreter Laufzeitfehler `G-13` in `crates/kis-doctors/src/lib.rs::list_doctors` identifiziert | Umsetzung + PostgreSQL-Tests in Rust/PG-Checkout durch `hermes` |
| **Bestandspaket `AP-01`** | **Lokal im Code vorhanden — CI-Nachweis ausstehend** | `doctor_schedule` liegt in `crates/kis-appointments/src/lib.rs` + `tests/schedule.rs`; keine Ausnahme bei `doctors` in `module_tables.toml` | Verifikation über `scripts/check.sh` in Rust/PG-Umgebung |
| **M01 / `EP-01`–`EP-07`** | **Spezifiziert** (zerlegt in `EP-01a`–`EP-07b`) | ADRs `ADR-001`–`ADR-007` und Negativtest-Katalog `NVT-01`–`NVT-18` erstellt | Freigabe `EP-00` (`G6`), danach Start `EP-01a` und parallel `EP-06a` |
| **M02–M07 / `EP-08`–`EP-26`** | **Geplant / Spezifiziert** | Eindeutige Zuordnung & Traceability bereinigt | Abhängig von Freeze der P0-Verträge (`M01`) & offenen Entscheidungen `DEC-01`–`DEC-09` |
