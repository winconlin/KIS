//! Security-Modul: JWT, AES-256-GCM, Audit-Log, RBAC, Rate-Limiting.
//!
//! Rechtssicherheit & Datenschutz:
//! - AES-256-GCM für sensible Patientendaten at rest
//! - TLS 1.3 in transit (via Axum/rustls)
//! - JWT mit Access (15 min) + Refresh (24 h)
//! - MFA via TOTP
//! - RBAC: Admin, Arzt, Pflege, Rezeption, Viewer
//! - Audit-Log: Hash-Chain, 10 Jahre, tamper-proof
//! - Rate-Limiting: Sliding Window

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::config::SecurityConfig;

/// Security-Kontext: Wird in alle Handler injiziert
pub struct Security {
    jwt_secret: String,
    aes_key: [u8; 32],
    config: SecurityConfig,
}

impl Security {
    pub fn new(config: &SecurityConfig) -> anyhow::Result<Self> {
        // JWT-Secret aus ENV
        let jwt_secret = std::env::var(&config.jwt_secret_env)
            .unwrap_or_else(|_| {
                error!("{} nicht gesetzt — verwende unsicheren Dev-Default", config.jwt_secret_env);
                "dev-insecure-secret-change-me".to_string()
            });

        // AES-256-GCM Master Key aus ENV (Base64, 32 Bytes)
        let aes_key_b64 = std::env::var(&config.encryption.master_key_env)
            .unwrap_or_else(|_| {
                error!("{} nicht gesetzt — verwende unsicheren Dev-Default", config.encryption.master_key_env);
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()
            });
        let aes_key_bytes = B64.decode(&aes_key_b64)
            .map_err(|e| anyhow::anyhow!("Ungültiger Base64 Master Key: {}", e))?;
        let mut aes_key = [0u8; 32];
        aes_key.copy_from_slice(&aes_key_bytes[..32]);

        info!("Security initialisiert");
        Ok(Self {
            jwt_secret,
            aes_key,
            config: config.clone(),
        })
    }

