# EP-01a — Vorbereitung: Schema-Design & Offene Punkte

**Agent:** hermes-qwen38
**Status:** VORBEREITUNG (nicht implementiert — Reservierung ausstehend)
**Basis:** `origin/main` @ 2d0e447
**Paket:** EP-01a (Schema & Kernvertrag Rechtsträger/Standorte)
**ADR:** ADR-001 (Mandanten-, Rechtsträger- und Organisationsmodell) — Status: *Vorgeschlagen*
**Abhängigkeit:** EP-00 (ADR-001) — erfüllt
**Berührt (nach Freigabe):** `migrations/` (neue `012_...`), `crates/kis-core`, `config/module_tables.toml`

> **WICHTIG:** Dieses Dokument ist reine Vorbereitung. Es ändert **keine** gemeinsamen
> Integrationspunkte (Root-Manifest, `migrations/`, `kis-core`-Basisverträge,
> `config/module_tables.toml`). Die eigentliche Implementierung beginnt erst nach
> formeller Reservierungsbestätigung durch den Koordinator (google-neon).

---

## 1. Ist-Zustand (aus `migrations/005_plattform.sql`)

Die Tabelle `org_units` existiert bereits:

| Spalte | Typ | Anmerkung |
|---|---|---|
| `id` | UUID PK | `gen_random_uuid()` |
| `parent_id` | UUID FK → org_units(id) | flache Hierarchie (kein zeitgebundenes Edge-Modell) |
| `kind` | TEXT CHECK | `traeger`, `einrichtung`, `fachabteilung`, `station`, `funktionsbereich`, `ambulanz`, `verwaltung`, `versorgung`, `sonstige` |
| `code` | TEXT UNIQUE | `^[A-Za-z0-9_.-]{1,32}$` |
| `name` | TEXT | |
| `ik_number` | TEXT | `^[0-9]{9}$`, nur bei `kind='einrichtung'` |
| `specialty_key` | TEXT | `^[0-9]{4}$`, nur bei `kind='fachabteilung'` |
| `care_level` | TEXT | nur bei `kind='station'` |
| `active` | BOOLEAN | |
| `created_at`/`updated_at`/`version` | | |

**Beobachtungen:**
- `kind='traeger'` ist bereits ein Wert — aber es gibt **keine** erstklassige `legal_entities`-Tabelle. Der Rechtsträger ist aktuell nur eine `org_unit`-Zeile, nicht die DSGVO-Verantwortliche-Grenze.
- Es gibt **keine** `sites`-Tabelle, **keine** `org_identifiers`-Tabelle, **keine** `service_agreements`-Tabelle.
- `ik_number` ist bereits eine Kennung, aber nicht versioniert und nicht typisiert (nur 9-stellig, kein `identifier_type`).
- Es gibt **kein** zeitgebundenes Edge-Modell (`valid_from`/`valid_to`) — nur flache `parent_id`.
- Es gibt **keinen** `TenantContext` in `kis-core` (zu prüfen: `crates/kis-core/src/`).

## 2. Soll-Zustand gemäß ADR-001

ADR-001 verlangt:

1. **`legal_entities`** (erstklassig): `id` (unveränderlich), Rechtsform, Status, Datenschutz-Verantwortlicher, Standard-Aufbewahrungsprofil.
2. **`sites`**: `legal_entity_id` NOT NULL (FK), `sector` (`inpatient`/`mvz`/`practice`/`central_service`/`mixed`).
3. **`org_units` + `legal_entity_id` NOT NULL** (und optional `site_id`): Hierarchiekante darf **niemals** zwei verschiedene `legal_entity_id` verbinden → DB-Trigger/Constraint.
4. **`org_identifiers`** (versioniert): `identifier_type` aus Allowlist (`IK`, `BSNR`, `NBSNR`, `LANR`, `TELEMATIK_ID`, `STANDORTNUMMER_293`, `UMSATZSTEUER_ID`), Gültigkeitszeitraum `valid_from`–`valid_to`.
5. **`service_agreements`**: zeitgebunden, expliziter Zweck + Modul-Scope (für zentrale Dienste).
6. **Konfigurationsvererbung**: `verbund_default → legal_entity → site/org_unit` + `policy_floor` (unveränderlich).

## 3. Entwurfsfragen (OFFEN — benötigen Koordinator-/ADR-Klärung)

