# KIS — Betriebshandbuch

> **Krankenhaus-Informations-System (KIS)**
> Operations-Manual: Monitoring, Logging, Backup-Schedules, Upgrade-Prozedur, Troubleshooting, Performance-Tuning.

---

## 1. Übersicht

Dieses Dokument richtet sich an das **Betriebs- und IT-Team**, das den KIS-Server im klinischen Umfeld betreut. Es ergänzt [SECURITY.md](SECURITY.md) (sicherheitstechnisch) und [DEVELOPMENT.md](DEVELOPMENT.md) (Entwicklung).

| Bereich | Inhalt |
|---|---|
| Monitoring | Health-Checks, Metriken, Alerting |
| Logging | Log-Niveaus, Struktur, Aufbewahrung |
| Backup | Schedules, Verfahren, Restore-Test |
| Upgrade | Prozedur, Rollback, Wartungsfenster |
| Troubleshooting | Häufige Fehler, Diagnose |
| Performance | Tuning, Skalierung |

---

## 2. Monitoring

### 2.1 Health-Checks

| Endpunkt | Zweck | Intervall |
|---|---|---|
| `GET /api/health` | Grundlegende Erreichbarkeit | 30 s (Load-Balancer) |
| `GET /api/health` (erweitert, Soll) | DB-Status, Version, Uptime | 60 s (Monitoring) |

**Erwartete Antwort (200 OK):**

```
OK
```

> **Soll-Erweiterung:**
> ```json
> { "status": "ok", "version": "0.1.0", "db": "connected", "uptime_s": 86400 }
> ```

### 2.2 Metriken (Soll)

Für fortgeschrittenes Monitoring werden **Prometheus**-Metriken unter `/metrics` (nur intern) bereitgestellt:

| Metrik | Typ | Beschreibung |
|---|---|---|
| `kis_http_requests_total` | Counter | HTTP-Anfragen (nach Status, Methode) |
| `kis_http_request_duration_seconds` | Histogram | Antwortzeiten |
| `kis_db_pool_active` | Gauge | Aktive DB-Verbindungen |
| `kis_db_pool_idle` | Gauge | Idle DB-Verbindungen |
| `kis_db_queries_total` | Counter | DB-Abfragen (nach Dauer) |
| `kis_process_cpu_seconds_total` | Counter | CPU-Zeit |
| `kis_process_resident_memory_bytes` | Gauge | RAM |
| `kis_uptime_seconds` | Gauge | Uptime |
| `kis_audit_entries_total` | Counter | Audit-Log-Einträge |

**Prometheus-Scrape-Config:**

```yaml
# /etc/prometheus/prometheus.yml
scrape_configs:
  - job_name: kis
    static_configs:
      - targets: ['kis-server:8080']
    metrics_path: /metrics
    scrape_interval: 30s
```

### 2.3 Alerting-Regeln (Soll)

| Alert | Bedingung | Schwere |
|---|---|---|
| `KisDown` | `up{kis} == 0` für 2 min | P1 |
| `KisHighLatency` | `histogram_quantile(0.99, kis_http_request_duration_seconds) > 2s` | P2 |
| `KisDbPoolExhausted` | `kis_db_pool_active > 7` (Pool-Größe 8) für 5 min | P2 |
| `KisErrorRate` | `rate(kis_http_requests_total{status=~"5.."}[5m]) > 0.05` | P1 |
| `KisDiskSpace` | `node_filesystem_avail_bytes{mountpoint="/data"} < 10%` | P1 |
| `KisAuditIntegrity` | Hash-Ketten-Verifikation fehlgeschlagen | **P1** |
| `KisBackupFailed` | Letztes Backup älter als 26 h | P2 |

### 2.4 Uptime-Monitoring (extern)

```bash
# cron (jede Minute)
* * * * * curl -fsS --max-time 5 http://kis-server:8080/api/health > /dev/null || \
  /usr/local/bin/alert --level P1 --msg "KIS Health-Check fehlgeschlagen"
```

