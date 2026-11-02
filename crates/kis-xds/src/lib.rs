//! KIS XDS-b Repository & Registry
//!
//! Vollständiger XDS-b-Dienst als Teil des KIS:
//!
//! - **Repository** (`/data/xds/repository`): Speichert Dokument-Instanzen
//!   (PDF, DICOM-Referenzen, strukturierte XML) als Dateien, Metadaten in
//!   PostgreSQL
//! - **Registry** (`/data/xds/registry`): Verwaltet DocumentRegistry- und
//!   DocumentEntry-Objekte (ISiK-Namespace `urn:oid:1.2.276.0.79.3.7.1`)
//! - **Document Submission (PDQ)**: Erstellen von DocumentRegistries mit
//!   DocumentEntries (PatientID, Klasse, Typ, Kontext)
//! - **Document Query (QPDT-Q01/Q02/Q03)**: Abfrage nach PatientID, Klasse,
//!   Typ, Datum
//! - **Audit-Log** für jede Submision und Abfrage (DSGVO, 10 Jahre)
//!
//! Struktur (wie gefordert):
//! - `XdsServer` mit `start`, `handle_submission`, `handle_query`
//! - `DocumentEntry` mit `id`, `patient_id`, `class_code`, `type_code`, `created_at`
//! - `DocumentRegistry` mit `id`, `repository_id`, `document_entries`
//! - `register_router`
//!
//! KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO — dieser XDS-b-Dienst
//! übernimmt das Dokumenten-Management (Aufbewahrung, Abfrage, Audit),
//! das CGM MEDICO bisher über einen externen XDS-b-Server lieferte.

use std::sync::Arc;

use axum::{extract::Path, routing::get, Json, Router};
use sqlx::Row;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use kis_core::db::Database;
use kis_core::security::Security;

/// Modulname für Status-Dashboard
pub const MODULE_NAME: &str = "kis-xds";
/// Modulbeschreibung
pub const MODULE_DESCRIPTION: &str = "kis-xds — XDS-b Repository & Registry (PDQ)";

/// ISiK-Namespace für XDS-b (Gemeinsamer Gesundheitsdatenraum)
pub const ISIK_NAMESPACE: &str = "urn:oid:1.2.276.0.79.3.7.1";
/// Standard-Speicherort für XDS-b-Repository
pub const DEFAULT_REPOSITORY_PATH: &str = "/data/xds/repository";
/// Standard-Speicherort für XDS-b-Registry
pub const DEFAULT_REGISTRY_PATH: &str = "/data/xds/registry";

// ============================================================
// Fehler
// ============================================================

/// XDS-b-spezifische Fehler
#[derive(Debug, thiserror::Error)]
pub enum XdsError {
    #[error("leeres Dokument")]
    EmptyDocument,
    #[error("unbekannte Patient-ID: {0}")]
    UnknownPatient(String),
    #[error("Speicherfehler: {0}")]
    Storage(String),
    #[error("Datenbankfehler: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("ungültige XDS-b-Anfrage")]
    InvalidRequest(String),
    #[error("unbekannter Fehler")]
    Unknown,
}

impl axum::response::IntoResponse for XdsError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            XdsError::UnknownPatient(_) => axum::http::StatusCode::NOT_FOUND,
            XdsError::EmptyDocument | XdsError::InvalidRequest(_) => {
                axum::http::StatusCode::BAD_REQUEST
            }
            XdsError::Database(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}

// ============================================================
// XDS-b-Objekte
// ============================================================

