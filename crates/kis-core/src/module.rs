//! Modulsystem: Fachmodule als eigenständige, zu-/abschaltbare Bausteine.
//!
//! Ein Modul ist eine Crate, die [`Module`] implementiert. Es liefert
//! - [`ModuleInfo`] (Kennung, Abhängigkeiten, Reifegrad) und
//! - seine Routen über den Builder [`ModuleRoutes`].
//!
//! **Leitplanke 1 — keine Route ohne Zugriffsregel.** Routen werden
//! ausschließlich über [`ModuleRoutes`] registriert. Jede Registrierung
//! verlangt ein [`Access`]; daraus entsteht die serverseitige
//! [`AccessPolicy`]. Was dort nicht steht, ist unter `/api/` nur für
//! Admins erreichbar (deny-by-default).
//!
//! **Leitplanke 2 — Behandlungszusammenhang zentral.** Enthält ein Pfad den
//! Parameter `:case_id` oder `:unit_id`, prüft die zentrale Middleware vor
//! dem Handler, ob der Benutzer diesen Fall bzw. diese Einheit sehen darf
//! ([`Scope`], [`crate::treatment`]). Fallbezogene Daten gehören deshalb
//! immer unter `/api/v1/cases/:case_id/…`, Arbeitslisten einer Einheit
//! unter `/api/v1/units/:unit_id/…`, und Handler filtern zusätzlich mit
//! `WHERE case_id = $…` bzw. `unit_id = $…`.
//!
//! **Leitplanke 3 — Module kennen einander nicht.** Ein Modul greift nur
//! auf Kern-Tabellen (lesend) und seine eigenen Tabellen zu
//! (`config/module_tables.toml`, geprüft von `scripts/check_modules.py`).
//! Zusammenspiel läuft über Ereignisse ([`crate::events`]): ein Modul
//! meldet „Fall entlassen“, ein anderes reagiert darauf. Fällt das
//! reagierende Modul aus oder wird es abgeschaltet, läuft der auslösende
//! Vorgang unverändert weiter.
//!
//! Ein neues Modul wird in `kis-app/src/modules.rs` mit einer Zeile
//! registriert und kann über `[modules]` in der Konfiguration abgeschaltet
//! werden. Anleitung: `docs/MODULE.md`.

use std::sync::Arc;

use axum::handler::Handler;
use axum::http::Method;
use axum::routing::{self, MethodRouter};
use axum::Router;

use crate::config::{AppConfig, ModulesConfig};
use crate::db::Database;
use crate::events::{EventBus, EventSubscriber};
use crate::identity::AuthUser;
use crate::platform::Platform;
use crate::rbac::Permission;
use crate::security::Security;

// ===========================================================================
// Zugriffsregeln
// ===========================================================================

/// Was eine Route verlangt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Ohne Anmeldung erreichbar (Health, Login, statische Oberfläche).
    Public,
    /// Jede gültige Sitzung.
    Authenticated,
    /// Gültige Sitzung und diese Berechtigung.
    Permission(Permission),
    /// Gültige Sitzung und mindestens eine der Berechtigungen.
    AnyOf(&'static [Permission]),
}

impl Access {
    /// Regel in Worten, z. B. `case.read` oder `nursing.read oder
    /// clinical.read` (für die Routenübersicht).
    pub fn describe(&self) -> String {
        match self {
            Access::Public => "öffentlich".to_string(),
            Access::Authenticated => "angemeldet".to_string(),
            Access::Permission(p) => p.key().to_string(),
            Access::AnyOf(list) => list
                .iter()
                .map(|p| p.key())
                .collect::<Vec<_>>()
                .join(" oder "),
        }
    }

    /// Erfüllt der angemeldete Benutzer die Regel? (`Public` und
    /// `Authenticated` sind für jeden angemeldeten Benutzer erfüllt.)
    pub fn allows(&self, user: &AuthUser) -> bool {
        match self {
            Access::Public | Access::Authenticated => true,
            Access::Permission(p) => user.can(p),
            Access::AnyOf(list) => user.can_any(list),
        }
    }
}

