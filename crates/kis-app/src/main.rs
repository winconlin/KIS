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
use kis_core::db::Database;
use tokio::signal;
use tracing::{info, warn};

mod api;
mod graceful_shutdown;

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
    // Task 5.3: Getrennte Pool-Größen, wenn ein separater Read-Pool konfiguriert ist.
    let db = kis_core::db::Database::connect_pools(
        &config.database.write_url,
        config.database.read_url.as_deref(),
        config.database.pool_size,
        config.database.read_max_connections,
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
    // Graceful Shutdown initialisieren (Task 5.12):
    // - Phase 1: Shutdown-Flag → Middleware lehnt neue Requests mit 503 ab
    // - Phase 2: axum serve() wird über watch-Channel beendet, in-flight
    //            Requests werden bis shutdown_timeout_seconds abgewartet
    // - Phase 3: DB-Pools werden geschlossen
    // - Phase 4: Clean Exit
    let shutdown = Arc::new(graceful_shutdown::GracefulShutdown::new(
        config.server.shutdown_timeout_seconds,
    ));
    let state = api::AppState::new(config.clone(), db.clone(), security, rate_limiter, cache);
    let app = api::build_router(state);
    // Shutdown-Middleware auf den gesamten Router legen
    let app = graceful_shutdown::router_with_shutdown(app, &shutdown);

    // --- 7. Server starten (mit Graceful Shutdown) ---
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(%addr, shutdown_timeout_secs = config.server.shutdown_timeout_seconds, "KIS listening");

    // Graceful Shutdown (Task 5.12):
    // 1. SIGTERM/SIGINT → Phase 1: neue Requests → 503 (via Middleware-Flag)
    // 2. axum serve() wird beendet, in-flight Requests abwarten (max timeout)
    // 3. DB-Pools schließen
    // 4. Clean Exit
    run_server_with_graceful_shutdown(listener, app, Arc::clone(&shutdown), &db).await?;

    info!("KIS shut down cleanly");
    Ok(())
}

/// Führt den Server mit strukturiertem Graceful Shutdown aus (Task 5.12).
///
/// Phasen:
/// 1. **SIGTERM/SIGINT** → `shutdown.trigger()` → Middleware antwortet 503
/// 2. **axum serve()** wird beendet → in-flight Requests bis `timeout` abwarten
/// 3. **DB-Pools schließen** → `write_pool.close()`, `read_pool.close()`
/// 4. **Clean Exit**
///
/// Der SIGTERM/SIGINT-Signal-Task löst `shutdown.trigger()` aus (Phase 1)
/// und signalisiert dem axum-Server-Task über den watch-Channel (Phase 2).
/// axum serve() wartet dann intern auf in-flight Requests.
/// Der Timeout begrenzt die Gesamt-Beendigungszeit auf `shutdown_timeout`.
async fn run_server_with_graceful_shutdown(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    shutdown: Arc<graceful_shutdown::GracefulShutdown>,
    db: &Arc<Database>,
) -> anyhow::Result<()> {
    let timeout = shutdown.timeout();

    // Watch-Receiver: axum serve() wartet auf dieses Signal, um aufzuhören,
    // neue Verbindungen anzunehmen. In-flight Requests werden von axum
    // intern abgewartet (serve() gibt erst zurück, wenn alle fertig sind).
    let mut shutdown_rx = shutdown.shutdown_receiver();

    // Signal-Task: Wartet auf SIGTERM/SIGINT, dann:
    // - Phase 1: Flag setzen (Middleware → 503 für neue Requests)
    // - Phase 2: watch-Channel → axum serve() hört auf, neue Tabs anzunehmen
    let signal_task = tokio::spawn({
        let shutdown_clone = Arc::clone(&shutdown);
        async move {
            // SIGINT oder SIGTERM
            let mut term_signal = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "Fehler beim Registrieren des SIGTERM-Handlers");
                    return;
                }
            };
            tokio::select! {
                res = signal::ctrl_c() => {
                    if let Err(e) = res {
                        warn!(error = %e, "Fehler beim SIGINT-Empfang");
                        return;
                    }
                    info!("SIGINT empfangen — Graceful Shutdown gestartet");
                }
                _ = term_signal.recv() => {
                    info!("SIGTERM empfangen — Graceful Shutdown gestartet");
                }
            }

            // Phase 1: Neue Requests ablehnen (503)
            shutdown_clone.trigger();

            // Phase 2: axum serve() benachrichtigen
            shutdown_clone.notify_axum_shutdown();
        }
    });

    // axum serve() mit Graceful Shutdown:
    // Das Graceful-Shutdown-Future löst sich auf, wenn shutdown_rx true wird
    // (gesetzt durch den Signal-Task via notify_axum_shutdown()).
    // axum wartet dann auf in-flight Requests und gibt serve() zurück.
    let serve_future = axum::serve(listener, app).with_graceful_shutdown(async move {
        // Warten auf das watch-Signal vom Signal-Task
        let _ = shutdown_rx
            .wait_for(|v| *v)
            .await
            .inspect_err(|e| {
                warn!(error = %e, "watch-Channel geschlossen vor Shutdown");
            });
    });

    let serve_result = tokio::time::timeout(timeout, serve_future).await;

    // Signal-Task beenden (falls er noch läuft)
    signal_task.abort();

    match serve_result {
        Ok(Ok(())) => {
            info!("Graceful Shutdown: axum serve() abgeschlossen (alle in-flight Requests fertig)");
        }
        Ok(Err(e)) => {
            warn!(error = %e, "Graceful Shutdown: axum serve() mit Fehler beendet");
        }
        Err(_) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "Graceful Shutdown: Timeout erreicht — erzwinge Beendigung"
            );
        }
    }

    // Phase 3: DB-Pools schließen
    shutdown.close_pools(db).await;

    // Phase 4: Clean Exit (main() gibt Ok(()) zurück → Exit-Code 0)
    Ok(())
}
