//! kis-admin — lokale Verwaltungswerkzeuge.
//!
//! ```text
//! kis-admin migrate
//!     Wendet Migrationen an (über database.migration_url, sonst write_url)
//!     und legt die Grundausstattung der Rollen an.
//! kis-admin create-user <username> <email> <display-name> <rolle>
//!     Passwort aus KIS_ADMIN_PASSWORD oder von stdin (eine Zeile).
//!     <rolle> ist der Schlüssel einer vorhandenen Rolle (siehe `roles`).
//! kis-admin roles
//!     Listet die vorhandenen Rollen.
//! kis-admin modules
//!     Listet die Module dieser Programmversion.
//! kis-admin verify-audit
//!     Prüft die Audit-Hash-Kette; Exit-Code 1 bei Bruch.
//! ```
//!
//! Nutzt dieselbe Konfiguration und dieselben Secrets wie der Server.

use std::io::{BufRead, Write};

use kis_core::auth::NewUser;
use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::platform::Platform;
use kis_core::security::Security;

/// Schreibt eine Zeile nach stdout. Ist die Ausgabe geschlossen (z. B.
/// `kis-admin roles | head`), endet das Werkzeug still statt mit Panik.
fn out(line: impl std::fmt::Display) {
    if writeln!(std::io::stdout(), "{line}").is_err() {
        std::process::exit(0);
    }
}

fn usage() -> ! {
    eprintln!(
        "Verwendung:\n  kis-admin migrate\n  kis-admin create-user <username> <email> <display-name> <rolle>\n  kis-admin roles\n  kis-admin modules\n  kis-admin verify-audit"
    );
    std::process::exit(2)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command: Vec<&str> = args.iter().map(String::as_str).collect();

    if command == ["modules"] {
        for m in kis_app::modules::module_infos() {
            out(format_args!(
                "{:<14} {:<8} {}{}",
                m.id,
                m.version,
                m.title,
                if m.kernel { " (Kern)" } else { "" }
            ));
        }
        return Ok(());
    }

    let config = AppConfig::load()?;
    match command.as_slice() {
        ["migrate"] => {
            // Braucht nur die Migrationsverbindung (Eigentümerrolle): Die
            // Laufzeitverbindung ist beim ersten Einrichten oft noch nicht
            // erreichbar oder ihr Passwort nicht in der Umgebung.
            let url = config
                .database
                .migration_url
                .clone()
                .unwrap_or_else(|| config.database.write_url.clone());
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(std::time::Duration::from_secs(10))
                .connect(&url)
                .await?;
            Database::run_migrations(&pool).await?;
            let mut tx = pool.begin().await?;
            kis_core::rbac::seed_builtin_roles(&mut tx).await?;
            tx.commit().await?;
            pool.close().await;
            out("Migrationen angewendet, Rollen-Grundausstattung vorhanden");
        }
        ["create-user", username, email, display_name, role] => {
            let security = Security::new(&config.security)?;
            let password = match std::env::var("KIS_ADMIN_PASSWORD") {
                Ok(p) => p,
                Err(_) => {
                    eprint!("Passwort: ");
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line.trim_end_matches(['\r', '\n']).to_string()
                }
            };
            let db = connect(&config).await?;
            seed_roles(&db).await?;
            let platform = Platform::load(db.write(), &config.security).await?;
            let id = kis_core::auth::create_user(
                db.write(),
                &security,
                &platform,
                NewUser {
                    username,
                    email,
                    display_name,
                    role,
                    password: &password,
                },
                "kis-admin",
            )
            .await
            .map_err(|e| match e {
                kis_core::auth::AuthError::Internal(inner) => inner,
                other => anyhow::anyhow!(other.to_string()),
            })?;
            out(format_args!("Benutzer angelegt: {id}"));
        }
        ["roles"] => {
            let db = connect(&config).await?;
            let rows: Vec<(String, String, i64)> = sqlx::query_as(
                "SELECT r.key, r.label,
                        (SELECT count(*) FROM role_permissions p WHERE p.role_key = r.key)
                 FROM roles r ORDER BY r.key",
            )
            .fetch_all(db.write())
            .await?;
            for (key, label, permissions) in rows {
                out(format_args!("{key:<16} {permissions:>3} Rechte  {label}"));
            }
        }
        ["verify-audit"] => {
            let security = Security::new(&config.security)?;
            let db = connect(&config).await?;
            let (ok, index) = security.verify_audit_chain(db.write()).await?;
            if ok {
                out("Audit-Kette intakt");
            } else {
                out(format_args!(
                    "Audit-Kette GEBROCHEN bei Index {}",
                    index.unwrap_or_default()
                ));
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
    Ok(())
}

async fn connect(config: &AppConfig) -> anyhow::Result<Database> {
    let mut cfg = config.database.clone();
    cfg.pool_size = cfg.pool_size.clamp(1, 2);
    cfg.read_url = None;
    Database::connect(&cfg, config.database.migration_url.is_none()).await
}

async fn seed_roles(db: &Database) -> anyhow::Result<()> {
    let mut tx = db.write().begin().await?;
    kis_core::rbac::seed_builtin_roles(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}
