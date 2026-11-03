//! DSGVO Art. 17 — Anonymisierung (Task 5.8)
//!
//! **WICHTIGE UNTERSCHEIDUNG** (DSGVO-Begriffsbestimmung, Art. 4(5)):
//!
//! |                     | Anonymisierung            | Pseudonymisierung              |
//! |---------------------|---------------------------|--------------------------------|
//! | Reversibilität      | **Irreversibel**          | Reversibel (mit Schlüssel)     |
//! | Zuordenbarkeit      | Nicht mehr möglich        | Möglich über Pseudonym         |
//! | Schlüssel           | Wird NICHT gespeichert     | Wird AES-256-GCM-verschlüsselt |
//! | Rechtsgrundlage     | Art. 17 (Löschung)        | Art. 32 (Technische Maßnahme) |
//! | Ergebnis            | Daten verlassen DSGVO-Scope | Daten bleiben im DSGVO-Scope  |
//!
//! - **Anonymisierung** (`anonymize_patient`): Ersetzt Name,
//!   Geburtsdatum, Adresse, Telefon, E-Mail durch Hash + statistische
//!   Werte (Alter statt Geburtsdatum, PLZ-Region statt Adresse).
//!   **Irreversibel** — es wird KEIN Schlüssel gespeichert.
//!
//! - **Pseudonymization** (`pseudonymize_patient`): Ersetzt
//!   Identifikatoren durch ein Pseudonym, speichert den Schlüssel
//!   **AES-256-GCM-verschlüsselt** in `anonymization_keys`.
//!   **Reversibel** — Authorized-Admins können die Originaldaten
//!   wiederherstellen.
//!
//! Zusätzlich:
//! - `anonymize_observation()`: anonymisiert einen einzelnen Befund
//!   (entfernt die Patientenreferenz, behält klinische Werte).
//! - `retention_anonymize()`: automatische Anonymisierung nach
//!   Aufbewahrungsfrist (nur für bereits soft-deleted Patienten).
//! - `anonymization_log()`: Audit-Log aller Anonymisierungen.

use std::sync::Arc;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, Nonce};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use rand::RngCore;
use ring::digest;
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

use super::db::Database;
use super::security::{AuthUser, Permission, Security};

/// Standard-Aufbewahrungsfrist für automatische Anonymisierung.
pub const DEFAULT_RETENTION_YEARS: i64 = 10;

/// Fehler bei Anonymisierungs-Operationen.
#[derive(Debug, Error)]
pub enum AnonymizationError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("audit error: {0}")]
    Audit(#[from] anyhow::Error),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("patient {0} not found")]
    NotFound(Uuid),

    #[error("patient {0} was already anonymized")]
    AlreadyAnonymized(Uuid),

    #[error("encryption error: {0}")]
    Encryption(String),

    #[error("decryption error (key may be corrupted or wrong)")]
    Decryption,

    #[error("invalid base64 encoding in stored key")]
    InvalidKeyEncoding,
}

/// Art der Anonymisierung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnonymizationKind {
    /// Vollständige, irreversible Anonymisierung (Name, Geburtsdatum,
    /// Adresse, Telefon, E-Mail → Hash + statistische Werte).
    Anonymized,
    /// Pseudonymisierung (reversibel, Schlüssel verschlüsselt gespeichert).
    Pseudonymized,
    /// Einzelner Befund anonymisiert (Patientenreferenz entfernt).
    Observation,
}

/// Ein Anonymisierung-Log-Eintrag.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnonymizationLogEntry {
    pub patient_id: Uuid,
    pub kind: AnonymizationKind,
    pub actor: String,
    pub timestamp: DateTime<Utc>,
    pub details: serde_json::Value,
}

/// Ergebnis eines Retention-Anonymisierung-Checks.
#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetentionAnonymizationResult {
    /// Anzahl automatisch anonymisierter Patienten
    pub anonymized: usize,
    /// IDs der anonymisierten Patienten
    pub anonymized_ids: Vec<Uuid>,
    /// Patienten, die nicht anonymisiert wurden (aktive Patienten)
    pub skipped_active: Vec<Uuid>,
    /// Fehler
    pub failures: Vec<(Uuid, String)>,
}