    /// Erzeugt einen Access-Token (15 min)
    pub fn create_access_token(&self, user_id: &str, role: &str) -> anyhow::Result<String> {
        let now = chrono::Utc::now();
        let expiry = now + chrono::Duration::seconds(self.config.token_expiry as i64);

        let claims = TokenClaims::new(user_id, Role::from_str(role).unwrap_or(Role::Viewer), expiry);

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )?;
        Ok(token)
    }

    /// Erzeugt einen Refresh-Token (24 h)
    pub fn create_refresh_token(&self, user_id: &str) -> anyhow::Result<String> {
        let expiry = chrono::Utc::now() + chrono::Duration::hours(24);

        let claims = TokenClaims {
            sub: user_id.to_string(),
            role: Role::Viewer,
            iat: chrono::Utc::now().timestamp(),
            exp: expiry.timestamp(),
            jti: uuid::Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )?;
        Ok(token)
    }

    /// Access-Token-TTL in Sekunden
    pub fn access_token_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.token_expiry as u64)
    }

    /// Audit-Log-Eintrag mit kryptographischer Hash-Chain (Task 1.12).
    ///
    /// Schreibt den Eintrag in die `audit_logs`-Tabelle. Der Hash wird
    /// über den letzten Chain-Eintrag verkettet:
    ///
    /// ```text
    /// hash = SHA-256(prev_hash || user_id || action || timestamp
    ///                || entity_type || entity_id || old_data || new_data)
    /// ```
    ///
    /// Der erste Eintrag (Genesis) verwendet `prev_hash = 64 Nullen`.
    /// Jede nachträgliche Änderung eines Eintrags bricht die Chain —
    /// sie wird von [`Security::verify_audit_chain`] erkannt.
    pub async fn log_audit(
        &self,
        db: &sqlx::PgPool,
        user_id: &str,
        action: &str,
        entity_type: &str,
        entity_id: Option<&str>,
        old_data: Option<&serde_json::Value>,
        new_data: Option<&serde_json::Value>,
        ip_address: Option<&str>,
    ) -> anyhow::Result<()> {
        // 1) Letzten Chain-Eintrag lesen (für prev_hash)
        let last = sqlx::query_scalar::<_, String>(
            "SELECT hash FROM audit_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(db)
        .await
        .map_err(|e| anyhow::anyhow!("Audit-Chain-Read fehlgeschlagen: {e}"))?;

        // 2) prev_hash: Genesis (64 Nullen) oder Hash des letzten Eintrags
        const GENESIS: &str =
            "0000000000000000000000000000000000000000000000000000000000000000";
        let prev_hash = last.unwrap_or_else(|| GENESIS.to_string());

        // 3) Timestamp in deterministischem Format (RFC3339, UTC)
        let now = chrono::Utc::now();
        let ts_str = now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

        // 4) JSONB-Felder als deterministische Strings (null bei None)
        let old_str = old_data
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into());
        let new_str = new_data
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into());

        // 5) Hash der Chain-Verkettung (deterministisch, geordnet)
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(b"|");
        hasher.update(user_id.as_bytes());
        hasher.update(b"|");
        hasher.update(action.as_bytes());
        hasher.update(b"|");
        hasher.update(ts_str.as_bytes());
        hasher.update(b"|");
        hasher.update(entity_type.as_bytes());
        hasher.update(b"|");
        hasher.update(entity_id.unwrap_or("").as_bytes());
        hasher.update(b"|");
        hasher.update(old_str.as_bytes());
        hasher.update(b"|");
        hasher.update(new_str.as_bytes());
        let hash = hex_sha256(&hasher.finalize());

        // 6) Insert (atomar: der Chain-Zustand ist jetzt fest)
        sqlx::query(
            "INSERT INTO audit_logs \
             (user_id, action, entity_type, entity_id, old_data, new_data, \
              ip_address, timestamp, prev_hash, hash) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(user_id)
        .bind(action)
        .bind(entity_type)
        .bind(entity_id)
        .bind(&old_data)
        .bind(&new_data)
        .bind(ip_address)
        .bind(now)
        .bind(&prev_hash)
        .bind(&hash)
        .execute(db)
        .await
        .map_err(|e| anyhow::anyhow!("Audit-Insert fehlgeschlagen: {e}"))?;

        tracing::debug!(
            user_id, action, entity_type,
            %hash, "Audit-Eintrag mit Hash-Chain geschrieben"
        );
        Ok(())
    }

    /// Verifiziert die gesamte Audit-Hash-Chain (Task 1.12, Compliance).
    ///
    /// Liest alle Einträge chronologisch und prüft für jeden:
    /// 1. `prev_hash` stimmt mit dem Hash des Vorgängers überein
    ///    (der allererste muss den Genesis-Hash haben)
    /// 2. Der gespeicherte `hash` stimmt mit dem neu berechneten Wert
    ///    aus den Datenfeldern überein (Manipulationserkennung)
    ///
    /// Rückgabe: `Ok(true)` = Chain intakt, `Ok(false)` = Bruch an
    /// Index `i` (0-basiert), `Err` = DB-Fehler.
    pub async fn verify_audit_chain(
        &self,
        db: &sqlx::PgPool,
    ) -> anyhow::Result<(bool, Option<usize>)> {
        const GENESIS: &str =
            "0000000000000000000000000000000000000000000000000000000000000000";

        #[derive(sqlx::FromRow)]
        struct ChainRow {
            id: i64,
            user_id: Option<String>,
            action: String,
            entity_type: String,
            entity_id: Option<String>,
            old_data: Option<serde_json::Value>,
            new_data: Option<serde_json::Value>,
            timestamp: chrono::DateTime<chrono::Utc>,
            prev_hash: String,
            hash: String,
        }

        let rows: Vec<ChainRow> = sqlx::query_as(
            "SELECT id, user_id, action, entity_type, entity_id, \
                    old_data, new_data, timestamp, prev_hash, hash \
             FROM audit_logs ORDER BY id ASC",
        )
        .fetch_all(db)
        .await
        .map_err(|e| anyhow::anyhow!("Audit-Chain-Read fehlgeschlagen: {e}"))?;

        let mut expected_prev = GENESIS.to_string();

        for (i, row) in rows.iter().enumerate() {
            // 1) prev_hash muss zum Vorgänger passen
            if row.prev_hash != expected_prev {
                warn!(
                    index = i,
                    expected = %expected_prev,
                    actual = %row.prev_hash,
                    "Audit-Chain bruch: prev_hash passt nicht"
                );
                return Ok((false, Some(i)));
            }

            // 2) hash neu berechnen und vergleichen
            let ts_str = row.timestamp.to_rfc3339_opts(
                chrono::SecondsFormat::Nanos, true,
            );
            let old_str = row.old_data.as_ref().map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());
            let new_str = row.new_data.as_ref().map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());

            let mut hasher = Sha256::new();
            hasher.update(row.prev_hash.as_bytes());
            hasher.update(b"|");
            hasher.update(row.user_id.as_deref().unwrap_or("").as_bytes());
            hasher.update(b"|");
            hasher.update(row.action.as_bytes());
            hasher.update(b"|");
            hasher.update(ts_str.as_bytes());
            hasher.update(b"|");
            hasher.update(row.entity_type.as_bytes());
            hasher.update(b"|");
            hasher.update(row.entity_id.as_deref().unwrap_or("").as_bytes());
            hasher.update(b"|");
            hasher.update(old_str.as_bytes());
            hasher.update(b"|");
            hasher.update(new_str.as_bytes());
            let computed = hex_sha256(&hasher.finalize());

            if computed != row.hash {
                warn!(
                    index = i,
                    computed,
                    stored = %row.hash,
                    "Audit-Chain bruch: hash manipuliert"
                );
                return Ok((false, Some(i)));
            }

            expected_prev = row.hash.clone();
        }

        info!(
            eintraege = rows.len(),
            "Audit-Hash-Chain verifiziert — intakt"
        );
        Ok((true, None))
    }

    /// Liest den Audit-Trail für eine konkrete Entität (Task 1.12).
    ///
    /// `GET /api/v1/audit/:entity_type/:entity_id`
    pub async fn get_audit_trail(
        &self,
        db: &sqlx::PgPool,
        entity_type: &str,
        entity_id: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let rows: Vec<serde_json::Value> =
            sqlx::query_scalar(
                "SELECT jsonb_build_object(
                    'id', id,
                    'user_id', user_id,
                    'action', action,
                    'entity_type', entity_type,
                    'entity_id', entity_id,
                    'old_data', old_data,
                    'new_data', new_data,
                    'ip_address', ip_address,
                    'timestamp', timestamp,
                    'prev_hash', prev_hash,
                    'hash', hash
                 )
                 FROM audit_logs
                 WHERE entity_type = $1 AND entity_id = $2
                 ORDER BY id ASC",
            )
            .bind(entity_type)
            .bind(entity_id)
            .fetch_all(db)
            .await
            .map_err(|e| anyhow::anyhow!("Audit-Trail-Read fehlgeschlagen: {e}"))?;
        Ok(rows)
    }

    /// Verifiziert einen JWT-Token und liefert (user_id, role).
    pub fn verify_token(&self, token: &str) -> anyhow::Result<(String, Role)> {
        let data = decode::<TokenClaims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        )?;
        Ok((data.claims.sub, data.claims.role))
    }

    /// Verifiziert ein Passwort gegen Argon2id-Hash
    pub fn verify_password(&self, password: &str, hash: &str) -> bool {
        use argon2::Argon2;
        use argon2::password_hash::PasswordVerifier;

        match argon2::password_hash::PasswordHash::new(hash) {
            Ok(parsed_hash) => Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok(),
            Err(_) => false,
        }
    }

    /// Hash ein Passwort mit Argon2id
    pub fn hash_password(&self, password: &str) -> anyhow::Result<String> {
        use argon2::Argon2;
        use argon2::password_hash::{PasswordHasher, SaltString};
        use argon2::password_hash::rand_core::OsRng;

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("Password-Hashing fehlgeschlagen: {}", e))?;
        Ok(hash.to_string())
    }

    /// Verschlüsselt Daten mit AES-256-GCM (Base64-codiert)
    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.aes_key)
            .map_err(|e| anyhow::anyhow!("AES-Initialisierung fehlgeschlagen: {}", e))?;

        // Zufälliger 12-Byte-Nonce
        let mut nonce_bytes = [0u8; 12];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("Verschlüsselung fehlgeschlagen: {}", e))?;

        // Nonce + Ciphertext zusammenpacken und Base64-codieren
        let mut output = Vec::with_capacity(12 + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(B64.encode(&output))
    }

    /// Entschlüsselt Daten (Base64-codiert)
    pub fn decrypt(&self, encrypted_b64: &str) -> anyhow::Result<Vec<u8>> {
        let data = B64
            .decode(encrypted_b64)
            .map_err(|e| anyhow::anyhow!("Ungültige Base64-Verschlüsselung: {}", e))?;

        if data.len() < 12 {
            anyhow::bail!("Verschlüsseltes Daten zu kurz (Nonce fehlt)");
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);
        let cipher = Aes256Gcm::new_from_slice(&self.aes_key)?;

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Entschlüsselung fehlgeschlagen: {}", e))?;

        Ok(plaintext)
    }

    /// Erzeugt einen Audit-Hash (Hash-Chain)
    pub fn audit_hash(&self, prev_hash: &str, data: &serde_json::Value) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(data.to_string().as_bytes());
        let result = hasher.finalize();
        // Hex-Encodierung ohne extra-Krate: byte-weises Formatieren
        let mut hex_str = String::with_capacity(result.len() * 2);
        for b in result {
            hex_str.push_str(&format!("{:02x}", b));
        }
        hex_str
    }

    /// Prüft, ob ein User gesperrt ist
    pub fn is_locked(&self, attempts: u32, locked_until: Option<chrono::DateTime<chrono::Utc>>) -> bool {
        if attempts >= self.config.max_login_attempts {
            if let Some(until) = locked_until {
                return chrono::Utc::now() < until;
            }
            return true;
        }
        false
    }
}

