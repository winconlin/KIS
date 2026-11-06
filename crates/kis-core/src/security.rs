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
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

use super::config::SecurityConfig;

/// Security-Kontext: Wird in alle Handler injiziert
pub struct Security {
    jwt_secret: String,
    aes_key: [u8; 32],
    config: SecurityConfig,
}

impl Security {
    /// Mindestlänge des JWT-Secrets in Bytes (HS256: Schlüssel >= Hashlänge).
    pub const MIN_JWT_SECRET_LEN: usize = 32;

    /// Initialisiert das Security-Subsystem.
    ///
    /// Es gibt bewusst **keinen** Fallback auf einen Dev-Schlüssel: Fehlen
    /// JWT-Secret oder Master-Key oder sind sie ungültig, bricht der Start ab.
    pub fn new(config: &SecurityConfig) -> anyhow::Result<Self> {
        let jwt_secret = std::env::var(&config.jwt_secret_env).map_err(|_| {
            anyhow::anyhow!(
                "{} ist nicht gesetzt (JWT-Secret erforderlich)",
                config.jwt_secret_env
            )
        })?;
        let master_key = std::env::var(&config.encryption.master_key_env).map_err(|_| {
            anyhow::anyhow!(
                "{} ist nicht gesetzt (Base64-codierter 32-Byte-Master-Key erforderlich)",
                config.encryption.master_key_env
            )
        })?;
        Self::from_secrets(config, &jwt_secret, &master_key)
    }

    /// Initialisiert mit expliziten Secrets (z. B. für Tests oder
    /// Secret-Manager-Integration). Gleiche Prüfungen wie [`Security::new`].
    pub fn from_secrets(
        config: &SecurityConfig,
        jwt_secret: &str,
        master_key_b64: &str,
    ) -> anyhow::Result<Self> {
        if jwt_secret.len() < Self::MIN_JWT_SECRET_LEN {
            anyhow::bail!(
                "JWT-Secret ist zu kurz ({} Bytes, mindestens {} erforderlich)",
                jwt_secret.len(),
                Self::MIN_JWT_SECRET_LEN
            );
        }
        let aes_key_bytes = B64
            .decode(master_key_b64.trim())
            .map_err(|_| anyhow::anyhow!("Master-Key ist kein gültiges Base64"))?;
        let aes_key: [u8; 32] = aes_key_bytes.as_slice().try_into().map_err(|_| {
            anyhow::anyhow!(
                "Master-Key muss genau 32 Bytes lang sein (ist {})",
                aes_key_bytes.len()
            )
        })?;
        if aes_key.iter().all(|b| *b == 0) {
            anyhow::bail!("Master-Key besteht nur aus Nullbytes");
        }

        info!("Security initialisiert");
        Ok(Self {
            jwt_secret: jwt_secret.to_string(),
            aes_key,
            config: config.clone(),
        })
    }

    /// Erzeugt einen (nicht sitzungsgebundenen) Access-Token.
    pub fn create_access_token(&self, user_id: &str, role: &str) -> anyhow::Result<String> {
        Ok(self.issue_token(user_id, role, TokenType::Access, None)?.0)
    }

    /// Erzeugt einen Refresh-Token. Refresh-Tokens werden von
    /// [`Security::verify_token`] nicht als Zugriffstoken akzeptiert.
    pub fn create_refresh_token(&self, user_id: &str) -> anyhow::Result<String> {
        Ok(self
            .issue_token(user_id, "viewer", TokenType::Refresh, None)?
            .0)
    }

    /// Erzeugt einen signierten Token und liefert `(token, jti)`.
    ///
    /// Unbekannte Rollen werden abgelehnt (kein stilles Herabstufen).
    pub fn issue_token(
        &self,
        user_id: &str,
        role: &str,
        typ: TokenType,
        session_id: Option<&str>,
    ) -> anyhow::Result<(String, String)> {
        let role = Role::from_str(role).map_err(|e| anyhow::anyhow!(e))?;
        let now = chrono::Utc::now();
        let ttl = match typ {
            TokenType::Access => self.config.token_expiry as i64,
            TokenType::Refresh => self.config.refresh_expiry as i64,
        };
        let jti = uuid::Uuid::new_v4().to_string();
        let claims = TokenClaims {
            sub: user_id.to_string(),
            role,
            iat: now.timestamp(),
            exp: now.timestamp() + ttl,
            jti: jti.clone(),
            typ,
            sid: session_id.map(str::to_string),
        };
        let token = encode(
            &Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )?;
        Ok((token, jti))
    }

