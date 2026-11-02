# KIS — Datenbank

Stand: 02.11.2026 · Aktives Schema aus migrations/001 bis 004

## Engine und Verbindungen

Der Root-Build verwendet SQLite über SQLx. Dateipfad, Pool-Größe und Journal-Modus kommen aus der Konfiguration. WAL ist der Default; delete ist ebenfalls unterstützt. Foreign Keys, fünf Sekunden Busy-Timeout und synchronous=FULL werden auf Verbindungen angewendet.

Die Datei und ihr Elternverzeichnis werden beim Start angelegt. Migrationen sind in das Binary eingebettet. Ein Fehler verhindert den Serverstart. PostgreSQL und Read-Replicas sind Ausbauziele; die Entwürfe unter crates/ werden noch nicht gebaut.

## Aktuelle Tabellen

Die folgende Referenz wurde aus den eingecheckten SQLite-Migrationen erzeugt. PK, NOT NULL und Defaults entsprechen dem Schema; die Darstellung ersetzt keine fachliche Modellvalidierung.

### appointments

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| external_id | TEXT |  |
| patient_id | TEXT |  |
| doctor_id | TEXT |  |
| start_time | TEXT | NOT NULL |
| end_time | TEXT | NOT NULL |
| status | TEXT | NOT NULL, Default 'scheduled' |
| reason | TEXT |  |
| location | TEXT |  |
| notes | TEXT |  |
| created_at | TEXT | NOT NULL |
| updated_at | TEXT | NOT NULL |
| deleted_at | TEXT |  |

### audit_log

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | INTEGER | PK |
| timestamp | TEXT | NOT NULL |
| user_id | TEXT |  |
| username | TEXT |  |
| action | TEXT | NOT NULL |
| entity_type | TEXT | NOT NULL |
| entity_id | TEXT |  |
| old_value | TEXT |  |
| new_value | TEXT |  |
| ip_address | TEXT |  |
| prev_hash | TEXT |  |
| current_hash | TEXT | NOT NULL |
| hash_version | INTEGER | NOT NULL, Default 1 |

### consents

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| patient_id | TEXT |  |
| type | TEXT | NOT NULL |
| granted | INTEGER | NOT NULL |
| granted_at | TEXT | NOT NULL |
| granted_by | TEXT |  |
| withdrawn_at | TEXT |  |
| withdrawn_by | TEXT |  |
| legal_basis | TEXT |  |
| created_at | TEXT | NOT NULL |
| updated_at | TEXT | NOT NULL |

### doctors

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| patient_id | TEXT |  |
| external_id | TEXT |  |
| first_name | TEXT | NOT NULL |
| last_name | TEXT | NOT NULL |
| title | TEXT |  |
| specialty | TEXT |  |
| license_no | TEXT |  |
| phone | TEXT |  |
| email | TEXT |  |
| department | TEXT |  |
| created_at | TEXT | NOT NULL |
| updated_at | TEXT | NOT NULL |
| deleted_at | TEXT |  |

### medications

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| patient_id | TEXT |  |
| name | TEXT | NOT NULL |
| dosage | TEXT |  |
| form | TEXT |  |
| frequency | TEXT |  |
| prescriber_id | TEXT |  |
| start_date | TEXT | NOT NULL |
| end_date | TEXT |  |
| active | INTEGER | NOT NULL, Default 1 |
| notes | TEXT |  |
| created_at | TEXT | NOT NULL |
| updated_at | TEXT | NOT NULL |
| deleted_at | TEXT |  |

### patients

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| external_id | TEXT |  |
| first_name | TEXT | NOT NULL |
| last_name | TEXT | NOT NULL |
| birth_date | TEXT | NOT NULL |
| gender | TEXT |  |
| insurance_no | TEXT |  |
| insurance_name | TEXT |  |
| address_line | TEXT |  |
| address_city | TEXT |  |
| address_zip | TEXT |  |
| phone | TEXT |  |
| email | TEXT |  |
| blood_type | TEXT |  |
| allergies | TEXT | NOT NULL, Default '[]' |
| notes | TEXT |  |
| created_at | TEXT | NOT NULL |
| updated_at | TEXT | NOT NULL |
| deleted_at | TEXT |  |
| insurance_no_enc | TEXT |  |
| notes_enc | TEXT |  |
| phone_enc | TEXT |  |

### sessions

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| user_id | TEXT |  |
| jti | TEXT | NOT NULL |
| issued_at | TEXT | NOT NULL |
| expires_at | TEXT | NOT NULL |
| revoked_at | TEXT |  |
| ip_address | TEXT |  |
| access_jti | TEXT |  |

### users

| Spalte | Typ | Constraint / Default |
|---|---|---|
| id | TEXT | PK |
| username | TEXT | NOT NULL |
| password_hash | TEXT | NOT NULL |
| display_name | TEXT | NOT NULL |
| email | TEXT | NOT NULL |
| role | TEXT | NOT NULL, Default 'reception' |
| is_active | INTEGER | NOT NULL, Default 1 |
| mfa_secret | TEXT |  |
| mfa_enabled | INTEGER | NOT NULL, Default 0 |
| last_login_at | TEXT |  |
| login_attempts | INTEGER | NOT NULL, Default 0 |
| locked_until | TEXT |  |
| created_at | TEXT | NOT NULL |
| updated_at | TEXT | NOT NULL |
| deleted_at | TEXT |  |