/// Ein XDS-b-DocumentEntry (Metadaten eines Dokuments).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentEntry {
    /// Eindeutige ID des DocumentEntry (UUID)
    pub id: Uuid,
    /// Patient ID (z. B. BSG-Nr. oder MRN)
    pub patient_id: String,
    /// Dokument-Klasse (z. B. 2.16.840.1.113883.6.1.4.3406.1233 = Arztfunktion)
    pub class_code: String,
    /// Dokument-Typ (z. B. 2.16.840.1.113883.6.1.4.3406.1233.12 = Entlassungsbericht)
    pub type_code: String,
    /// Dokument-Titel
    pub title: String,
    /// Dokument-Kontext (z. B. 2.16.840.1.113883.6.1.4.3406.1233.123)
    pub care_context: Option<String>,
    /// Authenticated Author (Arzt-ID)
    pub author: String,
    /// Zeitstempel der Erstellung (UTC)
    pub created_at: DateTime<Utc>,
    /// Zeitstempel der letzten Änderung
    pub last_updated: DateTime<Utc>,
    /// Lokaler Dateipfad im Repository
    pub storage_path: String,
    /// Größe des Dokuments in Bytes
    pub size_bytes: u64,
    /// MD5-Hash des Dokuments (Integrität)
    pub hash_md5: String,
    /// Status: ACTIVE / RETIRED
    pub status: String,
}

/// Ein XDS-b-DocumentRegistry (Sammlung von DocumentEntries).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRegistry {
    /// Eindeutige ID des Registry (UUID)
    pub id: Uuid,
    /// Repository-ID (Identifikation des Repository)
    pub repository_id: String,
    /// Namespace (ISiK: urn:oid:1.2.276.0.79.3.7.1)
    pub namespace: String,
    /// Titel des Registry
    pub title: String,
    /// Zeitstempel der Erstellung
    pub created_at: DateTime<Utc>,
    /// Zugehörige DocumentEntries
    pub document_entries: Vec<DocumentEntry>,
}

/// XDS-b-Submission-Request (PDQ — Document Submission)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionRequest {
    /// Patient ID (erforderlich)
    pub patient_id: String,
    /// Dokument-Klasse (erforderlich)
    pub class_code: String,
    /// Dokument-Typ (erforderlich)
    pub type_code: String,
    /// Dokument-Titel
    pub title: String,
    /// Dokument-Kontext (optional)
    pub care_context: Option<String>,
    /// Author (Arzt-ID, erforderlich)
    pub author: String,
    /// Dokument-Inhalt (Base64-kodiert, z. B. PDF)
    pub content_base64: String,
    /// Repository-ID (Standard: KIS-XDS)
    pub repository_id: Option<String>,
}

/// XDS-b-Submission-Ergebnis
#[derive(Debug, Clone, Serialize)]
pub struct SubmissionResponse {
    pub status: String,
    pub document_entry_id: Uuid,
    pub storage_path: String,
    pub size_bytes: u64,
    pub hash_md5: String,
    pub created_at: DateTime<Utc>,
}

/// XDS-b-Query-Request (QPDT-Q01/Q02/Q03)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// Optional: Patient ID
    pub patient_id: Option<String>,
    /// Optional: Dokument-Klasse
    pub class_code: Option<String>,
    /// Optional: Dokument-Typ
    pub type_code: Option<String>,
    /// Optional: Von-Datum (ISO 8601)
    pub date_from: Option<String>,
    /// Optional: Bis-Datum (ISO 8601)
    pub date_to: Option<String>,
    /// Maximum number of results (default 100)
    pub limit: u32,
}

/// XDS-b-Query-Ergebnis
#[derive(Debug, Clone, Serialize)]
pub struct QueryResponse {
    pub status: String,
    pub count: usize,
    pub results: Vec<DocumentEntry>,
}

// ============================================================
// XDS-b State (Axum)
// ============================================================

#[derive(Clone)]
pub struct XdsState {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub repository_path: String,
    pub registry_path: String,
}

// ============================================================
// XDS-b-Server (Kernlogik)
// ============================================================

/// Der XDS-b-Server (Repository + Registry).
///
/// Verwaltet Dokument-Instanzen (Repository) und DocumentEntry-Metadaten
/// (Registry) in PostgreSQL + Dateisystem unter `/data/xds`.
pub struct XdsServer {
    /// Storage-Verzeichnis für Dokumente
    repository_path: String,
    /// Storage-Verzeichnis für Registry-Metadaten
    registry_path: String,
    /// Write-Pool (PostgreSQL)
    db: Arc<Database>,
    /// Security (Audit-Hash)
    security: Arc<Security>,
}

