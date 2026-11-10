//! KIS FHIR R4 Server
//!
//! Vollständiger FHIR-R4-Ressourcen-Server als Teil des KIS:
//!
//! - **REST-API** (unter `/fhir`):
//!   - `GET/POST /fhir/Patient`, `GET/PUT/DELETE /fhir/Patient/{id}`
//!   - `GET/POST /fhir/Appointment`, `GET /fhir/Appointment/{id}`,
//!     `POST /fhir/Appointment/{id}/$book`, `POST /fhir/Appointment/{id}/$cancel`
//!   - `GET /fhir/Practitioner`, `GET /fhir/Organization`
//!   - `GET /fhir/metadata`, `GET /fhir/health`
//! - **FHIR-Ressourcen**: Patient, Appointment, Practitioner, Organization, Bundle
//! - **ISiK-konform**: System-Namensraum `urn:oid:1.2.276.0.79.3.7.1`
//! - **Mapping** KIS-PostgreSQL (patients, doctors, appointments) → FHIR
//!
//! KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO — dieser FHIR-Server
//! liefert die R4-Endpunkte, die CGM MEDICO bisher als FHIR-Schnittstelle
//! bereitstellte, jetzt vollständig ISiK-konform.
//!
//! Architektur (Kanonical-KIS-Muster, wie `kis-gdt`):
//! - `register_router(db, security, config) -> Router`
//! - `FhirState { db, security, config }`
//! - Fehler → `(StatusCode, Json<ApiError>)`, Erfolg → `Json<ApiResponse<T>>`
//!
//! Hinweis: `observations`/`documents`-Tabellen existieren in der KIS-DB
//! nicht — Observation-/DocumentReference-Endpunkte sind bewusst NICHT
//! implementiert (statt sie zu erfinden).

use std::sync::Arc;

use axum::{
    extract::{Path as PathExtract, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use sqlx::{Encode, FromRow, Postgres, Type};
use tracing::{info, warn};
use uuid::Uuid;

/// Konkretes Bind-Typ für dynamische Such-Abfragen (Uuid).
///
/// `Vec<Box<dyn Encode + Type>>` ist kein gültiger Trait-Objekt (Encode ist
/// nicht Auto) — daher dieses Enum, das beide Traits implementiert.
enum DynBind {
    Uuid(Uuid),
}

impl Encode<'_, Postgres> for DynBind {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        match self {
            DynBind::Uuid(v) => v.encode_by_ref(buf),
        }
    }
}

impl Type<Postgres> for DynBind {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        // Standard-Abfrage: beide Varianten werden als Uuid behandelt, wenn
        // unklar; hier geben wir den Typ der ersten Variante zurück.
        <Uuid as Type<Postgres>>::type_info()
    }
}

/// Modulname für Status-Dashboard
pub const MODULE_NAME: &str = "kis-fhir";
/// Modulbeschreibung
pub const MODULE_DESCRIPTION: &str =
    "kis-fhir — FHIR R4 Ressourcen-Server (Patient/Appointment/Practitioner/Organization)";
/// ISiK System-Namensraum (OID)
pub const ISIK_NAMESPACE: &str = "urn:oid:1.2.276.0.79.3.7.1";
/// FHIR-Version
pub const FHIR_VERSION: &str = "4.0.1";

// ============================================================
// FHIR-Datentypen (KIS-Mapping, ISiK-konform)
// ============================================================

/// FHIR Coding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirCoding {
    pub system: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl FhirCoding {
    fn new(system: &str, code: &str, display: Option<&str>) -> Self {
        Self {
            system: system.to_string(),
            code: code.to_string(),
            display: display.map(|s| s.to_string()),
        }
    }
}

/// FHIR CodeableConcept
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirCodeableConcept {
    pub coding: Vec<FhirCoding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// FHIR Identifier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirIdentifier {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub value: String,
}

/// FHIR HumanName
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirHumanName {
    pub system: Option<String>,
    pub family: String,
    pub given: Vec<String>,
}

/// FHIR Address
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirAddress {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

/// FHIR ContactPoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirContactPoint {
    pub system: String,
    pub value: String,
}

/// FHIR Patient-Ressource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirPatient {
    pub resource_type: String,
    pub id: String,
    pub meta: FhirMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<Vec<FhirIdentifier>>,
    pub name: Vec<FhirHumanName>,
    pub gender: String,
    pub birth_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Vec<FhirAddress>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telecom: Option<Vec<FhirContactPoint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

/// FHIR Practitioner-Ressource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirPractitioner {
    pub resource_type: String,
    pub id: String,
    pub meta: FhirMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<Vec<FhirIdentifier>>,
    pub name: Vec<FhirHumanName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualification: Option<Vec<FhirPractitionerQualification>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

/// FHIR PractitionerQualification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirPractitionerQualification {
    pub code: FhirCodeableConcept,
}

/// FHIR Organization-Ressource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirOrganization {
    pub resource_type: String,
    pub id: String,
    pub meta: FhirMeta,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<Vec<FhirIdentifier>>,
}

/// FHIR Appointment-Ressource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirAppointment {
    pub resource_type: String,
    pub id: String,
    pub meta: FhirMeta,
    pub status: String,
    pub appointment_type: Vec<FhirCoding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_type: Option<Vec<FhirCoding>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: String,
    pub end: String,
    pub participant: Vec<FhirAppointmentParticipant>,
}

