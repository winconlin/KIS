# M01-Umsetzungsvertrag: `EP-01` — Mandanten, Rechtsträger, Standorte, MVZ & Scoped Config

Stand: 25.11.2026 · Autor: `google-neon` (Architektur & Koordination) · Basis: [ADR-001](../EP00_ARCHITEKTUR_UND_ANALYSE.md), [ROADMAP.md](../ROADMAP.md) (`EP-01a`, `EP-01b`, `EP-01c`), [VERBUND_TESTMODELL.md](../VERBUND_TESTMODELL.md) · Status: **Spezifiziert (Umsetzung startet erst nach `EP-00`-`G6`-Freigabe und exklusiver Reservierung von Migration `012`)**

> **Verbindlicher Hinweis:** Dieses Dokument definiert den technischen Schnittstellen-, Schema- und Rust-Kernvertrag für `EP-01a`–`EP-01c`, damit Implementierer ohne Rückfragen oder Schema-Drift arbeiten können. Die eigentliche Migration `migrations/012_mandanten_organisation.sql` wird erst im `EP-01a`-Branch gegen eine echte PostgreSQL-16-Instanz angelegt, getestet und per `python3 scripts/check_guardrails.py --add-migration` in `migrations/CHECKSUMS` aufgenommen.

---

## 1. Paketabgrenzung (`EP-01a`, `EP-01b`, `EP-01c`)

| Teilpaket | Exklusiver Schreibbereich | Ziel | Pflicht-Nachweise |
|---|---|---|---|
| **`EP-01a`** | `migrations/012_mandanten_organisation.sql`, `migrations/CHECKSUMS`, `config/module_tables.toml`, `crates/kis-core/src/tenant.rs` (neu), `crates/kis-core/src/lib.rs`, `crates/kis-core/src/identity.rs` | DB-Schema für Rechtsträger/Standorte/Kennungen/Kanten + `TenantContext`-Kernvertrag + Bootstrap-Rückwärtskompatibilität für bestehende Tests | `NVT-01`, `NVT-02`; alle 17 bestehenden PG-Tests weiter grün; `G0`–`G6` |
| **`EP-01b`** | `crates/kis-org/src/lib.rs`, `crates/kis-app/tests/` | Mandantenfähige Org-API, MVZ-Kennungen, `devices`, `user_org_assignments`, `service_agreements` | `NVT-01`, `NVT-02`, `NVT-09` (Vorstufe); `G0`–`G6` |
| **`EP-01c`** | `crates/kis-core/src/platform.rs`, `crates/kis-app/src/admin_platform.rs`, `crates/kis-app/tests/` | Hierarchische Einstellungen (`verbund_default → legal_entity → site → org_unit`) & unveränderlicher `PolicyFloor` (`security.treatment_context.enforce` nicht mehr abschaltbar!) | `NVT-04`; `G0`–`G6` |

---

## 2. Schema-Entwurf: Migration `012_mandanten_organisation.sql`

Kompatibilitätsanforderung zu `migrations/005_plattform.sql` und `006_faelle.sql`:
- Bestehende `kind`-Werte in `org_units` (`traeger`, `einrichtung`, `fachabteilung`, `station`, `funktionsbereich`, `ambulanz`, `verwaltung`, `versorgung`, `sonstige`) **müssen erhalten bleiben**, da `rooms(ward_id, ward_kind)` und `cases(department_id, department_kind)` per zusammengesetztem Fremdschlüssel `(id, kind)` darauf verweisen.
- Neue `kind`-Werte ergänzen die `CHECK`-Constraint um `mvz_hauptbetriebsstaette`, `mvz_nebenbetriebsstaette`, `praxis`, `virtuelle_einheit`, `kostenstelle`.
- Bereits vorhandene Zeilen in `org_units` erhalten bei Migration `012` einen deterministischen Default-Rechtsträger (`00000000-0000-0000-0000-000000000001`, Code `DEFAULT-RT`), damit bestehende Einzelhaus-Tests weiterlaufen; für alle neuen Verbund-Anlagen wird `legal_entity_id NOT NULL` erzwungen.

