//! Authentifizierung mit persistenten Sitzungen (PostgreSQL).
//!
//! - Login: Argon2id-Prüfung, Kontosperre nach Fehlversuchen, optional TOTP.
//! - Jede Anmeldung erzeugt eine Zeile in `sessions` mit dem JTI des
//!   Refresh-Tokens (`jti`) und des aktuellen Access-Tokens (`access_jti`).
//! - Refresh rotiert beide JTIs atomar; bei parallelen Anfragen gewinnt
//!   genau eine.
//! - Logout und Widerruf machen Access- und Refresh-Token sofort ungültig,
//!   auch nach einem Neustart.
//! - [`validate_access`] prüft je Anfrage Sitzung, Kontostatus und
//!   Inaktivität und bestimmt die wirksamen Rechte aus den aktuellen
//!   Rollen des Benutzers — in **einer** Abfrage auf dem Schreib-Pool.
//!   Rollenmatrix und Richtlinien kommen aus dem Speicher
//!   ([`crate::platform::Platform`]) und werden nur bei geänderter Version
//!   neu geladen.

use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::AuthUser;
use crate::platform::Platform;
use crate::security::{Security, TokenType};

/// Fehler der Authentifizierung. Die Texte sind bewusst unspezifisch,
/// damit keine Konten aufgezählt werden können.
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
    /// Eingabe verletzt eine Richtlinie (z. B. Passwortlänge); der Text
    /// darf dem Aufrufer gezeigt werden.
    #[error("{0}")]
    Policy(String),
    #[error("Interner Fehler")]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AuthError {
    fn from(e: sqlx::Error) -> Self {
        AuthError::Internal(anyhow::anyhow!("DB: {e}"))
    }
}

/// Ergebnis eines erfolgreichen Logins oder Refreshs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
}

/// `last_seen_at` wird höchstens in diesem Abstand aktualisiert, damit
/// nicht jede Anfrage einen Schreibzugriff erzeugt.
const LAST_SEEN_RESOLUTION_SECS: f64 = 60.0;

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
    platform: &Platform,
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
        record_failure(db, security, platform, user_id, attempts, ip).await?;
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
            record_failure(db, security, platform, user_id, attempts, ip).await?;
            return Err(AuthError::InvalidCredentials);
        }
    }

    let role: String = row.get("role");
    let session_id = Uuid::new_v4();
    let sid = session_id.to_string();
    let uid = user_id.to_string();
    let (access_token, access_jti) =
        security.issue_token(&uid, &role, TokenType::Access, Some(&sid))?;
    let (refresh_token, refresh_jti) =
        security.issue_token(&uid, &role, TokenType::Refresh, Some(&sid))?;
    let refresh_secs = security.refresh_token_ttl().as_secs() as f64;

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
    .bind(refresh_secs)
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
    platform: &Platform,
    user_id: Uuid,
    attempts: i32,
    ip: Option<&str>,
) -> Result<(), AuthError> {
    let policies = platform.policies();
    let max = policies.login_max_attempts as i32;
    let lock_secs = (policies.login_lockout_minutes * 60) as f64;
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
    platform: &Platform,
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
    let inactivity_secs = (platform.policies().inactivity_minutes * 60) as f64;

    let user = sqlx::query(
        "SELECT u.id, u.role FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = $1 AND s.jti = $2 AND s.revoked_at IS NULL AND s.expires_at > now()
           AND COALESCE(s.last_seen_at, s.issued_at) > now() - make_interval(secs => $3)
           AND u.is_active = TRUE AND u.deleted_at IS NULL",
    )
    .bind(session_id)
    .bind(&claims.jti)
    .bind(inactivity_secs)
    .fetch_optional(db)
    .await?
    .ok_or(AuthError::InvalidToken)?;
    let user_id: Uuid = user.get("id");
    let role: String = user.get("role");

    let sid = session_id.to_string();
    let uid = user_id.to_string();
    let (access_token, access_jti) =
        security.issue_token(&uid, &role, TokenType::Access, Some(&sid))?;
    let (new_refresh, refresh_jti) =
        security.issue_token(&uid, &role, TokenType::Refresh, Some(&sid))?;

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

/// Prüft ein Access-Token vollständig und bestimmt die wirksamen Rechte.
///
/// Eine Abfrage liefert: gültige, nicht widerrufene, nicht inaktive
/// Sitzung mit passendem `access_jti`; aktives Konto; Haupt- und
/// Zusatzrollen; Versionszähler von Rollenmatrix und Einstellungen.
/// Rollen- und Rechteänderungen wirken damit bei der nächsten Anfrage.
pub async fn validate_access(
    db: &PgPool,
    security: &Security,
    platform: &Platform,
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
    let inactivity_secs = (platform.policies().inactivity_minutes * 60) as f64;

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
         SELECT s.user_id, s.role,
                ARRAY(SELECT role_key FROM user_roles WHERE user_id = s.user_id) AS extra_roles,
                p.rbac_version, p.settings_version
         FROM s CROSS JOIN platform_state p",
    )
    .bind(session_id)
    .bind(&claims.jti)
    .bind(inactivity_secs)
    .bind(LAST_SEEN_RESOLUTION_SECS)
    .fetch_optional(db)
    .await?
    .ok_or(AuthError::InvalidToken)?;

    let user_id: Uuid = row.get("user_id");
    let role: String = row.get("role");
    let extra_roles: Vec<String> = row.get("extra_roles");
    platform
        .refresh_if_stale(db, row.get("rbac_version"), row.get("settings_version"))
        .await?;
    let permissions = platform
        .permissions(std::iter::once(role.as_str()).chain(extra_roles.iter().map(String::as_str)));
    Ok(AuthUser {
        user_id: user_id.to_string(),
        role,
        permissions,
        session_id: Some(session_id),
    })
}

