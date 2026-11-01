use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("KIS — Krankenhaus-Informations-System v0.1.0");
    tracing::info!("Starte System...");

    // TODO: Load config
    // TODO: Init database
    // TODO: Start HTTP server
    // TODO: Start GUI

    tracing::info!("KIS läuft");
    Ok(())
}