```sql
-- Entwurf für EP-01a: migrations/012_mandanten_organisation.sql
-- (erst bei EP-01a gegen PostgreSQL 16 ausführen und testen)

-- 1. Optionaler Konzern-/Verbundrahmen (gewährt NIEMALS selbst Datenzugriff!)
CREATE TABLE organization_groups (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code        TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Za-z0-9_.-]{2,32}$'),
    name        TEXT NOT NULL CHECK (length(trim(name)) > 0),
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    version     INTEGER NOT NULL DEFAULT 1
);

-- 2. Rechtlicher Verantwortlicher / Mandant (Datenschutz- und Zugriffsgrenze)
CREATE TABLE legal_entities (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id                UUID REFERENCES organization_groups(id),
    code                    TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Za-z0-9_.-]{2,32}$'),
    legal_name              TEXT NOT NULL CHECK (length(trim(legal_name)) > 0),
    operating_company_name  TEXT,
    legal_form              TEXT,
    dpo_contact             TEXT,
    federal_state           TEXT CHECK (federal_state IS NULL OR federal_state ~ '^DE-[A-Z]{2}$'),
    default_timezone        TEXT NOT NULL DEFAULT 'Europe/Berlin',
    active                  BOOLEAN NOT NULL DEFAULT TRUE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    version                 INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_legal_entities_group ON legal_entities(group_id) WHERE active;

-- Bootstrap-Mandant nur für Rückwärtskompatibilität bestehender Einzelhaus-Migrationen
INSERT INTO legal_entities (id, code, legal_name)
VALUES ('00000000-0000-0000-0000-000000000001', 'DEFAULT-RT', 'Standard-Rechtsträger (Migration 012)')
ON CONFLICT (id) DO NOTHING;

-- 3. Standorte / Betriebsstätten (immer genau einem Rechtsträger zugeordnet)
CREATE TABLE sites (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    code            TEXT NOT NULL CHECK (code ~ '^[A-Za-z0-9_.-]{2,32}$'),
    name            TEXT NOT NULL CHECK (length(trim(name)) > 0),
    sector          TEXT NOT NULL CHECK (sector IN ('inpatient', 'mvz', 'practice', 'central_service', 'mixed')),
    street          TEXT,
    postal_code     TEXT,
    city            TEXT,
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    version         INTEGER NOT NULL DEFAULT 1,
    UNIQUE (legal_entity_id, code),
    UNIQUE (id, legal_entity_id)
);
CREATE INDEX idx_sites_legal_entity ON sites(legal_entity_id) WHERE active;

-- 4. org_units mandantenfähig erweitern (bestehende FKs (id, kind) bleiben intakt)
ALTER TABLE org_units
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN site_id         UUID;

UPDATE org_units
SET legal_entity_id = '00000000-0000-0000-0000-000000000001'
WHERE legal_entity_id IS NULL;

ALTER TABLE org_units
    ALTER COLUMN legal_entity_id SET NOT NULL;

ALTER TABLE org_units
    ADD CONSTRAINT org_units_id_tenant_unique UNIQUE (id, legal_entity_id),
    ADD CONSTRAINT org_units_site_same_tenant_fk
        FOREIGN KEY (site_id, legal_entity_id) REFERENCES sites(id, legal_entity_id);

-- Bestehende kind-Constraint erweitern (Name in PG prüfen: org_units_kind_check)
ALTER TABLE org_units DROP CONSTRAINT org_units_kind_check;
ALTER TABLE org_units ADD CONSTRAINT org_units_kind_check
    CHECK (kind IN (
        'traeger', 'einrichtung', 'fachabteilung', 'station',
        'funktionsbereich', 'ambulanz', 'verwaltung', 'versorgung', 'sonstige',
        'mvz_hauptbetriebsstaette', 'mvz_nebenbetriebsstaette',
        'praxis', 'virtuelle_einheit', 'kostenstelle'
    ));

-- Code-Eindeutigkeit bleibt optional global oder je Rechtsträger (für Abwärtskompatibilität bestehende UNIQUE-Index behalten, zusätzlichen Index für Mandant anlegen)
CREATE INDEX idx_org_units_tenant ON org_units(legal_entity_id, kind) WHERE active;
CREATE INDEX idx_org_units_site   ON org_units(site_id) WHERE site_id IS NOT NULL;

-- 5. Zeitgebundene, zyklenfreie Hierarchiekanten innerhalb DESSSELBEN Rechtsträgers
CREATE TABLE org_unit_edges (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    parent_unit_id  UUID NOT NULL,
    child_unit_id   UUID NOT NULL,
    relation_kind   TEXT NOT NULL DEFAULT 'hierarchy'
                    CHECK (relation_kind IN ('hierarchy', 'cost_allocation', 'functional_reporting')),
    valid_from      TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until     TIMESTAMPTZ,
    created_by      UUID NOT NULL REFERENCES users(id),
    CHECK (parent_unit_id <> child_unit_id),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    FOREIGN KEY (parent_unit_id, legal_entity_id) REFERENCES org_units(id, legal_entity_id),
    FOREIGN KEY (child_unit_id,  legal_entity_id) REFERENCES org_units(id, legal_entity_id)
);
CREATE INDEX idx_org_unit_edges_parent ON org_unit_edges(legal_entity_id, parent_unit_id);
CREATE INDEX idx_org_unit_edges_child  ON org_unit_edges(legal_entity_id, child_unit_id);

-- Trigger: Zyklenfreiheit & Mandantengrenze sowohl für org_units.parent_id als auch org_unit_edges
CREATE OR REPLACE FUNCTION kis_check_org_unit_parent() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    parent_le UUID;
    has_cycle BOOLEAN;
BEGIN
    IF NEW.parent_id IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT legal_entity_id INTO parent_le FROM org_units WHERE id = NEW.parent_id;
    IF parent_le IS DISTINCT FROM NEW.legal_entity_id THEN
        RAISE EXCEPTION 'org_units.parent_id darf keine Rechtsträgergrenze überschreiten'
            USING ERRCODE = 'check_violation';
    END IF;
    WITH RECURSIVE anc AS (
        SELECT parent_id FROM org_units WHERE id = NEW.parent_id
        UNION ALL
        SELECT o.parent_id FROM org_units o JOIN anc a ON o.id = a.parent_id WHERE o.parent_id IS NOT NULL
    )
    SELECT EXISTS (SELECT 1 FROM anc WHERE parent_id = NEW.id) INTO has_cycle;
    IF has_cycle THEN
        RAISE EXCEPTION 'Zyklus in org_units-Hierarchie verweigert'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER org_units_parent_guard
    BEFORE INSERT OR UPDATE OF parent_id, legal_entity_id ON org_units
    FOR EACH ROW EXECUTE FUNCTION kis_check_org_unit_parent();

-- 6. Versionierte institutionelle & berufsrechtliche Kennungen
CREATE TABLE org_identifiers (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id   UUID NOT NULL REFERENCES legal_entities(id),
    site_id           UUID,
    org_unit_id       UUID,
    identifier_type   TEXT NOT NULL CHECK (identifier_type IN (
        'IK', 'STANDORTNUMMER_293', 'BSNR', 'NBSNR', 'LANR',
        'TELEMATIK_ID', 'UMSATZSTEUER_ID', 'REGISTER_ID'
    )),
    identifier_value  TEXT NOT NULL CHECK (length(trim(identifier_value)) > 0),
    issuing_authority TEXT,
    valid_from        DATE NOT NULL,
    valid_until       DATE,
    created_by        UUID NOT NULL REFERENCES users(id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (valid_until IS NULL OR valid_until >= valid_from),
    FOREIGN KEY (site_id, legal_entity_id) REFERENCES sites(id, legal_entity_id),
    FOREIGN KEY (org_unit_id, legal_entity_id) REFERENCES org_units(id, legal_entity_id),
    -- Formatprüfungen für amtliche deutsche Kennungen (ohne erfundene Prüfziffern-Abkürzungen)
    CHECK (identifier_type <> 'IK' OR identifier_value ~ '^[0-9]{9}$'),
    CHECK (identifier_type <> 'STANDORTNUMMER_293' OR identifier_value ~ '^[0-9]{9}$'),
    CHECK (identifier_type NOT IN ('BSNR', 'NBSNR', 'LANR') OR identifier_value ~ '^[0-9]{9}$')
);
CREATE INDEX idx_org_identifiers_lookup
    ON org_identifiers(legal_entity_id, identifier_type, identifier_value);

-- 7. Medizinische & technische Geräte (Ressourcen je Standort/Einheit)
CREATE TABLE devices (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    site_id         UUID NOT NULL,
    org_unit_id     UUID,
    room_id         UUID REFERENCES rooms(id),
    code            TEXT NOT NULL CHECK (code ~ '^[A-Za-z0-9_.-]{1,64}$'),
    name            TEXT NOT NULL CHECK (length(trim(name)) > 0),
    device_kind     TEXT NOT NULL CHECK (device_kind IN (
        'modality', 'poct', 'lab_analyzer', 'monitor', 'infusion_pump',
        'card_terminal', 'scanner', 'printer', 'other'
    )),
    serial_number   TEXT,
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    version         INTEGER NOT NULL DEFAULT 1,
    UNIQUE (legal_entity_id, code),
    FOREIGN KEY (site_id, legal_entity_id) REFERENCES sites(id, legal_entity_id),
    FOREIGN KEY (org_unit_id, legal_entity_id) REFERENCES org_units(id, legal_entity_id)
);

-- 8. Mehrfach-Arbeitsverhältnisse / zeitgebundene Mandanten- & Rollenzuordnungen
CREATE TABLE user_org_assignments (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    site_id         UUID,
    org_unit_id     UUID,
    role_key        TEXT NOT NULL REFERENCES roles(key),
    assignment_kind TEXT NOT NULL DEFAULT 'employment'
                    CHECK (assignment_kind IN ('employment', 'rotation', 'deputy', 'on_call', 'consulting', 'central_service')),
    valid_from      TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until     TIMESTAMPTZ,
    assigned_by     UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    FOREIGN KEY (site_id, legal_entity_id) REFERENCES sites(id, legal_entity_id),
    FOREIGN KEY (org_unit_id, legal_entity_id) REFERENCES org_units(id, legal_entity_id)
);
CREATE INDEX idx_user_org_assignments_active
    ON user_org_assignments(user_id, legal_entity_id) WHERE valid_until IS NULL;

-- 9. Zentrale Dienste / Leistungs- & AVV-Verträge zwischen Rechtsträgern
CREATE TABLE service_agreements (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    provider_unit_id         UUID NOT NULL,
    consumer_legal_entity_id UUID NOT NULL REFERENCES legal_entities(id),
    consumer_site_id         UUID,
    service_domain           TEXT NOT NULL CHECK (service_domain IN (
        'central_lab', 'central_radiology', 'central_pharmacy',
        'central_it', 'central_billing', 'central_aemp', 'central_archive', 'other'
    )),
    allowed_purposes         TEXT[] NOT NULL CHECK (cardinality(allowed_purposes) > 0),
    avv_reference            TEXT NOT NULL CHECK (length(trim(avv_reference)) >= 3),
    valid_from               TIMESTAMPTZ NOT NULL,
    valid_until              TIMESTAMPTZ,
    active                   BOOLEAN NOT NULL DEFAULT TRUE,
    created_by               UUID NOT NULL REFERENCES users(id),
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (provider_legal_entity_id <> consumer_legal_entity_id),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    FOREIGN KEY (provider_unit_id, provider_legal_entity_id) REFERENCES org_units(id, legal_entity_id),
    FOREIGN KEY (consumer_site_id, consumer_legal_entity_id) REFERENCES sites(id, legal_entity_id)
);
CREATE INDEX idx_service_agreements_active
    ON service_agreements(provider_legal_entity_id, consumer_legal_entity_id, service_domain)
    WHERE active;

-- 10. Hierarchische Einstellungen (EP-01c) mit Vererbung verbund -> legal_entity -> site -> org_unit
CREATE TABLE scoped_settings (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_level     TEXT NOT NULL CHECK (scope_level IN ('verbund', 'legal_entity', 'site', 'org_unit')),
    legal_entity_id UUID REFERENCES legal_entities(id),
    site_id         UUID,
    org_unit_id     UUID,
    setting_key     TEXT NOT NULL CHECK (setting_key ~ '^[a-z_]+(\.[a-z_]+)+$'),
    value           JSONB NOT NULL,
    valid_from      TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until     TIMESTAMPTZ,
    updated_by      UUID NOT NULL REFERENCES users(id),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    CHECK (
        (scope_level = 'verbund'      AND legal_entity_id IS NULL     AND site_id IS NULL     AND org_unit_id IS NULL) OR
        (scope_level = 'legal_entity' AND legal_entity_id IS NOT NULL AND site_id IS NULL     AND org_unit_id IS NULL) OR
        (scope_level = 'site'         AND legal_entity_id IS NOT NULL AND site_id IS NOT NULL AND org_unit_id IS NULL) OR
        (scope_level = 'org_unit'     AND legal_entity_id IS NOT NULL AND org_unit_id IS NOT NULL)
    ),
    FOREIGN KEY (site_id, legal_entity_id) REFERENCES sites(id, legal_entity_id),
    FOREIGN KEY (org_unit_id, legal_entity_id) REFERENCES org_units(id, legal_entity_id)
);
CREATE UNIQUE INDEX idx_scoped_settings_active_unique
    ON scoped_settings (
        scope_level,
        COALESCE(legal_entity_id, '00000000-0000-0000-0000-000000000000'::uuid),
        COALESCE(site_id,         '00000000-0000-0000-0000-000000000000'::uuid),
        COALESCE(org_unit_id,     '00000000-0000-0000-0000-000000000000'::uuid),
        setting_key
    )
    WHERE valid_until IS NULL;
```

