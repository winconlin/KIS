# KIS — Bearbeitung der Repository-Prüfung

Stand: 02.11.2026 · Ausgangspunkt: Commit 6c364721fe75b4216ff7e9f7552f84e68c39ceef

Die ursprüngliche Prüfung fand 26 Themen. Dieser Korrektursatz richtet den aktiven SQLite-Prototyp und seine Dokumentation aus. Er implementiert nicht den gesamten geplanten Krankenhausbetrieb.

## Zuordnung der Befunde

| Nr. | Thema | Bearbeitung / verbleibende Aufgabe |
|---|---|---|
| 01 | SQLite-Syntax und Fehlerbehandlung | Migration korrigiert; Start bricht bei Fehlern ab. Projektverantwortlicher bestätigt nur frische Testinstallationen; aktuell kein Bestandsdaten-Upgrade erforderlich. |
| 02 | Spaltennamen | Auth/Audit an vorhandenes Schema angepasst; Login und Audit werden getestet. |
| 03 | Build/Workspace/Docker | Root-Toolchain gepinnt; Docker ohne erfundene Features, Compose passend zu SQLite. Docker-Build und Container-Smoke-Test in GitHub-CI bestanden. PostgreSQL-Workspace/Datenmodell sind die bestätigte nächste Priorität. |
| 04 | Doppeltes API-Präfix | Einmaliges Präfix, HTTP-Regressionstest. |
| 05 | Refresh als Access / Widerruf | Zweckprüfung und persistente gemeinsame Sitzung; atomare Rotation und Neustart-Widerruf getestet. |
| 06 | RBAC | Serverseitige Routenregeln, keine Rechtewahl per Query. Objektberechtigungen für zukünftiges CRUD bleiben offen. |
| 07 | Audit | Abgewartete DB-Schreibvorgänge, serielle Verkettung, tatsächliche Inhaltsprüfung; externer Anker/administrativer Schreibschutz bleiben offen. |
| 08 | MEDICO-Leseverluste | Paging, Fehlerbehandlung und spätere Versionen korrigiert und getestet. Persistenter Importdienst/ACK-Cursor fehlen weiterhin. |
| 09 | Backup/Restore-Test | Echtes SQLite-Snapshot-/age-Werkzeug mit konsistenten Artefakten und sechs Regressionstests. |
| 10 | Unsichere Rohkopien/Restore | Durch Snapshot-Verfahren und abgestimmte Stop-/Sicherungsanleitung ersetzt; kein pauschales Löschen bestehender DBs. |
| 11 | Unbelegte Fertigmeldungen | Ist/Ziel getrennt, Platzhalter als 501 erkennbar, Root-GUI und Schnittstellenstatus berichtigt. |
| 12 | ENV | Einheitliches __-Mapping, Config-Pfad und Beispiele korrigiert. |
| 13 | Erster Start / systemd | DB-Dateianlage, notwendige Secrets, absolute Pfade und lokale Admin-CLI dokumentiert. systemd-Lauf separat zu prüfen. |
| 14 | GDT / Rollback | Export als CSV benannt und gequotet; tatsächliches GDT und vollständiger Rollback bleiben offen. |
| 15 | DB-Referenz | Schema aus tatsächlichen Migrationen dargestellt; unfertiges API-/DB-Mapping kenntlich gemacht. |
| 16 | Auth-Dokumentation / MFA | HS256, reale Laufzeiten und Antwortformate beschrieben; MFA-Setup bis zur vollständigen Umsetzung deaktiviert. |
| 17 | Wirkungslose Konfiguration | Pool, Journal und Worker angewendet; übrige Ausbauoptionen ausdrücklich als reserviert markiert. |
| 18 | HL7 | Ereignisse, Feldpositionen, ACK und Framing korrigiert; Gegenstellenvalidierung offen. |
| 19 | FHIR | Suchparameter und R4-Beispiele korrigiert; konkrete ISiK-Profilvalidierung offen. |
| 20 | XDS/DICOM | Proprietäre REST-Idee von Standards getrennt; Begriffe und DCMTK-Beispiel berichtigt. Konformitätsnachweise offen. |
| 21 | MEDICO-Mapping | Spaltenreferenz und Konflikthinweis korrigiert; CamelCase-Felder und Client-Konfiguration angepasst. Fachlicher Upsert-/ID-Speicher offen. |
| 22 | Migration/WAL | SQLx-Versionierung, Backup-Konsistenz und Durabilitätsgrenzen richtiggestellt. |
| 23 | CI/Release/Monitoring | GitHub-CI für Rust, Dokumentation/Backups und Docker-Build bestanden. Signierte Releases und Monitoring-Infrastruktur bleiben offen. |
| 24 | Rust-Testfehler | Ungültige base64-Engine entfernt, Base32 und laufende Test-Runtime verwendet. |
| 25 | Recht/Normen | Pauschale Konformitäts-/Fristzusagen entfernt; betriebliches/fachliches Konzept und Normprüfung bleiben offen. |
| 26 | Markdown-Pflege | Index, Status, Formulierungen und Codeblock-Kennzeichnung bereinigt; automatischer Datei-/Codeblock-Check ergänzt. |