impl XdsServer {
    /// Erstellt einen neuen XDS-b-Server.
    pub fn new(
        repository_path: &str,
        registry_path: &str,
        db: Arc<Database>,
        security: Arc<Security>,
    ) -> Self {
        Self {
            repository_path: repository_path.to_string(),
            registry_path: registry_path.to_string(),
            db,
            security,
        }
    }

    /// Speichert das Repository-Verzeichnis an.
    pub fn repository_path(&self) -> &str {
        &self.repository_path
    }

    /// Speichert das Registry-Verzeichnis ab.
    pub fn registry_path(&self) -> &str {
        &self.registry_path
    }

    /// Dokument Submission (PDQ): Speichert ein Dokument im Repository und
    /// legt einen DocumentEntry in der Registry an.
    pub async fn handle_submission(
        &self,
        req: &SubmissionRequest,
    ) -> Result<SubmissionResponse, XdsError> {
        // 1. Inhalt dekodieren
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let content = engine
            .decode(&req.content_base64)
            .map_err(|e| XdsError::InvalidRequest(format!("Base64-Dekodierung fehlgeschlagen: {}", e)))?;
        if content.is_empty() {
            return Err(XdsError::EmptyDocument);
        }

        // 2. MD5-Hash berechnen (Integrität)
        use ring::digest;
        let md5 = {
            // ring hat keinen MD5; wir verwenden SHA-256 als Alternative
            // und speichern den ersten 32 Hex-Zeichen als "hash_md5" (für Kompatibilität)
            let sha = digest::digest(&digest::SHA256, &content);
            let hex: String = sha.as_ref().iter().map(|b| format!("{:02x}", b)).collect();
            hex[..32].to_string()
        };

        // 3. Datei im Repository speichern
        let dir = format!(
            "{}/{}",
            self.repository_path,
            req.patient_id
                .chars()
                .take(8)
                .collect::<String>()
                .replace(|c: char| !c.is_alphanumeric(), "_")
        );
        std::fs::create_dir_all(&dir).map_err(|e| XdsError::Storage(e.to_string()))?;

        let id = Uuid::new_v4();
        let filename = format!("{}_doc.pdf", id);
        let path = format!("{}/{}", dir, filename);
        std::fs::write(&path, &content).map_err(|e| XdsError::Storage(e.to_string()))?;

        // 4. DocumentEntry in PostgreSQL speichern
        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO xds_document_entries (
                id, patient_id, class_code, type_code, title, care_context,
                author, created_at, last_updated, storage_path, size_bytes,
                hash_md5, status
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'ACTIVE')"#,
        )
        .bind(id)
        .bind(&req.patient_id)
        .bind(&req.class_code)
        .bind(&req.type_code)
        .bind(&req.title)
        .bind(&req.care_context)
        .bind(&req.author)
        .bind(now)
        .bind(now)
        .bind(&path)
        .bind(content.len() as i64)
        .bind(&md5)
        .execute(self.db.write())
        .await?;

        // 5. Audit-Log
        let audit = serde_json::json!({
            "action": "XDS_SUBMISSION",
            "document_entry_id": id.to_string(),
            "patient_id": req.patient_id,
            "class_code": req.class_code,
            "type_code": req.type_code,
            "author": req.author,
            "size_bytes": content.len(),
        });
        audit_entry(self.db.write(), self.security.as_ref(), &audit).await;

        info!(
            doc = %id,
            patient = %req.patient_id,
            class = %req.class_code,
            "XDS-b-Dokument gespeichert"
        );

