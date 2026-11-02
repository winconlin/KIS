# KIS — Installation und lokaler Start

Stand: 02.11.2026 · Entwicklungsprototyp

Diese Anleitung startet den aktuellen SQLite-Prototyp. Es gibt noch keine freigegebene klinische Installation oder einen vollständigen MEDICO-Ersatz.

## Voraussetzungen

- Linux und die in [rust-toolchain.toml](../rust-toolchain.toml) gepinnte Rust-Version 1.99.1 einschließlich Cargo.
- C-Compiler/Build-Werkzeuge für native Dependencies.
- OpenSSL-CLI zur Erzeugung zufälliger Entwicklungsschlüssel.
- Optional Docker mit Compose; Python 3.11+ und age für die Backup-Werkzeuge.

Debian/Ubuntu: `sudo apt-get install build-essential pkg-config ca-certificates openssl python3 age sqlite3`.
Rust wird über rustup eingerichtet. Im Repo installiert bzw. wählt rustup die gepinnte Toolchain. Rust 1.75/1.79 sind mit dem aktuellen Lockfile nicht die dokumentierte Build-Basis. Ein extern installierter Datenbankserver wird nicht verwendet.

## Repository und Build

```bash
git clone https://github.com/winconlin/KIS.git
cd KIS
cargo build --locked --bins
cargo test --locked
```

Für optimierte Binärdateien: `cargo build --locked --release --bins`. Es gibt noch keine Modul-Feature-Flags. Die Entwürfe unter `crates/` gehören nicht zum Root-Build.

## Schlüssel für eine neue Entwicklungsumgebung

Die folgenden Werte gelten nur für diesen Shell-Prozess und seine Kindprozesse:

```bash
export KIS_JWT_SECRET="$(openssl rand -base64 48)"
export KIS_MASTER_KEY="$(openssl rand -base64 32)"
export KIS_SERVER__HOST=127.0.0.1
```

Für eine bestehende Datenbank die vorhandenen Schlüssel aus der geschützten Ablage wiederverwenden. Masterkey-Verlust bzw. unkoordinierte Rotation kann später verschlüsselte Datensätze unlesbar machen. Die globale Krypto-Initialisierung ist noch keine Verschlüsselung der gesamten DB.

## Administrator anlegen

Es gibt keinen Standardzugang. Die lokale CLI legt ein neues Administratorkonto an und überschreibt keine bestehenden Benutzer. Sie liest das Passwort über stdin, nicht über ein Kommandozeilenargument:

```bash
read -r -s -p 'Neues Admin-Passwort (mindestens 12 Zeichen): ' KIS_ADMIN_PASSWORD
printf '\n'
printf '%s\n' "$KIS_ADMIN_PASSWORD" | cargo run --locked --bin kis-admin -- admin admin@example.test
unset KIS_ADMIN_PASSWORD
```

Für den Server und die CLI müssen dieselben Konfigurations-/DB-Pfade gelten. Die CLI kann ohne JWT-/Masterkey starten; der Server benötigt beide Secrets.

## Start und Prüfung

```bash
cargo run --locked --bin kis
```

In einer zweiten Shell:

```bash
curl -fsS http://127.0.0.1:8080/api/health
```

Erwartet wird `OK`. Unter `/` erscheint ein Text mit Prototyp-Hinweis. Die Datenendpunkte liefern nach Auth-/Rollenprüfung HTTP 501. Das ist kein vorhandenes Patientenverwaltungssystem.

Der Server legt DB-Elternverzeichnis und DB-Datei an und führt alle eingebetteten Migrationen aus. Bei Migrations- oder Audit-Prüffehlern startet der HTTP-Listener nicht. Datenbankfehler sind kein Anlass, vorhandene Daten zu löschen.

## Konfiguration auswählen

```bash
export KIS_CONFIG_PATH=/etc/kis/kis.toml
export KIS_DATABASE__PATH=/var/lib/kis/kis.db
export KIS_SECURITY__AUDIT__LOG_FILE=/var/log/kis/audit.log
```

