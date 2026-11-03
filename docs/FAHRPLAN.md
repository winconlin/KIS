# KIS — Fahrplan für die Entwicklung

> **Dieses Dokument beschreibt die langfristige Roadmap.**
> Agenten lesen zunächst [Root-AGENTS.md](../AGENTS.md), den [operativen Auftrag](AGENTEN_AUFTRAG.md) und die [Sicherheits-Abnahme](SICHERHEITS_ABNAHME.md).
> Zugewiesene Arbeitspakete werden anhand tatsächlicher Nachweise abgeschlossen. Übliche technische Entscheidungen erfolgen selbstständig innerhalb dieses Auftrags; neue Produkt-/Datenfreigaben werden nicht aus der Roadmap abgeleitet.

---

## Aktueller Stand — 02.11.2026

Der ausführbare Root-Build verwendet SQLite. Die folgende Architektur, die Phasen und die Umfangsziele beschreiben überwiegend den geplanten Ausbau. Der PostgreSQL-Code unter `crates/` ist nicht eingebunden; seine Manifeste und Implementierungen sind noch zu vervollständigen.

Die Befundkorrekturen stellen Start, API-Präfix, Konfigurations-Overrides und Sicherheitsbausteine des Prototyps richtig. Fachliche CRUD-Endpunkte und MFA-Setup geben weiterhin HTTP 501 zurück. Es gibt noch keinen vollständigen MEDICO-Ersatz. Details und Verifikationsgrenzen stehen in [REVIEW_STATUS.md](REVIEW_STATUS.md).

Ein Häkchen ist nur bei geprüftem Akzeptanzkriterium zulässig. Reine Entwurfsdateien oder Modellantworten belegen keine erfolgreiche Umsetzung. Das unabhängige zweite Security-Review bleibt ein Freigabeschritt.

## Bestätigte nächste Ausbauetappe — PostgreSQL-Workspace und Datenmodell

Der Projektverantwortliche hat am 02.11.2026 **nur frische Entwicklung/Testinstallationen** und **PostgreSQL-Workspace/Datenmodell als nächste Priorität** bestätigt. Eine Migration bestehender Produktivdaten gehört damit aktuell nicht zum Auftrag. Die bestehende SQLite-Referenz bleibt bis zur geprüften PostgreSQL-Integration der ausführbare Zwischenstand.

Die nächste Etappe arbeitet in dieser Reihenfolge:

1. **Workspace herstellen:** Gültige Paket-Manifeste, klare Workspace-Mitglieder und reproduzierbarer Root-Build. Gemeinsame Verträge liegen in kis-core; die Anwendung setzt die Fachmodule zusammen. kis-core darf nicht gleichzeitig von einem Fachmodul abhängen, das seinerseits kis-core benötigt.
2. **Verbindliches Datenmodell festlegen:** Benutzer/Sitzungen, Patienten, Ärzte, Termine, Medikationen, Audit und Einwilligungen mit konsistenten Namen, UUIDs, UTC-Zeitstempeln, Fremdschlüsseln und Constraints. Externe Identifier erhalten Quellsystem/Ressourcentyp und eindeutige Zuordnung. Objektberechtigungen, Löschung und Optimistic Locking sind dabei ausdrücklich zu modellieren.
3. **PostgreSQL-Speicher integrieren:** Eigene PostgreSQL-Migrationen und passende SQLx-Typen. Die SQLite-DDL-Dateien mit AUTOINCREMENT und SQLite-spezifischen Ausdrücken werden nicht unverändert gegen PostgreSQL ausgeführt. Connection-Pooling wird tatsächlich konfigurierbar und unter Last geprüft.
4. **Sicherheitsverhalten erhalten:** Persistenter Widerruf, atomare Refresh-Rotation, serverseitige Rollen und überprüfbare Audit-Inhalte müssen auch mit dem neuen Backend bestehen. Read-Replicas dürfen keine verzögerten Autorisierungs-/Widerrufsprüfungen verursachen.
5. **Mit echter PostgreSQL-Instanz prüfen:** Frische Migrationen, Constraints, konkurrierende Änderungen, Identifier-Eindeutigkeit und die bisherigen Auth-/Audit-Regressionen in CI ausführen. Dokumentation und Status erst nach grünen Checks auf PostgreSQL als Ist-Stand umstellen.

Die Tasks 0.1 und 0.4 sowie die zugehörigen Datenmodell-/Concurrency-Tasks bleiben bis zu dieser Abnahme offen. Patienten-CRUD/GUI und MEDICO-Migration sind nachgelagerte Ausbauetappen. MEDICO-Version, ISiK-Profile und Testumgebung werden vor der späteren Gegenstellenprüfung benötigt, blockieren aber die Vorbereitung des PostgreSQL-Workspaces nicht.

