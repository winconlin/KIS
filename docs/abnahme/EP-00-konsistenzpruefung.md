# Abnahme- und Konsistenzprüfbericht — `EP-00` (`M00`: Abgleich, Bereinigung & Koordinationsregister)

- **Arbeitspaket / Meilenstein:** `EP-00` / `M00` (Architektur-, Konsistenz- & Koordinationsbereinigung)
- **Fachlicher Umfang:** Kritische Konsistenzprüfung und Bereinigung sämtlicher `EP-00`-Architekturdokumente (`ROADMAP.md`, `EP00_ARCHITEKTUR_UND_ANALYSE.md`, `NATIVE_FIRST_REGISTER.md`, `THREAT_MODEL_VERBUND.md`, `VERBUND_TESTMODELL.md`, `ABNAHMEBERICHT.md`) gegen die verbindlichen Auftragsdokumente (`ENTERPRISE_BACKLOG.md`, `ENTERPRISE_KIS.md`, `TI_COMPLIANCE.md`, `CLINICAL_WORKFLOWS.md`, `ARBEITSPAKETE.md`, `SICHERHEITS_ABNAHME.md`) sowie Einrichtung des zentralen Koordinationsregisters (`KOORDINATIONSREGISTER.md`).
- **Datum:** 24.11.2026
- **Agentenidentität (Bearbeiter & Koordinator):** `google-neon`
- **Ausdrückliche Unabhängigkeits-Erklärung (Pflicht):** Die ursprünglichen `EP-00`-Dokumente in diesem Arbeitsbereich wurden ebenfalls von `google-neon` erstellt. Diese Prüfung ist daher ausdrücklich eine **Konsistenz-, Abgleichs- und Selbstprüfung durch den Autor** und **KEIN unabhängiges `G6`-Review**. Das unabhängige Gegenreview (`G6`) wird verbindlich **`hermes` (bzw. einer anderen unabhängigen Prüfinstanz)** zugewiesen (`EP-00-REV` in [KOORDINATIONSREGISTER.md](../KOORDINATIONSREGISTER.md)).
- **Branch / Pull Request:** Vorgesehener Branch `agent/google-neon/ep00-architecture-coordination` · PR *offen* (Workspace-Snapshot unter `/app/applet` enthält kein `.git`-Verzeichnis und kein `gh`-CLI; Übergabe-Vorlage siehe Abschnitt 6).
- **Basis-SHA (`origin/main`):** *nicht ermittelbar (Snapshot ohne `.git`)*
- **Ergebnis-SHA:** *offen bis Commit/Push in Git-Checkout*
- **Unabhängiger Reviewer (`G6`):** **offen — zugewiesen an `hermes` / unabhängige Prüfinstanz**

---

## 1. Gefundene und bereinigte Widersprüche (Vollständiges Audit-Protokoll)

Während des systematischen Abgleichs zwischen den neuen `EP-00`-Dokumenten (v1) und den verbindlichen Bestandsdokumenten wurden **sieben wesentliche Widerspruchsklassen** identifiziert und ohne Entfernung oder Abschwächung bestehender Anforderungen vollständig bereinigt:

