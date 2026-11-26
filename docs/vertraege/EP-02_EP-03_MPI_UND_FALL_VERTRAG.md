# M01-Umsetzungsvertrag: `EP-02` & `EP-03` — Enterprise-MPI, Kontakte (`AP-12`), Episoden, Fälle & Kostenträger (`AP-13`)

Stand: 25.11.2026 · Autor: `google-neon` (Architektur & Koordination) · Basis: [ADR-002 & ADR-003](../EP00_ARCHITEKTUR_UND_ANALYSE.md), [ROADMAP.md](../ROADMAP.md) (`EP-02a`–`02c`, `EP-03a`–`03c`), [VERBUND_TESTMODELL.md](../VERBUND_TESTMODELL.md) · Status: **Spezifiziert (Umsetzung startet erst nach `EP-01a` und koordinierter Migrationsvergabe `013`/`014`)**

---

## 1. Paketabgrenzung (`EP-02a`–`02c` & `EP-03a`–`03c`)

| Teilpaket | Exklusiver Schreibbereich | Abhängigkeiten | Geplante Migration | Pflicht-Nachweise |
|---|---|---|---|---|
| **`EP-02a`** | `migrations/013_mpi_und_kontakte.sql`, `crates/kis-patients`, `crates/kis-core/src/security.rs` | `EP-01a` | **`013`** | DB-Rohinspektion (`*_enc` aktiv, kein Klartext in sensiblen Feldern), `D-03`, `D-05`, `NVT-05` |
| **`EP-02b`** | `crates/kis-patients` (MPI-Suche, `patient_records`, `patient_contacts` `AP-12`), `crates/kis-app/tests/` | `EP-02a` | keine | `NVT-05`, `NVT-10` (Lese-Audit auf MPI-Suche), keine globale `Scope::None`-Patientenliste |
| **`EP-02c`** | `crates/kis-patients` (Dubletten-Queue, atomarer Merge/Unmerge mit Lock & Prozesssperre) | `EP-02b` | keine | `NVT-06`, paralleler Merge-Race-Test, Notfall-/Neugeborenen-Überführung (`PAT-03`, `PAT-07`) |
| **`EP-03a`** | `migrations/014_episoden_faelle_kostentraeger.sql`, `config/module_tables.toml` | `EP-01a`, `EP-02a` | **`014`** | Schema-/Constraint-Tests (`D-01`–`D-08`), Doppelbelegungs-Constraint |
| **`EP-03b`** | `crates/kis-cases` (sektorübergreifende Fall-API, MVZ ohne Bett, Parallelfälle, Übergänge, `payers`/`case_coverages` `AP-13`) | `EP-03a` | keine | `NVT-07`, `PAT-01`, `PAT-02`, `PAT-04` |
| **`EP-03c`** | Durchreichung `legal_entity_id` in Bestandsmodule (`kis-clinical`, `kis-orders`, `kis-lab`, `kis-nursing`, `kis-prescriptions`, `kis-meals`, `kis-appointments`, `kis-doctors`, `kis-medications`) | `EP-03b` | optional `014b`/Folgenummer | Verbund-`hospital.rs`-Integrationstest |

---

## 2. Schema-Entwurf `EP-02`: Migration `013_mpi_und_kontakte.sql`

Wesentliche Änderungen gegenüber `migrations/001_init.sql` und `002_security.sql`:
- `patients.birth_date` wird **nullable** (`D-05` — Schluss mit fiktiven Geburtsdaten `1900-01-01` für unbekannte Notfälle oder Neugeborene).
- Globales `patients.external_id UNIQUE` wird durch mandanten- und namensraumbezogene `patient_identifiers` abgelöst.
- Aktive AEAD-Verschlüsselung (`*_enc` inkl. `key_version`) + HMAC-SHA-256-Blind-Index (`*_bidx`) für exakte Suche ohne Klartextspeicherung.
- Zweistufige Trennung `mpi_identities` (minimaler Index, Betriebsmodus `isolated_per_legal_entity` vs. `federated_with_legal_basis`) ↔ `patient_records` (rechtsträgerlokale Akte).

