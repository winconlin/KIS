//! Authentifizierung: Login, JWT (Access + Refresh), Argon2-Passwörter,
//! Login-Sperre nach zu vielen Fehlversuchen, MFA (TOTP)-Stub.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::SecurityConfig;
use crate::db::Database;
use super::audit::{AuditAction, AuditLogger};

/// Fehler bei der Authentifizierung
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Benutzername oder Passwort ungültig")]
    InvalidCredentials,
    #[error("Konto gesperrt bis {0}")]
    AccountLocked(String),
    #[error("MFA-Code erforderlich")]
    MfaRequired,
    #[error("MFA-Code ungültig")]
    MfaInvalid,
    #[error("Ungültiges Token")]
    InvalidToken,
    #[error("Token abgelaufen")]
    TokenExpired,
    #[error("Interner Fehler")]
    Internal,
}

impl AuthError {
    fn status(&self) -> StatusCode {
        match self {
            AuthError::InvalidCredentials | AuthError::MfaInvalid => StatusCode::UNAUTHORIZED,
            AuthError::AccountLocked(_) => StatusCode::LOCKED,
            AuthError::MfaRequired => StatusCode::UNAUTHORIZED,
            AuthError::InvalidToken | AuthError::TokenExpired => StatusCode::UNAUTHORIZED,
            AuthError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.to_string(),
            "code": self.code(),
        });
        (self.status(), Json(body)).into_response()
    }
}

impl AuthError {
    /// Fehlercode für JSON-Antworten
    pub fn code(&self) -> &'static str {
        // (siehe oben)
        match self {
            AuthError::InvalidCredentials => "invalid_credentials",
            AuthError::AccountLocked(_) => "account_locked",
            AuthError::MfaRequired => "mfa_required",
            AuthError::MfaInvalid => "mfa_invalid",
            AuthError::InvalidToken => "invalid_token",
            AuthError::TokenExpired => "token_expired",
            AuthError::Internal => "internal",
        }
    }
}

/// Rollen (RBAC)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Doctor,
    Nurse,
    Reception,
}

impl Role {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Self::Admin),
            "doctor" => Some(Self::Doctor),
            "nurse" => Some(Self::Nurse),
            "reception" => Some(Self::Reception),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Doctor => "doctor",
            Self::Nurse => "nurse",
            Self::Reception => "reception",
        }
    }
}

/// JWT-Claims für Access- und Refresh-Tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub iat: i64,
    pub exp: i64,
    /// true für Refresh-Tokens, false für Access-Tokens
    #[serde(default)]
    pub refresh: bool,
    /// JTI — eindeutige Token-ID für Blacklisting
    pub jti: String,
}

/// Auth-Kontext: wird als State in den Router injiziert
#[derive(Clone)]
pub struct AuthContext {
    pub jwt_secret: String,
    pub token_expiry: u64,
    pub refresh_expiry: u64,
    pub max_login_attempts: u32,
    pub lockout_duration: u64,
    pub db: Arc<Database>,
    pub audit: Arc<AuditLogger>,
    /// Blacklist abgelaufener/gekündigter Refresh-Token (JTI -> Ablaufzeitpunkt)
    pub revoked: Arc<tokio::sync::Mutex<std::collections::HashMap<String, i64>>>,
}