---

## 3. Rust-Kernvertrag (`kis-core/src/tenant.rs` & `platform.rs`)

### 3.1 `TenantContext` (Pflichtbestandteil von `AuthUser` ab `EP-01a`)

- Jede authentifizierte Anfrage führt einen serverseitig validierten `TenantContext` (niemals blind aus einem Client-Header übernommen!):
  - Auswahl der aktiven `legal_entity_id` über Sitzungskontext (`sessions.active_legal_entity_id`) oder expliziten Kontextwechsel-Endpunkt `POST /api/v1/session/context` — serverseitig gegen `user_org_assignments` auf `db.write()` geprüft.
  - Fehlt eine gültige Zuordnung zum angeforderten Rechtsträger, antwortet der Server mit `403 tenant_mismatch` (kein stiller Fallback auf einen Standard-Mandanten!).

```rust
// Verbindliche Struktur für crates/kis-core/src/tenant.rs (EP-01a)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PurposeOfUse {
    Treatment,
    EmergencyBreakGlass,
    Billing,
    QualityAssurance,
    ResearchPseudonymized,
    Hr,
    Finance,
    Audit,
    SystemAdmin,
    PatientRights,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TenantContext {
    pub legal_entity_id: uuid::Uuid,
    pub site_id: Option<uuid::Uuid>,
    pub purpose: PurposeOfUse,
    /// Aktive Service-Agreements, über die diese Sitzung im Auftrag eines anderen Rechtsträgers handeln darf
    pub active_service_agreement_id: Option<uuid::Uuid>,
}
```

