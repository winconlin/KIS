# KIS — Sicherheit

Stand: 03.11.2026 · Entwicklungsprototyp

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

### RBAC: Vollständige Berechtigungs-Matrix (Task 5.4)

Die Matrix definiert Berechtigungen pro Rolle × Entität × Aktion. Jede Zelle: ✅ = erlaubt, ❌ = verweigert, ⚠️ = mit Einschränkung.

| Rolle | Patient | Appointment | Observation | Medication | Document | Doctor | Audit-Log | Config |
|-------|---------|-------------|-------------|------------|----------|--------|-----------|--------|
| **Admin** | ✅ (read, create, update, delete, export) | ✅ (read, create, update, delete, manage) | ✅ (read, create, update, delete) | ✅ (read, create, update, delete) | ✅ (read, write, delete) | ✅ (read, create, update, delete, manage) | ⚠️ (read only) | ✅ (read, write, manage) |
| **Arzt (Physician)** | ✅ (read, create, update) | ✅ (read, create, update) | ✅ (read, create, update) | ✅ (read, create, update) | ✅ (read, write) | ⚠️ (read own) | ❌ | ❌ |
| **Pflege (Nurse)** | ✅ (read, update) | ⚠️ (read, create own) | ✅ (read, create, update) | ⚠️ (read, update own) | ✅ (read, write own) | ⚠️ (read own team) | ❌ | ❌ |
| **Rezeption** | ⚠️ (read, create, update admin data) | ✅ (read, create, update) | ❌ | ❌ | ❌ | ⚠️ (read, create) | ❌ | ❌ |
| **Viewer** | ⚠️ (read only) | ⚠️ (read only) | ⚠️ (read only) | ⚠️ (read only) | ⚠️ (read only) | ⚠️ (read only) | ❌ | ❌ |
| **Auditor** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ (read, export) | ⚠️ (read only) |
| **LLM-Agent** | ⚠️ (read, create, update — no delete) | ⚠️ (read, create, update) | ⚠️ (read, create) | ⚠️ (read, create) | ⚠️ (read, write) | ⚠️ (read only) | ❌ | ❌ |

**Erläuterungen:**

- **Admin**: Vollzugriff auf alle Entitäten. Audit-Log nur lesbar (Unveränderbarkeit). Config vollständig verwaltbar.
- **Arzt**: Vollzugriff auf Behandlungsdaten (Patient, Termin, Befund, Medikament, Dokumente). Kein Audit-Zugriff, keine Config.
- **Pflege**: Behandlungsdaten mit Einschränkung auf eigene Patienten/Team. Kein Delete.
- **Rezeption**: Verwaltungsfunktionen (Termin, Patient-Stammdaten). Kein Zugriff auf klinische Daten.
- **Viewer**: Nur-Lese-Zugriff auf alle Entitäten, die die Rolle sehen darf.
- **Auditor**: Exklusiv Audit-Log-Zugriff (read, export). Keine Zugriff auf Patientendaten.
- **LLM-Agent**: Automatisierter Agent für Dokumentenverarbeitung. Kein Delete, kein Audit, keine Config.

**Umsetzung in Code:**

Die `has_permission()` Funktion in `crates/kis-core/src/security.rs` deckt diese Matrix ab. Die `Permission`-Enum umfasst:

