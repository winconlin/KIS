//! KIS Medications — Medikations-Service (Phase 1)
//!
//! Vollständige CRUD-Implementierung mit:
//! - Optimistic Locking (version-Feld)
//! - Soft-Delete (deleted_at)
//! - Filter nach Patient-ID
//!
//! Endpunkte:
//!   GET    /api/v1/medications              — Liste (Filter: patient_id, name, active)
//!   GET    /api/v1/medications/:id          — Einzelmittel
//!   POST   /api/v1/medications              — Anlegen
//!   PUT    /api/v1/medications/:id          — Aktualisieren (Optimistic Locking)
//!   DELETE /api/v1/medications/:id          — Soft-Delete
//!   GET    /api/v1/medications/:patient_id  — Alle Medikationen eines Patienten

#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use kis_core::security::{append_audit, RequireRole};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

// ===========================================================================
// Modelle
// ===========================================================================

/// Medikament — vollständiges DB-Modell
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Medication {
    pub id: Uuid,
    pub patient_id: Uuid,
    /// Bezeichnung des Medikaments
    pub name: String,
    /// Dosierung (z.B. "100 mg")
    pub dosage: Option<String>,
    /// Einnahmefrequenz (z.B. "2x täglich", "bei Bedarf")
    pub frequency: Option<String>,
    /// Beginn der Medikation
    pub start_date: chrono::NaiveDate,
    /// Ende der Medikation (Optional)
    pub end_date: Option<chrono::NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub version: i32,
}

