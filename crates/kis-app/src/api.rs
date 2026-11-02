//! API-Router: Baut den Axum-Router mit allen aktivierten Modulen.
//!
//! Modularer Aufbau: Jedes Modul registriert seinen eigenen Router.
//! Alle Module erhalten den selben `Arc<Database>` (PostgreSQL-Pool),
//! damit sie auf demselben Write-/Read-Pool arbeiten.

use std::sync::Arc;
use axum::{routing::get, Json, Router};
use tower_http::trace::TraceLayer;

use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::security::Security;

/// Baut den Haupt-Router
pub fn build_router(
    _config: AppConfig,
    db: Arc<Database>,
    _security: Arc<Security>,
) -> Router {
    // Grund-Router: Health-Check + Status
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/status", get(status));

    // --- Phase 1: Kernmodule (CRUD + Optimistic Locking) ---

    // Patient-Service: /api/v1/patients
    let pool = db.write_pool.clone();
    let app = app.merge(kis_patients::register_router(std::sync::Arc::new(pool)));

    // Arzt-Service: /api/v1/doctors
    let pool = db.write_pool.clone();
    let app = app.merge(kis_doctors::register_router(std::sync::Arc::new(pool)));

    // Termin-Service: /api/v1/appointments (inkl. Doppelbuchungsprüfung, ISiK, Kalender)
    let pool = db.write_pool.clone();
    let app = app.merge(kis_appointments::register_router(std::sync::Arc::new(pool)));

    // Medikament-Service: /api/v1/medications
    let pool = db.write_pool.clone();
    let app = app.merge(kis_medications::register_router(std::sync::Arc::new(pool)));

    // --- Phase 2–3: Schnittstellen & Integrationen (noch Stubs) ---
    // Diese Module werden in späteren Phasen aktiviert:
    //   HL7 v2.5 (TCP), FHIR R4, DICOM SCP, XDS, GDT, MEDICO, Telematik, GUI

    // Trace-Layer: loggt jeden Request (Method, Path, Status, Latenz)
    app.layer(TraceLayer::new_for_http())
}

/// Health-Check: Liefert "ok", wenn der Server läuft.
async fn health_check() -> &'static str {
    "ok"
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
