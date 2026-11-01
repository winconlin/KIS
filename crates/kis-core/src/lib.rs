//! KIS Core: Config, DB, Security, API-Router
//!
//! Dies ist das Fundament-Modul. Alle anderen Module bauen darauf auf.

pub mod config;
pub mod db;
pub mod api;
pub mod security;
pub mod models;

/// Version des KIS
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// KIS-Baustein-Status
#[derive(Debug, Clone, serde::Serialize)]
pub struct KisStatus {
    pub version: String,
    pub modules: Vec<ModuleStatus>,
    pub db_connected: bool,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
}
