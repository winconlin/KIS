# KIS — Dokumentation

Stand: 02.11.2026 · Paketversion 0.1.0 · Entwicklungsprototyp

Die Referenz beschreibt den aktiven Root-Build unter `src/`. Der PostgreSQL-Workspace unter `crates/` ist ein nicht eingebundener Architekturentwurf. Geplante Funktionen werden ausdrücklich als Ausbauziel bezeichnet.

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

Agenten beginnen mit [AGENTS.md](../AGENTS.md). Der erste Auftrag ist PG-00; die PostgreSQL-Implementierung beginnt nach der dort festgelegten Bestands- und Vertragsklärung.

## Funktionsstand

| Bereich | Ist-Zustand |
|---|---|
| Datenbank | SQLite über SQLx; WAL und Pool konfigurierbar |
| HTTP | Standard `127.0.0.1:8080`; Health unter `/api/health` |
| Root-Seite | Text mit Prototyp-Hinweis; GUI-Entwurf noch nicht angeschlossen |
| Auth | Login, Refresh, Logout; persistenter Widerruf von Token-Paaren |
| Fachliche API | Registrierte Platzhalter; nach Auth/Rollenprüfung HTTP 501 |
| MFA | Hilfsfunktionen vorhanden; Setup-Endpunkt bewusst HTTP 501 |
| Schnittstellen-Server | HL7, FHIR, DICOM, XDS und TI noch nicht gestartet/implementiert |
| MEDICO | Separater FHIR-Client, kein vollständiger Migrationsdienst |
| LLM | Konfigurationsentwurf, kein laufender OpenRouter-Client |
| PostgreSQL/Workspace | Geplant; `crates/` wird nicht vom Root-Manifest gebaut |

Der Schnellstart erfordert erzeugte Schlüssel und eine beschreibbare Datenbankablage. Dafür die [vollständige Installationsanleitung](INSTALLATION.md) verwenden.
