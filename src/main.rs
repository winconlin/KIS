use axum::{routing::get, Router};
use kis::{api::create_router, config::AppConfig, db::Database, security};
use std::{net::SocketAddr, sync::Arc};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let config = AppConfig::load()?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(config.server.workers)
        .enable_all()
        .build()?
        .block_on(run(config))
}
async fn run(config: AppConfig) -> anyhow::Result<()> {
    let db = Arc::new(Database::from_config(&config.database).await?);
    let security_stack = Arc::new(security::initialize(&config.security, db).await?);
    // Der API-Router enthält sein Präfix bereits. Kein zweites /api-Nesting.
    let app = Router::new()
        .route("/", get(|| async { "KIS — Entwicklungsprototyp" }))
        .merge(create_router(security_stack))
        .into_make_service_with_connect_info::<SocketAddr>();
    // HTTP; für Netzbetrieb ist ein TLS-Terminator erforderlich.
    let listener =
        tokio::net::TcpListener::bind((config.server.host.as_str(), config.server.port)).await?;
    tracing::info!(address = %listener.local_addr()?, "KIS lauscht (HTTP)");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
