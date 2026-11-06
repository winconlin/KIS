# KIS — Operativer Auftrag für die nächste Agenten-Etappe

> **Hinweis (06.11.2026):** Teile dieses Dokuments beschreiben den entfernten SQLite-Prototyp oder ein Zielbild. Maßgeblich für den Ist-Zustand sind [INSTALLATION](INSTALLATION.md), [API](API.md), [DATABASE](DATABASE.md) und [REVIEW_STATUS](REVIEW_STATUS.md).


Stand: 02.11.2026 · Status: verbindlicher Umsetzungsauftrag, noch keine Implementierungs-Fertigmeldung

## 1. Ziel und Ausgangsbasis

Die nächste Etappe liefert einen baubaren Cargo-Workspace mit einem konsistenten, durch echte PostgreSQL-Tests abgesicherten Datenmodell und integriertem PostgreSQL-Betrieb des bisherigen technischen Prototyps. Fachliches Patienten-CRUD, GUI und vollständige MEDICO-Migration sind nachgelagert.

Der Benutzer hat bestätigt:

- Es gibt nur frische Entwicklung/Testinstallationen; aktuell keine erhaltenswerte Produktivdatenbank zu migrieren.
- PostgreSQL-Workspace und Datenmodell haben Vorrang.
- MEDICO-Version, konkrete ISiK-Profile und Sandboxangaben sind noch offen. Sie blockieren diese Basisetappe nicht.

Die geprüfte Codebasis ist PR #1 auf fix/review-startup-security-docs. Der technische Stand a95dc25 umfasst erfolgreiche Rust-, Backup-, Dokumentations- und Container-Prüfungen. Die Agenten-Anweisungen werden darauf aufgebaut. Vor Arbeitsbeginn den aktuellen geprüften HEAD auflösen; nach einem Merge stattdessen den entsprechenden main-Stand verwenden. Ein alter main-Checkout ist nicht automatisch die richtige Basis.

Root-[AGENTS.md](../AGENTS.md) enthält die Arbeitsregeln, [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md) die Sicherheitsnachweise. Der [FAHRPLAN.md](FAHRPLAN.md) bleibt die langfristige Übersicht. Dieses Dokument legt die unmittelbar nächste Etappe fest.

## 2. Was am Ende nachweisbar sein muss

1. Gültiger, zyklusfreier Workspace; bisherige funktionsfähige Sicherheitsbausteine sind übernommen statt dupliziert.
2. Ein verbindliches Domänen-/Datenmodell mit vollständigem Mapping zwischen Rust-Typen, SQL und technischen DTOs.
3. Frische PostgreSQL-Migrationen, echte Constraints und minimale Datenbankrechte.
4. Start, Benutzer-Bootstrap, Login, Refresh, Logout, Audit und Nicht-implementiert-Antworten auf PostgreSQL.
5. Keine erfolgreiche fachliche Antwort für noch nicht implementierte CRUD-/GUI-/Schnittstellenfunktionen.
6. Negativ-, Nebenläufigkeits-, Ausfall- und Wiederherstellungstests auf PostgreSQL und auf dem gebauten Container.
7. Unabhängiges Security-Review mit Nachweis auf dem geprüften Code; offene Betriebs-/Fachfreigaben bleiben sichtbar.

Eine grüne Workspace-Kompilierung allein erfüllt diesen Auftrag nicht. Es werden keine Last-, Compliance- oder klinischen Eignungszusagen aus der bloßen Umstellung abgeleitet.

## 3. Rollen und Übergaben

| Rolle | Aufgabe | Verantwortete Ergebnisse |
|---|---|---|
| Koordination/Integration | Basis-SHA, Reihenfolge, Dateizuständigkeiten, Konflikte, Integration | Taskstatus, gemeinsamer Vertragsstand, Root-Manifest/Lockfile, Integrations-PR |
| Architektur/Datenmodell | Kanonische Typen, Beziehungen, Transaktions-/Berechtigungsgrenzen | Architekturentscheidungen, Modell- und Mappingreferenz |
| Workspace/Storage | Crates, PostgreSQL-Pool, Migrationen, Repositories | Baubarer Workspace und echter DB-Adapter |
| Security-Implementierung | Auth, Rollen, Audit, Secret-Behandlung | Erhalt und Portierung des Sicherheitsverhaltens |
| QA/Betrieb | Gegenprüfungen, CI, Container, Restore, Ausfallfälle | Ausführbare Tests und reproduzierbare Prüfprotokolle |
| Unabhängiges Security-Review | Code und Negativfälle ohne Selbstfreigabe prüfen | Fundstellen, Restrisiken, Abnahme oder begründete Rückgabe |