### 2.5 Grafana-Dashboards (Soll)

| Dashboard | Inhalt |
|---|---|
| **KIS Übersicht** | Health, Uptime, CPU, RAM, DB-Pool |
| **HTTP-Traffic** | Anfragen, Status-Verteilung, Latenz-Percentiles |
| **Datenbank** | Query-Dauer, Pool-Utilisation, Integrität |
| **Schnittstellen** | HL7-Nachrichten, FHIR-Requests, DICOM-Events |
| **Sicherheit** | Login-Versuche, Fehlgeschlagene Logins, Audit-Volume |

---

## 3. Logging

### 3.1 Log-Niveaus

| Level | Bedeutung | Beispiel |
|---|---|---|
| `ERROR` | Fehler, Handlungsbedarf | DB-Verbindung verloren, API-500 |
| `WARN` | Auffällig, aber nicht fatal | Rate-Limit erreicht, Timeout |
| `INFO` | Normale Betriebsereignisse | Server gestartet, Login, Patient angelegt |
| `DEBUG` | Detail (nur Entwicklung / Fehlersuche) | SQL-Queries, Request-Details |

**Default:** `INFO`

### 3.2 Konfiguration

```bash
# Log-Level über Env
export RUST_LOG=kis=info          # oder: kis=debug, kis=error
```

```toml
# config/kis.toml (Soll)
[server]
log_level = "info"
log_format = "json"   # json | pretty
log_file = "data/kis.log"
```

### 3.3 Log-Format

Strukturiertes **JSON**-Logging (tracing-subscriber):

```json
{
  "timestamp": "2026-11-01T12:00:00.123Z",
  "level": "INFO",
  "module": "kis::api",
  "message": "patient created",
  "patient_id": "9f1c2a4e-...",
  "actor": "dr.mueller",
  "ip": "10.0.0.15",
  "duration_ms": 12
}
```

> **DSGVO:** **Keine** Patientennamen, Kassennummern oder Diagnosen im Klartext in den Logs. Nur pseudonymisierte IDs.

### 3.4 Log-Rotation

```bash
# /etc/logrotate.d/kis
/data/kis.log {
    daily
    rotate 30          # 30 Tage behalten
    compress
    delaycompress
    missingok
    notifempty
    copytruncate
    create 0640 root kis
}
```

### 3.5 Log-Aufbewahrung

| Log-Typ | Aufbewahrung |
|---|---|
| Anwendung-Log (`kis.log`) | 90 Tage lokal, 12 Monate archiviert |
| Audit-Log (DB) | 6+ Jahre (siehe [SECURITY.md](SECURITY.md)) |
| System-Log (`/var/log/syslog`) | 90 Tage |
| Firewall-Log | 90 Tage |

### 3.6 Zentrale Log-Aggregation (Soll)

Für verteilte oder große Installationen:

| Tool | Zweck |
|---|---|
| **Fluent Bit / Filebeat** | Log-Shipping |
| **Loki / Elasticsearch** | Aggregation, Suche |
| **Grafana** | Visualisierung |

```
[KIS-Server] → [Fluent Bit] → [Loki] → [Grafana]
```

---

## 4. Backup

### 4.1 Schedules

| Typ | Zeit | Aufbewahrung | Ziel |
|---|---|---|---|
| **Tägliches Vollbackup** | 02:00 (nachts, niedrige Last) | 30 Tage lokal | `/var/lib/kis-backup/` |
| **Wöchentliches Offsite** | Sonntag, 03:00 | 12 Monate | Externes Gerät / anderes Netz |
| **Monatliches Archiv** | 1. des Monats, 04:00 | 10 Jahre | Band / Langzeit-Speicher |
| **Audit-Log-Export** | Wöchentlich, 05:00 | 6+ Jahre | Getrennter, verschlüsselter Speicher |

### 4.2 Backup-Skript

