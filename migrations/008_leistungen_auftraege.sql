-- ============================================================
-- KIS — Migration 008: Leistungskatalog, Aufträge, Automatik (Modul `orders`),
-- Labor (Modul `lab`)
--
-- Ein Auftragswesen für das ganze Haus. Jede Leistung, die ein Bereich
-- für einen anderen erbringt, ist ein Katalogeintrag: Labor, Röntgen,
-- EKG, Konsil, Physiotherapie, Patiententransport, Bettenaufbereitung,
-- Sonderkost, Störmeldung an die Technik, Sozialdienst …
--
--   * service_catalog   Leistungskatalog (vom Haus gepflegt)
--   * orders            Auftrag mit Status, Arbeitsliste je Einheit
--   * automation_rules  Ereignis → Auftrag (z. B. Entlassung →
--                       Bettenaufbereitung), vom Haus konfigurierbar
--   * lab_specimens / lab_results   Proben und Einzelwerte
-- ============================================================

CREATE TABLE service_catalog (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code             TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Z0-9_.-]{1,32}$'),
    name             TEXT NOT NULL CHECK (length(trim(name)) > 0),
    -- Freie Gruppierung für Oberfläche und Auswertung,
    -- z. B. labor, bildgebung, konsil, transport, reinigung, technik
    category         TEXT NOT NULL CHECK (category ~ '^[a-z][a-z0-9_]{1,31}$'),
    -- Erbringende Einheit (Arbeitsliste, in der der Auftrag erscheint)
    provider_unit_id UUID NOT NULL REFERENCES org_units(id),
    -- Muss der Auftrag einem Fall zugeordnet sein?
    requires_case    BOOLEAN NOT NULL DEFAULT TRUE,
    -- Verlangt der Abschluss einen Befund-/Ergebnistext?
    requires_result  BOOLEAN NOT NULL DEFAULT FALSE,
    default_priority TEXT NOT NULL DEFAULT 'routine'
                     CHECK (default_priority IN ('routine', 'dringend', 'notfall')),
    active           BOOLEAN NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    version          INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_service_catalog_provider ON service_catalog(provider_unit_id) WHERE active;

CREATE TABLE orders (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    service_id         UUID NOT NULL REFERENCES service_catalog(id),
    -- Fallbezug (leer bei rein betrieblichen Aufträgen, z. B. Störmeldung)
    case_id            UUID REFERENCES cases(id) ON DELETE CASCADE,
    requesting_unit_id UUID REFERENCES org_units(id),
    provider_unit_id   UUID NOT NULL REFERENCES org_units(id),
    -- Ortsbezug: betroffenes Bett (Aufbereitung, Transport)
    bed_id             UUID REFERENCES beds(id),
    -- Erledigung gibt das gesperrte Bett wieder frei
    releases_bed       BOOLEAN NOT NULL DEFAULT FALSE,
    -- Fragestellung / Auftragstext
    details            TEXT,
    priority           TEXT NOT NULL DEFAULT 'routine' CHECK (priority IN ('routine', 'dringend', 'notfall')),
    status             TEXT NOT NULL DEFAULT 'angefordert'
                       CHECK (status IN ('angefordert', 'angenommen', 'erledigt', 'storniert')),
    scheduled_for      TIMESTAMPTZ,
    -- Herkunft: Benutzer oder Automatik-Regel
    requested_by       UUID REFERENCES users(id),
    automation_rule_id UUID,
    requested_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    accepted_by        UUID REFERENCES users(id),
    accepted_at        TIMESTAMPTZ,
    result_text        TEXT,
    completed_by       UUID REFERENCES users(id),
    completed_at       TIMESTAMPTZ,
    cancelled_reason   TEXT,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    version            INTEGER NOT NULL DEFAULT 1,
    CONSTRAINT orders_origin CHECK (requested_by IS NOT NULL OR automation_rule_id IS NOT NULL),
    CONSTRAINT orders_completed_consistent
        CHECK ((status = 'erledigt') = (completed_at IS NOT NULL)),
    CONSTRAINT orders_completed_author
        CHECK ((completed_at IS NULL) = (completed_by IS NULL)),
    CONSTRAINT orders_accepted_author
        CHECK ((accepted_at IS NULL) = (accepted_by IS NULL)),
    CONSTRAINT orders_cancel_reason
        CHECK (status <> 'storniert' OR length(trim(coalesce(cancelled_reason, ''))) > 0),
    CONSTRAINT orders_release_needs_bed CHECK (NOT releases_bed OR bed_id IS NOT NULL)
);
CREATE INDEX idx_orders_case ON orders(case_id, requested_at) WHERE case_id IS NOT NULL;
CREATE INDEX idx_orders_worklist ON orders(provider_unit_id, requested_at)
    WHERE status IN ('angefordert', 'angenommen');
