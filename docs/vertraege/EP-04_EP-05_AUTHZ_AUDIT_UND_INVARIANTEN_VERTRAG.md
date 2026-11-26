# M01-Umsetzungsvertrag: `EP-04` & `EP-05` — 4-Faktoren-AuthZ, Admin-Trennung, Audit/Retention & Klasse-A/B-Ereignisvertrag

Stand: 25.11.2026 · Autor: `google-neon` (Architektur & Koordination) · Basis: [ADR-004 & ADR-005](../EP00_ARCHITEKTUR_UND_ANALYSE.md), [THREAT_MODEL_VERBUND.md](../THREAT_MODEL_VERBUND.md), [SICHERHEITS_ABNAHME.md](../SICHERHEITS_ABNAHME.md), [ROADMAP.md](../ROADMAP.md) (`EP-04a`–`04d`, `EP-05a`–`05b`) · Status: **Spezifiziert (Umsetzung nach `EP-01`–`EP-03`, unabhängiges `G6`-Sicherheitsreview verpflichtend)**

---

## 1. Paketabgrenzung (`EP-04a`–`04d` & `EP-05a`–`05b`)

| Teilpaket | Exklusiver Schreibbereich | Abhängigkeiten | Geplante Migration | Pflicht-Nachweise |
|---|---|---|---|---|
| **`EP-04a`** | `crates/kis-core/src/rbac.rs`, `auth.rs`, `rate_limit.rs`, `crates/kis-app/src/core_routes.rs`, `admin_users.rs`, `static/js/` (`AP-02`) | `EP-01a`, `EP-01c` | **`015`** (Anteil Auth/MFA/SSO) | `NVT-03`, `NVT-04`, `A-01`–`A-13`, `F-03` |
| **`EP-04b`** | `crates/kis-core/src/treatment.rs`, `module.rs`, `crates/kis-app/src/middleware.rs` | `EP-01b`, `EP-03b`, `EP-04a` | keine | `NVT-08`, `NVT-09`, `A-14` (Write-Pool-Prüfung) |
| **`EP-04c`** | `crates/kis-core/src/audit.rs`, `crates/kis-app/src/admin_platform.rs` | `EP-04b` | **`015`** (Audit-Spalten, WORM-Anker) | `NVT-10`, `NVT-11`, `L-01`–`L-09` |
| **`EP-04d`** | `crates/kis-core/src/data_portability.rs`, `data_deletion.rs`, `anonymization.rs`, `crates/kis-app/src/gdpr_routes.rs` (`AP-03`/`04`) | `EP-03b`, `EP-04b`, `DEC-06`, `DEC-08` | **`015`** (`retention_policies`, `legal_holds`) | `NVT-12`, `NVT-13` |
| **`EP-05a`** | `crates/kis-core/src/events.rs`, `module.rs`, `crates/kis-cases`, `crates/kis-orders`, `crates/kis-lab` | `EP-01a`, `EP-03b`, `EP-04b` | keine | `NVT-14`, Fehler-Injektionstest, Modulabschaltungs-Schutztest |
| **`EP-05b`** | `crates/kis-core/src/events.rs`, `crates/kis-app/src/admin_platform.rs` | `EP-05a` | **`015`** (Outbox/Inbox/Dead-Letter) | `NVT-15`, Crash-/Retry-/Duplikat-/Out-of-order-Tests |

---

## 2. Rust-Kernvertrag `EP-04a`/`04b`: RBAC, Domänentrennung & 4-Faktoren-Zugriffsprüfung

### 2.1 Abschaffung von `admin => PermissionSet::all()` & Kapazitätsplanung `PermissionSet`

Aktueller Ist-Befund in `crates/kis-core/src/rbac.rs`:
- Zeile 327–329: `if role == ADMIN { return PermissionSet::all(); }` (**kritischer Befund `G-03`**, verletzt `NVT-03`).
- Zeile 242: `pub struct PermissionSet(u128)` belegt bereits **48 von 128 Bits**. Für Enterprise-Module (`E01`–`E24` + `TI01`–`TI15`) wird `128` absehbar überschritten.