```sql
-- Entwurf für EP-02a: migrations/013_mpi_und_kontakte.sql

-- 1. Bestehende patients-Tabelle für unbekannte Notfallpatienten, Neugeborene & Schlüsselrotation öffnen
ALTER TABLE patients
    ALTER COLUMN birth_date DROP NOT NULL,
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN identity_status TEXT NOT NULL DEFAULT 'verified'
        CHECK (identity_status IN ('verified', 'unverified', 'unknown_emergency', 'newborn_provisional')),
    ADD COLUMN mother_patient_id UUID REFERENCES patients(id),
    ADD COLUMN key_version SMALLINT NOT NULL DEFAULT 1,
    ADD COLUMN insurance_number_bidx BYTEA,
    ADD COLUMN phone_bidx BYTEA,
    ADD COLUMN email_bidx BYTEA;

UPDATE patients
SET legal_entity_id = '00000000-0000-0000-0000-000000000001'
WHERE legal_entity_id IS NULL;

ALTER TABLE patients
    ALTER COLUMN legal_entity_id SET NOT NULL,
    ADD CONSTRAINT patients_id_tenant_unique UNIQUE (id, legal_entity_id);

-- 2. Master Patient Index (minimaler demografischer Knoten; KEINE klinischen Daten!)
CREATE TABLE mpi_identities (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Bei mpi.mode = 'isolated_per_legal_entity' wird scope_legal_entity_id gesetzt;
    -- bei 'federated_with_legal_basis' ist scope_legal_entity_id NULL.
    scope_legal_entity_id   UUID REFERENCES legal_entities(id),
    identity_status         TEXT NOT NULL DEFAULT 'verified'
                            CHECK (identity_status IN ('verified', 'unverified', 'unknown_emergency', 'newborn_provisional')),
    merged_into_id          UUID REFERENCES mpi_identities(id),
    locked_for_merge        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    version                 INTEGER NOT NULL DEFAULT 1,
    CHECK (merged_into_id IS DISTINCT FROM id)
);
CREATE INDEX idx_mpi_identities_scope ON mpi_identities(scope_legal_entity_id) WHERE merged_into_id IS NULL;

-- 3. Rechtsträgerlokale Patientenakte (verbindet patients mit mpi_identities)
CREATE TABLE patient_records (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id         UUID NOT NULL REFERENCES legal_entities(id),
    patient_id              UUID NOT NULL,
    mpi_identity_id         UUID NOT NULL REFERENCES mpi_identities(id),
    local_mrn               TEXT NOT NULL CHECK (length(trim(local_mrn)) > 0),
    record_status           TEXT NOT NULL DEFAULT 'active'
                            CHECK (record_status IN ('active', 'merged', 'archived', 'anonymized')),
    federation_legal_basis  TEXT CHECK (federation_legal_basis IS NULL OR federation_legal_basis IN (
        'local_only', 'explicit_consent', 'statutory_mandate', 'emergency_vital_interest'
    )),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    version                 INTEGER NOT NULL DEFAULT 1,
    UNIQUE (legal_entity_id, local_mrn),
    UNIQUE (legal_entity_id, patient_id),
    UNIQUE (id, legal_entity_id),
    FOREIGN KEY (patient_id, legal_entity_id) REFERENCES patients(id, legal_entity_id)
);
CREATE INDEX idx_patient_records_mpi ON patient_records(mpi_identity_id, legal_entity_id);

-- 4. Identifikator-Namensräume & Provenienz (löst globales external_id UNIQUE ab)
CREATE TABLE patient_identifiers (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id   UUID NOT NULL REFERENCES legal_entities(id),
    patient_record_id UUID NOT NULL,
    namespace_oid     TEXT NOT NULL CHECK (namespace_oid ~ '^[0-9]+(\.[0-9]+)+$|^urn:[a-z0-9:._-]+$'),
    identifier_type   TEXT NOT NULL CHECK (identifier_type IN (
        'local_mrn', 'kvnr', 'pkv_number', 'external_his', 'emergency_temp', 'newborn_temp', 'other'
    )),
    value_enc         TEXT NOT NULL,
    value_bidx        BYTEA NOT NULL,
    key_version       SMALLINT NOT NULL DEFAULT 1,
    source_system     TEXT NOT NULL CHECK (length(trim(source_system)) > 0),
    verified_at       TIMESTAMPTZ,
    valid_from        TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until       TIMESTAMPTZ,
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    FOREIGN KEY (patient_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id)
);
CREATE UNIQUE INDEX idx_patient_identifiers_unique_active
    ON patient_identifiers(legal_entity_id, namespace_oid, identifier_type, value_bidx)
    WHERE valid_until IS NULL;

-- 5. Dubletten-Arbeitsliste (kein automatischer Merge!)
CREATE TABLE mpi_match_candidates (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id      UUID NOT NULL REFERENCES legal_entities(id),
    left_record_id       UUID NOT NULL,
    right_record_id      UUID NOT NULL,
    match_score          NUMERIC(5,4) NOT NULL CHECK (match_score >= 0 AND match_score <= 1),
    match_reasons        JSONB NOT NULL DEFAULT '[]'::jsonb,
    status               TEXT NOT NULL DEFAULT 'open'
                         CHECK (status IN ('open', 'confirmed_duplicate', 'rejected_distinct', 'merged')),
    reviewed_by          UUID REFERENCES users(id),
    reviewed_at          TIMESTAMPTZ,
    review_note          TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (left_record_id <> right_record_id),
    FOREIGN KEY (left_record_id,  legal_entity_id) REFERENCES patient_records(id, legal_entity_id),
    FOREIGN KEY (right_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id)
);

-- 6. Auditierter, reversibler Merge/Unmerge (strikt innerhalb eines Rechtsträgers!)
CREATE TABLE mpi_merge_events (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id      UUID NOT NULL REFERENCES legal_entities(id),
    source_record_id     UUID NOT NULL,
    target_record_id     UUID NOT NULL,
    action               TEXT NOT NULL CHECK (action IN ('merge', 'unmerge')),
    reason               TEXT NOT NULL CHECK (length(trim(reason)) >= 10),
    -- Snapshot betroffener Referenzen für verlustfreien Unmerge (ohne Klartext-PII-Dump)
    relinked_entities    JSONB NOT NULL DEFAULT '{}'::jsonb,
    performed_by         UUID NOT NULL REFERENCES users(id),
    performed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (source_record_id <> target_record_id),
    FOREIGN KEY (source_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id),
    FOREIGN KEY (target_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id)
);

-- 7. Patientenkontakte, Angehörige, Betreuung, Hausarzt & Einweiser (AP-12)
CREATE TABLE patient_contacts (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id   UUID NOT NULL REFERENCES legal_entities(id),
    patient_record_id UUID NOT NULL,
    contact_kind      TEXT NOT NULL CHECK (contact_kind IN (
        'next_of_kin', 'legal_guardian', 'power_of_attorney',
        'family_doctor', 'referring_physician', 'emergency_contact', 'other'
    )),
    display_name      TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    organization_name TEXT,
    lanr              TEXT CHECK (lanr IS NULL OR lanr ~ '^[0-9]{9}$'),
    bsnr              TEXT CHECK (bsnr IS NULL OR bsnr ~ '^[0-9]{9}$'),
    phone_enc         TEXT,
    email_enc         TEXT,
    address_enc       TEXT,
    key_version       SMALLINT NOT NULL DEFAULT 1,
    may_receive_info  BOOLEAN NOT NULL DEFAULT FALSE,
    valid_from        TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until       TIMESTAMPTZ,
    created_by        UUID NOT NULL REFERENCES users(id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    FOREIGN KEY (patient_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id)
);
CREATE INDEX idx_patient_contacts_record ON patient_contacts(patient_record_id) WHERE valid_until IS NULL;
```

