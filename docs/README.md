# KIS — Dokumentation

Stand: 06.11.2026 · Paketversion 0.1.0 · Entwicklungsprototyp

Die ausführbare Referenz ist der PostgreSQL-Workspace unter `crates/` (Server in `kis-app`). Der frühere SQLite-Prototyp unter `src/` wurde entfernt. Aktuell maßgeblich sind [INSTALLATION](INSTALLATION.md), [API](API.md), [DATABASE](DATABASE.md), [DEVELOPMENT](DEVELOPMENT.md) und [REVIEW_STATUS](REVIEW_STATUS.md). Andere Dokumente enthalten teils noch Zielbild-Beschreibungen; sie sind entsprechend markiert.

## Dokumente

| Dokument | Inhalt |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Aktiver Datenfluss und geplante Zielarchitektur |
| [INSTALLATION.md](INSTALLATION.md) | Toolchain, Entwicklung, Bootstrap, systemd und Docker |
| [CONFIGURATION.md](CONFIGURATION.md) | TOML, tatsächlich verwendete Felder und ENV-Overrides |
| [API.md](API.md) | Registrierte Routen, Authentifizierung und Antworten |
| [DATABASE.md](DATABASE.md) | Aktuelles Schema und Migrationsverfahren |
| [SECURITY.md](SECURITY.md) | Umgesetzte Maßnahmen, Grenzen und offene Anforderungen |
| [OPERATION.md](OPERATION.md) | Health, Logs, getestete Backup-Werkzeuge und Restore |
| [INTERFACES.md](INTERFACES.md) | Stand und technische Vorgaben für Schnittstellen |
| [CGM_MEDICO.md](CGM_MEDICO.md) | Client-Prototyp, Datenmapping und geplante Migration |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Struktur, Prüfungen und Arbeitsablauf |
| [FAHRPLAN.md](FAHRPLAN.md) | Geplanter vollständiger Ausbau |
| [REVIEW_STATUS.md](REVIEW_STATUS.md) | Korrekturen und verbleibende Prüfungsaufgaben |
| [AGENTEN_AUFTRAG.md](AGENTEN_AUFTRAG.md) | Ausführbare nächste Etappe PG-00 bis PG-10 mit Zuständigkeiten und Abnahme |
| [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md) | Sicherheitsfälle, echte Testnachweise und getrennte spätere Freigaben |

Agenten beginnen mit [AGENTS.md](../AGENTS.md).

## Funktionsstand

| Bereich | Ist-Zustand |
|---|---|
| Datenbank | PostgreSQL 16 über SQLx 0.8; getrennte Migrations- und Laufzeitrolle |
| HTTP | `kis-app`, Standard `127.0.0.1:8080`; Health unter `/health`; TLS 1.3 optional |
| GUI | Statische SPA unter `/` (strikte CSP) |
| Auth | Login, Refresh-Rotation, Logout, Widerruf, Inaktivitäts-Logout, Kontosperre |
| Autorisierung | Zentrale Routentabelle, deny-by-default |
| Fachliche API | Patienten, Ärzte, Termine, Medikation (CRUD) |
| Audit | Hash-Kette, transaktional, append-only, ohne Personendaten |
| DSGVO | Export, Hard-Delete, Anonymisierung, Pseudonymisierung |
| MFA | TOTP-Prüfung beim Login; Einrichtungs-Endpunkt fehlt |
| Schnittstellen | HL7, FHIR, DICOM, XDS, GDT, MEDICO, TI: Code vorhanden, **nicht eingebunden** |
| LLM | Nur Konfigurationsentwurf |