Eine Person oder ein Agent darf mehrere Implementierungsrollen übernehmen. Die unabhängige Reviewrolle bleibt vom Autor der geprüften Sicherheitsänderung getrennt. Die Rollenliste ist keine automatische Anweisung, weitere Agenten zu starten oder Personen zu benachrichtigen.

Bei tatsächlich paralleler Agentenarbeit erhält jeder Implementierer einen eigenen Worktree/Branch und eine isolierte Testdatenbank. Änderungen an Verträgen, Root-Cargo-Dateien, Lockfile und Migrationsversionen bekommen jeweils einen benannten Integrationsverantwortlichen. Kein gleichzeitiges Editieren derselben Dateien.

## 4. Abhängigkeiten und Priorität

| Paket | Inhalt | Benötigt | Zuständigkeit |
|---|---|---|---|
| PG-00 | Ausgangsbasis und Bestand erfassen | — | Koordination + QA |
| PG-01 | Architektur, Domänenmodell und Sicherheitsverträge | PG-00 | Architektur + Security |
| PG-02 | Workspace und Codezuständigkeiten | PG-01 | Workspace + Integration |
| PG-03 | PostgreSQL-Schema, Migrationen und Rollen | PG-01, PG-02 | Storage |
| PG-04 | Storage-Adapter, Konfiguration und Start | PG-03 | Storage + Integration |
| PG-05 | Auth-/Autorisierungs-/Audit-Portierung | PG-03, PG-04 | Security |
| PG-06 | PostgreSQL-Backup und Restore | PG-03, PG-04 | Betrieb + QA |
| PG-07 | Anwendung auf PostgreSQL umstellen | PG-05 | Integration |
| PG-08 | CI, Container und Betriebsnachweise | PG-06, PG-07 | QA + Betrieb |
| PG-09 | Unabhängige Sicherheitsabnahme | PG-05, PG-06, PG-08 | Security-Review |
| PG-10 | Integrationsabnahme und Dokumentation | PG-09 | Koordination + QA |

PG-05 und PG-06 können nach stabiler Schema-/Storage-Übergabe parallel laufen. Vorher keine konkurrierenden Modelle oder Auth-/Audit-Schemata entwickeln. PG-09 kann früh beratend mitlesen, ersetzt durch eine solche Vorprüfung aber nicht die Abnahme des tatsächlich integrierten Codes.

## 5. Verbindliche Architektur und Datenkonventionen

### Crate-Verantwortung

- Root-Anwendung kis: Prozessstart, Konfiguration, Runtime, Router-Komposition und CLI-Einstiegspunkte.
- kis-core: gemeinsame IDs, Domänentypen, Fehler, Richtlinien-/Repository-Verträge; keine Abhängigkeit auf Fachmodule, Anwendung oder konkrete Datenbanktreiber.
- kis-storage: konkrete PostgreSQL-Verbindungen, Repositories, Transaktionen und Migrationseinbindung; hängt von kis-core ab.
- kis-security: Auth-, Berechtigungs-, Audit- und Krypto-Implementierung; hängt von gemeinsamen Verträgen bzw. Storage ab, niemals umgekehrt als zyklische Rückreferenz.
- Fachmodule: verwenden die vereinbarten Verträge. Der gemeinsame Kern importiert keine Router aus seinen eigenen abhängigen Fachmodulen.

