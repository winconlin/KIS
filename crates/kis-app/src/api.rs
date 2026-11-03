//! API-Router: Baut den Axum-Router mit allen aktivierten Modulen.
//!
//! Modularer Aufbau: Jedes Modul registriert seinen eigenen Router.
//! Alle Module erhalten den selben `Arc<Database>` (PostgreSQL-Pool),
//! damit sie auf demselben Write-/Read-Pool arbeiten.
//!
//! Phase-1-Tasks integriert:
//! - Rate-Limiting (1.11): Sliding-Window-Middleware auf /auth/* + /api/v1/*
//! - Audit-Hash-Chain (1.12): GET /api/v1/audit/:entity_type/:entity_id
//! - Caching (1.17): In-Memory-Cache (moka) als State für die Services
//!
//! State-Architektur:
//! - `AppState` wird als einziger App-State in den Router injiziert
//! - `Arc<Database>`, `Arc<Security>`, `Arc<SlidingWindowRateLimiter>`,
//!   `Arc<EntityCache>` werden über `FromRef` aus `AppState` abgeleitet

use std::sync::Arc;
use axum::{extract::FromRef, routing::get, Json, Router};
use tower_http::trace::TraceLayer;

use kis_core::cache::EntityCache;
use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::rate_limit::{rate_limit_middleware, SlidingWindowRateLimiter};
use kis_core::security::Security;

/// App-State: Bündelt alle Komponenten, die die Handler brauchen.
///
/// `AppState` ist der "Root"-State, aus dem die anderen Komponenten
/// über `FromRef` abgeleitet werden.
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub rate_limiter: Arc<SlidingWindowRateLimiter>,
    pub cache: Arc<EntityCache>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: Arc<Database>,
        security: Arc<Security>,
        rate_limiter: Arc<SlidingWindowRateLimiter>,
        cache: Arc<EntityCache>,
    ) -> Self {
        Self {
            config,
            db,
            security,
            rate_limiter,
            cache,
        }
    }
}

// FromRef-Implementierungen: Der Router-State ist AppState,
// aber die Handler erwarten Arc<Database>, Arc<Security> etc.
impl FromRef<AppState> for Arc<Database> {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl FromRef<AppState> for Arc<Security> {
    fn from_ref(state: &AppState) -> Self {
        state.security.clone()
    }
}

impl FromRef<AppState> for Arc<SlidingWindowRateLimiter> {
    fn from_ref(state: &AppState) -> Self {
        state.rate_limiter.clone()
    }
}

impl FromRef<AppState> for Arc<EntityCache> {
    fn from_ref(state: &AppState) -> Self {
        state.cache.clone()
    }
}

impl FromRef<AppState> for AppConfig {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

/// Baut den Haupt-Router
pub fn build_router(state: AppState) -> Router {
    // Rate-Limit-Middleware als Closure: greift direkt auf state.rate_limiter zu.
    // (from_fn_with_state wäre nötig, wenn der Middleware-State ≠ Router-State wäre.)
    let rl_state = state.rate_limiter.clone();
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/status", get(status))
        .route("/api/v1/health/db", get(db_health))
        .route("/api/v1/health/replica", get(replica_health))
        .route("/api/v1/health/cache", get(cache_health))
        .route(
            "/api/v1/audit/{entity_type}/{entity_id}",
            get(audit_trail_handler),
        );

    // --- Phase 1: Kernmodule (CRUD + Optimistic Locking) ---

    // Auth-Service (kis-ui): /api/v1/auth/* (Login, Refresh, Logout)
    let app = app
        .with_state(state.clone())
        .merge(kis_ui::register_router(
            state.db.clone(),
            state.security.clone(),
            state.config.clone(),
        ));

    // Patient-Service: /api/v1/patients
    let pool = state.db.write_pool.clone();
    let app = app.merge(kis_patients::register_router(Arc::new(pool)));

    // Arzt-Service: /api/v1/doctors
    let pool = state.db.write_pool.clone();
    let app = app.merge(kis_doctors::register_router(Arc::new(pool)));

    // Termin-Service: /api/v1/appointments (inkl. Doppelbuchungsprüfung, ISiK, Kalender)
    let pool = state.db.write_pool.clone();
    let app = app.merge(kis_appointments::register_router(Arc::new(pool)));

    // Medikament-Service: /api/v1/medications
    let pool = state.db.write_pool.clone();
    let app = app.merge(kis_medications::register_router(Arc::new(pool)));

    // --- Phase 2–3: Schnittstellen & Integrationen (noch Stubs) ---
    // Diese Module werden in späteren Phasen aktiviert:
    //   HL7 v2.5 (TCP), FHIR R4, DICOM SCP, XDS, GDT, MEDICO, Telematik, GUI

    // --- State-Injection: AppState für alle Handler ---
    // (Schon beim ersten merge mit with_state injiziert — hier nur noch Layer)

    // Rate-Limit-Middleware (Closure greift direkt auf den Limiter zu)
    let app = app
        .layer(axum::middleware::from_fn_with_state(
            rl_state,
            rate_limit_middleware,
        ))
        // Trace-Layer: loggt jeden Request (Method, Path, Status, Latenz)
        .layer(TraceLayer::new_for_http());

    app
}

/// Health-Check: Liefert "ok", wenn der Server läuft.
async fn health_check() -> &'static str {
    "ok"
}

/// DB-Health-Check: Connection-Pool-Statistiken (Task 5.1).
///
/// `GET /api/v1/health/db`
///
/// Liefert die Pool-Metriken (active, idle, total, max, waiters) für
/// beide Pools. Funktioniert auch ohne aktive DB-Verbindung — die Werte
/// sind die sqlx-internen Zähler des Pools.
async fn db_health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    let (write, read) = state.db.pool_metrics();
    Json(serde_json::json!({
        "success": true,
        "write_pool": write,
        "read_pool": read,
    }))
}

