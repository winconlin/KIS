//! Fachmodul **Klinische Akte**: Diagnosen, Prozeduren, Dokumentation,
//! Aufgaben und Übergabe.
//!
//! - **Diagnosen** (ICD-10-GM) und **Prozeduren** (OPS) je Fall; je Fall
//!   höchstens eine Hauptdiagnose. Entfernen heißt „als entfernt
//!   markieren“.
//! - **Dokumentation**: Anamnese, Befund, Verlauf, Visite, Pflegebericht,
//!   Konsil, Epikrise, Arztbrief. Ein Eintrag ist Entwurf, bis sein Autor
//!   ihn freigibt; danach ist er unveränderlich (erzwingt die Datenbank).
//!   Korrekturen entstehen als neuer Eintrag, der den alten ersetzt.
//! - **Aufgaben** je Fall, als Liste je Fall und je Einheit.
//! - **Übergabe** je Einheit: Stabilität, Wenn–dann, Hauptdiagnose, offene
//!   Aufgaben, letzter Verlaufseintrag — in einer Abfrage.
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/cases/:case_id/diagnoses`, `…/procedures` | `clinical.read` oder `nursing.read` |
//! | `POST/DELETE …/diagnoses`, `…/procedures` | `clinical.write` oder `coding.write` |
//! | `GET  /api/v1/cases/:case_id/notes` | `clinical.read` oder `nursing.read` |
//! | `POST/PUT/DELETE …/notes`, `…/sign`, `…/amend` | `clinical.write` oder `nursing.write` |
//! | `GET  /api/v1/cases/:case_id/tasks` | `clinical.read` oder `nursing.read` |
//! | `POST …/tasks`, `…/done`, `…/reopen` | `task.write` |
//! | `GET  /api/v1/units/:unit_id/tasks`, `…/handover` | `clinical.read` oder `nursing.read` |

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, camel, json_sql, list, missing_or_stale, optional_text, required_text, row_id,
    violated_constraint, ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct ClinicalModule;

impl Module for ClinicalModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "clinical",
            title: "Klinische Akte",
            description: "Diagnosen, Prozeduren, Dokumentation, Aufgaben, Übergabe",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::AnyOf(&[P::ClinicalRead, P::NursingRead]);
        let code = Access::AnyOf(&[P::ClinicalWrite, P::CodingWrite]);
        let document = Access::AnyOf(&[P::ClinicalWrite, P::NursingWrite]);
        let task = Access::Permission(P::TaskWrite);
        ModuleRoutes::new()
            .get("/api/v1/cases/:case_id/diagnoses", read, list_diagnoses)
            .post("/api/v1/cases/:case_id/diagnoses", code, add_diagnosis)
            .delete(
                "/api/v1/cases/:case_id/diagnoses/:id",
                code,
                remove_diagnosis,
            )
            .get("/api/v1/cases/:case_id/procedures", read, list_procedures)
            .post("/api/v1/cases/:case_id/procedures", code, add_procedure)
            .delete(
                "/api/v1/cases/:case_id/procedures/:id",
                code,
                remove_procedure,
            )
            .get("/api/v1/cases/:case_id/notes", read, list_notes)
            .post("/api/v1/cases/:case_id/notes", document, create_note)
            .put("/api/v1/cases/:case_id/notes/:id", document, update_note)
            .delete("/api/v1/cases/:case_id/notes/:id", document, delete_note)
            .post("/api/v1/cases/:case_id/notes/:id/sign", document, sign_note)
            .post(
                "/api/v1/cases/:case_id/notes/:id/amend",
                document,
                amend_note,
            )
            .get("/api/v1/cases/:case_id/tasks", read, list_tasks)
            .post("/api/v1/cases/:case_id/tasks", task, create_task)
            .post("/api/v1/cases/:case_id/tasks/:id/done", task, complete_task)
            .post("/api/v1/cases/:case_id/tasks/:id/reopen", task, reopen_task)
            .get("/api/v1/units/:unit_id/tasks", read, unit_tasks)
            .get("/api/v1/units/:unit_id/handover", read, unit_handover)
            .with_state(ctx.clone())
    }
}

/// Offene Fälle der Einheit `$1` und ihrer Untereinheiten.
const UNIT_CASES: &str = "WITH RECURSIVE sub AS (
        SELECT id FROM org_units WHERE id = $1
      UNION
        SELECT o.id FROM org_units o JOIN sub s ON o.parent_id = s.id),
    unit_cases AS (
        SELECT c.* FROM cases c
        WHERE c.status = 'aufgenommen'
          AND (c.ward_id IN (SELECT id FROM sub) OR c.department_id IN (SELECT id FROM sub)))";

// --- Diagnosen ---------------------------------------------------------------

async fn list_diagnoses(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT id, kind, icd_code, text, certainty, laterality, recorded_at, version
         FROM case_diagnoses WHERE case_id = $1 AND removed_at IS NULL
         ORDER BY array_position(ARRAY['haupt', 'aufnahme', 'neben', 'entlassung'], kind), recorded_at",
    ))
    .bind(case_id)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewDiagnosis {
    /// `aufnahme`, `haupt`, `neben` oder `entlassung`
    kind: String,
    icd_code: Option<String>,
    text: String,
    /// `A`, `G`, `V` oder `Z`
    certainty: Option<String>,
    /// `R`, `L` oder `B`
    laterality: Option<String>,
}

async fn add_diagnosis(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewDiagnosis>,
) -> ApiResult<Value> {
    let text = required_text("text", &body.text)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO case_diagnoses (case_id, kind, icd_code, text, certainty, laterality, recorded_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, kind, icd_code, text, certainty, laterality, recorded_at, version",
    ))
    .bind(case_id)
    .bind(body.kind.trim())
    .bind(optional_text(body.icd_code).map(|c| c.to_uppercase()))
    .bind(&text)
    .bind(optional_text(body.certainty))
    .bind(optional_text(body.laterality))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match violated_constraint(&e) {
        Some("case_diagnoses_one_main") => ApiFailure::conflict(
            "Der Fall hat bereits eine Hauptdiagnose; bitte zuerst die bisherige entfernen",
        ),
        Some("case_diagnoses_icd_code_check") => {
            ApiFailure::bad_request("icdCode ist kein gültiger ICD-10-GM-Kode (z. B. I21.0)")
        }
        _ => e.into(),
    })?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.diagnosis_add",
        "case",
        case_id,
        Some(json!({ "kind": body.kind.trim(), "diagnosisId": row_id(&row)? })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn remove_diagnosis(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    soft_remove(
        &ctx,
        &user,
        "case_diagnoses",
        "clinical.diagnosis_remove",
        case_id,
        id,
    )
    .await
}

// --- Prozeduren --------------------------------------------------------------

async fn list_procedures(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT id, ops_code, text, performed_at, laterality, recorded_at
         FROM case_procedures WHERE case_id = $1 AND removed_at IS NULL ORDER BY performed_at",
    ))
    .bind(case_id)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewProcedure {
    ops_code: Option<String>,
    text: String,
    performed_at: DateTime<Utc>,
    laterality: Option<String>,
}

async fn add_procedure(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewProcedure>,
) -> ApiResult<Value> {
    let text = required_text("text", &body.text)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO case_procedures (case_id, ops_code, text, performed_at, laterality, recorded_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, ops_code, text, performed_at, laterality, recorded_at",
    ))
    .bind(case_id)
    .bind(optional_text(body.ops_code))
    .bind(&text)
    .bind(body.performed_at)
    .bind(optional_text(body.laterality))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match violated_constraint(&e) {
        Some("case_procedures_ops_code_check") => {
            ApiFailure::bad_request("opsCode ist kein gültiger OPS-Kode (z. B. 1-275.0)")
        }
        _ => e.into(),
    })?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.procedure_add",
        "case",
        case_id,
        Some(json!({ "procedureId": row_id(&row)? })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn remove_procedure(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    soft_remove(
        &ctx,
        &user,
        "case_procedures",
        "clinical.procedure_remove",
        case_id,
        id,
    )
    .await
}

/// Markiert eine Diagnose oder Prozedur als entfernt. `table` ist ein
/// fester Name aus diesem Modul.
async fn soft_remove(
    ctx: &ModuleContext,
    user: &CurrentUser,
    table: &'static str,
    action: &str,
    case_id: Uuid,
    id: Uuid,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query(&format!(
        "UPDATE {table} SET removed_at = now(), removed_by = $3
         WHERE id = $2 AND case_id = $1 AND removed_at IS NULL"
    ))
    .bind(case_id)
    .bind(id)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Eintrag nicht gefunden"));
    }
    audit(
        &mut tx,
        &user.auth,
        action,
        "case",
        case_id,
        Some(json!({ "entryId": id })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

// --- Dokumentation -----------------------------------------------------------

const NOTE_COLUMNS: &str = "n.id, n.kind, n.title, n.body, n.status, n.author_id, n.created_at,
    n.updated_at, n.signed_at, n.supersedes_id, n.version";

#[derive(Deserialize)]
struct NotesQuery {
    kind: Option<String>,
}

/// Freigegebene Einträge des Falls und eigene Entwürfe, neueste zuerst.
/// Entwürfe anderer und ersetzte Fassungen erscheinen nicht.
async fn list_notes(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Query(q): Query<NotesQuery>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {NOTE_COLUMNS}, a.display_name AS author_name
         FROM clinical_notes n JOIN users a ON a.id = n.author_id
         WHERE n.case_id = $1 AND ($2::text IS NULL OR n.kind = $2)
           AND (n.status = 'freigegeben' OR (n.status = 'entwurf' AND n.author_id = $3))
         ORDER BY n.created_at DESC LIMIT 500"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(q.kind)
        .bind(actor_id(&user.auth)?)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

/// Pflegeberichte schreibt die Pflege, alle anderen Arten der ärztliche
/// bzw. therapeutische Dienst.
fn check_note_kind(user: &CurrentUser, kind: &str) -> Result<(), ApiFailure> {
    let needed = if kind == "pflege" {
        P::NursingWrite
    } else {
        P::ClinicalWrite
    };
    if user.can(&needed) {
        Ok(())
    } else {
        Err(ApiFailure::forbidden(format!(
            "Für Einträge der Art „{kind}“ fehlt das Recht {}",
            needed.key()
        )))
    }
}

#[derive(Deserialize)]
struct NewNote {
    kind: String,
    title: Option<String>,
    body: String,
}

async fn create_note(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewNote>,
) -> ApiResult<Value> {
    let kind = body.kind.trim().to_string();
    check_note_kind(&user, &kind)?;
    let text = required_text("body", &body.body)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO clinical_notes AS n (case_id, kind, title, body, author_id)
         VALUES ($1, $2, $3, $4, $5) RETURNING {NOTE_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(&kind)
        .bind(optional_text(body.title))
        .bind(&text)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.note_create",
        "clinical_note",
        row_id(&row)?,
        Some(json!({ "kind": kind })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Sperrt einen Eintrag und prüft, dass er ein Entwurf des Benutzers in
/// der erwarteten Version ist.
async fn lock_own_draft(
    tx: &mut sqlx::PgConnection,
    case_id: Uuid,
    id: Uuid,
    actor: Uuid,
    version: Option<i32>,
) -> Result<Option<Uuid>, ApiFailure> {
    let row: Option<(String, Uuid, i32, Option<Uuid>)> = sqlx::query_as(
        "SELECT status, author_id, version, supersedes_id FROM clinical_notes
         WHERE id = $2 AND case_id = $1 FOR UPDATE",
    )
    .bind(case_id)
    .bind(id)
    .fetch_optional(tx)
    .await?;
    let (status, author, current, supersedes) =
        row.ok_or(ApiFailure::NotFound("Eintrag nicht gefunden"))?;
    if status != "entwurf" {
        return Err(ApiFailure::conflict(
            "Der Eintrag ist freigegeben und unveränderlich; Korrektur über „amend“",
        ));
    }
    if author != actor {
        return Err(ApiFailure::forbidden(
            "Nur der Autor bearbeitet seinen Entwurf",
        ));
    }
    if version.is_some_and(|v| v != current) {
        return Err(ApiFailure::stale_version());
    }
    Ok(supersedes)
}

#[derive(Deserialize)]
struct NoteUpdate {
    title: Option<String>,
    body: String,
    version: i32,
}

async fn update_note(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NoteUpdate>,
) -> ApiResult<Value> {
    let text = required_text("body", &body.body)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    lock_own_draft(&mut tx, case_id, id, actor, Some(body.version)).await?;
    let sql = json_sql(&format!(
        "UPDATE clinical_notes n SET title = $2, body = $3, updated_at = now(), version = version + 1
         WHERE id = $1 RETURNING {NOTE_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(optional_text(body.title))
        .bind(&text)
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.note_update",
        "clinical_note",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn delete_note(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    lock_own_draft(&mut tx, case_id, id, actor, None).await?;
    sqlx::query("DELETE FROM clinical_notes WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.note_delete_draft",
        "clinical_note",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct Sign {
    version: i32,
}

/// Gibt einen Entwurf frei. Ersetzt er einen älteren Eintrag, wird dieser
/// im selben Schritt als ersetzt markiert.
async fn sign_note(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Sign>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let supersedes = lock_own_draft(&mut tx, case_id, id, actor, Some(body.version)).await?;
    let sql = json_sql(&format!(
        "UPDATE clinical_notes n SET status = 'freigegeben', signed_by = $2, signed_at = now(),
                updated_at = now(), version = version + 1
         WHERE id = $1 RETURNING {NOTE_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(old) = supersedes {
        sqlx::query(
            "UPDATE clinical_notes SET status = 'ersetzt', updated_at = now()
             WHERE id = $1 AND status = 'freigegeben'",
        )
        .bind(old)
        .execute(&mut *tx)
        .await?;
    }
    audit(
        &mut tx,
        &user.auth,
        "clinical.note_sign",
        "clinical_note",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Legt zu einem freigegebenen Eintrag eine Korrekturfassung als Entwurf
/// an. Der alte Eintrag bleibt gültig, bis die Korrektur freigegeben ist.
async fn amend_note(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM clinical_notes
         WHERE id = $2 AND case_id = $1 AND status = 'freigegeben' FOR UPDATE",
    )
    .bind(case_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let kind = kind.ok_or(ApiFailure::NotFound(
        "Kein freigegebener Eintrag mit dieser ID",
    ))?;
    check_note_kind(&user, &kind)?;
    let sql = json_sql(&format!(
        "INSERT INTO clinical_notes AS n (case_id, kind, title, body, author_id, supersedes_id)
         SELECT case_id, kind, title, body, $2, id FROM clinical_notes WHERE id = $1
         RETURNING {NOTE_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.note_amend",
        "clinical_note",
        row_id(&row)?,
        Some(json!({ "supersedes": id })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

// --- Aufgaben ----------------------------------------------------------------

const TASK_COLUMNS: &str =
    "t.id, t.case_id, t.text, t.assignee_user_id, t.assignee_label, t.due_date,
    t.done_at, t.created_at, t.version";

#[derive(Deserialize)]
struct TaskQuery {
    /// `true`: auch erledigte
    all: Option<bool>,
}

async fn list_tasks(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<TaskQuery>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {TASK_COLUMNS} FROM case_tasks t
         WHERE t.case_id = $1 AND ($2 OR t.done_at IS NULL)
         ORDER BY t.done_at IS NOT NULL, t.due_date NULLS LAST, t.created_at"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(q.all.unwrap_or(false))
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewTask {
    text: String,
    assignee_user_id: Option<Uuid>,
    /// Freies Kürzel, z. B. „Dienstarzt“ oder „Spätdienst“
    assignee_label: Option<String>,
    due_date: Option<NaiveDate>,
}

async fn create_task(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewTask>,
) -> ApiResult<Value> {
    let text = required_text("text", &body.text)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO case_tasks AS t (case_id, text, assignee_user_id, assignee_label, due_date, created_by)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {TASK_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(&text)
        .bind(body.assignee_user_id)
        .bind(optional_text(body.assignee_label))
        .bind(body.due_date)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "clinical.task_create",
        "case_task",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
struct TaskVersion {
    version: i32,
}

async fn complete_task(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<TaskVersion>,
) -> ApiResult<Value> {
    set_task_done(&ctx, &user, case_id, id, body.version, true).await
}

async fn reopen_task(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<TaskVersion>,
) -> ApiResult<Value> {
    set_task_done(&ctx, &user, case_id, id, body.version, false).await
}

async fn set_task_done(
    ctx: &ModuleContext,
    user: &CurrentUser,
    case_id: Uuid,
    id: Uuid,
    version: i32,
    done: bool,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "UPDATE case_tasks t
         SET done_at = CASE WHEN $4 THEN now() END, done_by = CASE WHEN $4 THEN $5::uuid END,
             updated_at = now(), version = version + 1
         WHERE id = $2 AND case_id = $1 AND version = $3 AND (done_at IS NULL) = $4
         RETURNING {TASK_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(id)
        .bind(version)
        .bind(done)
        .bind(actor)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(missing_or_stale(&mut tx, "case_tasks", id, "Aufgabe nicht gefunden").await?);
    };
    let action = if done {
        "clinical.task_done"
    } else {
        "clinical.task_reopen"
    };
    audit(&mut tx, &user.auth, action, "case_task", id, None).await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Offene Aufgaben aller aktuellen Fälle der Einheit.
async fn unit_tasks(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "{UNIT_CASES}
         SELECT {TASK_COLUMNS}, c.case_number, p.name_family, p.name_given
         FROM case_tasks t
         JOIN unit_cases c ON c.id = t.case_id
         JOIN patients p ON p.id = c.patient_id
         WHERE t.done_at IS NULL
         ORDER BY t.due_date NULLS LAST, t.created_at"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

// --- Übergabe ----------------------------------------------------------------

/// Übergabeliste der Einheit: je aktuellem Fall Kurzstatus (Kern),
/// Hauptdiagnose, offene Aufgaben und der letzte freigegebene Verlaufs-
/// oder Visiteneintrag (gekürzt).
async fn unit_handover(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "{UNIT_CASES}
         SELECT c.id AS case_id, c.case_number, p.name_family, p.name_given, p.birth_date,
                w.name AS ward_name, r.name AS room_name, b.label AS bed_label,
                c.stability, c.contingency, c.fasting, c.dnr, c.dni, c.planned_discharge,
                (SELECT d.text FROM case_diagnoses d
                  WHERE d.case_id = c.id AND d.removed_at IS NULL
                  ORDER BY array_position(ARRAY['haupt', 'aufnahme', 'neben', 'entlassung'], d.kind),
                           d.recorded_at LIMIT 1) AS leading_diagnosis,
                (SELECT COALESCE(jsonb_agg(t.text ORDER BY t.due_date NULLS LAST, t.created_at), '[]')
                   FROM case_tasks t WHERE t.case_id = c.id AND t.done_at IS NULL) AS open_tasks,
                (SELECT left(n.body, 600) FROM clinical_notes n
                  WHERE n.case_id = c.id AND n.status = 'freigegeben' AND n.kind IN ('verlauf', 'visite')
                  ORDER BY n.signed_at DESC LIMIT 1) AS last_note
         FROM unit_cases c
         JOIN patients p ON p.id = c.patient_id
         LEFT JOIN org_units w ON w.id = c.ward_id
         LEFT JOIN beds b ON b.id = c.bed_id
         LEFT JOIN rooms r ON r.id = b.room_id
         ORDER BY array_position(ARRAY['instabil', 'beobachtung', 'stabil'], c.stability) NULLS LAST,
                  w.name NULLS LAST, r.name NULLS LAST, b.label NULLS LAST"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}