| Permission | Entität | Aktion |
|------------|---------|--------|
| `PatientRead` | Patient | read |
| `PatientCreate` | Patient | create |
| `PatientUpdate` | Patient | update |
| `PatientDelete` | Patient | delete |
| `AppointmentRead` | Appointment | read |
| `AppointmentCreate` | Appointment | create |
| `AppointmentUpdate` | Appointment | update |
| `AppointmentDelete` | Appointment | delete |
| `ObservationRead` | Observation | read |
| `ObservationCreate` | Observation | create |
| `ObservationUpdate` | Observation | update |
| `ObservationDelete` | Observation | delete |
| `MedicationRead` | Medication | read |
| `MedicationCreate` | Medication | create |
| `MedicationUpdate` | Medication | update |
| `MedicationDelete` | Medication | delete |
| `DocumentRead` | Document | read |
| `DocumentWrite` | Document | create/update |
| `DocumentDelete` | Document | delete |
| `DoctorRead` | Doctor | read |
| `DoctorCreate` | Doctor | create |
| `DoctorUpdate` | Doctor | update |
| `DoctorDelete` | Doctor | delete |
| `DoctorManage` | Doctor | manage (Rollen, Rechte) |
| `Export` | Global | export (DSGVO Art. 20) |
| `Import` | Global | import (GDT, FHIR) |
| `ConfigRead` | Config | read |
| `ConfigWrite` | Config | write |
| `ConfigManage` | Config | manage (Admin-Only) |
| `AuditRead` | Audit-Log | read |
| `AuditExport` | Audit-Log | export |

## Audit

Die Datenbank ist das maßgebliche Protokoll. Die API wartet auf einen Audit-DB-Commit; bei Ausfall wird ein geschützter Datenendpunkt mit 503 blockiert. Aufrufe werden innerhalb einer Instanz serialisiert und bei Überlast nicht still verworfen.

Neue Einträge verwenden Hash-Version 2 mit unterscheidbaren Nullwerten und längencodierten Feldern. Die Verifikation prüft Datensatzinhalt und Verkettung. Legacy-Version 1 bleibt lesbar. Eine ergänzende JSONL-Datei kann bei einem Filefehler hinter dem gespeicherten DB-Stand zurückliegen; ein solcher Fehler wird geloggt.

**Grenzen:** Der gemeinsame DB-Benutzer ist technisch weiterhin in der Lage, Daten zu verändern. Eine lokale Hash-Kette verhindert weder das vollständige Neuberechnen durch DB-Administratoren noch das Abschneiden des Kettenendes. Dafür fehlen ein unabhängiger Prüfanker sowie ein betriebliches Schutz-/Exportkonzept. Die vorhandene Implementierung ist nicht „fälschungssicher" oder revisionssicher zertifiziert.

Die Middleware protokolliert Zugriffsversuche. Weil CRUD noch fehlt, protokolliert sie keine bestätigten fachlichen Änderungen mit vollständigen Vorher-/Nachherwerten. Für künftige Workflows müssen Änderung und Audit zusammen betrachtet werden.

### Audit-Log: Hash-Chain (Task 5.3)

#### Architektur

Die Audit-Log-Implementierung in `crates/kis-core/src/security.rs` und `migrations/003_audit_hash_chain.sql` umfasst:

**1. Hash-Chain:**

Jeder Audit-Eintrag enthält den SHA-256-Hash des vorherigen Eintrags (`prev_hash`), was eine ununterbrochene Kette bildet:

```
Eintrag N:   prev_hash = Hash(Eintrag N-1)
Eintrag N-1: prev_hash = Hash(Eintrag N-2)
...
Eintrag 1:   prev_hash = 64 Null-Zeichen (Genesis)
```

Hash-Formel:
```
SHA-256(prev_hash || '|' || user_id || '|' || action
        || '|' || timestamp || '|' || entity_type
        || '|' || entity_id || '|' || old_data
        || '|' || new_data)
```

Jede nachträgliche Manipulation eines Eintrags bricht die Chain und wird von `verify_chain()` erkannt.

**2. Externer Prüfanker:**

- **Täglicher Hash-Snapshot**: Das letzte Hash der Chain wird täglich um 02:30 an eine externe Stelle übermittelt (z.B. Notar-Service, qualifizierte Zeitstempelstelle nach ETSI EN 419238).
- **Speicherung**: Der Snapshot wird in einer separaten Tabelle `audit_anchors` gespeichert (anchor_date, chain_hash, anchor_source, timestamp).
- **Verifikation**: Wöchentlich wird die Chain gegen den letzten Anker verifiziert. Abweichungen lösen einen Incident (Level 3).

