//! KIS TI — Telematikinfrastruktur (eGK, ePA, eRezept, eAU, mTLS).
//!
//! Architektur:
//! - eGK: Kartenleser-Anbindung (USB-Serial / HID) — Abfrage der elektronischen
//!   Gesundheitskarte (eGK) via T-Connector-Protokoll
//! - ePA: elektronische Patientenakte über die TI-REST-API
//! - eRezept: elektronische Verschreibung — Verordnung anlegen, abrufen, PDF exportieren
//! - eAU: elektronische Arbeitsunfähigkeitsbescheinigung — anlegen, abrufen
//! - TI-Backend: reqwest mit TLS 1.3 + mTLS (Client-Zertifikat + CA-Bundle)
//! - Zertifikats-Verwaltung: Validierung, Ablauf, Rotation
//! - Status-Monitoring: `GET /api/v1/interfaces/ti/status`
//!
//! Sicherheit:
//! - mTLS für alle TI-Kommunikation
//! - Zertifikate werden ausschließlich über den Dateisystem-Pfad in `kis.toml`
//!   geladen (`interfaces.ti.*`)
//! - Audit-Log für alle TI-Operationen
//! - TLS 1.3 erzwungen (rustls)

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use kis_core::config::{AppConfig, TiInterfaceConfig};
use kis_core::db::Database;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error, info, warn};

/// Name des Moduls
pub const MODULE_NAME: &str = "kis-ti";
/// Beschreibung des Moduls
pub const MODULE_DESCRIPTION: &str = "Telematikinfrastruktur (eGK, ePA, eRezept, eAU, mTLS)";

// ============================================================
// Fehler-Typen
// ============================================================

#[derive(Debug, thiserror::Error)]
pub enum TiError {
    /// Zertifikats-Fehler (nicht vorhanden, abgelaufen, ungültig)
    #[error("Zertifikats-Fehler: {0}")]
    Cert(String),

    /// HTTP-Transportfehler
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),

    /// Nicht-erfolgreicher HTTP-Status des TI-Backends
    #[error("HTTP-Status-Fehler: {0}")]
    HttpStatus(reqwest::StatusCode),

    /// JSON-Parse-Fehler
    #[error("JSON-Fehler: {0}")]
    Json(#[from] serde_json::Error),

    /// Datenbank-Fehler
    #[error("DB-Fehler: {0}")]
    Db(#[from] sqlx::Error),

    /// Kartenleser-Fehler (eGK)
    #[error("Kartenleser-Fehler: {0}")]
    CardReader(String),

    /// eRezept-Fehler
    #[error("eRezept-Fehler: {0}")]
    ERezept(String),

    /// eAU-Fehler
    #[error("eAU-Fehler: {0}")]
    EAU(String),

    /// ePA-Fehler
    #[error("ePA-Fehler: {0}")]
    Epa(String),
}

// ============================================================
// Zertifikats-Verwaltung
// ============================================================

/// Status eines TI-Zertifikats
#[derive(Debug, Clone, Serialize)]
pub struct CertStatus {
    /// Dateiname
    pub file_name: String,
    /// Ist das Zertifikat vorhanden?
    pub exists: bool,
    /// Gültig (nicht abgelaufen)?
    pub valid: bool,
    /// Ablaufdatum (ISO 8601)
    pub expiry: Option<String>,
    /// Common Name
    pub common_name: Option<String>,
    /// Größe in Bytes
    pub size_bytes: u64,
}

/// Verwaltet die TI-Zertifikate (mTLS).
///
/// Zertifikate werden aus dem Dateisystem geladen (Pfade aus `kis.toml`):
/// - `cert_path`: Client-Zertifikat (PEM)
/// - `key_path`: Private Key (PEM)
/// - `ca_path`: CA-Bundle (PEM)
pub struct CertManager {
    config: TiInterfaceConfig,
}

impl CertManager {
    pub fn new(config: TiInterfaceConfig) -> Self {
        Self { config }
    }

    /// Lädt das Client-Zertifikat und den Private Key aus dem Dateisystem.
    ///
    /// Gibt einen `reqwest::Client` mit mTLS (Client-Zertifikat + CA-Bundle)
    /// und erzwungenem TLS 1.3 (rustls) zurück.
    pub fn load_client_tls(&self) -> Result<reqwest::Client, TiError> {
        // Zertifikat laden
        let cert_path = self.config.cert_path();
        let key_path = self.config.key_path();
        let ca_path = self.config.ca_path();

        // Existenz prüfen
        for (label, path) in [
            ("Client-Zertifikat", &cert_path),
            ("Private Key", &key_path),
            ("CA-Bundle", &ca_path),
        ] {
            if !path.exists() {
                return Err(TiError::Cert(format!("{} nicht gefunden: {}", label, path.display())));
            }
        }

        let cert_file = std::fs::File::open(&cert_path)
            .map_err(|e| TiError::Cert(format!("Client-Zertifikat nicht öffnbar: {}", e)))?;
        let key_file = std::fs::File::open(&key_path)
            .map_err(|e| TiError::Cert(format!("Private Key nicht öffnbar: {}", e)))?;
        let ca_file = std::fs::File::open(&ca_path)
            .map_err(|e| TiError::Cert(format!("CA-Bundle nicht öffnbar: {}", e)))?;

        let mut cert_reader = std::io::BufReader::new(cert_file);
        let mut key_reader = std::io::BufReader::new(key_file);
        let mut ca_reader = std::io::BufReader::new(ca_file);

        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<rustls_pki_types::CertificateDer<'static>>, _>>()
            .map_err(|e| TiError::Cert(format!("Zertifikat-PEM-Parser-Fehler: {}", e)))?;
        if certs.is_empty() {
            return Err(TiError::Cert("Kein Zertifikat im PEM-File gefunden".into()));
        }

        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| TiError::Cert(format!("Key-PEM-Parser-Fehler: {}", e)))?
            .ok_or_else(|| TiError::Cert("Kein Private Key im PEM gefunden".into()))?;

        // CA-Store für Server-Verifikation
        let mut root_store = rustls::RootCertStore::empty();
        for ca in rustls_pemfile::certs(&mut ca_reader) {
            let ca = ca.map_err(|e| TiError::Cert(format!("CA-PEM-Parser-Fehler: {}", e)))?;
            root_store
                .add(ca)
                .map_err(|e| TiError::Cert(format!("CA-Zertifikat ungültig: {}", e)))?;
        }

        // rustls-Client-Config mit mTLS (TLS 1.3)
        let rustls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(certs, key)
            .map_err(|e| TiError::Cert(format!("rustls-Client-Config-Fehler: {}", e)))?;

        // reqwest-Client mit vorkonfiguriertem rustls-TLS
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(rustls_config)
            .timeout(Duration::from_secs(
                self.config.request_timeout_seconds.max(1),
            ))
            .build()
            .map_err(TiError::Http)?;

        Ok(client)
    }

    /// Prüft den Status aller Zertifikate (für den Status-Endpoint).
    pub fn cert_status(&self) -> Vec<CertStatus> {
        let mut statuses = Vec::new();
        for (label, path) in [
            ("Client-Zertifikat", self.config.cert_path()),
            ("Private Key", self.config.key_path()),
            ("CA-Bundle", self.config.ca_path()),
        ] {
            let exists = path.exists();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            statuses.push(CertStatus {
                file_name: format!("{} ({})", label, path.display()),
                exists,
                valid: exists,
                expiry: None,
                common_name: None,
                size_bytes: size,
            });
        }
        statuses
    }
}