Neue Crate-Namen und ihre Abhängigkeitsrichtung werden in PG-01 dokumentiert. Bestehende Entwürfe unter crates/ sind vollständig zu inventarisieren: gültiges Mitglied oder ausdrücklich nicht integrierter Entwurf. Nicht benötigte aktive Sicherheitskopien entfernen; Versionshistorie bleibt über Git erhalten. Kein absichtliches Verstecken fehlerhafter Module zur Erzeugung eines grünen Builds.

### Kanonische Typen und Schema

| Bereich | Vorgabe |
|---|---|
| Identitäten | UUID für fachliche IDs; externe IDs separat mit Quellsystem/Ressourcentyp/Namensraum |
| Zeitpunkte | TIMESTAMPTZ und UTC im Code; RFC3339 an technischen API-Grenzen |
| Kalendertage | DATE; fehlende Werte nullable, kein erfundener 01.01.-Ersatz für unvollständige Quelldaten |
| Änderungen | created_at, updated_at; version BIGINT mit positivem Default/Constraint für veränderliche fachliche Datensätze |
| Löschen | Referenzen zunächst RESTRICT/NO ACTION, sofern keine begründete technische Ausnahme; keine unbemerkte klinische Löschkaskade |
| SQL | Typen, Nullbarkeit, Unique-/Check-/FK-Constraints im echten Schema; keine ausschließliche Absicherung im HTTP-Handler |
| API-/DB-Mapping | Explizit dokumentiert und getestet; keine konkurrierenden Varianten wie insurance_no/insurance_number ohne Mapping |
| Sensible Inhalte | Klassifikation und Speicherentscheidung je Feld; keine unverschlüsselte Parallelkopie als versteckter Fallback |

Mindestens zu modellieren:

- users und sessions: eindeutiger Benutzername, Passwort-Hash, kontrollierte Rollen, Kontostatus, Fehlversuche/Sperre, Token-Zweck und gemeinsamer dauerhafter Widerruf eines Token-Paars.
- patients: Stammdaten; vollständiges Geburtsdatum kann fehlen. Unvollständige FHIR-Datumswerte dürfen nicht in ein erfundenes vollständiges Datum umgewandelt werden; deren spätere Übernahme braucht eine dokumentierte Importregel.
- doctors: eigenständige Arztidentität; kein Eigentum eines Patienten und keine Kaskadenlöschung über doctors.patient_id. Ein optionaler Benutzerbezug ist von der fachlichen Arztidentität getrennt.
- appointments: vorhandener Patient, optionaler Arzt entsprechend dem vereinbarten Vertragsmodell, Start vor Ende, kontrollierter Status und Version. Ohne nachgewiesene Fall-/Objektberechtigung keine Freigabe einer späteren fachlichen Route.
- medications: Patient und gegebenenfalls Verordnender mit echten Referenzen, Beginn/Ende mit konsistenter zeitlicher Bedeutung, Status/Version. Kein automatisch angenommener medizinischer Dosis- oder Allergiecheck.
- consents: Patient, Zweck/Rechtsgrundlage, erteilende Person/System, Zeitpunkte und Widerruf; keine universelle Einwilligung als Ersatz für eine fachlich zu prüfende Rechtsgrundlage.
- external_identifiers: Quelle, Ressourcentyp, Identifier-System und Wert mit geeigneter Eindeutigkeit sowie interner Zuordnung. Dasselbe ID-Zeichen in einem anderen Quellsystem darf keinen Patienten überschreiben.
- audit_log und audit_head: geordnete Ereignisse, Hash-Version/-Inhalt und ein transaktional gesperrter Kopf je definierter Kette für mehrere Writer/Instanzen.

PG-01 fixiert alle konkreten Feldnamen, Nullbarkeiten, Statuswerte, Typabbildungen und Repository-Signaturen. Sichere technische Defaults selbst festlegen und begründen. Ungeklärte klinische Geschäftsregeln nicht durch erfundene Workflows ersetzen; die entsprechenden Funktionen bleiben gesperrt oder außerhalb dieser Etappe.

## 6. Arbeitspakete

### PG-00 — Verifizierte Basis sichern

**Eingang:** Aktueller Stand von PR #1 bzw. sein integrierter Nachfolger.

