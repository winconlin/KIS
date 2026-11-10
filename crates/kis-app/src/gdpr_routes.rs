//! Datenschutz-Funktionen: Auskunft/Export (Art. 15/20), Löschung
//! (Art. 17), Anonymisierung und Einsicht in den Audit-Trail.
//!
//! Die Fachlogik liegt in `kis_core::{data_portability, data_deletion,
//! anonymization}`; hier stehen nur Routen, Zugriffsregeln und die
//! Übersetzung der Fehler in Statuscodes.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Json;
use uuid::Uuid;

use kis_core::anonymization::AnonymizationError;
use kis_core::data_deletion::DeletionError;
use kis_core::data_portability::{ExportError, ExportFormat};
use kis_core::http::ApiFailure;
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, ModuleRoutes};
use kis_core::rbac::Permission as P;

use crate::state::AppState;

pub fn routes() -> ModuleRoutes<AppState> {
    let admin = Access::Permission(P::Admin);
    ModuleRoutes::new()
        .get(
            "/api/v1/audit/:entity_type/:entity_id",
            Access::Permission(P::AuditRead),
            audit_trail,
        )
        .get(
            "/api/v1/patients/:id/export",
            Access::Permission(P::Export),
            export_patient,
        )
        .post("/api/v1/patients/:id/hard-delete", admin, hard_delete)
        .post("/api/v1/admin/deletion/check", admin, deletion_check)
        .post(
            "/api/v1/admin/anonymization/patients/:id",
            admin,
            anonymize_patient,
        )
        .post(
            "/api/v1/admin/anonymization/check",
            admin,
            anonymization_check,
        )
        .get("/api/v1/admin/anonymization/log", admin, anonymization_log)
}

fn deletion_error(e: DeletionError) -> ApiFailure {
    match e {
        DeletionError::NotFound(_) => ApiFailure::NotFound("Patient nicht gefunden"),
        DeletionError::Permission(_) => {
            ApiFailure::forbidden("Keine Berechtigung für diese Aktion")
        }
        DeletionError::AlreadyDeleted(..) => ApiFailure::conflict("Patient ist bereits gelöscht"),
        DeletionError::ConfirmationRequired => ApiFailure::bad_request(
            "Bestätigung erforderlich: \"confirmation\" muss den Familiennamen enthalten",
        ),
        other => ApiFailure::internal(other),
    }
}

fn anonymization_error(e: AnonymizationError) -> ApiFailure {
    match e {
        AnonymizationError::NotFound(_) => ApiFailure::NotFound("Patient nicht gefunden"),
        AnonymizationError::Permission(_) => {
            ApiFailure::forbidden("Keine Berechtigung für diese Aktion")
        }
        AnonymizationError::AlreadyAnonymized(_) => {
            ApiFailure::conflict("Patient ist bereits anonymisiert")
        }
        other => ApiFailure::internal(other),
    }
}

fn export_error(e: ExportError) -> ApiFailure {
    match e {
        ExportError::NotFound(_) => ApiFailure::NotFound("Patient nicht gefunden"),
        ExportError::Permission(_) => ApiFailure::forbidden("Keine Berechtigung für diese Aktion"),
        ExportError::UnsupportedFormat(_) => {
            ApiFailure::bad_request("Format muss json, csv oder fhir sein")
        }
        other => ApiFailure::internal(other),
    }
}

/// Audit-Trail eines Datensatzes. Das Lesen wird selbst protokolliert.
async fn audit_trail(
    State(st): State<AppState>,
    user: CurrentUser,
    Path((entity_type, entity_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiFailure> {
    let trail = st
        .ctx
        .security
        .get_audit_trail(st.pool(), &entity_type, &entity_id)
        .await?;
    st.ctx
        .security
        .log_audit(
            st.pool(),
            user.user_id(),
            "audit.read",
            &entity_type,
            Some(&entity_id),
            None,
            None,
            None,
        )
        .await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "entity_type": entity_type,
        "entity_id": entity_id,
        "entries": trail,
    })))
}

#[derive(serde::Deserialize)]
struct ExportQuery {
    format: Option<String>,
}