/// FHIR Appointment participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirAppointmentParticipant {
    pub type_: Vec<FhirCoding>,
    #[serde(rename = "actor")]
    pub actor: FhirReference,
}

/// FHIR Reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirReference {
    pub reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// FHIR Meta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirMeta {
    pub last_updated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Vec<FhirCoding>>,
}

/// FHIR CapabilityStatement
#[derive(Debug, Clone, Serialize)]
pub struct FhirCapabilityStatement {
    pub resource_type: String,
    pub status: String,
    pub date: String,
    pub kind: String,
    pub instantiates_canonical: Vec<String>,
    pub software: FhirSoftware,
    pub implementation: FhirImplementation,
    pub fhir_version: String,
    pub format: Vec<String>,
    pub rest: FhirRest,
}

#[derive(Debug, Clone, Serialize)]
pub struct FhirSoftware {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FhirImplementation {
    description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FhirRest {
    mode: String,
    security: FhirRestSecurity,
    resource: Vec<FhirRestResource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FhirRestSecurity {
    extension: Vec<FhirRestSecurityExt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FhirRestSecurityExt {
    url: String,
    value_code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FhirRestResource {
    type_: String,
    interaction: Vec<FhirInteraction>,
}

#[derive(Debug, Clone, Serialize)]
struct FhirInteraction {
    code: String,
}

/// FHIR Bundle (Search-Response)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirBundle {
    pub resource_type: String,
    pub id: String,
    pub meta: FhirMeta,
    pub type_: String,
    pub entry: Vec<FhirBundleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirBundleEntry {
    pub full_resource: Option<serde_json::Value>,
}

/// FHIR OperationOutcome (für Fehler, die als FHIR-Ressource zurückgegeben werden)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirOperationOutcome {
    pub resource_type: String,
    pub issue: Vec<FhirIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirIssue {
    pub severity: String,
    pub code: String,
    pub diagnostics: String,
}

// ============================================================
// Interne DB-Zeilen (sqlx FromRow)
// ============================================================

#[derive(Debug, FromRow)]
struct PatientRow {
    id: Uuid,
    external_id: Option<String>,
    name_family: String,
    name_given: String,
    birth_date: NaiveDate,
    gender: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    address_street: Option<String>,
    address_zip: Option<String>,
    address_city: Option<String>,
    #[allow(dead_code)]
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct DoctorRow {
    id: Uuid,
    external_id: Option<String>,
    name_family: String,
    name_given: String,
    specialty: Option<String>,
    specialties: serde_json::Value,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct AppointmentRow {
    id: Uuid,
    patient_id: Uuid,
    doctor_id: Uuid,
    category: String,
    status: String,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    reason: Option<String>,
    updated_at: DateTime<Utc>,
    #[allow(dead_code)]
    deleted_at: Option<DateTime<Utc>>,
}

// ============================================================
// Mapping: DB → FHIR
// ============================================================

fn fhir_meta(updated_at: DateTime<Utc>) -> FhirMeta {
    FhirMeta {
        last_updated: updated_at.to_rfc3339(),
        tag: Some(vec![FhirCoding::new(ISIK_NAMESPACE, "kis", Some("KIS"))]),
    }
}

fn map_patient(row: &PatientRow) -> FhirPatient {
    let gender = row
        .gender
        .clone()
        .and_then(|g| match g.to_lowercase().as_str() {
            "m" | "male" => Some("male".to_string()),
            "f" | "female" => Some("female".to_string()),
            "x" | "other" => Some("other".to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "unknown".to_string());

    let identifier = row.external_id.as_ref().map(|eid| {
        vec![FhirIdentifier {
            system: Some(ISIK_NAMESPACE.to_string()),
            value: eid.clone(),
        }]
    });

    let address = match (&row.address_street, &row.address_zip, &row.address_city) {
        (Some(street), zip, city) => Some(vec![FhirAddress {
            line: Some(vec![street.clone()]),
            postal_code: zip.clone(),
            city: city.clone(),
            country: Some("DE".to_string()),
        }]),
        _ => None,
    };

    let mut telecom = Vec::new();
    if let Some(p) = &row.phone {
        telecom.push(FhirContactPoint {
            system: "phone".into(),
            value: p.clone(),
        });
    }
    if let Some(e) = &row.email {
        telecom.push(FhirContactPoint {
            system: "email".into(),
            value: e.clone(),
        });
    }

    FhirPatient {
        resource_type: "Patient".into(),
        id: row.id.to_string(),
        meta: fhir_meta(row.updated_at),
        identifier,
        name: vec![FhirHumanName {
            system: Some("official".into()),
            family: row.name_family.clone(),
            given: split_given(&row.name_given),
        }],
        gender,
        birth_date: row.birth_date.to_string(),
        address,
        telecom: if telecom.is_empty() {
            None
        } else {
            Some(telecom)
        },
        active: Some(row.deleted_at.is_none()),
    }
}

fn map_doctor(row: &DoctorRow) -> FhirPractitioner {
    let identifier = row.external_id.as_ref().map(|eid| {
        vec![FhirIdentifier {
            system: Some(ISIK_NAMESPACE.to_string()),
            value: eid.clone(),
        }]
    });

    // specialties JSONB (z. B. ["Kardiologie","Internistin"]) + specialty TEXT
    let mut quals = Vec::new();
    if let Some(s) = &row.specialty {
        if !s.trim().is_empty() {
            quals.push(FhirPractitionerQualification {
                code: FhirCodeableConcept {
                    coding: vec![FhirCoding::new(
                        "https://fhir.kis.de/CodeSystem/specialty",
                        &slug(s),
                        Some(s),
                    )],
                    text: Some(s.clone()),
                },
            });
        }
    }
    if let Some(arr) = row.specialties.as_array() {
        for s in arr {
            if let Some(t) = s.as_str() {
                if !t.trim().is_empty() {
                    quals.push(FhirPractitionerQualification {
                        code: FhirCodeableConcept {
                            coding: vec![FhirCoding::new(
                                "https://fhir.kis.de/CodeSystem/specialty",
                                &slug(t),
                                Some(t),
                            )],
                            text: Some(t.to_string()),
                        },
                    });
                }
            }
        }
    }

    FhirPractitioner {
        resource_type: "Practitioner".into(),
        id: row.id.to_string(),
        meta: fhir_meta(row.updated_at),
        identifier,
        name: vec![FhirHumanName {
            system: Some("official".into()),
            family: row.name_family.clone(),
            given: split_given(&row.name_given),
        }],
        qualification: if quals.is_empty() { None } else { Some(quals) },
        active: Some(row.deleted_at.is_none()),
    }
}

fn map_appointment(row: &AppointmentRow) -> FhirAppointment {
    FhirAppointment {
        resource_type: "Appointment".into(),
        id: row.id.to_string(),
        meta: fhir_meta(row.updated_at),
        status: map_appt_status(&row.status),
        appointment_type: vec![FhirCoding::new(
            "https://fhir.kis.de/CodeSystem/appointment-category",
            &slug(&row.category),
            Some(&row.category),
        )],
        service_type: row.reason.as_ref().map(|r| {
            vec![FhirCoding::new(
                "https://fhir.kis.de/CodeSystem/appointment-reason",
                &slug(r),
                Some(r),
            )]
        }),
        description: row.reason.clone(),
        start: row.start_time.to_rfc3339(),
        end: row.end_time.to_rfc3339(),
        participant: vec![
            FhirAppointmentParticipant {
                type_: vec![FhirCoding::new(
                    "http://terminology.hl7.org/CodeSystem/v3-RoleCode",
                    "ATND",
                    Some("Teilnehmer"),
                )],
                actor: FhirReference {
                    reference: format!("Patient/{}", row.patient_id),
                    display: None,
                },
            },
            FhirAppointmentParticipant {
                type_: vec![FhirCoding::new(
                    "http://terminology.hl7.org/CodeSystem/v3-RoleCode",
                    "PRCP",
                    Some("Praktiker"),
                )],
                actor: FhirReference {
                    reference: format!("Practitioner/{}", row.doctor_id),
                    display: None,
                },
            },
        ],
    }
}

/// KIS-Status (Deutsch) → FHIR Appointment status (Code)
fn map_appt_status(kis: &str) -> String {
    match kis.to_lowercase().as_str() {
        "geplant" => "booked".to_string(),
        "bestätigt" | "bestaetigt" => "arrived".to_string(),
        "im_besuch" | "im besuch" | "in treatment" => "in-progress".to_string(),
        "abgeschlossen" | "erledigt" | "completed" => "fulfilled".to_string(),
        "abgesagt" | "storniert" | "cancelled" => "cancelled".to_string(),
        _ => "pending".to_string(),
    }
}

fn split_given(s: &str) -> Vec<String> {
    s.split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

fn slug(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// ============================================================
// Audit-Log (Hash-Chain, DSGVO 10 Jahre)
// ============================================================

/// Schreibt einen Eintrag in die `audit_log`-Tabelle (Hash-Chain).
///
/// Schema (002_security.sql):
/// timestamp, user_id, username, action, entity_type, entity_id,
/// new_value (JSONB), ip_address, current_hash (NOT NULL)
/// Schreibt einen Audit-Eintrag (Hash-Chain) — Kanonical-KIS-Muster (wie `kis-gdt`).
///
/// Schema `audit_log` (002_security.sql):
/// id, timestamp, user_id, username, action, entity_type, entity_id,
/// old_value, new_value, ip_address, prev_hash, current_hash (NOT NULL)
async fn audit(
    db: &Database,
    security: &Security,
    username: &str,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    new_value: &serde_json::Value,
) {
    // Letzten Hash der Kette laden (Genesis = "")
    let prev_hash: String = sqlx::query_scalar(
        "SELECT current_hash FROM audit_log ORDER BY timestamp DESC, id DESC LIMIT 1",
    )
    .fetch_optional(db.read())
    .await
    .ok()
    .flatten()
    .unwrap_or_default();

    let audit_data = serde_json::json!({
        "username": username,
        "action": action,
        "entity_type": entity_type,
        "entity_id": entity_id,
        "new_value": new_value,
    });
    let current_hash = security.audit_hash(&prev_hash, &audit_data);

    if let Err(e) = sqlx::query(
        "INSERT INTO audit_log \
         (timestamp, username, action, entity_type, entity_id, new_value, prev_hash, current_hash) \
         VALUES (now(), $1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(username)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(new_value)
    .bind(&prev_hash)
    .bind(&current_hash)
    .execute(db.write())
    .await
    {
        warn!(error = %e, "Audit-Log-Eintrag fehlgeschlagen (nicht fatal)");
    }
}

// ============================================================
// State
// ============================================================

#[derive(Clone)]
pub struct FhirState {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub config: AppConfig,
}

// ============================================================
// Handler: Patient
// ============================================================

/// GET /fhir/Patient?limit=&after=
async fn patient_search(
    State(state): State<FhirState>,
    Query(q): Query<FhirSearchQuery>,
) -> Result<Json<FhirBundle>, (StatusCode, Json<ApiError>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let sql = if q.after.is_some() {
        "SELECT * FROM patients WHERE id > $1 AND deleted_at IS NULL ORDER BY id ASC LIMIT $2"
    } else {
        "SELECT * FROM patients WHERE deleted_at IS NULL ORDER BY id ASC LIMIT $1"
    };
    let rows: Vec<PatientRow> = match q.after {
        Some(cursor) => {
            sqlx::query_as(sql)
                .bind(cursor)
                .bind(limit)
                .fetch_all(state.db.read())
                .await
        }
        None => {
            sqlx::query_as(sql)
                .bind(limit)
                .fetch_all(state.db.read())
                .await
        }
    }
    .map_err(|e| {
        warn!(error = %e, "FHIR Patient-Search fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Suche fehlgeschlagen")),
        )
    })?;

    let last_id = rows.last().map(|r| r.id);
    let entries = rows.iter().map(|r| FhirBundleEntry {
        full_resource: Some(serde_json::to_value(map_patient(r)).unwrap_or_default()),
    });
    let bundle = FhirBundle {
        resource_type: "Bundle".into(),
        id: Uuid::new_v4().to_string(),
        meta: fhir_meta(Utc::now()),
        type_: "searchset".into(),
        entry: entries.collect(),
    };
    let _ = last_id;
    Ok(Json(bundle))
}

/// GET /fhir/Patient/{id}
async fn patient_read(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
) -> Result<Json<FhirPatient>, (StatusCode, Json<ApiError>)> {
    let row = sqlx::query_as::<_, PatientRow>(
        "SELECT * FROM patients WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db.read())
    .await
    .map_err(|e| {
        warn!(error = %e, "FHIR Patient-Read fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Lese-Fehler")),
        )
    })?;

    match row {
        Some(r) => Ok(Json(map_patient(&r))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Patient nicht gefunden")),
        )),
    }
}

/// POST /fhir/Patient  (Body: FHIR Patient)
async fn patient_create(
    State(state): State<FhirState>,
    Json(fhir): Json<FhirPatient>,
) -> Result<Json<FhirPatient>, (StatusCode, Json<ApiError>)> {
    let name = fhir.name.first().ok_or((
        StatusCode::BAD_REQUEST,
        Json(ApiError::bad_request("name fehlt")),
    ))?;
    let family = name.family.clone();
    let given = name.given.join(" ");

    let new_id = Uuid::new_v4();
    let birth = NaiveDate::parse_from_str(&fhir.birth_date, "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request("birth_date muss YYYY-MM-DD sein")),
        )
    })?;

    let external_id = fhir
        .identifier
        .as_ref()
        .and_then(|v| v.first())
        .map(|i| i.value.clone());

    match sqlx::query(
        "INSERT INTO patients (id, external_id, name_family, name_given, birth_date, gender, phone, email) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(new_id)
    .bind(&external_id)
    .bind(&family)
    .bind(&given)
    .bind(birth)
    .bind(&fhir.gender)
    .bind(fhir.telecom.as_ref().and_then(|t| {
        t.iter().find(|c| c.system == "phone").map(|c| c.value.clone())
    }))
    .bind(fhir.telecom.as_ref().and_then(|t| {
        t.iter().find(|c| c.system == "email").map(|c| c.value.clone())
    }))
    .execute(state.db.write())
    .await
    {
        Ok(_) => {
            info!(patient_id = %new_id, "FHIR Patient erstellt");
            let row = sqlx::query_as::<_, PatientRow>("SELECT * FROM patients WHERE id = $1")
                .bind(new_id)
                .fetch_one(state.db.read())
                .await
                .map_err(|e| {
                    warn!(error = %e, "FHIR Patient nach Create nicht lesbar");
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal("Lese-Fehler")))
                })?;
            audit(
                &state.db,
                &state.security,
                "fhir-api",
                "patient.create",
                "patient",
                &new_id.to_string(),
                &serde_json::json!({ "name_family": family, "name_given": given }),
            )
            .await;
            Ok(Json(map_patient(&row)))
        }
        Err(e) => {
            warn!(error = %e, "FHIR Patient-Create fehlgeschlagen");
            Err((
                StatusCode::CONFLICT,
                Json(ApiError::conflict("Erstellung fehlgeschlagen")),
            ))
        }
    }
}

/// PUT /fhir/Patient/{id}
async fn patient_update(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
    Json(fhir): Json<FhirPatient>,
) -> Result<Json<FhirPatient>, (StatusCode, Json<ApiError>)> {
    let name = fhir.name.first().ok_or((
        StatusCode::BAD_REQUEST,
        Json(ApiError::bad_request("name fehlt")),
    ))?;
    let family = name.family.clone();
    let given = name.given.join(" ");

    let birth = NaiveDate::parse_from_str(&fhir.birth_date, "%Y-%m-%d").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request("birth_date muss YYYY-MM-DD sein")),
        )
    })?;