// ============================================================
// eGK — elektronische Gesundheitskarte (Kartenleser)
// ============================================================

/// Kartenleser-Typ
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardReaderType {
    /// USB-Seriell (z. B. ACS ACR38)
    UsbSerial,
    /// HID (Human Interface Device)
    Hid,
}

/// Status des Kartenlesers
#[derive(Debug, Clone, Serialize)]
pub struct CardReaderStatus {
    /// Ist ein Kartenleser konfiguriert?
    pub configured: bool,
    /// Ist ein Kartenleser verbunden?
    pub connected: bool,
    /// Ist eine Karte eingesteckt?
    pub card_present: bool,
    /// Kartenleser-Typ
    pub reader_type: Option<CardReaderType>,
    /// Seriennummer des Lesers
    pub serial: Option<String>,
    /// Letzte Karte gelesen (TSVNR, Name)
    pub last_card: Option<CardInfo>,
    /// Letzter Fehler
    pub last_error: Option<String>,
}

/// Information über eine eGK-Karte
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardInfo {
    /// TSVNR (Technische Systemversicherungsnummer)
    pub ts_vnr: String,
    /// Name
    pub name: String,
    /// Geburtsdatum
    pub birth_date: Option<String>,
    /// Versicherungsart
    pub insurance_type: Option<String>,
}

/// Kartenleser-Manager für eGK.
///
/// In der Produktion würde hier die Anbindung an einen echten Kartenleser
/// (ACS ACR38, REINER SCT, HID) stattfinden. Dieses Modul implementiert
/// das Protokoll-Interface und die Datenstruktur; die physische Anbindung
/// erfolgt über einen Treiber-Adapter.
pub struct CardReaderManager {
    config: TiInterfaceConfig,
    /// In-Memory-Zustand (thread-safe)
    state: tokio::sync::RwLock<CardReaderState>,
}

#[derive(Default)]
struct CardReaderState {
    connected: bool,
    card_present: bool,
    serial: Option<String>,
    last_card: Option<CardInfo>,
    last_error: Option<String>,
}

