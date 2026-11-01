# KIS — Sicherheit

> **Krankenhaus-Informations-System (KIS)**
> Umfassende Sicherheitsdokumentation: Datenschutz, Zugriffskontrolle, Audit-Logging, Netzwerk, Eingabevalidierung, Integrität, Compliance, Secret-Management, Update-Strategie, Incident Response und Threat Model.
>
> **Kritikalität: HOCH.** Das KIS verarbeitet Patientendaten im Sinne des Art. 4 Nr. 15 DSGVO (Gesundheitsdaten → besondere Kategorie nach Art. 9 DSGVO). Jede Sicherheitsentscheidung in diesem System hat direkte Auswirkungen auf die Betroffenenrechte und die haftungsfähigen Pflichten des Betreibers.

---

## 1. Datenschutz: DSGVO / GDPR Compliance

### 1.1 Rechtsrahmen

| Vorgabe | Relevanz für das KIS |
|---|---|
| **Art. 9 DSGVO** | Gesundheitsdaten sind besondere Kategorie → nur bei engen Ausnahmen verarbeitbar (Art. 9 Abs. 2 H: "Verrichtung einer Tätigkeit im öffentlichen Interesse im Bereich der öffentlichen Gesundheitsvorsorge" bzw. ärztliche Behandlung). |
| **Art. 25 DSGVO** (Datenschutz durch Design) | Technik muss Datenschutzgrundsätze by design umsetzen → Verschlüsselung, Minimierung, Löschung. |
| **Art. 30 DSGVO** | Verzeichnis von Verarbeitungstätigkeiten (VVT) → muss das KIS als Verarbeitungssystem aufführen. |
| **Art. 32 DSGVO** | Technische und organisatorische Maßnahmen (TOM) → Verschlüsselung, Integrität, Verfügbarkeit, Wiederherstellbarkeit. |
| **Art. 33 DSGVO** | Meldepflicht an Aufsichtsbehörde innerhalb von **72 Stunden** nach Kenntnis einer Datenschutzverletzung. |
| **Art. 34 DSGVO** | Mitteilung an betroffene Personen bei hohem Risiko. |
| **§ 203 StGB** (Deutschland) | Berufsgeheimnis von Ärzten / Pflegepersonal — Zugriff auf Patientendaten nur durch berechtigte Personen. |

### 1.2 Datenkategorien im KIS

| Tabelle | Enthaltene Daten | DSGVO-Kategorie |
|---|---|---|
| `patients` | Vorname, Nachname, Geburtsdatum, **Kassennummer** | **Besondere Kategorie** (Art. 9) — Gesundheitsdaten + Personendaten |
| `doctors` | Name, Fachgebiet, Approbationsnummer | Personendaten (Verantwortliche/Pfleger) |
| `appointments` | Patient ↔ Arzt, Zeitpunkt, Status, Notizen | **Besondere Kategorie** (indirekt, über patient_id) |
| `medications` | Patient, Medikament, Dosierung, Zeitraum, verschreibender Arzt | **Besondere Kategorie** (Therapiedaten) |

> **Wichtig:** Bereits das **Kopieren, Exportieren oder Anzeigen** von Zeilen aus `patients`, `appointments` oder `medications` ist eine Verarbeitung besonderer Kategorien und erfordert eine Rechtsgrundlage (i. d. R. Behandlungsvertrag / Einwilligung).

### 1.3 Grundsätze — Umsetzung im KIS

| Grundsatz (Art. 5 DSGVO) | Umsetzung im KIS |
|---|---|
| **Zweckbindung** | Daten werden nur für die klinische Versorgung verarbeitet (Termin, Medikation, Dokumentation). |
| **Datenminimierung** | Schema hält Felder minimal (siehe DATABASE.md). Keine überflüssigen Felder. |
| **Speicherbegrenzung** | Löschkonzept für entlassene Patienten / abgelaufene Termine (siehe § 1.4). |
| **Integrität & Vertraulichkeit** | Verschlüsselung at rest + in transit (§ 2, § 4). |
| **Rechenschaftspflicht** | Vollständiges, fälschungssicheres Audit-Log (§ 3). |

### 1.4 Löschkonzept und Aufbewahrung

| Datentyp | Aufbewahrungsfrist (empfohlen) | Löschmechanismus |
|---|---|---|
| Patientenstammdaten | 10 Jahre nach Behandlungsende (ärztliche Dokumentationspflicht, § 630f BGB) | Anonymisierung, nicht Löschung |
| Terminhistorie | 10 Jahre | Archivierung, danach Löschung |
| Medikationsplan | 10 Jahre | Archivierung, danach Löschung |
| Audit-Log | **Mindestens 6 Jahre** (BGB-Verjährung), bei MDR ggf. länger | Nur Anhängen, **nie** Löschen (siehe § 3.3) |