### Pflicht-Invariante beim Merge (`EP-02c` / `NVT-06`)
1. Ein Merge darf **niemals** zwei `patient_records` unterschiedlicher `legal_entity_id` zusammenführen (Datenbank-FK + Anwendungsprüfung → `403 cross_tenant_merge_forbidden`).
2. Vor jedem Merge erwirbt die Transaktion `SELECT ... FROM patient_records WHERE id IN ($1, $2) ORDER BY id FOR UPDATE` (Deadlock-freie Lock-Reihenfolge) und prüft aktive Sperrprozesse (laufende OP, offene Transfusion, gesperrte Abrechnung → `409 merge_blocked_active_critical_process`).
3. Unmerge stellt anhand von `relinked_entities` die ursprünglichen Zuordnungen wieder her und schreibt einen eigenen `unmerge`-Eintrag samt Audit.

---

## 3. Schema-Entwurf `EP-03`: Migration `014_episoden_faelle_kostentraeger.sql`

Rückwärtskompatibilitäts-Brücke zu `migrations/006_faelle.sql`:
- Die bestehende Tabelle `cases` fungiert weiterhin als primäre `clinical_cases`-Basis (so bleiben sämtliche bestehenden Fremdschlüssel aus `007`–`010` wie `case_diagnoses.case_id REFERENCES cases(id)` vollständig gültig!), wird jedoch um `legal_entity_id`, `site_id`, `patient_record_id`, `care_episode_id` und sektorübergreifende `care_setting`-Unterstützung erweitert.
- `cases.department_id` wird **nullable** (Pflicht nur bei `kind IN ('stationaer', 'teilstationaer')`), damit MVZ-, Praxis-, Ambulanz- und ZNA-Kontakte ohne stationäre Fachabteilung oder Bett abgebildet werden können (`G-08`).
- `cases_one_open_inpatient` wird auf `(legal_entity_id, patient_id)` umgestellt, damit gleichzeitige stationäre/teilstationäre Fälle in getrennten Rechtsträgern nicht blockiert werden.

