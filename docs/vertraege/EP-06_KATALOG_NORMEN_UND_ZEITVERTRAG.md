# M01-Umsetzungsvertrag: `EP-06` — Normen-, Terminologie-, Katalog- (`AP-14`) & Zeit-/Einheitenvertrag

Stand: 25.11.2026 · Autor: `google-neon` (Architektur & Koordination) · Basis: [ADR-006](../EP00_ARCHITEKTUR_UND_ANALYSE.md), [TI_COMPLIANCE.md](../TI_COMPLIANCE.md), [ENTERPRISE_KIS.md](../ENTERPRISE_KIS.md) § 4, [ROADMAP.md](../ROADMAP.md) (`EP-06a`, `EP-06b`) · Status: **Spezifiziert (kann nach `EP-00`-`G6` und koordinierter Migrationsvergabe parallel zu `EP-01b`/`EP-01c` starten!)**

---

## 1. Paketabgrenzung (`EP-06a` & `EP-06b`)

| Teilpaket | Exklusiver Schreibbereich | Abhängigkeiten | Geplante Migration | Pflicht-Nachweise |
|---|---|---|---|---|
| **`EP-06a`** | Neues Modul `crates/kis-catalogs/`, `Cargo.toml`, `config/module_tables.toml`, `crates/kis-app/src/modules.rs`, `crates/kis-app/src/bin/kis-admin.rs`, `migrations/016_kataloge_normen.sql` | `EP-00` (`G6`), koordinierte Migrationsnummer | **`016`** *(oder erste freie Nummer nach `012`)* | Wiederholbarer Import synthetischer Kataloge, Ablaufprüfung, maschinenlesbarer Abruf `normative_requirements`, `G0`–`G6` |
| **`EP-06b`** | `crates/kis-core/src/clinical_types.rs` (neu), `crates/kis-clinical`, `crates/kis-lab`, `crates/kis-prescriptions` | `EP-06a`, `EP-03c` | optional Folgenummer | Stichtagsbindung ICD/OPS, UCUM-Einheitenprüfung, Trennung `preliminary/final/corrected/cancelled`, `UTC`+lokale Zeitzone |

> **Wichtige Regel (`AGENTS.md` § 2):** Keine amtlichen Kataloge, medizinischen Grenzwerte oder Abrechnungsregeln erfinden! CI-Tests nutzen ausschließlich klar als `SYNTHETIC_TEST_ONLY` gekennzeichnete Testkataloge und die Seed-Struktur [normative_requirements_seed.json](normative_requirements_seed.json).

---

## 2. Schema-Entwurf `EP-06a`: Migration `016_kataloge_normen.sql`

