//! Task 4.8 — MEDICO-Medikament-Import
//!
//! `MedicoSync::import_medication_requests()`: importiert FHIR MedicationRequest
//! Ressourcen aus MEDICO in die KIS `medications`-Tabelle.
//!
//! Mapping:
//! - `MedicationRequest.medicationCodeableConcept.text/coding[0].display` → `name`
//! - `MedicationRequest.subject.reference`                                 → Patient (via `external_id`)
//! - `MedierungRequest.status`                                             → `active` (flag)
//! - `MedicationRequest.authoredOn`                                        → `start_date`
//! - `MedicationRequest.dosageInstruction[0].text`                         → `dosage`
//! - `MedicationRequest.medicationCodeableConcept.coding[0].code`          → `form`
//!
//! Endpoint: `GET /api/v1/interfaces/medico/import/medications`

use std::sync::Arc;

use kis_core::db::Database;
use kis_core::security::Security;
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::client::{FhirBundle, FhirClient};
use crate::medico_sync::MedicoSync;
use crate::MedicoError;

/// Ergebnis eines Medikamenten-Import-Laufs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MedicationImportResult {
    pub total: i64,
    pub imported: i64,
    pub skipped: i64,
    pub failed: i64,
    pub errors: Vec<String>,
}

impl MedicoSync {
    /// Importiert alle FHIR MedicationRequest-Ressourcen (paginiert) in KIS.
    pub async fn import_medication_requests(
        &self,
    ) -> Result<MedicationImportResult, MedicoError> {
        let mut result = MedicationImportResult::default();
        let mut next: Option<String> = None;

        loop {
            let bundle: FhirBundle = self
                .client
                .get_medication_requests(100, next.as_deref(), None)
                .await?;

            for entry in &bundle.entry {
                result.total += 1;
                match self.upsert_medication_request(&entry.resource).await {
                    Ok(true) => result.imported += 1,
                    Ok(false) => result.skipped += 1,
                    Err(e) => {
                        result.failed += 1;
                        warn!(error = %e, "Medikamenten-Import fehlgeschlagen");
                        result.errors.push(format!("{e:?}"));
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
            "MEDICO-Medikamenten-Import abgeschlossen"
        );
        Ok(result)
    }

    /// Upsert einer einzelnen MedicationRequest.
    ///
    /// Returns `Ok(true)` wenn importiert/aktualisiert, `Ok(false)` wenn übersprungen.
    async fn upsert_medication_request(&self, resource: &Value) -> Result<bool, MedicoError> {
        let ext_id = resource
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| MedicoError::Fhir("MedicationRequest ohne id".into()))?;

        // --- Patient auflösen ---
        let patient_ext = resource
            .pointer("/subject/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Patient/"))
            .map(|s| s.to_string());

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
                debug!(ext_id = %ext_id, "Medikament übersprungen: Patient nicht in KIS");
                return Ok(false);
            }
        };

        // --- Metadaten extrahieren ---
        let name = resource
            .pointer("/medicationCodeableConcept/text")
            .or_else(|| resource.pointer("/medicationCodeableConcept/coding/0/display"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unbekanntes Medikament".into());

        let form_code = resource
            .pointer("/medicationCodeableConcept/coding/0/code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let dosage = resource
            .pointer("/dosageInstruction/0/text")
            .or_else(|| {
                resource
                    .pointer("/dosageInstruction/0/doseAndRate/0/doseQuantity/text")
            })
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let authored_on = resource
            .get("authoredOn")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());

        // Status → active-Flag
        let status = resource
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("active");
        let active: i32 = match status {
            "active" | "draft" | "on-hold" | "in-progress" | "completed" => 1,
            "cancelled" | "entered-in-error" | "unknown" => 0,
            _ => 1,
        };

        let last_updated = resource
            .pointer("/meta/lastUpdated")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // --- Last-Write-Wins-Check ---
        let existing: Option<(chrono::DateTime<chrono::Utc>,)> = sqlx::query_as(
            "SELECT updated_at FROM medications WHERE patient_id = $1 AND name = $2 AND start_date = $3::date LIMIT 1",
        )
        .bind(patient_kis_id)
        .bind(&name)
        .bind(&authored_on)
        .fetch_optional(&self.db.read_pool)
        .await?;

        let should_write = match (&last_updated, &existing) {
            (Some(lu), Some((updated_at,))) => {
                let fhir_time = chrono::DateTime::parse_from_rfc3339(lu)
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());
                fhir_time >= *updated_at
            }
            _ => true,
        };

        if !should_write {
            return Ok(false);
        }

        // --- Upsert ---
        let start_date: chrono::NaiveDate = authored_on
            .parse::<chrono::NaiveDate>()
            .unwrap_or_else(|_| chrono::Utc::now().date_naive());

        sqlx::query(
            r#"INSERT INTO medications (
                id, patient_id, name, dosage, form, frequency,
                start_date, active, created_at, updated_at, deleted_at, version
            ) VALUES (
                gen_random_uuid(), $1, $2, $3, $4, NULL,
                $5, $6, NOW(), NOW(), NULL, 1
            )
            ON CONFLICT (patient_id, name, start_date) DO UPDATE SET
                dosage   = EXCLUDED.dosage,
                form     = EXCLUDED.form,
                active   = EXCLUDED.active,
                updated_at = NOW(),
                version  = medications.version + 1
            "#,
        )
        .bind(patient_kis_id)
        .bind(&name)
        .bind(&dosage)
        .bind(&form_code)
        .bind(start_date)
        .bind(active)
        .execute(&self.db.write_pool)
        .await?;