## Prüfungen

Für den bisherigen Code wurden 37 Rust-Tests und sechs Python-Backup-Tests bestanden. Formatierung, Clippy mit Warnungen als Fehler, Binär-Build und Dokumentationsprüfung wurden ebenfalls ausgeführt. Mit den ergänzten Agentenanweisungen umfasst der Dokumentationscheck nun 17 Markdown-Dateien einschließlich Root-AGENTS.md, 66 lokale Links sowie JSON-/TOML-/Shell-Codeblöcke. Die Befehle stehen in [DEVELOPMENT.md](DEVELOPMENT.md).

Ein zusätzlicher Prozess-Smoke-Test prüft den echten Admin-CLI-/Serverstart mit abweichender TOML-Datei und ENV-Overrides, Login, Refresh, Logout und erneuten Serverstart. Alle vier SQLx-Migrationen, WAL-Modus, DB-Integrität und Audit-Einträge wurden dabei bestätigt. Compose-Konfiguration und CI-YAML sind syntaktisch geprüft; das gepinnte Docker-Basisimage ist verfügbar.

Die Backup-Tests verwenden echte age-Schlüssel und -Verschlüsselung mit temporären synthetischen Daten. Der Client-Test umfasst 151 Ressourcen über zwei Seiten. Security-Tests prüfen auch fehlende Rechte, Refresh an Datenrouten, Logout, parallele Rotation, Kontosperre und manipulierten Audit-Inhalt.

Der [GitHub-CI-Lauf für Commit f19a79f](https://github.com/winconlin/KIS/actions/runs/41380825389) bestätigt Rust, Dokumentation/Backups, Docker-Build und den erfolgreichen Container-Smoke-Test. Dieser prüft Betrieb ohne root, Bootstrap, HTTP und Widerruf nach Neustart. Die Ergebnisse des jeweiligen Commits sind im PR einsehbar.

Offen bleiben der reale systemd-Betrieb, eine echte MEDICO-Integration, klinische Workflows, der vollständige PostgreSQL-Workspace und eine Zertifizierung. Ein unabhängiges zweites Security-Review gemäß Fahrplan bleibt vor einer Freigabe erforderlich.

## Bestätigte Projektangaben und verbleibende Rückfrage

- **Bestandsdaten, bestätigt am 02.11.2026:** Nur frische Entwicklung/Testinstallation. Ein gesonderter Datenerhalt-/Checksum-Upgrade-Pfad für Altbestände ist deshalb aktuell kein Projektblocker. Das ist keine allgemeine Freigabe für das Überschreiben fremder oder künftiger Bestandsdatenbanken.
- **Nächste Ausbauetappe, bestätigt am 02.11.2026:** PostgreSQL-Workspace und Datenmodell zuerst. Die Reihenfolge und Abnahmekriterien stehen im Fahrplan. Fachliche CRUD-/GUI- und MEDICO-Ausbauschritte folgen auf dieser Basis; sie werden hier noch nicht als umgesetzt markiert.
- **MEDICO-Gegenstelle:** Version, vereinbarte ISiK-Profile und eine Sandbox/Testumgebung sind noch zu benennen. Bis dahin bleiben fachliche Schreib-/Migrationsabläufe ungeprüft.

## Arbeitsauftrag für die Umsetzung

[AGENTS.md](../AGENTS.md) definiert die automatisch zu lesenden Repo-Regeln. Der [operative Agentenauftrag](AGENTEN_AUFTRAG.md) zerlegt die nächste PostgreSQL-Etappe in PG-00 bis PG-10 einschließlich Rollen, Abhängigkeiten, Architektur-/Datenverträgen, Abnahmekriterien und Übergabeformat. Die [Sicherheits-Abnahme](SICHERHEITS_ABNAHME.md) legt echte Negativ-, Rechte-, Race-, Ausfall- und Restore-Nachweise fest. Diese Dokumente definieren zukünftige Arbeit; sie melden keine PostgreSQL-Implementierung oder unabhängige Sicherheitsfreigabe als bereits erfolgt.