**Arbeit:** Basis-SHA, Arbeitsbaum, bestehende Module, Migrationen und offene Befunde erfassen. Vorhandene Rust-, Backup- und Container-Nachweise überprüfen. Feststellen, ob tatsächlich nur isolierte Testdaten vorliegen. Zu jedem der bisherigen Sicherheitsfälle ein zukünftiges Test-/Paket-Ziel zuordnen.

**Abnahme:** Dokumentierter Basisstand; keine verlorenen fremden Änderungen; bisherige Prüfungen ausgeführt oder mit belastbarem Commit-/CI-Verweis belegt. Fehler werden vor strukturellem Umbau eingegrenzt. Eine fehlende Docker-/PostgreSQL-Umgebung wird als fehlender Nachweis vermerkt, nicht durch SQLite ersetzt.

**Übergabe:** Inventar, Sicherheitsfall-Zuordnung, Basis-SHA und offene Umgebungsblocker.

### PG-01 — Architektur und Modell verbindlich machen

**Eingang:** PG-00.

**Arbeit:** Gemeinsame Typen, Schema, DTO-Mapping, Abhängigkeitsgraph, Rollen-/Objektgrenzen, Audit-Transaktionen, Verschlüsselungsumfang und spätere Backend-Umschaltung definieren. Architekturentscheidungen mit Problem, Entscheidung, Alternativen und Folgen dokumentieren. Datenfluss Client → API → Auth/Policy → Service → Storage/Audit → PostgreSQL markieren.

**Abnahme:** Für jede Entität stehen Felder/Constraints und Rust-/SQL-Typen fest. Ärzte hängen nicht von Patienten ab. Externe IDs sind mandanten-/quellsystembezogen abgegrenzt, soweit ein konkretes Organisationsmodell vorhanden ist. Ohne definiertes Mehrmandantenmodell keine Mandantenfähigkeit behaupten. Implementierungs- und Testrollen haben denselben Vertragsstand. Sicherheitsrelevante Modellentscheidungen sind unabhängig gegengelesen.

**Übergabe:** Freigegebene Vertragsversion und Architekturentscheidungen; keine bereits erledigt markierte PostgreSQL-Implementierung.

### PG-02 — Workspace und Verantwortungen herstellen

**Eingang:** PG-01.

**Arbeit:** Gültige Cargo-Manifeste, Workspace-Mitglieder, eindeutige Dependencies und tatsächliche Modulgrenzen schaffen. Bisherigen funktionierenden Code verschieben statt eine zweite Auth-/Config-/DB-Variante zu entwickeln. Nicht integrierte Platzhalter kennzeichnen und nicht als aktive medizinische Schnittstelle registrieren.

**Abnahme:** cargo metadata, Workspace-Build, Formatierung, Clippy und bisherige anwendbare Tests bestehen. Keine Crate-Zyklen, keine fehlenden Paketnamen, keine Geheimnis-Fallbacks. Toolchain und Lockfile bleiben reproduzierbar. Jede gültige Feature-Kombination ist benannt; ein deaktiviertes Modul wird weder gestartet noch fälschlich als aktiv gemeldet.

**Übergabe:** Dateiumzug-/API-Mapping, gültiger Dependency-Graph und grüne Workspace-Prüfung. Noch kein PostgreSQL-Fertigstatus allein aufgrund dieses Builds.

### PG-03 — PostgreSQL-Schema und minimale Rechte

**Eingang:** PG-01 und PG-02.

**Arbeit:** Eigene PostgreSQL-Migrationen erstellen. PostgreSQL 16 ist die Ausgangslinie der bisherigen Zielarchitektur; konkreten CI-/Container-Tag samt verfügbarem Digest dokumentieren. UUID, TIMESTAMPTZ, DATE, BOOLEAN und versionierte Daten statt SQLite-Sonderformen verwenden. Getrennte Rollen für Migration, Anwendung und Backup/Prüfung vorsehen.

**Abnahme:** Frische Migration auf echter PostgreSQL-Instanz gelingt; erneutes Ausführen ist ein sauberer No-op mit intakten SQLx-Prüfsummen. Ein fehlerhafter Migrationssatz wird nicht ignoriert. FK-, Unique-, Check- und Zeitbereichsfehler werden von der DB abgewiesen. Tests unterscheiden erlaubte, verbotene und NULL-Fälle.