        // --- Audit ---
        let audit = serde_json::json!({
            "action": "MEDICO_MEDICATION_IMPORT",
            "medication_request_id": ext_id,
            "patient_kis_id": patient_kis_id.to_string(),
            "name": name,
            "status": status,
        });
        let prev_hash = String::new();
        let hash = self.security.audit_hash(&prev_hash, &audit);
        if let Err(e) = sqlx::query(
            r#"INSERT INTO audit_logs
               (user_id, action, entity_type, entity_id, old_data, new_data,
                ip_address, timestamp, prev_hash, hash)
               VALUES ($1, $2, $3, NULL, NULL, $4, NULL, $5, $6, $7)"#,
        )
        .bind("medico-import")
        .bind("MEDICO_MEDICATION_IMPORT")
        .bind("medication")
        .bind(&audit)
        .bind(chrono::Utc::now())
        .bind(&prev_hash)
        .bind(&hash)
        .execute(&self.db.write_pool)
        .await
        {
            warn!(error = %e, "Audit-Eintrag für Medikamenten-Import fehlgeschlagen");
        }

        Ok(true)
    }
}

// ============================================================
// Unit-Tests (Mapping-Logik, ohne DB)
// ============================================================

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn sample_med_req() -> Value {
        json!({
            "resourceType": "MedicationRequest",
            "id": "medico-med-001",
            "meta": { "lastUpdated": "2026-11-03T09:00:00Z" },
            "status": "active",
            "medicationCodeableConcept": {
                "text": "Metformin 500mg",
                "coding": [{
                    "system": "http://www.whocdi.org",
                    "code": "11053926",
                    "display": "Metformin 500mg"
                }]
            },
            "subject": { "reference": "Patient/medico-pat-001" },
            "authoredOn": "2026-11-01T14:00:00Z",
            "dosageInstruction": [{
                "text": "1-0-1 Tab. zu den Mahlzeiten"
            }]
        })
    }

    #[test]
    fn extract_medication_name() {
        let r = sample_med_req();
        let name = r
            .pointer("/medicationCodeableConcept/text")
            .or_else(|| r.pointer("/medicationCodeableConcept/coding/0/display"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unbekanntes Medikament".into());
        assert_eq!(name, "Metformin 500mg");
    }

    #[test]
    fn extract_dosage() {
        let r = sample_med_req();
        let dosage = r
            .pointer("/dosageInstruction/0/text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        assert_eq!(dosage, Some("1-0-1 Tab. zu den Mahlzeiten".to_string()));
    }

    #[test]
    fn extract_status_active_flag() {
        let r = sample_med_req();
        let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("active");
        let active: i32 = match status {
            "active" | "draft" | "on-hold" | "in-progress" | "completed" => 1,
            "cancelled" | "entered-in-error" | "unknown" => 0,
            _ => 1,
        };
        assert_eq!(active, 1);

        let r2 = json!({ "status": "cancelled" });
        let status2 = r2.get("status").and_then(|v| v.as_str()).unwrap_or("active");
        let active2: i32 = match status2 {
            "active" | "draft" | "on-hold" | "in-progress" | "completed" => 1,
            "cancelled" | "entered-in-error" | "unknown" => 0,
            _ => 1,
        };
        assert_eq!(active2, 0);
    }

    #[test]
    fn extract_patient_ref() {
        let r = sample_med_req();
        let patient_ext = r
            .pointer("/subject/reference")
            .and_then(|v| v.as_str())
            .and_then(|r| r.strip_prefix("Patient/"))
            .map(|s| s.to_string());
        assert_eq!(patient_ext, Some("medico-pat-001".to_string()));
    }

    #[test]
    fn extract_form_code() {
        let r = sample_med_req();
        let form = r
            .pointer("/medicationCodeableConcept/coding/0/code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        assert_eq!(form, Some("11053926".to_string()));
    }

    #[test]
    fn missing_name_falls_back() {
        let r = json!({ "id": "no-name" });
        let name = r
            .pointer("/medicationCodeableConcept/text")
            .or_else(|| r.pointer("/medicationCodeableConcept/coding/0/display"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unbekanntes Medikament".into());
        assert_eq!(name, "Unbekanntes Medikament");
    }
}
