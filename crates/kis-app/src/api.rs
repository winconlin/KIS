//! API-Router: Baut den Axum-Router mit allen aktivierten Modulen.
//!
//! Sicherheitsarchitektur:
//! - **Eine** zentrale Authentifizierungs-Middleware ([`auth_middleware`])
//!   liegt über allen Routen, auch über den Routern der Fachmodule.
//! - Sie ermittelt für jede Anfrage über
//!   [`kis_core::access_policy::required_access`] die erforderliche
//!   Berechtigung (deny-by-default), prüft das Access-Token vollständig
//!   gegen die Sitzungstabelle und legt den geprüften [`AuthUser`] in die
//!   Request-Extensions. Handler erhalten ihn über [`RequireRole`].
//! - Fehlerantworten enthalten keine internen Details.

use std::sync::Arc;

use axum::{
    extract::{FromRef, Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use kis_core::access_policy::{required_access, Access};
use kis_core::anonymization::{AnonymizationError, AnonymizationService};
use kis_core::auth::{self, AuthError};
use kis_core::cache::EntityCache;
use kis_core::config::AppConfig;
use kis_core::data_deletion::{DataDeletionService, DeletionError};
use kis_core::data_portability::{DataPortabilityService, ExportError, ExportFormat};
use kis_core::db::Database;
use kis_core::rate_limit::{rate_limit_middleware, SlidingWindowRateLimiter};
use kis_core::security::{
    forbidden_response, unauthorized_response, Permission, RequireRole, Security,
};

/// Maximale Größe eines Request-Bodys (1 MiB).
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// App-State: Bündelt alle Komponenten, die die Handler brauchen.
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub rate_limiter: Arc<SlidingWindowRateLimiter>,
    pub cache: Arc<EntityCache>,
    pub deletion_service: Arc<DataDeletionService>,
    pub portability_service: Arc<DataPortabilityService>,
    pub anonymization_service: Arc<AnonymizationService>,
}

impl AppState {
    /// Baut den State. Schlägt fehl, wenn der Master-Key ungültig ist —
    /// es gibt keinen stillen Betrieb ohne Pseudonymisierungs-Schlüssel.
    pub fn new(
        config: AppConfig,
        db: Arc<Database>,
        security: Arc<Security>,
        rate_limiter: Arc<SlidingWindowRateLimiter>,
        cache: Arc<EntityCache>,
    ) -> anyhow::Result<Self> {
        let deletion_service = Arc::new(DataDeletionService::new(db.clone(), security.clone()));
        let portability_service =
            Arc::new(DataPortabilityService::new(db.clone(), security.clone()));
        let key_b64 = std::env::var(&config.security.encryption.master_key_env)
            .map_err(|_| anyhow::anyhow!("Master-Key nicht gesetzt"))?;
        let anonymization_service = Arc::new(
            AnonymizationService::with_default_retention(db.clone(), security.clone(), &key_b64)
                .map_err(|e| anyhow::anyhow!("Anonymisierungsdienst: {e}"))?,
        );
        Ok(Self {
            config,
            db,
            security,
            rate_limiter,
            cache,
            deletion_service,
            portability_service,
            anonymization_service,
        })
    }
}

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

/// Baut den Haupt-Router.
pub fn build_router(state: AppState) -> Router {
    let own = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/status", get(status))
        .route("/api/v1/health/db", get(db_health))
        .route("/api/v1/health/replica", get(replica_health))
        .route("/api/v1/health/cache", get(cache_health))
        .route(
            "/api/v1/audit/:entity_type/:entity_id",
            get(audit_trail_handler),
        )
        // --- Authentifizierung ---
        .route("/api/v1/auth/login", post(login_handler))
        .route("/api/v1/auth/refresh", post(refresh_handler))
        .route("/api/v1/auth/logout", post(logout_handler))
        .route("/api/v1/auth/sessions", get(auth_sessions_handler))
        .route(
            "/api/v1/auth/sessions/:id/revoke",
            post(auth_session_revoke_handler),
        )
        // --- DSGVO Art. 17: Löschung ---
        .route(
            "/api/v1/patients/:id/hard-delete",
            post(patient_hard_delete_handler),
        )
        .route("/api/v1/admin/deletion/check", post(deletion_check_handler))
        // --- DSGVO Art. 15/20: Datenportabilität ---
        .route("/api/v1/patients/:id/export", get(patient_export_handler))
        // --- Anonymisierung / Pseudonymisierung ---
        .route(
            "/api/v1/admin/anonymization/patients/:id",
            post(anonymize_patient_handler),
        )
        .route(
            "/api/v1/admin/anonymization/check",
            post(anonymization_check_handler),
        )
        .route(
            "/api/v1/admin/anonymization/log",
            get(anonymization_log_handler),
        )
        .with_state(state.clone());

    // Fachmodule (eigene Router, gleicher Write-Pool)
    let pool = Arc::new(state.db.write_pool.clone());
    let app = own
        .merge(kis_ui::register_router())
        .merge(kis_patients::register_router(pool.clone()))
        .merge(kis_doctors::register_router(pool.clone()))
        .merge(kis_appointments::register_router(pool.clone()))
        .merge(kis_medications::register_router(pool));

    // Layer-Reihenfolge (außen → innen): Trace, Security-Header, Body-Limit,
    // Rate-Limit, Authentifizierung, Handler.
    app.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ))
    .layer(axum::middleware::from_fn_with_state(
        state.rate_limiter.clone(),
        rate_limit_middleware,
    ))
    .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES))
    .layer(axum::middleware::from_fn(security_headers))
    .layer(TraceLayer::new_for_http())
}

