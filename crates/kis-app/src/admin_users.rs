//! Benutzerverwaltung: Konten, Zusatzrollen, Zuordnung zu Einheiten.
//!
//! Alle Routen erfordern das Recht `admin`. Jede Änderung wird in
//! derselben Transaktion auditiert. Mindestens ein aktives Konto mit der
//! Rolle `admin` bleibt immer erhalten.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use kis_core::auth::{self, NewUser};
use kis_core::http::{actor_id, audit, ApiFailure, ApiResult};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, ModuleRoutes};
use kis_core::rbac::{Permission as P, ADMIN};

use crate::core_routes::auth_error;
use crate::state::AppState;

pub fn routes() -> ModuleRoutes<AppState> {
    let admin = Access::Permission(P::Admin);
    ModuleRoutes::new()
        .get("/api/v1/admin/users", admin, list_users)
        .post("/api/v1/admin/users", admin, create_user)
        .put("/api/v1/admin/users/:id", admin, update_user)
        .put("/api/v1/admin/users/:id/roles", admin, set_extra_roles)
        .put("/api/v1/admin/users/:id/units", admin, set_units)
        .post("/api/v1/admin/users/:id/password", admin, reset_password)
}

async fn list_users(State(st): State<AppState>) -> ApiResult<serde_json::Value> {
    let rows = sqlx::query(
        "SELECT u.id, u.username, u.display_name, u.email, u.role, u.is_active,
                u.last_login_at, u.locked_until,
                ARRAY(SELECT role_key FROM user_roles r WHERE r.user_id = u.id ORDER BY role_key)
                    AS extra_roles,
                ARRAY(SELECT unit_id FROM user_org_units o
                      WHERE o.user_id = u.id AND o.valid_until IS NULL) AS unit_ids
         FROM users u WHERE u.deleted_at IS NULL ORDER BY u.username",
    )
    .fetch_all(st.pool())
    .await?;
    let users: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<Uuid, _>("id"),
                "username": r.get::<String, _>("username"),
                "displayName": r.get::<String, _>("display_name"),
                "email": r.get::<String, _>("email"),
                "role": r.get::<String, _>("role"),
                "isActive": r.get::<bool, _>("is_active"),
                "lastLoginAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_login_at"),
                "lockedUntil": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("locked_until"),
                "extraRoles": r.get::<Vec<String>, _>("extra_roles"),
                "unitIds": r.get::<Vec<Uuid>, _>("unit_ids"),
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "count": users.len(), "users": users }),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUser {
    username: String,
    email: String,
    display_name: String,
    role: String,
    password: String,
}

async fn create_user(
    State(st): State<AppState>,
    user: CurrentUser,
    Json(body): Json<CreateUser>,
) -> Response {
    let new = NewUser {
        username: &body.username,
        email: &body.email,
        display_name: &body.display_name,
        role: &body.role,
        password: &body.password,
    };
    match auth::create_user(
        st.pool(),
        &st.ctx.security,
        &st.ctx.platform,
        new,
        &user.auth.user_id,
    )
    .await
    {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => auth_error(e),
    }
}