/// `GET /api/v1/patients/:id/export?format=json|csv|fhir`
async fn export_patient(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(patient_id): Path<Uuid>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiFailure> {
    let format = ExportFormat::parse(q.format.as_deref().unwrap_or("json"))
        .ok_or_else(|| ApiFailure::bad_request("Format muss json, csv oder fhir sein"))?;
    let data = st
        .portability
        .export(patient_id, format, &user.auth)
        .await
        .map_err(export_error)?;
    let (content_type, ext) = match format {
        ExportFormat::Json => ("application/json", "json"),
        ExportFormat::Csv => ("text/csv; charset=utf-8", "csv"),
        ExportFormat::Fhir => ("application/fhir+json", "fhir.json"),
    };
    let body = match (&format, &data) {
        (ExportFormat::Csv, serde_json::Value::String(csv)) => csv.clone(),
        _ => serde_json::to_string_pretty(&data).map_err(ApiFailure::internal)?,
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"patient_{patient_id}.{ext}\""),
        )
        .body(axum::body::Body::from(body))
        .map_err(ApiFailure::internal)
}

#[derive(serde::Deserialize)]
struct HardDeleteRequest {
    confirmation: String,
}

/// Endgültige Löschung mit Bestätigung durch den Familiennamen.
async fn hard_delete(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(patient_id): Path<Uuid>,
    Json(body): Json<HardDeleteRequest>,
) -> Result<Json<serde_json::Value>, ApiFailure> {
    st.deletion
        .hard_delete_patient(patient_id, &user.auth, &body.confirmation)
        .await
        .map_err(deletion_error)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Listet Patienten, deren Aufbewahrungsfrist abgelaufen ist.
async fn deletion_check(State(st): State<AppState>) -> Result<Json<serde_json::Value>, ApiFailure> {
    let candidates = st
        .deletion
        .retention_check()
        .await
        .map_err(deletion_error)?;
    Ok(Json(serde_json::json!({
        "success": true,
        "count": candidates.len(),
        "candidates": candidates,
    })))
}

#[derive(Default, serde::Deserialize)]
struct AnonymizeRequest {
    /// `anonymize` (irreversibel, Standard) oder `pseudonymize` (reversibel)
    #[serde(default)]
    mode: Option<String>,
}

async fn anonymize_patient(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(patient_id): Path<Uuid>,
    body: Option<Json<AnonymizeRequest>>,
) -> Result<Json<serde_json::Value>, ApiFailure> {
    let mode = body
        .and_then(|Json(b)| b.mode)
        .unwrap_or_else(|| "anonymize".to_string());
    match mode.as_str() {
        "anonymize" => {
            st.anonymization
                .anonymize_patient(patient_id, &user.auth)
                .await
                .map_err(anonymization_error)?;
            Ok(Json(
                serde_json::json!({ "success": true, "mode": "anonymize" }),
            ))
        }
        "pseudonymize" => {
            let pseudonym = st
                .anonymization
                .pseudonymize_patient(patient_id, &user.auth)
                .await
                .map_err(anonymization_error)?;
            Ok(Json(serde_json::json!({
                "success": true,
                "mode": "pseudonymize",
                "pseudonym": pseudonym,
            })))
        }
        _ => Err(ApiFailure::bad_request(
            "mode muss anonymize oder pseudonymize sein",
        )),
    }
}

/// Fristbasierte Anonymisierung aller fälligen Patienten.
async fn anonymization_check(
    State(st): State<AppState>,
    user: CurrentUser,
) -> Result<Json<serde_json::Value>, ApiFailure> {
    let result = st
        .anonymization
        .retention_anonymize(&user.auth)
        .await
        .map_err(anonymization_error)?;
    Ok(Json(
        serde_json::json!({ "success": true, "result": result }),
    ))
}

#[derive(serde::Deserialize)]
struct LogQuery {
    limit: Option<u32>,
}

async fn anonymization_log(
    State(st): State<AppState>,
    Query(q): Query<LogQuery>,
) -> Result<Json<serde_json::Value>, ApiFailure> {
    let entries = st
        .anonymization
        .anonymization_log(q.limit.unwrap_or(100).clamp(1, 1000))
        .await
        .map_err(anonymization_error)?;
    Ok(Json(serde_json::json!({
        "success": true,
        "count": entries.len(),
        "entries": entries,
    })))
}