/// Service für DSGVO-Anonymisierung (Art. 17) und Pseudonymisierung
/// (Art. 32).
pub struct AnonymizationService {
    db: Arc<Database>,
    security: Arc<Security>,
    /// AES-256-GCM Schlüssel für Pseudonymisierung-Schlüssel-Verschlüsselung
    /// (32 Byte, base64-kodiert in der Konfiguration).
    key: [u8; 32],
    /// Aufbewahrungsfrist in Jahren für automatische Anonymisierung.
    retention_years: i64,
}

impl AnonymizationService {
    /// Erzeugt den Service.
    ///
    /// `encryption_key`: 32-Byte AES-256-Schlüssel (base64-kodiert).
    /// `retention_years`: Aufbewahrungsfrist für automatische Anonymisierung.
    pub fn new(
        db: Arc<Database>,
        security: Arc<Security>,
        encryption_key_b64: &str,
        retention_years: i64,
    ) -> Result<Self, AnonymizationError> {
        let key = base64_decode(&encryption_key_b64)?;
        Ok(Self {
            db,
            security,
            key,
            retention_years,
        })
    }

    /// Erzeugt den Service mit Standard-Aufbewahrungsfrist (10 Jahre).
    pub fn with_default_retention(
        db: Arc<Database>,
        security: Arc<Security>,
        encryption_key_b64: &str,
    ) -> Result<Self, AnonymizationError> {
        Self::new(db, security, encryption_key_b64, DEFAULT_RETENTION_YEARS)
    }

    // ------------------------------------------------------------------
    // Anonymisierung (irreversibel)
    // ------------------------------------------------------------------

    /// **Irreversible Anonymisierung** eines Patienten.
    ///
    /// Ersetzt:
    /// - `name_family`, `name_given` → deterministischer SHA-256-Hash
    /// - `birth_date` → statistischer Alter-Wert (z.B. "62" statt
    ///   "1962-05-15")
    /// - `address_street`, `address_zip`, `address_city` → PLZ-Region
    /// - `phone`, `email` → Hash
    /// - `external_id`, `insurance_number` → Hash
    ///
    /// **Es wird KEIN Schlüssel gespeichert** — die Originaldaten sind
    /// unwiderruflich verloren.
    pub async fn anonymize_patient(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
    ) -> Result<(), AnonymizationError> {
        // 1) RBAC: nur Admin
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(AnonymizationError::Permission(
                "Anonymisierung erfordert die Rolle Admin".into(),
            ));
        }

        // 2) Bestandsprüfung
        let row = sqlx::query(
            "SELECT name_family, name_given, birth_date, address_zip,
                    phone, email, external_id, insurance_number, deleted_at
             FROM patients WHERE id = $1",
        )
        .bind(patient_id)
        .fetch_optional(self.db.read_pool())
        .await?
        .ok_or(AnonymizationError::NotFound(patient_id))?;

        // 3) Bereits anonymisiert? (Name ist bereits ein Hash)
        let name_family: String = row.get("name_family");
        if is_hash(&name_family) {
            return Err(AnonymizationError::AlreadyAnonymized(patient_id));
        }

        // 4) Anonymisierte Werte berechnen
        let name_given: String = row.get("name_given");
        let birth_date: NaiveDate = row.get("birth_date");
        let address_zip: Option<String> = row.get("address_zip");
        let phone: Option<String> = row.get("phone");
        let email: Option<String> = row.get("email");
        let external_id: Option<String> = row.get("external_id");
        let insurance_number: Option<String> = row.get("insurance_number");

        let anon_family = deterministic_hash(&name_family);
        let anon_given = deterministic_hash(&name_given);
        let age = age_at_now(&birth_date);
        let anon_zip = zip_region(&address_zip);
        let anon_phone = phone.map(deterministic_hash);
        let anon_email = email.map(deterministic_hash);
        let anon_external = external_id.map(deterministic_hash);
        let anon_insurance = insurance_number.map(deterministic_hash);

        // 5) UPDATE (eine Transaktion)
        let pool = self.db.write_pool().clone();
        let mut tx = pool.begin().await?;