```bash
#!/usr/bin/env bash
# /usr/local/bin/kis-backup.sh
set -euo pipefail

BACKUP_DIR="/var/lib/kis-backup"
DB_FILE="data/kis.db"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/kis-${TIMESTAMP}.db"

mkdir -p "$BACKUP_DIR"

# 1. Online-Backup (konsistent, WAL-fähig)
sqlite3 "$DB_FILE" ".backup '${BACKUP_FILE}'"

# 2. Prüfsumme
sha256sum "$BACKUP_FILE" > "${BACKUP_FILE}.sha256"

# 3. Verschlüsseln (age)
age -r /etc/kis/backup-recipient.txt -o "${BACKUP_FILE}.age" "$BACKUP_FILE"
rm -f "$BACKUP_FILE"

# 4. Cleanup: älter als 30 Tage
find "$BACKUP_DIR" -name "kis-*.age" -mtime +30 -delete
find "$BACKUP_DIR" -name "kis-*.age.sha256" -mtime +30 -delete

# 5. Log
echo "$(date -Iseconds) Backup completed: ${BACKUP_FILE}.age" >> /var/log/kis-backup.log
```

**Cron:**

```cron
# /etc/cron.d/kis-backup
0 2 * * * kis /usr/local/bin/kis-backup.sh
```

### 4.3 Restore-Test

> **Pflicht:** Das Backup muss **regelmäßig getestet** werden. Ein ungetestetes Backup ist kein Backup.

**Test-Frequenz:** Monatlich

**Test-Verfahren:**

```bash
# 1. Test-DB erstellen
mkdir -p /tmp/kis-restore-test

# 2. Backup dechiffrieren
age -d -i /etc/kis/backup-key.txt \
  /var/lib/kis-backup/kis-latest.age \
  -o /tmp/kis-restore-test/kis-test.db

# 3. Prüfsumme verifizieren
sha256sum -c /var/lib/kis-backup/kis-latest.age.sha256

# 4. Integrität prüfen
sqlite3 /tmp/kis-restore-test/kis-test.db "PRAGMA integrity_check;"
# erwartet: ok

# 5. Daten-Abfrage testen
sqlite3 /tmp/kis-restore-test/kis-test.db \
  "SELECT count(*) FROM patients; SELECT count(*) FROM appointments;"

# 6. Aufräumen
rm -rf /tmp/kis-restore-test
```

### 4.4 Backup-Verifikation (automatisiert)

```bash
# Nach jedem Backup: Integrität prüfen
sqlite3 "/var/lib/kis-backup/kis-latest.db" "PRAGMA integrity_check;" | grep -q "ok" || \
  /usr/local/bin/alert --level P2 --msg "Backup-Integritätsprüfung fehlgeschlagen"
```

---

## 5. Upgrade-Prozedur

### 5.1 Vor dem Upgrade

| Schritt | Befehl / Aktion |
|---|---|
| 1. Wartungsfenster einplanen | Kommunikation an Klinik-IT + medizinisches Personal |
| 2. Vollbackup erstellen | `/usr/local/bin/kis-backup.sh` |
| 3. Backup-Integrität prüfen | `sqlite3 kis.db "PRAGMA integrity_check;"` |
| 4. Release prüfen | Prüfsumme + Signatur verifizieren |
| 5. Changelog lesen | Neue Features, Breaking Changes, Migrations |
| 6. Rollback-Plan erstellen | Vorherige Version + DB-Snapshot |
| 7. Wartungsmodus aktivieren | `systemctl stop kis` + Wartungsbanner |

### 5.2 Upgrade-Ausführung