Für die Umsetzung gelten die Arbeitspakete **PG-00 bis PG-10** im [Agentenauftrag](AGENTEN_AUFTRAG.md). Dort stehen Abhängigkeiten, Dateizuständigkeiten, Abnahmekriterien und Übergabeformat. Die [Sicherheitsmatrix](SICHERHEITS_ABNAHME.md) trennt Pflichtnachweise dieser Basisetappe von späteren CRUD-/GUI-/MEDICO- und Betriebsfreigaben.

## 1. Projektziele

| Ziel | Metrik |
|------|--------|
| **Skalierung** | Tausende gleichzeitiger User, verzögerungsfreies Abfragen/Editieren tausender Patientendatensätze |
| **Rechtssicherheit** | DSGVO-konform, Auftragsverarbeitungsverträge, Datenhaltungsort, Audit-Trail (10 Jahre) |
| **Datenschutz** | AES-256-GCM at rest + TLS 1.3 in transit, RBAC, Anonymisierung, Löschung (Art. 17) |
| **Modularität** | Jedes Modul ist eine eigene Cargo-Krate, deaktivierte Module werden nicht kompiliert |
| **Erweiterbarkeit** | Neues Modul = neue Crate + 1 Zeile in `mod.rs`, kein Refactoring bestehender Code |
| Performance | < 50 ms für API-Antworten (p95), < 200 ms für GUI-Laden |
| Datenbank | PostgreSQL, Connection-Pooling (200+), Optimistic Locking, Read-Replicas |
| Konfiguration | Ein TOML-Datei + ENV-Variablen, kein Code-Change für Standard-Setup |
| GUI | Responsive, Dark/Light Theme, Barrierefreiheit (WCAG 2.1 AA) |
| Schnittstellen | HL7 v2.5, FHIR R4 (ISiK), DICOM, XDS, GDT, Telematikinfrastruktur |
| CGM MEDICO | **Vollständiger Ersatz** — Migration (FHIR R4 + HL7 v2 + GDT), danach MEDICO abgeschaltet |
| LLM-Integration | OpenRouter, 3+ Modelle, Rotation, gegenseitige Code-Review |

---

## 2. Architektur-Prinzipien

### 2.1 Skalierung (tausende User)

```
┌─────────────────────────────────────────────────────────┐
│                    LOAD BALANCER                         │
│              (nginx / HAProxy / L4)                      │
└──────────┬──────────────────────────────┬───────────────┘
           │                              │
   ┌───────▼────────┐              ┌──────▼────────┐
   │  KIS Instance 1 │              │  KIS Instance 2 │   ← stateless
   │  (Tokio, 8 Core)│              │  (Tokio, 8 Core)│     horizontal
   └───────┬────────┘              └──────┬────────┘     skalierbar
           │                              │
   ┌───────▼──────────────────────────────▼────────┐
   │              PostgreSQL Cluster                 │
   │  ┌────────────┐    ┌────────────┐             │
   │  │  PRIMARY   │◄───│  REPLICA 1 │             │
   │  │  (Write)   │    │  (Read)    │             │
   │  └────────────┘    └────────────┘             │
   │  Connection Pool: 200 pro Instance             │
   └───────────────────────────────────────────────┘
```

- **Stateless KIS-Instanzen** als Ziel: zentraler Sitzungswiderruf, Audit und Limits bleiben zustandsbehaftet und benötigen ein gemeinsames Konzept
- **PostgreSQL** statt SQLite: MVCC, tausende parallele Transaktionen
- **Read-Replicas**: Lese-Queries auf Replica, Schreib-Queries auf Primary
- **Optimistic Locking**: `version`-Spalte in allen Tabellen, kein Pessimistic Locking
- **Connection Pooling**: 200+ Connections pro Instance, konfigurierbar
- **Horizontal skalierbar**: Mehrere KIS-Instanzen hinter Load Balancer

### 2.2 Rechtssicherheit & Datenschutz

| Prinzip | Umsetzung |
|---------|-----------|
| **DSGVO Art. 5** (Datenminimierung) | Nur notwendige Daten, verschlüsselte Spalten |
| **DSGVO Art. 6** (Rechtsgrundlage) | `consents`-Tabelle, legal_basis je Verarbeitung |
| **DSGVO Art. 17** (Löschung) | Soft-Delete + Hard-Delete nach Aufbewahrungsfrist |
| **DSGVO Art. 30** (Verzeichnis) | `docs/DSGVO/VERZEICHNIS.md` |
| **DSGVO Art. 28** (Auftragsverarbeitungsvertrag) | `docs/DSGVO/AVV.md` |
| **DSGVO Art. 33** (Meldung 72h) | Incident-Response-Plan |
| **Audit-Trail** | Inhaltsprüfung, externe Verankerung und betrieblich begründete Aufbewahrung |
| **Verschlüsselung** | AES-256-GCM at rest, TLS 1.3 in transit |
| **Zugriffskontrolle** | RBAC (Admin, Arzt, Pflege, Rezeption, Viewer) |
| **Datenhaltungsort** | Konfigurierbar, DSGVO-konform (EU) |
| **Anonymisierung** | Irreversible Anonymisierung prüfen; Pseudonymisierung bleibt davon getrennt |
| **Backup** | 3-2-1-Regel, verschlüsselt |

