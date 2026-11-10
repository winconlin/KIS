//! Modulliste und Zusammenbau der Anwendung.
//!
//! **Neues Modul einbinden:** Crate in `Cargo.toml` (Workspace und
//! `kis-app`) eintragen und unten in [`all_modules`] eine Zeile ergänzen.
//! Mehr ist nicht nötig — Routen, Zugriffsregeln und Reaktionen auf
//! Ereignisse bringt das Modul selbst mit.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use kis_core::anonymization::AnonymizationService;
use kis_core::config::AppConfig;
use kis_core::data_deletion::DataDeletionService;
use kis_core::data_portability::DataPortabilityService;
use kis_core::db::Database;
use kis_core::events::EventBus;
use kis_core::module::{resolve_enabled, AccessPolicy, Module, ModuleContext, ModuleInfo};
use kis_core::platform::Platform;
use kis_core::rate_limit::{rate_limit_middleware, SlidingWindowRateLimiter};
use kis_core::security::Security;

use crate::middleware::{auth_middleware, security_headers, Gate};
use crate::state::{AppState, ModuleStatus, RouteInfo};
use crate::{admin_platform, admin_users, core_routes, gdpr_routes};

/// Alle Module, die diese Programmversion mitbringt. Welche davon aktiv
/// sind, entscheidet `[modules]` in der Konfiguration.
pub fn all_modules() -> Vec<Box<dyn Module>> {
    vec![
        // Kernmodule (gemeinsamer Vertrag, nicht abschaltbar)
        Box::new(kis_org::OrgModule),
        Box::new(kis_patients::PatientsModule),
        Box::new(kis_cases::CasesModule),
        // Fachmodule
        Box::new(kis_clinical::ClinicalModule),
        Box::new(kis_orders::OrdersModule),
        Box::new(kis_lab::LabModule),
        Box::new(kis_nursing::NursingModule),
        Box::new(kis_prescriptions::PrescriptionsModule),
        Box::new(kis_meals::MealsModule),
        Box::new(kis_roster::RosterModule),
        Box::new(kis_doctors::DoctorsModule),
        Box::new(kis_appointments::AppointmentsModule),
        Box::new(kis_medications::MedicationsModule),
        Box::new(kis_ui::UiModule),
        // NEUE-MODULE: scripts/new_module.py trägt oberhalb dieser Zeile ein
    ]
}

/// Beschreibungen aller Module (ohne sie zu bauen).
pub fn module_infos() -> Vec<ModuleInfo> {
    all_modules().iter().map(|m| m.info()).collect()
}

/// Die zusammengebaute Anwendung.
pub struct App {
    pub router: Router,
    pub state: AppState,
    pub policy: Arc<AccessPolicy>,
}

/// Baut die Anwendung: Plattformzustand laden, aktive Module bestimmen,
/// Routen und Zugriffsregeln zusammenführen, Middleware auflegen.
///
/// Bricht mit Fehler ab bei ungültiger Modulauswahl oder ungültigem
/// Master-Key — es gibt keinen stillen Teilbetrieb.
pub async fn build_app(
    config: AppConfig,
    db: Arc<Database>,
    security: Arc<Security>,
) -> anyhow::Result<App> {
    // Grundausstattung der Rollen einspielen (idempotent), Zustand laden
    let mut tx = db.write().begin().await?;
    kis_core::rbac::seed_builtin_roles(&mut tx).await?;
    tx.commit().await?;
    let platform = Arc::new(Platform::load(db.write(), &config.security).await?);

    let config = Arc::new(config);
    let ctx = ModuleContext {
        db: db.clone(),
        security: security.clone(),
        config: config.clone(),
        platform,
        events: Arc::new(EventBus::new()),
    };

    let modules = all_modules();
    let infos: Vec<ModuleInfo> = modules.iter().map(|m| m.info()).collect();
    let enabled = resolve_enabled(&infos, &config.modules)?;

    let core = [
        core_routes::routes(),
        admin_users::routes(),
        admin_platform::routes(),
        gdpr_routes::routes(),
    ];
    let mut route_infos: Vec<RouteInfo> = core
        .iter()
        .flat_map(|r| RouteInfo::collect("kern", r.rules()))
        .collect();
    let mut statuses = Vec::with_capacity(modules.len());
    let mut module_routes = Vec::new();
    for ((module, info), on) in modules.iter().zip(infos).zip(enabled) {
        let mut routes = 0;
        if on {
            let built = module.routes(&ctx);
            routes = built.rules().len();
            route_infos.extend(RouteInfo::collect(info.id, built.rules()));
            module_routes.push(built);
            for subscriber in module.subscribers(&ctx) {
                ctx.events.register(subscriber);
            }
        }
        tracing::info!(module = info.id, enabled = on, routes, "Modul");
        statuses.push(ModuleStatus {
            info,
            enabled: on,
            routes,
        });
    }

    let key_b64 = std::env::var(&config.security.encryption.master_key_env)
        .map_err(|_| anyhow::anyhow!("Master-Key nicht gesetzt"))?;
    let state = AppState {
        ctx: ctx.clone(),
        modules: Arc::new(statuses),
        routes: Arc::new(route_infos),
        deletion: Arc::new(DataDeletionService::new(db.clone(), security.clone())),
        portability: Arc::new(DataPortabilityService::new(db.clone(), security.clone())),
        anonymization: Arc::new(
            AnonymizationService::with_default_retention(db.clone(), security.clone(), &key_b64)
                .map_err(|e| anyhow::anyhow!("Anonymisierungsdienst: {e}"))?,
        ),
    };

    let mut all = core
        .into_iter()
        .map(|r| r.with_state(state.clone()))
        .reduce(|a, b| a.merge(b))
        .expect("Kernrouten vorhanden");
    for routes in module_routes {
        all = all.merge(routes);
    }
    let (router, rules) = all.into_parts();
    let policy = Arc::new(AccessPolicy::new(rules));
    tracing::info!(rules = policy.len(), "Zugriffsregeln geladen");

    let gate = Gate {
        ctx,
        policy: policy.clone(),
    };
    let limiter = Arc::new(SlidingWindowRateLimiter::new(&config.security.rate_limit));

    // Schichten von außen nach innen: Protokoll, Panik-Schutz,
    // Sicherheits-Header, Zeitlimit, Größenlimit, Anfragebegrenzung,
    // Zugriffskontrolle, Handler. Ein Fehler oder Absturz in einem Handler
    // betrifft nur diese eine Anfrage.
    let router = router
        .layer(axum::middleware::from_fn_with_state(gate, auth_middleware))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            rate_limit_middleware,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(
            config.server.max_body_bytes,
        ))
        .layer(TimeoutLayer::new(Duration::from_secs(
            config.server.request_timeout_seconds,
        )))
        .layer(axum::middleware::from_fn(security_headers))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http());

    Ok(App {
        router,
        state,
        policy,
    })
}