Verbindliche Vorgabe für `EP-04a`:
1. **`PermissionSet` auf `[u64; 4]` (256 Bit, weiterhin `Copy`, ohne Heap-Allokation) erweitern**, damit spätere Enterprise-Rechte ohne Umbau passen.
2. **Kein implizites `PermissionSet::all()` mehr für irgendeine Rolle.** `RoleMatrix::effective` wertet ausschließlich die in der Datenbank (`roles` / `role_permissions` / mandantenspezifische Rollen) vergebenen Rechte aus.
3. **Verbindliche getrennte Grundausstattungs-Rollenfamilien (`BUILTIN_ROLES`):**
   - `platform_admin`: Systembetrieb, Mandanten-Metadaten, Schnittstellen/Health, Modulstatus — **KEIN** `PatientRead`, `CaseRead`, `ClinicalRead`, `LabRead`, `NursingRead`, `DocumentRead`, `BreakGlass`, `Export`.
   - `tenant_admin`: Benutzer, Rollen, Organisation, Standorte, Einstellungen **innerhalb des eigenen Rechtsträgers** — **KEIN** pauschales klinisches Leserecht.
   - `privacy_auditor`: `AuditRead`, `BreakGlassReview`, `LegalHoldManage`, `RetentionRead` **innerhalb des eigenen Rechtsträgers** — **KEIN** klinisches Schreibrecht.
   - `hr_manager`: Dienstplan/HR-Stammdaten — **KEIN** Patienten-/Fall-/Klinikrecht (`NVT-03`).
   - `finance_clerk`: Stammdaten/Faktura — **KEIN** unberechtigter klinischer Freitext (`NVT-03`).
   - `research_analyst`: ausschließlich genehmigte pseudonymisierte Forschungszone — **KEIN** direkter Zugriff auf produktive `patients`/`cases`.
4. **Schutz gegen Aussperren (ohne klinische Vollmacht):** Ein `platform_admin` behält immer das Recht `TenantManage` / `AdminSystem`, aber niemals klinische Patientenrechte; Wiederherstellung geschieht über `kis-admin` (CLI mit lokalem Betriebszugriff + Pflicht-Audit).

### 2.2 Erweiterung von `Scope` in `crates/kis-core/src/module.rs`

Bisher unterstützt `Scope` nur `None`, `Case` (`:case_id`) und `Unit` (`:unit_id`). Ab `EP-04b` wird `Scope` erweitert:

```rust
// Verbindlicher Zielvertrag für crates/kis-core/src/module.rs (EP-04b)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Nur zulässig für Public/Authenticated oder Endpunkte mit explizitem Tenant-Filter im Handler
    TenantOnly,
    /// Pfad enthält :patient_id -> Rechtsträger- & Behandlungszusammenhang + Lese-Audit
    Patient,
    /// Pfad enthält :case_id -> 4-Faktoren-Prüfung (Mandant, Einheit/ServiceAgreement, Recht/Zweck, Behandlung)
    Case,
    /// Pfad enthält :unit_id -> Rechtsträger- & Einheiten-Zugehörigkeit
    Unit,
    /// Ausschließlich für echte öffentliche Endpunkte (/health, /static/*, /api/v1/auth/login)
    Public,
}
```

### 2.3 Korrigiertes `MY_UNITS` mit strikter Rechtsträger- und Service-Agreement-Grenze (`crates/kis-core/src/treatment.rs`)

Die bisherige CTE `MY_UNITS` (Zeile 40–46 in `treatment.rs`) lief rekursiv ohne Mandantengrenze (`G-05`). Verbindliche Zielabfrage (immer gegen `db.write()`, `A-14`):

```sql
-- Verbindliche Referenz-CTE für EP-04b (crates/kis-core/src/treatment.rs)
-- $1 = user_id, $2 = active_legal_entity_id
WITH RECURSIVE direct_units AS (
    SELECT a.org_unit_id AS id, a.legal_entity_id
    FROM user_org_assignments a
    WHERE a.user_id = $1
      AND a.legal_entity_id = $2
      AND a.org_unit_id IS NOT NULL
      AND a.valid_from <= now()
      AND (a.valid_until IS NULL OR a.valid_until > now())
    UNION
    -- Rückwärtskompatibilität für bestehende user_org_units-Einträge innerhalb desselben Rechtsträgers
    SELECT u.unit_id AS id, o.legal_entity_id
    FROM user_org_units u
    JOIN org_units o ON o.id = u.unit_id
    WHERE u.user_id = $1
      AND o.legal_entity_id = $2
      AND u.valid_from <= now()
      AND (u.valid_until IS NULL OR u.valid_until > now())
),
mine AS (
    SELECT id, legal_entity_id FROM direct_units
    UNION
    SELECT o.id, o.legal_entity_id
    FROM org_units o
    JOIN mine m ON o.parent_id = m.id
    -- HARTE SCHRANKE: Rekursion überschreitet NIEMALS die Rechtsträgergrenze!
    WHERE o.legal_entity_id = m.legal_entity_id
)
SELECT id FROM mine;
```

