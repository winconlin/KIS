//! FHIR R4 Client für CGM MEDICO (Task 4.1).
//!
//! OAuth 2.0 Client-Credentials-Flow mit:
//! - automatischem Token-Refresh bei 401 (einmaliger Retry)
//! - Scope-Handling: `ISIK_READ`-Scope für alle Lese-Operationen
//! - strukturiertem Fehlerhandling: [`MedicoError`] mit
//!   `Unauthorized`, `RateLimited`, `NotFound`, `Other`
//!
//! Alle Methoden sind idempotent und verwenden `ISIK_READ`, solange
//! keine Schreib-Operationen erforderlich sind.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::MedicoInterfaceConfig;

/// Scope für Lese-Operationen (ISiK-Konvention).
pub const SCOPE_ISIK_READ: &str = "ISIK_READ";

/// FHIR-Bundle: `{ "resourceType": "Bundle", "entry": [...] }`
#[derive(Debug, Deserialize)]
pub struct FhirBundle {
    #[serde(default)]
    pub entry: Vec<FhirBundleEntry>,
}

/// Einzelner Eintrag in einem FHIR-Bundle
#[derive(Debug, Deserialize)]
pub struct FhirBundleEntry {
    pub resource: serde_json::Value,
}

impl FhirBundle {
    /// Liefert die `resource`-Felder aller Einträge.
    pub fn resources(&self) -> impl Iterator<Item = &serde_json::Value> {
        self.entry.iter().map(|e| &e.resource)
    }
}

/// Fehler-Typen für MEDICO-Operationen
#[derive(Debug, thiserror::Error)]
pub enum MedicoError {
    /// 401 Unauthorized — Token abgelaufen oder ungültig
    #[error("Medico Unauthorized (401): {0}")]
    Unauthorized(String),

    /// 429 Too Many Requests — Rate-Limit überschritten
    #[error("Medico RateLimited (429): {0}")]
    RateLimited(String),

    /// 404 Not Found — Ressource existiert nicht
    #[error("Medico NotFound (404): {0}")]
    NotFound(String),

    /// 403 Forbidden — fehlende Berechtigung
    #[error("Medico Forbidden (403): {0}")]
    Forbidden(String),

    /// Alle anderen Fehler (Netzwerk, Parsing, Serverfehler)
    #[error("Medico Error: {0}")]
    Other(String),
}

impl MedicoError {
    /// Wendet die HTTP-Status-Logik auf einen reqwest-Fehler an.
    pub fn from_response(status: u16, body: &str) -> Self {
        match status {
            401 => Self::Unauthorized(body.to_string()),
            429 => Self::RateLimited(body.to_string()),
            404 => Self::NotFound(body.to_string()),
            403 => Self::Forbidden(body.to_string()),
            _ => Self::Other(format!("HTTP {status}: {body}")),
        }
    }
}

/// OAuth 2.0 Token-Response
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// Interne Token-Darstellung (mit Ablaufzeitpunkt)
#[derive(Debug, Clone)]
struct Token {
    access_token: String,
    expires_at: DateTime<Utc>,
}

/// FHIR R4 Client für CGM MEDICO
///
/// Handhabt OAuth 2.0 Client-Credentials automatisch:
/// - `acquire_token()`: Initialer Token-Request
/// - Bei 401 auf FHIR-Endpunkt: Token wird erneut angefordert und der
///   Request genau einmal wiederholt (kein Endlosschleife).
///
/// `ISIK_READ`-Scope wird für alle Lese-Operationen angefragt.
/// Schreib-Operationen (Slot-Reservierung) verwenden `ISIK_WRITE`.
pub struct FhirClient {
    http: reqwest::Client,
    config: MedicoInterfaceConfig,
    token: RwLock<Option<Token>>,
    client_id: String,
    client_secret: String,
}