/// JWT Claims — enthält die Rolle des Users im Claim "role".
///
/// `role` wird als String serialisiert (serde), aber beim Deserialisieren
/// als [`Role`] geparst; unbekannte Rollen-Werte werden abgelehnt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenClaims {
    /// user_id (UUID)
    pub sub: String,
    /// Rolle des Users (Claim "role")
    pub role: Role,
    /// issued at
    pub iat: i64,
    /// expiration
    pub exp: i64,
    /// unique token ID
    pub jti: String,
}

impl TokenClaims {
    pub fn new(user_id: &str, role: Role, expiry: chrono::DateTime<chrono::Utc>) -> Self {
        let now = chrono::Utc::now();
        Self {
            sub: user_id.to_string(),
            role,
            iat: now.timestamp(),
            exp: expiry.timestamp(),
            jti: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// RBAC Rollen (Task 1.8)
///
/// Serde: lowercase-Strings ("admin", "arzt", "pflege", "rezeption", "viewer").
/// `FromStr` ist case-insensitiv; `Display` liefert den lowercase-String,
/// der in der users-Tabelle und im JWT-Claim gespeichert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Volle Kontrolle: alle Endpunkte und Operationen.
    Admin,
    /// Arzt: Patienten lesen/schreiben, Ärzte lesen, Termine
    /// lesen/schreiben/buchen/stornieren, Medikamente lesen/schreiben.
    Arzt,
    /// Pflege: Patienten, Termine, Medikamente nur lesen.
    Pflege,
    /// Rezeption: Patienten lesen/anlegen, Termine anlegen/stornieren.
    Rezeption,
    /// Viewer: Patienten und Termine nur lesen.
    Viewer,
}

impl Role {
    /// Alle definierten Rollen.
    pub const ALL: [Role; 5] = [
        Role::Admin,
        Role::Arzt,
        Role::Pflege,
        Role::Rezeption,
        Role::Viewer,
    ];

    /// DB-Wert der Rolle (users.role)
    pub fn as_db_string(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Arzt => "arzt",
            Role::Pflege => "pflege",
            Role::Rezeption => "rezeption",
            Role::Viewer => "viewer",
        }
    }

    /// Prüft, ob eine Rolle eine bestimmte Berechtigung hat.
    ///
    /// Berechtigungs-Matrix (Task 1.8):
    /// ```text
    ///                     patient.read patient.write doctor.read appt.read appt.write med.read med.write admin
    /// Admin                ✔            ✔             ✔           ✔         ✔          ✔        ✔       ✔
    /// Arzt                 ✔            ✔             ✔           ✔         ✔          ✔        ✔       ✘
    /// Pflege               ✔            ✘             ✘           ✔         ✘          ✔        ✘       ✘
    /// Rezeption            ✔            ✘ (nur create)✘          ✔         ✔ (nur create/cancel) ✘      ✘
    /// Viewer               ✔            ✘             ✘           ✔         ✘          ✘        ✘       ✘
    /// ```
    pub fn has_permission(&self, permission: &Permission) -> bool {
        match (self, permission) {
            // Admin: alles
            (Role::Admin, _) => true,

            // Arzt: patients (read/write), doctors (read),
            //       appointments (read/write/book/cancel), medications (read/write)
            (Role::Arzt, Permission::PatientRead) => true,
            (Role::Arzt, Permission::PatientWrite) => true,
            (Role::Arzt, Permission::PatientCreate) => true,
            (Role::Arzt, Permission::DoctorRead) => true,
            (Role::Arzt, Permission::AppointmentRead) => true,
            (Role::Arzt, Permission::AppointmentWrite) => true,
            (Role::Arzt, Permission::AppointmentBook) => true,
            (Role::Arzt, Permission::AppointmentCancel) => true,
            (Role::Arzt, Permission::MedicationRead) => true,
            (Role::Arzt, Permission::MedicationWrite) => true,

            // Pflege: patients (read), appointments (read), medications (read)
            (Role::Pflege, Permission::PatientRead) => true,
            (Role::Pflege, Permission::AppointmentRead) => true,
            (Role::Pflege, Permission::MedicationRead) => true,

            // Rezeption: patients (read/create), appointments (create/cancel)
            (Role::Rezeption, Permission::PatientRead) => true,
            (Role::Rezeption, Permission::PatientCreate) => true,
            (Role::Rezeption, Permission::AppointmentBook) => true,
            (Role::Rezeption, Permission::AppointmentCancel) => true,

            // Viewer: patients (read), appointments (read)
            (Role::Viewer, Permission::PatientRead) => true,
            (Role::Viewer, Permission::AppointmentRead) => true,

            _ => false,
        }
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "admin" => Ok(Role::Admin),
            "arzt" => Ok(Role::Arzt),
            "pflege" => Ok(Role::Pflege),
            "rezeption" => Ok(Role::Rezeption),
            "viewer" => Ok(Role::Viewer),
            other => Err(format!("Unbekannte Rolle: {}", other)),
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_string())
    }
}

