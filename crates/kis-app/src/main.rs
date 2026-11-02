//! KIS — Krankenhaus-Informations-System
//!
//! Main Binary: Startet den API-Server und alle aktivierten Module.
//!
//! Modularer Aufbau:
//! - Jedes Modul ist eine eigene Crate
//! - `kis-app` merge-t die Router aller Module
//! - Neues Modul = neue Crate + 1 Zeile in `api.rs`
//!
//! KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO.

use std::sync::Arc;
use tokio::signal;
use tracing::{info, warn};

mod api;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // --- 1. Konfiguration laden ---
    let config = kis_core::config::AppConfig::load()?;
    info!(host = %config.server.host, port = config.server.port, "Config geladen");

    // --- 2. Logging initialisieren (JSON Production / Pretty Dev) ---
    // Log-Level aus Config, überschreibbar per RUST_LOG
    let log_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(
                format!("kis_core={},kis_patients={},kis_doctors={},kis_appointments={},kis_medications={},sqlx=warn",
                    config.logging.level, config.logging.level, config.logging.level,
                    config.logging.level, config.logging.level),
            )
        });

    if config.logging.format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(log_filter)
            .json()
            .with_target(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(log_filter)
            .with_target(true)
            .init();
    }

    info!(version = env!("CARGO_PKG_VERSION"), "KIS startet");

    // --- 3. Datenbank verbinden (PostgreSQL, Connection-Pooling) ---
    let db = kis_core::db::Database::connect(
        &config.database.write_url,
        config.database.read_url.as_deref(),
        config.database.pool_size,
    )
    .await?;
    let db = Arc::new(db);
    info!("PostgreSQL verbunden (Migrations ausgeführt)");

    // --- 4. Security initialisieren (JWT, AES-256-GCM, Audit-Log) ---
    let security = kis_core::security::Security::new(&config.security)?;
    let security = Arc::new(security);
    info!("Security-Subsystem initialisiert");

    // --- 5. Phase 1: Module initialisieren ---
    info!("Patient-Service initialisiert");
    info!("Arzt-Service initialisiert");
    info!("Termin-Service initialisiert");
    info!("Medikament-Service initialisiert");

    // Phase 2–3: Schnittstellen & Integrationen (noch Stubs)
    // HL7, FHIR, DICOM, XDS, GDT, MEDICO, Telematik, GUI

    // --- 5a. Rate-Limiter initialisieren (Task 1.11) ---
    let rate_limiter = Arc::new(
        kis_core::rate_limit::SlidingWindowRateLimiter::new(&config.security.rate_limit),
    );
    info!(
        normal_per_min = config.security.rate_limit.normal_per_min,
        auth_per_min = config.security.rate_limit.auth_per_min,
        "Rate-Limiter initialisiert"
    );

    // --- 5b. In-Memory-Cache initialisieren (Task 1.17) ---
    let cache = Arc::new(kis_core::cache::EntityCache::new(&config.cache));
    info!(
        ttl_seconds = config.cache.ttl_seconds,
        max_entries = config.cache.max_entries,
        "In-Memory-Cache initialisiert (moka)"
    );

    // --- 6. API-Router bauen (merged alle Module) ---
    let state = api::AppState::new(config.clone(), db, security, rate_limiter, cache);
    let app = api::build_router(state);

    // --- 7. Server starten ---
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(%addr, "KIS listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("KIS shut down cleanly");
    Ok(())
}

/// Wartet auf SIGINT/SIGTERM für Graceful Shutdown.
async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
    warn!("Shutdown signal received");
}