/// Replica-Health-Check: Read-Replica-Status (Task 5.3).
///
/// `GET /api/v1/health/replica`
///
/// Liefert, ob der Read-Pool auf einem separaten Server läuft und ob
/// dieser Read-Only ist (`transaction_read_only = on`). `readonly` ist
/// `null`, wenn keine Verbindung zum Read-Pool besteht.
async fn replica_health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.db.replica_status().await;
    Json(serde_json::json!({
        "success": true,
        "separate_pool": status.separate_pool,
        "readonly": status.readonly,
        "write_max_connections": status.write_max_connections,
        "read_max_connections": status.read_max_connections,
    }))
}

/// Cache-Health-Check: Cache-Statistiken (Task 5.2).
///
/// `GET /api/v1/health/cache`
///
/// Liefert die Read-Through-Cache-Statistiken: hit_count, miss_count,
/// hit_rate und evictions. Funktioniert ohne DB-Verbindung — die Werte
/// sind die internen Zähler des in-memory Caches (moka).
async fn cache_health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    let stats = state.cache.stats();
    let entry_count = state.cache.entry_count();
    Json(serde_json::json!({
        "success": true,
        "hit_count": stats.hit_count,
        "miss_count": stats.miss_count,
        "hit_rate": stats.hit_rate,
        "evictions": stats.evictions,
        "entry_count": entry_count,
        "max_entries": state.cache.total_capacity(),
    }))
}

/// Audit-Trail-Handler: Liefert die Audit-Logs für eine Entität (Task 1.12).
///
/// `GET /api/v1/audit/:entity_type/:entity_id`
async fn audit_trail_handler(
    axum::extract::Path((entity_type, entity_id)): axum::extract::Path<(String, String)>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let security = &state.security;
    let db = &state.db;
    let trail = security
        .get_audit_trail(&db.write_pool, &entity_type, &entity_id)
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "success": false, "error": e.to_string() })),
            )
        })?;
    Ok(Json(serde_json::json!({
        "success": true,
        "entity_type": entity_type,
        "entity_id": entity_id,
        "entries": trail,
    })))
}

/// Status-Endpunkt: Version, laufende Module.
async fn status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": "running",
        "modules": [
            {"name": "patients", "enabled": true},
            {"name": "doctors", "enabled": true},
            {"name": "appointments", "enabled": true},
            {"name": "medications", "enabled": true},
            {"name": "hl7", "enabled": false},
            {"name": "fhir", "enabled": false},
            {"name": "dicom", "enabled": false},
            {"name": "xds", "enabled": false},
            {"name": "gdt", "enabled": false},
            {"name": "medico", "enabled": false},
            {"name": "ti", "enabled": false},
            {"name": "ui", "enabled": false},
        ]
    }))
}
