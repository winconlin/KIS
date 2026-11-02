# KIS — Installation

> **Krankenhaus-Informations-System (KIS)**
> Dieses Dokument beschreibt die Installation des KIS von der Toolchain-Vorbereitung bis zum produktiven Betrieb als systemd-Dienst oder Docker-Container.
>
> **Aktualisiert (Stand 01.11.2026):** Neue Crate-Struktur (`kis-core`, `kis-gdt`, `kis-medico`, `kis-ti`, `kis-ui`), statische UI-Dateien, MEDICO-Konfiguration, PostgreSQL-Setup.

---

## 1. Systemvoraussetzungen

### 1.1 Minimale Hardware-Anforderungen

Das KIS ist bewusst ressourcenschonend konzipiert. Folgende Werte sind **Minimum** für eine produktive Nutzung mit mehreren gleichzeitigen Benutzern:

| Ressource | Minimum | Empfohlen |
|---|---|---|
| CPU | 1 Kern (x86_64 oder aarch64) | 2 Kerne |
| Arbeitsspeicher (RAM) | 512 MB | 1 GB |
| Festplatte | 10 GB freier Platz (für Logs + Backup) | 20 GB |
| Betriebssystem | Linux (Debian 12+, Ubuntu 22.04+, RHEL 9+) | — |
| Netzwerk | LAN-Verbindung, ausgehende TLS-Verbindungen für MEDICO und TI | — |

> **Hinweis:** Das KIS benötigt einen **PostgreSQL-Server** (14+) und einen **Redis-Server** (7.x). Die PostgreSQL-Verbindung wird über `DATABASE_URL` konfiguriert, Redis über `REDIS_URL`.

### 1.2 Software-Voraussetzungen

| Komponente | Version | Zweck |
|---|---|---|
| Rust Toolchain | 1.75+ (stabil) | Compiler für das Backend |
| Cargo | (mit rustup) | Build- und Paket-Manager |
| PostgreSQL | 15+ (empfohlen), 14+ | Datenbank-Server |
| Redis | 7.x | Session-Store, Cache, Rate-Limit |
| make / gcc / pkg-config | — | Für native Dependencies (bei Bedarf) |
| systemd | 245+ | Prozessverwaltung (Linux) |
| Docker + Compose | 20.10+ / 2+ | Optional: Container-Betrieb |

> **Wichtig:** Das KIS verwendet **PostgreSQL** als Datenbank (nicht mehr SQLite). Die Verbindung wird über `DATABASE_URL` in der Konfiguration oder Umgebungsvariable gesetzt.

---

## 2. Installation der Rust-Toolchain

### 2.1 Mit rustup (empfohlen)

```bash
# rustup installieren (einmalig)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Shell neu laden
source "$HOME/.cargo/env"

# Stabile Toolchain sicherstellen
rustup default stable
rustup update

# Version prüfen
rustc --version   # erwartet: rustc 1.75.0 oder neuer
cargo --version
```

### 2.2 System-Pakete (Debian/Ubuntu)

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev
```

### 2.3 System-Pakete (RHEL/AlmaLinux)

```bash
sudo dnf groupinstall "Development Tools"
sudo dnf install -y pkg-config openssl-devel
```

---

## 3. Projekt beziehen und bauen

### 3.1 Repository klonen

```bash
git clone <repo-url> /opt/kis
cd /opt/kis
```

### 3.2 Workspace-Struktur

Das KIS ist als **Cargo-Workspace** mit mehreren Crates organisiert:

```
kis/
├── Cargo.toml              # Workspace-Definition
├── crates/
│   ├── kis-core/           # Kern: Config, DB, Security, API, Models
│   ├── kis-gdt/            # GDT-Export/-Import
│   ├── kis-medico/         # MEDICO-Synchronisation
│   ├── kis-ti/             # Telematikinfrastruktur (eGK, ePA, eRezept, eAU)
│   └── kis-ui/             # UI-Backend (Auth, statische Dateien)
├── static/                 # Statische UI-Dateien
│   ├── index.html
│   ├── css/style.css
│   └── js/
│       ├── api.js
│       ├── auth.js
│       └── app.js
├── config/
│   └── kis.toml
├── migrations/             # PostgreSQL-Migrationen
└── docs/
```

### 3.3 Release-Build

```bash
# Vollständiger Release-Build des Workspaces (alle Crates)
cargo build --release

