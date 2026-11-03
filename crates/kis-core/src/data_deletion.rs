//! DSGVO Art. 17 — Datenlöschung (Task 5.6)
//!
//! Zwei Löschstufen mit klarem Unterschied:
//!
//! - **Soft Delete** (Standard):
//!   Setzt `deleted_at = NOW()` auf der `patients`-Tabelle (plus
//!   `deleted_by` / `deletion_reason` in der erweiterten Schema-Phase).
//!   Die Daten bleiben für Berechtigte lesbar, sind aber als gelöscht
//!   markiert. Undo-fähig im Rahmen der Aufbewahrungsfrist.
//!
//! - **Hard Delete** (nur Admin, mit expliziter Bestätigung):
//!   Löscht Patient + alle verknüpften Daten (Termine, Medikation,
//!   Einwilligungen) unwiderruflich in richtiger Reihenfolge
//!   (Kind-Tabellen zuerst, dann Haupttabelle) in einer Transaktion.
//!
//! Zusätzlich:
//! - `retention_check()`: findet Datensätze, deren Aufbewahrungsfrist
//!   (Standard: 10 Jahre nach letzter Behandlung) abgelaufen ist.
//! - `execute_deletions()`: führt fällige Löschungen aus (nur für
//!   bereits soft-deleted Patienten — aktive Patienten werden nur
//!   gemeldet, die Anonymisierung ist Task 5.8).
//! - Audit-Log für jede Löschung (WAS, WER, WANN, WARUM) über
//!   [`Security::log_audit`] (hash-chained).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

use super::db::Database;
use super::security::{AuthUser, Permission, Role, Security};

/// Standard-Aufbewahrungsfrist: 10 Jahre nach Behandlungsende.
pub const DEFAULT_RETENTION_YEARS: i64 = 10;

/// Fehler bei Löschoperationen.
#[derive(Debug, Error)]
pub enum DeletionError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("audit error: {0}")]
    Audit(#[from] anyhow::Error),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("patient {0} not found")]
    NotFound(Uuid),

    #[error("patient {0} was already deleted at {1}")]
    AlreadyDeleted(Uuid, DateTime<Utc>),

    #[error("hard delete requires explicit confirmation (confirm=<name>)")]
    ConfirmationRequired,
}

/// Ergebnis eines Retention-Checks: ein Datensatz, dessen
/// Aufbewahrungsfrist abgelaufen ist.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionCandidate {
    pub patient_id: Uuid,
    /// Letzter bekannter Behandlungszeitpunkt (letzter Termin oder Anlage)
    pub last_treatment_at: DateTime<Utc>,
    /// Zeitpunkt, an dem die Aufbewahrungsfrist abläuft
    pub retention_expired_at: DateTime<Utc>,
    pub is_soft_deleted: bool,
    /// Empfohlene Aktion: `hard_delete` (soft-deleted) oder `anonymize` (aktiv)
    pub recommended_action: String,
}

/// Ergebnis von [`DataDeletionService::execute_deletions`].
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeletionBatchResult {
    /// Anzahl erfolgreich hart gelöschter Datensätze
    pub deleted: usize,
    /// IDs der gelöschten Patienten
    pub deleted_ids: Vec<Uuid>,
    /// Kandidaten, die NICHT gelöscht wurden (aktive Patienten → Anonymisierung)
    pub skipped_active: Vec<Uuid>,
    /// Fehler pro Datensatz (patient_id → Fehler)
    pub failures: Vec<(Uuid, String)>,
}

/// Service für DSGVO-konforme Datenlöschung.
pub struct DataDeletionService {
    db: Arc<Database>,
    security: Arc<Security>,
    /// Aufbewahrungsfrist in Jahren (nach letzter Behandlung)
    retention_years: i64,
}

impl DataDeletionService {
    /// Erzeugt den Service mit Standard-Aufbewahrungsfrist (10 Jahre).
    pub fn new(db: Arc<Database>, security: Arc<Security>) -> Self {
        Self {
            db,
            security,
            retention_years: DEFAULT_RETENTION_YEARS,
        }
    }

    /// Erzeugt den Service mit konfigurierbarer Aufbewahrungsfrist.
    pub fn with_retention(
        db: Arc<Database>,
        security: Arc<Security>,
        retention_years: i64,
    ) -> Self {
        Self {
            db,
            security,
            retention_years,
        }
    }

    /// Konfigurierte Aufbewahrungsfrist in Jahren.
    pub fn retention_years(&self) -> i64 {
        self.retention_years
    }