## Beziehungen und Modellgrenzen

appointments referenziert patients und doctors; medications referenziert patients und den verordnenden Arzt. sessions referenziert users, consents referenziert patients und Benutzer. Die ursprüngliche doctors.patient_id-Beziehung ist weiterhin im Schema vorhanden und vor fachlichen Workflows kritisch zu prüfen. Mehrere Fremdschlüssel sind nullable und besitzen ON DELETE CASCADE; daraus folgt noch kein korrektes klinisches Löschkonzept.

Die API-Modellentwürfe unter src/models/ sind keine direkten SQLx-Row-Abbildungen. Dort vorhandene Namen wie insurance_number bzw. Doctor.name müssen vor CRUD über ein ausdrückliches Mapping an insurance_no bzw. first_name/last_name angeschlossen werden. Aktuell sind die Datenhandler noch Platzhalter.

Es gibt noch keine Version-Spalten für Optimistic Locking oder verbindliche externe Identifier-Unique-Constraints. Patienten mit unvollständigem FHIR-Geburtsdatum benötigen eine abgestimmte Importregel, weil birth_date im aktuellen Schema verpflichtend ist.

## Audit und Sessions

audit_log verwendet id und current_hash. Neue Einträge schreiben hash_version=2. Der Code prüft Inhalt und Vorgänger; ein externer Prüfanker bleibt offen. Die Tabelle ist nicht durch Trigger gegen UPDATE/DELETE gesperrt. Die ergänzende JSONL-Kopie ist nicht der maßgebliche Datenbestand.

sessions speichert Refresh-JTI und ab Migration 003 die Access-JTI. Logout und Rotation widerrufen die gemeinsame Sitzung über revoked_at. Die Prüfung berücksichtigt auch den aktuellen Kontostatus. Bestehende Refresh-only-Zeilen ohne access_jti sind keine gültigen Access-Sitzungen.

## Migrationen

| Version | Datei | Inhalt |
|---|---|---|
| 001 | 001_init.sql | Benutzer und fachliche Grundtabellen |
| 002 | 002_security.sql | Verschlüsselte Zusatzspalten, Audit, Sessions und Consents |
| 003 | 003_session_access_token.sql | Access-JTI der Sitzung |
| 004 | 004_audit_hash_version.sql | Versionierter Audit-Hash |

SQLx verwaltet Versionsnummern und Prüfsummen in _sqlx_migrations. Keine eigene parallele schema_migrations-Tabelle oder manuelle Markierung einführen. Neue Migrationen bekommen eine freie fortlaufende Nummer. Bereits angewandte Dateien nicht ungeprüft verändern.

**Hinweis zur bisherigen 002:** Die ursprüngliche ADD COLUMN IF NOT EXISTS-Syntax war unter SQLite nicht ausführbar. Dieser Fehler wird korrigiert. Wer die Datei zuvor selbst repariert und angewandt hat, muss seine Prüfsumme und sein reales Schema mit der neuen Fassung abgleichen; ein solcher Bestand kann bewusst am SQLx-Checksum-Check scheitern. Nicht durch Löschen von Daten oder pauschales Überspringen der Prüfung umgehen. Zuerst sichern und einen geprüften Upgrade-/Restore-Pfad festlegen.

Ein DB-Rollback erfolgt mit einem passenden geprüften Snapshot und Release. Ein improvisiertes DROP audit_log würde Daten verlieren und SQLx-Metadaten nicht zurücksetzen.

## WAL, Durabilität und Backup

WAL-Leser verwenden eine konsistente Sicht aus Hauptdatei und WAL. Es gibt weiterhin nur einen gleichzeitigen Writer. Ein Checkpoint kann wegen aktiver Leser nicht jede gewünschte Bereinigung sofort ausführen. wal_autocheckpoint ist eine Schwelle, kein hartes Größenlimit.

synchronous=NORMAL im WAL-Modus ist nicht dieselbe Durabilitätszusage wie FULL: Bei Strom-/Systemausfall können zuletzt bestätigte Transaktionen verloren gehen. Das aktuelle Verbindungssetup verwendet FULL.

Für Backups die SQLite-Backup-API verwenden, nicht DB/WAL/SHM bei laufenden Schreibern nacheinander kopieren. Das vorhandene Werkzeug erzeugt einen konsistenten Snapshot und verschlüsselt ihn. Anleitung und Restore stehen in [OPERATION.md](OPERATION.md).

## Integritätsprüfung

Für eine existierende Datenbank und bei korrektem Arbeitsverzeichnis:

```bash
sqlite3 data/kis.db "PRAGMA integrity_check;"
sqlite3 data/kis.db "PRAGMA foreign_key_check;"
sqlite3 data/kis.db "SELECT id, action, timestamp FROM audit_log ORDER BY id DESC LIMIT 20;"
```

Ein Schreib-/Lesezugriff mit sqlite3 kann bei einem falschen Pfad eine neue leere Datei erzeugen. Das Backup-Prüfwerkzeug öffnet deshalb nur existierende Dateien im read-only-Modus und prüft zusätzlich die Tabellen. Für Wartung wie VACUUM sind Last, Platzbedarf, Sicherung und Wartungsfenster zu berücksichtigen.