/// Medikament mit Patient-Namen
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicationDetail {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub patient_name: String,
    pub name: String,
    pub dosage: Option<String>,
    pub frequency: Option<String>,
    pub start_date: chrono::NaiveDate,
    pub end_date: Option<chrono::NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

/// Medikament anlegen
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMedication {
    pub patient_id: Uuid,
    pub name: String,
    pub dosage: Option<String>,
    pub frequency: Option<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
}

/// Medikament aktualisieren
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMedication {
    /// PFLICHT: Aktuelle Version (Optimistic Locking)
    pub version: i32,
    pub patient_id: Option<Uuid>,
    pub name: Option<String>,
    pub dosage: Option<String>,
    pub frequency: Option<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
}

/// Listenfilter
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMedicationsQuery {
    /// Filter nach Patient-ID
    pub patient_id: Option<Uuid>,
    /// Suchbegriff (Name)
    pub name: Option<String>,
    /// Nur aktive Medikamente (Default: true)
    pub active: Option<bool>,
    /// Cursor-Pagination
    pub cursor: Option<Uuid>,
    pub limit: Option<i64>,
}

/// Paginierte Medikamenten-Liste
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicationPage {
    pub items: Vec<Medication>,
    pub next_cursor: Option<Uuid>,
    pub count: usize,
}

// ===========================================================================
// Fehler-Typen
// ===========================================================================

#[derive(Debug, thiserror::Error)]
pub enum MedicationError {
    #[error("Medikament nicht gefunden: {0}")]
    NotFound(Uuid),
    #[error("Versionenkonflikt (Optimistic Locking): erwartet {expected}, aber {actual}")]
    VersionConflict { expected: i32, actual: i32 },
    #[error("Patient nicht gefunden: {0}")]
    PatientNotFound(Uuid),
    #[error("Ungültige Datumsangabe: start_date muss vor end_date liegen")]
    InvalidDateRange,
    #[error("Datenbank-Fehler: {0}")]
    Db(String),
}

impl From<&MedicationError> for axum::http::StatusCode {
    fn from(e: &MedicationError) -> Self {
        match e {
            MedicationError::NotFound(_) | MedicationError::PatientNotFound(_) => {
                axum::http::StatusCode::NOT_FOUND
            }
            MedicationError::VersionConflict { .. } => axum::http::StatusCode::CONFLICT,
            MedicationError::InvalidDateRange => axum::http::StatusCode::BAD_REQUEST,
            MedicationError::Db(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub success: bool,
    pub error: String,
}

impl From<MedicationError> for axum::response::Response {
    fn from(e: MedicationError) -> Self {
        let status: axum::http::StatusCode = (&e).into();
        axum::response::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&ErrorBody {
                    success: false,
                    error: public_message(&e),
                })
                .unwrap_or_else(|_| "{}".into()),
            ))
            .unwrap_or_else(|_| {
                axum::response::Response::builder()
                    .status(500)
                    .body(axum::body::Body::empty())
                    .unwrap()
            })
    }
}

// ===========================================================================
// Router
// ===========================================================================

/// Registriert den Medikations-Router
pub fn register_router(db: Arc<PgPool>) -> Router {
    Router::new()
        .route(
            "/api/v1/medications",
            get(list_medications).post(create_medication),
        )
        // Eindeutiger Pfad: `/medications/:id` und `/medications/:patient_id`
        // kollidierten (Panic beim Start).
        .route(
            "/api/v1/medications/patient/:patient_id",
            get(medications_by_patient),
        )
        .route(
            "/api/v1/medications/:id",
            get(get_medication)
                .put(update_medication)
                .delete(delete_medication),
        )
        .with_state(db)
}

// ===========================================================================
// Handler
// ===========================================================================

/// GET /api/v1/medications — Liste mit Filter und Cursor-Pagination
async fn list_medications(
    State(db): State<Arc<PgPool>>,
    Query(params): Query<ListMedicationsQuery>,
) -> Result<Json<MedicationPage>, axum::response::Response> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let active = params.active.unwrap_or(true);

    let mut sql = String::from("SELECT * FROM medications");
    let mut conds: Vec<String> = Vec::new();
    let mut idx = 0;

    if active {
        conds.push("deleted_at IS NULL".into());
    }
    if let Some(ref pid) = params.patient_id {
        idx += 1;
        conds.push(format!("patient_id = ${idx}"));
        let _ = pid;
    }
    if let Some(ref name) = params.name {
        idx += 1;
        conds.push(format!("name ILIKE ${idx}"));
        let _ = name;
    }
    if let Some(ref cursor) = params.cursor {
        conds.push(format!("id > '{}'", cursor));
    }

    if !conds.is_empty() {
        sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));
    }
    sql.push_str(" ORDER BY created_at DESC, id ASC LIMIT ");
    sql.push_str(&limit.to_string());

    let pool = db.as_ref();

    // Da wir dynamische Bindings haben, bauen wir die Query mit Parametern
    // Für patient_id und name nutzen wir Prepared Statements
    let mut query = sqlx::query_as::<_, Medication>(&sql);

    // Bindings in der richtigen Reihenfolge
    if params.patient_id.is_some() {
        if let Some(pid) = params.patient_id {
            query = query.bind(pid);
        }
    }
    if params.name.is_some() {
        if let Some(ref n) = params.name {
            query = query.bind(format!("%{}%", n));
        }
    }

    let rows: Vec<Medication> = query
        .fetch_all(pool)
        .await
        .map_err(|e| MedicationError::Db(e.to_string()))?;

    let next_cursor = rows.last().map(|m| m.id);
    Ok(Json(MedicationPage {
        count: rows.len(),
        items: rows,
        next_cursor,
    }))
}

