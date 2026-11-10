//! Personal, Abwesenheiten und Feiertage.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, camel, json_sql, list, missing_or_stale, optional_text, required_text, row_id,
    ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::ModuleContext;

use crate::Range;

const STAFF_COLUMNS: &str = "s.id, s.user_id, s.display_name, s.personnel_number, s.qualification,
    s.unit_id, s.work_percent, s.active, s.version";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StaffFilter {
    unit_id: Option<Uuid>,
    all: Option<bool>,
}

pub(crate) async fn list_staff(
    State(ctx): State<ModuleContext>,
    Query(f): Query<StaffFilter>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {STAFF_COLUMNS}, u.name AS unit_name
         FROM staff_members s LEFT JOIN org_units u ON u.id = s.unit_id
         WHERE ($1::uuid IS NULL OR s.unit_id = $1) AND ($2 OR s.active)
         ORDER BY s.display_name"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(f.unit_id)
        .bind(f.all.unwrap_or(false))
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StaffBody {
    /// Benutzerkonto der Person (für den eigenen Dienstkalender)
    user_id: Option<Uuid>,
    display_name: String,
    /// Kennung in der Zeitwirtschaft; Pflicht für den Stundenexport
    personnel_number: Option<String>,
    /// Hauseigene Stufe, z. B. AA, FA, OA, PFK
    qualification: String,
    unit_id: Option<Uuid>,
    work_percent: Option<i32>,
    active: Option<bool>,
    /// nur beim Ändern
    version: Option<i32>,
}

fn personnel_number(value: Option<String>) -> Option<String> {
    optional_text(value).map(|v| v.to_uppercase())
}

pub(crate) async fn create_staff(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<StaffBody>,
) -> ApiResult<Value> {
    let name = required_text("displayName", &body.display_name)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO staff_members AS s (user_id, display_name, personnel_number, qualification,
                                        unit_id, work_percent)
         VALUES ($1, $2, $3, $4, $5, COALESCE($6, 100)) RETURNING {STAFF_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(body.user_id)
        .bind(&name)
        .bind(personnel_number(body.personnel_number))
        .bind(body.qualification.trim().to_uppercase())
        .bind(body.unit_id)
        .bind(body.work_percent)
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "roster.staff_create",
        "staff",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

pub(crate) async fn update_staff(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<StaffBody>,
) -> ApiResult<Value> {
    let name = required_text("displayName", &body.display_name)?;
    let version = body
        .version
        .ok_or_else(|| ApiFailure::bad_request("version fehlt"))?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "UPDATE staff_members s
         SET user_id = $2, display_name = $3, personnel_number = $4, qualification = $5,
             unit_id = $6, work_percent = COALESCE($7, work_percent),
             active = COALESCE($8, active), updated_at = now(), version = version + 1
         WHERE id = $1 AND version = $9 RETURNING {STAFF_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(body.user_id)
        .bind(&name)
        .bind(personnel_number(body.personnel_number))
        .bind(body.qualification.trim().to_uppercase())
        .bind(body.unit_id)
        .bind(body.work_percent)
        .bind(body.active)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(missing_or_stale(&mut tx, "staff_members", id, "Person nicht gefunden").await?);
    };
    audit(
        &mut tx,
        &user.auth,
        "roster.staff_update",
        "staff",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

// --- Abwesenheiten -----------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AbsenceFilter {
    from: NaiveDate,
    to: NaiveDate,
    staff_id: Option<Uuid>,
}

pub(crate) async fn list_absences(
    State(ctx): State<ModuleContext>,
    Query(f): Query<AbsenceFilter>,
) -> ApiResult<Value> {
    Range {
        from: f.from,
        to: f.to,
    }
    .check()?;
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT a.id, a.staff_id, s.display_name, a.kind, a.from_date, a.to_date, a.note
         FROM absences a JOIN staff_members s ON s.id = a.staff_id
         WHERE a.from_date <= $2 AND a.to_date >= $1 AND ($3::uuid IS NULL OR a.staff_id = $3)
         ORDER BY a.from_date, s.display_name",
    ))
    .bind(f.from)
    .bind(f.to)
    .bind(f.staff_id)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewAbsence {
    staff_id: Uuid,
    /// `urlaub`, `zeitausgleich`, `krank`, `fortbildung`, `wunschfrei`, `sperre`
    kind: String,
    from_date: NaiveDate,
    to_date: NaiveDate,
    note: Option<String>,
}

pub(crate) async fn create_absence(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<NewAbsence>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO absences (staff_id, kind, from_date, to_date, note, recorded_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, staff_id, kind, from_date, to_date, note",
    ))
    .bind(body.staff_id)
    .bind(body.kind.trim())
    .bind(body.from_date)
    .bind(body.to_date)
    .bind(optional_text(body.note))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    // Die Art der Abwesenheit steht bewusst nicht im Audit (kann auf
    // Gesundheit schließen lassen).
    audit(
        &mut tx,
        &user.auth,
        "roster.absence_create",
        "absence",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

pub(crate) async fn delete_absence(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Value> {
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query("DELETE FROM absences WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Abwesenheit nicht gefunden"));
    }
    audit(
        &mut tx,
        &user.auth,
        "roster.absence_delete",
        "absence",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

// --- Feiertage ---------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct YearQuery {
    year: i32,
}

pub(crate) async fn list_holidays(
    State(ctx): State<ModuleContext>,
    Query(q): Query<YearQuery>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT holiday_date, name FROM public_holidays
         WHERE extract(year FROM holiday_date) = $1 ORDER BY holiday_date",
    ))
    .bind(q.year)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
pub(crate) struct HolidayBody {
    name: String,
}

/// Legt einen Feiertag an oder benennt ihn um. Feiertage sind Daten des
/// Hauses, weil sie je Bundesland verschieden sind.
pub(crate) async fn put_holiday(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(date): Path<NaiveDate>,
    Json(body): Json<HolidayBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    sqlx::query(
        "INSERT INTO public_holidays (holiday_date, name) VALUES ($1, $2)
         ON CONFLICT (holiday_date) DO UPDATE SET name = EXCLUDED.name",
    )
    .bind(date)
    .bind(&name)
    .execute(&mut *tx)
    .await?;
    ctx.security
        .log_audit_tx(
            &mut tx,
            &user.auth.user_id,
            "roster.holiday_put",
            "holiday",
            Some(&date.to_string()),
            None,
            None,
            None,
        )
        .await?;
    tx.commit().await?;
    Ok(Json(json!({ "holidayDate": date, "name": name })))
}

pub(crate) async fn delete_holiday(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(date): Path<NaiveDate>,
) -> ApiResult<Value> {
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query("DELETE FROM public_holidays WHERE holiday_date = $1")
        .bind(date)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Feiertag nicht gefunden"));
    }
    ctx.security
        .log_audit_tx(
            &mut tx,
            &user.auth.user_id,
            "roster.holiday_delete",
            "holiday",
            Some(&date.to_string()),
            None,
            None,
            None,
        )
        .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}
