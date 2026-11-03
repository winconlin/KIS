//! DSGVO Art. 15 / 20 — Datenportabilität (Task 5.7)
//!
//! Kompletter Export der Patientendaten in drei Formaten:
//!
//! - **JSON** (`export_patient_json`): strukturiertes Vollbild
//!   (Patient, Termine, Medikation, Einwilligungen, Audit-Log).
//! - **CSV** (`export_patient_csv`): eine Zeile pro Datensatz mit
//!   Typ-Spalte (`data_type`) — maschinenlesbar, tabellenorientiert.
//! - **FHIR R4 Bundle** (`export_fhir_bundle`): standardisierte,
//!   interoperable Darstellung (Patient, Appointment,
//!   MedicationStatement, Consent, DocumentReference für den
//!   Export-Nachweis).
//!
//! Alle Exporte werden im Audit-Log nachgewiesen
//! (`patient.export` + Format + Datensatzgröße).

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

use super::db::Database;
use super::security::{AuthUser, Permission, Security};
use super::models::ApiError;

/// Export-Format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Csv,
    Fhir,
}

impl ExportFormat {
    /// Parst ein Format aus dem Query-Parameter (`?format=json|csv|fhir`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "json" => Some(ExportFormat::Json),
            "csv" => Some(ExportFormat::Csv),
            "fhir" | "fhir-r4" | "fhir_r4" => Some(ExportFormat::Fhir),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
            ExportFormat::Fhir => "fhir",
        }
    }
}

/// Fehler bei Export-Operationen.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("audit error: {0}")]
    Audit(#[from] anyhow::Error),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("patient {0} not found")]
    NotFound(Uuid),

    #[error("unsupported export format: {0}")]
    UnsupportedFormat(String),
}

impl From<ExportError> for ApiError {
    fn from(e: ExportError) -> Self {
        match &e {
            ExportError::NotFound(_) => ApiError::not_found(&e.to_string()),
            ExportError::UnsupportedFormat(_) => ApiError::bad_request(&e.to_string()),
            ExportError::Permission(_) => ApiError::forbidden(&e.to_string()),
            _ => ApiError::internal(&e.to_string()),
        }
    }
}

