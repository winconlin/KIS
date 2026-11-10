//! Fachmodul **Stationäre Verordnung**: Medikation während des
//! Aufenthalts und die Dokumentation jeder Gabe.
//!
//! - Verordnungen sind `aktiv`, `pausiert`, bei `bedarf` oder `abgesetzt`.
//!   Abgesetzte bleiben mit Enddatum erhalten.
//! - **Antiinfektiva** führen einen Therapiekurs: Beginn, geplante Dauer,
//!   daraus Therapietag und Überfälligkeit — die Liste zeigt, was zur
//!   Reevaluation ansteht.
//! - **Allergie-Abgleich**: Kommt ein Wort aus einer gültigen Allergie
//!   des Patienten (Kern-Tabelle `patient_alerts`) im Präparat oder
//!   Wirkstoff vor, wird die Verordnung nur mit ausdrücklicher Bestätigung
//!   angenommen. Das ist ein Textabgleich, keine Arzneimitteldatenbank:
//!   Er erkennt „Penicillin“ in „Penicillin V“, aber keine Kreuzallergien
//!   und keine Handelsnamen — er ersetzt keine pharmakologische Prüfung.
//! - **Gaben** dokumentiert, wer `medication.administer` hat; nicht
//!   gegebene Dosen verlangen eine Begründung (erzwingt die Datenbank).
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/cases/:case_id/prescriptions` | `prescription.read` |
//! | `POST /api/v1/cases/:case_id/prescriptions` | `prescription.write` |
//! | `PUT  /api/v1/cases/:case_id/prescriptions/:id/status` | `prescription.write` |
//! | `GET  /api/v1/cases/:case_id/administrations` | `prescription.read` |
//! | `POST /api/v1/cases/:case_id/prescriptions/:id/administrations` | `medication.administer` |

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, camel, json_sql, list, missing_or_stale, optional_text, required_text, row_id,
    ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct PrescriptionsModule;

impl Module for PrescriptionsModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "prescriptions",
            title: "Stationäre Verordnung",
            description: "Verordnungen, Antiinfektiva-Kurse, Allergie-Abgleich, Gabedokumentation",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::Permission(P::PrescriptionRead);
        let write = Access::Permission(P::PrescriptionWrite);
        ModuleRoutes::new()
            .get(
                "/api/v1/cases/:case_id/prescriptions",
                read,
                list_prescriptions,
            )
            .post("/api/v1/cases/:case_id/prescriptions", write, prescribe)
            .put(
                "/api/v1/cases/:case_id/prescriptions/:id/status",
                write,
                set_status,
            )
            .get(
                "/api/v1/cases/:case_id/administrations",
                read,
                list_administrations,
            )
            .post(
                "/api/v1/cases/:case_id/prescriptions/:id/administrations",
                Access::Permission(P::MedicationAdminister),
                administer,
            )
            .with_state(ctx.clone())
    }
}

/// Verordnung mit Therapietag und Überfälligkeit (nur Antiinfektiva).
const PRESCRIPTION_COLUMNS: &str = "x.id, x.drug_name, x.substance, x.atc_code, x.dose, x.schedule,
    x.route, x.status, x.indication, x.is_antiinfective, x.start_date, x.planned_days, x.end_date,
    x.version,
    CASE WHEN x.is_antiinfective AND x.status <> 'abgesetzt'
         THEN (CURRENT_DATE - x.start_date) + 1 END AS therapy_day,
    (x.is_antiinfective AND x.status IN ('aktiv', 'pausiert') AND x.planned_days IS NOT NULL
        AND CURRENT_DATE > x.start_date + x.planned_days - 1) AS overdue";

#[derive(Deserialize)]
struct ListQuery {
    /// `true`: auch abgesetzte
    all: Option<bool>,
}

async fn list_prescriptions(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {PRESCRIPTION_COLUMNS}, u.display_name AS prescribed_by_name
         FROM prescriptions x JOIN users u ON u.id = x.prescribed_by
         WHERE x.case_id = $1 AND ($2 OR x.status <> 'abgesetzt')
         ORDER BY array_position(ARRAY['aktiv', 'bedarf', 'pausiert', 'abgesetzt'], x.status),
                  x.drug_name"
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
struct NewPrescription {
    drug_name: String,
    substance: Option<String>,
    atc_code: Option<String>,
    dose: Option<String>,
    /// z. B. `1-0-1-0` oder „alle 8 h“
    schedule: Option<String>,
    route: Option<String>,
    /// `aktiv` (Standard) oder `bedarf`
    status: Option<String>,
    indication: Option<String>,
    #[serde(default)]
    is_antiinfective: bool,
    /// Standard: heute
    start_date: Option<NaiveDate>,
    planned_days: Option<i32>,
    /// Bestätigung, dass trotz passender Allergie verordnet wird
    #[serde(default)]
    acknowledge_allergy: bool,
}

async fn prescribe(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewPrescription>,
) -> ApiResult<Value> {
    let drug = required_text("drugName", &body.drug_name)?;
    let substance = optional_text(body.substance);
    let status = optional_text(body.status).unwrap_or_else(|| "aktiv".to_string());
    if !matches!(status.as_str(), "aktiv" | "bedarf") {
        return Err(ApiFailure::bad_request(
            "status muss aktiv oder bedarf sein",
        ));
    }
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;

    let open: Option<bool> =
        sqlx::query_scalar("SELECT status = 'aufgenommen' FROM cases WHERE id = $1")
            .bind(case_id)
            .fetch_optional(&mut *tx)
            .await?;
    match open {
        None => return Err(ApiFailure::NotFound("Fall nicht gefunden")),
        Some(false) => {
            return Err(ApiFailure::conflict(
                "Für abgeschlossene Fälle kann nicht verordnet werden",
            ))
        }
        Some(true) => {}
    }

    // Allergie-Abgleich gegen die Warnhinweise des Patienten (Kern): Ein
    // Hinweis passt, wenn eines seiner Wörter (ab vier Zeichen) im
    // Präparat oder Wirkstoff vorkommt. Lieber eine Rückfrage zu viel.
    let matching: Vec<String> = sqlx::query_scalar(
        "SELECT a.text FROM patient_alerts a JOIN cases c ON c.patient_id = a.patient_id
         WHERE c.id = $1 AND a.kind = 'allergie' AND a.ended_at IS NULL
           AND EXISTS (
               SELECT 1 FROM regexp_split_to_table(lower(a.text), '[^[:alnum:]]+') AS word
               WHERE length(word) >= 4
                 AND position(word IN lower($2 || ' ' || COALESCE($3, ''))) > 0)",
    )
    .bind(case_id)
    .bind(&drug)
    .bind(&substance)
    .fetch_all(&mut *tx)
    .await?;
    if !matching.is_empty() && !body.acknowledge_allergy {
        return Err(ApiFailure::conflict(format!(
            "Allergie-Hinweis passt zur Verordnung ({}). Verordnung nur mit Bestätigung (acknowledgeAllergy).",
            matching.join("; ")
        )));
    }

    let sql = json_sql(&format!(
        "INSERT INTO prescriptions AS x (case_id, drug_name, substance, atc_code, dose, schedule, route,
                                        status, indication, is_antiinfective, start_date,
                                        planned_days, prescribed_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, COALESCE($11, CURRENT_DATE), $12, $13)
         RETURNING {PRESCRIPTION_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(&drug)
        .bind(&substance)
        .bind(optional_text(body.atc_code))
        .bind(optional_text(body.dose))
        .bind(optional_text(body.schedule))
        .bind(optional_text(body.route))
        .bind(&status)
        .bind(optional_text(body.indication))
        .bind(body.is_antiinfective)
        .bind(body.start_date)
        .bind(body.planned_days)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "prescription.create",
        "prescription",
        row_id(&row)?,
        Some(json!({ "allergyAcknowledged": !matching.is_empty() })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
struct StatusChange {
    /// `aktiv`, `pausiert`, `bedarf` oder `abgesetzt`
    status: String,
    version: i32,
}

/// Pausiert, setzt fort oder setzt ab. Abgesetzte Verordnungen sind
/// endgültig; bei Bedarf wird neu verordnet.
async fn set_status(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<StatusChange>,
) -> ApiResult<Value> {
    let status = body.status.trim().to_string();
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "UPDATE prescriptions x
         SET status = $3,
             stopped_by = CASE WHEN $3 = 'abgesetzt' THEN $4::uuid END,
             end_date = CASE WHEN $3 = 'abgesetzt' THEN GREATEST(CURRENT_DATE, x.start_date) END,
             updated_at = now(), version = version + 1
         WHERE id = $2 AND case_id = $1 AND version = $5 AND status <> 'abgesetzt'
         RETURNING {PRESCRIPTION_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(id)
        .bind(&status)
        .bind(actor)
        .bind(body.version)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(
            missing_or_stale(&mut tx, "prescriptions", id, "Verordnung nicht gefunden").await?,
        );
    };
    audit(
        &mut tx,
        &user.auth,
        "prescription.status",
        "prescription",
        id,
        Some(json!({ "status": status })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
struct AdministrationQuery {
    /// Tag (Standard: heute)
    date: Option<NaiveDate>,
}

async fn list_administrations(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<AdministrationQuery>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT m.id, m.prescription_id, x.drug_name, m.given_at, m.outcome, m.dose, m.note,
                u.display_name AS recorded_by_name
         FROM medication_administrations m
         JOIN prescriptions x ON x.id = m.prescription_id
         JOIN users u ON u.id = m.recorded_by
         WHERE x.case_id = $1 AND m.given_at >= $2::date AND m.given_at < $2::date + 1
         ORDER BY m.given_at",
    ))
    .bind(case_id)
    .bind(q.date.unwrap_or_else(|| Utc::now().date_naive()))
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewAdministration {
    /// Standard: jetzt
    given_at: Option<DateTime<Utc>>,
    /// `gegeben` (Standard), `verweigert` oder `ausgelassen`
    outcome: Option<String>,
    dose: Option<String>,
    note: Option<String>,
}

async fn administer(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewAdministration>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM prescriptions WHERE id = $2 AND case_id = $1 FOR SHARE",
    )
    .bind(case_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    match status.as_deref() {
        None => return Err(ApiFailure::NotFound("Verordnung nicht gefunden")),
        Some("aktiv") | Some("bedarf") => {}
        Some(other) => {
            return Err(ApiFailure::conflict(format!(
                "Die Verordnung ist {other}; eine Gabe kann nicht dokumentiert werden"
            )))
        }
    }
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO medication_administrations (prescription_id, given_at, outcome, dose, note, recorded_by)
         VALUES ($1, COALESCE($2, now()), COALESCE($3, 'gegeben'), $4, $5, $6)
         RETURNING id, prescription_id, given_at, outcome, dose, note",
    ))
    .bind(id)
    .bind(body.given_at)
    .bind(optional_text(body.outcome))
    .bind(optional_text(body.dose))
    .bind(optional_text(body.note))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "prescription.administer",
        "prescription",
        id,
        Some(json!({ "outcome": row["outcome"] })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}
