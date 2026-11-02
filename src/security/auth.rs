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
    Extension,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::info;
use uuid::Uuid;

use super::audit::{AuditAction, AuditLogger};
use crate::config::SecurityConfig;
use crate::db::Database;

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
        super::middleware::json_error(self.status(), self.code(), &self.to_string())
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
    pub fn parse(s: &str) -> Option<Self> {
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
    dummy_password_hash: String,
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
        Self::from_secret(config, db, audit, jwt_secret)
    }

    pub fn from_secret(
        config: &SecurityConfig,
        db: Arc<Database>,
        audit: Arc<AuditLogger>,
        jwt_secret: String,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            jwt_secret.len() >= 32,
            "JWT-Secret muss mindestens 32 Bytes lang sein"
        );
        Ok(Self {
            jwt_secret,
            token_expiry: config.token_expiry,
            refresh_expiry: config.refresh_expiry,
            max_login_attempts: config.max_login_attempts,
            lockout_duration: config.lockout_duration,
            db,
            audit,
            dummy_password_hash: Self::hash_password("dummy-account-password")
                .map_err(|_| anyhow::anyhow!("Passwort-Initialisierung fehlgeschlagen"))?,
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
    pub fn generate_tokens(
        &self,
        user_id: &str,
        username: &str,
        role: &str,
    ) -> Result<(String, String), AuthError> {
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

    /// Signatur, Token-Zweck, persistente Sitzung und aktuelles Benutzerkonto prüfen.
    pub async fn verify_access_token(&self, token: &str) -> Result<Claims, AuthError> {
        let claims = self.verify_token(token)?;
        if claims.refresh {
            return Err(AuthError::InvalidToken);
        }
        let role: Option<String> = sqlx::query_scalar(
            "SELECT u.role FROM sessions s JOIN users u ON u.id = s.user_id
            WHERE s.access_jti = ? AND s.user_id = ? AND s.revoked_at IS NULL
            AND u.is_active = 1 AND u.deleted_at IS NULL",
        )
        .bind(&claims.jti)
        .bind(&claims.sub)
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        if role.as_deref() != Some(claims.role.as_str()) || Role::parse(&claims.role).is_none() {
            return Err(AuthError::InvalidToken);
        }
        Ok(claims)
    }

    async fn store_session(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        access: &str,
        refresh: &str,
    ) -> Result<(), AuthError> {
        let access = self.verify_token(access)?;
        let refresh = self.verify_token(refresh)?;
        let expires =
            chrono::DateTime::<Utc>::from_timestamp(refresh.exp, 0).ok_or(AuthError::Internal)?;
        sqlx::query("INSERT INTO sessions (id, user_id, jti, access_jti, issued_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(Uuid::new_v4().to_string()).bind(&refresh.sub).bind(&refresh.jti).bind(&access.jti)
            .bind(Utc::now().to_rfc3339()).bind(expires.to_rfc3339()).execute(&mut **tx).await
            .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    pub async fn issue_tokens(
        &self,
        user_id: &str,
        username: &str,
        role: &str,
    ) -> Result<(String, String), AuthError> {
        let tokens = self.generate_tokens(user_id, username, role)?;
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|_| AuthError::Internal)?;
        self.store_session(&mut tx, &tokens.0, &tokens.1).await?;
        tx.commit().await.map_err(|_| AuthError::Internal)?;
        Ok(tokens)
    }

    /// Beide Tokens der Sitzung widerrufen; der Zustand überlebt Neustarts.
    pub async fn revoke_refresh(&self, token: &str) -> Result<(), AuthError> {
        let claims = self.verify_token(token)?;
        if !claims.refresh {
            return Err(AuthError::InvalidToken);
        }
        sqlx::query("UPDATE sessions SET revoked_at = ? WHERE jti = ? AND user_id = ? AND revoked_at IS NULL")
            .bind(Utc::now().to_rfc3339()).bind(&claims.jti).bind(&claims.sub)
            .execute(self.db.pool()).await.map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    /// Atomare Rotation: nur ein Request kann die alte Sitzung verbrauchen.
    pub async fn refresh_tokens(&self, token: &str) -> Result<(String, String), AuthError> {
        let claims = self.verify_token(token)?;
        if !claims.refresh {
            return Err(AuthError::InvalidToken);
        }
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|_| AuthError::Internal)?;
        let updated = sqlx::query("UPDATE sessions SET revoked_at = ? WHERE jti = ? AND user_id = ? AND revoked_at IS NULL")
            .bind(Utc::now().to_rfc3339()).bind(&claims.jti).bind(&claims.sub)
            .execute(&mut *tx).await.map_err(|_| AuthError::Internal)?;
        if updated.rows_affected() != 1 {
            return Err(AuthError::InvalidToken);
        }
        let user: Option<(String, String)> = sqlx::query_as("SELECT username, role FROM users WHERE id = ? AND is_active = 1 AND deleted_at IS NULL")
            .bind(&claims.sub).fetch_optional(&mut *tx).await.map_err(|_| AuthError::Internal)?;
        let (username, role) = user.ok_or(AuthError::InvalidToken)?;
        if Role::parse(&role).is_none() {
            return Err(AuthError::InvalidToken);
        }
        let tokens = self.generate_tokens(&claims.sub, &username, &role)?;
        self.store_session(&mut tx, &tokens.0, &tokens.1).await?;
        tx.commit().await.map_err(|_| AuthError::Internal)?;
        Ok(tokens)
    }

    /// Prüft eine bestehende Login-Sperre; Fehlversuche werden separat gezählt.
    /// Liefert Ok(true) wenn das Konto gesperrt ist.
    pub async fn check_or_incr_failed(&self, user_id: &str) -> Result<bool, AuthError> {
        let pool = self.db.pool();
        let row: Option<(u32, Option<String>)> =
            sqlx::query_as("SELECT login_attempts, locked_until FROM users WHERE id = ?")
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
        let locked_until =
            (Utc::now() + chrono::Duration::seconds(self.lockout_duration as i64)).to_rfc3339();
        // Zähler und mögliche Sperre in einem atomaren Schreibvorgang erhöhen.
        sqlx::query(
            "UPDATE users SET
            login_attempts = CASE WHEN login_attempts + 1 >= ? THEN 0 ELSE login_attempts + 1 END,
            locked_until = CASE WHEN login_attempts + 1 >= ? THEN ? ELSE locked_until END
            WHERE id = ?",
        )
        .bind(self.max_login_attempts)
        .bind(self.max_login_attempts)
        .bind(&locked_until)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn reset_login_attempts(&self, user_id: &str) -> Result<(), AuthError> {
        let pool = self.db.pool();
        sqlx::query("UPDATE users SET login_attempts = 0, locked_until = NULL WHERE id = ?")
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    /// Erzeugt einen TOTP-Geheimnis (Base32) — MFA-Setup.
    pub fn generate_mfa_secret() -> String {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 20];
        rng.fill_bytes(&mut bytes);
        totp_rs::Secret::Raw(bytes.to_vec())
            .to_encoded()
            .to_string()
    }

    /// Prüft einen TOTP-Code gegen das Geheimnis (30 s-Periode, ±1 Toleranz).
    /// Implementiert nach RFC 6238, SHA-1 (Standard für TOTP).
    pub fn verify_totp(secret_b32: &str, code: &str) -> bool {
        let Ok(secret) = totp_rs::Secret::Encoded(secret_b32.into()).to_bytes() else {
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
    let row: Option<(String, String, String, String, i32)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, is_active
             FROM users WHERE username = ? AND deleted_at IS NULL",
    )
    .bind(&req.username)
    .fetch_optional(pool)
    .await
    .map_err(|_| AuthError::Internal)?;

    let (user_id, _username, password_hash, role, is_active) = match row {
        Some(r) => r,
        None => {
            // Zeitangriff-Schutz: trotzdem ein Passwort prüfen
            let _ = AuthContext::verify_password(&req.password, &auth.dummy_password_hash)?;
            auth.audit
                .log(
                    None,
                    AuditAction::LoginFailed,
                    "user",
                    None,
                    None,
                    None,
                    Some(&addr.ip().to_string()),
                )
                .await
                .map_err(|_| AuthError::Internal)?;
            return Err(AuthError::InvalidCredentials);
        }
    };

    if is_active == 0 {
        auth.audit
            .log(
                Some(&user_id),
                AuditAction::LoginFailed,
                "user",
                Some(&user_id),
                None,
                None,
                Some(&addr.ip().to_string()),
            )
            .await
            .map_err(|_| AuthError::Internal)?;
        return Err(AuthError::InvalidCredentials);
    }

    // Sperre prüfen
    if auth.check_or_incr_failed(&user_id).await? {
        auth.audit
            .log(
                Some(&user_id),
                AuditAction::Lockout,
                "user",
                Some(&user_id),
                None,
                None,
                Some(&addr.ip().to_string()),
            )
            .await
            .map_err(|_| AuthError::Internal)?;
        let until = sqlx::query_scalar::<_, String>("SELECT locked_until FROM users WHERE id = ?")
            .bind(&user_id)
            .fetch_one(pool)
            .await
            .unwrap_or_default();
        return Err(AuthError::AccountLocked(until));
    }

    // Passwort prüfen
    if !AuthContext::verify_password(&req.password, &password_hash)? {
        auth.register_failed_login(&user_id).await?;
        auth.audit
            .log(
                Some(&user_id),
                AuditAction::LoginFailed,
                "user",
                Some(&user_id),
                None,
                None,
                Some(&addr.ip().to_string()),
            )
            .await
            .map_err(|_| AuthError::Internal)?;
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
                    auth.audit
                        .log(
                            Some(&user_id),
                            AuditAction::Mfa,
                            "user",
                            Some(&user_id),
                            None,
                            None,
                            Some(&addr.ip().to_string()),
                        )
                        .await
                        .map_err(|_| AuthError::Internal)?;
                    return Err(AuthError::MfaRequired);
                }
                Some(code) => {
                    if !AuthContext::verify_totp(&secret, code) {
                        auth.audit
                            .log(
                                Some(&user_id),
                                AuditAction::Mfa,
                                "user",
                                Some(&user_id),
                                None,
                                None,
                                Some(&addr.ip().to_string()),
                            )
                            .await
                            .map_err(|_| AuthError::Internal)?;
                        return Err(AuthError::MfaInvalid);
                    }
                }
            }
        }
    }

    // Erfolg: Zähler zurücksetzen, Tokens ausstellen
    auth.reset_login_attempts(&user_id).await?;
    let (access, refresh) = auth.issue_tokens(&user_id, &req.username, &role).await?;
    auth.audit
        .log(
            Some(&user_id),
            AuditAction::Login,
            "user",
            Some(&user_id),
            None,
            None,
            Some(&addr.ip().to_string()),
        )
        .await
        .map_err(|_| AuthError::Internal)?;
    info!(user_id = %user_id, role, "Login erfolgreich");

    sqlx::query("UPDATE users SET last_login_at = datetime('now') WHERE id = ?")
        .bind(&user_id)
        .execute(pool)
        .await
        .map_err(|_| AuthError::Internal)?;

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
    auth.audit
        .log(
            Some(&claims.sub),
            AuditAction::Refresh,
            "user",
            Some(&claims.sub),
            None,
            None,
            None,
        )
        .await
        .map_err(|_| AuthError::Internal)?;
    Ok(Json(LoginResponse {
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer".into(),
        expires_in: auth.token_expiry,
        role: claims.role,
    }))
}

/// Logout-Handler: widerruft Access- und Refresh-Token der eigenen Sitzung.
pub async fn logout_handler(
    State(auth): State<Arc<AuthContext>>,
    Extension(user): Extension<super::middleware::AuthenticatedUser>,
    Json(req): Json<LogoutRequest>,
) -> Result<StatusCode, AuthError> {
    let claims = auth.verify_token(&req.refresh_token)?;
    if claims.sub != user.user_id {
        return Err(AuthError::InvalidToken);
    }
    auth.revoke_refresh(&req.refresh_token).await?;
    auth.audit
        .log(
            Some(&user.user_id),
            AuditAction::Logout,
            "user",
            Some(&user.user_id),
            None,
            None,
            None,
        )
        .await
        .map_err(|_| AuthError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reservierter MFA-Setup-Handler; bis zur vollständigen Umsetzung HTTP 501.
pub async fn mfa_setup_handler(State(_auth): State<Arc<AuthContext>>) -> Response {
    // Erst mit persistenter Zuordnung und Bestätigung freischalten.
    super::middleware::json_error(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "MFA-Setup ist noch nicht implementiert",
    )
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

    #[tokio::test]
    async fn concurrent_failed_logins_do_not_lose_increments() {
        let (context, _directory) = crate::security::tests::auth_fixture().await;
        let attempts = (0..5)
            .map(|_| {
                let context = context.clone();
                tokio::spawn(async move {
                    context.register_failed_login("u1").await.unwrap();
                })
            })
            .collect::<Vec<_>>();
        for attempt in attempts {
            attempt.await.unwrap();
        }
        assert!(context.check_or_incr_failed("u1").await.unwrap());
    }

    #[test]
    fn totp_verify() {
        // RFC 6238: Zeit 59 s, SHA1, sechs statt acht Ausgabestellen.
        assert_eq!(
            AuthContext::totp_at(b"12345678901234567890", 1).as_deref(),
            Some("287082")
        );
        // Fester Secret für Tests
        let secret = totp_rs::Secret::Raw(b"12345678901234567890".to_vec())
            .to_encoded()
            .to_string();
        // Erzeugt den aktuellen Code und prüft ihn
        let counter = (Utc::now().timestamp() / 30) as u64;
        let code = AuthContext::totp_at(b"12345678901234567890", counter).unwrap();
        assert!(AuthContext::verify_totp(&secret, &code));
    }

    #[tokio::test]
    async fn token_roundtrip() {
        let (ctx, _directory) = crate::security::tests::auth_fixture().await;
        let (access, refresh) = ctx.generate_tokens("u1", "alice", "doctor").unwrap();
        let claims = ctx.verify_token(&access).unwrap();
        assert_eq!(claims.sub, "u1");
        assert!(!claims.refresh);
        assert!(ctx.verify_token(&refresh).unwrap().refresh);
    }
}
