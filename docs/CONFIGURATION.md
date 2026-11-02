# KIS — Konfiguration

Stand: 02.11.2026 · Referenz des aktiven `src/config/mod.rs`

## Quellen und Priorität

Der Loader liest `KIS_CONFIG_PATH`, ansonsten `config/kis.toml` relativ zum Arbeitsverzeichnis. Anschließend überschreibt Figment Werte aus der Umgebung. Serde-Defaults gelten für fehlende Abschnitte und Felder. Ein vorhandener, aber nicht lesbarer oder syntaktisch fehlerhafter TOML-Inhalt ist ein Fehler.

Für verschachtelte Felder trennt `__` die Abschnitte:

| Feld | Variable | Beispiel |
|---|---|---|
| server.host | KIS_SERVER__HOST | 127.0.0.1 |
| server.port | KIS_SERVER__PORT | 9090 |
| server.workers | KIS_SERVER__WORKERS | 2 |
| database.path | KIS_DATABASE__PATH | /var/lib/kis/kis.db |
| database.pool_size | KIS_DATABASE__POOL_SIZE | 10 |
| database.journal_mode | KIS_DATABASE__JOURNAL_MODE | wal |
| security.token_expiry | KIS_SECURITY__TOKEN_EXPIRY | 900 |
| security.refresh_expiry | KIS_SECURITY__REFRESH_EXPIRY | 86400 |
| security.audit.log_file | KIS_SECURITY__AUDIT__LOG_FILE | /var/log/kis/audit.log |

Einfache Unterstriche sind Teil eines Feldnamens. `KIS_SERVER_PORT` ist deshalb kein Alias für `KIS_SERVER__PORT`. Eine `.env`-Datei wird vom Rust-Binärprogramm nicht eingelesen. Compose liest sie für die im Compose-Dokument referenzierten Variablen; systemd benötigt eine EnvironmentFile.

## Verwendete Einstellungen

| Abschnitt/Feld | Rust-Default | Wirkung |
|---|---|---|
| server.host | 127.0.0.1 | Bind-Adresse; das eingecheckte TOML nutzt ebenfalls Loopback |
| server.port | 8080 | HTTP-Port, größer 0 |
| server.workers | 4 | Worker der Tokio-Runtime, größer 0 |
| database.path | data/kis.db | SQLite-Datei, relativ zum Arbeitsverzeichnis oder absolut |
| database.pool_size | 8 | Maximale Verbindungen, größer 0; TOML setzt 10 |
| database.journal_mode | wal | Unterstützt: wal, delete |
| security.jwt_secret_env | KIS_JWT_SECRET | Name der Variablen mit JWT-Schlüssel |
| security.token_expiry | 900 | Access-Laufzeit in Sekunden |
| security.refresh_expiry | 86400 | Refresh-Laufzeit, mindestens Access-Laufzeit |
| security.max_login_attempts | 5 | Fehlversuche vor Kontosperre |
| security.lockout_duration | 900 | Sperrdauer in Sekunden |
| security.audit.log_file | logs/audit.log | Zusätzliche JSONL-Kopie des DB-Audits |
| security.rate_limit.requests_per_minute | 100 | API-Limit je TCP-IP |
| security.rate_limit.auth_requests_per_minute | 10 | Auth-Limit je TCP-IP |
| security.encryption.master_key_env | KIS_MASTER_KEY | Name der Variablen mit 32 Byte Base64-Key |

`security.audit.retention_days` hat den Default 3650, ist jedoch nur eine Konfigurationsvorgabe für ein betrieblich festzulegendes Archivkonzept. Der Server löscht oder archiviert nicht automatisch. Daraus folgt keine gesetzliche Zehnjahrespflicht.

SQLite verwendet zusätzlich Foreign Keys, fünf Sekunden Busy-Timeout und `synchronous=FULL`. WAL ist ein Journalverfahren, keine Datenbankverschlüsselung.

## Minimales Konfigurationsbeispiel

Durch Defaults genügt ein bewusst kleines TOML; Secrets stehen in der Prozessumgebung:

```toml
[server]
host = "127.0.0.1"
port = 8080
workers = 4

[database]
path = "data/kis.db"
pool_size = 10
journal_mode = "wal"

[security.audit]
log_file = "logs/audit.log"
```

Das vollständige eingecheckte Beispiel ist [config/kis.toml](../config/kis.toml). Es enthält zusätzlich Ausbauoptionen, die noch keine Dienste aktivieren.

## Secrets und separate Variablen

| Variable | Verwendung |
|---|---|
| KIS_JWT_SECRET | Mindestens 32 Byte langer zufälliger Schlüssel für HS256 |
| KIS_MASTER_KEY | Base64-kodierter Schlüssel mit genau 32 dekodierten Bytes |
| KIS_CONFIG_PATH | Auswahl der TOML-Datei |
| RUST_LOG | tracing-Filter, z. B. info oder kis=debug |

Die Secret-Namen können über die entsprechenden `*_env`-Felder geändert werden. Der Schlüsselwert gehört weder ins TOML noch in Versionskontrolle. Beide Standard-Secrets sind für den Serverstart erforderlich. Die Bootstrap-CLI benötigt sie nicht, da sie nur einen Passwort-Hash anlegt.

## Reservierte Ausbauoptionen

`ui.*`, `interfaces.*` und `llm.*` sind größtenteils Konfigurationsentwürfe. Sie aktivieren im Server weder GUI-Themes noch Auto-Refresh, Netzwerklistener oder OpenRouter-Rotation. `interfaces.fhir.enabled` ist standardmäßig false. XDS hat noch keinen aktiven Konfigurationsabschnitt.

Der separat konstruierte MEDICO-Client kann über `MedicoClient::from_config` Basis-URL, Token-Endpunkt, Scope-Liste, Secret-Namen und Request-Timeout übernehmen. Polling-, Slot-, GDT- und HL7-Optionen ergeben noch keinen gestarteten Migrationsdienst; siehe [CGM_MEDICO.md](CGM_MEDICO.md).

Unbekannte TOML-Felder werden derzeit nicht generell zurückgewiesen. Eine syntaktisch gültige zusätzliche Option ist daher kein Nachweis ihrer Unterstützung. Neue Einstellungen benötigen einen Verbraucher im Code und einen Test.