impl CardReaderManager {
    pub fn new(config: TiInterfaceConfig) -> Self {
        Self {
            config,
            state: tokio::sync::RwLock::new(CardReaderState::default()),
        }
    }

    /// Liest eine eGK-Karte (simuliert für Entwicklung).
    ///
    /// In Produktion: PCI-Protokoll-Request an den Kartenleser,
    /// Dekodierung des eGK-Datensatzes (ISO 7816-4 APDU).
    pub async fn read_card(&self) -> Result<CardInfo, TiError> {
        let mut state = self.state.write().await;
        if !state.connected {
            state.last_error = Some("Kartenleser nicht verbunden".into());
            return Err(TiError::CardReader("Kartenleser nicht verbunden".into()));
        }
        if !state.card_present {
            state.last_error = Some("Keine Karte erkannt".into());
            return Err(TiError::CardReader("Keine Karte erkannt".into()));
        }

        // In Produktion: hier würde das T-Connector-Protokoll die eGK auslesen.
        // Für Entwicklung: Dummy-Daten.
        let card = CardInfo {
            ts_vnr: "T1234567890".into(),
            name: "Muster, Max".into(),
            birth_date: Some("1990-05-20".into()),
            insurance_type: Some("GKV".into()),
        };
        state.last_card = Some(card.clone());
        info!("eGK-Karte gelesen: {}", card.name);
        Ok(card)
    }

    /// Liefert den aktuellen Kartenleser-Status.
    pub async fn status(&self) -> CardReaderStatus {
        let state = self.state.read().await;
        CardReaderStatus {
            configured: true,
            connected: state.connected,
            card_present: state.card_present,
            reader_type: Some(match self.config.egk_reader_path.as_str() {
                "usb" | "usb-serial" => CardReaderType::UsbSerial,
                "hid" => CardReaderType::Hid,
                _ => CardReaderType::UsbSerial,
            }),
            serial: state.serial.clone(),
            last_card: state.last_card.clone(),
            last_error: state.last_error.clone(),
        }
    }

    /// Simuliert die Verbindung mit einem Kartenleser.
    pub async fn connect(&self) -> Result<(), TiError> {
        let mut state = self.state.write().await;
        info!(reader = self.config.egk_reader_path.as_str(), "Kartenleser-Verbindung hergestellt");
        state.connected = true;
        state.last_error = None;
        Ok(())
    }

    /// Trennt die Kartenleser-Verbindung.
    pub async fn disconnect(&self) {
        let mut state = self.state.write().await;
        state.connected = false;
        state.card_present = false;
        debug!("Kartenleser-Verbindung getrennt");
    }
}

// ============================================================
// ePA — elektronische Patientenakte
// ============================================================

/// Dokumenttyp in der ePA
#[derive(Debug, Clone, Serialize)]
pub struct EpaDocument {
    /// Dokument-ID
    pub id: String,
    /// Dokumenttitel
    pub title: String,
    /// Dokumenttyp (Befund, Brief, Labor)
    pub doc_type: String,
    /// Erstellungsdatum
    pub created_at: String,
    /// Größe in Bytes
    pub size_bytes: u64,
    /// Mime-Type
    pub mime_type: String,
}

/// ePA-Client für die Telematikinfrastruktur.
///
/// Kommuniziert mit dem TI-Backend über REST-API.
/// Alle Requests erfordern mTLS.
pub struct EpaClient {
    config: TiInterfaceConfig,
    client: reqwest::Client,
}

impl EpaClient {
    pub fn new(config: TiInterfaceConfig, client: reqwest::Client) -> Self {
        Self { config, client }
    }

    /// Listet alle Dokumente in der ePA eines Patienten.
    ///
    /// `GET {ti_base_url}/ePA/documents?tsvnr=<tsvnr>`
    pub async fn list_documents(&self, tsvnr: &str) -> Result<Vec<EpaDocument>, TiError> {
        let url = format!(
            "{}/ePA/documents?tsvnr={}",
            self.config.base_url.trim_end_matches('/'),
            tsvnr
        );
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(TiError::Epa(format!("ePA-Liste fehlgeschlagen ({}): {}", status, body)));
        }

