//! Fachmodul **Verpflegung**: Kostformen, Kostanordnung je Fall und die
//! Produktionsliste der Küche.
//!
//! Die Küche pflegt keine eigene Patientenliste. Ihre Produktionsliste
//! entsteht aus dem, was ohnehin im System steht:
//!
//! - **Belegung** (Kern): wer liegt auf welcher Station in welchem Bett,
//! - **Kostanordnung** (dieses Modul): welche Kostform gilt; ohne
//!   Anordnung die Standardkost,
//! - **Nüchtern-Kennzeichen** (Kern): kein Tablett,
//! - **Nahrungsmittelallergien und Isolation** (Kern, Warnhinweise).
//!
//! Eine Aufnahme, Verlegung oder Entlassung wirkt damit sofort auf die
//! Liste, ohne dass jemand die Küche benachrichtigt.
//!
//! **Datensparsamkeit:** Die Produktionsliste nennt Station, Zimmer und
//! Bett — keine Namen, keine Diagnosen. Wer in der Küche arbeitet,
//! braucht nur das Recht `meal.read` und keinen Zugriff auf Fälle.
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/meals/diets` | angemeldet |
//! | `POST /api/v1/meals/diets`, `PUT …/:id` | `catalog.write` |
//! | `GET  /api/v1/cases/:case_id/diet` | `nursing.read` oder `clinical.read` |
//! | `PUT  /api/v1/cases/:case_id/diet` | `meal.write` |
//! | `GET  /api/v1/meals/production` | `meal.read` |

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, camel, camel_rows, json_sql, list, missing_or_stale, optional_text,
    required_text, row_id, violated_constraint, ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct MealsModule;

impl Module for MealsModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "meals",
            title: "Verpflegung",
            description: "Kostformen, Kostanordnung, Produktionsliste der Küche",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let manage = Access::Permission(P::CatalogWrite);
        ModuleRoutes::new()
            .get("/api/v1/meals/diets", Access::Authenticated, list_diets)
            .post("/api/v1/meals/diets", manage, create_diet)
            .put("/api/v1/meals/diets/:id", manage, update_diet)
            .get(
                "/api/v1/cases/:case_id/diet",
                Access::AnyOf(&[P::NursingRead, P::ClinicalRead]),
                case_diet,
            )
            .put(
                "/api/v1/cases/:case_id/diet",
                Access::Permission(P::MealWrite),
                order_diet,
            )
            .get(
                "/api/v1/meals/production",
                Access::Permission(P::MealRead),
                production,
            )
            .with_state(ctx.clone())
    }
}

const DIET_COLUMNS: &str = "id, code, name, description, is_default, active, version";

fn diet_error(e: sqlx::Error) -> ApiFailure {
    match violated_constraint(&e) {
        Some("diets_one_default") => ApiFailure::conflict(
            "Es gibt bereits eine Standardkost; bitte dort zuerst das Kennzeichen entfernen",
        ),
        _ => e.into(),
    }
}

async fn list_diets(State(ctx): State<ModuleContext>) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {DIET_COLUMNS} FROM diets WHERE active ORDER BY is_default DESC, name"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql).fetch_all(ctx.pool()).await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DietBody {
    code: String,
    name: String,
    description: Option<String>,
    #[serde(default)]
    is_default: bool,
    active: Option<bool>,
    /// nur beim Ändern
    version: Option<i32>,
}

async fn create_diet(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<DietBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO diets (code, name, description, is_default) VALUES ($1, $2, $3, $4)
         RETURNING {DIET_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(body.code.trim())
        .bind(&name)
        .bind(optional_text(body.description))
        .bind(body.is_default)
        .fetch_one(&mut *tx)
        .await
        .map_err(diet_error)?;
    audit(
        &mut tx,
        &user.auth,
        "meal.diet_create",
        "diet",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn update_diet(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<DietBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let version = body
        .version
        .ok_or_else(|| ApiFailure::bad_request("version fehlt"))?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "UPDATE diets SET name = $2, description = $3, is_default = $4,
                active = COALESCE($5, active), updated_at = now(), version = version + 1
         WHERE id = $1 AND version = $6 RETURNING {DIET_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(&name)
        .bind(optional_text(body.description))
        .bind(body.is_default)
        .bind(body.active)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await
        .map_err(diet_error)?;
    let Some(row) = row else {
        return Err(missing_or_stale(&mut tx, "diets", id, "Kostform nicht gefunden").await?);
    };
    audit(&mut tx, &user.auth, "meal.diet_update", "diet", id, None).await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Aktuelle Kostanordnung des Falls und ihre Vorgänger.
async fn case_diet(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT cd.id, cd.diet_id, d.code, d.name, cd.note, cd.valid_from, cd.valid_until,
                u.display_name AS ordered_by_name
         FROM case_diets cd
         JOIN diets d ON d.id = cd.diet_id
         JOIN users u ON u.id = cd.ordered_by
         WHERE cd.case_id = $1 ORDER BY cd.valid_from DESC LIMIT 100",
    ))
    .bind(case_id)
    .fetch_all(ctx.pool())
    .await?;
    let rows = camel_rows(rows);
    let current = rows.iter().find(|r| r["validUntil"].is_null()).cloned();
    Ok(Json(json!({ "current": current, "history": rows })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DietOrder {
    diet_id: Uuid,
    /// Hinweis an die Küche (Konsistenz, Abneigungen) — keine Diagnosen
    note: Option<String>,
}

/// Ordnet eine Kostform an; die bisherige Anordnung endet im selben
/// Augenblick.
async fn order_diet(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<DietOrder>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    // Fall sperren: gleichzeitige Anordnungen laufen nacheinander.
    let open: Option<bool> =
        sqlx::query_scalar("SELECT status = 'aufgenommen' FROM cases WHERE id = $1 FOR UPDATE")
            .bind(case_id)
            .fetch_optional(&mut *tx)
            .await?;
    if open != Some(true) {
        return Err(ApiFailure::conflict(
            "Kost kann nur für laufende Fälle angeordnet werden",
        ));
    }
    let active: Option<bool> = sqlx::query_scalar("SELECT active FROM diets WHERE id = $1")
        .bind(body.diet_id)
        .fetch_optional(&mut *tx)
        .await?;
    if active != Some(true) {
        return Err(ApiFailure::bad_request(
            "Kostform nicht gefunden oder nicht aktiv",
        ));
    }
    sqlx::query(
        "UPDATE case_diets SET valid_until = clock_timestamp()
         WHERE case_id = $1 AND valid_until IS NULL",
    )
    .bind(case_id)
    .execute(&mut *tx)
    .await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO case_diets (case_id, diet_id, note, valid_from, ordered_by)
         VALUES ($1, $2, $3, clock_timestamp(), $4)
         RETURNING id, diet_id, note, valid_from, valid_until",
    ))
    .bind(case_id)
    .bind(body.diet_id)
    .bind(optional_text(body.note))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "meal.diet_order",
        "case",
        case_id,
        Some(json!({ "dietId": body.diet_id })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Produktionsliste: je belegtem Bett die Kostform, dazu Summen je
/// Station und Kostform. Ohne Namen und ohne Diagnosen.
async fn production(State(ctx): State<ModuleContext>) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT w.id AS ward_id, w.name AS ward_name, r.name AS room_name, b.label AS bed_label,
                c.fasting,
                COALESCE(d.code, dd.code) AS diet_code,
                COALESCE(d.name, dd.name) AS diet_name,
                cd.note,
                COALESCE((SELECT jsonb_agg(a.text ORDER BY a.text) FROM patient_alerts a
                           WHERE a.patient_id = c.patient_id AND a.ended_at IS NULL
                             AND a.kind = 'nahrungsmittelallergie'), '[]') AS food_allergies,
                EXISTS (SELECT 1 FROM patient_alerts a
                         WHERE a.patient_id = c.patient_id AND a.ended_at IS NULL
                           AND a.kind = 'isolation') AS isolation
         FROM cases c
         JOIN org_units w ON w.id = c.ward_id
         LEFT JOIN beds b ON b.id = c.bed_id
         LEFT JOIN rooms r ON r.id = b.room_id
         LEFT JOIN case_diets cd ON cd.case_id = c.id AND cd.valid_until IS NULL
         LEFT JOIN diets d ON d.id = cd.diet_id
         LEFT JOIN diets dd ON dd.is_default AND dd.active
         WHERE c.status = 'aufgenommen' AND c.kind IN ('stationaer', 'teilstationaer')
         ORDER BY w.name, r.name NULLS LAST, b.label NULLS LAST",
    ))
    .fetch_all(ctx.pool())
    .await?;

    // Summen je Station und Kostform (nüchterne Patienten erhalten nichts).
    let mut totals: std::collections::BTreeMap<(String, String), i64> = Default::default();
    let mut fasting = 0_i64;
    let mut without_diet = 0_i64;
    for r in &rows {
        if r["fasting"] == true {
            fasting += 1;
            continue;
        }
        match r["diet_name"].as_str() {
            Some(diet) => {
                let ward = r["ward_name"].as_str().unwrap_or_default().to_string();
                *totals.entry((ward, diet.to_string())).or_default() += 1;
            }
            None => without_diet += 1,
        }
    }
    let totals: Vec<Value> = totals
        .into_iter()
        .map(|((ward, diet), count)| json!({ "wardName": ward, "dietName": diet, "count": count }))
        .collect();
    Ok(Json(json!({
        "generatedAt": chrono::Utc::now(),
        "trays": rows.len() as i64 - fasting,
        "fasting": fasting,
        // Belegte Betten ohne Kostform: es ist keine Standardkost festgelegt
        "withoutDiet": without_diet,
        "totals": totals,
        "items": camel_rows(rows),
    })))
}
