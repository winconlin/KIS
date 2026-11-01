# KIS — Datenbank

> **Krankenhaus-Informations-System (KIS)**
> Datenbank-Schemadokumentation: ER-Diagramm, Tabellendokumentation, Index-Strategie, WAL-Modus, Backup/Restore, Migrations-Guide.
>
> **Engine:** SQLite 3 (WAL-Modus)
> **Zugriff:** SQLX (Rust) mit Prepared Statements
> **Datei:** `data/kis.db` (konfigurierbar via `database.path` in `config/kis.toml`)

---

## 1. Überblick

Das KIS verwendet eine **SQLite-Datenbank** im **WAL-Modus** (Write-Ahead Logging). SQLite ist eingebettet (kein separater DB-Server nötig), was die Installation und den Betrieb vereinfacht. Für den produktiven Betrieb mit mehreren gleichzeitigen Schreibzugriffen und höheren Anforderungen sollte eine Migration auf PostgreSQL in Betracht gezogen werden (siehe [OPERATION.md](OPERATION.md) → Skalierung).

### 1.1 Eigenschaften

| Eigenschaft | Wert |
|---|---|
| Engine | SQLite 3 |
| Journal-Modus | **WAL** (Write-Ahead Logging) |
| Datei | `data/kis.db` |
| WAL-Datei | `data/kis.db-wal` |
| SHM-Datei | `data/kis.db-shm` |
| Pool-Größe | 8 Verbindungen (Default) |
| Zugriff | SQLX mit Prepared Statements (keine String-Konkatenation) |
| Migrations | `migrations/001_init.sql` |
| Verschlüsselung | SQLCipher (AES-256) — Soll (siehe [SECURITY.md](SECURITY.md)) |

### 1.2 Warum WAL-Modus?

Der **Write-Ahead Logging (WAL)-Modus** ist der empfohlene Journal-Modus für SQLite in Multi-Reader/Writer-Szenarien:

| Eigenschaft | WAL-Modus | Standard-Modus (rollback) |
|---|---|---|
| Gleichzeitiger Read + Write | ✅ Ja | ❌ Nein (schreibt blockiert Lesezugriffe) |
| Lesezugriffe bei aktivem Write | ✅ Ungehindert | ❌ Blockiert |
| Schreib-Performance | Höher (batched fsync) | Niedriger |
| Recovery nach Crash | Schnell | Langsamer |
| Datei | 3 Dateien (db + wal + shm) | 1 Datei (db + journal) |

> **Wirkung:** Während ein Termin geschrieben wird, können andere Anfragen Patientendaten lesen, ohne blockiert zu werden. Für eine KIS-Umgebung mit parallelem Zugriff durch mehrere Workstations essenziell.

**Aktivierung:**

```sql
PRAGMA journal_mode = WAL;
```

> **Wichtig:** Der WAL-Modus wird einmalig gesetzt und in der Datenbankdatei persistiert. Die `data/kis.db-wal` und `data/kis.db-shm` Dateien sind **Teil des Datenbestands** und müssen bei Backups **zusammen** mit `kis.db` gesichert werden (oder ein `wal_checkpoint(TRUNCATE)` vor dem Backup durchgeführt werden).

---

## 2. ER-Diagramm (ASCII)

```
┌──────────────────┐       ┌──────────────────┐
│    patients      │       │     doctors      │
├──────────────────┤       ├──────────────────┤
│ id (PK, UUID)    │       │ id (PK, UUID)    │
│ first_name       │       │ name             │
│ last_name        │       │ specialty        │
│ birth_date       │       │ license_number   │
│ insurance_number │       │ created_at       │
│ created_at       │       │ updated_at       │
│ updated_at       │       └────────┬─────────┘
└──────┬───────────┘                │
       │                            │
       │ 1                          │ 1
       │                            │
       │ N                          │ N
┌──────┴───────────┐       ┌────────┴─────────┐
│   appointments   │       │  medications     │
├──────────────────┤       ├──────────────────┤
│ id (PK, UUID)    │       │ id (PK, UUID)    │
│ patient_id (FK)──┼──►patients               │
│ doctor_id (FK)───┼──►doctors                │
│ start_time       │       │ patient_id (FK)──┼──►patients
│ end_time         │       │ name             │
│ status           │       │ dosage           │
│ notes            │       │ start_date       │
│ created_at       │       │ end_date         │
│ updated_at       │       │ prescriber_id(FK)┼──►doctors
└──────────────────┘       │ created_at       │
                           │ updated_at       │
                           └──────────────────┘

┌──────────────────┐
│    audit_log     │   (Soll — fälschungssicheres Prüfprotokoll)
├──────────────────┤
│ seq (PK)         │
│ timestamp        │
│ actor_id         │
│ action           │
│ resource_type    │
│ resource_id      │
│ ip_address       │
│ payload_hash     │
│ prev_hash        │
│ entry_hash       │
└──────────────────┘
```