/// Widerruft eine Sitzung. `owner` begrenzt den Widerruf auf Sitzungen
/// dieses Benutzers (`None` = beliebige Sitzung, für die Administration).
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

/// Angaben für ein neues Benutzerkonto.
pub struct NewUser<'a> {
    pub username: &'a str,
    pub email: &'a str,
    pub display_name: &'a str,
    /// Schlüssel einer vorhandenen Rolle
    pub role: &'a str,
    pub password: &'a str,
}

/// Legt einen Benutzer an. `actor` steht im Audit (Benutzer-ID oder
/// Werkzeugname). Passwortlänge nach Richtlinie; die Rolle muss existieren.
pub async fn create_user(
    db: &PgPool,
    security: &Security,
    platform: &Platform,
    new: NewUser<'_>,
    actor: &str,
) -> Result<Uuid, AuthError> {
    let min = platform.policies().password_min_length as usize;
    if new.password.chars().count() < min {
        return Err(AuthError::Policy(format!(
            "Passwort muss mindestens {min} Zeichen lang sein"
        )));
    }
    if new.username.trim().is_empty() || new.email.trim().is_empty() {
        return Err(AuthError::Policy(
            "Benutzername und E-Mail dürfen nicht leer sein".into(),
        ));
    }
    let hash = security.hash_password(new.password)?;
    let mut tx = db.begin().await?;
    let inserted: Result<Uuid, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, display_name, email, role)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(new.username.trim())
    .bind(&hash)
    .bind(new.display_name.trim())
    .bind(new.email.trim())
    .bind(new.role)
    .fetch_one(&mut *tx)
    .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            let code = e
                .as_database_error()
                .and_then(|d| d.code().map(|c| c.to_string()));
            return Err(match code.as_deref() {
                Some("23503") => AuthError::Policy(format!("Unbekannte Rolle: {}", new.role)),
                Some("23505") => {
                    AuthError::Policy("Benutzername oder E-Mail ist bereits vergeben".into())
                }
                _ => e.into(),
            });
        }
    };
    security
        .log_audit_tx(
            &mut tx,
            actor,
            "user.create",
            "user",
            Some(&id.to_string()),
            None,
            Some(&serde_json::json!({ "role": new.role })),
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
