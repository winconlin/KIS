# KIS — Verbindliche Sicherheits-Abnahme für die Agenten-Etappe

> **Hinweis (06.11.2026):** Teile dieses Dokuments beschreiben den entfernten SQLite-Prototyp oder ein Zielbild. Maßgeblich für den Ist-Zustand sind [INSTALLATION](INSTALLATION.md), [API](API.md), [DATABASE](DATABASE.md) und [REVIEW_STATUS](REVIEW_STATUS.md).


Stand: 02.11.2026 · Prüfvorgaben, keine bereits erteilte Freigabe

## 1. Anwendung und Nachweis

Diese Matrix ergänzt [AGENTS.md](../AGENTS.md) und [AGENTEN_AUFTRAG.md](AGENTEN_AUFTRAG.md). Die bisherige Basis besitzt 37 Rust- und sechs Backup-Tests sowie einen Container-Smoke-Test. Das ersetzt die hier geforderten PostgreSQL-Nachweise nicht.

**Pflicht vor PG-10:** Alle für die technische PostgreSQL-Basis geltenden Fälle der Abschnitte 3 bis 7. Ein Fall darf nur begründet nicht anwendbar sein, wenn die betroffene Funktion in dieser Etappe weder implementiert noch als vorhanden behauptet wird. Der Reviewer bestätigt diese Abgrenzung. Fehlende Infrastruktur, übersprungene Tests oder ein mock-basierter Ersatz sind keine bestandene PostgreSQL-Abnahme.

**Nachgelagerte Freigaben:** Die zusätzlichen Gates in Abschnitt 8 gelten vor Einführung der jeweiligen fachlichen Funktion oder Betriebsvariante. MEDICO-Profile, GUI und klinische Workflows blockieren die Basisetappe nicht, solange sie weiterhin erkennbar fehlen und keine entsprechenden Datenoperationen freigeschaltet werden.

Jeder Nachweis enthält Testfall-ID, Commit, Umgebung, echte Aktion, erwartetes und beobachtetes Ergebnis sowie CI-/Logverweis ohne Secrets. Ein Test muss das Sicherheitsversprechen prüfen, nicht nur eine interne Hilfsfunktion oder einen erfolgreichen Build. Gleichzeitigkeit durch echte überlappende Operationen mit getrennten Verbindungen/Sessions herstellen; keine bloß nacheinander ausgeführten Aufrufe als Race-Test ausgeben.

## 2. Bedrohungen und Vertrauensgrenzen

| Grenze | Relevantes Risiko | Erforderliche Kontrolle |
|---|---|---|
| Client → API | Gefälschte Identität, Rollen-/ID-Manipulation, ungültige Eingaben, Ressourcenverbrauch | Tokenprüfung, serverseitige Policy, Validierung, begrenzte Requests/Arbeit |
| API → Domain/Storage | Objektzugriff an der Policy vorbei, widersprüchliche DTOs | Ein Dienstvertrag und eine geprüfte Berechtigungsgrenze |
| Runtime → PostgreSQL | Zu weitreichende DB-Rechte, SQL-Injection, Datenverlust | Separate Rollen, parametrisierte Werte, echte Constraints, Transaktionen |
| Mehrere Requests/Instanzen → Zustand | Refresh-Replay, verlorene Änderungen, Audit-Verzweigung | Atomare Updates, Versionen, definierte Locks und gemeinsame persistente Wahrheit |
| Speicher/Logs → Betrieb | Geheimnisse oder unnötige Patientendaten in Logs/Backups | Datenklassifikation, redigierte Fehler, begrenzte Protokolle, Verschlüsselung |
| Backup → Restore | Scheinbar gültige, leere, unvollständige oder nicht entschlüsselbare Sicherung | Tatsächlicher Restore in isolierte PostgreSQL-DB plus Inhalts-/Anwendungsprüfung |
| Build/Dependencies → Artefakt | Verwundbare Pakete, eingeschleuste Secrets, ungetestete Images | Reproduzierbare Versionen, Scans, gebautes Image tatsächlich testen |

Der Datenbankadministrator bleibt eine eigene Vertrauensgrenze. Eine lokale Hash-Kette allein schützt nicht vor vollständigem Neuberechnen oder Abschneiden durch diesen Akteur. Entsprechende Aussagen benötigen einen unabhängigen Prüfanker und getrennte Zuständigkeiten.

## 3. Authentifizierung, Sitzungen und Autorisierung

