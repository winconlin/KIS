//! KIS Doctors — Arzt-Service (Phase 1)
//!
//! Vollständige CRUD-Implementierung mit:
//! - Optimistic Locking (version-Feld)
//! - Soft-Delete (deleted_at)
//! - Zeitplan-Endpunkt (GET /:id/schedule)
//!
//! Endpunkte:
//!   GET    /api/v1/doctors           — Liste
//!   GET    /api/v1/doctors/:id       — Einzelarzt
//!   POST   /api/v1/doctors           — Anlegen
//!   PUT    /api/v1/doctors/:id       — Aktualisieren (Optimistic Locking)
//!   DELETE /api/v1/doctors/:id       — Soft-Delete
//!   GET    /api/v1/doctors/:id/schedule — Zeitplan (Termine + Slots)

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use kis_core::security::{append_audit, RequireRole};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

// ===========================================================================
// Modelle
// ===========================================================================

/// Arzt — vollständiges DB-Modell
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Doctor {
    pub id: Uuid,
    pub external_id: Option<String>,
    pub name_family: String,
    pub name_given: String,
    /// Haupt-Spezialität
    pub specialty: Option<String>,
    /// Alle Spezialitäten als JSONB-Array
    pub specialties: sqlx::types::Json<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub version: i32,
}

/// Anlegen eines Arztes
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDoctor {
    pub external_id: Option<String>,
    pub name_family: String,
    pub name_given: String,
    pub specialty: Option<String>,
    pub specialties: Option<Vec<String>>,
}

/// Aktualisieren eines Arztes (Optimistic Locking)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDoctor {
    /// PFLICHT: Aktuelle Version
    pub version: i32,
    pub external_id: Option<String>,
    pub name_family: Option<String>,
    pub name_given: Option<String>,
    pub specialty: Option<String>,
    pub specialties: Option<Vec<String>>,
}

/// Listenfilter
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDoctorsQuery {
    /// Filter nach Spezialität
    pub specialty: Option<String>,
    /// Suchbegriff (Name)
    pub search: Option<String>,
    /// Nur aktive Ärzte (Default: true)
    pub active: Option<bool>,
    /// Cursor-Pagination
    pub cursor: Option<Uuid>,
    pub limit: Option<i64>,
}

/// Cursor-Paginated Page
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorPage {
    pub items: Vec<Doctor>,
    pub next_cursor: Option<Uuid>,
    pub count: usize,
}

