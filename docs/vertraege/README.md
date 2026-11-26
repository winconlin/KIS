# M01-Umsetzungsverträge & Kanonische Test-Fixtures (`EP-01` bis `EP-06`)

Stand: 25.11.2026 · Autor: `google-neon` (Architektur & Koordination) · Status: **Spezifiziert (Gegenreview `G6` zusammen mit `EP-00` durch `hermes` / unabhängige Instanz; noch keine Migrationen oder Rust-Codeänderungen eingespielt)**

Dieses Verzeichnis übersetzt die Architekturentscheidungen `ADR-001`–`ADR-007` aus [EP00_ARCHITEKTUR_UND_ANALYSE.md](../EP00_ARCHITEKTUR_UND_ANALYSE.md) in konkrete, implementierungsreife Schema-, Rust-Trait-, API- und Testverträge für den P0-Kernausbau (`M01`), damit mehrere Entwicklungsagenten nach `EP-00`-`G6`-Freigabe widerspruchsfrei und ohne Schema-Drift umsetzen können.

## Übersicht der Verträge

| Datei | Abgedeckte Pakete | Migration-Entwurf | Kerninhalt |
|---|---|---|---|
| [EP-01_MANDANTEN_UND_ORG_VERTRAG.md](EP-01_MANDANTEN_UND_ORG_VERTRAG.md) | `EP-01a`, `EP-01b`, `EP-01c` | `012_mandanten_organisation.sql` | `organization_groups`, `legal_entities`, `sites`, `org_units` + Zyklen-/Mandanten-Trigger, `org_identifiers`, `devices`, `user_org_assignments`, `service_agreements`, `TenantContext`, Scoped Config & `PolicyFloor` |
| [EP-02_EP-03_MPI_UND_FALL_VERTRAG.md](EP-02_EP-03_MPI_UND_FALL_VERTRAG.md) | `EP-02a`–`02c` (`AP-12`), `EP-03a`–`03c` (`AP-13`) | `013_mpi_und_kontakte.sql`, `014_episoden_faelle_kostentraeger.sql` | `mpi_identities`, `patient_records`, `patient_identifiers`, Merge/Unmerge mit Lock, `patient_contacts`, aktive AEAD-Verschlüsselung, `care_episodes`, `cases` (MVZ ohne Bett), `encounters`, klinische Doppelbelegungs-Constraint, `billing_cases`, `payers`, `case_coverages`, `case_transitions` |
| [EP-04_EP-05_AUTHZ_AUDIT_UND_INVARIANTEN_VERTRAG.md](EP-04_EP-05_AUTHZ_AUDIT_UND_INVARIANTEN_VERTRAG.md) | `EP-04a`–`04d` (`AP-02`/`03`/`04`), `EP-05a`–`05b` | `015_authz_audit_retention_events.sql` | Abschaffung `admin => PermissionSet::all()`, `PermissionSet([u64; 4])`, 4-Faktoren-AuthZ, mandantenbegrenztes `MY_UNITS`, Lese-Audit & WORM-Prüfanker, Retention/Legal Hold, Klasse-A-`InvariantHandler` vs. Klasse-B-Outbox/Inbox/Dead-Letter |
| [EP-06_KATALOG_NORMEN_UND_ZEITVERTRAG.md](EP-06_KATALOG_NORMEN_UND_ZEITVERTRAG.md) | `EP-06a`, `EP-06b` (`AP-14`) | `016_kataloge_normen.sql` | Neues Modul `crates/kis-catalogs/`, `catalog_sources`/`versions`/`entries`/`mappings`, `normative_requirements`, `kis-admin`-Importvertrag, `ClinicalTimestamp`/`MeasuredQuantity`/`ClinicalResultStatus` |
| [EP-07_BETRIEB_MONITORING_DOWNTIME_VERTRAG.md](EP-07_BETRIEB_MONITORING_DOWNTIME_VERTRAG.md) | `EP-07a`, `EP-07b` (`AP-05`, `AP-06`) | keine | PII-freies Monitoring (`/api/v1/metrics`), `TenantCacheKey`-Zwang, Schlüsselrotation (`kis-admin reencrypt`), Downtime-Read-Only, Verbund-Lasttest (1 000 Betten + MVZ + `EXPLAIN`) |

## Kanonische maschinenlesbare Fixtures (100 % synthetisch)

- [verbund_test_fixture.json](verbund_test_fixture.json) — Exakte JSON-Abbildung von [VERBUND_TESTMODELL.md](../VERBUND_TESTMODELL.md) (2 Rechtsträger, 4 Kliniken, 3 MVZ, `ZD-LAB`, 19 Einheiten, 15 synthetische Benutzer, `PAT-01`–`PAT-07`, `NVT-01`–`NVT-18`) als gemeinsame Datenbasis für PostgreSQL-Integrationstests ab `EP-01a`.
- [normative_requirements_seed.json](normative_requirements_seed.json) — Maschinenlesbarer Seed-Katalog für `normative_requirements` (`TI01`–`TI15`, `DOC-01`–`07`, `ED-01`–`06`) gemäß [TI_COMPLIANCE.md](../TI_COMPLIANCE.md) ohne vorweggenommene externe Bestätigungen.
