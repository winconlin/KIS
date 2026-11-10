-- ============================================================
-- KIS — Migration 007: Klinische Akte (Modul `clinical`)
--
--   * case_diagnoses    Diagnosen je Fall (ICD-10-GM optional)
--   * case_procedures   Prozeduren je Fall (OPS optional)
--   * clinical_notes    Dokumentation; freigegebene Einträge unveränderlich
--   * case_tasks        Aufgaben mit Zuständigkeit und Frist
--
-- Kodes werden nur formal geprüft. Die amtlichen Kataloge (BfArM) sind
-- nicht Bestandteil des Repositorys und müssen lizenziert importiert werden.
-- ============================================================

CREATE TABLE case_diagnoses (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id     UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('aufnahme', 'haupt', 'neben', 'entlassung')),
    -- ICD-10-GM, z. B. I21.0 oder E11.90; Zusatzkennzeichen + * ! erlaubt
    icd_code    TEXT CHECK (icd_code ~ '^[A-Z][0-9]{2}(\.[0-9A-Z-]{1,2})?[+*!]?$'),
    text        TEXT NOT NULL CHECK (length(trim(text)) > 0),
    -- Diagnosesicherheit: A ausgeschlossen, G gesichert, V Verdacht, Z Zustand nach
    certainty   TEXT CHECK (certainty IN ('A', 'G', 'V', 'Z')),
    laterality  TEXT CHECK (laterality IN ('R', 'L', 'B')),
    recorded_by UUID NOT NULL REFERENCES users(id),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_at  TIMESTAMPTZ,
    removed_by  UUID REFERENCES users(id),
    version     INTEGER NOT NULL DEFAULT 1,
    CHECK ((removed_at IS NULL) = (removed_by IS NULL))
);
CREATE INDEX idx_case_diagnoses_case ON case_diagnoses(case_id) WHERE removed_at IS NULL;
-- Genau eine Hauptdiagnose je Fall
CREATE UNIQUE INDEX case_diagnoses_one_main ON case_diagnoses(case_id)
    WHERE kind = 'haupt' AND removed_at IS NULL;

CREATE TABLE case_procedures (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id      UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    -- OPS, z. B. 1-275.0 oder 8-837.00
    ops_code     TEXT CHECK (ops_code ~ '^[1-9]-[0-9]{2}[0-9a-z]?(\.[0-9a-z]{1,2})?$'),
    text         TEXT NOT NULL CHECK (length(trim(text)) > 0),
    performed_at TIMESTAMPTZ NOT NULL,
    laterality   TEXT CHECK (laterality IN ('R', 'L', 'B')),
    recorded_by  UUID NOT NULL REFERENCES users(id),
    recorded_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    removed_at   TIMESTAMPTZ,
    removed_by   UUID REFERENCES users(id),
    CHECK ((removed_at IS NULL) = (removed_by IS NULL))
);
CREATE INDEX idx_case_procedures_case ON case_procedures(case_id) WHERE removed_at IS NULL;

-- ------------------------------------------------------------
-- Dokumentation. Lebenszyklus: entwurf → freigegeben → ersetzt.
-- Ein freigegebener Eintrag wird nie geändert; eine Korrektur ist ein
-- neuer Eintrag mit supersedes_id.
-- ------------------------------------------------------------
CREATE TABLE clinical_notes (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id       UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL
                  CHECK (kind IN ('anamnese', 'befund', 'verlauf', 'visite', 'pflege', 'konsil',
                                  'aufklaerung', 'epikrise', 'arztbrief', 'sonstiges')),
    title         TEXT,
    body          TEXT NOT NULL CHECK (length(trim(body)) > 0),
    status        TEXT NOT NULL DEFAULT 'entwurf' CHECK (status IN ('entwurf', 'freigegeben', 'ersetzt')),
    author_id     UUID NOT NULL REFERENCES users(id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    signed_by     UUID REFERENCES users(id),
    signed_at     TIMESTAMPTZ,
    supersedes_id UUID REFERENCES clinical_notes(id),
    version       INTEGER NOT NULL DEFAULT 1,
    CHECK ((status = 'entwurf') = (signed_at IS NULL)),
    CHECK ((signed_at IS NULL) = (signed_by IS NULL))
);
CREATE INDEX idx_clinical_notes_case ON clinical_notes(case_id, created_at);

CREATE OR REPLACE FUNCTION kis_clinical_notes_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        -- Kaskaden (Löschung des Falls nach Art. 17 DSGVO) laufen aus einem
        -- RI-Trigger heraus (Tiefe > 1) und bleiben möglich; ein direktes
        -- DELETE ist nur für Entwürfe erlaubt.
        IF OLD.status <> 'entwurf' AND pg_trigger_depth() = 1 THEN
            RAISE EXCEPTION 'Freigegebene Dokumentation kann nicht gelöscht werden'
                USING ERRCODE = 'insufficient_privilege';
        END IF;
        RETURN OLD;
    END IF;
    IF OLD.status <> 'entwurf' THEN
        IF NEW.body IS DISTINCT FROM OLD.body
           OR NEW.title IS DISTINCT FROM OLD.title
           OR NEW.kind IS DISTINCT FROM OLD.kind
           OR NEW.case_id IS DISTINCT FROM OLD.case_id
           OR NEW.author_id IS DISTINCT FROM OLD.author_id
           OR NEW.signed_by IS DISTINCT FROM OLD.signed_by
           OR NEW.signed_at IS DISTINCT FROM OLD.signed_at
           OR NEW.supersedes_id IS DISTINCT FROM OLD.supersedes_id THEN
            RAISE EXCEPTION 'Freigegebene Dokumentation ist unveränderlich'
                USING ERRCODE = 'insufficient_privilege';
        END IF;
        IF NEW.status IS DISTINCT FROM OLD.status
           AND NOT (OLD.status = 'freigegeben' AND NEW.status = 'ersetzt') THEN
            RAISE EXCEPTION 'Unzulässiger Statuswechsel % → %', OLD.status, NEW.status
                USING ERRCODE = 'insufficient_privilege';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER clinical_notes_guard
    BEFORE UPDATE OR DELETE ON clinical_notes
    FOR EACH ROW EXECUTE FUNCTION kis_clinical_notes_guard();

CREATE TABLE case_tasks (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id          UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    text             TEXT NOT NULL CHECK (length(trim(text)) > 0),
    -- Zuständigkeit: Benutzer und/oder freies Kürzel (z. B. Dienstarzt)
    assignee_user_id UUID REFERENCES users(id),
    assignee_label   TEXT,
    due_date         DATE,
    done_at          TIMESTAMPTZ,
    done_by          UUID REFERENCES users(id),
    created_by       UUID NOT NULL REFERENCES users(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    version          INTEGER NOT NULL DEFAULT 1,
    CHECK ((done_at IS NULL) = (done_by IS NULL))
);
CREATE INDEX idx_case_tasks_open ON case_tasks(case_id) WHERE done_at IS NULL;
