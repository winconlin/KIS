-- ============================================================
-- KIS — Migration 009: Pflege (Modul `nursing`) und stationäre Verordnungen
-- (Modul `prescriptions`)
--
--   * vital_signs                 Vitalwerte
--   * fluid_balance_entries       Ein- und Ausfuhr
--   * prescriptions               Stationäre Verordnung je Fall
--   * medication_administrations  Dokumentierte Gaben
--
-- Die Tabelle `medications` (Migration 001) bleibt die patientenbezogene
-- Dauermedikation (Medikationsplan bei Aufnahme). Verordnungen während
-- des Aufenthalts stehen in `prescriptions`.
-- ============================================================

CREATE TABLE vital_signs (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id      UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    measured_at  TIMESTAMPTZ NOT NULL,
    systolic     INTEGER CHECK (systolic BETWEEN 20 AND 350),
    diastolic    INTEGER CHECK (diastolic BETWEEN 10 AND 250),
    heart_rate   INTEGER CHECK (heart_rate BETWEEN 0 AND 400),
    temperature  NUMERIC(4, 1) CHECK (temperature BETWEEN 20 AND 46),
    spo2         INTEGER CHECK (spo2 BETWEEN 0 AND 100),
    resp_rate    INTEGER CHECK (resp_rate BETWEEN 0 AND 120),
    weight_kg    NUMERIC(5, 1) CHECK (weight_kg BETWEEN 0.2 AND 500),
    pain_nrs     INTEGER CHECK (pain_nrs BETWEEN 0 AND 10),
    note         TEXT,
    recorded_by  UUID NOT NULL REFERENCES users(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Fehleintrag: nicht löschen, sondern als ungültig markieren
    voided_at    TIMESTAMPTZ,
    voided_by    UUID REFERENCES users(id),
    CONSTRAINT vital_signs_not_empty CHECK (
        num_nonnulls(systolic, diastolic, heart_rate, temperature, spo2, resp_rate, weight_kg, pain_nrs) > 0
    ),
    CONSTRAINT vital_signs_bp_pair CHECK ((systolic IS NULL) = (diastolic IS NULL)),
    CONSTRAINT vital_signs_bp_order CHECK (systolic IS NULL OR systolic > diastolic),
    CHECK ((voided_at IS NULL) = (voided_by IS NULL))
);
CREATE INDEX idx_vital_signs_case ON vital_signs(case_id, measured_at) WHERE voided_at IS NULL;

CREATE TABLE fluid_balance_entries (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id     UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    recorded_at TIMESTAMPTZ NOT NULL,
    direction   TEXT NOT NULL CHECK (direction IN ('einfuhr', 'ausfuhr')),
    -- z. B. oral, infusion, urin, drainage
    category    TEXT NOT NULL CHECK (length(trim(category)) > 0),
    volume_ml   INTEGER NOT NULL CHECK (volume_ml BETWEEN 1 AND 20000),
    recorded_by UUID NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    voided_at   TIMESTAMPTZ,
    voided_by   UUID REFERENCES users(id),
    CHECK ((voided_at IS NULL) = (voided_by IS NULL))
);
CREATE INDEX idx_fluid_balance_case ON fluid_balance_entries(case_id, recorded_at) WHERE voided_at IS NULL;

CREATE TABLE prescriptions (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id          UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    -- Präparat oder Wirkstoff, wie verordnet
    drug_name        TEXT NOT NULL CHECK (length(trim(drug_name)) > 0),
    substance        TEXT,
    atc_code         TEXT CHECK (atc_code ~ '^[A-Z][0-9]{2}[A-Z]{2}[0-9]{2}$'),
    dose             TEXT,
    -- Dosierschema, z. B. 1-0-1-0 oder „alle 8 h“
    schedule         TEXT,
    route            TEXT CHECK (route IN ('po', 'iv', 'sc', 'im', 'inhalativ', 'topisch', 'rektal', 'sonstiges')),
    status           TEXT NOT NULL DEFAULT 'aktiv' CHECK (status IN ('aktiv', 'pausiert', 'bedarf', 'abgesetzt')),
    indication       TEXT,
    -- Antiinfektivum: Beginn und geplante Dauer tragen den Therapiekurs
    is_antiinfective BOOLEAN NOT NULL DEFAULT FALSE,
    start_date       DATE NOT NULL,
    planned_days     INTEGER CHECK (planned_days BETWEEN 1 AND 365),
    end_date         DATE,
    prescribed_by    UUID NOT NULL REFERENCES users(id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    stopped_by       UUID REFERENCES users(id),
    version          INTEGER NOT NULL DEFAULT 1,
    CONSTRAINT prescriptions_dates CHECK (end_date IS NULL OR end_date >= start_date),
    CONSTRAINT prescriptions_stop_consistent
        CHECK ((status = 'abgesetzt') = (stopped_by IS NOT NULL)),
    CONSTRAINT prescriptions_stop_has_end
        CHECK (status <> 'abgesetzt' OR end_date IS NOT NULL)
);
CREATE INDEX idx_prescriptions_case ON prescriptions(case_id, status);

CREATE TABLE medication_administrations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    prescription_id UUID NOT NULL REFERENCES prescriptions(id) ON DELETE CASCADE,
    given_at        TIMESTAMPTZ NOT NULL,
    outcome         TEXT NOT NULL DEFAULT 'gegeben' CHECK (outcome IN ('gegeben', 'verweigert', 'ausgelassen')),
    dose            TEXT,
    note            TEXT,
    recorded_by     UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (outcome = 'gegeben' OR length(trim(coalesce(note, ''))) > 0)
);
CREATE INDEX idx_med_admin_prescription ON medication_administrations(prescription_id, given_at);