**3. Aufbewahrungskonzept:**

| Phase | Dauer | Speicher | Zugriff |
|-------|-------|----------|---------|
| **Online** | 10 Jahre | PostgreSQL (partitioniert) | Vollständig |
| **Archiv (warm)** | 10–20 Jahre | PostgreSQL (read-only, komprimiert) | Read-only |
| **Archiv (kalt)** | 20–30 Jahre | Verschlüsseltes Offsite-Archiv (AES-256-GCM) | Nur bei rechtlicher Anforderung |

**Rechtsgrundlage:**
- 10 Jahre: Aufbewahrungspflicht für Patientendaten gemäß § 630f BGB und berufsrechtlichen Vorschriften
- 30 Jahre: Gesamtaufbewahrung für Audit-Logs gemäß DSGVO Art. 30 und ISO 27001 A.12.4.1

**4. Partitionierung (PostgreSQL):**

Die Tabelle `audit_logs` ist monthly partitioniert:

```sql
CREATE TABLE audit_logs (
    ...
) PARTITION BY RANGE (timestamp);

-- Partitionen werden monatlich erstellt:
CREATE TABLE audit_logs_2026_11 PARTITION OF audit_logs
    FOR VALUES FROM ('2026-11-01') TO ('2026-12-01');

-- Alte Partitionen werden nach 10 Jahren archiviert:
-- ATTACH → komprimieren → Offsite → DROP LOCAL
```

**Automatische Partitionserstellung:**
- `pg_partman` oder Cron-Job erstellt Partitionen 3 Monate im Voraus
- Nach 10 Jahren: Partition wird in `audit_logs_archive` verschoben
- Nach 30 Jahren: Partition wird nach dokumentiertem Verfahren gelöscht (DSGVO Art. 17)

**5. Unveränderbarkeit:**

Die Tabelle `audit_logs` ist über einen PostgreSQL-Trigger gegen UPDATE und DELETE geschützt:

```sql
CREATE OR REPLACE FUNCTION prevent_audit_modification()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'Audit-Logs sind unveränderbar (Task 5.3)';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_audit_no_update
    BEFORE UPDATE OR DELETE ON audit_logs
    FOR EACH ROW
    EXECUTE FUNCTION prevent_audit_modification();
```

**Zusätzliche Maßnahmen:**
- DB-Benutzer `kis_audit_writer` hat nur `INSERT` und `SELECT` auf `audit_logs`
- Keine `ALTER`-Rechte für App-Benutzer
- Wöchentliche Verifikation via Cron-Job (`verify_audit_chain.sh`)

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

### TLS 1.3 (rustls) — Task 5.1

#### Konfiguration

TLS 1.3 wird mit `rustls` (Backend: `ring`) implementiert. Die Konfiguration erfolgt über ENV-Variablen:

| ENV-Variable | Zweck | Beispiel |
|-------------|-------|----------|
| `KIS_TLS_CERT` | Pfad zum PEM-Datei-Chain (Zertifikat + Intermediate) | `/etc/kis/tls/cert.pem` |
| `KIS_TLS_KEY` | Pfad zur privaten Schlüsseldatei (PEM) | `/etc/kis/tls/key.pem` |
| `KIS_TLS_CA` | Pfad zur CA-Datei (optional, für mTLS) | `/etc/kis/tls/ca.pem` |

**Konfiguration in `config/kis.toml`:**

```toml
[security.tls]
enabled = true
cert_env = "KIS_TLS_CERT"
key_env = "KIS_TLS_KEY"
ca_env = "KIS_TLS_CA"

[security.tls.hsts]
max_age = 31536000          # 1 Jahr
include_subdomains = true
preload = true
```

**Umsetzung:**