# Binärdatei entsteht unter:
ls -lh target/release/kis
```

> **Hinweis:** Der Release-Profil in `Cargo.toml` setzt `lto = "fat"`, `codegen-units = 1` und `strip = true` — der Build ist **langsam**, das Ergebnis aber extrem kompakt und performant.

### 3.4 Datenbank-Initialisierung

Beim ersten Start werden die Migrationsdateien aus `migrations/` automatisch ausgeführt (siehe `crates/kis-core/src/db.rs` → `sqlx::migrate!`). Die PostgreSQL-Datenbank muss vorab existieren:

```bash
# PostgreSQL-Database anlegen (einmalig)
sudo -u postgres psql -c "CREATE DATABASE kis;"
sudo -u postgres psql -c "CREATE USER kis WITH PASSWORD 'CHANGE_ME';"
sudo -u postgres psql -c "GRANT ALL PRIVILEGES ON DATABASE kis TO kis;"
```

> **Hinweis:** Die Umgebungsvariable `DATABASE_URL` muss vor dem Start gesetzt sein:
> ```bash
> export DATABASE_URL="postgresql://kis:CHANGE_ME@localhost:5432/kis"
> ```

### 3.5 Konfiguration prüfen

Die Konfiguration liegt in `config/kis.toml`. Vor dem ersten Start prüfen:

```bash
cat config/kis.toml
```

Alle Felder sind in [CONFIGURATION.md](CONFIGURATION.md) dokumentiert. Standardwerte sind bereits gesetzt; für Produktion sollten mindestens `server.host`, `database.url` und die MEDICO-Zugangsdaten angepasst werden.

> **MEDICO-Konfiguration:** Die MEDICO-Anbindung wird über Umgebungsvariablen konfiguriert (`MEDICO_BASE_URL`, `MEDICO_CLIENT_ID`, `MEDICO_CLIENT_SECRET`, `MEDICO_SYNC_INTERVAL`). Details in [CONFIGURATION.md](CONFIGURATION.md) → MEDICO.

---

## 4. Ausführung

### 4.1 Manueller Start (Entwicklung)

```bash
# Aus dem Projektordner
cargo run

# Oder das Release-Binär direkt
./target/release/kis
```

Erwartete Startlog-Ausgabe (tracing):

```
KIS — Krankenhaus-Informations-System v0.1.0
Starte System...
KIS läuft
```

### 4.2 Zugriff

| Dienst | URL |
|---|---|
| Benutzeroberfläche (GUI) | `http://<host>:8080/` |
| Health-Check | `http://<host>:8080/api/health` |
| Patienten-API | `http://<host>:8080/api/patients` |
| Termine-API | `http://<host>:8080/api/appointments` |

> **Hinweis:** Der Server bindet standardmäßig an `0.0.0.0:8080` (alle Interfaces). Für lokale Entwicklung ohne Netzwerkkopplung `server.host = "127.0.0.1"` setzen.

### 4.3 Statische UI-Dateien

> **Ist (Stand 01.11.2026):** Vollständig implementiert in `static/`.
>
> Das KIS liefert eine **Vanilla-JS-SPA** (Single Page Application) als statische Dateien, die direkt vom Axum-Server ausgeliefert werden. **Kein Build-Schritt** erforderlich — die Dateien werden im Repository mitgeliefert.

#### 4.3.1 Dateistruktur

```
static/
├── index.html          # SPA-Einstiegspunkt (228 Zeilen)
├── css/
│   └── style.css       # Theming, responsive Grid (1.730 Zeilen)
└── js/
    ├── api.js          # Fetch-Wrapper, JWT-Handling (623 Zeilen)
    ├── auth.js         # Login, MFA/TOTP-Logik (468 Zeilen)
    └── app.js          # SPA-Router, Views, Toast/Modal (1.711 Zeilen)
```

#### 4.3.2 Auslieferung durch Axum

Die statischen Dateien werden durch das `kis-ui`-Crate ausgeliefert:

- **Pfad:** `/static/*` — alle Dateien aus `static/`
- **Cache:** `Cache-Control: public, max-age=3600` für CSS/JS
- **SPA-Fallback:** Alle Nicht-API-Pfade liefern `index.html` (Hash-Routing)

**Beispiel-Requests:**

```http
GET /static/index.html
→ HTTP/1.1 200 OK
Content-Type: text/html

GET /static/css/style.css
→ HTTP/1.1 200 OK
Content-Type: text/css
Cache-Control: public, max-age=3600

GET /patienten
→ HTTP/1.1 200 OK
Content-Type: text/html
(Body: index.html — SPA-Router übernimmt)
```

#### 4.3.3 UI-Features

| Feature | Implementierung |
|---|---|
| **Routing** | Hash-Basiert (`#/patienten`, `#/termine`, `#/settings`) |
| **Authentifizierung** | JWT (Access + Refresh Token), localStorage |
| **MFA** | TOTP-Code-Eingabe beim Login (wenn aktiviert) |
| **Toast-Notifications** | Success/Error-Feedback (3s auto-dismiss) |
| **Modal-Dialoge** | Bestätigungsdialoge für destructive Aktionen |
| **Responsive** | Mobile-First, Grid-Layout ab 768px |
| **Dark Mode** | Optional via CSS-Variable `--theme` |