### 2.1 Beziehungen

| Beziehung | Art | Beschreibung |
|---|---|---|
| `patients` → `appointments` | 1 : N | Ein Patient hat mehrere Termine |
| `doctors` → `appointments` | 1 : N | Ein Arzt hat mehrere Termine |
| `patients` → `medications` | 1 : N | Ein Patient hat mehrere Medikationen |
| `doctors` → `medications` | 1 : N | Ein Arzt verschreibt mehrere Medikamente |
| `patients` → `audit_log` | (indirekt) | Audit-Log referenziert Ressourcen |
| `doctors` → `audit_log` | (indirekt) | Audit-Log referenziert Ressourcen |

---

## 3. Tabellendokumentation

### 3.1 `patients`

Patientenstammdaten. Enthält **besondere Kategorie** von Daten (Art. 9 DSGVO).

| Spalte | Typ | Constraint | Beschreibung |
|---|---|---|---|
| `id` | TEXT | **PK** | UUID v4, primärer Schlüssel |
| `first_name` | TEXT | **NOT NULL** | Vorname (1–100 Zeichen) |
| `last_name` | TEXT | **NOT NULL** | Nachname (1–100 Zeichen) |
| `birth_date` | TEXT | NULL | Geburtsdatum, Format `YYYY-MM-DD` |
| `insurance_number` | TEXT | NULL | Kassennummer (max. 50 Zeichen) |
| `created_at` | TEXT | **NOT NULL** | Angelegt am (UTC, ISO 8601) |
| `updated_at` | TEXT | **NOT NULL** | Zuletzt aktualisiert (UTC, ISO 8601) |

**SQL:**