Für **Zentrale Dienste** (`NVT-09`, z. B. `ZD-LAB` in `RT-NORD` für einen Auftrag aus `KL-S1` in `RT-SUED`) gilt ein separater, eng begrenzter Pfad:
- Zugriff besteht **nur**, wenn (a) ein aktiver, zeitlich gültiger `service_agreements`-Eintrag zwischen `provider_legal_entity_id` und `consumer_legal_entity_id` für die passende `service_domain` und den angeforderten `purpose_of_use` vorliegt **und** (b) ein konkreter, offener Auftrag / `case_participants`-Eintrag für diese `provider_unit_id` existiert (plus begrenzter konfigurierbarer Nachlauf nach Abschluss).
- Sobald `service_agreements.active = false` oder `valid_until <= now()` gesetzt wird, lehnt `check_case` auf `db.write()` sofort mit `403 service_agreement_inactive` ab.

---

## 3. Rust-Kernvertrag `EP-05a`/`05b`: Zwei-Klassen-Ereignis- & Invariantenvertrag

Bisher fängt `EventBus::publish` (Zeile 128–151 in `crates/kis-core/src/events.rs`) jeden Abonnenten-Fehler per Savepoint ab. Das ist für optionale Reaktionen richtig, für sicherheitskritische Invarianten aber ein schwerer Fehler (`G-11`, `NVT-14`).

Verbindliche Trennung in `crates/kis-core/src/events.rs`:

```rust
// Verbindlicher Zielvertrag für crates/kis-core/src/events.rs (EP-05a)

/// Klasse A: Sicherheitskritische Invariante.
/// Läuft OHNE Savepoint-Abschwächung direkt in der Transaktion des Auslösers.
/// Ein Fehler führt zu vollständigem Rollback ODER einem atomar geschriebenen
/// sicheren Sperrzustand (niemals stiller Erfolg!).
#[axum::async_trait]
pub trait InvariantHandler: Send + Sync {
    fn name(&self) -> &'static str;
    fn topics(&self) -> &'static [&'static str];
    /// Welches Modul schützt diese Invariante? Abschaltung dieses Moduls wird beim Start verweigert!
    fn required_by_safety_policy(&self) -> bool { true }
    async fn enforce(&self, tx: &mut sqlx::PgConnection, event: &Event) -> anyhow::Result<()>;
}

/// Klasse B: Optionale Folgeaktion / Outbox-Verbraucher.
/// Läuft isoliert im Savepoint; Fehler landen mit legal_entity_id in event_failures / event_dead_letters.
#[axum::async_trait]
pub trait EventSubscriber: Send + Sync {
    fn name(&self) -> &'static str;
    fn topics(&self) -> &'static [&'static str];
    async fn handle(&self, tx: &mut sqlx::PgConnection, event: &Event) -> anyhow::Result<()>;
}
```

Verbindliche Klasse-A-Pflichtimplementierungen in `EP-05a`:
1. **Bettensperre bei Entlassung/Verlegung (`case.discharged`, `case.transferred`):** Die Sperre `beds.blocked_reason = 'cleaning_required'` (oder `isolation_cleaning_required` bei aktivem Isolations-Alert) wird atomar in derselben Transaktion gesetzt — **unabhängig** davon, ob das Auftragsmodul `kis-orders` aktiv ist! Das Abschalten von `kis-orders` darf niemals dazu führen, dass ein entlassenes/isoliertes Bett ungesperrt wiederbelegt werden kann (`NVT-14`).
2. **Kritischer Laborwert (`lab.critical_result`):** Atomare Erzeugung eines unquittierten kritischen Eskalationseintrags in derselben Transaktion wie die Befundfreigabe.
3. **Modul-Startprüfung (`resolve_enabled` in `kis-core/src/module.rs`):** Prüft beim Start, dass alle für die aktivierten klinischen Bereiche erforderlichen Klasse-A-Invarianten registriert sind; bei fehlender Sicherheitsinvariante bricht der Serverstart mit eindeutiger Fehlermeldung ab (`fail closed`).

---

## 4. Schema-Entwurf `EP-04` & `EP-05`: Migration `015_authz_audit_retention_events.sql`