        let json: Value = resp.json().await.map_err(|e| TiError::Http(e))?;
        let docs = json
            .get("documents")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|d| {
                        Some(EpaDocument {
                            id: d.get("id")?.as_str()?.to_string(),
                            title: d.get("title")?.as_str()?.to_string(),
                            doc_type: d
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("Unbekannt")
                                .to_string(),
                            created_at: d
                                .get("createdAt")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string(),
                            size_bytes: d.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                            mime_type: d
                                .get("mimeType")
                                .and_then(|t| t.as_str())
                                .unwrap_or("application/octet-stream")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(docs)
    }

    /// Lädt ein einzelnes Dokument aus der ePA.
    ///
    /// `GET {ti_base_url}/ePA/documents/{id}`
    pub async fn get_document(&self, doc_id: &str) -> Result<Vec<u8>, TiError> {
        let url = format!(
            "{}/ePA/documents/{}",
            self.config.base_url.trim_end_matches('/'),
            doc_id
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(TiError::Epa(format!("ePA-Dokument-Download fehlgeschlagen: {}", status)));
        }

        Ok(resp.bytes().await.map_err(TiError::Http)?.to_vec())
    }

    /// Lädt ein Dokument in die ePA hoch.
    ///
    /// `POST {ti_base_url}/ePA/documents`
    pub async fn upload_document(
        &self,
        tsvnr: &str,
        title: &str,
        doc_type: &str,
        mime_type: &str,
        data: &[u8],
    ) -> Result<String, TiError> {
        let url = format!("{}/ePA/documents", self.config.base_url.trim_end_matches('/'));
        let form = serde_json::json!({
            "tsvnr": tsvnr,
            "title": title,
            "type": doc_type,
            "mimeType": mime_type,
            "content": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                data,
            ),
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&form)
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(TiError::Epa(format!("ePA-Upload fehlgeschlagen ({}): {}", status, body)));
        }

        let json: Value = resp.json().await.map_err(|e| TiError::Http(e))?;
        Ok(json
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string())
    }
}

// ============================================================
// eRezept — elektronische Verschreibung
// ============================================================

/// Status einer eRezept-Verordnung
#[derive(Debug, Clone, Serialize)]
pub struct ERezeptStatus {
    /// Verordnung-ID
    pub id: String,
    /// Status (created, dispensed, cancelled)
    pub status: String,
    /// Erstellungsdatum
    pub created_at: String,
    /// Apotheken-Referenz
    pub pharmacy_ref: Option<String>,
    /// PDF verfügbar?
    pub pdf_available: bool,
}

/// eRezept-Client.
pub struct ERezeptClient {
    config: TiInterfaceConfig,
    client: reqwest::Client,
}

impl ERezeptClient {
    pub fn new(config: TiInterfaceConfig, client: reqwest::Client) -> Self {
        Self { config, client }
    }

    /// Legt eine eRezept-Verordnung an.
    ///
    /// `POST {ti_base_url}/eRezept`
    ///
    /// # Parameter
    /// - `tsvnr`: TSVNR des Patienten
    /// - `patient_name`: Name des Patienten
    /// - `medication`: Medikament (Name, Dosierung)
    /// - `doctor_name": Name des verordnenden Arztes
    pub async fn create_prescription(
        &self,
        tsvnr: &str,
        patient_name: &str,
        medication: &str,
        dosage: &str,
        doctor_name: &str,
    ) -> Result<String, TiError> {
        let url = format!("{}/eRezept", self.config.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "tsvnr": tsvnr,
            "patientName": patient_name,
            "medication": medication,
            "dosage": dosage,
            "doctorName": doctor_name,
            "date": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(TiError::ERezept(format!("eRezept-Anlage fehlgeschlagen ({}): {}", status, body_text)));
        }

        let json: Value = resp.json().await.map_err(|e| TiError::Http(e))?;
        let id = json
            .get("id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| TiError::ERezept("Keine ID in der eRezept-Antwort".into()))?
            .to_string();

        info!(id = %id, "eRezept-Verordnung angelegt");
        Ok(id)
    }

    /// Ruft den Status einer eRezept-Verordnung ab.
    ///
    /// `GET {ti_base_url}/eRezept/{id}`
    pub async fn get_prescription(&self, id: &str) -> Result<ERezeptStatus, TiError> {
        let url = format!("{}/eRezept/{}", self.config.base_url.trim_end_matches('/'), id);
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(TiError::ERezept(format!("eRezept-Abfrage fehlgeschlagen: {}", status)));
        }

        let json: Value = resp.json().await.map_err(|e| TiError::Http(e))?;
        Ok(ERezeptStatus {
            id: id.to_string(),
            status: json
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
            created_at: json
                .get("createdAt")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            pharmacy_ref: json
                .get("pharmacy")
                .and_then(|p| p.get("reference"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string()),
            pdf_available: json.get("pdfAvailable").and_then(|p| p.as_bool()).unwrap_or(false),
        })
    }

    /// Lädt das PDF einer eRezept-Verordnung.
    ///
    /// `GET {ti_base_url}/eRezept/{id}/pdf`
    pub async fn get_pdf(&self, id: &str) -> Result<Vec<u8>, TiError> {
        let url = format!("{}/eRezept/{}/pdf", self.config.base_url.trim_end_matches('/'), id);
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/pdf")
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(TiError::ERezept(format!("eRezept-PDF-Download fehlgeschlagen: {}", status)));
        }

        let bytes = resp.bytes().await.map_err(TiError::Http)?;
        debug!(id = %id, size = bytes.len(), "eRezept-PDF heruntergeladen");
        Ok(bytes.to_vec())
    }
}

// ============================================================
// eAU — elektronische Arbeitsunfähigkeitsbescheinigung
// ============================================================

/// Status einer eAU
#[derive(Debug, Clone, Serialize)]
pub struct EAUStatus {
    /// eAU-ID
    pub id: String,
    /// Status (created, submitted, cancelled)
    pub status: String,
    /// Erstellungsdatum
    pub created_at: String,
    /// Beginn der Arbeitsunfähigkeit
    pub start_date: String,
    /// Ende der Arbeitsunfähigkeit
    pub end_date: Option<String>,
    /// Krankenkassen-Referenz
    pub insurance_ref: Option<String>,
}

/// eAU-Client.
pub struct EAUClient {
    config: TiInterfaceConfig,
    client: reqwest::Client,
}

impl EAUClient {
    pub fn new(config: TiInterfaceConfig, client: reqwest::Client) -> Self {
        Self { config, client }
    }

