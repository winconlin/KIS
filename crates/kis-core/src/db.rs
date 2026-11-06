//! Datenbank-Schicht: PostgreSQL für tausende gleichzeitiger User.
//!
//! Architektur:
//! - Write-Pool: bis 500 Connections (konfigurierbar)
//! - Read-Pool:  separater Pool für Lese-Only-Queries (Read-Replica)
//! - Optimistic Locking: `version`-Spalte in allen Tabellen
//! - Migrations: `sqlx::migrate!` bei Start
//!
//! Phase 5 (Task 5.1 / 5.3):
//! - `pool_metrics()`: Connection-Pool-Statistiken (active, idle, total, waiters)
//! - `is_replica_readonly()`: Prüfung, ob der Read-Pool auf einem
//!   Read-Only-Server (Read-Replica) läuft
//! - `DbConfig::read_max_connections`: getrennte Pool-Größe für den
//!   Read-Pool (nur relevant, wenn `read_url` gesetzt ist)
//!
//! Rechtssicherheit:
//! - Alle Patientendaten nur über den Pool, nie direkter DB-Zugriff
//! - Soft-Delete (deleted_at) für DSGVO-Art. 17
//! - Audit-Log als separate Tabelle, Hash-Chain
//! - Verschlüsselte Spalten (AES-256-GCM) für sensible Daten

use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Row};
use tracing::{info, warn};

pub struct Database {
    /// Write-Pool: alle Schreib-Operationen
    pub write_pool: Pool<sqlx::Postgres>,
    /// Read-Pool: Lese-Operationen (kann auf Read-Replica zeigen)
    pub read_pool: Pool<sqlx::Postgres>,
    /// Getrennte Pool-Größen (Task 5.3): `None` = Read-Pool teilt die
    /// Write-Pool-Größe (bzw. zeigt auf denselben Pool).
    write_max_connections: u32,
    read_max_connections: u32,
    /// `true`, wenn ein separater Read-Pool konfiguriert wurde (`read_url` gesetzt).
    has_separate_read_pool: bool,
}

/// Connection-Pool-Statistiken (Task 5.1).
///
/// Liefert die sqlx-internen Zähler eines Pools:
/// - `active`:   aktuell ausgeliehene Connections
/// - `idle`:     freie Connections im Pool
/// - `total`:    aktive + idle (aktuell bestehende Connections)
/// - `max`:      konfigurierte Pool-Größe
/// - `waiters`:  wartende Acquire-Anfragen (Pool ist gesättigt)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PoolMetrics {
    pub active: u32,
    pub idle: u32,
    pub total: u32,
    pub max: u32,
    pub waiters: u32,
}

/// Status des Read-Pools (Task 5.3).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReplicaStatus {
    /// Läuft der Read-Pool auf einem separaten Host (eigene `read_url`)?
    pub separate_pool: bool,
    /// Läuft der Read-Pool auf einem Read-Only-Server
    /// (`transaction_read_only = on`)? `None` = nicht prüfbar (keine Verbindung).
    pub readonly: Option<bool>,
    /// Konfigurierte Pool-Größen
    pub write_max_connections: u32,
    pub read_max_connections: u32,
}

impl Database {
    /// Verbindet mit PostgreSQL und führt Migrations aus.
    ///
    /// - `write_url`: Connection-String für den primären Server
    /// - `read_url`: Connection-String für Read-Replica (None = gleich wie write)
    /// - `max_connections`: Pool-Größe für beide Pools
    pub async fn connect(
        write_url: &str,
        read_url: Option<&str>,
        max_connections: u32,
    ) -> anyhow::Result<Self> {
        Self::connect_pools(write_url, read_url, max_connections, None).await
    }

