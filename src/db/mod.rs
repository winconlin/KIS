use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Connection;
use std::path::Path;
use tracing::{info, error};

pub struct Database {
    pub pool: sqlx::Pool<sqlx::Sqlite>,
}

impl Database {
    pub async fn connect(path: &Path) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .create_if_missing(true);

        let pool = sqlx::SqlitePool::connect_with(options).await?;

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;

        info!("Datenbank verbunden: {:?}", path);

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &sqlx::Pool<sqlx::Sqlite> {
        &self.pool
    }
}
