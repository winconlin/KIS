//! KIS Patients — Patient-Service (Phase 1)
//!
//! Vollständige CRUD-Implementierung mit:
//! - Optimistic Locking (version-Feld)
//! - Soft-Delete (deleted_at)
//! - Cursor-Pagination (keyset-based)
//! - Audit-Log (GET /:id/history)
//!
//! Endpunkte:
//!   GET    /api/v1/patients            — Liste (Filter + Cursor-Pagination)
//!   GET    /api/v1/patients/:id        — Einzelpatient
//!   POST   /api/v1/patients            — Anlegen
//!   PUT    /api/v1/patients/:id        — Aktualisieren (Optimistic Locking)
//!   DELETE /api/v1/patients/:id        — Soft-Delete
//!   GET    /api/v1/patients/:id/history — Audit-Log

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
use tracing::{error, info, warn};
use uuid::Uuid;

// ===========================================================================
// Modelle
// ===========================================================================

/// Patient — vollständiges DB-Modell (entspricht der PostgreSQL-Tabelle)
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Patient {
    pub id: Uuid,
    pub external_id: Option<String>,
    pub name_family: String,
    pub name_given: String,
    pub birth_date: chrono::NaiveDate,
    pub gender: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address_street: Option<String>,
    pub address_zip: Option<String>,
    pub address_city: Option<String>,
    pub insurance_number: Option<String>,
    /// Allergien als JSONB-Array, z.B. ["Penizillin", "Nuss"]
    pub allergies: sqlx::types::Json<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Soft-Delete: NULL = aktiv, TIMESTAMP = gelöscht
    pub deleted_at: Option<DateTime<Utc>>,
    /// Optimistic Locking: muss bei jedem Update übergeben werden
    pub version: i32,
}

/// Anlegen eines Patienten (POST /patients)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePatient {
    pub external_id: Option<String>,
    pub name_family: String,
    pub name_given: String,
    pub birth_date: chrono::NaiveDate,
    pub gender: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address_street: Option<String>,
    pub address_zip: Option<String>,
    pub address_city: Option<String>,
    pub insurance_number: Option<String>,
    /// Allergien als Array, z.B. ["Penizillin"]
    pub allergies: Option<Vec<String>>,
}

/// Aktualisieren eines Patienten (PUT /patients/:id)
/// — Alle Felder optional; nur übergebene Felder werden aktualisiert.
/// — `version` ist PFLICHT (Optimistic Locking).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePatient {
    /// PFLICHT: Aktuelle Version (Optimistic Locking)
    pub version: i32,
    pub external_id: Option<String>,
    pub name_family: Option<String>,
    pub name_given: Option<String>,
    pub birth_date: Option<chrono::NaiveDate>,
    pub gender: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address_street: Option<String>,
    pub address_zip: Option<String>,
    pub address_city: Option<String>,
    pub insurance_number: Option<String>,
    pub allergies: Option<Vec<String>>,
}

/// Listenfilter + Cursor-Pagination (GET /patients)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPatientsQuery {
    /// Suchbegriff (Name, external_id)
    pub search: Option<String>,
    /// Filter: nur aktiven Patienten (Default: true)
    pub active: Option<bool>,
    /// Cursor-Pagination: letzter ID der vorherigen Seite
    pub cursor: Option<Uuid>,
    /// Seiten-Größe (Default: 50, Max: 200)
    pub limit: Option<i64>,
}

/// Cursor-basiertes Paginierungs-Response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientPage {
    pub items: Vec<Patient>,
    /// Cursor für die nächste Seite (letzter ID)
    pub next_cursor: Option<Uuid>,
    /// Anzahl der gelieferten Items
    pub count: usize,
}

/// Audit-Eintrag (GET /patients/:id/history)
#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub user_id: Option<String>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub old_data: Option<serde_json::Value>,
    pub new_data: Option<serde_json::Value>,
    pub ip_address: Option<String>,
}

/// API-Antwort-Wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
}

// ===========================================================================
// Fehler-Typen
// ===========================================================================

/// Patient-Service-Fehler
#[derive(Debug, thiserror::Error)]
pub enum PatientError {
    #[error("Patient nicht gefunden: {0}")]
    NotFound(Uuid),
    #[error("Versionenkonflikt (Optimistic Locking): erwartete Version {expected}, aber {actual}")]
    VersionConflict { expected: i32, actual: i32 },
    #[error("Datenbank-Fehler: {0}")]
    Db(String),
}

