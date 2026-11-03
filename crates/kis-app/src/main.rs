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
mod tls;

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

    // --- 7. TLS 1.3 vorbereiten (Task 5.1) ---
    let tls_config = tls::build_server_config(&config.security.tls)?;
    let hsts_value = Arc::new(tls::hsts_header(&config.security.tls));

    // --- 8. Server starten ---
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    match &tls_config {
        Some(_) => info!(%addr, hsts = %*hsts_value, "KIS listening (TLS 1.3, rustls)"),
        None => info!(%addr, "KIS listening (HTTP — TLS nicht konfiguriert, nur für Entwicklung!)"),
    }

    // Server ausführen: TLS oder HTTP, beide mit Graceful Shutdown
    if let Some(tls_server_config) = tls_config {
        run_server_tls(listener, app, shutdown, db, tls_server_config, hsts_value).await?;
    } else {
        run_server_http(listener, app, shutdown, db).await?;
    }

    info!("KIS shut down cleanly");
    Ok(())
}

/// HSTS-Middleware: Fügt `Strict-Transport-Security` Header hinzu (Task 5.1).
fn add_hsts_header(app: axum::Router, hsts_value: Arc<String>) -> axum::Router {
    use axum::{extract::Request, middleware::Next, response::Response};

    async fn hsts_middleware(
        hsts_value: Arc<String>,
        mut req: Request,
        next: Next,
    ) -> Result<Response, axum::http::StatusCode> {
        // HSTS-Header in den Request-Extensions für die Antwort
        req.extensions_mut().insert(HstsValue(hsts_value.clone()));
        let mut response = next.run(req).await;
        response
            .headers_mut()
            .insert("Strict-Transport-Security", hsts_value.parse().unwrap());
        Ok(response)
    }

    #[derive(Clone)]
    struct HstsValue(Arc<String>);

    app.layer(axum::middleware::from_fn_with_state(
        hsts_value,
        hsts_middleware,
    ))
}

/// TLS-Server starten (Task 5.1) mit tokio-rustls und Graceful Shutdown.
async fn run_server_tls(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    shutdown: Arc<graceful_shutdown::GracefulShutdown>,
    db: Arc<Database>,
    tls_server_config: Arc<rustls::ServerConfig>,
    hsts_value: Arc<String>,
) -> anyhow::Result<()> {
    let timeout = shutdown.timeout();
    let mut shutdown_rx = shutdown.shutdown_receiver();

    // Signal-Task: SIGTERM/SIGINT → Graceful Shutdown
    let signal_task = tokio::spawn({
        let shutdown_clone = Arc::clone(&shutdown);
        async move {
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
            shutdown_clone.trigger();
            shutdown_clone.notify_axum_shutdown();
        }
    });

    // TLS-Listener: Akzeptiert TCP und führt TLS-Handshake durch
    let acceptor = tokio_rustls::TlsAcceptor::from(tls_server_config);
    let tls_listener = tokio_rustls::TlsServer::new(acceptor, listener);

    // HSTS-Header-Middleware (Task 5.1)
    let app = add_hsts_header(app, hsts_value);

    let serve_future = axum::serve(tls_listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx
            .wait_for(|v| *v)
            .await
            .inspect_err(|e| {
                warn!(error = %e, "watch-Channel geschlossen vor Shutdown");
            });
    });

    let serve_result = tokio::time::timeout(timeout, serve_future).await;
    signal_task.abort();

    match serve_result {
        Ok(Ok(())) => info!("Graceful Shutdown: axum serve() abgeschlossen"),
        Ok(Err(e)) => warn!(error = %e, "Graceful Shutdown: axum serve() mit Fehler beendet"),
        Err(_) => warn!("Graceful Shutdown: Timeout erreicht — erzwinge Beendigung"),
    }

    shutdown.close_pools(&db).await;
    Ok(())
}

/// HTTP-Server starten (nur für Entwicklung) mit Graceful Shutdown.
async fn run_server_http(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    shutdown: Arc<graceful_shutdown::GracefulShutdown>,
    db: Arc<Database>,
) -> anyhow::Result<()> {
    let timeout = shutdown.timeout();
    let mut shutdown_rx = shutdown.shutdown_receiver();

    let signal_task = tokio::spawn({
        let shutdown_clone = Arc::clone(&shutdown);
        async move {
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
            shutdown_clone.trigger();
            shutdown_clone.notify_axum_shutdown();
        }
    });

    let serve_future = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx
            .wait_for(|v| *v)
            .await
            .inspect_err(|e| {
                warn!(error = %e, "watch-Channel geschlossen vor Shutdown");
            });
    });

    let serve_result = tokio::time::timeout(timeout, serve_future).await;
    signal_task.abort();

    match serve_result {
        Ok(Ok(())) => info!("Graceful Shutdown: axum serve() abgeschlossen"),
        Ok(Err(e)) => warn!(error = %e, "Graceful Shutdown: axum serve() mit Fehler beendet"),
        Err(_) => warn!("Graceful Shutdown: Timeout erreicht — erzwinge Beendigung"),
    }

    shutdown.close_pools(&db).await;
    Ok(())
}