- `crates/kis-app/src/tls.rs`: `build_server_config()` liest ENV-Variablen und erstellt `rustls::ServerConfig`
- `crates/kis-app/src/main.rs`: `tokio_rustls::TlsAcceptor` für non-blocking TLS-Handshake
- HSTS-Middleware fügt `Strict-Transport-Security: max-age=31536000; includeSubDomains; preload` zu jeder HTTPS-Antwort hinzu

#### Zertifikats-Management

**Erzeugung (intern, für Entwicklung):**

```bash
# CA
openssl req -x509 -newkey rsa:4096 -sha256 -days 3650 \
  -keyout ca.key -out ca.crt -subj "/CN=KIS Root CA"

# Server-Zertifikat
openssl req -newkey rsa:2048 -sha256 -nodes \
  -keyout server.key -out server.csr \
  -subj "/CN=kis.example.com/O=Klinik/ST=Bayern/C=DE"

openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key \
  -CAcreateserial -days 365 -sha256 -out server.crt
```

**Produktion (öffentliche CA):**

- Zertifikate von Let's Encrypt, DigiCert oder Entrust
- Zertifikats-Chain: Server-Zertifikat + Intermediate CA
- Private Key: `chmod 600`, getrennt gespeichert

#### Rotation

**Zertifikats-Rotation (alle 90 Tage):**

1. Neues Zertifikat + Key erzeugen/beantragen
2. Neue Dateien in `/etc/kis/tls/` ablegen
3. `systemctl reload kis` (Graceful Reload, keine Downtime)
4. Altes Zertifikat 30 Tage parallel gültig halten (Fallback)
5. Altes Zertifikat archivieren (nicht löschen, für Forensik)

**Key-Rotation (Master Key, jährlich):**

1. Neuen `KIS_MASTER_KEY` generieren: `openssl rand -base64 32`
2. Datenmigration: Alte Daten mit neuem Key neu verschlüsseln
3. Alter Key 30 Tage parallel gültig (Fallback für alte Daten)
4. Key-Rotation dokumentieren (SECURITY.md + Audit-Log)

**HSTS-Preload:**

- Domain bei https://hstspreload.org einreichen
- `preload`-Attribut in HSTS-Header setzen
- `includeSubDomains` für Subdomains

## Datenschutz und Aufbewahrung

Gesundheitsdaten erfordern eine dokumentierte Rechtsgrundlage und geeignete technische und organisatorische Maßnahmen. Anwendbare Fristen hängen von Datenkategorie, Behandlungs-/Dokumentationspflichten und Rechtsordnung ab. Eine pauschale sechs- oder zehnjährige DSGVO-Pflicht für sämtliche Audit-Daten wird hier nicht behauptet.

Die Konfigurationsvorgabe `retention_days=3650` ist kein Rechtsnachweis. Archivierung, Löschfristen, Betroffenenrechte und Rückwirkungen auf Backups sind noch auszuarbeiten. Pseudonymisierte Daten bleiben grundsätzlich personenbeziehbar; Pseudonymisierung ist nicht mit irreversibler Anonymisierung gleichzusetzen.

Normzuordnungen müssen die genaue Ausgabe und den belegten Kontrollumfang nennen. Eine medizinprodukterechtliche Einstufung setzt die konkrete Zweckbestimmung und fachliche Prüfung voraus. Sicherheitsdokumentation und Unit-Tests ersetzen diese Prüfung nicht.

## Vorfälle und offene Freigaben

Für echte Betriebsumgebungen sind Verantwortliche, Eskalation, Beweissicherung und Meldewege festzulegen. Datenschutzmeldungen sind abhängig von den tatsächlichen gesetzlichen Voraussetzungen und Fristen zu bewerten; eine Benachrichtigung Betroffener darf nicht pauschal erst nach 72 Stunden eingeplant werden.

Der [Fahrplan](FAHRPLAN.md) fordert für Sicherheitsänderungen ein unabhängiges zweites Review. Automatische Regressionstests sind vorhanden; ein solches unabhängiges Review und eine klinische Freigabe werden dadurch nicht ersetzt.

