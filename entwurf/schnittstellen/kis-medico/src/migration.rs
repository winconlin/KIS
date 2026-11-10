//! Task 4.14 — Migration-Orchestrierung
//!
//! `migration.rs`: Phasenplan als Code für die Migration von MEDICO → KIS.
//!
//! Phasen:
//! 1. **Initial-Import** — alle Patienten, Termine, Ärzte, Befunde aus MEDICO nach KIS
//! 2. **Parallelbetrieb** — KIS + MEDICO laufen parallel, Change-Detection
//! 3. **Ablösung** — MEDICO read-only, KIS primär
//! 4. **Rollback** — GDT-Import zurück in MEDICO
//!
//! Endpunkte:
//! - `GET  /api/v1/interfaces/medico/migration/status` — aktuelle Phase
//! - `POST /api/v1/interfaces/medico/migration/advance` — Phase fortschreiten
//! - `POST /api/v1/interfaces/medico/migration/rollback` — Rollback

use std::sync::Arc;

use kis_core::db::Database;
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::MedicoError;

// ============================================================
// Phasen-Definition
// ============================================================

/// Migration-Phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    /// Phase 1: Initial-Import
    InitialImport,
    /// Phase 2: Parallelbetrieb
    ParallelOperation,
    /// Phase 3: Ablösung (MEDICO read-only)
    Cutover,
    /// Phase 4: Rollback (falls nötig)
    Rollback,
    /// Abgeschlossen
    Completed,
}

impl MigrationPhase {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "initial_import" | "initial" => Some(Self::InitialImport),
            "parallel_operation" | "parallel" => Some(Self::ParallelOperation),
            "cutover" => Some(Self::Cutover),
            "rollback" => Some(Self::Rollback),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InitialImport => "initial_import",
            Self::ParallelOperation => "parallel_operation",
            Self::Cutover => "cutover",
            Self::Rollback => "rollback",
            Self::Completed => "completed",
        }
    }

    /// Nächste Phase (None wenn abgeschlossen).
    pub fn next(&self) -> Option<MigrationPhase> {
        match self {
            Self::InitialImport => Some(Self::ParallelOperation),
            Self::ParallelOperation => Some(Self::Cutover),
            Self::Cutover => Some(Self::Completed),
            Self::Rollback => Some(Self::Completed),
            Self::Completed => None,
        }
    }

    /// Vorgängige Phase (None bei InitialImport).
    pub fn prev(&self) -> Option<MigrationPhase> {
        match self {
            Self::InitialImport => None,
            Self::ParallelOperation => Some(Self::InitialImport),
            Self::Cutover => Some(Self::ParallelOperation),
            Self::Rollback => Some(Self::Cutover),
            Self::Completed => Some(Self::Cutover),
        }
    }
}

// ============================================================
// Migration-Status
// ============================================================

/// Status der Migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStatus {
    /// Aktuelle Phase
    pub current_phase: MigrationPhase,
    /// Zeitstempel des Phasen-Wechsels
    pub phase_started_at: chrono::DateTime<chrono::Utc>,
    /// Anzahl verarbeiteter Patienten (Phase 1)
    pub patients_migrated: i64,
    /// Anzahl verarbeiteter Termine (Phase 1)
    pub appointments_migrated: i64,
    /// Anzahl verarbeiteter Ärzte (Phase 1)
    pub practitioners_migrated: i64,
    /// Anzahl verarbeiteter Befunde (Phase 1)
    pub observations_migrated: i64,
    /// Anzahl offener Konflikte
    pub open_conflicts: i64,
    /// Letzter Sync-Lauf (Zeitstempel)
    pub last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Letzter Sync-Fehler
    pub last_sync_error: Option<String>,
    /// Rollback-Bereitschaft (GDT-Export existiert)
    pub gdt_export_available: bool,
    /// Anmerkungen
    pub notes: Vec<String>,
}

impl Default for MigrationStatus {
    fn default() -> Self {
        Self {
            current_phase: MigrationPhase::InitialImport,
            phase_started_at: chrono::Utc::now(),
            patients_migrated: 0,
            appointments_migrated: 0,
            practitioners_migrated: 0,
            observations_migrated: 0,
            open_conflicts: 0,
            last_sync_at: None,
            last_sync_error: None,
            gdt_export_available: false,
            notes: Vec::new(),
        }
    }
}

// ============================================================
// Migration-Orchestrator
// ============================================================