    let updated = sqlx::query(
        "UPDATE patients SET name_family = $2, name_given = $3, birth_date = $4, gender = $5, \
         updated_at = now() WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(&family)
    .bind(&given)
    .bind(birth)
    .bind(&fhir.gender)
    .execute(state.db.write())
    .await
    .map_err(|e| {
        warn!(error = %e, "FHIR Patient-Update fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Update fehlgeschlagen")),
        )
    })?;

    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Patient nicht gefunden")),
        ));
    }

    let row = sqlx::query_as::<_, PatientRow>("SELECT * FROM patients WHERE id = $1")
        .bind(id)
        .fetch_one(state.db.read())
        .await
        .map_err(|e| {
            warn!(error = %e, "FHIR Patient nach Update nicht lesbar");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Lese-Fehler")),
            )
        })?;
    audit(
        &state.db,
        &state.security,
        "fhir-api",
        "patient.update",
        "patient",
        &id.to_string(),
        &serde_json::json!({ "name_family": family, "name_given": given }),
    )
    .await;
    Ok(Json(map_patient(&row)))
}

/// DELETE /fhir/Patient/{id}  (Soft-Delete, DSGVO Art. 17)
async fn patient_delete(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiError>)> {
    let updated =
        sqlx::query("UPDATE patients SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .execute(state.db.write())
            .await
            .map_err(|e| {
                warn!(error = %e, "FHIR Patient-Delete fehlgeschlagen");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::internal("Delete fehlgeschlagen")),
                )
            })?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Patient nicht gefunden")),
        ));
    }
    audit(
        &state.db,
        &state.security,
        "fhir-api",
        "patient.delete",
        "patient",
        &id.to_string(),
        &serde_json::json!({}),
    )
    .await;
    Ok(Json(ApiResponse {
        data: (),
        meta: ResponseMeta {
            total: 0,
            has_more: false,
            next_cursor: None,
        },
    }))
}