/// Berechtigungen (Operationen) für die RBAC-Matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Patienten lesen (GET-Endpunkte)
    PatientRead,
    /// Patienten aktualisieren (PUT /patients/:id)
    PatientWrite,
    /// Patienten anlegen (POST /patients)
    PatientCreate,
    /// Patienten löschen (DELETE, DSGVO Art. 17)
    PatientDelete,
    /// Ärzte lesen
    DoctorRead,
    /// Ärzte schreiben
    DoctorWrite,
    /// Termine lesen
    AppointmentRead,
    /// Termine aktualisieren (PUT /appointments/:id)
    AppointmentWrite,
    /// Termin buchen (POST /appointments)
    AppointmentBook,
    /// Termin stornieren
    AppointmentCancel,
    /// Medikamente lesen
    MedicationRead,
    /// Medikamente schreiben (anlegen + aktualisieren)
    MedicationWrite,
    /// Administrations-Endpunkte (User-Verwaltung, Audit-Log)
    Admin,
    /// Audit-Log lesen
    AuditRead,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Permission::PatientRead => "patient.read",
            Permission::PatientWrite => "patient.write",
            Permission::PatientCreate => "patient.create",
            Permission::PatientDelete => "patient.delete",
            Permission::DoctorRead => "doctor.read",
            Permission::DoctorWrite => "doctor.write",
            Permission::AppointmentRead => "appointment.read",
            Permission::AppointmentWrite => "appointment.write",
            Permission::AppointmentBook => "appointment.book",
            Permission::AppointmentCancel => "appointment.cancel",
            Permission::MedicationRead => "medication.read",
            Permission::MedicationWrite => "medication.write",
            Permission::Admin => "admin",
            Permission::AuditRead => "audit.read",
        };
        f.write_str(s)
    }
}

