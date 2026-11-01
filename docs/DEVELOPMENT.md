# KIS — Entwicklerhandbuch

> **Krankenhaus-Informations-System (KIS)**
> Entwicklerhandbuch: Projektstruktur, Coding-Konventionen, Teststrategie, LLM-gestützter Entwicklungs-Workflow (Modell-Rotation, Cross-Review) und CI/CD-Pipeline.

---

## 1. Projektstruktur

```
kis/
├── Cargo.toml              # Abhängigkeiten, Profil-Definitionen
├── config/
│   └── kis.toml            # Laufzeitkonfiguration (Default-Werte)
├── data/                   # Laufzeitdaten (SQLite, Logs) — .gitignore
│   └── kis.db              # Datenbankdatei
├── migrations/
│   └── 001_init.sql        # DB-Migrations (SQLX)
├── static/
│   └── index.html          # Frontend-Entry (GUI)
├── src/
│   ├── main.rs             # Binary-Entry-Point
│   ├── lib.rs              # Bibliotheken-Export
│   ├── api/
│   │   └── mod.rs          # Axum-Router + Handler (REST)
│   ├── config/
│   │   └── mod.rs          # Figment-basierte Konfigurations-Ladung
│   ├── db/
│   │   └── mod.rs          # SQLX-Pool, Migration
│   ├── models/
│   │   └── mod.rs          # Datenmodelle (Patient, Doctor, ...)
│   ├── ui/
│   │   └── mod.rs          # GUI-Modul (statische Assets, Theming)
│   └── schnittstellen/
│       ├── mod.rs          # Schnittstellen-Modul
│       ├── hl7.rs          # HL7 v2.5 (MLLP) — In Entwicklung
│       ├── fhir.rs         # FHIR R4 — In Entwicklung
│       ├── dicom.rs        # DICOM C-STORE SCP — In Entwicklung
│       └── xds.rs          # XDS — In Entwicklung
├── docs/                   # Dieses Dokumentationspaket
│   ├── README.md
│   ├── ARCHITECTURE.md
│   ├── API.md
│   ├── CONFIGURATION.md
│   ├── DATABASE.md
│   ├── DEVELOPMENT.md
│   ├── INSTALLATION.md
│   ├── INTERFACES.md
│   ├── OPERATION.md
│   └── SECURITY.md
├── .gitignore
└── README.md
```

### 1.1 Module und Verantwortung

| Modul | Pfad | Verantwortung |
|---|---|---|
| `main` | `src/main.rs` | Binary-Entry: Config laden, DB init, Server starten |
| `api` | `src/api/mod.rs` | Axum-Router, HTTP-Handler, CORS, Request-Handling |
| `config` | `src/config/mod.rs` | Figment: TOML + ENV → `Config`-Struktur |
| `db` | `src/db/mod.rs` | SQLX-SqlitePool, Migration, Connection-Management |
| `models` | `src/models/mod.rs` | Ser/Deser-Strukturen, DB-Mapping (`FromRow`) |
| `ui` | `src/ui/mod.rs` | Statische Assets, GUI-Theming, Frontend-Service |
| `schnittstellen::hl7` | `src/schnittstellen/hl7.rs` | HL7 v2.5 MLLP-Parser/Encoder (In Entwicklung) |
| `schnittstellen::fhir` | `src/schnittstellen/fhir.rs` | FHIR R4 Resource-Mapping (In Entwicklung) |
| `schnittstellen::dicom` | `src/schnittstellen/dicom.rs` | DICOM SCP (In Entwicklung) |
| `schnittstellen::xds` | `src/schnittstellen/xds.rs` | XDS Registry/Repository (In Entwicklung) |

---

## 2. Coding-Konventionen

### 2.1 Sprache und Style

- **Rust Edition:** 2021
- **Formatter:** `rustfmt` (Standard-Style, `cargo fmt`)
- **Linter:** `clippy` (Warnungen = Fehler in CI)
- **Namenskonventionen:**
  - Module / Dateien: `snake_case`
  - Typen / Strukturen: `PascalCase`
  - Funktionen / Variablen: `snake_case`
  - Konstanten: `SCREAMING_SNAKE_CASE`

### 2.2 Fehlerbehandlung

- **Kein `unwrap()` / `expect()`** in Produktionscode (nur in Tests / `main`-Bootstrap).
- Fehler werden mit `?` propagiert oder in einen App-spezifischen `AppError` umgewandelt.
- HTTP-Handler mappen `AppError` auf HTTP-Status + JSON-Fehlerformat (siehe [API.md](API.md) → Fehlercodes).