### 3.2 `PolicyFloor` in `kis-core/src/platform.rs` (`EP-01c`)

- Jede `SettingDef` erhält eine verbindliche Schutzklasse `SettingPolicyClass`:
  - `PolicyFloor`: rechtliche Mindestgrenze — auf **keiner** Ebene (`verbund`, `legal_entity`, `site`, `org_unit`) unter den gesetzlichen/sicherheitskritischen Boden absenkbar!
  - `Configurable`: innerhalb der definierten Grenzen je Ebene überschreibbar (`org_unit` überschreibt `site` überschreibt `legal_entity` überschreibt `verbund_default`).
- Konkrete Bodenwerte (`NVT-04`):
  - `security.treatment_context.enforce`: **immer `true`** (`PolicyFloor`, Versuch `false` zu schreiben → `400 policy_floor_violation` + Audit-Eintrag).
  - `security.password.min_length`: Boden `>= 12` (Verbund oder Mandant darf nur strenger, z. B. `16`, nicht lockerer stellen).
  - `security.session.inactivity_minutes`: Obergrenze `<= 60` im klinischen Betrieb (Ebene darunter darf kürzer, z. B. `15`, nicht beliebig lang stellen).
  - `security.audit.read_audit_enforce`: **immer `true`** (`PolicyFloor`).