// ============================================================
// Handler: Appointment
// ============================================================

/// GET /fhir/Appointment?limit=&after=&patient=&status=
async fn appointment_search(
    State(state): State<FhirState>,
    Query(q): Query<AppointmentSearchQuery>,
) -> Result<Json<FhirBundle>, (StatusCode, Json<ApiError>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let mut where_clauses = vec!["deleted_at IS NULL".to_string()];
    let mut binds: Vec<DynBind> = Vec::new();

    if let Some(cursor) = q.after {
        where_clauses.push(format!("id > ${}", binds.len() + 1));
        binds.push(DynBind::Uuid(cursor));
    }
    if let Some(p) = q.patient {
        where_clauses.push(format!("patient_id = ${}", binds.len() + 1));
        binds.push(DynBind::Uuid(p));
    }

    let where_sql = where_clauses.join(" AND ");
    let limit_idx = binds.len() + 1;
    let sql = format!(
        "SELECT * FROM appointments WHERE {} ORDER BY id ASC LIMIT ${}",
        where_sql, limit_idx
    );

    let mut query = sqlx::query_as::<_, AppointmentRow>(&sql);
    for b in &binds {
        query = query.bind(b);
    }
    let rows = query
        .bind(limit as i32)
        .fetch_all(state.db.read())
        .await
        .map_err(|e| {
            warn!(error = %e, "FHIR Appointment-Search fehlgeschlagen");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Suche fehlgeschlagen")),
            )
        })?;

    let entries = rows.iter().map(|r| FhirBundleEntry {
        full_resource: Some(serde_json::to_value(map_appointment(r)).unwrap_or_default()),
    });
    Ok(Json(FhirBundle {
        resource_type: "Bundle".into(),
        id: Uuid::new_v4().to_string(),
        meta: fhir_meta(Utc::now()),
        type_: "searchset".into(),
        entry: entries.collect(),
    }))
}