/// GET /api/v1/medications/:id — Einzelmittel
async fn get_medication(
    State(db): State<Arc<PgPool>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Medication>, axum::response::Response> {
    let pool = db.as_ref();
    let med: Option<Medication> =
        sqlx::query_as("SELECT * FROM medications WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| MedicationError::Db(e.to_string()))?;

    match med {
        Some(m) => Ok(Json(m)),
        None => Err(axum::response::Response::from(MedicationError::NotFound(
            id,
        ))),
    }
}

/// POST /api/v1/medications — Medikament anlegen
async fn create_medication(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Json(body): Json<CreateMedication>,
) -> Result<Json<Medication>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| MedicationError::Db(e.to_string()))?;

    // Patient existiert?
    let p: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM patients WHERE id = $1 AND deleted_at IS NULL")
            .bind(body.patient_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| MedicationError::Db(e.to_string()))?;

    if p.is_none() {
        return Err(axum::response::Response::from(
            MedicationError::PatientNotFound(body.patient_id),
        ));
    }

    // Datums-Validierung
    let start = body.start_date.unwrap_or_else(|| Utc::now().date_naive());
    if let Some(end) = body.end_date {
        if end < start {
            return Err(axum::response::Response::from(
                MedicationError::InvalidDateRange,
            ));
        }
    }

    let med: Medication = sqlx::query_as(
        r#"
        INSERT INTO medications (
            patient_id, name, dosage, frequency,
            start_date, end_date, created_at, updated_at, version
        ) VALUES (
            $1, $2, $3, $4, $5, $6, now(), now(), 1
        )
        RETURNING *
        "#,
    )
    .bind(body.patient_id)
    .bind(&body.name)
    .bind(&body.dosage)
    .bind(&body.frequency)
    .bind(start)
    .bind(body.end_date)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| MedicationError::Db(e.to_string()))?;

    info!(medication_id = %med.id, patient_id = %med.patient_id, "Medikament angelegt");
    audit(&mut tx, &auth, "medication.create", "medication", med.id).await?;
    tx.commit()
        .await
        .map_err(|e| MedicationError::Db(e.to_string()))?;
    Ok(Json(med))
}

/// PUT /api/v1/medications/:id — Medikament aktualisieren (Optimistic Locking)
async fn update_medication(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateMedication>,
) -> Result<Json<Medication>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| MedicationError::Db(e.to_string()))?;

    let current: Option<(i32,)> =
        sqlx::query_as("SELECT version FROM medications WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| MedicationError::Db(e.to_string()))?;

    let current_version = match current {
        Some((v,)) => v,
        None => {
            return Err(axum::response::Response::from(MedicationError::NotFound(
                id,
            )))
        }
    };

    if current_version != body.version {
        warn!(medication_id = %id, expected = body.version, actual = current_version, "Versionenkonflikt");
        return Err(axum::response::Response::from(
            MedicationError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        ));
    }

    // Datums-Validierung bei Änderung
    if let Some(ref new_start) = body.start_date {
        if let Some(ref new_end) = body.end_date {
            if new_end < new_start {
                return Err(axum::response::Response::from(
                    MedicationError::InvalidDateRange,
                ));
            }
        }
    }

    let new_version = body.version + 1;
    let med: Option<Medication> = sqlx::query_as(
        r#"
        UPDATE medications SET
            patient_id = COALESCE($2, patient_id),
            name       = COALESCE($3, name),
            dosage     = COALESCE($4, dosage),
            frequency  = COALESCE($5, frequency),
            start_date = COALESCE($6, start_date),
            end_date   = COALESCE($7, end_date),
            updated_at = now(),
            version    = $8
        WHERE id = $1 AND version = $9 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(body.patient_id)
    .bind(&body.name)
    .bind(&body.dosage)
    .bind(&body.frequency)
    .bind(body.start_date)
    .bind(body.end_date)
    .bind(new_version)
    .bind(body.version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| MedicationError::Db(e.to_string()))?;

    match med {
        Some(m) => {
            info!(medication_id = %m.id, new_version = m.version, "Medikament aktualisiert");
            audit(&mut tx, &auth, "medication.update", "medication", m.id).await?;
            tx.commit()
                .await
                .map_err(|e| MedicationError::Db(e.to_string()))?;
            Ok(Json(m))
        }
        None => Err(axum::response::Response::from(
            MedicationError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        )),
    }
}

/// DELETE /api/v1/medications/:id — Soft-Delete
async fn delete_medication(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| MedicationError::Db(e.to_string()))?;
    let result = sqlx::query(
        r#"
        UPDATE medications
        SET deleted_at = now(), updated_at = now(), version = version + 1
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING id
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| MedicationError::Db(e.to_string()))?;

    match result {
        Some(_) => {
            info!(medication_id = %id, "Medikament soft-gelöscht");
            audit(&mut tx, &auth, "medication.delete", "medication", id).await?;
            tx.commit()
                .await
                .map_err(|e| MedicationError::Db(e.to_string()))?;
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Medikament soft-gelöscht",
                "id": id.to_string(),
            })))
        }
        None => Err(axum::response::Response::from(MedicationError::NotFound(
            id,
        ))),
    }
}