> Diese Punkte sind **nicht** eigenmächtig zu entscheiden. Sie betreffen gemeinsame
> Verträge oder das Datenmodell und brauchen Klärung vor Implementierung.

### Q1: `legal_entity_id` auf `org_units` — NOT NULL vs. Backfill
- `org_units` existiert bereits mit Daten (synthetische Testdaten in jeder Test-DB).
- ADR-001: `legal_entity_id` **NOT NULL**. → Migration braucht einen Backfill-Schritt:
  - Welche bestehenden `org_units`-Zeilen sind bereits Rechtsträger (`kind='traeger'`)?
  - Sollen diese automatisch zu `legal_entities`-Zeilen hochgezogen werden, oder ist
    `legal_entity_id` für `kind='traeger'`-Einträge `NULL` (der Rechtsträger *ist* die
    `legal_entity`)?
- **Empfehlung (zur Diskussion):** `kind='traeger'`-Einträge werden 1:1 zu
  `legal_entities` kopiert, `org_units.legal_entity_id` wird NOT NULL und zeigt auf die
  eigene `legal_entity_id` (bei `kind='traeger'`) bzw. die des übergeordneten Trägers
  (bei Unter-Einheiten). Backfill über einen deterministischen Walk der `parent_id`-Kette.

### Q2: `ik_number` → `org_identifiers` (Migration bestehender Kennungen)
- `org_units.ik_number` enthält bereits 9-stellige IK-Werte.
- ADR-001: Kennungen gehören in `org_identifiers` (versioniert, typisiert).
- **Offen:** Sollen bestehende `ik_number`-Werte nach `org_identifiers` migriert werden
  (mit `identifier_type='IK'`, `valid_from=created_at`)? Wird `ik_number` danach
  deprecated/entfernt oder bleibt sie als Denormalisierung?
- **Empfehlung:** Migration nach `org_identifiers`, `ik_number` in dieser Migration
  **nicht** entfernen (kompatibel), Abhängigkeit in nachgelagertem Paket.

### Q3: Hierarchiekanten — flach (`parent_id`) vs. zeitgebunden (`org_unit_edges`)
- ADR-001 sagt: "Jede Hierarchiekante (`org_unit_edges` **oder** zeitgebundene
  Versionierung mit `valid_from`/`valid_to`)".
- Der Ist-Zustand ist flach (`parent_id`).
- **Offen:** Führt EP-01a das `org_unit_edges`-Modell ein, oder bleibt `parent_id`
  und der Trigger greift auf `parent_id`? Die ADR lässt beides zu.
- **Empfehlung:** In EP-01a **bleibt `parent_id`** (kein Modell-Wechsel), der
  rechtsträgerübergreifende-Trigger greift auf `parent_id`. `org_unit_edges`
  (zeitgebundene Versionierung) wäre ein separater, nachgelagerter Schritt. Das
  hält die Migration klein und prüfbar (Roadmap-Prinzip: "klein genug für ein
  eigenes Branch, eine einzelne prüfbare Migration").

### Q4: `TenantContext` in `kis-core` — KLÄRUNG
- ADR-001: `TenantContext` in `kis-core`.
- **Ist-Zustand (geprüft):** Es existiert **kein** `TenantContext` und **kein**
  `TreatmentContext` in `crates/kis-core/src/`. Der einzige Treffer war ein
  Kommentar in `http.rs`. → **Neu zu implementieren.**
- **Offen:** Wie wird `TenantContext` von den Routen gelesen? Vorschlag:
  - `struct TenantContext { pub legal_entity_id: Uuid }`
  - Gelesen aus: (a) Session (nach Login, Nutzer gehört zu einem Rechtsträger) oder
    (b) HTTP-Header `X-Legal-Entity` (für interne Service-Zufragen).
  - Zugriff: `Extension<TenantContext>` in axum, gesetzt in einer Middleware.
  - `kis-core::treatment`/`facility`-Funktionen erweitern um `tenant: &TenantContext`.
- **Empfehlung:** `TenantContext` in `crates/kis-core/src/tenant.rs` (neue Datei),
  Middleware in `crates/kis-core/src/http.rs`, Session-basiert (primär) + Header
  (faktisch für interne Calls). Details im Implementierungs-Paket.

### Q7: `config/module_tables.toml` — KLÄRUNG
- ADR-001: `legal_entities`, `sites`, `org_identifiers`, `service_agreements` sind
  **Kern-Tabellen** (fast jede fachliche Tabelle trägt `legal_entity_id`).
- **Ist-Zustand (geprüft):** `config/module_tables.toml` hat `[kernel] tables = [...]`
  mit `"org_units"` bereits gelistet. `scripts/check_modules.py` prüft:
  - jede Tabelle aus `migrations/` steht hier genau einmal,
  - Fachmodule schreiben nie in Kern-Tabellen.
- **Empfehlung:** Die 4 neuen Tabellen in `[kernel] tables` unter dem Kommentar
  `# Organisation` ergänzen:
  ```toml
  "legal_entities", "sites", "org_units", "org_identifiers",
  "service_agreements", "rooms", "beds", "user_org_units",
  ```
- **Wichtig:** `scripts/check_modules.py` wird nach der Migration jede neue Tabelle
  in `module_tables.toml` finden müssen — sonst schlägt `scripts/check.sh` fehl.
  Das ist Teil des EP-01a-Scope und kein separater Schritt.

### Q5: Trigger gegen rechtsträgerübergreifende Kanten
- ADR-001: "DB-Trigger/Constraint".
- **Offen:** `BEFORE INSERT OR UPDATE`-Trigger auf `org_units`, der prüft, dass
  `legal_entity_id` = `legal_entity_id` des Parents (via `parent_id`). Oder ein
  `CHECK`-Constraint (schwerer, weil JOIN nötig → Trigger ist der saubere Weg).
- **Empfehlung:** Trigger `prevent_cross_legal_entity_edge()` auf `org_units`.
  Testfall: INSERT mit `parent_id` auf eine Einheit mit anderem `legal_entity_id`
  muss fehlschlagen (NVT-01/NVT-02).

### Q6: `service_agreements` — Schema
- ADR-001: "zeitgebundene `service_agreements` (Leistungs-/AVV-Verträge mit
  erlaubtem Zweck und Modul-Scope)".