Die Anwendungsrolle ist weder Superuser noch Schema-/Tabellen-Owner und kann keine Rollen, Schemas oder Tabellen erzeugen bzw. löschen. Insbesondere keine Änderung/Löschung/Trunkierung gespeicherter Audit-Einträge. Falls Audit-Sequenzen benötigt werden, nur die nötigen Sequence-Rechte vergeben. PG-Superuser-Zugänge ausschließlich für isolierte Testeinrichtung bzw. ausdrücklich benannte Administration verwenden.

**Übergabe:** Schema, Rollen-/Grant-Matrix und echte Integrationsnachweise mit getrennten Credentials. Migrations-Credentials werden nicht in den dauerhaften App-Prozess übernommen.

### PG-04 — Storage, Konfiguration und Lebenszyklus

**Eingang:** PG-03.

**Arbeit:** PostgreSQL-Pool, Repositories, typisierte Fehler und Transaktionsgrenzen anschließen. Schema-/Migrationsschritt getrennt mit Migrationsrolle ausführen, danach App mit begrenzter Rolle starten. DATABASE_URL bzw. die konkret festgelegten Konfigurationsfelder einheitlich dokumentieren; URLs und Secrets redigieren. Pool-Größe und Zeitlimits tatsächlich anwenden.

**Abnahme:** DB-Ausfall, falsche Credentials, fehlendes Schema und Pool-Erschöpfung erzeugen begrenzte, nachvollziehbare Fehler. Kein Fallback auf SQLite oder Dev-Secrets. DB-abhängige Readiness liefert bei fehlender Arbeitsfähigkeit 503; der bisherige /api/health-Liveness-Vertrag bleibt eindeutig. Pro Instanz ist ein Connection-Budget festgelegt, das zusammen mit anderen Instanzen und Admin-Reserve zur Serverkapazität passt.

Technische Startwerte für den neuen Adapter sind maximal zehn DB-Verbindungen pro Instanz, fünf Sekunden Acquire-Timeout, ein MiB JSON-Body und 30 Sekunden HTTP-Request-Deadline. Diese Werte werden konfigurierbar, auf gültige Bereiche geprüft und in Grenzwerttests nachgewiesen. Migration, Backup und spätere Bulk-Endpunkte benötigen eigene begründete Limits; sie dürfen die Standardgrenzen nicht pauschal abschalten. Teure Passwortarbeit läuft außerhalb asynchroner Worker mit begrenzter Parallelität. Der Connection-Pool ersetzt weder Request-Limits noch ein Ressourcenbudget.

PostgreSQL-Verbindungen außerhalb ausdrücklich isolierter Entwicklung/CI prüfen TLS einschließlich Gegenstellenidentität. Eine dokumentierte Testausnahme gilt nur für temporäre synthetische Testumgebungen und darf nicht still zur normalen Netzwerk-Konfiguration werden. Datenbank-Ports werden in Compose nicht ohne Notwendigkeit öffentlich veröffentlicht.

Kritische Queries werden gegen das echte Schema ausgeführt. Verwendete SQLx-Query-Makros zusätzlich mit zum Projekt passender SQLx-CLI vorbereiten/prüfen; dynamische Queries benötigen weiterhin Integrationstests.

**Übergabe:** Adapter und Start-/Migrationsbefehle; reproduzierbare Konfigurations- und Ausfalltests.

### PG-05 — Auth, Berechtigungen und Audit auf PostgreSQL

**Eingang:** PG-03 und PG-04.

**Arbeit:** Bestehendes Auth-/Rollen-/Audit-Verhalten portieren und die Fälle der Sicherheitsmatrix auf PostgreSQL ausführen. Sitzung/Widerruf nicht auf nachlaufende Replicas auslagern. Audit-Kopf mit DB-Transaktion/Lock vor konkurrierender Fortschreibung schützen; ein Prozess-Mutex reicht für mehrere Instanzen nicht.

