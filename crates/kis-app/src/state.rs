//! Zustand der Kern-Handler (Anmeldung, Verwaltung, DSGVO).

use std::sync::Arc;

use kis_core::anonymization::AnonymizationService;
use kis_core::data_deletion::DataDeletionService;
use kis_core::data_portability::DataPortabilityService;
use kis_core::module::{ModuleContext, ModuleInfo, RouteRule, Scope};

/// Ein Modul mit seinem tatsächlichen Zustand in dieser Instanz.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModuleStatus {
    #[serde(flatten)]
    pub info: ModuleInfo,
    pub enabled: bool,
    /// Anzahl registrierter Routen (0 bei abgeschalteten Modulen)
    pub routes: usize,
}

/// Eine registrierte Route, wie sie `GET /api/v1/routes` ausgibt.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteInfo {
    /// Modulkennung oder `kern`
    pub module: &'static str,
    pub method: String,
    pub path: &'static str,
    /// Verlangtes Recht in Worten
    pub access: String,
    /// `fall` (Behandlungszusammenhang), `einheit` (Zuordnung) oder leer
    pub scope: &'static str,
}

impl RouteInfo {
    pub fn collect(module: &'static str, rules: &[RouteRule]) -> Vec<RouteInfo> {
        rules
            .iter()
            .map(|r| RouteInfo {
                module,
                method: r.method.to_string(),
                path: r.path,
                access: r.access.describe(),
                scope: match r.scope {
                    Scope::None => "",
                    Scope::Case => "fall",
                    Scope::Unit => "einheit",
                },
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub ctx: ModuleContext,
    pub modules: Arc<Vec<ModuleStatus>>,
    /// Alle Routen dieser Instanz mit ihren Zugriffsregeln
    pub routes: Arc<Vec<RouteInfo>>,
    pub deletion: Arc<DataDeletionService>,
    pub portability: Arc<DataPortabilityService>,
    pub anonymization: Arc<AnonymizationService>,
}

impl AppState {
    pub fn pool(&self) -> &sqlx::PgPool {
        self.ctx.pool()
    }
}