> **Hinweis:** Die konkreten Fristen hängen von der Klinik (Krankenhaus vs. ambulante Praxis) und der Rechtsordnung ab. Der Betreiber ist verpflichtet, die Fristen im Verarbeitungsverzeichnis zu dokumentieren.

---

## 2. Verschlüsselung

### 2.1 Datenverschlüsselung at rest (Ruhezustand)

Die Datenbankdatei `data/kis.db` (SQLite) enthält **unverschlüsselte** Patientendaten. Für den produktiven Betrieb **muss** eine der folgenden Verschlüsselungsebenen umgesetzt werden:

| Ebene | Methode | Aufwand | Empfehlung |
|---|---|---|---|
| **L1: Datei-Verschlüsselung** | `sqlcipher` (AES-256) als SQLite-Erweiterung | Mittel (Treiber-Wechsel von `rusqlite` auf `libsqlcipher-sys`) | **Empfohlen** — native, performant, durchsuchbar |
| **L2: Blockgeräte-Verschlüsselation** | LUKS / dm-crypt auf der Partition | Niedrig (Betriebssystem) | Ergänzend zu L1, nicht ersetzend |
| **L3: Container-Verschlüsselung** | Docker-Volumes mit LUKS | Niedrig | Für Container-Betrieb |

**Empfohlene Konfiguration (L1 — SQLCipher):**

```toml
# config/kis.toml (Soll — Erweiterung des [database]-Abschnitts)
[database]
path = "data/kis.db"
pool_size = 8
journal_mode = "wal"
# --- Verschlüsselung (Soll) ---
cipher = "sqlcipher"          # sqlcipher | plaintext
cipher_key_env = "KIS_DB_KEY" # Name der Env mit dem 256-bit-Schlüssel (hex, 64 Zeichen)
```

> **Schlüsselmanagement:** Der Datenbank-Schlüssel wird **nie** in der `kis.toml` oder im Code gespeichert. Er liegt in der Env-Variable `KIS_DB_KEY` (siehe § 8). Bei Verlust des Schlüssels sind die Daten **nicht wiederherstellbar** — Schlüssel sicher in einem HSM / Secret-Manager ablegen.

**AES-256-Parameter (SQLCipher):**
- Verschlüsselungsalgorithmus: **AES-256 in CBC-Modus**
- HMAC: **HMAC-SHA512** für Integrität jedes 4-KB-Blocks
- Key-Stretching: **PBKDF2-HMAC-SHA512** mit 256.000 Iterationen
- Seitenverschlüsselung: jede 4-KB-Seite wird separat verschlüsselt

### 2.2 Verschlüsselung in transit (Datenübertragung)

| Kanal | Protokoll | Verschlüsselung |
|---|---|---|
| HTTP API / GUI / FHIR | **TLS 1.3** (min. TLS 1.2) | **Pflicht** für jede nicht-lokale Erreichbarkeit |
| HL7 v2.5 (MLLP) | TLS 1.3 (STARTTLS) oder TLS-Wrapper | **Pflicht** für Übertragung von Patientennachrichten |
| DICOM | TLS 1.3 (DICOM TLS Security Profile) | **Pflicht** für C-STORE-Übertragungen |
| XDS | TLS 1.3 (REST) bzw. WS-Security (SOAP) | **Pflicht** |

> **Wichtig:** Der KIS-Server bindet standardmäßig an Klartext-TCP. **TLS-Terminierung ist zwingend vorgeschaltet** (Nginx, Caddy, HAProxy). Konfiguration: [CONFIGURATION.md](CONFIGURATION.md) → TLS.

**Empfohlene TLS-Parameter:**
- Protokoll: TLS 1.3 (Rückwärtskompatibilität TLS 1.2)
- Cipher Suites: `TLS_AES_256_GCM_SHA384`, `TLS_CHACHA20_POLY1305_SHA256`
- Zertifikate: CA-signiert (interne CA für klinisches Netz)
- HSTS: `Strict-Transport-Security: max-age=31536000; includeSubDomains`
- Zertifikatspinnbacking (Certificate Pinning) für mobile Clients (optional)

---

## 3. Audit-Logging (Prüfprotokoll)

### 3.1 Grundsatz

