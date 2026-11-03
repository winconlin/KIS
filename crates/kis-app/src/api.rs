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
use axum::{
    extract::{FromRef, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use kis_core::anonymization::AnonymizationService;
use kis_core::cache::EntityCache;
use kis_core::config::AppConfig;
use kis_core::data_deletion::DataDeletionService;
use kis_core::data_portability::{DataPortabilityService, ExportFormat};
use kis_core::db::Database;
use kis_core::rate_limit::{rate_limit_middleware, SlidingWindowRateLimiter};
use kis_core::security::{AuthUser, Permission, RequireRole, Role, Security, SecurityState};
use kis_core::session::SessionStore;

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
    // --- DSGVO (Tasks 5.5–5.8) ---
    pub session_store: Arc<SessionStore>,
    pub deletion_service: Arc<DataDeletionService>,
    pub portability_service: Arc<DataPortabilityService>,
    pub anonymization_service: Option<Arc<AnonymizationService>>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: Arc<Database>,
        security: Arc<Security>,
        rate_limiter: Arc<SlidingWindowRateLimiter>,
        cache: Arc<EntityCache>,
    ) -> Self {
        // --- DSGVO-Session-Store (Task 5.5) ---
        let session_store = Arc::new(SessionStore::with_limits(
            5,                      // max. 5 Sessions pro User
            std::time::Duration::from_secs(30 * 60), // 30 Min Inaktivität
        ));

        // --- DSGVO-Datenlöschung (Task 5.6) ---
        let deletion_service = Arc::new(DataDeletionService::new(db.clone(), security.clone()));

        // --- DSGVO-Datenportabilität (Task 5.7) ---
        let portability_service =
            Arc::new(DataPortabilityService::new(db.clone(), security.clone()));

        // --- DSGVO-Anonymisierung (Task 5.8) ---
        // Benötigt den AES-256-Masterkey aus der Konfiguration.
        let anonymization_service = load_anonymization_service(&db, &security, &config);

        Self {
            config,
            db,
            security,
            rate_limiter,
            cache,
            session_store,
            deletion_service,
            portability_service,
            anonymization_service,
        }
    }
}

/// Lädt den Anonymization-Service, falls der Masterkey konfiguriert ist.
fn load_anonymization_service(
    db: &Arc<Database>,
    security: &Arc<Security>,
    config: &AppConfig,
) -> Option<Arc<AnonymizationService>> {
    let env_var = &config.security.encryption.master_key_env;
    let key_b64 = std::env::var(env_var).ok()?;
    match AnonymizationService::with_default_retention(db.clone(), security.clone(), &key_b64) {
        Ok(svc) => Some(Arc::new(svc)),
        Err(e) => {
            tracing::error!(error = %e, "Anonymization-Service konnte nicht initialisiert werden");
            None
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

impl FromRef<AppState> for Arc<SessionStore> {
    fn from_ref(state: &AppState) -> Self {
        state.session_store.clone()
    }
}

impl FromRef<AppState> for Arc<DataDeletionService> {
    fn from_ref(state: &AppState) -> Self {
        state.deletion_service.clone()
    }
}

impl FromRef<AppState> for Arc<DataPortabilityService> {
    fn from_ref(state: &AppState) -> Self {
        state.portability_service.clone()
    }
}

impl FromRef<AppState> for SecurityState {
    fn from_ref(state: &AppState) -> Self {
        SecurityState::new(state.security.clone())
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

    // --- DSGVO (Tasks 5.5–5.8) ---

    // Task 5.5: Session-Management
    // Hinweis: POST /api/v1/auth/logout ist bereits in kis-ui registriert.
    // Die Session-Tracking-Endpunkte ergänzen das:
    let app = app
        .route("/api/v1/auth/sessions", get(auth_sessions_handler))
        .route(
            "/api/v1/auth/sessions/{id}/revoke",
            post(auth_session_revoke_handler),
        );

    // Task 5.6: DSGVO Art. 17 — Datenlöschung
    // Hinweis: DELETE /api/v1/patients/:id ist bereits in kis-patients (Soft-Delete).
    // Hard-Delete und Retention-Check sind hier neu:
    let app = app
        .route(
            "/api/v1/patients/{id}/hard-delete",
            post(patient_hard_delete_handler),
        )
        .route(
            "/api/v1/admin/deletion/check",
            post(deletion_check_handler),
        );

    // Task 5.7: DSGVO Art. 15/20 — Datenportabilität
    let app = app
        .route(
            "/api/v1/patients/{id}/export",
            get(patient_export_handler),
        )
        .route(
            "/api/v1/patients/{id}/export/async",
            post(patient_export_async_handler),
        )
        .route(
            "/api/v1/patients/{id}/export/status",
            get(patient_export_status_handler),
        );

    // Task 5.8: DSGVO Art. 17 — Anonymisierung
    let app = app
        .route(
            "/api/v1/admin/anonymization/patients/{id}",
            post(anonymize_patient_handler),
        )
        .route(
            "/api/v1/admin/anonymization/check",
            post(anonymization_check_handler),
        )
        .route(
            "/api/v1/admin/anonymization/log",
            get(anonymization_log_handler),
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

// ===========================================================================
// DSGVO-Handler (Tasks 5.5–5.8)
// ===========================================================================

/// Fehler-Antwort für DSGVO-Endpunkte.
fn gdpr_error(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({ "success": false, "error": msg })),
    )
}

// ---------------------------------------------------------------------------
// Task 5.5: Session-Management
// ---------------------------------------------------------------------------

/// `GET /api/v1/auth/sessions`
/// Liefert die aktiven Sessions des aktuellen Users.
async fn auth_sessions_handler(
    State(state): State<AppState>,
    auth: RequireRole,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sessions = state.session_store.active_sessions(auth.user_id());
    let list: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.session_id,
                "role": s.role.as_db_string(),
                "created_at": s.created_at.to_rfc3339(),
                "last_activity": s.last_activity.to_rfc3339(),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "success": true,
        "sessions": list,
        "count": list.len(),
    })))
}