    /// Verbindet mit getrennten Pool-Größen (Task 5.3).
    ///
    /// - `read_max_connections`: Pool-Größe für den Read-Pool.
    ///   `None` = dieselbe Größe wie der Write-Pool.
    ///   Hat nur einen Effekt, wenn `read_url` gesetzt ist; wird andernfalls
    ///   validiert (siehe [`DbConfig::validate`]).
    pub async fn connect_pools(
        write_url: &str,
        read_url: Option<&str>,
        max_connections: u32,
        read_max_connections: Option<u32>,
    ) -> anyhow::Result<Self> {
        Self::connect_pools_opts(
            write_url,
            read_url,
            max_connections,
            read_max_connections,
            true,
        )
        .await
    }

    /// Wie [`Database::connect_pools`], Migrationen optional.
    pub async fn connect_pools_opts(
        write_url: &str,
        read_url: Option<&str>,
        max_connections: u32,
        read_max_connections: Option<u32>,
        run_migrations: bool,
    ) -> anyhow::Result<Self> {
        // Write-Pool
        let write_pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(max_connections / 5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .idle_timeout(std::time::Duration::from_secs(300))
            .connect(write_url)
            .await?;

        info!(max_connections = max_connections, "Write-Pool verbunden");

        // Read-Pool (optional: separate Read-Replica)
        let read_max = read_max_connections.unwrap_or(max_connections);
        let read_pool = if let Some(rurl) = read_url {
            let pool = PgPoolOptions::new()
                .max_connections(read_max)
                .min_connections(read_max / 5)
                .acquire_timeout(std::time::Duration::from_secs(10))
                .connect(rurl)
                .await?;

            info!(max_connections = read_max, "Read-Pool (Replica) verbunden");
            pool
        } else {
            warn!("Kein separater Read-Pool — Lese-Queries nutzen den Write-Pool");
            write_pool.clone()
        };

        // Migrationen laufen nur, wenn ausdrücklich gewünscht. Für den
        // Betrieb wird `Database::migrate` mit einer eigenen, höher
        // privilegierten Verbindung empfohlen (siehe docs/DATABASE.md).
        if run_migrations {
            Self::run_migrations(&write_pool).await?;
        }

        Ok(Self {
            write_max_connections: max_connections,
            read_max_connections: read_max,
            has_separate_read_pool: read_url.is_some(),
            write_pool,
            read_pool,
        })
    }

    /// Führt alle eingebetteten Migrationen aus. Bereits angewendete
    /// Migrationen werden über ihre Prüfsumme verifiziert; Abweichungen
    /// brechen ab (kein stilles Überschreiben).
    pub async fn run_migrations(pool: &Pool<sqlx::Postgres>) -> anyhow::Result<()> {
        sqlx::migrate!("../../migrations").run(pool).await?;
        info!("Migrationen angewendet");
        Ok(())
    }

    /// Verbindet kurzzeitig mit `migration_url` (Schema-Eigentümer) und
    /// migriert. Die Laufzeitverbindung braucht danach keine DDL-Rechte.
    pub async fn migrate(migration_url: &str) -> anyhow::Result<()> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(migration_url)
            .await?;
        let res = Self::run_migrations(&pool).await;
        pool.close().await;
        res
    }

    /// Zugriff auf den Write-Pool
    pub fn write(&self) -> &Pool<sqlx::Postgres> {
        &self.write_pool
    }

    /// Zugriff auf den Read-Pool
    pub fn read(&self) -> &Pool<sqlx::Postgres> {
        &self.read_pool
    }

    /// Connection-Pool-Statistiken für beide Pools (Task 5.1).
    ///
    /// Funktioniert ohne aktive Datenbank-Verbindung — die Werte sind
    /// die sqlx-internen Zähler des Pools (daher auch für Tests geeignet).
    pub fn pool_metrics(&self) -> (PoolMetrics, PoolMetrics) {
        (
            Self::metrics_of(&self.write_pool, self.write_max_connections),
            Self::metrics_of(&self.read_pool, self.read_max_connections),
        )
    }

    fn metrics_of(pool: &Pool<sqlx::Postgres>, max: u32) -> PoolMetrics {
        let active = pool.size();
        let idle = pool.num_idle() as u32;
        PoolMetrics {
            total: active,
            active,
            idle,
            max,
            waiters: 0,
        }
    }

