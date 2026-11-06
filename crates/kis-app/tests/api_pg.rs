//! Integrationstests gegen eine echte PostgreSQL-Datenbank.
//!
//! Voraussetzung: `KIS_TEST_DATABASE_URL` zeigt auf einen PostgreSQL-Server,
//! auf dem der Benutzer Datenbanken anlegen darf, z. B.
//! `postgres://postgres@127.0.0.1:5432/postgres`. Jeder Test legt eine
//! eigene, frische Datenbank an und entfernt sie danach.
//!
//! Ohne die Variable werden die Tests mit deutlichem Hinweis übersprungen;
//! mit `KIS_REQUIRE_PG_TESTS=1` (CI) schlagen sie stattdessen fehl.
//! Übersprungene Tests gelten nicht als bestanden.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use kis_app::api::{build_router, AppState};
use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::security::{Role, Security};

const JWT_SECRET: &str = "integration-test-secret-0123456789abcdef";
// 32 Bytes, Base64 (synthetischer Testschlüssel)
const MASTER_KEY: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";
const PASSWORD: &str = "correct-horse-battery";

struct TestDb {
    admin_url: String,
    name: String,
    db: Arc<Database>,
}

impl TestDb {
    async fn new() -> Option<Self> {
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
        let name = format!("kis_it_{}", uuid::Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("Admin-Verbindung");
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .expect("CREATE DATABASE");
        admin.close().await;
        let url = replace_db(&admin_url, &name);
        let db = Database::connect_pools(&url, None, 8, None)
            .await
            .expect("Verbindung + Migrationen");
        Some(Self {
            admin_url,
            name,
            db: Arc::new(db),
        })
    }

    async fn drop(self) {
        self.db.write_pool.close().await;
        if let Ok(admin) = sqlx::PgPool::connect(&self.admin_url).await {
            let _ = sqlx::query(&format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                self.name
            ))
            .execute(&admin)
            .await;
        }
    }
}

fn replace_db(url: &str, name: &str) -> String {
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

fn config() -> AppConfig {
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

struct App {
    router: axum::Router,
    security: Arc<Security>,
    tdb: TestDb,
}

impl App {
    async fn new() -> Option<Self> {
        let tdb = TestDb::new().await?;
        let cfg = config();
        let security = Arc::new(Security::new(&cfg.security).unwrap());
        let rl = Arc::new(kis_core::rate_limit::SlidingWindowRateLimiter::new(
            &cfg.security.rate_limit,
        ));
        let cache = Arc::new(kis_core::cache::EntityCache::new(&cfg.cache));
        let state = AppState::new(cfg, tdb.db.clone(), security.clone(), rl, cache).unwrap();
        Some(Self {
            router: build_router(state),
            security,
            tdb,
        })
    }

    fn pool(&self) -> &sqlx::PgPool {
        self.tdb.db.write()
    }

    async fn user(&self, name: &str, role: Role) -> uuid::Uuid {
        kis_core::auth::create_user(
            self.pool(),
            &self.security,
            name,
            &format!("{name}@example.test"),
            name,
            role,
            PASSWORD,
            "test",
        )
        .await
        .unwrap()
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
        let v = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, v)
    }

    async fn login(&self, login: &str, password: &str) -> (StatusCode, Value) {
        self.call(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": login, "password": password })),
        )
        .await
    }

    async fn token(&self, login: &str) -> (String, String) {
        let (s, v) = self.login(login, PASSWORD).await;
        assert_eq!(s, StatusCode::OK, "Login {login}: {v}");
        (
            v["access_token"].as_str().unwrap().to_string(),
            v["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    async fn audit_ok(&self) -> bool {
        self.security
            .verify_audit_chain(self.pool())
            .await
            .unwrap()
            .0
    }
}

macro_rules! app {
    () => {
        match App::new().await {
            Some(a) => a,
            None => return,
        }
    };
}

fn patient_body() -> Value {
    json!({
        "nameFamily": "Testfrau",
        "nameGiven": "Erika",
        "birthDate": "1970-05-01",
        "addressZip": "83022",
        "allergies": ["Penizillin"]
    })
}

#[tokio::test]
async fn authentication_and_session_lifecycle() {
    let app = app!();
    app.user("admin1", Role::Admin).await;

    // Ohne Token kein Zugriff
    let (s, _) = app.call("GET", "/api/v1/patients", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Falsches Passwort und unbekannter Benutzer: identische Antwort
    let (s1, v1) = app.login("admin1", "wrong-password-xyz").await;
    let (s2, v2) = app.login("nobody", "wrong-password-xyz").await;
    assert_eq!(s1, StatusCode::UNAUTHORIZED);
    assert_eq!((s1, v1["error"].clone()), (s2, v2["error"].clone()));

    let (access, refresh) = app.token("admin1").await;
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access), None)
        .await;
    assert_eq!(s, StatusCode::OK);

    // Refresh-Token ist kein Access-Token
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&refresh), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Rotation: neues Paar gültig, altes Access und alter Refresh ungültig
    let (s, v) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let access2 = v["access_token"].as_str().unwrap().to_string();
    let refresh2 = v["refresh_token"].as_str().unwrap().to_string();
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access), None)
        .await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "altes Access-Token nach Rotation"
    );
    let (s, _) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh })),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "Wiederverwendung des alten Refresh-Tokens"
    );

    // Parallele Rotation desselben Refresh-Tokens: genau eine gewinnt
    let mut handles = Vec::new();
    for _ in 0..8 {
        let router = app.router.clone();
        let r = refresh2.clone();
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/auth/refresh")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "refresh_token": r }).to_string()))
                .unwrap();
            router.oneshot(req).await.unwrap().status()
        }));
    }
    let mut ok = 0;
    for h in handles {
        if h.await.unwrap() == StatusCode::OK {
            ok += 1;
        }
    }
    assert_eq!(ok, 1, "genau eine parallele Rotation darf gelingen");
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access2), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Logout widerruft die Sitzung sofort
    let (access3, refresh3) = app.token("admin1").await;
    let (s, _) = app
        .call("POST", "/api/v1/auth/logout", Some(&access3), None)
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access3), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh3 })),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Manipuliertes Token
    let (access4, _) = app.token("admin1").await;
    let mut forged = access4.clone();
    forged.pop();
    forged.push(if access4.ends_with('A') { 'B' } else { 'A' });
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&forged), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    assert!(app.audit_ok().await);
    app.tdb.drop().await;
}

