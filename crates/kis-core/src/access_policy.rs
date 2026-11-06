//! Serverseitige Zugriffsregeln je Route (deny-by-default).
//!
//! Die zentrale Authentifizierungs-Middleware ruft [`required_access`] für
//! jede Anfrage auf. Nur hier aufgeführte Routen sind erreichbar; alles
//! Unbekannte unter `/api/` erfordert die Admin-Berechtigung. Fachmodule
//! können zusätzlich feinere Prüfungen vornehmen, aber keine Route
//! öffnen, die hier nicht erlaubt ist.

use axum::http::Method;

use crate::security::Permission;

/// Was eine Route verlangt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Ohne Anmeldung erreichbar (Health, Login, Refresh, statische UI).
    Public,
    /// Jede gültige Sitzung (z. B. eigene Sitzungen, Logout).
    Authenticated,
    /// Gültige Sitzung **und** diese Berechtigung.
    Permission(Permission),
}

/// Ermittelt die Anforderungen einer Route.
pub fn required_access(method: &Method, path: &str) -> Access {
    use Permission as P;

    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let is_get = method == Method::GET || method == Method::HEAD;

    // Nicht-API-Pfade: UI und statische Dateien
    if segs.first() != Some(&"api") {
        return if is_get {
            Access::Public
        } else {
            Access::Permission(P::Admin)
        };
    }

    let rest: &[&str] = if segs.get(1) == Some(&"v1") {
        &segs[2..]
    } else {
        &segs[1..]
    };

    match (method.as_str(), rest) {
        // --- Öffentlich ---
        ("POST", ["auth", "login"]) | ("POST", ["auth", "refresh"]) => Access::Public,
        ("GET", ["health"]) => Access::Public,

        // --- Sitzungen ---
        ("POST", ["auth", "logout"]) => Access::Authenticated,
        ("GET", ["auth", "sessions"]) => Access::Authenticated,
        ("POST", ["auth", "sessions", _, "revoke"]) => Access::Authenticated,

        // --- Betrieb ---
        ("GET", ["status"]) => Access::Authenticated,
        ("GET", ["health", _]) => Access::Permission(P::ConfigRead),

        // --- Audit ---
        ("GET", ["audit", _, _]) => Access::Permission(P::AuditRead),

        // --- DSGVO ---
        ("POST", ["patients", _, "hard-delete"]) => Access::Permission(P::Admin),
        ("GET", ["patients", _, "export"])
        | ("POST", ["patients", _, "export", "async"])
        | ("GET", ["patients", _, "export", "status"]) => Access::Permission(P::Export),
        (_, ["admin", ..]) => Access::Permission(P::Admin),

        // --- Patienten ---
        ("GET", ["patients"]) | ("GET", ["patients", _]) => Access::Permission(P::PatientRead),
        ("GET", ["patients", _, "history"]) => Access::Permission(P::AuditRead),
        ("POST", ["patients"]) => Access::Permission(P::PatientCreate),
        ("PUT", ["patients", _]) | ("PATCH", ["patients", _]) => {
            Access::Permission(P::PatientWrite)
        }
        ("DELETE", ["patients", _]) => Access::Permission(P::PatientDelete),

        // --- Ärzte ---
        ("GET", ["doctors", ..]) => Access::Permission(P::DoctorRead),
        (_, ["doctors", ..]) => Access::Permission(P::DoctorWrite),

        // --- Termine ---
        ("GET", ["appointments", ..]) => Access::Permission(P::AppointmentRead),
        ("POST", ["appointments"]) | ("POST", ["appointments", _, "book"]) => {
            Access::Permission(P::AppointmentBook)
        }
        ("POST", ["appointments", _, "cancel"]) | ("DELETE", ["appointments", _]) => {
            Access::Permission(P::AppointmentCancel)
        }
        (_, ["appointments", ..]) => Access::Permission(P::AppointmentWrite),

        // --- Medikation ---
        ("GET", ["medications", ..]) => Access::Permission(P::MedicationRead),
        (_, ["medications", ..]) => Access::Permission(P::MedicationWrite),

        // --- Alles andere: nur Admin ---
        _ => Access::Permission(P::Admin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::Role;

    fn allowed(role: Role, method: Method, path: &str) -> bool {
        match required_access(&method, path) {
            Access::Public | Access::Authenticated => true,
            Access::Permission(p) => role.has_permission(&p),
        }
    }

    #[test]
    fn public_routes() {
        assert_eq!(
            required_access(&Method::POST, "/api/v1/auth/login"),
            Access::Public
        );
        assert_eq!(
            required_access(&Method::POST, "/api/v1/auth/refresh"),
            Access::Public
        );
        assert_eq!(required_access(&Method::GET, "/health"), Access::Public);
        assert_eq!(required_access(&Method::GET, "/"), Access::Public);
        assert_eq!(
            required_access(&Method::GET, "/static/js/app.js"),
            Access::Public
        );
    }

    #[test]
    fn unknown_api_routes_are_admin_only() {
        assert_eq!(
            required_access(&Method::GET, "/api/v1/something/new"),
            Access::Permission(Permission::Admin)
        );
        assert!(!allowed(Role::Arzt, Method::GET, "/api/v1/fhir/Patient"));
        assert!(!allowed(Role::Viewer, Method::DELETE, "/api/v1/unknown"));
    }

    #[test]
    fn non_get_outside_api_is_admin_only() {
        assert!(!allowed(Role::Arzt, Method::POST, "/static/x"));
    }

    #[test]
    fn patient_matrix() {
        assert!(allowed(Role::Pflege, Method::GET, "/api/v1/patients"));
        assert!(!allowed(Role::Pflege, Method::POST, "/api/v1/patients"));
        assert!(allowed(Role::Rezeption, Method::POST, "/api/v1/patients"));
        assert!(!allowed(Role::Rezeption, Method::PUT, "/api/v1/patients/x"));
        assert!(allowed(Role::Arzt, Method::PUT, "/api/v1/patients/x"));
        assert!(!allowed(Role::Arzt, Method::DELETE, "/api/v1/patients/x"));
        assert!(allowed(Role::Admin, Method::DELETE, "/api/v1/patients/x"));
        assert!(!allowed(
            Role::Arzt,
            Method::POST,
            "/api/v1/patients/x/hard-delete"
        ));
        assert!(!allowed(
            Role::Viewer,
            Method::GET,
            "/api/v1/patients/x/export"
        ));
        assert!(!allowed(
            Role::Arzt,
            Method::GET,
            "/api/v1/patients/x/history"
        ));
        assert!(allowed(
            Role::Auditor,
            Method::GET,
            "/api/v1/patients/x/history"
        ));
    }

    #[test]
    fn audit_and_admin() {
        assert!(allowed(
            Role::Auditor,
            Method::GET,
            "/api/v1/audit/patient/1"
        ));
        assert!(!allowed(Role::Arzt, Method::GET, "/api/v1/audit/patient/1"));
        assert!(!allowed(
            Role::Arzt,
            Method::POST,
            "/api/v1/admin/deletion/check"
        ));
        assert!(allowed(
            Role::Admin,
            Method::POST,
            "/api/v1/admin/deletion/check"
        ));
    }

    #[test]
    fn reception_can_book_and_cancel_but_not_edit() {
        assert!(allowed(
            Role::Rezeption,
            Method::POST,
            "/api/v1/appointments"
        ));
        assert!(allowed(
            Role::Rezeption,
            Method::POST,
            "/api/v1/appointments/x/book"
        ));
        assert!(allowed(
            Role::Rezeption,
            Method::POST,
            "/api/v1/appointments/x/cancel"
        ));
        assert!(!allowed(
            Role::Rezeption,
            Method::PUT,
            "/api/v1/appointments/x"
        ));
        assert!(!allowed(
            Role::Rezeption,
            Method::GET,
            "/api/v1/appointments"
        ));
    }

    #[test]
    fn llm_agent_is_read_only() {
        for (m, p) in [
            (Method::POST, "/api/v1/patients"),
            (Method::PUT, "/api/v1/medications/x"),
            (Method::POST, "/api/v1/appointments"),
            (Method::DELETE, "/api/v1/patients/x"),
        ] {
            assert!(!allowed(Role::LlmAgent, m.clone(), p), "{m} {p}");
        }
        assert!(allowed(Role::LlmAgent, Method::GET, "/api/v1/medications"));
    }

    #[test]
    fn trailing_slash_and_prefix_variants() {
        assert_eq!(
            required_access(&Method::GET, "/api/v1/patients/"),
            Access::Permission(Permission::PatientRead)
        );
        assert_eq!(
            required_access(&Method::GET, "/api/patients"),
            Access::Permission(Permission::PatientRead)
        );
    }
}
