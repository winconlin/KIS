use axum::{
    response::Html,
    routing::get,
    Router,
};

pub fn create_ui_router() -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("../../static/index.html")) }))
        .route("/css/:file", get(|| async { "TODO" }))
        .route("/js/:file", get(|| async { "TODO" }))
}
