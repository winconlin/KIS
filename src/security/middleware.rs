//! Serverseitige Authentifizierung und Rollenprüfung für den Entwicklungsprototyp.
use super::{
    audit::{AuditAction, AuditLogger},
    auth::AuthContext,
    rate_limit::{RateLimitResult, SlidingWindowLimiter},
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{net::SocketAddr, sync::Arc};

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub username: String,
    pub role: String,
}
impl AuthenticatedUser {
    pub fn has_role(&self, required: &str) -> bool {
        self.role == required || self.role == "admin"
    }
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|role| self.has_role(role))
    }
}
fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/api/health" | "/api/auth/login" | "/api/auth/refresh"
    )
}
pub async fn auth_middleware(
    State(auth): State<Arc<AuthContext>>,
    mut req: Request,
    next: Next,
) -> Response {
    if is_public_path(req.uri().path()) {
        return next.run(req).await;
    }
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(token) = token else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "missing_token",
            "Bearer-Token erforderlich",
        );
    };
    let claims = match auth.verify_access_token(token).await {
        Ok(claims) => claims,
        Err(error) => return error.into_response(),
    };
    req.extensions_mut().insert(AuthenticatedUser {
        user_id: claims.sub,
        username: claims.username,
        role: claims.role,
    });
    next.run(req).await
}

pub async fn rbac_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if is_public_path(path) {
        return next.run(req).await;
    }
    let Some(user) = req.extensions().get::<AuthenticatedUser>() else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "missing_user",
            "Authentifizierung erforderlich",
        );
    };
    // Der Client kann die benötigte Berechtigung nicht über Queryparameter ändern.
    let roles: &[&str] = match (req.method().as_str(), path) {
        ("POST", "/api/auth/logout" | "/api/auth/mfa/setup") => {
            &["admin", "doctor", "nurse", "reception"]
        }
        ("GET", "/api/patients" | "/api/appointments") => {
            &["admin", "doctor", "nurse", "reception"]
        }
        ("GET", p) if p.starts_with("/api/patients/") => &["admin", "doctor", "nurse", "reception"],
        ("POST", "/api/patients") => &["admin", "reception"],
        ("POST", "/api/appointments") => &["admin", "doctor", "nurse", "reception"],
        ("GET", "/api/medications") => &["admin", "doctor", "nurse"],
        _ => &[],
    };
    if !user.has_any_role(roles) {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Nicht ausreichend berechtigt",
        );
    }
    next.run(req).await
}

pub async fn audit_middleware(
    State(audit): State<Arc<AuditLogger>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let entity = if path.starts_with("/api/patients") {
        Some("patient")
    } else if path.starts_with("/api/appointments") {
        Some("appointment")
    } else if path.starts_with("/api/medications") {
        Some("medication")
    } else {
        None
    };
    if let Some(entity) = entity {
        let user = req.extensions().get::<AuthenticatedUser>();
        let action = match req.method().as_str() {
            "POST" => AuditAction::Create,
            "PUT" | "PATCH" => AuditAction::Update,
            "DELETE" => AuditAction::Delete,
            _ => AuditAction::Read,
        };
        let id = path.split('/').nth(3);
        // Protokolliert den Zugriffsversuch, keinen bestätigten fachlichen DB-Commit.
        if audit
            .log(
                user.map(|u| u.user_id.as_str()),
                action,
                entity,
                id,
                None,
                None,
                Some(&addr.ip().to_string()),
            )
            .await
            .is_err()
        {
            return json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "audit_unavailable",
                "Audit-Protokollierung nicht verfügbar",
            );
        }
    }
    next.run(req).await
}

pub async fn rate_limit_middleware(
    State(limiter): State<Arc<SlidingWindowLimiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let scope = if req.uri().path().starts_with("/api/auth/") {
        "auth"
    } else {
        "api"
    };
    match limiter.check(&addr.ip().to_string(), scope) {
        RateLimitResult::Allowed => next.run(req).await,
        RateLimitResult::RetryAfter(seconds) => {
            let mut response = json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Zu viele Anfragen",
            );
            if let Ok(value) = seconds.to_string().parse() {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
    }
}

pub fn json_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(serde_json::json!({"error": message, "code": code, "status": status.as_u16()})),
    )
        .into_response()
}
