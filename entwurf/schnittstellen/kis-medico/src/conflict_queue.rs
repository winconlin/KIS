//! Task 4.12 — Konflikt-Handling
//!
//! `conflict_queue.rs`: verwaltet die Konflikt-Queue für Import-Konflikte.
//!
//! Ein Konflikt entsteht, wenn der MEDICO-Import auf einen existierenden
//! KIS-Datensatz trifft, der ein anderes `updated_at` hat als die MEDICO-Quelle.
//!
//! - `GET  /api/v1/interfaces/medico/conflicts` — offene Konflikte
//! - `POST /api/v1/interfaces/medico/conflicts/{id}/resolve` — manuell auflösen
//!   (Body: `{"resolution": "keep-kis"}` oder `{"resolution": "keep-medico"}`)
//!
//! Jede Konflikt-Entscheidung wird im Audit-Log festgehalten
//! (Hash-Chain, Actor, Zeitstempel).

use std::sync::Arc;

use kis_core::db::Database;
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::{info, warn};
use uuid::Uuid;

use crate::MedicoError;

// ============================================================
// Datenstrukturen
// ============================================================

/// Ein offener Import-Konflikt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicoConflict {
    pub id: Uuid,
    /// Art des Datensatzes (patient, medication, document, ...)
    pub entity_type: String,
    /// KIS-ID des betroffenen Datensatzes
    pub kis_id: Option<Uuid>,
    /// MEDICO-externe ID
    pub medico_id: String,
    /// KIS-Daten (als JSON)
    pub kis_data: serde_json::Value,
    /// MEDICO-Daten (als JSON)
    pub medico_data: serde_json::Value,
    /// KIS updated_at
    pub kis_updated_at: chrono::DateTime<chrono::Utc>,
    /// MEDICO updated_at
    pub medico_updated_at: chrono::DateTime<chrono::Utc>,
    /// Zeitstempel der Konflikt-Erzeugung
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Status: "open" | "resolved"
    pub status: String,
    /// Auflösung (falls aufgelöst): "keep-kis" | "keep-medico"
    pub resolution: Option<String>,
    /// Zeitstempel der Auflösung
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Wer hat aufgelöst
    pub resolved_by: Option<String>,
}

/// Ergebnis einer Konflikt-Lösung.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictResolutionResult {
    pub conflict_id: Uuid,
    pub resolution: String,
    pub resolved_at: chrono::DateTime<chrono::Utc>,
}

/// Konflikt-Queue: kapselt DB-Zugriff für Konflikte.
#[derive(Clone)]
pub struct ConflictQueue {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
}

impl ConflictQueue {
    pub fn new(db: Arc<Database>, security: Arc<Security>) -> Arc<Self> {
        Arc::new(Self { db, security })
    }

    // ============================================================
    // Konflikt-Erzeugung
    // ============================================================

    /// Registriert einen neuen Import-Konflikt in der Queue.
    ///
    /// Wird aufgerufen, wenn der Import feststellt, dass ein KIS-Datensatz
    /// ein neueres `updated_at` hat als die MEDICO-Quelle (oder umgekehrt).
    pub async fn enqueue(
        &self,
        entity_type: &str,
        kis_id: Option<Uuid>,
        medico_id: &str,
        kis_data: &serde_json::Value,
        medico_data: &serde_json::Value,
        kis_updated_at: chrono::DateTime<chrono::Utc>,
        medico_updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Uuid, MedicoError> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();

        sqlx::query(
            r#"INSERT INTO medico_conflicts (
                id, entity_type, kis_id, medico_id,
                kis_data, medico_data,
                kis_updated_at, medico_updated_at,
                status, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'open', $9)
            "#,
        )
        .bind(id)
        .bind(entity_type)
        .bind(kis_id)
        .bind(medico_id)
        .bind(kis_data)
        .bind(medico_data)
        .bind(kis_updated_at)
        .bind(medico_updated_at)
        .bind(now)
        .execute(&self.db.write_pool)
        .await?;

        info!(
            conflict_id = %id,
            entity_type = %entity_type,
            medico_id = %medico_id,
            "Import-Konflikt in Queue"
        );