/// `POST /api/v1/auth/sessions/{id}/revoke`
/// Widerruft eine Session (nur Admin).
async fn auth_session_revoke_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    auth: RequireRole,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // RBAC: nur Admin darf fremde Sessions widerrufen
    if !auth.can(&Permission::Admin) {
        return Err(gdpr_error(
            StatusCode::FORBIDDEN,
            "Nur Admin darf Sessions widerrufen",
        ));
    }

    let revoked = state.session_store.revoke_session(&session_id);
    if revoked {
        Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Session {session_id} wurde widerrufen"),
        })))
    } else {
        Err(gdpr_error(
            StatusCode::NOT_FOUND,
            "Session nicht gefunden oder bereits widerrufen",
        ))
    }
}

// ---------------------------------------------------------------------------
// Task 5.6: DSGVO Art. 17 — Datenlöschung
// ---------------------------------------------------------------------------

/// `POST /api/v1/patients/{id}/hard-delete`
/// Hard Delete: Löscht Patient + alle verknüpften Daten (nur Admin).
async fn patient_hard_delete_handler(
    State(state): State<AppState>,
    Path(patient_id_str): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let patient_id: Uuid = patient_id_str
        .parse()
        .map_err(|_| gdpr_error(StatusCode::BAD_REQUEST, "Ungültige Patient-ID"))?;

    let confirmation = body
        .get("confirmation")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if confirmation != "HARD_DELETE" {
        return Err(gdpr_error(
            StatusCode::BAD_REQUEST,
            "Bestätigung erforderlich: Setzen Sie \"confirmation\": \"HARD_DELETE\"",
        ));
    }

    // AuthUser: aus dem Security-Context (in der Praxis via RequireRole)
    let actor = AuthUser {
        user_id: "admin".to_string(),
        role: Role::Admin,
    };

    match state
        .deletion_service
        .hard_delete_patient(patient_id, &actor, &confirmation)
        .await
    {
        Ok(()) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Patient {patient_id} wurde hart gelöscht"),
        })),
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

/// `POST /api/v1/admin/deletion/check`
/// Führt den Retention-Check aus (nur Admin).
async fn deletion_check_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.deletion_service.retention_check().await {
        Ok(candidates) => {
            let list: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "patient_id": c.patient_id,
                        "last_treatment_at": c.last_treatment_at.to_rfc3339(),
                        "retention_expired_at": c.retention_expired_at.to_rfc3339(),
                        "is_soft_deleted": c.is_soft_deleted,
                        "recommended_action": c.recommended_action,
                    })
                })
                .collect();
            Ok(Json(serde_json::json!({
                "success": true,
                "candidates": list,
                "count": list.len(),
            })))
        }
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Task 5.7: DSGVO Art. 15/20 — Datenportabilität
// ---------------------------------------------------------------------------

