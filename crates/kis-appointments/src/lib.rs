//! KIS Appointments — Termin-Service (Phase 1)
//!
//! Vollständige Implementierung mit:
//! - CRUD + Optimistic Locking (version) + Soft-Delete (deleted_at)
//! - Doppelbuchungsprüfung (zeitliche Überlappung pro Arzt)
//! - ISiK-Integration (book/cancel — Placeholder für FHIR-Appointments)
//! - Kalender-Ansicht (GET /:doctor_id/calendar)
//! - Slot-Verfügbarkeit (GET /slots)
//!
//! Endpunkte:
//!   GET    /api/v1/appointments             — Liste (Filter: date_from, date_to, doctor_id, patient_id, status)
//!   GET    /api/v1/appointments/:id         — Einzeltermin
//!   POST   /api/v1/appointments             — Termin anlegen (mit Doppelbuchungsprüfung)
//!   PUT    /api/v1/appointments/:id         — Termin aktualisieren (Optimistic Locking + Doppelbuchungsprüfung)
//!   DELETE /api/v1/appointments/:id         — Soft-Delete
//!   POST   /api/v1/appointments/:id/book    — ISiK: Termin buchen
//!   POST   /api/v1/appointments/:id/cancel  — ISiK: Termin stornieren
//!   GET    /api/v1/appointments/:doctor_id/calendar — Kalender-Ansicht
//!   GET    /api/v1/appointments/slots       — Slot-Verfügbarkeit

#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use kis_core::security::{append_audit, RequireRole};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

// ===========================================================================
// Modelle
// ===========================================================================

/// Termin — vollständiges DB-Modell
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Appointment {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub doctor_id: Uuid,
    /// Kategorie: ordination, konsultation, kontrolle, etc.
    pub category: String,
    /// Status: geplant, bestaetigt, abgesagt, abgeschlossen
    pub status: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub reason: Option<String>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub version: i32,
}

/// Termin mit Patient- und Arzt-Namen (JOIN)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppointmentDetail {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub patient_name: String,
    pub doctor_id: Uuid,
    pub doctor_name: String,
    pub category: String,
    pub status: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub reason: Option<String>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

/// Termin anlegen
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAppointment {
    pub patient_id: Uuid,
    pub doctor_id: Uuid,
    pub category: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Termin aktualisieren
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppointment {
    /// PFLICHT: Aktuelle Version (Optimistic Locking)
    pub version: i32,
    pub patient_id: Option<Uuid>,
    pub doctor_id: Option<Uuid>,
    pub category: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub reason: Option<String>,
}

/// Listenfilter
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAppointmentsQuery {
    /// Beginn des Zeitraums
    pub date_from: Option<DateTime<Utc>>,
    /// Ende des Zeitraums
    pub date_to: Option<DateTime<Utc>>,
    pub doctor_id: Option<Uuid>,
    pub patient_id: Option<Uuid>,
    /// Status-Filter: geplant, bestaetigt, abgesagt, abgeschlossen
    pub status: Option<String>,
    /// Cursor-Pagination
    pub cursor: Option<Uuid>,
    pub limit: Option<i64>,
}

/// Paginierte Termin-Liste
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppointmentPage {
    pub items: Vec<Appointment>,
    pub next_cursor: Option<Uuid>,
    pub count: usize,
}

/// Slot für die Terminvergabe
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    /// Verfügbar?
    pub available: bool,
    /// Grund für Nicht-Verfügbarkeit (wenn nicht verfügbar)
    pub reason: Option<String>,
}

/// Slot-Response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotList {
    pub doctor_id: Uuid,
    pub date: chrono::NaiveDate,
    pub slots: Vec<Slot>,
    pub total: usize,
    pub available_count: usize,
}

