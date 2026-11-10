-- ============================================================
-- KIS — Migration 005: Plattformkern
--
-- Gemeinsame Grundlage für alle Berufsgruppen und Module:
--   * roles / role_permissions / user_roles   Rollen und Rechte als Daten
--   * settings / platform_state               Laufzeit-Einstellungen
--   * org_units                               Organisationsbaum (Träger,
--                                             Einrichtung, Fachabteilung,
--                                             Station, Funktions-,
--                                             Verwaltungs- und
--                                             Versorgungsbereiche)
--   * rooms / beds                            Zimmer und Betten einer Station
--   * user_org_units                          Zuordnung Benutzer ↔ Einheit
--   * domain_events / event_consumers /       Transaktionale Outbox und
--     event_failures                          fehlgeschlagene Folgeaktionen
--
-- Regel für alle Folgemigrationen: angewendete Dateien nie ändern,
-- sondern eine neue nummerierte Datei anlegen (migrations/CHECKSUMS).
-- ============================================================

-- ------------------------------------------------------------
-- Rollen als Daten. Die Rechteschlüssel (z. B. 'case.read') definiert der
-- Code (kis-core, rbac.rs); welche Rolle welche Rechte hat, ist
-- Konfiguration des Hauses.
--   is_system  eingebaute Rolle, kann nicht gelöscht werden
--   seeded     Standardrechte wurden bereits eingespielt (danach gehören
--              die Rechte dem Haus und werden nie überschrieben)
-- Die Rolle 'admin' hat immer alle Rechte (im Code verankert), damit sich
-- niemand aussperren kann.
-- ------------------------------------------------------------
CREATE TABLE roles (
    key         TEXT PRIMARY KEY CHECK (key ~ '^[a-z][a-z0-9_]{1,31}$'),
    label       TEXT NOT NULL CHECK (length(trim(label)) > 0),
    description TEXT,
    is_system   BOOLEAN NOT NULL DEFAULT FALSE,
    seeded      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE role_permissions (
    role_key   TEXT NOT NULL REFERENCES roles(key) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission ~ '^[a-z_]+(\.[a-z_]+)?$'),
    PRIMARY KEY (role_key, permission)
);

-- Bereits vergebene Rollen übernehmen, dann die bisherige feste Liste
-- durch den Fremdschlüssel ersetzen.
INSERT INTO roles (key, label, is_system) VALUES
    ('admin',     'Administration', TRUE),
    ('arzt',      'Ärztlicher Dienst', TRUE),
    ('pflege',    'Pflegedienst', TRUE),
    ('rezeption', 'Patientenservice / Aufnahme', TRUE),
    ('viewer',    'Lesezugriff Stammdaten', TRUE),
    ('auditor',   'Revision / Datenschutz', TRUE),
    ('llm_agent', 'Assistenzsystem (nur lesend)', TRUE);

ALTER TABLE users DROP CONSTRAINT users_role_check;
ALTER TABLE users ADD CONSTRAINT users_role_fk FOREIGN KEY (role) REFERENCES roles(key);

-- Zusatzrollen: wirksame Rechte = Vereinigung aus users.role und user_roles.
CREATE TABLE user_roles (
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_key   TEXT NOT NULL REFERENCES roles(key) ON DELETE CASCADE,
    granted_by UUID NOT NULL REFERENCES users(id),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role_key)
);