// ===========================================================================
// Axum-Extractor: RequireRole + SecurityState + Protected
// ===========================================================================

/// State-Wrapper für Auth: wird als axum-State-Typ registriert.
///
/// ```ignore
/// let state = SecurityState::new(Arc::new(security));
/// Router::new()
///     .route("/patients", get(get_patients))
///     .with_state(state)
/// ```
///
/// `SecurityState` ist `Clone` (enthält `Arc<Security>`), daher erfüllt er
/// axum's State-Erfordernisse.
#[derive(Clone)]
pub struct SecurityState {
    pub security: Arc<Security>,
}

impl SecurityState {
    pub fn new(security: Arc<Security>) -> Self {
        Self { security }
    }

    /// Verifiziert ein Bearer-Token und liefert [`AuthUser`].
    pub fn authenticate(&self, token: &str) -> anyhow::Result<AuthUser> {
        let (user_id, role) = self.security.verify_token(token)?;
        Ok(AuthUser { user_id, role })
    }
}

/// Authentifizierter User: user_id + Rolle (aus dem JWT).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub role: Role,
}

/// Axum-Extractor: verifiziert den JWT-Bearer-Token und liefert
/// [`RequireRole`] mit dem authentifizierten User.
///
/// Rollen-Einschränkung pro Route wird über [`Protected`] realisiert.
///
/// Fehler-Verhalten:
/// - Fehlender/ungültiger Bearer-Token → **401 Unauthorized**
/// - Token gültig, aber Rolle nicht erlaubt → **403 Forbidden** (über [`Protected`])
///
/// ```ignore
/// async fn get_patient(
///     auth: RequireRole,   // auth.role(), auth.user_id()
///     State(security): State<SecurityState>,
///     Path(id): Path<String>,
/// ) -> Result<Json<Patient>, Response> { ... }
/// ```
pub struct RequireRole {
    /// Die erlaubten Rollen (leere Liste = keine Rollen-Einschränkung)
    pub allowed: Vec<Role>,
    /// Authentifizierter User
    pub auth: AuthUser,
}