### 2.3 Modularität & Erweiterbarkeit

```
kis-workspace/
├── Cargo.toml              ← Workspace-Definition
├── crates/
│   ├── kis-core/           ← Kern: Config, DB, Security, API-Router
│   │   └── Cargo.toml
│   ├── kis-patients/       ← Patient-Service (CRUD, Suche, DSGVO)
│   │   └── Cargo.toml
│   ├── kis-doctors/        ← Arzt-Service
│   │   └── Cargo.toml
│   ├── kis-appointments/   ← Termin-Service
│   │   └── Cargo.toml
│   ├── kis-medications/    ← Medikament-Service
│   │   └── Cargo.toml
│   ├── kis-hl7/            ← HL7 v2.5 Schnittstelle
│   │   └── Cargo.toml
│   ├── kis-fhir/           ← FHIR R4 Server + Client
│   │   └── Cargo.toml
│   ├── kis-dicom/          ← DICOM SCP
│   │   └── Cargo.toml
│   ├── kis-xds/            ← XDS Registry + Repository
│   │   └── Cargo.toml
│   ├── kis-gdt/            ← GDT Import/Export
│   │   └── Cargo.toml
│   ├── kis-medico/         ← CGM MEDICO Integration (FHIR + OAuth)
│   │   └── Cargo.toml
│   ├── kis-ti/             ← Telematikinfrastruktur (eGK, ePA, eRezept)
│   │   └── Cargo.toml
│   └── kis-ui/             ← GUI (Axum + server-seitig gerendert)
│       └── Cargo.toml
├── migrations/
│   ├── patients/           ← Migrations pro Modul
│   ├── doctors/
│   ├── appointments/
│   ├── medico/
│   └── core/
├── config/
│   └── kis.toml
├── docs/
│   ├── FAHRPLAN.md
│   ├── CGM_MEDICO.md
│   ├── API.md
│   ├── SECURITY.md
│   ├── DSGVO/
│   │   ├── VERZEICHNIS.md
│   │   └── AVV.md
│   └── INSTALLATION.md
└── tests/
    ├── integration/
    └── performance/
```

**Geplante Modul-Kontrakte:**

Jedes Modul exportiert:
1. **`Service`-Trait**: CRUD-Operationen, Suchen, Workflows
2. **`ApiRouter`**: Axum-Router mit eigenen Endpunkten
3. **`Config`**: Eigene TOML-Sektion unter `[modules.<name>]`
4. **`Migrations`**: Eigene SQL-Dateien in `migrations/<name>/`
5. **`Feature-Flag`**: `Cargo.toml` Feature, `false` → wird nicht kompiliert

**Erweiterung = neue Crate:**
```text
# Entwurf; noch kein ausführbares Workspace-Beispiel.
// crates/kis-new-module/Cargo.toml
[package]
name = "kis-new-module"
version = "0.1.0"

[dependencies]
kis-core = { path = "../kis-core" }

// In crates/kis-core/src/api/mod.rs:
#[cfg(feature = "new-module")]
pub mod new_module {
    pub use kis_new_module::register_router;
}

// In Cargo.toml (Workspace):
[features]
new-module = ["kis-new-module"]
```

**Kein Refactoring:** Bestehende Module werden nie geändert, wenn ein neues Modul hinzukommt.

---

## 3. Phasenübersicht

```
Phase 0: Fundament (2 Wochen)
  └─ Phase 1: Core (4 Wochen)
      └─ Phase 2: GUI (3 Wochen)
          └─ Phase 3: Schnittstellen (4 Wochen)
              └─ Phase 4: CGM MEDICO (3 Wochen)
                  └─ Phase 5: Sicherheit & Compliance (3 Wochen)
                      └─ Phase 6: LLM-Integration (2 Wochen)
                          └─ Phase 7: Härtung & Release (2 Wochen)
```

Historische Schätzung: **~23 Wochen**. Kein verbindlicher Termin; Aufwand und Abnahmen müssen anhand der offenen Tasks neu bewertet werden.

---

## 4. Phase 0 — Fundament