**Jeder** Lese- und Schreibzugriff auf Patientendaten wird protokolliert. Das Prüfprotokoll ist ein zentrales Element der Rechenschaftspflicht (Art. 5 Abs. 2 DSGVO) und der forensischen Aufarbeitung.

### 3.2 Protokollierte Ereignisse

| Ereignis | Ausgelöst durch | Datenfeld |
|---|---|---|
| `patient.read` | `GET /api/patients/:id` | Benutzer, Patient-ID, Zeitpunkt, IP |
| `patient.create` | `POST /api/patients` | Benutzer, neue Patient-ID, Zeitpunkt |
| `patient.update` | `PUT /api/patients/:id` | Benutzer, Patient-ID, geänderte Felder |
| `patient.delete` | `DELETE /api/patients/:id` | Benutzer, Patient-ID, Zeitpunkt, Grund |
| `appointment.read` | `GET /api/appointments` | Benutzer, Zeitstempel |
| `appointment.create` | `POST /api/appointments` | Benutzer, Termin-ID, Patient-ID |
| `medication.create` | `POST /api/medications` | Benutzer, Medikament, Patient-ID, verschreibender Arzt |
| `medication.update` | `PUT /api/medications/:id` | Benutzer, Medikament-ID, geänderte Felder |
| `auth.login` | Login | Benutzer, IP, Erfolg/Feher |
| `auth.logout` | Logout | Benutzer, IP |
| `auth.failed` | Fehlgeschlagene Login | Benutzername, IP, Fehlergrund |
| `interface.hl7.receive` | HL7-Nachricht empfangen | Nachrichtentyp, Patient-PIID, Quelle |
| `interface.fhir.query` | FHIR-GET | Benutzer, Resource-Typ, Filter |

### 3.3 Fälschungsschutz: Hash-Kette

Jede Audit-Log-Eintrag enthält:
1. `timestamp` (UTC, ISO 8601, Millisekunden)
2. `actor_id` (Benutzer- oder System-ID)
3. `action` (z. B. `patient.read`)
4. `resource_type` + `resource_id`
5. `payload_hash` (SHA-256 des Dateninhalts, falls relevant)
6. `prev_hash` (SHA-256 des vorherigen Eintrags)
7. `entry_hash` (SHA-256 von `prev_hash` + allen obigen Feldern)

```
Eintrag N:
  prev_hash = entry_hash(N-1)
  entry_hash(N) = SHA256(prev_hash || timestamp || actor || action || resource || payload_hash)
```

> **Wirkung:** Eine nachträgliche Änderung eines Eintrags ändert dessen `entry_hash` und damit den `prev_hash` **aller** folgenden Einträge → Manipulation ist sofort erkennbar. Der erste Eintrag wird mit einem **Genesis-Hash** (z. B. aus dem Deployment-Seed) verankert.

**Tabelle `audit_log` (Soll):**

```sql
CREATE TABLE IF NOT EXISTS audit_log (
    seq          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp    TEXT NOT NULL,          -- ISO 8601 UTC
    actor_id     TEXT NOT NULL,
    action       TEXT NOT NULL,          -- patient.read, patient.create, ...
    resource_type TEXT,
    resource_id  TEXT,
    ip_address   TEXT,
    payload_hash TEXT,
    prev_hash    TEXT NOT NULL,
    entry_hash   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_resource ON audit_log(resource_type, resource_id);
```

### 3.4 Aufbewahrung und Integrität

| Eigenschaft | Anforderung |
|---|---|
| Aufbewahrung | **Mindestens 6 Jahre**, bei MDR-Komponente ggf. 10 Jahre |
| Schreibschutz | Audit-Tabelle ist **append-only** — UPDATE/DELETE sind auf DB-Ebene blockiert (Trigger) |
| Externe Verifikation | Wöchentlich: Export des letzten Eintrags, Hash-Vergleich mit externem Speicher (z. B. append-only-Log auf separates Gerät) |
| Zugriff | Nur `admin`-Rolle und Datenschutzbeauftragte(r) |
| Format | Strukturiert (JSON), maschinenlesbar, maschinenverarbeitbar |

> **Wichtig:** Das Audit-Log wird **nie** gelöscht oder archiviert, bevor die Aufbewahrungsfrist abgelaufen ist. Archivierung erfolgt in einer **getrennten, verschlüsselten** Kopie.

---

## 4. Netzwerk-Sicherheit

### 4.1 Offene Ports

