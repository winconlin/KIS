//! Fachmodul **Labor**: Proben und Befunde.
//!
//! Befunde kommen als Probe mit ihren Messwerten ins System — von Hand
//! oder über eine Schnittstelle (Laborsystem). Eine erneute Lieferung
//! derselben Probe (`externalId`) ersetzt deren Werte; so können vorläufige
//! Befunde durch endgültige überschrieben werden, ohne Dubletten.
//!
//! Die Bewertung (`flag`) liefert das Labor. Fehlt sie und liegen
//! Referenzgrenzen vor, wird beim Lesen H/L/N aus dem Zahlenwert
//! abgeleitet. Kritische Werte (HH/LL) erscheinen je Einheit in einer
//! eigenen Liste.
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/cases/:case_id/lab/specimens` | `lab.read` oder `clinical.read` |
//! | `GET  /api/v1/cases/:case_id/lab/cumulative` | wie oben |
//! | `POST /api/v1/cases/:case_id/lab/specimens` | `lab.write` |
//! | `GET  /api/v1/units/:unit_id/lab/critical` | `lab.read` oder `clinical.read` |

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, json_sql, list, optional_text, page_limit, ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct LabModule;

impl Module for LabModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "lab",
            title: "Labor",
            description: "Proben, Befunde, Verlauf je Analyt, kritische Werte",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::AnyOf(&[P::LabRead, P::ClinicalRead]);
        ModuleRoutes::new()
            .get("/api/v1/cases/:case_id/lab/specimens", read, list_specimens)
            .get("/api/v1/cases/:case_id/lab/cumulative", read, cumulative)
            .post(
                "/api/v1/cases/:case_id/lab/specimens",
                Access::Permission(P::LabWrite),
                submit_specimen,
            )
            .get("/api/v1/units/:unit_id/lab/critical", read, unit_critical)
            .with_state(ctx.clone())
    }
}

/// Bewertung: vom Labor geliefert, sonst aus Wert und Referenz abgeleitet.
const EFFECTIVE_FLAG: &str = "COALESCE(r.flag, CASE
        WHEN r.value_num IS NULL THEN NULL
        WHEN r.ref_high IS NOT NULL AND r.value_num > r.ref_high THEN 'H'
        WHEN r.ref_low IS NOT NULL AND r.value_num < r.ref_low THEN 'L'
        WHEN r.ref_high IS NOT NULL OR r.ref_low IS NOT NULL THEN 'N'
    END)";

#[derive(Deserialize)]
struct SpecimenQuery {
    limit: Option<i64>,
}

/// Proben des Falls mit ihren Werten, neueste zuerst.
async fn list_specimens(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<SpecimenQuery>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT s.id, s.kind, s.material, s.collected_at, s.external_id,
                COALESCE((SELECT jsonb_agg(jsonb_build_object(
                              'analyteCode', r.analyte_code, 'loinc', r.loinc, 'name', r.name,
                              'valueNum', r.value_num, 'valueText', r.value_text, 'unit', r.unit,
                              'refLow', r.ref_low, 'refHigh', r.ref_high, 'refText', r.ref_text,
                              'flag', {EFFECTIVE_FLAG}, 'status', r.status, 'comment', r.comment)
                            ORDER BY r.analyte_code)
                          FROM lab_results r WHERE r.specimen_id = s.id), '[]') AS results
         FROM lab_specimens s
         WHERE s.case_id = $1 ORDER BY s.collected_at DESC LIMIT $2"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(page_limit(q.limit, 50, 500))
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
struct CumulativeQuery {
    /// Kommagetrennte Analytkürzel; leer = alle
    analytes: Option<String>,
}

/// Verlauf je Analyt (Kumulativbefund): alle Werte in zeitlicher Folge
/// mit der Änderung zum Vorwert.
async fn cumulative(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<CumulativeQuery>,
) -> ApiResult<Value> {
    let analytes: Vec<String> = q
        .analytes
        .unwrap_or_default()
        .split(',')
        .map(|a| a.trim().to_uppercase())
        .filter(|a| !a.is_empty())
        .collect();
    let sql = json_sql(&format!(
        "SELECT v.analyte_code, max(v.name) AS name, max(v.unit) AS unit,
                jsonb_agg(jsonb_build_object(
                    'collectedAt', v.collected_at, 'valueNum', v.value_num,
                    'valueText', v.value_text, 'flag', v.flag, 'delta', v.delta)
                  ORDER BY v.collected_at) AS values
         FROM (SELECT r.analyte_code, r.name, r.unit, s.collected_at, r.value_num, r.value_text,
                      {EFFECTIVE_FLAG} AS flag,
                      r.value_num - lag(r.value_num) OVER (
                          PARTITION BY r.analyte_code ORDER BY s.collected_at) AS delta
               FROM lab_results r JOIN lab_specimens s ON s.id = r.specimen_id
               WHERE s.case_id = $1 AND (cardinality($2::text[]) = 0 OR r.analyte_code = ANY($2))) v
         GROUP BY v.analyte_code ORDER BY v.analyte_code"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .bind(&analytes)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewSpecimen {
    /// `labor` (Standard), `bga` oder `mikrobiologie`
    kind: Option<String>,
    material: Option<String>,
    collected_at: DateTime<Utc>,
    /// Probennummer des Laborsystems; erneute Lieferung ersetzt die Werte
    external_id: Option<String>,
    results: Vec<NewResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewResult {
    analyte_code: String,
    loinc: Option<String>,
    name: Option<String>,
    value_num: Option<f64>,
    value_text: Option<String>,
    unit: Option<String>,
    ref_low: Option<f64>,
    ref_high: Option<f64>,
    ref_text: Option<String>,
    flag: Option<String>,
    status: Option<String>,
    comment: Option<String>,
}

/// Nimmt eine Probe mit ihren Werten an (alles oder nichts).
async fn submit_specimen(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewSpecimen>,
) -> ApiResult<Value> {
    if body.results.is_empty() {
        return Err(ApiFailure::bad_request(
            "Eine Probe braucht mindestens einen Wert",
        ));
    }
    let actor = actor_id(&user.auth)?;
    let external_id = optional_text(body.external_id);
    let mut tx = ctx.pool().begin().await?;

    // Bekannte Probe (gleiche Probennummer, gleicher Fall) wird ergänzt.
    let existing: Option<(Uuid, Uuid)> = match &external_id {
        Some(ext) => {
            sqlx::query_as(
                "SELECT id, case_id FROM lab_specimens WHERE external_id = $1 FOR UPDATE",
            )
            .bind(ext)
            .fetch_optional(&mut *tx)
            .await?
        }
        None => None,
    };
    let (specimen_id, replaced) = match existing {
        Some((_, other_case)) if other_case != case_id => {
            return Err(ApiFailure::conflict(
                "Diese Probennummer gehört zu einem anderen Fall",
            ));
        }
        Some((id, _)) => (id, true),
        None => {
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO lab_specimens (case_id, kind, material, collected_at, external_id, recorded_by)
                 VALUES ($1, COALESCE($2, 'labor'), $3, $4, $5, $6) RETURNING id",
            )
            .bind(case_id)
            .bind(optional_text(body.kind))
            .bind(optional_text(body.material))
            .bind(body.collected_at)
            .bind(&external_id)
            .bind(actor)
            .fetch_one(&mut *tx)
            .await?;
            (id, false)
        }
    };

    let count = body.results.len();
    for r in body.results {
        sqlx::query(
            "INSERT INTO lab_results (specimen_id, analyte_code, loinc, name, value_num, value_text,
                                      unit, ref_low, ref_high, ref_text, flag, status, comment)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, COALESCE($12, 'final'), $13)
             ON CONFLICT (specimen_id, analyte_code) DO UPDATE
             SET loinc = EXCLUDED.loinc, name = EXCLUDED.name, value_num = EXCLUDED.value_num,
                 value_text = EXCLUDED.value_text, unit = EXCLUDED.unit, ref_low = EXCLUDED.ref_low,
                 ref_high = EXCLUDED.ref_high, ref_text = EXCLUDED.ref_text, flag = EXCLUDED.flag,
                 status = EXCLUDED.status, comment = EXCLUDED.comment",
        )
        .bind(specimen_id)
        .bind(r.analyte_code.trim().to_uppercase())
        .bind(optional_text(r.loinc))
        .bind(optional_text(r.name))
        .bind(r.value_num)
        .bind(optional_text(r.value_text))
        .bind(optional_text(r.unit))
        .bind(r.ref_low)
        .bind(r.ref_high)
        .bind(optional_text(r.ref_text))
        .bind(optional_text(r.flag))
        .bind(optional_text(r.status))
        .bind(optional_text(r.comment))
        .execute(&mut *tx)
        .await?;
    }
    audit(
        &mut tx,
        &user.auth,
        "lab.specimen_submit",
        "lab_specimen",
        specimen_id,
        Some(json!({ "results": count, "replaced": replaced })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({ "id": specimen_id, "results": count, "replaced": replaced }),
    ))
}

#[derive(Deserialize)]
struct CriticalQuery {
    /// Zeitraum in Stunden (Standard 24, höchstens 168)
    hours: Option<i64>,
}

/// Kritische Werte (HH/LL) der aktuellen Fälle einer Einheit.
async fn unit_critical(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
    Query(q): Query<CriticalQuery>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "WITH RECURSIVE sub AS (
             SELECT id FROM org_units WHERE id = $1
           UNION
             SELECT o.id FROM org_units o JOIN sub x ON o.parent_id = x.id)
         SELECT c.id AS case_id, c.case_number, p.name_family, p.name_given,
                s.collected_at, r.analyte_code, r.name, r.value_num, r.value_text, r.unit, r.flag
         FROM lab_results r
         JOIN lab_specimens s ON s.id = r.specimen_id
         JOIN cases c ON c.id = s.case_id AND c.status = 'aufgenommen'
         JOIN patients p ON p.id = c.patient_id
         WHERE r.flag IN ('HH', 'LL')
           AND s.collected_at > now() - make_interval(hours => $2)
           AND (c.ward_id IN (SELECT id FROM sub) OR c.department_id IN (SELECT id FROM sub))
         ORDER BY s.collected_at DESC LIMIT 500",
    ))
    .bind(unit_id)
    .bind(q.hours.unwrap_or(24).clamp(1, 168) as i32)
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}