impl FhirClient {
    /// Erzeugt einen neuen Client. Credentials werden aus ENV gelesen
    /// (NIE im Code — siehe `client_id_env` / `client_secret_env` in Config).
    pub fn new(config: MedicoInterfaceConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.request_timeout_seconds.max(1)))
            .build()
            .expect("reqwest Client konnte nicht erstellt werden");
        let client_id = std::env::var(&config.client_id_env).unwrap_or_default();
        let client_secret = std::env::var(&config.client_secret_env).unwrap_or_default();
        Self {
            http,
            config,
            token: RwLock::new(None),
            client_id,
            client_secret,
        }
    }

    /// Für Unit-Tests: Client ohne ENV-Lookup.
    #[cfg(test)]
    pub fn new_for_test(config: MedicoInterfaceConfig, client_id: &str, client_secret: &str) -> Self {
        let http = reqwest::Client::new();
        Self {
            http,
            config,
            token: RwLock::new(None),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        }
    }

    // -------------------------------------------------------------------------
    // Token-Verwaltung
    // -------------------------------------------------------------------------

    /// Liefert den gültigen Access-Token; refresh bei Ablauf oder 401.
    ///
    /// `force_refresh`: `true`, wenn der letzte Request mit 401 geantwortet hat.
    async fn access_token(&self, force_refresh: bool) -> Result<String, MedicoError> {
        // 1) Bestehenden Token prüfen
        {
            let guard = self.token.read().await;
            if !force_refresh {
                if let Some(t) = guard.as_ref() {
                    if t.expires_at > Utc::now() + Duration::seconds(30) {
                        return Ok(t.access_token.clone());
                    }
                    warn!("MEDICO-Token läuft ab — erneute Anforderung");
                }
            }
        }

        // 2) Token neu anfordern
        let token = self.acquire_token(SCOPE_ISIK_READ).await?;
        let mut guard = self.token.write().await;
        *guard = Some(token.clone());
        Ok(token.access_token)
    }

    /// Führt einen OAuth 2.0 Client-Credentials-Request durch.
    pub async fn acquire_token(&self, scope: &str) -> Result<Token, MedicoError> {
        let resp = self
            .http
            .post(&self.config.auth_endpoint)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("scope", scope),
            ])
            .send()
            .await
            .map_err(|e| MedicoError::Other(format!("Token-Request fehlgeschlagen: {e}")))?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| MedicoError::Other(format!("Token-Body-Read fehlgeschlagen: {e}")))?;

        if status != 200 {
            return Err(MedicoError::from_response(status, &body));
        }

        let tr: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| MedicoError::Other(format!("Token-JSON-Parse fehlgeschlagen: {e}: {body}")))?;

        info!(
            scope = %scope,
            expires_in = tr.expires_in,
            "MEDICO-OAuth-Token erhalten"
        );

        Ok(Token {
            access_token: tr.access_token,
            expires_at: Utc::now() + Duration::seconds(tr.expires_in.max(1)),
        })
    }

    /// Baut einen authentifizierten GET-Request auf den MEDICO-FHIR-Server.
    ///
    /// Bei 401 wird der Token einmalig erneuert und der Request wiederholt.
    async fn authenticated_get(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<serde_json::Value, MedicoError> {
        // Erster Versuch mit dem aktuellen Token
        for attempt in 0..2u8 {
            let token = self.access_token(attempt == 1).await?;

            let url = self.base_url(path);
            let resp = self
                .http
                .get(&url)
                .query(query)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/fhir+json")
                .send()
                .await
                .map_err(|e| MedicoError::Other(format!("HTTP-Request fehlgeschlagen: {e}")))?;

            let status = resp.status().as_u16();
            let body = resp
                .text()
                .await
                .map_err(|e| MedicoError::Other(format!("Response-Read fehlgeschlagen: {e}")))?;

            match status {
                200 | 201 => {
                    return serde_json::from_str(&body)
                        .map_err(|e| MedicoError::Other(format!("FHIR-JSON-Parse fehlgeschlagen: {e}")));
                }
                401 if attempt == 0 => {
                    warn!("401 bei {path} — Token-Refresh und Retry");
                    continue; // einmaliger Retry
                }
                401 => {
                    return Err(MedicoError::Unauthorized(format!(
                        "401 nach Token-Refresh bei {path}: {body}"
                    )));
                }
                429 => {
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("60");
                    warn!(
                        path = %path,
                        retry_after = %retry_after,
                        "MEDICO-Rate-Limit erreicht"
                    );
                    return Err(MedicoError::RateLimited(format!(
                        "Rate-Limit bei {path} (retry-after: {retry_after} s)"
                    )));
                }
                404 => return Err(MedicoError::NotFound(format!("404 bei {path}: {body}"))),
                403 => return Err(MedicoError::Forbidden(format!("403 bei {path}: {body}"))),
                _ => {
                    return Err(MedicoError::Other(format!("HTTP {status} bei {path}: {body}")));
                }
            }
        }
        // Theoretisch unerreichbar (Loop endet nach attempt 1)
        Err(MedicoError::Other("Token-Refresh-Schleife verlassen".into()))
    }

    /// Baut eine URL aus Base-URL und Pfad.
    fn base_url(&self, path: &str) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{base}/{path}")
    }

    // -------------------------------------------------------------------------
    // FHIR-Lese-Operationen (ISIK_READ)
    // -------------------------------------------------------------------------

    /// `GET /Patient` — alle Patienten aus MEDICO.
    pub async fn get_patients(&self) -> Result<FhirBundle, MedicoError> {
        let v = self.authenticated_get("Patient", &[]).await?;
        Ok(serde_json::from_value(v)
            .map_err(|e| MedicoError::Other(format!("Bundle-Parse (Patient): {e}")))?
    )

    /// `GET /Appointment` — alle Termine aus MEDICO.
    ///
    /// `since`: Optionaler `_lastUpdated`-Filter für inkrementellen Sync.
    pub async fn get_appointments(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<FhirBundle, MedicoError> {
        let query: Vec<(&str, String)> = match since {
            Some(ts) => vec![("_lastUpdated", format!("ge{ts}"))],
            None => vec![],
        };
        let v = self.authenticated_get("Appointment", &query).await?;
        Ok(serde_json::from_value(v)
            .map_err(|e| MedicoError::Other(format!("Bundle-Parse (Appointment): {e}")))?
    }

    /// `GET /Slot?start={date}` — freie Slots ab einem Datum.
    pub async fn get_slots(
        &self,
        start: chrono::NaiveDate,
        practitioner_id: Option<&str>,
    ) -> Result<FhirBundle, MedicoError> {
        let mut query: Vec<(&str, String)> = vec![("start", format!("{}", start))];
        if let Some(pid) = practitioner_id {
            query.push(("practitioner", pid.to_string()));
        }
        let v = self.authenticated_get("Slot", &query).await?;
        Ok(serde_json::from_value(v)
            .map_err(|e| MedicoError::Other(format!("Bundle-Parse (Slot): {e}")))?
    }

    /// `GET /Practitioner` — alle Practitioners aus MEDICO.
    pub async fn get_practitioners(&self) -> Result<FhirBundle, MedicoError> {
        let v = self.authenticated_get("Practitioner", &[]).await?;
        Ok(serde_json::from_value(v)
            .map_err(|e| MedicoError::Other(format!("Bundle-Parse (Practitioner): {e}")))?
    }

    /// `GET /PractitionerRole` — Rollen und Sprechzeiten der Practitioners.
    pub async fn get_practitioner_roles(&self) -> Result<FhirBundle, MedicoError> {
        let v = self.authenticated_get("PractitionerRole", &[]).await?;
        Ok(serde_json::from_value(v)
            .map_err(|e| MedicoError::Other(format!("Bundle-Parse (PractitionerRole): {e}")))?
    }

    /// `GET /Observation` — alle Observations (Befunde/Laborwerte).
    ///
    /// `since`: Optionaler `_lastUpdated`-Filter für inkrementellen Sync.
    pub async fn get_observations(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<FhirBundle, MedicoError> {
        let query: Vec<(&str, String)> = match since {
            Some(ts) => vec![("_lastUpdated", format!("ge{ts}"))],
            None => vec![],
        };
        let v = self.authenticated_get("Observation", &query).await?;
        Ok(serde_json::from_value(v)
            .map_err(|e| MedicoError::Other(format!("Bundle-Parse (Observation): {e}")))?
    }

    /// `GET /DocumentReference` — alle Dokument-Referenzen (Arztbriefe etc.).
    pub async fn get_document_references(&self) -> Result<FhirBundle, MedicoError> {
        let v = self.authenticated_get("DocumentReference", &[]).await?;
        Ok(serde_json::from_value(v).map_err(|e| {
            MedicoError::Other(format!("Bundle-Parse (DocumentReference): {e}"))
        })?
    }

    /// `GET /MedicationRequest` — alle Medikamenten-Aufforderungen.
    pub async fn get_medication_requests(&self) -> Result<FhirBundle, MedicoError> {
        let v = self.authenticated_get("MedicationRequest", &[]).await?;
        Ok(serde_json::from_value(v).map_err(|e| {
            MedicoError::Other(format!("Bundle-Parse (MedicationRequest): {e}"))
        })?
    }
}

impl Clone for FhirClient {
    fn clone(&self) -> Self {
        // Token wird nicht geklont — bei Bedarf neu angefordert.
        let token = {
            let guard = self.token.blocking_read();
            guard.clone()
        };
        Self {
            http: reqwest::Client::new(),
            config: self.config.clone(),
            token: RwLock::new(token),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
        }
    }
}

// `Arc`-Kompatibilität (für Handler, die den Client im State haben)
pub type SharedFhirClient = Arc<FhirClient>;

// ===========================================================================
// Unit-Tests mit wiremock
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Baut eine Test-Config, die auf den wiremock-Server zeigt.
    fn test_config(server: &MockServer) -> MedicoInterfaceConfig {
        let mut cfg = MedicoInterfaceConfig::default();
        cfg.base_url = format!("{}/fhir", server.uri());
        cfg.auth_endpoint = format!("{}/auth/token", server.uri());
        cfg.client_id_env = "KIS_MEDICO_TEST_CLIENT_ID".into();
        cfg.client_secret_env = "KIS_MEDICO_TEST_CLIENT_SECRET".into();
        cfg.scopes = vec![SCOPE_ISIK_READ.into()];
        cfg.request_timeout_seconds = 5;
        cfg
    }

    /// Mock: Token-Endpunkt liefert gültiges Token.
    fn mock_token(server: &MockServer, token: &str) -> Mock {
        Mock::given(method("POST"))
            .and(path("/auth/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "access_token": token,
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "scope": SCOPE_ISIK_READ
                    })),
            )
            .expect(1..3)
    }

    /// Mock: FHIR-Endpunkt liefert leeres Bundle.
    fn mock_bundle(server: &MockServer, fhir_path: &str, count: usize) -> Mock {
        Mock::given(method("GET"))
            .and(path(format!("/fhir/{fhir_path}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "resourceType": "Bundle",
                        "entry": vec![
                            {"resource": {"resourceType": "Patient", "id": "p1"}};
                            count
                        ]
                    })),
            )
            .expect(1..3)
    }

    #[tokio::test]
    async fn test_get_patients_ok() {
        let server = MockServer::start().await;
        mock_token(&server, "tok1").mount(&server).await;
        mock_bundle(&server, "Patient", 2).mount(&server).await;

        let client = FhirClient::new_for_test(test_config(&server), "cid", "cs");
        let bundle = client.get_patients().await.expect("get_patients");
        assert_eq!(bundle.entry.len(), 2);
    }

    #[tokio::test]
    async fn test_token_refresh_on_401() {
        let server = MockServer::start().await;
        // Token-Endpunkt: 2x aufgerufen (initial + Refresh nach 401)
        Mock::given(method("POST"))
            .and(path("/auth/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "fresh-token",
                    "token_type": "Bearer",
                    "expires_in": 3600
                })),
            )
            .expect(2)
            .mount(&server)
            .await;

        // FHIR: 1x 401, dann 200
        Mock::given(method("GET"))
            .and(path("/fhir/Patient"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/fhir/Patient"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "resourceType": "Bundle",
                        "entry": []
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = FhirClient::new_for_test(test_config(&server), "cid", "cs");
        let bundle = client.get_patients().await.expect("401 → Refresh → 200");
        assert_eq!(bundle.entry.len(), 0);
    }

    #[tokio::test]
    async fn test_persistent_401_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "bad",
                    "expires_in": 3600
                })),
            )
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/fhir/Patient"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .expect(2)
            .mount(&server)
            .await;

        let client = FhirClient::new_for_test(test_config(&server), "cid", "cs");
        let err = client.get_patients().await.unwrap_err();
        assert!(
            matches!(err, MedicoError::Unauthorized(_)),
            "erwartet Unauthorized, bekommen: {err}"
        );
    }

    #[tokio::test]
    async fn test_404_is_notfound() {
        let server = MockServer::start().await;
        mock_token(&server, "t").mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/fhir/Patient"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let client = FhirClient::new_for_test(test_config(&server), "cid", "cs");
        let err = client.get_patients().await.unwrap_err();
        assert!(matches!(err, MedicoError::NotFound(_)), "erwartet NotFound: {err}");
    }

    #[tokio::test]
    async fn test_429_is_rate_limited() {
        let server = MockServer::start().await;
        mock_token(&server, "t").mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/fhir/Patient"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "30")
                    .set_body_string("slow down"),
            )
            .mount(&server)
            .await;

        let client = FhirClient::new_for_test(test_config(&server), "cid", "cs");
        let err = client.get_patients().await.unwrap_err();
        assert!(matches!(err, MedicoError::RateLimited(_)), "erwartet RateLimited: {err}");
    }

    #[tokio::test]
    async fn test_appointments_since_query() {
        let server = MockServer::start().await;
        mock_token(&server, "t").mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/fhir/Appointment"))
            .and(wiremock::matchers::query_param(
                "_lastUpdated",
                "ge2025-01-01T00:00:00Z",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "resourceType": "Bundle",
                        "entry": [
                            {"resource": {"resourceType": "Appointment", "id": "a1"}}
                        ]
                    })),
            )
            .mount(&server)
            .await;

        let client = FhirClient::new_for_test(test_config(&server), "cid", "cs");
        let since = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let bundle = client.get_appointments(Some(since)).await.unwrap();
        assert_eq!(bundle.entry.len(), 1);
    }
}