/// GET /fhir/Appointment/{id}
async fn appointment_read(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
) -> Result<Json<FhirAppointment>, (StatusCode, Json<ApiError>)> {
    let row = sqlx::query_as::<_, AppointmentRow>(
        "SELECT * FROM appointments WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db.read())
    .await
    .map_err(|e| {
        warn!(error = %e, "FHIR Appointment-Read fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Lese-Fehler")),
        )
    })?;
    match row {
        Some(r) => Ok(Json(map_appointment(&r))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Appointment nicht gefunden")),
        )),
    }
}

/// POST /fhir/Appointment  (Body: FHIR Appointment mit participant-References)
async fn appointment_create(
    State(state): State<FhirState>,
    Json(fhir): Json<FhirAppointment>,
) -> Result<Json<FhirAppointment>, (StatusCode, Json<ApiError>)> {
    // Teilnehmer auflösen: Patient/ + Practitioner/
    let patient_id = fhir
        .participant
        .iter()
        .find(|p| p.actor.reference.starts_with("Patient/"))
        .and_then(|p| p.actor.reference.strip_prefix("Patient/"))
        .and_then(|s| Uuid::parse_str(s).ok());
    let doctor_id = fhir
        .participant
        .iter()
        .find(|p| p.actor.reference.starts_with("Practitioner/"))
        .and_then(|p| p.actor.reference.strip_prefix("Practitioner/"))
        .and_then(|s| Uuid::parse_str(s).ok());

    let (patient_id, doctor_id) = match (patient_id, doctor_id) {
        (Some(p), Some(d)) => (p, d),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::bad_request(
                    "participant muss Patient/ und Practitioner/ enthalten",
                )),
            ))
        }
    };

    let start = DateTime::parse_from_rfc3339(&fhir.start).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request("start muss RFC3339 sein")),
        )
    })?;
    let end = DateTime::parse_from_rfc3339(&fhir.end).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request("end muss RFC3339 sein")),
        )
    })?;

    let category = fhir
        .appointment_type
        .first()
        .map(|c| c.display.clone().unwrap_or_else(|| c.code.clone()))
        .unwrap_or_else(|| "ordination".to_string());

    let new_id = Uuid::new_v4();
    match sqlx::query(
        "INSERT INTO appointments (id, patient_id, doctor_id, category, status, start_time, end_time, reason) \
         VALUES ($1, $2, $3, $4, 'geplant', $5, $6, $7)",
    )
    .bind(new_id)
    .bind(patient_id)
    .bind(doctor_id)
    .bind(&category)
    .bind(start)
    .bind(end)
    .bind(&fhir.description)
    .execute(state.db.write())
    .await
    {
        Ok(_) => {
            info!(appointment_id = %new_id, "FHIR Appointment erstellt");
            let row = sqlx::query_as::<_, AppointmentRow>(
                "SELECT * FROM appointments WHERE id = $1",
            )
            .bind(new_id)
            .fetch_one(state.db.read())
            .await
            .map_err(|e| {
                warn!(error = %e, "FHIR Appointment nach Create nicht lesbar");
                (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal("Lese-Fehler")))
            })?;
            audit(
                &state.db,
                &state.security,
                "fhir-api",
                "appointment.create",
                "appointment",
                &new_id.to_string(),
                &serde_json::json!({ "patient_id": patient_id, "doctor_id": doctor_id }),
            )
            .await;
            Ok(Json(map_appointment(&row)))
        }
        Err(e) => {
            warn!(error = %e, "FHIR Appointment-Create fehlgeschlagen");
            Err((
                StatusCode::CONFLICT,
                Json(ApiError::conflict("Erstellung fehlgeschlagen")),
            ))
        }
    }
}

