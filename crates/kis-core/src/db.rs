//! PostgreSQL-Zugang: Verbindungspools und Migrationen.
//!
//! - **Schreib-Pool** (`write()`): maßgeblicher Stand; für alles, was
//!   schreibt oder Zugriffsentscheidungen trifft.
//! - **Lese-Pool** (`read()`): optional eine Replica für Auswertungen;
//!   ohne `read_url` derselbe Pool.
//! - Verbindungen entstehen erst bei Bedarf und werden nach Leerlauf
//!   geschlossen — ein unbenutzter Server hält keine offenen Verbindungen.
//! - Es gibt keinen Fallback auf eine andere Datenbank.

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tracing::{info, warn};

use crate::config::DatabaseConfig;

pub struct Database {
    pub write_pool: PgPool,
    pub read_pool: PgPool,
    write_max_connections: u32,
    read_max_connections: u32,
    has_separate_read_pool: bool,
}

/// Zähler eines Pools.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PoolMetrics {
    /// aktuell bestehende Verbindungen
    pub open: u32,
    /// davon unbenutzt
    pub idle: u32,
    /// konfigurierte Obergrenze
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReplicaStatus {
    pub separate_pool: bool,
    /// Läuft der Lese-Pool auf einem Read-only-Server? `None` = nicht prüfbar.
    pub readonly: Option<bool>,
}

fn pool_options(cfg: &DatabaseConfig, max: u32) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(max)
        .min_connections(cfg.min_connections.min(max))
        .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_seconds))
        .idle_timeout(Duration::from_secs(cfg.idle_timeout_seconds))
}

impl Database {
    /// Verbindet. Mit `run_migrations` werden die eingebetteten
    /// Migrationen über die Laufzeitverbindung angewendet (Entwicklung);
    /// im Betrieb migriert [`Database::migrate`] mit der Eigentümerrolle.
    pub async fn connect(cfg: &DatabaseConfig, run_migrations: bool) -> anyhow::Result<Self> {
        let write_pool = pool_options(cfg, cfg.pool_size)
            .connect(&cfg.write_url)
            .await?;
        let read_max = cfg.read_max_connections.unwrap_or(cfg.pool_size);
        let read_pool = match cfg.read_url.as_deref() {
            Some(url) => {
                info!(max_connections = read_max, "Lese-Pool (Replica) verbunden");
                pool_options(cfg, read_max).connect(url).await?
            }
            None => write_pool.clone(),
        };
        if run_migrations {
            Self::run_migrations(&write_pool).await?;
        }
        Ok(Self {
            write_max_connections: cfg.pool_size,
            read_max_connections: read_max,
            has_separate_read_pool: cfg.read_url.is_some(),
            write_pool,
            read_pool,
        })
    }

    /// Führt alle eingebetteten Migrationen aus. Bereits angewendete
    /// werden über ihre Prüfsumme verifiziert; Abweichungen brechen ab.
    pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
        sqlx::migrate!("../../migrations").run(pool).await?;
        info!("Migrationen angewendet");
        Ok(())
    }

    /// Verbindet kurzzeitig mit `migration_url` (Schema-Eigentümer) und
    /// migriert. Die Laufzeitverbindung braucht danach keine DDL-Rechte.
    pub async fn migrate(migration_url: &str) -> anyhow::Result<()> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .connect(migration_url)
            .await?;
        let res = Self::run_migrations(&pool).await;
        pool.close().await;
        res
    }

    pub fn write(&self) -> &PgPool {
        &self.write_pool
    }

    pub fn read(&self) -> &PgPool {
        &self.read_pool
    }

    /// Pool-Zähler (Schreib-Pool, Lese-Pool).
    pub fn pool_metrics(&self) -> (PoolMetrics, PoolMetrics) {
        let of = |pool: &PgPool, max: u32| PoolMetrics {
            open: pool.size(),
            idle: pool.num_idle() as u32,
            max,
        };
        (
            of(&self.write_pool, self.write_max_connections),
            of(&self.read_pool, self.read_max_connections),
        )
    }

    pub async fn replica_status(&self) -> ReplicaStatus {
        let readonly = match sqlx::query("SELECT current_setting('transaction_read_only') AS v")
            .fetch_one(&self.read_pool)
            .await
        {
            Ok(row) => Some(row.get::<String, _>("v") == "on"),
            Err(e) => {
                warn!(error = %e, "Replica-Status nicht prüfbar");
                None
            }
        };
        ReplicaStatus {
            separate_pool: self.has_separate_read_pool,
            readonly,
        }
    }

    pub async fn close(&self) {
        self.write_pool.close().await;
        self.read_pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lazy_pool_holds_no_connections() {
        let cfg = DatabaseConfig::default();
        let pool = pool_options(&cfg, cfg.pool_size)
            .connect_lazy("postgres://localhost/unbenutzt")
            .unwrap();
        assert_eq!(pool.size(), 0, "ohne Bedarf keine offene Verbindung");
    }
}