| ID | Sicherheitsfall | Erwartetes Ergebnis auf PostgreSQL |
|---|---|---|
| A-01 | Fehlendes, manipuliertes, abgelaufenes oder mit unzulässigem Algorithmus signiertes Token | 401 vor Datenfreigabe; keine internen Schlüssel-/Parserdetails in der Antwort |
| A-02 | Refresh-Token als Bearer für eine Datenroute | 401; keine Daten und kein Umgehen des Access-Token-Zwecks |
| A-03 | Access-Token oder widerrufenes Token am Refresh-Endpunkt | Abgewiesen gemäß vereinbartem Fehlervertrag; kein neues Token-Paar |
| A-04 | Logout, danach neue Anfrage und erneuter Prozessstart | Beide Tokens der widerrufenen Sitzung bleiben für neue Anfragen ungültig |
| A-05 | Zwei gleichzeitig gestartete Refresh-Anfragen mit demselben Token | Genau eine Rotation gelingt; alte Sitzung ist dauerhaft verbraucht |
| A-06 | Rollenentzug, Kontodeaktivierung oder gelöschtes Konto bei vorhandenen Tokens | Keine weitere Nutzung entzogener Rechte; deaktivierte/gelöschte Benutzer erhalten auch per Refresh keinen Zugang |
| A-07 | Mehrere gleichzeitig falsche Passwörter | Keine verlorenen Fehlversuche; die festgelegte Kontosperre wird wirksam |
| A-08 | Unbekannter Benutzer, kaputter Passwort-Hash, falsches Passwort | Kein Panic und kein Secret-Abfluss; nachvollziehbare Ablehnung, keine erfundene erfolgreiche Sitzung |
| A-09 | Rollenänderung über Query-/Body-/Headerwerte | Client kann die geforderte Berechtigung nicht auswählen oder erhöhen |
| A-10 | Gültiger Benutzer mit unzureichender Route-/Service-Rolle | 403 bzw. dokumentiertes konsistentes Ablehnungsverhalten vor geschützter Operation |
| A-11 | Fehlendes/kurzes Secret oder falsches Masterkey-Format | Kein betriebsbereiter Start mit Dev-Default oder Ersatzschlüssel |
| A-12 | Datenbankfehler während Issue/Rotation | Keine teilweise gültige unprotokollierte Sitzung; Rollback-/Widerrufsverhalten ist getestet |
| A-13 | Nicht implementiertes MFA-Setup oder fachlicher Handler | Bleibt als 501 erkennbar; keine scheinerfolgreichen Daten-/Setupantworten |
| A-14 | Autorisierungsentscheidung bei Replica-Verzögerung | Maßgeblicher Schreibstand entscheidet; kein Rückfall auf veraltete Rollen oder widerrufene Sessions |

Die bisher verwendete feste HS256-Algorithmuswahl wird nicht ohne begründete Architekturentscheidung gegen beliebige Algorithmen oder Key-Typen ausgetauscht. Eine Migration zu einem anderen Verfahren muss alte/neue Tokenzwecke, Schlüsselrotation und Kompatibilität eigens prüfen.

## 4. Datenmodell, SQL und Nebenläufigkeit

| ID | Sicherheitsfall | Erwartetes Ergebnis |
|---|---|---|
| D-01 | Datensatz mit nicht existierender Referenz | PostgreSQL-FK weist ihn zurück; kein Ersatzdatensatz wird heimlich erzeugt |
| D-02 | Gleicher externer Identifier in derselben Quelle/Ressource/System | Definierter Unique-Konflikt; kein Duplikat und kein unbeabsichtigtes Überschreiben |
| D-03 | Gleicher ID-Wert in anderer Quelle oder anderem Identifier-System | Getrennte Zuordnung; keine falsche Patientenzusammenführung |
| D-04 | Terminende vor/gleich Beginn, unzulässiger Status, ungültige Version | Schema-/Vertragsverletzung wird abgewiesen |
| D-05 | Fehlendes oder unvollständiges Geburtsdatum | Definierter nullable-/Importpfad; kein erfundener Stichtag |
| D-06 | Zwei Änderungen mit derselben erwarteten Version | Genau eine Änderung commitet; die andere erhält einen nachvollziehbaren Konflikt |
| D-07 | Fehlgeschlagene Mehrschrittänderung | Keine unvollständigen fachlichen Daten; Transaktion wird korrekt zurückgesetzt |
| D-08 | Löschung eines referenzierten Patienten/Arztes | Kein Verlust von Arztidentitäten, Verordnungen oder Audit-Historie durch unbeabsichtigte Kaskaden |
| D-09 | SQL-Metazeichen in Werten, unzulässige dynamische Sortier-/Feldnamen | Werte bleiben Parameter; Strukturelemente kommen nur aus serverseitiger Allowlist |
| D-10 | Frische Migration, Wiederholung, absichtlich beschädigter Migrationssatz | Korrekte Erstinstallation/No-op; beschädigter Stand wird nicht ignoriert |
| D-11 | Änderung einer angewandten Migration/Prüfsumme | Sichtbarer Fehler statt stiller Umschreibung der SQLx-Historie |
| D-12 | Rust-/SQL-/DTO-Abbildung einschließlich NULL/UTC/DATE/Status | Roundtrip und echte Queries erhalten die vereinbarte Bedeutung |