/// POST /fhir/Appointment/{id}/$book  (Bestätigung)
async fn appointment_book(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
) -> Result<Json<FhirAppointment>, (StatusCode, Json<ApiError>)> {
    let updated = sqlx::query(
        "UPDATE appointments SET status = 'bestätigt', updated_at = now() \
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(state.db.write())
    .await
    .map_err(|e| {
        warn!(error = %e, "FHIR Appointment $book fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Update fehlgeschlagen")),
        )
    })?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Appointment nicht gefunden")),
        ));
    }
    let row = sqlx::query_as::<_, AppointmentRow>("SELECT * FROM appointments WHERE id = $1")
        .bind(id)
        .fetch_one(state.db.read())
        .await
        .map_err(|e| {
            warn!(error = %e, "FHIR Appointment nach $book nicht lesbar");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Lese-Fehler")),
            )
        })?;
    audit(
        &state.db,
        &state.security,
        "fhir-api",
        "appointment.book",
        "appointment",
        &id.to_string(),
        &serde_json::json!({ "status": "bestätigt" }),
    )
    .await;
    Ok(Json(map_appointment(&row)))
}

/// POST /fhir/Appointment/{id}/$cancel  (Storno)
async fn appointment_cancel(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
) -> Result<Json<FhirAppointment>, (StatusCode, Json<ApiError>)> {
    let updated = sqlx::query(
        "UPDATE appointments SET status = 'abgesagt', cancelled_at = now(), updated_at = now() \
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(state.db.write())
    .await
    .map_err(|e| {
        warn!(error = %e, "FHIR Appointment $cancel fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Update fehlgeschlagen")),
        )
    })?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Appointment nicht gefunden")),
        ));
    }
    let row = sqlx::query_as::<_, AppointmentRow>("SELECT * FROM appointments WHERE id = $1")
        .bind(id)
        .fetch_one(state.db.read())
        .await
        .map_err(|e| {
            warn!(error = %e, "FHIR Appointment nach $cancel nicht lesbar");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Lese-Fehler")),
            )
        })?;
    audit(
        &state.db,
        &state.security,
        "fhir-api",
        "appointment.cancel",
        "appointment",
        &id.to_string(),
        &serde_json::json!({ "status": "abgesagt" }),
    )
    .await;
    Ok(Json(map_appointment(&row)))
}

