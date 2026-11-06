# KIS — Installation und lokaler Start

Stand: 06.11.2026

Diese Anleitung richtet eine **Entwicklungs- oder Testinstallation** mit synthetischen Daten ein. Eine freigegebene klinische Installation gibt es nicht.

## Voraussetzungen

- Linux, Rust in der Version aus [rust-toolchain.toml](../rust-toolchain.toml) (rustup wählt sie automatisch), C-Compiler.
- PostgreSQL 16 (lokal, im Container oder entfernt).
- `openssl` zum Erzeugen von Secrets.
- Optional: Docker mit Compose; `age`, `pg_dump`/`pg_restore` und Python 3.11+ für Backups.

Debian/Ubuntu: `sudo apt-get install build-essential pkg-config ca-certificates openssl python3 age postgresql-client`

## Variante A: Docker Compose

```bash
cp .env.example .env && chmod 600 .env
# .env ausfüllen:
#   KIS_JWT_SECRET   = $(openssl rand -base64 48)
#   KIS_MASTER_KEY   = $(openssl rand -base64 32)
#   POSTGRES_PASSWORD, KIS_OWNER_PASSWORD, KIS_APP_PASSWORD = je $(openssl rand -base64 24)
docker compose up -d db
docker compose run --rm --entrypoint kis-admin kis migrate
docker compose run --rm --entrypoint kis-admin kis create-user admin admin@example.test "Admin" admin
docker compose up -d kis
curl -fsS http://127.0.0.1:8080/health
```

`create-user` fragt das Passwort (mindestens 12 Zeichen) interaktiv ab oder liest `KIS_ADMIN_PASSWORD`. Die Datenbank legt beim ersten Start die Rollen `kis_owner` und `kis_app` an ([init-roles.sh](../config/postgres/init-roles.sh)).

## Variante B: Ohne Container

1. Rollen und Datenbank anlegen (als PostgreSQL-Superuser, Passwörter nicht in Dateien speichern):

   ```bash
   psql -v owner_pw="'…'" -v app_pw="'…'" -f config/postgres/roles.sql
   ```

2. Umgebung setzen (z. B. in einer Datei mit Rechten 600, nicht im Repository):

   ```bash
   export KIS_JWT_SECRET="$(openssl rand -base64 48)"
   export KIS_MASTER_KEY="$(openssl rand -base64 32)"
   export KIS_DATABASE__WRITE_URL="postgres://kis_app:…@localhost:5432/kis"
   export KIS_DATABASE__MIGRATION_URL="postgres://kis_owner:…@localhost:5432/kis"
   ```

3. Bauen, migrieren, ersten Benutzer anlegen, starten:

   ```bash
   cargo build --release --locked --bins
   ./target/release/kis-admin migrate
   ./target/release/kis-admin create-user admin admin@example.test "Admin" admin
   ./target/release/kis
   ```

Der Server liest `config/kis.toml` (oder die Datei aus `KIS_CONFIG`) und anschließend Umgebungsvariablen `KIS_<ABSCHNITT>__<SCHLÜSSEL>`, die Vorrang haben. Statische GUI-Dateien liegen unter `static/` relativ zum Arbeitsverzeichnis (`KIS_STATIC_DIR` überschreibt).

**Der Start bricht ab**, wenn JWT-Secret (mindestens 32 Bytes) oder Master-Key (genau 32 Bytes Base64, nicht nur Nullbytes) fehlen oder ungültig sind, die Datenbank nicht erreichbar ist oder eine Migration fehlschlägt.

## TLS

- Direkt: `KIS_TLS_CERT` und `KIS_TLS_KEY` (PEM) setzen. Es wird nur TLS 1.3 angeboten und ein HSTS-Header gesetzt. Mit `KIS_TLS_CA` ist ein Client-Zertifikat Pflicht (mTLS).
- Hinter einem Reverse-Proxy: siehe [config/nginx/kis.conf](../config/nginx/kis.conf). KIS lauscht dann ohne TLS auf `127.0.0.1`. `X-Forwarded-For` wird nur von einer Loopback-Gegenstelle akzeptiert (Rate-Limit-Schlüssel).

## systemd

[systemd/kis.service](../systemd/kis.service) startet `/usr/local/bin/kis` als Benutzer `kis` mit Arbeitsverzeichnis `/data` und liest Secrets aus `/etc/kis/kis.env` (Rechte 600). Die Unit wurde mit `systemd-analyze verify` geprüft; ein realer Betrieb unter systemd ist noch nicht nachgewiesen.

## Backup

```bash
# Backup (Archiv wird vor Veröffentlichung auf Lesbarkeit geprüft)
python3 scripts/backup.py backup --database-url "postgres://kis_owner@localhost/kis" \
  --destination /srv/kis-backup --recipients /etc/kis/backup-recipients.txt \
  --check-identity /etc/kis/backup-identity.txt
# Prüfung mit echtem Restore in eine LEERE Prüfdatenbank
python3 scripts/backup.py verify --archive /srv/kis-backup/kis-….dump.age \
  --identity /etc/kis/backup-identity.txt --restore-into "postgres://…/kis_restore_check"
```

Passwörter über `PGPASSFILE`/`PGPASSWORD`, nicht in der URL (sonst in der Prozessliste sichtbar).
