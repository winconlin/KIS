# KIS — Horizontal Scaling & Stateless Design (Task 5.12)

> **Hinweis (06.11.2026):** Teile dieses Dokuments beschreiben den entfernten SQLite-Prototyp oder ein Zielbild. Maßgeblich für den Ist-Zustand sind [INSTALLATION](INSTALLATION.md), [API](API.md), [DATABASE](DATABASE.md) und [REVIEW_STATUS](REVIEW_STATUS.md).


Dieses Dokument beschreibt, warum der KIS-Server **horizontalskalierbar** ist,
wie man ihn hinter einem Load-Balancer betreibt und wie der Graceful Shutdown
funktioniert.

## 1. Stateless-Design: Warum der Server horizontalskalierbar ist

Eine Server-Instanz ist **stateless**, wenn kein Request-Resultat in
Prozess-Speicher (RAM) gespeichert wird, das für einen **späteren** Request
derselben oder anderer Clients nötig ist. Jede Anfrage wird vollständig aus
der Anfrage selbst + PostgreSQL beantwortet.

### Was `AppState` enthält (crates/kis-app/src/api.rs)

| Feld | Art | Stateful? |
|---|---|---|
| `config: AppConfig` | Konfig (statisch, aus TOML/ENV) | Nein |
| `db: Arc<Database>` | PostgreSQL Write/Read-Pool | Nein (State in DB) |
| `security: Arc<Security>` | JWT-Keys, AES-Masterkey, Audit | Nein (nur Kryptografie-Params) |
| `rate_limiter: Arc<SlidingWindowRateLimiter>` | Sliding-Window (in-memory) | **Teilweise**¹ |
| `cache: Arc<EntityCache>` | moka In-Memory-Cache (TTL 5 min) | **Teilweise**² |

¹ **Rate-Limiter**: In-memory Sliding-Window. Bei Multi-Node-Deploy ist das
Limit *pro Instanz* — d. h. ein Client kann bis zu `N × Limit` Requests/min
durchkommen (N = Anzahl Instanzen). Für exakte globale Limits: auf Redis umstellen
(dokumentiert in `crates/kis-core/src/rate_limit.rs`). Für typische
Klinik-Deployments (2–4 Instanzen) ist das tolerierbar.

² **EntityCache**: Lese-Cache mit 5-min-TTL. Kein *korrekter* State — bei Cache-Miss
wird aus der DB gelesen, bei Stale-Data höchstens ein kurzzeitiges Stale-Read.
Kein Client hängt an einem bestimmten Cache-Eintrag einer bestimmten Instanz.

### Sessions & Authentifizierung

- **Access-Tokens**: JWT (stateless, signiert, keine Server-DB-Abfrage pro Request)
- **Refresh-Tokens**: JWT (24 h), Verifikation rein kryptografisch
- **Logout**: client-seitig (Token aus localStorage entfernen)
- **`sessions`-Tabelle** (PostgreSQL): existiert für Audit-Track (access_jti),
  aber **kein Request hängt an einer DB-Session-Zeile** — der Auth-Flow liest
  nie eine Session aus, um einen Request zu autorisieren

**Fazit:** Kein Request erfordert eine Verbindung zu einer bestimmten
Server-Instanz. Jeder Request kann auf jede Instanz routiert werden.

## 2. Load-Balancer-Setup

### 2.1 HAProxy (empfohlen)

```haproxy
# /etc/haproxy/haproxy.cfg

global
    log /dev/log local0
    maxconn 4096

defaults
    mode http
    log global
    timeout connect 5s
    timeout client  30s
    timeout server  30s
    retries 2
    option httplog

# --- TLS-Terminierung (empfohlen) ---
# Zertifikat: /etc/haproxy/certs/kis.pem (fullchain + key)
frontend kis_https
    bind *:443 ssl crt /etc/haproxy/certs/kis.pem
    http-request set-header X-Forwarded-Proto https
    default_backend kis_backend

frontend kis_http
    bind *:80
    redirect scheme https code 301

# --- Health-Check auf /api/v1/health ---
backend kis_backend
    balance roundrobin
    option httpchk GET /api/v1/health
    http-check expect status 200
    http-check connect inter 5s
    http-check rise 2 fall 3
    # Sticky Sessions: NICHT nötig (Stateless), aber TLS-Terminierung hier
    server kis1 10.0.0.11:8080 check
    server kis2 10.0.0.12:8080 check
    server kis3 10.0.0.13:8080 check
```

### 2.2 Nginx (Alternative)