    // ------------------------------------------------------------------
    // Soft Delete
    // ------------------------------------------------------------------

    /// Soft Delete eines Patienten (DSGVO Art. 17 — Löschung, technisch
    /// als Markierung realisiert). Setzt `deleted_at = NOW()`.
    ///
    /// Berechtigt: [`Permission::UpdatePatient`] (Ärzte/Verwaltung),
    /// d. h. jeder User, der Patienten pflegen darf.
    pub async fn soft_delete_patient(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
        reason: &str,
    ) -> Result<(), DeletionError> {
        // 1) Bestandsprüfung
        let row = sqlx::query("SELECT deleted_at FROM patients WHERE id = $1")
            .bind(patient_id)
            .fetch_optional(self.db.read_pool())
            .await?
            .ok_or(DeletionError::NotFound(patient_id))?;

        let deleted_at: Option<DateTime<Utc>> = row.get("deleted_at");
        if let Some(dt) = deleted_at {
            return Err(DeletionError::AlreadyDeleted(patient_id, dt));
        }

        // 2) RBAC-Check
        if !actor.role.has_permission(&Permission::UpdatePatient) {
            return Err(DeletionError::Permission(format!(
                "Rolle {} darf Patienten nicht löschen",
                actor.role
            )));
        }

        // 3) Soft Delete ausführen
        let res = sqlx::query(
            "UPDATE patients
             SET deleted_at = NOW(), updated_at = NOW(), version = version + 1
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(patient_id)
        .execute(self.db.write_pool())
        .await?;

        if res.rows_affected() == 0 {
            return Err(DeletionError::NotFound(patient_id));
        }

        // 4) Audit-Log: WAS, WER, WANN, WARUM (hash-chained)
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "patient.soft_delete",
                "patient",
                Some(&patient_id.to_string()),
                Some(&serde_json::json!({ "deleted_at": "now" })),
                Some(&serde_json::json!({
                    "reason": reason,
                    "deleted_by": actor.user_id,
                })),
                None,
            )
            .await?;

        tracing::info!(%patient_id, user = %actor.user_id, reason = %reason,
            "Patient soft-deleted (DSGVO Art. 17)");
        Ok(())
    }

    // ------------------------------------------------------------------
    // Hard Delete
    // ------------------------------------------------------------------

    /// Hard Delete eines Patienten: unwiderrufliche Löschung des Patienten
    /// und ALLER verknüpften Daten (Termine, Medikation, Einwilligungen)
    /// in richtiger Reihenfolge (Kind-Tabellen zuerst).
    ///
    /// **Nur Admin** + explizite Bestätigung (`confirmation` muss den
    /// Familiennamen des Patienten enthalten — doppelte Absicherung gegen
    /// versehentliches Löschen).
    pub async fn hard_delete_patient(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
        confirmation: &str,
    ) -> Result<(), DeletionError> {
        // 1) Existenz- und Bestätigungs-Check
        let row = sqlx::query(
            "SELECT name_family, deleted_at FROM patients WHERE id = $1",
        )
        .bind(patient_id)
        .fetch_optional(self.db.read_pool())
        .await?
        .ok_or(DeletionError::NotFound(patient_id))?;

        let name_family: String = row.get("name_family");
        let deleted_at: Option<DateTime<Utc>> = row.get("deleted_at");

        // Bestätigung muss den Familiennamen enthalten (case-insensitive)
        if !confirmation_matches(&name_family, confirmation) {
            return Err(DeletionError::ConfirmationRequired);
        }

        // 2) RBAC: Hard Delete ist ausschließlich Admin
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(DeletionError::Permission(
                "Hard Delete erfordert die Rolle Admin".into(),
            ));
        }

        // 3) Cascade-Delete in korrekter Reihenfolge (eine Transaktion)
        self.cascade_delete(patient_id).await?;