- **Offen:** Spalten? Vorschlag:
  - `id UUID PK`
  - `legal_entity_a_id UUID NOT NULL FK → legal_entities`
  - `legal_entity_b_id UUID NOT NULL FK → legal_entities`
  - `purpose TEXT NOT NULL` (erlaubter Zweck)
  - `module_scope TEXT[] NOT NULL` (erlaubte Module)
  - `valid_from TIMESTAMPTZ NOT NULL`
  - `valid_to TIMESTAMPTZ` (NULL = offen)
  - `CHECK (legal_entity_a_id <> legal_entity_b_id)`
  - `UNIQUE (legal_entity_a_id, legal_entity_b_id, purpose, module_scope, valid_from)`
- **Empfehlung:** Schema wie oben; Validierungslogik in `kis-core`/`kis-org`
  (EP-01b), nicht in dieser Migration.

## 4. Migration-Skizze (`012_legal_entities.sql` — ENTWURF, nicht final)

```sql
-- 012_legal_entities.sql (ENTWURF — erst nach Reservierung finalisieren)

-- 1) legal_entities
CREATE TABLE legal_entities (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_form          TEXT NOT NULL CHECK (legal_form IN
                            ('ggmbh','ag','gmbh','mvz_gmbh','kaser','sonstige')),
    name                TEXT NOT NULL CHECK (length(trim(name)) > 0),
    status              TEXT NOT NULL DEFAULT 'active'
                        CHECK (status IN ('active','suspended','dissolved')),
    dpo_name            TEXT,  -- Datenschutz-Verantwortlicher
    dpo_contact         TEXT,
    retention_profile   TEXT NOT NULL DEFAULT 'standard',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INTEGER NOT NULL DEFAULT 1
);

-- 2) sites
CREATE TABLE sites (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id     UUID NOT NULL REFERENCES legal_entities(id),
    name                TEXT NOT NULL CHECK (length(trim(name)) > 0),
    sector              TEXT NOT NULL CHECK (sector IN
                            ('inpatient','mvz','practice','central_service','mixed')),
    address             TEXT,
    active              BOOLEAN NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    version             INTEGER NOT NULL DEFAULT 1
);

-- 3) org_identifiers (versioniert)
CREATE TABLE org_identifiers (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_unit_id         UUID NOT NULL REFERENCES org_units(id),
    legal_entity_id     UUID REFERENCES legal_entities(id),
    identifier_type     TEXT NOT NULL CHECK (identifier_type IN
                            ('IK','BSNR','NBSNR','LANR','TELEMATIK_ID',
                             'STANDORTNUMMER_293','UMSATZSTEUER_ID')),
    value               TEXT NOT NULL CHECK (length(trim(value)) > 0),
    valid_from          TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_to            TIMESTAMPTZ,
    CHECK (valid_to IS NULL OR valid_to > valid_from),
    UNIQUE (org_unit_id, identifier_type, value, valid_from)
);

-- 4) service_agreements
CREATE TABLE service_agreements (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_a_id   UUID NOT NULL REFERENCES legal_entities(id),
    legal_entity_b_id   UUID NOT NULL REFERENCES legal_entities(id),
    purpose             TEXT NOT NULL,
    module_scope        TEXT[] NOT NULL,
    valid_from          TIMESTAMPTZ NOT NULL,
    valid_to            TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (legal_entity_a_id <> legal_entity_b_id),
    CHECK (valid_to IS NULL OR valid_to > valid_from)
);

-- 5) org_units + legal_entity_id
ALTER TABLE org_units
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN site_id         UUID REFERENCES sites(id);

-- 6) Backfill legal_entities aus org_units (kind='traeger')
--    + legal_entity_id auf org_units (Walk der parent_id-Kette)
--    [Details je nach Q1-Klärung]

-- 7) legal_entity_id NOT NULL (nach Backfill)
ALTER TABLE org_units ALTER COLUMN legal_entity_id SET NOT NULL;

-- 8) Trigger: keine rechtsträgerübergreifende Kante
CREATE FUNCTION prevent_cross_legal_entity_edge() RETURNS trigger AS $$
BEGIN
    IF NEW.parent_id IS NOT NULL THEN
        IF (SELECT legal_entity_id FROM org_units WHERE id = NEW.parent_id)
           IS DISTINCT FROM NEW.legal_entity_id THEN
            RAISE EXCEPTION 'rechtsträgerübergreifende Org-Kante verboten (ADR-001)';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_org_units_cross_le
    BEFORE INSERT OR UPDATE ON org_units
    FOR EACH ROW EXECUTE FUNCTION prevent_cross_legal_entity_edge();
```