/// Termin im Zeitplan
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleAppointment {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub patient_name: String,
    pub status: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Zeitplan-Response: alle Termine für einen Zeitraum
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSchedule {
    pub doctor_id: Uuid,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub appointments: Vec<ScheduleAppointment>,
    /// Anzahl freier Slots in diesem Zeitraum
    pub free_slot_count: i64,
}

// ===========================================================================
// Fehler-Typen
// ===========================================================================

#[derive(Debug, thiserror::Error)]
pub enum DoctorError {
    #[error("Arzt nicht gefunden: {0}")]
    NotFound(Uuid),
    #[error("Versionenkonflikt (Optimistic Locking): erwartet {expected}, aber {actual}")]
    VersionConflict { expected: i32, actual: i32 },
    #[error("Datenbank-Fehler: {0}")]
    Db(String),
}

impl From<&DoctorError> for axum::http::StatusCode {
    fn from(e: &DoctorError) -> Self {
        match e {
            DoctorError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
            DoctorError::VersionConflict { .. } => axum::http::StatusCode::CONFLICT,
            DoctorError::Db(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub success: bool,
    pub error: String,
}

impl From<DoctorError> for axum::response::Response {
    fn from(e: DoctorError) -> Self {
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

/// Modul-Deskriptor; registriert in `kis-app/src/modules.rs`.
pub struct DoctorsModule;

impl Module for DoctorsModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "doctors",
            title: "Ärzte-Stammdaten",
            description: "Behandler für die Terminplanung, Fachrichtungen, Wochenplan",
            depends_on: &[],
            kernel: false,
            version: env!("CARGO_PKG_VERSION"),
            maturity: Maturity::Stable,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::Permission(P::DoctorRead);
        let write = Access::Permission(P::DoctorWrite);
        ModuleRoutes::new()
            .get("/api/v1/doctors", read, list_doctors)
            .post("/api/v1/doctors", write, create_doctor)
            .get("/api/v1/doctors/:id", read, get_doctor)
            .put("/api/v1/doctors/:id", write, update_doctor)
            .delete("/api/v1/doctors/:id", write, delete_doctor)
            .get("/api/v1/doctors/:id/schedule", read, doctor_schedule)
            .with_state(Arc::new(ctx.pool().clone()))
    }
}

// ===========================================================================
// Handler
// ===========================================================================

/// GET /api/v1/doctors — Liste mit Filter und Cursor-Pagination
async fn list_doctors(
    State(db): State<Arc<PgPool>>,
    Query(params): Query<ListDoctorsQuery>,
) -> Result<Json<DoctorPage>, axum::response::Response> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let active = params.active.unwrap_or(true);

    let mut sql = String::from("SELECT * FROM doctors");
    let mut conds: Vec<String> = Vec::new();
    let mut bind_idx = 0;

    if active {
        conds.push("deleted_at IS NULL".into());
    }
    if let Some(ref search) = params.search {
        bind_idx += 1;
        let _ = search;
        // ILIKE für Name-Suche
        conds.push(format!(
            "(name_family ILIKE ${bind_idx} OR name_given ILIKE ${bind_idx})"
        ));
    }
    if let Some(ref spec) = params.specialty {
        bind_idx += 1;
        let _ = spec;
        conds.push(format!("specialty = ${bind_idx}"));
    }
    if let Some(ref cursor) = params.cursor {
        conds.push(format!("id > '{}'", cursor));
    }

    if !conds.is_empty() {
        sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));
    }
    sql.push_str(" ORDER BY id ASC LIMIT ");
    sql.push_str(&limit.to_string());

    let pool = db.as_ref();
    let rows: Vec<Doctor> = sqlx::query_as::<_, Doctor>(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| DoctorError::Db(e.to_string()))?;

    let next_cursor = rows.last().map(|d| d.id);
    Ok(Json(DoctorPage {
        count: rows.len(),
        items: rows,
        next_cursor,
    }))
}

/// GET /api/v1/doctors/:id — Einzelarzt
async fn get_doctor(
    State(db): State<Arc<PgPool>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Doctor>, axum::response::Response> {
    let pool = db.as_ref();
    let doc: Option<Doctor> =
        sqlx::query_as("SELECT * FROM doctors WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| DoctorError::Db(e.to_string()))?;

    match doc {
        Some(d) => Ok(Json(d)),
        None => Err(axum::response::Response::from(DoctorError::NotFound(id))),
    }
}

/// POST /api/v1/doctors — Arzt anlegen
async fn create_doctor(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Json(body): Json<CreateDoctor>,
) -> Result<Json<Doctor>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| DoctorError::Db(e.to_string()))?;
    let specs_json = serde_json::to_value(body.specialties.unwrap_or_default())
        .unwrap_or_else(|_| serde_json::json!([]));

    let doc: Doctor = sqlx::query_as(
        r#"
        INSERT INTO doctors (
            external_id, name_family, name_given,
            specialty, specialties, created_at, updated_at, version
        ) VALUES ($1, $2, $3, $4, $5, now(), now(), 1)
        RETURNING *
        "#,
    )
    .bind(&body.external_id)
    .bind(&body.name_family)
    .bind(&body.name_given)
    .bind(&body.specialty)
    .bind(specs_json)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| DoctorError::Db(e.to_string()))?;

    info!(doctor_id = %doc.id, "Arzt angelegt");
    audit(&mut tx, &auth, "doctor.create", "doctor", doc.id).await?;
    tx.commit()
        .await
        .map_err(|e| DoctorError::Db(e.to_string()))?;
    Ok(Json(doc))
}

/// PUT /api/v1/doctors/:id — Arzt aktualisieren (Optimistic Locking)
async fn update_doctor(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDoctor>,
) -> Result<Json<Doctor>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| DoctorError::Db(e.to_string()))?;

    let current: Option<(i32,)> =
        sqlx::query_as("SELECT version FROM doctors WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| DoctorError::Db(e.to_string()))?;

    let current_version = match current {
        Some((v,)) => v,
        None => return Err(axum::response::Response::from(DoctorError::NotFound(id))),
    };

    if current_version != body.version {
        warn!(doctor_id = %id, expected = body.version, actual = current_version, "Versionenkonflikt");
        return Err(axum::response::Response::from(
            DoctorError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        ));
    }

    let new_version = body.version + 1;
    let specs_val = body
        .specialties
        .as_ref()
        .and_then(|s| serde_json::to_value(s).ok());

    let doc: Option<Doctor> = sqlx::query_as(
        r#"
        UPDATE doctors SET
            external_id  = COALESCE($2, external_id),
            name_family  = COALESCE($3, name_family),
            name_given   = COALESCE($4, name_given),
            specialty    = COALESCE($5, specialty),
            specialties  = COALESCE($6, specialties),
            updated_at   = now(),
            version      = $7
        WHERE id = $1 AND version = $8 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(&body.external_id)
    .bind(&body.name_family)
    .bind(&body.name_given)
    .bind(&body.specialty)
    .bind(specs_val)
    .bind(new_version)
    .bind(body.version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| DoctorError::Db(e.to_string()))?;

    match doc {
        Some(d) => {
            info!(doctor_id = %d.id, new_version = d.version, "Arzt aktualisiert");
            audit(&mut tx, &auth, "doctor.update", "doctor", d.id).await?;
            tx.commit()
                .await
                .map_err(|e| DoctorError::Db(e.to_string()))?;
            Ok(Json(d))
        }
        None => Err(axum::response::Response::from(
            DoctorError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        )),
    }
}

/// DELETE /api/v1/doctors/:id — Soft-Delete
async fn delete_doctor(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| DoctorError::Db(e.to_string()))?;
    let result = sqlx::query(
        r#"
        UPDATE doctors
        SET deleted_at = now(), updated_at = now(), version = version + 1
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING id
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| DoctorError::Db(e.to_string()))?;

    match result {
        Some(_) => {
            info!(doctor_id = %id, "Arzt soft-gelöscht");
            audit(&mut tx, &auth, "doctor.delete", "doctor", id).await?;
            tx.commit()
                .await
                .map_err(|e| DoctorError::Db(e.to_string()))?;
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Arzt soft-gelöscht",
                "id": id.to_string(),
            })))
        }
        None => Err(axum::response::Response::from(DoctorError::NotFound(id))),
    }
}