/// Migration-Orchestrator: verwaltet den Phasenplan.
#[derive(Clone)]
pub struct MigrationOrchestrator {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
}

impl MigrationOrchestrator {
    pub fn new(db: Arc<Database>, security: Arc<Security>) -> Arc<Self> {
        Arc::new(Self { db, security })
    }

    /// Liest den aktuellen Migration-Status aus der DB.
    pub async fn get_status(&self) -> Result<MigrationStatus, MedicoError> {
        let row = sqlx::query(
            r#"SELECT current_phase, phase_started_at,
                      patients_migrated, appointments_migrated,
                      practitioners_migrated, observations_migrated,
                      open_conflicts, last_sync_at, last_sync_error,
                      gdt_export_available, notes
               FROM medico_migration_status
               WHERE id = 1"#,
        )
        .fetch_optional(&self.db.read_pool)
        .await?;

        Ok(match row {
            Some(row) => MigrationStatus {
                current_phase: MigrationPhase::from_str(
                    &row.get::<String, _>("current_phase"),
                )
                .unwrap_or(MigrationPhase::InitialImport),
                phase_started_at: row.get("phase_started_at"),
                patients_migrated: row.get("patients_migrated"),
                appointments_migrated: row.get("appointments_migrated"),
                practitioners_migrated: row.get("practitioners_migrated"),
                observations_migrated: row.get("observations_migrated"),
                open_conflicts: row.get("open_conflicts"),
                last_sync_at: row.get("last_sync_at"),
                last_sync_error: row.get("last_sync_error"),
                gdt_export_available: row.get("gdt_export_available"),
                notes: row
                    .get::<Vec<String>, _>("notes")
                    .into_iter()
                    .collect(),
            },
            None => MigrationStatus::default(),
        })
    }

    /// Phasen-Wechsel: aktuelle Phase → nächste Phase.
    ///
    /// Vor dem Wechsel wird geprüft, ob die Voraussetzungen der nächsten
    /// Phase erfüllt sind (z. B. keine offenen Konflikte vor Cutover).
    pub async fn advance(
        &self,
        actor: &str,
    ) -> Result<MigrationStatus, MedicoError> {
        let mut status = self.get_status().await?;
        let current = status.current_phase;
        let next = current
            .next()
            .ok_or_else(|| MedicoError::Fhir("Migration bereits abgeschlossen".into()))?;

        // Guard: Vor Cutover müssen keine offenen Konflikte existieren
        if next == MigrationPhase::Cutover && status.open_conflicts > 0 {
            return Err(MedicoError::Fhir(format!(
                "Cannot advance to cutover: {} open conflicts",
                status.open_conflicts
            )));
        }

        let now = chrono::Utc::now();
        status.current_phase = next;
        status.phase_started_at = now;

        sqlx::query(
            r#"UPDATE medico_migration_status
               SET current_phase = $2,
                   phase_started_at = $3
               WHERE id = 1
               ON CONFLICT (id) DO UPDATE SET
                   current_phase = EXCLUDED.current_phase,
                   phase_started_at = EXCLUDED.phase_started_at"#,
        )
        .bind(1i32)
        .bind(next.as_str())
        .bind(now)
        .execute(&self.db.write_pool)
        .await?;

        // Audit-Log
        let audit = serde_json::json!({
            "action": "MEDICO_MIGRATION_ADVANCE",
            "from_phase": current.as_str(),
            "to_phase": next.as_str(),
            "actor": actor,
        });
        self.write_audit(&audit).await;

        info!(
            from = %current.as_str(),
            to = %next.as_str(),
            actor = %actor,
            "Migration-Phase vorangeschritten"
        );

        Ok(status)
    }