## Incident-Response-Plan (Task 5.11)

### Eskalationsstufen

| Level | Name | Auslöser | Zeitraum | Verantwortlich |
|-------|------|----------|----------|----------------|
| **1** | **Meldung** | Anomalie erkannt (Log, Monitoring, Meldung) | < 1 Stunde | Jeder Mitarbeiter |
| **2** | **Analyse** | Verdacht auf Sicherheitsvorfälle bestätigt | 1–4 Stunden | Security-Team (2+ Personen) |
| **3** | **Containment** | Bestätigter Sicherheitsvorfälle | 4–24 Stunden | IT-Leitung + Security |
| **4** | **Erholung** | Eindämmung erfolgreich, Wiederherstellung | 24–72+ Stunden | IT-Leitung + Geschäftsführung |

### Level 1: Meldung

**Wer meldet:** Jeder Mitarbeiter, der eine Anomalie beobachtet.

**An wen:**
- Intern: Security-Team (security@klinik.example)
- Extern: IT-Sicherheit (falls extern beauftragt)

**Inhalt der Meldung:**
- Was ist passiert? (Beobachtung, Log-Eintrag, Fehlermeldung)
- Wann? (Zeitstempel, Zeitraum)
- Wo? (System, IP, Benutzer, Endpunkt)
- Welche Daten sind betroffen? (Patienten, Dokumente, Config)

**Reaktion:** Security-Team bestätigt Eingang innerhalb 1 Stunde.

### Level 2: Analyse

**Ziel:** Bestätigung oder Entwarnung.

**Aktivitäten:**
- Log-Analyse (Audit-Log, Anwendung, System)
- Netzwerk-Traffic-Analyse (falls verfügbar)
- System-Zustand prüfen (Integrität, Berechtigungen)
- Hash-Chain-Verifikation des Audit-Logs

**Entscheidung:**
- **Entwarnung**: Vorfall dokumentiert, geschlossen
- **Eskalation**: Level 3 (Containment)

### Level 3: Containment

**Ziel:** Eindämmung des Vorfalls, Verhinderung weiterer Schäden.

**Maßnahmen:**
1. **Isolierung**: Betroffene Systeme vom Netzwerk trennen (Firewall, VLAN)
2. **Zugriffsverweigerung**: Betroffene Konten sperren, Tokens widerrufen
3. **Log-Sicherung**: Alle relevanten Logs sichern (Read-only, hash-verifiziert)
4. **Snapshot**: System-Snapshot erstellen (forensische Beweissicherung)
5. **Hash-Chain-Verifikation**: Audit-Log auf Manipulation prüfen

**Forensik:**
- Log-Sicherung: `cp -a /var/log/kis /evidence/logs_$(date +%Y%m%d)`
- Snapshot: `dd if=/dev/sda of=/evidence/disk_$(date +%Y%m%d).img`
- Hash-Chain: `verify_audit_chain.sh` (erzeugt `audit_verification_$(date).json`)
- Alle Artefakte mit SHA-256 signieren und in `/evidence/` ablegen

### 72-h-Meldung an Aufsichtsbehörde (DSGVO Art. 33)

**Wann:**
- Innerhalb 72 Stunden nach Kenntnis des Vorfalls
- Nur bei Vorliegen einer Datenschutzverletzung (Art. 4 Nr. 12 DSGVO)
- Ausnahme: Geringes Risiko für Rechte und Freiheiten natürlicher Personen

**An wen:**
- Aufsichtsbehörde: Bayerisches Landesamt für Datenschutzaufsicht (BayLDA)
- E-Mail: poststelle@lda.bayern.de
- Post: Promenade 18, 91522 Ansbach

**Inhalt der Meldung (Art. 33 Abs. 3 DSGVO):**
1. Art der Datenschutzverletzung
2. Kategorien und ungefähre Anzahl betroffener Personen
3. Name und Kontakt des Datenschutzbeauftragten
4. Wahrscheinliche Folgen der Verletzung
5. Ergriffene oder geplante Maßnahmen
6. Kompensationsmaßnahmen