/// Datenbezug einer Route, aus dem Pfad abgeleitet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Kein Fall-/Stationsbezug im Pfad.
    None,
    /// Pfad enthält `:case_id` → Behandlungszusammenhang wird geprüft.
    Case,
    /// Pfad enthält `:unit_id` → Zugehörigkeit zur Einheit wird geprüft.
    Unit,
}

/// Eine registrierte Route mit ihrer Zugriffsregel.
#[derive(Debug, Clone)]
pub struct RouteRule {
    pub method: Method,
    pub path: &'static str,
    pub access: Access,
    pub scope: Scope,
}

const CASE_PARAM: &str = ":case_id";
const UNIT_PARAM: &str = ":unit_id";

/// Leitet den [`Scope`] aus dem Pfad ab und prüft die Namenskonventionen.
///
/// # Panics
/// Bei Verstößen gegen die Konventionen (Programmierfehler; fällt in jedem
/// Test auf, der den Router baut).
fn scope_of(path: &'static str, access: Access) -> Scope {
    assert!(
        path.starts_with('/'),
        "Routenpfad muss mit / beginnen: {path}"
    );
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let has_case = segs.contains(&CASE_PARAM);
    let has_unit = segs.contains(&UNIT_PARAM);
    assert!(
        !(has_case && has_unit),
        "Pfad darf nicht :case_id und :unit_id zugleich enthalten: {path}"
    );
    for (prefix, param) in [("cases", CASE_PARAM), ("units", UNIT_PARAM)] {
        if segs.len() > 3 && segs[..3] == ["api", "v1", prefix] && segs[3].starts_with(':') {
            assert_eq!(
                segs[3], param,
                "Unter /api/v1/{prefix}/ muss der Parameter {param} heißen: {path}"
            );
        }
    }
    let scope = if has_case {
        Scope::Case
    } else if has_unit {
        Scope::Unit
    } else {
        Scope::None
    };
    assert!(
        scope == Scope::None || access != Access::Public,
        "Fall-/stationsbezogene Route darf nicht öffentlich sein: {path}"
    );
    scope
}

// ===========================================================================
// Routen-Builder
// ===========================================================================

/// Builder für die Routen eines Moduls. `S` ist der Handler-State; für
/// neue Module üblicherweise [`ModuleContext`].
pub struct ModuleRoutes<S = ()> {
    router: Router<S>,
    rules: Vec<RouteRule>,
}

impl<S> Default for ModuleRoutes<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S> ModuleRoutes<S>
where
    S: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            rules: Vec::new(),
        }
    }

    /// Registriert eine Route zusammen mit ihrer Zugriffsregel.
    pub fn route<H, T>(
        mut self,
        method: Method,
        path: &'static str,
        access: Access,
        handler: H,
    ) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        let scope = scope_of(path, access);
        let method_router: MethodRouter<S> = match method {
            Method::GET => routing::get(handler),
            Method::POST => routing::post(handler),
            Method::PUT => routing::put(handler),
            Method::PATCH => routing::patch(handler),
            Method::DELETE => routing::delete(handler),
            ref other => panic!("Nicht unterstützte HTTP-Methode {other} für {path}"),
        };
        self.router = self.router.route(path, method_router);
        self.rules.push(RouteRule {
            method,
            path,
            access,
            scope,
        });
        self
    }

    pub fn get<H, T>(self, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.route(Method::GET, path, access, handler)
    }

    pub fn post<H, T>(self, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.route(Method::POST, path, access, handler)
    }

    pub fn put<H, T>(self, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.route(Method::PUT, path, access, handler)
    }

    pub fn patch<H, T>(self, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.route(Method::PATCH, path, access, handler)
    }

    pub fn delete<H, T>(self, path: &'static str, access: Access, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        self.route(Method::DELETE, path, access, handler)
    }

    /// Die bisher registrierten Routen mit ihren Zugriffsregeln.
    pub fn rules(&self) -> &[RouteRule] {
        &self.rules
    }

    /// Bindet den Handler-State; danach ist der Router zustandslos und
    /// kann mit anderen Modulen zusammengeführt werden.
    pub fn with_state(self, state: S) -> ModuleRoutes<()> {
        ModuleRoutes {
            router: self.router.with_state(state),
            rules: self.rules,
        }
    }
}