**Abnahme:** Abgelaufene, manipulierte, zweckfalsche und widerrufene Tokens werden abgewiesen. Nach Logout/Neustart bleiben Tokens ungültig. Paralleler Refresh hat genau einen Gewinner. Kontosperre, Deaktivierung und Rollenänderung wirken serverseitig. Ein Client kann keine Rollen oder Objektberechtigungen über Query/Body wählen.

Nebenläufige Writer unterschiedlicher Prozesse/DB-Verbindungen erzeugen eine überprüfbare Audit-Kette ohne stillen Verlust. Inhalte werden neu gehasht. Eine gescheiterte erforderliche Audit-Speicherung erlaubt keine Datenfreigabe. Künftige fachliche Mutation und Audit gehören in dieselbe Transaktion; Sicherheitsmaßnahmen wie Widerruf/Sperre dürfen bei Audit-Ausfall nicht versehentlich zurückgenommen werden. Die festgelegten Ausnahmen und ihre Fehlerantworten ausdrücklich testen.

**Grenze:** Ohne externen Prüfanker wird weiterhin kein Schutz vor komplettem Umschreiben/Abschneiden durch einen DB-Administrator behauptet. Externe Ziele nur nach konfiguriertem, autorisiertem Betriebskonzept verwenden.

**Übergabe:** Fallzuordnung zur Matrix, Rechte-/Race-/Neustart-Tests und benannte Restrisiken für den unabhängigen Reviewer.

### PG-06 — PostgreSQL-Backup und verifizierter Restore

**Eingang:** PG-03 und PG-04; parallel zu PG-05 möglich.

**Arbeit:** PostgreSQL-tauglichen konsistenten Snapshot mit kompatiblen pg_dump/pg_restore-Werkzeugen vorsehen, mit age verschlüsseln und Artefakt-/Prüfsummenschema vereinheitlichen. SQLite-backup.py nicht als PostgreSQL-Verfahren umbenennen. Rollen, Schema, benötigte Erweiterungen und separat gesicherte Anwendungsschlüssel im Restore-Verfahren berücksichtigen.

**Abnahme:** Frische, laufend geänderte synthetische Test-DB sichern und in eine zweite isolierte DB mit definierten Rollen wiederherstellen. Prüfsumme, Entschlüsselung, Schema, fachliche Kontrollwerte und echte Anmeldung nach Restore prüfen. Auch fehlendes Archiv, falscher Schlüssel, manipuliertes Archiv, falsche Rolle und unvollständiger Restore müssen fehlschlagen.

Klartext-Snapshots nur in privater temporärer Ablage; Fehlerpfade räumen auf. Es werden nur selbst erzeugte Testressourcen entfernt. Kein behauptetes Point-in-Time-Recovery oder RPO/RTO ohne entsprechende Implementierung und Messung.

**Übergabe:** Ausführbare Backup-/Restore-Befehle, Schlüssel-/Rechtekonzept und bestandenes tatsächliches Restore-Protokoll.

### PG-07 — Anwendung auf PostgreSQL umstellen

**Eingang:** PG-05.

**Arbeit:** Root-Server und Admin-CLI auf den geprüften Workspace/Pg-Adapter anschließen. Den Übergang von der SQLite-Referenz explizit dokumentieren. Ein gegebenenfalls verbleibender SQLite-Referenzmodus ist ausdrücklich benannt und niemals automatischer Ersatz für eine fehlende PostgreSQL-Verbindung.

**Abnahme:** Neue Testinstallation → Migration mit Migrationsrolle → Benutzer-Bootstrap → Start mit Anwendungsrolle → Health/Readiness → Login/Refresh/Logout → Neustart funktionieren über echte HTTP-Anfragen. DB-Verbindungsfehler und leere Secrets verhindern eine betriebsbereite Instanz. Daten-Platzhalter liefern nach Auth-/Rollenprüfung weiterhin 501. Keine Fake-Patientendaten, um eine GUI als fertig aussehen zu lassen.

**Übergabe:** Einzelner ausführbarer PostgreSQL-Pfad, Smoke-Test und eindeutige Dokumentation des verbleibenden SQLite-Status.