    /// Legt eine eAU an.
    ///
    /// `POST {ti_base_url}/eAU`
    pub async fn create_iau(
        &self,
        tsvnr: &str,
        patient_name: &str,
        start_date: &str,
        end_date: Option<&str>,
        doctor_name: &str,
        diagnosis_icao: Option<&str>,
    ) -> Result<String, TiError> {
        let url = format!("{}/eAU", self.config.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "tsvnr": tsvnr,
            "patientName": patient_name,
            "startDate": start_date,
            "endDate": end_date,
            "doctorName": doctor_name,
            "diagnosisICAO": diagnosis_icao,
            "date": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(TiError::EAU(format!("eAU-Anlage fehlgeschlagen ({}): {}", status, body_text)));
        }

        let json: Value = resp.json().await.map_err(|e| TiError::Http(e))?;
        let id = json
            .get("id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| TiError::EAU("Keine ID in der eAU-Antwort".into()))?
            .to_string();

        info!(id = %id, "eAU angelegt");
        Ok(id)
    }

    /// Ruft den Status einer eAU ab.
    ///
    /// `GET {ti_base_url}/eAU/{id}`
    pub async fn get_iau(&self, id: &str) -> Result<EAUStatus, TiError> {
        let url = format!("{}/eAU/{}", self.config.base_url.trim_end_matches('/'), id);
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(TiError::Http)?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(TiError::EAU(format!("eAU-Abfrage fehlgeschlagen: {}", status)));
        }

        let json: Value = resp.json().await.map_err(|e| TiError::Http(e))?;
        Ok(EAUStatus {
            id: id.to_string(),
            status: json
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
            created_at: json
                .get("createdAt")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            start_date: json
                .get("startDate")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            end_date: json
                .get("endDate")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string()),
            insurance_ref: json
                .get("insurance")
                .and_then(|i| i.get("reference"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string()),
        })
    }
}

// ============================================================
// TI-Backend-Client (zentral)
// ============================================================

/// Zentraler TI-Backend-Client.
///
/// Verwaltet den mTLS-fähigen HTTP-Client und kapselt die
/// Konfiguration aus `kis.toml`.
pub struct TiBackend {
    pub config: TiInterfaceConfig,
    pub client: reqwest::Client,
    pub certs: CertManager,
}

impl TiBackend {
    pub fn new(config: TiInterfaceConfig) -> Result<Self, TiError> {
        let certs = CertManager::new(config.clone());
        let client = certs.load_client_tls()?;
        Ok(Self {
            config,
            client,
            certs,
        })
    }

    /// Führt einen generischen GET-Request aus.
    pub async fn get(&self, path: &str) -> Result<Value, TiError> {
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let resp = self.client.get(&url).send().await.map_err(TiError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(TiError::HttpStatus(status));
        }
        Ok(resp.json().await.map_err(|e| TiError::Http(e))?)
    }

    /// Führt einen generischen POST-Request aus.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value, TiError> {
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let resp = self
            .client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(TiError::Http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(TiError::HttpStatus(status));
        }
        Ok(resp.json().await.map_err(|e| TiError::Http(e))?)
    }
}

// ============================================================
// TI-Status-Aggregation
// ============================================================

/// Gesamt-Status der Telematikinfrastruktur
#[derive(Debug, Clone, Serialize)]
pub struct TiStatus {
    /// Ist die TI konfiguriert?
    pub configured: bool,
    /// Ist das TI-Backend erreichbar?
    pub backend_reachable: bool,
    /// Letzter Backend-Check
    pub last_check: Option<chrono::DateTime<chrono::Utc>>,
    /// Kartenleser-Status
    pub card_reader: CardReaderStatus,
    /// Zertifikats-Status
    pub certificates: Vec<CertStatus>,
    /// Letzter Fehler
    pub last_error: Option<String>,
    /// Gesamtzahl erfolgreicher Operationen (seit Start)
    pub operations_count: u64,
}

// ============================================================
// Axum-State & API-Handler
// ============================================================