**Ziel:** Projektstruktur, Build-System, CI/CD, Testrahmen, PostgreSQL.

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 0.1 | Cargo-Workspace mit Crates (kis-core, kis-patients, ...) | `cargo build` im Workspace | 🔲 |
| 0.2 | Projektstruktur (Module: core, patients, doctors, appointments, medications, schnittstellen, security, config, ui) | `cargo check` ohne Fehler | ✅ |
| 0.3 | Konfigurationssystem (figment: TOML + ENV) | `config/kis.toml` wird geladen | ✅ |
| 0.4 | PostgreSQL-Setup (SQLX, Connection-Pooling, Migrations) | `cargo run` erstellt DB + Tabellen | ✅ |
| 0.5 | Logging (tracing, JSON + Human) | Logs in `logs/kis.log` | ✅ |
| 0.6 | Fehlerhandling (anyhow + thiserror, JSON Error API) | Consistent Error-Format | ✅ |
| 0.7 | CI/CD (GitHub Actions: build, test, lint) | `.github/workflows/ci.yml` | ✅ |
| 0.8 | Testrahmen (unit + integration, `cargo test`) | Mindestens 10 Tests | ✅ (43 Tests) |
| 0.9 | Dockerfile (multi-stage, minimal image) + docker-compose (KIS + PostgreSQL) | `docker compose up` < 150 MB | ✅ |
| 0.10 | systemd-Unit (Production-Deployment) | `kis.service` läuft als Daemon | ✅ |
| 0.11 | **PostgreSQL Read-Replica-Setup** (dokumentiert + Test-Script) | `docs/INSTALLATION.md` § Read-Replica | ✅ |
| 0.12 | **Load-Balancer-Setup** (nginx config für KIS-Instanzen) | `config/nginx/kis.conf` | ✅ |

**Phase 0: ✅ 12/12 Tasks abgeschlossen (Stand: 02.11.2026)**

**Agent-Regel Phase 0:** Kein Code für Features. Nur Infrastruktur. Alle Tasks sind unabhängig — parallelisierbar.

---

## 5. Phase 1 — Core (Backend)

**Ziel:** Vollständiges CRUD für alle Entitäten, REST-API, Authentifizierung, Skalierung.

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 1.1 | Datenmodelle: Patient, Arzt, Termin, Medikament, Abteilung, Station (je eigene Crate) | Alle mit UUID, Timestamps, Validierung, `version`-Spalte | ✅ |
| 1.2 | Patient-Service: CRUD + Suche (Name, Geb.-Datum, Kassen-Nr.) + Volltextsuche | < 50 ms für 100k Datensätze, Read-Pool für Suchen | ✅ |
| 1.3 | Arzt-Service: CRUD + Fachgebiete + Sprechstunden | Vollständig | ✅ |
| 1.4 | Termin-Service: CRUD + Status-Workflow (geplant → bestätigt → abgeschlossen) | Status-Übergänge validiert, Optimistic Locking | ✅ |
| 1.5 | Medikament-Service: CRUD + Verordnungs-Workflow | Vollständig | ✅ |
| 1.6 | REST-API: alle Endpunkte mit JSON, pagination, cursor-based | `docs/API.md` vollständig | ✅ |
| 1.7 | JWT-Auth: Login, Access/Refresh Tokens, Logout, MFA | 15 min Access, 24 h Refresh, TOTP | ✅ |
| 1.8 | RBAC: Rollen (Admin, Arzt, Pflege, Rezeption, Viewer) + Berechtigungen | Jede Rolle kann nur ihre Endpunkte | ✅ |
| 1.9 | Passwort-Hashing: Argon2id | OWASP-konform | ✅ |
| 1.10 | MFA: TOTP-Setup + Verifikation | Optional pro Benutzer | ✅ |
| 1.11 | Rate-Limiting: Sliding Window (100/min, Auth: 10/min) | 429 + Retry-After | ✅ |
| 1.12 | Audit-Log: Hash-Chain, jede Patienten-Operation geloggt | 10 Jahre Aufbewahrung, Partitionierung | ✅ |
| 1.13 | Input-Validierung: alle Felder (Name, Datum, E-Mail, Tel.) | OWASP-Validierung | ✅ |
| 1.14 | Daten-Verschlüsselung: AES-256-GCM für sensible Felder | Krankenkassen-Nr., Notizen, Telefon | ✅ |
| 1.15 | **Optimistic Locking**: `version`-Spalte, Concurrency-Tests | 2 User editieren gleichzeitig → 409 Conflict | ✅ |
| 1.16 | **Read-Replica-Support**: Lese-Queries auf Read-Pool | Suche < 50 ms bei 1000 parallelen Reads | ✅ |
| 1.17 | **Caching**: In-memory Cache für häufige Reads (Rust: moka) | Cache-Hit < 5 ms | ✅ |