```sql
-- Entwurf für EP-03a: migrations/014_episoden_faelle_kostentraeger.sql

-- 1. Längsschnitt-Episode innerhalb eines Rechtsträgers
CREATE TABLE care_episodes (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id   UUID NOT NULL REFERENCES legal_entities(id),
    patient_record_id UUID NOT NULL,
    episode_kind      TEXT NOT NULL CHECK (episode_kind IN (
        'general', 'oncology', 'pregnancy', 'chronic', 'rehab', 'emergency_chain', 'other'
    )),
    title             TEXT NOT NULL CHECK (length(trim(title)) > 0),
    status            TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'closed', 'cancelled')),
    started_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at          TIMESTAMPTZ,
    created_by        UUID NOT NULL REFERENCES users(id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    version           INTEGER NOT NULL DEFAULT 1,
    CHECK (ended_at IS NULL OR ended_at >= started_at),
    UNIQUE (id, legal_entity_id),
    FOREIGN KEY (patient_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id)
);

-- 2. Bestehende cases-Tabelle mandanten- & sektorübergreifend ertüchtigen
ALTER TABLE cases
    ADD COLUMN legal_entity_id   UUID REFERENCES legal_entities(id),
    ADD COLUMN site_id           UUID,
    ADD COLUMN patient_record_id UUID,
    ADD COLUMN care_episode_id   UUID,
    ADD COLUMN care_setting      TEXT NOT NULL DEFAULT 'inpatient' CHECK (care_setting IN (
        'inpatient', 'day_clinic', 'emergency', 'outpatient_hospital',
        'mvz_kv', 'private_practice', 'pre_inpatient', 'post_inpatient',
        'rehab', 'aop', 'asv'
    )),
    ADD COLUMN care_responsibility TEXT NOT NULL DEFAULT 'attending' CHECK (care_responsibility IN (
        'attending', 'consulting', 'belegarzt', 'shared_care', 'external_partner'
    ));

UPDATE cases
SET legal_entity_id = '00000000-0000-0000-0000-000000000001'
WHERE legal_entity_id IS NULL;

ALTER TABLE cases
    ALTER COLUMN legal_entity_id SET NOT NULL,
    ALTER COLUMN department_id DROP NOT NULL,
    ADD CONSTRAINT cases_id_tenant_unique UNIQUE (id, legal_entity_id),
    ADD CONSTRAINT cases_site_same_tenant_fk
        FOREIGN KEY (site_id, legal_entity_id) REFERENCES sites(id, legal_entity_id),
    ADD CONSTRAINT cases_episode_same_tenant_fk
        FOREIGN KEY (care_episode_id, legal_entity_id) REFERENCES care_episodes(id, legal_entity_id),
    ADD CONSTRAINT cases_inpatient_needs_department
        CHECK (care_setting NOT IN ('inpatient', 'day_clinic') OR department_id IS NOT NULL);

-- Mandantenbezogener Index ersetzt globalen cases_one_open_inpatient
DROP INDEX IF EXISTS cases_one_open_inpatient;
CREATE UNIQUE INDEX cases_one_open_inpatient_per_tenant
    ON cases(legal_entity_id, patient_id)
    WHERE status = 'aufgenommen' AND kind IN ('stationaer', 'teilstationaer');

-- 3. Encounters (einzelner Kontakt / Sprechstunde / ZNA / Aufenthaltsabschnitt)
CREATE TABLE encounters (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id     UUID NOT NULL REFERENCES legal_entities(id),
    site_id             UUID NOT NULL,
    case_id             UUID NOT NULL,
    org_unit_id         UUID NOT NULL,
    encounter_kind      TEXT NOT NULL CHECK (encounter_kind IN (
        'inpatient_stay', 'day_clinic_visit', 'emergency_visit',
        'mvz_consultation', 'outpatient_visit', 'telemedicine', 'consil'
    )),
    status              TEXT NOT NULL DEFAULT 'in_progress'
                        CHECK (status IN ('planned', 'arrived', 'in_progress', 'finished', 'cancelled')),
    responsible_user_id UUID REFERENCES users(id),
    responsibility_role TEXT NOT NULL DEFAULT 'attending'
                        CHECK (responsibility_role IN ('attending', 'consulting', 'belegarzt', 'shared_care', 'external_partner')),
    started_at          TIMESTAMPTZ NOT NULL,
    ended_at            TIMESTAMPTZ,
    created_by          UUID NOT NULL REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INTEGER NOT NULL DEFAULT 1,
    CHECK (ended_at IS NULL OR ended_at >= started_at),
    UNIQUE (id, legal_entity_id),
    FOREIGN KEY (site_id, legal_entity_id)     REFERENCES sites(id, legal_entity_id),
    FOREIGN KEY (case_id, legal_entity_id)     REFERENCES cases(id, legal_entity_id),
    FOREIGN KEY (org_unit_id, legal_entity_id) REFERENCES org_units(id, legal_entity_id)
);
CREATE INDEX idx_encounters_case ON encounters(case_id, started_at);
CREATE INDEX idx_encounters_unit ON encounters(legal_entity_id, org_unit_id, started_at);

-- 4. Klinische Doppelbelegungsprüfung auf case_stays (gleicher Patient darf nicht gleichzeitig 2 physische Betten belegen)
ALTER TABLE case_stays
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN patient_id      UUID REFERENCES patients(id),
    ADD COLUMN encounter_id    UUID;

UPDATE case_stays cs
SET legal_entity_id = c.legal_entity_id,
    patient_id = c.patient_id
FROM cases c
WHERE c.id = cs.case_id AND cs.legal_entity_id IS NULL;

ALTER TABLE case_stays
    ALTER COLUMN legal_entity_id SET NOT NULL,
    ALTER COLUMN patient_id SET NOT NULL,
    ADD CONSTRAINT case_stays_encounter_fk
        FOREIGN KEY (encounter_id, legal_entity_id) REFERENCES encounters(id, legal_entity_id),
    ADD CONSTRAINT case_stays_no_patient_bed_overlap
        EXCLUDE USING gist (patient_id WITH =, tstzrange(started_at, ended_at) WITH &&)
        WHERE (bed_id IS NOT NULL);

-- 5. Eigenständiger Abrechnungsfall (getrennt vom klinischen Fall!)
CREATE TABLE billing_cases (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id     UUID NOT NULL REFERENCES legal_entities(id),
    site_id             UUID NOT NULL,
    patient_record_id   UUID NOT NULL,
    billing_number      TEXT NOT NULL,
    billing_regime      TEXT NOT NULL CHECK (billing_regime IN (
        'drg', 'pepp', 'kv_ebm', 'goae', 'bg_uv', 'aop', 'asv', 'hybrid_drg', 'self_pay', 'other'
    )),
    quarter             TEXT CHECK (quarter IS NULL OR quarter ~ '^[0-9]{4}Q[1-4]$'),
    status              TEXT NOT NULL DEFAULT 'open'
                        CHECK (status IN ('open', 'ready_for_coding', 'coded', 'billed', 'disputed', 'settled', 'cancelled')),
    created_by          UUID NOT NULL REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INTEGER NOT NULL DEFAULT 1,
    UNIQUE (legal_entity_id, billing_number),
    UNIQUE (id, legal_entity_id),
    FOREIGN KEY (site_id, legal_entity_id)           REFERENCES sites(id, legal_entity_id),
    FOREIGN KEY (patient_record_id, legal_entity_id) REFERENCES patient_records(id, legal_entity_id),
    CHECK (billing_regime <> 'kv_ebm' OR quarter IS NOT NULL)
);

CREATE TABLE clinical_billing_case_links (
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    case_id         UUID NOT NULL,
    billing_case_id UUID NOT NULL,
    link_reason     TEXT NOT NULL DEFAULT 'primary'
                    CHECK (link_reason IN ('primary', 'case_merge_ogvd', 'pre_inpatient', 'post_inpatient', 'supplementary')),
    linked_by       UUID NOT NULL REFERENCES users(id),
    linked_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (case_id, billing_case_id),
    FOREIGN KEY (case_id, legal_entity_id)         REFERENCES cases(id, legal_entity_id),
    FOREIGN KEY (billing_case_id, legal_entity_id) REFERENCES billing_cases(id, legal_entity_id)
);

-- 6. Kostenträger & Versicherungsverhältnisse (AP-13)
CREATE TABLE payers (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    payer_kind      TEXT NOT NULL CHECK (payer_kind IN ('gkv', 'pkv', 'bg_uv', 'sozialhilfe', 'self_pay', 'other')),
    ik_number       TEXT CHECK (ik_number IS NULL OR ik_number ~ '^[0-9]{9}$'),
    name            TEXT NOT NULL CHECK (length(trim(name)) > 0),
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, legal_entity_id),
    CHECK (payer_kind <> 'gkv' OR ik_number IS NOT NULL)
);

CREATE TABLE case_coverages (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    billing_case_id UUID NOT NULL,
    payer_id        UUID NOT NULL,
    coverage_rank   TEXT NOT NULL CHECK (coverage_rank IN ('primary', 'secondary', 'supplementary')),
    policy_no_enc   TEXT,
    key_version     SMALLINT NOT NULL DEFAULT 1,
    valid_from      DATE NOT NULL,
    valid_until     DATE,
    CHECK (valid_until IS NULL OR valid_until >= valid_from),
    FOREIGN KEY (billing_case_id, legal_entity_id) REFERENCES billing_cases(id, legal_entity_id),
    FOREIGN KEY (payer_id, legal_entity_id)        REFERENCES payers(id, legal_entity_id),
    -- Höchstens ein Hauptkostenträger je Abrechnungsfall im selben Zeitraum
    CONSTRAINT case_coverages_one_primary
        EXCLUDE USING gist (billing_case_id WITH =, daterange(valid_from, valid_until, '[]') WITH &&)
        WHERE (coverage_rank = 'primary')
);

-- 7. Überweisungen, Rücküberweisungen, Konsile & Rechtsträger-Überleitungen (erhält ID-Historie)
CREATE TABLE case_transitions (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    transition_kind         TEXT NOT NULL CHECK (transition_kind IN (
        'internal_transfer', 'cross_entity_discharge_admission',
        'referral_out', 'referral_back', 'consil_request'
    )),
    source_legal_entity_id  UUID NOT NULL REFERENCES legal_entities(id),
    source_case_id          UUID NOT NULL,
    target_legal_entity_id  UUID NOT NULL REFERENCES legal_entities(id),
    target_case_id          UUID,
    legal_basis             TEXT NOT NULL CHECK (legal_basis IN (
        'same_legal_entity', 'explicit_patient_consent', 'referral_mandate', 'emergency_vital_interest'
    )),
    status                  TEXT NOT NULL DEFAULT 'initiated'
                            CHECK (status IN ('initiated', 'accepted', 'completed', 'rejected', 'cancelled')),
    created_by              UUID NOT NULL REFERENCES users(id),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (source_case_id, source_legal_entity_id) REFERENCES cases(id, legal_entity_id),
    FOREIGN KEY (target_case_id, target_legal_entity_id) REFERENCES cases(id, legal_entity_id),
    CHECK (transition_kind <> 'internal_transfer' OR source_legal_entity_id = target_legal_entity_id),
    CHECK (transition_kind <> 'cross_entity_discharge_admission' OR source_legal_entity_id <> target_legal_entity_id)
);
```