```sql
-- Entwurf für EP-04 / EP-05: migrations/015_authz_audit_retention_events.sql

-- 1. Sitzungen: aktiver Mandant, Standort & Zweck + verschlüsseltes MFA-Secret
ALTER TABLE sessions
    ADD COLUMN active_legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN active_site_id         UUID,
    ADD COLUMN auth_method            TEXT NOT NULL DEFAULT 'local_password'
        CHECK (auth_method IN ('local_password', 'local_mfa', 'oidc', 'saml', 'ldap'));

ALTER TABLE users
    ADD COLUMN mfa_secret_enc TEXT,
    ADD COLUMN mfa_key_version SMALLINT NOT NULL DEFAULT 1,
    ADD COLUMN external_idp_subject TEXT,
    ADD COLUMN external_idp_issuer  TEXT;
CREATE UNIQUE INDEX idx_users_external_idp
    ON users(external_idp_issuer, external_idp_subject)
    WHERE external_idp_issuer IS NOT NULL AND external_idp_subject IS NOT NULL;

-- 2. Audit-Trail: Mandantenbezug, Zweck, Zugriffsklasse & externer Prüfanker (WORM)
-- Hinweis: audit_logs ist append-only; bestehende Trigger bleiben unberührt!
ALTER TABLE audit_logs
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN purpose_of_use  TEXT,
    ADD COLUMN access_kind     TEXT NOT NULL DEFAULT 'mutation'
        CHECK (access_kind IN ('mutation', 'read', 'export', 'break_glass', 'auth', 'admin', 'replay'));
CREATE INDEX idx_audit_logs_tenant_time ON audit_logs(legal_entity_id, timestamp DESC);

-- Externer Prüfanker (periodisch signierter Hash-Wurzel-Export in externen WORM-Speicher)
CREATE TABLE audit_log_anchors (
    id                BIGSERIAL PRIMARY KEY,
    from_audit_id     BIGINT NOT NULL,
    to_audit_id       BIGINT NOT NULL,
    tip_hash          TEXT NOT NULL,
    anchor_signature  TEXT NOT NULL,
    key_id            TEXT NOT NULL,
    external_worm_ref TEXT NOT NULL,
    anchored_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (to_audit_id >= from_audit_id)
);

-- 3. Break-Glass mandantenbezogen erweitern
ALTER TABLE emergency_access_grants
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN review_due_at   TIMESTAMPTZ;
UPDATE emergency_access_grants g
SET legal_entity_id = c.legal_entity_id,
    review_due_at   = g.granted_at + interval '7 days'
FROM cases c
WHERE c.id = g.case_id AND g.legal_entity_id IS NULL;
ALTER TABLE emergency_access_grants
    ALTER COLUMN legal_entity_id SET NOT NULL,
    ALTER COLUMN review_due_at SET NOT NULL;

-- 4. Aufbewahrungsmatrix (DEC-06) & Legal Hold (EP-04d)
CREATE TABLE retention_policies (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id     UUID REFERENCES legal_entities(id), -- NULL = Verbund-Standard
    document_category   TEXT NOT NULL CHECK (document_category IN (
        'medical_record', 'radiology_radiation', 'transfusion_tfg',
        'billing_tax', 'audit_log', 'hr_roster', 'consent', 'other'
    )),
    legal_basis         TEXT NOT NULL CHECK (length(trim(legal_basis)) > 0),
    retention_years     INTEGER NOT NULL CHECK (retention_years BETWEEN 1 AND 50),
    federal_state       TEXT CHECK (federal_state IS NULL OR federal_state ~ '^DE-[A-Z]{2}$'),
    valid_from          DATE NOT NULL,
    valid_until         DATE,
    updated_by          UUID NOT NULL REFERENCES users(id),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE legal_holds (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    legal_entity_id   UUID NOT NULL REFERENCES legal_entities(id),
    entity_type       TEXT NOT NULL CHECK (entity_type IN ('patient_record', 'case', 'document', 'billing_case')),
    entity_id         UUID NOT NULL,
    reason            TEXT NOT NULL CHECK (length(trim(reason)) >= 10),
    placed_by         UUID NOT NULL REFERENCES users(id),
    placed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    released_by       UUID REFERENCES users(id),
    released_at       TIMESTAMPTZ,
    release_reason    TEXT,
    CHECK ((released_by IS NULL) = (released_at IS NULL))
);
CREATE INDEX idx_legal_holds_active ON legal_holds(legal_entity_id, entity_type, entity_id) WHERE released_at IS NULL;

-- 5. Mandantenfähige Outbox, Inbox (Deduplizierung) & Dead-Letter-Quarantäne (EP-05b)
ALTER TABLE domain_events
    ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id),
    ADD COLUMN event_uuid      UUID NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN correlation_id  UUID,
    ADD COLUMN idempotency_key TEXT,
    ADD COLUMN schema_version  SMALLINT NOT NULL DEFAULT 1,
    ADD COLUMN payload_class   TEXT NOT NULL DEFAULT 'metadata_only'
        CHECK (payload_class IN ('metadata_only', 'encrypted_reference'));
CREATE UNIQUE INDEX idx_domain_events_event_uuid ON domain_events(event_uuid);
CREATE UNIQUE INDEX idx_domain_events_idempotency
    ON domain_events(legal_entity_id, topic, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE INDEX idx_domain_events_tenant_topic ON domain_events(legal_entity_id, topic, id);

-- Persistente Inbox zur Verbraucher-Deduplizierung & Out-of-order-Erkennung
CREATE TABLE event_inbox (
    consumer_name    TEXT NOT NULL CHECK (consumer_name ~ '^[a-z0-9_.-]{1,64}$'),
    event_uuid       UUID NOT NULL,
    legal_entity_id  UUID NOT NULL REFERENCES legal_entities(id),
    topic            TEXT NOT NULL,
    entity_id        UUID NOT NULL,
    entity_version   INTEGER,
    processed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (consumer_name, event_uuid)
);

-- Quarantäne / Dead-Letter mit mandantengetrennter Einsicht und Pflicht-Audit beim Replay
CREATE TABLE event_dead_letters (
    id                BIGSERIAL PRIMARY KEY,
    legal_entity_id   UUID NOT NULL REFERENCES legal_entities(id),
    event_id          BIGINT NOT NULL REFERENCES domain_events(id) ON DELETE CASCADE,
    consumer_name     TEXT NOT NULL,
    attempts          INTEGER NOT NULL DEFAULT 1,
    last_error        TEXT NOT NULL,
    next_retry_at     TIMESTAMPTZ,
    quarantined_at    TIMESTAMPTZ,
    resolved_at       TIMESTAMPTZ,
    resolved_by       UUID REFERENCES users(id),
    resolution_reason TEXT,
    CHECK ((resolved_at IS NULL) = (resolved_by IS NULL)),
    CHECK (resolved_at IS NULL OR length(trim(resolution_reason)) >= 10)
);
CREATE INDEX idx_event_dead_letters_open
    ON event_dead_letters(legal_entity_id, quarantined_at) WHERE resolved_at IS NULL;
```