| Nr. | Fundstelle (v1) | Festgestellter Widerspruch / Mangel | Bereinigung (v2) |
|---|---|---|---|
| **K-01** | `ROADMAP.md`, `NATIVE_FIRST_REGISTER.md`, `EP00_ARCHITEKTUR_UND_ANALYSE.md`, `THREAT_MODEL_VERBUND.md` | **Schwere ID-Kollision bei `EP-08` bis `EP-26`:** `ROADMAP.md` (v1) belegte `EP-08`–`EP-26` völlig anders als das verbindliche [ENTERPRISE_BACKLOG.md](../ENTERPRISE_BACKLOG.md) (z. B. war `EP-08` als Aufnahme statt Kommunikationsengine/FHIR/ISiK/HL7, `EP-09` als Arztbrief statt TI, `EP-12` als ZNA statt Arzt-/Pflegearbeitsplatz/`DOC-01`–`07`, `EP-26` als UI statt Release-Freigabeprogramm geführt). | Alle `EP-00`–`EP-26`-Zuordnungen, Prioritäten (`P0`/`P1`/`P2`) und Abhängigkeiten in sämtlichen `EP-00`-Dokumenten 1:1 auf [ENTERPRISE_BACKLOG.md](../ENTERPRISE_BACKLOG.md) ausgerichtet. |
| **K-02** | `NATIVE_FIRST_REGISTER.md` | **Schwere ID-Kollision bei `E01` bis `E24`:** v1 nummerierte Verbund/MPI/Fallmodell als `E01`–`E03`, wodurch alle nachfolgenden `E`-IDs gegenüber [ENTERPRISE_KIS.md](../ENTERPRISE_KIS.md) § 3 verschoben waren und Bereiche wie `E11` (E-ConsentPro), `E13` (Onkologie/Register), `E15` (Einkauf/AEMP/ERP), `E18` (QM/CIRS/DMS), `E19` (Wissenssuche/KI) und `E21` (Patientenportal) fehlten oder falsch lagen. | `NATIVE_FIRST_REGISTER.md` exakt nach `ENTERPRISE_KIS.md` § 3 (`E01`–`E24`) neu aufgebaut; Querschnitts-Blocker (`ENTERPRISE_KIS.md` § 2) sauber als eigene Tabelle `EP-00`–`EP-07` geführt. |
| **K-03** | `NATIVE_FIRST_REGISTER.md` | **ID-Verschiebung bei `TI02` bis `TI15`:** v1 vertauschte u. a. `TI02`/`TI03` (Identitäten/Signatur vs. VSDM), `TI04`–`TI06` (ePA/E-Rezept/KIM), `TI08`–`TI15` (TIM, NFDM/eMP, ISiK, ISiP, TI 2.0, eFormulare, Register/Meldewesen, EHDS) gegenüber [TI_COMPLIANCE.md](../TI_COMPLIANCE.md). | Abschnitt 4 von `NATIVE_FIRST_REGISTER.md` 1:1 auf `TI01`–`TI15` aus [TI_COMPLIANCE.md](../TI_COMPLIANCE.md) bereinigt. |
| **K-04** | `EP00_ARCHITEKTUR_UND_ANALYSE.md`, `ROADMAP.md`, `THREAT_MODEL_VERBUND.md` | **`AP-01`-Kollision & fehlendes `AP-08`:** `AP-01` (`GET /api/v1/doctors/:id/schedule` in `kis-appointments`) ist laut Code (`crates/kis-appointments/src/lib.rs:289`, `tests/schedule.rs`, `config/module_tables.toml`) bereits im Bestandscode umgesetzt (CI-Nachweis offen). v1 hatte `AP-01` fälschlich für den neu gefundenen `list_doctors`-SQL-Bind-Bug (`G-13`) wiederverwendet und `AP-08` (ungewartete Crates `chrono`/`rustls-pemfile`) ausgelassen. | `AP-01` sauber vom neuen Fehler getrennt; `G-13` als eigenes unabhängiges Härtungspaket **`AP-09`** ausgewiesen; `AP-08` (`G-14`) sowie sämtliche `AP-02`–`AP-43`-Zuordnungen in `ROADMAP.md` und `KOORDINATIONSREGISTER.md` aufgenommen. |
| **K-05** | `EP00_ARCHITEKTUR_UND_ANALYSE.md` § A.1 | **Faktische Ungenauigkeiten in der Bestandszählung:** v1 nannte „13 Crates“ statt exakt **16 Workspace-Crates** ([Cargo.toml](../../Cargo.toml)) und verwendete teils abweichende Tabellennamen (`anonymized_patients` statt `anonymization_keys`, `case_care_team`/`break_glass_grants` statt `case_participants`/`emergency_access_grants`, `fluid_balance`/`medication_orders`/`case_diet_orders`/`staff_profiles` statt exakter Namen aus `config/module_tables.toml`). | Alle 16 Crates und alle 49 Tabellen exakt gegen `Cargo.toml` und `config/module_tables.toml` abgeglichen. |
| **K-06** | `ADR-001`–`ADR-006` | **Fehlende Details aus `ENTERPRISE_KIS.md` § 2/§ 4 & `TI_COMPLIANCE.md`:** Es fehlten explizite Vertragsbestandteile für Betriebsgesellschaft, virtuelle Einheiten, Kostenstellen, Geräte, Zyklenfreiheit, Mehrfach-Arbeitsverhältnisse (`ADR-001`); Neugeborene (`newborn_provisional`), getrennte Identitätsräume als Betriebsoption, `AP-12`-Kontakte (`ADR-002`); klinische Doppelbelegung, Belegarzt/Shared Care, `AP-13`-Kostenträger (`ADR-003`); IAM/SSO, HR-/Finanz-/Forschungs-Trennung, Schlüsselrotation (`ADR-004`); Deduplizierung statt „exactly-once“, Verbot der Schutz-Abschaltung bei Moduldeaktivierung (`ADR-005`); maschinenlesbares Normenregister, UTC+lokal, Originaleinheit+UCUM, Ereignis-/Erfassungs-/Freigabezeit (`ADR-006`). | Alle sieben ADRs vollständig ergänzt, ohne bestehende Anforderungen abzuschwächen. |
| **K-07** | `THREAT_MODEL_VERBUND.md`, `VERBUND_TESTMODELL.md`, `ABNAHMEBERICHT.md` | **Ergänzungsbedarf bei Bedrohungen, Testszenarien & Status-Taxonomie:** `THREAT_MODEL_VERBUND.md` deckte DMS/OCR/Signatur-Wiederverwendung, BCMA/Blutbank, CIRS-Trennung, KI-Prompt-Injection, ZNA-Warteraumanzeige und Downtime noch nicht ab (`T19`–`T24`); `VERBUND_TESTMODELL.md` enthielt noch kein Neugeborenen-Szenario (`PAT-07`) und keine `NVT-17`/`NVT-18`; `ABNAHMEBERICHT.md` führte noch nicht die volle Status-Taxonomie und die Nicht-Selbstreview-Regel. | `T19`–`T24`, `PAT-07`, `NVT-17`/`NVT-18`, Bezug auf alle 8 Abnahmeszenarien, volle Status-Taxonomie sowie das zentrale `KOORDINATIONSREGISTER.md` ergänzt. |