```sql
-- Entwurf für EP-06a: migrations/016_kataloge_normen.sql

-- 1. Katalogquellen & Lizenzregister (Pflicht laut TI_COMPLIANCE.md & ADR-006)
CREATE TABLE catalog_sources (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code               TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Z0-9_.-]{2,32}$'),
    title              TEXT NOT NULL CHECK (length(trim(title)) > 0),
    publisher          TEXT NOT NULL CHECK (length(trim(publisher)) > 0),
    official_url       TEXT,
    license_kind       TEXT NOT NULL CHECK (license_kind IN (
        'public_domain', 'open_license', 'official_free_with_terms',
        'commercial_licensed', 'synthetic_test_only'
    )),
    license_reference  TEXT NOT NULL CHECK (length(trim(license_reference)) > 0),
    license_expires_at DATE,
    owner_role         TEXT NOT NULL CHECK (length(trim(owner_role)) > 0),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 2. Versionierte Katalogstände mit Stichtagsgültigkeit & Integritätsprüfsumme
CREATE TABLE catalog_versions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id           UUID NOT NULL REFERENCES catalog_sources(id),
    version_label       TEXT NOT NULL CHECK (length(trim(version_label)) > 0),
    valid_from          DATE NOT NULL,
    valid_until         DATE,
    status              TEXT NOT NULL DEFAULT 'staged'
                        CHECK (status IN ('staged', 'active', 'retired', 'rejected')),
    source_sha256       TEXT NOT NULL CHECK (source_sha256 ~ '^[0-9a-f]{64}$'),
    is_synthetic        BOOLEAN NOT NULL DEFAULT FALSE,
    imported_by         UUID REFERENCES users(id),
    imported_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    activated_at        TIMESTAMPTZ,
    UNIQUE (source_id, version_label),
    CHECK (valid_until IS NULL OR valid_until >= valid_from)
);
CREATE INDEX idx_catalog_versions_validity
    ON catalog_versions(source_id, valid_from, valid_until) WHERE status = 'active';

-- 3. Katalogeinträge (ICD-10-GM, OPS, Alpha-ID, UCUM, LOINC, SNOMED CT, ATC, PZN, oBDS ...)
CREATE TABLE catalog_entries (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    catalog_version_id  UUID NOT NULL REFERENCES catalog_versions(id) ON DELETE CASCADE,
    code                TEXT NOT NULL CHECK (length(trim(code)) > 0),
    display_de          TEXT NOT NULL CHECK (length(trim(display_de)) > 0),
    display_en          TEXT,
    parent_code         TEXT,
    is_billable         BOOLEAN NOT NULL DEFAULT TRUE,
    attributes          JSONB NOT NULL DEFAULT '{}'::jsonb,
    UNIQUE (catalog_version_id, code)
);
CREATE INDEX idx_catalog_entries_search
    ON catalog_entries(catalog_version_id, code);

-- 4. Versionierte Querverweise / Mappings (z. B. ICD-10-GM <-> Alpha-ID, lokaler Laborcode <-> LOINC)
CREATE TABLE catalog_mappings (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_entry_id     UUID NOT NULL REFERENCES catalog_entries(id) ON DELETE CASCADE,
    target_entry_id     UUID NOT NULL REFERENCES catalog_entries(id) ON DELETE CASCADE,
    equivalence         TEXT NOT NULL CHECK (equivalence IN (
        'equivalent', 'wider', 'narrower', 'inexact', 'unmatched'
    )),
    mapping_authority   TEXT NOT NULL CHECK (length(trim(mapping_authority)) > 0),
    valid_from          DATE NOT NULL,
    valid_until         DATE,
    CHECK (source_entry_id <> target_entry_id),
    CHECK (valid_until IS NULL OR valid_until >= valid_from)
);

-- 5. Maschinenlesbares Normen- & Compliance-Register (Pflicht laut TI_COMPLIANCE.md)
CREATE TABLE normative_requirements (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    requirement_code    TEXT NOT NULL UNIQUE CHECK (requirement_code ~ '^[A-Z0-9_-]{3,32}$'),
    title               TEXT NOT NULL CHECK (length(trim(title)) > 0),
    source_authority    TEXT NOT NULL CHECK (length(trim(source_authority)) > 0),
    source_url          TEXT NOT NULL CHECK (source_url ~ '^https://'),
    spec_version        TEXT NOT NULL CHECK (length(trim(spec_version)) > 0),
    valid_from          DATE,
    valid_until         DATE,
    legal_obligation    TEXT NOT NULL CHECK (length(trim(legal_obligation)) > 0),
    sector              TEXT NOT NULL CHECK (sector IN ('inpatient', 'mvz', 'both', 'central_service')),
    system_role         TEXT NOT NULL CHECK (system_role IN ('primary_system', 'connector_gateway', 'trust_service', 'interface')),
    native_class        TEXT NOT NULL CHECK (native_class IN ('N', 'H', 'X')),
    status              TEXT NOT NULL CHECK (status IN (
        'geplant', 'spezifiziert', 'in_bearbeitung', 'blockiert', 'im_review',
        'technisch_integriert', 'lokal_getestet', 'gegenstelle_getestet',
        'fachlich_abgenommen', 'extern_bestaetigt', 'betriebsfreigegeben'
    )),
    test_ids            TEXT[] NOT NULL DEFAULT '{}'::text[],
    evidence_ref        TEXT,
    owner_role          TEXT NOT NULL CHECK (length(trim(owner_role)) > 0),
    next_review_date    DATE NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_normative_requirements_review ON normative_requirements(next_review_date, status);
```

---

## 3. Neues Modul `crates/kis-catalogs/` & `config/module_tables.toml`

Gemäß [MODULE.md](../MODULE.md) hängt `crates/kis-catalogs` ausschließlich von `kis-core` ab und erhält einen eigenen Eintrag in `config/module_tables.toml`:

```toml
# Ausschnitt für config/module_tables.toml bei Umsetzung von EP-06a
[modules.catalogs]
crate = "kis-catalogs"
tables = [
    "catalog_sources",
    "catalog_versions",
    "catalog_entries",
    "catalog_mappings",
    "normative_requirements",
]
```