    /// Dekodiert und prüft Signatur, Ablauf, Algorithmus und Token-Typ.
    pub fn decode_token(&self, token: &str, expected: TokenType) -> anyhow::Result<TokenClaims> {
        let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.leeway = 0;
        let data = decode::<TokenClaims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        )?;
        if data.claims.typ != expected {
            anyhow::bail!("falscher Token-Typ");
        }
        Ok(data.claims)
    }

    /// Refresh-Token-TTL
    pub fn refresh_token_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.refresh_expiry)
    }

    /// Maximale Fehlversuche vor Sperre
    pub fn max_login_attempts(&self) -> u32 {
        self.config.max_login_attempts
    }

    /// Sperrdauer nach zu vielen Fehlversuchen
    pub fn lockout_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.lockout_duration)
    }

    /// Access-Token-TTL in Sekunden
    pub fn access_token_ttl(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.config.token_expiry)
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
    #[allow(clippy::too_many_arguments)]
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
        let mut tx = db
            .begin()
            .await
            .map_err(|e| anyhow::anyhow!("Audit-Transaktion fehlgeschlagen: {e}"))?;
        append_audit(
            &mut tx,
            user_id,
            action,
            entity_type,
            entity_id,
            old_data,
            new_data,
            ip_address,
        )
        .await?;
        tx.commit()
            .await
            .map_err(|e| anyhow::anyhow!("Audit-Commit fehlgeschlagen: {e}"))?;
        Ok(())
    }

    /// Wie [`Security::log_audit`], aber innerhalb einer bestehenden
    /// Transaktion. Die fachliche Änderung und ihr Audit-Eintrag werden
    /// damit gemeinsam committet oder gemeinsam verworfen.
    #[allow(clippy::too_many_arguments)]
    pub async fn log_audit_tx(
        &self,
        conn: &mut sqlx::PgConnection,
        user_id: &str,
        action: &str,
        entity_type: &str,
        entity_id: Option<&str>,
        old_data: Option<&serde_json::Value>,
        new_data: Option<&serde_json::Value>,
        ip_address: Option<&str>,
    ) -> anyhow::Result<()> {
        append_audit(
            conn,
            user_id,
            action,
            entity_type,
            entity_id,
            old_data,
            new_data,
            ip_address,
        )
        .await
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
        const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

        #[derive(sqlx::FromRow)]
        struct ChainRow {
            #[allow(dead_code)]
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
            let computed = audit_hash_v1(
                &row.prev_hash,
                row.user_id.as_deref().unwrap_or(""),
                &row.action,
                &row.timestamp,
                &row.entity_type,
                row.entity_id.as_deref(),
                row.old_data.as_ref(),
                row.new_data.as_ref(),
            );

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
        let rows: Vec<serde_json::Value> = sqlx::query_scalar(
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
        // Nur Access-Tokens; Widerruf/Kontostatus prüft `crate::auth`.
        let claims = self.decode_token(token, TokenType::Access)?;
        Ok((claims.sub, claims.role))
    }

    /// Verifiziert ein Passwort gegen Argon2id-Hash
    pub fn verify_password(&self, password: &str, hash: &str) -> bool {
        use argon2::password_hash::PasswordVerifier;
        use argon2::Argon2;

        match argon2::password_hash::PasswordHash::new(hash) {
            Ok(parsed_hash) => Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok(),
            Err(_) => false,
        }
    }

    /// Hash ein Passwort mit Argon2id
    pub fn hash_password(&self, password: &str) -> anyhow::Result<String> {
        use argon2::password_hash::rand_core::OsRng;
        use argon2::password_hash::{PasswordHasher, SaltString};
        use argon2::Argon2;

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
    pub fn is_locked(
        &self,
        attempts: u32,
        locked_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> bool {
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
    /// Zweck des Tokens; Refresh-Tokens werden an Datenrouten abgelehnt.
    pub typ: TokenType,
    /// Sitzungs-ID (Zeile in `sessions`), falls sitzungsgebunden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
}

/// Zweck eines JWT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    Access,
    Refresh,
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
            typ: TokenType::Access,
            sid: None,
        }
    }
}

/// RBAC Rollen (Task 1.8)
///
/// Serde: lowercase-Strings ("admin", "arzt", "pflege", "rezeption", "viewer").
/// `FromStr` ist case-insensitiv; `Display` liefert den lowercase-String,
/// der in der users-Tabelle und im JWT-Claim gespeichert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    /// Auditor: Nur Audit-Log lesen (DSGVO-konforme Prüfungen).
    Auditor,
    /// LLM-Agent: Read-only Zugang für KI-Assistenzfunktionen.
    LlmAgent,
}

