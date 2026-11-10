//! Kryptografie und Token: JWT (HS256), AES-256-GCM, Argon2id sowie die
//! Prüfung der Audit-Kette.
//!
//! - Kein Fallback auf Entwicklungsschlüssel: fehlende oder zu schwache
//!   Schlüssel brechen den Start ab.
//! - Token tragen einen Zweck (`access`/`refresh`) und eine Sitzungs-ID.
//!   Ob ein Token gilt und welche Rechte es hat, entscheidet nicht das
//!   Token, sondern die Datenbank ([`crate::auth::validate_access`]).
//! - Nur bewährte Bibliotheken; keine eigenen Verfahren.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
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

    /// Erzeugt einen signierten Token und liefert `(token, jti)`.
    ///
    /// Die Rolle im Token dient nur der Anzeige. Rechte werden bei jeder
    /// Anfrage aus der Datenbank bestimmt ([`crate::auth::validate_access`]).
    pub fn issue_token(
        &self,
        user_id: &str,
        role: &str,
        typ: TokenType,
        session_id: Option<&str>,
    ) -> anyhow::Result<(String, String)> {
        let now = chrono::Utc::now();
        let ttl = match typ {
            TokenType::Access => self.config.token_expiry as i64,
            TokenType::Refresh => self.config.refresh_expiry as i64,
        };
        let jti = uuid::Uuid::new_v4().to_string();
        let claims = TokenClaims {
            sub: user_id.to_string(),
            role: role.to_string(),
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
    pub role: String,
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

pub use crate::audit::{append_audit, audit_hash_v1, AUDIT_GENESIS_HASH};
pub use crate::identity::{forbidden_response, unauthorized_response, AuthUser, RequireRole};
pub use crate::rbac::Permission;

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER_KEY: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";
    const JWT_SECRET: &str = "test-secret-0123456789-0123456789-abc";

    fn security() -> Security {
        Security::from_secrets(&SecurityConfig::default(), JWT_SECRET, MASTER_KEY).unwrap()
    }

    #[test]
    fn access_token_roundtrip() {
        let sec = security();
        let (token, jti) = sec
            .issue_token("user-123", "arzt", TokenType::Access, Some("sid-1"))
            .unwrap();
        let claims = sec.decode_token(&token, TokenType::Access).unwrap();
        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.role, "arzt");
        assert_eq!(claims.jti, jti);
        assert_eq!(claims.sid.as_deref(), Some("sid-1"));
    }

    /// Ein Refresh-Token darf niemals als Access-Token akzeptiert werden
    /// und umgekehrt.
    #[test]
    fn token_types_are_not_interchangeable() {
        let sec = security();
        let (refresh, _) = sec
            .issue_token("user-123", "arzt", TokenType::Refresh, None)
            .unwrap();
        let (access, _) = sec
            .issue_token("user-123", "arzt", TokenType::Access, None)
            .unwrap();
        assert!(sec.decode_token(&refresh, TokenType::Access).is_err());
        assert!(sec.decode_token(&access, TokenType::Refresh).is_err());
        assert!(sec.decode_token(&refresh, TokenType::Refresh).is_ok());
    }

    #[test]
    fn foreign_signature_is_rejected() {
        let other = Security::from_secrets(
            &SecurityConfig::default(),
            "ein-ganz-anderes-geheimnis-0123456789abcdef",
            MASTER_KEY,
        )
        .unwrap();
        let (token, _) = other
            .issue_token("u", "admin", TokenType::Access, None)
            .unwrap();
        assert!(security().decode_token(&token, TokenType::Access).is_err());
    }

    #[test]
    fn weak_or_missing_secrets_are_rejected() {
        let cfg = SecurityConfig::default();
        assert!(Security::from_secrets(&cfg, "short", MASTER_KEY).is_err());
        assert!(Security::from_secrets(&cfg, JWT_SECRET, "AAAA").is_err());
        assert!(Security::from_secrets(
            &cfg,
            JWT_SECRET,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        )
        .is_err());
    }

    #[test]
    fn encryption_roundtrip_and_password_hash() {
        let sec = security();
        let ct = sec.encrypt(b"geheim").unwrap();
        assert_ne!(ct, "geheim");
        assert_eq!(sec.decrypt(&ct).unwrap(), b"geheim");
        let hash = sec.hash_password("correct horse").unwrap();
        assert!(sec.verify_password("correct horse", &hash));
        assert!(!sec.verify_password("falsch", &hash));
    }
}