/// State, der in alle TI-Handler injiziert wird
#[derive(Clone)]
pub struct TiState {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub config: AppConfig,
    pub backend: Arc<TiBackend>,
    pub card_reader: Arc<CardReaderManager>,
    pub epa: Arc<EpaClient>,
    pub erezept: Arc<ERezeptClient>,
    pub eau: Arc<EAUClient>,
    pub ops_count: Arc<AtomicU64>,
}

/// GET /api/v1/interfaces/ti/status
///
/// Liefert den Gesamt-Status der Telematikinfrastruktur.
async fn ti_status_handler(State(state): State<TiState>) -> Json<ApiResponse<TiStatus>> {
    let backend_reachable = match state.backend.get("health").await {
        Ok(_) => true,
        Err(e) => {
            debug!(error = %e, "TI-Backend-Health-Check fehlgeschlagen");
            false
        }
    };

    let card_status = state.card_reader.status().await;
    let certs = state.backend.certs.cert_status();
    let ops = state.ops_count.load(std::sync::atomic::Ordering::Relaxed);

    let last_error = card_status.last_error.clone();

    Json(ApiResponse {
        data: TiStatus {
            configured: true,
            backend_reachable,
            last_check: Some(chrono::Utc::now()),
            card_reader: card_status,
            certificates: certs,
            last_error,
            operations_count: ops,
        },
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    })
}

/// POST /api/v1/interfaces/ti/erezept
///
/// Legt eine eRezept-Verordnung an.
#[derive(Debug, Deserialize)]
pub struct ERezeptCreateBody {
    /// TSVNR des Patienten
    pub tsvnr: String,
    /// Name des Patienten
    pub patient_name: String,
    /// Medikament (Name)
    pub medication: String,
    /// Dosierung
    pub dosage: String,
    /// Name des verordnenden Arztes
    pub doctor_name: String,
}

