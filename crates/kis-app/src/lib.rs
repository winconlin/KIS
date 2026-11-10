//! KIS-Anwendung: setzt den Kern und die Module zu einem Server zusammen.
//!
//! | Datei | Aufgabe |
//! |---|---|
//! | `modules.rs` | Liste aller Module (eine Zeile je Modul) und Zusammenbau |
//! | `middleware.rs` | Anmeldung, Rechte, Behandlungszusammenhang, Sicherheits-Header |
//! | `state.rs` | Gemeinsamer Zustand der Kern-Handler |
//! | `core_routes.rs` | Anmeldung, Status, Module, Notfallzugriff, Outbox |
//! | `admin_users.rs` | Benutzer, Zusatzrollen, Zuordnung zu Einheiten |
//! | `admin_platform.rs` | Rollen, Einstellungen, Prüfungen, Folgeaktions-Fehler |
//! | `gdpr_routes.rs` | Auskunft, Löschung, Anonymisierung, Audit-Einsicht |
//! | `tls.rs` | TLS 1.3 |
//!
//! Als Bibliothek ausgelegt, damit Integrationstests den echten Router
//! gegen eine echte PostgreSQL-Datenbank prüfen können.

pub mod admin_platform;
pub mod admin_users;
pub mod core_routes;
pub mod gdpr_routes;
pub mod middleware;
pub mod modules;
pub mod state;
pub mod tls;

pub use modules::{build_app, App};