-- ------------------------------------------------------------
-- Laufzeit-Einstellungen (Richtlinien des Hauses), änderbar ohne
-- Neustart. Gültige Schlüssel und Standardwerte definiert der Code
-- (kis-core, platform.rs). Infrastruktur (Datenbank, Ports, Module)
-- bleibt in der Konfigurationsdatei.
-- ------------------------------------------------------------
CREATE TABLE settings (
    key        TEXT PRIMARY KEY CHECK (key ~ '^[a-z_]+(\.[a-z_]+)+$'),
    value      JSONB NOT NULL,
    updated_by UUID NOT NULL REFERENCES users(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Versionszähler für Rollen/Rechte und Einstellungen. Jede Anfrage liest
-- sie zusammen mit der Sitzung; nur bei Änderung wird der Speicher-Cache
-- der Instanz neu geladen. So wirken Änderungen sofort auf allen
-- Instanzen, ohne zusätzliche Abfragen im Normalbetrieb.
CREATE TABLE platform_state (
    id               BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    rbac_version     BIGINT NOT NULL DEFAULT 1,
    settings_version BIGINT NOT NULL DEFAULT 1
);
INSERT INTO platform_state DEFAULT VALUES;

-- ------------------------------------------------------------
-- Organisationsbaum. Eine Tabelle für alle Bereiche des Hauses, damit
-- Zuordnung, Arbeitslisten, Aufträge und Dienstpläne überall gleich
-- funktionieren.
-- ------------------------------------------------------------
CREATE TABLE org_units (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_id     UUID REFERENCES org_units(id),
    kind          TEXT NOT NULL
                  CHECK (kind IN ('traeger', 'einrichtung', 'fachabteilung', 'station',
                                  'funktionsbereich', 'ambulanz', 'verwaltung', 'versorgung',
                                  'sonstige')),
    code          TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Za-z0-9_.-]{1,32}$'),
    name          TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Einrichtung: Institutionskennzeichen (9 Ziffern)
    ik_number     TEXT CHECK (ik_number ~ '^[0-9]{9}$'),
    -- Fachabteilung: Schlüssel nach § 301 SGB V (4 Ziffern)
    specialty_key TEXT CHECK (specialty_key ~ '^[0-9]{4}$'),
    -- Station: Versorgungsstufe (normal, imc, intensiv, notaufnahme, tagesklinik)
    care_level    TEXT CHECK (care_level IN ('normal', 'imc', 'intensiv', 'notaufnahme', 'tagesklinik')),
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    version       INTEGER NOT NULL DEFAULT 1,
    CHECK (parent_id IS DISTINCT FROM id),
    CHECK (ik_number IS NULL OR kind = 'einrichtung'),
    CHECK (specialty_key IS NULL OR kind = 'fachabteilung'),
    CHECK (care_level IS NULL OR kind = 'station'),
    -- Ziel für zusammengesetzte Fremdschlüssel, die die Art festlegen
    UNIQUE (id, kind)
);
CREATE INDEX idx_org_units_parent ON org_units(parent_id);
CREATE INDEX idx_org_units_kind ON org_units(kind) WHERE active;

CREATE TABLE rooms (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ward_id    UUID NOT NULL,
    ward_kind  TEXT NOT NULL DEFAULT 'station' CHECK (ward_kind = 'station'),
    name       TEXT NOT NULL CHECK (length(trim(name)) > 0),
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (ward_id, name),
    FOREIGN KEY (ward_id, ward_kind) REFERENCES org_units(id, kind)
);

CREATE TABLE beds (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    room_id        UUID NOT NULL REFERENCES rooms(id),
    label          TEXT NOT NULL CHECK (length(trim(label)) > 0),
    -- gesetzt = Bett gesperrt (Aufbereitung, Defekt, Isolation …)
    blocked_reason TEXT CHECK (length(trim(blocked_reason)) > 0),
    active         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    version        INTEGER NOT NULL DEFAULT 1,
    UNIQUE (room_id, label)
);

-- ------------------------------------------------------------
-- Zuordnung Benutzer ↔ Organisationseinheit. Gilt auch für alle
-- untergeordneten Einheiten. Grundlage für Behandlungszusammenhang und
-- Arbeitslisten.
-- ------------------------------------------------------------
CREATE TABLE user_org_units (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    unit_id     UUID NOT NULL REFERENCES org_units(id),
    valid_from  TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ,
    assigned_by UUID NOT NULL REFERENCES users(id),
    CHECK (valid_until IS NULL OR valid_until > valid_from)
);
CREATE INDEX idx_user_org_units_user ON user_org_units(user_id);
CREATE UNIQUE INDEX idx_user_org_units_open ON user_org_units(user_id, unit_id)
    WHERE valid_until IS NULL;

-- ------------------------------------------------------------
-- Ereignis-Outbox. Fachmodule schreiben Ereignisse in derselben
-- Transaktion wie die fachliche Änderung; Schnittstellen (HL7, FHIR,
-- Kommunikationsserver) lesen sie über einen Cursor. Die Nutzlast enthält
-- nur Schlüssel und Codes, keine Personendaten.
-- ------------------------------------------------------------
CREATE TABLE domain_events (
    id          BIGSERIAL PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    topic       TEXT NOT NULL CHECK (topic ~ '^[a-z_]+\.[a-z_]+$'),
    entity_type TEXT NOT NULL,
    entity_id   UUID NOT NULL,
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX idx_domain_events_topic ON domain_events(topic, id);

CREATE TABLE event_consumers (
    name          TEXT PRIMARY KEY CHECK (name ~ '^[a-z0-9_-]{1,64}$'),
    last_event_id BIGINT NOT NULL DEFAULT 0,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Folgeaktionen anderer Module auf ein Ereignis laufen isoliert
-- (Savepoint). Schlägt eine fehl, läuft der auslösende Vorgang weiter;
-- der Fehler wird hier festgehalten und ist für die Administration
-- sichtbar.
CREATE TABLE event_failures (
    id          BIGSERIAL PRIMARY KEY,
    event_id    BIGINT NOT NULL REFERENCES domain_events(id) ON DELETE CASCADE,
    subscriber  TEXT NOT NULL,
    error       TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    resolved_by UUID REFERENCES users(id),
    CHECK ((resolved_at IS NULL) = (resolved_by IS NULL))
);
CREATE INDEX idx_event_failures_open ON event_failures(occurred_at) WHERE resolved_at IS NULL;

-- Generischer Schutz gegen nachträgliche Änderung (Tabellenname aus dem
-- Trigger-Kontext).
CREATE OR REPLACE FUNCTION kis_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% ist append-only (% verweigert)', TG_TABLE_NAME, TG_OP
        USING ERRCODE = 'insufficient_privilege';
END;
$$;

CREATE TRIGGER domain_events_no_update
    BEFORE UPDATE ON domain_events
    FOR EACH ROW EXECUTE FUNCTION kis_append_only();
