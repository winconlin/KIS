//! SQLite-Pool mit verbindlichen Migrationen. PostgreSQL bleibt ein Ausbauziel.
use crate::config::DatabaseConfig;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};
use std::{str::FromStr, time::Duration};

pub struct Database {
    pub pool: Pool<Sqlite>,
}

impl Database {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        Self::connect_options(
            SqliteConnectOptions::from_str(url)?.create_if_missing(true),
            5,
        )
        .await
    }
    pub async fn from_config(config: &DatabaseConfig) -> anyhow::Result<Self> {
        if let Some(parent) = std::path::Path::new(&config.path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let journal = match config.journal_mode.as_str() {
            "wal" => SqliteJournalMode::Wal,
            "delete" => SqliteJournalMode::Delete,
            _ => anyhow::bail!("database.journal_mode muss wal oder delete sein"),
        };
        let options = SqliteConnectOptions::new()
            .filename(&config.path)
            .create_if_missing(true)
            .journal_mode(journal);
        Self::connect_options(options, config.pool_size).await
    }
    async fn connect_options(
        options: SqliteConnectOptions,
        pool_size: u32,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(pool_size > 0, "database.pool_size muss größer als 0 sein");
        let pool = SqlitePoolOptions::new()
            .max_connections(pool_size)
            .connect_with(
                options
                    .foreign_keys(true)
                    .synchronous(SqliteSynchronous::Full)
                    .busy_timeout(Duration::from_secs(5)),
            )
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        Self {
            pool: Pool::<Sqlite>::connect_lazy("sqlite::memory:").expect("Test-Pool"),
        }
    }
}
