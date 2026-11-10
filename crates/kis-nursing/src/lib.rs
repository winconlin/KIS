//! Fachmodul **Pflege**: Vitalwerte und Flüssigkeitsbilanz.
//!
//! # Dieses Modul ist die Vorlage für neue Module
//!
//! Es ist bewusst klein und zeigt jede Regel einmal. Wer ein neues Modul
//! baut (`scripts/new_module.py <name>`), hält sich an dieses Muster:
//!
//! 1. **Eine Crate, eine Datei `lib.rs`** (größere Module teilen in
//!    Untermodule auf). Abhängigkeit nur auf `kis-core`, nie auf andere
//!    Module.
//! 2. **`Module` implementieren**: Kennung, Abhängigkeiten, Routen.
//! 3. **Jede Route nennt ihr Recht** (`Access::…`). Ohne Recht lässt sich
//!    keine Route registrieren.
//! 4. **Falldaten liegen unter `/api/v1/cases/:case_id/…`**. Dann prüft
//!    der Kern vor dem Handler, ob der Benutzer den Fall sehen darf. Der
//!    Handler filtert zusätzlich immer mit `WHERE case_id = $1`.
//! 5. **Eigene Tabellen** stehen in `config/module_tables.toml` unter der
//!    Modulkennung. Fremde Modultabellen sind tabu; Kern-Tabellen
//!    (`cases`, `patients`, `org_units`, …) dürfen gelesen werden.
//! 6. **Schreiben = Transaktion + Audit**: fachliche Änderung und
//!    `audit(…)` in derselben Transaktion. Im Audit stehen nur Schlüssel
//!    und Codes, nie Messwerte oder Freitext.
//! 7. **Plausibilität prüft die Datenbank** (CHECK-Constraints in der
//!    Migration). Der Handler reicht Fehler mit `?` durch; daraus wird
//!    automatisch 400 bzw. 409.
//! 8. **Nichts löschen**: Fehleinträge werden als ungültig markiert.
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/cases/:case_id/vitals` | `nursing.read` oder `clinical.read` |
//! | `POST /api/v1/cases/:case_id/vitals` | `nursing.write` oder `clinical.write` |
//! | `POST /api/v1/cases/:case_id/vitals/:id/void` | wie Schreiben |
//! | `GET  /api/v1/cases/:case_id/fluid-balance` | wie Lesen |
//! | `POST /api/v1/cases/:case_id/fluid-balance` | wie Schreiben |
//! | `POST /api/v1/cases/:case_id/fluid-balance/:id/void` | wie Schreiben |

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use kis_core::http::{
    actor_id, audit, camel, camel_rows, json_sql, list, optional_text, page_limit, required_text,
    row_id, ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct NursingModule;

impl Module for NursingModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "nursing",
            title: "Pflege",
            description: "Vitalwerte und Flüssigkeitsbilanz",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::AnyOf(&[P::NursingRead, P::ClinicalRead]);
        let write = Access::AnyOf(&[P::NursingWrite, P::ClinicalWrite]);
        ModuleRoutes::new()
            .get("/api/v1/cases/:case_id/vitals", read, list_vitals)
            .post("/api/v1/cases/:case_id/vitals", write, record_vitals)
            .post("/api/v1/cases/:case_id/vitals/:id/void", write, void_vitals)
            .get("/api/v1/cases/:case_id/fluid-balance", read, fluid_balance)
            .post("/api/v1/cases/:case_id/fluid-balance", write, record_fluid)
            .post(
                "/api/v1/cases/:case_id/fluid-balance/:id/void",
                write,
                void_fluid,
            )
            .with_state(ctx.clone())
    }
}

// --- Vitalwerte --------------------------------------------------------------

#[derive(Deserialize)]
struct VitalsQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

/// Gültige Messungen eines Falls, neueste zuerst.
async fn list_vitals(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<VitalsQuery>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT v.id, v.measured_at, v.systolic, v.diastolic, v.heart_rate, v.temperature,
                v.spo2, v.resp_rate, v.weight_kg, v.pain_nrs, v.note,
                u.display_name AS recorded_by_name
         FROM vital_signs v JOIN users u ON u.id = v.recorded_by
         WHERE v.case_id = $1 AND v.voided_at IS NULL
           AND ($2::timestamptz IS NULL OR v.measured_at >= $2)
           AND ($3::timestamptz IS NULL OR v.measured_at < $3)
         ORDER BY v.measured_at DESC LIMIT $4",
    ))
    .bind(case_id)
    .bind(q.from)
    .bind(q.to)
    .bind(page_limit(q.limit, 200, 1000))
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewVitals {
    /// Standard: jetzt
    measured_at: Option<DateTime<Utc>>,
    systolic: Option<i32>,
    diastolic: Option<i32>,
    heart_rate: Option<i32>,
    temperature: Option<f64>,
    spo2: Option<i32>,
    resp_rate: Option<i32>,
    weight_kg: Option<f64>,
    pain_nrs: Option<i32>,
    note: Option<String>,
}

/// Erfasst eine Messung. Wertebereiche, „mindestens ein Wert“ und
/// „Blutdruck nur paarweise“ erzwingt die Datenbank.
async fn record_vitals(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewVitals>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO vital_signs (case_id, measured_at, systolic, diastolic, heart_rate,
                                  temperature, spo2, resp_rate, weight_kg, pain_nrs, note, recorded_by)
         VALUES ($1, COALESCE($2, now()), $3, $4, $5, $6::numeric, $7, $8, $9::numeric, $10, $11, $12)
         RETURNING id, measured_at, systolic, diastolic, heart_rate, temperature, spo2,
                   resp_rate, weight_kg, pain_nrs, note",
    ))
    .bind(case_id)
    .bind(body.measured_at)
    .bind(body.systolic)
    .bind(body.diastolic)
    .bind(body.heart_rate)
    .bind(body.temperature)
    .bind(body.spo2)
    .bind(body.resp_rate)
    .bind(body.weight_kg)
    .bind(body.pain_nrs)
    .bind(optional_text(body.note))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "nursing.vitals_record",
        "vital_signs",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Markiert eine Messung als Fehleintrag.
async fn void_vitals(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query(
        "UPDATE vital_signs SET voided_at = now(), voided_by = $3
         WHERE id = $2 AND case_id = $1 AND voided_at IS NULL",
    )
    .bind(case_id)
    .bind(id)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound("Messung nicht gefunden"));
    }
    audit(
        &mut tx,
        &user.auth,
        "nursing.vitals_void",
        "vital_signs",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

// --- Flüssigkeitsbilanz ------------------------------------------------------

#[derive(Deserialize)]
struct BalanceQuery {
    /// Bilanztag (Standard: heute); der Tag läuft von 00:00 bis 24:00 UTC
    date: Option<NaiveDate>,
}

/// Einträge eines Bilanztages mit Summen für Einfuhr, Ausfuhr und Bilanz.
async fn fluid_balance(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
    Query(q): Query<BalanceQuery>,
) -> ApiResult<Value> {
    let date = q.date.unwrap_or_else(|| Utc::now().date_naive());
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT id, recorded_at, direction, category, volume_ml
         FROM fluid_balance_entries
         WHERE case_id = $1 AND voided_at IS NULL
           AND recorded_at >= $2::date AND recorded_at < $2::date + 1
         ORDER BY recorded_at",
    ))
    .bind(case_id)
    .bind(date)
    .fetch_all(ctx.pool())
    .await?;
    let sum = |direction: &str| -> i64 {
        rows.iter()
            .filter(|r| r["direction"] == direction)
            .filter_map(|r| r["volume_ml"].as_i64())
            .sum()
    };
    let (intake, output) = (sum("einfuhr"), sum("ausfuhr"));
    Ok(Json(json!({
        "date": date,
        "intakeMl": intake,
        "outputMl": output,
        "balanceMl": intake - output,
        "items": camel_rows(rows),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewFluid {
    /// Standard: jetzt
    recorded_at: Option<DateTime<Utc>>,
    /// `einfuhr` oder `ausfuhr`
    direction: String,
    /// z. B. oral, infusion, urin, drainage
    category: String,
    volume_ml: i32,
}

async fn record_fluid(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewFluid>,
) -> ApiResult<Value> {
    let category = required_text("category", &body.category)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO fluid_balance_entries (case_id, recorded_at, direction, category, volume_ml, recorded_by)
         VALUES ($1, COALESCE($2, now()), $3, $4, $5, $6)
         RETURNING id, recorded_at, direction, category, volume_ml",
    ))
    .bind(case_id)
    .bind(body.recorded_at)
    .bind(body.direction.trim())
    .bind(&category)
    .bind(body.volume_ml)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "nursing.fluid_record",
        "fluid_balance",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn void_fluid(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query(
        "UPDATE fluid_balance_entries SET voided_at = now(), voided_by = $3
         WHERE id = $2 AND case_id = $1 AND voided_at IS NULL",
    )
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
        "nursing.fluid_void",
        "fluid_balance",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}