### PG-08 — CI, Container und Betriebsprüfungen

**Eingang:** PG-06 und PG-07.

**Arbeit:** CI mit echter PostgreSQL-Instanz und begrenzter App-Rolle versehen. Workspace-, Rechte-, Race-, Restore- und Container-Prüfungen als erforderliche Jobs ausführen. Dependency-/Secret-Scans mit reproduzierbar ausgewählten Versionen ergänzen und konkrete Funde beheben/triagieren. Keine stillen Skip-Erfolge bei fehlender Datenbank oder fehlendem age.

**Abnahme:** Gebautes Image startet ohne root gegen PostgreSQL, verwendet keine eingebauten Secrets und besteht den vollständigen Lebenszyklus einschließlich Neustart und Restore. Compose-Konfiguration, Health/Readiness und Betriebspfade passen zur tatsächlich verwendeten Architektur. Abhängigkeiten und Container-Basis haben dokumentierte Prüfstände. Für einen systemd-Nachweis eine echte unterstützte Testumgebung verwenden; reine Unit-Syntaxprüfung gesondert benennen.

Performance-Messungen benötigen Datenumfang, Parallelität, Hardware und Perzentile. Die langfristigen Ziele aus dem Fahrplan sind erst nach Messung als erreicht markierbar. Ein langsamer Shared-CI-Runner ist kein Beleg einer klinischen Lastfreigabe.

**Übergabe:** Grüne CI-URLs mit Commit-SHA, Testberichte und klar benannte noch nicht geprüfte Betriebsvarianten.

### PG-09 — Unabhängiges Security-Review

**Eingang:** Integrierter Stand aus PG-05, PG-06 und PG-08.

**Arbeit:** Anderer Reviewer als der Autor prüft Diff, erreichbare Codepfade, Grants, Datenflüsse, Sicherheitsmatrix und tatsächliche Testergebnisse. Kritische Negativfälle unabhängig reproduzieren statt ausschließlich Autorberichte zu übernehmen. Geheimnisse oder echte Patientendaten sind dafür nicht erforderlich.

**Abnahme:** Kein unbehandelter Befund mit Zugriffsumgehung, Token-Wiederverwendung, Geheimnisabfluss, unkontrollierter DB-Berechtigung, Audit-Verlust oder Datenverlust. Sonstige Funde haben Schwere, betroffenen Pfad, Reproduktion und Zuständigkeit. Der Reviewer nennt getestete SHA, Umfang, Ergebnisse und verbleibende Grenzen. Ein Modellkonsens ohne solche Nachweise ist keine Freigabe.

Korrekturen gehen an das zuständige Paket zurück. Sicherheitsrelevante Nachänderungen werden erneut geprüft; Tests/Scans werden nicht abgeschwächt. Bei fehlendem unabhängigen Reviewer bleibt ausschließlich dieses Freigabegate offen; sichere Implementierung und sonstige Tests dürfen weiterlaufen.

**Übergabe:** Konkreter Reviewbericht und Freigabeempfehlung oder begründete Rückgabe.

### PG-10 — Integrationsabnahme und wahrheitsgemäße Übergabe

**Eingang:** PG-09 ohne offene blockierende Sicherheitsbefunde.

**Arbeit:** Gegen den finalen Integrationsstand die betroffenen Prüfungen ausführen, Dokumentation/Status konsistent aktualisieren und PR zur Integration bereitstellen. Belegte Ergebnisse von fehlenden Nachweisen trennen.

**Abnahme:** Sämtliche für diese Etappe erforderlichen Prüfungen bestehen. Der Zwischenstand kann aus einem sauberen Checkout aufgebaut und mit synthetischen Daten betrieben/wiederhergestellt werden. Keine zweite veraltete Auth-/DB-Konfiguration ist versehentlich erreichbar. Fachliche CRUD-, GUI-, MEDICO-, Zertifizierungs- und Langzeitbetriebsziele bleiben als solche gekennzeichnet.

**Übergabe:** Finaler Commit-/PR-Verweis, Prüfmatrix, unabhängiges Review und Restrisiken. Kein automatischer Merge, Release oder Produktivbetrieb aus der technischen Abnahme ableiten.