```rust
// ✅ Empfohlen
async fn get_patient(pool: &SqlitePool, id: Uuid) -> Result<Patient, AppError> {
    let patient = sqlx::query_as::<_, Patient>(
        "SELECT ... FROM patients WHERE id = ?"
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::from(e))?;
    Ok(patient)
}

// ❌ Verboten
let patient = sqlx::query_as::<_, Patient>(...)
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
```

### 2.3 Datenbank-Zugriff

- **Immer** Prepared Statements via SQLX (keine String-Konkatenation).
- **Immer** `bind()` für Parameter.
- UUIDs als `Uuid`-Typ (uuid-Crate), nicht als String.
- Zeitstempel als ISO 8601-String (UTC).
- Transaktionen für mehrfache Schreiboperationen:

```rust
let mut tx = pool.begin().await?;
// ... mehrere Schreiboperationen ...
tx.commit().await?;
```

### 2.4 Asynchronität

- **Tokio**-Runtime, Multi-Thread.
- Alle I/O-Operationen sind `async`.
- CPU-lastige Operationen (z. B. DICOM-Decode) in `spawn_blocking`.
- Timeout für externe Aufrufe (LLM, HL7, ...) setzen.

### 2.5 Logging

- **tracing** (nicht log).
- Log-Level pro Modul konfigurierbar.
- **Nie** Patientendaten in Klartext loggen (DSGVO).
- Strukturierter Kontext:

```rust
use tracing::{info, warn, error, instrument};

#[instrument(skip(pool), fields(patient_id = %id))]
async fn get_patient(pool: &SqlitePool, id: Uuid) -> Result<Patient, AppError> {
    // ...
}
```

### 2.6 Sicherheit (Code-Ebene)

- Keine Secrets im Code (nur Env-Variablen).
- Validierung aller Benutzereingaben vor DB-Zugriff.
- RBAC-Check vor jeder Daten-Operation (Soll).
- Audit-Log-Eintrag bei jeder Lese-/Schreiboperation auf Patientendaten.

### 2.7 Kommentierung

- Module mit `//!` (dok-Kommentar) beschreiben.
- Öffentliche API-Funktionen mit `///` dokumentieren.
- Nicht-triviale Logik mit `//` erklären (Warum, nicht Was).
- `TODO`-Kommentare mit Issue-Referenz: `// TODO(#42): ...`

---

## 3. Teststrategie

### 3.1 Test-Pyramide

```
        /\
       /  \        E2E-Tests (wenige)
      /----\       (cargo test --test e2e)
     /      \
    /--------\     Integrationstests (mittleres Volumen)
   /          \    (DB + API, in-process)
  /------------\
 /              \  Unit-Tests (viele)
/________________\ (pure functions, Parsing)
```

### 3.2 Unit-Tests

