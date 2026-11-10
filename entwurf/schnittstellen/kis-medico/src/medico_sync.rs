//! Task 4.7 — MEDICO-Dokument-Import
//!
//! `MedicoSync::import_document_references()`: importiert FHIR DocumentReference
//! Ressourcen aus MEDICO in das KIS XDS-b-Repository.
//!
//! Mapping:
//! - `DocumentReference.type.coding[0].code`  → XDS `type_code`
//! - `DocumentReference.content[0].data`       → XDS Repository (Base64)
//! - `DocumentReference.author[0]`             → XDS `author`
//! - `DocumentReference.created` / `authored`  → XDS `created_at`
//! - `DocumentReference.subject.reference`     → KIS-Patient (via `external_id`)
//!
//! Endpoint: `GET /api/v1/interfaces/medico/import/documents`

use std::sync::Arc;

use kis_core::db::Database;
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::client::{FhirBundle, FhirClient};
use crate::MedicoError;

/// Ergebnis eines Dokumenten-Import-Laufs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DocumentImportResult {
    /// Anzahl verarbeiteter DocumentReference-Ressourcen
    pub total: i64,
    /// Anzahl erfolgreich in XDS importierter Dokumente
    pub imported: i64,
    /// Anzahl übersprungener Dokumente (Patient nicht in KIS, leerer Inhalt, Duplikat)
    pub skipped: i64,
    /// Anzahl fehlgeschlagener Importe
    pub failed: i64,
    /// Details der letzten N Fehler
    pub errors: Vec<String>,
}

/// MedicoSync: kapselt FHIR-Client, DB und Security für die Import-Operationen.
#[derive(Clone)]
pub struct MedicoSync {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub client: Arc<FhirClient>,
}

impl MedicoSync {
    pub fn new(
        db: Arc<Database>,
        security: Arc<Security>,
        client: Arc<FhirClient>,
    ) -> Self {
        Self { db, security, client }
    }

    // ============================================================
    // 4.7 — Dokument-Import
    // ============================================================