/// Verhindert, dass das letzte aktive Admin-Konto verloren geht.
async fn ensure_admin_remains(tx: &mut sqlx::PgConnection) -> Result<(), ApiFailure> {
    let admins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users u
         WHERE u.is_active AND u.deleted_at IS NULL
           AND (u.role = $1 OR EXISTS (SELECT 1 FROM user_roles r
                                       WHERE r.user_id = u.id AND r.role_key = $1))",
    )
    .bind(ADMIN)
    .fetch_one(tx)
    .await?;
    if admins == 0 {
        return Err(ApiFailure::conflict(
            "Mindestens ein aktives Konto mit der Rolle admin muss bestehen bleiben",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateUser {
    role: Option<String>,
    is_active: Option<bool>,
    display_name: Option<String>,
}

async fn update_user(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUser>,
) -> ApiResult<serde_json::Value> {
    let mut tx = st.pool().begin().await?;
    let updated = sqlx::query(
        "UPDATE users SET role = COALESCE($2, role),
                          is_active = COALESCE($3, is_active),
                          display_name = COALESCE($4, display_name),
                          updated_at = now(), version = version + 1
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(&body.role)
    .bind(body.is_active)
    .bind(body.display_name.as_deref().map(str::trim))
    .execute(&mut *tx)
    .await
    .map_err(|e| match kis_core::http::violated_constraint(&e) {
        Some("users_role_fk") => ApiFailure::bad_request("Unbekannte Rolle"),
        _ => e.into(),
    })?;
    if updated.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Benutzer nicht gefunden"));
    }
    if body.is_active == Some(false) {
        // Deaktivierung beendet alle Sitzungen sofort.
        sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    ensure_admin_remains(&mut tx).await?;
    audit(
        &mut tx,
        &user.auth,
        "user.update",
        "user",
        id,
        Some(serde_json::json!({ "role": body.role, "isActive": body.is_active })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct ExtraRoles {
    roles: Vec<String>,
}

/// Ersetzt die Zusatzrollen eines Benutzers.
async fn set_extra_roles(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ExtraRoles>,
) -> ApiResult<serde_json::Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = st.pool().begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiFailure::NotFound("Benutzer nicht gefunden"));
    }
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1 AND NOT (role_key = ANY($2))")
        .bind(id)
        .bind(&body.roles)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_key, granted_by)
         SELECT $1, unnest($2::text[]), $3 ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&body.roles)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(|e| match kis_core::http::violated_constraint(&e) {
        Some("user_roles_role_key_fkey") => ApiFailure::bad_request("Unbekannte Rolle"),
        _ => e.into(),
    })?;
    ensure_admin_remains(&mut tx).await?;
    audit(
        &mut tx,
        &user.auth,
        "user.extra_roles",
        "user",
        id,
        Some(serde_json::json!({ "roles": body.roles })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Units {
    unit_ids: Vec<Uuid>,
}

/// Ersetzt die Zuordnung zu Organisationseinheiten. Entfernte Zuordnungen
/// werden beendet (nicht gelöscht), damit nachvollziehbar bleibt, wer
/// wann zu welcher Einheit gehörte.
async fn set_units(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<Units>,
) -> ApiResult<serde_json::Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = st.pool().begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM users WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiFailure::NotFound("Benutzer nicht gefunden"));
    }
    sqlx::query(
        "UPDATE user_org_units SET valid_until = now()
         WHERE user_id = $1 AND valid_until IS NULL AND NOT (unit_id = ANY($2))",
    )
    .bind(id)
    .bind(&body.unit_ids)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO user_org_units (user_id, unit_id, assigned_by)
         SELECT $1, u, $3 FROM unnest($2::uuid[]) AS u
         WHERE NOT EXISTS (SELECT 1 FROM user_org_units o
                           WHERE o.user_id = $1 AND o.unit_id = u AND o.valid_until IS NULL)",
    )
    .bind(id)
    .bind(&body.unit_ids)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "user.units",
        "user",
        id,
        Some(serde_json::json!({ "unitIds": body.unit_ids })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct NewPassword {
    password: String,
}

/// Setzt ein neues Passwort und beendet alle Sitzungen des Benutzers.
async fn reset_password(
    State(st): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<NewPassword>,
) -> ApiResult<serde_json::Value> {
    let min = st.ctx.platform.policies().password_min_length as usize;
    if body.password.chars().count() < min {
        return Err(ApiFailure::bad_request(format!(
            "Passwort muss mindestens {min} Zeichen lang sein"
        )));
    }
    let hash = st.ctx.security.hash_password(&body.password)?;
    let mut tx = st.pool().begin().await?;
    let updated = sqlx::query(
        "UPDATE users SET password_hash = $2, login_attempts = 0, locked_until = NULL,
                          updated_at = now()
         WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(&hash)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Benutzer nicht gefunden"));
    }
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &user.auth, "user.password_reset", "user", id, None).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}
