//! CGM MEDICO FHIR R4 Client
//!
//! Bidirektionaler Datenaustausch mit CGM MEDICO über FHIR R4,
//! OAuth 2.0 (Client Credentials), ISiK-konform.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{error, info};


/// MEDICO FHIR Client mit OAuth 2.0 und Auto-Refresh
pub struct MedicoClient {
    base_url: String,
    client: Client,
    token: Option<String>,
    token_expiry: Option<DateTime<Utc>>,
    scopes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MedicoError {
    #[error("OAuth Token-Fehler: {0}")]
    AuthError(String),
    #[error("HTTP-Fehler: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("FHIR-Parsing-Fehler: {0}")]
    ParseError(String),
    #[error("MEDICO-Endpunkt nicht erreichbar: {0}")]
    Unreachable(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(default)]
    pub scope: Option<String>,
}

/// MEDICO-Patient (FHIR R4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoPatient {
    pub id: String,
    #[serde(rename = "resourceType")]
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub name: Vec<MedicoName>,
    #[serde(rename = "birthDate", default)]
    pub birth_date: Option<String>,
    #[serde(default)]
    pub gender: Option<String>,
    #[serde(default)]
    pub identifier: Vec<MedicoIdentifier>,
    #[serde(default)]
    pub address: Vec<MedicoAddress>,
    #[serde(default)]
    pub telecom: Vec<MedicoTelecom>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoName {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub given: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoIdentifier {
    pub system: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoAddress {
    #[serde(default)]
    pub line: Vec<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoTelecom {
    pub system: String,
    pub value: String,
    #[serde(default)]
    pub use_type: Option<String>,
}

/// MEDICO-Termin (FHIR R4 Appointment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoAppointment {
    pub id: String,
    #[serde(rename = "resourceType")]
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub service_type: Vec<MedicoCoding>,
    #[serde(default)]
    pub participant: Vec<MedicoParticipant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoCoding {
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoParticipant {
    #[serde(default)]
    pub actor: Option<MedicoReference>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoReference {
    pub reference: String,
    #[serde(default)]
    pub display: Option<String>,
}

/// MEDICO-Slot (FHIR R4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoSlot {
    pub id: String,
    #[serde(rename = "resourceType")]
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub appointment: Option<MedicoReference>,
}

/// FHIR Bundle (Antwort auf Listen-Requests)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(rename = "resourceType")]
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub entry: Vec<BundleEntry>,
    #[serde(default)]
    pub total: Option<i64>,
    #[serde(default)]
    pub link: Vec<MedicoLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEntry {
    #[serde(default)]
    pub full_url: Option<String>,
    #[serde(default)]
    pub resource: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoLink {
    pub relation: String,
    pub url: String,
}

/// Changes von MEDICO (für Change-Detection)
#[derive(Debug, Default)]
pub struct MedicoChanges {
    pub patients: Vec<MedicoPatient>,
    pub appointments: Vec<MedicoAppointment>,
    pub slots: Vec<MedicoSlot>,
}

impl MedicoClient {
    /// Neue MEDICO-Client-Instanz
    ///
    /// `base_url`: z.B. "https://fhir.medico.example.com/fhir/"
    /// `scopes`: OAuth 2.0 Scopes
    pub fn new(base_url: &str, scopes: Vec<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP Client konnte nicht erstellt werden");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            token: None,
            token_expiry: None,
            scopes,
        }
    }

    /// OAuth 2.0 Client Credentials Flow
    ///
    /// Credentials kommen ausschließlich aus ENV-Variablen, nie aus Code.
    pub async fn get_token(&mut self, client_id: &str, client_secret: &str) -> Result<String, MedicoError> {
        let scope = self.scopes.join(" ");
        let response = self
            .client
            .post(format!("{}/auth/token", self.base_url))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("scope", &scope),
            ])
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(status = %status, body = %body, "MEDICO OAuth Token-Fehler");
            return Err(MedicoError::AuthError(format!(
                "OAuth Token-Request fehlgeschlagen ({}): {}",
                status, body
            )));
        }

        let body: TokenResponse = response
            .json()
            .await
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;

        info!(
            expires_in = body.expires_in,
            scope = ?body.scope,
            "MEDICO OAuth Token erfolgreich"
        );

        self.token = Some(body.access_token.clone());
        self.token_expiry = Some(Utc::now() + chrono::Duration::seconds(body.expires_in - 60));
        Ok(body.access_token)
    }

    /// Token prüfen und bei Bedarf aktualisieren (Auto-Refresh)
    pub async fn ensure_token(&mut self) -> Result<(), MedicoError> {
        if let Some(expiry) = &self.token_expiry {
            if Utc::now() < *expiry {
                return Ok(());
            }
        }
        let client_id = std::env::var("KIS_MEDICO_CLIENT_ID")
            .map_err(|_| MedicoError::AuthError("KIS_MEDICO_CLIENT_ID nicht gesetzt".into()))?;
        let client_secret = std::env::var("KIS_MEDICO_CLIENT_SECRET")
            .map_err(|_| MedicoError::AuthError("KIS_MEDICO_CLIENT_SECRET nicht gesetzt".into()))?;
        self.get_token(&client_id, &client_secret)
            .await?;
        Ok(())
    }

    /// Patienten aus MEDICO importieren (seit letztem Update)
    pub async fn import_patients(
        &mut self,
        since: &DateTime<Utc>,
    ) -> Result<Vec<MedicoPatient>, MedicoError> {
        self.ensure_token().await?;
        let token = self.token.as_ref().unwrap().clone();

        let url = format!(
            "{}/Patient?_lastUpdated=ge{}&_count=100",
            self.base_url,
            since.to_rfc3339()
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/fhir+json")
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            error!(status = %status, "MEDICO Patient-Import fehlgeschlagen");
            return Err(MedicoError::HttpError(response.error_for_status().unwrap_err()));
        }

        let bundle: Bundle = response
            .json()
            .await
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;

        let patients: Vec<MedicoPatient> = bundle
            .entry
            .iter()
            .filter_map(|e| {
                e.resource
                    .as_ref()
                    .and_then(|r| serde_json::from_value(r.clone()).ok())
            })
            .collect();

        info!(count = patients.len(), "MEDICO Patient-Import");
        Ok(patients)
    }

    /// Termine aus MEDICO importieren
    pub async fn import_appointments(
        &mut self,
        since: &DateTime<Utc>,
    ) -> Result<Vec<MedicoAppointment>, MedicoError> {
        self.ensure_token().await?;
        let token = self.token.as_ref().unwrap().clone();

        let url = format!(
            "{}/Appointment?_lastUpdated=ge{}&_count=100",
            self.base_url,
            since.to_rfc3339()
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/fhir+json")
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        let bundle: Bundle = response
            .json()
            .await
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;

        let appointments: Vec<MedicoAppointment> = bundle
            .entry
            .iter()
            .filter_map(|e| {
                e.resource
                    .as_ref()
                    .and_then(|r| serde_json::from_value(r.clone()).ok())
            })
            .collect();

        info!(count = appointments.len(), "MEDICO Termin-Import");
        Ok(appointments)
    }

    /// Slots aus MEDICO importieren (für angegebene Tagesliste)
    pub async fn import_slots(&mut self, date: &str) -> Result<Vec<MedicoSlot>, MedicoError> {
        self.ensure_token().await?;
        let token = self.token.as_ref().unwrap().clone();

        let url = format!("{}/Slot?start={}&_count=200", self.base_url, date);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/fhir+json")
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        let bundle: Bundle = response
            .json()
            .await
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;

        let slots: Vec<MedicoSlot> = bundle
            .entry
            .iter()
            .filter_map(|e| {
                e.resource
                    .as_ref()
                    .and_then(|r| serde_json::from_value(r.clone()).ok())
            })
            .collect();

        info!(date = %date, count = slots.len(), "MEDICO Slot-Import");
        Ok(slots)
    }

    /// Patienten in MEDICO exportieren (bidirektional)
    pub async fn export_patient(
        &mut self,
        patient: &MedicoPatient,
    ) -> Result<String, MedicoError> {
        self.ensure_token().await?;
        let token = self.token.as_ref().unwrap().clone();

        let response = self
            .client
            .post(format!("{}/Patient", self.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/fhir+json")
            .json(patient)
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            error!(status = %status, "MEDICO Patient-Export fehlgeschlagen");
            return Err(MedicoError::HttpError(response.error_for_status().unwrap_err()));
        }

        let created: MedicoPatient = response
            .json()
            .await
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;

        info!(patient_id = %created.id, "MEDICO Patient-Export");
        Ok(created.id)
    }

    /// Termin in MEDICO buchen (ISiK $book Operation)
    pub async fn book_appointment(
        &mut self,
        appointment: &MedicoAppointment,
    ) -> Result<String, MedicoError> {
        self.ensure_token().await?;
        let token = self.token.as_ref().unwrap().clone();

        let response = self
            .client
            .post(format!("{}/Appointment/$book", self.base_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/fhir+json")
            .json(appointment)
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            error!(status = %status, "MEDICO Termin-Buchung fehlgeschlagen");
            return Err(MedicoError::HttpError(response.error_for_status().unwrap_err()));
        }

        let result: MedicoAppointment = response
            .json()
            .await
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;

        info!(appointment_id = %result.id, "MEDICO Termin gebucht");
        Ok(result.id)
    }

    /// Termin in MEDICO stornieren
    pub async fn cancel_appointment(
        &mut self,
        appointment_id: &str,
        cancel_reason: &str,
    ) -> Result<(), MedicoError> {
        self.ensure_token().await?;
        let token = self.token.as_ref().unwrap().clone();

        let response = self
            .client
            .delete(format!("{}/Appointment/{}", self.base_url, appointment_id))
            .header("Authorization", format!("Bearer {}", token))
            .query(&[
                ("cancelled-appt-id", appointment_id),
                ("reason", cancel_reason),
            ])
            .send()
            .await
            .map_err(|e| MedicoError::Unreachable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            error!(
                status = %status,
                appointment_id = %appointment_id,
                "MEDICO Termin-Storno fehlgeschlagen"
            );
            return Err(MedicoError::HttpError(response.error_for_status().unwrap_err()));
        }

        info!(appointment_id = %appointment_id, "MEDICO Termin storniert");
        Ok(())
    }

    /// Health Check: Ist MEDICO erreichbar?
    pub async fn health_check(&mut self) -> bool {
        self.ensure_token().await
            .map(|_| true)
            .unwrap_or(false)
    }
}

/// Change-Detection: Polling + ID-Tracking
pub struct MedicoChangeDetector {
    last_processed: HashMap<String, DateTime<Utc>>,
    processed_ids: std::collections::HashSet<String>,
}

impl MedicoChangeDetector {
    pub fn new() -> Self {
        Self {
            last_processed: HashMap::new(),
            processed_ids: std::collections::HashSet::new(),
        }
    }

    /// Polling: Neue/geänderte Daten von MEDICO abholen
    pub async fn poll_changes(
        &mut self,
        client: &mut MedicoClient,
    ) -> Result<MedicoChanges, MedicoError> {
        let mut changes = MedicoChanges::default();

        // Patienten
        let since = self
            .last_processed
            .get("patient")
            .copied()
            .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24));

        let patients = client.import_patients(&since).await?;
        for p in patients {
            let key = format!("patient:{}", p.id);
            if self.processed_ids.insert(key) {
                changes.patients.push(p);
            }
        }
        self.last_processed.insert("patient".into(), Utc::now());

        // Termine
        let since = self
            .last_processed
            .get("appointment")
            .copied()
            .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24));

        let appointments = client.import_appointments(&since).await?;
        for a in appointments {
            let key = format!("appointment:{}", a.id);
            if self.processed_ids.insert(key) {
                changes.appointments.push(a);
            }
        }
        self.last_processed.insert("appointment".into(), Utc::now());

        Ok(changes)
    }
}

/// ID-Mapping zwischen MEDICO und KIS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdMapping {
    pub id: String,
    pub externo_system: String,
    pub externo_id: String,
    pub kiso_id: String,
    pub entity_type: String,
    pub created_at: DateTime<Utc>,
}