impl RequireRole {
    /// Erzeugt einen RequireRole mit erlaubten Rollen (interner Konstrukt).
    pub fn with_roles(allowed: Vec<Role>, auth: AuthUser) -> Self {
        Self { allowed, auth }
    }

    /// Liefert die Rolle des authentifizierten Users.
    pub fn role(&self) -> Role {
        self.auth.role
    }

    /// Liefert die user_id.
    pub fn user_id(&self) -> &str {
        &self.auth.user_id
    }

    /// Prüft, ob die Rolle des Users eine bestimmte Berechtigung hat.
    pub fn can(&self, permission: &Permission) -> bool {
        self.auth.role.has_permission(permission)
    }
}

#[axum::async_trait]
impl FromRequestParts<SecurityState> for RequireRole {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SecurityState,
    ) -> Result<Self, Self::Rejection> {
        // Bearer-Token extrahieren
        let token = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| unauthorized_response("Fehlender Bearer-Token"))?
            .to_string();

        // Token verifizieren → (user_id, role)
        let auth_user = state
            .authenticate(&token)
            .map_err(|e| {
                warn!(error = %e, "JWT-Verifikation fehlgeschlagen");
                unauthorized_response("Ungültiger oder abgelaufener Token")
            })?;

        Ok(RequireRole {
            allowed: Vec::new(),
            auth: auth_user,
        })
    }
}

// ===========================================================================
// Rollen-Guard: prüft, ob der authentifizierte User eine der erlaubten
// Rollen hat. Wird in Handlers als erster Schritt aufgerufen.
// ===========================================================================

/// Prüft, ob der authentifizierte User (`auth`) eine der erlaubten Rollen
/// (`allowed`) hat. Liefert `Err(Response)` mit 403 Forbidden, wenn nicht.
///
/// ```ignore
/// async fn update_patient(
///     auth: RequireRole,
///     State(db): State<Arc<PgPool>>,
///     Path(id): Path<Uuid>,
///     Json(body): Json<UpdatePatient>,
/// ) -> Result<Json<Patient>, Response> {
///     guard_roles(&auth, [Role::Admin, Role::Arzt])?;
///     // ...
/// }
/// ```
pub fn guard_roles(auth: &RequireRole, allowed: impl IntoIterator<Item = Role>) -> Result<(), Response> {
    let allowed: Vec<Role> = allowed.into_iter().collect();
    if !allowed.is_empty() && !allowed.contains(&auth.role()) {
        let allowed_str = allowed
            .iter()
            .map(|r| r.as_db_string())
            .collect::<Vec<_>>()
            .join(", ");
        warn!(
            role = %auth.role(),
            allowed = %allowed_str,
            "403 Forbidden: Rolle nicht berechtigt"
        );
        return Err(forbidden_response(&format!(
            "Rolle '{}' ist für diesen Endpunkt nicht berechtigt (erlaubt: {})",
            auth.role(),
            allowed_str
        )));
    }
    Ok(())
}