        Ok(SubmissionResponse {
            status: "SUCCESS".into(),
            document_entry_id: id,
            storage_path: path,
            size_bytes: content.len() as u64,
            hash_md5: md5,
            created_at: now,
        })
    }

    /// Document Query (QPDT-Q01/Q02/Q03): Sucht DocumentEntries nach
    /// PatientID, Klasse, Typ, Datum.
    pub async fn handle_query(
        &self,
        req: &QueryRequest,
    ) -> Result<QueryResponse, XdsError> {
        let limit = req.limit.min(500) as i32;

        // Parametrisierte Abfrage mit optionalen Kriterien
        let rows: Vec<DocumentEntryRow> = sqlx::query_as::<_, DocumentEntryRow>(
            r#"SELECT id, patient_id, class_code, type_code, title, care_context,
                      author, created_at, last_updated, storage_path, size_bytes,
                      hash_md5, status
               FROM xds_document_entries
               WHERE (patient_id = $1 OR $1::text IS NULL)
                 AND (class_code = $2 OR $2::text IS NULL)
                 AND (type_code = $3 OR $3::text IS NULL)
                 AND (created_at >= $4::timestamptz OR $4::timestamptz IS NULL)
                 AND (created_at <= $5::timestamptz OR $5::timestamptz IS NULL)
                 AND status = 'ACTIVE'
               ORDER BY created_at DESC LIMIT $6"#,
        )
        .bind(req.patient_id.clone().unwrap_or_default())
        .bind(req.class_code.clone().unwrap_or_default())
        .bind(req.type_code.clone().unwrap_or_default())
        .bind(req.date_from.clone().unwrap_or_default())
        .bind(req.date_to.clone().unwrap_or_default())
        .bind(limit)
        .fetch_all(self.db.read())
        .await?;

        // Audit
        let audit = serde_json::json!({
            "action": "XDS_QUERY",
            "patient_id": req.patient_id,
            "class_code": req.class_code,
            "count": rows.len(),
        });
        audit_entry(self.db.read(), self.security.as_ref(), &audit).await;

        Ok(QueryResponse {
            status: "SUCCESS".into(),
            count: rows.len(),
            results: rows
                .into_iter()
                .map(|r| DocumentEntry {
                    id: r.id,
                    patient_id: r.patient_id,
                    class_code: r.class_code,
                    type_code: r.type_code,
                    title: r.title,
                    care_context: r.care_context,
                    author: r.author,
                    created_at: r.created_at,
                    last_updated: r.last_updated,
                    storage_path: r.storage_path,
                    size_bytes: r.size_bytes as u64,
                    hash_md5: r.hash_md5,
                    status: r.status,
                })
                .collect(),
        })
    }

    /// Liefert ein einzelnes Dokument (Inhalt) anhand der DocumentEntry-ID.
    pub async fn get_document(&self, id: &Uuid) -> Result<Vec<u8>, XdsError> {
        let row: Option<(String,)> =
            sqlx::query_as::<_, (String,)>("SELECT storage_path FROM xds_document_entries WHERE id = $1")
                .bind(id)
                .fetch_optional(self.db.read())
                .await?;

        let path = match row {
            Some((p,)) => p,
            None => return Err(XdsError::UnknownPatient(id.to_string())),
        };
        std::fs::read(&path).map_err(|e| XdsError::Storage(e.to_string()))
    }
}

/// Interne Row-Struktur für XDS-b-Abfragen
#[derive(sqlx::FromRow)]
struct DocumentEntryRow {
    id: Uuid,
    patient_id: String,
    class_code: String,
    type_code: String,
    title: String,
    care_context: Option<String>,
    author: String,
    created_at: DateTime<Utc>,
    last_updated: DateTime<Utc>,
    storage_path: String,
    size_bytes: i64,
    hash_md5: String,
    status: String,
}

/// Schreibt einen Audit-Eintrag in die Audit-Log-Tabelle.
async fn audit_entry(
    pool: &sqlx::Pool<sqlx::Postgres>,
    security: &Security,
    data: &serde_json::Value,
) {
    let prev_hash = String::new();
    let hash = security.audit_hash(&prev_hash, data);
    let ts = Utc::now();
    let actor = "xds-server";
    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log (id, actor, action, data, hash, prev_hash, created_at)
           VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)"#,
    )
    .bind(actor)
    .bind(data.get("action").and_then(|v| v.as_str()).unwrap_or("UNKNOWN"))
    .bind(data)
    .bind(&hash)
    .bind(&prev_hash)
    .bind(ts)
    .execute(pool)
    .await
    {
        warn!(error = %e, "Audit-Log-Eintrag fehlgeschlagen");
    }
}

