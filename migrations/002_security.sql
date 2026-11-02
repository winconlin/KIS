-- ============================================================
-- KIS — Datenbank-Migration 002: Sicherheit (PostgreSQL)
-- DSGVO: Audit-Einträge mind. 10 Jahre (3650 Tage) aufbewahren,
-- niemals löschen. Hash-Chain gegen Manipulation.
-- Verschlüsselte Spalten für sensible Patientendaten (AES-256-GCM, Base64)
-- ============================================================

-- Verschlüsselte Spalten für sensible Patientendaten
ALTER TABLE patients ADD COLUMN IF NOT EXISTS insurance_number_enc TEXT;
ALTER TABLE patients ADD COLUMN IF NOT EXISTS phone_enc TEXT;
ALTER TABLE patients ADD COLUMN IF NOT EXISTS email_enc TEXT;

-- ============================================================
-- AUDIT-LOG (Hash-Chain, 10 Jahre Aufbewahrung, DSGVO-konform)
-- ============================================================
CREATE TABLE IF NOT EXISTS audit_log (
    id              BIGSERIAL PRIMARY KEY,
    timestamp       TIMESTAMPTZ NOT NULL DEFAULT now(),
    user_id         UUID,
    username        TEXT,
    action          TEXT NOT NULL,
    entity_type     TEXT NOT NULL,
    entity_id       TEXT,
    old_value       JSONB,
    new_value       JSONB,
    ip_address      TEXT,
    prev_hash       TEXT,
    current_hash    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_entity ON audit_log(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_time ON audit_log(timestamp);

-- ============================================================
-- SESSIONS (JWT-Blacklist für Refresh-Tokens)
-- ============================================================
CREATE TABLE IF NOT EXISTS sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID REFERENCES users(id) ON DELETE CASCADE,
    jti             TEXT UNIQUE NOT NULL,
    issued_at       TIMESTAMPTZ NOT NULL,
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ,
    ip_address      TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

-- ============================================================
-- CONSENT & DATENSCHUTZ (DSGVO)
-- ============================================================
CREATE TABLE IF NOT EXISTS consents (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    patient_id      UUID REFERENCES patients(id) ON DELETE CASCADE,
    type            TEXT NOT NULL,
    granted         BOOLEAN NOT NULL,
    granted_at      TIMESTAMPTZ NOT NULL,
    granted_by      UUID REFERENCES users(id),
    withdrawn_at    TIMESTAMPTZ,
    withdrawn_by    UUID REFERENCES users(id),
    legal_basis     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_consents_patient ON consents(patient_id);
CREATE INDEX IF NOT EXISTS idx_consents_type ON consents(type) WHERE withdrawn_at IS NULL;
