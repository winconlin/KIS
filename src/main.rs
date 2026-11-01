use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    routing::get,
    Router,
};
use tracing_subscriber::EnvFilter;

use kis::api::create_router;
use kis::config::AppConfig;
use kis::db::Database;
use kis::security;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging initialisieren (RUST_LOG Umgebungsvariable, Default: info)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    tracing::info!("KIS — Krankenhaus-Informations-System startet");

    // 1) Konfiguration laden
    let config = AppConfig::load()?;
    tracing::info!(
        host = %config.server.host,
        port = config.server.port,
        "Konfiguration geladen"
    );

    // 2) Datenbank verbinden (SQLite, Migrations anwenden)
    let db_url = format!("sqlite://{}", config.database.path);
    let db = Arc::new(Database::connect(&db_url).await?);
    tracing::info!(path = %config.database.path, "Datenbank verbunden");

    // 3) Sicherheits-Stack initialisieren (Krypto, Audit, Auth, Rate-Limit)
    let security_stack = Arc::new(security::initialize(&config.security, db.clone()).await?);
    tracing::info!("Sicherheits-Stack initialisiert");

    // 4) Router mit Sicherheits-Middleware aufbauen
    let api_router = create_router(security_stack);

    // 5) Health-Check auf Root + API-Router mounten
    let app = Router::new()
        .route("/", get(|| async { "KIS" }))
        .nest("/api", api_router)
        // ConnectInfo ist nötig für IP-Extraktion (Rate-Limit + Audit)
        .into_make_service_with_connect_info::<SocketAddr>();

    // 6) Server starten (TLS: Zertifikat aus ENV laden, falls vorhanden)
    let bind_addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "Server lauscht");

    axum::serve(listener, app).await?;
    Ok(())
}
