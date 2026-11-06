//! KIS UI — Auslieferung der Web-GUI (SPA) als statische Dateien.
//!
//! Authentifizierung (Login/Refresh/Logout) liegt zentral in `kis-app`
//! auf Basis von `kis_core::auth`. Dieses Crate liefert nur Dateien aus
//! dem Verzeichnis `static/` aus.
//!
//! Endpunkte:
//! - `GET /`                → `static/index.html`
//! - `GET /static/*path`    → Datei unter `static/`

use std::path::{Component, Path, PathBuf};

use axum::{
    body::Body,
    extract::Path as AxPath,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Serialize;

pub const MODULE_NAME: &str = "kis-ui";
pub const MODULE_DESCRIPTION: &str = "Web-GUI (SPA, statische Auslieferung)";

/// Wurzelverzeichnis der statischen Dateien (relativ zum Arbeitsverzeichnis,
/// überschreibbar mit `KIS_STATIC_DIR`).
fn static_root() -> PathBuf {
    std::env::var_os("KIS_STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("static"))
}

/// Registriert den UI-Router.
pub fn register_router() -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/static/*path", get(serve_static))
}

/// Content-Type anhand der Dateiendung.
pub fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// Liefert einen sicheren relativen Pfad oder `None` bei Traversal-Versuchen
/// (`..`, absolute Pfade, Laufwerks-/Root-Präfixe, NUL-Bytes).
pub fn sanitize_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains('\0') || path.contains('\\') {
        return None;
    }
    let p = Path::new(path);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

async fn file_response(path: PathBuf, content_type: &'static str) -> Response {
    match tokio::fs::read(&path).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, "no-cache, must-revalidate")
            .body(Body::from(data))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(_) => (StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    }
}

async fn serve_index() -> Response {
    file_response(static_root().join("index.html"), "text/html; charset=utf-8").await
}

async fn serve_static(AxPath(path): AxPath<String>) -> Response {
    match sanitize_relative(&path) {
        Some(rel) => file_response(static_root().join(rel), content_type_for(&path)).await,
        None => (StatusCode::BAD_REQUEST, "Ungültiger Pfad").into_response(),
    }
}

/// Modul-Status für das Dashboard
#[derive(Debug, Clone, Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

impl ModuleStatus {
    pub fn new(enabled: bool) -> Self {
        Self {
            name: MODULE_NAME.to_string(),
            enabled,
            description: MODULE_DESCRIPTION.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types() {
        assert_eq!(content_type_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type_for("js/app.js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(content_type_for("x.unknown"), "application/octet-stream");
    }

    #[test]
    fn traversal_is_rejected() {
        for p in [
            "../etc/passwd",
            "css/../../secret",
            "/etc/passwd",
            "..",
            "",
            "a\\..\\b",
            "a\0b",
        ] {
            assert!(sanitize_relative(p).is_none(), "nicht abgewiesen: {p:?}");
        }
    }

    #[test]
    fn normal_paths_are_accepted() {
        assert_eq!(
            sanitize_relative("css/style.css"),
            Some(PathBuf::from("css/style.css"))
        );
        assert_eq!(
            sanitize_relative("./js/app.js"),
            Some(PathBuf::from("js/app.js"))
        );
    }

    #[tokio::test]
    async fn serves_existing_file_and_404() {
        let dir = std::env::temp_dir().join(format!("kis-ui-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("css")).unwrap();
        std::fs::write(dir.join("css/a.css"), "body{}").unwrap();
        let res = file_response(dir.join("css/a.css"), "text/css; charset=utf-8").await;
        assert_eq!(res.status(), StatusCode::OK);
        let res = file_response(dir.join("missing.css"), "text/css; charset=utf-8").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn module_status() {
        let status = ModuleStatus::new(true);
        assert_eq!(status.name, MODULE_NAME);
        assert!(status.enabled);
    }
}