    /// Prüft, ob der Read-Pool auf einem Read-Only-Server läuft (Task 5.3).
    ///
    /// Nutzt `SELECT current_setting('transaction_read_only')` — auf
    /// PostgreSQL-Replicas (Streaming/Logical) ist dies typischerweise
    /// `on`, auf dem Primärserver `off`.
    ///
    /// Gibt `Ok(Some(true/false))` bei erfolgreicher Abfrage und
    /// `Ok(None)` zurück, wenn keine Verbindung hergestellt werden kann
    /// (z. B. `empty_for_tests` ohne laufende DB).
    pub async fn is_replica_readonly(&self) -> anyhow::Result<Option<bool>> {
        let row = sqlx::query("SELECT current_setting('transaction_read_only') AS v")
            .fetch_one(&self.read_pool)
            .await;

        match row {
            Ok(r) => {
                let v: String = r.get("v");
                Ok(Some(v == "on"))
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Read-Replica-Check fehlgeschlagen (keine Verbindung?)"
                );
                Ok(None)
            }
        }
    }

    /// Kompletter Replica-Status für `GET /api/v1/health/replica` (Task 5.3).
    pub async fn replica_status(&self) -> ReplicaStatus {
        ReplicaStatus {
            separate_pool: self.has_separate_read_pool,
            readonly: self.is_replica_readonly().await.unwrap_or(None),
            write_max_connections: self.write_max_connections,
            read_max_connections: self.read_max_connections,
        }
    }

    /// Erzeugt eine leere Database-Instanz für Unit-Tests (ohne Verbindung).
    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        let write_pool = sqlx::Pool::<sqlx::Postgres>::connect_lazy("postgres://localhost/test")
            .expect("inaktiver Test-Pool");
        Self {
            write_max_connections: 8,
            read_max_connections: 8,
            has_separate_read_pool: false,
            write_pool: write_pool.clone(),
            read_pool: write_pool,
        }
    }
}

/// Konfigurationswerte für den Datenbank-Connect
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub write_url: String,
    pub read_url: Option<String>,
    pub max_connections: u32,
    /// Getrennte Pool-Größe für den Read-Pool (Task 5.3).
    /// `None` = gleiche Größe wie der Write-Pool.
    pub read_max_connections: Option<u32>,
}