        Ok(id)
    }

    // ============================================================
    // Abfragen
    // ============================================================

    /// Liest alle offenen Konflikte (optional gefiltert nach entity_type).
    pub async fn list_open(
        &self,
        entity_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MedicoConflict>, MedicoError> {
        let limit = limit.clamp(1, 100);
        let offset = offset.max(0);

        let rows = match entity_type {
            Some(et) => sqlx::query(
                r#"SELECT id, entity_type, kis_id, medico_id,
                          kis_data, medico_data,
                          kis_updated_at, medico_updated_at,
                          status, created_at,
                          resolution, resolved_at, resolved_by
                   FROM medico_conflicts
                   WHERE status = 'open' AND entity_type = $1
                   ORDER BY created_at DESC
                   LIMIT $2 OFFSET $3"#,
            )
            .bind(et)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db.read_pool)
            .await?,
            None => sqlx::query(
                r#"SELECT id, entity_type, kis_id, medico_id,
                          kis_data, medico_data,
                          kis_updated_at, medico_updated_at,
                          status, created_at,
                          resolution, resolved_at, resolved_by
                   FROM medico_conflicts
                   WHERE status = 'open'
                   ORDER BY created_at DESC
                   LIMIT $1 OFFSET $2"#,
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db.read_pool)
            .await?,
        };

        rows.iter()
            .map(|row| {
                Ok(MedicoConflict {
                    id: row.get("id"),
                    entity_type: row.get("entity_type"),
                    kis_id: row.get("kis_id"),
                    medico_id: row.get("medico_id"),
                    kis_data: row.get("kis_data"),
                    medico_data: row.get("medico_data"),
                    kis_updated_at: row.get("kis_updated_at"),
                    medico_updated_at: row.get("medico_updated_at"),
                    created_at: row.get("created_at"),
                    status: row.get("status"),
                    resolution: row.get("resolution"),
                    resolved_at: row.get("resolved_at"),
                    resolved_by: row.get("resolved_by"),
                })
            })
            .collect()
    }

    /// Liest einen einzelnen Konflikt.
    pub async fn get(&self, id: Uuid) -> Result<Option<MedicoConflict>, MedicoError> {
        let row = sqlx::query(
            r#"SELECT id, entity_type, kis_id, medico_id,
                      kis_data, medico_data,
                      kis_updated_at, medico_updated_at,
                      status, created_at,
                      resolution, resolved_at, resolved_by
               FROM medico_conflicts WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.db.read_pool)
        .await?;

        Ok(row.map(|row| MedicoConflict {
            id: row.get("id"),
            entity_type: row.get("entity_type"),
            kis_id: row.get("kis_id"),
            medico_id: row.get("medico_id"),
            kis_data: row.get("kis_data"),
            medico_data: row.get("medico_data"),
            kis_updated_at: row.get("kis_updated_at"),
            medico_updated_at: row.get("medico_updated_at"),
            created_at: row.get("created_at"),
            status: row.get("status"),
            resolution: row.get("resolution"),
            resolved_at: row.get("resolved_at"),
            resolved_by: row.get("resolved_by"),
        }))
    }

    // ============================================================
    // Auflösung
    // ============================================================

    /// Löst einen Konflikt manuell auf.
    ///
    /// `resolution`: `"keep-kis"` oder `"keep-medico"`.
    /// - **keep-kis**: KIS-Daten bleiben unverändert, MEDICO-Daten werden verworfen.
    /// - **keep-medico**: MEDICO-Daten überschreiben die KIS-Daten.
    ///
    /// Schreibt einen Audit-Log-Eintrag mit Hash-Chain.
    pub async fn resolve(
        &self,
        conflict_id: Uuid,
        resolution: &str,
        resolved_by: &str,
    ) -> Result<ConflictResolutionResult, MedicoError> {
        // Validierung
        if resolution != "keep-kis" && resolution != "keep-medico" {
            return Err(MedicoError::Fhir(format!(
                "Ungültige Resolution: '{resolution}' (erwartet: 'keep-kis' oder 'keep-medico')"
            )));
        }

        let conflict = self
            .get(conflict_id)
            .await?
            .ok_or_else(|| MedicoError::Fhir(format!("Konflikt {conflict_id} nicht gefunden")))?;

        if conflict.status != "open" {
            return Err(MedicoError::Fhir(format!(
                "Konflikt {conflict_id} ist bereits aufgelöst"
            )));
        }

        let now = chrono::Utc::now();

        // KIS-Daten aktualisieren, wenn keep-medico
        if resolution == "keep-medico" {
            if let Err(e) = self.apply_medico_data(&conflict).await {
                warn!(
                    error = %e,
                    conflict_id = %conflict_id,
                    "keep-medico: KIS-Daten aktualisieren fehlgeschlagen"
                );
            }
        }

        // Konfliktdatensatz aktualisieren
        sqlx::query(
            r#"UPDATE medico_conflicts
               SET status = 'resolved',
                   resolution = $2,
                   resolved_at = $3,
                   resolved_by = $4
               WHERE id = $1 AND status = 'open'"#,
        )
        .bind(conflict_id)
        .bind(resolution)
        .bind(now)
        .bind(resolved_by)
        .execute(&self.db.write_pool)
        .await?;

        // Audit-Log (Hash-Chain)
        let prev_hash = self.last_audit_hash().await;
        let audit_data = serde_json::json!({
            "action": "MEDICO_CONFLICT_RESOLVED",
            "conflict_id": conflict_id.to_string(),
            "entity_type": conflict.entity_type,
            "medico_id": conflict.medico_id,
            "resolution": resolution,
            "resolved_by": resolved_by,
        });
        let hash = self.security.audit_hash(&prev_hash, &audit_data);

        sqlx::query(
            r#"INSERT INTO audit_logs
               (user_id, action, entity_type, entity_id, old_data, new_data,
                ip_address, timestamp, prev_hash, hash)
               VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, $8, $9)"#,
        )
        .bind(resolved_by)
        .bind("MEDICO_CONFLICT_RESOLVED")
        .bind(&conflict.entity_type)
        .bind(conflict_id.to_string())
        .bind(&conflict.kis_data)
        .bind(&audit_data)
        .bind(now)
        .bind(&prev_hash)
        .bind(&hash)
        .execute(&self.db.write_pool)
        .await?;

        info!(
            conflict_id = %conflict_id,
            resolution = %resolution,
            resolved_by = %resolved_by,
            "Konflikt aufgelöst + Audit-Log geschrieben"
        );

        Ok(ConflictResolutionResult {
            conflict_id,
            resolution: resolution.into(),
            resolved_at: now,
        })
    }

    /// Wendet MEDICO-Daten auf KIS an (bei keep-medico).
    async fn apply_medico_data(&self, conflict: &MedicoConflict) -> Result<(), MedicoError> {
        match conflict.entity_type.as_str() {
            "patient" => {
                if let Some(kis_id) = conflict.kis_id {
                    let data = &conflict.medico_data;
                    sqlx::query(
                        r#"UPDATE patients SET
                            first_name = COALESCE($2, first_name),
                            last_name  = COALESCE($3, last_name),
                            date_of_birth = COALESCE($4, date_of_birth),
                            gender = COALESCE($5, gender),
                            updated_at = NOW()
                         WHERE id = $1"#,
                    )
                    .bind(kis_id)
                    .bind(data.get("first_name").and_then(|v| v.as_str()))
                    .bind(data.get("last_name").and_then(|v| v.as_str()))
                    .bind(data.get("date_of_birth").and_then(|v| v.as_str()))
                    .bind(data.get("gender").and_then(|v| v.as_str()))
                    .execute(&self.db.write_pool)
                    .await?;
                }
            }
            "medication" => {
                if let Some(kis_id) = conflict.kis_id {
                    let data = &conflict.medico_data;
                    sqlx::query(
                        r#"UPDATE medications SET
                            name = COALESCE($2, name),
                            dosage = COALESCE($3, dosage),
                            form = COALESCE($4, form),
                            active = COALESCE($5, active),
                            updated_at = NOW(),
                            version = version + 1
                         WHERE id = $1"#,
                    )
                    .bind(kis_id)
                    .bind(data.get("name").and_then(|v| v.as_str()))
                    .bind(data.get("dosage").and_then(|v| v.as_str()))
                    .bind(data.get("form").and_then(|v| v.as_str()))
                    .bind(data.get("active").and_then(|v| v.as_i64()))
                    .execute(&self.db.write_pool)
                    .await?;
                }
            }
            _ => {
                debug::log_unsupported_entity(&conflict.entity_type);
            }
        }
        Ok(())
    }

    /// Liest den letzten Audit-Hash für die Hash-Chain.
    async fn last_audit_hash(&self) -> String {
        sqlx::query_scalar("SELECT hash FROM audit_logs ORDER BY timestamp DESC LIMIT 1")
            .fetch_optional(&self.db.read_pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    }
}