impl AuthContext {
    /// Liest das JWT-Secret aus der Umgebung und baut den Auth-Kontext.
    pub async fn new(
        config: &SecurityConfig,
        db: Arc<Database>,
        audit: Arc<AuditLogger>,
    ) -> anyhow::Result<Self> {
        let jwt_secret = std::env::var(&config.jwt_secret_env).map_err(|_| {
            anyhow::anyhow!(
                "JWT-Secret fehlt: Umgebung {env} ist nicht gesetzt",
                env = config.jwt_secret_env
            )
        })?;
        if jwt_secret.len() < 32 {
            anyhow::bail!("JWT-Secret muss mindestens 32 Zeichen lang sein");
        }
        Ok(Self {
            jwt_secret,
            token_expiry: config.token_expiry,
            refresh_expiry: config.refresh_expiry,
            max_login_attempts: config.max_login_attempts,
            lockout_duration: config.lockout_duration,
            db,
            audit,
            revoked: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// Hashed ein Passwort mit Argon2id.
    pub fn hash_password(password: &str) -> Result<String, AuthError> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| AuthError::Internal)?
            .to_string();
        Ok(hash)
    }

    /// Prüft ein Passwort gegen einen Argon2-Hash.
    pub fn verify_password(password: &str, hash: &str) -> Result<bool, AuthError> {
        let parsed = PasswordHash::new(hash).map_err(|_| AuthError::Internal)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }

    /// Erzeugt Access- und Refresh-Token für einen Nutzer.
    pub fn generate_tokens(&self, user_id: &str, username: &str, role: &str) -> Result<(String, String), AuthError> {
        let now = Utc::now().timestamp();
        let access_claims = Claims {
            sub: user_id.to_string(),
            username: username.to_string(),
            role: role.to_string(),
            iat: now,
            exp: now + self.token_expiry as i64,
            refresh: false,
            jti: Uuid::new_v4().to_string(),
        };
        let refresh_claims = Claims {
            sub: user_id.to_string(),
            username: username.to_string(),
            role: role.to_string(),
            iat: now,
            exp: now + self.refresh_expiry as i64,
            refresh: true,
            jti: Uuid::new_v4().to_string(),
        };
        let key = EncodingKey::from_secret(self.jwt_secret.as_bytes());
        let access = encode(&Header::new(Algorithm::HS256), &access_claims, &key)
            .map_err(|_| AuthError::Internal)?;
        let refresh = encode(&Header::new(Algorithm::HS256), &refresh_claims, &key)
            .map_err(|_| AuthError::Internal)?;
        Ok((access, refresh))
    }

    /// Validiert ein JWT und liefert die Claims.
    pub fn verify_token(&self, token: &str) -> Result<Claims, AuthError> {
        // Validation mit Leeway für Clock-Skew (10 Sekunden)
        let mut validation = jsonwebtoken::Validation::new(Algorithm::HS256);
        validation.leeway = 10;
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("expired") {
                AuthError::TokenExpired
            } else {
                AuthError::InvalidToken
            }
        })?;
        Ok(data.claims)
    }

    /// Kündigt einen Refresh-Token (Blacklisting).
    pub async fn revoke_refresh(&self, token: &str) -> Result<(), AuthError> {
        let claims = self.verify_token(token)?;
        if !claims.refresh {
            return Err(AuthError::InvalidToken);
        }
        let mut guard = self.revoked.lock().await;
        guard.insert(claims.jti.clone(), claims.exp);
        info!(jti = %claims.jti, "Refresh-Token gekündigt");
        Ok(())
    }

    /// Prüft, ob ein Refresh-Token gekündigt wurde.
    pub async fn is_revoked(&self, token: &str) -> bool {
        match self.verify_token(token) {
            Ok(claims) if claims.refresh => self
                .revoked
                .lock()
                .await
                .contains_key(&claims.jti),
            _ => true,
        }
    }

    /// Erzeugt neue Tokens aus einem gültigen Refresh-Token.
    pub async fn refresh_tokens(&self, token: &str) -> Result<(String, String), AuthError> {
        let claims = self.verify_token(token)?;
        if !claims.refresh {
            return Err(AuthError::InvalidToken);
        }
        if self.is_revoked(token).await {
            return Err(AuthError::InvalidToken);
        }
        // Altes Refresh-Token kündigen (Rotation)
        let mut guard = self.revoked.lock().await;
        guard.insert(claims.jti.clone(), claims.exp);
        drop(guard);

        let role_str = claims.role.clone();
        self.generate_tokens(&claims.sub, &claims.username, &role_str)
    }

    /// Prüft die Login-Sperre und erhöht den Fehlversuchs-Zähler.
    /// Liefert Ok(true) wenn das Konto gesperrt ist.
    pub async fn check_or_incr_failed(&self, user_id: &str) -> Result<bool, AuthError> {
        let pool = self.db.pool();
        let row: Option<(u32, Option<String>)> = sqlx::query_as(
            "SELECT failed_logins, locked_until FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| AuthError::Internal)?;

        let (_failed, locked_until) = match row {
            Some(r) => r,
            None => return Err(AuthError::InvalidCredentials),
        };

        if let Some(until) = locked_until {
            if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&until) {
                if t.with_timezone(&Utc) > Utc::now() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    async fn register_failed_login(&self, user_id: &str) -> Result<(), AuthError> {
        let pool = self.db.pool();
        let row: Option<(u32,)> = sqlx::query_as("SELECT failed_logins FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| AuthError::Internal)?;

        let failed = row.map(|r| r.0).unwrap_or(0) + 1;
        if failed >= self.max_login_attempts {
            let locked_until =
                (Utc::now() + chrono::Duration::seconds(self.lockout_duration as i64)).to_rfc3339();
            sqlx::query(
                "UPDATE users SET failed_logins = 0, locked_until = ? WHERE id = ?",
            )
            .bind(&locked_until)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(|_| AuthError::Internal)?;
            warn!(user_id, "Konto gesperrt bis {locked_until}");
        } else {
            sqlx::query("UPDATE users SET failed_logins = ? WHERE id = ?")
                .bind(failed)
                .bind(user_id)
                .execute(pool)
                .await
                .map_err(|_| AuthError::Internal)?;
        }
        Ok(())
    }

    async fn reset_failed_logins(&self, user_id: &str) {
        let pool = self.db.pool();
        let _ = sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = ?")
            .bind(user_id)
            .execute(pool)
            .await;
    }

    /// Erzeugt einen TOTP-Geheimnis (Base32) — MFA-Setup.
    pub fn generate_mfa_secret() -> String {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 20];
        rng.fill_bytes(&mut bytes);
        use base64::{engine::general_purpose::URL_SAFE as B32, Engine};
        B32.encode(bytes)
    }

    /// Prüft einen TOTP-Code gegen das Geheimnis (30 s-Periode, ±1 Toleranz).
    /// Implementiert nach RFC 6238, SHA-1 (Standard für TOTP).
    pub fn verify_totp(secret_b32: &str, code: &str) -> bool {
        use base64::{engine::general_purpose::URL_SAFE as B32, Engine};
        let Ok(secret) = B32.decode(secret_b32) else {
            return false;
        };
        let digits = code.trim();
        if digits.len() != 6 {
            return false;
        }
        let counter = (Utc::now().timestamp() / 30) as u64;
        for delta in [counter - 1, counter, counter + 1].iter().copied() {
            if let Some(otp) = Self::totp_at(secret.as_slice(), delta) {
                if otp == digits {
                    return true;
                }
            }
        }
        false
    }

    /// Berechnet den TOTP-Wert für einen Counter (RFC 6238, HMAC-SHA-1, 6 Ziffern).
    fn totp_at(secret: &[u8], counter: u64) -> Option<String> {
        // HMAC-SHA1 ist der Standard für TOTP (RFC 6238)
        use ring::hmac;
        let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
        let counter_bytes = counter.to_be_bytes();
        let mac = hmac::sign(&key, &counter_bytes);
        let bytes = mac.as_ref();
        let offset = (bytes[bytes.len() - 1] & 0x0f) as usize;
        let mut binary = 0i32;
        for i in 0..4 {
            binary = (binary << 8) | (bytes[offset + i] as i32 & 0xff);
        }
        let otp = (binary & 0x7fffffff) % 1_000_000;
        Some(format!("{otp:06}"))
    }
}

/// Login-Request
#[derive(serde::Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Optional: TOTP-Code, wenn MFA aktiviert ist
    #[serde(default)]
    pub mfa_code: Option<String>,
}

/// Login-Antwort
#[derive(serde::Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub role: String,
}