Klinische Plausibilität wird durch solche technischen Constraints nicht vollständig bewiesen. Insbesondere sind ein Dosierungstext und ein Fremdschlüssel noch kein Verordnungs- oder Allergiesicherheitskonzept.

## 5. Datenbankrechte und Audit

| ID | Sicherheitsfall | Erwartetes Ergebnis |
|---|---|---|
| P-01 | App-Zugang inspizieren | Kein Superuser, kein Rollen-/Schema-/Tabellen-Owner, kein CREATEDB/CREATEROLE |
| P-02 | App führt CREATE/DROP/ALTER/TRUNCATE außerhalb ihres Vertrags aus | Von PostgreSQL verweigert; Rechteprüfung läuft mit echter App-Rolle |
| P-03 | App versucht UPDATE/DELETE/TRUNCATE auf gespeicherte Audit-Einträge | Verweigert; keine geerbten/öffentlichen Rechte heben die Begrenzung auf |
| P-04 | Runtime ohne Migrations-Credentials | Normalbetrieb funktioniert nach separater Migration; fehlendes Schema erzeugt keine betriebsbereite Readiness |
| L-01 | Mindestens 100 überlappende Audit-Schreibvorgänge mit unabhängigen DB-Verbindungen | Alle bestätigten Ereignisse vorhanden; Kette und Inhalt überprüfbar, kein Fork oder stiller Verlust |
| L-02 | Audit- und Session-/fachliche Writes gleichzeitig | Definierte Lock-Reihenfolge und Fehlerbehandlung; keine verlorenen Sicherheitszustände |
| L-03 | Audit-Feld oder mittlerer Ketteneintrag administrativ manipuliert | Verifikationsfunktion schlägt tatsächlich fehl; nicht nur Hash-Hilfsfunktion testen |
| L-04 | Abbruch/Neustart zwischen Schreiben und Antwort | Nur bestätigte Commit-Zustände werden verwendet; keine erfundene erfolgreiche Operation |
| L-05 | Audit-DB nicht beschreibbar | Geschützte Datenoperation wird abgelehnt; kein fire-and-forget-Fallback |
| L-06 | Fachliche Mutation scheitert nach Audit-Anlage bzw. Audit scheitert nach Mutation | Vereinbarte gemeinsame Transaktion verhindert ein unvollständiges fachliches Ergebnis |
| L-07 | Audit-Fehler bei Widerruf/Kontosperre | Sicherheitsmaßnahme wird nicht versehentlich zurückgenommen; Fehler-/Alarmierungsweg und Antwort sind ausdrücklich festgelegt und getestet |
| L-08 | Zwei Anwendungsinstanzen/Writer verwenden denselben Audit-Kopf | DB-seitige Koordination schützt die Kette; ein lokaler Prozess-Lock wird nicht als Mehrinstanz-Nachweis ausgegeben |
| L-09 | JSONL-Kopie fällt aus | Maßgeblicher DB-Zustand und Fehler bleiben sichtbar; Dateikopie wird nicht als vollständig ausgegeben |

Die Prüfung von Audit-Integrität und normalen DB-Grants sind getrennt: Für einen Manipulationstest darf ein isolierter Test-Administrator gezielt eingreifen. Daraus folgt keine Erlaubnis, der Anwendung dieselben Rechte zu geben. Sequenzlücken nach einem PostgreSQL-Rollback sind technisch möglich und nicht automatisch der Nachweis einer gelöschten Audit-Zeile.

Ohne unabhängigen Anker bleiben das Abschneiden des Kettenendes und vollständiges Neuberechnen durch einen DB-Administrator benannte Restrisiken. Vor einer gegenteiligen Zusage muss ein eigener Test gespeicherte vertrauenswürdige Checkpoints gegen manipulierte Stände prüfen.

## 6. Fehlerfälle, Ressourcen und Wiederherstellung