impl ModuleRoutes<()> {
    /// Führt die Routen eines weiteren Moduls hinzu.
    pub fn merge(mut self, other: ModuleRoutes<()>) -> Self {
        self.router = self.router.merge(other.router);
        self.rules.extend(other.rules);
        self
    }

    pub fn into_parts(self) -> (Router, Vec<RouteRule>) {
        (self.router, self.rules)
    }
}

// ===========================================================================
// Zugriffs-Policy
// ===========================================================================

#[derive(Debug, Clone)]
enum Seg {
    Literal(&'static str),
    Param,
    Rest,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    method: Method,
    segs: Vec<Seg>,
    access: Access,
    scope: Scope,
    /// Index des Segments mit `:case_id` bzw. `:unit_id`
    scope_index: Option<usize>,
    specificity: usize,
}

/// Ergebnis der Policy für eine Anfrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub access: Access,
    pub scope: Scope,
    /// Wert des Pfadsegments `:case_id` bzw. `:unit_id` (ungeprüfter Text)
    pub scope_id: Option<String>,
}

/// Serverseitige Zugriffsregeln aller aktiven Module (deny-by-default).
#[derive(Debug, Clone, Default)]
pub struct AccessPolicy {
    rules: Vec<CompiledRule>,
}

impl AccessPolicy {
    pub fn new(rules: impl IntoIterator<Item = RouteRule>) -> Self {
        let rules = rules
            .into_iter()
            .map(|r| {
                let mut segs = Vec::new();
                let mut scope_index = None;
                for (i, s) in r.path.trim_matches('/').split('/').enumerate() {
                    if s.is_empty() {
                        continue;
                    }
                    if s == CASE_PARAM || s == UNIT_PARAM {
                        scope_index = Some(i);
                    }
                    segs.push(if s.starts_with('*') {
                        Seg::Rest
                    } else if s.starts_with(':') {
                        Seg::Param
                    } else {
                        Seg::Literal(s)
                    });
                }
                let literals = segs.iter().filter(|s| matches!(s, Seg::Literal(_))).count();
                let exact = !segs.iter().any(|s| matches!(s, Seg::Rest));
                CompiledRule {
                    method: r.method,
                    specificity: literals * 2 + usize::from(exact),
                    segs,
                    access: r.access,
                    scope: r.scope,
                    scope_index,
                }
            })
            .collect();
        Self { rules }
    }

    /// Anzahl der Regeln (für Statusanzeigen und Tests).
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Ermittelt, was eine Anfrage verlangt. Die spezifischste passende
    /// Regel gewinnt. Ohne passende Regel gilt: `/api/…` nur für Admins,
    /// sonstige GET-Anfragen öffentlich (statische Dateien bzw. 404),
    /// alles andere nur für Admins.
    pub fn decide(&self, method: &Method, path: &str) -> Decision {
        let method = if method == Method::HEAD {
            Method::GET
        } else {
            method.clone()
        };
        let segs: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let best = self
            .rules
            .iter()
            .filter(|r| r.method == method && matches(&r.segs, &segs))
            .fold(None::<&CompiledRule>, |best, r| match best {
                Some(b) if b.specificity >= r.specificity => Some(b),
                _ => Some(r),
            });

        if let Some(rule) = best {
            return Decision {
                access: rule.access,
                scope: rule.scope,
                scope_id: rule
                    .scope_index
                    .and_then(|i| segs.get(i))
                    .map(|s| s.to_string()),
            };
        }

        let is_api = segs.first() == Some(&"api");
        let access = if !is_api && method == Method::GET {
            Access::Public
        } else {
            Access::Permission(Permission::Admin)
        };
        Decision {
            access,
            scope: Scope::None,
            scope_id: None,
        }
    }
}

