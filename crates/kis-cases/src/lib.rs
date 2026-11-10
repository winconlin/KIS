//! Kernmodul **Fälle**: Aufnahme, Verlegung, Entlassung, Belegung.
//!
//! Der Fall ist die Klammer um alles, was im Haus an einem Patienten
//! geschieht. Dieses Modul führt ihn von der Aufnahme bis zur Entlassung
//! und meldet jeden Schritt als Ereignis (`case.admitted`,
//! `case.transferred`, `case.discharged`, `case.cancelled`). Andere Module
//! reagieren darauf (Bettenaufbereitung, Küche, Schnittstellen), ohne dass
//! dieses Modul sie kennt.
//!
//! Regeln, die die Datenbank erzwingt (Migration 006): ein Fall ist zu
//! jedem Zeitpunkt an genau einem Ort, ein Bett trägt höchstens einen
//! Fall, ein Patient hat höchstens einen offenen stationären Fall.
//!
//! | Route | Recht |
//! |---|---|
//! | `POST /api/v1/cases` | `case.admit` |
//! | `GET  /api/v1/cases?caseNumber=` | `case.read` |
//! | `GET  /api/v1/patients/:id/cases` | `case.read` |
//! | `GET  /api/v1/cases/:case_id`, `…/stays` | `case.read` + Behandlungszusammenhang |
//! | `POST /api/v1/cases/:case_id/transfer` | `case.transfer` |
//! | `POST /api/v1/cases/:case_id/discharge`, `…/cancel` | `case.admit` |
//! | `PUT  /api/v1/cases/:case_id/clinical-status` | `clinical.write` oder `nursing.write` |
//! | `GET  /api/v1/units/:unit_id/census`, `…/beds` | `case.read` + Zuordnung zur Einheit |

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use kis_core::events::topics;
use kis_core::facility;
use kis_core::http::{
    actor_id, audit, camel, camel_rows, json_sql, list, optional_text, violated_constraint,
    ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct CasesModule;

impl Module for CasesModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "cases",
            title: "Fälle und Belegung",
            description: "Aufnahme, Verlegung, Entlassung, Aufenthalte, Stationsbelegung, Betten",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["org", "patients"],
            maturity: Maturity::Stable,
            kernel: true,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::Permission(P::CaseRead);
        let admit = Access::Permission(P::CaseAdmit);
        ModuleRoutes::new()
            .post("/api/v1/cases", admit, admit_case)
            .get("/api/v1/cases", read, find_cases)
            .get("/api/v1/patients/:id/cases", read, patient_cases)
            .get("/api/v1/cases/:case_id", read, get_case)
            .get("/api/v1/cases/:case_id/stays", read, list_stays)
            .post(
                "/api/v1/cases/:case_id/transfer",
                Access::Permission(P::CaseTransfer),
                transfer_case,
            )
            .post("/api/v1/cases/:case_id/discharge", admit, discharge_case)
            .post("/api/v1/cases/:case_id/cancel", admit, cancel_case)
            .put(
                "/api/v1/cases/:case_id/clinical-status",
                Access::AnyOf(&[P::ClinicalWrite, P::NursingWrite]),
                put_clinical_status,
            )
            .get("/api/v1/units/:unit_id/census", read, unit_census)
            .get("/api/v1/units/:unit_id/beds", read, unit_beds)
            .with_state(ctx.clone())
    }
}

/// Fall mit Patient, Ort und Behandler — eine Zeile.
const CASE_SELECT: &str = "
    SELECT c.id, c.case_number, c.kind, c.status, c.admission_at, c.admission_type,
           c.admission_reason, c.discharge_at, c.discharge_type,
           c.department_id, d.name AS department_name,
           c.ward_id, w.name AS ward_name, c.bed_id, b.label AS bed_label, r.name AS room_name,
           c.attending_user_id, u.display_name AS attending_name,
           c.stability, c.fasting, c.dnr, c.dni, c.contingency, c.planned_discharge, c.version,
           c.patient_id, p.name_family, p.name_given, p.birth_date, p.gender
    FROM cases c
    JOIN patients p ON p.id = c.patient_id
    JOIN org_units d ON d.id = c.department_id
    LEFT JOIN org_units w ON w.id = c.ward_id
    LEFT JOIN beds b ON b.id = c.bed_id
    LEFT JOIN rooms r ON r.id = b.room_id
    LEFT JOIN users u ON u.id = c.attending_user_id";

