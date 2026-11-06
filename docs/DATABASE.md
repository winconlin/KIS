# KIS — Datenbank

Stand: 06.11.2026 · PostgreSQL 16 · Schema aus `migrations/001` bis `004`

## Engine, Rollen und Migrationen

KIS verwendet ausschließlich PostgreSQL (SQLx 0.8). Es gibt keinen Fallback auf SQLite oder eine In-Memory-Datenbank.

| Rolle | Zweck | Rechte |
|---|---|---|
| `kis_owner` | Schema-Eigentümer, nur für Migrationen (`database.migration_url`) | DDL |
| `kis_app` | Laufzeitrolle des Servers (`database.write_url`) | SELECT/INSERT/UPDATE/DELETE; auf `audit_logs` nur SELECT/INSERT |

Einrichtung: [`config/postgres/roles.sql`](../config/postgres/roles.sql) einmalig als Superuser ausführen. Migrationen: `kis-admin migrate` oder beim Serverstart, wenn `database.migration_url` gesetzt ist. Ohne `migration_url` migriert der Server über die Laufzeitverbindung und warnt (nur Entwicklung).

Migrationen sind in das Binary eingebettet und werden über `_sqlx_migrations` mit Prüfsumme verfolgt. Eine geänderte, bereits angewendete Migration bricht den Start ab.

**Hinweis zu 004:** Die früheren Dateien `003_session_access_token.sql` (doppelte Version 3) und `004_audit_hash_version.sql` (änderte die in 003 entfernte Tabelle `audit_log`) stammten aus dem SQLite-Prototyp. Mit ihnen ließ sich keine PostgreSQL-Datenbank vollständig migrieren. Sie wurden durch `004_sessions_anonymization_audit_guard.sql` ersetzt. Bestehende Datenbanken mit einem Teilstand gibt es laut Projektentscheidung vom 02.11.2026 nicht (nur frische Installationen).

## Integritätsregeln auf Datenbankebene

- `audit_logs`: Trigger verweigern UPDATE, DELETE und TRUNCATE. Der Laufzeitrolle sind diese Rechte zusätzlich entzogen. Ein Superuser oder der Eigentümer kann Trigger abschalten. Die Hash-Kette ist ohne externen Prüfanker daher manipulationserkennend, aber nicht fälschungssicher.
- `appointments`: `appointments_no_overlap` (GiST-Exclusion) verhindert überlappende aktive Termine je Arzt, auch bei parallelen Buchungen. `appointments_time_order` erzwingt `start_time < end_time`.
- `users.role`: nur Werte der serverseitigen Rollenliste.
- `anonymization_keys.encrypted_key` ist genau bei Pseudonymisierung gesetzt.

## Audit-Hash-Kette

`hash = SHA-256("kis-audit-v1" ‖ len(f₁) ‖ f₁ ‖ … ‖ len(f₈) ‖ f₈)` über `prev_hash`, `user_id`, `action`, Zeitstempel (RFC 3339, Mikrosekunden), `entity_type`, `entity_id`, `old_data`, `new_data`. Die Längenpräfixe verhindern Kollisionen durch Trennzeichen in Feldern. Eine transaktionsgebundene Advisory-Lock serialisiert alle Schreiber; der Eintrag wird in derselben Transaktion wie die fachliche Änderung geschrieben. Prüfen: `kis-admin verify-audit`.

Personendaten werden nicht in den Audit-Trail geschrieben (nur Aktion, Entität, geänderte Feldnamen, Export-Prüfsumme). Der Trail muss eine Löschung nach Art. 17 DSGVO überdauern.

## Sitzungen

`sessions` speichert pro Anmeldung das JTI des Refresh-Tokens (`jti`) und des aktuellen Access-Tokens (`access_jti`), `last_seen_at` und `revoked_at`. Jede Anfrage prüft Sitzung, Ablauf, Inaktivität (30 min) und Kontostatus auf dem Write-Pool.

## Schema (aus der migrierten Datenbank erzeugt)

### anonymization_keys

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| patient_id | uuid | nein |  |
| kind | text | nein |  |
| pseudonym | text | ja |  |
| encrypted_key | bytea | ja |  |
| created_by | text | nein |  |
| created_at | timestamp with time zone | nein | `now()` |

