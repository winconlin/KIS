# KIS — Sicherheit

Stand: 02.11.2026 · Entwicklungsprototyp

Dieses Dokument beschreibt vorhandene Maßnahmen und offene Anforderungen. Es ist kein Nachweis einer DSGVO-, MDR-, ISO-27001- oder Schnittstellenzertifizierung.

## Authentifizierung und Sitzungen

- Passwörter werden mit Argon2id gespeichert; die Parameter stammen aus der verwendeten Bibliothekskonfiguration.
- JWTs verwenden HS256 mit einem zufälligen Secret aus der Umgebung. Default: Access 15 Minuten, Refresh 24 Stunden.
- Jede Sitzung speichert Refresh-JTI und Access-JTI. Datenrouten prüfen Token-Zweck, Sitzung, aktives Konto und aktuelle Rolle.
- Logout widerruft das eigene Token-Paar dauerhaft. Rotation verbraucht die bisherige Sitzung atomar.
- Fünf falsche Passwörter sperren ein Konto standardmäßig für 15 Minuten.
- Der API-Limiter liegt vor der Authentifizierung: 100 API-/10 Auth-Anfragen je Minute und tatsächlicher TCP-IP.
- MFA-Setup ist noch nicht fertig und liefert HTTP 501. Es wird kein ungespeichertes Secret als betriebsbereites Setup ausgegeben.

Ein Inaktivitäts-Timeout, ein Concurrent-Login-Limit, eine sichere browserseitige Tokenablage und ein vollständiger MFA-Einrichtungs-/Recovery-Ablauf sind offen. Vorhandene manuell eingerichtete MFA-Secrets müssen zum verwendeten Base32-TOTP-Format passen.

## Autorisierung

Rollenanforderungen kommen aus dem Servercode, nicht aus Queryparametern. Die aktuelle Matrix steht in [API.md](API.md). Ein authentifizierter Arzt kann beispielsweise keine Patientenanlage freischalten, indem er `?role=reception` sendet.

Die Datenhandler sind Platzhalter. Vor ihrer fachlichen Umsetzung fehlen insbesondere Objektberechtigungen nach Behandlungsteam/Fall, Berechtigungen für besondere Datenkategorien und ein abgestimmter Notfallzugriff.

Cross-Origin-Anfragen erhalten keine permissiven CORS-Header. Der Server akzeptiert Forwarded-/X-Forwarded-For-Header nicht als Beleg einer Client-IP.

## Audit

Die Datenbank ist das maßgebliche Protokoll. Die API wartet auf einen Audit-DB-Commit; bei Ausfall wird ein geschützter Datenendpunkt mit 503 blockiert. Aufrufe werden innerhalb einer Instanz serialisiert und bei Überlast nicht still verworfen.

Neue Einträge verwenden Hash-Version 2 mit unterscheidbaren Nullwerten und längencodierten Feldern. Die Verifikation prüft Datensatzinhalt und Verkettung. Legacy-Version 1 bleibt lesbar. Eine ergänzende JSONL-Datei kann bei einem Dateifehler hinter dem gespeicherten DB-Stand zurückliegen; ein solcher Fehler wird geloggt.

**Grenzen:** Der gemeinsame DB-Benutzer ist technisch weiterhin in der Lage, Daten zu verändern. Eine lokale Hash-Kette verhindert weder das vollständige Neuberechnen durch DB-Administratoren noch das Abschneiden des Kettenendes. Dafür fehlen ein unabhängiger Prüfanker sowie ein betriebliches Schutz-/Exportkonzept. Die vorhandene Implementierung ist nicht „fälschungssicher“ oder revisionssicher zertifiziert.

Die Middleware protokolliert Zugriffsversuche. Weil CRUD noch fehlt, protokolliert sie keine bestätigten fachlichen Änderungen mit vollständigen Vorher-/Nachherwerten. Für künftige Workflows müssen Änderung und Audit zusammen betrachtet werden.

## Verschlüsselung und Schlüssel