#### 4.3.4 Zugriff

| URL | Inhalt |
|---|---|
| `http://<host>:8080/` | SPA-Start (Login-Seite) |
| `http://<host>:8080/#/patienten` | Patienten-Liste |
| `http://<host>:8080/#/termine` | Terminplan |
| `http://<host>:8080/#/settings` | Einstellungen (nur admin) |

> **Hinweis:** Die UI erfordert eine gültige Session. Ohne Token wird die Login-Seite angezeigt.

### 4.4 Umgebungsvariablen

Vor dem Start können Umgebungsvariablen gesetzt werden, die die `config/kis.toml` ergänzen oder überschreiben:

```bash
export DATABASE_URL="postgresql://kis:CHANGE_ME@localhost:5432/kis"
export REDIS_URL="redis://localhost:6379"
export MEDICO_BASE_URL="https://fhir.medico.example.com/fhir/"
export MEDICO_CLIENT_ID="your-client-id"
export MEDICO_CLIENT_SECRET="your-client-secret"
export MEDICO_SYNC_INTERVAL=30
export RUST_LOG=info
./target/release/kis
```

Details zu allen Variablen: [CONFIGURATION.md](CONFIGURATION.md).

---

## 5. Betrieb als systemd-Dienst (empfohlen für Produktion)

### 5.1 Benutzer und Verzeichnis

```bash
# Dedizierten Systembenutzer anlegen (keine Shell, kein Login)
sudo useradd --system --no-create-home --shell /usr/sbin/nologin kis

# Verzeichnisse anlegen und Rechte setzen
sudo mkdir -p /opt/kis /var/lib/kis /var/log/kis
sudo chown -R kis:kis /var/lib/kis /var/log/kis
```

### 5.2 Konfiguration in die Produktivlage kopieren

```bash
sudo mkdir -p /etc/kis
sudo cp /opt/kis/config/kis.toml /etc/kis/kis.toml   # bei Bedarf anpassen
sudo chown root:kis /etc/kis/kis.toml
sudo chmod 640 /etc/kis/kis.toml

# Statische UI-Dateien kopieren
sudo cp -r /opt/kis/static /opt/kis/static
sudo chown -R kis:kis /opt/kis/static
```

### 5.3 systemd-Unit-Datei

Datei: `/etc/systemd/system/kis.service`

```ini
[Unit]
Description=KIS - Krankenhaus-Informations-System
Documentation=file:///opt/kis/docs/OPERATION.md
After=network.target

[Service]
Type=simple
User=kis
Group=kis
WorkingDirectory=/opt/kis
ExecStart=/opt/kis/target/release/kis
Restart=always
RestartSec=5
Environment=RUST_LOG=info
EnvironmentFile=-/etc/kis/env

# Sicherheitshärtung
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/kis /var/log/kis
PrivateTmp=true
LimitNOFILE=65536

# Ressourcen-Limits (optional)
MemoryMax=1G

[Install]
WantedBy=multi-user.target
```

> **`EnvironmentFile=-/etc/kis/env`** enthält Secrets (z. B. `DATABASE_URL`, `MEDICO_CLIENT_SECRET`). Das `-` bedeutet: Datei optional. Diese Datei **niemals** in Versionskontrolle aufnehmen (`chmod 600 kis:kis`).

### 5.4 Dienst aktivieren und starten

```bash
sudo systemctl daemon-reload
sudo systemctl enable kis
sudo systemctl start kis

# Status prüfen
sudo systemctl status kis

# Logs anzeigen
sudo journalctl -u kis -f
```

### 5.5 Smoke-Test

```bash
# Health-Check
curl -fsS http://localhost:8080/api/health
# erwartet: OK
```

---

## 6. Betrieb mit Docker

### 6.1 Dockerfile

Datei: `Dockerfile` (Projektwurzel)

```dockerfile
# ---- Build-Stage ----
FROM rust:1.75-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN cargo build --release

# ---- Laufzeit-Stage ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /opt/kis
COPY --from=builder /build/target/release/kis /opt/kis/kis
COPY config/kis.toml /opt/kis/config/kis.toml
COPY static ./static
RUN mkdir -p /var/lib/kis && chown -R 1000:1000 /var/lib/kis
USER 1000
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://localhost:8080/api/health || exit 1
ENTRYPOINT ["/opt/kis/kis"]
```

> **Hinweis:** Das Binär bindet `config/kis.toml` relativ zum Arbeitsverzeichnis. Im Container wird `WorkingDirectory=/opt/kis` und die PostgreSQL-Verbindung über `DATABASE_URL` gesetzt. Die statischen UI-Dateien liegen unter `/opt/kis/static`.

### 6.2 docker-compose.yml

Datei: `docker-compose.yml` (Projektwurzel)