CREATE INDEX idx_orders_requester ON orders(requesting_unit_id, requested_at);

-- ------------------------------------------------------------
-- Automatik: bei einem Fachereignis einen Auftrag anlegen.
-- Beispiel: on_topic = 'case.discharged', Leistung „Bettenaufbereitung“,
-- block_bed = TRUE → das frei gewordene Bett bleibt gesperrt, bis die
-- Reinigung den Auftrag erledigt.
-- ------------------------------------------------------------
CREATE TABLE automation_rules (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL CHECK (length(trim(name)) > 0),
    on_topic   TEXT NOT NULL
               CHECK (on_topic IN ('case.admitted', 'case.transferred', 'case.discharged')),
    service_id UUID NOT NULL REFERENCES service_catalog(id),
    -- Nur für diese Fallarten (leer = alle)
    case_kinds TEXT[] NOT NULL DEFAULT '{}',
    -- Betrifft das Bett, das durch das Ereignis frei wird, und sperrt es
    block_bed  BOOLEAN NOT NULL DEFAULT FALSE,
    details    TEXT,
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    updated_by UUID NOT NULL REFERENCES users(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (NOT block_bed OR on_topic IN ('case.transferred', 'case.discharged'))
);
CREATE INDEX idx_automation_rules_topic ON automation_rules(on_topic) WHERE active;

ALTER TABLE orders ADD CONSTRAINT orders_automation_rule_fk
    FOREIGN KEY (automation_rule_id) REFERENCES automation_rules(id) ON DELETE SET NULL;

-- ------------------------------------------------------------
-- Labor: Probe und Einzelwerte. Werte kommen in der Regel über eine
-- Schnittstelle (Laborsystem); external_id verhindert Doppelimporte.
-- ------------------------------------------------------------
CREATE TABLE lab_specimens (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    case_id      UUID NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL DEFAULT 'labor' CHECK (kind IN ('labor', 'bga', 'mikrobiologie')),
    material     TEXT,
    collected_at TIMESTAMPTZ NOT NULL,
    external_id  TEXT UNIQUE,
    recorded_by  UUID NOT NULL REFERENCES users(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_lab_specimens_case ON lab_specimens(case_id, collected_at);

CREATE TABLE lab_results (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    specimen_id  UUID NOT NULL REFERENCES lab_specimens(id) ON DELETE CASCADE,
    -- Hauseigenes Analytkürzel (Großbuchstaben, Ziffern, _), z. B. HB, KREA
    analyte_code TEXT NOT NULL CHECK (analyte_code ~ '^[A-Z0-9_]{1,32}$'),
    loinc        TEXT CHECK (loinc ~ '^[0-9]{1,7}-[0-9]$'),
    name         TEXT,
    value_num    DOUBLE PRECISION,
    value_text   TEXT,
    unit         TEXT,
    ref_low      DOUBLE PRECISION,
    ref_high     DOUBLE PRECISION,
    ref_text     TEXT,
    -- Bewertung des Labors: N normal, H/L erhöht/erniedrigt, HH/LL kritisch, A auffällig
    flag         TEXT CHECK (flag IN ('N', 'H', 'L', 'HH', 'LL', 'A')),
    status       TEXT NOT NULL DEFAULT 'final' CHECK (status IN ('vorlaeufig', 'final', 'korrigiert')),
    comment      TEXT,
    UNIQUE (specimen_id, analyte_code),
    CHECK (value_num IS NOT NULL OR value_text IS NOT NULL),
    CHECK (ref_low IS NULL OR ref_high IS NULL OR ref_low <= ref_high)
);
CREATE INDEX idx_lab_results_analyte ON lab_results(analyte_code, specimen_id);