- Für reine Funktionen (Parsing, Validierung, Mapping).
- Im selben File (`#[cfg(test)]`) oder in `tests/`.
- Schnell, isoliert, keine I/O.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_birth_date() {
        assert_eq!(parse_hl7_date("19850312"), "1985-03-12");
        assert!(parse_hl7_date("invalid").is_err());
    }
}
```

### 3.3 Integrationstests

- In `tests/`-Ordner.
- Testen API + DB zusammen.
- Verwenden eine **in-memory SQLite**-DB (`:memory:`) oder temporäre Datei.
- Jeden Test mit einer frischen DB.

```rust
#[tokio::test]
async fn test_create_and_get_patient() {
    let pool = setup_test_pool().await;   // in-memory SQLite
    let app = build_app(pool).await;

    // POST /api/patients
    let req = Request::builder()
        .method("POST")
        .uri("/api/patients")
        .body(Body::from(r#"{"first_name":"Anna","last_name":"Müller"}"#))
        .unwrap();
    let res = app.call(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // GET /api/patients/:id
    // ...
}
```

### 3.4 E2E-Tests

- Starten den echten Server (auf zufälligem Port).
- Testen über HTTP (reqwest / curl).
- Wenige, aber kritische Pfade (Login → Patient anlegen → Termin).

### 3.5 Test-Kommandos

```bash
# Alle Tests
cargo test

# Unit-Tests nur
cargo test --lib

# Integrationstests
cargo test --tests

# Mit Output
cargo test -- --nocapture

# Einzelner Test
cargo test test_create_and_get_patient

# Coverage
cargo install cargo-llvm-cov
cargo llvm-cov
```

### 3.6 Test-Ziele

| Bereich | Abdeckung |
|---|---|
| Modelle (Ser/Deser) | 100 % |
| API-Handler | ≥ 90 % |
| Config-Parsing | 100 % |
| HL7-Parsing | 100 % (alle Nachrichtentypen) |
| FHIR-Mapping | 100 % |
| DB-Migrationen | 100 % (Up + Down) |

---

## 4. LLM-gestützter Entwicklungs-Workflow

Das KIS-Projekt nutzt ein **LLM-Cluster** (OpenRouter) zur Unterstützung der Entwicklung. Drei Modelle sind konfiguriert und rotieren:

| Modell | Rolle |
|---|---|
| `deepseek/deepseek-chat-v3-0324` | Allrounder, Code-Generierung |
| `qwen/qwen3-coder` | Code-spezifisch, Refactoring |
| `meta-llama/llama-4-maverick` | Cross-Review, alternative Perspektive |

### 4.1 Modell-Rotation

```toml
[llm]
enabled = true
provider = "openrouter"
api_key_env = "OPENROUTER_API_KEY"   # Secret über Env (NIEMALS in TOML!)
models = [
    "deepseek/deepseek-chat-v3-0324",
    "qwen/qwen3-coder",
    "meta-llama/llama-4-maverick",
]
rotation_interval = 300
max_tokens = 4096
temperature = 0.7
```

- **Rotation:** Alle 300 Sekunden (5 min) wird das aktive Modell gewechselt.
- **Zweck:** Vermeidung von Bias eines einzelnen Modells, Lastverteilung, höhere Robustheit.
- **API-Key:** Exklusiv über `OPENROUTER_API_KEY`-Env (siehe [SECURITY.md](SECURITY.md) → Secret-Management).

### 4.2 Cross-Review-Workflow

```
┌─────────────────┐
│  1. Code-Entwurf│  ← Aktives LLM-Modell (z. B. deepseek)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  2. Cross-Review│  ← Anderes LLM-Modell (z. B. qwen3-coder)
│  prüft auf:     │     prüft auf:
│  - Bugs         │     - Korrektheit
│  - Sicherheit   │     - Sicherheit (DSGVO, RBAC)
│  - Performance  │     - Performance
│  - Style        │     - Style (rustfmt, clippy)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  3. Konsolidierung│ ← Menschliches Review
│  entscheidet     │     (letzte Instanz)
└─────────────────┘
```

**Regeln für das Cross-Review:**
- Das Review-Modell erhält **nicht** die Antwort des Entwurfs-Modells (unabhängige Bewertung).
- Der Reviewer soll spezifische Probleme nennen, keine allgemeinen Lob.
- Kritische Punkte (Sicherheit, DSGVO) müssen **explizit** benannt werden.
- Der menschliche Entwickler trifft die finale Entscheidung.

### 4.3 LLM-Nutzung im Code (Soll)

Für LLM-gestützte Funktionen (z. B. automatische Kodierung von Diagnosen, Termin-Optimierung) gilt:

- **Nie** Patientendaten ungeprüft an ein externes LLM-Backend senden.
- Pseudonymisierung vor dem Senden (Patient-ID → anonymen Token).
- Antwortvalidierung (JSON-Schema) vor Nutzung.
- Fallback auf manuellen Workflow bei LLM-Fehler.

```rust
// Pseudocode (Soll)
async fn llm_assist(pool: &SqlitePool, patient_id: Uuid, query: &str) -> Result<String, AppError> {
    // 1. Pseudonymisierung
    let pseudo_id = pseudonymize(patient_id)?;
    // 2. LLM-Aufruf (aktives rotiertes Modell)
    let active_model = current_llm_model().await?;
    let response = openrouter_chat(active_model, &format!(
        "Patient {}: {}", pseudo_id, query
    )).await?;
    // 3. Antwortvalidierung
    let validated = validate_llm_response(&response)?;
    Ok(validated)
}
```

> **DSGVO-Hinweis:** Die Übermittlung von Gesundheitsdaten an ein externes LLM-Backend erfordert eine **Auftragsverarbeitung** (Art. 28 DSGVO) mit dem Anbieter. Vor Produktivbetrieb rechtlich klären.

### 4.4 LLM-Prompting-Konventionen

| Aspekt | Konvention |
|---|---|
| Sprache | Deutsch (medizinisch-technisch) |
| Kontext | Immer Modul + Ziel benennen |
| Constraints | Sicherheitsanforderungen explizit nennen |
| Format | Erwartetes Ausgabeformat (JSON, Code) angeben |
| Iteration | Max. 3 Iterationen, dann menschliches Review |

---

## 5. CI/CD-Pipeline

### 5.1 Pipeline-Übersicht

```
[Commit] → [PR] → [CI] → [Merge] → [Release] → [Deploy]
             │
             ├── Lint (clippy)
             ├── Format (rustfmt)
             ├── Unit-Tests
             ├── Integrationstests
             ├── Security-Scan (cargo-audit)
             └── Build (release)
```

### 5.2 CI-Schritte (GitHub Actions)

```yaml
# .github/workflows/ci.yml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Format check
        run: cargo fmt --check
      - name: Clippy
        run: cargo clippy -- -D warnings
      - name: Security audit
        run: |
          cargo install cargo-audit
          cargo audit
      - name: Build
        run: cargo build --release
      - name: Test
        run: cargo test
```

### 5.3 Release

```yaml
  release:
    needs: check
    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build
        run: cargo build --release
      - name: Sign
        run: |
          # GPG/Minisign-Signatur
          minisign -s secret.key -m target/release/kis
      - name: Checksum
        run: sha256sum target/release/kis > kis.sha256
      - name: Upload
        uses: softprops/action-gh-release@v2
        with:
          files: |
            target/release/kis
            target/release/kis.minisig
            kis.sha256
```

### 5.4 Deployment (Soll)

```yaml
  deploy:
    needs: release
    if: github.ref == 'refs/heads/main'
    runs-on: ubuntu-latest
    environment: production
    steps:
      - name: Deploy
        run: |
          # 1. Neues Release herunterladen
          # 2. Prüfsumme + Signatur verifizieren
          # 3. Systemd-Dienst neu starten
          # 4. Health-Check
          curl -fsS http://localhost:8080/api/health
```

### 5.5 Branch-Strategie

| Branch | Zweck |
|---|---|
| `main` | Produktiv, stets deploybar |
| `develop` | Integration |
| `feature/*` | Neue Features |
| `fix/*` | Bugfixes |
| `hotfix/*` | Kritische Patches (direkt nach `main`) |

**Merge-Regeln:**
- Nur via Pull Request.
- Mindestens 1 Review (menschlich).
- CI muss grün sein.
- Squash-Merge für Features/Fixes.

---

## 6. Lokale Entwicklungsumgebung

### 6.1 Einrichtung

```bash
# 1. Repository klonen
git clone https://github.com/<org>/kis.git
cd kis

# 2. Abhängigkeiten installieren
cargo build

# 3. Env-Variable setzen (LLM)
export OPENROUTER_API_KEY="sk-..."

# 4. Migrations ausführen
sqlite3 data/kis.db < migrations/001_init.sql

# 5. Server starten
cargo run
# → http://localhost:8080
```

### 6.2 Entwicklungshilfen

| Werkzeug | Befehl | Zweck |
|---|---|---|
| `cargo fmt` | `cargo fmt` | Code formatieren |
| `cargo clippy` | `cargo clippy` | Lint |
| `cargo test` | `cargo test` | Tests |
| `cargo watch` | `cargo install cargo-watch && cargo watch -x run` | Auto-Reload |
| `sqlite3` | `sqlite3 data/kis.db` | DB-Interaktiv |
| `curl` | `curl http://localhost:8080/api/health` | API-Test |

### 6.3 Debugging

```bash
# Log-Level erhöhen
export RUST_LOG=kis=debug
cargo run

# Oder in config/kis.toml:
# [server]
# log_level = "debug"
```

---

## 7. Dokumentation

- **Code-Doku:** `cargo doc --open`
- **API-Doku:** [docs/API.md](API.md)
- **Architektur:** [docs/ARCHITECTURE.md](ARCHITECTURE.md)
- **Konfiguration:** [docs/CONFIGURATION.md](CONFIGURATION.md)
- **Sicherheit:** [docs/SECURITY.md](SECURITY.md)
- **Datenbank:** [docs/DATABASE.md](DATABASE.md)

> **Regel:** Jede neue API-Endpunkt / Konfigurations-Option / Schnittstelle wird **gleichzeitig** in der Dokumentation vermerkt. Dokumentation ist Teil des PRs.

---

## 8. Checkliste für Pull Requests

| # | Punkt |
|---|---|
| 1 | `cargo fmt --check` grün |
| 2 | `cargo clippy -- -D warnings` grün |
| 3 | `cargo test` grün |
| 4 | `cargo audit` ohne kritische Befunde |
| 5 | Neue Endpunkte in [API.md](API.md) dokumentiert |
| 6 | Neue Config-Optionen in [CONFIGURATION.md](CONFIGURATION.md) dokumentiert |
| 7 | Neue DB-Spalten/Tabellen in [DATABASE.md](DATABASE.md) dokumentiert |
| 8 | Sicherheitsrelevante Änderungen in [SECURITY.md](SECURITY.md) dokumentiert |
| 9 | Keine Secrets im Code / in der TOML |
| 10 | Audit-Log-Einträge für neue Daten-Operationen |
| 11 | RBAC-Checks für neue Endpunkte |
| 12 | Changelog-Eintrag |

---

**Verwandte Dokumente:** [API.md](API.md) · [SECURITY.md](SECURITY.md) · [DATABASE.md](DATABASE.md) · [OPERATION.md](OPERATION.md)