| Port | Protokoll | Dienst | Richtung | Zugriff |
|---|---|---|---|---|
| 8080 | TCP/HTTP | GUI + REST API + FHIR R4 | Eingang | **Nur** über TLS-Proxy / klinisches Netz |
| 6667 | TCP/MLLP | HL7 v2.5 (MLLP) | Eingang | **Nur** vom LIS/HIS des Hauses |
| 104 | TCP | DICOM C-STORE SCP | Eingang | **Nur** von registrierten Modalities / PACS |

> **Grundsatz:** Nur die drei Ports 8080, 6667 und 104 sind geöffnet. Alle anderen Ports sind **geschlossen**.

### 4.2 Firewall-Regeln (empfohlen)

```bash
# /etc/ufw/rules.d/kis.rules (Beispiel für UFW)
# HTTP (nur über Reverse-Proxy)
ufw allow from 10.0.0.0/24 to any port 8080 proto tcp comment 'KIS HTTP (intern)'
# HL7 (nur vom LIS-Server)
ufw allow from 10.0.0.50 to any port 6667 proto tcp comment 'KIS HL7 MLLP'
# DICOM (nur von registrierten Modalities)
ufw allow from 10.0.1.0/24 to any port 104 proto tcp comment 'KIS DICOM SCP'
# Default: deny
ufw default deny incoming
ufw default allow outgoing
```

### 4.3 TLS-Terminierung

```
[Externe Welt / Klinisches Netz]
        │
        ▼
[Nginx / Caddy]  ← TLS 1.3 terminiert, Zertifikat, HSTS
        │  (127.0.0.1:8080, Klartext)
        ▼
[KIS Axum-Server]  ← Nur auf 127.0.0.1 binden, wenn hinter Proxy
```

> **Best-Practice:** Der KIS-Server sollte in der Produktion **nur auf 127.0.0.1** binden (`server.host = "127.0.0.1"`) und ausschließlich über den TLS-Terminator erreichbar sein.

### 4.4 Netzwerksegmentierung

| Segment | Zweck |
|---|---|
| **DMZ** (falls extern erreichbar) | TLS-Terminator, kein direkter DB-Zugriff |
| **Klinisches Netz (VLAN 10)** | Workstations, KIS-Server, LIS/HIS |
| **DICOM-Netz (VLAN 20)** | Modalities (CT, MRT, Ultraschall), PACS, KIS SCP |
| **Management** | Backup, Monitoring, Secret-Manager |

---

## 5. Eingabevalidierung

### 5.1 SQL-Injection (SQLX / Prepared Statements)

Das KIS nutzt **SQLX** mit **prepared statements**. Alle Datenbankabfragen verwenden parametrisierte Abfragen — **niemals** String-Konkatenation:

```rust
// ✅ RICHTIG (SQLX prepared statement)
let patient = sqlx::query_as::<_, Patient>(
    "SELECT id, first_name, last_name, birth_date, insurance_number, created_at, updated_at
     FROM patients WHERE id = ?"
)
.bind(patient_id)
.fetch_one(&pool)
.await?;

// ❌ NIEMALS (String-Konkatenation → SQL-Injection)
let sql = format!("SELECT * FROM patients WHERE id = '{}'", user_input);
```

> **Grundsatz:** Jeder Wert, der aus Benutzereingabe stammt, wird als **Parameter** an SQLX übergeben. Die Abfrage-Struktur ist statisch.

### 5.2 XSS (Cross-Site Scripting)

| Maßnahme | Umsetzung |
|---|---|
| Serverseitige Escaping | Alle dynamischen Inhalte in HTML-Antworten werden geescaped (Axum `Html` + manuelle Escaping bei Template-Ersetzung) |
| Content-Security-Policy | HTTP-Header `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'` |
| Keine Inline-Scripts | Kein `<script>` in dynamisch generiertem HTML |
| Eingabevalidierung | API-Validierung (Längen, erlaubte Zeichen) verhindert Injection in Daten, die später gerendert werden |

### 5.3 API-Eingabevalidierung

| Feld | Validierungsregel |
|---|---|
| `Patient.first_name` | 1–100 Zeichen, alphanumerisch + Leerzeichen + Bindestrich |
| `Patient.last_name` | 1–100 Zeichen, alphanumerisch + Leerzeichen + Bindestrich |
| `Patient.birth_date` | Gültiges Datum im Format `YYYY-MM-DD`, nicht in der Zukunft |
| `Patient.insurance_number` | Max. 50 Zeichen, alphanumerisch |
| `Appointment.start_time` | Gültiges ISO-8601-Datum, nicht in der Vergangenheit (bei Creation) |
| `Appointment.status` | Enum: `scheduled`, `confirmed`, `completed`, `cancelled`, `noshow` |
| `Medication.dosage` | 1–200 Zeichen |
| `Medication.start_date` | Gültiges Datum, ≤ `end_date` (falls gesetzt) |