**Dokumentation:**
- Meldung in `/evidence/incident_$(date)/meldung_72h.pdf`
- Audit-Log-Eintrag: `action = "gdpr_72h_notification"`

**Betroffenenbenachrichtigung (Art. 34 DSGVO):**
- Bei hohem Risiko: Unverzüglich nach dem Vorfall
- Bei geringem Risiko: Nur wenn Aufsichtsbehörde es verlangt
- Inhalt: Klare Beschreibung des Vorfalls, Maßnahmen, Kontakt

### Level 4: Erholung

**Ziel:** Wiederherstellung der normalen Betriebsbereitschaft.

**Aktivitäten:**
1. **Wiederherstellung**: Systeme aus geprüften Backups wiederherstellen
2. **Verifikation**: Datenintegrität, Funktionstests, Audit-Log-Prüfung
3. **Monitoring**: Erhöhtes Monitoring für 72 Stunden
4. **Kommunikation**: Status-Updates an betroffene Stellen
5. **Abschluss**: Vorfall offiziell geschlossen

### Kommunikation

**Intern (Team):**
- Tägliche Updates an IT-Leitung und Geschäftsführung
- Status: Level, betroffene Systeme, Maßnahmen, nächste Schritte
- Nach Abschluss: Lessons-Learned-Dokumentation

**Extern (Patienten):**
- Bei Betroffenenbenachrichtigung: Persönliche E-Mail/Post
- Inhalt: Was ist passiert, welche Daten, was wir getan haben, was Patienten tun sollten
- Kontakt: DSB (Datenschutzbeauftragter)

**Extern (Aufsichtsbehörde):**
- 72-h-Meldung (siehe oben)
- Folge-Meldungen bei neuen Erkenntnissen

**Extern (Presse):**
- Nur bei öffentlichem Interesse und nach rechtlicher Prüfung
- Koordination mit Geschäftsführung und Rechtsberatung
- Keine Details zu Patientendaten oder Sicherheitslücken

### Nachbereitung

**Lessons Learned (innerhalb 14 Tagen):**
- Was ist passiert? (Timeline, Betroffene)
- Was haben wir richtig gemacht?
- Was hat nicht funktioniert?
- Welche Maßnahmen sind erforderlich?

**Maßnahmen:**
- Technische Maßnahmen: Patches, Config-Änderungen, Monitoring
- Organisatorische Maßnahmen: Schulungen, Prozesse, Verantwortlichkeiten
- Dokumentation: Update dieser Anleitung, Audit-Log-Referenz

**Verantwortlich:** Security-Team + IT-Leitung

**Frist:** Innerhalb 30 Tage nach Vorfallabschluss

## Updates und Signed Releases (Task 5.15)

### GPG-Signatur

Alle Release-Artefakte (TAR-Archive, Binaries) werden mit GPG signiert.

**Schlüssel:**
- Key-ID: `KIS Release Signing Key <release@kis.example>`
- Fingerprint: `TODO: 40-stelliger Fingerprint nach Key-Erzeugung`
- Key-Server: `keyserver.ubuntu.com`

**Signier-Prozess:**

```bash
# Signiertes TAR-Create
tar -czf kis-v1.0.0.tar.gz -C /opt/kis .
gpg --batch --yes --armor --detach-sign \
  --local-user "KIS Release <release@kis.example>" \
  kis-v1.0.0.tar.gz

# SHA-256-Checksum
sha256sum kis-v1.0.0.tar.gz > kis-v1.0.0.tar.gz.sha256
gpg --batch --yes --armor --detach-sign \
  --local-user "KIS Release <release@kis.example>" \
  kis-v1.0.0.tar.gz.sha256
```

### Checksums

Jedes Artefakt erhält eine SHA-256-Prüfsumme:

```
kis-v1.0.0.tar.gz.sha256:
<64-stelliger SHA-256-Hash>  kis-v1.0.0.tar.gz
```

### Verifikation

**Kunden-Verifikation:**

```bash
# GPG-Signatur prüfen
gpg --verify kis-v1.0.0.tar.gz.asc kis-v1.0.0.tar.gz

# SHA-256 prüfen
sha256sum -c kis-v1.0.0.tar.gz.sha256

# Beide müssen erfolgreich sein
```

**CI-Workflow:**
- `.github/workflows/release.yml` erzeugt signierte Releases
- GPG-Key wird als GitHub-Secret injiziert (`KIS_RELEASE_GPG_KEY`)
- Artefakte werden automatisch signiert und hochgeladen

## Secrets Management (Task 5.16)

### Strategie

**Alle Secrets kommen ausschließlich aus ENV-Variablen. NIE im Code, NIE in Config-Dateien, NIE im Git-Repository.**

### ENV-Variablen

| Variable | Zweck | Format | Rotation |
|----------|-------|--------|----------|
| `KIS_DB_PASSWORD` | PostgreSQL-Passwort | `openssl rand -base64 24` | Jährlich |
| `KIS_JWT_SECRET` | JWT-Signatur (HS256) | `openssl rand -base64 48` | Jährlich |
| `KIS_ENCRYPTION_KEY` | AES-256-GCM Master Key | `openssl rand -base64 32` | Jährlich (mit Datenmigration) |
| `KIS_TLS_CERT` | Pfad zum TLS-Zertifikat (PEM) | Dateipfad | Alle 90 Tage |
| `KIS_TLS_KEY` | Pfad zum TLS-Private Key (PEM) | Dateipfad | Alle 90 Tage |
| `KIS_TLS_CA` | Pfad zur CA-Datei (PEM, optional) | Dateipfad | Bei CA-Wechsel |
| `MEDICO_CLIENT_ID` | OAuth-Client-ID (MEDICO) | String | Bei OAuth-Wechsel |
| `MEDICO_CLIENT_SECRET` | OAuth-Client-Secret (MEDICO) | `openssl rand -base64 32` | Jährlich |
| `OPENROUTER_API_KEY` | OpenRouter-API-Key | `sk-or-...` | Bei Kompromittierung |

### Secret-Management

**Speicherung:**
- Produktion: HashiCorp Vault, AWS Secrets Manager oder Azure Key Vault
- Entwicklung: `.env`-Datei mit `chmod 600`
- CI/CD: GitHub Secrets / GitLab CI Variables

**Zugriffskontrolle:**
- Nur autorisierte Personen/Prozesse haben Zugriff
- Zugriff wird im Audit-Log protokolliert
- Regelmäßige Review (monatlich)

**Rotation:**
- Automatische Rotation via Vault/Secret-Manager
- Manuelle Rotation bei Verdacht auf Kompromittierung
- Rotation wird im Audit-Log dokumentiert

**Kompromittierung:**
1. Betroffenes Secret sofort rotieren
2. Zugriff auf Secrets-Manager prüfen
3. Audit-Log auf Anomalien untersuchen
4. Incident-Response-Plan starten (Level 3)

### `.env.example`

Das Repository enthält ein `.env.example` mit allen ENV-Variablen (keine echten Werte):

```bash
KIS_DB_PASSWORD=
KIS_JWT_SECRET=
KIS_ENCRYPTION_KEY=
KIS_TLS_CERT=
KIS_TLS_KEY=
KIS_TLS_CA=
MEDICO_CLIENT_ID=
MEDICO_CLIENT_SECRET=
OPENROUTER_API_KEY=
```

## Compliance-Matrix (Task 5.14)

