//! Angemeldeter Benutzer einer Anfrage.
//!
//! Die zentrale Middleware in `kis-app` prüft Token, Sitzung, Kontostatus,
//! Berechtigung und Behandlungszusammenhang und legt danach einen
//! [`AuthUser`] in die Request-Extensions. Handler erhalten ihn über den
//! Extractor [`RequireRole`] (Alias [`CurrentUser`]). Der Extractor prüft
//! selbst nichts mehr — fehlt der Eintrag, antwortet er mit 401.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::rbac::{Permission, PermissionSet, RoleMatrix};

/// Authentifizierter Benutzer mit wirksamen Rechten.
#[derive(Debug, Clone)]
pub struct AuthUser {
    /// Benutzer-ID (UUID als Text, so wie sie im Audit steht)
    pub user_id: String,
    /// Schlüssel der Hauptrolle (`users.role`), nur zur Anzeige
    pub role: String,
    /// Wirksame Rechte aus Haupt- und Zusatzrollen
    pub permissions: PermissionSet,
    /// Sitzung, aus der die Anfrage stammt
    pub session_id: Option<Uuid>,
}

impl AuthUser {
    /// Benutzer mit den Rechten einer Rolle der Grundausstattung (Tests,
    /// Werkzeuge). Im Betrieb erzeugt nur `auth::validate_access` Benutzer.
    pub fn with_builtin_role(user_id: impl Into<String>, role: &str) -> Self {
        Self {
            user_id: user_id.into(),
            role: role.to_string(),
            permissions: RoleMatrix::builtin().effective([role]),
            session_id: None,
        }
    }

    /// Hat der Benutzer das Recht?
    pub fn can(&self, permission: &Permission) -> bool {
        self.permissions.contains(*permission)
    }

    /// Hat der Benutzer mindestens eines der Rechte?
    pub fn can_any(&self, permissions: &[Permission]) -> bool {
        permissions.iter().any(|p| self.can(p))
    }

    /// Benutzer-ID als UUID. `None`, wenn die ID kein UUID ist (kann bei
    /// regulär angemeldeten Benutzern nicht vorkommen).
    pub fn uuid(&self) -> Option<Uuid> {
        Uuid::parse_str(&self.user_id).ok()
    }
}

/// Extractor für den geprüften Benutzer.
pub struct RequireRole {
    pub auth: AuthUser,
}

/// Sprechenderer Name für [`RequireRole`] in neuen Modulen.
pub type CurrentUser = RequireRole;

impl RequireRole {
    pub fn role(&self) -> &str {
        &self.auth.role
    }

    pub fn user_id(&self) -> &str {
        &self.auth.user_id
    }

    pub fn can(&self, permission: &Permission) -> bool {
        self.auth.can(permission)
    }
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireRole {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth = parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(|| unauthorized_response("Nicht authentifiziert"))?;
        Ok(RequireRole { auth })
    }
}

pub fn unauthorized_response(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", "Bearer")],
        axum::Json(serde_json::json!({ "success": false, "error": msg, "status": 401 })),
    )
        .into_response()
}

pub fn forbidden_response(msg: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({ "success": false, "error": msg, "status": 403 })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_role_permissions() {
        let u = AuthUser::with_builtin_role("u", "arzt");
        assert!(u.can(&Permission::ClinicalWrite));
        assert!(!u.can(&Permission::RosterWrite));
        assert!(!u.can(&Permission::Admin));
        assert!(u.can_any(&[Permission::Admin, Permission::RosterRead]));
        assert!(AuthUser::with_builtin_role("a", "admin").can(&Permission::PatientDelete));
        assert!(AuthUser::with_builtin_role("x", "gibt-es-nicht")
            .permissions
            .is_empty());
    }

    #[test]
    fn uuid_parsing() {
        assert!(AuthUser::with_builtin_role("not-a-uuid", "viewer")
            .uuid()
            .is_none());
        let id = Uuid::new_v4();
        assert_eq!(
            AuthUser::with_builtin_role(id.to_string(), "viewer").uuid(),
            Some(id)
        );
    }
}