impl Role {
    /// Alle definierten Rollen.
    pub const ALL: [Role; 7] = [
        Role::Admin,
        Role::Arzt,
        Role::Pflege,
        Role::Rezeption,
        Role::Viewer,
        Role::Auditor,
        Role::LlmAgent,
    ];

    /// DB-Wert der Rolle (users.role)
    pub fn as_db_string(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Arzt => "arzt",
            Role::Pflege => "pflege",
            Role::Rezeption => "rezeption",
            Role::Viewer => "viewer",
            Role::Auditor => "auditor",
            Role::LlmAgent => "llm_agent",
        }
    }

    /// Prüft, ob eine Rolle eine bestimmte Berechtigung hat.
    ///
    /// Berechtigungs-Matrix (Task 1.8 / 5.4):
    /// Vollständige Matrix siehe `docs/SECURITY.md` § RBAC.
    ///
    /// Kurzform:
    /// ```text
    ///                          patient.read patient.write patient.create patient.delete doctor.read doctor.write appt.read appt.write appt.book appt.cancel med.read med.write doc.read doc.write export import audit.read config.read admin
    /// Admin                     ✔           ✔             ✔              ✔            ✔           ✔          ✔         ✔         ✔         ✔         ✔        ✔       ✔        ✔       ✔      ✔      ✔        ✔         ✔
    /// Arzt (Physician)          ✔           ✔             ✔              ✘            ✔           ✘          ✔         ✔         ✔         ✔         ✔        ✔       ✔        ✘       ✘      ✘      ✘        ✘         ✘
    /// Pflege (Nurse)            ✔           ✘             ✘              ✘            ✘           ✘          ✔         ✘         ✘         ✘         ✔        ✘       ✘        ✘       ✘      ✘      ✘        ✘         ✘
    /// Rezeption (Admin)         ✔           ✘             ✔              ✘            ✘           ✘          ✔         ✘         ✔         ✔         ✘        ✘       ✘        ✘       ✘      ✘      ✘        ✘         ✘
    /// Viewer                    ✔           ✘             ✘              ✘            ✘           ✘          ✔         ✘         ✘         ✘         ✘        ✘       ✘        ✘       ✘      ✘      ✘        ✘         ✘
    /// Auditor                   ✘           ✘             ✘              ✘            ✘           ✘          ✘         ✘         ✘         ✘         ✘        ✘       ✘        ✘       ✘      ✘      ✔        ✘         ✘
    /// LLM-Agent                 ✔           ✘             ✘              ✘            ✘           ✘          ✔         ✘         ✘         ✘         ✔        ✘       ✔        ✘       ✘      ✘      ✘        ✘         ✘
    /// ```
    pub fn has_permission(&self, permission: &Permission) -> bool {
        match (self, permission) {
            // Admin: alles
            (Role::Admin, _) => true,

            // Arzt: patients (read/write/create/delete), doctors (read),
            //       appointments (read/write/book/cancel), medications (read/write),
            //       documents (read)
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
            (Role::Arzt, Permission::DocumentRead) => true,

            // Pflege: patients (read), appointments (read), medications (read)
            (Role::Pflege, Permission::PatientRead) => true,
            (Role::Pflege, Permission::AppointmentRead) => true,
            (Role::Pflege, Permission::MedicationRead) => true,

            // Rezeption: patients (read/create), appointments (book/cancel)
            (Role::Rezeption, Permission::PatientRead) => true,
            (Role::Rezeption, Permission::PatientCreate) => true,
            (Role::Rezeption, Permission::AppointmentBook) => true,
            (Role::Rezeption, Permission::AppointmentCancel) => true,

            // Viewer: patients (read), appointments (read)
            (Role::Viewer, Permission::PatientRead) => true,
            (Role::Viewer, Permission::AppointmentRead) => true,

            // Auditor: nur Audit-Log lesen
            (Role::Auditor, Permission::AuditRead) => true,

            // LLM-Agent: read-only Zugang zu Patienten, Terminen, Medikamenten, Dokumenten
            // (für Assistenzfunktionen, keine Schreibrechte)
            (Role::LlmAgent, Permission::PatientRead) => true,
            (Role::LlmAgent, Permission::AppointmentRead) => true,
            (Role::LlmAgent, Permission::MedicationRead) => true,
            (Role::LlmAgent, Permission::DocumentRead) => true,

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
            "auditor" => Ok(Role::Auditor),
            "llm_agent" => Ok(Role::LlmAgent),
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
    /// Dokument lesen (Patienten-Dokumente)
    DocumentRead,
    /// Dokument schreiben (Patienten-Dokumente)
    DocumentWrite,
    /// Export: Datenexport für Patienten (DSGVO Art. 20)
    Export,
    /// Import: Datenimport
    Import,
    /// Konfiguration lesen
    ConfigRead,
    /// Konfiguration schreiben (nur Admin)
    ConfigWrite,
    /// Audit-Log lesen
    AuditRead,
    /// Administrations-Endpunkte (User-Verwaltung)
    Admin,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Permission::PatientRead => "patient.read",
            Permission::PatientWrite => "patient.write",
            Permission::PatientCreate => "patient.create",
            Permission::PatientDelete => "patient.delete",
            Permission::DocumentRead => "document.read",
            Permission::DocumentWrite => "document.write",
            Permission::DoctorRead => "doctor.read",
            Permission::DoctorWrite => "doctor.write",
            Permission::AppointmentRead => "appointment.read",
            Permission::AppointmentWrite => "appointment.write",
            Permission::AppointmentBook => "appointment.book",
            Permission::AppointmentCancel => "appointment.cancel",
            Permission::MedicationRead => "medication.read",
            Permission::MedicationWrite => "medication.write",
            Permission::Export => "data.export",
            Permission::Import => "data.import",
            Permission::ConfigRead => "config.read",
            Permission::ConfigWrite => "config.write",
            Permission::AuditRead => "audit.read",
            Permission::Admin => "admin",
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
    /// Prüft nur Signatur, Ablauf und Token-Typ — **ohne** Sitzungs- und
    /// Kontostatus. Für Anfragen ist [`crate::auth::validate_access`] zu
    /// verwenden.
    pub fn authenticate(&self, token: &str) -> anyhow::Result<AuthUser> {
        let (user_id, role) = self.security.verify_token(token)?;
        Ok(AuthUser {
            user_id,
            role,
            session_id: None,
        })
    }
}

/// Authentifizierter User: user_id + Rolle (aus dem JWT).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub role: Role,
    /// Sitzung, aus der die Anfrage stammt (bei sitzungsgebundenen Tokens).
    pub session_id: Option<uuid::Uuid>,
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

/// Liefert den von der Authentifizierungs-Middleware geprüften Benutzer.
///
/// Der Extractor verifiziert selbst kein Token: Er akzeptiert nur einen
/// [`AuthUser`], den die zentrale Middleware nach vollständiger Prüfung
/// (Signatur, Token-Typ, Sitzung, Kontostatus) in die Request-Extensions
/// gelegt hat. Fehlt dieser, wird mit 401 abgelehnt.
#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireRole {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth = parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(|| unauthorized_response("Nicht authentifiziert"))?;
        Ok(RequireRole {
            allowed: Vec::new(),
            auth,
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
#[allow(clippy::result_large_err)]
pub fn guard_roles(
    auth: &RequireRole,
    allowed: impl IntoIterator<Item = Role>,
) -> Result<(), Response> {
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

pub fn unauthorized_response(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", "Bearer")],
        axum::Json(serde_json::json!({ "error": msg, "status": 401 })),
    )
        .into_response()
}

pub fn forbidden_response(msg: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({ "error": msg, "status": 403 })),
    )
        .into_response()
}

/// Hex-Encodierung eines SHA-256-Hashes.
/// Genesis-Wert der Audit-Hash-Kette.
pub const AUDIT_GENESIS_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Schlüssel der transaktionsgebundenen Advisory-Lock, mit der alle
/// Audit-Schreiber serialisiert werden (`'KISAUDIT'` als i64).
const AUDIT_CHAIN_LOCK_KEY: i64 = 0x4b49_5341_5544_4954;

/// Hängt einen Eintrag an die Audit-Hash-Kette an.
///
/// Läuft auf der übergebenen Verbindung bzw. Transaktion. Eine
/// transaktionsgebundene Advisory-Lock serialisiert parallele Schreiber,
/// damit zwei Einträge nie denselben Vorgänger verwenden. Die Lock wird mit
/// dem Commit oder Rollback der umgebenden Transaktion freigegeben. Ohne
/// umgebende Transaktion wäre die Lock wirkungslos — deshalb nur über
/// [`Security::log_audit`] oder [`Security::log_audit_tx`] verwenden.
#[allow(clippy::too_many_arguments)]
pub async fn append_audit(
    conn: &mut sqlx::PgConnection,
    user_id: &str,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    old_data: Option<&serde_json::Value>,
    new_data: Option<&serde_json::Value>,
    ip_address: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AUDIT_CHAIN_LOCK_KEY)
        .execute(&mut *conn)
        .await
        .map_err(|e| anyhow::anyhow!("Audit-Lock fehlgeschlagen: {e}"))?;

    let last =
        sqlx::query_scalar::<_, String>("SELECT hash FROM audit_logs ORDER BY id DESC LIMIT 1")
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| anyhow::anyhow!("Audit-Chain-Read fehlgeschlagen: {e}"))?;
    let prev_hash = last.unwrap_or_else(|| AUDIT_GENESIS_HASH.to_string());

    // PostgreSQL speichert TIMESTAMPTZ mit Mikrosekunden. Der Hash muss
    // über exakt den gespeicherten Wert gebildet werden, sonst schlägt die
    // spätere Verifikation fehl.
    let now = truncate_to_micros(chrono::Utc::now());
    let hash = audit_hash_v1(
        &prev_hash,
        user_id,
        action,
        &now,
        entity_type,
        entity_id,
        old_data,
        new_data,
    );

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
    .bind(old_data)
    .bind(new_data)
    .bind(ip_address)
    .bind(now)
    .bind(&prev_hash)
    .bind(&hash)
    .execute(&mut *conn)
    .await
    .map_err(|e| anyhow::anyhow!("Audit-Insert fehlgeschlagen: {e}"))?;

    tracing::debug!(user_id, action, entity_type, %hash, "Audit-Eintrag geschrieben");
    Ok(())
}

