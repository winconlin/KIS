# KIS — Entwicklung

Stand: 02.11.2026

## Aktive Struktur

| Pfad | Rolle |
|---|---|
| Cargo.toml, Cargo.lock, rust-toolchain.toml | Root-Paket und reproduzierbare Toolchain-/Dependency-Auswahl |
| src/main.rs | HTTP-Server |
| src/bin/kis-admin.rs | Lokale Benutzeranlage |
| src/config/, src/db/ | Konfiguration und SQLite |
| src/security/ | Auth, Rollen, Audit, Limits, Verschlüsselung und Regressionstests |
| src/schnittstellen/fhir/medico.rs | Separater MEDICO-Client-Prototyp |
| migrations/ | Eingebettete, versionierte SQLx-Migrationen |
| scripts/backup.py | Verschlüsselter Snapshot und Überprüfung |
| tests/test_backup.py | Backup-/Restore-Regressionsfälle mit echtem age |
| crates/ | Nicht eingebundene Vorarbeiten für die Zielarchitektur |
| docs/ | Ist-Referenz und ausdrücklich gekennzeichneter Fahrplan |

Die Unter-Crate-Manifeste sind noch kein gültiger Workspace. Sie dürfen nicht als erfolgreich gebaute Module im Status erscheinen. Der Root-Build bleibt bis zu einer expliziten Integration die ausführbare Referenz.

## Lokale Einrichtung

Die vollständigen Schritte einschließlich Secret-Generierung und erstem Benutzer stehen in [INSTALLATION.md](INSTALLATION.md). Ein OpenRouter-Key ist für den aktuellen Serverstart weder ausreichend noch erforderlich.

## Prüfungen

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --bins
python3 scripts/check_docs.py
python3 -m unittest discover -s tests -p 'test_*.py' -v
```

Die Backup-Tests benötigen age und age-keygen. Ohne diese Werkzeuge werden sie lokal übersprungen; ein solcher Skip ist kein bestandener Backup-Nachweis. In CI müssen die Werkzeuge installiert sein.

Der Container-Job baut das Image und führt anschließend `python3 scripts/check_container.py --image kis-check` aus. Der Test verwendet ein eigenes temporäres Volume und prüft Admin-Bootstrap, Betrieb ohne root, Health, Token-Zwecke und Widerruf nach einem Neustart. Container und Volume werden danach entfernt.

Die Security-Regressionstests starten echte lokale HTTP-Listener und prüfen unter anderem Login/Lockout, einheitliche Pfade, Rollen, Refresh-Verwendung, dauerhaften Widerruf, atomare Rotation und Audit-Manipulation. Die Datenhandler bleiben auch im Test absichtlich HTTP 501.

## Konventionen

- Neue Optionen benötigen einen tatsächlichen Verbraucher, Defaults, Validierung und Dokumentation.
- SQL-Abfragen und Migrationen müssen dieselben Spaltennamen verwenden. Fachliche Modelle können von DB-Spalten abweichen, benötigen dann aber ein ausdrückliches Mapping.
- Datenbankfehler dürfen bei Start/Migration nicht als bloße Warnung übersprungen werden.
- Unfertige Handler geben einen klaren Nicht-implementiert-Status zurück.
- Asynchrone Audit-Aufrufe müssen abgewartet und Fehler behandelt werden.
- Geheimnisse und Patientendaten nicht in Logs, Beispielen oder Tests verwenden.
- Für mehrschrittige Änderungen Transaktionen sowie negative Berechtigungs-/Fehlertests vorsehen.
- Reine Formatkorrekturen brauchen keine zusätzlichen Implementierungstests.

## Migrationen

[DATABASE.md](DATABASE.md) beschreibt das aktuelle Schema und die Hinweise zu zuvor manuell reparierten 002-Migrationen. Bereits angewandte Dateien nicht blind umschreiben. Neue Migrationen brauchen eine eindeutige freie Version und Tests auf leerer sowie bestehender DB.

## CI und Release

Die eingecheckte CI führt Rust- und Dokumentations-/Backup-Prüfungen aus. Sie ist kein Nachweis eines vergangenen erfolgreichen Releases. Ein signierter Release-/Deploy-Prozess und ein gepflegter Changelog sind weiterhin auszubauen.

Branches sollen prüfbare Änderungen enthalten. Vor einem Merge sind Tests und Review erforderlich; Sicherheitsänderungen benötigen gemäß [FAHRPLAN.md](FAHRPLAN.md) ein unabhängiges zweites Review.

## LLM-Entwicklung: geplant

OpenRouter-Modelle sind in der Konfiguration als Entwurf aufgeführt. Eine automatische Modellrotation, gegenseitige Reviews und klinische LLM-Funktionen laufen derzeit nicht.

Für einen späteren Workflow müssen Geheimnisse, Datenübertragung, Modellantworten, Kostenlimits und menschliche Freigaben gesondert geklärt werden. Pseudonymisierung allein macht Gesundheitsdaten nicht anonym oder automatisch zur externen Übermittlung geeignet.
