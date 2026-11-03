# KIS — Load-Testing mit k6 (Task 5.6)

Produktionsnahe Lasttests für die KIS-API (Patienten + Termine) mit [k6](https://k6.io/).

## Dateien

| Datei | Zweck |
|---|---|
| `patients.k6.js` | Patient-Endpunkte: GET (Liste + Cursor-Pagination), POST (Create), PUT (Update + Optimistic Locking) |
| `appointments.k6.js` | Termin-Endpunkte: analog, inkl. Doppelbuchungsprüfung |
| `Dockerfile` | k6-Image für Docker-Execution (`k6io/k6:latest`) |
| `README.md` | Diese Anleitung |

## 1. k6 installieren

**Option A — Binär (empfohlen für Linux/macOS):**

```bash
# Debian/Ubuntu
curl -s https://dl.k6.io/release/v0.51.0/k6-v0.51.0-linux-amd64.tar.gz | tar xz
sudo mv k6-v0.51.0-linux-amd64 /usr/local/bin/k6
k6 version

# macOS (Homebrew)
brew install k6
```

**Option B — Docker:**

```bash
docker pull k6io/k6:latest
```

**Option C — npm (Node-Umgebungen):**

```bash
npm install -g k6
```

## 2. Test-Instanz vorbereiten

Die Skripte laufen gegen eine **eigene Test-Instanz** (nie Produktion!):

```bash
# 1. KIS mit PostgreSQL starten (z. B. docker-compose oder lokal)
#    PostgreSQL muss laufen: docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=kis postgres:16

# 2. Rate-Limiting für den Load-Test erhöhen (sonst 429-Fehler):
export KIS_SECURITY__RATE_LIMIT__NORMAL_PER_MIN=1000000
export KIS_SECURITY__RATE_LIMIT__AUTH_PER_MIN=1000000

# 3. DB-Connection-Limits für 2.000 VUs anpassen (siehe unten)
export KIS_DATABASE__POOL_SIZE=400

# 4. KIS starten
cargo run --release -p kis-app
```

### PostgreSQL-Config für realistische Tests

```sql
-- postgresql.conf (Test-Instanz)
max_connections = 1000          -- 2 Instanzen × 400 Pool + Headroom
shared_buffers = 2GB            -- ~25% RAM
effective_cache_size = 6GB
work_mem = 16MB
maintenance_work_mem = 512MB
wal_buffers = 16MB
checkpoint_completion_target = 0.9
```

Für 2.000 VUs braucht die DB:
- **Write-Pool**: 400 Connections pro KIS-Instanz (Default in KIS: 200)
- **Read-Pool**: 400 (falls Read-Replica konfiguriert)
- `max_connections` ≥ (Anzahl-Instanzen × Pool-Size) + 50 Headroom

## 3. Tests starten

```bash
# Patienten-Load-Test (3 Szenarien, 2.000 VUs max parallel, 60s)
KIS_BASE_URL=http://localhost:8080 k6 run load-tests/patients.k6.js

# Termin-Load-Test (analog)
KIS_BASE_URL=http://localhost:8080 k6 run load-tests/appointments.k6.js
```

**Parameter (ENV):**

| Variable | Default | Beschreibung |
|---|---|---|
| `KIS_BASE_URL` | `http://localhost:8080` | Ziel-URL |
| `KIS_LOAD_SEED` | `1000` | Anzahl Setup-Patienten (patients.k6.js) |
| `KIS_LOAD_DOCTORS` | `200` | Setup-Ärzte (appointments.k6.js) |
| `KIS_LOAD_PATIENTS` | `1000` | Setup-Patienten (appointments.k6.js) |
| `KIS_LOAD_APPTS` | `1000` | Setup-Termine (appointments.k6.js) |

**Mit Docker:**

```bash
docker run --rm \
  -v "$(pwd)/load-tests:/scripts" \
  -e KIS_BASE_URL=http://host.docker.internal:8080 \
  k6io/k6:latest run /scripts/patients.k6.js
```

**JSON-Report (für CI/Auswertung):**

```bash
KIS_BASE_URL=http://localhost:8080 k6 run \
  --out json=results/patients.json \
  load-tests/patients.k6.js
```

## 4. Test-Szenarien

### patients.k6.js

| Szenario | VUs | Dauer | Endpunkt | Verhalten |
|---|---|---|---|---|
| `list_patients` | 1.000 | 60s | `GET /api/v1/patients` | 2–4 Seiten Cursor-Pagination (limit=50) |
| `create_patients` | 500 | 60s | `POST /api/v1/patients` | Realistische Payload (Name, Alter, Allergien, Adresse) |
| `update_patients` | 500 | 60s | `PUT /api/v1/patients/:id` | GET (Version lesen) → PUT (Optimistic Locking) |

### appointments.k6.js

| Szenario | VUs | Dauer | Endpunkt | Verhalten |
|---|---|---|---|---|
| `list_appointments` | 1.000 | 60s | `GET /api/v1/appointments` | 1–3 Seiten Cursor-Pagination |
| `create_appointments` | 500 | 60s | `POST /api/v1/appointments` | Doppelbuchungsprüfung, fiktives Jahr 2099 |
| `update_appointments` | 500 | 60s | `PUT /api/v1/appointments/:id` | GET → PUT (Optimistic Locking) |

**Setup/Teardown:**
- Setup: Erzeugt Test-Daten per API (Patienten: 1.000; Termine: 200 Ärzte + 1.000 Patienten + 1.000 Termine im fiktiven Jahr 2099 → keine Kollision mit echten Daten)
- Teardown: Löscht alle Setup-Daten per API (Soft-Delete)

**Error-Handling im Skript:**
- 409 CONFLICT (Optimistic Locking / Doppelbuchung) wird als **erwartet** behandelt, nicht als Fehler
- Checks verifizieren Struktur (id vorhanden, version gesteigert, items-Array)
- Teardown loggt Anzahl erfolgter Löschungen

## 5. Ergebnisse interpretieren

### Wichtige Metriken (k6-Output)

```
http_req_duration.....: avg=45.2ms  min=12ms  med=38ms  max=890ms  p(95)=120ms  p(99)=320ms
http_req_failed.......: 0.32%  ✓ 0 failed out of 540000
checks.................: 99.8%  ✓ 488000 out of 489000
```

**Thresholds (in den Skripten definiert):**

| Metrik | Ziel | Bedeutung |
|---|---|---|
| `http_req_duration p(95)` | **< 200ms** | 95% der Requests unter 200ms |
| `http_req_duration p(99)` | **< 500ms** | 99% der Requests unter 500ms |
| `http_req_failed` | **< 1%** | Fehlerquote (HTTP 5xx + Netzwerkfehler) |
| `checks` | **> 99%** | Antwort-Struktur valide |

### Auswertung

- **Grüner Haken (✓)** = Threshold erfüllt
- **Roter Fehler (✗)** = Threshold verletzt → Test abgebrochen (Exit-Code ≠ 0)
- Bei `p(95) > 200ms`: DB-Pool vergrößern, Read-Replica prüfen, Index-Check
- Bei `p(99) > 500ms`: Long-Tail-Analyse — einzelne langsame Queries (EXPLAIN ANALYZE)
- Bei `error_rate > 1%`: 503 = Pool erschöpft (max_connections), 500 = App-Fehler, 429 = Rate-Limit

### Typische Problemquellen bei 2.000 VUs

1. **DB-Connection-Erschöpfung**: `max_connections` zu niedrig → 503/Timeout. Pool-Size × Instanzen < max_connections + Headroom einhalten.
2. **CPU-Sättigung**: k6 selbst braucht ~1 CPU-Core pro 500 VUs → k6 auf separater Maschine oder mit mehr Cores laufen lassen.
3. **Lock-Waits**: Optimistic-Locking-Konflikte (409) sind normal; aber lange DB-Transaktionen (Audit-Log-Hash-Chain) können Serialize-Conflicts erzeugen.

## 6. Hardware-Empfehlung für realistische Tests

| Rolle | Mindest-Config |
|---|---|
| k6-Runner | 4 vCPU, 4GB RAM, 1Gbit/s (2.000 VUs) |
| KIS-Instanz(en) | 4 vCPU, 8GB RAM pro Instanz |
| PostgreSQL | 8 vCPU, 16GB RAM, NVMe SSD, `shared_buffers` = 25% RAM |
| Netzwerk | Alle Komponenten im selben Rechenzentrum/VM-Host (keine WAN-Latenz) |

**Wichtig:** k6-Runner und Ziel-Server sollten **nicht** dieselbe Maschine sein (k6 verbraucht signifikante CPU für 1.000+ VUs).

## 7. CI-Integration (optional)

```yaml
# .github/workflows/loadtest.yml
- name: Run k6 load test
  uses: grafana/k6-action@v0.3.0
  env:
    KIS_BASE_URL: http://kis-test:8080
  with:
    filename: load-tests/patients.k6.js
    output-file: loadtest-results.json
```