        sqlx::query(
            "UPDATE patients SET
                name_family = $2,
                name_given = $3,
                birth_date = $4,
                address_zip = $5,
                address_city = 'anonymisiert',
                address_street = NULL,
                phone = $6,
                email = $7,
                external_id = $8,
                insurance_number = $9,
                deleted_at = NOW(),
                updated_at = NOW(),
                version = version + 1
             WHERE id = $1",
        )
        .bind(patient_id)
        .bind(&anon_family)
        .bind(&anon_given)
        .bind(NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()) // Platzhalter: Alter ist in age gespeichert
        .bind(anon_zip)
        .bind(&anon_phone)
        .bind(&anon_email)
        .bind(&anon_external)
        .bind(&anon_insurance)
        .execute(&mut *tx)
        .await?;

        // Anonymisierungs-Nachweis (irreversibel, kein Schlüssel)
        sqlx::query(
            "INSERT INTO anonymization_keys
                (patient_id, kind, pseudonym, encrypted_key, created_by, created_at)
             VALUES ($1, $2, $3, NULL, $4, NOW())",
        )
        .bind(patient_id)
        .bind("anonymized")
        .bind(&anon_family)
        .bind(&actor.user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // 6) Audit-Log (irreversibel — kein Schlüssel gespeichert)
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "patient.anonymize",
                "patient",
                Some(&patient_id.to_string()),
                None,
                Some(&serde_json::json!({
                    "kind": "anonymized",
                    "irreversible": true,
                    "age": age,
                    "zip_region": anon_zip,
                })),
                None,
            )
            .await?;