/// GET /api/v1/medications/patient/:patient_id — Alle Medikationen eines Patienten
async fn medications_by_patient(
    State(db): State<Arc<PgPool>>,
    Path(patient_id): Path<Uuid>,
) -> Result<Json<Vec<MedicationDetail>>, axum::response::Response> {
    let pool = db.as_ref();

    // Patient existiert?
    let p: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM patients WHERE id = $1 AND deleted_at IS NULL")
            .bind(patient_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| MedicationError::Db(e.to_string()))?;

    if p.is_none() {
        return Err(axum::response::Response::from(
            MedicationError::PatientNotFound(patient_id),
        ));
    }

    let rows: Vec<(
        Uuid,
        String,
        Uuid,
        String,
        Option<String>,
        Option<String>,
        chrono::NaiveDate,
        Option<chrono::NaiveDate>,
        DateTime<Utc>,
        DateTime<Utc>,
        i32,
    )> = sqlx::query_as(
        r#"
            SELECT m.id,
                   (p.name_family || ' ' || p.name_given) AS patient_name,
                   m.patient_id, m.name, m.dosage, m.frequency,
                   m.start_date, m.end_date,
                   m.created_at, m.updated_at, m.version
            FROM medications m
            JOIN patients p ON p.id = m.patient_id
            WHERE m.patient_id = $1
              AND m.deleted_at IS NULL
            ORDER BY m.start_date DESC, m.created_at DESC
            "#,
    )
    .bind(patient_id)
    .fetch_all(pool)
    .await
    .map_err(|e| MedicationError::Db(e.to_string()))?;

    let meds: Vec<MedicationDetail> = rows
        .into_iter()
        .map(
            |(
                id,
                patient_name,
                pid,
                name,
                dosage,
                frequency,
                start,
                end,
                created,
                updated,
                version,
            )| {
                MedicationDetail {
                    id,
                    patient_id: pid,
                    patient_name,
                    name,
                    dosage,
                    frequency,
                    start_date: start,
                    end_date: end,
                    created_at: created,
                    updated_at: updated,
                    version,
                }
            },
        )
        .collect();

    Ok(Json(meds))
}

// ===========================================================================
// Modul-Status
// ===========================================================================

#[derive(Debug, Clone, Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

pub const MODULE_NAME: &str = "kis-medications";
pub const MODULE_DESCRIPTION: &str =
    "KIS Medikations-Service — Phase 1: CRUD, Optimistic Locking, Patient-Filter";

/// Schreibt den Audit-Eintrag einer Änderung in die laufende Transaktion.
/// Schlägt das Audit fehl, wird die Änderung verworfen.
async fn audit(
    tx: &mut sqlx::PgConnection,
    auth: &RequireRole,
    action: &str,
    entity: &str,
    id: Uuid,
) -> Result<(), MedicationError> {
    append_audit(
        tx,
        auth.user_id(),
        action,
        entity,
        Some(&id.to_string()),
        None,
        None,
        None,
    )
    .await
    .map_err(|e| MedicationError::Db(e.to_string()))
}

/// Fehlermeldung für den Client: interne DB-Details nur ins Log.
fn public_message(e: &MedicationError) -> String {
    match e {
        MedicationError::Db(detail) => {
            tracing::error!(%detail, "medications: DB-Fehler");
            "Interner Serverfehler".to_string()
        }
        other => other.to_string(),
    }
}
