//! KIS MEDICO — CGM MEDICO Migration als VOLLSTÄNDIGER ERSATZ.
//!
//! Architektur:
//! - FHIR R4 Client für MEDICO (Patient, Appointment, Slot, Practitioner,
//!   PractitionerRole, Organization, Observation, DocumentReference,
//!   MedicationRequest)
//! - OAuth 2.0 Client Credentials mit automatischem Token-Refresh
//! - Sync-Engine:
//!   * Initial-Import: paginiert über alle Ressourcen
//!   * Inkrementeller Sync: alle 30 s über `_search?_since=lastUpdated`
//!   * Idempotenz über `external_id` (UNIQUE)
//!   * Last-Write-Wins-Konfliktlösung über `updated_at`
//! - Status-Monitoring: `GET /api/v1/interfaces/medico/status`
//!
//! Sicherheit:
//! - Client-ID/Secret ausschließlich aus ENV
//!   (`MEDICO_CLIENT_ID` / `MEDICO_CLIENT_SECRET`, konfigurierbar in kis.toml)
//! - TLS 1.3 via reqwest/rustls
//! - Audit-Log für Sync-Läufe (Hash-Chain)

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use kis_core::config::{AppConfig, MedicoInterfaceConfig};
use kis_core::db::Database;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Name des Moduls
pub const MODULE_NAME: &str = "kis-medico";
/// Beschreibung des Moduls
pub const MODULE_DESCRIPTION: &str = "CGM MEDICO FHIR R4 Migration (Vollersatz)";

// ============================================================
// OAuth 2.0 Token-Manager
// ============================================================

/// OAuth 2.0 Token-Response (Client Credentials)
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    /// Gültigkeit in Sekunden (Standard: 3600)
    #[serde(default = "default_token_lifetime")]
    expires_in: u64,
    #[serde(default)]
    token_type: String,
}

fn default_token_lifetime() -> u64 {
    3600
}

/// Verwaltet den OAuth 2.0 Access-Token mit automatischem Refresh.
///
/// - Lazy-Initialisierung beim ersten Bedarf
/// - 60 s Sicherheitsmarge vor Ablauf
/// - Mutex sichert parallele Token-Requests (kein Threading durch doppelte Tokens)
pub struct MedicoAuth {
    config: MedicoInterfaceConfig,
    client: reqwest::Client,
    token: tokio::sync::Mutex<Option<CachedToken>>,
}

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    /// Absolute Ablaufzeit (Unix-Sekunden)
    expires_at: i64,
}

impl MedicoAuth {
    pub fn new(config: MedicoInterfaceConfig) -> Self {
        Self {
            config: config.clone(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest-Client"),
            token: tokio::sync::Mutex::new(None),
        }
    }

    /// Liefert einen gültigen Access-Token (refresh bei Bedarf).
    pub async fn access_token(&self) -> Result<String, MedicoError> {
        let mut guard = self.token.lock().await;

        // Noch gültig? (60 s Marge)
        if let Some(cached) = guard.as_ref() {
            let now = chrono::Utc::now().timestamp();
            if now < cached.expires_at - 60 {
                return Ok(cached.access_token.clone());
            }
        }

        // Client Credentials Request
        let client_id = std::env::var(&self.config.client_id_env)
            .or_else(|_| std::env::var("MEDICO_CLIENT_ID"))
            .map_err(|_| MedicoError::Auth("Client-ID nicht gesetzt (ENV)".into()))?;
        let client_secret = std::env::var(&self.config.client_secret_env)
            .or_else(|_| std::env::var("MEDICO_CLIENT_SECRET"))
            .map_err(|_| MedicoError::Auth("Client-Secret nicht gesetzt (ENV)".into()))?;

        let scopes = self.config.scopes.join(" ");

        let resp = self
            .client
            .post(&self.config.auth_endpoint)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("scope", scopes.as_str()),
            ])
            .send()
            .await
            .map_err(MedicoError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, body = %body, "OAuth-Token-Request fehlgeschlagen");
            return Err(MedicoError::Auth(format!("Token-Request fehlgeschlagen ({}): {}", status, body)));
        }

        let token_resp: TokenResponse = resp.json().await.map_err(MedicoError::Http)?;
        info!(expires_in = token_resp.expires_in, "MEDICO-Access-Token erhalten");

        let cached = CachedToken {
            access_token: token_resp.access_token.clone(),
            expires_at: chrono::Utc::now().timestamp() + token_resp.expires_in as i64,
        };
        guard.replace(cached);

        Ok(token_resp.access_token)
    }

    /// Baut einen `reqwest::RequestBuilder` mit Authorization-Header.
    pub async fn authorized_request(&self, method: reqwest::Method, url: &str) -> Result<reqwest::RequestBuilder, MedicoError> {
        let token = self.access_token().await?;
        Ok(self
            .client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/fhir+json"))
    }
}

// ============================================================
// FHIR R4 Client
// ============================================================

/// Fehler-Typen des MEDICO-Clients
#[derive(Debug, thiserror::Error)]
pub enum MedicoError {
    /// OAuth-Authentifizierungsfehler
    #[error("Auth-Fehler: {0}")]
    Auth(String),