Für lokale Entwicklung genügen die eingecheckte Konfiguration und die Standardpfade. Details stehen in [CONFIGURATION.md](CONFIGURATION.md). Eine `.env` wird bei `cargo run` nicht automatisch geladen.

## systemd für eine betreute Testinstanz

Diese Schritte setzen einen gebauten Release voraus. Sie sind eine technische Dienstkonfiguration, keine klinische Produktfreigabe.

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin kis
sudo install -d /opt/kis/bin /etc/kis
sudo install -d -o kis -g kis /var/lib/kis /var/log/kis
sudo install -m 755 target/release/kis target/release/kis-admin /opt/kis/bin/
sudo install -o root -g kis -m 640 config/kis.toml /etc/kis/kis.toml
sudoedit /etc/kis/env
```

`/etc/kis/env` muss echte, separat erzeugte Schlüssel enthalten. Die Namen und Pfade sind:

```text
KIS_CONFIG_PATH=/etc/kis/kis.toml
KIS_SERVER__HOST=127.0.0.1
KIS_DATABASE__PATH=/var/lib/kis/kis.db
KIS_SECURITY__AUDIT__LOG_FILE=/var/log/kis/audit.log
KIS_JWT_SECRET=<hier einen erzeugten JWT-Schlüssel eintragen>
KIS_MASTER_KEY=<hier den erzeugten Base64-Masterkey eintragen>
RUST_LOG=info
```

`sudo chown root:root /etc/kis/env` und `sudo chmod 600 /etc/kis/env` schützen diese Datei. Die Platzhalter sind keine gültigen Schlüssel. systemd liest die Datei vor dem Benutzerwechsel.

Unit unter `/etc/systemd/system/kis.service`:

```ini
[Unit]
Description=KIS Entwicklungsprototyp
After=network.target

[Service]
Type=simple
User=kis
Group=kis
WorkingDirectory=/opt/kis
ExecStart=/opt/kis/bin/kis
EnvironmentFile=/etc/kis/env
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/kis /var/log/kis
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Mit `sudo systemctl daemon-reload` und `sudo systemctl enable --now kis` starten. Logs: `journalctl -u kis -f`. Das Benutzerkonto muss vor Anmeldung auf derselben DB über die CLI angelegt werden; dabei dieselben absoluten DB-/Config-Pfade verwenden.

Netzwerkzugriff erfordert TLS-Terminierung und abgestimmte Zugriffsbeschränkungen. Der KIS-Prozess bietet selbst nur HTTP.

## Docker Compose

Für eine neue Testinstallation erstellt dieser Befehl eine geschützte `.env`; eine vorhandene Datei wird nicht überschrieben:

```bash
(
  umask 077
  set -C
  {
    printf 'KIS_JWT_SECRET='
    openssl rand -base64 48
    printf 'KIS_MASTER_KEY='
    openssl rand -base64 32
  } > .env
)
docker compose build
read -r -s -p 'Neues Admin-Passwort: ' KIS_ADMIN_PASSWORD
printf '\n'
printf '%s\n' "$KIS_ADMIN_PASSWORD" | docker compose run --rm -T --entrypoint kis-admin kis admin admin@example.test
unset KIS_ADMIN_PASSWORD
docker compose up -d
curl -fsS http://127.0.0.1:8080/api/health
```

Das Image startet als eigener Benutzer. Compose verwendet SQLite und persistiert Daten und Audit-Dateikopie in benannten Volumes. Nur HTTP wird auf Host-Loopback veröffentlicht. HL7-/DICOM-Ports und ein unbenutzter PostgreSQL-Dienst sind nicht Bestandteil dieses Prototyp-Deployments.

Die Container-Builds müssen separat in einer Docker-fähigen Umgebung geprüft werden. Für Backup und Restore siehe [OPERATION.md](OPERATION.md); bei Container-Volumes müssen die Snapshot-/Restore-Werkzeuge auf genau das verwendete Volume zugreifen.
