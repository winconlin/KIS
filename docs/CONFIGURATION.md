# KIS — Konfiguration

Es gibt zwei Ebenen:

| Ebene | Wo | Wer ändert | Wirkt |
|---|---|---|---|
| **Infrastruktur** | Datei `config/kis.toml` und Umgebungsvariablen | Betrieb | nach Neustart |
| **Richtlinien des Hauses** | Datenbank, über `/api/v1/admin/…` | Administration | sofort, auf allen Instanzen |

## Infrastruktur

Der Server liest `KIS_CONFIG_PATH`, sonst `config/kis.toml` relativ zum
Arbeitsverzeichnis. Umgebungsvariablen überschreiben die Datei; `__`
trennt die Ebenen (`KIS_DATABASE__WRITE_URL`, `KIS_SERVER__PORT`).
Unbekannte Schlüssel in der Datei sind ein Fehler — ein Tippfehler bleibt
nicht unbemerkt.

```toml
[server]
host = "127.0.0.1"
port = 8080
shutdown_timeout_seconds = 30     # laufende Anfragen dürfen so lange zu Ende laufen
request_timeout_seconds = 30
max_body_bytes = 1048576

[database]
write_url = "postgres://kis_app@localhost:5432/kis"   # Laufzeitrolle ohne DDL-Rechte
migration_url = ""                # Eigentümerrolle; leer = Migration über write_url (nur Entwicklung)
read_url = ""                     # optionale Replica für Auswertungen
pool_size = 10                    # Obergrenze; Verbindungen entstehen erst bei Bedarf
min_connections = 0               # 0 = im Leerlauf keine offene Verbindung
acquire_timeout_seconds = 10
idle_timeout_seconds = 300

[security]
jwt_secret_env = "KIS_JWT_SECRET"
token_expiry = 900
refresh_expiry = 86400
max_login_attempts = 5            # Startwert; zur Laufzeit änderbar
lockout_duration = 900            # Startwert; zur Laufzeit änderbar

[security.rate_limit]
requests_per_minute = 600         # je Client-IP und Instanz
auth_requests_per_minute = 10

[security.encryption]
master_key_env = "KIS_MASTER_KEY"

[logging]
level = "info"
format = "json"                   # oder "text"

[modules]
disabled = []                     # z. B. ["roster", "meals"]
experimental = []                 # experimentelle Module ausdrücklich einschalten
```

TLS: Sind die Variablen `KIS_TLS_CERT` und `KIS_TLS_KEY` gesetzt (PEM),
lauscht der Server mit TLS 1.3; mit `KIS_TLS_CA` verlangt er
Client-Zertifikate. Ohne sie spricht er HTTP und gehört hinter einen
TLS-Proxy.

### Secrets

| Variable | Inhalt |
|---|---|
| `KIS_JWT_SECRET` | mindestens 32 Zeichen, zufällig |
| `KIS_MASTER_KEY` | 32 zufällige Bytes, Base64 |

Secrets stehen nie in der Konfigurationsdatei oder im Repository. Fehlt
eines oder ist es zu schwach, startet der Server nicht.

### Module

`kis-admin modules` listet die Module dieser Programmversion. Kernmodule
(`org`, `patients`, `cases`) lassen sich nicht abschalten. Der Start bricht
ab bei unbekannten Kennungen und wenn ein aktives Modul ein abgeschaltetes
braucht. Abgeschaltete Module registrieren keine Routen und reagieren auf
keine Ereignisse; ihre Tabellen bleiben unberührt.

## Richtlinien des Hauses

### Rollen und Rechte

Die Rechte sind im Programm festgelegt (`GET /api/v1/admin/permissions`),
die Rollen gehören dem Haus: anlegen, Rechte zuordnen, löschen über
`/api/v1/admin/roles`. Beim ersten Start entsteht eine Grundausstattung
(`kis-admin roles`), die danach frei geändert werden kann. Ein Benutzer hat
eine Hauptrolle und beliebig viele Zusatzrollen. Nur die Rolle `admin` ist
fest: Sie hat immer alle Rechte, damit sich niemand aussperrt.

### Einstellungen

`GET /api/v1/admin/settings` zeigt alle Einstellungen mit Wertebereich,
`PUT /api/v1/admin/settings/<schlüssel>` mit `{"value": …}` ändert eine.

| Schlüssel | Standard | Bedeutung |
|---|---|---|
| `security.treatment_context.enforce` | `true` | Falldaten nur im Behandlungszusammenhang. `false` für sehr kleine Häuser, in denen jeder jeden Patienten behandelt. |
| `security.treatment_context.grace_days` | `14` | Nachlauf des Zugriffs nach Verlegung, Entlassung oder Auftragsende |
| `security.emergency_access.minutes` | `240` | Dauer eines Notfallzugriffs |
| `security.session.inactivity_minutes` | `30` | automatische Abmeldung |
| `security.login.max_attempts` | aus der Datei | Fehlversuche bis zur Kontosperre |
| `security.login.lockout_minutes` | aus der Datei | Dauer der Kontosperre |
| `security.password.min_length` | `12` | Mindestlänge neuer Passwörter |

### Kataloge

Ebenfalls Daten des Hauses und über die API gepflegt: Aufbau des Hauses
(`/api/v1/org/…`), Leistungen und Automatik-Regeln (`/api/v1/catalog/…`),
Kostformen (`/api/v1/meals/diets`), Dienstarten und Feiertage
(`…/roster/…`).