/// Einheit `$1` mit allen untergeordneten Einheiten.
const UNIT_SUBTREE: &str = "WITH RECURSIVE sub AS (
        SELECT id FROM org_units WHERE id = $1
      UNION
        SELECT o.id FROM org_units o JOIN sub s ON o.parent_id = s.id)";

async fn load_case(conn: &mut PgConnection, case_id: Uuid) -> Result<Value, ApiFailure> {
    let sql = json_sql(&format!("{CASE_SELECT} WHERE c.id = $1"));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .fetch_optional(conn)
        .await?;
    row.map(camel)
        .ok_or(ApiFailure::NotFound("Fall nicht gefunden"))
}

/// Übersetzt die Integritätsregeln der Datenbank in verständliche Meldungen.
fn place_error(e: sqlx::Error) -> ApiFailure {
    match violated_constraint(&e) {
        Some("case_stays_bed_free") => ApiFailure::conflict("Das Bett ist bereits belegt"),
        Some("cases_one_open_inpatient") => {
            ApiFailure::conflict("Der Patient hat bereits einen offenen stationären Fall")
        }
        Some("case_stays_order") | Some("case_stays_one_place") => ApiFailure::bad_request(
            "Der Zeitpunkt muss nach dem Beginn des laufenden Aufenthaltsabschnitts liegen",
        ),
        Some("cases_department_id_department_kind_fkey")
        | Some("case_stays_department_id_department_kind_fkey") => {
            ApiFailure::bad_request("departmentId muss eine Fachabteilung sein")
        }
        Some("cases_ward_id_ward_kind_fkey") | Some("case_stays_ward_id_ward_kind_fkey") => {
            ApiFailure::bad_request("wardId muss eine Station sein")
        }
        Some("cases_discharge_after_admission") => {
            ApiFailure::bad_request("Die Entlassung kann nicht vor der Aufnahme liegen")
        }
        _ => e.into(),
    }
}

/// Prüft, dass das Bett zur Station gehört, aktiv und nicht gesperrt ist.
async fn check_bed(
    tx: &mut PgConnection,
    ward_id: Option<Uuid>,
    bed_id: Option<Uuid>,
) -> Result<(), ApiFailure> {
    let Some(bed_id) = bed_id else { return Ok(()) };
    let Some(ward_id) = ward_id else {
        return Err(ApiFailure::bad_request(
            "Ein Bett setzt eine Station voraus",
        ));
    };
    match facility::lock_bed_for_occupancy(tx, bed_id).await? {
        Ok(bed_ward) if bed_ward == ward_id => Ok(()),
        Ok(_) => Err(ApiFailure::bad_request(
            "Das Bett gehört nicht zur angegebenen Station",
        )),
        Err(reason) => Err(ApiFailure::conflict(reason)),
    }
}

// --- Aufnahme ----------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Admission {
    patient_id: Uuid,
    kind: String,
    department_id: Uuid,
    ward_id: Option<Uuid>,
    bed_id: Option<Uuid>,
    /// Standard: jetzt
    admission_at: Option<DateTime<Utc>>,
    admission_type: Option<String>,
    admission_reason: Option<String>,
    attending_user_id: Option<Uuid>,
}