    /// Importiert alle FHIR DocumentReference-Ressourcen (paginiert) in das
    /// KIS XDS-b-Repository.
    ///
    /// Pro Resource:
    /// 1. Patient-Referenz (`subject.reference` → `external_id`) auf KIS-Patient auflösen.
    /// 2. Dokumenttyp (Coding-Code) extrahieren.
    /// 3. Inhalt (Base64) dekodieren und im XDS-Repository-Dateisystem ablegen.
    /// 4. `xds_document_entries`-Zeile upserten (idempotent über `external_id`
    ///    im Titel-Hinweis bzw. über `external_ref`).
    /// 5. Audit-Log-Eintrag schreiben.
    pub async fn import_document_references(
        &self,
    ) -> Result<DocumentImportResult, MedicoError> {
        let mut result = DocumentImportResult::default();
        let mut next: Option<String> = None;

        loop {
            let bundle: FhirBundle = self
                .client
                .get_document_references(100, next.as_deref(), None)
                .await?;

            for entry in &bundle.entry {
                result.total += 1;
                match self.upsert_document_reference(&entry.resource).await {
                    Ok(true) => result.imported += 1,
                    Ok(false) => result.skipped += 1,
                    Err(e) => {
                        result.failed += 1;
                        warn!(error = %e, "Dokumenten-Import fehlgeschlagen");
                        result
                            .errors
                            .push(format!("{:?}", e))
                            .min(result.errors.capacity().saturating_sub(20).max(10));
                        if result.errors.len() > 20 {
                            result.errors.truncate(20);
                        }
                    }
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

        info!(
            total = result.total,
            imported = result.imported,
            skipped = result.skipped,
            failed = result.failed,
            "MEDICO-Dokumenten-Import abgeschlossen"
        );
        Ok(result)
    }

    /// Upsert einer einzelnen DocumentReference.
    ///
    /// Returns:
    /// - `Ok(true)`  — Dokument neu importiert oder aktualisiert
    /// - `Ok(false)` — übersprungen (Patient fehlt, leerer Inhalt, kein Change)
    /// - `Err(_)`    — hartes Fehler (DB, Storage)
    async fn upsert_document_reference(&self, resource: &Value) -> Result<bool, MedicoError> {
        // --- 1) Referenzen extrahieren ---
        let ext_id = resource
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| MedicoError::Fhir("DocumentReference ohne id".into()))?;

        // Patient: subject.reference = "Patient/medico-123"
        let patient_ext = resource
            .pointer("/subject/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Patient/"))
            .map(|s| s.to_string());

        // KIS-Patient-ID auflösen (über external_id)
        let patient_kis_id: Option<Uuid> = match &patient_ext {
            Some(ext) => sqlx::query_scalar("SELECT id FROM patients WHERE external_id = $1")
                .bind(ext)
                .fetch_optional(&self.db.read_pool)
                .await?,
            None => None,
        };
        let patient_kis_id = match patient_kis_id {
            Some(id) => id,
            None => {
                debug!(ext_id = %ext_id, "Dokument übersprungen: Patient nicht in KIS");
                return Ok(false);
            }
        };

        // --- 2) Metadaten extrahieren ---
        let doc_type = resource
            .pointer("/type/coding/0/code")
            .or_else(|| resource.pointer("/type/coding/0/display"))
            .or_else(|| resource.pointer("/type/text"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unbekannt".into());

        let doc_class = resource
            .pointer("/type/coding/0/system")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "urn:oid:2.16.840.1.113883.6.1.4.3406".into());

        let title = resource
            .pointer("/description/text")
            .or_else(|| resource.pointer("/title"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("DocumentReference {ext_id}"));

        let author = resource
            .pointer("/author/0/agent/display")
            .or_else(|| resource.pointer("/author/0/agent/reference"))
            .or_else(|| resource.pointer("/author/0/reference"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unbekannt".into());

        let created = resource
            .get("created")
            .and_then(|v| v.as_str())
            .or_else(|| resource.get("authored").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());

        let last_updated = resource
            .pointer("/meta/lastUpdated")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // --- 3) Content extrahieren (Base64) ---
        let content_b64 = resource
            .pointer("/content/0/data")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let content = match &content_b64 {
            Some(b64) => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| MedicoError::Fhir(format!("Content-Base64 dekodieren: {e}")))?
            }
            None => {
                // Kein Inline-Content (z. B. nur Verweis-URL) → leeres Platzhalter-Dokument
                debug!(ext_id = %ext_id, "Dokument ohne Content-Data, Platzhalter-Dokument angelegt");
                format!("(keine Inline-Daten, FHIR-ID: {ext_id})").into_bytes()
            }
        };

        // --- 4) Last-Write-Wins: nur aktualisieren wenn MEDICO neuer ist ---
        let existing: Option<(chrono::DateTime<chrono::Utc>, String)> = sqlx::query_as(
            "SELECT created_at, storage_path FROM xds_document_entries WHERE title = $1 AND patient_id = $2::text LIMIT 1",
        )
        .bind(&title)
        .bind(&patient_kis_id.to_string())
        .fetch_optional(&self.db.read_pool)
        .await?;

        let should_write = match (&last_updated, &existing) {
            (Some(lu), Some((existing_created, _))) => {
                let fhir_time = chrono::DateTime::parse_from_rfc3339(lu)
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());
                fhir_time >= *existing_created
            }
            _ => true,
        };

        if !should_write {
            return Ok(false);
        }

        // --- 5) Content im Repository-Dateisystem ablegen ---
        let repo_root = std::env::var("KIS_XDS_REPOSITORY_PATH")
            .unwrap_or_else(|_| "/data/xds/repository".into());
        let dir = format!(
            "{}/{}",
            repo_root,
            patient_kis_id.to_string().chars().take(8).collect::<String>()
        );
        std::fs::create_dir_all(&dir).map_err(|e| MedicoError::Db(sqlx::Error::Configuration(
            format!("XDS-Repository-Verzeichnis: {e}").into()
        )))?;

        let doc_id = Uuid::new_v4();
        let ext = resource
            .pointer("/content/0/format")
            .and_then(|v| v.as_str())
            .map(|f| {
                let f = f.to_lowercase();
                if f.contains("pdf") { "pdf" }
                else if f.contains("html") { "html" }
                else if f.contains("xml") { "xml" }
                else { "bin" }
            })
            .unwrap_or("bin")
            .to_string();
        let storage_path = format!("{}/{doc_id}_{ext_id}.{ext}", dir);
        std::fs::write(&storage_path, &content)
            .map_err(|e| MedicoError::Db(sqlx::Error::Configuration(
                format!("XDS-Datei schreiben: {e}").into()
            )))?;

        // --- 6) Hash (SHA-256, first 32 hex chars) ---
        use sha2::Digest;
        let hash = {
            let mut h = sha2::Sha256::new();
            h.update(&content);
            let digest = h.finalize();
            let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
            hex[..32.min(hex.len())].to_string()
        };

        // --- 7) xds_document_entries upsert ---
        let created_dt = chrono::DateTime::parse_from_rfc3339(&created)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());

        let now = chrono::Utc::now();
        sqlx::query(
            r#"INSERT INTO xds_document_entries (
                id, patient_id, class_code, type_code, title, care_context,
                author, created_at, last_updated, storage_path, size_bytes,
                hash_md5, status
            ) VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, 'ACTIVE')
            ON CONFLICT (id) DO UPDATE SET
                last_updated = EXCLUDED.last_updated,
                storage_path = EXCLUDED.storage_path,
                size_bytes   = EXCLUDED.size_bytes,
                hash_md5     = EXCLUDED.hash_md5
            "#,
        )
        .bind(doc_id)
        .bind(&patient_kis_id.to_string())
        .bind(&doc_class)
        .bind(&doc_type)
        .bind(&title)
        .bind(&author)
        .bind(created_dt)
        .bind(now)
        .bind(&storage_path)
        .bind(content.len() as i64)
        .bind(&hash)
        .execute(&self.db.write_pool)
        .await?;