```bash
# 1. Wartungsmodus
sudo systemctl stop kis

# 2. Neuen Release-Server entpacken
cd /tmp
curl -O https://releases.kis.example.com/kis-0.2.0-x86_64-linux.tar.gz
echo "<sha256>  kis-0.2.0-x86_64-linux.tar.gz" | sha256sum -c -
minisign -Vm kis-0.2.0-x86_64-linux.tar.gz -P <public-key>
tar xzf kis-0.2.0-x86_64-linux.tar.gz

# 3. Binary ersetzen
sudo cp kis-0.2.0/kis /opt/kis/target/release/kis
sudo chmod 755 /opt/kis/target/release/kis

# 4. Migrations ausführen (falls vorhanden)
# (SQLX-Migrations werden beim Start automatisch angewendet)

# 5. Server starten
sudo systemctl start kis

# 6. Health-Check
curl -fsS http://localhost:8080/api/health
# erwartet: OK

# 7. Daten-Verifikation
sqlite3 data/kis.db "SELECT count(*) FROM patients;"

# 8. Wartungsmodus beenden
echo "Upgrade abgeschlossen, KIS ist wieder verfügbar."
```

### 5.3 Rollback

```bash
# 1. Server stoppen
sudo systemctl stop kis

# 2. Vorherige Binary wiederherstellen
sudo cp /opt/kis/target/release/kis.bak /opt/kis/target/release/kis

# 3. DB-Rollback (falls Migration reversibel)
sqlite3 data/kis.db "..."   # DOWN-Migration

# 4. Server starten
sudo systemctl start kis

# 5. Health-Check
curl -fsS http://localhost:8080/api/health
```

> **Wichtig:** Der Rollback muss **vor** dem Upgrade vorbereitet werden. Bei nicht-reversiblen Migrations ist ein **DB-Restore aus dem Backup** erforderlich.

### 5.4 Upgrade-Checkliste

| # | Schritt | Status |
|---|---|---|
| 1 | Wartungsfenster geplant | ☐ |
| 2 | Vollbackup erstellt | ☐ |
| 3 | Backup-Integrität geprüft | ☐ |
| 4 | Release-Prüfsumme + Signatur verifiziert | ☐ |
| 5 | Changelog gelesen | ☐ |
| 6 | Rollback-Plan vorbereitet | ☐ |
| 7 | Wartungsmodus aktiviert | ☐ |
| 8 | Binary ersetzt | ☐ |
| 9 | Migrations ausgeführt | ☐ |
| 10 | Server gestartet | ☐ |
| 11 | Health-Check bestanden | ☐ |
| 12 | Daten-Verifikation bestanden | ☐ |
| 13 | Wartungsmodus beendet | ☐ |
| 14 | Changelog aktualisiert | ☐ |

---

## 6. Troubleshooting

### 6.1 Häufige Probleme und Lösungen

| Problem | Symptom | Ursache | Lösung |
|---|---|---|---|
| Server startet nicht | `systemctl status kis` → failed | Config-Fehler, Port belegt | `journalctl -u kis` prüfen, Port-Belegung prüfen |
| DB nicht erreichbar | `500 Internal Server Error` | Datei fehlend, Berechtigungen | `ls -la data/kis.db`, `chown kis:kis data/` |
| Port 8080 nicht erreichbar | `Connection refused` | Server down, Firewall | `ss -tlnp \| grep 8080`, Firewall prüfen |
| Langsame Antworten | `429` oder Timeouts | DB-Pool erschöpft, Last | Pool-Größe erhöhen, Index-Prüfung |
| HL7: keine Nachrichten | LIS meldet Verbindungsfehler | `enabled = false`, Port 6667 | `interfaces.hl7.enabled = true`, Firewall |
| FHIR: 404 | `Not Found` auf `/fhir` | `enabled = false` | `interfaces.fhir.enabled = true` |
| DICOM: C-STORE abgelehnt | `0xC011` | AE Title / SOP Class | AE-Whitelist, Modality-Mapping |
| LLM: kein Response | Timeout, 500 | API-Key ungültig, Rate-Limit | `OPENROUTER_API_KEY` prüfen, Provider-Status |
| Disk voll | `no space left on device` | Logs, Backup, DB-Wachstum | Logs rotieren, DB vakuumpacken, Backup-Cleanup |
| Audit-Integrität verletzt | Alert `KisAuditIntegrity` | Manipulation, Bug | Sofort P1-Eskalation, forensische Aufarbeitung |