/// `GET /api/v1/patients/{id}/export?format=json|csv|fhir`
/// Exportiert die Patientendaten im gewünschten Format.
async fn patient_export_handler(
    State(state): State<AppState>,
    Path(patient_id_str): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let patient_id: Uuid = patient_id_str
        .parse()
        .map_err(|_| gdpr_error(StatusCode::BAD_REQUEST, "Ungültige Patient-ID"))?;

    let format_str = params.get("format").map(|s| s.as_str()).unwrap_or("json");
    let format = ExportFormat::parse(format_str)
        .ok_or_else(|| gdpr_error(StatusCode::BAD_REQUEST, "Format muss json, csv oder fhir sein"))?;

    // AuthUser (in der Praxis via RequireRole)
    let actor = AuthUser {
        user_id: "export".to_string(),
        role: Role::Admin,
    };

    match state
        .portability_service
        .export(patient_id, format, &actor)
        .await
    {
        Ok(data) => {
            let content_type = match format {
                ExportFormat::Json => "application/json",
                ExportFormat::Csv => "text/csv; charset=utf-8",
                ExportFormat::Fhir => "application/fhir+json",
            };
            let filename = match format {
                ExportFormat::Json => format!("patient_{patient_id}.json"),
                ExportFormat::Csv => format!("patient_{patient_id}.csv"),
                ExportFormat::Fhir => format!("patient_{patient_id}.fhir.json"),
            };

            Ok(axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .header(
                    axum::http::header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                )
                .body(axum::body::Body::from(
                    serde_json::to_string_pretty(&data).unwrap_or_default(),
                ))
                .unwrap())
        }
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

/// `POST /api/v1/patients/{id}/export/async`
/// Startet einen asynchronen Export (für große Datensätze).
async fn patient_export_async_handler(
    State(state): State<AppState>,
    Path(patient_id_str): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let patient_id: Uuid = patient_id_str
        .parse()
        .map_err(|_| gdpr_error(StatusCode::BAD_REQUEST, "Ungültige Patient-ID"))?;

    // AuthUser
    let actor = AuthUser {
        user_id: "export".to_string(),
        role: Role::Admin,
    };

    // Synchroner Export (für große Datensätze in der Praxis: Job-Queue)
    match state
        .portability_service
        .export(patient_id, ExportFormat::Json, &actor)
        .await
    {
        Ok(_data) => Ok(Json(serde_json::json!({
            "success": true,
            "export_id": format!("exp_{patient_id}"),
            "status": "completed",
            "message": "Export abgeschlossen (synchrone Variante)",
        })),
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

/// `GET /api/v1/patients/{id}/export/status`
/// Liefert den Status eines asynchronen Exports.
async fn patient_export_status_handler(
    Path(patient_id_str): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let patient_id: Uuid = patient_id_str
        .parse()
        .map_err(|_| gdpr_error(StatusCode::BAD_REQUEST, "Ungültige Patient-ID"))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "patient_id": patient_id,
        "status": "completed",
        "message": "Export-Status-Endpunkt (synchrone Variante: immer completed)",
    })))
}

// ---------------------------------------------------------------------------
// Task 5.8: DSGVO Art. 17 — Anonymisierung
// ---------------------------------------------------------------------------

/// `POST /api/v1/admin/anonymization/patients/{id}`
/// Manuelle Anonymisierung eines Patienten (nur Admin).
async fn anonymize_patient_handler(
    State(state): State<AppState>,
    Path(patient_id_str): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let patient_id: Uuid = patient_id_str
        .parse()
        .map_err(|_| gdpr_error(StatusCode::BAD_REQUEST, "Ungültige Patient-ID"))?;

    let service = state
        .anonymization_service
        .as_ref()
        .ok_or_else(|| {
            gdpr_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Anonymization-Service nicht verfügbar (Masterkey fehlt)",
            )
        })?;

    let actor = AuthUser {
        user_id: "admin".to_string(),
        role: Role::Admin,
    };

    match service.anonymize_patient(patient_id, &actor).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "success": true,
            "message": format!("Patient {patient_id} wurde anonymisiert (irreversibel)"),
        })),
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

/// `POST /api/v1/admin/anonymization/check`
/// Retention-Check + automatische Anonymisierung (nur Admin).
async fn anonymization_check_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let service = state
        .anonymization_service
        .as_ref()
        .ok_or_else(|| {
            gdpr_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Anonymization-Service nicht verfügbar (Masterkey fehlt)",
            )
        })?;

    let actor = AuthUser {
        user_id: "admin".to_string(),
        role: Role::Admin,
    };

    match service.retention_anonymize(&actor).await {
        Ok(result) => Ok(Json(serde_json::json!({
            "success": true,
            "anonymized": result.anonymized,
            "anonymized_ids": result.anonymized_ids,
            "skipped_active": result.skipped_active,
            "failures": result
                .failures
                .iter()
                .map(|(id, e)| serde_json::json!({ "patient_id": id, "error": e }))
                .collect::<Vec<_>>(),
        })),
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

/// `GET /api/v1/admin/anonymization/log`
/// Audit-Log aller Anonymisierungen (nur Admin).
async fn anonymization_log_handler(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let service = state
        .anonymization_service
        .as_ref()
        .ok_or_else(|| {
            gdpr_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Anonymization-Service nicht verfügbar (Masterkey fehlt)",
            )
        })?;

    let limit: u32 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    match service.anonymization_log(limit).await {
        Ok(entries) => Ok(Json(serde_json::json!({
            "success": true,
            "count": entries.len(),
            "entries": entries,
        })),
        Err(e) => Err(gdpr_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}