## 7. Prüfkommandos und Ausführung

Nach Herstellung des Workspaces mindestens:

```bash
cargo metadata --locked --no-deps --format-version 1
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --workspace --locked
python3 scripts/check_docs.py
```

Die im Paket ergänzten echten Pg-, Rechte-, Restore-, Container- und Scan-Befehle zusätzlich ausführen. PG-08 hält deren genaue Voraussetzungen und reproduzierbare Versionen im Entwicklerhandbuch fest. Erforderliche Jobs dürfen beim Fehlen dieser Voraussetzungen keinen grünen Erfolgsersatz liefern.

Verwendet das Projekt gültige optionale Features, deren unterstützte Kombinationen einzeln prüfen. Kein blindes all-features-Kommando als Ersatz für die tatsächlich unterstützte Matrix. SQLx-Offline-Metadaten werden nicht erfunden oder als Ersatz für echte DB-Ausführung ausgegeben.

## 8. Fertigstatus und Übergabeformat

Zustände: **geplant → in Arbeit → implementiert → verifiziert → unabhängig geprüft → fertig**. **blockiert** bezeichnet einen konkreten fehlenden Eingang oder Nachweis. Nicht sicherheitsrelevante Pakete benötigen das unabhängige Security-Gate nur, wenn ihre Änderung tatsächlich eine Sicherheitsgrenze beeinflusst.

Fertig bedeutet: Vertrag erfüllt, notwendige Fehlerfälle behandelt, relevante Prüfungen tatsächlich bestanden, erforderliches unabhängiges Review vorhanden, Dokumentation konsistent und keine verschwiegene Einschränkung. Geänderte Codezeilen oder ein vorhandenes Testfile sind kein eigener Fertignachweis.

Übergabevorlage; null und leere Arrays werden durch tatsächliche Ergebnisse ersetzt, nicht durch erfundene Erfolgswerte:

```json
{
  "task_id": "PG-00",
  "base_sha": "<gepruefte-basis-sha>",
  "head_sha": "<ergebnis-sha>",
  "status": "geplant",
  "changed_paths": [],
  "checks": [],
  "security_reviewer": null,
  "blocking_findings": [],
  "remaining_risks": [],
  "next_task": "PG-01"
}
```

Jeder Prüfdatensatz nennt Befehl/Testfall-ID, Umgebung, Commit, tatsächlichen Exit-Code/Status und einen CI-/Lognachweis ohne Secrets. Reviewdatensätze nennen Reviewer, unabhängig geprüften Umfang und Ergebnis. Fehlende Fach-/Betriebsnachweise bleiben im Ergebnistext sichtbar.

## 9. Startauftrag für den ersten Umsetzungsagenten

> Übernimm PG-00. Lies Root-AGENTS.md, diesen Auftrag und die Sicherheits-Abnahme. Ermittle den aktuellen geprüften Stand von PR #1 bzw. dessen integrierten Nachfolger. Sichere fremde Änderungen, inventarisiere Workspace-/Schema-/Auth-Kopien und ordne die vorhandenen Regressionen der Sicherheitsmatrix zu. Liefere eine konkrete Basis-/Lückenübersicht mit tatsächlichen Prüfverweisen. Bereite danach PG-01 mit eindeutigem Daten-/Crate-Vertrag vor; beginne kein fachliches CRUD, keine GUI und keine klinischen MEDICO-Schreiboperationen. Stelle nur Fragen zu tatsächlich fehlenden, auftragsrelevanten Informationen und setze unabhängige Arbeit fort.

## 10. Nach dieser Etappe

Erst nach der PostgreSQL-Abnahme folgen fachliches Patienten-CRUD mit Objektberechtigungen, danach GUI/Arbeitsabläufe auf diesem Vertrag. MEDICO erhält eine eigene Etappe mit bestätigter Version, ISiK-Profilen, Sandbox und fachlichem Migrations-/Rollback-Nachweis. Klinische Entscheidungsunterstützung, echte Patientendaten und eine Produktivfreigabe sind dadurch nicht automatisch autorisiert.
