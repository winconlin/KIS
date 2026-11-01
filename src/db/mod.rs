//! Datenbank-Schicht: SQLite für den Prototyp.
//!
//! - `sqlx::migrate!` führt alle Migrations bei Start aus
//! - Single-Pool (SQLite ist file-based, kein Read-Replica nötig)
//! - Audit-Log als separate Tabelle mit Hash-Chain

use sqlx::sqlite::SqlitePoolOptions;
use sqlx::Pool;
use tracing::{info, warn};

pub struct Database {
    /// Haupt-Pool für alle Lese- und Schreib-Operationen
    pub pool: Pool<sqlx::Sqlite>,
}

impl Database {
    /// Verbindet mit SQLite und führt Migrations aus.
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;

        info!(url = %redact_url(url), "Datenbank verbunden");

        // Migrations ausführen
        match sqlx::migrate!("./migrations").run(&pool).await {
            Ok(_) => info!("Migrations erfolgreich ausgeführt"),
            Err(e) => warn!(error = %e, "Migrations fehlerhaft — bestehende DB?"),
        }

        Ok(Self { pool })
    }

    /// Zugriff auf den Pool
    pub fn pool(&self) -> &Pool<sqlx::Sqlite> {
        &self.pool
    }

    /// Erzeugt eine leere Database-Instanz für Unit-Tests (ohne Verbindung).
    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        let pool = sqlx::Pool::<sqlx::Sqlite>::connect_lazy(":memory:")
            .expect("inaktiver Test-Pool");
        Self { pool }
    }
}

/// Maskiert den Pfad in der URL für Logging
fn redact_url(url: &str) -> String {
    if url.starts_with("sqlite://") {
        url.replace("sqlite://", "sqlite://***")
    } else {
        "***".into()
    }
}