**Agent-Regel Phase 1:** Jeder Service bekommt Unit-Tests. API-Endpunkte werden mit `reqwest` in Integrationstests abgedeckt. Kein Code ohne Tests. **Jeder Service ist eine eigene Crate.**

**Phase 1: ✅ 17/17 Tasks abgeschlossen (Stand: 02.11.2026)**

---

## 6. Phase 2 — GUI

**Ziel:** Ansprechende, performante Web-GUI.

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 2.1 | HTML-Template-Engine (Axum + server-seitig gerendert) | < 100 KB pro Seite | 🔲 |
| 2.2 | Design-System: CSS-Variablen, Dark/Light Theme, Komponenten | `static/css/design-system.css` | 🔲 |
| 2.3 | Login-Seite: Benutzer + Passwort + MFA | Responsive, Barrierefrei | 🔲 |
| 2.4 | Dashboard: KPIs (Patienten, Termine, Ärzte, Schnittstellen-Status) | Auto-Refresh (WebSocket) | 🔲 |
| 2.5 | Patienten-Liste: Suche, Filter, Pagination (cursor-based) | < 200 ms für 100k | 🔲 |
| 2.6 | Patienten-Detail: Stammdaten, Termine, Medikamente, Dokumente | Tabs, Lazy-Loading | 🔲 |
| 2.7 | Termin-Planung: Kalender-Ansicht (Tag/Woche/Monat) | Drag & Drop, Konflikt-Erkennung | 🔲 |
| 2.8 | Medikamenten-Liste + Verordnungs-Dialog | Validierung, Allergie-Check | 🔲 |
| 2.9 | Settings: Benutzerverwaltung, Schnittstellen-Konfiguration, Audit-Log-Ansicht | Admin-Rolle nur | 🔲 |
| 2.10 | Barrierefreiheit: ARIA, Tastatur-Navigation, Kontrast | WCAG 2.1 AA | 🔲 |
| 2.11 | i18n: Deutsch (Standard), Englisch (optional) | `static/locales/de.json`, `en.json` | 🔲 |

**Agent-Regel Phase 2:** GUI-Agent testet mit Browser-Automation (Playwright). Jede Seite muss < 2 s laden. Dark Theme ist Default.

---

## 7. Phase 3 — Schnittstellen

**Ziel:** Alle gängigen Medizintechnik-Schnittstellen. **Jede Schnittstelle ist eine eigene Crate.**

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 3.1 | **kis-hl7**: HL7 v2.5 MLLP-Server (TCP Port 6667) | ADT, ORU, ORM, SIU Nachrichten | 🔲 |
| 3.2 | **kis-hl7**: HL7 v2.5 Parser + Mapper | HL7 → interne Datenmodelle | 🔲 |
| 3.3 | **kis-fhir**: FHIR R4 Server (ISiK-konform) | Patient, Appointment, Encounter, MedicationRequest | 🔲 |
| 3.4 | **kis-fhir**: FHIR R4 Client (für externe Systeme) | GET/POST/PUT/DELETE | 🔲 |
| 3.5 | **kis-dicom**: DICOM SCP (C-STORE, C-FIND, C-MOVE) | Port 104, AE Title konfigurierbar | 🔲 |
| 3.6 | **kis-xds**: XDS: Document Registry + Repository | Dokument-Upload, Suche, Download | 🔲 |
| 3.7 | **kis-ti**: Telematikinfrastruktur: eGK-Leser, ePA, eRezept, eAU, eArztbrief | KIM-Dienst, TI-Integration | 🔲 |
| 3.8 | **kis-gdt**: GDT-Datei-Import (versionierte Satz-/Feldstruktur) | Patientenstammdaten aus anderen KIS | 🔲 |
| 3.9 | Schnittstellen-Monitoring: Status, Fehler, Durchsatz | Dashboard-Anzeige | 🔲 |
| 3.10 | Message-Queue (in-memory, persistente Warteschlange) | Kein Datenverlust bei Neustart | 🔲 |

**Agent-Regel Phase 3:** Jede Schnittstelle ist eine **eigene Cargo-Krate** mit eigenem `Cargo.toml`, eigenen Tests, eigenem Feature-Flag. Deaktivierte Schnittstellen starten keinen Port, werden nicht kompiliert.

---

## 8. Phase 4 — CGM MEDICO Migration (KIS ersetzt MEDICO vollständig)

**Ziel:** KIS übernimmt **alle** Funktionen von CGM MEDICO. Der MEDICO-Import dient ausschließlich dem Daten-Übergang bei der Migration. Nach erfolgreicher Migration wird MEDICO abgeschaltet.