#[tokio::test]
async fn lockout_and_account_status() {
    let app = app!();
    let uid = app.user("arzt1", Role::Arzt).await;

    for _ in 0..3 {
        let (s, _) = app.login("arzt1", "wrong-password-xyz").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    // Gesperrt: auch das richtige Passwort wird abgelehnt
    let (s, _) = app.login("arzt1", PASSWORD).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    sqlx::query("UPDATE users SET locked_until = NULL WHERE id = $1")
        .bind(uid)
        .execute(app.pool())
        .await
        .unwrap();
    let (access, _) = app.token("arzt1").await;
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access), None)
        .await;
    assert_eq!(s, StatusCode::OK);

    // Rollenänderung wirkt sofort (Rolle aus der DB, nicht aus dem Token)
    sqlx::query("UPDATE users SET role = 'auditor' WHERE id = $1")
        .bind(uid)
        .execute(app.pool())
        .await
        .unwrap();
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access), None)
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Deaktiviertes Konto: Token sofort ungültig
    sqlx::query("UPDATE users SET is_active = FALSE WHERE id = $1")
        .bind(uid)
        .execute(app.pool())
        .await
        .unwrap();
    let (s, _) = app
        .call("GET", "/api/v1/audit/patient/x", Some(&access), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Inaktivität > 30 min: Auto-Logout
    app.user("pflege1", Role::Pflege).await;
    let (access, refresh) = app.token("pflege1").await;
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '31 minutes', issued_at = now() - interval '31 minutes'")
        .execute(app.pool())
        .await
        .unwrap();
    let (s, _) = app
        .call("GET", "/api/v1/patients", Some(&access), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh })),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    app.tdb.drop().await;
}

