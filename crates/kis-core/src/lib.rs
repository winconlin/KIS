//! KIS-Kern: alles, was jedes Modul braucht — und nichts Fachliches.
//!
//! | Baustein | Aufgabe |
//! |---|---|
//! | [`config`] | Infrastruktur-Konfiguration (TOML + Umgebung) |
//! | [`db`] | PostgreSQL-Pools und Migrationen |
//! | [`security`] | Token, Verschlüsselung, Passwort-Hash |
//! | [`auth`] | Anmeldung, Sitzungen, Prüfung je Anfrage |
//! | [`rbac`] | Rechte (Code) und Rollen (Daten) |
//! | [`platform`] | Rollenmatrix und Laufzeit-Einstellungen im Speicher |
//! | [`identity`] | Angemeldeter Benutzer einer Anfrage |
//! | [`module`] | Modulsystem, Routen mit Zugriffsregel, Policy |
//! | [`treatment`] | Behandlungszusammenhang, Notfallzugriff |
//! | [`events`] | Ereignisse zwischen Modulen, Outbox |
//! | [`facility`] | Betten sperren und freigeben |
//! | [`audit`] | Audit-Trail (Hash-Kette) |
//! | [`http`] | Fehlerantworten und Helfer für Handler |
//! | [`rate_limit`] | Anfragebegrenzung je Client |
//! | DSGVO | [`data_portability`], [`data_deletion`], [`anonymization`] |
//!
//! Abhängigkeiten zeigen immer von Modulen auf den Kern, nie umgekehrt und
//! nie von Modul zu Modul.

pub mod anonymization;
pub mod audit;
pub mod auth;
pub mod config;
pub mod data_deletion;
pub mod data_portability;
pub mod db;
pub mod events;
pub mod facility;
pub mod http;
pub mod identity;
pub mod models;
pub mod module;
pub mod platform;
pub mod rate_limit;
pub mod rbac;
pub mod security;
pub mod treatment;

/// Version des Kerns (aus Cargo.toml)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