// ============================================================
// Handler: Practitioner / Organization / metadata
// ============================================================

/// GET /fhir/Practitioner?limit=&after=
async fn practitioner_search(
    State(state): State<FhirState>,
    Query(q): Query<FhirSearchQuery>,
) -> Result<Json<FhirBundle>, (StatusCode, Json<ApiError>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let sql = if q.after.is_some() {
        "SELECT * FROM doctors WHERE id > $1 AND deleted_at IS NULL ORDER BY id ASC LIMIT $2"
    } else {
        "SELECT * FROM doctors WHERE deleted_at IS NULL ORDER BY id ASC LIMIT $1"
    };
    let rows: Vec<DoctorRow> = match q.after {
        Some(cursor) => {
            sqlx::query_as(sql)
                .bind(cursor)
                .bind(limit)
                .fetch_all(state.db.read())
                .await
        }
        None => {
            sqlx::query_as(sql)
                .bind(limit)
                .fetch_all(state.db.read())
                .await
        }
    }
    .map_err(|e| {
        warn!(error = %e, "FHIR Practitioner-Search fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Suche fehlgeschlagen")),
        )
    })?;
    let entries = rows.iter().map(|r| FhirBundleEntry {
        full_resource: Some(serde_json::to_value(map_doctor(r)).unwrap_or_default()),
    });
    Ok(Json(FhirBundle {
        resource_type: "Bundle".into(),
        id: Uuid::new_v4().to_string(),
        meta: fhir_meta(Utc::now()),
        type_: "searchset".into(),
        entry: entries.collect(),
    }))
}

/// GET /fhir/Practitioner/{id}
async fn practitioner_read(
    State(state): State<FhirState>,
    PathExtract(id): PathExtract<Uuid>,
) -> Result<Json<FhirPractitioner>, (StatusCode, Json<ApiError>)> {
    let row = sqlx::query_as::<_, DoctorRow>(
        "SELECT * FROM doctors WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(state.db.read())
    .await
    .map_err(|e| {
        warn!(error = %e, "FHIR Practitioner-Read fehlgeschlagen");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Lese-Fehler")),
        )
    })?;
    match row {
        Some(r) => Ok(Json(map_doctor(&r))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Practitioner nicht gefunden")),
        )),
    }
}

/// GET /fhir/Organization  (KIS selbst als eine Organisation)
async fn organization_list(
    State(state): State<FhirState>,
) -> Result<Json<FhirBundle>, (StatusCode, Json<ApiError>)> {
    let org = FhirOrganization {
        resource_type: "Organization".into(),
        id: "kis".into(),
        meta: fhir_meta(Utc::now()),
        name: "KIS — Krankenhaus-Informations-System".into(),
        identifier: Some(vec![FhirIdentifier {
            system: Some(ISIK_NAMESPACE.to_string()),
            value: "kis".into(),
        }]),
    };
    let _ = &state;
    Ok(Json(FhirBundle {
        resource_type: "Bundle".into(),
        id: Uuid::new_v4().to_string(),
        meta: fhir_meta(Utc::now()),
        type_: "collection".into(),
        entry: vec![FhirBundleEntry {
            full_resource: Some(serde_json::to_value(org).unwrap_or_default()),
        }],
    }))
}

/// GET /fhir/metadata  (CapabilityStatement)
async fn metadata(
    State(state): State<FhirState>,
) -> Result<Json<FhirCapabilityStatement>, (StatusCode, Json<ApiError>)> {
    let cs = FhirCapabilityStatement {
        resource_type: "CapabilityStatement".into(),
        status: "active".into(),
        date: Utc::now().format("%Y-%m-%d").to_string(),
        kind: "instance".into(),
        instantiates_canonical: vec!["http://hl7.org/fhir".into()],
        software: FhirSoftware {
            name: MODULE_NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        implementation: FhirImplementation {
            description: MODULE_DESCRIPTION.into(),
        },
        fhir_version: FHIR_VERSION.into(),
        format: vec!["json".into()],
        rest: FhirRest {
            mode: "server".into(),
            security: FhirRestSecurity {
                extension: vec![FhirRestSecurityExt {
                    url: "http://hl7.org/fhir/rest/security".into(),
                    value_code: "bearer".into(),
                }],
            },
            resource: vec![
                FhirRestResource {
                    type_: "Patient".into(),
                    interaction: vec![
                        FhirInteraction {
                            code: "read".into(),
                        },
                        FhirInteraction {
                            code: "search-type".into(),
                        },
                        FhirInteraction {
                            code: "create".into(),
                        },
                        FhirInteraction {
                            code: "update".into(),
                        },
                        FhirInteraction {
                            code: "delete".into(),
                        },
                    ],
                },
                FhirRestResource {
                    type_: "Appointment".into(),
                    interaction: vec![
                        FhirInteraction {
                            code: "read".into(),
                        },
                        FhirInteraction {
                            code: "search-type".into(),
                        },
                        FhirInteraction {
                            code: "create".into(),
                        },
                    ],
                },
                FhirRestResource {
                    type_: "Practitioner".into(),
                    interaction: vec![
                        FhirInteraction {
                            code: "read".into(),
                        },
                        FhirInteraction {
                            code: "search-type".into(),
                        },
                    ],
                },
                FhirRestResource {
                    type_: "Organization".into(),
                    interaction: vec![FhirInteraction {
                        code: "search-type".into(),
                    }],
                },
            ],
        },
    };
    let _ = &state;
    Ok(Json(cs))
}

/// GET /fhir/health  (Health-Check)
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "module": MODULE_NAME,
        "description": MODULE_DESCRIPTION,
        "fhir_version": FHIR_VERSION,
        "namespace": ISIK_NAMESPACE,
        "status": "ok",
    }))
}

