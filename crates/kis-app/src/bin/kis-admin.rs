//! kis-admin — lokale Verwaltungswerkzeuge.
//!
//! ```text
//! kis-admin create-user <username> <email> <display-name> <role>
//!     Passwort aus KIS_ADMIN_PASSWORD oder interaktiv von stdin (eine Zeile).
//! kis-admin verify-audit
//!     Prüft die Audit-Hash-Kette; Exit-Code 1 bei Bruch.
//! kis-admin migrate
//!     Wendet Migrationen an (über database.migration_url, sonst write_url).
//! ```
//!
//! Nutzt dieselbe Konfiguration und dieselben Secrets wie der Server.

use std::io::BufRead;
use std::str::FromStr;

use kis_core::security::{Role, Security};

fn usage() -> ! {
    eprintln!(
        "Verwendung:\n  kis-admin create-user <username> <email> <display-name> <role>\n  kis-admin verify-audit\n  kis-admin migrate\n\nRollen: admin, arzt, pflege, rezeption, viewer, auditor, llm_agent"
    );
    std::process::exit(2)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = kis_core::config::AppConfig::load()?;
    if args.first().map(String::as_str) == Some("migrate") && args.len() == 1 {
        let url = config
            .database
            .migration_url
            .clone()
            .unwrap_or_else(|| config.database.write_url.clone());
        kis_core::db::Database::migrate(&url).await?;
        println!("Migrationen angewendet");
        return Ok(());
    }
    let security = Security::new(&config.security)?;

    match args.first().map(String::as_str) {
        Some("create-user") if args.len() == 5 => {
            let role = Role::from_str(&args[4]).map_err(|e| anyhow::anyhow!(e))?;
            let password = match std::env::var("KIS_ADMIN_PASSWORD") {
                Ok(p) => p,
                Err(_) => {
                    eprint!("Passwort (min. 12 Zeichen): ");
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line.trim_end_matches(['\r', '\n']).to_string()
                }
            };
            let db = connect(&config).await?;
            let id = kis_core::auth::create_user(
                db.write(),
                &security,
                &args[1],
                &args[2],
                &args[3],
                role,
                &password,
                "kis-admin",
            )
            .await
            .map_err(|e| match e {
                kis_core::auth::AuthError::Internal(inner) => inner,
                other => anyhow::anyhow!(other.to_string()),
            })?;
            println!("Benutzer angelegt: {id}");
        }
        Some("verify-audit") if args.len() == 1 => {
            let db = connect(&config).await?;
            let (ok, index) = security.verify_audit_chain(db.write()).await?;
            if ok {
                println!("Audit-Kette intakt");
            } else {
                println!(
                    "Audit-Kette GEBROCHEN bei Index {}",
                    index.unwrap_or_default()
                );
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
    Ok(())
}

async fn connect(config: &kis_core::config::AppConfig) -> anyhow::Result<kis_core::db::Database> {
    kis_core::db::Database::connect_pools_opts(
        &config.database.write_url,
        None,
        config.database.pool_size.min(4),
        None,
        config.database.migration_url.is_none(),
    )
    .await
}
