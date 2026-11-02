-- ============================================================
-- KIS — Datenbank-Migration 001: Basis-Schema (PostgreSQL)
-- KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO.
--
-- Konventionen:
--   * UUID-Primärschlüssel (gen_random_uuid(), pgcrypto)
--   * Optimistic Locking: version INT NOT NULL DEFAULT 1
--   * Soft-Delete (DSGVO-Art. 17): deleted_at TIMESTAMPTZ
--   * Audit-Felder: created_at / updated_at
-- ============================================================

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ============================================================
-- USER & RBAC
-- ============================================================
CREATE TABLE IF NOT EXISTS users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username        TEXT UNIQUE NOT NULL,
    password_hash   TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    email           TEXT UNIQUE NOT NULL,
    role            TEXT NOT NULL DEFAULT 'rezeption',
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    mfa_secret      TEXT,
    mfa_enabled     BOOLEAN NOT NULL DEFAULT FALSE,
    last_login_at   TIMESTAMPTZ,
    login_attempts  INTEGER NOT NULL DEFAULT 0,
    locked_until    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_role ON users(role) WHERE is_active = TRUE;

-- ============================================================
-- PATIENTEN
-- ============================================================
CREATE TABLE IF NOT EXISTS patients (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id       TEXT UNIQUE,
    name_family       TEXT NOT NULL,
    name_given        TEXT NOT NULL,
    birth_date        DATE NOT NULL,
    gender            TEXT,
    phone             TEXT,
    email             TEXT,
    address_street    TEXT,
    address_zip       TEXT,
    address_city      TEXT,
    insurance_number  TEXT,
    allergies         JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ,
    version           INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_patients_name ON patients(name_family, name_given);
CREATE INDEX IF NOT EXISTS idx_patients_birth ON patients(birth_date);
CREATE INDEX IF NOT EXISTS idx_patients_external ON patients(external_id) WHERE external_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_patients_active ON patients(id) WHERE deleted_at IS NULL;

-- ============================================================
-- ÄRZTE
-- ============================================================
CREATE TABLE IF NOT EXISTS doctors (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id       TEXT UNIQUE,
    name_family       TEXT NOT NULL,
    name_given        TEXT NOT NULL,
    specialty         TEXT,
    specialties       JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ,
    version           INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_doctors_name ON doctors(name_family, name_given);
CREATE INDEX IF NOT EXISTS idx_doctors_specialty ON doctors(specialty);
CREATE INDEX IF NOT EXISTS idx_doctors_active ON doctors(id) WHERE deleted_at IS NULL;

-- ============================================================
-- TERMINE
-- ============================================================
CREATE TABLE IF NOT EXISTS appointments (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    patient_id        UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    doctor_id         UUID NOT NULL REFERENCES doctors(id) ON DELETE CASCADE,
    category          TEXT NOT NULL DEFAULT 'ordination',
    status            TEXT NOT NULL DEFAULT 'geplant',
    start_time        TIMESTAMPTZ NOT NULL,
    end_time          TIMESTAMPTZ NOT NULL,
    reason            TEXT,
    cancelled_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ,
    version           INTEGER NOT NULL DEFAULT 1,
    CONSTRAINT appointments_start_before_end CHECK (end_time > start_time)
);

CREATE INDEX IF NOT EXISTS idx_appointments_time ON appointments(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_appointments_patient ON appointments(patient_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_appointments_doctor ON appointments(doctor_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_appointments_status ON appointments(status) WHERE deleted_at IS NULL;

-- ============================================================
-- MEDIKAMENTE
-- ============================================================
CREATE TABLE IF NOT EXISTS medications (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    patient_id        UUID NOT NULL REFERENCES patients(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    dosage            TEXT,
    frequency         TEXT,
    start_date        DATE NOT NULL,
    end_date          DATE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ,
    version           INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_medications_patient ON medications(patient_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_medications_active ON medications(patient_id, start_date, end_date) WHERE deleted_at IS NULL;
