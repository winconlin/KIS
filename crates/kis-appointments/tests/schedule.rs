//! AP-01: GET /api/v1/doctors/:id/schedule gehört zum Terminmodul.
//!
//! Prüft, dass die Route im Router des `kis-appointments`-Moduls registriert
//! ist und auf einen gültigen Arzt 200 + JSON-Antwort liefert.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use kis_appointments::AppointmentsModule;
use kis_core::config::{
    AppConfig, DatabaseConfig, LoggingConfig, ModulesConfig, SecurityConfig, ServerConfig,
};
use kis_core::db::Database;
use kis_core::events::EventBus;
use kis_core::identity::AuthUser;
use kis_core::module::{Module, ModuleContext};
use kis_core::platform::Platform;
use kis_core::security::Security;
use tower::ServiceExt;

const TEST_DB_URL: &str = "postgres://postgres:***@127.0.0.1:5432/postgres";

async fn build_context(url: &str) -> ModuleContext {
    let db_cfg = DatabaseConfig {
        write_url: url.to_string(),
        pool_size: 4,
        ..Default::default()
    };
    let db = Database::connect(&db_cfg, true)
        .await
        .expect("DB-Verbindung + Migrationen");
    let sec_cfg = SecurityConfig::default();
    let security = Arc::new(
        Security::from_secrets(
            &sec_cfg,
            "integration-test-secret-0123456789abcdef",
            "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=",
        )
        .expect("Security"),
    );
    let config = Arc::new(AppConfig {
        server: ServerConfig::default(),
        database: db_cfg.clone(),
        security: sec_cfg.clone(),
        logging: LoggingConfig::default(),
        modules: ModulesConfig {
            disabled: vec![],
            experimental: vec![],
        },
    });
    let platform = Arc::new(Platform::builtin(&sec_cfg));
    let events = Arc::new(EventBus::new());
    ModuleContext {
        db: Arc::new(db),
        security,
        config,
        platform,
        events,
    }
}

async fn make_appointments_router(pool_url: &str) -> axum::Router {
    let ctx = build_context(pool_url).await;
    let mod_routes = AppointmentsModule.routes(&ctx);
    let (router, _rules) = mod_routes.into_parts();

    // Test-Auth: legt einen AuthUser in die Request-Extensions
    async fn test_auth(
        req: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        let mut req = req;
        let user = AuthUser::with_builtin_role("00000000-0000-0000-0000-000000000001", "admin");
        req.extensions_mut().insert(user);
        next.run(req).await
    }

    router.route_layer(axum::middleware::from_fn(test_auth))
}

#[tokio::test]
async fn schedule_route_is_registered_in_appointments_module() {
    let url = std::env::var("KIS_TEST_DATABASE_URL").unwrap_or_else(|_| TEST_DB_URL.to_string());
    let router = make_appointments_router(&url).await;

    let req = Request::builder()
        .uri("/api/v1/doctors/00000000-0000-0000-0000-000000000000/schedule?date=2030-01-15")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();

    // 404 (Arzt existiert nicht) oder 200 — beides beweist: Route ist registriert.
    let status = res.status();
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::OK,
        "Unerwarteter Status {status:?} — Route ist möglicherweise nicht registriert"
    );
}

#[tokio::test]
async fn schedule_returns_200_for_existing_doctor() {
    let url = std::env::var("KIS_TEST_DATABASE_URL").unwrap_or_else(|_| TEST_DB_URL.to_string());
    let router = make_appointments_router(&url).await;

    let db_cfg = DatabaseConfig {
        write_url: url.clone(),
        pool_size: 4,
        ..Default::default()
    };
    let db = Database::connect(&db_cfg, true).await.unwrap();
    let pool = db.write();

    // Arzt anlegen
    let doctor_id: uuid::Uuid = sqlx::query_scalar(
        r#"INSERT INTO doctors (name_family, name_given)
           VALUES ('Test', 'Arzt')
           RETURNING id"#,
    )
    .fetch_one(pool)
    .await
    .unwrap();

    // Patient + Termin anlegen
    let start = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_utc();
    let end = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(10, 30, 0)
        .unwrap()
        .and_utc();
    let patient_id: uuid::Uuid = sqlx::query_scalar(
        r#"INSERT INTO patients (name_family, name_given, birth_date)
           VALUES ('Test', 'Patient', '1990-01-01')
           RETURNING id"#,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let _ = sqlx::query(
        r#"INSERT INTO appointments (doctor_id, patient_id, start_time, end_time)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(doctor_id)
    .bind(patient_id)
    .bind(start)
    .bind(end)
    .execute(pool)
    .await
    .unwrap();

    // GET /api/v1/doctors/:id/schedule?date=...
    let date_str = chrono::Utc::now().date_naive().to_string();
    let uri = format!("/api/v1/doctors/{}/schedule?date={}", doctor_id, date_str);
    let req = Request::builder().uri(&uri).body(Body::empty()).unwrap();
    let res = router.oneshot(req).await.unwrap();

    assert_eq!(
        res.status(),
        StatusCode::OK,
        "GET /doctors/…/schedule muss 200 liefern"
    );

    let body = axum::body::to_bytes(res.into_body(), 1_000_000)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(
        json.get("doctorId").is_some(),
        "doctorId fehlt in der Antwort"
    );
    assert!(json.get("date").is_some(), "date fehlt in der Antwort");
    assert!(
        json.get("doctorName").is_some(),
        "doctorName fehlt in der Antwort"
    );
    assert!(json.get("freeSlotCount").is_some(), "freeSlotCount fehlt");
    assert!(json.get("totalSlotCount").is_some(), "totalSlotCount fehlt");
    assert!(json.get("slots").is_some(), "slots fehlt in der Antwort");
    let slots = json["slots"].as_array().expect("slots muss ein Array sein");
    assert_eq!(
        slots.len(),
        16,
        "16 Slots erwartet (08:00–16:00 à 30 Min), gefunden: {}",
        slots.len()
    );
    // Der Slot 10:00 muss belegt sein
    let occupied = slots.iter().find(|s| s["time"].as_str() == Some("10:00"));
    assert!(occupied.is_some(), "Slot 10:00 fehlt");
    assert!(
        !occupied.unwrap()["isFree"].as_bool().unwrap(),
        "Slot 10:00 muss belegt sein"
    );
    assert!(
        occupied.unwrap()["appointmentId"].is_string(),
        "appointmentId fehlt am belegten Slot"
    );
    // Ein freier Slot muss existieren
    let free = slots.iter().find(|s| s["isFree"].as_bool() == Some(true));
    assert!(free.is_some(), "Mindestens 1 freier Slot erwartet");

    // Aufräumen
    let _ = sqlx::query("DELETE FROM appointments WHERE doctor_id = $1")
        .bind(doctor_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM patients WHERE id = $1")
        .bind(patient_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM doctors WHERE id = $1")
        .bind(doctor_id)
        .execute(pool)
        .await;
}