## 5. Test-Plan (NVT-01, NVT-02 + Schema-Tests)

| Test | Zweck |
|---|---|
| `legal_entity_created` | `legal_entities`-Zeile wird angelegt, `id` unveränderlich |
| `site_requires_legal_entity` | `sites` ohne gültiges `legal_entity_id` → Fehlschlag |
| `org_unit_requires_legal_entity` | `org_units` ohne `legal_entity_id` → Fehlschlag (NOT NULL) |
| `cross_legal_entity_edge_blocked` | INSERT `org_unit` mit `parent_id` auf anderen Rechtsträger → Trigger feuert, INSERT schlägt fehl |
| `org_identifier_typed` | `org_identifiers` mit ungültigem `identifier_type` → Fehlschlag (Allowlist) |
| `org_identifier_versioned` | Zwei gültige Zeiten desselben `identifier_type`/`value` → OK; Überlappung → geprüft |
| `service_agreement_self_blocked` | `service_agreements` mit `a = b` → Fehlschlag (CHECK) |
| `migration_backfill_deterministic` | Nach Migration: jede `org_unit` hat `legal_entity_id`, `kind='traeger'`-Einträge sind in `legal_entities` |

## 6. Nächste Schritte

1. **[AUTARK, jetzt machbar]** `crates/kis-core/src/` nach `TenantContext`/`TreatmentContext` durchsuchen (Q4).
2. **[AUTARK]** `config/module_tables.toml` lesen und prüfen, ob `legal_entities`/`sites`/`org_identifiers`/`service_agreements` ergänzt werden müssen (Schema für das TOML-Format verstehen).
3. **[BEDIENBAR, nach Reservierung]** Migration `012_legal_entities.sql` finalisieren (offene Punkte Q1–Q6 klären).
4. **[BEDIENBAR, nach Reservierung]** `TenantContext` in `kis-core` implementieren.
5. **[BEDIENBAR, nach Reservierung]** `config/module_tables.toml` erweitern.
6. **[BEDIENBAR, nach Reservierung]** PostgreSQL-Integrationstests (NVT-01, NVT-02 + obige).
7. **[BEDIENBAR, nach Reservierung]** `scripts/check.sh` + CI grün.
8. **[ABGABE]** Arbeitsbericht (Basis-/Ergebnis-SHA, Prüfungen, verbleibende Risiken) + Abnahmebericht-Vorlage.
