//! CGM MEDICO FHIR R4 Client
//!
//! Bidirektionaler Datenaustausch mit CGM MEDICO über FHIR R4,
//! OAuth 2.0 (Client Credentials). Keine nachgewiesene ISiK-Konformität.

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
    auth_endpoint: String,
    client_id_env: String,
    client_secret_env: String,
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
#[serde(rename_all = "camelCase")]
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
    #[serde(rename = "use")]
    pub use_type: Option<String>,
}

/// MEDICO-Termin (FHIR R4 Appointment)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub service_type: Vec<MedicoCodeableConcept>,
    #[serde(default)]
    pub participant: Vec<MedicoParticipant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoCodeableConcept {
    #[serde(default)]
    pub coding: Vec<MedicoCoding>,
    #[serde(default)]
    pub text: Option<String>,
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
#[serde(rename_all = "camelCase")]
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
    pub fn new(base_url: &str, scopes: Vec<String>) -> Result<Self, MedicoError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let base_url = base_url.trim_end_matches('/').to_string();
        Ok(Self {
            auth_endpoint: format!("{base_url}/auth/token"),
            base_url,
            client,
            token: None,
            token_expiry: None,
            scopes,
            client_id_env: "KIS_MEDICO_CLIENT_ID".into(),
            client_secret_env: "KIS_MEDICO_CLIENT_SECRET".into(),
        })
    }

    pub fn from_config(config: &crate::config::MedicoInterfaceConfig) -> Result<Self, MedicoError> {
        let mut client = Self::new(&config.base_url, config.scopes.clone())?;
        client.auth_endpoint = config.auth_endpoint.clone();
        client.client_id_env = config.client_id_env.clone();
        client.client_secret_env = config.client_secret_env.clone();
        client.client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_seconds))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(client)
    }

    /// OAuth 2.0 Client Credentials Flow
    ///
    /// Credentials kommen ausschließlich aus ENV-Variablen, nie aus Code.
    pub async fn get_token(
        &mut self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<String, MedicoError> {
        let scope = self.scopes.join(" ");
        let response = self
            .client
            .post(&self.auth_endpoint)
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
            error!(status = %status, "MEDICO OAuth Token-Fehler");
            return Err(MedicoError::AuthError(format!(
                "OAuth Token-Request fehlgeschlagen ({})",
                status
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

        if body.expires_in <= 0
            || body.access_token.is_empty()
            || !body.token_type.eq_ignore_ascii_case("bearer")
        {
            return Err(MedicoError::AuthError(
                "Ungültige OAuth-Tokenantwort".into(),
            ));
        }
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
        let client_id = std::env::var(&self.client_id_env)
            .map_err(|_| MedicoError::AuthError("KIS_MEDICO_CLIENT_ID nicht gesetzt".into()))?;
        let client_secret = std::env::var(&self.client_secret_env)
            .map_err(|_| MedicoError::AuthError("KIS_MEDICO_CLIENT_SECRET nicht gesetzt".into()))?;
        self.get_token(&client_id, &client_secret).await?;
        Ok(())
    }

    async fn fetch_resources<T: serde::de::DeserializeOwned>(
        &mut self,
        resource_type: &str,
        parameters: &[(&str, String)],
    ) -> Result<Vec<T>, MedicoError> {
        self.ensure_token().await?;
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| MedicoError::AuthError("Token fehlt".into()))?
            .clone();
        let base = reqwest::Url::parse(&format!("{}/", self.base_url))
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;
        let mut page = base
            .join(resource_type)
            .map_err(|e| MedicoError::ParseError(e.to_string()))?;
        for (key, value) in parameters {
            page.query_pairs_mut().append_pair(key, value);
        }
        page.query_pairs_mut().append_pair("_count", "100");
        let mut visited = std::collections::HashSet::new();
        let mut resources = Vec::new();
        loop {
            if page.origin() != base.origin()
                || !page.path().starts_with(base.path())
                || !page.username().is_empty()
                || page.password().is_some()
            {
                return Err(MedicoError::ParseError(
                    "Paging-Link verlässt die konfigurierte FHIR-Basis".into(),
                ));
            }
            if visited.len() >= 10000 || !visited.insert(page.as_str().to_owned()) {
                return Err(MedicoError::ParseError(
                    "Zyklisches oder zu langes FHIR-Paging".into(),
                ));
            }
            let response = self
                .client
                .get(page.clone())
                .bearer_auth(&token)
                .header("Accept", "application/fhir+json")
                .send()
                .await?
                .error_for_status()?;
            let bundle: Bundle = response.json().await?;
            if bundle.resource_type != "Bundle" {
                return Err(MedicoError::ParseError(
                    "FHIR-Antwort ist kein Bundle".into(),
                ));
            }
            for entry in bundle.entry {
                let value = entry.resource.ok_or_else(|| {
                    MedicoError::ParseError("Bundle-Eintrag ohne Ressource".into())
                })?;
                if value.get("resourceType").and_then(|v| v.as_str()) != Some(resource_type) {
                    return Err(MedicoError::ParseError(
                        "Unerwarteter Ressourcentyp im Suchergebnis".into(),
                    ));
                }
                resources.push(
                    serde_json::from_value(value)
                        .map_err(|e| MedicoError::ParseError(e.to_string()))?,
                );
            }
            let next: Vec<_> = bundle
                .link
                .iter()
                .filter(|link| link.relation == "next")
                .collect();
            match next.as_slice() {
                [] => break,
                [link] => {
                    page = page
                        .join(&link.url)
                        .map_err(|e| MedicoError::ParseError(e.to_string()))?
                }
                _ => {
                    return Err(MedicoError::ParseError(
                        "Mehrere next-Links im Bundle".into(),
                    ))
                }
            }
        }
        Ok(resources)
    }

    pub async fn import_patients(
        &mut self,
        since: &DateTime<Utc>,
    ) -> Result<Vec<MedicoPatient>, MedicoError> {
        self.fetch_resources(
            "Patient",
            &[("_lastUpdated", format!("ge{}", since.to_rfc3339()))],
        )
        .await
    }
    pub async fn import_appointments(
        &mut self,
        since: &DateTime<Utc>,
    ) -> Result<Vec<MedicoAppointment>, MedicoError> {
        self.fetch_resources(
            "Appointment",
            &[("_lastUpdated", format!("ge{}", since.to_rfc3339()))],
        )
        .await
    }
    pub async fn import_slots(&mut self, date: &str) -> Result<Vec<MedicoSlot>, MedicoError> {
        self.fetch_resources("Slot", &[("start", date.to_string())])
            .await
    }

    /// Patienten in MEDICO exportieren (für Rollback während Migrationsphase)
    pub async fn export_patient(&mut self, patient: &MedicoPatient) -> Result<String, MedicoError> {
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
            return Err(MedicoError::HttpError(
                response.error_for_status().unwrap_err(),
            ));
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
            return Err(MedicoError::HttpError(
                response.error_for_status().unwrap_err(),
            ));
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
            return Err(MedicoError::HttpError(
                response.error_for_status().unwrap_err(),
            ));
        }

        info!(appointment_id = %appointment_id, "MEDICO Termin storniert");
        Ok(())
    }

    /// Health Check: Ist MEDICO erreichbar?
    pub async fn health_check(&mut self) -> bool {
        self.ensure_token().await.map(|_| true).unwrap_or(false)
    }
}

/// Change-Detection: Polling + ID-Tracking
#[derive(Default)]
pub struct MedicoChangeDetector {
    last_processed: HashMap<String, DateTime<Utc>>,
}

impl MedicoChangeDetector {
    pub fn new() -> Self {
        Self {
            last_processed: HashMap::new(),
        }
    }

    /// Polling: Neue/geänderte Daten von MEDICO abholen
    pub async fn poll_changes(
        &mut self,
        client: &mut MedicoClient,
    ) -> Result<MedicoChanges, MedicoError> {
        let poll_started = Utc::now();
        let mut changes = MedicoChanges::default();

        // Patienten
        let since = self
            .last_processed
            .get("patient")
            .copied()
            .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);

        let patients = client.import_patients(&since).await?;
        changes.patients = patients;

        // Termine
        let since = self
            .last_processed
            .get("appointment")
            .copied()
            .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);

        let appointments = client.import_appointments(&since).await?;
        changes.appointments = appointments;
        // Wiederholungen sind möglich und müssen beim DB-Upsert idempotent sein.
        // Kein dauerhaftes Verwerfen aller späteren Versionen derselben ID.
        self.last_processed.insert("patient".into(), poll_started);
        self.last_processed
            .insert("appointment".into(), poll_started);

        Ok(changes)
    }
}