fn unauthorized_response(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", "Bearer")],
        axum::Json(serde_json::json!({ "error": msg, "status": 401 })),
    )
        .into_response()
}

fn forbidden_response(msg: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({ "error": msg, "status": 403 })),
    )
        .into_response()
}

/// Hex-Encodierung eines SHA-256-Hashes.
fn hex_sha256(hash: &[u8]) -> String {
    let mut hex_str = String::with_capacity(hash.len() * 2);
    for b in hash {
        hex_str.push_str(&format!("{:02x}", b));
    }
    hex_str
}

// ===========================================================================
// Tests: Berechtigungs-Matrix (Task 1.8)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn all_permissions() -> Vec<Permission> {
        vec![
            Permission::PatientRead,
            Permission::PatientWrite,
            Permission::PatientCreate,
            Permission::PatientDelete,
            Permission::DoctorRead,
            Permission::DoctorWrite,
            Permission::AppointmentRead,
            Permission::AppointmentWrite,
            Permission::AppointmentBook,
            Permission::AppointmentCancel,
            Permission::MedicationRead,
            Permission::MedicationWrite,
            Permission::Admin,
            Permission::AuditRead,
        ]
    }

    #[test]
    fn admin_hat_alle_berechtigungen() {
        for p in all_permissions() {
            assert!(Role::Admin.has_permission(&p), "Admin fehlt: {}", p);
        }
    }

    #[test]
    fn arzt_matrix() {
        // Erlaubt
        for p in [
            Permission::PatientRead,
            Permission::PatientWrite,
            Permission::PatientCreate,
            Permission::DoctorRead,
            Permission::AppointmentRead,
            Permission::AppointmentWrite,
            Permission::AppointmentBook,
            Permission::AppointmentCancel,
            Permission::MedicationRead,
            Permission::MedicationWrite,
        ] {
            assert!(Role::Arzt.has_permission(&p), "Arzt fehlt: {}", p);
        }
        // Nicht erlaubt
        for p in [
            Permission::PatientDelete,
            Permission::DoctorWrite,
            Permission::Admin,
            Permission::AuditRead,
        ] {
            assert!(!Role::Arzt.has_permission(&p), "Arzt darf nicht: {}", p);
        }
    }

    #[test]
    fn pflege_matrix() {
        assert!(Role::Pflege.has_permission(&Permission::PatientRead));
        assert!(Role::Pflege.has_permission(&Permission::AppointmentRead));
        assert!(Role::Pflege.has_permission(&Permission::MedicationRead));
        for p in [
            Permission::PatientWrite,
            Permission::PatientCreate,
            Permission::PatientDelete,
            Permission::DoctorRead,
            Permission::DoctorWrite,
            Permission::AppointmentWrite,
            Permission::AppointmentBook,
            Permission::AppointmentCancel,
            Permission::MedicationWrite,
            Permission::Admin,
            Permission::AuditRead,
        ] {
            assert!(!Role::Pflege.has_permission(&p), "Pflege darf nicht: {}", p);
        }
    }

    #[test]
    fn rezeption_matrix() {
        assert!(Role::Rezeption.has_permission(&Permission::PatientRead));
        assert!(Role::Rezeption.has_permission(&Permission::PatientCreate));
        assert!(Role::Rezeption.has_permission(&Permission::AppointmentBook));
        assert!(Role::Rezeption.has_permission(&Permission::AppointmentCancel));
        for p in [
            Permission::PatientWrite,
            Permission::PatientDelete,
            Permission::DoctorRead,
            Permission::DoctorWrite,
            Permission::AppointmentRead,
            Permission::AppointmentWrite,
            Permission::MedicationRead,
            Permission::MedicationWrite,
            Permission::Admin,
            Permission::AuditRead,
        ] {
            assert!(!Role::Rezeption.has_permission(&p), "Rezeption darf nicht: {}", p);
        }
    }

    #[test]
    fn viewer_matrix() {
        assert!(Role::Viewer.has_permission(&Permission::PatientRead));
        assert!(Role::Viewer.has_permission(&Permission::AppointmentRead));
        for p in [
            Permission::PatientWrite,
            Permission::PatientCreate,
            Permission::PatientDelete,
            Permission::DoctorRead,
            Permission::DoctorWrite,
            Permission::AppointmentWrite,
            Permission::AppointmentBook,
            Permission::AppointmentCancel,
            Permission::MedicationRead,
            Permission::MedicationWrite,
            Permission::Admin,
            Permission::AuditRead,
        ] {
            assert!(!Role::Viewer.has_permission(&p), "Viewer darf nicht: {}", p);
        }
    }

    #[test]
    fn serde_roundtrip_lowercase() {
        for role in Role::ALL {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, format!("\"{}\"", role.as_db_string()));
            let parsed: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn fromstr_case_insensitiv() {
        assert_eq!(Role::from_str("admin").unwrap(), Role::Admin);
        assert_eq!(Role::from_str("Arzt").unwrap(), Role::Arzt);
        assert_eq!(Role::from_str("PFLEGE").unwrap(), Role::Pflege);
        assert_eq!(Role::from_str("Rezeption").unwrap(), Role::Rezeption);
        assert_eq!(Role::from_str("viewer").unwrap(), Role::Viewer);
        assert!(Role::from_str("superuser").is_err());
        assert!(Role::from_str("").is_err());
    }

    #[test]
    fn display_liefert_db_wert() {
        assert_eq!(Role::Admin.to_string(), "admin");
        assert_eq!(Role::Arzt.to_string(), "arzt");
        assert_eq!(Role::Pflege.to_string(), "pflege");
        assert_eq!(Role::Rezeption.to_string(), "rezeption");
        assert_eq!(Role::Viewer.to_string(), "viewer");
    }

    /// JWT-Roundtrip: Rolle landet im Claim "role" und kommt zurück.
    #[test]
    fn jwt_roundtrip_role_im_claim() {
        let cfg = SecurityConfig {
            jwt_secret_env: "KIS_JWT_SECRET_TEST".to_string(),
            token_expiry: 900,
            refresh_expiry: 86400,
            max_login_attempts: 5,
            lockout_duration: 300,
            audit: super::super::config::AuditConfig {
                retention_days: 3650,
                log_file: "/tmp/kis-test-audit.log".to_string(),
            },
            rate_limit: super::super::config::RateLimitConfig {
                normal_per_min: 100,
                auth_per_min: 10,
            },
            encryption: super::super::config::EncryptionConfig {
                master_key_env: "KIS_MASTER_KEY_TEST".to_string(),
            },
        };
        std::env::set_var("KIS_JWT_SECRET_TEST", "test-secret-0123456789");
        let master_key_bytes = [0u8; 32]; // 32-Byte AES-256-Key
        let master_key_b64 = B64.encode(&master_key_bytes);
        std::env::set_var("KIS_MASTER_KEY_TEST", &master_key_b64);
        let sec = Security::new(&cfg).unwrap();

        for role in Role::ALL {
            let token = sec.create_access_token("user-123", &role.to_string()).unwrap();
            let (user_id, parsed) = sec.verify_token(&token).unwrap();
            assert_eq!(user_id, "user-123");
            assert_eq!(parsed, role, "Rolle im JWT-Claim erhalten");
        }
    }

    /// Refresh-Token: role-Claim ist Viewer.
    #[test]
    fn refresh_token_role_viewer() {
        let cfg = SecurityConfig {
            jwt_secret_env: "KIS_JWT_SECRET_TEST".to_string(),
            token_expiry: 900,
            refresh_expiry: 86400,
            max_login_attempts: 5,
            lockout_duration: 300,
            audit: super::super::config::AuditConfig {
                retention_days: 3650,
                log_file: "/tmp/kis-test-audit.log".to_string(),
            },
            rate_limit: super::super::config::RateLimitConfig {
                normal_per_min: 100,
                auth_per_min: 10,
            },
            encryption: super::super::config::EncryptionConfig {
                master_key_env: "KIS_MASTER_KEY_TEST".to_string(),
            },
        };
        std::env::set_var("KIS_JWT_SECRET_TEST", "test-secret-0123456789");
        let master_key_bytes = [0u8; 32];
        let master_key_b64 = B64.encode(&master_key_bytes);
        std::env::set_var("KIS_MASTER_KEY_TEST", &master_key_b64);
        let sec = Security::new(&cfg).unwrap();

        let token = sec.create_refresh_token("user-123").unwrap();
        let (user_id, role) = sec.verify_token(&token).unwrap();
        assert_eq!(user_id, "user-123");
        assert_eq!(role, Role::Viewer);
    }
}