async fn admit_case(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<Admission>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let patient_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM patients WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(body.patient_id)
    .fetch_one(&mut *tx)
    .await?;
    if !patient_ok {
        return Err(ApiFailure::bad_request("Patient nicht gefunden"));
    }
    check_bed(&mut tx, body.ward_id, body.bed_id).await?;

    let (case_id, admission_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO cases (patient_id, kind, admission_at, admission_type, admission_reason,
                            department_id, ward_id, bed_id, attending_user_id, created_by)
         VALUES ($1, $2, COALESCE($3, now()), COALESCE($4, 'einweisung'), $5, $6, $7, $8, $9, $10)
         RETURNING id, admission_at",
    )
    .bind(body.patient_id)
    .bind(body.kind.trim())
    .bind(body.admission_at)
    .bind(optional_text(body.admission_type))
    .bind(optional_text(body.admission_reason))
    .bind(body.department_id)
    .bind(body.ward_id)
    .bind(body.bed_id)
    .bind(body.attending_user_id)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(place_error)?;
    sqlx::query(
        "INSERT INTO case_stays (case_id, department_id, ward_id, bed_id, started_at, reason, created_by)
         VALUES ($1, $2, $3, $4, $5, 'aufnahme', $6)",
    )
    .bind(case_id)
    .bind(body.department_id)
    .bind(body.ward_id)
    .bind(body.bed_id)
    .bind(admission_at)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(place_error)?;

    let kind = body.kind.trim();
    audit(
        &mut tx,
        &user.auth,
        "case.admit",
        "case",
        case_id,
        Some(json!({ "kind": kind })),
    )
    .await?;
    ctx.events
        .publish(
            &mut tx,
            topics::CASE_ADMITTED,
            "case",
            case_id,
            json!({
                "kind": kind,
                "department_id": body.department_id,
                "ward_id": body.ward_id,
                "bed_id": body.bed_id,
            }),
        )
        .await?;
    let case = load_case(&mut tx, case_id).await?;
    tx.commit().await?;
    Ok(Json(case))
}

// --- Lesen -------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaseSearch {
    case_number: String,
}

/// Sucht einen Fall über die Fallnummer. Liefert nur Verwaltungsdaten
/// (Nummer, Art, Status, Abteilung); die Akte selbst verlangt den
/// Behandlungszusammenhang.
async fn find_cases(
    State(ctx): State<ModuleContext>,
    Query(q): Query<CaseSearch>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT c.id, c.case_number, c.kind, c.status, c.admission_at, c.discharge_at,
                c.patient_id, d.name AS department_name
         FROM cases c JOIN org_units d ON d.id = c.department_id
         WHERE c.case_number = $1",
    ))
    .bind(q.case_number.trim())
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

/// Fälle eines Patienten (Verwaltungsdaten, neueste zuerst).
async fn patient_cases(
    State(ctx): State<ModuleContext>,
    Path(patient_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT c.id, c.case_number, c.kind, c.status, c.admission_at, c.discharge_at,
                d.name AS department_name, w.name AS ward_name
         FROM cases c
         JOIN org_units d ON d.id = c.department_id
         LEFT JOIN org_units w ON w.id = c.ward_id
         WHERE c.patient_id = $1 ORDER BY c.admission_at DESC LIMIT 200",
    ))
    .bind(patient_id)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

/// Fall mit Patient, Ort und gültigen Warnhinweisen.
async fn get_case(State(ctx): State<ModuleContext>, Path(case_id): Path<Uuid>) -> ApiResult<Value> {
    let mut conn = ctx.pool().acquire().await?;
    let mut case = load_case(&mut conn, case_id).await?;
    let alerts: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT a.id, a.kind, a.text, a.recorded_at
         FROM patient_alerts a JOIN cases c ON c.patient_id = a.patient_id
         WHERE c.id = $1 AND a.ended_at IS NULL ORDER BY a.kind, a.recorded_at",
    ))
    .bind(case_id)
    .fetch_all(&mut *conn)
    .await?;
    case["alerts"] = Value::Array(camel_rows(alerts));
    Ok(Json(case))
}

