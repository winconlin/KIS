use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;

pub async fn create_router() -> Router {
    Router::new()
        .route("/api/health", get(health_check))
        .route("/api/patients", get(list_patients).post(create_patient))
        .route("/api/patients/:id", get(get_patient))
        .route("/api/appointments", get(list_appointments).post(create_appointment))
        .layer(CorsLayer::permissive())
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
