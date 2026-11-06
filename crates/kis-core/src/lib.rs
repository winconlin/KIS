//! KIS Core: Konfiguration, PostgreSQL-Zugang, Sicherheit, gemeinsame Modelle.
//!
//! Dies ist das Fundament-Modul. Alle anderen Crates bauen darauf auf.
//!
//! ## Architektur
//! - [`config`]   — TOML + ENV-Konfiguration (figment)
//! - [`db`]       — PostgreSQL Connection-Pool (Write/Read-Replica), Migrations
//! - [`security`] — JWT, AES-256-GCM, Audit-Log (Hash-Chain), RBAC, Argon2id
//! - [`models`]   — Gemeinsame Basis-Typen: EntityId, Pagination, ApiResponse, ApiError
//!
//! ## Module
//! Die Crates `kis-patients`, `kis-doctors`, `kis-appointments`,
//! `kis-medications` (Phase 1) sowie `kis-hl7`, `kis-fhir`, `kis-dicom`,
//! `kis-xds`, `kis-gdt`, `kis-medico`, `kis-ti`, `kis-ui` (Phase 2–3)
//! sind separate Crates im Workspace. Der Binary-Crate `kis-app`
//! merge-t deren Router.

pub mod access_policy;
pub mod anonymization;
pub mod auth;
pub mod cache;
pub mod config;
pub mod data_deletion;
pub mod data_portability;
pub mod db;
pub mod models;
pub mod query_optimizer;
pub mod rate_limit;
pub mod security;
pub mod validation;

/// Version des KIS (aus Cargo.toml)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// KIS-Baustein-Status
#[derive(Debug, Clone, serde::Serialize)]
pub struct KisStatus {
    pub version: String,
    pub modules: Vec<ModuleStatus>,
    pub db_connected: bool,
    pub uptime_seconds: u64,
}

/// Status eines einzelnen Moduls
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
}