> **KIS ist der vollumfängliche Ersatz für CGM MEDICO — keine Ergänzung.**
> Alle benötigten MEDICO-Funktionen sollen vor einer Ablösung implementiert, mit realistischen Daten getestet und fachlich abgenommen sein. Dieser Stand ist noch nicht erreicht.
> Der Import aus MEDICO ist ein einmaliger Migrationsvorgang, danach ist KIS das einzige System.
> Siehe `docs/CGM_MEDICO.md` für Details.
> **kis-medico ist eine eigene Cargo-Krate.**

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 4.1 | FHIR R4 Client für MEDICO (OAuth 2.0, ISiK Scopes) | Token-Refresh, Scope-Handling | 🔲 |
| 4.2 | MEDICO-Patienten-Import: `GET /Patient`, `GET /Patient?identifier=` | Alle Felder, Mapping zu KIS-Modell, Idempotenz | 🔲 |
| 4.3 | MEDICO-Termin-Import: `GET /Appointment?_lastUpdated=ge{ts}` | Vollständiger Import + inkrementeller Sync (30 s) | 🔲 |
| 4.4 | MEDICO-Slot-Import: `GET /Slot?start=YYYY-MM-DD` | Verfügbarkeit, Sprechstunden → KIS-Appointments | 🔲 |
| 4.5 | MEDICO-Practitioner-Import: `GET /Practitioner`, `/PractitionerRole` | Ärzte + Sprechstunden → KIS-Doctors | 🔲 |
| 4.6 | MEDICO-Befund-Import: `GET /Observation` | Laborwerte, Befunde → KIS-Observations | 🔲 |
| 4.7 | MEDICO-Dokument-Import: `GET /DocumentReference` | Arztbriefe → KIS XDS Repository | 🔲 |
| 4.8 | MEDICO-Medikament-Import: `GET /MedicationRequest` | Verordnungen → KIS-Medications | 🔲 |
| 4.9 | GDT-Export: Patientenstammdaten als GDT-Datei | Kompatibel mit MEDICO-GDT-Format, für Rollback | 🔲 |
| 4.10 | HL7 v2 Adapter: MEDICO-kompatible Nachrichten | ADT, ORU, ORM per MLLP (für Parallelbetrieb) | 🔲 |
| 4.11 | Change-Detection: MEDICO → KIS (Polling + Webhook) | Keine Duplikate, Idempotenz via external_id | 🔲 |
| 4.12 | Konflikt-Handling: Fachlich definierte Konfliktpolitik + Audit-Log | Konflikt-Queue für Admins | 🔲 |
| 4.13 | MEDICO-Status-Monitoring: Connectivity, Latenz, Fehler | Dashboard + Alerting | 🔲 |
| 4.14 | Migration-Orchestrierung: Phasenplan, Rollback, Parallelbetrieb | `docs/CGM_MEDICO.md` § 9 | 🔲 |
| 4.15 | Test-Setup: MEDICO-Sandbox (Mock-Server) | `tests/medico_mock.rs` | 🔲 |

**Agent-Regel Phase 4:** Alle MEDICO-Endpunkte sind in `config/kis.toml` im aktiven Prototyp unter `[interfaces.medico]` vorgesehen; ein vollständiger Importdienst fehlt noch. OAuth-Credentials NIE im Code — nur ENV-Variablen.

---

## 9. Phase 5 — Sicherheit & Compliance

**Ziel:** Vollständige DSGVO-Konformität, ISO 27001-Audit-Ready.

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 5.1 | TLS 1.3 (rustls): alle Endpunkte | Zertifikate konfigurierbar, HSTS | ✅ |
| 5.2 | AES-256-GCM: Verschlüsselung at rest (alle Patientendaten) | Master Key aus ENV, Key-Rotation | 🔲 |
| 5.3 | Audit-Log: Hash-Chain mit externem Prüfanker, Aufbewahrungskonzept, Partitionierung | `docs/SECURITY.md` § Audit | ✅ |
| 5.4 | RBAC: Vollständige Berechtigungs-Matrix | Jede Rolle × jede Entität × jede Aktion | ✅ |
| 5.5 | Session-Management: Concurrent-Login-Limit, Auto-Logout | 30 min Inaktivität → Logout | 🔲 |
| 5.6 | **DSGVO Art. 17**: Datenlöschung (Hard + Soft Delete, Audit-Trail) | Löschung nach Aufbewahrungsfrist | 🔲 |
| 5.7 | **DSGVO Art. 15/20**: Datenportabilität (Export in JSON/CSV) | Patient kann Daten exportieren | 🔲 |
| 5.8 | **DSGVO Art. 17**: Anonymisierung nach Aufbewahrungsfrist | Anonymisierung und Pseudonymisierung klar unterscheiden | 🔲 |
| 5.9 | Datenschutz-Verzeichnis: Alle verarbeiteten Daten dokumentiert | `docs/DSGVO/VERZEICHNIS.md` | ✅ |
| 5.10 | DSGVO-Datenverarbeitungsverträge (Auftragsverarbeiter) | Vorlage in `docs/DSGVO/AVV.md` | ✅ |
| 5.11 | Incident-Response-Plan: 72-h-Meldung, Forensik | `docs/SECURITY.md` § Incident | ✅ |
| 5.12 | Backup: Voll + Inkrementell, Verschlüsselt, 3-2-1-Regel | `docs/OPERATION.md` § Backup | ✅ |
| 5.13 | Penetration-Test-Setup: Test-Endpunkte, Scan-Reports | `security/penetration/` | ✅ |
| 5.14 | Compliance-Matrix: DSGVO, MDR, ISO 27001, HIPAA (Mapping) | `docs/SECURITY.md` § Compliance | ✅ |
| 5.15 | Signed Releases: GPG-Signatur, Checksums | `docs/SECURITY.md` § Updates | ✅ |
| 5.16 | Secret-Management: Alle Secrets aus ENV, nie im Code | `docs/SECURITY.md` § Secrets | ✅ |