| ID | Sicherheitsfall | Erwartetes Ergebnis |
|---|---|---|
| F-01 | PostgreSQL fehlt, falsche Credentials oder Pool erschöpft | Begrenzter Fehler/Timeout; keine erfolgreiche Readiness und kein SQLite-Fallback |
| F-02 | Viele ungültige Auth-Anfragen | Festgelegtes Limit liegt vor teurer Verarbeitung; 429 mit sinnvollem Retry-After |
| F-03 | Gefälschte Forwarded-/X-Forwarded-For-Werte | Ohne vertrauenswürdig konfigurierte Proxy-Grenze kein Umgehen des IP-Limits |
| F-04 | Unzulässige Body-Größe bzw. ungültiger Inhalt | Definierte Ablehnung, begrenzter Ressourcenverbrauch; Grenzwerte werden tatsächlich angewendet |
| F-05 | Nicht freigegebener Browser-Origin | Keine permissive Datenfreigabe über CORS |
| F-06 | Fehler-/Debug-Logging bei Auth-, DB- und Clientfehlern | Keine Tokens, Schlüssel, Passwörter, Verbindungs-URLs mit Secret oder unnötigen Patientendaten |
| F-07 | Falsches/fehlendes Datenbank-Zertifikat oder falsche Gegenstellenidentität außerhalb isolierter Testausnahme | Verbindung wird abgelehnt; kein Abschalten der TLS-Verifikation als Fehlerbehebung |
| F-08 | Viele teure Passwortprüfungen, langsame Queries oder zu viele offene Requests | Festgelegte Parallelitäts-/Zeit-/Größengrenzen greifen; keine unbegrenzte Queue und keine blockierte gesamte Runtime |
| B-01 | Online-Snapshot mit gleichzeitigen synthetischen Änderungen | Konsistenter PostgreSQL-Snapshot, überprüfbare Referenzen und Kontrollwerte |
| B-02 | Archiv entschlüsseln und in zweite isolierte DB einspielen | Schema, Constraints, erwartete Inhalte, Rollen und echte Anmeldung nach Restore funktionieren |
| B-03 | Fehlende, leere oder nur teilweise erzeugte Sicherung | Fehler; keine neu angelegte leere DB als erfolgreicher Ersatz |
| B-04 | Falscher Schlüssel, beschädigtes Archiv oder falsche Prüfsumme | Fehler vor Freigabe der Wiederherstellung |
| B-05 | Snapshot mit falscher Rolle oder fehlenden Schema-/Rollenbestandteilen | Kein scheinerfolgreicher Restore; Abweichung wird erkannt |
| B-06 | Abbruch bei Backup/Restore | Temporärer Klartext und Testressourcen werden sicher behandelt; keine fremden Daten gelöscht |
| B-07 | Wiederherstellung verschlüsselter Werte | Separat bereitgestellter passender Anwendungsschlüssel erlaubt tatsächliche Entschlüsselung; Schlüsselverlust wird nicht durch einen Dev-Key verdeckt |

Ressourcenlimits müssen mit messbaren konkreten Werten dokumentiert werden. Grenzwerte sind keine angegebene Eigenschaft, solange sie im ausführenden Server/Proxy nicht nachweisbar greifen. Ein aktueller Health-Check allein ist kein Backup- oder Bereitschaftsnachweis.

## 7. Lieferkette, Image und unabhängiges Review

| ID | Sicherheitsfall | Erwartetes Ergebnis |
|---|---|---|
| C-01 | Sauberer Checkout mit gepinnter Toolchain/Lockfile | Reproduzierbare Workspace-Prüfungen ohne lokale versteckte Artefakte |
| C-02 | Dependency-/Secret-Scan | Tatsächlich ausgeführt; Funde konkret behoben oder mit benannter Zuständigkeit/Umfang/Frist bewertet, nicht pauschal ignoriert |
| C-03 | Fertiges Container-Image | Non-root, keine eingebauten Secrets/Testdaten, notwendige Werkzeuge vorhanden |
| C-04 | Container gegen PostgreSQL mit App-Rolle | Bootstrap-/Start-/Auth-/Neustartpfad und erforderlicher Restore funktionieren tatsächlich |
| C-05 | Dienstkonfiguration | Schreibpfade und Secret-Zugriff passen zum Dienstbenutzer; Syntaxprüfung und echter systemd-Lauf werden unterschieden |
| C-06 | Nicht ausgeführte oder übersprungene Pflichtprüfung | Gate bleibt offen/blockiert; kein grüner Ersatz durch Mocks oder Konfiguration allein |
| C-07 | Sicherheitsreview | Anderer Reviewer als Autor, konkrete SHA/Fundstellen/Negativnachweise und getrennte Restrisiken |
| C-08 | Sicherheitsrelevante Änderung nach Review | Betroffene Tests und unabhängiges Review werden erneuert |