> **Fehlerverhalten:** Ungültige Eingaben werden mit **HTTP 400 Bad Request** abgelehnt, **bevor** sie die Datenbank erreichen.

### 5.4 Request-Size-Limits

| Limit | Wert |
|---|---|
| Maximale Body-Größe (JSON) | 1 MB |
| Maximale URI-Länge | 8 KB |
| Maximale Header-Größe | 64 KB |
| Timeout (Request) | 30 s |

---

## 6. Datenintegrität

### 6.1 Prüfsummen und Integritätsprüfungen

| Prüfung | Intervall | Methode |
|---|---|---|
| SQLite-Integrität | Täglich | `PRAGMA integrity_check;` → `ok` |
| Audit-Log-Hash-Kette | Wöchentlich | Verifikation der gesamten Kette |
| Backup-Prüfsumme | Nach jedem Backup | SHA-256 des Backup-Files |
| Datenbank-Checkpoint | Bei Bedarf | `PRAGMA wal_checkpoint(TRUNCATE);` |

### 6.2 Backup-Strategie

| Strategie | Details |
|---|---|
| **Täglich** | Vollbackup der `data/kis.db` (online, per `sqlite3 .backup`) → verschlüsselt auf externes Gerät |
| **Wöchentlich** | Vollbackup + WAL-Checkpoint → offsite (getrenntes Netz) |
| **Monatlich** | Archivierung auf Band / Langzeit-Speicher |
| **Verschlüsselung** | AES-256 (AES-256-GCM) mit separat verwaltetem Schlüssel |
| **Aufbewahrung** | 30 Tage lokal, 12 Monate offsite, 10 Jahre archiviert |

**Backup-Befehl:**

```bash
# Online-Backup (konsistent, ohne Datenverlust)
sqlite3 data/kis.db ".backup '/var/lib/kis-backup/kis-$(date +%Y%m%d-%H%M%S).db'"

# Prüfsumme erzeugen
sha256sum /var/lib/kis-backup/kis-*.db > /var/lib/kis-backup/kis-*.db.sha256

# Verschlüsseln (beispielhaft mit age)
age -r recipient.txt /var/lib/kis-backup/kis-*.db
```

### 6.3 Disaster Recovery

| Szenario | RTO (Recovery Time Objective) | RPO (Recovery Point Objective) |
|---|---|---|
| Datenbankdatei korrupt | < 1 Stunde | < 24 Stunden (tägliches Backup) |
| Serverausfall | < 4 Stunden | < 24 Stunden |
| Kompletter Datenverlust | < 24 Stunden | < 24 Stunden |
| Ransomware (verschlüsselte DB) | < 24 Stunden | < 24 Stunden (aus unverschlüsseltem Backup) |

**Wiederherstellung:**

```bash
# 1. Backup dechiffrieren
age -d -i key.txt kis-20260101.db.age > kis-20260101.db

# 2. Prüfsumme verifizieren
sha256sum -c kis-20260101.db.sha256

# 3. Integrität prüfen
sqlite3 kis-20260101.db "PRAGMA integrity_check;"
# erwartet: ok

# 4. Kopieren und starten
cp kis-20260101.db data/kis.db
sudo systemctl restart kis
```

---

## 7. Compliance

### 7.1 MDR (Medical Device Regulation, EU 2017/745)

Das KIS kann als **Medizinprodukt der Klasse IIa** eingestuft werden, wenn es zur Unterstützung klinischer Entscheidungen (z. B. Medikationsprüfung, Terminplanung) beiträgt.

| MDR-Anforderung | Umsetzung im KIS |
|---|---|
| **Risikoanalyse (Annex I)** | STRIDE-Threat-Modell (siehe § 10) |
| **Software-qualifikation** | Unit-Tests, Integrationstests, Code-Review |
| **Validierung & Verifikation** | Teststrategie (DEVELOPMENT.md) |
| **Aufzeichnungen** | Audit-Log (§ 3) |
| **Post-Market Surveillance** | Monitoring + Incident-Response (§ 9) |
| **CE-Kennzeichnung** | Erfordert zertifizierten Hersteller + benannte Stelle (Class IIa) |

> **Hinweis:** Die MDR-Zertifizierung ist ein **mehrjähriger Prozess** mit Beteiligung einer benannten Stelle (Notified Body). Die Dokumentation in diesem Repository ist ein **Baustein**, ersetzt aber nicht die formale MDR-Dokumentation.