### 6.2 Diagnose-Kommandos

```bash
# Service-Status
sudo systemctl status kis
journalctl -u kis --since "1 hour ago"

# Port-Prüfung
ss -tlnp | grep -E '8080|6667|104'

# DB-Integrität
sqlite3 data/kis.db "PRAGMA integrity_check;"
sqlite3 data/kis.db "PRAGMA foreign_key_check;"

# DB-Größe
ls -lh data/kis.db
sqlite3 data/kis.db "PRAGMA page_count; PRAGMA page_size;"

# Aktive Verbindungen
sqlite3 data/kis.db "SELECT * FROM pragma_database_list;"

# System-Ressourcen
df -h /data
free -h
top -p $(pgrep -d',' kis)

# HTTP-Test
curl -v http://localhost:8080/api/health
curl -v http://localhost:8080/fhir/metadata
```

### 6.3 Log-Analyse

```bash
# Letzte 100 Fehler
journalctl -u kis -p err -n 100

# Gesucht nach Patient (pseudonymisiert)
journalctl -u kis | grep "9f1c2a4e-..."

# Audit-Log-Integrität
sqlite3 data/kis.db \
  "SELECT seq, action, timestamp FROM audit_log ORDER BY seq DESC LIMIT 20;"
```

### 6.4 Eskalationsmatrix

| Stufe | Auslöser | Benachrichtigung | Antwortzeit |
|---|---|---|---|
| **P1** | Systemdown, Datenverlust, Sicherheitsverletzung | IT-Leitung, DSB, Geschäftsführung | < 15 min |
| **P2** | Ausfall einer Schnittstelle, Performance-Einbruch | IT-Team | < 1 h |
| **P3** | Einzelne Fehler, geringe Performance-Einbuße | IT-Team | < 4 h |
| **P4** | Kosmetische Probleme, Einzelne Benutzereingaben | IT-Team | < 24 h |

> Details siehe [SECURITY.md](SECURITY.md) → Incident Response.

---

## 7. Performance-Tuning

### 7.1 Datenbank-Tuning

```sql
-- Cache vergrößern (64 MB)
PRAGMA cache_size = -64000;

-- mmap (256 MB)
PRAGMA mmap_size = 268435456;

-- Synchronisierung (WAL: NORMAL ist sicher + schnell)
PRAGMA synchronous = NORMAL;

-- Temp in RAM
PRAGMA temp_store = MEMORY;

-- WAL-Checkpoint-Intervall (1000 Seiten ≈ 4 MB)
PRAGMA wal_autocheckpoint = 1000;
```

> Diese PRAGMAs werden idealerweise in der `db`-Initialisierung (SQLX After-Connect-Hook) gesetzt.

### 7.2 Pool-Tuning

```toml
[database]
pool_size = 8        # Default; bei hoher Last auf 16–32 erhöhen
```

> **Faustregel:** `pool_size` ≈ Anzahl der gleichzeitigen Anfragen × 1.5. Bei 20 parallelen Workstations: `pool_size = 30`.

### 7.3 Server-Tuning

```toml
[server]
workers = 4          # Default; auf Anzahl der CPU-Kerne setzen
```

> **Faustregel:** `workers` = Anzahl der physischen CPU-Kerne. Auf einem 8-Core-Server: `workers = 8`.

### 7.4 Skalierung

| Last | Empfohlene Maßnahme |
|---|---|
| < 50 Anfragen/min | Default-Config (1 Server) |
| 50–500 Anfragen/min | `pool_size` erhöhen, `workers` = Kerne |
| 500–5000 Anfragen/min | **PostgreSQL** statt SQLite, Connection-Pooling (PgBouncer) |
| > 5000 Anfragen/min | Cluster, Replikation, Load-Balancing, Caching (Redis) |