// ===========================================================================
// Middleware
// ===========================================================================

/// Zentrale Authentifizierung und Autorisierung (deny-by-default).
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let access = required_access(req.method(), req.uri().path());
    if access == Access::Public {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let Some(token) = token else {
        return unauthorized_response("Fehlender Bearer-Token");
    };

    let user = match auth::validate_access(state.db.write(), &state.security, token).await {
        Ok(u) => u,
        Err(AuthError::Internal(e)) => {
            tracing::error!(error = %e, "Sitzungsprüfung fehlgeschlagen");
            return internal_error();
        }
        Err(_) => return unauthorized_response("Ungültiger oder abgelaufener Token"),
    };

    if let Access::Permission(p) = access {
        if !user.role.has_permission(&p) {
            tracing::warn!(role = %user.role, permission = %p, path = %req.uri().path(),
                "403: Berechtigung fehlt");
            return forbidden_response("Keine Berechtigung für diese Aktion");
        }
    }

    req.extensions_mut().insert(user);
    next.run(req).await
}

/// Setzt konservative Sicherheits-Header auf jede Antwort.
async fn security_headers(req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    h.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'",
        ),
    );
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "success": false, "error": msg, "status": status.as_u16() })),
    )
        .into_response()
}

fn internal_error() -> Response {
    json_error(StatusCode::INTERNAL_SERVER_ERROR, "Interner Serverfehler")
}

fn parse_uuid(s: &str) -> Result<Uuid, Response> {
    s.parse()
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "Ungültige ID"))
}

// ===========================================================================
// Health / Status
// ===========================================================================

async fn health_check() -> &'static str {
    "ok"
}

/// `GET /api/v1/health/db` — Pool-Metriken (Berechtigung `config.read`).
async fn db_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (write, read) = state.db.pool_metrics();
    Json(serde_json::json!({ "success": true, "write_pool": write, "read_pool": read }))
}

/// `GET /api/v1/health/replica`
async fn replica_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let status = state.db.replica_status().await;
    Json(serde_json::json!({ "success": true, "replica": status }))
}

/// `GET /api/v1/health/cache`
async fn cache_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let stats = state.cache.stats();
    Json(serde_json::json!({
        "success": true,
        "hit_count": stats.hit_count,
        "miss_count": stats.miss_count,
        "hit_rate": stats.hit_rate,
        "evictions": stats.evictions,
        "entry_count": state.cache.entry_count(),
        "max_entries": state.cache.total_capacity(),
    }))
}