### 7.2 ISO 27001 (Informationssicherheits-Management)

| ISO-27001-Kontrollbereich | KIS-Entsprechung |
|---|---|
| **A.5.1** (Politik) | Diese Sicherheitsdokumentation |
| **A.8.11** (Zugriffskontrolle) | RBAC + JWT (§ 8) |
| **A.8.13** (Verschlüsselung) | TLS 1.3 + SQLCipher (§ 2) |
| **A.8.15** (Logging) | Audit-Log mit Hash-Kette (§ 3) |
| **A.8.16** (Monitoring) | OPERATION.md → Monitoring |
| **A.5.24** (DSGVO-Compliance) | § 1 |
| **A.5.29** (Backup) | § 6.2 |
| **A.17.1** (DR) | § 6.3 |

### 7.3 HIPAA-Mapping (für internationale Referenz)

| HIPAA-Anforderung | KIS-Entsprechung |
|---|---|
| **§ 164.312(a)** (Zugriffskontrolle) | RBAC + JWT |
| **§ 164.312(e)** (Übertragungssicherheit) | TLS 1.3 |
| **§ 164.312(b)** (Audit-Controls) | Audit-Log |
| **§ 164.312(c)** (Integrität) | Hash-Kette, SQLX |
| **§ 164.308** (Sicherheitsverwaltung) | SECURITY.md + OPERATION.md |

### 7.4 TÜV-Zertifizierungspfad

| Schritt | Inhalt |
|---|---|
| 1. Lückenanalyse | GAP-Analyse gegen ISO 27001 / MDR |
| 2. ISMS-Aufbau | Dokumentation, Richtlinien, Verantwortlichkeiten |
| 3. Umsetzung | Technische Maßnahmen (dieses Dokument) |
| 4. Interne Audit | Prüfung der Umsetzung |
| 5. Externes Audit | TÜV / Zertifizierungsstelle |
| 6. Zertifizierung | Zertifikat (gültig 3 Jahre, jährliches Aufrechterhaltungs-Audit) |

> **Empfehlung:** Parallel zur MDR die ISO 27001-Zertifizierung anstreben — viele Anforderungen überlappen und die gemeinsame Zertifizierung reduziert den Aufwand.

---

## 8. Zugriffskontrolle

### 8.1 RBAC (Role-Based Access Control)

| Rolle | patient.read | patient.write | appointment.read | appointment.write | medication.read | medication.write | doctor.read | admin |
|---|---|---|---|---|---|---|---|---|
| **admin** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **doctor** | ✅ (eigene Patienten) | ✅ (eigene) | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| **nurse** | ✅ (eigene Patienten) | ✅ (eingeschränkt) | ✅ | ✅ (Status) | ✅ | ❌ | ❌ | ❌ |
| **reception** | ✅ (Stammdaten) | ✅ (Stammdaten) | ✅ | ✅ (Termin) | ❌ | ❌ | ✅ | ❌ |

> **Prinzip der geringsten Rechte:** Jede Rolle erhält **nur** die Berechtigungen, die für ihre Aufgabe erforderlich sind.

### 8.2 JWT-Authentifizierung

**Token-Struktur (JSON Web Token, RS256):**

```json
{
  "sub": "user-uuid",
  "role": "doctor",
  "org": "klinik-01",
  "iat": 1730000000,
  "exp": 1730003600,
  "jti": "token-uuid"
}
```

| Feld | Beschreibung |
|---|---|
| `sub` | Benutzer-ID (UUID) |
| `role` | RBAC-Rolle |
| `org` | Organisation/Klinik-Identifikation |
| `iat` | Issued At (Unix-Timestamp) |
| `exp` | Expires At (Unix-Timestamp) |
| `jti` | JWT-ID (für Revocation) |

**Token-Lebenszyklus:**

| Parameter | Wert |
|---|---|
| Access-Token | 15 Minuten |
| Refresh-Token | 8 Stunden (rotierend) |
| Refresh-Token-Speicher | HttpOnly-Cookie + DB (für Revocation) |
| Algorithmus | RS256 (asymmetrisch) |
| Revocation | `jti`-Blacklist in DB |

**Login-Ablauf:**

```
1. Benutzer sendet POST /api/auth/login { username, password }
2. Server verifiziert Passwort (Argon2id, m=64MB, t=3, p=4)
3. Server erzeugt Access-Token (15 Min) + Refresh-Token (8 h)
4. Refresh-Token wird in HttpOnly-Cookie gesetzt
5. Client verwendet Access-Token in Authorization-Header:
   Authorization: Bearer <access_token>
```

