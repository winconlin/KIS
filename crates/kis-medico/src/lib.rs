//! KIS medico — selbstständiges Modul im KIS-Workspace.
//!
//! Modularer Aufbau: Diese Crate kann unabhängig entwickelt, getestet
//! und deaktivierte werden. Neues Modul = neue Crate + Feature-Flag.

use serde::Serialize;

/// Modul-Status für das Dashboard
#[derive(Debug, Clone, Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

/// Registriert den Modul-Router
pub fn register_router() -> axum::Router {
    axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
}

pub const MODULE_NAME: &str = "kis-medico";
pub const MODULE_DESCRIPTION: &str = "kis-medico — Phase 1/3/4";