```nginx
# /etc/nginx/conf.d/kis.conf

upstream kis_backend {
    # Stateless: kein session-stickiness nötig
    # round-robin ist Default
    server 10.0.0.11:8080 max_fails=3 fail_timeout=15s;
    server 10.0.0.12:8080 max_fails=3 fail_timeout=15s;
    server 10.0.0.13:8080 max_fails=3 fail_timeout=15s;

    # Passive Health-Check: Nginx entfernt Server bei Fehlern
}

server {
    listen 443 ssl;
    server_name kis.klinik.example;

    # TLS-Terminierung
    ssl_certificate     /etc/letsencrypt/live/kis.klinik.example/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/kis.klinik.example/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    # Health-Check-Endpunkt (direkt, kein Upstream)
    location /api/v1/health {
        proxy_pass http://kis_backend;
        access_log off;
    }

    location / {
        proxy_pass http://kis_backend;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_connect_timeout 5s;
        proxy_read_timeout 30s;
    }
}
```

## 3. Sticky Sessions

**Nicht nötig.** KIS ist stateless — jeder Request ist unabhängig.
Sticky Sessions würden nur die Lastverteilung verschlechtern und die
Skalierbarkeit einschränken (eine Instanz fällt aus → deren Clients hängen).

**Empfohlen:** TLS-Terminierung am Load-Balancer (siehe oben). Der KIS-Server
läuft dann intern über HTTP — das ist schneller (kein TLS-Handshake pro
Request im Server) und der LB terminiert TLS.

## 4. DB-Connection-Pooling

### Pro KIS-Instanz

Jede Instanz hält **zwei** Pools:

| Pool | Standard | Zweck |
|---|---|---|
| `write_pool` | 200 Connections | Alle Schreib-Operationen + Migrations |
| `read_pool` | 200 Connections | Lese-Operationen (kann auf Read-Replica zeigen) |

Konfigurierbar via `KIS_DATABASE__POOL_SIZE` (beide Pools gleicher Size).

### max_connections-Budget (PostgreSQL)

```
max_connections ≥ (Anzahl-Instanzen × (write_pool + read_pool)) + 50
```

Beispiele:

| Instanzen | Pool-Size | max_connections (mind.) |
|---|---|---|
| 1 | 200 | 450 |
| 2 | 200 | 850 |
| 4 | 200 | 1650 |
| 4 | 100 | 850 |

**Empfehlung:** Bei > 4 Instanzen Pool-Size reduzieren (100) und
Read-Replicas für Lese-Last nutzen, statt die Primary-DB zu überladen.

## 5. Horizontal Scaling & Read-Replicas

### Architektur

```
                        ┌──────────────┐
                        │   Load-Balancer  │
                        │  (HAProxy/Nginx) │
                        └──────┬───────┘
                   ┌───────────┼───────────┐
                   ▼           ▼           ▼
            ┌──────────┐ ┌──────────┐ ┌──────────┐
            │ KIS #1   │ │ KIS #2   │ │ KIS #3   │
            │ Pool 200 │ │ Pool 200 │ │ Pool 200 │
            └────┬─────┘ └────┬─────┘ └────┬─────┘
                 │             │             │
                 └─────────────┼─────────────┘
                               ▼
                    ┌─────────────────────┐
                    │   PostgreSQL Primary │  ← Single Point of Failure
                    │   (Write-Pool)       │
                    └──────────┬──────────┘
                               │ Streaming-Replication
                    ┌──────────┴──────────┐
                    ▼                     ▼
             ┌───────────┐         ┌───────────┐
             │ Read-Replica 1 │     │ Read-Replica 2 │
             │ (Read-Pool)    │     │ (Read-Pool)    │
             └───────────┘         └───────────┘
```

### Read-Replicas

- KIS unterstützt `KIS_DATABASE__READ_URL` — der Read-Pool zeigt dann auf
  eine Read-Replica
- **Schreib-Requests** gehen immer an die Primary
- **Lesen-Requests** (Patienten-Liste, Termin-Kalender, Suche) gehen an
  die Read-Replica → entlastet die Primary
- Bei > 2.000 concurrent users: mindestens 2 Read-Replicas empfehlenswert

### Single Point of Failure: Primary-DB

Die PostgreSQL-Primary ist der kritische Punkt. Mitigation:
1. **Streaming-Replication** mit 1+ Standby (hot standby)
2. **Patroni** (HA-Automatic Failover) für automatisches Failover
3. **pgBackRest** für Backups & Point-in-Time Recovery
4. KIS-Instanzen überleben einen Primary-Ausfall: Requests hängen im
   Connection-Pool (Timeout 10s), danach 503 → Load-Balancer entfernt Instanz
   via Health-Check → nach Failover neu erreichbar

## 6. Health-Check für Load-Balancer

**Endpunkt:** `GET /api/v1/health` → `200 OK` (Body: `"ok"`)

Der Health-Check prüft nur, dass der axum-Server läuft. Für einen
**tiefen** Health-Check (inkl. DB-Verbindung) kann man `GET /api/v1/status`
nutzen, der die Module und Version meldet.

**HAProxy-Config (siehe oben):**
```
option httpchk GET /api/v1/health
http-check expect status 200
http-check connect inter 5s
http-check rise 2 fall 3
```

