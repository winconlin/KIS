# KIS — Installation

> **Krankenhaus-Informations-System (KIS)**
> Dieses Dokument beschreibt die Installation des KIS von der Toolchain-Vorbereitung bis zum produktiven Betrieb als systemd-Dienst oder Docker-Container.

---

## 1. Systemvoraussetzungen

### 1.1 Minimale Hardware-Anforderungen

Das KIS ist bewusst ressourcenschonend konzipiert. Folgende Werte sind **Minimum** für eine produktive Nutzung mit mehreren gleichzeitigen Benutzern:

| Ressource | Minimum | Empfohlen |
|---|---|---|
| CPU | 1 Kern (x86_64 oder aarch64) | 2 Kerne |
| Arbeitsspeicher (RAM) | 512 MB | 1 GB |
| Festplatte | 10 GB freier Platz (für Datenbank + Logs + Backup) | 20 GB |
| Betriebssystem | Linux (Debian 12+, Ubuntu 22.04+, RHEL 9+) | — |
| Netzwerk | LAN-Verbindung, ausgehende TLS-Verbindungen für LLM | — |

> **Hinweis:** Da die Datenbank in SQLite (WAL-Modus) implementiert ist, ist **kein externer Datenbankserver** (z. B. PostgreSQL, MySQL) erforderlich. Der gesamte Datenbestand liegt in einer einzigen Datei unter `data/kis.db`.

### 1.2 Software-Voraussetzungen

| Komponente | Version | Zweck |
|---|---|---|
| Rust Toolchain | 1.75+ (stabil) | Compiler für das Backend |
| Cargo | (mit rustup) | Build- und Paket-Manager |
| SQLite | 3.35+ (via `rusqlite` bundled) | Datenbank-Engine, wird in den Binärbaum kompiliert |
| make / gcc / pkg-config | — | Für native Dependencies (bei Bedarf) |
| systemd | 245+ | Prozessverwaltung (Linux) |
| Docker + Compose | 20.10+ / 2+ | Optional: Container-Betrieb |

> **Wichtig:** `rusqlite` wird mit der Feature-Flag `bundled` eingebunden (siehe `Cargo.toml`). Dadurch wird eine eigene SQLite-Version kompiliert — eine systemweite SQLite-Installation ist **nicht** erforderlich.

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

### 3.2 Release-Build

```bash
# Vollständiger Release-Build (LTO + strip, siehe Cargo.toml [profile.release])
cargo build --release

# Binärdatei entsteht unter:
ls -lh target/release/kis
```

> **Hinweis:** Der Release-Profil in `Cargo.toml` setzt `lto = "fat"`, `codegen-units = 1` und `strip = true` — der Build ist **langsam**, das Ergebnis aber extrem kompakt und performant.

### 3.3 Datenbank-Initialisierung

Beim ersten Start werden die Migrationsdateien aus `migrations/` automatisch ausgeführt (siehe `src/db/mod.rs` → `sqlx::migrate!`). Die Datenbankdatei `data/kis.db` wird bei Bedarf angelegt.

```bash
# Verzeichnis für die Datenbank anlegen
mkdir -p data
```

### 3.4 Konfiguration prüfen

Die Konfiguration liegt in `config/kis.toml`. Vor dem ersten Start prüfen:

```bash
cat config/kis.toml
```

Alle Felder sind in [CONFIGURATION.md](CONFIGURATION.md) dokumentiert. Standardwerte sind bereits gesetzt; für Produktion sollten mindestens `server.host`, `database.path` und die LLM-`api_key_env` angepasst werden.

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

### 4.3 Umgebungsvariablen

Vor dem Start können Umgebungsvariablen gesetzt werden, die die `config/kis.toml` ergänzen oder überschreiben:

```bash
export KIS_SERVER__PORT=9090
export KIS_DATABASE__PATH=/var/lib/kis/kis.db
export RUST_LOG=info        # tracing-Filter
export OPENROUTER_API_KEY=sk-...   # für LLM-Integration
./target/release/kis
```

Details zu allen `KIS_*`-Variablen: [CONFIGURATION.md](CONFIGURATION.md).

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
sudo cp /opt/kis/config/kis.toml /etc/kis/kis.toml   # bei Bedarf anpassen
sudo chown root:kis /etc/kis/kis.toml
sudo chmod 640 /etc/kis/kis.toml
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

> **`EnvironmentFile=-/etc/kis/env`** enthält Secrets (z. B. `OPENROUTER_API_KEY`). Das `-` bedeutet: Datei optional. Diese Datei **niemals** in Versionskontrolle aufnehmen (`chmod 600 kis:kis`).

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
COPY src ./src
COPY migrations ./migrations
COPY config ./config
RUN cargo build --release

# ---- Laufzeit-Stage ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /opt/kis
COPY --from=builder /build/target/release/kis /opt/kis/kis
COPY config/kis.toml /opt/kis/config/kis.toml
COPY migrations ./migrations
COPY static ./static
RUN mkdir -p /var/lib/kis && chown -R 1000:1000 /var/lib/kis
USER 1000
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://localhost:8080/api/health || exit 1
ENTRYPOINT ["/opt/kis/kis"]
```

> **Hinweis:** Das Binär bindet `config/kis.toml` und `migrations/` relativ zum Arbeitsverzeichnis. Im Container wird `WorkingDirectory=/opt/kis` und die Datenbank in `/var/lib/kis` geschrieben (`KIS_DATABASE__PATH=/var/lib/kis/kis.db`).

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
      - kis_data:/var/lib/kis
      - ./config:/opt/kis/config:ro
      - ./migrations:/opt/kis/migrations:ro
    environment:
      - RUST_LOG=info
      - KIS_DATABASE__PATH=/var/lib/kis/kis.db
      - OPENROUTER_API_KEY=${OPENROUTER_API_KEY}
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8080/api/health"]
      interval: 30s
      timeout: 5s
      retries: 3

volumes:
  kis_data:
```

### 6.3 Build und Start

```bash
# .env mit Secrets anlegen (nicht committen)
cat > .env <<'EOF'
OPENROUTER_API_KEY=sk-...
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
| Datenbank angelegt | `ls -l data/kis.db` | Datei existiert |
| Migrationen gelaufen | `sqlite3 data/kis.db ".tables"` | `patients doctors appointments medications` |
| systemd aktiv | `systemctl is-active kis` | `active` |

---

## 8. Häufige Installationsprobleme

| Problem | Ursache | Lösung |
|---|---|---|
| `cargo build` dauert sehr lange | `lto = "fat"` im Release-Profil | Normal; Build parallelisieren oder `cargo build` (Debug) für Entwicklung |
| Port 8080 belegt | Anderer Dienst | `server.port` in `config/kis.toml` ändern oder `KIS_SERVER__PORT` setzen |
| `data/kis.db` nicht beschreibbar | Falsche Dateiberechtigungen | `chown -R <user> data/` |
| Migration schlägt fehl | Bestehende, inkompatible DB | Datenbankdatei entfernen oder Migrationen prüfen (siehe DATABASE.md) |
| LLM-Verbindung schlägt fehl | Fehlende `OPENROUTER_API_KEY` | Env-Variable setzen (siehe CONFIGURATION.md → LLM) |

---

**Nächste Schritte:** [CONFIGURATION.md](CONFIGURATION.md) · [OPERATION.md](OPERATION.md) · [SECURITY.md](SECURITY.md)
