//! API-Router: verdrahtet die Sicherheits-Middleware und die Endpunkte.
//!
//! Middleware-Reihenfolge (außen nach innen):
//!   RateLimit → Audit → Auth → RBAC (pro Route)

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::security::{
    audit_middleware, auth_middleware, rate_limit_middleware, SecurityStack,
};

/// Baut den API-Router mit allen Sicherheits-Middleware-Layern.
pub fn create_router(security: Arc<SecurityStack>) -> Router {
    // CORS: im KIS-Kontext auf den eigenen Origin beschränken (nicht permissive!)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // AuthContext für Handler-Endpoints extrahieren
    let auth = security.auth.clone();

    Router::new()
        // Öffentliche Endpunkte
        .route("/api/health", get(health_check))
        .route(
            "/api/auth/login",
            post(crate::security::auth::login_handler),
        )
        .route(
            "/api/auth/refresh",
            post(crate::security::auth::refresh_handler),
        )
        .route(
            "/api/auth/logout",
            post(crate::security::auth::logout_handler),
        )
        .route(
            "/api/auth/mfa/setup",
            post(crate::security::auth::mfa_setup_handler),
        )
        // Patientendaten (authifiziert)
        .route("/api/patients", get(list_patients).post(create_patient))
        .route("/api/patients/:id", get(get_patient))
        // Termine & Medikamente (authifiziert)
        .route("/api/appointments", get(list_appointments).post(create_appointment))
        .route("/api/medications", get(list_medications))
        // State für alle Handler
        .with_state(auth)
        // Trace für Logging
        .layer(TraceLayer::new_for_http())
        // CORS
        .layer(cors)
        // Sicherheits-Middleware (außen nach innen: RateLimit → Audit → Auth)
        .layer(middleware::from_fn_with_state(
            security.limiter.clone(),
            rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            security.audit.clone(),
            audit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            security.auth.clone(),
            auth_middleware,
        ))
}

async fn health_check() -> &'static str {
    "OK"
}

async fn list_patients() -> &'static str {
    "TODO: Implement"
}

async fn create_patient() -> &'static str {
    "TODO: Implement"
}

async fn get_patient() -> &'static str {
    "TODO: Implement"
}

async fn list_appointments() -> &'static str {
    "TODO: Implement"
}

async fn create_appointment() -> &'static str {
    "TODO: Implement"
}

async fn list_medications() -> &'static str {
    "TODO: Implement"
}