→ 2 aufeinanderfolgende OKs = Server healthy, 3 Fehlschläge = Server entfernt.

**Nginx:** Passive Health-Check via `max_fails=3 fail_timeout=15s`.

## 7. Graceful Shutdown

Beim Empfang von **SIGTERM** (oder SIGINT) führt KIS einen strukturierten
Shutdown aus (implementiert in `crates/kis-app/src/main.rs`, Modul
`graceful_shutdown`):

```
SIGTERM
  │
  ├─ 1. Neue Requests ablehnen (503 Service Unavailable)
  │     → Load-Balancer sieht Health-Check-Fehler, entfernt Instanz
  │
  ├─ 2. Laufende Requests abschließen (max 30s Timeout)
  │     → axum serve() gibt zurück, wenn alle in-flight Requests fertig sind
  │
  ├─ 3. DB-Pools schließen (write_pool, read_pool)
  │     → sqlx Pool::close() — wartet bis alle Connections zurückgegeben sind
  │
  └─ 4. Clean Exit (Exit-Code 0)
```

**Konfiguration:** `KIS_SERVER__SHUTDOWN_TIMEOUT_SECS` (Standard: 30s).

**Systemd:**
```ini
[Service]
TimeoutStopSec=35        # 30s Grace-Timeout + 5s Buffer
KillSignal=SIGTERM
```

**Docker:**
```yaml
# docker-compose.yml
stop_grace_period: 35s
```

**K8s:**
```yaml
spec:
  terminationGracePeriodSeconds: 35
  containers:
  - name: kis
    lifecycle:
      preStop:
        exec:
          command: ["sh", "-c", "sleep 5"]  # LB-Drain vor Shutdown
```

### Unit-Tests

Die Graceful-Shutdown-Logik ist in `crates/kis-app/src/graceful_shutdown.rs`
implementiert und hat Unit-Tests (`cargo test -p kis-app`):

- `test_shutdown_timeout_completes` — Shutdown beendet sich innerhalb des Timeouts
- `test_shutdown_pool_close` — Pools werden geschlossen (Connection-Count → 0)
- `test_shutdown_rejects_new_requests` — Nach Shutdown-Signal: 503
- `test_shutdown_waits_for_inflight` — In-flight Requests werden abgewartet

## 8. Kapazitätsplanung

### Annahmen (aus Load-Test-Erfahrung)

| Operation | Durchsatz pro Instanz (4 vCPU) |
|---|---|
| GET /patients (List, 50 Items) | ~800 req/s |
| POST /patients (Create) | ~400 req/s |
| PUT /patients/:id (Update + Lock) | ~300 req/s |
| GET /appointments (List) | ~700 req/s |
| POST /appointments (Create + Doppelbuchung) | ~250 req/s |

### Berechnung

```
Anzahl-Instanzen = ceil(Concurrent-Users × Requests-pro-User-per-Min / 60 / Throughsatz-pro-Instanz)
```

**Beispiel: 5.000 Patienten, 500 concurrent users**

- Durchschnitt: 2 Requests/Minute pro User (List + 1 Create/Update)
- Gesamtlast: 500 × 2 / 60 ≈ 17 req/s
- Pro Instanz (4 vCPU, konservativ 300 req/s gemischte Last): ~167 req/s nutzbar (50% Headroom)
- **1 Instanz reicht**, aber für High-Availability: **2 Instanzen** (N+1)

**Beispiel: 50.000 Patienten, 2.000 concurrent users**

- Gesamtlast: 2000 × 2 / 60 ≈ 67 req/s
- Pro Instanz: ~167 req/s nutzbar
- **1 Instanz reicht**, aber:
  - DB wird zum Bottleneck (2.000 concurrent × 2 Pools × 200 = 800 Connections)
  - **Empfehlung: 2 Instanzen × 100 Pool-Size + 2 Read-Replicas**

**Beispiel: 100.000 Patienten, 5.000 concurrent users (Stress-Fall)**

- Gesamtlast: 5000 × 2 / 60 ≈ 167 req/s
- **4 Instanzen × 100 Pool-Size + 4 Read-Replicas + 8 vCPU DB**

### Faustformel

```
Instanzen = max(2, ceil(Concurrent-Users / 1000))
Pool-Size = min(200, max(50, 5000 / Instanzen))
Read-Replicas = max(1, floor(Instanzen / 2))
```

### Skalierungspunkte (in dieser Reihenfolge)

1. **Vertikal**: Mehr CPU/RAM für KIS-Instanz (bis 8 vCPU)
2. **Read-Replicas**: Lese-Last auf Replicas verteilen
3. **Horizontal**: Mehr KIS-Instanzen hinter LB
4. **DB-Vertikal**: Mehr CPU/RAM für PostgreSQL
5. **DB-Partitionierung**: Patienten-Tabelle nach `created_at` partitionieren
6. **Rate-Limiter auf Redis**: Globale Limits bei Multi-Node
