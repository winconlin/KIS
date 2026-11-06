//! Authentifizierung mit persistenten Sitzungen (PostgreSQL).
//!
//! - Login: Argon2id-Prüfung, Kontosperre nach Fehlversuchen, optional TOTP.
//! - Jede Anmeldung erzeugt eine Zeile in `sessions`. Sie speichert das
//!   JTI des Refresh-Tokens (`jti`) und das des aktuellen Access-Tokens
//!   (`access_jti`).
//! - Refresh rotiert beide JTIs atomar (`UPDATE … WHERE jti = alt
//!   AND revoked_at IS NULL`). Bei parallelen Anfragen gewinnt genau eine.
//! - Logout und Widerruf setzen `revoked_at` und machen damit Access- und
//!   Refresh-Token sofort ungültig, auch nach einem Neustart.
//! - Jede Anfrage mit Access-Token prüft Sitzung, Kontostatus und aktuelle
//!   Rolle auf dem Write-Pool (maßgeblicher Stand, keine Read-Replica).
//! - Inaktivität: Sitzungen ohne Aktivität länger als
//!   [`INACTIVITY_TIMEOUT_SECS`] (30 min) werden abgelehnt (Auto-Logout).

use std::str::FromStr;

use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

use crate::security::{AuthUser, Role, Security, TokenType};

/// Fehler der Authentifizierung. Die `Display`-Texte sind bewusst
/// unspezifisch, damit keine Konten aufgezählt werden können.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Ungültige Anmeldedaten")]
    InvalidCredentials,
    #[error("Konto vorübergehend gesperrt")]
    Locked,
    #[error("MFA-Code erforderlich")]
    MfaRequired,
    #[error("Ungültiger oder abgelaufener Token")]
    InvalidToken,
    #[error("Interner Fehler")]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AuthError {
    fn from(e: sqlx::Error) -> Self {
        AuthError::Internal(anyhow::anyhow!("DB: {e}"))
    }
}

/// Inaktivitäts-Timeout einer Sitzung (automatische Abmeldung).
pub const INACTIVITY_TIMEOUT_SECS: i64 = 30 * 60;

/// `last_seen_at` wird höchstens in diesem Abstand aktualisiert, damit
/// nicht jede Anfrage einen Schreibzugriff erzeugt.
const LAST_SEEN_RESOLUTION_SECS: i64 = 60;

/// Ergebnis eines erfolgreichen Logins oder Refreshs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
}

/// Fester Argon2id-Hash, gegen den bei unbekannten Benutzern geprüft wird,
/// damit die Antwortzeit nicht verrät, ob ein Konto existiert.
fn dummy_hash(security: &Security) -> &'static str {
    use std::sync::OnceLock;
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        security
            .hash_password("kis-timing-equalizer")
            .unwrap_or_default()
    })
}

/// Meldet einen Benutzer per Benutzername oder E-Mail an.
pub async fn login(
    db: &PgPool,
    security: &Security,
    login: &str,
    password: &str,
    totp_code: Option<&str>,
    ip: Option<&str>,
) -> Result<TokenPair, AuthError> {
    let row = sqlx::query(
        "SELECT id, password_hash, role, mfa_enabled, mfa_secret,
                login_attempts, locked_until
         FROM users
         WHERE (username = $1 OR email = $1)
           AND is_active = TRUE AND deleted_at IS NULL",
    )
    .bind(login)
    .fetch_optional(db)
    .await?;

    let Some(row) = row else {
        let _ = security.verify_password(password, dummy_hash(security));
        return Err(AuthError::InvalidCredentials);
    };

    let user_id: Uuid = row.get("id");
    let password_hash: String = row.get("password_hash");
    let attempts: i32 = row.get("login_attempts");
    let locked_until: Option<chrono::DateTime<chrono::Utc>> = row.get("locked_until");

    if let Some(until) = locked_until {
        if until > chrono::Utc::now() {
            let _ = security.verify_password(password, dummy_hash(security));
            return Err(AuthError::Locked);
        }
    }

    if !security.verify_password(password, &password_hash) {
        record_failure(db, security, user_id, attempts, ip).await?;
        return Err(AuthError::InvalidCredentials);
    }

    let mfa_enabled: bool = row.get("mfa_enabled");
    if mfa_enabled {
        let secret: Option<String> = row.get("mfa_secret");
        let code = totp_code.ok_or(AuthError::MfaRequired)?;
        let ok = secret
            .as_deref()
            .map(|s| verify_totp(s, code))
            .unwrap_or(false);
        if !ok {
            record_failure(db, security, user_id, attempts, ip).await?;
            return Err(AuthError::InvalidCredentials);
        }
    }

    let role_str: String = row.get("role");
    let role = Role::from_str(&role_str).map_err(|e| anyhow::anyhow!(e))?;

    let session_id = Uuid::new_v4();
    let sid = session_id.to_string();
    let uid = user_id.to_string();
    let (access_token, access_jti) =
        security.issue_token(&uid, role.as_db_string(), TokenType::Access, Some(&sid))?;
    let (refresh_token, refresh_jti) =
        security.issue_token(&uid, role.as_db_string(), TokenType::Refresh, Some(&sid))?;
    let refresh_secs = security.refresh_token_ttl().as_secs() as i64;

    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE users SET login_attempts = 0, locked_until = NULL,
                last_login_at = now(), updated_at = now()
         WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sessions (id, user_id, jti, access_jti, issued_at, expires_at, last_seen_at, ip_address)
         VALUES ($1, $2, $3, $4, now(), now() + make_interval(secs => $5), now(), $6)",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(&refresh_jti)
    .bind(&access_jti)
    .bind(refresh_secs as f64)
    .bind(ip)
    .execute(&mut *tx)
    .await?;
    security
        .log_audit_tx(
            &mut tx,
            &uid,
            "auth.login",
            "session",
            Some(&sid),
            None,
            None,
            ip,
        )
        .await?;
    tx.commit().await?;

    Ok(TokenPair {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: security.access_token_ttl().as_secs(),
        session_id,
        user_id,
        role,
    })
}