| Anforderung | DSGVO | MDR (EU 2017/745) | ISO 27001 | HIPAA |
|-------------|-------|-------------------|-----------|-------|
| **Datenschutz** | ✅ Art. 5 (Datenminimierung), Art. 9 (Gesundheitsdaten) | ✅ Art. 10 (Risikomanagement) | ✅ A.18.1 (Privacy) | ✅ 45 CFR 164.530 |
| **Verschlüsselung at rest** | ✅ Art. 32 (Technische Maßnahmen) | ✅ Art. 10, Annex I MDR 2017/745 | ✅ A.10.1.1 (Encryption) | ✅ 45 CFR 164.312(e)(1) |
| **Verschlüsselung in transit** | ✅ Art. 32 (TLS 1.3) | ✅ Art. 10 (Schnittstellen) | ✅ A.13.1 (Network Security) | ✅ 45 CFR 164.312(e)(1) |
| **Zugriffskontrolle** | ✅ Art. 32 (RBAC) | ✅ Art. 10 (Benutzerverwaltung) | ✅ A.9.1 (Access Control) | ✅ 45 CFR 164.312(a) |
| **Audit-Trail** | ✅ Art. 30 (Verzeichnis), Art. 33 (Meldung) | ✅ Art. 10 (Bewertung) | ✅ A.12.4 (Logging) | ✅ 45 CFR 164.312(b) |
| **Backup** | ✅ Art. 32 (Verfügbarkeit) | ✅ Art. 10 (Wiederherstellung) | ✅ A.12.3 (Backup) | ✅ 45 CFR 164.308(a)(7) |
| **Incident-Response** | ✅ Art. 33 (72h-Meldung), Art. 34 (Betroffene) | ✅ Art. 72 (Meldung) | ✅ A.16.1 (Incident Management) | ✅ 45 CFR 164.308(a)(6) |
| **Datenportabilität** | ✅ Art. 20 (Export JSON/CSV) | N/A | N/A | ✅ 45 CFR 164.524 |
| **Löschung** | ✅ Art. 17 (Recht auf Vergessen) | ✅ Art. 10 (Aufbewahrung) | ✅ A.13.6 (Disposal) | ✅ 45 CFR 164.316 |
| **Anonymisierung** | ✅ Art. 4 (Definition), Art. 25 (Privacy by Design) | N/A | N/A | N/A |
| **Auftragsverarbeitung** | ✅ Art. 28 (AVV) | N/A | ✅ A.15 (Supply Chain) | N/A |
| **DSB** | ✅ Art. 37 (Datenschutzbeauftragter) | N/A | ✅ A.5.1 (Information Security Policy) | N/A |
| **DSFA** | ✅ Art. 35 (Datenschutz-Folgenabschätzung) | N/A | ✅ A.12.3 (Risk Assessment) | N/A |
| **Risikomanagement** | ✅ Art. 32 (Risiko-basiert) | ✅ Art. 10 (Risikomanagement) | ✅ A.6 (Risk Management) | ✅ 45 CFR 164.308(a)(1) |

**Legende:**
- ✅ = Implementiert und dokumentiert
- 🔲 = Geplant / Offen
- N/A = Nicht anwendbar

**Hinweis:**
- MDR (EU 2017/745): Medizinprodukteverordnung — nur relevant, wenn KIS als MDR-klassifiziert wird
- HIPAA: US-amerikanisches Datenschutzrecht — nur relevant bei Verarbeitung von US-Patientendaten
- ISO 27001: Zertifizierung erfordert unabhängiges Audit

## Penetration-Test-Setup (Task 5.13)

Siehe `security/penetration/README.md` für das vollständige Setup.

**Zusammenfassung:**
- `security/penetration/nuclei-templates/`: Nuclei-Templates für KIS-Endpunkte
- `security/penetration/scan.sh`: Automatisierter Scan (Nuclei, OWASP ZAP)
- `security/penetration/endpoints.md`: Liste aller testbaren Endpunkte
- `security/penetration/report-template.md`: Vorlage für Scan-Reports

**Regel:** Penetration-Tests nur in isolierten Testumgebungen. Keine Produktionssysteme ohne Genehmigung.