### 8.3 Session-Management

| Maßnahme | Wert |
|---|---|
| Session-Timeout (inaktiv) | 30 Minuten |
| Session-Timeout (absolut) | 8 Stunden |
| Concurrent Sessions | 1 pro Benutzer (neue Login invalidiert alte) |
| Cookie-Flags | `HttpOnly; Secure; SameSite=Strict` |
| Logout | Access-Token + Refresh-Token revocieren |

---

## 9. Incident Response

### 9.1 Eskalationsstufen

| Stufe | Auslöser | Antwortzeit | Benachrichtigung |
|---|---|---|---|
| **P1 — Kritisch** | Datenleck, Ransomware, Systemdown | < 15 Minuten | Datenschutzbeauftragte(r), Geschäftsführung, IT-Sicherheit |
| **P2 — Hoch** | Ausfall einer Schnittstelle, Integritätsverletzung | < 1 Stunde | IT-Leitung, Datenschutzbeauftragte(r) |
| **P3 — Mittel** | Performance-Einbruch, einzelne fehlerhafte Anfrage | < 4 Stunden | IT-Team |
| **P4 — Gering** | Einzelne Benutzereingabe-Fehler | < 24 Stunden | IT-Team |

### 9.2 Datenschutzverletzung — 72-Stunden-Frist

```
T+0h      : Incident erkannt
T+0–1h    : Klassifizierung (DSGVO-relevant?), Eskalation an DSB
T+1–4h    : Eindämmung (Isolation, Schlüssel-Reset, Log-Sicherung)
T+4–24h   : Umfangsanalyse (welche Daten, welche Betroffenen)
T+24–72h  : Meldung an Aufsichtsbehörde (Art. 33 DSGVO)
T+72h+    : Mitteilung an Betroffene (Art. 34 DSGVO), falls hohes Risiko
```

> **Wichtig:** Die 72-Stunden-Frist beginnt mit der **Kenntnisnahme** der Datenschutzverletzung durch den Verantwortlichen. Jede Verzögerung muss dokumentiert werden.

### 9.3 Forensik und Log-Sicherung

| Maßnahme | Details |
|---|---|
| **Memoriereichen** | RAM-Dump des betroffenen Servers (vor Neustart!) |
| **Disk-Image** | Bitweises Image der DB-Partition |
| **Log-Export** | Audit-Log, System-Logs, Firewall-Logs |
| **Hash-Vergleich** | Integritätsprüfung der Audit-Log-Hash-Kette |
| **Timeline** | Rekonstruktion der Ereignissequenz aus Audit-Log |

---

## 10. Threat Model: STRIDE-Analyse

| STRIDE-Kategorie | KIS-spezifische Bedrohung | Auswirkung | Gegenmaßnahme |
|---|---|---|---|
| **S**poofing | Angreifer gibt sich als Arzt aus, greift auf Patientendaten zu | Datenschutzverletzung, § 203 StGB | JWT + RBAC (§ 8), MFA (Soll), Audit-Log |
| **T**ampering | Nachträgliche Änderung von Medikationsdaten | Falsche Therapie, Patientenschaden | Hash-Kette im Audit-Log (§ 3), SQLX (keine String-Konkatenation) |
| **R**epudiation | Benutzer bestreitet, Patientendaten gesehen zu haben | Forensische Aufarbeitung unmöglich | Audit-Log mit Zeitstempel + IP (§ 3) |
| **I**nformation Disclosure | Unbefugter erhält Kassennummern oder Diagnosen | Art. 9 DSGVO, § 203 StGB | Verschlüsselung at rest + transit (§ 2), RBAC |
| **D**enial of Service | Attacke auf Port 8080, KIS nicht erreichbar | Versorgungslücke | Rate-Limiting (§ 5.4), Firewall, Load-Balancer, Redundanz |
| **E**levation of Privilege | Benutzer mit Rolle `reception` erhält `admin`-Rechte | Vollständiger Datenzugriff | JWT-Rolle wird serverseitig verifiziert, RBAC-Matrix (§ 8.1) |

### KIS-spezifische Bedrohungen (ergänzend zu STRIDE)

