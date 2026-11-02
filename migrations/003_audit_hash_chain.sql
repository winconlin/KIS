-- ============================================================
-- KIS — Datenbank-Migration 003: Audit-Hash-Chain (Task 1.12)
--
-- DSGVO-konforme Audit-Trail mit kryptographischer Hash-Chain:
--   current_hash = SHA-256(prev_hash || user_id || action || timestamp
--                          || entity_type || entity_id
--                          || old_data || new_data)
--
-- Der erste Eintrag (Genesis) verwendet prev_hash = 64 Null-Zeichen.
-- Jede nachträgliche Manipulation eines Eintrags bricht die Chain —
-- sie wird von verify_chain() erkannt (Compliance-Prüfung).
--
-- Die Tabelle wird aus der Legacy-Tabelle `audit_log` (Migration 002)
-- migriert und anschließend ersetzt. Bestehende Zeilen erhalten die
-- Genesis-Chain, damit die Verifikation direkt nach dem Upgrade läuft.
-- ============================================================

CREATE TABLE IF NOT EXISTS audit_logs (
    id              BIGSERIAL PRIMARY KEY,
    user_id         TEXT,                       -- UUID oder "system" (ohne Auth)
    action          TEXT NOT NULL,              -- create / update / delete / login ...
    entity_type     TEXT NOT NULL,              -- patient / doctor / medication ...
    entity_id       TEXT,                       -- ID der betroffenen Entität
    old_data        JSONB,                      -- Zustand vor der Änderung
    new_data        JSONB,                      -- Zustand nach der Änderung
    ip_address      TEXT,                       -- Herkunft-IP (DSGVO Art. 30)
    timestamp       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Hash-Chain:
    prev_hash       TEXT NOT NULL DEFAULT '0000000000000000000000000000000000000000000000000000000000000000',
    hash            TEXT NOT NULL
);

-- Audit-Trail-Abfragen: nach Entität, nach User, nach Zeit
CREATE INDEX IF NOT EXISTS idx_audit_logs_entity ON audit_logs(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_user   ON audit_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_time   ON audit_logs(timestamp);

-- ============================================================
-- Datenmigration: Legacy-Tabelle audit_log → audit_logs
-- (bestehende Zeilen erhalten die Genesis-Chain, damit
-- verify_chain() direkt nach dem Upgrade grün ist)
--
-- WICHTIG: Der Hash-String muss exakt zur Rust-Formel in
-- security.rs (log_audit/verify_audit_chain) passen:
--   SHA-256(prev_hash || '|' || user_id || '|' || action
--            || '|' || timestamp || '|' || entity_type
--            || '|' || entity_id || '|' || old_data
--            || '|' || new_data)
-- Timestamp: RFC3339-Nanos (ISO 8601), JSONB: canonicalisiertes JSON.
-- ============================================================
INSERT INTO audit_logs (
    user_id, action, entity_type, entity_id,
    old_data, new_data, ip_address, timestamp,
    prev_hash, hash
)
SELECT
    COALESCE(username, 'system'),
    action,
    entity_type,
    entity_id,
    old_value,
    new_value,
    ip_address,
    timestamp,
    '0000000000000000000000000000000000000000000000000000000000000000',
    encode(digest(
        '0000000000000000000000000000000000000000000000000000000000000000' || '|' ||
        COALESCE(username, 'system') || '|' ||
        action || '|' ||
        to_char(timestamp AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') || '|' ||
        entity_type || '|' ||
        COALESCE(entity_id, '') || '|' ||
        COALESCE(to_jsonb(old_value)::text, 'null') || '|' ||
        COALESCE(to_jsonb(new_value)::text, 'null'),
        'sha256'
    ), 'hex')
FROM audit_log
ORDER BY id;

-- Legacy-Tabelle entfernen: audit_logs ist die einzige Audit-Quelle.
-- (Aufbewahrung 10 Jahre gilt für audit_logs — DSGVO, keine Löschung.)
DROP TABLE IF EXISTS audit_log;