fn matches(pattern: &[Seg], path: &[&str]) -> bool {
    let mut i = 0;
    for seg in pattern {
        match seg {
            Seg::Rest => return true,
            Seg::Literal(l) => {
                if path.get(i) != Some(l) {
                    return false;
                }
            }
            Seg::Param => {
                if path.get(i).is_none() {
                    return false;
                }
            }
        }
        i += 1;
    }
    i == path.len()
}

// ===========================================================================
// Modul-Deskriptor und -Auswahl
// ===========================================================================

/// Reifegrad eines Moduls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    /// Standardmäßig aktiv.
    Stable,
    /// Nur aktiv, wenn in `[modules] experimental` ausdrücklich genannt.
    Experimental,
}

/// Selbstbeschreibung eines Moduls.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModuleInfo {
    /// Kennung für Konfiguration und Abhängigkeiten, z. B. `cases`
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Version der Modul-Crate (`env!("CARGO_PKG_VERSION")`)
    pub version: &'static str,
    /// Kennungen der Module, die aktiv sein müssen
    pub depends_on: &'static [&'static str],
    pub maturity: Maturity,
    /// Kernmodul: Teil des gemeinsamen Vertrags, nicht abschaltbar
    pub kernel: bool,
}

/// Gemeinsame Abhängigkeiten, die jedes Modul erhält.
#[derive(Clone)]
pub struct ModuleContext {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub config: Arc<AppConfig>,
    /// Rollenmatrix und Laufzeit-Einstellungen
    pub platform: Arc<Platform>,
    /// Ereignisse an andere Module und Schnittstellen
    pub events: Arc<EventBus>,
}

impl ModuleContext {
    /// Schreib-Pool (maßgeblicher Stand). Für alles, was Entscheidungen
    /// trifft oder schreibt.
    pub fn pool(&self) -> &sqlx::PgPool {
        self.db.write()
    }

    /// Lese-Pool (kann eine Replica mit Verzögerung sein). Nur für
    /// Auswertungen, bei denen leicht veraltete Daten unkritisch sind.
    pub fn read_pool(&self) -> &sqlx::PgPool {
        self.db.read()
    }
}

/// Ein Fachmodul.
pub trait Module: Send + Sync {
    fn info(&self) -> ModuleInfo;
    /// Baut die Routen. Wird nur für aktive Module aufgerufen.
    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes;
    /// Reaktionen auf Ereignisse anderer Module (Standard: keine).
    fn subscribers(&self, _ctx: &ModuleContext) -> Vec<Arc<dyn EventSubscriber>> {
        Vec::new()
    }
}