impl From<&PatientError> for axum::http::StatusCode {
    fn from(e: &PatientError) -> Self {
        match e {
            PatientError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
            PatientError::VersionConflict { .. } => axum::http::StatusCode::CONFLICT,
            PatientError::Db(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Fehler-Antwort-Struktur
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub success: bool,
    pub error: String,
}

impl From<PatientError> for axum::response::Response {
    fn from(e: PatientError) -> Self {
        let status: axum::http::StatusCode = (&e).into();
        axum::response::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&ErrorBody {
                    success: false,
                    // Interne Details (SQL etc.) nur ins Log, nicht zum Client
                    error: match &e {
                        PatientError::Db(detail) => {
                            error!(%detail, "Patient-Service: DB-Fehler");
                            "Interner Serverfehler".to_string()
                        }
                        other => other.to_string(),
                    },
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
// Router-Registrierung
// ===========================================================================

/// Registriert den Patient-Router unter /api/v1/patients
///
/// `db` wird als `State` an alle Handler weitergegeben.
pub fn register_router(db: Arc<PgPool>) -> Router {
    Router::new()
        .route("/api/v1/patients", get(list_patients).post(create_patient))
        .route(
            "/api/v1/patients/:id",
            get(get_patient).put(update_patient).delete(delete_patient),
        )
        .route("/api/v1/patients/:id/history", get(patient_history))
        .with_state(db)
}

// ===========================================================================
// Handler-Implementierungen
// ===========================================================================

/// GET /api/v1/patients
///
/// Liste Patienten mit Filter und Cursor-Pagination.
///
/// Query-Parameter:
///   search    — Suchbegriff (Name, external_id)
///   active    — nur aktive (Default: true)
///   cursor    — letzter ID der vorherigen Seite (Uuid)
///   limit     — Seiten-Größe (Default: 50, Max: 200)
async fn list_patients(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Query(params): Query<ListPatientsQuery>,
) -> Result<Json<PatientPage>, axum::response::Response> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    // Gelöschte (gesperrte) Datensätze sieht nur, wer löschen darf.
    let may_see_deleted = auth.can(&kis_core::security::Permission::PatientDelete);
    let active = params.active.unwrap_or(true) || !may_see_deleted;

    // Cursor-basierte Paginierung (keyset-based, performant auch bei großen Tabellen)
    let mut query = String::from("SELECT * FROM patients");
    let mut conditions: Vec<String> = Vec::new();

    if active {
        conditions.push("deleted_at IS NULL".to_string());
    }
    if params.search.is_some() {
        conditions.push(
            "(name_family ILIKE $1 OR name_given ILIKE $1 OR external_id ILIKE $1)".to_string(),
        );
    }
    if let Some(ref cursor) = params.cursor {
        // `cursor` ist als Uuid typisiert; Display liefert nur Hex/Bindestriche.
        conditions.push(format!("id > '{}'", cursor));
    }

    if !conditions.is_empty() {
        query.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
    }
    query.push_str(" ORDER BY id ASC LIMIT ");
    query.push_str(&limit.to_string());

    let pool = db.as_ref();

    let rows: Vec<Patient> = if let Some(ref search) = params.search {
        sqlx::query_as::<_, Patient>(&query)
            .bind(format!("%{}%", search))
            .fetch_all(pool)
            .await
            .map_err(|e| PatientError::Db(e.to_string()))?
    } else {
        sqlx::query_as::<_, Patient>(&query)
            .fetch_all(pool)
            .await
            .map_err(|e| PatientError::Db(e.to_string()))?
    };

    let next_cursor = rows.last().map(|p| p.id);
    Ok(Json(PatientPage {
        count: rows.len(),
        items: rows,
        next_cursor,
    }))
}

/// GET /api/v1/patients/:id
///
/// Einzelpatient (nur aktive, d.h. deleted_at IS NULL).
async fn get_patient(
    State(db): State<Arc<PgPool>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Patient>, axum::response::Response> {
    let pool = db.as_ref();
    let patient: Option<Patient> =
        sqlx::query_as("SELECT * FROM patients WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| PatientError::Db(e.to_string()))?;

    match patient {
        Some(p) => Ok(Json(p)),
        None => Err(axum::response::Response::from(PatientError::NotFound(id))),
    }
}

/// POST /api/v1/patients
///
/// Neuen Patienten anlegen.
async fn create_patient(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Json(body): Json<CreatePatient>,
) -> Result<Json<Patient>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| PatientError::Db(e.to_string()))?;

    let allergies_json = serde_json::to_value(body.allergies.unwrap_or_default())
        .unwrap_or_else(|_| serde_json::json!([]));

    let patient: Patient = sqlx::query_as(
        r#"
        INSERT INTO patients (
            external_id, name_family, name_given, birth_date,
            gender, phone, email, address_street, address_zip, address_city,
            insurance_number, allergies, created_at, updated_at, version
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now(), now(), 1
        )
        RETURNING *
        "#,
    )
    .bind(&body.external_id)
    .bind(&body.name_family)
    .bind(&body.name_given)
    .bind(body.birth_date)
    .bind(&body.gender)
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address_street)
    .bind(&body.address_zip)
    .bind(&body.address_city)
    .bind(&body.insurance_number)
    .bind(allergies_json)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| PatientError::Db(e.to_string()))?;

    // Audit in derselben Transaktion; ohne Klartext-Personendaten
    audit(&mut tx, &auth, "patient.create", patient.id, None).await?;
    tx.commit()
        .await
        .map_err(|e| PatientError::Db(e.to_string()))?;

    info!(patient_id = %patient.id, "Patient angelegt");
    Ok(Json(patient))
}

/// PUT /api/v1/patients/:id
///
/// Patienten aktualisieren mit Optimistic Locking.
///
/// Der Client muss die aktuelle `version` übergeben. Falls sich die
/// Version in der DB geändert hat (ein anderer Nutzer hat in der
/// Zwischenzeit aktualisiert), wird mit HTTP 409 CONFLICT geantwortet.
async fn update_patient(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePatient>,
) -> Result<Json<Patient>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| PatientError::Db(e.to_string()))?;
    let pool = &mut *tx;

    // 1. Aktuelle Version in der DB lesen
    let current: Option<(i32,)> =
        sqlx::query_as("SELECT version FROM patients WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *pool)
            .await
            .map_err(|e| PatientError::Db(e.to_string()))?;

    let current_version = match current {
        Some((v,)) => v,
        None => return Err(axum::response::Response::from(PatientError::NotFound(id))),
    };

    if current_version != body.version {
        warn!(
            patient_id = %id,
            expected = body.version,
            actual = current_version,
            "Optimistic Locking: Versionenkonflikt"
        );
        return Err(axum::response::Response::from(
            PatientError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        ));
    }

    // 2. Update mit WHERE version = $expected (atomar)
    let new_version = body.version + 1;
    let patient: Option<Patient> = sqlx::query_as(
        r#"
        UPDATE patients SET
            external_id     = COALESCE($2, external_id),
            name_family     = COALESCE($3, name_family),
            name_given      = COALESCE($4, name_given),
            birth_date      = COALESCE($5, birth_date),
            gender          = COALESCE($6, gender),
            phone           = COALESCE($7, phone),
            email           = COALESCE($8, email),
            address_street  = COALESCE($9, address_street),
            address_zip     = COALESCE($10, address_zip),
            address_city    = COALESCE($11, address_city),
            insurance_number= COALESCE($12, insurance_number),
            allergies       = COALESCE($13, allergies),
            updated_at      = now(),
            version         = $14
        WHERE id = $1 AND version = $15 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(&body.external_id)
    .bind(&body.name_family)
    .bind(&body.name_given)
    .bind(body.birth_date)
    .bind(&body.gender)
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address_street)
    .bind(&body.address_zip)
    .bind(&body.address_city)
    .bind(&body.insurance_number)
    .bind(
        body.allergies
            .as_ref()
            .and_then(|a| serde_json::to_value(a).ok()),
    )
    .bind(new_version)
    .bind(body.version)
    .fetch_optional(&mut *pool)
    .await
    .map_err(|e| PatientError::Db(e.to_string()))?;

    match patient {
        Some(p) => {
            let fields = body.changed_fields();
            audit(
                &mut tx,
                &auth,
                "patient.update",
                p.id,
                Some(serde_json::json!({ "fields": fields, "version": p.version })),
            )
            .await?;
            tx.commit()
                .await
                .map_err(|e| PatientError::Db(e.to_string()))?;
            info!(patient_id = %p.id, new_version = p.version, "Patient aktualisiert");
            Ok(Json(p))
        }
        None => Err(axum::response::Response::from(
            PatientError::VersionConflict {
                expected: body.version,
                actual: current_version,
            },
        )),
    }
}

/// DELETE /api/v1/patients/:id
///
/// Soft-Delete: deleted_at wird gesetzt, version wird erhöht.
/// (DSGVO Art. 17 — Daten werden nicht physikalisch gelöscht,
/// sondern logisch markiert. Physische Löschung nach Aufbewahrungsfrist.)
async fn delete_patient(
    State(db): State<Arc<PgPool>>,
    auth: RequireRole,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, axum::response::Response> {
    let mut tx = db
        .begin()
        .await
        .map_err(|e| PatientError::Db(e.to_string()))?;

    let result = sqlx::query(
        r#"
        UPDATE patients
        SET deleted_at = now(), updated_at = now(), version = version + 1
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING id
        "#,
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| PatientError::Db(e.to_string()))?;

    match result {
        Some(_) => {
            audit(&mut tx, &auth, "patient.soft_delete", id, None).await?;
            tx.commit()
                .await
                .map_err(|e| PatientError::Db(e.to_string()))?;
            info!(patient_id = %id, "Patient soft-gelöscht");
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Patient soft-gelöscht",
                "id": id.to_string(),
            })))
        }
        None => Err(axum::response::Response::from(PatientError::NotFound(id))),
    }
}