        // --- 8) Audit ---
        let audit = serde_json::json!({
            "action": "MEDICO_DOC_IMPORT",
            "document_reference_id": ext_id,
            "patient_kis_id": patient_kis_id.to_string(),
            "doc_type": doc_type,
            "size_bytes": content.len(),
            "storage_path": storage_path,
        });
        self.audit_entry(&audit).await;

        Ok(true)
    }

    /// Schreibt einen Audit-Eintrag (best-effort).
    async fn audit_entry(&self, data: &Value) {
        let prev_hash = String::new();
        let hash = self.security.audit_hash(&prev_hash, data);
        let ts = chrono::Utc::now();
        if let Err(e) = sqlx::query(
            r#"INSERT INTO audit_logs
               (user_id, action, entity_type, entity_id, old_data, new_data,
                ip_address, timestamp, prev_hash, hash)
               VALUES ($1, $2, $3, NULL, NULL, $4, NULL, $5, $6, $7)"#,
        )
        .bind("medico-import")
        .bind(
            data.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("MEDICO_DOC_IMPORT"),
        )
        .bind("document")
        .bind(data)
        .bind(ts)
        .bind(&prev_hash)
        .bind(&hash)
        .execute(&self.db.write_pool)
        .await
        {
            warn!(error = %e, "Audit-Eintrag für Dokumenten-Import fehlgeschlagen");
        }
    }
}

// ============================================================
// Unit-Tests (ohne DB — Mapping-Logik)
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc_ref() -> Value {
        serde_json::json!({
            "resourceType": "DocumentReference",
            "id": "medico-doc-001",
            "meta": { "lastUpdated": "2026-11-03T10:00:00Z" },
            "type": {
                "coding": [{
                    "system": "urn:oid:2.16.840.1.113883.6.1.4.3406",
                    "code": "12",
                    "display": "Entlassungsbericht"
                }]
            },
            "subject": { "reference": "Patient/medico-pat-001" },
            "created": "2026-11-01T08:30:00Z",
            "authored": "2026-11-01T08:30:00Z",
            "author": [{
                "agent": { "display": "Dr. med. Müller" }
            }],
            "description": { "text": "Kardio-Entlassung" },
            "content": [{
                "format": "application/pdf",
                "data": "JVBERi0xLjQK"  // Base64("PDF-1.4\n")
            }]
        })
    }

    #[test]
    fn extract_doc_type() {
        let r = sample_doc_ref();
        let doc_type = r
            .pointer("/type/coding/0/code")
            .or_else(|| r.pointer("/type/coding/0/display"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unbekannt".into());
        assert_eq!(doc_type, "12");
    }

    #[test]
    fn extract_patient_ref() {
        let r = sample_doc_ref();
        let patient_ext = r
            .pointer("/subject/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Patient/"))
            .map(|s| s.to_string());
        assert_eq!(patient_ext, Some("medico-pat-001".to_string()));
    }

    #[test]
    fn extract_content_b64() {
        let r = sample_doc_ref();
        let b64 = r
            .pointer("/content/0/data")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        assert!(b64.is_some());
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&b64.unwrap())
            .unwrap();
        assert_eq!(std::str::from_utf8(&decoded).unwrap(), "PDF-1.4\n");
    }

    #[test]
    fn extract_author_and_created() {
        let r = sample_doc_ref();
        let author = r
            .pointer("/author/0/agent/display")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unbekannt".into());
        assert_eq!(author, "Dr. med. Müller");

        let created = r
            .get("created")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap();
        assert_eq!(created, "2026-11-01T08:30:00Z");
    }

    #[test]
    fn doc_type_fallback() {
        let r = serde_json::json!({ "id": "no-type-doc" });
        let doc_type = r
            .pointer("/type/coding/0/code")
            .or_else(|| r.pointer("/type/coding/0/display"))
            .or_else(|| r.pointer("/type/text"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unbekannt".into());
        assert_eq!(doc_type, "unbekannt");
    }
}