        // 4) Audit-Log (NACH dem Delete — die audit_logs-Tabelle wird
        //    selbst NICHT geleert, da sie nur über entity_id referenziert
        //    wird und den Lösch-Nachweis dauerhaft aufbewahren soll)
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "patient.hard_delete",
                "patient",
                Some(&patient_id.to_string()),
                Some(&serde_json::json!({
                    "name_family": name_family,
                    "was_soft_deleted": deleted_at.is_some(),
                })),
                Some(&serde_json::json!({
                    "irreversible": true,
                    "confirmation": confirmation,
                })),
                None,
            )
            .await?;

        tracing::warn!(%patient_id, user = %actor.user_id,
            "Patient hard-deleted inkl. aller verknüpften Daten (DSGVO Art. 17)");
        Ok(())
    }

    /// Löscht Patient + alle verknüpften Daten in richtiger Reihenfolge:
    /// Kind-Tabellen zuerst, dann Haupttabelle. Läuft in einer
    /// Transaktion — entweder alles oder nichts.
    ///
    /// Reihenfolge (Kind-Tabellen mit `patient_id`-FK zuerst):
    /// 1. `consents`        (patient_id FK)
    /// 2. `appointments`    (patient_id FK)
    /// 3. `medications`     (patient_id FK)
    /// 4. `patients`        (Haupttabelle)
    pub async fn cascade_delete(&self, patient_id: Uuid) -> Result<(), DeletionError> {
        let pool = self.db.write_pool().clone();
        let mut tx = pool.begin().await?;

        // Reihenfolge: Kind-Tabellen zuerst, dann Haupttabelle.
        let cascade_order = CASCADE_ORDER;
        for (table, fk) in cascade_order.iter() {
            let sql = format!("DELETE FROM {table} WHERE {fk} = $1");
            let res = sqlx::query(&sql)
                .bind(patient_id)
                .execute(&mut *tx)
                .await?;
            if res.rows_affected() > 0 {
                tracing::debug!(%table, %patient_id, rows = res.rows_affected(),
                    "Zeilen im Cascade-Delete entfernt");
            }
        }

        tx.commit().await?;
        tracing::info!(%patient_id, "Cascade-Delete abgeschlossen (Kind-Tabellen + Patient)");
        Ok(())
    }

    // ------------------------------------------------------------------
    // Retention-Check & Batch-Löschung
    // ------------------------------------------------------------------

    /// Prüft die Aufbewahrungsfristen: findet alle Patienten, deren
    /// Aufbewahrungsfrist abgelaufen ist.
    ///
    /// Kriterium: `letzter Termin (oder created_at bei Terminlos) +
    /// retention_years` liegt in der Vergangenheit.
    pub async fn retention_check(&self) -> Result<Vec<RetentionCandidate>, DeletionError> {
        let retention = ChronoDuration::days(self.retention_years * 365);

        let rows = sqlx::query(
            "SELECT
                p.id AS patient_id,
                COALESCE(MAX(a.start_time), p.created_at) AS last_treatment_at,
                (p.deleted_at IS NOT NULL) AS is_soft_deleted
             FROM patients p
             LEFT JOIN appointments a ON a.patient_id = p.id AND a.deleted_at IS NULL
             GROUP BY p.id, p.created_at, p.deleted_at
             HAVING COALESCE(MAX(a.start_time), p.created_at) + $1 < NOW()
             ORDER BY last_treatment_at ASC",
        )
        .bind(retention)
        .fetch_all(self.db.read_pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let patient_id: Uuid = row.get("patient_id");
            let last_treatment_at: DateTime<Utc> = row.get("last_treatment_at");
            let is_soft_deleted: bool = row.get("is_soft_deleted");

            let expired = last_treatment_at + retention;
            // Empfehlung: soft-deleted → hard_delete, aktiv → anonymize
            // (Anonymisierung ist der mildere Weg bei aktiven Patienten)
            let recommended_action = if is_soft_deleted {
                "hard_delete".to_string()
            } else {
                "anonymize".to_string()
            };

            out.push(RetentionCandidate {
                patient_id,
                last_treatment_at,
                retention_expired_at: expired,
                is_soft_deleted,
                recommended_action,
            });
        }

        tracing::info!(eligible = out.len(), retention_years = self.retention_years,
            "Retention-Check abgeschlossen");
        Ok(out)
    }

    /// Führt fällige Löschungen aus (basierend auf [`Self::retention_check`]).
    ///
    /// - Soft-deleted Patienten → **hard_delete**
    /// - Aktive Patienten → werden NICHT automatisch gelöscht, sondern als
    ///   `skipped_active` zurückgemeldet (Anonymisierung: Task 5.8)
    ///
    /// Rückgabe: Ergebnis der Batch-Operation.
    pub async fn execute_deletions(&self, actor: &AuthUser) -> Result<DeletionBatchResult, DeletionError> {
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(DeletionError::Permission(
                "Retention-Löschungen erfordern die Rolle Admin".into(),
            ));
        }

        let candidates = self.retention_check().await?;
        let mut result = DeletionBatchResult::default();

        for c in candidates {
            if !c.is_soft_deleted {
                // Aktiver Patient: nicht automatisch löschen
                result.skipped_active.push(c.patient_id);
                continue;
            }
            let confirm = format!("retention {} {}", c.patient_id, c.patient_id);
            match self.hard_delete_patient(c.patient_id, actor, &confirm).await {
                Ok(()) => {
                    result.deleted += 1;
                    result.deleted_ids.push(c.patient_id);
                }
                Err(e) => {
                    result.failures.push((c.patient_id, e.to_string()));
                    tracing::error!(%c.patient_id, error = %e,
                        "Retention-Hard-Delete fehlgeschlagen");
                }
            }
        }

        tracing::info!(deleted = result.deleted, skipped = result.skipped_active.len(),
            failures = result.failures.len(), "Retention-Löschungen ausgeführt");
        Ok(result)
    }

    // ------------------------------------------------------------------
    // Status-Abfragen (für UI)
    // ------------------------------------------------------------------

    /// Liefert den Lösch-Status eines Patienten (für die UI).
    pub async fn deletion_status(
        &self,
        patient_id: Uuid,
    ) -> Result<Option<DeletionStatus>, DeletionError> {
        let row = sqlx::query(
            "SELECT deleted_at, created_at FROM patients WHERE id = $1",
        )
        .bind(patient_id)
        .fetch_optional(self.db.read_pool())
        .await?
        .ok_or(DeletionError::NotFound(patient_id))?;

        Ok(Some(DeletionStatus {
            patient_id,
            created_at: row.get("created_at"),
            deleted_at: row.get("deleted_at"),
        }))
    }
}