---

## 2. Durchgeführte Änderungen (ausschließlich Architektur, Koordination & Dokumentation)

- **Kein Rust-Code (`crates/`) und keine Migrationen (`migrations/`) verändert.**
- Geänderte / neue Dateien:
  - `docs/KOORDINATIONSREGISTER.md` *(neu)*
  - `docs/abnahme/EP-00-konsistenzpruefung.md` *(neu)*
  - `docs/ROADMAP.md` *(konsistenzbereinigt v2)*
  - `docs/EP00_ARCHITEKTUR_UND_ANALYSE.md` *(konsistenzbereinigt v2)*
  - `docs/NATIVE_FIRST_REGISTER.md` *(konsistenzbereinigt v2)*
  - `docs/THREAT_MODEL_VERBUND.md` *(konsistenzbereinigt v2)*
  - `docs/VERBUND_TESTMODELL.md` *(konsistenzbereinigt v2)*
  - `docs/ABNAHMEBERICHT.md` *(aktualisiert v2)*
  - `docs/abnahme/EP-00.md` *(Verweis auf v2-Bereinigung ergänzt)*
  - `docs/README.md`, `docs/FUNKTIONSLANDKARTE.md`, `docs/REVIEW_STATUS.md` *(Status & Index aktualisiert)*

---

## 3. Tatsächlich ausgeführte Prüfungen (24.11.2026)

| Prüfung | Befehl | Status | Tatsächliches Ergebnis |
|---|---|---|---|
| Leitplanken-Check | `python3 scripts/check_guardrails.py` | **bestanden** | `Leitplanken geprüft, 0 Verstöße` |
| Modulgrenzen-Check | `python3 scripts/check_modules.py` | **bestanden** | `49 Tabellen, 11 Fachmodule geprüft, 0 Verstöße` |
| Dokumentations- & Link-Check | `python3 scripts/check_docs.py` | **bestanden** | `0 Fehler` (alle Markdown-Dokumente, lokalen Links, JSON-, TOML- und Shell-Codeblöcke geprüft) |
| Preview-Server Build & Lint | `npm run build` / `npm run lint` | **bestanden** | `node --check server.mjs` fehlerfrei |
| Git / GitHub-CLI | `git status -sb`, `command -v gh` | **blockiert / nicht verfügbar** | Kein `.git`-Verzeichnis im Snapshot; `gh` nicht installiert |
| Rust / PostgreSQL / Audit / Container (`scripts/check.sh` gesamt) | `KIS_TEST_DATABASE_URL=... scripts/check.sh` | **blockiert / nicht ausführbar** | `cargo`, `rustc`, `psql`, `postgres`, `docker`, `age` und `Cargo.lock` fehlen in dieser Container-Umgebung |

---

## 4. Quality-Gate-Checkliste (`G0`–`G7`)