/// Bestimmt, welche Module aktiv sind. Fehler bei unbekannten Kennungen in
/// der Konfiguration, bei widersprüchlichen Angaben und bei aktiven Modulen
/// mit inaktiver Abhängigkeit — der Start bricht dann ab.
pub fn resolve_enabled(infos: &[ModuleInfo], cfg: &ModulesConfig) -> anyhow::Result<Vec<bool>> {
    let known = |id: &str| infos.iter().any(|m| m.id == id);
    for id in cfg.disabled.iter().chain(cfg.experimental.iter()) {
        if !known(id) {
            anyhow::bail!("[modules]: unbekanntes Modul „{id}“");
        }
    }
    for id in &cfg.disabled {
        if cfg.experimental.contains(id) {
            anyhow::bail!("[modules]: „{id}“ steht in disabled und experimental");
        }
    }
    let mut seen = std::collections::HashSet::new();
    for m in infos {
        if !seen.insert(m.id) {
            anyhow::bail!("Modulkennung „{}“ ist doppelt vergeben", m.id);
        }
    }

    for m in infos.iter().filter(|m| m.kernel) {
        if cfg.disabled.iter().any(|d| d == m.id) {
            anyhow::bail!(
                "[modules]: Kernmodul „{}“ kann nicht abgeschaltet werden",
                m.id
            );
        }
    }

    let enabled: Vec<bool> = infos
        .iter()
        .map(|m| match m.maturity {
            _ if m.kernel => true,
            Maturity::Stable => !cfg.disabled.iter().any(|d| d == m.id),
            Maturity::Experimental => cfg.experimental.iter().any(|e| e == m.id),
        })
        .collect();

    for (m, on) in infos.iter().zip(&enabled) {
        if !on {
            continue;
        }
        for dep in m.depends_on {
            let dep_on = infos
                .iter()
                .zip(&enabled)
                .find(|(i, _)| i.id == *dep)
                .map(|(_, on)| *on);
            match dep_on {
                Some(true) => {}
                Some(false) => {
                    anyhow::bail!("Modul „{}“ benötigt „{dep}“, das abgeschaltet ist", m.id)
                }
                None => anyhow::bail!("Modul „{}“ nennt unbekannte Abhängigkeit „{dep}“", m.id),
            }
        }
    }
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ok() -> &'static str {
        "ok"
    }

    fn sample() -> ModuleRoutes {
        ModuleRoutes::<()>::new()
            .get("/health", Access::Public, ok)
            .get("/static/*path", Access::Public, ok)
            .get(
                "/api/v1/patients",
                Access::Permission(Permission::PatientRead),
                ok,
            )
            .get(
                "/api/v1/patients/:id",
                Access::Permission(Permission::PatientRead),
                ok,
            )
            .post(
                "/api/v1/patients/:id/hard-delete",
                Access::Permission(Permission::Admin),
                ok,
            )
            .get(
                "/api/v1/cases/:case_id/notes",
                Access::AnyOf(&[Permission::ClinicalRead, Permission::NursingRead]),
                ok,
            )
            .get(
                "/api/v1/units/:unit_id/worklist",
                Access::Permission(Permission::OrderProcess),
                ok,
            )
            .get("/api/v1/me", Access::Authenticated, ok)
    }

    fn policy() -> AccessPolicy {
        AccessPolicy::new(sample().into_parts().1)
    }

    #[test]
    fn unknown_api_routes_are_admin_only() {
        let p = policy();
        for (m, path) in [
            (Method::GET, "/api/v1/unknown"),
            (Method::POST, "/api/v1/patients"),
            (Method::DELETE, "/api/v1/patients/1"),
            (Method::GET, "/api/patients"),
            (Method::POST, "/static/x"),
            (Method::PUT, "/"),
        ] {
            assert_eq!(
                p.decide(&m, path).access,
                Access::Permission(Permission::Admin),
                "{m} {path}"
            );
        }
    }

    #[test]
    fn public_and_static() {
        let p = policy();
        assert_eq!(p.decide(&Method::GET, "/health").access, Access::Public);
        assert_eq!(p.decide(&Method::HEAD, "/health").access, Access::Public);
        assert_eq!(
            p.decide(&Method::GET, "/static/js/app.js").access,
            Access::Public
        );
        assert_eq!(p.decide(&Method::GET, "/").access, Access::Public);
    }

    #[test]
    fn most_specific_rule_wins_and_trailing_slash_is_ignored() {
        let p = policy();
        assert_eq!(
            p.decide(&Method::GET, "/api/v1/patients/").access,
            Access::Permission(Permission::PatientRead)
        );
        assert_eq!(
            p.decide(&Method::POST, "/api/v1/patients/7/hard-delete")
                .access,
            Access::Permission(Permission::Admin)
        );
        // Zu lange Pfade passen nicht auf kürzere Muster.
        assert_eq!(
            p.decide(&Method::GET, "/api/v1/patients/7/extra").access,
            Access::Permission(Permission::Admin)
        );
    }

    #[test]
    fn scope_is_derived_from_path() {
        let p = policy();
        let d = p.decide(&Method::GET, "/api/v1/cases/abc/notes");
        assert_eq!(d.scope, Scope::Case);
        assert_eq!(d.scope_id.as_deref(), Some("abc"));
        let d = p.decide(&Method::GET, "/api/v1/units/u1/worklist");
        assert_eq!(d.scope, Scope::Unit);
        assert_eq!(d.scope_id.as_deref(), Some("u1"));
        assert_eq!(p.decide(&Method::GET, "/api/v1/me").scope, Scope::None);
    }

    #[test]
    fn access_allows() {
        let arzt = AuthUser::with_builtin_role("u", "arzt");
        let viewer = AuthUser::with_builtin_role("v", "viewer");
        let any = Access::AnyOf(&[Permission::ClinicalRead, Permission::NursingRead]);
        assert!(any.allows(&arzt));
        assert!(!any.allows(&viewer));
        assert!(Access::Authenticated.allows(&viewer));
        assert!(!Access::Permission(Permission::Admin).allows(&arzt));
    }

    #[test]
    #[should_panic(expected = "muss der Parameter :case_id heißen")]
    fn case_routes_must_use_case_id() {
        let _ = ModuleRoutes::<()>::new().get("/api/v1/cases/:id/notes", Access::Authenticated, ok);
    }

    #[test]
    #[should_panic(expected = "darf nicht öffentlich sein")]
    fn scoped_routes_cannot_be_public() {
        let _ = ModuleRoutes::<()>::new().get("/api/v1/cases/:case_id", Access::Public, ok);
    }

    fn info(id: &'static str, deps: &'static [&'static str], maturity: Maturity) -> ModuleInfo {
        ModuleInfo {
            id,
            title: id,
            description: "",
            version: "0",
            depends_on: deps,
            maturity,
            kernel: false,
        }
    }

    #[test]
    fn module_selection() {
        let infos = vec![
            info("org", &[], Maturity::Stable),
            info("cases", &["org"], Maturity::Stable),
            info("cds", &["cases"], Maturity::Experimental),
        ];
        let cfg = ModulesConfig::default();
        assert_eq!(resolve_enabled(&infos, &cfg).unwrap(), [true, true, false]);

        let cfg = ModulesConfig {
            experimental: vec!["cds".into()],
            ..Default::default()
        };
        assert_eq!(resolve_enabled(&infos, &cfg).unwrap(), [true, true, true]);

        // Abhängigkeit abgeschaltet → Fehler statt stiller Teilfunktion
        let cfg = ModulesConfig {
            disabled: vec!["org".into()],
            ..Default::default()
        };
        assert!(resolve_enabled(&infos, &cfg).is_err());

        let cfg = ModulesConfig {
            disabled: vec!["org".into(), "cases".into()],
            ..Default::default()
        };
        assert_eq!(
            resolve_enabled(&infos, &cfg).unwrap(),
            [false, false, false]
        );

        // Tippfehler in der Konfiguration
        let cfg = ModulesConfig {
            disabled: vec!["cass".into()],
            ..Default::default()
        };
        assert!(resolve_enabled(&infos, &cfg).is_err());

        // Kernmodule lassen sich nicht abschalten
        let mut kern = infos.clone();
        kern[0].kernel = true;
        let cfg = ModulesConfig {
            disabled: vec!["org".into(), "cases".into()],
            ..Default::default()
        };
        assert!(resolve_enabled(&kern, &cfg).is_err());
    }
}
