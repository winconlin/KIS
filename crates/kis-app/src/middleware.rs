//! Zentrale Zugriffskontrolle für jede Anfrage (deny-by-default).
//!
//! Reihenfolge je Anfrage:
//! 1. Policy fragen, was die Route verlangt ([`AccessPolicy::decide`]).
//! 2. Öffentliche Routen durchlassen.
//! 3. Token, Sitzung, Kontostatus prüfen; wirksame Rechte bestimmen.
//! 4. Recht der Route prüfen.
//! 5. Bei `:case_id` / `:unit_id` im Pfad den Behandlungszusammenhang
//!    bzw. die Zugehörigkeit zur Einheit prüfen.
//! 6. Geprüften Benutzer für den Handler hinterlegen.
//!
//! Fachmodule können nichts davon umgehen oder vergessen.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use uuid::Uuid;

use kis_core::auth::{self, AuthError};
use kis_core::identity::{forbidden_response, unauthorized_response};
use kis_core::module::{Access, AccessPolicy, ModuleContext, Scope};
use kis_core::treatment::{self, AccessCheck};

/// Zustand der Zugriffskontrolle.
#[derive(Clone)]
pub struct Gate {
    pub ctx: ModuleContext,
    pub policy: Arc<AccessPolicy>,
}

fn json_error(status: StatusCode, code: &str, msg: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "error": msg,
            "code": code,
            "status": status.as_u16(),
        })),
    )
        .into_response()
}

fn internal_error() -> Response {
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "Interner Serverfehler",
    )
}

pub async fn auth_middleware(State(gate): State<Gate>, mut req: Request, next: Next) -> Response {
    let decision = gate.policy.decide(req.method(), req.uri().path());
    if decision.access == Access::Public {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let Some(token) = token else {
        return unauthorized_response("Fehlender Bearer-Token");
    };

    let pool = gate.ctx.pool();
    let user =
        match auth::validate_access(pool, &gate.ctx.security, &gate.ctx.platform, token).await {
            Ok(u) => u,
            Err(AuthError::Internal(e)) => {
                tracing::error!(error = %e, "Sitzungsprüfung fehlgeschlagen");
                return internal_error();
            }
            Err(_) => return unauthorized_response("Ungültiger oder abgelaufener Token"),
        };

    if !decision.access.allows(&user) {
        tracing::warn!(user = %user.user_id, role = %user.role, path = %req.uri().path(),
            "403: Recht fehlt");
        return forbidden_response("Keine Berechtigung für diese Aktion");
    }

    if decision.scope != Scope::None {
        let Some(id) = decision
            .scope_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            return json_error(StatusCode::BAD_REQUEST, "invalid_id", "Ungültige ID");
        };
        let policies = gate.ctx.platform.policies();
        let checked = match decision.scope {
            Scope::Case => treatment::check_case(pool, &user, id, &policies).await,
            Scope::Unit => treatment::check_unit(pool, &user, id, &policies).await,
            Scope::None => Ok(AccessCheck::Allowed),
        };
        match checked {
            Ok(AccessCheck::Allowed) => {}
            Ok(AccessCheck::NotFound) => {
                return json_error(StatusCode::NOT_FOUND, "not_found", "Nicht gefunden")
            }
            Ok(AccessCheck::Denied) => {
                tracing::warn!(user = %user.user_id, path = %req.uri().path(),
                    "403: kein Behandlungszusammenhang");
                let (code, msg) = match decision.scope {
                    Scope::Case => (
                        "no_treatment_context",
                        "Kein Behandlungszusammenhang zu diesem Fall",
                    ),
                    _ => ("not_member_of_unit", "Keine Zuordnung zu dieser Einheit"),
                };
                return json_error(StatusCode::FORBIDDEN, code, msg);
            }
            Err(e) => {
                tracing::error!(error = %e, "Prüfung des Behandlungszusammenhangs fehlgeschlagen");
                return internal_error();
            }
        }
    }

    req.extensions_mut().insert(user);
    next.run(req).await
}

/// Setzt konservative Sicherheits-Header auf jede Antwort.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api/");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    h.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'",
        ),
    );
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}
