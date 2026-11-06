-- ============================================================
-- KIS — Migration 004: Sitzungen, Pseudonymisierungs-Schlüssel,
-- Schreibschutz für den Audit-Trail (PostgreSQL)
--
-- Ersetzt die beiden aus dem SQLite-Prototyp übernommenen Dateien
-- 003_session_access_token.sql (doppelte Version 3) und
-- 004_audit_hash_version.sql (änderte die in 003 entfernte Tabelle
-- audit_log). Mit diesen Dateien konnte keine PostgreSQL-Datenbank
-- vollständig migriert werden; es gibt daher keinen Bestand, dessen
-- Prüfsummen hier gebrochen würden (nur frische Installationen, bestätigt
-- am 02.11.2026).
-- ============================================================

-- ------------------------------------------------------------
-- Sitzungen: Refresh-JTI (jti) und aktuelles Access-JTI gemeinsam,
-- damit Logout/Widerruf beide Tokens sofort ungültig macht.
-- ------------------------------------------------------------
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS access_jti TEXT;
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS last_seen_at TIMESTAMPTZ;
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_access_jti ON sessions(access_jti);
ALTER TABLE sessions ALTER COLUMN user_id SET NOT NULL;

-- ------------------------------------------------------------
-- Rollen nur aus der serverseitigen Allowlist
-- ------------------------------------------------------------
ALTER TABLE users ADD CONSTRAINT users_role_check
    CHECK (role IN ('admin', 'arzt', 'pflege', 'rezeption', 'viewer', 'auditor', 'llm_agent'));

-- ------------------------------------------------------------
-- Anonymisierungs-/Pseudonymisierungs-Nachweise
-- encrypted_key: AES-256-GCM (nonce || ciphertext), nur bei Pseudonymisierung
-- ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS anonymization_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    patient_id      UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('anonymized', 'pseudonymized')),
    pseudonym       TEXT,
    encrypted_key   BYTEA,
    created_by      TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((kind = 'pseudonymized') = (encrypted_key IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS idx_anonymization_keys_patient ON anonymization_keys(patient_id);

-- ------------------------------------------------------------
-- Audit-Trail: append-only auf Datenbankebene.
-- Schützt gegen UPDATE/DELETE/TRUNCATE durch die Anwendungsrolle.
-- Ein Superuser oder Tabelleneigentümer kann Trigger deaktivieren; die
-- Hash-Kette ist ohne externen Prüfanker daher NICHT fälschungssicher.
-- ------------------------------------------------------------
CREATE OR REPLACE FUNCTION kis_audit_logs_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_logs ist append-only (% verweigert)', TG_OP
        USING ERRCODE = 'insufficient_privilege';
END;
$$;

DROP TRIGGER IF EXISTS audit_logs_no_update_delete ON audit_logs;
CREATE TRIGGER audit_logs_no_update_delete
    BEFORE UPDATE OR DELETE ON audit_logs
    FOR EACH ROW EXECUTE FUNCTION kis_audit_logs_append_only();

DROP TRIGGER IF EXISTS audit_logs_no_truncate ON audit_logs;
CREATE TRIGGER audit_logs_no_truncate
    BEFORE TRUNCATE ON audit_logs
    FOR EACH STATEMENT EXECUTE FUNCTION kis_audit_logs_append_only();

-- ------------------------------------------------------------
-- Termine: Doppelbuchungen auf Datenbankebene ausschließen.
-- Die Vorabprüfung in der Anwendung ist nicht race-frei; diese
-- Constraint greift auch bei parallelen Buchungen.
-- ------------------------------------------------------------
CREATE EXTENSION IF NOT EXISTS btree_gist;
ALTER TABLE appointments ADD CONSTRAINT appointments_time_order
    CHECK (start_time < end_time);
ALTER TABLE appointments ADD CONSTRAINT appointments_no_overlap
    EXCLUDE USING gist (doctor_id WITH =, tstzrange(start_time, end_time) WITH &&)
    WHERE (status <> 'abgesagt' AND deleted_at IS NULL);

-- ------------------------------------------------------------
-- Rechte: Keine Rolle außer dem Eigentümer darf den Audit-Trail
-- ändern oder löschen (ergänzt die Trigger oben). Greift auch für
-- über ALTER DEFAULT PRIVILEGES vergebene Rechte der Laufzeitrolle,
-- siehe config/postgres/roles.sql.
-- ------------------------------------------------------------
REVOKE UPDATE, DELETE, TRUNCATE ON audit_logs FROM PUBLIC;
DO $$
DECLARE r record;
BEGIN
    FOR r IN
        SELECT DISTINCT grantee FROM information_schema.role_table_grants
        WHERE table_name = 'audit_logs'
          AND privilege_type IN ('UPDATE', 'DELETE', 'TRUNCATE')
          AND grantee <> (SELECT tableowner FROM pg_tables WHERE tablename = 'audit_logs' LIMIT 1)
          AND grantee <> 'PUBLIC'
    LOOP
        EXECUTE format('REVOKE UPDATE, DELETE, TRUNCATE ON audit_logs FROM %I', r.grantee);
    END LOOP;
END;
$$;
