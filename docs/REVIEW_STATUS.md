# KIS — Prüfstatus

## Prüfung und Überarbeitung vom 06.11.2026

Ausgangspunkt: `main` @ `ee6685a` („Phase 4+5"). Ergebnis-Branch: `improve/workspace-security-hardening`.

### Ausgangslage

- Der Workspace kompilierte nicht (90 Fehler in `kis-core`, Syntaxfehler in `kis-app`). Das Root-Paket unter `src/` (SQLite-Prototyp) wurde seit der Umstellung auf den Workspace nicht mehr gebaut; README, AGENTS.md und Doku beschrieben aber weiter diesen Stand.
- Keine PostgreSQL-Datenbank ließ sich vollständig migrieren: doppelte Version 3 und eine Migration auf die zuvor gelöschte Tabelle `audit_log`.
- Der Server wäre beim Start abgestürzt (kollidierende Routen in `kis-medications`). Alle Routen mit `{param}`-Syntax waren unter axum 0.7 unerreichbar.

### Behobene Sicherheitsmängel (Auswahl, alle mit Test)

| Befund | Behebung |
|---|---|
| Login akzeptierte **jedes Passwort** und jeden MFA-Code (Platzhalter `true`) | Neues `kis_core::auth`: Argon2id, Kontosperre, TOTP, gleiche Antwort für unbekannt/falsch/gesperrt |
| Patienten-, Ärzte-, Termin- und Medikations-API **ohne jede Authentifizierung** | Zentrale Middleware mit deny-by-default-Routentabelle über allen Routern |
| DSGVO-Endpunkte (Hard-Delete, Export, Anonymisierung) mit fest verdrahtetem Admin-Akteur | Akteur aus geprüfter Sitzung; Admin-/Export-Rechte erzwungen |
| Refresh-Token als Access-Token nutzbar; kein Widerruf, Logout ohne Wirkung | Token-Typ im Claim; DB-Sitzungen mit Rotation, Widerruf, Inaktivitäts-Logout |
| Unsicherer Dev-Default für JWT-Secret und Null-Master-Key; Panic bei kurzem Key | Start bricht ab; Längen- und Inhaltsprüfung |
| Unbekannte Rollen still zu `viewer` herabgestuft; `auditor`/`llm_agent` nicht parsebar | Strikte Rollenliste, DB-CHECK-Constraint |
| Audit „best effort" in nicht existierende Tabelle, ohne Akteur; Kettenwettlauf; Verifikation scheiterte immer (Nanosekunden vs. Mikrosekunden) | Audit in derselben Transaktion, Advisory-Lock, längenpräfixierte Hashes, append-only per Trigger und Rechten |
| Volle Patientendaten und komplette Exporte im unveränderlichen Audit-Trail | Nur Feldnamen bzw. SHA-256 |
| „Anonymisierung" per ungesalzenem SHA-256 des Namens (Wörterbuchangriff) | Zufällige Ersatzwerte |
| Rate-Limit-Schlüssel aus unverifiziertem JWT (`sub` frei wählbar) und gefälschtem `X-Forwarded-For` | Client-IP; XFF nur von Loopback-Proxy |
| SQL-/Interna in Fehlerantworten | Generische Meldung, Details ins Log |
| mTLS-CA wurde geladen und ignoriert; „TLS 1.3" bot auch 1.2 an; TLS-Server kompilierte nicht | Nur TLS 1.3, echte Client-Zertifikatsprüfung |
| Doppelbuchungsprüfung nicht race-frei | GiST-Exclusion-Constraint |
| Laufzeitverbindung als Schema-Eigentümer | Getrennte Rollen `kis_owner`/`kis_app`, `kis-admin migrate` |
| sqlx 0.7 (RUSTSEC-2024-0363) | sqlx 0.8 |
| DICOM-Listener: Index-Panic bei jeder Assoziationsanfrage | Normgerechtes A-ABORT |

### Weitere Korrekturen

- Konfiguration: TOML überschrieb bisher die Umgebung; jetzt TOML < `KIS_<ABSCHNITT>__<SCHLÜSSEL>`. Leere `read_url` wird ignoriert; die mitgelieferte `kis.toml` lässt sich laden (Test).
- JSONB-Felder (`allergies`, `specialties`) wurden falsch dekodiert; die statische Auslieferung der GUI war wegen falscher Routensyntax tot; GUI an die strikte CSP angepasst.
- Backup auf PostgreSQL umgestellt (`pg_dump` → `age`, Restore-Test in leere Datenbank). Das alte SQLite-Werkzeug passte nicht mehr zum Schema.
- Dockerfile, Compose, `.env.example` (falscher Variablenname `KIS_ENCRYPTION_KEY`), systemd-Unit (ungültige Inline-Kommentare, fehlende Pfade) korrigiert.
- CI ohne abgeschwächte Schritte (`continue-on-error`, `|| echo` entfernt); PostgreSQL-Integration verpflichtend; SQL-Schema-Abgleich; Container-Smoke-Test.
- Verwaister SQLite-Prototyp `src/` entfernt (in der Git-Historie erhalten).

### Nachweise (auf dem Ergebnis-Commit ausgeführt)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`: sauber.
- `cargo test --workspace`: alle Unit-Tests und 8 PostgreSQL-Integrationstests gegen PostgreSQL 16 bestanden.
- `scripts/check_sql.py` für alle aktiven Crates: 99 Anweisungen, 0 Fehler.
- Backup-/Restore-Tests: 6 bestanden (echtes age, pg_dump, Restore).
- Prozess-Smoke-Test: `kis-admin migrate` mit Eigentümerrolle, Server als `kis_app`, Login, Patientenanlage, `DELETE FROM audit_logs` und DDL für `kis_app` verweigert, `verify-audit` intakt, sauberer Shutdown.
- `cargo audit`: keine Schwachstelle; eine begründete Ausnahme (`.cargo/audit.toml`), eine Warnung (`rustls-pemfile` unmaintained, über `axum-server`).
- Toolchain: lokal mit Rust 1.97 geprüft, da 1.99.1 in der Prüfumgebung nicht herunterladbar war. CI verwendet 1.99.1.

### Nicht geprüft / offen

- Docker-Build und Container-Smoke-Test liefen mangels Docker-Daemon nur in CI-Form (Skript vorhanden, nicht lokal ausgeführt).
- Echter TLS-Handshake (nur Konfigurationspfade getestet), realer systemd-Betrieb.
- Schnittstellen-Crates (`kis-hl7`, `kis-fhir`, `kis-dicom`, `kis-xds`, `kis-gdt`, `kis-medico`, `kis-ti`): nicht eingebunden; `check_sql.py` meldet fehlende Tabellen/Spalten (z. B. `medico_conflicts`, `xds_document_entries`, `dicom_sops`, `patients.first_name`) und direkte Schreibzugriffe auf die entfernte Tabelle `audit_log`. Ein Schema dafür setzt Gegenstellen- und Fachentscheidungen voraus.
- MFA-Einrichtung (Endpunkt fehlt); `mfa_secret` liegt unverschlüsselt in der Datenbank.
- Fachliche Rollenmatrix bestätigen: z. B. darf `rezeption` Termine buchen, aber nicht lesen; `arzt` darf nicht löschen.
- Hash-Kette ohne externen Prüfanker; unabhängiges Sicherheitsreview gemäß [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md) steht aus — diese Überarbeitung ist keine Freigabe.

---

## Frühere Prüfung (02.11.2026, SQLite-Prototyp — überholt)


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