**`ModuleInfo`:**
- `id: "catalogs"`
- `depends_on: &["org"]`
- `maturity: Maturity::Experimental` (erst nach bestandenen `G0`–`G6`-Nachweisen auf `Stable` anheben)
- `kernel: false` (lesende Stichtags-Abfragen für andere Module laufen über klar abgegrenzte Kern-Abfrage oder dokumentierte Lese-Schnittstelle; Schreibhoheit liegt exklusiv bei `kis-catalogs` und `kis-admin`).

---

## 4. Kern-Werttypen für Zeit, Einheiten & Befundstatus (`EP-06b` in `kis-core/src/clinical_types.rs`)

Setzt [ENTERPRISE_KIS.md](../ENTERPRISE_KIS.md) § 4 („Zeit, Einheiten und Befundstatus“) verbindlich im Typsystem um:

```rust
// Verbindlicher Zielvertrag für crates/kis-core/src/clinical_types.rs (EP-06b)

/// Verbindlicher klinischer Zeitstempel-Vertrag:
/// - Speicherung immer in UTC + expliziter IANA-Zeitzone der Einrichtung (z. B. "Europe/Berlin")
/// - Klare Trennung von Ereigniszeit, Erfassungszeit und Freigabezeit
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClinicalTimestamp {
    /// Wann das klinische Ereignis stattfand (UTC)
    pub event_time_utc: chrono::DateTime<chrono::Utc>,
    /// Lokale IANA-Zeitzone am Entstehungsort (z. B. "Europe/Berlin")
    pub local_timezone: String,
    /// Wann der Wert im System dokumentiert wurde (UTC)
    pub recorded_time_utc: chrono::DateTime<chrono::Utc>,
    /// Wann der Wert fachlich freigegeben wurde (None solange preliminary)
    pub released_time_utc: Option<chrono::DateTime<chrono::Utc>>,
}

/// Verbindlicher Einheiten-Vertrag: Originaleinheit + normierter UCUM-Code.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeasuredQuantity {
    pub value: f64,
    pub original_unit: String,
    /// Normierter UCUM-Code (z. B. "mmol/L", "mg/dL", "mm[Hg]")
    pub ucum_code: String,
    pub normalized_value: Option<f64>,
    pub normalized_ucum_code: Option<String>,
}

/// Verbindliche Befund-Statusmaschine — keine stille Gleichsetzung!
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClinicalResultStatus {
    Preliminary,
    Final,
    Corrected,
    Cancelled,
}
```

---

## 5. CLI-Import- & Prüfvertrag (`kis-admin`, `EP-06a`)

```bash
# Beispiel-Schnittstelle für EP-06a (synthetischer Katalog-Import & Normenprüfung)
kis-admin import-catalog \
  --source SYNTH-ICD10 \
  --version 2026-SYNTH \
  --valid-from 2026-01-01 \
  --valid-until 2026-12-31 \
  --file tests/fixtures/synth_icd10_2026.json

kis-admin check-normative --fail-on-expired-catalogs
```

---

## 6. Kanonisches JSON-Austauschformat für Katalog-Import & Normen-Seed

Der maschinenlesbare Seed-Katalog für `normative_requirements` liegt unter [normative_requirements_seed.json](normative_requirements_seed.json). Beispiel eines synthetischen Katalog-Importpakets (nur `SYNTHETIC_TEST_ONLY` für CI-Tests):

```json
{
  "source_code": "SYNTH_ICD10_TEST",
  "title": "Synthetischer Testkatalog ICD (NUR CI-TESTDATEN)",
  "publisher": "KIS Test Harness (Synthetic)",
  "license_kind": "synthetic_test_only",
  "license_reference": "CI-SYNTHETIC-NO-CLINICAL-USE",
  "version_label": "2026-synth-1",
  "valid_from": "2026-01-01",
  "valid_until": "2026-12-31",
  "is_synthetic": true,
  "entries": [
    {
      "code": "SYNTH-A01",
      "display_de": "Synthetische Testdiagnose Alpha",
      "is_billable": true
    },
    {
      "code": "SYNTH-B02",
      "display_de": "Synthetische Testdiagnose Beta",
      "is_billable": true
    }
  ]
}
```