/// GDT-Export: Patientenstammdaten als CSV
pub fn export_gdt(
    patients: &[MedicoPatient],
    output_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut buf = Vec::new();
    buf.write_all(b"GDT-1;Patientenstammdaten;KIS;1.0;")?;
    buf.write_all(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string().as_bytes())?;
    buf.write_all(b"\n")?;

    for p in patients {
        let last = p.name.first().and_then(|n| n.family.as_deref()).unwrap_or("");
        let first = p.name.first().and_then(|n| n.given.first()).map(String::as_str).unwrap_or("");
        let birth = p.birth_date.as_deref().unwrap_or("");
        let gender = p.gender.as_deref().unwrap_or("");
        let insurance = p
            .identifier
            .iter()
            .find(|i| i.system.contains("MR") || i.system.contains("Krankenkasse"))
            .map(|i| i.value.as_str())
            .unwrap_or("");
        let address = p
            .address
            .first()
            .and_then(|a| a.line.first())
            .map(String::as_str)
            .unwrap_or("");
        let phone = p
            .telecom
            .iter()
            .find(|t| t.system == "phone")
            .map(|t| t.value.as_str())
            .unwrap_or("");

        buf.write_all(
            format!(
                "{};{};{};{};{};{};{};{}\n",
                p.id, last, first, birth, gender, insurance, address, phone
            )
            .as_bytes(),
        )?;
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patient_parsing() {
        let json = r#"{
            "resourceType": "Patient",
            "id": "MEDICO-001",
            "name": [{"family": "Muster", "given": ["Max"]}],
            "birthDate": "1980-01-15",
            "gender": "male",
            "identifier": [{"system": "MR", "value": "123456789"}]
        }"#;
        let patient: MedicoPatient = serde_json::from_str(json).unwrap();
        assert_eq!(patient.id, "MEDICO-001");
        assert_eq!(patient.name[0].family, Some("Muster".into()));
        assert_eq!(patient.birth_date, Some("1980-01-15".into()));
    }

    #[test]
    fn test_appointment_parsing() {
        let json = r#"{
            "resourceType": "Appointment",
            "id": "MEDICO-APT-001",
            "status": "booked",
            "start": "2026-11-02T09:00:00Z",
            "end": "2026-11-02T09:30:00Z"
        }"#;
        let appt: MedicoAppointment = serde_json::from_str(json).unwrap();
        assert_eq!(appt.id, "MEDICO-APT-001");
        assert_eq!(appt.status, Some("booked".into()));
    }

    #[test]
    fn test_bundle_parsing() {
        let json = r#"{
            "resourceType": "Bundle",
            "entry": [
                {"resource": {"resourceType": "Patient", "id": "P1"}},
                {"resource": {"resourceType": "Patient", "id": "P2"}}
            ],
            "total": 2
        }"#;
        let bundle: Bundle = serde_json::from_str(json).unwrap();
        assert_eq!(bundle.entry.len(), 2);
        assert_eq!(bundle.total, Some(2));
    }

    #[test]
    fn test_gdt_export() {
        let patients = vec![
            MedicoPatient {
                id: "MEDICO-001".into(),
                resource_type: "Patient".into(),
                name: vec![MedicoName {
                    family: Some("Muster".into()),
                    given: vec!["Max".into()],
                }],
                birth_date: Some("1980-01-15".into()),
                gender: Some("male".into()),
                identifier: vec![MedicoIdentifier {
                    system: "MR".into(),
                    value: "123456789".into(),
                }],
                address: vec![],
                telecom: vec![],
            },
        ];

        let path = std::path::PathBuf::from("/tmp/test_gdt.csv");
        export_gdt(&patients, &path).unwrap();
        assert!(path.exists());
        std::fs::remove_file(path).ok();
    }
}