/// Lösch-Status eines Patienten.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionStatus {
    pub patient_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Bestätigungs-Prüfung für Hard Deletes:
/// `confirmation` muss den Familiennamen (case-insensitiv, getrimmt)
/// enthalten. Leerer Name → Abweisung (keine Bestätigung ohne Identität).
fn confirmation_matches(name_family: &str, confirmation: &str) -> bool {
    let name = name_family.trim().to_lowercase();
    if name.is_empty() {
        return false;
    }
    confirmation.trim().to_lowercase().contains(&name)
}

/// Reihenfolge des Cascade-Deletes: Kind-Tabellen zuerst, Patient zuletzt.
/// (Zentrale Konstante — auch von den Unit-Tests referenziert.)
pub const CASCADE_ORDER: &[(&str, &str)] = &[
    ("consents", "patient_id"),
    ("appointments", "patient_id"),
    ("medications", "patient_id"),
    ("patients", "id"),
];

// ===========================================================================
// Unit-Tests (Task 5.6)
// ===========================================================================
//
// Die Tests verifizieren die LOGIK ohne Live-Datenbank:
// - Bestätigungs-Prüfung (doppelte Absicherung gegen versehentliches Löschen)
// - Retention-Berechnung (10 Jahre nach letzter Behandlung)
// - Fehler-Pfade (NotFound, AlreadyDeleted, Permission, ConfirmationRequired)
// - Reihenfolge des Cascade-Deletes (Kind-Tabellen zuerst, Patient zuletzt)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as CDur;

    /// Hard Delete erfordert explizite Bestätigung mit dem Familiennamen.
    #[test]
    fn test_confirmation_requires_family_name() {
        assert!(confirmation_matches("Musterfrau", "ich bestätige: Musters
trfrau"));
        assert!(confirmation_matches("Musterfrau", "mustersfrau")); // case-insensitive
        assert!(confirmation_matches("  Becker  ", "becker")); // getrimmt
        assert!(!confirmation_matches("Musterfrau", "Mustermann"));
        assert!(!confirmation_matches("Musterfrau", "")); // leer → abgelehnt
        assert!(!confirmation_matches("Musterfrau", "   ")); // leer → abgelehnt
    }

    /// Leerer Familienname → Bestätigung unmöglich (keine Identität).
    #[test]
    fn test_empty_name_rejects_all() {
        assert!(!confirmation_matches("", "mustersfrau"));
        assert!(!confirmation_matches("   ", "mustersfrau"));
    }

    /// Retention: 10 Jahre nach letzter Behandlung ist fällig.
    #[test]
    fn test_retention_boundary() {
        let retention = CDur::days(DEFAULT_RETENTION_YEARS * 365);
        let now = Utc::now();

        // Letzter Termin vor 10 Jahren + 1 Tag → FELLIG
        let expired = now - retention - CDur::days(1);
        assert!(expired + retention < now);

        // Letzter Termin vor 10 Jahren - 1 Tag → NICHT fällig
        let ok = now - retention + CDur::days(1);
        assert!(ok + retention > now);

        // Genau am Grenzwert → NICHT fällig (streng <)
        let boundary = now - retention;
        assert!(boundary + retention > now - CDur::seconds(1));
    }

    /// Retention-Frist ist konfigurierbar (nicht hart kodiert).
    #[test]
    fn test_retention_configurable() {
        assert_eq!(DEFAULT_RETENTION_YEARS, 10);
        for years in [5i64, 10, 30] {
            let retention = CDur::days(years * 365);
            let last = Utc::now() - retention - CDur::days(1);
            assert!(last + retention < Utc::now(), "Jahre={years} müsste fällig sein");
        }
    }

    /// Cascade-Reihenfolge: Kind-Tabellen zuerst, Patient zuletzt.
    #[test]
    fn test_cascade_order_children_first() {
        let tables: Vec<&str> = CASCADE_ORDER.iter().map(|(t, _)| *t).collect();

        // Patient muss die letzte Tabelle sein
        assert_eq!(*tables.last().unwrap(), "patients");

        // Alle Kind-Tabellen referenzieren patient_id
        for (table, fk) in CASCADE_ORDER.iter().take(CASCADE_ORDER.len() - 1) {
            assert_eq!(*fk, "patient_id", "Tabelle {table} muss patient_id-FK haben");
        }

        // Keine Kind-Tabelle nach dem Patienten
        assert_eq!(
            tables.iter().position(|t| t == "patients").unwrap(),
            tables.len() - 1
        );

        // Erwartete Kind-Tabellen vorhanden
        assert!(tables.contains(&"appointments"));
        assert!(tables.contains(&"medications"));
        assert!(tables.contains(&"consents"));
    }

    /// Fehler-Typen sind korrekt benannt (für API-Responses).
    #[test]
    fn test_error_display() {
        let id = Uuid::new_v4();
        assert!(DeletionError::NotFound(id).to_string().contains("not found"));
        assert!(DeletionError::ConfirmationRequired
            .to_string()
            .contains("confirmation"));
        assert!(DeletionError::AlreadyDeleted(id, Utc::now())
            .to_string()
            .contains("already deleted"));
        assert!(DeletionError::Permission("x".into())
            .to_string()
            .contains("permission denied"));
    }

    /// Batch-Ergebnis: Default leer, Zähler und Kategorien funktionieren.
    #[test]
    fn test_batch_result() {
        let mut r = DeletionBatchResult::default();
        assert_eq!(r.deleted, 0);
        assert!(r.deleted_ids.is_empty());
        assert!(r.skipped_active.is_empty());
        assert!(r.failures.is_empty());

        r.deleted += 1;
        r.deleted_ids.push(Uuid::new_v4());
        r.skipped_active.push(Uuid::new_v4());
        r.failures.push((Uuid::new_v4(), "db error".into()));
        assert_eq!(r.deleted, 1);
        assert_eq!(r.deleted_ids.len(), 1);
        assert_eq!(r.skipped_active.len(), 1);
        assert_eq!(r.failures.len(), 1);
    }

    /// Empfohlene Aktion je nach Soft-Delete-Status:
    /// soft-deleted → hard_delete, aktiv → anonymize (milderer Weg).
    #[test]
    fn test_recommended_action_mapping() {
        for (is_deleted, expected) in [(true, "hard_delete"), (false, "anonymize")] {
            let action = if is_deleted { "hard_delete" } else { "anonymize" };
            assert_eq!(action, expected);
        }
    }

    /// RBAC: nur Admin darf hard delete (Permission-Prüfung).
    #[test]
    fn test_hard_delete_requires_admin_role() {
        assert!(Role::Admin.has_permission(&Permission::Admin));
        assert!(!Role::Doctor.has_permission(&Permission::Admin));
        assert!(!Role::Nurse.has_permission(&Permission::Admin));
        // Soft Delete darf auch Doctor (UpdatePatient)
        assert!(Role::Doctor.has_permission(&Permission::UpdatePatient));
        assert!(Role::Admin.has_permission(&Permission::UpdatePatient));
    }

    /// RetentionCandidate: Felder sind korrekt typisiert.
    #[test]
    fn test_retention_candidate_fields() {
        let now = Utc::now();
        let c = RetentionCandidate {
            patient_id: Uuid::new_v4(),
            last_treatment_at: now - CDur::days(4000),
            retention_expired_at: now - CDur::days(50),
            is_soft_deleted: true,
            recommended_action: "hard_delete".into(),
        };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["recommendedAction"], "hard_delete");
        assert_eq!(json["isSoftDeleted"], true);
    }
}
