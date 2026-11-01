-- ============================================================
-- KIS — Datenbank-Migrationen (SQLite)
-- Für den Prototyp: SQLite mit WAL-Modus
-- ============================================================

-- ============================================================
-- USER & RBAC
-- ============================================================
CREATE TABLE IF NOT EXISTS users (
    id              TEXT PRIMARY KEY,
    username        TEXT UNIQUE NOT NULL,
    password_hash   TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    email           TEXT UNIQUE NOT NULL,
    role            TEXT NOT NULL DEFAULT 'reception',
    is_active       INTEGER NOT NULL DEFAULT 1,
    mfa_secret      TEXT,
    mfa_enabled     INTEGER NOT NULL DEFAULT 0,
    last_login_at   TEXT,
    login_attempts  INTEGER NOT NULL DEFAULT 0,
    locked_until    TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_role ON users(role) WHERE is_active = 1;

-- ============================================================
-- PATIENTEN
-- ============================================================
CREATE TABLE IF NOT EXISTS patients (
    id              TEXT PRIMARY KEY,
    external_id     TEXT,
    first_name      TEXT NOT NULL,
    last_name       TEXT NOT NULL,
    birth_date      TEXT NOT NULL,
    gender          TEXT,
    insurance_no    TEXT,
    insurance_name  TEXT,
    address_line    TEXT,
    address_city    TEXT,
    address_zip     TEXT,
    phone           TEXT,
    email           TEXT,
    blood_type      TEXT,
    allergies       TEXT NOT NULL DEFAULT '[]',
    notes           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_patients_name ON patients(last_name, first_name);
CREATE INDEX IF NOT EXISTS idx_patients_birth ON patients(birth_date);
CREATE INDEX IF NOT EXISTS idx_patients_insurance ON patients(insurance_no) WHERE insurance_no IS NOT NULL;

-- ============================================================
-- ARZTE
-- ============================================================
CREATE TABLE IF NOT EXISTS doctors (
    id              TEXT PRIMARY KEY,
    patient_id      TEXT REFERENCES patients(id) ON DELETE CASCADE,
    external_id     TEXT,
    first_name      TEXT NOT NULL,
    last_name       TEXT NOT NULL,
    title           TEXT,
    specialty       TEXT,
    license_no      TEXT,
    phone           TEXT,
    email           TEXT,
    department      TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_doctors_name ON doctors(last_name, first_name);
CREATE INDEX IF NOT EXISTS idx_doctors_specialty ON doctors(specialty);

-- ============================================================
-- TERMINE
-- ============================================================
CREATE TABLE IF NOT EXISTS appointments (
    id              TEXT PRIMARY KEY,
    external_id     TEXT,
    patient_id      TEXT REFERENCES patients(id) ON DELETE CASCADE,
    doctor_id       TEXT REFERENCES doctors(id) ON DELETE CASCADE,
    start_time      TEXT NOT NULL,
    end_time        TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'scheduled',
    reason          TEXT,
    location        TEXT,
    notes           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_appointments_time ON appointments(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_appointments_patient ON appointments(patient_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_appointments_doctor ON appointments(doctor_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_appointments_status ON appointments(status) WHERE deleted_at IS NULL;

-- ============================================================
-- MEDIKAMENTE
-- ============================================================
CREATE TABLE IF NOT EXISTS medications (
    id              TEXT PRIMARY KEY,
    patient_id      TEXT REFERENCES patients(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    dosage          TEXT,
    form            TEXT,
    frequency       TEXT,
    prescriber_id   TEXT REFERENCES doctors(id),
    start_date      TEXT NOT NULL,
    end_date        TEXT,
    active          INTEGER NOT NULL DEFAULT 1,
    notes           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
);

CREATE INDEX IF NOT EXISTS idx_medications_patient ON medications(patient_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_medications_active ON medications(patient_id, active) WHERE deleted_at IS NULL AND active = 1;