---

## 4. `config/module_tables.toml`-Erweiterung (Kern-Tabellen)

Alle in Migration `012` eingeführten Tabellen gehören zum **Kern** (`[core].tables`), da sie den gemeinsamen Mandanten- und Organisationsvertrag für sämtliche Module darstellen:

```toml
# Ausschnitt der in EP-01a unter [core] in config/module_tables.toml zu ergänzenden Kern-Tabellen
[core]
tables_ep01_additions = [
    "organization_groups",
    "legal_entities",
    "sites",
    "org_unit_edges",
    "org_identifiers",
    "devices",
    "user_org_assignments",
    "service_agreements",
    "scoped_settings",
]
```

*(Hinweis: Beim tatsächlichen Eintrag in `config/module_tables.toml` werden diese Namen in das bestehende Array `tables = [...]` unter `[core]` aufgenommen und per `python3 scripts/check_modules.py` verifiziert.)*

---

## 5. REST-API-Vertrag (`EP-01b` / `EP-01c`)

Alle Routen laufen über `ModuleRoutes` mit explizitem `Access` (kein `Scope::None` ohne `TenantContext`-Filterung!):

| Methode & Pfad | `Access` | Mandantenbindung | Zweck |
|---|---|---|---|
| `GET /api/v1/legal-entities` | `Permission(OrgRead)` | Nur zugeordnete Rechtsträger (oder `PlatformAdmin` für Metadaten ohne Klinikdaten) | Liste eigener Rechtsträger |
| `POST /api/v1/legal-entities` | `Permission(TenantManage)` | `PlatformAdmin` | Neuen Rechtsträger anlegen (mit Audit) |
| `GET /api/v1/sites` | `Permission(OrgRead)` | `WHERE legal_entity_id = $tenant` | Standorte/MVZ des aktiven Mandanten |
| `POST /api/v1/sites` | `Permission(OrgWrite)` | `legal_entity_id = $tenant` | Standort/Betriebsstätte anlegen |
| `GET /api/v1/org/units` | `Permission(OrgRead)` | `WHERE legal_entity_id = $tenant` | Einheitenbaum strikt innerhalb des aktiven Mandanten (`NVT-01`) |
| `POST /api/v1/org/identifiers` | `Permission(OrgWrite)` | `legal_entity_id = $tenant` | Versionierte IK/BSNR/NBSNR/LANR/Telematik-ID anlegen |
| `GET/POST /api/v1/devices` | `Permission(OrgRead)` / `OrgWrite` | `WHERE legal_entity_id = $tenant` | Geräte je Standort/Einheit |
| `POST /api/v1/service-agreements` | `Permission(TenantManage)` | Vier-Augen-/Audit-Pflicht beider Rechtsträger | Zentraldienst-Vertrag anlegen/beenden |
| `POST /api/v1/session/context` | `Authenticated` | Server-Prüfung gegen `user_org_assignments` | Aktiven Rechtsträger-/Standortkontext wechseln |
| `GET/PUT /api/v1/admin/settings` | `Permission(ConfigRead)` / `ConfigWrite` | Scoped auf Ebene des Administrators | Hierarchische Einstellungen inkl. `PolicyFloor`-Prüfung (`NVT-04`) |

