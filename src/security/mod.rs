//! Sicherheitsmodul: Authentifizierung, Autorisierung (RBAC), Audit-Log,
//! Rate-Limiting, Verschlüsselung, Eingabevalidierung.
//!
//! Untermodule:
//!   - [`auth`]         — Login, JWT (Access + Refresh), Argon2, MFA (TOTP)
//!   - [`audit`]        — Append-only Audit-Log mit Hash-Kette
//!   - [`middleware`]   — Axum-Middleware (Auth, RBAC, Audit, Rate-Limit)
//!   - [`rate_limit`]   — In-memory gleitendes Fenster pro IP
//!   - [`encryption`]   — AES-256-GCM für sensible Patientendaten
//!   - [`validation`]   — Eingabevalidierung (E-Mail, Telefon, Datum, Name)

pub mod audit;
pub mod auth;
pub mod encryption;
pub mod middleware;
pub mod rate_limit;
pub mod validation;

pub use audit::{AuditAction, AuditLog, AuditLogger};
pub use auth::{AuthContext, AuthError, Claims, LoginRequest, LoginResponse, Role};
pub use encryption::{init_crypto, CryptoError, CryptoService};
pub use middleware::{
    audit_middleware, auth_middleware, json_error, rate_limit_middleware, rbac_middleware,
    AuthenticatedUser,
};
pub use rate_limit::{RateLimitResult, SlidingWindowLimiter};

/// Initialisiert den kompletten Sicherheits-Stack:
/// 1. Verschlüsselung (Masterkey aus Umgebung)
/// 2. Audit-Logger (Log-Datei + DB)
/// 3. Auth-Kontext (JWT-Secret aus Umgebung)
/// 4. Rate-Limiter
///
/// Liefert die Bausteine, die in den Router injiziert werden müssen.
pub async fn initialize(
    config: &crate::config::SecurityConfig,
    db: std::sync::Arc<crate::db::Database>,
) -> anyhow::Result<SecurityStack> {
    // 1) Verschlüsselung
    encryption::init_crypto(&config.encryption.master_key_env)?;

    // 2) Audit-Logger
    let audit_path = std::path::PathBuf::from(&config.audit.log_file);
    let audit = AuditLogger::start(&audit_path, db.pool().clone()).await?;
    let audit = std::sync::Arc::new(audit);

    // 3) Auth-Kontext
    let auth_context = std::sync::Arc::new(
        auth::AuthContext::new(config, db.clone(), audit.clone()).await?,
    );

    // 4) Rate-Limiter
    let limiter = std::sync::Arc::new(SlidingWindowLimiter::new(
        config.rate_limit.requests_per_minute,
        config.rate_limit.auth_requests_per_minute,
    ));

    Ok(SecurityStack {
        auth: auth_context,
        audit,
        limiter,
    })
}

/// Bündel aller initialisierten Sicherheits-Bausteine
pub struct SecurityStack {
    pub auth: std::sync::Arc<AuthContext>,
    pub audit: std::sync::Arc<AuditLogger>,
    pub limiter: std::sync::Arc<SlidingWindowLimiter>,
}
