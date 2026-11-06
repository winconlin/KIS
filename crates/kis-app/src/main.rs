//! KIS — Krankenhaus-Informations-System: API-Server.
//!
//! Startreihenfolge: Konfiguration → Logging → Secrets prüfen →
//! PostgreSQL verbinden und migrieren → Router → HTTP/TLS-Server.
//! Jeder Fehler bricht den Start ab (kein stiller Fallback).

use std::sync::Arc;
use std::time::Duration;

use kis_app::{api, graceful_shutdown::GracefulShutdown, tls};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = kis_core::config::AppConfig::load()?;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!("{},sqlx=warn", config.logging.level))
    });
    if config.logging.format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
    info!(version = env!("CARGO_PKG_VERSION"), "KIS startet");

    // Secrets zuerst prüfen: ohne gültige Schlüssel keine DB-Verbindung.
    let security = Arc::new(kis_core::security::Security::new(&config.security)?);

    let migrate_separately = match config.database.migration_url.as_deref() {
        Some(url) => {
            kis_core::db::Database::migrate(url).await?;
            true
        }
        None => {
            warn!("database.migration_url nicht gesetzt — Migrationen über die Laufzeitverbindung (nur Entwicklung)");
            false
        }
    };
    let db = Arc::new(
        kis_core::db::Database::connect_pools_opts(
            &config.database.write_url,
            config.database.read_url.as_deref(),
            config.database.pool_size,
            config.database.read_max_connections,
            !migrate_separately,
        )
        .await?,
    );
    info!("PostgreSQL verbunden");

    let rate_limiter = Arc::new(kis_core::rate_limit::SlidingWindowRateLimiter::new(
        &config.security.rate_limit,
    ));
    let cache = Arc::new(kis_core::cache::EntityCache::new(&config.cache));

    let shutdown = Arc::new(GracefulShutdown::new(
        config.server.shutdown_timeout_seconds,
    ));
    let state = api::AppState::new(config.clone(), db.clone(), security, rate_limiter, cache)?;
    let mut app = api::build_router(state);
    app = kis_app::graceful_shutdown::router_with_shutdown(app, &shutdown);

    let tls_config = tls::build_server_config(&config.security.tls)?;
    if tls_config.is_some() {
        let hsts = axum::http::HeaderValue::from_str(&tls::hsts_header(&config.security.tls))?;
        app = app.layer(axum::middleware::from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                let hsts = hsts.clone();
                async move {
                    let mut res = next.run(req).await;
                    res.headers_mut().insert("Strict-Transport-Security", hsts);
                    res
                }
            },
        ));
    }

    let addr: std::net::SocketAddr =
        format!("{}:{}", config.server.host, config.server.port).parse()?;
    let handle = axum_server::Handle::new();

    // SIGINT/SIGTERM → 503 für neue Requests, dann geordnetes Beenden.
    {
        let handle = handle.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            wait_for_signal().await;
            shutdown.trigger();
            shutdown.notify_axum_shutdown();
            handle.graceful_shutdown(Some(shutdown.timeout()));
        });
    }

    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    match tls_config {
        Some(cfg) => {
            info!(%addr, "KIS lauscht (TLS 1.3)");
            let rustls = axum_server::tls_rustls::RustlsConfig::from_config(cfg);
            axum_server::bind_rustls(addr, rustls)
                .handle(handle)
                .serve(service)
                .await?;
        }
        None => {
            warn!(%addr, "KIS lauscht ohne TLS — nur hinter TLS-Proxy oder lokal betreiben");
            axum_server::bind(addr)
                .handle(handle)
                .serve(service)
                .await?;
        }
    }

    let close = shutdown.close_pools(&db);
    if tokio::time::timeout(Duration::from_secs(10), close)
        .await
        .is_err()
    {
        warn!("DB-Pools konnten nicht rechtzeitig geschlossen werden");
    }
    info!("KIS beendet");
    Ok(())
}

async fn wait_for_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => info!("SIGINT empfangen"),
        _ = term => info!("SIGTERM empfangen"),
    }
}