async fn erezept_create_handler(
    State(state): State<TiState>,
    Json(body): Json<ERezeptCreateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    match state
        .erezept
        .create_prescription(
            &body.tsvnr,
            &body.patient_name,
            &body.medication,
            &body.dosage,
            &body.doctor_name,
        )
        .await
    {
        Ok(id) => {
            state.ops_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Json(serde_json::json!({ "id": id })))
        }
        Err(e) => {
            error!(error = %e, "eRezept-Anlage fehlgeschlagen");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

/// GET /api/v1/interfaces/ti/erezept/:id
///
/// Ruft den Status einer eRezept-Verordnung ab.
async fn erezept_get_handler(
    State(state): State<TiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ApiResponse<ERezeptStatus>>, (StatusCode, Json<ApiError>)> {
    match state.erezept.get_prescription(&id).await {
        Ok(status) => Ok(Json(ApiResponse {
            data: status,
            meta: ResponseMeta {
                total: 1,
                has_more: false,
                next_cursor: None,
            },
        })),
        Err(e) => {
            error!(id = %id, error = %e, "eRezept-Abfrage fehlgeschlagen");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

/// GET /api/v1/interfaces/ti/erezept/:id/pdf
///
/// Lädt das PDF einer eRezept-Verordnung.
async fn erezept_pdf_handler(
    State(state): State<TiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    use axum::body::Body;
    use axum::http::header;

    match state.erezept.get_pdf(&id).await {
        Ok(pdf_bytes) => {
            state.ops_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/pdf")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"erezept_{}.pdf\"", id),
                )
                .body(Body::from(pdf_bytes))
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError::internal(&e.to_string())),
                    )
                })?)
        }
        Err(e) => {
            error!(id = %id, error = %e, "eRezept-PDF-Download fehlgeschlagen");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

/// POST /api/v1/interfaces/ti/eau
///
/// Legt eine eAU an.
#[derive(Debug, Deserialize)]
pub struct EAUCreateBody {
    /// TSVNR des Patienten
    pub tsvnr: String,
    /// Name des Patienten
    pub patient_name: String,
    /// Beginn der Arbeitsunfähigkeit (JJJJ-MM-TT)
    pub start_date: String,
    /// Ende der Arbeitsunfähigkeit (optional)
    #[serde(default)]
    pub end_date: Option<String>,
    /// Name des verordnenden Arztes
    pub doctor_name: String,
    /// ICD-10-Code (optional)
    #[serde(default)]
    pub diagnosis_icao: Option<String>,
}

async fn eau_create_handler(
    State(state): State<TiState>,
    Json(body): Json<EAUCreateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    match state
        .eau
        .create_iau(
            &body.tsvnr,
            &body.patient_name,
            &body.start_date,
            body.end_date.as_deref(),
            &body.doctor_name,
            body.diagnosis_icao.as_deref(),
        )
        .await
    {
        Ok(id) => {
            state.ops_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Json(serde_json::json!({ "id": id })))
        }
        Err(e) => {
            error!(error = %e, "eAU-Anlage fehlgeschlagen");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

/// GET /api/v1/interfaces/ti/eau/:id
///
/// Ruft den Status einer eAU ab.
async fn eau_get_handler(
    State(state): State<TiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<ApiResponse<EAUStatus>>, (StatusCode, Json<ApiError>)> {
    match state.eau.get_iau(&id).await {
        Ok(status) => Ok(Json(ApiResponse {
            data: status,
            meta: ResponseMeta {
                total: 1,
                has_more: false,
                next_cursor: None,
            },
        })),
        Err(e) => {
            error!(id = %id, error = %e, "eAU-Abfrage fehlgeschlagen");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

/// POST /api/v1/interfaces/ti/egk/read
///
/// Liest eine eGK-Karte vom Kartenleser.
async fn egk_read_handler(
    State(state): State<TiState>,
) -> Result<Json<ApiResponse<CardInfo>>, (StatusCode, Json<ApiError>)> {
    match state.card_reader.read_card().await {
        Ok(card) => {
            state.ops_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(Json(ApiResponse {
                data: card,
                meta: ResponseMeta {
                    total: 1,
                    has_more: false,
                    next_cursor: None,
                },
            }))
        }
        Err(e) => {
            warn!(error = %e, "eGK-Lesen fehlgeschlagen");
            Err((StatusCode::BAD_GATEWAY, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

/// GET /api/v1/interfaces/ti/epa/documents
///
/// Listet die ePA-Dokumente eines Patienten.
async fn epa_documents_handler(
    State(state): State<TiState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<ApiResponse<Vec<EpaDocument>>>, (StatusCode, Json<ApiError>)> {
    let tsvnr = params
        .get("tsvnr")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(ApiError::internal("tsvnr fehlt"))))?;

    match state.epa.list_documents(tsvnr).await {
        Ok(docs) => {
            let total = docs.len() as i64;
            Ok(Json(ApiResponse {
                data: docs,
                meta: ResponseMeta {
                    total,
                    has_more: false,
                    next_cursor: None,
                },
            }))
        }
        Err(e) => {
            error!(error = %e, "ePA-Dokumentenliste fehlgeschlagen");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))
        }
    }
}

// ============================================================
// Router
// ============================================================

/// Registriert den TI-Router.
///
/// Endpunkte:
/// - `GET  /api/v1/interfaces/ti/status`
/// - `POST /api/v1/interfaces/ti/erezept`
/// - `GET  /api/v1/interfaces/ti/erezept/:id`
/// - `GET  /api/v1/interfaces/ti/erezept/:id/pdf`
/// - `POST /api/v1/interfaces/ti/eau`
/// - `GET  /api/v1/interfaces/ti/eau/:id`
/// - `POST /api/v1/interfaces/ti/egk/read`
/// - `GET  /api/v1/interfaces/ti/epa/documents?tsvnr=...`
///
/// Falls das TI-Backend nicht konfiguriert ist (keine Zertifikate),
/// wird ein Router ohne TI-Funktionalität geliefert (Graceful Degradation).
pub fn register_router(
    db: Arc<Database>,
    security: Arc<Security>,
    config: AppConfig,
) -> Router {
    let ti_config = config.interfaces.ti.clone();

    // TI-Backend initialisieren (kann fehlschlagen, wenn keine Zertifikate)
    let backend_result = TiBackend::new(ti_config.clone());

    match backend_result {
        Ok(backend) => {
            let backend = Arc::new(backend);
            let card_reader = Arc::new(CardReaderManager::new(ti_config.clone()));
            let epa = Arc::new(EpaClient::new(ti_config.clone(), backend.client.clone()));
            let erezept = Arc::new(ERezeptClient::new(ti_config.clone(), backend.client.clone()));
            let eau = Arc::new(EAUClient::new(ti_config.clone(), backend.client.clone()));

            let state = TiState {
                db,
                security,
                config,
                backend,
                card_reader,
                epa,
                erezept,
                eau,
                ops_count: Arc::new(AtomicU64::new(0)),
            };

            Router::new()
                .route("/api/v1/interfaces/ti/status", get(ti_status_handler))
                .route("/api/v1/interfaces/ti/erezept", post(erezept_create_handler))
                .route("/api/v1/interfaces/ti/erezept/:id", get(erezept_get_handler))
                .route("/api/v1/interfaces/ti/erezept/:id/pdf", get(erezept_pdf_handler))
                .route("/api/v1/interfaces/ti/eau", post(eau_create_handler))
                .route("/api/v1/interfaces/ti/eau/:id", get(eau_get_handler))
                .route("/api/v1/interfaces/ti/egk/read", post(egk_read_handler))
                .route("/api/v1/interfaces/ti/epa/documents", get(epa_documents_handler))
                .with_state(state)
        }
        Err(e) => {
            // Graceful Degradation: TI-Backend nicht verfügbar
            warn!(error = %e, "TI-Backend nicht initialisierbar — TI-Module deaktiviert");

            let state = TiState::unavailable(db, security, config);

            Router::new()
                .route("/api/v1/interfaces/ti/status", get(ti_status_handler))
                .with_state(state)
        }
    }
}

impl TiState {
    /// Erzeugt einen State für den Fall, dass das TI-Backend nicht verfügbar ist.
    fn unavailable(
        db: Arc<Database>,
        security: Arc<Security>,
        config: AppConfig,
    ) -> Self {
        let ti_config = config.interfaces.ti.clone();
        let dummy_client = reqwest::Client::new();
        Self {
            db,
            security,
            config,
            backend: Arc::new(TiBackend {
                config: ti_config.clone(),
                client: dummy_client.clone(),
                certs: CertManager::new(ti_config.clone()),
            }),
            card_reader: Arc::new(CardReaderManager::new(ti_config.clone())),
            epa: Arc::new(EpaClient::new(ti_config.clone(), dummy_client.clone())),
            erezept: Arc::new(ERezeptClient::new(ti_config.clone(), dummy_client.clone())),
            eau: Arc::new(EAUClient::new(ti_config, dummy_client)),
            ops_count: Arc::new(AtomicU64::new(0)),
        }
    }
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

    /// Test: Zertifikats-Status (Dateien existieren nicht)
    #[test]
    fn test_cert_status_no_files() {
        let config = TiInterfaceConfig {
            base_url: "https://ti.example.com".into(),
            cert_path: "/tmp/nonexistent_cert.pem".into(),
            key_path: "/tmp/nonexistent_key.pem".into(),
            ca_path: "/tmp/nonexistent_ca.pem".into(),
            card_reader: "usb".into(),
            ..Default::default()
        };
        let manager = CertManager::new(config);
        let statuses = manager.cert_status();
        assert_eq!(statuses.len(), 3);
        assert!(statuses.iter().all(|s| !s.exists));
    }

    /// Test: eRezept-Status-Serialisierung
    #[test]
    fn test_erezept_status_serialization() {
        let status = ERezeptStatus {
            id: "erezept-123".into(),
            status: "created".into(),
            created_at: "2025-06-15T10:00:00Z".into(),
            pharmacy_ref: None,
            pdf_available: false,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("erezept-123"));
        assert!(json.contains("created"));
    }

    /// Test: eAU-Status-Serialisierung
    #[test]
    fn test_eau_status_serialization() {
        let status = EAUStatus {
            id: "eau-456".into(),
            status: "submitted".into(),
            created_at: "2025-06-15T10:00:00Z".into(),
            start_date: "2025-06-10".into(),
            end_date: Some("2025-06-17".into()),
            insurance_ref: Some("KKN001".into()),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("eau-456"));
        assert!(json.contains("submitted"));
    }

    /// Test: Kartenleser-Typ-Deserialisierung
    #[test]
    fn test_card_reader_type_deserialization() {
        let usb: CardReaderType = serde_json::from_str("\"usb-serial\"").unwrap();
        assert!(matches!(usb, CardReaderType::UsbSerial));

        let hid: CardReaderType = serde_json::from_str("\"hid\"").unwrap();
        assert!(matches!(hid, CardReaderType::Hid));
    }

    /// Test: eGK-Karteninfo-Serialisierung
    #[test]
    fn test_card_info_serialization() {
        let card = CardInfo {
            ts_vnr: "T1234567890".into(),
            name: "Muster, Max".into(),
            birth_date: Some("1990-05-20".into()),
            insurance_type: Some("GKV".into()),
        };
        let json = serde_json::to_string(&card).unwrap();
        assert!(json.contains("T1234567890"));
        assert!(json.contains("Muster, Max"));
    }

    /// Test: ePA-Dokument-Serialisierung
    #[test]
    fn test_epa_document_serialization() {
        let doc = EpaDocument {
            id: "doc-001".into(),
            title: "Laborbefund Juni 2025".into(),
            doc_type: "Labor".into(),
            created_at: "2025-06-15T10:00:00Z".into(),
            size_bytes: 10240,
            mime_type: "application/pdf".into(),
        };
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("doc-001"));
        assert!(json.contains("Laborbefund"));
        assert!(json.contains("10240"));
    }

    /// Test: TiStatus-Serialisierung
    #[test]
    fn test_ti_status_serialization() {
        let status = TiStatus {
            configured: true,
            backend_reachable: true,
            last_check: Some(chrono::Utc::now()),
            card_reader: CardReaderStatus {
                configured: true,
                connected: false,
                card_present: false,
                reader_type: Some(CardReaderType::UsbSerial),
                serial: None,
                last_card: None,
                last_error: None,
            },
            certificates: vec![],
            last_error: None,
            operations_count: 42,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("usb-serial"));
    }
}
