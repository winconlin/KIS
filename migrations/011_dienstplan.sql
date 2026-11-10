-- ============================================================
-- KIS — Migration 011: Dienstplan und Bereichsbesetzung
--
-- Für alle Berufsgruppen und Einheiten des Organisationsbaums.
--
--   * staff_members         Personalstamm der Dienstplanung
--   * duty_types            Dienstarten einer Einheit (24-h-Dienst,
--                           Hintergrund, Visite, Früh/Spät/Nacht …)
--   * duty_assignments      Einteilung: eine Person je Dienstart und Tag
--   * absences              Urlaub, Zeitausgleich, Wunschfrei, Sperre …
--   * public_holidays       Feiertage (vom Haus gepflegt)
--   * staffing_positions    Positionen der Bereichsbesetzung
--   * staffing_assignments  Wochenweise Besetzung einer Position
-- ============================================================

CREATE TABLE staff_members (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id          UUID UNIQUE REFERENCES users(id) ON DELETE SET NULL,
    display_name     TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    -- Kennung im Zeitwirtschaftssystem (z. B. ATOSS), für den Export
    personnel_number TEXT UNIQUE CHECK (personnel_number = upper(trim(personnel_number))
                                        AND length(personnel_number) > 0),
    -- Hauseigene Qualifikationsstufe, z. B. AA, FA, OA, PFK
    qualification    TEXT NOT NULL CHECK (qualification ~ '^[A-Z0-9_-]{1,16}$'),
    unit_id          UUID REFERENCES org_units(id),
    work_percent     INTEGER NOT NULL DEFAULT 100 CHECK (work_percent BETWEEN 1 AND 100),
    active           BOOLEAN NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    version          INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_staff_unit ON staff_members(unit_id) WHERE active;

CREATE TABLE duty_types (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    unit_id                UUID NOT NULL REFERENCES org_units(id),
    code                   TEXT NOT NULL CHECK (code ~ '^[A-Z0-9_-]{1,16}$'),
    name                   TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Zulässige Qualifikationen; leer = jede
    qualifications         TEXT[] NOT NULL DEFAULT '{}',
    -- Wochentage nach ISO (1 = Montag … 7 = Sonntag), an denen der Dienst anfällt
    weekdays               SMALLINT[] NOT NULL DEFAULT '{1,2,3,4,5,6,7}'
                           CHECK (weekdays <@ ARRAY[1,2,3,4,5,6,7]::smallint[] AND cardinality(weekdays) > 0),
    -- Feiertage zählen nicht als Wochentag: an einem Feiertag fällt der Dienst
    -- genau dann an, wenn on_holidays gesetzt ist.
    on_holidays            BOOLEAN NOT NULL DEFAULT TRUE,
    -- Muss der Dienst an jedem vorgesehenen Tag besetzt sein?
    required               BOOLEAN NOT NULL DEFAULT TRUE,
    -- Ruhezeit: wer diesen Dienst leistet, darf am Folgetag keinen
    -- weiteren Dienst mit dieser Eigenschaft leisten
    blocks_next_day        BOOLEAN NOT NULL DEFAULT FALSE,
    start_time             TIME NOT NULL DEFAULT '08:00',
    duration_hours         NUMERIC(4, 1) NOT NULL CHECK (duration_hours BETWEEN 0.5 AND 48),
    -- Abweichende Zeiten an Samstag, Sonntag und Feiertagen (optional)
    weekend_start_time     TIME,
    weekend_duration_hours NUMERIC(4, 1) CHECK (weekend_duration_hours BETWEEN 0.5 AND 48),
    -- Anrechenbare Stunden und Lohnart für den Zeitwirtschafts-Export
    credited_hours         NUMERIC(4, 1) NOT NULL CHECK (credited_hours BETWEEN 0 AND 48),
    weekend_credited_hours NUMERIC(4, 1) CHECK (weekend_credited_hours BETWEEN 0 AND 48),
    wage_type              TEXT NOT NULL DEFAULT 'BD' CHECK (wage_type ~ '^[A-Z0-9_-]{1,16}$'),
    active                 BOOLEAN NOT NULL DEFAULT TRUE,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    version                INTEGER NOT NULL DEFAULT 1,
    UNIQUE (unit_id, code),
    CHECK ((weekend_start_time IS NULL) = (weekend_duration_hours IS NULL))
);

CREATE TABLE duty_assignments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    duty_type_id UUID NOT NULL REFERENCES duty_types(id),
    duty_date    DATE NOT NULL,
    staff_id     UUID NOT NULL REFERENCES staff_members(id),
    note         TEXT,
    assigned_by  UUID NOT NULL REFERENCES users(id),
    assigned_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (duty_type_id, duty_date)
);
CREATE INDEX idx_duty_assignments_staff ON duty_assignments(staff_id, duty_date);
CREATE INDEX idx_duty_assignments_date ON duty_assignments(duty_date);

CREATE TABLE absences (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    staff_id    UUID NOT NULL REFERENCES staff_members(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL
                CHECK (kind IN ('urlaub', 'zeitausgleich', 'krank', 'fortbildung', 'wunschfrei', 'sperre')),
    from_date   DATE NOT NULL,
    to_date     DATE NOT NULL,
    note        TEXT,
    recorded_by UUID NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (to_date >= from_date)
);
CREATE INDEX idx_absences_staff ON absences(staff_id, from_date, to_date);

CREATE TABLE public_holidays (
    holiday_date DATE PRIMARY KEY,
    name         TEXT NOT NULL CHECK (length(trim(name)) > 0)
);

CREATE TABLE staffing_positions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    unit_id    UUID NOT NULL REFERENCES org_units(id),
    code       TEXT NOT NULL CHECK (code ~ '^[A-Za-z0-9_-]{1,32}$'),
    name       TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Gruppierung in der Besetzungsmatrix, z. B. „Station 7“, „HKL“, „Echo“
    category   TEXT NOT NULL CHECK (length(trim(category)) > 0),
    sort_order INTEGER NOT NULL DEFAULT 0,
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (unit_id, code)
);

CREATE TABLE staffing_assignments (
    position_id UUID NOT NULL REFERENCES staffing_positions(id) ON DELETE CASCADE,
    -- Montag der Kalenderwoche
    week_start  DATE NOT NULL CHECK (extract(isodow FROM week_start) = 1),
    staff_id    UUID NOT NULL REFERENCES staff_members(id),
    assigned_by UUID NOT NULL REFERENCES users(id),
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (position_id, week_start)
);
CREATE INDEX idx_staffing_assignments_staff ON staffing_assignments(staff_id, week_start);