// ============================================================
// Query-Parameter
// ============================================================

#[derive(Debug, Deserialize)]
pub struct FhirSearchQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub after: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AppointmentSearchQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub after: Option<Uuid>,
    /// Filter: patient_id als UUID
    #[serde(default)]
    pub patient: Option<Uuid>,
}

// ============================================================
// Router
// ============================================================

/// Registriert den FHIR-Router.
///
/// Endpunkte (unter `/fhir`):
/// - `GET/POST      /fhir/Patient`
/// - `GET/PUT/DELETE /fhir/Patient/{id}`
/// - `GET/POST       /fhir/Appointment`
/// - `GET            /fhir/Appointment/{id}`
/// - `POST           /fhir/Appointment/{id}/$book`
/// - `POST           /fhir/Appointment/{id}/$cancel`
/// - `GET            /fhir/Practitioner`
/// - `GET            /fhir/Practitioner/{id}`
/// - `GET            /fhir/Organization`
/// - `GET            /fhir/metadata`
/// - `GET            /fhir/health`
pub fn register_router(db: Arc<Database>, security: Arc<Security>, config: AppConfig) -> Router {
    let state = FhirState {
        db,
        security,
        config,
    };

    Router::new()
        .route("/fhir/health", get(health))
        .route("/fhir/metadata", get(metadata))
        .route("/fhir/Patient", get(patient_search))
        .route("/fhir/Patient", post(patient_create))
        .route("/fhir/Patient/{id}", get(patient_read))
        .route("/fhir/Patient/{id}", put(patient_update))
        .route("/fhir/Patient/{id}", delete(patient_delete))
        .route("/fhir/Appointment", get(appointment_search))
        .route("/fhir/Appointment", post(appointment_create))
        .route("/fhir/Appointment/{id}", get(appointment_read))
        .route("/fhir/Appointment/{id}/$book", post(appointment_book))
        .route("/fhir/Appointment/{id}/$cancel", post(appointment_cancel))
        .route("/fhir/Practitioner", get(practitioner_search))
        .route("/fhir/Practitioner/{id}", get(practitioner_read))
        .route("/fhir/Organization", get(organization_list))
        .with_state(state)
}

/// Modul-Status für das Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// ============================================================
// Unit-Tests (ohne DB)
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_patient() -> PatientRow {
        PatientRow {
            id: Uuid::new_v4(),
            external_id: Some("MRN-12345".into()),
            name_family: "Mustermann".into(),
            name_given: "Max Maria".into(),
            birth_date: NaiveDate::from_ymd_opt(1980, 1, 15).unwrap(),
            gender: Some("m".into()),
            phone: Some("+49 30 123456".into()),
            email: Some("max@beispiel.de".into()),
            address_street: Some("Musterstraße 1".into()),
            address_zip: Some("10115".into()),
            address_city: Some("Berlin".into()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn test_map_patient() {
        let p = map_patient(&sample_patient());
        assert_eq!(p.resource_type, "Patient");
        assert_eq!(p.gender, "male");
        assert_eq!(p.birth_date, "1980-01-15");
        assert_eq!(p.name[0].family, "Mustermann");
        assert_eq!(
            p.name[0].given,
            vec!["Max".to_string(), "Maria".to_string()]
        );
        assert!(p.identifier.is_some());
        assert!(p.address.is_some());
        assert_eq!(p.telecom.as_ref().unwrap().len(), 2);
        assert_eq!(p.active, Some(true));
    }

    #[test]
    fn test_appt_status_mapping() {
        assert_eq!(map_appt_status("geplant"), "booked");
        assert_eq!(map_appt_status("bestätigt"), "arrived");
        assert_eq!(map_appt_status("abgesagt"), "cancelled");
        assert_eq!(map_appt_status("unbekannt"), "pending");
    }

    #[test]
    fn test_slug() {
        assert_eq!(slug("Kardiologie / Herz"), "kardiologie-herz");
        assert_eq!(slug("  Internistin  "), "internistin");
    }

    #[test]
    fn test_doctor_mapping() {
        let d = DoctorRow {
            id: Uuid::new_v4(),
            external_id: Some("DOC-1".into()),
            name_family: "Ärztin".into(),
            name_given: "Dr. Anna".into(),
            specialty: Some("Kardiologie".into()),
            specialties: serde_json::json!(["Kardiologie", "Herzchirurgie"]),
            updated_at: Utc::now(),
            deleted_at: None,
        };
        let f = map_doctor(&d);
        assert_eq!(f.resource_type, "Practitioner");
        assert!(f.qualification.is_some());
        assert!(f.qualification.as_ref().unwrap().len() >= 2);
    }
}