Das Krypto-Modul stellt AES-256-GCM-Funktionen mit einem pro Datensatz abgeleiteten Schlüssel bereit. Seine Initialisierung verschlüsselt weder automatisch sämtliche Spalten noch die SQLite-Datei. Klarnamen, Metadaten und nicht explizit verschlüsselte Werte können im Klartext vorliegen. SQLCipher ist nicht eingebunden.

Für eine neue Entwicklungsinstanz:

```bash
export KIS_JWT_SECRET="$(openssl rand -base64 48)"
export KIS_MASTER_KEY="$(openssl rand -base64 32)"
```

Bestehende Schlüssel in einer geschützten Ablage erhalten; Wiederherstellung und Rotation einschließlich alter Datensätze sind noch als vollständiger Ablauf umzusetzen. Secrets nicht in TOML, Logs, Git, Kommandozeilenargumente oder allgemeine Backups aufnehmen. Lokale Secret-Dateien haben eingeschränkte Rechte. In-Memory-Schlüssel werden noch nicht durchgängig zeroisiert.

Die Backup-Verschlüsselung verwendet age. Die dafür erforderliche private Identity wird getrennt vom Archiv aufbewahrt; siehe [OPERATION.md](OPERATION.md).

## Netzwerk und Laufzeit

Der Server bietet HTTP, standardmäßig auf 127.0.0.1. Für Netzbetrieb sind ein TLS-Terminator, beschränkte Erreichbarkeit und abgestimmte Proxy-Limits erforderlich. Es existiert keine native TLS-Zertifikatskonfiguration im Server. Die minimale TLS-Version ist mit den Anforderungen und Gegenstellen festzulegen.

HL7-, DICOM- und XDS-Listener werden noch nicht gestartet. Daher lassen sich aus Porttabellen oder enabled-Feldern keine vorhandenen Sicherheitsfunktionen dieser Schnittstellen ableiten.

CPU-Aufwand für Passwortprüfung und Lastverhalten sind weiter zu bewerten. Der In-Memory-Limiter ist keine verteilte DoS-Abwehr. Vor Einsatz mit echten Daten sind insbesondere Penetrationstest, Abhängigkeitsprüfung, Request-Limits und ein unabhängiges Security-Review erforderlich.

## Datenschutz und Aufbewahrung

Gesundheitsdaten erfordern eine dokumentierte Rechtsgrundlage und geeignete technische und organisatorische Maßnahmen. Anwendbare Fristen hängen von Datenkategorie, Behandlungs-/Dokumentationspflichten und Rechtsordnung ab. Eine pauschale sechs- oder zehnjährige DSGVO-Pflicht für sämtliche Audit-Daten wird hier nicht behauptet.

Die Konfigurationsvorgabe `retention_days=3650` ist kein Rechtsnachweis. Archivierung, Löschfristen, Betroffenenrechte und Rückwirkungen auf Backups sind noch auszuarbeiten. Pseudonymisierte Daten bleiben grundsätzlich personenbeziehbar; Pseudonymisierung ist nicht mit irreversibler Anonymisierung gleichzusetzen.

Normzuordnungen müssen die genaue Ausgabe und den belegten Kontrollumfang nennen. Eine medizinprodukterechtliche Einstufung setzt die konkrete Zweckbestimmung und fachliche Prüfung voraus. Sicherheitsdokumentation und Unit-Tests ersetzen diese Prüfung nicht.

## Vorfälle und offene Freigaben

Für echte Betriebsumgebungen sind Verantwortliche, Eskalation, Beweissicherung und Meldewege festzulegen. Datenschutzmeldungen sind abhängig von den tatsächlichen gesetzlichen Voraussetzungen und Fristen zu bewerten; eine Benachrichtigung Betroffener darf nicht pauschal erst nach 72 Stunden eingeplant werden.

Der [Fahrplan](FAHRPLAN.md) fordert für Sicherheitsänderungen ein unabhängiges zweites Review. Automatische Regressionstests sind vorhanden; ein solches unabhängiges Review und eine klinische Freigabe werden dadurch nicht ersetzt.
