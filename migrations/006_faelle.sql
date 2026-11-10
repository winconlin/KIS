-- ============================================================
-- KIS — Migration 006: Fälle, Aufenthalte, Warnhinweise, Zugriffskontext
--
-- Kern-Tabellen (gemeinsamer Vertrag aller Module):
--   * cases                    Behandlungsfall (Aufnahme … Entlassung)
--   * case_stays               Aufenthaltsabschnitte je Fachabteilung /
--                              Station / Bett (lückenlose Historie)
--   * case_participants        Mitbehandelnde Einheiten
--   * patient_alerts           Allergien, CAVE, Isolation
--   * emergency_access_grants  Notfallzugriff außerhalb des
--                              Behandlungszusammenhangs (Break-Glass)
--
-- Integrität liegt in der Datenbank, nicht nur im Anwendungscode:
--   * ein Fall ist zu jedem Zeitpunkt an genau einem Ort
--   * ein Bett trägt zu jedem Zeitpunkt höchstens einen Fall
--   * ein Patient hat höchstens einen offenen (teil-)stationären Fall
--   * Fachabteilung und Station sind Einheiten der passenden Art
-- ============================================================

CREATE SEQUENCE case_number_seq;

CREATE TABLE cases (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Fallnummer: Jahr + laufende Nummer, z. B. 2026-0000042
    case_number       TEXT NOT NULL UNIQUE
                      DEFAULT (to_char(now(), 'YYYY') || '-' || lpad(nextval('case_number_seq')::text, 7, '0')),
    patient_id        UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL
                      CHECK (kind IN ('stationaer', 'teilstationaer', 'vorstationaer',
                                      'nachstationaer', 'ambulant', 'notfall')),
    status            TEXT NOT NULL DEFAULT 'aufgenommen'
                      CHECK (status IN ('aufgenommen', 'entlassen', 'storniert')),
    admission_at      TIMESTAMPTZ NOT NULL,
    admission_type    TEXT NOT NULL DEFAULT 'einweisung'
                      CHECK (admission_type IN ('einweisung', 'notfall', 'verlegung', 'geburt', 'sonstige')),
    admission_reason  TEXT,
    discharge_at      TIMESTAMPTZ,
    discharge_type    TEXT
                      CHECK (discharge_type IN ('regulaer', 'gegen_aerztlichen_rat', 'verlegung',
                                                'verstorben', 'sonstige')),
    -- Aktueller Ort (denormalisiert aus case_stays, in derselben Transaktion gepflegt)
    department_id     UUID NOT NULL,
    department_kind   TEXT NOT NULL DEFAULT 'fachabteilung' CHECK (department_kind = 'fachabteilung'),
    ward_id           UUID,
    ward_kind         TEXT NOT NULL DEFAULT 'station' CHECK (ward_kind = 'station'),
    bed_id            UUID REFERENCES beds(id),
    attending_user_id UUID REFERENCES users(id),
    -- Klinischer Kurzstatus für Stationsliste und Übergabe
    stability         TEXT CHECK (stability IN ('stabil', 'beobachtung', 'instabil')),
    fasting           BOOLEAN NOT NULL DEFAULT FALSE,
    dnr               BOOLEAN NOT NULL DEFAULT FALSE,
    dni               BOOLEAN NOT NULL DEFAULT FALSE,
    -- „Wenn–dann“-Anweisung an den Dienst
    contingency       TEXT,
    planned_discharge DATE,
    created_by        UUID NOT NULL REFERENCES users(id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    version           INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (department_id, department_kind) REFERENCES org_units(id, kind),
    FOREIGN KEY (ward_id, ward_kind) REFERENCES org_units(id, kind),
    CONSTRAINT cases_discharge_consistent
        CHECK ((status = 'entlassen') = (discharge_at IS NOT NULL)),
    CONSTRAINT cases_discharge_type_required
        CHECK (status <> 'entlassen' OR discharge_type IS NOT NULL),
    CONSTRAINT cases_discharge_after_admission
        CHECK (discharge_at IS NULL OR discharge_at >= admission_at),
    CONSTRAINT cases_bed_needs_ward CHECK (bed_id IS NULL OR ward_id IS NOT NULL)
);

CREATE INDEX idx_cases_patient ON cases(patient_id);
CREATE INDEX idx_cases_ward_open ON cases(ward_id) WHERE status = 'aufgenommen';
CREATE INDEX idx_cases_department_open ON cases(department_id) WHERE status = 'aufgenommen';
CREATE UNIQUE INDEX cases_one_open_inpatient ON cases(patient_id)
    WHERE status = 'aufgenommen' AND kind IN ('stationaer', 'teilstationaer');

-- ------------------------------------------------------------
-- Aufenthaltsabschnitte. ended_at leer = aktueller Abschnitt.
-- ------------------------------------------------------------
CREATE TABLE case_stays (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id         UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    department_id   UUID NOT NULL,
    department_kind TEXT NOT NULL DEFAULT 'fachabteilung' CHECK (department_kind = 'fachabteilung'),
    ward_id         UUID,
    ward_kind       TEXT NOT NULL DEFAULT 'station' CHECK (ward_kind = 'station'),
    bed_id          UUID REFERENCES beds(id),
    started_at      TIMESTAMPTZ NOT NULL,
    ended_at        TIMESTAMPTZ,
    reason          TEXT NOT NULL CHECK (reason IN ('aufnahme', 'verlegung')),
    created_by      UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (department_id, department_kind) REFERENCES org_units(id, kind),
    FOREIGN KEY (ward_id, ward_kind) REFERENCES org_units(id, kind),
    CONSTRAINT case_stays_order CHECK (ended_at IS NULL OR ended_at > started_at),
    CONSTRAINT case_stays_bed_needs_ward CHECK (bed_id IS NULL OR ward_id IS NOT NULL),
    CONSTRAINT case_stays_one_place
        EXCLUDE USING gist (case_id WITH =, tstzrange(started_at, ended_at) WITH &&),
    CONSTRAINT case_stays_bed_free
        EXCLUDE USING gist (bed_id WITH =, tstzrange(started_at, ended_at) WITH &&)
        WHERE (bed_id IS NOT NULL)
);
CREATE INDEX idx_case_stays_case ON case_stays(case_id, started_at);
CREATE INDEX idx_case_stays_department ON case_stays(department_id);
CREATE INDEX idx_case_stays_ward ON case_stays(ward_id);

-- ------------------------------------------------------------
-- Mitbehandlung: eine weitere Einheit ist am Fall beteiligt (Konsil,
-- Diagnostik, Therapie, Sozialdienst …) und erhält dadurch für die Dauer
-- der Beteiligung Zugriff. Wird von Modulen über die Kernfunktion
-- `treatment::add_participant` gepflegt, nie direkt.
-- ------------------------------------------------------------
CREATE TABLE case_participants (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id     UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    unit_id     UUID NOT NULL REFERENCES org_units(id),
    -- Herkunft, z. B. 'order:<uuid>'; eindeutig je Fall und Einheit
    source      TEXT NOT NULL CHECK (source ~ '^[a-z_]+:[0-9a-f-]{36}$'),
    valid_from  TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until TIMESTAMPTZ,
    UNIQUE (case_id, unit_id, source),
    CHECK (valid_until IS NULL OR valid_until >= valid_from)
);
CREATE INDEX idx_case_participants_unit ON case_participants(unit_id, case_id);

-- ------------------------------------------------------------
-- Patientenbezogene Warnhinweise (Allergien, CAVE, Isolation). Gehören
-- zum Kern, weil viele Bereiche sie brauchen (Pflege, Küche, Apotheke).
-- ------------------------------------------------------------
CREATE TABLE patient_alerts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    patient_id  UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL
                CHECK (kind IN ('allergie', 'nahrungsmittelallergie', 'cave', 'isolation',
                                'implantat', 'sonstiges')),
    text        TEXT NOT NULL CHECK (length(trim(text)) > 0),
    recorded_by UUID NOT NULL REFERENCES users(id),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- gesetzt = Hinweis nicht mehr gültig (Historie bleibt erhalten)
    ended_at    TIMESTAMPTZ,
    ended_by    UUID REFERENCES users(id),
    CHECK ((ended_at IS NULL) = (ended_by IS NULL))
);
CREATE INDEX idx_patient_alerts_active ON patient_alerts(patient_id) WHERE ended_at IS NULL;

-- ------------------------------------------------------------
-- Notfallzugriff: zeitlich begrenzt, mit Pflichtbegründung, jede
-- Gewährung wird auditiert und ist nachträglich zu prüfen.
-- ------------------------------------------------------------
CREATE TABLE emergency_access_grants (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    case_id     UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    reason      TEXT NOT NULL CHECK (length(trim(reason)) >= 10),
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    reviewed_by UUID REFERENCES users(id),
    reviewed_at TIMESTAMPTZ,
    review_note TEXT,
    CHECK (expires_at > granted_at),
    CHECK ((reviewed_by IS NULL) = (reviewed_at IS NULL))
);
CREATE INDEX idx_emergency_access_lookup ON emergency_access_grants(user_id, case_id, expires_at);
CREATE INDEX idx_emergency_access_unreviewed ON emergency_access_grants(granted_at)
    WHERE reviewed_at IS NULL;