```sql
CREATE TABLE IF NOT EXISTS patients (
    id TEXT PRIMARY KEY,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    birth_date TEXT,
    insurance_number TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

**Rust-Modell** (`src/models/mod.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Patient {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub birth_date: Option<String>,
    pub insurance_number: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

---

### 3.2 `doctors`

Ärzte / medizinisches Fachpersonal.

| Spalte | Typ | Constraint | Beschreibung |
|---|---|---|---|
| `id` | TEXT | **PK** | UUID v4 |
| `name` | TEXT | **NOT NULL** | Name (z. B. "Dr. med. Thomas Weber") |
| `specialty` | TEXT | **NOT NULL** | Fachgebiet (z. B. "Kardiologie") |
| `license_number` | TEXT | NULL | Approbationsnummer |
| `created_at` | TEXT | — | **Soll:** Angelegt am (aktuell nicht in `001_init.sql`) |
| `updated_at` | TEXT | — | **Soll:** Zuletzt aktualisiert (aktuell nicht in `001_init.sql`) |

**SQL (Ist, `migrations/001_init.sql`):**

```sql
CREATE TABLE IF NOT EXISTS doctors (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    specialty TEXT NOT NULL,
    license_number TEXT
);
```

> **Soll (Migration 002+):** Index `idx_doctors_name (name)` sowie `created_at`/`updated_at`-Spalten.

---

### 3.3 `appointments`

Termine / Patientenbesuche.

| Spalte | Typ | Constraint | Beschreibung |
|---|---|---|---|
| `id` | TEXT | **PK** | UUID v4 |
| `patient_id` | TEXT | **NOT NULL, FK** | Verweis auf `patients.id` |
| `doctor_id` | TEXT | NULL | Verweis auf `doctors.id` |
| `start_time` | TEXT | **NOT NULL** | Terminbeginn (UTC, ISO 8601) |
| `end_time` | TEXT | **NOT NULL** | Terminende (UTC, ISO 8601) |
| `status` | TEXT | **NOT NULL** | Status (Enum, siehe unten) |
| `notes` | TEXT | NULL | Notizen (max. 2000 Zeichen) |
| `created_at` | TEXT | — | **Soll:** Angelegt am (aktuell nicht in `001_init.sql`) |
| `updated_at` | TEXT | — | **Soll:** Zuletzt aktualisiert (aktuell nicht in `001_init.sql`) |

**Status-Enum** (`AppointmentStatus` in `src/models/mod.rs`, `rename_all = "lowercase"`):

| Wert (DB) | Bedeutung |
|---|---|
| `scheduled` | Termin eingeplant |
| `confirmed` | Bestätigt |
| `completed` | Abgeschlossen |
| `cancelled` | Storniert |
| `noshow` | Patient nicht erschienen |

**SQL (Ist, `migrations/001_init.sql`):**

```sql
CREATE TABLE IF NOT EXISTS appointments (
    id TEXT PRIMARY KEY,
    patient_id TEXT NOT NULL REFERENCES patients(id),
    doctor_id TEXT REFERENCES doctors(id),
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'scheduled',
    notes TEXT
);
CREATE INDEX IF NOT EXISTS idx_appointments_patient ON appointments(patient_id);
CREATE INDEX IF NOT EXISTS idx_appointments_time ON appointments(start_time);
```

> **Soll (Migration 002+):** Zusätzliche Indizes `idx_appointments_doctor (doctor_id)`, `idx_appointments_status (status)` sowie `created_at`/`updated_at`-Spalten.
>
> **FK-Constraints:** SQLite setzt FK-Constraints nur, wenn `PRAGMA foreign_keys = ON;` aktiv ist. Das KIS muss dies bei jeder Verbindung setzen (SQLX: `options.with_foreign_key_enforcement(true)` bzw. PRAGMA im After-Connect-Hook).

---

### 3.4 `medications`

Medikamente / Verschreibungen.

| Spalte | Typ | Constraint | Beschreibung |
|---|---|---|---|
| `id` | TEXT | **PK** | UUID v4 |
| `patient_id` | TEXT | **NOT NULL, FK** | Verweis auf `patients.id` |
| `name` | TEXT | **NOT NULL** | Medikament (z. B. "Ibuprofen") |
| `dosage` | TEXT | **NOT NULL** | Dosierung (z. B. "600 mg 3x/Tag") |
| `start_date` | TEXT | **NOT NULL** | Beginn (UTC, ISO 8601) |
| `end_date` | TEXT | NULL | Ende (UTC, ISO 8601, NULL = offen) |
| `prescriber_id` | TEXT | NULL, FK | Verweis auf `doctors.id` (verschreibender Arzt) |
| `created_at` | TEXT | — | **Soll:** Angelegt am (aktuell nicht in `001_init.sql`) |
| `updated_at` | TEXT | — | **Soll:** Zuletzt aktualisiert (aktuell nicht in `001_init.sql`) |

**SQL (Ist, `migrations/001_init.sql`):**

```sql
CREATE TABLE IF NOT EXISTS medications (
    id TEXT PRIMARY KEY,
    patient_id TEXT NOT NULL REFERENCES patients(id),
    name TEXT NOT NULL,
    dosage TEXT NOT NULL,
    start_date TEXT NOT NULL,
    end_date TEXT,
    prescriber_id TEXT REFERENCES doctors(id)
);
```

> **Soll (Migration 002+):** Indizes `idx_medications_patient (patient_id)`, `idx_medications_name (name)`, `idx_medications_prescriber (prescriber_id)` sowie `created_at`/`updated_at`-Spalten.

---

### 3.5 `audit_log` (Soll)

Fälschungssicheres Prüfprotokoll. Siehe [SECURITY.md](SECURITY.md) → Audit-Logging.

| Spalte | Typ | Constraint | Beschreibung |
|---|---|---|---|
| `seq` | INTEGER | **PK, AUTOINCREMENT** | Sequenznummer |
| `timestamp` | TEXT | **NOT NULL** | Ereigniszeit (UTC, ISO 8601) |
| `actor_id` | TEXT | **NOT NULL** | Benutzer-/System-ID |
| `action` | TEXT | **NOT NULL** | Aktion (z. B. `patient.read`) |
| `resource_type` | TEXT | NULL | Ressourcenart |
| `resource_id` | TEXT | NULL | Ressourcen-ID |
| `ip_address` | TEXT | NULL | Quell-IP |
| `payload_hash` | TEXT | NULL | SHA-256 des Inhalts |
| `prev_hash` | TEXT | **NOT NULL** | SHA-256 des vorherigen Eintrags |
| `entry_hash` | TEXT | **NOT NULL** | SHA-256 des Eintrags (Hash-Kette) |

**SQL:**

```sql
CREATE TABLE IF NOT EXISTS audit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    ip_address TEXT,
    payload_hash TEXT,
    prev_hash TEXT NOT NULL,
    entry_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_resource ON audit_log(resource_type, resource_id);

-- Append-only: UPDATE und DELETE blockieren
CREATE TRIGGER IF NOT EXISTS audit_no_update
    BEFORE UPDATE ON audit_log
    BEGIN
        SELECT RAISE(ABORT, 'audit_log ist append-only');
    END;
CREATE TRIGGER IF NOT EXISTS audit_no_delete
    BEFORE DELETE ON audit_log
    BEGIN
        SELECT RAISE(ABORT, 'audit_log ist append-only');
    END;
```

---

## 4. Index-Strategie

### 4.1 Ist (aus `migrations/001_init.sql`)

Die initiale Migration legt **drei** Indizes an (die Primärschlüssel sind implizit indiziert):

| Tabelle | Index | Spalte(n) | Typ | Zweck |
|---|---|---|---|---|
| `patients` | (PK) | `id` | UNIQUE | Primärschlüssel |
| `patients` | `idx_patients_name` | `(last_name, first_name)` | BTREE (Composite) | Namenssuche |
| `doctors` | (PK) | `id` | UNIQUE | Primärschlüssel |
| `appointments` | (PK) | `id` | UNIQUE | Primärschlüssel |
| `appointments` | `idx_appointments_patient` | `patient_id` | BTREE | Termine des Patienten |
| `appointments` | `idx_appointments_time` | `start_time` | BTREE | Zeitraum-Abfragen |
| `medications` | (PK) | `id` | UNIQUE | Primärschlüssel |

### 4.2 Soll (empfohlene Ergänzungen, Migration 002+)

Für die in der API verwendeten Abfragen (Arzt-Filter, Status-Filter, Medikament-Suche) werden folgende Indizes empfohlen:

| Tabelle | Index | Spalte(n) | Typ | Zweck |
|---|---|---|---|---|
| `doctors` | `idx_doctors_name` | `name` | BTREE | Namenssuche |
| `appointments` | `idx_appointments_doctor` | `doctor_id` | BTREE | Termine des Arztes |
| `appointments` | `idx_appointments_status` | `status` | BTREE | Status-Filter |
| `appointments` | `idx_appointments_doctor_time` | `(doctor_id, start_time)` | BTREE (Composite) | Doppelte Filterung |
| `medications` | `idx_medications_patient` | `patient_id` | BTREE | Medikation des Patienten |
| `medications` | `idx_medications_name` | `name` | BTREE | Medikament-Suche |
| `medications` | `idx_medications_prescriber` | `prescriber_id` | BTREE | Verschreibungen des Arztes |
| `audit_log` | (PK) | `seq` | INTEGER | Primärschlüssel |
| `audit_log` | `idx_audit_time` | `timestamp` | BTREE | Zeitraum-Abfragen |
| `audit_log` | `idx_audit_actor` | `actor_id` | BTREE | Actor-Filter |
| `audit_log` | `idx_audit_resource` | `(resource_type, resource_id)` | BTREE (Composite) | Ressourcen-Filter |

```sql
-- Migration 002_add_indexes.sql (Soll)
CREATE INDEX IF NOT EXISTS idx_doctors_name ON doctors(name);
CREATE INDEX IF NOT EXISTS idx_appointments_doctor ON appointments(doctor_id);
CREATE INDEX IF NOT EXISTS idx_appointments_status ON appointments(status);
CREATE INDEX IF NOT EXISTS idx_appointments_doctor_time ON appointments(doctor_id, start_time);
CREATE INDEX IF NOT EXISTS idx_medications_patient ON medications(patient_id);
CREATE INDEX IF NOT EXISTS idx_medications_name ON medications(name);
CREATE INDEX IF NOT EXISTS idx_medications_prescriber ON medications(prescriber_id);
```

### 4.3 Composite-Index

Der Composite-Index `idx_appointments_doctor_time (doctor_id, start_time)` (Soll) optimiert die häufige Abfrage "alle Termine eines Arztes im Zeitraum":

```sql
SELECT * FROM appointments
WHERE doctor_id = ? AND start_time BETWEEN ? AND ?
ORDER BY start_time;
```

> **Regel:** Die häufigsten Filter-/Sortier-Kombinationen erhalten eigene Composite-Indizes. Einzel-Indizes werden nur für Spalten verwendet, die häufig isoliert gefiltert werden.

### 4.2 Volltextsuche (FTS5) — Soll

Für die Patientensuche (`q`-Parameter) wird ein **FTS5**-Volltextindex empfohlen:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS patients_fts USING fts5(
    first_name,
    last_name,
    content='patients',
    content_rowid='rowid'
);

-- Trigger zur Synchronisation
CREATE TRIGGER IF NOT EXISTS patients_ai AFTER INSERT ON patients BEGIN
    INSERT INTO patients_fts(rowid, first_name, last_name)
    VALUES (new.rowid, new.first_name, new.last_name);
END;
CREATE TRIGGER IF NOT EXISTS patients_ad AFTER DELETE ON patients BEGIN
    INSERT INTO patients_fts(patients_fts, rowid, first_name, last_name)
    VALUES ('delete', old.rowid, old.first_name, old.last_name);
END;
CREATE TRIGGER IF NOT EXISTS patients_au AFTER UPDATE ON patients BEGIN
    INSERT INTO patients_fts(patients_fts, rowid, first_name, last_name)
    VALUES ('delete', old.rowid, old.first_name, old.last_name);
    INSERT INTO patients_fts(rowid, first_name, last_name)
    VALUES (new.rowid, new.first_name, new.last_name);
END;
```

**Abfrage:**

```sql
SELECT p.id, p.first_name, p.last_name
FROM patients_fts f
JOIN patients p ON p.rowid = f.rowid
WHERE patients_fts MATCH ?
ORDER BY rank
LIMIT 20;
```

---

## 5. WAL-Modus — Details

### 5.1 Funktion

```
┌─────────────────────────────────────────────────────────┐
│  SQLite WAL-Modus                                       │
│                                                         │
│  [kis.db]  ──schreiben──►  [kis.db-wal]  ──checkpoint──► [kis.db]
│     ▲                              │                     │
│     │                              └──lesen──►  [kis.db-shm]
│     │                                        (Shared Memory)
│     └──────────lesen──────────────────────────┘
│                                                         │
│  Lesezugriffe:  immer auf [kis.db] (konsistent)         │
│  Schreibzugriffe: erst in [kis.db-wal], dann checkpoint │
└─────────────────────────────────────────────────────────┘
```

- **Lesezugriffe** lesen direkt aus `kis.db`. Sie sind **nie blockiert** durch Schreibzugriffe.
- **Schreibzugriffe** schreiben zunächst in `kis.db-wal`. Ein **Checkpoint** (automatisch oder manuell) übernimmt die Änderungen in `kis.db`.
- **`kis.db-shm`** (Shared Memory) koordiniert die Zugriffe mehrerer Prozesse.

### 5.2 Checkpoint

| Typ | Befehl | Wirkung |
|---|---|---|
| `PASSIVE` | `PRAGMA wal_checkpoint(PASSIVE);` | Übernimmt, was möglich ist (blockiert nie) |
| `FULL` | `PRAGMA wal_checkpoint(FULL);` | Übernimmt alles, blockiert bis fertig |
| `RESTART` | `PRAGMA wal_checkpoint(RESTART);` | Wie FULL, aber WAL-Datei wird zurückgesetzt |
| `TRUNCATE` | `PRAGMA wal_checkpoint(TRUNCATE);` | Wie RESTART + WAL-Datei wird auf 0 Bytes gekürzt |

> **Empfehlung:** Vor Backups `TRUNCATE` ausführen, um sicherzustellen, dass alle Änderungen in `kis.db` sind und die WAL-Datei leer ist.

### 5.3 WAL-Größe steuern

```sql
-- Maximale WAL-Größe auf 1000 Seiten (~4 MB) begrenzen
PRAGMA wal_autocheckpoint = 1000;   -- Default: 1000 Seiten

-- Synchronisierungsmodus für Performance/Integrität
PRAGMA synchronous = NORMAL;        -- Empfohlen für WAL (schnell + sicher)
```

> **Hinweis:** `synchronous = NORMAL` ist in WAL-Modus sicher (kein Datenverlust) und deutlich schneller als `FULL`. Für höchste Integrität `FULL` verwenden.

### 5.4 Backup im WAL-Modus

**Option A: Online-Backup (empfohlen)**

```bash
sqlite3 data/kis.db ".backup '/backup/kis-$(date +%Y%m%d).db'"
```

> Das `.backup`-Kommando erzeugt ein konsistentes Snapshot, auch bei aktivem WAL-Modus. WAL- und SHM-Dateien müssen **nicht** gesichert werden.

**Option B: Datei-Kopie (nach Checkpoint)**

```bash
# 1. Checkpoint durchführen
sqlite3 data/kis.db "PRAGMA wal_checkpoint(TRUNCATE);"

# 2. Alle drei Dateien kopieren
cp data/kis.db /backup/kis.db
cp data/kis.db-wal /backup/kis.db-wal
cp data/kis.db-shm /backup/kis.db-shm
```

> **Wichtig:** Bei Option B müssen **alle drei** Dateien (`kis.db`, `kis.db-wal`, `kis.db-shm`) konsistent zusammen kopiert werden, sonst ist das Backup unvollständig.

---

## 6. Backup und Restore

### 6.1 Backup-Strategie

| Typ | Frequenz | Aufbewahrung |
|---|---|---|
| Vollbackup (online) | Täglich (02:00) | 30 Tage lokal |
| Vollbackup offsite | Wöchentlich | 12 Monate |
| Archiv | Monatlich | 10 Jahre |

> Details siehe [OPERATION.md](OPERATION.md) → Backup und [SECURITY.md](SECURITY.md) → Datenintegrität.

### 6.2 Restore-Verfahren

```bash
# 1. KIS-Server stoppen
sudo systemctl stop kis

# 2. Backup (ggf. dechiffrieren + Prüfsumme) prüfen
sha256sum -c kis-20261101.db.sha256
sqlite3 kis-20261101.db "PRAGMA integrity_check;"   # erwartet: ok

# 3. Altes DB-Verzeichnis sichern
mv data/kis.db data/kis.db.bak-$(date +%s)
rm -f data/kis.db-wal data/kis.db-shm

# 4. Backup kopieren
cp kis-20261101.db data/kis.db

# 5. Server starten
sudo systemctl start kis

# 6. Verifikation
curl -fsS http://localhost:8080/api/health   # erwartet: OK
sqlite3 data/kis.db "SELECT count(*) FROM patients;"
```

### 6.3 Point-in-Time-Recovery

SQLite unterstützt **keine** native Point-in-Time-Recovery (im Gegensatz zu PostgreSQL mit WAL-Archivierung). Die Wiederherstellung ist auf das **letzte Vollbackup** beschränkt (RPO = Zeit seit letztem Backup).

> **Kompensation:** Häufige Backups (mindestens täglich) + Audit-Log (für forensische Rekonstruktion) + HL7-Replay (wenn LIS die Quelle ist).

---

## 7. Migrations-Guide

### 7.1 Migrationssystem

Migrations liegen in `migrations/` als nummerierte SQL-Dateien:

```
migrations/
├── 001_init.sql          # Initiales Schema
├── 002_add_audit_log.sql # (Soll) Audit-Log-Tabelle
├── 003_add_fts.sql       # (Soll) FTS5-Index
└── ...
```

> **KIS-Ist:** Aktuell enthält `migrations/001_init.sql` das initiale Schema. SQLX-Migrations werden über `sqlx::migrate!()` eingebunden (Soll).

### 7.2 Migration schreiben

1. Neue Datei anlegen: `migrations/002_add_audit_log.sql`
2. Idempotent schreiben (`IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`)
3. **Reversibel** gestalten (Down-Migration in Kommentar)
4. Testen (siehe unten)
5. Version bumpen (Changelog)

**Beispiel:**

```sql
-- 002_add_audit_log.sql
-- UP:
CREATE TABLE IF NOT EXISTS audit_log (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    ip_address TEXT,
    payload_hash TEXT,
    prev_hash TEXT NOT NULL,
    entry_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp);

-- DOWN (manuell, bei Rollback):
-- DROP INDEX IF EXISTS idx_audit_time;
-- DROP TABLE IF EXISTS audit_log;
```

### 7.3 Migration ausführen

```bash
# Über SQLX (im Code):
# sqlx::migrate!("./migrations").run(&pool).await?;

# Oder manuell:
sqlite3 data/kis.db < migrations/002_add_audit_log.sql
```

### 7.4 Migrations-Status prüfen

```bash
# Welche Tabellen existieren?
sqlite3 data/kis.db ".tables"

# Schema anzeigen
sqlite3 data/kis.db ".schema"
```

> **Soll:** Eine `schema_migrations`-Tabelle zur Verfolgung angewendeter Migrations:
> ```sql
> CREATE TABLE IF NOT EXISTS schema_migrations (
>     version INTEGER PRIMARY KEY,
>     name TEXT NOT NULL,
>     applied_at TEXT NOT NULL
> );
> ```

### 7.5 Rollback

```bash
# Letzte Migration zurückbauen (manuell, DOWN-Skript aus Kommentar)
sqlite3 data/kis.db "DROP INDEX IF EXISTS idx_audit_time; DROP TABLE IF EXISTS audit_log;"
```

> **Wichtig:** Rollbacks in der Produktion sind **höchst vorsichtig** zu behandeln. Patientendaten in abgebrochenen Migrations dürfen nicht verloren gehen. Vor Rollback immer ein Vollbackup erstellen.

---

## 8. Wartung und Integrität

### 8.1 Regelmäßige Wartung

| Aufgabe | Befehl | Intervall |
|---|---|---|
| Integritätsprüfung | `PRAGMA integrity_check;` | Täglich |
| Vakuumpacken | `VACUUM;` | Wöchentlich (außerhalb der Stoßzeit) |
| WAL-Checkpoint | `PRAGMA wal_checkpoint(TRUNCATE);` | Nach Backups |
| Analyse-Stats | `PRAGMA optimize;` | Nach großen Schreiboperationen |
| FK-Prüfung | `PRAGMA foreign_key_check;` | Wöchentlich |

### 8.2 Integritätsprüfung

```bash
sqlite3 data/kis.db "PRAGMA integrity_check;"
# erwartet: ok

sqlite3 data/kis.db "PRAGMA foreign_key_check;"
# erwartet: (leere Ausgabe)

sqlite3 data/kis.db "PRAGMA quick_check;"
# schnellere Grundprüfung
```

### 8.3 VACUUM

```bash
# Datenbankdatei kompaktieren (freier Speicher wird freigegeben)
sqlite3 data/kis.db "VACUUM;"
# Hinweis: blockiert Schreibzugriffe während der Ausführung → außerhalb der Stoßzeit
```

> **Wichtig:** `VACUUM` sperrt die Datenbank. Nicht während des laufenden Betriebs ausführen (bzw. auf niedrige Last-Zeiten legen).

---

## 9. Performance-Optimierung

| Parameter | Empfehlung |
|---|---|
| `cache_size` | `-64000` (64 MB) — abhängig vom RAM |
| `journal_mode` | `WAL` |
| `synchronous` | `NORMAL` |
| `temp_store` | `MEMORY` |
| `mmap_size` | `268435456` (256 MB) |
| `pool_size` | 8 (Default), bei hoher Last erhöhen |

```sql
PRAGMA cache_size = -64000;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;
```

> **Skalierung:** Bei sehr hohem Durchsatz (mehrere hundert Anfragen/Sekunde) oder verteiltem Betrieb sollte eine Migration auf **PostgreSQL** in Betracht gezogen werden (Replikation, Connection-Pooling, höhere Parallelität).

---

**Verwandte Dokumente:** [SECURITY.md](SECURITY.md) · [OPERATION.md](OPERATION.md) · [API.md](API.md)
