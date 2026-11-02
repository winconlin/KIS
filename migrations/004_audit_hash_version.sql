-- Bestehende Hashes behalten Version 1, neue Einträge verwenden längencodierte Felder.
ALTER TABLE audit_log ADD COLUMN hash_version INTEGER NOT NULL DEFAULT 1;
