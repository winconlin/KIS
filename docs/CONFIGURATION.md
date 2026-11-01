# KIS — Konfiguration

> **Krankenhaus-Informations-System (KIS)**
> Vollständiger Referenz der Konfiguration in `config/kis.toml`. Alle Felder, ihre Datentypen, Standardwerte, ENV-Overrides und sicherheitsrelevante Hinweise.

---

## 1. Konfigurationsquellen und Priorität

Das KIS lädt seine Konfiguration über [figment](https://docs.rs/figment) (siehe `src/config/mod.rs` → `AppConfig::load()`). Die Quellen werden in folgender Reihenfolge gemergt (höher = später gemergt = überschreibt):

| Priorität | Quelle | Beschreibung |
|---|---|---|
| 3 (höchste) | Umgebungsvariablen `KIS_*` | Laufzeit-Override, z. B. für Secrets und Deploy-Spezifika |
| 2 | `config/kis.toml` | Hauptkonfigurationsdatei (im Projektordner bzw. `WorkingDirectory`) |
| 1 (niedrigste) | `Default`-Implementierung | Rust-`Default`-Traits der Config-Strukturen in `src/config/mod.rs` |

> **Wichtig:** Fehlt `config/kis.toml`, werden die `Default`-Werte verwendet. Fehlt eine ENV-Variable, greift der TOML-Wert. Fehlt auch dieser, greift der Default.

### ENV-Override-Mapping

figment liest ENV-Variablen mit Präfix `KIS_`. Die Mapping-Regel für verschachtelte TOML-Felder lautet: **Abschnitte werden mit `__` (doppeltem Unterstrich) getrennt**.

| TOML-Pfad | ENV-Variable | Beispiel |
|---|---|---|
| `server.host` | `KIS_SERVER__HOST` | `KIS_SERVER__HOST=127.0.0.1` |
| `server.port` | `KIS_SERVER__PORT` | `KIS_SERVER__PORT=9090` |
| `server.workers` | `KIS_SERVER__WORKERS` | `KIS_SERVER__WORKERS=2` |
| `database.path` | `KIS_DATABASE__PATH` | `KIS_DATABASE__PATH=/var/lib/kis/kis.db` |
| `database.pool_size` | `KIS_DATABASE__POOL_SIZE` | `KIS_DATABASE__POOL_SIZE=16` |
| `ui.theme` | `KIS_UI__THEME` | `KIS_UI__THEME=light` |
| `interfaces.hl7.enabled` | `KIS_INTERFACES__HL7__ENABLED` | `KIS_INTERFACES__HL7__ENABLED=true` |
| `interfaces.hl7.port` | `KIS_INTERFACES__HL7__PORT` | `KIS_INTERFACES__HL7__PORT=6667` |
| `interfaces.fhir.enabled` | `KIS_INTERFACES__FHIR__ENABLED` | `KIS_INTERFACES__FHIR__ENABLED=false` |
| `interfaces.dicom.ae_title` | `KIS_INTERFACES__DICOM__AE_TITLE` | `KIS_INTERFACES__DICOM__AE_TITLE=KIS_01` |
| `llm.rotation_interval` | `KIS_LLM__ROTATION_INTERVAL` | `KIS_LLM__ROTATION_INTERVAL=600` |

> **Hinweis:** Secrets wie API-Keys werden **nicht** direkt als ENV-Override für ein TOML-Feld gesetzt, sondern über die in der Konfiguration referenzierte **Name der Env-Variable** (siehe `[llm].api_key_env`). Das KIS liest den Wert zur Laufzeit aus dieser ENV — der Key selbst steht also nie in der `kis.toml`.

### Logging

Das Logging-Setup (tracing) liest separat die Standard-Env `RUST_LOG` (nicht `KIS_`-geprägt), da `tracing_subscriber` das übernimmt:

```bash
RUST_LOG=info                 # Standard: nur Info und höher
RUST_LOG=kis=debug            # Modul-spezifisch
RUST_LOG=info,sqlx=debug      # Kombination
```

---

## 2. Komplettes Referenz: `config/kis.toml`

### 2.0 Vollständiges Beispiel (Ist-Stand)

```toml
# KIS Konfiguration
# Alle Werte können per KIS_*_ENV übergeben werden

[server]
host = "0.0.0.0"
port = 8080
workers = 4

[database]
path = "data/kis.db"
pool_size = 8
journal_mode = "wal"

[ui]
theme = "dark"
language = "de"
auto_refresh = true

[interfaces.hl7]
enabled = false
port = 6667
version = "2.5"

[interfaces.fhir]
enabled = true
base_url = "http://localhost:8080/fhir"
version = "R4"

[interfaces.dicom]
enabled = false
port = 104
ae_title = "KIS"

[llm]
provider = "openrouter"
api_key_env = "OPENROUTER_API_KEY"
rotation_interval = 300
models = [
    "deepseek/deepseek-chat-v3-0324",
    "qwen/qwen3-coder",
    "meta-llama/llama-4-maverick"
]
```

---

### 2.1 Abschnitt `[server]`

HTTP-Server-Einstellungen (Axum). Struktur: `ServerConfig` in `src/config/mod.rs`.

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `host` | `String` | `"0.0.0.0"` | IP-Adresse, an die der Server bindet. `0.0.0.0` = alle Interfaces (Produktion). `127.0.0.1` = nur lokal (Entwicklung). |
| `port` | `u16` | `8080` | HTTP-Port für GUI, API und FHIR (R4). |
| `workers` | `usize` | `4` | Anzahl der Tokio-Worker-Threads. Empfehlung: Anzahl der physischen CPU-Kerne. |

**Sicherheit:** Bei `host = "0.0.0.0"` **muss** der Server hinter einem TLS-Terminator oder Reverse-Proxy betrieben werden (siehe [SECURITY.md](SECURITY.md) → Netzwerk). Für rein lokale Entwicklung `127.0.0.1` verwenden.

---

### 2.2 Abschnitt `[database]`

SQLite-Datenbank-Einstellungen. Struktur: `DatabaseConfig`.

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `path` | `String` (PathBuf) | `"data/kis.db"` | Pfad zur SQLite-Datei. Relativ zum `WorkingDirectory` oder absolut. |
| `pool_size` | `usize` | `8` | Maximale Anzahl paralleler Verbindungen im Connection-Pool. Empfehlung: `workers × 2`. |
| `journal_mode` | `String` | `"wal"` | SQLite-Journal-Modus. Erlaubt: `wal`, `delete`, `memory`, `off`. **Nur `wal` in Produktion.** |

**Sicherheit:** `journal_mode = "wal"` erlaubt paralleles Lesen während Schreibvorgängen und erhöht die Datenintegrität. `off` **niemals** verwenden (Verlust bei Abbruch). Details: [DATABASE.md](DATABASE.md) → WAL-Modus.

> **Hinweis:** Die Datei `data/kis.db` sowie die WAL-/SHM-Sidecar-Dateien (`kis.db-wal`, `kis.db-shm`) müssen dem ausführenden Benutzer beschreibbar sein.

---

### 2.3 Abschnitt `[ui]`

Benutzeroberflächen-Einstellungen. Struktur: `UiConfig`. Die GUI ist server-seitig generiertes HTML (siehe `src/ui/mod.rs` und `static/index.html`).

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `theme` | `String` | `"dark"` | Farbtheme der GUI. Erlaubt: `dark`, `light`. |
| `language` | `String` | `"de"` | Anzeigesprache. Aktuell implementiert: `de` (Deutsch). |
| `auto_refresh` | `bool` | `true` | Automatische Aktualisierung der Dashboard-Kacheln (Patienten, Termine, Ärzte). |

**Bezug zur GUI:** `static/index.html` implementiert das Dark Theme mit CSS-Variablen (`--bg`, `--surface`, `--accent` etc.). `auto_refresh` steuert die periodische Abfrage der API für die Kachelwerte.

---

### 2.4 Abschnitt `[interfaces]`

Medizinische Schnittstellen. Struktur: `InterfaceConfig` mit Unterstrukturen `Hl7Config`, `FhirConfig`, `DicomConfig`.

#### 2.4.1 `[interfaces.hl7]` — HL7 v2.5 (MLLP)

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `enabled` | `bool` | `false` | Schaltet das HL7-Interface ein/aus. |
| `port` | `u16` | `6667` | TCP-Port für MLLP (Minimal Lower Layer Protocol). |
| `version` | `String` | `"2.5"` | Unterstützte HL7-Versionsnummer. |

**Port 6667** ist der de-facto-Standardport für HL7-MLLP-Verbindungen. Details zu Protokoll, Nachrichtentypen (ADT, ORU, ORM) und Mapping: [INTERFACES.md](INTERFACES.md) → HL7 v2.5.

#### 2.4.2 `[interfaces.fhir]` — FHIR R4 (REST/JSON)

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `enabled` | `bool` | `true` | Schaltet das FHIR-Interface ein/aus. **Standard: aktiviert.** |
| `base_url` | `String` | `"http://localhost:8080/fhir"` | Externe Basis-URL, unter der das FHIR-API erreichbar ist. |
| `version` | `String` | `"R4"` | FHIR-Version. Standard: R4 (Release 4). |

> **Hinweis:** Die FHIR-Endpoints liegen **unterhalb** des HTTP-Ports 8080 (Pfad `/fhir`), nicht auf einem separaten Port. In Produktion sollte `base_url` auf die tatsächliche öffentliche URL (mit `https://`) geändert werden.

#### 2.4.3 `[interfaces.dicom]` — DICOM C-STORE SCP

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `enabled` | `bool` | `false` | Schaltet das DICOM-Interface ein/aus. |
| `port` | `u16` | `104` | TCP-Port für DICOM. **104 ist der DICOM-Standardport.** |
| `ae_title` | `String` | `"KIS"` | Application Entity Title — eindeutiger Identifikator des DICOM-SCP (max. 16 Zeichen, alphanumerisch + `_`). |

**AE Title:** Muss bei der Gegenstelle (z. B. PACS, Modality) exakt so registriert werden, wie hier konfiguriert. Max. 16 Zeichen. Details: [INTERFACES.md](INTERFACES.md) → DICOM.

---

### 2.5 Abschnitt `[llm]`

LLM-Integration über OpenRouter (OpenAI-kompatible API). Struktur: `LlmConfig`.

| Feld | Typ | Standard | Beschreibung |
|---|---|---|---|
| `provider` | `String` | `"openrouter"` | LLM-Anbieter. Aktuell: `openrouter`. |
| `api_key_env` | `String` | `"OPENROUTER_API_KEY"` | **Name der Umgebungsvariablen**, in der der API-Key abgelegt ist. Der Key selbst steht hier **nicht** — nur der Name. |
| `rotation_interval` | `u64` | `300` | Intervall in Sekunden, nach dem das aktive LLM-Modell gewechselt wird. |
| `models` | `Vec<String>` | (siehe unten) | Liste der rotierenden Modelle. |

**Standard-Modelle:**

| Modell | Rolle im Rotationszyklus |
|---|---|
| `deepseek/deepseek-chat-v3-0324` | Primärmodell (Generierung) |
| `qwen/qwen3-coder` | Review-Modell (Cross-Review) |
| `meta-llama/llama-4-maverick` | Drittes Modell (mehrstufige Prüfung) |

> **Sicherheit:** Der API-Key wird **niemals** in der `kis.toml` gespeichert. `api_key_env` referenziert nur den Namen der Env-Variable. Der Wert wird zur Laufzeit aus der Umgebung gelesen. Bei Fehlen wird die LLM-Funktionalität deaktiviert, der Rest des Systems läuft weiter. Details: [SECURITY.md](SECURITY.md) → Secret-Management.

**LLM-Workflow (Modellrotation + Cross-Review):**
1. Modell A generiert Code/Antwort.
2. Modell B reviewt und gibt Feedback.
3. Modell A integriert das Feedback.
4. Wiederholung bis die Qualitätsschwelle erreicht ist.
5. Nach `rotation_interval` Sekunden wird das aktive Modell gewechselt.

Details: [DEVELOPMENT.md](DEVELOPMENT.md) → LLM-Workflow.

---

## 3. Sicherheitsrelevante Konfiguration

> **Zusammenfassung der wichtigsten Sicherheitseinstellungen.** Vollständige Details: [SECURITY.md](SECURITY.md).

| Einstellung | Wo | Empfehlung für Produktion |
|---|---|---|
| `server.host` | `config/kis.toml` | `0.0.0.0` **nur** hinter TLS-Proxy; sonst `127.0.0.1` |
| TLS-Terminierung | Extern (Nginx/Caddy) | **Pflicht** für jede nicht-lokale Erreichbarkeit (TLS 1.3) |
| `database.journal_mode` | `config/kis.toml` | `wal` (nie `off`) |
| `api_key_env` | `config/kis.toml` | Name der Env, Wert in `/etc/kis/env` (chmod 600) |
| `interfaces.*.enabled` | `config/kis.toml` | Nur benötigte Schnittstellen aktivieren (grundsätzlich: **minimal halten**) |
| CORS | `src/api/mod.rs` | Aktuell `permissive()` → für Produktion auf konkrete Origin einschränken |

### 3.1 TLS / HTTPS

Das KIS bindet standardmäßig **unverschlüsselt** auf TCP 8080. Für den produktiven Betrieb mit Patientendaten **muss** TLS 1.3 vorgeschaltet werden. Empfohlene Topologien:

1. **Reverse-Proxy (empfohlen):** Nginx, Caddy oder Traefik terminiert TLS und leitet an `127.0.0.1:8080` weiter.
2. **Eigene Zertifikate:** CA signierte Zertifikate (z. B. Let's Encrypt für öffentlich erreichbare Instanzen, interne CA für klinisches Netz).

> **Wichtig:** Alle Datenflüsse, die Patientendaten enthalten (API, FHIR), **müssen** über TLS 1.3 laufen. Klartext-HTTP ist nur für die lokale Entwicklung auf `127.0.0.1` akzeptabel.

### 3.2 CORS (Cross-Origin Resource Sharing)

In `src/api/mod.rs` ist aktuell `CorsLayer::permissive()` aktiv. Für Produktion **muss** dies auf die tatsächlich benötigten Origin(s) eingeschränkt werden:

```rust
// Beispiel (Soll-Zustand für Produktion)
use tower_http::cors::{CorsLayer, Any};
let cors = CorsLayer::new()
    .allow_origin(vec!["https://klinik.example.de".parse().unwrap()])
    .allow_methods(vec!["GET", "POST", "PUT", "DELETE"])
    .allow_headers(vec!["authorization", "content-type"]);
```

### 3.3 Authentifizierung und Autorisierung

Die aktuelle API-Implementierung enthält **noch keinen** Authentifizierungs-Layer (JWT). Für Produktion ist dies ein **Pflicht-Feature** (siehe [SECURITY.md](SECURITY.md) → Zugriffskontrolle). Bis zur Umsetzung:

- API **nicht** öffentlich erreichbar machen (Firewall, Reverse-Proxy mit Auth).
- Nur über VPN / klinisches isoliertes Netz erreichbar halten.

---

## 4. Umgebungsvariablen (Nicht-`KIS_`)

| Variable | Zweck | Standard |
|---|---|---|
| `RUST_LOG` | tracing-Loglevel/Filter | `info` |
| `OPENROUTER_API_KEY` | LLM-API-Key (Name wird von `api_key_env` referenziert) | — |

> **Hinweis:** `OPENROUTER_API_KEY` wird **nicht** als `KIS_*`-Variable gelesen, sondern direkt. Der Name ist in `config/kis.toml` → `[llm].api_key_env` hinterlegt.

---

## 5. Konfigurationsvalidierung

| Prüfung | Auswirkung bei Fehler |
|---|---|
| `port` außerhalb 1–65535 | figment-Extraktionsfehler → System startet **nicht** |
| `journal_mode` nicht in {wal, delete, memory, off} | SQLite-Fehler zur Laufzeit |
| `database.path` nicht beschreibbar | Datenbank-Initialisierung schlägt fehl |
| `api_key_env` verweist auf nicht gesetzte Variable | LLM-Feature deaktiviert, Rest startet |
| Fehrende TOML-Felder | `Default`-Werte greifen (keine Fehler) |

> **Best-Practice:** Vor jedem Deploy die Konfiguration mit `cargo run -- --check-config` (Soll) oder einem Smoke-Test gegen `/api/health` verifizieren.

---

## 6. Referenz: Alle ENV-Variablen (vollständig)

| Variable | Setzt | Beispiel |
|---|---|---|
| `KIS_SERVER__HOST` | `server.host` | `KIS_SERVER__HOST=127.0.0.1` |
| `KIS_SERVER__PORT` | `server.port` | `KIS_SERVER__PORT=8443` |
| `KIS_SERVER__WORKERS` | `server.workers` | `KIS_SERVER__WORKERS=8` |
| `KIS_DATABASE__PATH` | `database.path` | `KIS_DATABASE__PATH=/var/lib/kis/kis.db` |
| `KIS_DATABASE__POOL_SIZE` | `database.pool_size` | `KIS_DATABASE__POOL_SIZE=16` |
| `KIS_DATABASE__JOURNAL_MODE` | `database.journal_mode` | `KIS_DATABASE__JOURNAL_MODE=wal` |
| `KIS_UI__THEME` | `ui.theme` | `KIS_UI__THEME=light` |
| `KIS_UI__LANGUAGE` | `ui.language` | `KIS_UI__LANGUAGE=de` |
| `KIS_UI__AUTO_REFRESH` | `ui.auto_refresh` | `KIS_UI__AUTO_REFRESH=false` |
| `KIS_INTERFACES__HL7__ENABLED` | `interfaces.hl7.enabled` | `KIS_INTERFACES__HL7__ENABLED=true` |
| `KIS_INTERFACES__HL7__PORT` | `interfaces.hl7.port` | `KIS_INTERFACES__HL7__PORT=6667` |
| `KIS_INTERFACES__HL7__VERSION` | `interfaces.hl7.version` | `KIS_INTERFACES__HL7__VERSION=2.5` |
| `KIS_INTERFACES__FHIR__ENABLED` | `interfaces.fhir.enabled` | `KIS_INTERFACES__FHIR__ENABLED=true` |
| `KIS_INTERFACES__FHIR__BASE_URL` | `interfaces.fhir.base_url` | `KIS_INTERFACES__FHIR__BASE_URL=https://fhir.klinik.de` |
| `KIS_INTERFACES__DICOM__ENABLED` | `interfaces.dicom.enabled` | `KIS_INTERFACES__DICOM__ENABLED=true` |
| `KIS_INTERFACES__DICOM__PORT` | `interfaces.dicom.port` | `KIS_INTERFACES__DICOM__PORT=104` |
| `KIS_INTERFACES__DICOM__AE_TITLE` | `interfaces.dicom.ae_title` | `KIS_INTERFACES__DICOM__AE_TITLE=KIS_PAX` |
| `KIS_LLM__PROVIDER` | `llm.provider` | `KIS_LLM__PROVIDER=openrouter` |
| `KIS_LLM__API_KEY_ENV` | `llm.api_key_env` | `KIS_LLM__API_KEY_ENV=OPENROUTER_API_KEY` |
| `KIS_LLM__ROTATION_INTERVAL` | `llm.rotation_interval` | `KIS_LLM__ROTATION_INTERVAL=600` |
| `RUST_LOG` | tracing-Filter | `RUST_LOG=info,kis=debug` |
| `OPENROUTER_API_KEY` | LLM-API-Key | (Secret, nicht committen) |

---

**Nächste Schritte:** [SECURITY.md](SECURITY.md) · [INSTALLATION.md](INSTALLATION.md) · [OPERATION.md](OPERATION.md)