> **Wichtig:** SQLite ist für **ein** Schreib-Node optimiert. Für verteilte oder sehr hohe Schreib-Lasten ist PostgreSQL der richtige Weg (siehe [DATABASE.md](DATABASE.md) → Skalierung).

### 7.5 Monitoring der Performance

```bash
# Query-Analyse
sqlite3 data/kis.db "EXPLAIN QUERY PLAN SELECT * FROM appointments WHERE doctor_id = 'x' AND start_time > 'y';"

# Langsame Queries (Soll: über tracing)
RUST_LOG=kis::db=debug cargo run   # zeigt SQL + Dauer

# Top-Queries (aus den Logs)
grep "duration_ms" data/kis.log | sort -t' ' -k4 -rn | head -20
```

---

## 8. Betriebstagebuch

### 8.1 Tägliche Routinen

| Zeit | Aufgabe |
|---|---|
| 06:00 | Health-Check prüfen (`/api/health`) |
| 06:00 | Vortages-Backup erfolgreich? (Log-Prüfung) |
| 06:00 | Disk-Auslastung prüfen (< 80 %) |
| 06:00 | Kritische Logs prüfen (ERROR/WARN) |

### 8.2 Wöchentliche Routinen

| Tag | Aufgabe |
|---|---|
| Montag | Audit-Log-Hash-Kette verifizieren |
| Mittwoch | DB-Integritätsprüfung (`integrity_check`) |
| Freitag | Backup-Test (Restore in Test-System) |
| Sonntag | Offsite-Backup prüfen |

### 8.3 Monatliche Routinen

| Aufgabe |
|---|
| Restore-Test (vollständiges Restore in Test-System) |
| Performance-Review (Top-Queries, Latenz-Trends) |
| Sicherheits-Review (Patches, `cargo audit`) |
| Dokumentation aktualisieren (Konfigurationsänderungen) |

### 8.4 Jährliche Routinen

| Aufgabe |
|---|
| Disaster-Recovery-Test (vollständiges Restore) |
| Penetration-Test / Sicherheits-Audit |
| MDR-/ISO-27001-Aufrechterhaltungs-Audit |
| Kapazitätsplanung (DB-Wachstum, Hardware) |
| Zertifizierungen prüfen (TLS-Zertifikate, Software-Lizenzen) |

---

## 9. Wartungs-Modus

### 9.1 Wartung einleiten

```bash
# 1. Wartungsbanner setzen
echo "KIS befindet sich in Wartung. Bitte später erneut versuchen." > /var/www/maintenance.html

# 2. Server stoppen
sudo systemctl stop kis

# 3. Wartungsseite aktivieren (über Reverse-Proxy)
# (Nginx/Caddy: auf maintenance.html umleiten)
```

### 9.2 Wartung beenden

```bash
# 1. Wartungsbanner entfernen
rm /var/www/maintenance.html

# 2. Server starten
sudo systemctl start kis

# 3. Health-Check
curl -fsS http://localhost:8080/api/health
```

---

## 10. Kontakt und Eskalation

| Rolle | Kontakt | Verantwortung |
|---|---|---|
| IT-Service-Desk | service-desk@klinik.de | Erste Anlaufstelle, P3/P4 |
| KIS-Administrator | kis-admin@klinik.de | Betrieb, Upgrades, P1/P2 |
| Datenschutzbeauftragte(r) | dsgvo@klinik.de | Datenschutz-Vorfälle |
| Geschäftsführung | gf@klinik.de | P1-Eskalation |
| Hersteller-Support | support@kis.example.com | Software-Fehler, Patches |

---

**Verwandte Dokumente:** [SECURITY.md](SECURITY.md) · [DATABASE.md](DATABASE.md) · [API.md](API.md) · [DEVELOPMENT.md](DEVELOPMENT.md)