---

## 6. Pflicht-Abnahmetests (`G2`/`G3`) für `EP-01`

Auf Basis der kanonischen Test-Fixture [verbund_test_fixture.json](verbund_test_fixture.json) (exakt nach [VERBUND_TESTMODELL.md](../VERBUND_TESTMODELL.md)):
1. **`NVT-01` (Rechtsträgergrenze):** `U-ADM-N` und `U-DOC-N1` (aktiv auf `RT-NORD`) fragen `GET /api/v1/org/units` und `GET /api/v1/sites` ab → enthält ausschließlich `RT-NORD`-Einheiten; direkter Zugriff auf `INN-S1` (`KL-S1`, `RT-SUED`) oder Kontextwechsel auf `RT-SUED` liefert `403`.
2. **`NVT-02` (Träger-Vererbungssperre):** Versuch, eine `org_units.parent_id`- oder `org_unit_edges`-Kante zwischen `RT-NORD` und `RT-SUED` oder einen Zyklus `A → B → A` anzulegen → PostgreSQL lehnt über Trigger ab (`check_violation`); Zuordnung auf `organization_groups` gewährt **keinen** Einheiten- oder Fallzugriff.
3. **`NVT-04` (Policy-Floor):** `U-PLAT` oder `U-ADM-N` versucht `security.treatment_context.enforce = false` oder `password.min_length = 6` auf `RT-NORD` zu setzen → `400 policy_floor_violation` + Audit-Eintrag; der Wert bleibt wirksam `true` / `>= 12`.
4. **Rückwärtskompatibilität:** Alle bestehenden Tests in `crates/kis-app/tests/` laufen gegen PostgreSQL 16 mit Migration `001`–`012` fehlerfrei durch.
