//! Verwaltung der Plattform: Rollen und Rechte, Laufzeit-Einstellungen,
//! Prüfung von Notfallzugriffen, fehlgeschlagene Folgeaktionen.
//!
//! Änderungen an Rollen und Einstellungen erhöhen den Versionszähler in
//! `platform_state`; alle Instanzen übernehmen sie bei der nächsten
//! Anfrage. Jede Änderung wird in derselben Transaktion auditiert.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use kis_core::http::{actor_id, audit, required_text, ApiFailure, ApiResult};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, ModuleRoutes};
use kis_core::platform::{self, SETTINGS};
use kis_core::rbac::{self, Permission as P, ADMIN};

use crate::state::AppState;

pub fn routes() -> ModuleRoutes<AppState> {
    let admin = Access::Permission(P::Admin);
    let audit_read = Access::Permission(P::AuditRead);
    ModuleRoutes::new()
        .get("/api/v1/admin/permissions", admin, list_permissions)
        .get("/api/v1/admin/roles", admin, list_roles)
        .put("/api/v1/admin/roles/:key", admin, put_role)
        .delete("/api/v1/admin/roles/:key", admin, delete_role)
        .get("/api/v1/admin/settings", admin, list_settings)
        .put("/api/v1/admin/settings/:key", admin, put_setting)
        .get("/api/v1/admin/emergency-access", audit_read, list_emergency)
        .post(
            "/api/v1/admin/emergency-access/:id/review",
            audit_read,
            review_emergency,
        )
        .get("/api/v1/admin/event-failures", admin, list_event_failures)
        .post(
            "/api/v1/admin/event-failures/:id/resolve",
            admin,
            resolve_event_failure,
        )
}

// --- Rollen und Rechte -------------------------------------------------------

/// Alle Rechte, die diese Programmversion kennt.
async fn list_permissions() -> Json<serde_json::Value> {
    let keys: Vec<&str> = P::ALL.iter().map(|p| p.key()).collect();
    Json(serde_json::json!({ "permissions": keys }))
}

async fn list_roles(State(st): State<AppState>) -> ApiResult<serde_json::Value> {
    let rows = sqlx::query(
        "SELECT r.key, r.label, r.description, r.is_system,
                ARRAY(SELECT permission FROM role_permissions p
                      WHERE p.role_key = r.key ORDER BY permission) AS permissions,
                (SELECT count(*) FROM users u WHERE u.role = r.key AND u.deleted_at IS NULL)
                  + (SELECT count(*) FROM user_roles x WHERE x.role_key = r.key) AS users
         FROM roles r ORDER BY r.key",
    )
    .fetch_all(st.pool())
    .await?;
    let roles: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let key: String = r.get("key");
            // admin hat immer alle Rechte, unabhängig von den Zeilen
            let permissions: Vec<String> = if key == ADMIN {
                P::ALL.iter().map(|p| p.key().to_string()).collect()
            } else {
                r.get("permissions")
            };
            serde_json::json!({
                "key": key,
                "label": r.get::<String, _>("label"),
                "description": r.get::<Option<String>, _>("description"),
                "isSystem": r.get::<bool, _>("is_system"),
                "permissions": permissions,
                "users": r.get::<i64, _>("users"),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "roles": roles })))
}

#[derive(Deserialize)]
struct RoleBody {
    label: String,
    description: Option<String>,
    permissions: Vec<String>,
}