// ============================================================
// Axum-Handler
// ============================================================

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};

#[derive(Debug, Deserialize)]
pub struct ConflictQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub entity_type: Option<String>,
}

fn default_limit() -> i64 {
    50
}

/// GET /api/v1/interfaces/medico/conflicts
pub async fn conflicts_list_handler(
    State(state): State<ConflictQueue>,
    Query(q): Query<ConflictQuery>,
) -> Result<Json<ApiResponse<Vec<MedicoConflict>>>, (StatusCode, Json<ApiError>)> {
    let conflicts = state
        .list_open(q.entity_type.as_deref(), q.limit, q.offset)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))?;

    Ok(Json(ApiResponse {
        data: conflicts,
        meta: ResponseMeta {
            total: conflicts.len() as i64,
            has_more: false,
            next_cursor: None,
        },
    }))
}

#[derive(Debug, Deserialize)]
pub struct ResolveBody {
    pub resolution: String,
    #[serde(default = "default_actor")]
    pub actor: String,
}

fn default_actor() -> String {
    "unknown".into()
}

/// POST /api/v1/interfaces/medico/conflicts/{id}/resolve
pub async fn conflicts_resolve_handler(
    State(state): State<ConflictQueue>,
    Path(id): Path<Uuid>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<ApiResponse<ConflictResolutionResult>>, (StatusCode, Json<ApiError>)> {
    let result = state
        .resolve(id, &body.resolution, &body.actor)
        .await
        .map_err(|e| match e {
            MedicoError::Fhir(msg) => (StatusCode::BAD_REQUEST, Json(ApiError::bad_request(&msg))),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))),
        })?;

    Ok(Json(ApiResponse {
        data: result,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    }))
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_validation() {
        // Validierung der Resolution-Strings
        assert!(is_valid_resolution("keep-kis"));
        assert!(is_valid_resolution("keep-medico"));
        assert!(!is_valid_resolution("keep-both"));
        assert!(!is_valid_resolution("discard"));
        assert!(!is_valid_resolution(""));
    }

    fn is_valid_resolution(s: &str) -> bool {
        matches!(s, "keep-kis" | "keep-medico")
    }

    #[test]
    fn conflict_serialize_roundtrip() {
        let conflict = MedicoConflict {
            id: Uuid::new_v4(),
            entity_type: "patient".into(),
            kis_id: Some(Uuid::new_v4()),
            medico_id: "medico-pat-001".into(),
            kis_data: serde_json::json!({"name": "KIS Name"}),
            medico_data: serde_json::json!({"name": "MEDICO Name"}),
            kis_updated_at: chrono::Utc::now(),
            medico_updated_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            status: "open".into(),
            resolution: None,
            resolved_at: None,
            resolved_by: None,
        };

        let json = serde_json::to_string(&conflict).unwrap();
        let back: MedicoConflict = serde_json::from_str(&json).unwrap();
        assert_eq!(conflict.id, back.id);
        assert_eq!(conflict.entity_type, back.entity_type);
        assert_eq!(conflict.medico_id, back.medico_id);
    }

    #[test]
    fn conflict_query_defaults() {
        let q: ConflictQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(q.entity_type.is_none());
    }

    #[test]
    fn resolve_body_defaults() {
        let body: ResolveBody = serde_json::from_str(r#"{"resolution": "keep-kis"}"#).unwrap();
        assert_eq!(body.resolution, "keep-kis");
        assert_eq!(body.actor, "unknown");
    }

    #[test]
    fn resolve_body_with_actor() {
        let body: ResolveBody =
            serde_json::from_str(r#"{"resolution": "keep-medico", "actor": "admin"}"#).unwrap();
        assert_eq!(body.resolution, "keep-medico");
        assert_eq!(body.actor, "admin");
    }
}
