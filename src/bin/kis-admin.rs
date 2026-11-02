//! Lokaler Bootstrap. Das Passwort wird ausschließlich über stdin eingelesen.
use kis::{config::AppConfig, db::Database, security::AuthContext};
use std::io;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        arguments.len() == 2,
        "Aufruf: kis-admin BENUTZERNAME EMAIL (Passwort über stdin)"
    );
    let username = arguments[0].trim();
    anyhow::ensure!(
        !username.is_empty() && username.len() <= 100,
        "Ungültiger Benutzername"
    );
    anyhow::ensure!(
        kis::security::validation::validate_email(&arguments[1]),
        "Ungültige E-Mail-Adresse"
    );
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let password = input.trim_end_matches(['\r', '\n']);
    anyhow::ensure!(
        password.chars().count() >= 12,
        "Passwort benötigt mindestens 12 Zeichen"
    );
    let password_hash = AuthContext::hash_password(password)?;
    let config = AppConfig::load()?;
    let db = Database::from_config(&config.database).await?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT INTO users (id, username, password_hash, display_name, email, role, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, 'admin', ?, ?)")
        .bind(uuid::Uuid::new_v4().to_string()).bind(username).bind(password_hash).bind(username)
        .bind(&arguments[1]).bind(&now).bind(&now).execute(db.pool()).await?;
    eprintln!("Administrator angelegt. Vorhandene Konten wurden nicht verändert.");
    Ok(())
}