/// Legt eine Rolle an oder ersetzt Bezeichnung und Rechte einer
/// vorhandenen. Unbekannte Rechteschlüssel werden abgelehnt.
async fn put_role(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(body): Json<RoleBody>,
) -> ApiResult<serde_json::Value> {
    if key == ADMIN {
        return Err(ApiFailure::conflict(
            "Die Rolle admin hat immer alle Rechte und kann nicht geändert werden",
        ));
    }
    let label = required_text("label", &body.label)?;
    if let Some(unknown) = body.permissions.iter().find(|k| P::from_key(k).is_none()) {
        return Err(ApiFailure::bad_request(format!(
            "Unbekanntes Recht: {unknown}"
        )));
    }
    let mut tx = st.pool().begin().await?;
    sqlx::query(
        "INSERT INTO roles (key, label, description, seeded) VALUES ($1, $2, $3, TRUE)
         ON CONFLICT (key) DO UPDATE
         SET label = EXCLUDED.label, description = EXCLUDED.description,
             seeded = TRUE, updated_at = now()",
    )
    .bind(&key)
    .bind(&label)
    .bind(&body.description)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM role_permissions WHERE role_key = $1")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO role_permissions (role_key, permission)
         SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
    )
    .bind(&key)
    .bind(&body.permissions)
    .execute(&mut *tx)
    .await?;
    rbac::bump_rbac_version(&mut tx).await?;
    st.ctx
        .security
        .log_audit_tx(
            &mut tx,
            &user.auth.user_id,
            "role.put",
            "role",
            Some(&key),
            None,
            Some(&serde_json::json!({ "permissions": body.permissions })),
            None,
        )
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Löscht eine hauseigene Rolle, die niemandem mehr zugeordnet ist.
async fn delete_role(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut tx = st.pool().begin().await?;
    let row = sqlx::query("SELECT is_system FROM roles WHERE key = $1 FOR UPDATE")
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiFailure::NotFound("Rolle nicht gefunden"))?;
    if row.get::<bool, _>("is_system") {
        return Err(ApiFailure::conflict(
            "Rollen der Grundausstattung können nicht gelöscht werden",
        ));
    }
    let in_use: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users WHERE role = $1)
             OR EXISTS (SELECT 1 FROM user_roles WHERE role_key = $1)",
    )
    .bind(&key)
    .fetch_one(&mut *tx)
    .await?;
    if in_use {
        return Err(ApiFailure::conflict(
            "Die Rolle ist noch Benutzern zugeordnet",
        ));
    }
    sqlx::query("DELETE FROM roles WHERE key = $1")
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    rbac::bump_rbac_version(&mut tx).await?;
    st.ctx
        .security
        .log_audit_tx(
            &mut tx,
            &user.auth.user_id,
            "role.delete",
            "role",
            Some(&key),
            None,
            None,
            None,
        )
        .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

// --- Einstellungen -----------------------------------------------------------

async fn list_settings(State(st): State<AppState>) -> Json<serde_json::Value> {
    let policies = st.ctx.platform.policies();
    let settings: Vec<serde_json::Value> = SETTINGS
        .iter()
        .map(|def| {
            serde_json::json!({
                "key": def.key,
                "label": def.label,
                "kind": def.kind,
                "value": policies.value_of(def.key),
            })
        })
        .collect();
    Json(serde_json::json!({ "settings": settings }))
}

#[derive(Deserialize)]
struct SettingBody {
    value: serde_json::Value,
}

