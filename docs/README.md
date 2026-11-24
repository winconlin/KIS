# KIS — Dokumentation

Stand: 24.11.2026 · in Entwicklung, nicht für den Echtbetrieb freigegeben

## Aktuell und maßgeblich

| Dokument | Inhalt |
|---|---|
| [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md) | Verbindliches Zielbild für Klinikverbünde und MVZ, Funktionsparität E01–E24 |
| [TI_COMPLIANCE.md](TI_COMPLIANCE.md) | Gematik/TI, ISiK, ISiP und externe Nachweise |
| [CLINICAL_WORKFLOWS.md](CLINICAL_WORKFLOWS.md) | Native Notaufnahme, Arztbriefredaktion, Dokumentenmanagement und Freigabeprozesse |
| [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md) | Priorisierte EP-00–26, Abhängigkeiten und Abnahme |
| [ROADMAP.md](ROADMAP.md) | Abhängigkeitsbasierte Gesamt-Roadmap (M00–M10), Quality Gates G0–G7 und Paketzerlegung |
| [EP00_ARCHITEKTUR_UND_ANALYSE.md](EP00_ARCHITEKTUR_UND_ANALYSE.md) | EP-00 Ist-Analyse, identifizierte Lücken und verbindliche ADRs (ADR-001–ADR-007) |
| [NATIVE_FIRST_REGISTER.md](NATIVE_FIRST_REGISTER.md) | Verbindliches N/H/X-Entscheidungsregister für E01–E24, TI01–TI15, DOC/ED |
| [THREAT_MODEL_VERBUND.md](THREAT_MODEL_VERBUND.md) | Bedrohungsmodell für Klinikverbund, MVZ, zentrale Dienste und Sicherheitsinvarianten |
| [VERBUND_TESTMODELL.md](VERBUND_TESTMODELL.md) | Synthetisches Verbund-Testmodell und Pflicht-Negativtestkatalog (NVT-01–NVT-16) |
| [ABNAHMEBERICHT.md](ABNAHMEBERICHT.md) | Verbindliche Abnahmebericht-Vorlage (G0–G7) und Berichtsverzeichnis |
| [FUNKTIONSLANDKARTE.md](FUNKTIONSLANDKARTE.md) | Was je Arbeitsbereich vorhanden ist und was fehlt |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Kern, Module, Weg einer Anfrage, Ereignisse |
| [MODULE.md](MODULE.md) | Regeln und Anleitung für Module |
| [ARBEITSPAKETE.md](ARBEITSPAKETE.md) | Offene Arbeit in einzeln bearbeitbaren Paketen |
| [API.md](API.md) | Alle Routen mit Rechten (aus dem System erzeugt) |
| [DATABASE.md](DATABASE.md) | Schema, Integritätsregeln, Migrationen |
| [SECURITY.md](SECURITY.md) | Umgesetzte Maßnahmen, Grenzen, offene Anforderungen |
| [CONFIGURATION.md](CONFIGURATION.md) | Konfigurationsdatei, Umgebung, Einstellungen zur Laufzeit |
| [INSTALLATION.md](INSTALLATION.md) | Toolchain, Bootstrap, systemd, Docker |
| [OPERATION.md](OPERATION.md) | Health, Logs, Backup und Restore |
| [SCALING.md](SCALING.md) | Ressourcenbedarf, mehrere Instanzen, große Häuser |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Struktur, Prüfungen, Arbeitsablauf |
| [REVIEW_STATUS.md](REVIEW_STATUS.md) | Was geprüft ist und was nicht |
| [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md) | Sicherheitsfälle und Nachweise |
| [AGENTEN_AUFTRAG.md](AGENTEN_AUFTRAG.md) | Aktueller Auftrag für Agenten |

Agenten beginnen mit [AGENTS.md](../AGENTS.md).

## Älter, nur zur Einordnung

Diese Dokumente stammen aus früheren Planungsständen. Sie beschreiben
Zielbilder und Entwürfe, nicht den Ist-Zustand, und sind entsprechend
gekennzeichnet.

| Dokument | Inhalt |
|---|---|
| [FAHRPLAN.md](FAHRPLAN.md) | Früherer Gesamtfahrplan |
| [INTERFACES.md](INTERFACES.md) | Entwürfe für HL7, FHIR, DICOM, XDS (Code unter `entwurf/`, nicht im Build) |
| [CGM_MEDICO.md](CGM_MEDICO.md) | Entwurf eines MEDICO-Clients und Datenmapping |