/// ID-Mapping zwischen MEDICO und KIS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdMapping {
    pub id: String,
    pub external_system: String,
    pub external_id: String,
    pub kis_id: String,
    pub entity_type: String,
    pub created_at: DateTime<Utc>,
}

/// Semikolon-CSV für Entwicklung; kein standardkonformer GDT-Export.
pub fn export_patients_csv(
    patients: &[MedicoPatient],
    output_path: &std::path::Path,
) -> std::io::Result<()> {
    use std::io::Write;

    let mut buf = Vec::new();
    buf.write_all(b"id;last_name;first_name;birth_date;gender;identifier;address;phone\n")?;

    for p in patients {
        let last = p
            .name
            .first()
            .and_then(|n| n.family.as_deref())
            .unwrap_or("");
        let first = p
            .name
            .first()
            .and_then(|n| n.given.first())
            .map(String::as_str)
            .unwrap_or("");
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

        let fields = [
            p.id.as_str(),
            last,
            first,
            birth,
            gender,
            insurance,
            address,
            phone,
        ];
        let row = fields
            .iter()
            .map(|field| format!("\"{}\"", field.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(";");
        writeln!(&mut buf, "{row}")?;
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

    async fn mock_client(router: axum::Router) -> (MedicoClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/fhir/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut client = MedicoClient::new(&base, vec![]).unwrap();
        client.token = Some("mock-token".into());
        client.token_expiry = Some(Utc::now() + chrono::Duration::hours(1));
        (client, task)
    }

    #[tokio::test]
    async fn import_follows_all_pages() {
        use axum::{extract::Query, routing::get, Json};
        let router = axum::Router::new().route("/fhir/Patient", get(|Query(parameters): Query<HashMap<String, String>>| async move {
            let second = parameters.get("page").is_some_and(|p| p == "2");
            let entries: Vec<_> = (if second { 100..151 } else { 0..100 }).map(|id|
                serde_json::json!({"resource":{"resourceType":"Patient", "id":format!("P{id}")}})).collect();
            Json(serde_json::json!({"resourceType":"Bundle", "type":"searchset", "entry":entries,
                "link":if second { vec![] } else { vec![serde_json::json!({"relation":"next", "url":"?page=2"})] }}))
        }));
        let (mut client, task) = mock_client(router).await;
        let patients = client
            .import_patients(&DateTime::<Utc>::UNIX_EPOCH)
            .await
            .unwrap();
        assert_eq!(patients.len(), 151);
        assert_eq!(patients.last().unwrap().id, "P150");
        task.abort();
    }

    #[tokio::test]
    async fn later_versions_of_same_resource_are_not_dropped() {
        use axum::{routing::get, Json};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let version = Arc::new(AtomicUsize::new(0));
        let router = axum::Router::new()
            .route("/fhir/Patient", get(move || { let version = version.clone(); async move {
                let number = version.fetch_add(1, Ordering::SeqCst);
                Json(serde_json::json!({"resourceType":"Bundle", "entry":[{"resource":{"resourceType":"Patient", "id":"P1", "name":[{"family":number.to_string()}]}}]}))
            }}))
            .route("/fhir/Appointment", get(|| async { Json(serde_json::json!({"resourceType":"Bundle", "entry":[]})) }));
        let (mut client, task) = mock_client(router).await;
        let mut detector = MedicoChangeDetector::new();
        let first = detector.poll_changes(&mut client).await.unwrap();
        let second = detector.poll_changes(&mut client).await.unwrap();
        assert_eq!(first.patients.len(), 1);
        assert_eq!(second.patients.len(), 1);
        assert_ne!(
            first.patients[0].name[0].family,
            second.patients[0].name[0].family
        );
        task.abort();
    }

    #[tokio::test]
    async fn errors_and_foreign_paging_links_are_rejected() {
        use axum::{http::StatusCode, routing::get, Json};
        let router = axum::Router::new()
            .route("/fhir/Patient", get(|| async { Json(serde_json::json!({"resourceType":"Bundle", "link":[{"relation":"next", "url":"http://127.0.0.1:1/stolen"}]})) }))
            .route("/fhir/Appointment", get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"resourceType":"OperationOutcome"}))) }))
            .route("/fhir/Slot", get(|| async { Json(serde_json::json!({"resourceType":"Bundle", "entry":[{"resource":{"resourceType":"Slot"}}]})) }));
        let (mut client, task) = mock_client(router).await;
        assert!(matches!(
            client.import_patients(&DateTime::<Utc>::UNIX_EPOCH).await,
            Err(MedicoError::ParseError(_))
        ));
        assert!(client
            .import_appointments(&DateTime::<Utc>::UNIX_EPOCH)
            .await
            .is_err());
        assert!(client.import_slots("2026-11-02").await.is_err());
        task.abort();
    }

    #[test]
    fn fhir_camel_case_fields_survive_roundtrip() {
        let address: MedicoAddress =
            serde_json::from_value(serde_json::json!({"postalCode":"12345"})).unwrap();
        assert_eq!(address.postal_code.as_deref(), Some("12345"));
        assert_eq!(
            serde_json::to_value(address).unwrap()["postalCode"],
            "12345"
        );
        let appointment: MedicoAppointment = serde_json::from_value(serde_json::json!({"id":"A1", "serviceType":[{"coding":[{"code":"consult"}], "text":"Visit"}]})).unwrap();
        assert_eq!(
            appointment.service_type[0].coding[0].code.as_deref(),
            Some("consult")
        );
        let entry: BundleEntry = serde_json::from_value(
            serde_json::json!({"fullUrl":"https://example.test/Patient/P1"}),
        )
        .unwrap();
        assert!(entry.full_url.is_some());
    }

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
    fn test_csv_export() {
        let patients = vec![MedicoPatient {
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
        }];

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("patients.csv");
        export_patients_csv(&patients, &path).unwrap();
        assert!(path.exists());
        std::fs::remove_file(path).ok();
    }
}
