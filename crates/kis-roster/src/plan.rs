//! Dienstarten, Besetzung, Prüfung und Wochenbesetzung einer Einheit.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, camel, camel_rows, json_sql, list, missing_or_stale, optional_text,
    required_text, row_id, ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::ModuleContext;

use crate::{Range, UNIT_SUBTREE};

// --- Dienstarten -------------------------------------------------------------

const DUTY_TYPE_COLUMNS: &str = "t.id, t.unit_id, t.code, t.name, t.qualifications, t.weekdays,
    t.on_holidays, t.required, t.blocks_next_day, t.start_time, t.duration_hours,
    t.weekend_start_time, t.weekend_duration_hours, t.credited_hours, t.weekend_credited_hours,
    t.wage_type, t.active, t.version";

pub(crate) async fn list_duty_types(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT {DUTY_TYPE_COLUMNS} FROM duty_types t
         WHERE t.unit_id IN (SELECT id FROM sub) ORDER BY t.active DESC, t.code"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DutyTypeBody {
    code: String,
    name: String,
    /// Zulässige Qualifikationen (leer = jede)
    #[serde(default)]
    qualifications: Vec<String>,
    /// ISO-Wochentage 1 (Montag) … 7 (Sonntag); Standard: alle
    weekdays: Option<Vec<i16>>,
    /// Fällt der Dienst an Feiertagen an? (Standard: ja)
    on_holidays: Option<bool>,
    required: Option<bool>,
    /// Ruhezeit: am Folgetag kein Dienst
    #[serde(default)]
    blocks_next_day: bool,
    start_time: NaiveTime,
    duration_hours: f64,
    weekend_start_time: Option<NaiveTime>,
    weekend_duration_hours: Option<f64>,
    credited_hours: f64,
    weekend_credited_hours: Option<f64>,
    wage_type: Option<String>,
    active: Option<bool>,
    /// nur beim Ändern
    version: Option<i32>,
}

impl DutyTypeBody {
    fn qualifications(&self) -> Vec<String> {
        self.qualifications
            .iter()
            .map(|q| q.trim().to_uppercase())
            .filter(|q| !q.is_empty())
            .collect()
    }
}