async fn list_stays(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT s.id, s.started_at, s.ended_at, s.reason,
                s.department_id, d.name AS department_name,
                s.ward_id, w.name AS ward_name, s.bed_id, b.label AS bed_label
         FROM case_stays s
         JOIN org_units d ON d.id = s.department_id
         LEFT JOIN org_units w ON w.id = s.ward_id
         LEFT JOIN beds b ON b.id = s.bed_id
         WHERE s.case_id = $1 ORDER BY s.started_at",
    ))
    .bind(case_id)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

// --- Verlegung, Entlassung, Storno -------------------------------------------

#[derive(sqlx::FromRow)]
struct OpenCase {
    kind: String,
    status: String,
    version: i32,
    department_id: Uuid,
    ward_id: Option<Uuid>,
    bed_id: Option<Uuid>,
}

/// Sperrt den Fall und prüft Status und Version.
async fn lock_open_case(
    tx: &mut PgConnection,
    case_id: Uuid,
    version: i32,
) -> Result<OpenCase, ApiFailure> {
    let case: Option<OpenCase> = sqlx::query_as(
        "SELECT kind, status, version, department_id, ward_id, bed_id
         FROM cases WHERE id = $1 FOR UPDATE",
    )
    .bind(case_id)
    .fetch_optional(tx)
    .await?;
    let case = case.ok_or(ApiFailure::NotFound("Fall nicht gefunden"))?;
    if case.status != "aufgenommen" {
        return Err(ApiFailure::conflict(format!(
            "Der Fall ist bereits {}",
            case.status
        )));
    }
    if case.version != version {
        return Err(ApiFailure::stale_version());
    }
    Ok(case)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Transfer {
    department_id: Uuid,
    ward_id: Option<Uuid>,
    bed_id: Option<Uuid>,
    /// Standard: jetzt
    at: Option<DateTime<Utc>>,
    version: i32,
}

async fn transfer_case(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<Transfer>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let before = lock_open_case(&mut tx, case_id, body.version).await?;
    if before.department_id == body.department_id
        && before.ward_id == body.ward_id
        && before.bed_id == body.bed_id
    {
        return Err(ApiFailure::bad_request(
            "Der Fall befindet sich bereits dort",
        ));
    }
    if body.bed_id != before.bed_id {
        check_bed(&mut tx, body.ward_id, body.bed_id).await?;
    }
    let at: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE case_stays SET ended_at = COALESCE($2, now())
         WHERE case_id = $1 AND ended_at IS NULL RETURNING ended_at",
    )
    .bind(case_id)
    .bind(body.at)
    .fetch_one(&mut *tx)
    .await
    .map_err(place_error)?;
    sqlx::query(
        "INSERT INTO case_stays (case_id, department_id, ward_id, bed_id, started_at, reason, created_by)
         VALUES ($1, $2, $3, $4, $5, 'verlegung', $6)",
    )
    .bind(case_id)
    .bind(body.department_id)
    .bind(body.ward_id)
    .bind(body.bed_id)
    .bind(at)
    .bind(actor)
    .execute(&mut *tx)
    .await
    .map_err(place_error)?;
    sqlx::query(
        "UPDATE cases SET department_id = $2, ward_id = $3, bed_id = $4,
                updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(case_id)
    .bind(body.department_id)
    .bind(body.ward_id)
    .bind(body.bed_id)
    .execute(&mut *tx)
    .await
    .map_err(place_error)?;

    audit(&mut tx, &user.auth, "case.transfer", "case", case_id, None).await?;
    let freed_bed = before.bed_id.filter(|b| Some(*b) != body.bed_id);
    ctx.events
        .publish(
            &mut tx,
            topics::CASE_TRANSFERRED,
            "case",
            case_id,
            json!({
                "kind": before.kind,
                "from_department_id": before.department_id,
                "from_ward_id": before.ward_id,
                "freed_bed_id": freed_bed,
                "department_id": body.department_id,
                "ward_id": body.ward_id,
                "bed_id": body.bed_id,
            }),
        )
        .await?;
    let case = load_case(&mut tx, case_id).await?;
    tx.commit().await?;
    Ok(Json(case))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Discharge {
    /// Standard: jetzt
    discharge_at: Option<DateTime<Utc>>,
    discharge_type: String,
    version: i32,
}

async fn discharge_case(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<Discharge>,
) -> ApiResult<Value> {
    let mut tx = ctx.pool().begin().await?;
    let before = lock_open_case(&mut tx, case_id, body.version).await?;
    let at: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE case_stays SET ended_at = COALESCE($2, now())
         WHERE case_id = $1 AND ended_at IS NULL RETURNING ended_at",
    )
    .bind(case_id)
    .bind(body.discharge_at)
    .fetch_one(&mut *tx)
    .await
    .map_err(place_error)?;
    sqlx::query(
        "UPDATE cases SET status = 'entlassen', discharge_at = $2, discharge_type = $3,
                bed_id = NULL, updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(case_id)
    .bind(at)
    .bind(body.discharge_type.trim())
    .execute(&mut *tx)
    .await
    .map_err(place_error)?;

    audit(
        &mut tx,
        &user.auth,
        "case.discharge",
        "case",
        case_id,
        Some(json!({ "dischargeType": body.discharge_type.trim() })),
    )
    .await?;
    ctx.events
        .publish(
            &mut tx,
            topics::CASE_DISCHARGED,
            "case",
            case_id,
            json!({
                "kind": before.kind,
                "department_id": before.department_id,
                "ward_id": before.ward_id,
                "freed_bed_id": before.bed_id,
                "discharge_type": body.discharge_type.trim(),
            }),
        )
        .await?;
    let case = load_case(&mut tx, case_id).await?;
    tx.commit().await?;
    Ok(Json(case))
}

#[derive(Deserialize)]
struct Cancel {
    reason: String,
    version: i32,
}

/// Storniert eine irrtümliche Aufnahme. Der Fall bleibt mit Status
/// `storniert` erhalten; das Bett wird frei.
async fn cancel_case(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<Cancel>,
) -> ApiResult<Value> {
    if body.reason.trim().chars().count() < 5 {
        return Err(ApiFailure::bad_request(
            "Bitte den Grund der Stornierung angeben",
        ));
    }
    let mut tx = ctx.pool().begin().await?;
    let before = lock_open_case(&mut tx, case_id, body.version).await?;
    sqlx::query(
        "UPDATE case_stays SET ended_at = GREATEST(now(), started_at + interval '1 second')
         WHERE case_id = $1 AND ended_at IS NULL",
    )
    .bind(case_id)
    .execute(&mut *tx)
    .await
    .map_err(place_error)?;
    sqlx::query(
        "UPDATE cases SET status = 'storniert', bed_id = NULL,
                updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(case_id)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &user.auth, "case.cancel", "case", case_id, None).await?;
    ctx.events
        .publish(
            &mut tx,
            topics::CASE_CANCELLED,
            "case",
            case_id,
            json!({
                "kind": before.kind,
                "department_id": before.department_id,
                "ward_id": before.ward_id,
                "freed_bed_id": before.bed_id,
            }),
        )
        .await?;
    let case = load_case(&mut tx, case_id).await?;
    tx.commit().await?;
    Ok(Json(case))
}

// --- Klinischer Kurzstatus ---------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClinicalStatus {
    /// `stabil`, `beobachtung`, `instabil` oder leer
    stability: Option<String>,
    fasting: bool,
    dnr: bool,
    dni: bool,
    /// „Wenn–dann“-Anweisung an den Dienst
    contingency: Option<String>,
    planned_discharge: Option<NaiveDate>,
    attending_user_id: Option<Uuid>,
    version: i32,
}

/// Ersetzt den klinischen Kurzstatus (Stationsliste, Übergabe). Die
/// Festlegungen DNR/DNI ändert nur, wer `clinical.write` hat.
async fn put_clinical_status(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<ClinicalStatus>,
) -> ApiResult<Value> {
    let mut tx = ctx.pool().begin().await?;
    let current: Option<(bool, bool, i32)> =
        sqlx::query_as("SELECT dnr, dni, version FROM cases WHERE id = $1 FOR UPDATE")
            .bind(case_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (dnr, dni, version) = current.ok_or(ApiFailure::NotFound("Fall nicht gefunden"))?;
    if version != body.version {
        return Err(ApiFailure::stale_version());
    }
    let code_status_changed = dnr != body.dnr || dni != body.dni;
    if code_status_changed && !user.can(&P::ClinicalWrite) {
        return Err(ApiFailure::forbidden(
            "DNR/DNI kann nur ärztlich festgelegt werden",
        ));
    }
    sqlx::query(
        "UPDATE cases SET stability = $2, fasting = $3, dnr = $4, dni = $5, contingency = $6,
                planned_discharge = $7, attending_user_id = $8,
                updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(case_id)
    .bind(optional_text(body.stability))
    .bind(body.fasting)
    .bind(body.dnr)
    .bind(body.dni)
    .bind(optional_text(body.contingency))
    .bind(body.planned_discharge)
    .bind(body.attending_user_id)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "case.clinical_status",
        "case",
        case_id,
        Some(json!({ "codeStatusChanged": code_status_changed })),
    )
    .await?;
    let case = load_case(&mut tx, case_id).await?;
    tx.commit().await?;
    Ok(Json(case))
}

// --- Belegung einer Einheit --------------------------------------------------

/// Aktuelle Fälle einer Einheit und aller untergeordneten Einheiten
/// (Stationsliste). Enthält nur Kern-Daten; Aufgaben, Aufträge usw.
/// liefern die jeweiligen Module unter demselben Pfadpräfix.
async fn unit_census(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT c.id AS case_id, c.case_number, c.kind, c.admission_at, c.planned_discharge,
                c.stability, c.fasting, c.dnr, c.dni, c.contingency, c.version,
                c.department_id, d.name AS department_name,
                c.ward_id, w.name AS ward_name, r.name AS room_name, b.label AS bed_label,
                c.patient_id, p.name_family, p.name_given, p.birth_date, p.gender,
                u.display_name AS attending_name,
                (SELECT count(*) FROM patient_alerts a
                  WHERE a.patient_id = c.patient_id AND a.ended_at IS NULL) AS alert_count,
                EXISTS (SELECT 1 FROM patient_alerts a
                         WHERE a.patient_id = c.patient_id AND a.ended_at IS NULL
                           AND a.kind = 'isolation') AS isolation
         FROM cases c
         JOIN patients p ON p.id = c.patient_id
         JOIN org_units d ON d.id = c.department_id
         LEFT JOIN org_units w ON w.id = c.ward_id
         LEFT JOIN beds b ON b.id = c.bed_id
         LEFT JOIN rooms r ON r.id = b.room_id
         LEFT JOIN users u ON u.id = c.attending_user_id
         WHERE c.status = 'aufgenommen'
           AND (c.ward_id IN (SELECT id FROM sub) OR c.department_id IN (SELECT id FROM sub))
         ORDER BY w.name NULLS LAST, r.name NULLS LAST, b.label NULLS LAST, p.name_family"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

/// Betten einer Station (oder aller Stationen unterhalb der Einheit) mit
/// Belegung und Sperre.
async fn unit_beds(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "{UNIT_SUBTREE}
         SELECT b.id AS bed_id, b.label, b.blocked_reason, b.version,
                r.id AS room_id, r.name AS room_name, r.ward_id, w.name AS ward_name,
                c.id AS case_id, c.case_number, p.name_family, p.name_given, p.gender
         FROM rooms r
         JOIN org_units w ON w.id = r.ward_id
         JOIN beds b ON b.room_id = r.id AND b.active
         LEFT JOIN cases c ON c.bed_id = b.id AND c.status = 'aufgenommen'
         LEFT JOIN patients p ON p.id = c.patient_id
         WHERE r.active AND r.ward_id IN (SELECT id FROM sub)
         ORDER BY w.name, r.name, b.label"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}