#[tokio::test]
async fn rbac_on_module_routes() {
    let app = app!();
    app.user("admin1", Role::Admin).await;
    app.user("pflege1", Role::Pflege).await;
    app.user("rez1", Role::Rezeption).await;
    app.user("arzt1", Role::Arzt).await;
    let (admin, _) = app.token("admin1").await;
    let (pflege, _) = app.token("pflege1").await;
    let (rez, _) = app.token("rez1").await;
    let (arzt, _) = app.token("arzt1").await;

    let (s, _) = app
        .call(
            "POST",
            "/api/v1/patients",
            Some(&pflege),
            Some(patient_body()),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = app
        .call("POST", "/api/v1/patients", Some(&rez), Some(patient_body()))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(v["allergies"], json!(["Penizillin"]));

    let (s, v) = app
        .call(
            "GET",
            &format!("/api/v1/patients/{id}"),
            Some(&pflege),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let version = v["version"].as_i64().unwrap();

    let (s, _) = app
        .call(
            "PUT",
            &format!("/api/v1/patients/{id}"),
            Some(&rez),
            Some(json!({ "version": version, "phone": "0800" })),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = app
        .call(
            "PUT",
            &format!("/api/v1/patients/{id}"),
            Some(&arzt),
            Some(json!({ "version": version, "phone": "0800" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // Veraltete Version → 409
    let (s, _) = app
        .call(
            "PUT",
            &format!("/api/v1/patients/{id}"),
            Some(&arzt),
            Some(json!({ "version": version, "phone": "0900" })),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Löschen nur Admin
    let (s, _) = app
        .call(
            "DELETE",
            &format!("/api/v1/patients/{id}"),
            Some(&arzt),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Unbekannte API-Route: nur Admin, für andere 403
    let (s, _) = app.call("GET", "/api/v1/unknown", Some(&arzt), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Audit-Trail: nicht für Arzt, aber für Admin; enthält Einträge
    let (s, _) = app
        .call(
            "GET",
            &format!("/api/v1/audit/patient/{id}"),
            Some(&arzt),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = app
        .call(
            "GET",
            &format!("/api/v1/audit/patient/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let entries = v["entries"].as_array().unwrap();
    assert!(entries.len() >= 2, "create + update protokolliert: {v}");
    // Audit enthält keine Klartext-Personendaten
    assert!(!v.to_string().contains("Testfrau"));
    assert!(!v.to_string().contains("0800"));

    let (s, _) = app
        .call(
            "DELETE",
            &format!("/api/v1/patients/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = app
        .call("GET", &format!("/api/v1/patients/{id}"), Some(&admin), None)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Interne Fehler verraten keine Details: ungültige Pfad-ID
    let (s, v) = app
        .call("GET", "/api/v1/patients/not-a-uuid", Some(&admin), None)
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(!v.to_string().to_lowercase().contains("sql"));

    assert!(app.audit_ok().await);
    app.tdb.drop().await;
}

#[tokio::test]
async fn gdpr_export_delete_anonymize() {
    let app = app!();
    app.user("admin1", Role::Admin).await;
    let (admin, _) = app.token("admin1").await;

    let mk = |app: &App, admin: &str| {
        let admin = admin.to_string();
        let router = app.router.clone();
        async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/patients")
                .header("Authorization", format!("Bearer {admin}"))
                .header("content-type", "application/json")
                .body(Body::from(patient_body().to_string()))
                .unwrap();
            let res = router.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let b = res.into_body().collect().await.unwrap().to_bytes();
            let v: Value = serde_json::from_slice(&b).unwrap();
            v["id"].as_str().unwrap().to_string()
        }
    };

    // Export
    let p1 = mk(&app, &admin).await;
    for fmt in ["json", "csv", "fhir"] {
        let (s, v) = app
            .call(
                "GET",
                &format!("/api/v1/patients/{p1}/export?format={fmt}"),
                Some(&admin),
                None,
            )
            .await;
        assert_eq!(s, StatusCode::OK, "Export {fmt}: {v}");
        assert!(
            v.to_string().contains("Testfrau"),
            "Export {fmt} enthält die Daten"
        );
    }

    // Hard Delete: falsche Bestätigung → 400, richtige → gelöscht
    let (s, _) = app
        .call(
            "POST",
            &format!("/api/v1/patients/{p1}/hard-delete"),
            Some(&admin),
            Some(json!({ "confirmation": "HARD_DELETE" })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = app
        .call(
            "POST",
            &format!("/api/v1/patients/{p1}/hard-delete"),
            Some(&admin),
            Some(json!({ "confirmation": "Testfrau" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM patients WHERE id = $1::uuid")
        .bind(&p1)
        .fetch_one(app.pool())
        .await
        .unwrap();
    assert_eq!(n, 0);
    // Der Lösch-Nachweis enthält keinen Namen
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE old_data::text ILIKE '%Testfrau%' OR new_data::text ILIKE '%Testfrau%'",
    )
    .fetch_one(app.pool())
    .await
    .unwrap();
    assert_eq!(leaked, 0);

    // Anonymisierung (irreversibel)
    let p2 = mk(&app, &admin).await;
    let (s, v) = app
        .call(
            "POST",
            &format!("/api/v1/admin/anonymization/patients/{p2}"),
            Some(&admin),
            Some(json!({})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let name: String = sqlx::query_scalar("SELECT name_family FROM patients WHERE id = $1::uuid")
        .bind(&p2)
        .fetch_one(app.pool())
        .await
        .unwrap();
    assert!(name.starts_with("ANON-"));
    let (s, _) = app
        .call(
            "POST",
            &format!("/api/v1/admin/anonymization/patients/{p2}"),
            Some(&admin),
            Some(json!({})),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "doppelte Anonymisierung");

    // Pseudonymisierung (reversibel über den Dienst)
    let p3 = mk(&app, &admin).await;
    let (s, v) = app
        .call(
            "POST",
            &format!("/api/v1/admin/anonymization/patients/{p3}"),
            Some(&admin),
            Some(json!({ "mode": "pseudonymize" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let svc = kis_core::anonymization::AnonymizationService::with_default_retention(
        app.tdb.db.clone(),
        app.security.clone(),
        MASTER_KEY,
    )
    .unwrap();
    let actor = kis_core::security::AuthUser {
        user_id: "admin1".into(),
        role: Role::Admin,
        session_id: None,
    };
    svc.depseudonymize_patient(p3.parse().unwrap(), &actor)
        .await
        .unwrap();
    let (name, birth): (String, chrono::NaiveDate) =
        sqlx::query_as("SELECT name_family, birth_date FROM patients WHERE id = $1::uuid")
            .bind(&p3)
            .fetch_one(app.pool())
            .await
            .unwrap();
    assert_eq!(name, "Testfrau");
    assert_eq!(birth.to_string(), "1970-05-01");

    let (s, v) = app
        .call("GET", "/api/v1/admin/anonymization/log", Some(&admin), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    assert!(app.audit_ok().await);
    app.tdb.drop().await;
}

#[tokio::test]
async fn audit_chain_is_serialized_append_only_and_tamper_evident() {
    let Some(tdb) = TestDb::new().await else {
        return;
    };
    let cfg = config();
    let security = Arc::new(Security::new(&cfg.security).unwrap());

    // Parallele Schreiber dürfen die Kette nicht verzweigen
    let mut handles = Vec::new();
    for i in 0..40 {
        let sec = security.clone();
        let pool = tdb.db.write().clone();
        handles.push(tokio::spawn(async move {
            sec.log_audit(
                &pool,
                "tester",
                "test.write",
                "thing",
                Some(&i.to_string()),
                None,
                Some(&json!({ "n": i, "text": "a|b" })),
                None,
            )
            .await
            .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let (ok, _) = security.verify_audit_chain(tdb.db.write()).await.unwrap();
    assert!(ok, "Kette nach parallelem Schreiben intakt");

    // Append-only: UPDATE und DELETE werden von der DB verweigert
    assert!(sqlx::query("UPDATE audit_logs SET action = 'x'")
        .execute(tdb.db.write())
        .await
        .is_err());
    assert!(sqlx::query("DELETE FROM audit_logs")
        .execute(tdb.db.write())
        .await
        .is_err());

    // Manipulation (nur mit Owner-Rechten möglich) wird erkannt
    let mut tx = tdb.db.write().begin().await.unwrap();
    sqlx::query("ALTER TABLE audit_logs DISABLE TRIGGER audit_logs_no_update_delete")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE audit_logs SET new_data = '{\"n\": 999}' WHERE id = (SELECT min(id) + 5 FROM audit_logs)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE audit_logs ENABLE TRIGGER audit_logs_no_update_delete")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (ok, idx) = security.verify_audit_chain(tdb.db.write()).await.unwrap();
    assert!(!ok);
    assert_eq!(idx, Some(5));

    tdb.drop().await;
}

#[tokio::test]
async fn startup_rejects_missing_or_weak_secrets() {
    let mut cfg = AppConfig::default();
    cfg.security.jwt_secret_env = "KIS_IT_UNSET_SECRET_VAR".into();
    assert!(Security::new(&cfg.security).is_err());
    assert!(Security::from_secrets(&cfg.security, "short", MASTER_KEY).is_err());
    assert!(Security::from_secrets(&cfg.security, JWT_SECRET, "AAAA").is_err());
    assert!(
        Security::from_secrets(
            &cfg.security,
            JWT_SECRET,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        )
        .is_err(),
        "Null-Schlüssel abgelehnt"
    );
    assert!(Security::from_secrets(&cfg.security, JWT_SECRET, MASTER_KEY).is_ok());
}

#[tokio::test]
async fn appointments_reject_double_booking_even_in_parallel() {
    let app = app!();
    app.user("admin1", Role::Admin).await;
    let (admin, _) = app.token("admin1").await;

    let (s, d) = app
        .call(
            "POST",
            "/api/v1/doctors",
            Some(&admin),
            Some(json!({ "nameFamily": "Doc", "nameGiven": "A", "specialties": ["Kardiologie"] })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    let doctor = d["id"].as_str().unwrap().to_string();
    assert_eq!(d["specialties"], json!(["Kardiologie"]));
    let (s, p) = app
        .call(
            "POST",
            "/api/v1/patients",
            Some(&admin),
            Some(patient_body()),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let patient = p["id"].as_str().unwrap().to_string();

    let body = |start: &str, end: &str| json!({ "patientId": patient, "doctorId": doctor, "startTime": start, "endTime": end });
    let (s, v) = app
        .call(
            "POST",
            "/api/v1/appointments",
            Some(&admin),
            Some(body("2030-01-01T08:00:00Z", "2030-01-01T08:30:00Z")),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = app
        .call(
            "POST",
            "/api/v1/appointments",
            Some(&admin),
            Some(body("2030-01-01T08:15:00Z", "2030-01-01T08:45:00Z")),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Parallel: gleiche, noch freie Zeit — genau eine Buchung gelingt
    let mut handles = Vec::new();
    for _ in 0..6 {
        let router = app.router.clone();
        let b = body("2030-01-02T09:00:00Z", "2030-01-02T09:30:00Z").to_string();
        let t = admin.clone();
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/v1/appointments")
                .header("Authorization", format!("Bearer {t}"))
                .header("content-type", "application/json")
                .body(Body::from(b))
                .unwrap();
            router.oneshot(req).await.unwrap().status()
        }));
    }
    let mut ok = 0;
    for h in handles {
        let st = h.await.unwrap();
        assert!(st == StatusCode::OK || st == StatusCode::CONFLICT, "{st}");
        if st == StatusCode::OK {
            ok += 1;
        }
    }
    assert_eq!(ok, 1);

    // Medikation anlegen (Arzt-Workflow) und auditieren
    let (s, v) = app
        .call("POST", "/api/v1/medications", Some(&admin), Some(json!({ "patientId": patient, "name": "Testmedikament", "startDate": "2030-01-01" })))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    assert!(app.audit_ok().await);
    app.tdb.drop().await;
}

#[tokio::test]
async fn soft_deleted_patients_hidden_from_non_admins() {
    let app = app!();
    app.user("admin1", Role::Admin).await;
    app.user("viewer1", Role::Viewer).await;
    let (admin, _) = app.token("admin1").await;
    let (viewer, _) = app.token("viewer1").await;
    let (_, p) = app
        .call(
            "POST",
            "/api/v1/patients",
            Some(&admin),
            Some(patient_body()),
        )
        .await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, _) = app
        .call(
            "DELETE",
            &format!("/api/v1/patients/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v) = app
        .call("GET", "/api/v1/patients?active=false", Some(&viewer), None)
        .await;
    assert_eq!(
        v["count"], 0,
        "Viewer sieht keine gelöschten Patienten: {v}"
    );
    let (_, v) = app
        .call("GET", "/api/v1/patients?active=false", Some(&admin), None)
        .await;
    assert_eq!(v["count"], 1);
    app.tdb.drop().await;
}
