//! Axum-Middleware: Authentifizierung (JWT + RBAC), Audit-Logging, Rate-Limiting
//!
//! Anwendungsort (Reihenfolge, äußerste zuerst):
//!   1. RateLimitMiddleware  — lehnt überschrittene Limits mit 429 ab
//!   2. AuditMiddleware      — protokolliert jeden Request (Patientendaten!)
//!   3. AuthMiddleware       — validiert das JWT und legt den User in Extensions

use axum::{
    extract::{ConnectInfo, Request, State},
    http::header,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::warn;

use super::audit::{AuditAction, AuditLogger};
use super::auth::{AuthContext, Claims};
use super::rate_limit::{RateLimitResult, SlidingWindowLimiter};

/// Authenticated-User, der in die Request-Extensions gelegt wird
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub username: String,
    pub role: String,
}

impl AuthenticatedUser {
    /// Prüft, ob der User die geforderte Rolle hat (einfache RBAC).
    pub fn has_role(&self, required: &str) -> bool {
        self.role == required
            || (required == "doctor" && self.role == "admin")
            || (required == "nurse" && self.role == "admin")
            || (required == "reception" && self.role == "admin")
    }

    /// Prüft, ob der User eine der gegebenen Rollen hat.
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|r| self.has_role(r))
    }
}

/// Pfad-Präfixe, die ohne JWT erreichbar bleiben (Login, Health, UI).
fn is_public_path(path: &str) -> bool {
    path.starts_with("/api/health")
        || path.starts_with("/api/auth/login")
        || path.starts_with("/api/auth/refresh")
        || path.starts_with("/api/auth/mfa")
        || path == "/"
        || path.starts_with("/css/")
        || path.starts_with("/js/")
}

/// AuthMiddleware: validiert das JWT und legt den User in die Extensions.
///
/// Öffentliche Pfade (Login, Health, UI) passieren ohne Token.
pub async fn auth_middleware(
    State(auth): State<Arc<AuthContext>>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if is_public_path(&path) {
        return next.run(req).await;
    }

    // Authorization-Header parsen
    let header_val = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let token = match header_val {
        Some(h) if h.starts_with("Bearer ") => h[7..].to_string(),
        _ => {
            return json_error(
                axum::http::StatusCode::UNAUTHORIZED,
                "missing_token",
                "Fehlender Authorization-Header (Bearer <token>)",
            );
        }
    };

    // JWT validieren
    let claims: Claims = match auth.verify_token(&token) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path, error = %e, "Auth-Middleware: ungültiges Token");
            return json_error(
                axum::http::StatusCode::UNAUTHORIZED,
                e.code(),
                &e.to_string(),
            );
        }
    };

    let user = AuthenticatedUser {
        user_id: claims.sub.clone(),
        username: claims.username.clone(),
        role: claims.role.clone(),
    };
    req.extensions_mut().insert(user.clone());
    next.run(req).await
}

/// RBAC-Middleware: prüft die in `:role` erwartete Rolle.
///
/// Wird über `middleware::from_fn_with_state` an Routen gehängt, die eine
/// bestimmte Rolle erfordern. Die erwartete Rolle kommt aus der Query-Param
/// `?role=...` oder wird pro Route festgelegt (simpler Ansatz: Query).
pub async fn rbac_middleware(
    req: Request,
    next: Next,
) -> Response {
    // Erhoffte Rolle aus dem Query-Parameter (für einfache Routen)
    let required_role = req
        .uri()
        .query()
        .and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("role="))
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    if required_role.is_empty() {
        return next.run(req).await;
    }

    let user: Option<AuthenticatedUser> = req.extensions().get::<AuthenticatedUser>().cloned();
    let Some(user) = user else {
        return json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing_user",
            "Kein authentifizierter User im Request",
        );
    };

    if !user.has_role(&required_role) {
        warn!(
            user = %user.username,
            required = %required_role,
            actual = %user.role,
            "RBAC: Rolle nicht berechtigt"
        );
        return json_error(
            axum::http::StatusCode::FORBIDDEN,
            "forbidden",
            "Nicht ausreichend berechtigt",
        );
    }
    next.run(req).await
}

/// AuditMiddleware: protokolliert jeden Request, der Patientendaten betrifft.
///
/// Erkennt Patientendaten-Endpoints an `/api/patients`, `/api/appointments`,
/// `/api/medications`. Liest die IP aus ConnectInfo.
pub async fn audit_middleware(
    State(audit): State<Arc<AuditLogger>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // Nur Endpunkte mit Patientendaten protokollieren
    let is_patient_data = path.starts_with("/api/patients")
        || path.starts_with("/api/appointments")
        || path.starts_with("/api/medications");
    if !is_patient_data {
        return next.run(req).await;
    }

    // User aus Extensions lesen (falls bereits authifiziert)
    let user_id: Option<String> = req
        .extensions()
        .get::<AuthenticatedUser>()
        .map(|u| u.user_id.clone());

    let entity_type = extract_entity_type(&path);
    let entity_id = extract_entity_id(&path);
    let action = action_from_method(&method);

    // Audit-Log-Eintrag anlegen (asynchron)
    audit.log(
        user_id.as_deref(),
        action,
        &entity_type,
        entity_id.as_deref(),
        None,
        None,
        Some(addr.ip().to_string().as_str()),
    );

    next.run(req).await
}

/// RateLimitMiddleware: gleitendes Fenster pro IP.
///
/// Scope "auth" für `/api/auth/*` (strenger), sonst "api".
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<SlidingWindowLimiter>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let scope = if path.starts_with("/api/auth") {
        "auth"
    } else {
        "api"
    };

    match limiter.check(&addr.ip().to_string(), scope) {
        RateLimitResult::Allowed => next.run(req).await,
        RateLimitResult::RetryAfter(secs) => {
            warn!(ip = %addr.ip(), scope, "Rate-Limit erreicht");
            let mut resp = json_error(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Zu viele Anfragen — bitte später erneut versuchen",
            );
            resp.headers_mut().insert(
                header::RETRY_AFTER,
                secs.to_string().parse().unwrap(),
            );
            resp
        }
    }
}

/// Leitet die Entitäten-Art aus dem Pfad ab
fn extract_entity_type(path: &str) -> &'static str {
    if path.starts_with("/api/patients") {
        "patient"
    } else if path.starts_with("/api/appointments") {
        "appointment"
    } else if path.starts_with("/api/medications") {
        "medication"
    } else {
        "other"
    }
}

/// Extrahiert die ID aus `/api/patients/:id`
fn extract_entity_id(path: &str) -> Option<String> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // /api/patients/uuid → letztes Segment
    segs.last().filter(|_| segs.len() >= 3).map(|s| s.to_string())
}

/// Übersetzt die HTTP-Methode in eine Audit-Action
fn action_from_method(method: &axum::http::Method) -> AuditAction {
    match method.as_str() {
        "GET" => AuditAction::Read,
        "POST" => AuditAction::Create,
        "PUT" | "PATCH" => AuditAction::Update,
        "DELETE" => AuditAction::Delete,
        _ => AuditAction::Read,
    }
}

/// Erzeugt eine JSON-Fehlerantwort mit Fehlercode und Nachricht
pub fn json_error(status: axum::http::StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "error": message,
        "code": code,
        "status": status.as_u16(),
    });
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}