Ein Scanner-Ergebnis ersetzt keine fachliche Bewertung, ein unabhängiges Modell ersetzt keine Belege und ein grünes Image-Build beweist keinen gestarteten Dienst. Bestätigte Zugriffsumgehung, Token-Replay, Geheimnisabfluss, unkontrollierte DB-Rechte, verpflichtender Audit-Verlust oder Datenverlust blockieren die technische Abnahme bis zur Korrektur.

Ein Implementierer darf sich keine Ausnahme von seinem eigenen Sicherheitsbefund erteilen. Ausnahmen oder Nicht-Anwendbarkeit benötigen konkrete Evidenz, abgegrenzte betroffene Version/Pfade, Zuständigkeit, gegebenenfalls Frist und Bestätigung durch die unabhängige Reviewrolle. Pflichtjobs nicht über continue-on-error, pauschale Scanner-Ignores, deaktivierte Prüfungen oder ein erfolgreiches Exit nach einem Skip umgehen. Fehlende Rechte zur Einrichtung solcher Repository-Gates werden als offener Betriebs-/Administrationspunkt benannt, nicht als konfigurierte Branch-Protection ausgegeben.

## 8. Zusätzliche Gates vor späteren Funktionen

| Funktion/Betriebsvariante | Vor Freischaltung zusätzlich nachzuweisen |
|---|---|
| Patienten-/Termin-/Medikations-CRUD | Objekt-/Fall-/Teamrechte, fremde IDs und direkte Servicezugriffe, Feldvalidierung, Transaktions-/Audit-Verhalten und Konflikte |
| Multi-Tenancy | Verbindliches Organisationsmodell; jeder Zugriff einschließlich Suche, Export, Audit und Hintergrundjobs hält Mandantengrenzen ein |
| MFA | Gespeicherter und bestätigter Setup-Ablauf, Secret-Schutz, falsche/wiederverwendete Codes und Recovery-Politik |
| Spaltenverschlüsselung/Rotation | Kein Klartext-Duplikat; falsche Datensatz-ID/Nonce-/Tag-Manipulation scheitert; Schlüsselversionen und echter Re-Encrypt-/Restore-Test |
| Verteilte Skalierung | Gemeinsamer Widerruf/Audit, passende Limits, Replica-Verzögerung, Ausfälle und dokumentierter Lasttest |
| FHIR/MEDICO | Bestätigtes Gegenstellenprofil, vollständiges Paging, wiederholte Versionen, persistenter Commit-/ACK-Cursor, Retry-/Idempotenz-/Löschregeln und fachliche Abnahme |
| GDT/HL7/DICOM/XDS/TI | Exakte Version/Profile, valide Nachrichten, Fehler-/Retry-/Quarantänepfade und Gegenstellen-/Konformitätstests |
| Externes LLM | Autorisierter Datenfluss und Anbieter-/Datenschutzgrundlagen; keine automatische Weitergabe von Gesundheitsdaten durch bloße Pseudonymisierung |
| Klinische Entscheidungshilfe | Zweckbestimmung, fachliche Validierung, Risikomanagement und anwendbare regulatorische Prüfung |
| Produktivbetrieb | Betreiberverantwortung, TLS/Schlüsselablage, Aufbewahrung/Löschung, Alarmierung, Restore-Übung, Incident-Prozess und tatsächlich geprüfte Betriebsvariante |

Diese Punkte werden nicht zu pauschalen Konformitäts- oder Fristzusagen zusammengefasst. Der jeweilige implementierte und geprüfte Umfang bleibt entscheidend.

## 9. Abnahmebericht

Der unabhängige Reviewer liefert:

1. Geprüfte Commit-SHA, Pakete und tatsächlich erreichbare Code-/Datenpfade.
2. Matrix-IDs mit echter Umgebung und Ergebnis; nicht anwendbare oder fehlende Fälle mit Begründung.
3. Pro Befund Schwere, Fundstelle, Reproduktion, Auswirkung und konkrete Korrekturanforderung.
4. Nachprüfung der Korrekturen und Sicherheitsgrenzen, die weiterhin nicht nachgewiesen sind.
5. Eindeutige Aussage: technische Abnahme empfohlen oder wegen benannter Punkte blockiert. Keine automatische klinische Freigabe.

Erst Koordination/Integration setzt PG-10 auf fertig, nachdem Implementierung, tatsächliche Prüfungen und erforderliches unabhängiges Review zusammen vorliegen.
