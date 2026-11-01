//! KIS — Krankenhaus-Informations-System
//!
//! Main Binary: Startet den API-Server, die GUI und alle aktivierten Module.
//!
//! Modularer Aufbau:
//! - Jedes Modul ist eine eigene Crate
//! - Feature-Flags bestimmen, welche Module geladen werden
//! - Neues Modul = neue Crate + 1 Zeile hier

use std::sync::Arc;
use tokio::signal;
use tracing::{info, error};

mod config;
mod db;
mod api;
mod security;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging initialisieren
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kis=debug,sqlx=warn".into()),
        )
        .init();

    info!("KIS starting up...");

    // Konfiguration laden
    let config = config::AppConfig::load()?;
    info!("Config geladen: {}", config.server.host);

    // Datenbank verbinden (PostgreSQL, Connection-Pooling)
    let db = db::Database::connect(
        &config.database.write_url,
        config.database.read_url.as_deref(),
        config.database.pool_size,
    ).await?;
    let db = Arc::new(db);

    // Security initialisieren (JWT, AES-256-GCM, Audit-Log)
    let security = security::Security::new(&config.security)?;
    let security = Arc::new(security);

    // API-Router bauen
    let app = api::build_router(config, db, security);

    // Server starten
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("KIS listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("KIS shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
    info!("Shutdown signal received");
}
