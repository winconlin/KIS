//! Router mit genau einem /api-Präfix.
//! Außen nach innen: Trace → RateLimit → Auth → RBAC → Audit → Handler.
use crate::security::{self, SecurityStack};
use axum::{
    http::StatusCode,
    middleware,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

pub fn create_router(stack: Arc<SecurityStack>) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/auth/login", post(security::auth::login_handler))
        .route("/api/auth/refresh", post(security::auth::refresh_handler))
        .route("/api/auth/logout", post(security::auth::logout_handler))
        .route(
            "/api/auth/mfa/setup",
            post(security::auth::mfa_setup_handler),
        )
        .route("/api/patients", get(not_implemented).post(not_implemented))
        .route("/api/patients/:id", get(not_implemented))
        .route(
            "/api/appointments",
            get(not_implemented).post(not_implemented),
        )
        .route("/api/medications", get(not_implemented))
        .with_state(stack.auth.clone())
        .route_layer(middleware::from_fn_with_state(
            stack.audit.clone(),
            security::audit_middleware,
        ))
        .route_layer(middleware::from_fn(security::rbac_middleware))
        .route_layer(middleware::from_fn_with_state(
            stack.auth.clone(),
            security::auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            stack.limiter.clone(),
            security::rate_limit_middleware,
        ))
        .layer(TraceLayer::new_for_http())
}

async fn not_implemented() -> axum::response::Response {
    security::json_error(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "Dieser Datenendpunkt ist noch nicht implementiert",
    )
}