---

## 5. Pflicht-Abnahmetests (`G2`/`G3`) für `EP-04` & `EP-05`

- **`NVT-03`:** `U-PLAT` (`platform_admin`), `U-ADM-N` (`tenant_admin`) und `U-HR-N` (`hr_manager`) erhalten `403` auf klinische Patienten-/Fallakten (`GET /api/v1/patients/:id`, `GET /api/v1/cases/:case_id/notes`); entsprechend gilt die Domänentrennung auch für `finance_clerk` und `research_analyst`.
- **`NVT-08`:** `U-DOC-S1` (`KL-S1`, `RT-SUED`) erhält `403` auf Fall `PAT-05` in `KL-S2` (`PSY-S2`, gleicher Rechtsträger, anderer Standort, kein Behandlungszusammenhang).
- **`NVT-09`:** `U-LAB-Z` (`ZD-LAB`) bearbeitet Auftrag für `PAT-06` (`KL-N1`) bei aktivem `service_agreement` (`200`); bei fehlendem Auftrag in `RT-SUED`, nach Auftragsabschluss oder nach Deaktivierung/Ablauf des Vertrags sofort `403` (über `db.write()`, `A-14`).
- **`NVT-10` / `NVT-11`:** Lesezugriffe von `U-DOC-N1` auf `PAT-01` erzeugen mandantenbezogene `audit_logs`-Einträge ohne PII-Volldump; Break-Glass von `U-DOC-S1` auf `PAT-05` läuft fristgerecht ab und `U-AUD-N` darf `RT-SUED`-Break-Glass nicht einsehen (`403`).
- **`NVT-12` / `NVT-13`:** DSGVO-Export von `U-EXT-N` für `PAT-02` (Fälle in `RT-NORD` und `RT-SUED`) exportiert ausschließlich `RT-NORD`-Daten über `db.write()` in JSON/CSV/FHIR; aktiver `legal_holds`-Eintrag auf `PAT-05` oder laufende `retention_policies`-Frist blockiert Hard-Delete/Anonymisierung (`409 retention_or_legal_hold_active`).
- **`NVT-14` / `NVT-15`:** Entlassung von `PAT-01` mit Fehler-Injektion in `kis-orders` (oder abgeschaltetem `kis-orders`) belässt das Bett atomar in `blocked_reason = 'cleaning_required'` (oder vollem Rollback); `RT-NORD`-Event-Consumer sieht keine `RT-SUED`-Events; doppelte und vertauschte (Out-of-order) Zustellung wird über `event_inbox` sauber dedupliziert; Replay erzeugt Pflicht-Audit.