    /// Rollback: zurück in die vorherige Phase.
    ///
    /// Phase 4 (Rollback) ist ein Spezialzustand: hier wird der
    /// GDT-Export in MEDICO importiert.
    pub async fn rollback(
        &self,
        actor: &str,
    ) -> Result<MigrationStatus, MedicoError> {
        let mut status = self.get_status().await?;
        let current = status.current_phase;
        let prev = current
            .prev()
            .ok_or_else(|| MedicoError::Fhir("Cannot rollback from initial phase".into()))?;

        let now = chrono::Utc::now();
        status.current_phase = prev;
        status.phase_started_at = now;
        status.notes.push(format!("Rollback by {actor} at {now}"));

        sqlx::query(
            r#"UPDATE medico_migration_status
               SET current_phase = $2,
                   phase_started_at = $3,
                   notes = $4
               WHERE id = 1
               ON CONFLICT (id) DO UPDATE SET
                   current_phase = EXCLUDED.current_phase,
                   phase_started_at = EXCLUDED.phase_started_at,
                   notes = EXCLUDED.notes"#,
        )
        .bind(1i32)
        .bind(prev.as_str())
        .bind(now)
        .bind(&status.notes)
        .execute(&self.db.write_pool)
        .await?;

        // Audit-Log
        let audit = serde_json::json!({
            "action": "MEDICO_MIGRATION_ROLLBACK",
            "from_phase": current.as_str(),
            "to_phase": prev.as_str(),
            "actor": actor,
        });
        self.write_audit(&audit).await;

        warn!(
            from = %current.as_str(),
            to = %prev.as_str(),
            actor = %actor,
            "Migration-ROLLBACK"
        );

        Ok(status)
    }

    /// Aktualisiert die Migrationszähler nach einem Sync-Lauf.
    pub async fn update_counters(
        &self,
        patients: i64,
        appointments: i64,
        practitioners: i64,
        observations: i64,
    ) -> Result<(), MedicoError> {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"UPDATE medico_migration_status
               SET patients_migrated = $2,
                   appointments_migrated = $3,
                   practitioners_migrated = $4,
                   observations_migrated = $5,
                   last_sync_at = $6
               WHERE id = 1
               ON CONFLICT (id) DO UPDATE SET
                   patients_migrated = EXCLUDED.patients_migrated,
                   appointments_migrated = EXCLUDED.appointments_migrated,
                   practitioners_migrated = EXCLUDED.practitioners_migrated,
                   observations_migrated = EXCLUDED.observations_migrated,
                   last_sync_at = EXCLUDED.last_sync_at"#,
        )
        .bind(1i32)
        .bind(patients)
        .bind(appointments)
        .bind(practitioners)
        .bind(observations)
        .bind(now)
        .execute(&self.db.write_pool)
        .await?;
        Ok(())
    }

    /// Schreibt einen Audit-Log-Eintrag.
    async fn write_audit(&self, data: &serde_json::Value) {
        let prev_hash = sqlx::query_scalar(
            "SELECT hash FROM audit_logs ORDER BY timestamp DESC LIMIT 1",
        )
        .fetch_optional(&self.db.read_pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        let hash = self.security.audit_hash(&prev_hash, data);
        if let Err(e) = sqlx::query(
            r#"INSERT INTO audit_logs
               (user_id, action, entity_type, entity_id, old_data, new_data,
                ip_address, timestamp, prev_hash, hash)
               VALUES ($1, $2, $3, NULL, NULL, $4, NULL, $5, $6, $7)"#,
        )
        .bind("migration-orchestrator")
        .bind(
            data.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("MEDICO_MIGRATION"),
        )
        .bind("migration")
        .bind(data)
        .bind(chrono::Utc::now())
        .bind(&prev_hash)
        .bind(&hash)
        .execute(&self.db.write_pool)
        .await
        {
            warn!(error = %e, "Audit-Eintrag für Migration fehlgeschlagen");
        }
    }
}

// ============================================================
// Axum-Handler
// ============================================================

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};

/// GET /api/v1/interfaces/medico/migration/status
pub async fn migration_status_handler(
    State(orchestrator): State<Arc<MigrationOrchestrator>>,
) -> Result<Json<ApiResponse<MigrationStatus>>, (StatusCode, Json<ApiError>)> {
    let status = orchestrator
        .get_status()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))))?;

    Ok(Json(ApiResponse {
        data: status,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    }))
}

#[derive(Debug, Deserialize)]
pub struct MigrationActionBody {
    #[serde(default = "default_migration_actor")]
    pub actor: String,
}

fn default_migration_actor() -> String {
    "unknown".into()
}

/// POST /api/v1/interfaces/medico/migration/advance
pub async fn migration_advance_handler(
    State(orchestrator): State<Arc<MigrationOrchestrator>>,
    Json(body): Json<MigrationActionBody>,
) -> Result<Json<ApiResponse<MigrationStatus>>, (StatusCode, Json<ApiError>)> {
    let status = orchestrator
        .advance(&body.actor)
        .await
        .map_err(|e| match e {
            MedicoError::Fhir(msg) => (StatusCode::BAD_REQUEST, Json(ApiError::bad_request(&msg))),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))),
        })?;

    Ok(Json(ApiResponse {
        data: status,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    }))
}

