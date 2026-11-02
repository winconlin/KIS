//! Datenbank-Schicht: PostgreSQL für tausende gleichzeitiger User.
//!
//! Architektur:
//! - Write-Pool: bis 500 Connections (konfigurierbar)
//! - Read-Pool:  separater Pool für Lese-Only-Queries (Read-Replica)
//! - Optimistic Locking: `version`-Spalte in allen Tabellen
//! - Migrations: `sqlx::migrate!` bei Start
//!
//! Rechtssicherheit:
//! - Alle Patientendaten nur über den Pool, nie direkter DB-Zugriff
//! - Soft-Delete (deleted_at) für DSGVO-Art. 17
//! - Audit-Log als separate Tabelle, Hash-Chain
//! - Verschlüsselte Spalten (AES-256-GCM) für sensible Daten

use sqlx::postgres::{PgPoolOptions, PgConnectOptions};
use sqlx::Pool;
use std::str::FromStr;
use tracing::{info, warn};

pub struct Database {
    /// Write-Pool: alle Schreib-Operationen
    pub write_pool: Pool<sqlx::Postgres>,
    /// Read-Pool: Lese-Operationen (kann auf Read-Replica zeigen)
    pub read_pool: Pool<sqlx::Postgres>,
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
        let read_pool = if let Some(rurl) = read_url {
            let pool = PgPoolOptions::new()
                .max_connections(max_connections)
                .min_connections(max_connections / 5)
                .acquire_timeout(std::time::Duration::from_secs(10))
                .connect(rurl)
                .await?;

            info!(max_connections = max_connections, "Read-Pool (Replica) verbunden");
            pool
        } else {
            warn!("Kein separater Read-Pool — Lese-Queries nutzen den Write-Pool");
            write_pool.clone()
        };

        // Migrations auf dem Write-Pool
        // Hinweis: Der Pfad ist relativ zur Crate (crates/kis-core) —
        // die Workspace-Migrationen liegen unter /migrations
        sqlx::migrate!("../../migrations")
            .run(&write_pool)
            .await?;

        info!("Migrations erfolgreich ausgeführt");

        Ok(Self {
            write_pool,
            read_pool,
        })
    }

    /// Zugriff auf den Write-Pool
    pub fn write(&self) -> &Pool<sqlx::Postgres> {
        &self.write_pool
    }

    /// Zugriff auf den Read-Pool
    pub fn read(&self) -> &Pool<sqlx::Postgres> {
        &self.read_pool
    }

    /// Erzeugt eine leere Database-Instanz für Unit-Tests (ohne Verbindung).
    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        let write_pool = sqlx::Pool::<sqlx::Postgres>::connect_lazy("postgres://localhost/test")
            .expect("inaktiver Test-Pool");
        Self {
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
}

impl DbConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let write_url = std::env::var("KIS_DB_WRITE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .map_err(|_| anyhow::anyhow!("KIS_DB_WRITE_URL oder DATABASE_URL nicht gesetzt"))?;

        let read_url = std::env::var("KIS_DB_READ_URL").ok();
        let max_connections: u32 = std::env::var("KIS_DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);

        Ok(Self {
            write_url,
            read_url,
            max_connections,
        })
    }
}