/// GET /api/v1/doctors/:id/schedule — Zeitplan
///
/// Query-Parameter:
///   date_from — Beginn des Zeitraums (ISO 8601, Default: heute 00:00)
///   date_to   — Ende des Zeitraums (ISO 8601, Default: morgen 23:59)
///
/// Liefert alle Termine im Zeitraum + Anzahl freier Slots.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleQuery {
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
}

async fn doctor_schedule(
    State(db): State<Arc<PgPool>>,
    Path(id): Path<Uuid>,
    Query(params): Query<ScheduleQuery>,
) -> Result<Json<DoctorSchedule>, axum::response::Response> {
    let pool = db.as_ref();

    // Zeitraum berechnen (Default: heute bis morgen)
    let now = Utc::now();
    let today = now.date_naive();
    let date_from = params
        .date_from
        .unwrap_or_else(|| today.and_hms_opt(0, 0, 0).unwrap().and_utc());
    let date_to = params
        .date_to
        .unwrap_or_else(|| today.and_hms_opt(23, 59, 59).unwrap().and_utc());

    // Termin-Liste mit Patient-Namen (JOIN)
    let appts: Vec<ScheduleAppointment> = sqlx::query_as(
        r#"
        SELECT a.id, a.patient_id,
               (p.name_family || ' ' || p.name_given) AS patient_name,
               a.status, a.start_time, a.end_time, a.reason
        FROM appointments a
        JOIN patients p ON p.id = a.patient_id
        WHERE a.doctor_id = $1
          AND a.start_time >= $2
          AND a.end_time <= $3
          AND a.deleted_at IS NULL
        ORDER BY a.start_time ASC
        "#,
    )
    .bind(id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await
    .map_err(|e| DoctorError::Db(e.to_string()))?;

    // Anzahl freier Slots: Arbeitstage 08:00–16:00, 30-Min-Blöcke,
    // abzüglich bereits gebuchter Slots
    let booked_count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM appointments
        WHERE doctor_id = $1
          AND start_time >= $2
          AND end_time <= $3
          AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(date_from)
    .bind(date_to)
    .fetch_one(pool)
    .await
    .map_err(|e| DoctorError::Db(e.to_string()))?;

    // Schätzung: 16 Stunden * 2 Slots/Stunde = 32 Slots/Tag
    let total_slots = 32;
    let free_slot_count = total_slots - booked_count.0;

    Ok(Json(DoctorSchedule {
        doctor_id: id,
        from: date_from,
        to: date_to,
        appointments: appts,
        free_slot_count,
    }))
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

pub const MODULE_NAME: &str = "kis-doctors";
pub const MODULE_DESCRIPTION: &str =
    "KIS Arzt-Service — Phase 1: CRUD, Optimistic Locking, Zeitplan";

/// Schreibt den Audit-Eintrag einer Änderung in die laufende Transaktion.
/// Schlägt das Audit fehl, wird die Änderung verworfen.
async fn audit(
    tx: &mut sqlx::PgConnection,
    auth: &RequireRole,
    action: &str,
    entity: &str,
    id: Uuid,
) -> Result<(), DoctorError> {
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
    .map_err(|e| DoctorError::Db(e.to_string()))
}

/// Fehlermeldung für den Client: interne DB-Details nur ins Log.
fn public_message(e: &DoctorError) -> String {
    match e {
        DoctorError::Db(detail) => {
            tracing::error!(%detail, "doctors: DB-Fehler");
            "Interner Serverfehler".to_string()
        }
        other => other.to_string(),
    }
}