```yaml
services:
  kis:
    build: .
    image: kis:0.1.0
    container_name: kis
    restart: unless-stopped
    ports:
      - "8080:8080"     # HTTP (GUI + API + FHIR)
      - "6667:6667"     # HL7 v2.5 MLLP (falls aktiviert)
      - "104:104"       # DICOM C-STORE SCP (falls aktiviert)
    volumes:
      - ./config:/opt/kis/config:ro
      - ./static:/opt/kis/static:ro
    environment:
      - RUST_LOG=info
      - DATABASE_URL=postgresql://kis:CHANGE_ME@postgres:5432/kis
      - MEDICO_BASE_URL=https://fhir.medico.example.com/fhir/
      - MEDICO_CLIENT_ID=${MEDICO_CLIENT_ID}
      - MEDICO_CLIENT_SECRET=${MEDICO_CLIENT_SECRET}
      - MEDICO_SYNC_INTERVAL=30
    depends_on:
      postgres:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8080/api/health"]
      interval: 30s
      timeout: 5s
      retries: 3

  postgres:
    image: postgres:15
    container_name: kis_postgres
    restart: unless-stopped
    environment:
      POSTGRES_DB: kis
      POSTGRES_USER: kis
      POSTGRES_PASSWORD: CHANGE_ME
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "kis"]
      interval: 10s
      timeout: 5s
      retries: 5

volumes:
  postgres_data:
```

### 6.3 Build und Start

```bash
# .env mit Secrets anlegen (nicht committen)
cat > .env <<'EOF'
DATABASE_URL=postgresql://kis:CHANGE_ME@localhost:5432/kis
MEDICO_BASE_URL=https://fhir.medico.example.com/fhir/
MEDICO_CLIENT_ID=your-client-id
MEDICO_CLIENT_SECRET=your-client-secret
EOF
chmod 600 .env

docker compose up -d --build

# Status
docker compose ps

# Logs
docker compose logs -f

# Health-Check
curl -fsS http://localhost:8080/api/health
```

### 6.4 Backup-Volume

```bash
# Datenbestand sichern (siehe DATABASE.md → Backup)
docker compose exec kis sqlite3 /var/lib/kis/kis.db ".backup '/tmp/kis-backup.db'"
docker cp kis:/tmp/kis-backup.db ./backup/kis-$(date +%F).db
```

---

## 7. Verifikation nach der Installation

| Prüfung | Befehl | Erwartetes Ergebnis |
|---|---|---|
| Binär startet | `./target/release/kis` | Log: `KIS läuft` |
| Health-Check | `curl http://localhost:8080/api/health` | `OK` |
| GUI erreichbar | `curl -I http://localhost:8080/` | `HTTP/1.1 200 OK` |
| Statische UI-Dateien | `curl http://localhost:8080/static/index.html` | `HTTP/1.1 200 OK` |
| PostgreSQL verbunden | `psql $DATABASE_URL -c "SELECT 1;"` | `1` |
| Migrationen gelaufen | `psql $DATABASE_URL -c "\dt"` | `patients doctors appointments medications audit_log sessions users api_keys encryption_keys roles user_roles` |
| systemd aktiv | `systemctl is-active kis` | `active` |

---

## 8. Häufige Installationsprobleme

| Problem | Ursache | Lösung |
|---|---|---|
| `cargo build` dauert sehr lange | `lto = "fat"` im Release-Profil | Normal; Build parallelisieren oder `cargo build` (Debug) für Entwicklung |
| Port 8080 belegt | Anderer Dienst | `server.port` in `config/kis.toml` ändern oder `KIS_SERVER__PORT` setzen |
| PostgreSQL-Verbindung schlägt fehl | Falsche `DATABASE_URL` | Umgebungsvariable prüfen, PostgreSQL-Status: `sudo systemctl status postgresql` |
| MEDICO-Sync nicht aktiv | Fehlende `MEDICO_*`-Variablen | Env-Variablen setzen (siehe CONFIGURATION.md → MEDICO) |
| UI-Dateien nicht erreichbar | `static/`-Verzeichnis fehlt | Verzeichnis kopieren: `cp -r /opt/kis/static /opt/kis/static` |
| Migration schlägt fehl | Bestehende, inkompatible DB | Datenbank-Dump erstellen, Schema manuell prüfen (siehe DATABASE.md) |

---

**Nächste Schritte:** [CONFIGURATION.md](CONFIGURATION.md) · [OPERATION.md](OPERATION.md) · [SECURITY.md](SECURITY.md) · [API.md](API.md) · [INTERFACES.md](INTERFACES.md) · [CGM_MEDICO.md](CGM_MEDICO.md)

---

*Stand: 01.11.2026 — KIS v0.1.0 (PostgreSQL, Crate-Workspace, statische UI-Dateien)*