/// Refresh-Request
#[derive(serde::Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Logout-Request
#[derive(serde::Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

/// Login-Handler: POST /api/auth/login
pub async fn login_handler(
    State(auth): State<Arc<AuthContext>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AuthError> {
    let pool = auth.db.pool();

    // Nutzer suchen
    let row: Option<(String, String, String, String, i32)> =
        sqlx::query_as(
            "SELECT id, username, password_hash, role, is_active
             FROM users WHERE username = ?",
        )
        .bind(&req.username)
        .fetch_optional(pool)
        .await
        .map_err(|_| AuthError::Internal)?;

    let (user_id, _username, password_hash, role, is_active) = match row {
        Some(r) => r,
        None => {
            // Zeitangriff-Schutz: trotzdem ein Passwort prüfen
            let _ = Argon2::default().verify_password(
                req.password.as_bytes(),
                &PasswordHash::new("$argon2id$v=19$m=19456,t=2,p=1$ZG9udGNoYW5nZQ$invalid").unwrap(),
            );
            auth.audit.log(None, AuditAction::LoginFailed, "user", None, None, None, Some(&addr.ip().to_string()));
            return Err(AuthError::InvalidCredentials);
        }
    };

    if is_active == 0 {
        auth.audit.log(Some(&user_id), AuditAction::LoginFailed, "user", Some(&user_id), None, None, Some(&addr.ip().to_string()));
        return Err(AuthError::InvalidCredentials);
    }

    // Sperre prüfen
    if auth.check_or_incr_failed(&user_id).await? {
        auth.audit.log(Some(&user_id), AuditAction::Lockout, "user", Some(&user_id), None, None, Some(&addr.ip().to_string()));
        let until = sqlx::query_scalar::<_, String>(
            "SELECT locked_until FROM users WHERE id = ?",
        )
        .bind(&user_id)
        .fetch_one(pool)
        .await
        .unwrap_or_default();
        return Err(AuthError::AccountLocked(until));
    }

    // Passwort prüfen
    if !AuthContext::verify_password(&req.password, &password_hash)? {
        auth.register_failed_login(&user_id).await?;
        auth.audit.log(Some(&user_id), AuditAction::LoginFailed, "user", Some(&user_id), None, None, Some(&addr.ip().to_string()));
        return Err(AuthError::InvalidCredentials);
    }

    // MFA-Prüfung (wenn aktiviert)
    let mfa_secret: Option<String> =
        sqlx::query_scalar("SELECT mfa_secret FROM users WHERE id = ?")
            .bind(&user_id)
            .fetch_one(pool)
            .await
            .map_err(|_| AuthError::Internal)?;

    if let Some(secret) = mfa_secret {
        if !secret.is_empty() {
            match &req.mfa_code {
                None => {
                    auth.audit.log(Some(&user_id), AuditAction::Mfa, "user", Some(&user_id), None, None, Some(&addr.ip().to_string()));
                    return Err(AuthError::MfaRequired);
                }
                Some(code) => {
                    if !AuthContext::verify_totp(&secret, code) {
                        auth.audit.log(Some(&user_id), AuditAction::Mfa, "user", Some(&user_id), None, None, Some(&addr.ip().to_string()));
                        return Err(AuthError::MfaInvalid);
                    }
                }
            }
        }
    }

    // Erfolg: Zähler zurücksetzen, Tokens ausstellen
    auth.reset_failed_logins(&user_id).await;
    let (access, refresh) = auth.generate_tokens(&user_id, &req.username, &role)?;
    auth.audit.log(Some(&user_id), AuditAction::Login, "user", Some(&user_id), None, None, Some(&addr.ip().to_string()));
    info!(user_id = %user_id, role, "Login erfolgreich");

    sqlx::query("UPDATE users SET last_login = datetime('now') WHERE id = ?")
        .bind(&user_id)
        .execute(pool)
        .await
        .ok();

    Ok(Json(LoginResponse {
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer".into(),
        expires_in: auth.token_expiry,
        role,
    }))
}

/// Refresh-Handler: POST /api/auth/refresh
pub async fn refresh_handler(
    State(auth): State<Arc<AuthContext>>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<LoginResponse>, AuthError> {
    let (access, refresh) = auth.refresh_tokens(&req.refresh_token).await?;
    let claims = auth.verify_token(&access)?;
    auth.audit.log(Some(&claims.sub), AuditAction::Refresh, "user", Some(&claims.sub), None, None, None);
    Ok(Json(LoginResponse {
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer".into(),
        expires_in: auth.token_expiry,
        role: claims.role,
    }))
}

/// Logout-Handler: POST /api/auth/logout (kündigt den Refresh-Token)
pub async fn logout_handler(
    State(auth): State<Arc<AuthContext>>,
    Json(req): Json<LogoutRequest>,
) -> Result<StatusCode, AuthError> {
    auth.revoke_refresh(&req.refresh_token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// MFA-Setup-Handler: POST /api/auth/mfa/setup (liefert Geheimnis + QR-URI)
pub async fn mfa_setup_handler(State(_auth): State<Arc<AuthContext>>) -> Result<Json<serde_json::Value>, AuthError> {
    let secret = AuthContext::generate_mfa_secret();
    // URI für TOTP-Apps (Google Authenticator & Co.)
    let otpauth = format!(
        "otpauth://totp/KIS:user?secret={secret}&issuer=KIS&algorithm=SHA1&digits=6&period=30"
    );
    // TODO: secret im DB-Speicher ablegen (muss vor Aktivierung geschehen)
    Ok(Json(serde_json::json!({
        "secret": secret,
        "otpauth_url": otpauth,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = AuthContext::hash_password("secret123").unwrap();
        assert!(AuthContext::verify_password("secret123", &hash).unwrap());
        assert!(!AuthContext::verify_password("wrong", &hash).unwrap());
    }

    #[test]
    fn totp_verify() {
        // Fester Secret für Tests
        let secret = base64::engine::general_purpose::ALPHA.encode(b"12345678901234567890");
        // Erzeugt den aktuellen Code und prüft ihn
        let counter = (Utc::now().timestamp() / 30) as u64;
        let code = AuthContext::totp_at(b"12345678901234567890", counter).unwrap();
        assert!(AuthContext::verify_totp(&secret, &code));
    }

    #[test]
    fn token_roundtrip() {
        // Manuell gebauten Kontext ohne DB (nur für Token-Tests)
        use std::sync::Arc;
        let ctx = crate::security::auth::AuthContext {
            jwt_secret: "test-secret-key-min-32-chars-long!!".into(),
            token_expiry: 900,
            refresh_expiry: 86400,
            max_login_attempts: 5,
            lockout_duration: 900,
            db: Arc::new(crate::db::Database::empty_for_tests()),
            audit: Arc::new(crate::security::audit::tests::dummy_logger()),
            revoked: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        };
        let (access, refresh) = ctx.generate_tokens("u1", "alice", "doctor").unwrap();
        let claims = ctx.verify_token(&access).unwrap();
        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.role, "doctor");
        assert!(!claims.refresh);
        let rclaims = ctx.verify_token(&refresh).unwrap();
        assert!(rclaims.refresh);
    }
}