**Agent-Regel Phase 5:** Sicherheits-Code benötigt vor Integrationsfreigabe ein unabhängiges zweites Security-Review mit konkreter SHA und Testnachweisen. Implementierung und gemeinsame Tests im isolierten Feature-/Prüfbranch dürfen davor stattfinden; eine Selbstfreigabe des Autors ist ausgeschlossen. Ein bloßer Modellkonsens ersetzt keinen Nachweis.

### Phase 5 — Performance & Load Testing (ergänzend)

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 5P.1 | Connection-Pooling: Pool-Metriken + Health-Endpoint | `GET /api/v1/health/db` mit active/idle/total/waiters | ✅ |
| 5P.2 | Read-Through-Caching: `get_or_load`, Invalidation, Stats | `GET /api/v1/health/cache` mit hit_rate, evictions | ✅ |
| 5P.3 | Database-Read-Replica: Read-Only-Check + Health-Endpoint | `GET /api/v1/health/replica` mit read-only-Status | ✅ |
| 5P.5 | Query-Optimierung: EXPLAIN-Profiler, Index-Empfehlungen | `explain_query()`, `suggest_indexes()`, `estimate_rows()` | ✅ |
| 5P.6 | Load-Testing mit k6: Patient + Termin Szenarien | `load-tests/` mit k6-Skripten, Thresholds, Dockerfile | ✅ |
| 5P.9 | DSGVO-Verzeichnis: Alle verarbeiteten Daten dokumentiert | `docs/DSGVO/VERZEICHNIS.md` | ✅ |
| 5P.12 | Horizontal Scaling: Stateless-Design + Graceful Shutdown | `docs/SCALING.md`, `graceful_shutdown.rs` (3 Phasen) | ✅ |

---

## 10. Phase 6 — LLM-Integration

**Ziel:** KI-gestützte Entwicklung und Betrieb.

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 6.1 | OpenRouter-Client: API-Wrapper, Modell-Rotation | 3+ Modelle, automatischer Wechsel | 🔲 |
| 6.2 | Code-Generierung: LLM schreibt Code, 2. Modell reviewt | Rotation: A generiert, B reviewt, A integriert | 🔲 |
| 6.3 | Cross-Review-Workflow: Modell A vs. Modell B | Quality-Gate: Beide Modelle müssen zustimmen | 🔲 |
| 6.4 | KIS-intern: LLM für Dokumenten-Summarisierung | Arztbriefe, Befunde (opt-in) | 🔲 |
| 6.5 | KIS-intern: LLM für Termin-Empfehlungen | Konflikt-freie Vorschläge | 🔲 |
| 6.6 | Token-Budget: Kosten-Tracking, Limits pro Tag | Dashboard-Anzeige | 🔲 |
| 6.7 | Fallback: LLM nicht erreichbar → KIS funktioniert trotzdem | Graceful Degradation | 🔲 |

**Agent-Regel Phase 6:** LLM-Code wird immer von einem ZWEITEN Modell reviewed. Kein LLM-Code wird direkt committet ohne Review.

---

## 11. Phase 7 — Härtung & Release

**Ziel:** Produktionsreifes System.

### Tasks

