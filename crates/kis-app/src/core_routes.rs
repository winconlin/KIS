//! Kern-Routen: Anmeldung, eigener Benutzer, Status, Module,
//! Notfallzugriff, Ereignis-Outbox für Schnittstellen.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use kis_core::auth::{self, AuthError};
use kis_core::events;
use kis_core::http::{actor_id, audit, ApiFailure, ApiResult};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, ModuleRoutes};
use kis_core::rbac::Permission as P;
use kis_core::treatment;

use crate::state::AppState;

pub fn routes() -> ModuleRoutes<AppState> {
    ModuleRoutes::new()
        .get("/health", Access::Public, health)
        .get("/api/v1/health", Access::Public, health)
        .get(
            "/api/v1/health/db",
            Access::Permission(P::ConfigRead),
            db_health,
        )
        .get("/api/v1/status", Access::Authenticated, status)
        .get("/api/v1/modules", Access::Authenticated, modules)
        .get("/api/v1/routes", Access::Authenticated, routes_overview)
        .get("/api/v1/me", Access::Authenticated, me)
        .post("/api/v1/auth/login", Access::Public, login)
        .post("/api/v1/auth/refresh", Access::Public, refresh)
        .post("/api/v1/auth/logout", Access::Authenticated, logout)
        .get("/api/v1/auth/sessions", Access::Authenticated, sessions)
        .post(
            "/api/v1/auth/sessions/:id/revoke",
            Access::Authenticated,
            revoke_session,
        )
        .post(
            "/api/v1/emergency-access",
            Access::Permission(P::BreakGlass),
            emergency_access,
        )
        .get(
            "/api/v1/interfaces/events",
            Access::Permission(P::InterfaceConsume),
            read_events,
        )
        .post(
            "/api/v1/interfaces/events/ack",
            Access::Permission(P::InterfaceConsume),
            ack_events,
        )
}

async fn health() -> &'static str {
    "ok"
}

async fn db_health(State(st): State<AppState>) -> Json<serde_json::Value> {
    let (write, read) = st.ctx.db.pool_metrics();
    let replica = st.ctx.db.replica_status().await;
    Json(serde_json::json!({ "writePool": write, "readPool": read, "replica": replica }))
}

async fn status(State(st): State<AppState>) -> Json<serde_json::Value> {
    let active: Vec<&str> = st
        .modules
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.info.id)
        .collect();
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "status": "running",
        "activeModules": active,
    }))
}

/// Alle Module mit tatsächlichem Zustand dieser Instanz.
async fn modules(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "modules": *st.modules,
        "eventSubscribers": st.ctx.events.subscriber_names(),
    }))
}

/// Eigener Benutzer: Rolle, wirksame Rechte, Einheiten. Die Oberfläche
/// blendet danach Funktionen ein oder aus; maßgeblich bleibt der Server.
/// Alle Routen dieser Instanz mit dem jeweils verlangten Recht — die
/// API beschreibt sich selbst (Grundlage für `docs/API.md`).
async fn routes_overview(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "count": st.routes.len(), "routes": *st.routes }))
}

async fn me(State(st): State<AppState>, user: CurrentUser) -> ApiResult<serde_json::Value> {
    let units = treatment::unit_ids_of(st.pool(), &user.auth).await?;
    Ok(Json(serde_json::json!({
        "userId": user.auth.user_id,
        "role": user.auth.role,
        "permissions": user.auth.permissions.keys(),
        "unitIds": units,
        "treatmentContextEnforced": st.ctx.platform.policies().treatment_enforce,
    })))
}

// --- Anmeldung -------------------------------------------------------------

#[derive(Deserialize)]
struct LoginRequest {
    /// Benutzername oder E-Mail (Feld `email` für die bestehende Oberfläche)
    #[serde(alias = "email", alias = "username")]
    login: String,
    password: String,
    #[serde(default)]
    totp_code: Option<String>,
}

#[derive(Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "success": false, "error": msg, "status": status.as_u16() })),
    )
        .into_response()
}

pub(crate) fn auth_error(e: AuthError) -> Response {
    match e {
        AuthError::Internal(inner) => {
            tracing::error!(error = %inner, "Authentifizierung: interner Fehler");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Interner Serverfehler")
        }
        AuthError::Policy(msg) => json_error(StatusCode::BAD_REQUEST, &msg),
        AuthError::MfaRequired => json_error(StatusCode::UNAUTHORIZED, "MFA-Code erforderlich"),
        // Gesperrt und falsche Daten bewusst gleich beantworten (keine
        // Konto-Aufzählung über unterschiedliche Antworten).
        AuthError::Locked | AuthError::InvalidCredentials => {
            json_error(StatusCode::UNAUTHORIZED, "Ungültige Anmeldedaten")
        }
        AuthError::InvalidToken => json_error(
            StatusCode::UNAUTHORIZED,
            "Ungültiger oder abgelaufener Token",
        ),
    }
}

fn token_response(pair: auth::TokenPair) -> Response {
    Json(serde_json::json!({
        "access_token": pair.access_token,
        "refresh_token": pair.refresh_token,
        "tokenType": pair.token_type,
        "expiresIn": pair.expires_in,
        "sessionId": pair.session_id,
        "user": { "id": pair.user_id, "role": pair.role },
    }))
    .into_response()
}