- [x] **G0:** Auftrag, ID-Konsistenz, Native-First-Register und Koordinationsregister vollständig abgeglichen.
- [x] **G1:** `check_guardrails.py` & `check_modules.py` = 0 Verstöße; keine Änderungen an `crates/` oder `migrations/`.
- [x] **G2 (Spezifikation):** Bedrohungsmodell (`T01`–`T24`) und Negativtest-Katalog (`NVT-01`–`NVT-18`) vollständig mit `SICHERHEITS_ABNAHME.md` verknüpft.
- [ ] **G3:** PostgreSQL-Tests nicht betroffen in reinem Doku-Paket, aber im Container nicht ausführbar — **offen für CI**.
- [ ] **G4:** Vollständiges `scripts/check.sh` & CI-Jobs wegen fehlender Rust-/PG-Toolchain im Container **offen**.
- [x] **G5:** Dokumentation, Index, Funktionslandkarte, Prüfstatus und Koordinationsregister konsistent aktualisiert (`check_docs.py` = 0 Fehler).
- [ ] **G6:** **Offen — ausdrücklich zugewiesen an `hermes` (oder unabhängige Prüfinstanz).** Keine Selbstfreigabe durch `google-neon`.
- [ ] **G7:** **Offen** — Hausentscheidungen `DEC-01`–`DEC-09` sowie spätere Fach-/Betriebs-/Regulatorikfreigaben stehen aus.

---

## 5. Offene Architekturentscheidungen & Blocker vor `EP-01`

1. **Unabhängiges `G6`-Gegenreview (`EP-00-REV`):** Prüfung von `ADR-001`–`ADR-007`, `NATIVE_FIRST_REGISTER.md`, `THREAT_MODEL_VERBUND.md`, `VERBUND_TESTMODELL.md`, `ROADMAP.md` und `KOORDINATIONSREGISTER.md` durch `hermes` oder einen menschlichen Architektur-/Sicherheitsreviewer.
2. **Anlage von Git-Branch, PR und GitHub-Koordinations-Issue:** Da dieser Container nur einen Snapshot ohne `.git`/`gh` bereitstellt, müssen die Änderungen auf einem Git-/GitHub-Host auf Branch `agent/google-neon/ep00-architecture-coordination` eingespielt, das Koordinations-Issue über `docs/KOORDINATIONSREGISTER.md` erstellt und als **Draft Pull Request** eingereicht werden.
3. **Offene Haus- und Fachkonzeptentscheidungen (`DEC-01`–`DEC-09`):** Müssen vor Beginn der jeweiligen abhängigen Pakete (`EP-04d`, `EP-09`, `EP-11`, `EP-13`, `EP-15`, `EP-16`, `EP-23`, `EP-26`) geklärt werden; blockieren **nicht** den Start von `EP-01a`, `EP-06a` oder der unabhängigen Bestandskorrektur `AP-09`.

---

## 6. Vorbereitete Pull-Request-Beschreibung (für Übernahme in Git-Host)

```text
Titel: [EP-00] Architektur-Baseline, ID-Bereinigung (EP/E/TI/AP) und zentrales Multi-Agenten-Koordinationsregister (Draft)
Branch: agent/google-neon/ep00-architecture-coordination
Agent: google-neon (Architektur & Koordination)
Arbeitspaket: EP-00 (M00) — Ist-Analyse, ADRs 001-007, Native-First-Register, Threat Model, Verbund-Testmodell, Roadmap & Koordinationsregister

Zusammenfassung:
- Bereinigt sämtliche ID-, Abhängigkeits- und Scope-Widersprüche zwischen ROADMAP.md, NATIVE_FIRST_REGISTER.md, EP00_ARCHITEKTUR_UND_ANALYSE.md, THREAT_MODEL_VERBUND.md, VERBUND_TESTMODELL.md und den verbindlichen Auftragsdokumenten (ENTERPRISE_BACKLOG.md, ENTERPRISE_KIS.md, TI_COMPLIANCE.md, CLINICAL_WORKFLOWS.md, ARBEITSPAKETE.md).
- Richtet docs/KOORDINATIONSREGISTER.md als alleinige Vorlage/Referenz für das zentrale GitHub-Koordinations-Issue ein.
- Belässt Rust-Code (crates/) und Migrationen (migrations/) unverändert.

Unabhängiges Review (G6):
- Da die ursprünglichen EP-00-Dokumente von google-neon stammen, erfolgt KEINE Selbstfreigabe.
- Unabhängiges Gegenreview (G6) angefordert bei: hermes (oder unabhängigem Architektur-/Sicherheitsreviewer).

Prüfungen:
- Lokal bestanden: check_guardrails.py (0), check_modules.py (49 Tabellen / 11 Fachmodule / 0), check_docs.py (0 Fehler).
- Offen für CI: vollständiges scripts/check.sh (Rust/PostgreSQL/Cargo.lock), GitHub-CI-Jobs (lint, test, audit, docs, container).

Abnahmebericht: docs/abnahme/EP-00-konsistenzpruefung.md
```