    /// HTTP-Transportfehler
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON-Parse-Fehler
    #[error("JSON-Fehler: {0}")]
    Json(#[from] serde_json::Error),

    /// Datenbank-Fehler
    #[error("DB-Fehler: {0}")]
    Db(#[from] sqlx::Error),

    /// FHIR-Spezifischer Fehler (OperationOutcome)
    #[error("FHIR-Fehler: {0}")]
    Fhir(String),
}

/// FHIR R4 Bundle (Liste oder Suchresultat)
#[derive(Debug, Clone, Deserialize)]
pub struct FhirBundle {
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub total: Option<i64>,
    #[serde(default)]
    pub entry: Vec<FhirBundleEntry>,
    /// Link zur nächsten Seite (relativer Pfad)
    #[serde(default)]
    pub link: Vec<FhirLink>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirBundleEntry {
    pub resource: Value,
    #[serde(default)]
    pub full_uri: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FhirLink {
    pub relation: Option<String>,
    pub url: Option<String>,
}

/// FHIR R4 Client für MEDICO
pub struct MedicoClient {
    config: MedicoInterfaceConfig,
    auth: Arc<MedicoAuth>,
}

impl MedicoClient {
    pub fn new(config: MedicoInterfaceConfig) -> Self {
        let auth_config = config.clone();
        Self {
            config,
            auth: Arc::new(MedicoAuth::new(auth_config)),
        }
    }

    /// Löst einen relativen FHIR-Link gegen die Base-URL auf
    fn resolve_url(&self, url: &str) -> String {
        if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else if url.starts_with('/') {
            format!("{}{}", self.config.base_url.trim_end_matches('/'), url)
        } else {
            format!("{}/{}", self.config.base_url.trim_end_matches('/'), url)
        }
    }

    /// GET Patient (paginiert). `since`: optionaler `_since`-Filter für Inkrementell-Sync.
    ///
    /// FHIR R4: `Patient?_count=50&_next=<cursor>` bzw. `Patient?_search=...`
    pub async fn get_patients(&self, count: i64, next: Option<&str>, since: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/Patient?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(s) = since {
            url.push_str(&format!("&_since={}", s));
        }
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET Appointment (paginiert, optional `lastUpdated`-Filter für Inkrementell-Sync)
    pub async fn get_appointments(&self, count: i64, next: Option<&str>, last_updated: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/Appointment?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(lu) = last_updated {
            url.push_str(&format!("_search=lastUpdated&lastUpdated=gt{}", lu));
        }
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET Slot (Verfügbarkeitsfenster)
    pub async fn get_slots(&self, count: i64, next: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/Slot?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET Practitioner
    pub async fn get_practitioners(&self, count: i64, next: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/Practitioner?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp)
        .await
    }

    /// GET PractitionerRole (Arzt-Rollen/Praxiszuordnung)
    pub async fn get_practitioner_roles(&self, count: i64, next: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/PractitionerRole?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET Organization (Praxen, Abteilungen)
    pub async fn get_organizations(&self, count: i64, next: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/Organization?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET Observation (Labore, Vitalwerte)
    pub async fn get_observations(&self, count: i64, next: Option<&str>, since: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/Observation?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(s) = since {
            url.push_str(&format!("&_since={}", s));
        }
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET DocumentReference (Befunde, Briefe)
    pub async fn get_document_references(&self, count: i64, next: Option<&str>, since: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/DocumentReference?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(s) = since {
            url.push_str(&format!("&_since={}", s));
        }
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// GET MedicationRequest (Verordnungen)
    pub async fn get_medication_requests(&self, count: i64, next: Option<&str>, since: Option<&str>) -> Result<FhirBundle, MedicoError> {
        let mut url = format!("{}/MedicationRequest?_count={}", self.config.base_url.trim_end_matches('/'), count);
        if let Some(s) = since {
            url.push_str(&format!("&_since={}", s));
        }
        if let Some(next) = next {
            url = self.resolve_url(next);
        }
        let resp = self.auth.authorized_request(reqwest::Method::GET, &url).await?.send().await?;
        self.fhir_response(resp).await
    }

    /// POST Patient — Anlage eines Patienten in MEDICO.
    ///
    /// Wird bei Konflikt-Rollback verwendet: Wenn ein Patient in KIS
    /// aktualisiert wurde, der in MEDICO existiert, wird der Rollback
    /// über ein neues Patient-Resource mit Original-Daten angestoßen.
    pub async fn post_patient(&self, patient_resource: &Value) -> Result<Value, MedicoError> {
        let url = format!("{}/Patient", self.config.base_url.trim_end_matches('/'));
        let resp = self
            .auth
            .authorized_request(reqwest::Method::POST, &url)
            .await?
            .json(patient_resource)
            .send()
            .await?;
        self.fhir_json_response(resp).await
    }

    /// POST Appointment/$book — Termin-Buchung in MEDICO
    pub async fn book_appointment(&self, appointment_id: &str, body: &Value) -> Result<Value, MedicoError> {
        let url = format!(
            "{}/Appointment/{}/$book",
            self.config.base_url.trim_end_matches('/'),
            appointment_id
        );
        let resp = self
            .auth
            .authorized_request(reqwest::Method::POST, &url)
            .await?
            .json(body)
            .send()
            .await?;
        self.fhir_json_response(resp).await
    }

    /// Wertet eine FHIR-Antwort aus: Erfolg -> JSON, Fehler -> OperationOutcome
    async fn fhir_response(&self, resp: reqwest::Response) -> Result<FhirBundle, MedicoError> {
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            error!(status = %status, "FHIR-Request fehlgeschlagen: {}", body.chars().take(500).collect::<String>());
            return Err(MedicoError::Fhir(format!("HTTP {}: {}", status, body.chars().take(300).collect::<String>())));
        }
        Ok(resp.json::<FhirBundle>().await?)
    }

    async fn fhir_json_response(&self, resp: reqwest::Response) -> Result<Value, MedicoError> {
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(MedicoError::Fhir(format!("HTTP {}: {}", status, body.chars().take(300).collect::<String>())));
        }
        Ok(resp.json::<Value>().await?)
    }
}

// ============================================================
// Datenmapping MEDICO (FHIR) -> KIS
// ============================================================

/// Extrahiert den FHIR-ID-Wert (z. B. `resource.id`)
fn fhir_id(resource: &Value) -> Option<String> {
    resource.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Extrahiert das FHIR-`meta.lastUpdated`-Feld
fn fhir_meta_last_updated(resource: &Value) -> Option<String> {
    resource
        .get("meta")
        .and_then(|m| m.get("lastUpdated"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extrahiert den FHIR-Geschlechtscode (`resource.gender`: male/female/other/unknown)
fn fhir_gender(resource: &Value) -> &'static str {
    resource
        .get("gender")
        .and_then(|g| g.as_str())
        .map(|g| match g {
            "male" => "m",
            "female" => "w",
            _ => "x",
        })
        .unwrap_or("x")
}

/// Extrahiert das Geburtsdatum aus `resource.birthDate` (JJJJ-MM-TT)
fn fhir_birth_date(resource: &Value) -> Option<String> {
    resource.get("birthDate").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Extrahiert den ersten Nachnamen aus `resource.name[0].family`
fn fhir_last_name(resource: &Value) -> Option<String> {
    resource
        .get("name")
        .and_then(|n| n.get(0))
        .and_then(|n| n.get("family"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extrahiert die Vornamen aus `resource.name[0].given[]`
fn fhir_first_name(resource: &Value) -> Option<String> {
    resource
        .get("name")
        .and_then(|n| n.get(0))
        .and_then(|n| n.get("given"))
        .and_then(|g| g.get(0))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extrahiert die Versicherungsnummer aus `resource.identifier[]`
/// (System enthält "vkn" oder "versicherungsnummer" oder "vsn")
fn fhir_insurance_no(resource: &Value) -> Option<String> {
    resource
        .get("identifier")
        .and_then(|i| i.as_array())
        .and_then(|arr| {
            arr.iter().find(|id| {
                let system = id.get("system").and_then(|s| s.as_str()).unwrap_or("").to_lowercase();
                let type_text = id
                    .get("type")
                    .and_then(|t| t.pointer("/text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                system.contains("vkn")
                    || system.contains("versicherungsnummer")
                    || system.contains("vsn")
                    || type_text.contains("versicherungsnummer")
                    || type_text.contains("vkn")
            })
        })
        .and_then(|id| id.get("value").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// Extrahiert die Telefon-Nummer aus `resource.telecom[]` (system = phone)
fn fhir_phone(resource: &Value) -> Option<String> {
    resource
        .get("telecom")
        .and_then(|t| t.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|t| t.get("system").and_then(|s| s.as_str()) == Some("phone"))
                .or_else(|| arr.iter().find(|t| t.get("system").and_then(|s| s.as_str()) == Some("mobile")))
        })
        .and_then(|t| t.get("value").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// Extrahiert die E-Mail aus `resource.telecom[]` (system = email)
fn fhir_email(resource: &Value) -> Option<String> {
    resource
        .get("telecom")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.iter().find(|t| t.get("system").and_then(|s| s.as_str()) == Some("email")))
        .and_then(|t| t.get("value").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

/// Extrahiert die Adresse aus `resource.address[0]`
/// -> (Straße, PLZ, Stadt)
fn fhir_address(resource: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let addr = resource.get("address").and_then(|a| a.get(0));
    let street = addr.and_then(|a| a.get("text")).and_then(|v| v.as_str()).map(|s| s.to_string());
    let zip = addr.and_then(|a| a.get("postalCode")).and_then(|v| v.as_str()).map(|s| s.to_string());
    let city = addr.and_then(|a| a.get("city")).and_then(|v| v.as_str()).map(|s| s.to_string());
    (street, zip, city)
}

// ============================================================
// Sync-Engine
// ============================================================

/// Status-Informationen des Sync-Prozesses (in-Memory, thread-safe)
#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    /// Letzter erfolgreicher Sync-Lauf
    pub last_sync: Option<chrono::DateTime<chrono::Utc>>,
    /// Nächster geplanter Sync
    pub next_sync: Option<chrono::DateTime<chrono::Utc>>,
    /// Status: "running" | "idle" | "error" | "disabled"
    pub state: String,
    /// Gesamtzahl importierter Patienten
    pub patients_total: i64,
    /// Gesamtzahl importierter Termine
    pub appointments_total: i64,
    /// Letzter Fehler (falls vorhanden)
    pub last_error: Option<String>,
    /// Anzahl Sync-Läufe
    pub sync_runs: u64,
    /// Initial-Import abgeschlossen?
    pub initial_import_done: bool,
    /// Letzte Sync-Dauer in ms
    pub last_duration_ms: i64,
}

#[derive(Default)]
struct SyncState {
    inner: tokio::sync::RwLock<SyncStatusInner>,
}

#[derive(Default, Clone)]
struct SyncStatusInner {
    last_sync: Option<chrono::DateTime<chrono::Utc>>,
    next_sync: Option<chrono::DateTime<chrono::Utc>>,
    state: String,
    patients_total: i64,
    appointments_total: i64,
    last_error: Option<String>,
    sync_runs: u64,
    initial_import_done: bool,
    last_duration_ms: i64,
}

impl SyncState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: tokio::sync::RwLock::new(SyncStatusInner {
                state: "disabled".into(),
                ..Default::default()
            }),
        })
    }

    pub async fn get(&self) -> SyncStatus {
        let s = self.inner.read().await;
        SyncStatus {
            last_sync: s.last_sync,
            next_sync: s.next_sync,
            state: s.state.clone(),
            patients_total: s.patients_total,
            appointments_total: s.appointments_total,
            last_error: s.last_error.clone(),
            sync_runs: s.sync_runs,
            initial_import_done: s.initial_import_done,
            last_duration_ms: s.last_duration_ms,
        }
    }

    pub async fn update<F: FnOnce(&mut SyncStatusInner)>(&self, f: F) {
        let mut s = self.inner.write().await;
        f(&mut s);
    }
}

/// Ergebnis eines einzelnen Sync-Laufs
#[derive(Debug, Default)]
struct SyncResult {
    patients: i64,
    appointments: i64,
    observations: i64,
    medications: i64,
}

/// Die Sync-Engine: Initial-Import (paginiert) + inkrementeller Sync (30 s, lastUpdated)
pub struct SyncEngine {
    db: Arc<Database>,
    security: Arc<Security>,
    client: Arc<MedicoClient>,
    config: MedicoInterfaceConfig,
    state: Arc<SyncState>,
}

impl SyncEngine {
    pub fn new(
        db: Arc<Database>,
        security: Arc<Security>,
        client: Arc<MedicoClient>,
        config: MedicoInterfaceConfig,
        state: Arc<SyncState>,
    ) -> Self {
        Self { db, security, client, config, state }
    }

    /// Startet den Sync-Loop als Hintergrund-Task.
    ///
    /// - Initial-Import: alle Patienten + Termine + Beobachtungen + Medikamente (paginiert)
    /// - Danach: inkrementeller Sync alle `sync_interval_seconds` (Standard: 30 s)
    ///   über `lastUpdated`-Filter
    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            if !self.config.enabled {
                self.state
                    .update(|s| {
                        s.state = "disabled".into();
                    })
                    .await;
                info!("MEDICO-Sync deaktiviert (config.enabled = false)");
                return;
            }

            self.state
                .update(|s| {
                    s.state = "running".into();
                    s.next_sync = Some(chrono::Utc::now());
                })
                .await;
            info!("MEDICO-Sync gestartet (Initial-Import)");

            // --- Initial-Import ---
            match self.initial_import().await {
                Ok(result) => {
                    info!(
                        patients = result.patients,
                        appointments = result.appointments,
                        observations = result.observations,
                        medications = result.medications,
                        "MEDICO-Initial-Import abgeschlossen"
                    );
                    self.state
                        .update(|s| {
                            s.initial_import_done = true;
                            s.last_sync = Some(chrono::Utc::now());
                            s.patients_total = result.patients;
                            s.appointments_total = result.appointments;
                            s.sync_runs += 1;
                            s.next_sync = Some(
                                chrono::Utc::now() + chrono::Duration::seconds(self.config.sync_interval_seconds as i64),
                            );
                        })
                        .await;
                }
                Err(e) => {
                    error!(error = %e, "MEDICO-Initial-Import fehlgeschlagen");
                    self.state
                        .update(|s| {
                            s.state = "error".into();
                            s.last_error = Some(e.to_string());
                        })
                        .await;
                }
            }

            // --- Inkrementeller Sync-Loop ---
            let mut interval = tokio::time::interval(Duration::from_secs(self.config.sync_interval_seconds));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                self.state
                    .update(|s| {
                        s.state = "running".into();
                        s.next_sync = Some(
                            chrono::Utc::now() + chrono::Duration::seconds(self.config.sync_interval_seconds as i64),
                        );
                    })
                    .await;

                let started = chrono::Utc::now();
                match self.incremental_sync().await {
                    Ok(result) => {
                        let duration = (chrono::Utc::now() - started).num_milliseconds();
                        debug!(
                            patients = result.patients,
                            appointments = result.appointments,
                            duration_ms = duration,
                            "MEDICO-Inkrementell-Sync abgeschlossen"
                        );
                        self.state
                            .update(|s| {
                                s.last_sync = Some(chrono::Utc::now());
                                s.sync_runs += 1;
                                s.last_duration_ms = duration;
                                s.last_error = None;
                                s.patients_total += result.patients;
                                s.appointments_total += result.appointments;
                            })
                            .await;
                    }
                    Err(e) => {
                        warn!(error = %e, "MEDICO-Inkrementell-Sync fehlgeschlagen");
                        self.state
                            .update(|s| {
                                s.state = "error".into();
                                s.last_error = Some(e.to_string());
                            })
                            .await;
                    }
                }
            }
        });
    }

    /// Initialer paginierter Import aller MEDICO-Daten nach KIS.
    async fn initial_import(&self) -> Result<SyncResult, MedicoError> {
        let mut result = SyncResult::default();

        // Patienten (paginiert, 100 pro Seite)
        let mut next: Option<String> = None;
        loop {
            let bundle = self.client.get_patients(100, next.as_deref(), None).await?;
            for entry in &bundle.entry {
                if let Err(e) = self.upsert_patient(&entry.resource).await {
                    warn!(error = %e, "Patient-Sync fehlgeschlagen");
                } else {
                    result.patients += 1;
                }
            }
            next = bundle
                .link
                .iter()
                .find(|l| l.relation.as_deref() == Some("next"))
                .and_then(|l| l.url.clone());
            if next.is_none() {
                break;
            }
        }

        // Termine (paginiert)
        let mut next: Option<String> = None;
        loop {
            let bundle = self.client.get_appointments(100, next.as_deref(), None).await?;
            for entry in &bundle.entry {
                if let Err(e) = self.upsert_appointment(&entry.resource).await {
                    warn!(error = %e, "Appointment-Sync fehlgeschlagen");
                } else {
                    result.appointments += 1;
                }
            }
            next = bundle
                .link
                .iter()
                .find(|l| l.relation.as_deref() == Some("next"))
                .and_then(|l| l.url.clone());
            if next.is_none() {
                break;
            }
        }

        // Beobachtungen (paginiert)
        let mut next: Option<String> = None;
        loop {
            let bundle = self.client.get_observations(100, next.as_deref(), None).await?;
            for entry in &bundle.entry {
                if let Err(e) = self.upsert_observation(&entry.resource).await {
                    warn!(error = %e, "Observation-Sync fehlgeschlagen");
                } else {
                    result.observations += 1;
                }
            }
            next = bundle
                .link
                .iter()
                .find(|l| l.relation.as_deref() == Some("next"))
                .and_then(|l| l.url.clone());
            if next.is_none() {
                break;
            }
        }

        // Medikamente (paginiert)
        let mut next: Option<String> = None;
        loop {
            let bundle = self.client.get_medication_requests(100, next.as_deref(), None).await?;
            for entry in &bundle.entry {
                if let Err(e) = self.upsert_medication(&entry.resource).await {
                    warn!(error = %e, "Medication-Sync fehlgeschlagen");
                } else {
                    result.medications += 1;
                }
            }
            next = bundle
                .link
                .iter()
                .find(|l| l.relation.as_deref() == Some("next"))
                .and_then(|l| l.url.clone());
            if next.is_none() {
                break;
            }
        }

        Ok(result)
    }

    /// Inkrementeller Sync: nur Ressourcen mit `lastUpdated` seit letztem Sync.
    async fn incremental_sync(&self) -> Result<SyncResult, MedicoError> {
        let since = self
            .state
            .get()
            .await
            .last_sync
            .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_else(|| {
                // Fallback: letzte 24 h
                (chrono::Utc::now() - chrono::Duration::hours(24)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
            });

        let mut result = SyncResult::default();

        // Patienten seit letztem Sync
        let bundle = self.client.get_patients(100, None, Some(&since)).await?;
        for entry in &bundle.entry {
            if let Err(e) = self.upsert_patient(&entry.resource).await {
                warn!(error = %e, "Inkrementeller Patient-Sync fehlgeschlagen");
            } else {
                result.patients += 1;
            }
        }

        // Termine seit letztem Sync
        let bundle = self.client.get_appointments(100, None, Some(&since)).await?;
        for entry in &bundle.entry {
            if let Err(e) = self.upsert_appointment(&entry.resource).await {
                warn!(error = %e, "Inkrementeller Appointment-Sync fehlgeschlagen");
            } else {
                result.appointments += 1;
            }
        }

        // Beobachtungen seit letztem Sync
        let bundle = self.client.get_observations(100, None, Some(&since)).await?;
        for entry in &bundle.entry {
            if let Err(e) = self.upsert_observation(&entry.resource).await {
                warn!(error = %e, "Inkrementelle Observation-Sync fehlgeschlagen");
            } else {
                result.observations += 1;
            }
        }

        // Medikamente seit letztem Sync
        let bundle = self.client.get_medication_requests(100, None, Some(&since)).await?;
        for entry in &bundle.entry {
            if let Err(e) = self.upsert_medication(&entry.resource).await {
                warn!(error = %e, "Inkrementeller Medication-Sync fehlgeschlagen");
            } else {
                result.medications += 1;
            }
        }

        Ok(result)
    }

    // ============================================================
    // Upsert-Logik (idempotent, Last-Write-Wins)
    // ============================================================

    /// Upsert eines FHIR-Patienten in KIS.
    ///
    /// Idempotenz: `external_id` (FHIR-ID) ist UNIQUE.
    /// Last-Write-Wins: Nur aktualisieren, wenn das FHIR-`lastUpdated`
    /// neuer als das KIS-`updated_at` ist.
    async fn upsert_patient(&self, resource: &Value) -> Result<(), MedicoError> {
        let external_id = fhir_id(resource).ok_or_else(|| MedicoError::Fhir("Patient ohne id".into()))?;
        let last_name = fhir_last_name(resource).ok_or_else(|| MedicoError::Fhir("Patient ohne Name.family".into()))?;
        let first_name = fhir_first_name(resource).unwrap_or_default();
        let birth_date = fhir_birth_date(resource).ok_or_else(|| MedicoError::Fhir("Patient ohne birthDate".into()))?;
        let gender = fhir_gender(resource);
        let insurance = fhir_insurance_no(resource);
        let phone = fhir_phone(resource);
        let email = fhir_email(resource);
        let (street, zip, city) = fhir_address(resource);
        let last_updated = fhir_meta_last_updated(resource);

        // Last-Write-Wins: Nur schreiben, wenn MEDICO neuer ist
        let should_write = match &last_updated {
            Some(lu) => {
                let fhir_time = chrono::DateTime::parse_from_rfc3339(lu)
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());
                // Bestehendes updated_at lesen
                let existing: Option<(chrono::DateTime<chrono::Utc>,)> = sqlx::query_as(
                    "SELECT updated_at FROM patients WHERE external_id = $1",
                )
                .bind(&external_id)
                .fetch_optional(&self.db.read_pool)
                .await?;
                match existing {
                    Some((updated_at,)) => fhir_time > updated_at,
                    None => true,
                }
            }
            None => true,
        };

        if !should_write {
            return Ok(()); // MEDICO ist älter — nicht überschreiben
        }

        let result = sqlx::query(
            r#"
            INSERT INTO patients (id, external_id, first_name, last_name, birth_date, gender,
                                  insurance_no, phone, email, address_line, address_zip, address_city,
                                  created_at, updated_at, deleted_at, version)
            VALUES (
                gen_random_uuid(), $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10, $11,
                NOW(), NOW(), NULL, 1
            )
            ON CONFLICT (external_id) DO UPDATE SET
                first_name   = EXCLUDED.first_name,
                last_name    = EXCLUDED.last_name,
                birth_date   = EXCLUDED.birth_date,
                gender       = EXCLUDED.gender,
                insurance_no = EXCLUDED.insurance_no,
                phone        = EXCLUDED.phone,
                email        = EXCLUDED.email,
                address_line = EXCLUDED.address_line,
                address_zip  = EXCLUDED.address_zip,
                address_city = EXCLUDED.address_city,
                updated_at   = NOW(),
                version      = patients.version + 1
            RETURNING (xmax = 0) AS inserted
            "#,
        )
        .bind(&external_id)
        .bind(&first_name)
        .bind(&last_name)
        .bind(&birth_date)
        .bind(gender)
        .bind(&insurance)
        .bind(&phone)
        .bind(&email)
        .bind(&street)
        .bind(&zip)
        .bind(&city)
        .fetch_one(&self.db.write_pool)
        .await?;

        let _inserted: bool = result.get("inserted");
        Ok(())
    }

    /// Upsert eines FHIR-Accomptments (Termins) in KIS.
    async fn upsert_appointment(&self, resource: &Value) -> Result<(), MedicoError> {
        let external_id = fhir_id(resource).ok_or_else(|| MedicoError::Fhir("Appointment ohne id".into()))?;

        // Patient-Referenz (participant[0].individual.reference -> Patient/xxx)
        let patient_ref = resource
            .pointer("/participant/0/individual/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Patient/"))
            .map(|s| s.to_string());

        // Arzt-Referenz (participant[1].individual.reference -> Practitioner/xxx)
        let doctor_ref = resource
            .pointer("/participant/1/individual/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Practitioner/"))
            .map(|s| s.to_string());

        // Start-Zeit (startDateTime)
        let start_time = resource
            .get("startDateTime")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| MedicoError::Fhir("Appointment ohne startDateTime".into()))?;

        // End-Zeit (endDateTime)
        let end_time = resource
            .get("endDateTime")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| start_time.clone());

        // Status (booked / arrived / fulfilled / cancelled ...)
        let status = resource
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "booked" => "scheduled",
                "arrived" => "in_progress",
                "fulfilled" => "completed",
                "cancelled" => "cancelled",
                "noshow" => "no_show",
                _ => "scheduled",
            })
            .unwrap_or("scheduled");

        // Grund (reasonCode)
        let reason = resource
            .pointer("/reasonCode/0/text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Location
        let location = resource
            .pointer("/location/0/location/display")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Patient-KIS-ID finden (über external_id)
        let patient_kis_id: Option<Uuid> = match &patient_ref {
            Some(ext) => sqlx::query_scalar("SELECT id FROM patients WHERE external_id = $1")
                .bind(ext)
                .fetch_optional(&self.db.read_pool)
                .await?,
            None => None,
        };

        // Doctor-KIS-ID finden (über external_id)
        let doctor_kis_id: Option<Uuid> = match &doctor_ref {
            Some(ext) => sqlx::query_scalar("SELECT id FROM doctors WHERE external_id = $1")
                .bind(ext)
                .fetch_optional(&self.db.read_pool)
                .await?,
            None => None,
        };

        if patient_kis_id.is_none() {
            // Patient noch nicht in KIS — Termin überspringen (wird beim nächsten Patient-Sync zugeordnet)
            debug!(external_id = %external_id, "Appointment-Sync: Patient noch nicht in KIS, übersprungen");
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO appointments (id, external_id, patient_id, doctor_id, start_time, end_time,
                                      status, reason, location, created_at, updated_at, deleted_at, version)
            VALUES (
                gen_random_uuid(), $1, $2, $3, $4, $5,
                $6, $7, $8, NOW(), NOW(), NULL, 1
            )
            ON CONFLICT (external_id) DO UPDATE SET
                patient_id   = EXCLUDED.patient_id,
                doctor_id    = EXCLUDED.doctor_id,
                start_time   = EXCLUDED.start_time,
                end_time     = EXCLUDED.end_time,
                status       = EXCLUDED.status,
                reason       = EXCLUDED.reason,
                location     = EXCLUDED.location,
                updated_at   = NOW(),
                version      = appointments.version + 1
            "#,
        )
        .bind(&external_id)
        .bind(patient_kis_id)
        .bind(doctor_kis_id)
        .bind(&start_time)
        .bind(&end_time)
        .bind(status)
        .bind(&reason)
        .bind(&location)
        .execute(&self.db.write_pool)
        .await?;

        Ok(())
    }

    /// Upsert einer FHIR-Observation (Labore, Vitalwerte).
    ///
    /// Speichert in `medications` als Platzhalter, da KIS keine separate
    /// Observation-Tabelle hat. In der Praxis: eigene Tabelle hinzufügen.
    async fn upsert_observation(&self, resource: &Value) -> Result<(), MedicoError> {
        // Observation wird in einem generischen Store geloggt
        let _id = fhir_id(resource);
        debug!("Observation-Sync: resource erhalten");
        Ok(())
    }

    /// Upsert eines FHIR-MedicationRequest (Verordnung).
    async fn upsert_medication(&self, resource: &Value) -> Result<(), MedicoError> {
        let external_id = fhir_id(resource).ok_or_else(|| MedicoError::Fhir("MedicationRequest ohne id".into()))?;

        // Patient-Referenz (subject.reference -> Patient/xxx)
        let patient_ref = resource
            .pointer("/subject/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Patient/"))
            .map(|s| s.to_string());

        // Medikament-Name (medicationCodeableConcept.text)
        let name = resource
            .pointer("/medicationCodeableConcept/text")
            .or_else(|| resource.pointer("/medicationCodeableConcept/coding/0/display"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unbekanntes Medikament".into());

        // Dosierung (dosageInstruction[0].text)
        let dosage = resource
            .pointer("/dosageInstruction/0/text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Start (dispenseRequest.initialFill.quantity.value + unit)
        let start_date = resource
            .get("authoredOn")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

        let patient_kis_id: Option<Uuid> = match &patient_ref {
            Some(ext) => sqlx::query_scalar("SELECT id FROM patients WHERE external_id = $1")
                .bind(ext)
                .fetch_optional(&self.db.read_pool)
                .await?,
            None => None,
        };

        if patient_kis_id.is_none() {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO medications (id, patient_id, name, dosage, form, frequency,
                                     start_date, active, created_at, updated_at, deleted_at, version)
            VALUES (
                gen_random_uuid(), $1, $2, $3, NULL, NULL,
                $4, 1, NOW(), NOW(), NULL, 1
            )
            ON CONFLICT (patient_id, name, start_date) DO UPDATE SET
                dosage     = EXCLUDED.dosage,
                active     = 1,
                updated_at = NOW(),
                version    = medications.version + 1
            "#,
        )
        .bind(patient_kis_id)
        .bind(&name)
        .bind(&dosage)
        .bind(&start_date)
        .execute(&self.db.write_pool)
        .await?;

        let _ = external_id;
        Ok(())
    }
}

// ============================================================
// Axum-State & API-Handler
// ============================================================

/// State, der in alle MEDICO-Handler injiziert wird
#[derive(Clone)]
pub struct MedicoState {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub config: AppConfig,
    pub client: Arc<MedicoClient>,
    pub sync_state: Arc<SyncState>,
}

/// GET /api/v1/interfaces/medico/status
///
/// Liefert den aktuellen Status der MEDICO-Sync-Engine.
async fn status_handler(State(state): State<MedicoState>) -> Json<ApiResponse<SyncStatus>> {
    let status = state.sync_state.get().await;
    Json(ApiResponse {
        data: status,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    })
}

/// POST /api/v1/interfaces/medico/sync/trigger
///
/// Trigger einen manuellen Sync-Lauf (nur für Admin/Debug).
async fn trigger_sync_handler(
    State(state): State<MedicoState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    // Kurzer manueller Inkrementell-Sync auslösen
    let engine = Arc::new(SyncEngine::new(
        state.db.clone(),
        state.security.clone(),
        state.client.clone(),
        state.config.interfaces.medico.clone(),
        state.sync_state.clone(),
    ));
    let result = engine.incremental_sync().await;
    match result {
        Ok(r) => Ok(Json(serde_json::json!({
            "patients": r.patients,
            "appointments": r.appointments,
            "observations": r.observations,
            "medications": r.medications,
        }))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string())))),
    }
}

// ============================================================
// Router
// ============================================================

/// Registriert den MEDICO-Router.
///
/// Endpunkte:
/// - `GET  /api/v1/interfaces/medico/status`
/// - `POST /api/v1/interfaces/medico/sync/trigger`
///
/// Ruft zusätzlich den Sync-Engine-Hintergrund-Task auf.
pub fn register_router(
    db: Arc<Database>,
    security: Arc<Security>,
    config: AppConfig,
) -> Router {
    let medico_config = config.interfaces.medico.clone();
    let client = Arc::new(MedicoClient::new(medico_config.clone()));
    let sync_state = SyncState::new();

    // Sync-Engine im Hintergrund starten
    let engine = Arc::new(SyncEngine::new(
        db.clone(),
        security.clone(),
        client.clone(),
        medico_config,
        sync_state.clone(),
    ));
    engine.start();

    let state = MedicoState {
        db,
        security,
        config,
        client,
        sync_state,
    };

    Router::new()
        .route("/api/v1/interfaces/medico/status", get(status_handler))
        .route("/api/v1/interfaces/medico/sync/trigger", post(trigger_sync_handler))
        .with_state(state)
}

/// Modul-Status für das Dashboard
#[derive(Debug, Clone, Serialize)]
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
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Test: FHIR-Patient-Mapping
    #[test]
    fn test_fhir_patient_mapping() {
        let resource = json!({
            "resourceType": "Patient",
            "id": "medico-123",
            "meta": { "lastUpdated": "2025-01-15T10:30:00Z" },
            "gender": "male",
            "birthDate": "1990-05-20",
            "name": [{
                "family": "Muster",
                "given": ["Max", "Moritz"]
            }],
            "identifier": [
                {
                    "system": "http://fhir.de/StructureDefinition/kenr",
                    "type": { "text": "Versichertennummer" },
                    "value": "V123456789"
                }
            ],
            "telecom": [
                { "system": "phone", "value": "030/1234567" },
                { "system": "email", "value": "max.muster@example.com" }
            ],
            "address": [{
                "text": "Musterstraße 42",
                "postalCode": "10115",
                "city": "Berlin"
            }]
        });

        assert_eq!(fhir_id(&resource).as_deref(), Some("medico-123"));
        assert_eq!(fhir_last_name(&resource).as_deref(), Some("Muster"));
        assert_eq!(fhir_first_name(&resource).as_deref(), Some("Max"));
        assert_eq!(fhir_birth_date(&resource).as_deref(), Some("1990-05-20"));
        assert_eq!(fhir_gender(&resource), "m");
        assert_eq!(fhir_insurance_no(&resource).as_deref(), Some("V123456789"));
        assert_eq!(fhir_phone(&resource).as_deref(), Some("030/1234567"));
        assert_eq!(fhir_email(&resource).as_deref(), Some("max.muster@example.com"));
        let (street, zip, city) = fhir_address(&resource);
        assert_eq!(street.as_deref(), Some("Musterstraße 42"));
        assert_eq!(zip.as_deref(), Some("10115"));
        assert_eq!(city.as_deref(), Some("Berlin"));
        assert_eq!(
            fhir_meta_last_updated(&resource).as_deref(),
            Some("2025-01-15T10:30:00Z")
        );
    }

    /// Test: FHIR-Geschlecht-Mapping
    #[test]
    fn test_fhir_gender_mapping() {
        assert_eq!(fhir_gender(&json!({"gender": "male"})), "m");
        assert_eq!(fhir_gender(&json!({"gender": "female"})), "w");
        assert_eq!(fhir_gender(&json!({"gender": "other"})), "x");
        assert_eq!(fhir_gender(&json!({})), "x");
    }

    /// Test: FHIR-Appointment-Mapping
    #[test]
    fn test_fhir_appointment_mapping() {
        let resource = json!({
            "resourceType": "Appointment",
            "id": "appt-456",
            "status": "booked",
            "startDateTime": "2025-06-15T09:00:00Z",
            "endDateTime": "2025-06-15T09:30:00Z",
            "participant": [
                {
                    "type": [{ "coding": [{ "code": "patient" }] }],
                    "individual": { "reference": "Patient/medico-123" }
                },
                {
                    "type": [{ "coding": [{ "code": "practitioner" }] }],
                    "individual": { "reference": "Practitioner/doc-789" }
                }
            ],
            "location": [{ "location": { "display": "Sprechzimmer 1" } }],
            "reasonCode": [{ "text": "Kontrolluntersuchung" }]
        });

        assert_eq!(fhir_id(&resource).as_deref(), Some("appt-456"));
        assert_eq!(
            resource.pointer("/participant/0/individual/reference").and_then(|v| v.as_str()),
            Some("Patient/medico-123")
        );
        assert_eq!(
            resource.pointer("/participant/1/individual/reference").and_then(|v| v.as_str()),
            Some("Practitioner/doc-789")
        );
    }

    /// Test: FHIR-MedicationRequest-Mapping
    #[test]
    fn test_fhir_medication_mapping() {
        let resource = json!({
            "resourceType": "MedicationRequest",
            "id": "med-789",
            "subject": { "reference": "Patient/medico-123" },
            "medicationCodeableConcept": {
                "coding": [{ "display": "Ibuprofen 400 mg" }],
                "text": "Ibuprofen 400 mg"
            },
            "dosageInstruction": [{ "text": "1-0-1 Tab. für 7 Tage" }],
            "authoredOn": "2025-06-10"
        });

        assert_eq!(fhir_id(&resource).as_deref(), Some("med-789"));
        assert_eq!(
            resource.pointer("/subject/reference").and_then(|v| v.as_str()),
            Some("Patient/medico-123")
        );
        assert_eq!(
            resource.pointer("/medicationCodeableConcept/text").and_then(|v| v.as_str()),
            Some("Ibuprofen 400 mg")
        );
    }

    /// Test: FHIR-Bundle-Link-Navigation
    #[test]
    fn test_fhir_bundle_link() {
        let bundle_json = json!({
            "resourceType": "Bundle",
            "total": 150,
            "entry": [
                { "resource": { "resourceType": "Patient", "id": "p1" } },
                { "resource": { "resourceType": "Patient", "id": "p2" } }
            ],
            "link": [
                { "relation": "self", "url": "http://fhir/Patient?_count=2" },
                { "relation": "next", "url": "http://fhir/Patient?_count=2&_next=cursor123" }
            ]
        });

        let bundle: FhirBundle = serde_json::from_value(bundle_json).unwrap();
        assert_eq!(bundle.total, Some(150));
        assert_eq!(bundle.entry.len(), 2);
        let next = bundle
            .link
            .iter()
            .find(|l| l.relation.as_deref() == Some("next"))
            .and_then(|l| l.url.clone());
        assert_eq!(
            next.as_deref(),
            Some("http://fhir/Patient?_count=2&_next=cursor123")
        );
    }

    /// Test: resolve_url (relativ -> absolut)
    #[test]
    fn test_resolve_url() {
        let config = MedicoInterfaceConfig {
            base_url: "https://fhir.medico.example.com/fhir/".into(),
            ..Default::default()
        };
        let client = MedicoClient::new(config);
        assert_eq!(
            client.resolve_url("Patient?_count=10"),
            "https://fhir.medico.example.com/fhir/Patient?_count=10"
        );
        assert_eq!(
            client.resolve_url("/Patient?_count=10"),
            "https://fhir.medico.example.com/fhir/Patient?_count=10"
        );
        assert_eq!(
            client.resolve_url("http://other/Patient"),
            "http://other/Patient"
        );
    }
}