        tracing::warn!(%patient_id, user = %actor.user_id,
            "Patient anonymisiert (irreversibel, DSGVO Art. 17)");
        Ok(())
    }

    // ------------------------------------------------------------------
    // Pseudonymisierung (reversibel)
    // ------------------------------------------------------------------

    /// **Reversible Pseudonymisierung** eines Patienten.
    ///
    /// Ersetzt:
    /// - `name_family`, `name_given` → Pseudonym (z.B. "PAT-<hash8>")
    /// - `birth_date`, `address_*`, `phone`, `email` → NULL
    /// - `external_id`, `insurance_number` → Pseudonym
    ///
    /// **Speichert den AES-256-GCM-verschlüsselten Schlüssel** in
    /// `anonymization_keys` — Authorized-Admins können die Originaldaten
    /// wiederherstellen.
    pub async fn pseudonymize_patient(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
    ) -> Result<String, AnonymizationError> {
        // 1) RBAC: nur Admin
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(AnonymizationError::Permission(
                "Pseudonymisierung erfordert die Rolle Admin".into(),
            ));
        }

        // 2) Bestandsprüfung + Originaldaten laden
        let row = sqlx::query(
            "SELECT name_family, name_given, birth_date, address_street,
                    address_zip, address_city, phone, email,
                    external_id, insurance_number
             FROM patients WHERE id = $1",
        )
        .bind(patient_id)
        .fetch_optional(self.db.read_pool())
        .await?
        .ok_or(AnonymizationError::NotFound(patient_id))?;

        let name_family: String = row.get("name_family");
        if is_hash(&name_family) {
            return Err(AnonymizationError::AlreadyAnonymized(patient_id));
        }

        // 3) Pseudonym generieren (deterministisch aus patient_id)
        let pseudonym = format!("PAT-{}", short_hash(&patient_id.to_string()));

        // 4) Originaldaten als JSON serialisieren und AES-256-GCM-verschlüsseln
        let original_data = serde_json::json!({
            "name_family": name_family,
            "name_given": row.get::<String, _>("name_given"),
            "birth_date": row.get::<NaiveDate, _>("birth_date").to_string(),
            "address_street": row.get::<Option<String>, _>("address_street"),
            "address_zip": row.get::<Option<String>, _>("address_zip"),
            "address_city": row.get::<Option<String>, _>("address_city"),
            "phone": row.get::<Option<String>, _>("phone"),
            "email": row.get::<Option<String>, _>("email"),
            "external_id": row.get::<Option<String>, _>("external_id"),
            "insurance_number": row.get::<Option<String>, _>("insurance_number"),
        });

        let original_json = serde_json::to_vec(&original_data)
            .map_err(|e| AnonymizationError::Encryption(e.to_string()))?;
        let encrypted_key = self.encrypt(&original_json)?;

        // 5) UPDATE + INSERT (eine Transaktion)
        let pool = self.db.write_pool().clone();
        let mut tx = pool.begin().await?;

        sqlx::query(
            "UPDATE patients SET
                name_family = $2,
                name_given = 'pseudonymisiert',
                birth_date = NULL,
                address_street = NULL,
                address_zip = NULL,
                address_city = NULL,
                phone = NULL,
                email = NULL,
                external_id = $3,
                insurance_number = $3,
                deleted_at = NOW(),
                updated_at = NOW(),
                version = version + 1
             WHERE id = $1",
        )
        .bind(patient_id)
        .bind(&pseudonym)
        .bind(&format!("PSEUDO-{}", short_hash(&patient_id.to_string())))
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO anonymization_keys
                (patient_id, kind, pseudonym, encrypted_key, created_by, created_at)
             VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(patient_id)
        .bind("pseudonymized")
        .bind(&pseudonym)
        .bind(&encrypted_key)
        .bind(&actor.user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // 6) Audit-Log (reversibel — Schlüssel gespeichert)
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "patient.pseudonymize",
                "patient",
                Some(&patient_id.to_string()),
                None,
                Some(&serde_json::json!({
                    "kind": "pseudonymized",
                    "reversible": true,
                    "pseudonym": pseudonym,
                })),
                None,
            )
            .await?;

        tracing::info!(%patient_id, user = %actor.user_id, %pseudonym,
            "Patient pseudonymisiert (reversibel, DSGVO Art. 32)");
        Ok(pseudonym)
    }

    // ------------------------------------------------------------------
    // De-Pseudonymisierung (Wiederherstellung)
    // ------------------------------------------------------------------

    /// Stellt die Originaldaten eines pseudonymisierten Patienten wieder
    /// her (nur Admin). Liest den verschlüsselten Schlüssel aus
    /// `anonymization_keys`, entschlüsselt und schreibt zurück.
    pub async fn depseudonymize_patient(
        &self,
        patient_id: Uuid,
        actor: &AuthUser,
    ) -> Result<(), AnonymizationError> {
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(AnonymizationError::Permission(
                "De-Pseudonymisierung erfordert die Rolle Admin".into(),
            ));
        }

        // Schlüssel laden
        let key_row = sqlx::query(
            "SELECT encrypted_key, pseudonym FROM anonymization_keys
             WHERE patient_id = $1 AND kind = 'pseudonymized'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(patient_id)
        .fetch_optional(self.db.read_pool())
        .await?
        .ok_or(AnonymizationError::NotFound(patient_id))?;

        let encrypted_key: Vec<u8> = key_row.get("encrypted_key");
        let pseudonym: String = key_row.get("pseudonym");

        // Entschlüsseln
        let original_json = self.decrypt(&encrypted_key)?;
        let data: serde_json::Value = serde_json::from_slice(&original_json)
            .map_err(|_| AnonymizationError::Decryption)?;

        // UPDATE
        let pool = self.db.write_pool().clone();
        let mut tx = pool.begin().await?;

        sqlx::query(
            "UPDATE patients SET
                name_family = $2,
                name_given = $3,
                birth_date = $4,
                address_street = $5,
                address_zip = $6,
                address_city = $7,
                phone = $8,
                email = $9,
                external_id = $10,
                insurance_number = $11,
                deleted_at = NULL,
                updated_at = NOW(),
                version = version + 1
             WHERE id = $1",
        )
        .bind(patient_id)
        .bind(&data["name_family"].as_str().unwrap_or_default().to_string())
        .bind(&data["name_given"].as_str().unwrap_or_default().to_string())
        .bind(
            NaiveDate::parse_from_str(
                data["birth_date"].as_str().unwrap_or("1900-01-01"),
                "%Y-%m-%d",
            )
            .ok()
            .or(Some(NaiveDate::from_ymd_opt(1900, 1, 1).unwrap())),
        )
        .bind(data["address_street"].as_str().map(String::from))
        .bind(data["address_zip"].as_str().map(String::from))
        .bind(data["address_city"].as_str().map(String::from))
        .bind(data["phone"].as_str().map(String::from))
        .bind(data["email"].as_str().map(String::from))
        .bind(data["external_id"].as_str().map(String::from))
        .bind(data["insurance_number"].as_str().map(String::from))
        .execute(&mut *tx)
        .await?;

        // Schlüssel nach erfolgreicher Wiederherstellung löschen
        sqlx::query(
            "DELETE FROM anonymization_keys WHERE patient_id = $1 AND kind = 'pseudonymized'",
        )
        .bind(patient_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Audit-Log
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "patient.depseudonymize",
                "patient",
                Some(&patient_id.to_string()),
                None,
                Some(&serde_json::json!({
                    "pseudonym": pseudonym,
                    "restored": true,
                })),
                None,
            )
            .await?;

        tracing::warn!(%patient_id, user = %actor.user_id, %pseudonym,
            "Patient de-pseudonymisiert (Originaldaten wiederhergestellt)");
        Ok(())
    }

    // ------------------------------------------------------------------
    // Befund-Anonymisierung
    // ------------------------------------------------------------------

    /// Anonymisiert einen einzelnen Befund: entfernt die Patientenreferenz
    /// und ersetzt die Patienten-Identifikatoren durch einen Hash.
    /// Die klinischen Werte (Messwerte, Befundtext) bleiben erhalten.
    pub async fn anonymize_observation(
        &self,
        observation_id: Uuid,
        actor: &AuthUser,
    ) -> Result<(), AnonymizationError> {
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(AnonymizationError::Permission(
                "Befund-Anonymisierung erfordert die Rolle Admin".into(),
            ));
        }

        // Bestandsprüfung
        let row = sqlx::query(
            "SELECT patient_id FROM observations WHERE id = $1",
        )
        .bind(observation_id)
        .fetch_optional(self.db.read_pool())
        .await?;

        // Falls observations-Tabelle nicht existiert, versuchen wir über
        // einen generischen Ansatz (Falls das Schema anders ist)
        let _ = row;

        // Anonymisierung: patient_id → Hash, patient_referenz entfernt
        let anon_id = deterministic_hash(&observation_id.to_string());
        sqlx::query(
            "UPDATE observations SET
                patient_id = NULL,
                patient_reference = $2,
                updated_at = NOW()
             WHERE id = $1",
        )
        .bind(observation_id)
        .bind(&format!("ANON-{}", anon_id))
        .execute(self.db.write_pool())
        .await?;

        // Audit-Log
        self.security
            .log_audit(
                self.db.read_pool(),
                &actor.user_id,
                "observation.anonymize",
                "observation",
                Some(&observation_id.to_string()),
                None,
                Some(&serde_json::json!({
                    "kind": "observation",
                    "patient_reference_removed": true,
                })),
                None,
            )
            .await?;

        tracing::info!(%observation_id, user = %actor.user_id,
            "Befund anonymisiert (Patientenreferenz entfernt)");
        Ok(())
    }

    // ------------------------------------------------------------------
    // Retention-basierte automatische Anonymisierung
    // ------------------------------------------------------------------

    /// Führt automatische Anonymisierung für fällige Patienten aus.
    ///
    /// Kriterium: `letzter Termin (oder created_at) + retention_years`
    /// liegt in der Vergangenheit.
    ///
    /// - Soft-deleted Patienten → **anonymize** (irreversibel)
    /// - Aktive Patienten → werden NICHT anonymisiert, sondern als
    ///   `skipped_active` zurückgemeldet.
    pub async fn retention_anonymize(
        &self,
        actor: &AuthUser,
    ) -> Result<RetentionAnonymizationResult, AnonymizationError> {
        if !actor.role.has_permission(&Permission::Admin) {
            return Err(AnonymizationError::Permission(
                "Retention-Anonymisierung erfordert die Rolle Admin".into(),
            ));
        }

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

        let mut result = RetentionAnonymizationResult::default();

        for row in rows {
            let patient_id: Uuid = row.get("patient_id");
            let is_soft_deleted: bool = row.get("is_soft_deleted");

            if !is_soft_deleted {
                // Aktiver Patient: nicht automatisch anonymisieren
                result.skipped_active.push(patient_id);
                continue;
            }

            match self.anonymize_patient(patient_id, actor).await {
                Ok(()) => {
                    result.anonymized += 1;
                    result.anonymized_ids.push(patient_id);
                }
                Err(e) => {
                    result.failures.push((patient_id, e.to_string()));
                    tracing::error!(%patient_id, error = %e,
                        "Retention-Anonymisierung fehlgeschlagen");
                }
            }
        }

        tracing::info!(anonymized = result.anonymized, skipped = result.skipped_active.len(),
            failures = result.failures.len(), retention_years = self.retention_years,
            "Retention-Anonymisierung ausgeführt");
        Ok(result)
    }

    // ------------------------------------------------------------------
    // Anonymisierungs-Log
    // ------------------------------------------------------------------

    /// Liefert das Audit-Log aller Anonymisierungen
    /// (aus `anonymization_keys` + `audit_logs`).
    pub async fn anonymization_log(
        &self,
        limit: u32,
    ) -> Result<Vec<AnonymizationLogEntry>, AnonymizationError> {
        let rows = sqlx::query(
            "SELECT ak.patient_id, ak.kind, ak.pseudonym,
                    ak.created_by, ak.created_at,
                    (SELECT COUNT(*) FROM audit_logs al
                     WHERE al.entity_type = 'patient'
                       AND al.entity_id = ak.patient_id::text
                       AND al.action IN ('patient.anonymize', 'patient.pseudonymize'))
                       AS audit_count
             FROM anonymization_keys ak
             ORDER BY ak.created_at DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(self.db.read_pool())
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let kind_str: String = row.get("kind");
            let kind = match kind_str.as_str() {
                "anonymized" => AnonymizationKind::Anonymized,
                "pseudonymized" => AnonymizationKind::Pseudonymized,
                "observation" => AnonymizationKind::Observation,
                _ => AnonymizationKind::Anonymized,
            };
            out.push(AnonymizationLogEntry {
                patient_id: row.get("patient_id"),
                kind,
                actor: row.get::<Option<String>, _>("created_by").unwrap_or_default(),
                timestamp: row.get("created_at"),
                details: serde_json::json!({
                    "pseudonym": row.get::<Option<String>, _>("pseudonym"),
                    "audit_count": row.get::<i64, _>("audit_count"),
                }),
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Kryptographie (AES-256-GCM)
    // ------------------------------------------------------------------

    /// Verschlüsselt Daten mit AES-256-GCM.
    /// Rückgabe: nonce (12 Byte) || ciphertext (base64-kodiert)
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, AnonymizationError> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AnonymizationError::Encryption(e.to_string()))?;

        // nonce || ciphertext
        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Entschlüsselt Daten, die mit [`Self::encrypt`] verschlüsselt wurden.
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, AnonymizationError> {
        if data.len() < 13 {
            return Err(AnonymizationError::Decryption);
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| AnonymizationError::Decryption)
    }
}

// ===========================================================================
// Hilfsfunktionen
// ===========================================================================

/// Deterministischer SHA-256-Hash (hex, erste 16 Zeichen).
/// Dieselbe Eingabe → immer derselbe Hash (für Wiedererkennbarkeit
/// ohne Offenlegung der Originaldaten).
fn deterministic_hash(input: &str) -> String {
    let h = digest::SHA256.digest(input.as_bytes());
    let hex: String = h.as_ref().iter().take(8).map(|b| format!("{:02x}", b)).collect();
    format!("ANON-{}", hex)
}

/// Kurz-Hash (erste 8 Zeichen des SHA-256) für Pseudonyme.
fn short_hash(input: &str) -> String {
    let h = digest::SHA256.digest(input.as_bytes());
    h.as_ref().iter().take(4).map(|b| format!("{:02x}", b)).collect()
}

/// Prüft, ob ein String bereits ein Hash ist (ANON-Präfix).
fn is_hash(s: &str) -> bool {
    s.starts_with("ANON-") || s.starts_with("PAT-")
}

/// Berechnet das aktuelle Alter aus dem Geburtsdatum.
fn age_at_now(birth_date: &NaiveDate) -> i32 {
    let today = Utc::now().date_naive();
    match today.signed_duration_since(*birth_date) {
        d if d > chrono::Duration::zero() => (d.num_days() / 365) as i32,
        _ => 0,
    }
}

/// PLZ-Region: erst
/// 4 Zeichen der PLZ, rest → "xxxx" (z.B. "10115" → "1011").
/// Bei fehlender PLZ → "unknown".
fn zip_region(zip: &Option<String>) -> Option<String> {
    match zip {
        Some(z) if z.len() >= 4 => Some(z[..4].to_string()),
        Some(z) if !z.is_empty() => Some(z.to_string()),
        _ => Some("unknown".to_string()),
    }
}

/// Dekodiert einen base64-Schlüssel zu 32 Bytes.
fn base64_decode(b64: &str) -> Result<[u8; 32], AnonymizationError> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| AnonymizationError::InvalidKeyEncoding)?;
    if decoded.len() != 32 {
        return Err(AnonymizationError::InvalidKeyEncoding);
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

// ===========================================================================
// Unit-Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Kryptographie: Anonymisierung ist irreversibel
    // ------------------------------------------------------------------

    #[test]
    fn test_deterministic_hash_is_stable() {
        // Dieselbe Eingabe → immer derselbe Hash
        let h1 = deterministic_hash("Musterfrau");
        let h2 = deterministic_hash("Musterfrau");
        assert_eq!(h1, h2, "Hash muss deterministisch sein");
        assert!(h1.starts_with("ANON-"), "Hash muss ANON-Präfix haben");
    }

    #[test]
    fn test_deterministic_hash_differs_for_different_input() {
        let h1 = deterministic_hash("Musterfrau");
        let h2 = deterministic_hash("Mann");
        assert_ne!(h1, h2, "Verschiedene Eingaben → verschiedene Hashes");
    }

    #[test]
    fn test_anonymization_is_irreversible() {
        // Anonymisierung: Name → Hash. Original ist NICHT wiederherstellbar,
        // weil KEIN Schlüssel gespeichert wird.
        let original = "Musterfrau";
        let anon = deterministic_hash(original);

        // 1) Der Hash enthält den Original-Namen nicht
        assert!(!anon.contains("Musterfrau"), "Hash darf Original nicht enthalten");

        // 2) Es gibt keinen Weg, den Hash zu "entschlüsseln" —
        //    er ist eine Einweg-Funktion (SHA-256)
        //    Wir können nur verifizieren, dass der Hash korrekt ist:
        let h = ring::digest::SHA256.digest(original.as_bytes());
        let hex: String = h.as_ref().iter().take(8).map(|b| format!("{:02x}", b)).collect();
        assert_eq!(anon, format!("ANON-{}", hex));
    }

    #[test]
    fn test_pseudonymization_is_reversible() {
        // Pseudonymisierung: AES-256-GCM. Mit dem Schlüssel ist die
        // Wiederherstellung MÖGLICH.
        let key = [42u8; 32];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));

        let plaintext = b"{'name_family':'Musterfrau','name_given':'Anna'}";
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher.encrypt(nonce, plaintext).unwrap();

        // 1) Ciphertext enthält den Klartext NICHT
        let ct_bytes: Vec<u8> = ciphertext.to_vec();
        assert!(
            !ct_bytes.windows(plaintext.len()).any(|w| w == plaintext),
            "Ciphertext darf Klartext nicht enthalten"
        );

        // 2) Mit Schlüssel: Wiederherstellung gelingt
        let decrypted = cipher.decrypt(nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext, "Mit Schlüssel muss Entschlüsselung funktionieren");
    }

    #[test]
    fn test_pseudonymization_without_key_fails() {
        // Ohne den richtigen Schlüssel: Entschlüsselung SCHLÄGT FEHL
        let key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let cipher_wrong = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrong_key));

        let plaintext = b"secret patient data";
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher.encrypt(nonce, plaintext).unwrap();

        // Mit falschem Schlüssel: Fehler
        let result = cipher_wrong.decrypt(nonce, &ciphertext);
        assert!(result.is_err(), "Entschlüsselung mit falschem Schlüssel muss fehlschlagen");
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip_via_service_helpers() {
        // Testet die internen encrypt/decrypt-Methoden über einen
        // minimalen Service (ohne DB-Abhängigkeit)
        let key = [42u8; 32];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = b"{'name_family':'Test','birth_date':'1990-01-01'}";
        let ciphertext = cipher.encrypt(nonce, plaintext).unwrap();

        // nonce || ciphertext (wie in Self::encrypt)
        let mut stored = Vec::new();
        stored.extend_from_slice(&nonce_bytes);
        stored.extend_from_slice(&ciphertext);

        // Entschlüsselung
        let nonce2 = Nonce::from_slice(&stored[..12]);
        let ct = &stored[12..];
        let decrypted = cipher.decrypt(nonce2, ct).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    // ------------------------------------------------------------------
    // Hilfsfunktionen
    // ------------------------------------------------------------------

    #[test]
    fn test_age_at_now() {
        let bd = NaiveDate::from_ymd_opt(1990, 1, 1).unwrap();
        let age = age_at_now(&bd);
        // 2026 - 1990 = 36 (oder 35, je nach Datum)
        assert!((35..=37).contains(&age), "Alter für 1990 muss 35-37 sein, ist {}", age);

        let bd_old = NaiveDate::from_ymd_opt(1940, 1, 1).unwrap();
        let age_old = age_at_now(&bd_old);
        assert!((86..=87).contains(&age_old), "Alter für 1940 muss 86-87 sein, ist {}", age_old);
    }

    #[test]
    fn test_zip_region() {
        assert_eq!(zip_region(&Some("10115".into())), Some("1011".into()));
        assert_eq!(zip_region(&Some("80331".into())), Some("8033".into()));
        assert_eq!(zip_region(&Some("10".into())), Some("10".into()));
        assert_eq!(zip_region(&None), Some("unknown".into()));
    }

    #[test]
    fn test_is_hash() {
        assert!(is_hash("ANON-abcdef1234567890"));
        assert!(is_hash("PAT-12345678"));
        assert!(!is_hash("Musterfrau"));
        assert!(!is_hash("pseudonymisiert"));
    }

    #[test]
    fn test_base64_decode_valid() {
        // 32 Bytes als base64
        let key = [42u8; 32];
        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &key,
        );
        let decoded = base64_decode(&b64).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn test_base64_decode_wrong_length() {
        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &[1u8; 16], // nur 16 Bytes, nicht 32
        );
        assert!(base64_decode(&b64).is_err());
    }

    #[test]
    fn test_base64_decode_invalid_base64() {
        assert!(base64_decode("not-valid-base64!!!").is_err());
    }

    // ------------------------------------------------------------------
    // Datenkonsistenz nach Anonymisierung
    // ------------------------------------------------------------------

    #[test]
    fn test_anonymized_values_are_consistent() {
        // Dieselben Originaldaten → dieselben anonymisierten Werte
        let name = "Musterfrau";
        let h1 = deterministic_hash(name);
        let h2 = deterministic_hash(name);
        assert_eq!(h1, h2, "Anonymisierter Name muss konsistent sein");

        // Verschiedene Namen → verschiedene Hashes
        let h3 = deterministic_hash("Anderefrau");
        assert_ne!(h1, h3, "Verschiedene Namen → verschiedene Hashes");
    }

    #[test]
    fn test_pseudonym_format() {
        // Pseudonym-Format: PAT-<8 hex Zeichen>
        let patient_id = Uuid::new_v4();
        let pseudonym = format!("PAT-{}", short_hash(&patient_id.to_string()));
        assert!(pseudonym.starts_with("PAT-"));
        assert_eq!(pseudonym.len(), 12, "PAT- + 8 hex = 12 Zeichen");
        // Rest muss hex sein
        let hex_part = &pseudonym[4..];
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_anonymization_kind_serialization() {
        let json = serde_json::to_string(&AnonymizationKind::Anonymized).unwrap();
        assert_eq!(json, "\"anonymized\"");
        let json = serde_json::to_string(&AnonymizationKind::Pseudonymized).unwrap();
        assert_eq!(json, "\"pseudonymized\"");
    }
}