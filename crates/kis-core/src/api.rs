//! API-Router: Baut den Axum-Router mit allen aktivierten Modulen.
//!
//! Modularer Aufbau: Jedes Modul registriert seinen eigenen Router.
//! Neues Modul = 1 #[cfg(feature)]-Block hier.

use std::sync::Arc;
use axum::{routing::get, Router};

use super::config::AppConfig;
use super::db::Database;
use super::security::Security;

/// Baut den Haupt-Router
pub fn build_router(
    _config: AppConfig,
    _db: Arc<Database>,
    _security: Arc<Security>,
) -> Router {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/status", get(status));

    // --- Module werden hier registriert ---
    // Jedes Modul ist eine eigene Crate mit Feature-Flag.
    // Deaktivierte Module werden nicht kompiliert.

    // Patient-Service
    // #[cfg(feature = "patients")]
    // {
    //     app = app.merge(kis_patients::register_router());
    // }

    // Arzt-Service
    // #[cfg(feature = "doctors")]
    // {
    //     app = app.merge(kis_doctors::register_router());
    // }

    // Termin-Service
    // #[cfg(feature = "appointments")]
    // {
    //     app = app.merge(kis_appointments::register_router());
    // }

    // Medikament-Service
    // #[cfg(feature = "medications")]
    // {
    //     app = app.merge(kis_medications::register_router());
    // }

    // HL7 v2.5 Schnittstelle (TCP-Port, nicht HTTP)
    // #[cfg(feature = "hl7")]
    // {
    //     tokio::spawn(kis_hl7::start_server());
    // }

    // FHIR R4 Server (ISiK)
    // #[cfg(feature = "fhir")]
    // {
    //     app = app.nest("/fhir", kis_fhir::register_router());
    // }

    // DICOM SCP (TCP-Port 104, nicht HTTP)
    // #[cfg(feature = "dicom")]
    // {
    //     tokio::spawn(kis_dicom::start_server());
    // }

    // XDS
    // #[cfg(feature = "xds")]
    // {
    //     app = app.nest("/xds", kis_xds::register_router());
    // }

    // GDT
    // #[cfg(feature = "gdt")]
    // {
    //     app = app.nest("/gdt", kis_gdt::register_router());
    // }

    // CGM MEDICO
    // #[cfg(feature = "medico")]
    // {
    //     app = app.nest("/medico", kis_medico::register_router());
    // }

    // Telematikinfrastruktur
    // #[cfg(feature = "ti")]
    // {
    //     app = app.nest("/ti", kis_ti::register_router());
    // }

    // GUI
    // #[cfg(feature = "ui")]
    // {
    //     app = app.merge(kis_ui::register_router());
    // }

    app
}

async fn health_check() -> &'static str {
    "ok"
}

async fn status() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": "running",
        "modules": [
            {"name": "patients", "enabled": false},
            {"name": "doctors", "enabled": false},
            {"name": "appointments", "enabled": false},
            {"name": "medications", "enabled": false},
            {"name": "hl7", "enabled": false},
            {"name": "fhir", "enabled": false},
            {"name": "dicom", "enabled": false},
            {"name": "xds", "enabled": false},
            {"name": "gdt", "enabled": false},
            {"name": "medico", "enabled": false},
            {"name": "ti", "enabled": false},
            {"name": "ui", "enabled": false},
        ]
    })
}
