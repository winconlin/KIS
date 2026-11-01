-- 002_security.sql
-- DSGVO: Audit-Einträge mind. 10 Jahre (3650 Tage) aufbewahren, niemals löschen
-- Hash-Chain gegen Manipulation
-- Verschlüsselte Spalten für sensible Patientendaten (AES-256-GCM, Base64)

-- Verschlüsselte Spalten für sensible Patientendaten
ALTER TABLE patients ADD COLUMN IF NOT EXISTS insurance_no_enc TEXT;
ALTER TABLE patients ADD COLUMN IF NOT EXISTS notes_enc TEXT;
ALTER TABLE patients ADD COLUMN IF NOT EXISTS phone_enc TEXT;

-- ============================================================
-- AUDIT-LOG (Hash-Chain, 10 Jahre Aufbewahrung)
-- ============================================================
CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,
    user_id         TEXT,
    username        TEXT,
    action          TEXT NOT NULL,
    entity_type     TEXT NOT NULL,
    entity_id       TEXT,
    old_value       TEXT,
    new_value       TEXT,
    ip_address      TEXT,
    prev_hash       TEXT,
    current_hash    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_entity ON audit_log(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_time ON audit_log(timestamp);

-- ============================================================
-- SESSIONS (JWT-Blacklist)
-- ============================================================
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    user_id         TEXT REFERENCES users(id) ON DELETE CASCADE,
    jti             TEXT UNIQUE NOT NULL,
    issued_at       TEXT NOT NULL,
    expires_at      TEXT NOT NULL,
    revoked_at      TEXT,
    ip_address      TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

-- ============================================================
-- CONSENT & DATENSCHUTZ (DSGVO)
-- ============================================================
CREATE TABLE IF NOT EXISTS consents (
    id              TEXT PRIMARY KEY,
    patient_id      TEXT REFERENCES patients(id) ON DELETE CASCADE,
    type            TEXT NOT NULL,
    granted         INTEGER NOT NULL,
    granted_at      TEXT NOT NULL,
    granted_by      TEXT REFERENCES users(id),
    withdrawn_at    TEXT,
    withdrawn_by    TEXT REFERENCES users(id),
    legal_basis     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_consents_patient ON consents(patient_id);
CREATE INDEX IF NOT EXISTS idx_consents_type ON consents(type) WHERE withdrawn_at IS NULL;