async fn record_failure(
    db: &PgPool,
    security: &Security,
    user_id: Uuid,
    attempts: i32,
    ip: Option<&str>,
) -> Result<(), AuthError> {
    let max = security.max_login_attempts().max(1) as i32;
    let lock_secs = security.lockout_duration().as_secs() as f64;
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE users SET
            login_attempts = CASE WHEN login_attempts + 1 >= $2 THEN 0 ELSE login_attempts + 1 END,
            locked_until = CASE WHEN login_attempts + 1 >= $2
                                THEN now() + make_interval(secs => $3)
                                ELSE locked_until END,
            updated_at = now()
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(max)
    .bind(lock_secs)
    .execute(&mut *tx)
    .await?;
    let locked = attempts + 1 >= max;
    security
        .log_audit_tx(
            &mut tx,
            &user_id.to_string(),
            if locked {
                "auth.lockout"
            } else {
                "auth.login_failed"
            },
            "user",
            Some(&user_id.to_string()),
            None,
            None,
            ip,
        )
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Rotiert ein Refresh-Token. Das alte Refresh- und Access-Token werden
/// dabei ungültig.
pub async fn refresh(
    db: &PgPool,
    security: &Security,
    refresh_token: &str,
    ip: Option<&str>,
) -> Result<TokenPair, AuthError> {
    let claims = security
        .decode_token(refresh_token, TokenType::Refresh)
        .map_err(|_| AuthError::InvalidToken)?;
    let session_id: Uuid = claims
        .sid
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(AuthError::InvalidToken)?;

    // Aktuelle Rolle und Kontostatus vom maßgeblichen Stand lesen.
    let user = sqlx::query(
        "SELECT u.id, u.role FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = $1 AND s.jti = $2 AND s.revoked_at IS NULL AND s.expires_at > now()
           AND COALESCE(s.last_seen_at, s.issued_at) > now() - make_interval(secs => $3)
           AND u.is_active = TRUE AND u.deleted_at IS NULL",
    )
    .bind(session_id)
    .bind(&claims.jti)
    .bind(INACTIVITY_TIMEOUT_SECS as f64)
    .fetch_optional(db)
    .await?
    .ok_or(AuthError::InvalidToken)?;
    let user_id: Uuid = user.get("id");
    let role = Role::from_str(&user.get::<String, _>("role")).map_err(|e| anyhow::anyhow!(e))?;

    let sid = session_id.to_string();
    let uid = user_id.to_string();
    let (access_token, access_jti) =
        security.issue_token(&uid, role.as_db_string(), TokenType::Access, Some(&sid))?;
    let (new_refresh, refresh_jti) =
        security.issue_token(&uid, role.as_db_string(), TokenType::Refresh, Some(&sid))?;

    let mut tx = db.begin().await?;
    let res = sqlx::query(
        "UPDATE sessions SET jti = $3, access_jti = $4, last_seen_at = now()
         WHERE id = $1 AND jti = $2 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(session_id)
    .bind(&claims.jti)
    .bind(&refresh_jti)
    .bind(&access_jti)
    .execute(&mut *tx)
    .await?;
    if res.rows_affected() != 1 {
        // Parallel bereits rotiert oder widerrufen.
        return Err(AuthError::InvalidToken);
    }
    security
        .log_audit_tx(
            &mut tx,
            &uid,
            "auth.refresh",
            "session",
            Some(&sid),
            None,
            None,
            ip,
        )
        .await?;
    tx.commit().await?;

    Ok(TokenPair {
        access_token,
        refresh_token: new_refresh,
        token_type: "Bearer",
        expires_in: security.access_token_ttl().as_secs(),
        session_id,
        user_id,
        role,
    })
}

/// Prüft ein Access-Token vollständig: Signatur/Ablauf/Typ, nicht
/// widerrufene Sitzung mit passendem `access_jti`, aktives Konto.
/// Die Rolle wird aus der Datenbank übernommen, damit Rollenänderungen
/// sofort wirken.
pub async fn validate_access(
    db: &PgPool,
    security: &Security,
    token: &str,
) -> Result<AuthUser, AuthError> {
    let claims = security
        .decode_token(token, TokenType::Access)
        .map_err(|_| AuthError::InvalidToken)?;
    let session_id: Uuid = claims
        .sid
        .as_deref()
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(AuthError::InvalidToken)?;

    // Prüfen und `last_seen_at` in einem Statement fortschreiben.
    let row = sqlx::query(
        "WITH s AS (
             SELECT s.id, u.id AS user_id, u.role,
                    COALESCE(s.last_seen_at, s.issued_at) AS seen
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.id = $1 AND s.access_jti = $2
               AND s.revoked_at IS NULL AND s.expires_at > now()
               AND COALESCE(s.last_seen_at, s.issued_at) > now() - make_interval(secs => $3)
               AND u.is_active = TRUE AND u.deleted_at IS NULL
         ), touch AS (
             UPDATE sessions SET last_seen_at = now()
             FROM s WHERE sessions.id = s.id
               AND s.seen < now() - make_interval(secs => $4)
         )
         SELECT user_id AS id, role FROM s",
    )
    .bind(session_id)
    .bind(&claims.jti)
    .bind(INACTIVITY_TIMEOUT_SECS as f64)
    .bind(LAST_SEEN_RESOLUTION_SECS as f64)
    .fetch_optional(db)
    .await?
    .ok_or(AuthError::InvalidToken)?;

    let user_id: Uuid = row.get("id");
    let role = Role::from_str(&row.get::<String, _>("role")).map_err(|e| anyhow::anyhow!(e))?;
    Ok(AuthUser {
        user_id: user_id.to_string(),
        role,
        session_id: Some(session_id),
    })
}

/// Widerruft eine Sitzung. `owner` begrenzt den Widerruf auf Sitzungen
/// dieses Benutzers (`None` = Admin-Widerruf beliebiger Sitzungen).
pub async fn revoke_session(
    db: &PgPool,
    security: &Security,
    session_id: Uuid,
    actor: &AuthUser,
    owner: Option<Uuid>,
) -> Result<bool, AuthError> {
    let mut tx = db.begin().await?;
    let res = sqlx::query(
        "UPDATE sessions SET revoked_at = now()
         WHERE id = $1 AND revoked_at IS NULL AND ($2::uuid IS NULL OR user_id = $2)",
    )
    .bind(session_id)
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    let revoked = res.rows_affected() == 1;
    if revoked {
        security
            .log_audit_tx(
                &mut tx,
                &actor.user_id,
                "auth.session_revoke",
                "session",
                Some(&session_id.to_string()),
                None,
                None,
                None,
            )
            .await?;
    }
    tx.commit().await?;
    Ok(revoked)
}

/// Aktive Sitzungen eines Benutzers (ohne Token-Material).
pub async fn list_sessions(
    db: &PgPool,
    user_id: Uuid,
) -> Result<Vec<serde_json::Value>, AuthError> {
    let rows = sqlx::query(
        "SELECT id, issued_at, expires_at, last_seen_at, ip_address FROM sessions
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()
         ORDER BY issued_at DESC",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "issuedAt": r.get::<chrono::DateTime<chrono::Utc>, _>("issued_at"),
                "expiresAt": r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at"),
                "lastSeenAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_seen_at"),
                "ipAddress": r.get::<Option<String>, _>("ip_address"),
            })
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
/// Legt einen Benutzer an (Bootstrap/Administration). Das Passwort muss
/// mindestens 12 Zeichen lang sein.
pub async fn create_user(
    db: &PgPool,
    security: &Security,
    username: &str,
    email: &str,
    display_name: &str,
    role: Role,
    password: &str,
    actor: &str,
) -> Result<Uuid, AuthError> {
    if password.chars().count() < 12 {
        return Err(AuthError::Internal(anyhow::anyhow!(
            "Passwort muss mindestens 12 Zeichen lang sein"
        )));
    }
    let hash = security.hash_password(password)?;
    let mut tx = db.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, display_name, email, role)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(username)
    .bind(&hash)
    .bind(display_name)
    .bind(email)
    .bind(role.as_db_string())
    .fetch_one(&mut *tx)
    .await?;
    security
        .log_audit_tx(
            &mut tx,
            actor,
            "user.create",
            "user",
            Some(&id.to_string()),
            None,
            Some(&serde_json::json!({ "username": username, "role": role })),
            None,
        )
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// Prüft einen 6-stelligen TOTP-Code (RFC 6238, SHA-1, 30 s, ±1 Schritt).
/// `secret` ist Base32-codiert.
pub fn verify_totp(secret: &str, code: &str) -> bool {
    use totp_rs::{Algorithm, Secret, TOTP};
    let Ok(bytes) = Secret::Encoded(secret.to_string()).to_bytes() else {
        return false;
    };
    match TOTP::new(Algorithm::SHA1, 6, 1, 30, bytes) {
        Ok(totp) => totp.check_current(code).unwrap_or(false),
        Err(_) => false,
    }
}