---

## 4. `config/module_tables.toml`-Erweiterung (`EP-02`/`EP-03`)

Alle Tabellen aus Migration `013` und `014` gehören zum **Kern** (`[core].tables`), da Patientenidentität, Kontakte, Episoden, Fälle, Encounters, Abrechnungsfälle und Kostenträger von mehreren Fachmodulen konsistent referenziert werden:
- `mpi_identities`, `patient_records`, `patient_identifiers`, `mpi_match_candidates`, `mpi_merge_events`, `patient_contacts`
- `care_episodes`, `encounters`, `billing_cases`, `clinical_billing_case_links`, `payers`, `case_coverages`, `case_transitions`

---

## 5. Pflicht-Abnahmetests (`G2`/`G3`) für `EP-02` & `EP-03`

1. **`NVT-05` (MPI ohne Quersicht):** `U-DOC-N1` sucht `PAT-02` über MPI/KVNR → sieht nur minimale Identitätsdaten bzw. seine lokale `RT-NORD`-Akte (bei `isolated_per_legal_entity` 0 Treffer für `RT-SUED`), **niemals** `RT-SUED`-Fälle, Diagnosen oder Befunde; Suchzugriff erzeugt Lese-Audit (`NVT-10`).
2. **`NVT-06` (Merge-Sperre & Race):** Versuch, `PAT-02` (`RT-NORD`) mit `PAT-02` (`RT-SUED`) zu mergen → `403`/`409`; paralleler Merge derselben zwei `RT-NORD`-Dubletten (`PAT-03`) wird serialisiert; Unmerge stellt Vorzustand vollständig her.
3. **`NVT-07` (Rechtsträgerwechsel & Fallnummern-Isolation):** `U-DOC-N1` ruft `GET /api/v1/cases?caseNumber=<RT-SUED>` oder direkte `case_id` von `PAT-02` in `RT-SUED` auf → `403`/`404` ohne Existenz-Leak; Versuch einer internen Verlegung von `KL-N2` (`RT-NORD`) nach `KL-S1` (`RT-SUED`, `PAT-04`) → `400`/`403`; zulässig ist ausschließlich Entlassung + Neuaufnahme mit `case_transitions(kind = 'cross_entity_discharge_admission')`.
4. **Klinische Doppelbelegung & MVZ ohne Bett:** Gleichzeitige physische Bettbelegung desselben Patienten auf zwei Stationen schlägt über `case_stays_no_patient_bed_overlap` fehl; sektorübergreifender `mvz_kv`-Fall (`PAT-01` in `MVZ-N1`) ohne `department_id` und ohne `bed_id` gelingt sauber.
5. **Roh-DB-Inspektion (`D-03`/`D-05`):** Nach `POST /api/v1/patients` stehen `insurance_number`, `phone`, `email` in der DB nicht mehr im Klartext; Notfallpatient (`PAT-03`) und Neugeborenes (`PAT-07`) lassen sich mit `birth_date = NULL` anlegen.
