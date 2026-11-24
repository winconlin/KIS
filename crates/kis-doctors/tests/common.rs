//! Gemeinsames Testgerüst: echter Router gegen eine echte, frische
//! PostgreSQL-Datenbank je Test.
//!
//! Voraussetzung: `KIS_TEST_DATABASE_URL` zeigt auf einen PostgreSQL-Server,
//! auf dem der Benutzer Datenbanken anlegen darf, z. B.
//! `postgres://postgres@127.0.0.1:5432/postgres`. Ohne die Variable werden
//! die Tests mit deutlichem Hinweis übersprungen; mit
//! `KIS_REQUIRE_PG_TESTS=1` (CI, `scripts/check.sh`) schlagen sie
//! stattdessen fehl. Übersprungene Tests gelten nicht als bestanden.
//!
//! Alle Testdaten sind synthetisch.
#![allow(dead_code)]

use std::sync::Arc;

use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::config::AppConfig;
use kis_core::db::Database;

pub const JWT_SECRET: &str = "integration-test-secret-0123456789abcdef";
// 32 Bytes, Base64 (synthetischer Testschlüssel)
pub const MASTER_KEY: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";
pub const PASSWORD: &str = "correct-horse-battery";

pub struct TestDb {
    admin_url: String,
    name: String,
    pub db: Arc<Database>,
}

impl TestDb {
    pub async fn new() -> Option<Self> {
        let admin_url = match std::env::var("KIS_TEST_DATABASE_URL") {
            Ok(u) => u,
            Err(_) => {
                if std::env::var("KIS_REQUIRE_PG_TESTS").as_deref() == Ok("1") {
                    panic!("KIS_TEST_DATABASE_URL fehlt, PostgreSQL-Tests sind verpflichtend");
                }
                eprintln!("ÜBERSPRUNGEN: KIS_TEST_DATABASE_URL nicht gesetzt");
                return None;
            }
        };
        let name = format!("kis_it_{}", Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("Admin-Verbindung");
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .expect("CREATE DATABASE");
        admin.close().await;
        let cfg = kis_core::config::DatabaseConfig {
            write_url: replace_db(&admin_url, &name),
            pool_size: 8,
            ..Default::default()
        };
        let db = Database::connect(&cfg, true)
            .await
            .expect("Verbindung + Migrationen");
        Some(Self {
            admin_url,
            name,
            db: Arc::new(db),
        })
    }

    /// Verbindungs-URL dieser Testdatenbank.
    pub fn url(&self) -> String {
        replace_db(&self.admin_url, &self.name)
    }

    pub async fn drop(self) {
        self.db.close().await;
        if let Ok(admin) = sqlx::PgPool::connect(&self.admin_url).await {
            let dropped = sqlx::query(&format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                self.name
            ))
            .execute(&admin)
            .await;
            if let Err(e) = dropped {
                eprintln!("Testdatenbank {} nicht entfernt: {e}", self.name);
            }
        }
    }
}

pub fn replace_db(url: &str, name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let idx = base.rfind('/').expect("URL mit Datenbankpfad");
    let mut out = format!("{}/{}", &base[..idx], name);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    out
}

pub fn config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.security.jwt_secret_env = "KIS_IT_JWT_SECRET".into();
    cfg.security.encryption.master_key_env = "KIS_IT_MASTER_KEY".into();
    cfg.security.max_login_attempts = 3;
    cfg.security.lockout_duration = 600;
    // Hohe Limits: Tests prüfen Auth, nicht das Rate-Limit
    cfg.security.rate_limit.normal_per_min = 100_000;
    cfg.security.rate_limit.auth_per_min = 100_000;
    std::env::set_var("KIS_IT_JWT_SECRET", JWT_SECRET);
    std::env::set_var("KIS_IT_MASTER_KEY", MASTER_KEY);
    cfg
}

#[macro_export]
macro_rules! app {
    () => {
        match common::App::new().await {
            Some(a) => a,
            None => return,
        }
    };
    ($cfg:expr) => {
        match common::App::with_config($cfg).await {
            Some(a) => a,
            None => return,
        }
    };
}

pub fn patient_body() -> Value {
    json!({
        "nameFamily": "Testfrau",
        "nameGiven": "Erika",
        "birthDate": "1970-05-01",
        "addressZip": "83022",
        "allergies": ["Penizillin"]
    })
}

pub fn id_of(v: &Value) -> Uuid {
    Uuid::parse_str(
        v["id"]
            .as_str()
            .unwrap_or_else(|| panic!("id fehlt in {v}")),
    )
    .unwrap()
}