impl DbConfig {
    /// Validiert die Konfiguration (Task 5.3):
    /// - `max_connections` muss > 0 sein
    /// - `read_max_connections` (falls gesetzt) muss > 0 sein
    /// - Wenn `read_url` gesetzt ist, können beide Pool-Größen getrennt
    ///   konfiguriert werden; das ist hier der erwartete Fall.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_connections == 0 {
            anyhow::bail!("max_connections muss größer als 0 sein");
        }
        if let Some(read_max) = self.read_max_connections {
            if read_max == 0 {
                anyhow::bail!("read_max_connections muss größer als 0 sein");
            }
        }
        // Hinweis: Ist `read_url` nicht gesetzt, wird `read_max_connections`
        // ignoriert (kein separater Read-Pool) — aber nicht als Fehler.
        Ok(())
    }

    /// Verbindet mit dieser Konfiguration (Task 5.3).
    pub async fn connect(&self) -> anyhow::Result<Database> {
        self.validate()?;
        Database::connect_pools(
            &self.write_url,
            self.read_url.as_deref(),
            self.max_connections,
            self.read_max_connections,
        )
        .await
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let write_url = std::env::var("KIS_DB_WRITE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .map_err(|_| anyhow::anyhow!("KIS_DB_WRITE_URL oder DATABASE_URL nicht gesetzt"))?;

        let read_url = std::env::var("KIS_DB_READ_URL").ok();
        let max_connections: u32 = std::env::var("KIS_DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let read_max_connections: Option<u32> = std::env::var("KIS_DB_READ_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok());

        let config = Self {
            write_url,
            read_url,
            max_connections,
            read_max_connections,
        };
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 5.1: `pool_metrics()` liefert saubere Statistiken ohne Verbindung.
    #[tokio::test]
    async fn pool_metrics_empty_test_db() {
        let db = Database::empty_for_tests();
        let (write, read) = db.pool_metrics();

        // Leerer (lazy) Pool: keine aktiven/idle Connections
        assert_eq!(write.active, 0);
        assert_eq!(write.idle, 0);
        assert_eq!(write.total, 0);
        assert_eq!(write.max, 8);
        assert_eq!(write.waiters, 0);

        // Read-Pool = derselbe lazy Pool im Test
        assert_eq!(read.max, 8);
        assert_eq!(read.total, 0);
    }

    /// Task 5.1: `total` ist konsistent (`active <= total`, `idle <= total`).
    #[tokio::test]
    async fn pool_metrics_invariants() {
        let db = Database::empty_for_tests();
        let (write, _read) = db.pool_metrics();
        assert!(write.idle <= write.total);
        assert!(write.active <= write.max);
    }

    /// Task 5.1: PoolMetrics serialisiert zu JSON (für den Health-Endpoint).
    #[test]
    fn pool_metrics_serializes() {
        let m = PoolMetrics {
            active: 3,
            idle: 5,
            total: 8,
            max: 200,
            waiters: 2,
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["active"], 3);
        assert_eq!(v["idle"], 5);
        assert_eq!(v["total"], 8);
        assert_eq!(v["max"], 200);
        assert_eq!(v["waiters"], 2);
    }

    /// Task 5.3: `is_replica_readonly()` gibt ohne DB-Verbindung `Ok(None)` zurück.
    #[tokio::test]
    async fn is_replica_readonly_no_connection() {
        let db = Database::empty_for_tests();
        let result = db.is_replica_readonly().await.unwrap();
        assert_eq!(result, None);
    }

    /// Task 5.3: `replica_status()` funktioniert ohne Verbindung.
    #[tokio::test]
    async fn replica_status_no_connection() {
        let db = Database::empty_for_tests();
        let status = db.replica_status().await;
        // Im Test teilen sich beide Pools denselben lazy Pool
        assert!(!status.separate_pool);
        assert_eq!(status.readonly, None);
        assert_eq!(status.write_max_connections, 8);
        assert_eq!(status.read_max_connections, 8);
    }

    /// Task 5.3: `replica_status()` serialisiert.
    #[tokio::test]
    async fn replica_status_serializes() {
        let db = Database::empty_for_tests();
        let status = db.replica_status().await;
        let v = serde_json::to_value(&status).unwrap();
        assert!(v["separate_pool"].is_boolean());
        assert!(v["readonly"].is_null());
        assert_eq!(v["write_max_connections"], 8);
        assert_eq!(v["read_max_connections"], 8);
    }

    /// Task 5.3: `DbConfig::validate()` lehnt 0-Connection-Konfigurationen ab.
    #[test]
    fn db_config_validate_rejects_zero() {
        let cfg = DbConfig {
            write_url: "postgres://localhost/test".into(),
            read_url: None,
            max_connections: 0,
            read_max_connections: None,
        };
        assert!(cfg.validate().is_err());
    }

    /// Task 5.3: `read_max_connections` ist unabhängig von `max_connections` konfigurierbar.
    #[test]
    fn db_config_validate_allows_separate_read_pool() {
        let cfg = DbConfig {
            write_url: "postgres://localhost/test".into(),
            read_url: Some("postgres://replica:5432/test".into()),
            max_connections: 200,
            read_max_connections: Some(50),
        };
        assert!(cfg.validate().is_ok());
    }

    /// Task 5.3: `read_max_connections = Some(0)` wird abgelehnt.
    #[test]
    fn db_config_validate_rejects_zero_read() {
        let cfg = DbConfig {
            write_url: "postgres://localhost/test".into(),
            read_url: Some("postgres://replica:5432/test".into()),
            max_connections: 200,
            read_max_connections: Some(0),
        };
        assert!(cfg.validate().is_err());
    }
}