/// POST /api/v1/interfaces/medico/migration/rollback
pub async fn migration_rollback_handler(
    State(orchestrator): State<Arc<MigrationOrchestrator>>,
    Json(body): Json<MigrationActionBody>,
) -> Result<Json<ApiResponse<MigrationStatus>>, (StatusCode, Json<ApiError>)> {
    let status = orchestrator
        .rollback(&body.actor)
        .await
        .map_err(|e| match e {
            MedicoError::Fhir(msg) => (StatusCode::BAD_REQUEST, Json(ApiError::bad_request(&msg))),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError::internal(&e.to_string()))),
        })?;

    Ok(Json(ApiResponse {
        data: status,
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
    fn phase_sequence() {
        assert_eq!(
            MigrationPhase::InitialImport.next(),
            Some(MigrationPhase::ParallelOperation)
        );
        assert_eq!(
            MigrationPhase::ParallelOperation.next(),
            Some(MigrationPhase::Cutover)
        );
        assert_eq!(MigrationPhase::Cutover.next(), Some(MigrationPhase::Completed));
        assert_eq!(MigrationPhase::Completed.next(), None);
    }

    #[test]
    fn phase_prev() {
        assert_eq!(MigrationPhase::InitialImport.prev(), None);
        assert_eq!(
            MigrationPhase::ParallelOperation.prev(),
            Some(MigrationPhase::InitialImport)
        );
        assert_eq!(
            MigrationPhase::Cutover.prev(),
            Some(MigrationPhase::ParallelOperation)
        );
        assert_eq!(
            MigrationPhase::Completed.prev(),
            Some(MigrationPhase::Cutover)
        );
    }

    #[test]
    fn phase_from_str() {
        assert_eq!(
            MigrationPhase::from_str("initial_import"),
            Some(MigrationPhase::InitialImport)
        );
        assert_eq!(
            MigrationPhase::from_str("parallel_operation"),
            Some(MigrationPhase::ParallelOperation)
        );
        assert_eq!(
            MigrationPhase::from_str("cutover"),
            Some(MigrationPhase::Cutover)
        );
        assert_eq!(
            MigrationPhase::from_str("rollback"),
            Some(MigrationPhase::Rollback)
        );
        assert_eq!(
            MigrationPhase::from_str("completed"),
            Some(MigrationPhase::Completed)
        );
        assert_eq!(MigrationPhase::from_str("invalid"), None);
    }

    #[test]
    fn phase_as_str_roundtrip() {
        for phase in [
            MigrationPhase::InitialImport,
            MigrationPhase::ParallelOperation,
            MigrationPhase::Cutover,
            MigrationPhase::Rollback,
            MigrationPhase::Completed,
        ] {
            assert_eq!(
                MigrationPhase::from_str(phase.as_str()),
                Some(phase),
                "Roundtrip failed for {:?}",
                phase
            );
        }
    }

    #[test]
    fn phase_serialize_snake_case() {
        let json = serde_json::to_string(&MigrationPhase::InitialImport).unwrap();
        assert_eq!(json, "\"initial_import\"");
        let json = serde_json::to_string(&MigrationPhase::ParallelOperation).unwrap();
        assert_eq!(json, "\"parallel_operation\"");
        let json = serde_json::to_string(&MigrationPhase::Cutover).unwrap();
        assert_eq!(json, "\"cutover\"");
    }

    #[test]
    fn migration_status_default() {
        let status = MigrationStatus::default();
        assert_eq!(status.current_phase, MigrationPhase::InitialImport);
        assert_eq!(status.patients_migrated, 0);
        assert!(status.last_sync_at.is_none());
        assert!(!status.gdt_export_available);
    }

    #[test]
    fn migration_action_body_default() {
        let body: MigrationActionBody = serde_json::from_str("{}").unwrap();
        assert_eq!(body.actor, "unknown");
    }

    #[test]
    fn phase_ordering() {
        assert!(MigrationPhase::InitialImport < MigrationPhase::ParallelOperation);
        assert!(MigrationPhase::ParallelOperation < MigrationPhase::Cutover);
        assert!(MigrationPhase::Cutover < MigrationPhase::Completed);
    }
}