async fn login(State(st): State<AppState>, Json(req): Json<LoginRequest>) -> Response {
    match auth::login(
        st.pool(),
        &st.ctx.security,
        &st.ctx.platform,
        req.login.trim(),
        &req.password,
        req.totp_code.as_deref(),
        None,
    )
    .await
    {
        Ok(pair) => token_response(pair),
        Err(e) => auth_error(e),
    }
}

/// Rotiert Refresh- und Access-Token.
async fn refresh(State(st): State<AppState>, Json(req): Json<RefreshRequest>) -> Response {
    match auth::refresh(
        st.pool(),
        &st.ctx.security,
        &st.ctx.platform,
        &req.refresh_token,
        None,
    )
    .await
    {
        Ok(pair) => token_response(pair),
        Err(e) => auth_error(e),
    }
}

/// Widerruft die aktuelle Sitzung.
async fn logout(State(st): State<AppState>, user: CurrentUser) -> Response {
    let Some(sid) = user.auth.session_id else {
        return json_error(StatusCode::BAD_REQUEST, "Token ist nicht sitzungsgebunden");
    };
    match auth::revoke_session(st.pool(), &st.ctx.security, sid, &user.auth, None).await {
        Ok(_) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => auth_error(e),
    }
}

async fn sessions(State(st): State<AppState>, user: CurrentUser) -> Response {
    let Some(uid) = user.auth.uuid() else {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Interner Serverfehler");
    };
    match auth::list_sessions(st.pool(), uid).await {
        Ok(list) => {
            Json(serde_json::json!({ "count": list.len(), "sessions": list })).into_response()
        }
        Err(e) => auth_error(e),
    }
}

/// Eigene Sitzung beenden; mit Admin-Recht jede Sitzung.
async fn revoke_session(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Response {
    let owner = if user.can(&P::Admin) {
        None
    } else {
        match user.auth.uuid() {
            Some(u) => Some(u),
            None => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Interner Serverfehler"),
        }
    };
    match auth::revoke_session(st.pool(), &st.ctx.security, id, &user.auth, owner).await {
        Ok(true) => Json(serde_json::json!({ "success": true })).into_response(),
        Ok(false) => json_error(StatusCode::NOT_FOUND, "Sitzung nicht gefunden"),
        Err(e) => auth_error(e),
    }
}

// --- Notfallzugriff ---------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmergencyRequest {
    case_id: Uuid,
    reason: String,
}

/// Notfallzugriff auf einen Fall außerhalb des Behandlungszusammenhangs.
/// Zeitlich begrenzt, mit Pflichtbegründung, auditiert und nachträglich
/// zu prüfen (`/api/v1/admin/emergency-access`).
async fn emergency_access(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(req): Json<EmergencyRequest>,
) -> ApiResult<serde_json::Value> {
    if req.reason.trim().chars().count() < 10 {
        return Err(ApiFailure::bad_request(
            "Begründung mit mindestens 10 Zeichen erforderlich",
        ));
    }
    let minutes = st.ctx.platform.policies().emergency_minutes;
    let mut tx = st.pool().begin().await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM cases WHERE id = $1)")
        .bind(req.case_id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(ApiFailure::NotFound("Fall nicht gefunden"));
    }
    let grant = treatment::grant_emergency_access(
        &mut tx,
        actor_id(&user.auth)?,
        req.case_id,
        &req.reason,
        minutes,
    )
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "access.emergency_granted",
        "case",
        req.case_id,
        Some(serde_json::json!({ "grantId": grant, "minutes": minutes })),
    )
    .await?;
    tx.commit().await?;
    tracing::warn!(user = %user.auth.user_id, case = %req.case_id, "Notfallzugriff gewährt");
    Ok(Json(
        serde_json::json!({ "grantId": grant, "validMinutes": minutes }),
    ))
}

// --- Ereignis-Outbox --------------------------------------------------------

#[derive(Deserialize)]
struct EventsQuery {
    consumer: String,
    limit: Option<i64>,
}

fn consumer_name(name: &str) -> Result<&str, ApiFailure> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if ok {
        Ok(name)
    } else {
        Err(ApiFailure::bad_request(
            "consumer: 1–64 Zeichen aus a–z, 0–9, _ und -",
        ))
    }
}

/// Ereignisse nach dem Cursor des Konsumenten (verschiebt ihn nicht).
async fn read_events(
    State(st): State<AppState>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<serde_json::Value> {
    let consumer = consumer_name(&q.consumer)?;
    let list = events::read_events(st.pool(), consumer, q.limit.unwrap_or(100)).await?;
    Ok(Json(
        serde_json::json!({ "count": list.len(), "events": list }),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AckRequest {
    consumer: String,
    up_to: i64,
}

/// Bestätigt die Verarbeitung bis einschließlich `upTo`.
async fn ack_events(
    State(st): State<AppState>,
    Json(req): Json<AckRequest>,
) -> ApiResult<serde_json::Value> {
    let consumer = consumer_name(&req.consumer)?;
    let cursor = events::acknowledge(st.pool(), consumer, req.up_to).await?;
    Ok(Json(
        serde_json::json!({ "consumer": consumer, "cursor": cursor }),
    ))
}