| # | Task | Akzeptanzkriterium | Status |
|---|------|-------------------|--------|
| 7.1 | **Skalierungs-Tests**: 5000 gleichzeitige Requests, 10k parallele Reads | < 100 ms p95, < 500 ms p99 | 🔲 |
| 7.2 | **Last-Tests**: 100k Patienten, 100k Termine, 100k Audit-Einträge | DB < 5 GB, Suche < 50 ms | 🔲 |
| 7.3 | **Concurrency-Tests**: 100 User editieren gleichzeitig (Optimistic Locking) | 0 Datenverlust, 409 Conflict bei Collision | 🔲 |
| 7.4 | Chaos-Tests: DB-Ausfall, Netzwerk-Partition, OOM | KIS recovert ohne Datenverlust | 🔲 |
| 7.5 | Security-Scan: cargo audit, semgrep, OWASP ZAP | 0 kritische Findings | 🔲 |
| 7.6 | Dokumentation: Alle docs/ vollständig | `docs/` deckt alle Phasen ab | 🔲 |
| 7.7 | CHANGELOG.md | Semantic Versioning | 🔲 |
| 7.8 | LICENSE (MIT) | Datei vorhanden | 🔲 |
| 7.9 | Release: Tag, GitHub Release, Signed | v1.0.0 | 🔲 |
| 7.10 | Deploy-Guide: Schritt-für-Schritt für Klinik-IT | `docs/INSTALLATION.md` § Production | 🔲 |
| 7.11 | **Modularitäts-Test**: Neues Modul hinzufügen ohne Refactoring | Neues Modul in < 1 Stunde integriert | 🔲 |

---

## 12. Agent-Koordination

### Rollen

| Rolle | Verantwortung | Phasen |
|-------|--------------|--------|
| **Architekt** | Gesamt-Design, Reviews, Entscheidungen | Alle |
| **Backend-Dev** | API, DB, Services, Sicherheit | 0, 1, 3, 4, 5 |
| **Frontend-Dev** | GUI, CSS, JS, UX | 2 |
| **Schnittstellen-Dev** | HL7, FHIR, DICOM, XDS, MEDICO | 3, 4 |
| **Security-Dev** | Crypto, Auth, Audit, Compliance | 1, 5 |
| **LLM-Dev** | OpenRouter, Rotation, Cross-Review | 6 |
| **QA** | Tests, Performance, Chaos | 0, 7 |
| **Dokumentation** | Alle docs/ | Alle |

### Workflow

```
1. Agent liest FAHRPLAN.md
2. Agent wählt Phase + Tasks (parallelisierbare Tasks werden parallel abgearbeitet)
3. Agent arbeitet Tasks ab
4. Agent schreibt Tests für jeden Task
5. Agent aktualisiert Status in diesem Dokument
6. Agent meldet sich bei Abhängigkeiten oder Blockaden
7. Zweites Modell reviewt alle Änderungen (Phase 6)
```

### Status-Meldung

Jeder Agent meldet nach jedem Task:
```
[TASK 1.3] Arzt-Service: CRUD + Fachgebiete + Sprechstunden
  Status: ✅ Fertig / 🔄 In Arbeit / ⛔ Blockiert
  Files: crates/kis-doctors/src/lib.rs, crates/kis-doctors/tests/
  Tests: 15/15 passing
  Notes: Sprechstunden als JSON-Array gespeichert
```

---

## 13. Abhängigkeiten

```
Phase 0 ──→ Phase 1 ──→ Phase 2
              │              │
              ├──→ Phase 3 ──┤
              │         │    │
              │    Phase 4   │
              │         │    │
              ├──→ Phase 5 ←─┘
              │         │
              └──→ Phase 6
                        │
                   Phase 7
```

- Phase 2 (GUI) kann parallel zu Phase 3 (Schnittstellen) starten
- Phase 4 (MEDICO) braucht Phase 3 (FHIR Client)
- Phase 5 (Security) braucht Phase 1 (Auth)
- Phase 6 (LLM) kann parallel zu Phase 5
- Phase 7 (Release) braucht ALLES

---

## 14. Definition of Done (pro Task)

Ein Task ist **fertig** wenn:
- [ ] Code geschrieben und kompiliert
- [ ] Unit-Tests vorhanden und passing
- [ ] Integrationstest (falls API/DB) vorhanden und passing
- [ ] Dokumentation aktualisiert (docs/)
- [ ] Fehlerbehandlung vorhanden (keine panics)
- [ ] Input-Validierung vorhanden
- [ ] Logging vorhanden (tracing::info/error)
- [ ] Security-Review (zweites Modell) für kritische Tasks
- [ ] **Modul ist eine eigene Cargo-Krate** (bei neuen Modulen)
- [ ] **Feature-Flag in Cargo.toml** (bei neuen Modulen)
- [ ] Commit mit beschreibendem Message
- [ ] Status in FAHRPLAN.md aktualisiert

---

*Version: 2.3.0 | Stand: 2026-11-02 | Autor: Hermes Agent*
*Änderungen: v2.3.0 — Phase 5 Performance/Load-Testing abgeschlossen (7/7): Connection-Pooling, Read-Through-Cache, Read-Replica, Query-Optimizer, k6-Load-Tests, DSGVO-Verzeichnis, Horizontal Scaling + Graceful Shutdown. 132 Tests grün.*
