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

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::security::Security;

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

pub struct App {
    pub router: axum::Router,
    pub security: Arc<Security>,
    pub state: kis_app::state::AppState,
    pub tdb: TestDb,
}

impl App {
    pub async fn new() -> Option<Self> {
        Self::with_config(config()).await
    }

    /// Anwendung mit angepasster Konfiguration (z. B. abgeschaltete Module).
    pub async fn with_config(cfg: AppConfig) -> Option<Self> {
        let tdb = TestDb::new().await?;
        let security = Arc::new(Security::new(&cfg.security).unwrap());
        let built = kis_app::build_app(cfg, tdb.db.clone(), security.clone())
            .await
            .expect("Anwendung baut");
        Some(Self {
            router: built.router,
            state: built.state,
            security,
            tdb,
        })
    }

    pub fn pool(&self) -> &sqlx::PgPool {
        self.tdb.db.write()
    }

    /// Legt einen Benutzer mit einer Rolle der Grundausstattung an.
    pub async fn user(&self, name: &str, role: &str) -> Uuid {
        kis_core::auth::create_user(
            self.pool(),
            &self.security,
            &self.state.ctx.platform,
            kis_core::auth::NewUser {
                username: name,
                email: &format!("{name}@example.test"),
                display_name: name,
                role,
                password: PASSWORD,
            },
            "test",
        )
        .await
        .unwrap()
    }

    /// Legt einen Benutzer an und meldet ihn an; liefert (ID, Access-Token).
    pub async fn session(&self, name: &str, role: &str) -> (Uuid, String) {
        let id = self.user(name, role).await;
        (id, self.token(name).await.0)
    }

    /// Ordnet einen Benutzer einer Organisationseinheit zu.
    pub async fn assign(&self, user_id: Uuid, unit_id: Uuid) {
        sqlx::query(
            "INSERT INTO user_org_units (user_id, unit_id, assigned_by) VALUES ($1, $2, $1)",
        )
        .bind(user_id)
        .bind(unit_id)
        .execute(self.pool())
        .await
        .unwrap();
    }

    pub async fn raw(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    pub async fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, text) = self.raw(method, path, token, body).await;
        let v = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, v)
    }

    /// Aufruf, der gelingen muss; liefert den Antwortkörper.
    pub async fn ok(&self, method: &str, path: &str, token: &str, body: Option<Value>) -> Value {
        let (status, v) = self.call(method, path, Some(token), body).await;
        assert_eq!(status, StatusCode::OK, "{method} {path}: {v}");
        v
    }

    /// Aufruf, der mit dem erwarteten Status scheitern muss.
    pub async fn fails(
        &self,
        expected: StatusCode,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
    ) -> Value {
        let (status, v) = self.call(method, path, Some(token), body).await;
        assert_eq!(status, expected, "{method} {path}: {v}");
        v
    }

    pub async fn login(&self, login: &str, password: &str) -> (StatusCode, Value) {
        self.call(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": login, "password": password })),
        )
        .await
    }

    pub async fn token(&self, login: &str) -> (String, String) {
        let (s, v) = self.login(login, PASSWORD).await;
        assert_eq!(s, StatusCode::OK, "Login {login}: {v}");
        (
            v["access_token"].as_str().unwrap().to_string(),
            v["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    pub async fn audit_ok(&self) -> bool {
        self.security
            .verify_audit_chain(self.pool())
            .await
            .unwrap()
            .0
    }

    pub async fn finish(self) {
        assert!(self.audit_ok().await, "Audit-Kette intakt");
        self.tdb.drop().await;
    }
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
