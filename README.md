# KIS — Native Healthcare Enterprise Platform

Ein modulares Krankenhaus- und Praxisinformationssystem in Rust und PostgreSQL. Ziel ist ein weitgehend eigenständiges Gesamtsystem für alle Berufsgruppen: vom MVZ und der Einzelpraxis bis zum Maximalversorger und Klinikverbund mit mehreren Standorten, Rechtsträgern, Schwesterkliniken und gemeinsamen Diensten.

**Entwicklungsstand:** ausschließlich Entwicklungs- und Testinstallationen mit synthetischen Daten. Nicht für den Echtbetrieb freigegeben; keine Behauptung einer gematik-/KBV-Bestätigung oder Medizinproduktekonformität.

[Entwicklungsroadmap](docs/ROADMAP.md) · [Funktionslandkarte (Ist)](docs/FUNKTIONSLANDKARTE.md) · [Enterprise-Zielbild (Soll)](docs/ENTERPRISE_KIS.md) · [Arbeitspakete](docs/ENTERPRISE_BACKLOG.md) · [CI](https://github.com/winconlin/KIS/actions)

## Vision: Native First

- **Vollständige Arbeitsabläufe statt Einzellösungen.** Stationär, ambulant, teilstationär, Notfall, OP, Intensivmedizin und sektorenübergreifende Versorgung auf einer gemeinsamen, berechtigungsgesteuerten Plattform.
- **Eigene Fachmodule statt Pflicht-Fremdsoftware.** Native UI, API, Datenhaltung, Workflow, Freigabe, Audit und Export; offene Standards und optionale Adapter für Koexistenz, Migration und Geräte. Technisch, rechtlich oder regulatorisch notwendige externe Komponenten sind ausdrücklich begründete Ausnahmen.
- **Verbundfähig und konfigurierbar.** Mandanten-/Rechtsträgergrenzen, Master Patient Index, Fall- und Kontaktmodell, gemeinsame Dienste, Rollen, Standort-/Fachbereichsvorlagen und lokale Regelwerke sind verbindliche Ausbauziele, nicht vollständig implementierte Ist-Funktionen.
- **Modular und ressourcenschonend.** Rust-Workspace, PostgreSQL, kleine Kernverträge, unabhängige Crates, Ereignisverarbeitung; optionale Folgeaktionen isolieren, sicherheitskritische Invarianten atomar sichern oder in einem sicheren Zustand blockieren. Externe Dienste nur bei begründetem Bedarf. [Architektur](docs/ARCHITECTURE.md) · [Module](docs/MODULE.md) · [Skalierung](docs/SCALING.md)
- **Sicherheit und Nachweisbarkeit.** Rechte, Behandlungszusammenhang, Audit, sichere Migration, unabhängiges Review und dokumentierte fachliche/regulatorische Gates; keine Selbstzertifizierung.

## Funktionsumfang

Die rechte Spalte beschreibt das **Ziel**, nicht die bereits verfügbare Funktionalität. Details, native Ausnahmen und Nachweise in den verlinkten Spezifikationen.

| Domäne | Geplanter nativer Umfang |
|---|---|
| Verbund / Patientenservice | MPI, getrennte Rechtsträger, Standorte, MVZ, Aufnahme/Verlegung/Entlassung, Versicherungen, Termine, Betten |
| Klinische Akte | Ärztliche und pflegerische Dokumentation, Kurven, Assessments, Aufträge, Übergaben, Konsile |
| Arztbrief 2.0 / DMS | Strukturierte Befundaggregation, WYSIWYG, Vorlagen, Kommentare, Versionsvergleich, AA↔OA↔CA↔Sekretariat, Signatur, Versand, Archiv, Scan/OCR/Suche |
| Notaufnahme | epias-ähnlicher nativer Arbeitsplatz: Voranmeldung, Triage/Re-Triage, Live-Board, Lageplan, Schockraum, INZ, MANV, Disposition, Nachbefunde |
| Fachdiagnostik | LIS/POCT/Blutbank, RIS/PACS/DICOM, Kardiologie, EKG/Langzeit-EKG, Echo, HKU/EP, Lungenfunktion und weitere Fachbereiche |
| Therapie | CPOE/AMTS/eMAR/BCMA, Apotheke, OP/AIMS/PDMS, Intensivmedizin, Geriatrie, Palliativ, Onkologie, Entlassmanagement |
| MVZ/PVS und Abrechnung | Quartalsfälle, EBM/KVDT, GOÄ, DRG/PEPP, §301/§21, AOP/ASV/Hybrid-DRG, Kodierung/Controlling |
| TI und Interoperabilität | Gematik-Primärsystemworkflows, ePA/eRezept/eAU/KIM/VSDM, ISiK/ISiP, HL7/FHIR/DICOM/IHE/xDT, Register; externe Pflichtkomponenten separat |
| Krankenhausorganisation | Transport, Küche, Hauswirtschaft, Technik, AEMP, Einkauf/ERP/Fibu, HR/Dienstplan/Zeitwirtschaft, QM/CIRS, Intranet, Projekte und Formulare |
| Patientenportal, Wissen und KI | Sichere Kommunikation, Termine, PROM/PREM, SOP/Wissen, Diktat/KI-Entwürfe mit menschlicher Freigabe, BI/Forschung |
| Betrieb und Migration | On-Premises, Backup/Restore, HA/Downtime, sichere Migration, versionierte Kataloge, Export, Monitoring |

Siehe [klinische Workflows](docs/CLINICAL_WORKFLOWS.md), [TI/Gematik](docs/TI_COMPLIANCE.md) und [Enterprise-Zielbild](docs/ENTERPRISE_KIS.md).

## Aktuell vorhandene Basis

Laut Funktionslandkarte/Prüfstatus sind die folgenden Grundfunktionen vorhanden; keine vollständige Fachsystemparität oder Produktionsfreigabe.

| Bereich | Module |
|---|---|
| Aufbau des Hauses, Zimmer, Betten | `org` |
| Patientenstamm, Warnhinweise | `patients` |
| Aufnahme, Verlegung, Entlassung, Stationsliste, Bettenübersicht | `cases` |
| Diagnosen, Prozeduren, Dokumentation, Aufgaben, Übergabe | `clinical` |
| Leistungskatalog, Anforderungen, Arbeitslisten, Automatik | `orders` |
| Laborbefunde, Verlauf, kritische Werte | `lab` |
| Vitalwerte, Flüssigkeitsbilanz | `nursing` |
| Verordnung, Antiinfektiva-Kurse, Gabedokumentation | `prescriptions` |
| Kostformen, Produktionsliste der Küche | `meals` |
| Dienstplan, Prüfung, Wochenbesetzung, Export | `roster` |
| Termine, Behandler, Dauermedikation | `appointments`, `doctors`, `medications` |
| Weboberfläche | `ui` |
| Anmeldung, Rollen, Einstellungen, Audit, Auskunft, Löschung | Kern |

Schnittstellenentwürfe unter `entwurf/` sind nicht Teil des ausführbaren Builds. Der Workspace unter `crates/` ist maßgeblich. Details und Grenzen: [Funktionslandkarte](docs/FUNKTIONSLANDKARTE.md), [Prüfstatus](docs/REVIEW_STATUS.md).

## Entwicklungsroadmap

| Phase | Ziel |
|---|---|
| M00–M01 | Architektur, Sicherheitsbaseline, Mandant, MPI, Fallmodell, Rechte, Ereignisse, Kataloge |
| M02 | Betrieb, Resilienz, native Kommunikationsengine und Standardprofile |
| M03 | Klinischer Arbeitsplatz, native Dokumenten-/Workflow-Engine und Arztbriefredaktion |
| M04–M05 | Medikation, Labor, Fachdiagnostik, Notaufnahme, OP und PDMS |
| M06–M07 | MVZ/PVS, Abrechnung, gematik/TI, Register und externe Nachweise |
| M08–M09 | Fachbereiche, Organisation, Portal, Wissen, BI und optionale KI |
| M10–M11 | Gesamtintegration, Migration, Pilotvorbereitung und getrennte Betreiber-/Fach-/Regulatorikfreigaben |

Alle neuen Meilensteine sind offen, bis die konkreten Nachweise vorliegen. Abhängigkeiten und Parallelisierung, 8 Quality Gates (G0–G7), unabhängige Reviews, Status- und Release-Regeln: [ROADMAP.md](docs/ROADMAP.md). Pro Paket gilt die [Abnahmebericht-Vorlage](docs/ABNAHMEBERICHT.md). Implementierungsdetails und Paket-IDs stehen im [Enterprise-Backlog](docs/ENTERPRISE_BACKLOG.md), [AP-Backlog](docs/ARBEITSPAKETE.md) und [klinischen Teilpaketen](docs/CLINICAL_WORKFLOWS.md).

## Schnellstart (Entwicklung)

```bash
cp .env.example .env && chmod 600 .env     # Secrets eintragen (openssl rand …)
docker compose up -d db
docker compose run --rm --entrypoint kis-admin kis migrate
docker compose run --rm --entrypoint kis-admin kis create-user admin admin@example.test "Admin" admin
docker compose up -d kis
curl -fsS http://127.0.0.1:8080/health
```

Ohne Docker und alle Details: [Installation](docs/INSTALLATION.md).
Oberfläche: `http://127.0.0.1:8080/`, tägliche Arbeitsansicht unter
„Arbeitsplatz“.

## Entwicklung und Qualitätssicherung

Ein Paket, ein Branch, synthetische Daten, keine stillen Ausnahmen. Agenten und Mitwirkende beginnen mit [AGENTS.md](AGENTS.md) und [ROADMAP.md](docs/ROADMAP.md).

```bash
KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh
```

Das Skript prüft Format, Leitplanken, Modulgrenzen, Dokumentation, Audit-Ausnahmen, Clippy, Unit-/echte PostgreSQL-Tests und SQL-Schema. Die [CI](.github/workflows/ci.yml) prüft zusätzlich `cargo audit`, Build, Backup/Restore, DB-Laufzeitrechte und Container-Smoke-Test. Alle relevanten Nachweise müssen auf dem betreffenden Commit vorliegen; kein Überspringen, keine Audit-Abschwächung. Technische Integration, fachliche Abnahme, externe Bestätigung und Betriebsfreigabe sind getrennte Entscheidungen.

## Dokumentation

| Für | Dokument |
|---|---|
| Ziel und Planung | [Enterprise](docs/ENTERPRISE_KIS.md), [Roadmap](docs/ROADMAP.md), [TI](docs/TI_COMPLIANCE.md), [Klinische Workflows](docs/CLINICAL_WORKFLOWS.md) |
| Ist und Nachweise | [Funktionslandkarte](docs/FUNKTIONSLANDKARTE.md), [Prüfstatus](docs/REVIEW_STATUS.md), [Abnahmebericht](docs/ABNAHMEBERICHT.md) |
| Mitarbeit | [Module](docs/MODULE.md), [Arbeitspakete](docs/ARBEITSPAKETE.md), [Enterprise-Backlog](docs/ENTERPRISE_BACKLOG.md), [Agentenregeln](AGENTS.md) |
| Technik | [Architektur](docs/ARCHITECTURE.md), [API](docs/API.md), [Datenbank](docs/DATABASE.md), [Sicherheit](docs/SECURITY.md), [Sicherheitsabnahme](docs/SICHERHEITS_ABNAHME.md) |
| Betrieb | [Installation](docs/INSTALLATION.md), [Konfiguration](docs/CONFIGURATION.md), [Betrieb](docs/OPERATION.md), [Skalierung](docs/SCALING.md) |
| Alles | [Dokumentationsübersicht](docs/README.md) |

## Lizenzangabe

`Cargo.toml` nennt MIT. Eine vollständige `LICENSE`-Datei mit den vom
Rechteinhaber festgelegten Angaben ist noch zu ergänzen.