async fn put_setting(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(key): Path<String>,
    Json(body): Json<SettingBody>,
) -> ApiResult<serde_json::Value> {
    platform::validate_setting(&key, &body.value).map_err(ApiFailure::BadRequest)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = st.pool().begin().await?;
    platform::write_setting(&mut tx, &key, &body.value, actor)
        .await
        .map_err(ApiFailure::Internal)?;
    st.ctx
        .security
        .log_audit_tx(
            &mut tx,
            &user.auth.user_id,
            "setting.put",
            "setting",
            Some(&key),
            None,
            Some(&serde_json::json!({ "value": body.value })),
            None,
        )
        .await?;
    tx.commit().await?;
    // Diese Instanz sofort aktualisieren; andere folgen bei ihrer nächsten Anfrage.
    st.ctx.platform.reload(st.pool()).await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

// --- Notfallzugriffe prüfen --------------------------------------------------

#[derive(Deserialize)]
struct EmergencyQuery {
    /// `true` (Standard): nur ungeprüfte
    open: Option<bool>,
}

async fn list_emergency(
    State(st): State<AppState>,
    Query(q): Query<EmergencyQuery>,
) -> ApiResult<serde_json::Value> {
    let rows = sqlx::query(
        "SELECT g.id, g.user_id, u.username, g.case_id, c.case_number, g.reason,
                g.granted_at, g.expires_at, g.reviewed_by, g.reviewed_at, g.review_note
         FROM emergency_access_grants g
         JOIN users u ON u.id = g.user_id
         JOIN cases c ON c.id = g.case_id
         WHERE ($1 = FALSE OR g.reviewed_at IS NULL)
         ORDER BY g.granted_at DESC LIMIT 500",
    )
    .bind(q.open.unwrap_or(true))
    .fetch_all(st.pool())
    .await?;
    let grants: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "userId": r.get::<Uuid, _>("user_id"),
                "username": r.get::<String, _>("username"),
                "caseId": r.get::<Uuid, _>("case_id"),
                "caseNumber": r.get::<String, _>("case_number"),
                "reason": r.get::<String, _>("reason"),
                "grantedAt": r.get::<chrono::DateTime<chrono::Utc>, _>("granted_at"),
                "expiresAt": r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at"),
                "reviewedBy": r.get::<Option<Uuid>, _>("reviewed_by"),
                "reviewedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("reviewed_at"),
                "reviewNote": r.get::<Option<String>, _>("review_note"),
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "count": grants.len(), "grants": grants }),
    ))
}

#[derive(Deserialize)]
struct ReviewBody {
    note: Option<String>,
}

/// Vermerkt die Prüfung eines Notfallzugriffs. Niemand prüft den eigenen.
async fn review_emergency(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ReviewBody>,
) -> ApiResult<serde_json::Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = st.pool().begin().await?;
    let updated = sqlx::query(
        "UPDATE emergency_access_grants
         SET reviewed_by = $2, reviewed_at = now(), review_note = $3
         WHERE id = $1 AND reviewed_at IS NULL AND user_id <> $2",
    )
    .bind(id)
    .bind(actor)
    .bind(&body.note)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiFailure::conflict(
            "Nicht gefunden, bereits geprüft oder eigener Zugriff",
        ));
    }
    audit(
        &mut tx,
        &user.auth,
        "access.emergency_reviewed",
        "emergency_access",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

// --- Fehlgeschlagene Folgeaktionen ------------------------------------------

async fn list_event_failures(State(st): State<AppState>) -> ApiResult<serde_json::Value> {
    let rows = sqlx::query(
        "SELECT f.id, f.event_id, e.topic, e.entity_type, e.entity_id, f.subscriber,
                f.error, f.occurred_at
         FROM event_failures f JOIN domain_events e ON e.id = f.event_id
         WHERE f.resolved_at IS NULL ORDER BY f.occurred_at DESC LIMIT 500",
    )
    .fetch_all(st.pool())
    .await?;
    let failures: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<i64, _>("id"),
                "eventId": r.get::<i64, _>("event_id"),
                "topic": r.get::<String, _>("topic"),
                "entityType": r.get::<String, _>("entity_type"),
                "entityId": r.get::<Uuid, _>("entity_id"),
                "subscriber": r.get::<String, _>("subscriber"),
                "error": r.get::<String, _>("error"),
                "occurredAt": r.get::<chrono::DateTime<chrono::Utc>, _>("occurred_at"),
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "count": failures.len(), "failures": failures }),
    ))
}

async fn resolve_event_failure(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let actor = actor_id(&user.auth)?;
    let updated = sqlx::query(
        "UPDATE event_failures SET resolved_at = now(), resolved_by = $2
         WHERE id = $1 AND resolved_at IS NULL",
    )
    .bind(id)
    .bind(actor)
    .execute(st.pool())
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiFailure::NotFound(
            "Eintrag nicht gefunden oder bereits erledigt",
        ));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}