| Bedrohung | Kontext | Gegenmaßnahme |
|---|---|---|
| **Insider-Bedrohung** | Berechtigtes Personal greift auf Daten unberechtigter Patienten zu | RBAC (eigene Patienten), Audit-Log, Zugriffsalarm |
| **Schnittstellen-Missbrauch** | Externes System sendet gefälschte HL7-Nachrichten | AE-Whitelist, Nachrichtensignatur, MLLP-Auth |
| **DICOM-Injection** | Manipulierte DICOM-Daten (z. B. falscher Patient) | AE-Auth, Patient-ID-Validierung, Hash-Prüfung |
| **Backup-Verschlüsselung umgangen** | Angreifer entwendet unverschlüsseltes Backup | AES-256-GCM, separater Schlüssel, offsite |
| **Supply-Chain** | Kompromittierte Rust-Crate in Cargo.toml | `Cargo.lock` prüfen, `cargo-audit`, signierte Releases (§ 11) |

---

## 11. Secret-Management und Update-Strategie

### 11.1 Secret-Management

| Secret | Wo abgelegt | **NIEMALS** |
|---|---|---|
| LLM-API-Key | Env-Variable `OPENROUTER_API_KEY` (in `/etc/kis/env`) | In `config/kis.toml`, im Code, in Logs |
| Datenbank-Schlüssel (SQLCipher) | Env-Variable `KIS_DB_KEY` (HSM / Secret-Manager) | In `config/kis.toml`, im Code |
| TLS-Private Keys | Dateisystem (chmod 600), nur TLS-Terminator | In `config/kis.toml`, im Code |
| JWT-Signier-Schlüssel (RS256) | HSM / Secret-Manager | In `config/kis.toml`, im Code |
| Datenbank-Passwort (falls vorhanden) | Env-Variable | In `config/kis.toml`, im Code |

> **Grundsatz:** **Kein Secret steht im Code oder in der `config/kis.toml`.** Secrets werden ausschließlich über Umgebungsvariablen oder einen Secret-Manager (HashiCorp Vault, AWS Secrets Manager) injiziert.

### 11.2 Update-Strategie

| Aspekt | Maßnahme |
|---|---|
| **Signierte Releases** | Binärdatei wird mit GPG/Minisign signiert; Signaturen werden auf dem Release-Server publiziert |
| **Prüfsummen** | SHA-256-Prüfsumme für jedes Release; `sha256sum -c` vor Installation |
| **Changelog** | Jedes Release enthält einen Changelog mit Sicherheitsänderungen |
| **Rollback** | Vorheriges Release bleibt verfügbar; DB-Migrationen sind reversibel (Soll) |
| **Patch-Zyklus** | Kritische Patches: < 48 h; Regelmäßige Releases: monatlich |

**Release-Verifikation:**

```bash
# 1. Release herunterladen
curl -O https://releases.kis.example.com/kis-0.2.0-x86_64-linux.tar.gz

# 2. Prüfsumme verifizieren
echo "<sha256>  kis-0.2.0-x86_64-linux.tar.gz" | sha256sum -c -

# 3. Signatur verifizieren
minisign -Vm kis-0.2.0-x86_64-linux.tar.gz -P <public-key>

# 4. Entpacken und installieren
tar xzf kis-0.2.0-x86_64-linux.tar.gz
sudo cp kis-0.2.0/kis /opt/kis/target/release/kis
sudo systemctl restart kis
```

---

## 12. Checkliste für den produktiven Betrieb

| # | Maßnahme | Status |
|---|---|---|
| 1 | TLS 1.3 über Reverse-Proxy | ☐ |
| 2 | `server.host = "127.0.0.1"` (nur hinter Proxy) | ☐ |
| 3 | SQLCipher mit AES-256 aktiviert | ☐ |
| 4 | API-Keys nur in Env, nicht in TOML | ☐ |
| 5 | RBAC + JWT implementiert | ☐ |
| 6 | Audit-Log mit Hash-Kette aktiv | ☐ |
| 7 | Firewall: nur 8080, 6667, 104 offen | ☐ |
| 8 | CORS auf konkrete Origin(s) eingeschränkt | ☐ |
| 9 | Backup: täglich + wöchentlich offsite | ☐ |
| 10 | Disaster-Recovery-Prozedur getestet | ☐ |
| 11 | Incident-Response-Plan bekannt | ☐ |
| 12 | Signierte Releases + Prüfsummen | ☐ |
| 13 | MDR-/ISO-27001-Dokumentation vorhanden | ☐ |
| 14 | Datenschutz-Folgenabschätzung (DSFA) erstellt | ☐ |

---

**Verwandte Dokumente:** [CONFIGURATION.md](CONFIGURATION.md) · [OPERATION.md](OPERATION.md) · [DATABASE.md](DATABASE.md) · [INTERFACES.md](INTERFACES.md)