/// `GET /api/v1/status` — Version und tatsächlich registrierte Module.
async fn status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": "running",
        "modules": [
            {"name": "patients", "enabled": true},
            {"name": "doctors", "enabled": true},
            {"name": "appointments", "enabled": true},
            {"name": "medications", "enabled": true},
            {"name": "ui", "enabled": true},
            {"name": "hl7", "enabled": false},
            {"name": "fhir", "enabled": false},
            {"name": "dicom", "enabled": false},
            {"name": "xds", "enabled": false},
            {"name": "gdt", "enabled": false},
            {"name": "medico", "enabled": false},
            {"name": "ti", "enabled": false},
        ]
    }))
}

/// `GET /api/v1/audit/:entity_type/:entity_id` (Berechtigung `audit.read`).
async fn audit_trail_handler(
    State(state): State<AppState>,
    auth: RequireRole,
    Path((entity_type, entity_id)): Path<(String, String)>,
) -> Response {
    match state
        .security
        .get_audit_trail(state.db.write(), &entity_type, &entity_id)
        .await
    {
        Ok(trail) => {
            // Das Lesen des Audit-Trails ist selbst protokollpflichtig.
            if let Err(e) = state
                .security
                .log_audit(
                    state.db.write(),
                    auth.user_id(),
                    "audit.read",
                    &entity_type,
                    Some(&entity_id),
                    None,
                    None,
                    None,
                )
                .await
            {
                tracing::error!(error = %e, "Audit des Audit-Zugriffs fehlgeschlagen");
                return internal_error();
            }
            Json(serde_json::json!({
                "success": true,
                "entity_type": entity_type,
                "entity_id": entity_id,
                "entries": trail,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Audit-Trail-Abfrage fehlgeschlagen");
            internal_error()
        }
    }
}

// ===========================================================================
// Authentifizierung
// ===========================================================================

#[derive(Debug, serde::Deserialize)]
struct LoginRequest {
    /// Benutzername oder E-Mail (Feld `email` für die bestehende GUI).
    #[serde(alias = "email", alias = "username")]
    login: String,
    password: String,
    #[serde(default)]
    totp_code: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

fn auth_error_response(e: AuthError) -> Response {
    match e {
        AuthError::Internal(inner) => {
            tracing::error!(error = %inner, "Authentifizierung: interner Fehler");
            internal_error()
        }
        AuthError::MfaRequired => json_error(StatusCode::UNAUTHORIZED, "MFA-Code erforderlich"),
        // Gesperrt und falsche Daten bewusst gleich beantworten (keine
        // Konto-Aufzählung über unterschiedliche Antworten).
        AuthError::Locked | AuthError::InvalidCredentials => {
            json_error(StatusCode::UNAUTHORIZED, "Ungültige Anmeldedaten")
        }
        AuthError::InvalidToken => json_error(
            StatusCode::UNAUTHORIZED,
            "Ungültiger oder abgelaufener Token",
        ),
    }
}

fn token_response(pair: auth::TokenPair) -> Response {
    Json(serde_json::json!({
        "access_token": pair.access_token,
        "refresh_token": pair.refresh_token,
        "tokenType": pair.token_type,
        "expiresIn": pair.expires_in,
        "sessionId": pair.session_id,
        "user": { "id": pair.user_id, "role": pair.role },
    }))
    .into_response()
}

/// `POST /api/v1/auth/login`
async fn login_handler(State(state): State<AppState>, Json(req): Json<LoginRequest>) -> Response {
    match auth::login(
        state.db.write(),
        &state.security,
        req.login.trim(),
        &req.password,
        req.totp_code.as_deref(),
        None,
    )
    .await
    {
        Ok(pair) => token_response(pair),
        Err(e) => auth_error_response(e),
    }
}

/// `POST /api/v1/auth/refresh` — rotiert Refresh- und Access-Token.
async fn refresh_handler(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Response {
    match auth::refresh(state.db.write(), &state.security, &req.refresh_token, None).await {
        Ok(pair) => token_response(pair),
        Err(e) => auth_error_response(e),
    }
}

/// `POST /api/v1/auth/logout` — widerruft die aktuelle Sitzung.
async fn logout_handler(State(state): State<AppState>, auth_user: RequireRole) -> Response {
    let Some(sid) = auth_user.auth.session_id else {
        return json_error(StatusCode::BAD_REQUEST, "Token ist nicht sitzungsgebunden");
    };
    match auth::revoke_session(
        state.db.write(),
        &state.security,
        sid,
        &auth_user.auth,
        None,
    )
    .await
    {
        Ok(_) => {
            Json(serde_json::json!({ "success": true, "message": "Abgemeldet" })).into_response()
        }
        Err(e) => auth_error_response(e),
    }
}

/// `GET /api/v1/auth/sessions` — eigene aktive Sitzungen.
async fn auth_sessions_handler(State(state): State<AppState>, auth_user: RequireRole) -> Response {
    let Ok(uid) = Uuid::parse_str(auth_user.user_id()) else {
        return internal_error();
    };
    match auth::list_sessions(state.db.write(), uid).await {
        Ok(list) => Json(serde_json::json!({
            "success": true,
            "count": list.len(),
            "sessions": list,
        }))
        .into_response(),
        Err(e) => auth_error_response(e),
    }
}

/// `POST /api/v1/auth/sessions/:id/revoke` — eigene Sitzung beenden;
/// Admins dürfen beliebige Sitzungen widerrufen.
async fn auth_session_revoke_handler(
    State(state): State<AppState>,
    auth_user: RequireRole,
    Path(id): Path<String>,
) -> Response {
    let sid = match parse_uuid(&id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let owner = if auth_user.can(&Permission::Admin) {
        None
    } else {
        match Uuid::parse_str(auth_user.user_id()) {
            Ok(u) => Some(u),
            Err(_) => return internal_error(),
        }
    };
    match auth::revoke_session(
        state.db.write(),
        &state.security,
        sid,
        &auth_user.auth,
        owner,
    )
    .await
    {
        Ok(true) => Json(serde_json::json!({ "success": true })).into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "Sitzung nicht gefunden"),
        Err(e) => auth_error_response(e),
    }
}

// ===========================================================================
// DSGVO-Handler
// ===========================================================================

fn deletion_error(e: DeletionError) -> Response {
    match e {
        DeletionError::NotFound(_) => json_error(StatusCode::NOT_FOUND, "Patient nicht gefunden"),
        DeletionError::Permission(_) => {
            json_error(StatusCode::FORBIDDEN, "Keine Berechtigung für diese Aktion")
        }
        DeletionError::AlreadyDeleted(..) => {
            json_error(StatusCode::CONFLICT, "Patient ist bereits gelöscht")
        }
        DeletionError::ConfirmationRequired => json_error(
            StatusCode::BAD_REQUEST,
            "Bestätigung erforderlich: \"confirmation\" muss den Familiennamen enthalten",
        ),
        other => {
            tracing::error!(error = %other, "Löschvorgang fehlgeschlagen");
            internal_error()
        }
    }
}

fn anonymization_error(e: AnonymizationError) -> Response {
    match e {
        AnonymizationError::NotFound(_) => {
            json_error(StatusCode::NOT_FOUND, "Patient nicht gefunden")
        }
        AnonymizationError::Permission(_) => {
            json_error(StatusCode::FORBIDDEN, "Keine Berechtigung für diese Aktion")
        }
        AnonymizationError::AlreadyAnonymized(_) => {
            json_error(StatusCode::CONFLICT, "Patient ist bereits anonymisiert")
        }
        other => {
            tracing::error!(error = %other, "Anonymisierung fehlgeschlagen");
            internal_error()
        }
    }
}

fn export_error(e: ExportError) -> Response {
    match e {
        ExportError::NotFound(_) => json_error(StatusCode::NOT_FOUND, "Patient nicht gefunden"),
        ExportError::Permission(_) => {
            json_error(StatusCode::FORBIDDEN, "Keine Berechtigung für diese Aktion")
        }
        ExportError::UnsupportedFormat(_) => json_error(
            StatusCode::BAD_REQUEST,
            "Format muss json, csv oder fhir sein",
        ),
        other => {
            tracing::error!(error = %other, "Export fehlgeschlagen");
            internal_error()
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct HardDeleteRequest {
    confirmation: String,
}

/// `POST /api/v1/patients/:id/hard-delete` — nur Admin, mit Namensbestätigung.
async fn patient_hard_delete_handler(
    State(state): State<AppState>,
    auth_user: RequireRole,
    Path(id): Path<String>,
    Json(body): Json<HardDeleteRequest>,
) -> Response {
    let patient_id = match parse_uuid(&id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match state
        .deletion_service
        .hard_delete_patient(patient_id, &auth_user.auth, &body.confirmation)
        .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => deletion_error(e),
    }
}

/// `POST /api/v1/admin/deletion/check` — listet fällige Patienten (nur Admin).
async fn deletion_check_handler(State(state): State<AppState>, _auth: RequireRole) -> Response {
    match state.deletion_service.retention_check().await {
        Ok(candidates) => Json(serde_json::json!({
            "success": true,
            "count": candidates.len(),
            "candidates": candidates,
        }))
        .into_response(),
        Err(e) => deletion_error(e),
    }
}

/// `GET /api/v1/patients/:id/export?format=json|csv|fhir`
async fn patient_export_handler(
    State(state): State<AppState>,
    auth_user: RequireRole,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let patient_id = match parse_uuid(&id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let format_str = params.get("format").map(|s| s.as_str()).unwrap_or("json");
    let Some(format) = ExportFormat::parse(format_str) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Format muss json, csv oder fhir sein",
        );
    };

    match state
        .portability_service
        .export(patient_id, format, &auth_user.auth)
        .await
    {
        Ok(data) => {
            let (content_type, ext) = match format {
                ExportFormat::Json => ("application/json", "json"),
                ExportFormat::Csv => ("text/csv; charset=utf-8", "csv"),
                ExportFormat::Fhir => ("application/fhir+json", "fhir.json"),
            };
            let body = match (&format, &data) {
                (ExportFormat::Csv, serde_json::Value::String(csv)) => csv.clone(),
                _ => serde_json::to_string_pretty(&data).unwrap_or_default(),
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"patient_{patient_id}.{ext}\""),
                )
                .body(axum::body::Body::from(body))
                .unwrap_or_else(|_| internal_error())
        }
        Err(e) => export_error(e),
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct AnonymizeRequest {
    /// `anonymize` (irreversibel, Standard) oder `pseudonymize` (reversibel)
    #[serde(default)]
    mode: Option<String>,
}

/// `POST /api/v1/admin/anonymization/patients/:id` (nur Admin)
async fn anonymize_patient_handler(
    State(state): State<AppState>,
    auth_user: RequireRole,
    Path(id): Path<String>,
    body: Option<Json<AnonymizeRequest>>,
) -> Response {
    let patient_id = match parse_uuid(&id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let mode = body
        .and_then(|Json(b)| b.mode)
        .unwrap_or_else(|| "anonymize".to_string());
    let svc = &state.anonymization_service;
    match mode.as_str() {
        "anonymize" => match svc.anonymize_patient(patient_id, &auth_user.auth).await {
            Ok(()) => {
                Json(serde_json::json!({ "success": true, "mode": "anonymize" })).into_response()
            }
            Err(e) => anonymization_error(e),
        },
        "pseudonymize" => match svc.pseudonymize_patient(patient_id, &auth_user.auth).await {
            Ok(pseudonym) => Json(serde_json::json!({
                "success": true,
                "mode": "pseudonymize",
                "pseudonym": pseudonym,
            }))
            .into_response(),
            Err(e) => anonymization_error(e),
        },
        _ => json_error(
            StatusCode::BAD_REQUEST,
            "mode muss anonymize oder pseudonymize sein",
        ),
    }
}

/// `POST /api/v1/admin/anonymization/check` — fristbasierte Anonymisierung.
async fn anonymization_check_handler(
    State(state): State<AppState>,
    auth_user: RequireRole,
) -> Response {
    match state
        .anonymization_service
        .retention_anonymize(&auth_user.auth)
        .await
    {
        Ok(result) => {
            Json(serde_json::json!({ "success": true, "result": result })).into_response()
        }
        Err(e) => anonymization_error(e),
    }
}

/// `GET /api/v1/admin/anonymization/log?limit=N`
async fn anonymization_log_handler(
    State(state): State<AppState>,
    _auth: RequireRole,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let limit: u32 = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
        .clamp(1, 1000);
    match state.anonymization_service.anonymization_log(limit).await {
        Ok(entries) => Json(serde_json::json!({
            "success": true,
            "count": entries.len(),
            "entries": entries,
        }))
        .into_response(),
        Err(e) => anonymization_error(e),
    }
}