pub(crate) async fn create_duty_type(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<DutyTypeBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO duty_types AS t (unit_id, code, name, qualifications, weekdays, on_holidays,
                                     required, blocks_next_day, start_time, duration_hours,
                                     weekend_start_time, weekend_duration_hours, credited_hours,
                                     weekend_credited_hours, wage_type)
         VALUES ($1, $2, $3, $4, COALESCE($5, '{{1,2,3,4,5,6,7}}'::smallint[]), COALESCE($6, TRUE),
                 COALESCE($7, TRUE), $8, $9, $10::numeric, $11, $12::numeric, $13::numeric,
                 $14::numeric, COALESCE($15, 'BD'))
         RETURNING {DUTY_TYPE_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .bind(body.code.trim().to_uppercase())
        .bind(&name)
        .bind(body.qualifications())
        .bind(&body.weekdays)
        .bind(body.on_holidays)
        .bind(body.required)
        .bind(body.blocks_next_day)
        .bind(body.start_time)
        .bind(body.duration_hours)
        .bind(body.weekend_start_time)
        .bind(body.weekend_duration_hours)
        .bind(body.credited_hours)
        .bind(body.weekend_credited_hours)
        .bind(optional_text(body.wage_type).map(|w| w.to_uppercase()))
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "roster.duty_type_create",
        "duty_type",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

pub(crate) async fn update_duty_type(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((unit_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<DutyTypeBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let version = body
        .version
        .ok_or_else(|| ApiFailure::bad_request("version fehlt"))?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "{UNIT_SUBTREE}
         UPDATE duty_types t
         SET name = $3, qualifications = $4, weekdays = COALESCE($5, weekdays),
             on_holidays = COALESCE($6, on_holidays), required = COALESCE($7, required),
             blocks_next_day = $8, start_time = $9, duration_hours = $10::numeric,
             weekend_start_time = $11, weekend_duration_hours = $12::numeric,
             credited_hours = $13::numeric, weekend_credited_hours = $14::numeric,
             wage_type = COALESCE($15, wage_type), active = COALESCE($16, active),
             updated_at = now(), version = version + 1
         WHERE t.id = $2 AND t.version = $17 AND t.unit_id IN (SELECT id FROM sub)
         RETURNING {DUTY_TYPE_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .bind(id)
        .bind(&name)
        .bind(body.qualifications())
        .bind(&body.weekdays)
        .bind(body.on_holidays)
        .bind(body.required)
        .bind(body.blocks_next_day)
        .bind(body.start_time)
        .bind(body.duration_hours)
        .bind(body.weekend_start_time)
        .bind(body.weekend_duration_hours)
        .bind(body.credited_hours)
        .bind(body.weekend_credited_hours)
        .bind(optional_text(body.wage_type).map(|w| w.to_uppercase()))
        .bind(body.active)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(missing_or_stale(&mut tx, "duty_types", id, "Dienstart nicht gefunden").await?);
    };
    audit(
        &mut tx,
        &user.auth,
        "roster.duty_type_update",
        "duty_type",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

// --- Plan --------------------------------------------------------------------

/// Plan der Einheit im Zeitraum: Dienstarten, Besetzung, Abwesenheiten
/// der eingeplanten und der Einheit zugeordneten Personen, Feiertage.
pub(crate) async fn get_plan(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
    Query(range): Query<Range>,
) -> ApiResult<Value> {
    range.check()?;
    let pool = ctx.pool();
    let duty_types: Vec<Value> = sqlx::query_scalar(&json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT {DUTY_TYPE_COLUMNS} FROM duty_types t
         WHERE t.active AND t.unit_id IN (SELECT id FROM sub) ORDER BY t.code"
    )))
    .bind(unit_id)
    .fetch_all(pool)
    .await?;
    let assignments: Vec<Value> = sqlx::query_scalar(&json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT a.id, a.duty_type_id, t.code AS duty_code, a.duty_date, a.staff_id,
                s.display_name, s.qualification, a.note
         FROM duty_assignments a
         JOIN duty_types t ON t.id = a.duty_type_id
         JOIN staff_members s ON s.id = a.staff_id
         WHERE t.unit_id IN (SELECT id FROM sub) AND a.duty_date BETWEEN $2 AND $3
         ORDER BY a.duty_date, t.code"
    )))
    .bind(unit_id)
    .bind(range.from)
    .bind(range.to)
    .fetch_all(pool)
    .await?;
    let absences: Vec<Value> = sqlx::query_scalar(&json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT ab.id, ab.staff_id, s.display_name, ab.kind, ab.from_date, ab.to_date
         FROM absences ab JOIN staff_members s ON s.id = ab.staff_id
         WHERE ab.from_date <= $3 AND ab.to_date >= $2
           AND (s.unit_id IN (SELECT id FROM sub)
                OR s.id IN (SELECT a.staff_id FROM duty_assignments a
                            JOIN duty_types t ON t.id = a.duty_type_id
                            WHERE t.unit_id IN (SELECT id FROM sub)
                              AND a.duty_date BETWEEN $2 AND $3))
         ORDER BY ab.from_date, s.display_name"
    )))
    .bind(unit_id)
    .bind(range.from)
    .bind(range.to)
    .fetch_all(pool)
    .await?;
    let holidays: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT holiday_date, name FROM public_holidays
         WHERE holiday_date BETWEEN $1 AND $2 ORDER BY holiday_date",
    ))
    .bind(range.from)
    .bind(range.to)
    .fetch_all(pool)
    .await?;
    Ok(Json(json!({
        "from": range.from,
        "to": range.to,
        "dutyTypes": camel_rows(duty_types),
        "assignments": camel_rows(assignments),
        "absences": camel_rows(absences),
        "holidays": camel_rows(holidays),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssignmentBody {
    duty_type_id: Uuid,
    duty_date: NaiveDate,
    staff_id: Uuid,
    note: Option<String>,
}

/// Besetzt einen Dienst an einem Tag (ersetzt eine vorhandene Besetzung).
/// Konflikte verhindert das nicht; sie erscheinen in der Prüfung.
pub(crate) async fn put_assignment(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<AssignmentBody>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "{UNIT_SUBTREE}
         INSERT INTO duty_assignments (duty_type_id, duty_date, staff_id, note, assigned_by)
         SELECT t.id, $3, $4, $5, $6 FROM duty_types t
         WHERE t.id = $2 AND t.active AND t.unit_id IN (SELECT id FROM sub)
         ON CONFLICT (duty_type_id, duty_date) DO UPDATE
         SET staff_id = EXCLUDED.staff_id, note = EXCLUDED.note,
             assigned_by = EXCLUDED.assigned_by, assigned_at = now()
         RETURNING id, duty_type_id, duty_date, staff_id, note"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .bind(body.duty_type_id)
        .bind(body.duty_date)
        .bind(body.staff_id)
        .bind(optional_text(body.note))
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await?;
    let row = row.ok_or(ApiFailure::NotFound(
        "Dienstart nicht gefunden, nicht aktiv oder nicht in dieser Einheit",
    ))?;
    audit(
        &mut tx,
        &user.auth,
        "roster.assign",
        "duty_assignment",
        row_id(&row)?,
        Some(json!({ "dutyDate": body.duty_date })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

pub(crate) async fn delete_assignment(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((unit_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query(&format!(
        "{UNIT_SUBTREE}
         DELETE FROM duty_assignments a USING duty_types t
         WHERE a.id = $2 AND t.id = a.duty_type_id AND t.unit_id IN (SELECT id FROM sub)"
    ))
    .bind(unit_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Besetzung nicht gefunden"));
    }
    audit(
        &mut tx,
        &user.auth,
        "roster.unassign",
        "duty_assignment",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

// --- Prüfung -----------------------------------------------------------------

/// Prüft den Plan der Einheit im Zeitraum. Jede Meldung nennt Art, Tag,
/// Dienst und (falls betroffen) Person.
///
/// | `kind` | Bedeutung |
/// |---|---|
/// | `unbesetzt` | Pflichtdienst an einem fälligen Tag ohne Besetzung |
/// | `doppelt` | Person hat am selben Tag mehrere Dienste (hausweit) |
/// | `ruhezeit` | Dienst am Tag nach einem Dienst mit Ruhezeit |
/// | `abwesend` | Dienst während einer eingetragenen Abwesenheit |
/// | `qualifikation` | Qualifikation passt nicht zur Dienstart |
pub(crate) async fn validate(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
    Query(range): Query<Range>,
) -> ApiResult<Value> {
    range.check()?;
    let sql = json_sql(&format!(
        "{UNIT_SUBTREE},
         types AS (SELECT * FROM duty_types t WHERE t.active AND t.unit_id IN (SELECT id FROM sub)),
         mine AS (SELECT a.*, t.code, t.qualifications
                  FROM duty_assignments a JOIN types t ON t.id = a.duty_type_id
                  WHERE a.duty_date BETWEEN $2 AND $3),
         -- alle Dienste der hier eingeplanten Personen, auch in anderen Einheiten
         everywhere AS (SELECT a.staff_id, a.duty_date, t.code, t.blocks_next_day
                        FROM duty_assignments a JOIN duty_types t ON t.id = a.duty_type_id
                        WHERE a.duty_date BETWEEN $2 - 1 AND $3
                          AND a.staff_id IN (SELECT staff_id FROM mine)),
         due AS (SELECT t.id, t.code, d::date AS day
                 FROM types t
                 CROSS JOIN generate_series($2::date, $3::date, interval '1 day') d
                 LEFT JOIN public_holidays h ON h.holiday_date = d::date
                 WHERE t.required
                   AND CASE WHEN h.holiday_date IS NOT NULL THEN t.on_holidays
                            ELSE extract(isodow FROM d)::smallint = ANY(t.weekdays) END),
         issues AS (
             SELECT 'unbesetzt' AS kind, due.day AS duty_date, due.code AS duty_code,
                    NULL::uuid AS staff_id, NULL::text AS detail
             FROM due LEFT JOIN duty_assignments a
                    ON a.duty_type_id = due.id AND a.duty_date = due.day
             WHERE a.id IS NULL
           UNION ALL
             SELECT 'doppelt', e.duty_date, string_agg(e.code, ' + ' ORDER BY e.code), e.staff_id, NULL
             FROM everywhere e WHERE e.duty_date >= $2
             GROUP BY e.staff_id, e.duty_date HAVING count(*) > 1
           UNION ALL
             SELECT 'ruhezeit', m.duty_date, m.code, m.staff_id, 'Vortag: ' || e.code
             FROM mine m JOIN everywhere e
                    ON e.staff_id = m.staff_id AND e.duty_date = m.duty_date - 1 AND e.blocks_next_day
           UNION ALL
             SELECT 'abwesend', m.duty_date, m.code, m.staff_id, ab.kind
             FROM mine m JOIN absences ab
                    ON ab.staff_id = m.staff_id AND m.duty_date BETWEEN ab.from_date AND ab.to_date
           UNION ALL
             SELECT 'qualifikation', m.duty_date, m.code, m.staff_id, s.qualification
             FROM mine m JOIN staff_members s ON s.id = m.staff_id
             WHERE cardinality(m.qualifications) > 0 AND NOT s.qualification = ANY(m.qualifications))
         SELECT i.kind, i.duty_date, i.duty_code, i.staff_id, s.display_name AS staff_name, i.detail
         FROM issues i LEFT JOIN staff_members s ON s.id = i.staff_id
         ORDER BY i.duty_date, i.kind, i.duty_code"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .bind(range.from)
        .bind(range.to)
        .fetch_all(ctx.pool())
        .await?;
    let issues = camel_rows(rows);
    Ok(Json(
        json!({ "ok": issues.is_empty(), "count": issues.len(), "issues": issues }),
    ))
}

// --- Wochenbesetzung ---------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeekQuery {
    /// Montag der Woche
    week_start: NaiveDate,
}

/// Besetzungsmatrix einer Woche: jeder Platz mit der eingeteilten Person,
/// dazu die noch nicht eingeteilten und die abwesenden Personen der
/// Einheit.
pub(crate) async fn get_staffing(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
    Query(q): Query<WeekQuery>,
) -> ApiResult<Value> {
    let pool = ctx.pool();
    let positions: Vec<Value> = sqlx::query_scalar(&json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT p.id AS position_id, p.code, p.name, p.category, p.sort_order,
                sa.staff_id, s.display_name, s.qualification
         FROM staffing_positions p
         LEFT JOIN staffing_assignments sa ON sa.position_id = p.id AND sa.week_start = $2
         LEFT JOIN staff_members s ON s.id = sa.staff_id
         WHERE p.active AND p.unit_id IN (SELECT id FROM sub)
         ORDER BY p.category, p.sort_order, p.name"
    )))
    .bind(unit_id)
    .bind(q.week_start)
    .fetch_all(pool)
    .await?;
    let unassigned: Vec<Value> = sqlx::query_scalar(&json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT s.id AS staff_id, s.display_name, s.qualification,
                EXISTS (SELECT 1 FROM absences ab WHERE ab.staff_id = s.id
                         AND ab.from_date <= $2::date + 6 AND ab.to_date >= $2::date) AS absent_this_week
         FROM staff_members s
         WHERE s.active AND s.unit_id IN (SELECT id FROM sub)
           AND NOT EXISTS (SELECT 1 FROM staffing_assignments sa
                            WHERE sa.staff_id = s.id AND sa.week_start = $2)
         ORDER BY s.display_name"
    )))
    .bind(unit_id)
    .bind(q.week_start)
    .fetch_all(pool)
    .await?;
    Ok(Json(json!({
        "weekStart": q.week_start,
        "positions": camel_rows(positions),
        "unassigned": camel_rows(unassigned),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PositionBody {
    code: String,
    name: String,
    /// Gruppe in der Matrix, z. B. „Station 7“, „HKL“, „Echo“
    category: String,
    sort_order: Option<i32>,
}

pub(crate) async fn create_position(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<PositionBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let category = required_text("category", &body.category)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO staffing_positions (unit_id, code, name, category, sort_order)
         VALUES ($1, $2, $3, $4, COALESCE($5, 0))
         RETURNING id, unit_id, code, name, category, sort_order, active",
    ))
    .bind(unit_id)
    .bind(body.code.trim())
    .bind(&name)
    .bind(&category)
    .bind(body.sort_order)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "roster.position_create",
        "staffing_position",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StaffingBody {
    position_id: Uuid,
    /// Montag der Woche
    week_start: NaiveDate,
    /// fehlt = Platz in dieser Woche frei geben
    staff_id: Option<Uuid>,
}

/// Teilt eine Person für eine Woche auf einen Platz ein oder gibt den
/// Platz frei.
pub(crate) async fn put_staffing(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<StaffingBody>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let in_unit: bool = sqlx::query_scalar(&format!(
        "{UNIT_SUBTREE}
         SELECT EXISTS (SELECT 1 FROM staffing_positions p
                        WHERE p.id = $2 AND p.active AND p.unit_id IN (SELECT id FROM sub))"
    ))
    .bind(unit_id)
    .bind(body.position_id)
    .fetch_one(&mut *tx)
    .await?;
    if !in_unit {
        return Err(ApiFailure::NotFound(
            "Platz nicht gefunden oder nicht in dieser Einheit",
        ));
    }
    match body.staff_id {
        Some(staff_id) => {
            sqlx::query(
                "INSERT INTO staffing_assignments (position_id, week_start, staff_id, assigned_by)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (position_id, week_start) DO UPDATE
                 SET staff_id = EXCLUDED.staff_id, assigned_by = EXCLUDED.assigned_by,
                     assigned_at = now()",
            )
            .bind(body.position_id)
            .bind(body.week_start)
            .bind(staff_id)
            .bind(actor)
            .execute(&mut *tx)
            .await?;
        }
        None => {
            sqlx::query(
                "DELETE FROM staffing_assignments WHERE position_id = $1 AND week_start = $2",
            )
            .bind(body.position_id)
            .bind(body.week_start)
            .execute(&mut *tx)
            .await?;
        }
    }
    audit(
        &mut tx,
        &user.auth,
        "roster.staffing_put",
        "staffing_position",
        body.position_id,
        Some(json!({ "weekStart": body.week_start, "assigned": body.staff_id.is_some() })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}