fn truncate_to_micros(ts: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    use chrono::Timelike;
    let micros = ts.nanosecond() / 1_000 * 1_000;
    ts.with_nanosecond(micros).unwrap_or(ts)
}

/// Berechnet den Kettenhash eines Audit-Eintrags.
///
/// Jedes Feld wird mit seiner Byte-Länge präfixiert. Damit können Felder,
/// die selbst `|` enthalten, keine Kollision mit anders aufgeteilten
/// Eingaben erzeugen.
#[allow(clippy::too_many_arguments)]
pub fn audit_hash_v1(
    prev_hash: &str,
    user_id: &str,
    action: &str,
    timestamp: &chrono::DateTime<chrono::Utc>,
    entity_type: &str,
    entity_id: Option<&str>,
    old_data: Option<&serde_json::Value>,
    new_data: Option<&serde_json::Value>,
) -> String {
    let ts = timestamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let old_str = old_data
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".into());
    let new_str = new_data
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".into());
    let mut hasher = Sha256::new();
    hasher.update(b"kis-audit-v1");
    for field in [
        prev_hash,
        user_id,
        action,
        ts.as_str(),
        entity_type,
        entity_id.unwrap_or(""),
        old_str.as_str(),
        new_str.as_str(),
    ] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    hex_sha256(&hasher.finalize())
}

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
            assert!(
                !Role::Rezeption.has_permission(&p),
                "Rezeption darf nicht: {}",
                p
            );
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
            tls: Default::default(),
        };
        std::env::set_var(
            "KIS_JWT_SECRET_TEST",
            "test-secret-0123456789-0123456789-abc",
        );
        let master_key_bytes = [7u8; 32]; // 32-Byte AES-256-Key
        let master_key_b64 = B64.encode(master_key_bytes);
        std::env::set_var("KIS_MASTER_KEY_TEST", &master_key_b64);
        let sec = Security::new(&cfg).unwrap();

        for role in Role::ALL {
            let token = sec
                .create_access_token("user-123", &role.to_string())
                .unwrap();
            let (user_id, parsed) = sec.verify_token(&token).unwrap();
            assert_eq!(user_id, "user-123");
            assert_eq!(parsed, role, "Rolle im JWT-Claim erhalten");
        }
    }

    /// Refresh-Token: role-Claim ist Viewer.
    #[test]
    fn refresh_token_is_not_an_access_token() {
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
            tls: Default::default(),
        };
        std::env::set_var(
            "KIS_JWT_SECRET_TEST",
            "test-secret-0123456789-0123456789-abc",
        );
        let master_key_bytes = [7u8; 32];
        let master_key_b64 = B64.encode(master_key_bytes);
        std::env::set_var("KIS_MASTER_KEY_TEST", &master_key_b64);
        let sec = Security::new(&cfg).unwrap();

        let token = sec.create_refresh_token("user-123").unwrap();
        // Ein Refresh-Token darf niemals als Access-Token akzeptiert werden.
        assert!(sec.verify_token(&token).is_err());
        let claims = sec.decode_token(&token, TokenType::Refresh).unwrap();
        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.typ, TokenType::Refresh);
    }
}