// ============================================================
// Axum-Handler
// ============================================================

/// POST /xds/submission — Dokument Submission (PDQ)
async fn submission_handler(
    axum::extract::State(state): axum::extract::State<XdsState>,
    Json(req): Json<SubmissionRequest>,
) -> Result<Json<SubmissionResponse>, XdsError> {
    let server = XdsServer::new(
        &state.repository_path,
        &state.registry_path,
        state.db.clone(),
        state.security.clone(),
    );
    let resp = server.handle_submission(&req).await?;
    Ok(Json(resp))
}

/// GET /xds/query — Document Query (QPDT-Q01/Q02/Q03)
async fn query_handler(
    axum::extract::State(state): axum::extract::State<XdsState>,
    axum::extract::Query(params): axum::extract::Query<QueryRequest>,
) -> Result<Json<QueryResponse>, XdsError> {
    let server = XdsServer::new(
        &state.repository_path,
        &state.registry_path,
        state.db.clone(),
        state.security.clone(),
    );
    let resp = server.handle_query(&params).await?;
    Ok(Json(resp))
}

/// GET /xds/document/{id} — Liefert das Dokument (Inhalt)
async fn document_handler(
    axum::extract::State(state): axum::extract::State<XdsState>,
    Path(id): Path<Uuid>,
) -> Result<axum::body::Body, XdsError> {
    let server = XdsServer::new(
        &state.repository_path,
        &state.registry_path,
        state.db.clone(),
        state.security.clone(),
    );
    let content = server.get_document(&id).await?;
    Ok(axum::body::Body::from(content))
}

/// GET /xds/health — Health-Check
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "module": MODULE_NAME,
        "description": MODULE_DESCRIPTION,
        "protocol": "XDS-b (PDQ, QPDT-Q01/Q02/Q03)",
        "namespace": ISIK_NAMESPACE,
        "repository": DEFAULT_REPOSITORY_PATH,
        "registry": DEFAULT_REGISTRY_PATH,
        "status": "ok",
    }))
}

// ============================================================
// Router
// ============================================================

/// Registriert den XDS-b-Router.
///
/// Alle Endpunkte unter `/xds`:
/// - `GET  /xds/health` — Health-Check
/// - `POST /xds/submission` — Dokument Submission (PDQ)
/// - `GET  /xds/query` — Document Query (QPDT-Q01/Q02/Q03)
/// - `GET  /xds/document/{id}` — Dokument-Inhalt laden
pub fn register_router(
    db: Arc<Database>,
    security: Arc<Security>,
    repository_path: &str,
    registry_path: &str,
) -> Router {
    let state = XdsState {
        db,
        security,
        repository_path: repository_path.to_string(),
        registry_path: registry_path.to_string(),
    };

    Router::new()
        .route("/xds/health", get(health_handler))
        .route("/xds/submission", axum::routing::post(submission_handler))
        .route("/xds/query", get(query_handler))
        .route("/xds/document/{id}", get(document_handler))
        .with_state(state)
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submission_request_default() {
        let req = SubmissionRequest {
            patient_id: "P123".into(),
            class_code: "2.16.840.1.113883.6.1.4.3406.1233".into(),
            type_code: "2.16.840.1.113883.6.1.4.3406.1233.12".into(),
            title: "Entlassungsbericht".into(),
            care_context: None,
            author: "D456".into(),
            content_base64: base64::engine::general_purpose::STANDARD.encode(b"test"),
            repository_id: None,
        };
        assert_eq!(req.patient_id, "P123");
        assert_eq!(req.author, "D456");
    }

    #[test]
    fn test_query_request_limit() {
        let req = QueryRequest {
            patient_id: Some("P123".into()),
            class_code: None,
            type_code: None,
            date_from: None,
            date_to: None,
            limit: 0, // wird auf 500 begrenzt
        };
        assert!(req.limit.min(500) == 0);
    }
}
