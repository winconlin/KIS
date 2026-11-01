# KIS — Dokumentation

> **Krankenhaus-Informations-System (KIS)**
> Version 0.1.0 · Lizenz MIT · Sprache: Deutsch

Dieses Dokument ist das zentrale Verzeichnis (Index) für die vollständige Dokumentation des KIS-Projekts. Alle Dokumente befinden sich im Verzeichnis `docs/` des Repositories und sind in professionellem, medizinisch-technischem Deutsch verfasst.

## Über das Projekt

Das KIS ist ein minimalistisches, performantes Krankenhaus-Informations-System, entwickelt in [Rust](https://www.rust-lang.org/). Es ist bewusst auf minimale Hardware-Anforderungen und maximale Datenintegrität ausgelegt und adressiert die besonderen Anforderungen an Datenschutz, Sicherheit und Interoperabilität im klinischen Umfeld.

| Eigenschaft | Wert |
|---|---|
| Sprache | Rust (Edition 2021) |
| Web-Framework | Axum 0.7 |
| Datenbank | SQLite (WAL-Modus), Zugriff über SQLX 0.7 |
| Runtime | Tokio (asynchron) |
| Standard-Port (HTTP) | 8080 |
| Medizinschnittstellen | HL7 v2.5 · FHIR R4 · DICOM · XDS |
| Lizenz | MIT |

## Verzeichnis der Dokumente

| Dokument | Inhalt | Status |
|---|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Systemarchitektur, Komponentenübersicht, Schnittstellen- und LLM-Integration | ✅ Vorhanden |
| [INSTALLATION.md](INSTALLATION.md) | Schritt-für-Schritt-Installation, Build, systemd, Docker, Hardware-Anforderungen | ✅ Neu |
| [CONFIGURATION.md](CONFIGURATION.md) | Vollständiger Konfigurationsreferenz, ENV-Overrides, Sicherheitseinstellungen | ✅ Neu |
| [SECURITY.md](SECURITY.md) | Datenschutz, Zugriffskontrolle, Audit-Logging, Netzwerk, Compliance, Threat Model | ✅ Neu |
| [API.md](API.md) | Komplette REST-API-Referenz, Endpunkte, Beispiele, Fehlercodes | ✅ Neu |
| [INTERFACES.md](INTERFACES.md) | Medizinische Schnittstellen: HL7 v2.5, FHIR R4, DICOM, XDS | ✅ Neu |
| [DATABASE.md](DATABASE.md) | Datenbank-Schema, ER-Diagramm, Indizes, WAL, Backup, Migrationen | ✅ Neu |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Entwicklerhandbuch, Projektstruktur, Konventionen, Tests, LLM-Workflow, CI/CD | ✅ Neu |
| [OPERATION.md](OPERATION.md) | Betriebshandbuch: Monitoring, Logging, Backups, Upgrades, Troubleshooting | ✅ Neu |

## Leseempfehlung

| Rolle | Empfohlene Reihenfolge |
|---|---|
| **Betrieb / IT-Administrator** | INSTALLATION → OPERATION → SECURITY → CONFIGURATION |
| **Entwickler** | ARCHITECTURE → DEVELOPMENT → API → DATABASE → INTERFACES |
| **Datenschutzbeauftragte(r) / Compliance** | SECURITY → CONFIGURATION → OPERATION (Audit-Logging) |
| **Integrationstechnik (Schnittstellen)** | INTERFACES → CONFIGURATION → API |
| **Projektverantwortung / Review** | ARCHITECTURE → SECURITY → OPERATION → DEVELOPMENT |

## Konventionen in der Doku

- **`code`** bzw. `` `pfad` ``: Dateipfade, Konfigurationsfelder, Befehle.
- **Fett**: Wichtige Warnungen oder zentrale Begriffe.
- **Tabelle**: Strukturierte Referenzen (Felder, Ports, Fehlercodes).
- **Beispiel**: Code- und JSON-Beispiele in geblockter Formatierung.
- **Hinweis / Warnung / Wichtig**: Vorangestellte Blöcke kennzeichnen Kritikalität.

> **Wichtig:** Alle in dieser Dokumentation genannten Pfade, Ports, Konfigurationsfelder und Datenbankschemata beziehen sich auf den **tatsächlichen Projektstand** in `config/kis.toml`, `src/`, `migrations/001_init.sql` und `static/index.html`.

## Schnellstart (Kurzform)

```bash
# 1. Projekt klonen
git clone <repo-url> && cd kis

# 2. Build
cargo build --release

# 3. Start (Konfiguration: config/kis.toml)
./target/release/kis
```

Die Benutzeroberfläche ist anschließend unter `http://<host>:8080/` erreichbar, die API unter `http://<host>:8080/api/`.

## Versionsstand

| Version | Datum | Hinweis |
|---|---|---|
| 0.1.0 | — | Erstes dokumentiertes Release; Kern-API-Endpunkte als Grundgerüst vorhanden, Schnittstellen in Entwicklung. |

> **Status-Hinweis:** Die Module unter `src/schnittstellen/` (HL7, FHIR, DICOM, XDS) und Teile der API-Handler sind im aktuellen Stand als **Grundgerüst / In Entwicklung** vorhanden. Die Dokumentation beschreibt das **Soll-Konzept** sowie den Ist-Zustand, wo abweichend.
