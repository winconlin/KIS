-- ============================================================
-- KIS — Migration 010: Verpflegung
--
--   * diets       Kostformen (vom Haus gepflegt)
--   * case_diets  Kostanordnung je Fall mit Historie
--
-- Die Produktionsliste der Küche wird nicht gespeichert, sondern aus
-- Belegung, Kostanordnung, Nüchtern-Status und Allergien berechnet.
-- ============================================================

CREATE TABLE diets (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code        TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Z0-9_-]{1,16}$'),
    name        TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT,
    -- Standardkost für Fälle ohne eigene Anordnung (höchstens eine)
    is_default  BOOLEAN NOT NULL DEFAULT FALSE,
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    version     INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX diets_one_default ON diets(is_default) WHERE is_default;

CREATE TABLE case_diets (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id     UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    diet_id     UUID NOT NULL REFERENCES diets(id),
    -- Hinweis an die Küche (Konsistenz, Abneigungen); keine Diagnosen
    note        TEXT,
    valid_from  TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ,
    ordered_by  UUID NOT NULL REFERENCES users(id),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    -- je Fall gilt zu jedem Zeitpunkt höchstens eine Kostanordnung
    CONSTRAINT case_diets_one_current
        EXCLUDE USING gist (case_id WITH =, tstzrange(valid_from, valid_until) WITH &&)
);
CREATE INDEX idx_case_diets_current ON case_diets(case_id) WHERE valid_until IS NULL;