### appointments

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| patient_id | uuid | nein |  |
| doctor_id | uuid | nein |  |
| category | text | nein | `'ordination'::text` |
| status | text | nein | `'geplant'::text` |
| start_time | timestamp with time zone | nein |  |
| end_time | timestamp with time zone | nein |  |
| reason | text | ja |  |
| cancelled_at | timestamp with time zone | ja |  |
| created_at | timestamp with time zone | nein | `now()` |
| updated_at | timestamp with time zone | nein | `now()` |
| deleted_at | timestamp with time zone | ja |  |
| version | integer | nein | `1` |

### audit_logs

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | bigint | nein | `nextval('audit_logs_id_seq'::regclass)` |
| user_id | text | ja |  |
| action | text | nein |  |
| entity_type | text | nein |  |
| entity_id | text | ja |  |
| old_data | jsonb | ja |  |
| new_data | jsonb | ja |  |
| ip_address | text | ja |  |
| timestamp | timestamp with time zone | nein | `now()` |
| prev_hash | text | nein | `'0000000000000000000000000000000000000000000000000000000000000000'::text` |
| hash | text | nein |  |

### consents

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| patient_id | uuid | ja |  |
| type | text | nein |  |
| granted | boolean | nein |  |
| granted_at | timestamp with time zone | nein |  |
| granted_by | uuid | ja |  |
| withdrawn_at | timestamp with time zone | ja |  |
| withdrawn_by | uuid | ja |  |
| legal_basis | text | ja |  |
| created_at | timestamp with time zone | nein | `now()` |
| updated_at | timestamp with time zone | nein | `now()` |

### doctors

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| external_id | text | ja |  |
| name_family | text | nein |  |
| name_given | text | nein |  |
| specialty | text | ja |  |
| specialties | jsonb | nein | `'[]'::jsonb` |
| created_at | timestamp with time zone | nein | `now()` |
| updated_at | timestamp with time zone | nein | `now()` |
| deleted_at | timestamp with time zone | ja |  |
| version | integer | nein | `1` |

### medications

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| patient_id | uuid | nein |  |
| name | text | nein |  |
| dosage | text | ja |  |
| frequency | text | ja |  |
| start_date | date | nein |  |
| end_date | date | ja |  |
| created_at | timestamp with time zone | nein | `now()` |
| updated_at | timestamp with time zone | nein | `now()` |
| deleted_at | timestamp with time zone | ja |  |
| version | integer | nein | `1` |

### patients

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| external_id | text | ja |  |
| name_family | text | nein |  |
| name_given | text | nein |  |
| birth_date | date | nein |  |
| gender | text | ja |  |
| phone | text | ja |  |
| email | text | ja |  |
| address_street | text | ja |  |
| address_zip | text | ja |  |
| address_city | text | ja |  |
| insurance_number | text | ja |  |
| allergies | jsonb | nein | `'[]'::jsonb` |
| created_at | timestamp with time zone | nein | `now()` |
| updated_at | timestamp with time zone | nein | `now()` |
| deleted_at | timestamp with time zone | ja |  |
| version | integer | nein | `1` |
| insurance_number_enc | text | ja |  |
| phone_enc | text | ja |  |
| email_enc | text | ja |  |

### sessions

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| user_id | uuid | nein |  |
| jti | text | nein |  |
| issued_at | timestamp with time zone | nein |  |
| expires_at | timestamp with time zone | nein |  |
| revoked_at | timestamp with time zone | ja |  |
| ip_address | text | ja |  |
| access_jti | text | ja |  |
| last_seen_at | timestamp with time zone | ja |  |

### users

| Spalte | Typ | NULL | Default |
|---|---|---|---|
| id | uuid | nein | `gen_random_uuid()` |
| username | text | nein |  |
| password_hash | text | nein |  |
| display_name | text | nein |  |
| email | text | nein |  |
| role | text | nein | `'rezeption'::text` |
| is_active | boolean | nein | `true` |
| mfa_secret | text | ja |  |
| mfa_enabled | boolean | nein | `false` |
| last_login_at | timestamp with time zone | ja |  |
| login_attempts | integer | nein | `0` |
| locked_until | timestamp with time zone | ja |  |
| created_at | timestamp with time zone | nein | `now()` |
| updated_at | timestamp with time zone | nein | `now()` |
| deleted_at | timestamp with time zone | ja |  |
| version | integer | nein | `1` |
