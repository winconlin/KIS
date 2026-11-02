# KIS — Krankenhaus-Informations-System

KIS ist ein Rust-Prototyp für ein Krankenhaus-Informations-System. Ziel ist ein eigenständiges System, das langfristig auch eine Migration von CGM MEDICO ermöglicht.

**Stand: Entwicklungsprototyp.** Ein HTTP-Server, SQLite-Migrationen und Sicherheitsbausteine sind vorhanden. Die fachlichen Datenendpunkte antworten mit `501 Not Implemented`. GUI, medizinische Schnittstellen und ein vollständiger MEDICO-Ersatz sind noch nicht betriebsbereit.

## Einstieg

- [Dokumentation und Projektstatus](docs/README.md)
- [Installation und lokaler Start](docs/INSTALLATION.md)
- [Aktuelle Architektur und Ausbauziel](docs/ARCHITECTURE.md)
- [Fahrplan](docs/FAHRPLAN.md)
- [Bearbeitung der Prüfungsbefunde](docs/REVIEW_STATUS.md)
- [Agentenregeln](AGENTS.md) und [nächste Arbeitspakete mit Sicherheits-Abnahme](docs/AGENTEN_AUFTRAG.md)

Für Entwicklung werden die in [rust-toolchain.toml](rust-toolchain.toml) gepinnte Rust-Version und ein C-Compiler benötigt. Installation, Schlüsselgenerierung und Administrator-Bootstrap stehen in der Installationsanleitung.

## Aktuell vorhanden

- Axum-HTTP-Server mit `GET /api/health`.
- SQLite-Pool mit verbindlichen SQLx-Migrationen.
- Argon2-Passwörter, HS256-Tokens, persistente Sitzungen und serverseitige Rollenprüfung.
- Audit-Verkettung mit Inhaltsprüfung und Sliding-Window-Limit.
- Separater FHIR-Client-Prototyp; noch kein gestarteter Importdienst.
- Verschlüsseltes SQLite-Backup und Prüfung eines entschlüsselten Snapshots.

Diese Bausteine sind keine Zusage einer klinischen Einsatzfähigkeit, Schnittstellenkonformität oder Zertifizierung. Die Datenbank wird nicht automatisch vollständig verschlüsselt.

## Lizenzangabe

`Cargo.toml` nennt MIT. Eine vollständige `LICENSE`-Datei mit den vom Rechteinhaber festgelegten Angaben ist noch zu ergänzen; siehe Fahrplan 7.8.