/// Kalender-Eintrag
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEntry {
    pub id: Uuid,
    pub patient_id: Uuid,
    pub patient_name: String,
    pub category: String,
    pub status: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Kalender-Response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCalendar {
    pub doctor_id: Uuid,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub entries: Vec<CalendarEntry>,
}

// ===========================================================================
// Fehler-Typen
// ===========================================================================

#[derive(Debug, thiserror::Error)]
pub enum AppointmentError {
    #[error("Termin nicht gefunden: {0}")]
    NotFound(Uuid),
    #[error("Versionenkonflikt (Optimistic Locking): erwartet {expected}, aber {actual}")]
    VersionConflict { expected: i32, actual: i32 },
    #[error("Doppelbuchung: Zeitliche Überlappung mit Termin {existing_id} ({existing_start} – {existing_end})")]
    DoubleBooking {
        existing_id: Uuid,
        existing_start: String,
        existing_end: String,
    },
    #[error("Doppelbuchung: Der Arzt hat in diesem Zeitraum bereits einen Termin")]
    Overlap,
    #[error("Ungültiger Zeitraum: start_time muss vor end_time liegen")]
    InvalidTimeRange,
    #[error("Patient nicht gefunden: {0}")]
    PatientNotFound(Uuid),
    #[error("Arzt nicht gefunden: {0}")]
    DoctorNotFound(Uuid),
    #[error("Datenbank-Fehler: {0}")]
    Db(String),
}

impl From<&AppointmentError> for axum::http::StatusCode {
    fn from(e: &AppointmentError) -> Self {
        match e {
            AppointmentError::NotFound(_)
            | AppointmentError::PatientNotFound(_)
            | AppointmentError::DoctorNotFound(_) => axum::http::StatusCode::NOT_FOUND,
            AppointmentError::VersionConflict { .. } => axum::http::StatusCode::CONFLICT,
            AppointmentError::DoubleBooking { .. } | AppointmentError::Overlap => {
                axum::http::StatusCode::CONFLICT
            }
            AppointmentError::InvalidTimeRange => axum::http::StatusCode::BAD_REQUEST,
            AppointmentError::Db(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub success: bool,
    pub error: String,
}

impl From<AppointmentError> for axum::response::Response {
    fn from(e: AppointmentError) -> Self {
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
pub struct AppointmentsModule;

impl Module for AppointmentsModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "appointments",
            title: "Terminplanung",
            description: "Termine, freie Zeitfenster, Kalender je Behandler, Buchung und Absage",
            depends_on: &["patients", "doctors"],
            kernel: false,
            version: env!("CARGO_PKG_VERSION"),
            maturity: Maturity::Stable,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::Permission(P::AppointmentRead);
        let book = Access::Permission(P::AppointmentBook);
        let cancel = Access::Permission(P::AppointmentCancel);
        ModuleRoutes::new()
            .get("/api/v1/appointments", read, list_appointments)
            .post("/api/v1/appointments", book, create_appointment)
            .get("/api/v1/appointments/slots", read, list_slots)
            .get(
                "/api/v1/appointments/:doctor_id/calendar",
                read,
                doctor_calendar,
            )
            .get("/api/v1/appointments/:id", read, get_appointment)
            .put(
                "/api/v1/appointments/:id",
                Access::Permission(P::AppointmentWrite),
                update_appointment,
            )
            .delete("/api/v1/appointments/:id", cancel, delete_appointment)
            .post("/api/v1/appointments/:id/book", book, book_appointment)
            .post(
                "/api/v1/appointments/:id/cancel",
                cancel,
                cancel_appointment,
            )
            .with_state(Arc::new(ctx.pool().clone()))
    }
}

// ===========================================================================
// Handler
// ===========================================================================

/// GET /api/v1/appointments
///
/// Liste Termine mit Filtern und Cursor-Pagination.
async fn list_appointments(
    State(db): State<Arc<PgPool>>,
    Query(params): Query<ListAppointmentsQuery>,
) -> Result<Json<AppointmentPage>, axum::response::Response> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);

    // Dynamische WHERE-Bedingungen: alle Filter-Parameter sind typsicher
    // (DateTime, Uuid, String) — keine SQL-Injection möglich.
    let mut sql = String::from("SELECT * FROM appointments WHERE deleted_at IS NULL");
    let mut conds: Vec<String> = Vec::new();

    if params.date_from.is_some() {
        conds.push("start_time >= $1".to_string());
    }
    if params.date_to.is_some() {
        let offset = if params.date_from.is_some() { 1 } else { 0 };
        conds.push(format!("end_time <= ${}", offset + 1));
    }
    if params.doctor_id.is_some() {
        let mut offset = 0;
        if params.date_from.is_some() {
            offset += 1;
        }
        if params.date_to.is_some() {
            offset += 1;
        }
        conds.push(format!("doctor_id = ${}", offset + 1));
    }
    if params.patient_id.is_some() {
        let mut offset = 0;
        if params.date_from.is_some() {
            offset += 1;
        }
        if params.date_to.is_some() {
            offset += 1;
        }
        if params.doctor_id.is_some() {
            offset += 1;
        }
        conds.push(format!("patient_id = ${}", offset + 1));
    }
    if params.status.is_some() {
        let mut offset = 0;
        if params.date_from.is_some() {
            offset += 1;
        }
        if params.date_to.is_some() {
            offset += 1;
        }
        if params.doctor_id.is_some() {
            offset += 1;
        }
        if params.patient_id.is_some() {
            offset += 1;
        }
        conds.push(format!("status = ${}", offset + 1));
    }
    if let Some(ref cursor) = params.cursor {
        conds.push(format!("id > '{}'", cursor));
    }

    if !conds.is_empty() {
        sql.push_str(&format!(" AND {}", conds.join(" AND ")));
    }
    sql.push_str(" ORDER BY start_time ASC, id ASC LIMIT ");
    sql.push_str(&limit.to_string());

    let pool = db.as_ref();
    let mut query = sqlx::query_as::<_, Appointment>(&sql);
    if let Some(ref df) = params.date_from {
        query = query.bind(*df);
    }
    if let Some(ref dt) = params.date_to {
        query = query.bind(*dt);
    }
    if let Some(ref did) = params.doctor_id {
        query = query.bind(*did);
    }
    if let Some(ref pid) = params.patient_id {
        query = query.bind(*pid);
    }
    if let Some(ref status) = params.status {
        query = query.bind(status.clone());
    }
    let rows: Vec<Appointment> = query.fetch_all(pool).await.map_err(db_err)?;

    let next_cursor = rows.last().map(|a| a.id);
    Ok(Json(AppointmentPage {
        count: rows.len(),
        items: rows,
        next_cursor,
    }))
}

/// GET /api/v1/appointments/:id — Einzeltermin
async fn get_appointment(
    State(db): State<Arc<PgPool>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Appointment>, axum::response::Response> {
    let pool = db.as_ref();
    let appt: Option<Appointment> =
        sqlx::query_as("SELECT * FROM appointments WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;

    match appt {
        Some(a) => Ok(Json(a)),
        None => Err(axum::response::Response::from(AppointmentError::NotFound(
            id,
        ))),
    }
}

/// POST /api/v1/appointments — Termin anlegen
///
/// Mit Doppelbuchungsprüfung: Prüft, ob der Arzt bereits einen Termin
/// im selben Zeitfenster hat (zeitliche Überlappung).
async fn create_appointment(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Json(body): Json<CreateAppointment>,
) -> Result<Json<Appointment>, axum::response::Response> {
    let mut tx = db.begin().await.map_err(db_err)?;

    // 1. Zeitfenster validieren
    if body.start_time >= body.end_time {
        return Err(axum::response::Response::from(
            AppointmentError::InvalidTimeRange,
        ));
    }

    // 2. Patient und Arzt existieren?
    check_patient_doctor(&mut tx, body.patient_id, body.doctor_id).await?;

    // 3. Doppelbuchungsprüfung: zeitliche Überlappung
    //    Zwei Zeitintervalle [a1, a2] und [b1, b2] überlappen sich, wenn
    //    a1 < b2 AND b1 < a2.
    let overlap: Option<Appointment> = sqlx::query_as(
        r#"
        SELECT * FROM appointments
        WHERE doctor_id = $1
          AND start_time < $3
          AND end_time > $2
          AND status != 'abgesagt'
          AND deleted_at IS NULL
        "#,
    )
    .bind(body.doctor_id)
    .bind(body.start_time)
    .bind(body.end_time)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;

    if let Some(existing) = overlap {
        error!(
            doctor_id = %body.doctor_id,
            existing_id = %existing.id,
            "Doppelbuchung erkannt"
        );
        return Err(axum::response::Response::from(
            AppointmentError::DoubleBooking {
                existing_id: existing.id,
                existing_start: existing.start_time.to_rfc3339(),
                existing_end: existing.end_time.to_rfc3339(),
            },
        ));
    }

    // 4. Termin anlegen
    let appt: Appointment = sqlx::query_as(
        r#"
        INSERT INTO appointments (
            patient_id, doctor_id, category, status,
            start_time, end_time, reason,
            created_at, updated_at, version
        ) VALUES (
            $1, $2, $3, 'geplant',
            $4, $5, $6, now(), now(), 1
        )
        RETURNING *
        "#,
    )
    .bind(body.patient_id)
    .bind(body.doctor_id)
    .bind(body.category.unwrap_or_else(|| "ordination".to_string()))
    .bind(body.start_time)
    .bind(body.end_time)
    .bind(&body.reason)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;

    info!(appointment_id = %appt.id, doctor_id = %appt.doctor_id, "Termin angelegt");
    audit(&mut tx, &auth, "appointment.create", "appointment", appt.id).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(Json(appt))
}

/// PUT /api/v1/appointments/:id — Termin aktualisieren
///
/// Mit Optimistic Locking + Doppelbuchungsprüfung (nur bei Zeit-/Arzt-Änderung).
async fn update_appointment(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAppointment>,
) -> Result<Json<Appointment>, axum::response::Response> {
    let mut tx = db.begin().await.map_err(db_err)?;

    // 1. Aktuelle Version prüfen
    let current: Option<(i32,)> =
        sqlx::query_as("SELECT version FROM appointments WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;

    let current_version = match current {
        Some((v,)) => v,
        None => {
            return Err(axum::response::Response::from(AppointmentError::NotFound(
                id,
            )))
        }
    };

    if current_version != body.version {
        warn!(appointment_id = %id, expected = body.version, actual = current_version, "Versionenkonflikt");
        return Err(axum::response::Response::from(
            AppointmentError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        ));
    }

    // 2. Doppelbuchungsprüfung nur, wenn sich Zeit oder Arzt ändert
    if body.start_time.is_some() || body.end_time.is_some() || body.doctor_id.is_some() {
        // Aktuellen Termin laden, um die neuen Werte zu berechnen
        let existing: Appointment = sqlx::query_as("SELECT * FROM appointments WHERE id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;

        let new_start = body.start_time.unwrap_or(existing.start_time);
        let new_end = body.end_time.unwrap_or(existing.end_time);
        let new_doctor = body.doctor_id.unwrap_or(existing.doctor_id);

        if new_start >= new_end {
            return Err(axum::response::Response::from(
                AppointmentError::InvalidTimeRange,
            ));
        }

        let overlap: Option<Appointment> = sqlx::query_as(
            r#"
            SELECT * FROM appointments
            WHERE doctor_id = $1
              AND id != $2
              AND start_time < $4
              AND end_time > $3
              AND status != 'abgesagt'
              AND deleted_at IS NULL
            "#,
        )
        .bind(new_doctor)
        .bind(id)
        .bind(new_start)
        .bind(new_end)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;

        if let Some(ex) = overlap {
            return Err(axum::response::Response::from(
                AppointmentError::DoubleBooking {
                    existing_id: ex.id,
                    existing_start: ex.start_time.to_rfc3339(),
                    existing_end: ex.end_time.to_rfc3339(),
                },
            ));
        }
    }

    // 3. Update mit WHERE version = $expected (atomar)
    let new_version = body.version + 1;
    let appt: Option<Appointment> = sqlx::query_as(
        r#"
        UPDATE appointments SET
            patient_id = COALESCE($2, patient_id),
            doctor_id  = COALESCE($3, doctor_id),
            category   = COALESCE($4, category),
            start_time = COALESCE($5, start_time),
            end_time   = COALESCE($6, end_time),
            reason     = COALESCE($7, reason),
            updated_at = now(),
            version    = $8
        WHERE id = $1 AND version = $9 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(body.patient_id)
    .bind(body.doctor_id)
    .bind(&body.category)
    .bind(body.start_time)
    .bind(body.end_time)
    .bind(&body.reason)
    .bind(new_version)
    .bind(body.version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;

    match appt {
        Some(a) => {
            info!(appointment_id = %a.id, new_version = a.version, "Termin aktualisiert");
            audit(&mut tx, &auth, "appointment.update", "appointment", a.id).await?;
            tx.commit().await.map_err(db_err)?;
            Ok(Json(a))
        }
        None => Err(axum::response::Response::from(
            AppointmentError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        )),
    }
}

/// DELETE /api/v1/appointments/:id — Soft-Delete
async fn delete_appointment(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, axum::response::Response> {
    let mut tx = db.begin().await.map_err(db_err)?;
    let result = sqlx::query(
        r#"
        UPDATE appointments
        SET deleted_at = now(), updated_at = now(), version = version + 1
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING id
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;

    match result {
        Some(_) => {
            info!(appointment_id = %id, "Termin soft-gelöscht");
            audit(&mut tx, &auth, "appointment.delete", "appointment", id).await?;
            tx.commit().await.map_err(db_err)?;
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Termin soft-gelöscht",
                "id": id.to_string(),
            })))
        }
        None => Err(axum::response::Response::from(AppointmentError::NotFound(
            id,
        ))),
    }
}

/// POST /api/v1/appointments/:id/book — ISiK: Termin buchen
///
/// Setzt den Status von 'geplant' auf 'bestaetigt'.
/// (ISiK-Integration: In der Produktion wird hier zusätzlich
///  ein FHIR-Appointment-Resource erstellt/synchronisiert.)
async fn book_appointment(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
) -> Result<Json<Appointment>, axum::response::Response> {
    let mut tx = db.begin().await.map_err(db_err)?;

    let appt: Option<Appointment> = sqlx::query_as(
        r#"
        UPDATE appointments
        SET status = 'bestaetigt', updated_at = now(), version = version + 1
        WHERE id = $1
          AND status = 'geplant'
          AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;

    match appt {
        Some(a) => {
            info!(appointment_id = %a.id, "Termin via ISiK gebucht");
            audit(&mut tx, &auth, "appointment.book", "appointment", a.id).await?;
            tx.commit().await.map_err(db_err)?;
            Ok(Json(a))
        }
        None => Err(axum::response::Response::from(AppointmentError::NotFound(
            id,
        ))),
    }
}

/// POST /api/v1/appointments/:id/cancel — ISiK: Termin stornieren
///
/// Setzt den Status auf 'abgesagt' und setzt cancelled_at.
async fn cancel_appointment(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
) -> Result<Json<Appointment>, axum::response::Response> {
    let mut tx = db.begin().await.map_err(db_err)?;

    let appt: Option<Appointment> = sqlx::query_as(
        r#"
        UPDATE appointments
        SET status = 'abgesagt',
            cancelled_at = now(),
            updated_at = now(),
            version = version + 1
        WHERE id = $1
          AND status != 'abgesagt'
          AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;

    match appt {
        Some(a) => {
            info!(appointment_id = %a.id, "Termin via ISiK storniert");
            audit(&mut tx, &auth, "appointment.cancel", "appointment", a.id).await?;
            tx.commit().await.map_err(db_err)?;
            Ok(Json(a))
        }
        None => Err(axum::response::Response::from(AppointmentError::NotFound(
            id,
        ))),
    }
}

/// GET /api/v1/appointments/:doctor_id/calendar — Kalender-Ansicht
///
/// Query-Parameter:
///   date_from — Beginn (ISO 8601, Default: heute 00:00)
///   date_to   — Ende (ISO 8601, Default: heute 23:59)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarQuery {
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
}

async fn doctor_calendar(
    State(db): State<Arc<PgPool>>,
    Path(doctor_id): Path<Uuid>,
    Query(params): Query<CalendarQuery>,
) -> Result<Json<DoctorCalendar>, axum::response::Response> {
    let pool = db.as_ref();
    let now = Utc::now();
    let today = now.date_naive();
    let date_from = params
        .date_from
        .unwrap_or_else(|| today.and_hms_opt(0, 0, 0).unwrap().and_utc());
    let date_to = params
        .date_to
        .unwrap_or_else(|| today.and_hms_opt(23, 59, 59).unwrap().and_utc());

    let rows: Vec<(
        Uuid,
        Uuid,
        String,
        String,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
            SELECT a.id,
                   a.patient_id,
                   (p.name_family || ' ' || p.name_given) AS patient_name,
                   a.category, a.status,
                   a.start_time, a.end_time, a.reason
            FROM appointments a
            JOIN patients p ON p.id = a.patient_id
            WHERE a.doctor_id = $1
              AND a.start_time >= $2
              AND a.end_time <= $3
              AND a.deleted_at IS NULL
            ORDER BY a.start_time ASC
            "#,
    )
    .bind(doctor_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let entries: Vec<CalendarEntry> = rows
        .into_iter()
        .map(
            |(id, patient_id, patient_name, category, status, start, end, reason)| CalendarEntry {
                id,
                patient_id,
                patient_name,
                category,
                status,
                start_time: start,
                end_time: end,
                reason,
            },
        )
        .collect();

    Ok(Json(DoctorCalendar {
        doctor_id,
        from: date_from,
        to: date_to,
        entries,
    }))
}

/// GET /api/v1/appointments/slots — Slot-Verfügbarkeit
///
/// Query-Parameter:
///   doctor_id — PFLICHT: Arzt-ID
///   date      — Datum (YYYY-MM-DD, Default: heute)
///
/// Erzeugt 30-Minuten-Blöcke von 08:00 bis 16:00 und prüft,
/// ob für jeden Slot ein Termin vorliegt.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlotsQuery {
    pub doctor_id: Uuid,
    pub date: Option<chrono::NaiveDate>,
}

async fn list_slots(
    State(db): State<Arc<PgPool>>,
    Query(params): Query<SlotsQuery>,
) -> Result<Json<SlotList>, axum::response::Response> {
    let pool = db.as_ref();
    let date = params.date.unwrap_or_else(|| Utc::now().date_naive());

    // Arbeitstage: 08:00–16:00, 30-Min-Blöcke
    let mut slots: Vec<Slot> = Vec::new();
    for hour in 8..16 {
        for minute in [0, 30] {
            let start = chrono::NaiveDateTime::new(
                date,
                chrono::NaiveTime::from_hms_opt(hour, minute, 0).unwrap(),
            )
            .and_utc();
            let end = start + chrono::Duration::minutes(30);

            // Prüfen, ob ein Termin diesen Slot überlappt
            let booked: (bool,) = sqlx::query_as(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM appointments
                    WHERE doctor_id = $1
                      AND start_time < $4
                      AND end_time > $3
                      AND status != 'abgesagt'
                      AND deleted_at IS NULL
                )
                "#,
            )
            .bind(params.doctor_id)
            .bind(start)
            .bind(end)
            .bind(end)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;

            slots.push(Slot {
                start_time: start,
                end_time: end,
                available: !booked.0,
                reason: if booked.0 {
                    Some("Termin belegt".to_string())
                } else {
                    None
                },
            });
        }
    }

    let available_count = slots.iter().filter(|s| s.available).count();

    Ok(Json(SlotList {
        doctor_id: params.doctor_id,
        date,
        total: slots.len(),
        available_count,
        slots,
    }))
}

// ===========================================================================
// Hilfsfunktionen
// ===========================================================================

/// Prüft, ob Patient und Arzt existieren (und nicht soft-gelöscht sind).
async fn check_patient_doctor(
    pool: &mut sqlx::PgConnection,
    patient_id: Uuid,
    doctor_id: Uuid,
) -> Result<(), AppointmentError> {
    let p: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM patients WHERE id = $1 AND deleted_at IS NULL")
            .bind(patient_id)
            .fetch_optional(&mut *pool)
            .await
            .map_err(db_err)?;

    if p.is_none() {
        return Err(AppointmentError::PatientNotFound(patient_id));
    }

    let d: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM doctors WHERE id = $1 AND deleted_at IS NULL")
            .bind(doctor_id)
            .fetch_optional(&mut *pool)
            .await
            .map_err(db_err)?;

    if d.is_none() {
        return Err(AppointmentError::DoctorNotFound(doctor_id));
    }

    Ok(())
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

pub const MODULE_NAME: &str = "kis-appointments";
pub const MODULE_DESCRIPTION: &str =
    "KIS Termin-Service — Phase 1: CRUD, Doppelbuchungsprüfung, ISiK, Kalender, Slots";

/// Schreibt den Audit-Eintrag einer Änderung in die laufende Transaktion.
/// Schlägt das Audit fehl, wird die Änderung verworfen.
async fn audit(
    tx: &mut sqlx::PgConnection,
    auth: &RequireRole,
    action: &str,
    entity: &str,
    id: Uuid,
) -> Result<(), AppointmentError> {
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
    .map_err(|e| AppointmentError::Db(e.to_string()))
}

/// Fehlermeldung für den Client: interne DB-Details nur ins Log.
fn public_message(e: &AppointmentError) -> String {
    match e {
        AppointmentError::Db(detail) => {
            tracing::error!(%detail, "appointments: DB-Fehler");
            "Interner Serverfehler".to_string()
        }
        other => other.to_string(),
    }
}

/// Wandelt DB-Fehler um. Die Exclusion-Constraint `appointments_no_overlap`
/// (Migration 004) verhindert Doppelbuchungen auch bei parallelen Anfragen;
/// ihre Verletzung (SQLSTATE 23P01) wird als 409 gemeldet.
fn db_err(e: sqlx::Error) -> AppointmentError {
    if let Some(db) = e.as_database_error() {
        if db.code().as_deref() == Some("23P01") {
            return AppointmentError::Overlap;
        }
    }
    AppointmentError::Db(e.to_string())
}