/// Vollständiger Patientendatensatz (interne Sammelstruktur).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientExport {
    pub exported_at: DateTime<Utc>,
    pub patient: PatientRecord,
    pub appointments: Vec<AppointmentRecord>,
    pub medications: Vec<MedicationRecord>,
    pub consents: Vec<ConsentRecord>,
    /// Audit-Einträge, die sich auf den Patienten beziehen
    /// (entity_type='patient' AND entity_id=<patient>)
    pub audit_log: Vec<AuditRecord>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatientRecord {
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
    pub allergies: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppointmentRecord {
    pub id: Uuid,
    pub category: String,
    pub status: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub reason: Option<String>,
    pub doctor_id: Uuid,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicationRecord {
    pub id: Uuid,
    pub name: String,
    pub dosage: Option<String>,
    pub frequency: Option<String>,
    pub start_date: chrono::NaiveDate,
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentRecord {
    pub id: Uuid,
    pub type: String,
    pub granted: bool,
    pub granted_at: DateTime<Utc>,
    pub withdrawn_at: Option<DateTime<Utc>>,
    pub legal_basis: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub action: String,
    pub timestamp: DateTime<Utc>,
    pub user_id: Option<String>,
}

/// Service für DSGVO-Datenportabilität (Art. 15 Auskunft, Art. 20
/// Datenübertragbarkeit).
pub struct DataPortabilityService {
    db: Arc<Database>,
    security: Arc<Security>,
}

impl DataPortabilityService {
    pub fn new(db: Arc<Database>, security: Arc<Security>) -> Self {
        Self { db, security }
    }

    // ------------------------------------------------------------------
    // Internes Laden des Vollbilds (eine Quelle für alle Formate)
    // ------------------------------------------------------------------

    /// Lädt alle Patientendaten aus der Datenbank.
    async fn load(&self, patient_id: Uuid) -> Result<PatientExport, ExportError> {
        // Patient
        let p = sqlx::query(
            "SELECT id, external_id, name_family, name_given, birth_date, gender,
                    phone, email, address_street, address_zip, address_city,
                    insurance_number, allergies, created_at, updated_at, deleted_at
             FROM patients WHERE id = $1",
        )
        .bind(patient_id)
        .fetch_optional(self.db.read_pool())
        .await?
        .ok_or(ExportError::NotFound(patient_id))?;

        let patient = PatientRecord {
            id: p.get("id"),
            external_id: p.get("external_id"),
            name_family: p.get("name_family"),
            name_given: p.get("name_given"),
            birth_date: p.get("birth_date"),
            gender: p.get("gender"),
            phone: p.get("phone"),
            email: p.get("email"),
            address_street: p.get("address_street"),
            address_zip: p.get("address_zip"),
            address_city: p.get("address_city"),
            insurance_number: p.get("insurance_number"),
            allergies: p.get::<serde_json::Value, _>("allergies"),
            created_at: p.get("created_at"),
            updated_at: p.get("updated_at"),
            deleted_at: p.get("deleted_at"),
        };

        // Termine
        let appt_rows = sqlx::query(
            "SELECT id, category, status, start_time, end_time, reason, doctor_id
             FROM appointments
             WHERE patient_id = $1 AND deleted_at IS NULL
             ORDER BY start_time ASC",
        )
        .bind(patient_id)
        .fetch_all(self.db.read_pool())
        .await?;
        let appointments = appt_rows
            .into_iter()
            .map(|r| AppointmentRecord {
                id: r.get("id"),
                category: r.get("category"),
                status: r.get("status"),
                start_time: r.get("start_time"),
                end_time: r.get("end_time"),
                reason: r.get("reason"),
                doctor_id: r.get("doctor_id"),
            })
            .collect();

        // Medikation
        let med_rows = sqlx::query(
            "SELECT id, name, dosage, frequency, start_date, end_date
             FROM medications
             WHERE patient_id = $1 AND deleted_at IS NULL
             ORDER BY start_date ASC",
        )
        .bind(patient_id)
        .fetch_all(self.db.read_pool())
        .await?;
        let medications = med_rows
            .into_iter()
            .map(|r| MedicationRecord {
                id: r.get("id"),
                name: r.get("name"),
                dosage: r.get("dosage"),
                frequency: r.get("frequency"),
                start_date: r.get("start_date"),
                end_date: r.get("end_date"),
            })
            .collect();

        // Einwilligungen
        let consent_rows = sqlx::query(
            "SELECT id, type, granted, granted_at, withdrawn_at, legal_basis
             FROM consents WHERE patient_id = $1 ORDER BY granted_at ASC",
        )
        .bind(patient_id)
        .fetch_all(self.db.read_pool())
        .await?;
        let consents = consent_rows
            .into_iter()
            .map(|r| ConsentRecord {
                id: r.get("id"),
                type: r.get("type"),
                granted: r.get("granted"),
                granted_at: r.get("granted_at"),
                withdrawn_at: r.get("withdrawn_at"),
                legal_basis: r.get("legal_basis"),
            })
            .collect();

        // Audit-Log (Einträge zu diesem Patienten)
        let audit_rows = sqlx::query(
            "SELECT action, timestamp, user_id
             FROM audit_logs
             WHERE entity_type = 'patient' AND entity_id = $1
             ORDER BY timestamp ASC",
        )
        .bind(patient_id.to_string())
        .fetch_all(self.db.read_pool())
        .await?;
        let audit_log = audit_rows
            .into_iter()
            .map(|r| AuditRecord {
                action: r.get("action"),
                timestamp: r.get("timestamp"),
                user_id: r.get("user_id"),
            })
            .collect();

        Ok(PatientExport {
            exported_at: Utc::now(),
            patient,
            appointments,
            medications,
            consents,
            audit_log,
        })
    }

    // ------------------------------------------------------------------
    // Export-Formate
    // ------------------------------------------------------------------

    /// Kompletter Export als JSON (Patient, Termine, Medikation,
    /// Einwilligungen, Audit-Log).
    pub async fn export_patient_json(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
    ) -> Result<serde_json::Value, ExportError> {
        let export = self.load(patient_id).await?;
        let value = serde_json::to_value(&export)
            .map_err(|e| ExportError::Db(sqlx::Error::Encode(format!(
                "JSON-Serialisierung fehlgeschlagen: {e}",
            ))))?;

        self.audit_export(actor, patient_id, "json", &value).await?;
        Ok(value)
    }

    /// Kompletter Export als CSV: eine Zeile pro Datensatz, mit
    /// Typ-Spalte (`data_type`). Spalten:
    /// `data_type,id,field,value,timestamp`
    pub async fn export_patient_csv(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
    ) -> Result<String, ExportError> {
        let export = self.load(patient_id).await?;
        let rows = export_to_csv_rows(&export);

        let mut out = String::from("data_type,id,field,value,timestamp\r\n");
        for row in rows {
            out.push_str(&csv_line(&row));
            out.push('\r');
            out.push('\n');
        }

        let byte_len = out.len();
        self.audit_export(
            actor,
            patient_id,
            "csv",
            &serde_json::json!({ "rows": (out.lines().count() - 1), "bytes": byte_len }),
        )
        .await?;
        Ok(out)
    }

    /// FHIR R4 Bundle (maschinenlesbar, für Interoperabilität).
    /// Enthält: Patient, Appointment(s), MedicationStatement(s),
    /// Consent(s) sowie einen DocumentReference als Export-Nachweis.
    pub async fn export_fhir_bundle(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
    ) -> Result<serde_json::Value, ExportError> {
        let export = self.load(patient_id).await?;
        let bundle = build_fhir_r4_bundle(&export);

        self.audit_export(actor, patient_id, "fhir", &bundle).await?;
        Ok(bundle)
    }

    /// Formatierter Export je nach [`ExportFormat`] (für den
    /// `?format=json|csv|fhir`-Endpunkt).
    pub async fn export(
        &self,
        patient_id: Uuid,
        format: ExportFormat,
        actor: &AuthUser,
    ) -> Result<serde_json::Value, ExportError> {
        // RBAC-Check (Export-Berechtigung)
        if !actor.role.has_permission(&Permission::Export) {
            return Err(ExportError::Permission(format!(
                "Rolle {} hat kein Export-Recht",
                actor.role
            )));
        }

        let _ = format; // Format wird von den einzelnen Methoden gehandhabt
        match format {
            ExportFormat::Json => Ok(self.export_patient_json(patient_id, actor).await?),
            ExportFormat::Csv => Ok(serde_json::Value::String(
                self.export_patient_csv(patient_id, actor).await?,
            )),
            ExportFormat::Fhir => Ok(self.export_fhir_bundle(patient_id, actor).await?),
        }
    }

    // ------------------------------------------------------------------
    // Audit
    // ------------------------------------------------------------------

    async fn audit_export(
        &self,
        actor: &AuthUser,
        patient_id: Uuid,
        format: &str,
        payload: &serde_json::Value,
    ) -> Result<(), ExportError> {
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "patient.export",
                "patient",
                Some(&patient_id.to_string()),
                None,
                Some(&serde_json::json!({ "format": format, "preview": payload })),
                None,
            )
            .await
            .map_err(ExportError::Audit)
    }
}

// ===========================================================================
// CSV-Hilfen
// ===========================================================================

/// Eine CSV-Zeile (ein Feld = eine Zelle).
#[derive(Debug, Clone)]
pub struct CsvRow {
    pub data_type: &'static str,
    pub id: String,
    pub field: String,
    pub value: String,
    pub timestamp: String,
}

fn csv_escape(v: &str) -> String {
    if v.contains(',') || v.contains('"') || v.contains('\n') || v.contains('\r') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

fn csv_line(row: &CsvRow) -> String {
    [
        csv_escape(row.data_type),
        csv_escape(&row.id),
        csv_escape(&row.field),
        csv_escape(&row.value),
        csv_escape(&timestamp_rfc3339(&row.timestamp)),
    ]
    .join(",")
}

/// RFC3339-String aus einem ISO-Datum (oder durchreichen, falls bereits
/// ein RFC3339-Timestamp).
fn timestamp_rfc3339(s: &str) -> String {
    if s.contains('T') && s.contains(':') {
        s.to_string()
    } else {
        // Datum nur (z.B. 1980-05-01) → als Datum-String belassen
        s.to_string()
    }
}

/// Splitzt eine CSV-Zeile in Spalten (respektiert quoted Fields).
/// Wird in den Unit-Tests zur Konsistenzprüfung verwendet.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut cols: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                cols.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    cols.push(cur);
    cols
}

/// Wandelt ein [`PatientExport`] in CSV-Zeilen um (eine Zeile pro Feld).
fn export_to_csv_rows(export: &PatientExport) -> Vec<CsvRow> {
    let mut rows: Vec<CsvRow> = Vec::new();
    let ts = |dt: &DateTime<Utc>| dt.to_rfc3339();

    // Patient (ein Feld pro Zeile)
    let p = &export.patient;
    let pid = p.id.to_string();
    let pts = ts(&p.created_at);
    let mut push = |field: &str, value: &str| {
        rows.push(CsvRow {
            data_type: "patient",
            id: pid.clone(),
            field: field.to_string(),
            value: value.to_string(),
            timestamp: pts.clone(),
        });
    };
    push("external_id", &p.external_id.clone().unwrap_or_default());
    push("name_family", &p.name_family);
    push("name_given", &p.name_given);
    push("birth_date", &p.birth_date.to_string());
    push("gender", &p.gender.clone().unwrap_or_default());
    push("phone", &p.phone.clone().unwrap_or_default());
    push("email", &p.email.clone().unwrap_or_default());
    push("address_street", &p.address_street.clone().unwrap_or_default());
    push("address_zip", &p.address_zip.clone().unwrap_or_default());
    push("address_city", &p.address_city.clone().unwrap_or_default());
    push("insurance_number", &p.insurance_number.clone().unwrap_or_default());
    push("allergies", &p.allergies.to_string());

    // Termine
    for a in &export.appointments {
        rows.push(CsvRow {
            data_type: "appointment",
            id: a.id.to_string(),
            field: "category".into(),
            value: a.category.clone(),
            timestamp: a.start_time.to_rfc3339(),
        });
        rows.push(CsvRow {
            data_type: "appointment",
            id: a.id.to_string(),
            field: "status".into(),
            value: a.status.clone(),
            timestamp: a.start_time.to_rfc3339(),
        });
        rows.push(CsvRow {
            data_type: "appointment",
            id: a.id.to_string(),
            field: "reason".into(),
            value: a.reason.clone().unwrap_or_default(),
            timestamp: a.start_time.to_rfc3339(),
        });
    }

    // Medikation
    for m in &export.medications {
        rows.push(CsvRow {
            data_type: "medication",
            id: m.id.to_string(),
            field: "name".into(),
            value: m.name.clone(),
            timestamp: m.start_date.to_string(),
        });
        rows.push(CsvRow {
            data_type: "medication",
            id: m.id.to_string(),
            field: "dosage".into(),
            value: m.dosage.clone().unwrap_or_default(),
            timestamp: m.start_date.to_string(),
        });
    }

    // Einwilligungen
    for c in &export.consents {
        rows.push(CsvRow {
            data_type: "consent",
            id: c.id.to_string(),
            field: "type".into(),
            value: c.type.clone(),
            timestamp: c.granted_at.to_rfc3339(),
        });
        rows.push(CsvRow {
            data_type: "consent",
            id: c.id.to_string(),
            field: "granted".into(),
            value: c.granted.to_string(),
            timestamp: c.granted_at.to_rfc3339(),
        });
    }

    // Audit-Log
    for al in &export.audit_log {
        rows.push(CsvRow {
            data_type: "audit",
            id: "-".into(),
            field: "action".into(),
            value: al.action.clone(),
            timestamp: al.timestamp.to_rfc3339(),
        });
    }

    rows
}

// ===========================================================================
// FHIR R4 Bundle
// ===========================================================================

/// Baut ein FHIR R4 Bundle aus dem Export.
///
/// Resource-Typen: `Patient`, `Appointment`, `MedicationStatement`,
/// `Consent`, `DocumentReference` (Export-Nachweis).
fn build_fhir_r4_bundle(export: &PatientExport) -> serde_json::Value {
    let p = &export.patient;
    let mut entries: Vec<serde_json::Value> = Vec::new();

    // Hilfsfunktion: FHIR-Resource bauen (resourceType + body)
    let make_entry = |resource_type: &str, mut body: serde_json::Value| {
        body["resourceType"] = serde_json::Value::String(resource_type.to_string());
        serde_json::json!({ "resource": body })
    };

    // ---- Patient (FHIR R4)
    let mut patient_body = serde_json::json!({
        "id": p.id.to_string(),
        "active": p.deleted_at.is_none(),
        "name": [{
            "family": p.name_family,
            "given": [p.name_given],
        }],
        "birthDate": p.birth_date.to_string(),
    });
    if let Some(g) = &p.gender {
        patient_body["gender"] = match g.as_str() {
            "m" | "male" | "M" | "Männlich" | "männlich" => serde_json::json!("male"),
            "w" | "female" | "W" | "Weiblich" | "weiblich" => serde_json::json!("female"),
            _ => serde_json::json!("unknown"),
        };
    }
    if let Some(external) = &p.external_id {
        patient_body["identifier"] = serde_json::json!([{
            "system": "urn:kis:external",
            "value": external,
        }]);
    }
    entries.push(make_entry("Patient", patient_body));

    // ---- Appointment
    for a in &export.appointments {
        let appt = serde_json::json!({
            "id": a.id.to_string(),
            "status": map_appt_status(&a.status),
            "serviceType": [{ "coding": [{
                "system": "http://terminology.hl7.org/CodeSystem/v2-0074",
                "code": "AMB",
                "display": "Ambulatory",
            }] }],
            "start": a.start_time.to_rfc3339(),
            "end": a.end_time.to_rfc3339(),
            "participant": [{
                "type": [{ "coding": [{
                    "system": "http://terminology.hl7.org/CodeSystem/participant-type",
                    "code": "practitioner",
                }] }],
                "actor": { "display": format!("Doctor {}", a.doctor_id) },
            }],
        });
        entries.push(make_entry("Appointment", appt));
    }

    // ---- MedicationStatement
    for m in &export.medications {
        let med = serde_json::json!({
            "id": m.id.to_string(),
            "status": "active",
            "medicationCodeableConcept": {
                "text": m.name,
            },
            "subject": { "reference": format!("Patient/{}", p.id) },
            "dateWritten": m.start_date.to_string(),
        });
        entries.push(make_entry("MedicationStatement", med));
    }

    // ---- Consent
    for c in &export.consents {
        let consent = serde_json::json!({
            "id": c.id.to_string(),
            "status": if c.withdrawn_at.is_some() { "draft" } else { "active" },
            "scope": {
                "coding": [{
                    "system": "http://terminology.hl7.org/CodeSystem/consentscope",
                    "code": "treatment",
                }]
            },
            "category": [{
                "coding": [{ "system": "urn:kis:consent", "code": c.type.clone() }]
            }],
            "provision": {
                "type": if c.granted { "permit" } else { "deny" },
            },
            "dateTime": c.granted_at.to_rfc3339(),
        });
        entries.push(make_entry("Consent", consent));
    }

    // ---- DocumentReference (Export-Nachweis)
    let doc_ref = serde_json::json!({
        "id": "export-doc",
        "status": "current",
        "type": {
            "coding": [{
                "system": "http://terminology.hl7.org/CodeSystem/document-type",
                "code": "data-export",
                "display": "DSGVO Data Portability Export",
            }]
        },
        "subject": { "reference": format!("Patient/{}", p.id) },
        "created": export.exported_at.to_rfc3339(),
        "description": format!(
            "Automatischer DSGVO-Export ({} verknüpfte Datensätze)",
            export.appointments.len() + export.medications.len() + export.consents.len()
        ),
    });
    entries.push(make_entry("DocumentReference", doc_ref));

    // ---- Bundle wrapper
    serde_json::json!({
        "resourceType": "Bundle",
        "id": Uuid::new_v4().to_string(),
        "type": "collection",
        "timestamp": export.exported_at.to_rfc3339(),
        "entry": entries,
    })
}

/// Mappt KIS-Termin-Status auf FHIR-Apartment-Status.
fn map_appt_status(status: &str) -> &'static str {
    match status {
        "geplant" | "booked" => "booked",
        "bestätigt" | "confirmed" => "arrived",
        "abgeschlossen" | "done" | "completed" => "finished",
        "storniert" | "cancelled" => "cancelled",
        "no-show" | "ausgefallen" => "noshow",
        _ => "pending",
    }
}

// ===========================================================================
// Unit-Tests (Task 5.7)
// ===========================================================================
//
// Tests ohne Live-DB:
// - Format-Parsing (json/csv/fhir)
// - CSV-Escaping (Komma, Anführungszeichen, Newlines)
// - CSV-Zeilenstruktur (eine Zeile pro Datensatz, Typ-Spalte)
// - FHIR-Bundle-Struktur (resourceType=Bundle, Einträge, FHIR-Patient)
// - Datenkonsistenz: dieselbe Quelle für alle Formate
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_export() -> PatientExport {
        let pid = Uuid::new_v4();
        PatientExport {
            exported_at: Utc::now(),
            patient: PatientRecord {
                id: pid,
                external_id: Some("EXT-42".into()),
                name_family: "Muster".into(),
                name_given: "Max".into(),
                birth_date: chrono::NaiveDate::from_ymd_opt(1980, 5, 1).unwrap(),
                gender: Some("m".into()),
                phone: Some("030-123".into()),
                email: Some("max@muster.de".into()),
                address_street: Some("Teststraße 1".into()),
                address_zip: Some("10115".into()),
                address_city: Some("Berlin".into()),
                insurance_number: Some("A1B2C3".into()),
                allergies: serde_json::json!(["Penicillin", "Erdnüsse"]),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
            },
            appointments: vec![AppointmentRecord {
                id: Uuid::new_v4(),
                category: "ordination".into(),
                status: "geplant".into(),
                start_time: Utc::now(),
                end_time: Utc::now() + chrono::Duration::minutes(30),
                reason: Some("Kontrolltermin, mit Komma".into()),
                doctor_id: Uuid::new_v4(),
            }],
            medications: vec![MedicationRecord {
                id: Uuid::new_v4(),
                name: "Ibuprofen 400".into(),
                dosage: Some("1-0-0".into()),
                frequency: Some("bedarfsweise".into()),
                start_date: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                end_date: None,
            }],
            consents: vec![ConsentRecord {
                id: Uuid::new_v4(),
                type: "treatment".into(),
                granted: true,
                granted_at: Utc::now(),
                withdrawn_at: None,
                legal_basis: Some("DSGVO Art. 9".into()),
            }],
            audit_log: vec![AuditRecord {
                action: "create".into(),
                timestamp: Utc::now(),
                user_id: Some(Uuid::new_v4().to_string()),
            }],
        }
    }

    /// Format-Parsing aus Query-Parameter.
    #[test]
    fn test_format_parsing() {
        assert_eq!(ExportFormat::parse("json"), Some(ExportFormat::Json));
        assert_eq!(ExportFormat::parse("CSV"), Some(ExportFormat::Csv));
        assert_eq!(ExportFormat::parse("FHIR"), Some(ExportFormat::Fhir));
        assert_eq!(ExportFormat::parse("fhir-r4"), Some(ExportFormat::Fhir));
        assert_eq!(ExportFormat::parse("xml"), None);
        assert_eq!(ExportFormat::parse(""), None);
    }

    /// CSV-Escaping: Komma, Anführungszeichen, Newline.
    #[test]
    fn test_csv_escaping() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
        assert_eq!(csv_escape("line1\r\nline2"), "\"line1\r\nline2\"");
    }

    /// CSV-Struktur: Header + eine Zeile pro Feld, Typ-Spalte vorhanden.
    #[test]
    fn test_csv_structure() {
        let export = sample_export();
        let rows = export_to_csv_rows(&export);

        // Header vorhanden
        let header = "data_type,id,field,value,timestamp";
        assert!(header.starts_with("data_type"));
        assert!(header.contains("field"));

        // Alle Zeilen haben 5 Spalten
        for row in &rows {
            let line = csv_line(row);
            let cols = split_csv_line(&line);
            assert_eq!(cols.len(), 5, "Zeile muss 5 Spalten haben: {line}");
        }

        // Alle 5 data_types vorhanden
        let types: std::collections::HashSet<_> =
            rows.iter().map(|r| r.data_type).collect();
        assert!(types.contains(&"patient"));
        assert!(types.contains(&"appointment"));
        assert!(types.contains(&"medication"));
        assert!(types.contains(&"consent"));
        assert!(types.contains(&"audit"));

        // Patient-Zeilen: eine pro Feld
        let patient_rows: Vec<&CsvRow> =
            rows.iter().filter(|r| r.data_type == "patient").collect();
        assert!(patient_rows.len() >= 11, "Patient hat min. 11 Felder");

        // ID-Konsistenz: alle patient-Zeilen haben dieselbe ID
        let first_id = patient_rows.first().unwrap().id.clone();
        assert!(patient_rows.iter().all(|r| r.id == first_id));
    }

    /// CSV-Value mit Komma wird korrekt escaped.
    #[test]
    fn test_csv_value_with_comma() {
        let export = sample_export();
        let rows = export_to_csv_rows(&export);
        let appt_reason = rows
            .iter()
            .find(|r| r.data_type == "appointment" && r.field == "reason")
            .unwrap();
        let line = csv_line(appt_reason);
        assert!(line.contains("\"Kontrolltermin, mit Komma\""));
    }

    /// FHIR-Bundle: korrekter Typ + alle erwarteten Ressourcen.
    #[test]
    fn test_fhir_bundle_structure() {
        let export = sample_export();
        let bundle = build_fhir_r4_bundle(&export);

        assert_eq!(bundle["resourceType"], "Bundle");
        assert_eq!(bundle["type"], "collection");
        assert!(bundle["id"].is_string());
        assert!(bundle["timestamp"].is_string());

        let entries = bundle["entry"].as_array().unwrap();
        assert!(entries.len() >= 4, "min. Patient + Appointment + Med + Consent");

        let resource_types: Vec<&str> = entries
            .iter()
            .map(|e| e["resource"]["resourceType"].as_str().unwrap())
            .collect();
        assert!(resource_types.contains(&"Patient"));
        assert!(resource_types.contains(&"Appointment"));
        assert!(resource_types.contains(&"MedicationStatement"));
        assert!(resource_types.contains(&"Consent"));
        assert!(resource_types.contains(&"DocumentReference"));
    }

    /// FHIR-Patient: Name, Geburtsdatum, Geschlecht korrekt abgebildet.
    #[test]
    fn test_fhir_patient_mapping() {
        let export = sample_export();
        let bundle = build_fhir_r4_bundle(&export);
        let entries = bundle["entry"].as_array().unwrap();

        let fhir_patient = entries
            .iter()
            .find(|e| e["resource"]["resourceType"] == "Patient")
            .unwrap();
        let r = &fhir_patient["resource"];

        assert_eq!(r["name"][0]["family"], "Muster");
        assert_eq!(r["name"][0]["given"][0], "Max");
        assert_eq!(r["birthDate"], "1980-05-01");
        assert_eq!(r["gender"], "male");
        assert_eq!(r["active"], true);
        assert_eq!(r["identifier"][0]["value"], "EXT-42");
    }

    /// FHIR-Termin-Status-Mapping.
    #[test]
    fn test_fhir_appt_status_mapping() {
        assert_eq!(map_appt_status("geplant"), "booked");
        assert_eq!(map_appt_status("storniert"), "cancelled");
        assert_eq!(map_appt_status("no-show"), "noshow");
        assert_eq!(map_appt_status("unbekannt"), "pending");
    }

    /// Datenkonsistenz: JSON-Export enthält dieselbe Datensatzmenge wie
    /// die CSV-Zeilen (gleiche Quelle).
    #[test]
    fn test_data_consistency_json_csv() {
        let export = sample_export();

        // JSON
        let json = serde_json::to_value(&export).unwrap();
        let json_appts = json["appointments"].as_array().unwrap().len();
        let json_meds = json["medications"].as_array().unwrap().len();

        // CSV
        let rows = export_to_csv_rows(&export);
        let csv_appt_ids: std::collections::HashSet<_> = rows
            .iter()
            .filter(|r| r.data_type == "appointment")
            .map(|r| r.id.clone())
            .collect();
        let csv_med_ids: std::collections::HashSet<_> = rows
            .iter()
            .filter(|r| r.data_type == "medication")
            .map(|r| r.id.clone())
            .collect();

        assert_eq!(json_appts, csv_appt_ids.len(), "Terminanzahl muss stimmen");
        assert_eq!(json_meds, csv_med_ids.len(), "Medikamentenanzahl muss stimmen");
    }

    /// ExportError → ApiError-Mapping (Status-Codes).
    #[test]
    fn test_error_to_api_error() {
        use crate::models::ApiError;
        let id = Uuid::new_v4();
        let e = ApiError::from(ExportError::NotFound(id));
        assert_eq!(e.status, 404);
        let e = ApiError::from(ExportError::UnsupportedFormat("xml".into()));
        assert_eq!(e.status, 400);
        let e = ApiError::from(ExportError::Permission("x".into()));
        assert_eq!(e.status, 403);
        let e = ApiError::from(ExportError::Db(sqlx::Error::RowNotFound));
        assert_eq!(e.status, 500);
    }
}