/// GET /api/v1/patients/:id/history
///
/// Audit-Log für einen Patienten (DSGVO-konform, 10 Jahre Aufbewahrung).
async fn patient_history(
    State(db): State<Arc<PgPool>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<AuditEntry>>, axum::response::Response> {
    let pool = db.as_ref();

    let entries: Vec<AuditEntry> = sqlx::query_as(
        r#"
        SELECT id, timestamp, user_id, action,
               entity_type, entity_id, old_data, new_data, ip_address
        FROM audit_logs
        WHERE entity_type = 'patient' AND entity_id = $1
        ORDER BY timestamp DESC, id DESC
        LIMIT 100
        "#,
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await
    .map_err(|e| PatientError::Db(e.to_string()))?;

    Ok(Json(entries))
}

// ===========================================================================
// Hilfsfunktionen
// ===========================================================================

/// Schreibt den Audit-Eintrag einer Patientenänderung in die laufende
/// Transaktion. Schlägt das Audit fehl, wird die Änderung verworfen.
/// Es werden nur Feldnamen protokolliert, keine Werte: der Audit-Trail ist
/// append-only und muss eine spätere Löschung der Personendaten überdauern.
async fn audit(
    tx: &mut sqlx::PgConnection,
    auth: &RequireRole,
    action: &str,
    id: Uuid,
    details: Option<serde_json::Value>,
) -> Result<(), PatientError> {
    append_audit(
        tx,
        auth.user_id(),
        action,
        "patient",
        Some(&id.to_string()),
        None,
        details.as_ref(),
        None,
    )
    .await
    .map_err(|e| PatientError::Db(e.to_string()))
}

impl UpdatePatient {
    /// Namen der im Update gesetzten Felder (ohne Werte).
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        macro_rules! chk { ($($name:ident => $label:literal),* $(,)?) => { $( if self.$name.is_some() { f.push($label); } )* } }
        chk!(external_id => "externalId", name_family => "nameFamily", name_given => "nameGiven",
             birth_date => "birthDate", gender => "gender", phone => "phone", email => "email",
             address_street => "addressStreet", address_zip => "addressZip", address_city => "addressCity",
             insurance_number => "insuranceNumber", allergies => "allergies");
        f
    }
}

/// Modul-Status (für das Dashboard)
#[derive(Debug, Clone, Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

pub const MODULE_NAME: &str = "kis-patients";
pub const MODULE_DESCRIPTION: &str =
    "KIS Patient-Service — Phase 1: CRUD, Optimistic Locking, Soft-Delete, Audit-Log";
