//! Warnhinweise zum Patienten: Allergien, Nahrungsmittelallergien, CAVE,
//! Isolation, Implantate.
//!
//! Sie gehören zum Kern (`patient_alerts`), weil viele Bereiche sie
//! brauchen: Stationsliste, Verordnung, Küche. Hinweise werden nie
//! gelöscht, sondern beendet — so bleibt nachvollziehbar, was wann galt.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use kis_core::http::{actor_id, audit, camel, json_sql, required_text, ApiFailure, ApiResult};
use kis_core::identity::CurrentUser;

pub(crate) async fn list(
    State(db): State<Arc<PgPool>>,
    Path(patient_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT a.id, a.kind, a.text, a.recorded_at, a.ended_at, u.display_name AS recorded_by_name
         FROM patient_alerts a JOIN users u ON u.id = a.recorded_by
         WHERE a.patient_id = $1 ORDER BY a.ended_at IS NOT NULL, a.kind, a.recorded_at",
    ))
    .bind(patient_id)
    .fetch_all(&*db)
    .await?;
    Ok(kis_core::http::list(rows))
}

#[derive(Deserialize)]
pub(crate) struct NewAlert {
    /// `allergie`, `nahrungsmittelallergie`, `cave`, `isolation`,
    /// `implantat` oder `sonstiges`
    kind: String,
    text: String,
}

pub(crate) async fn create(
    State(db): State<Arc<PgPool>>,
    user: CurrentUser,
    Path(patient_id): Path<Uuid>,
    Json(body): Json<NewAlert>,
) -> ApiResult<Value> {
    let text = required_text("text", &body.text)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = db.begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO patient_alerts (patient_id, kind, text, recorded_by)
         SELECT id, $2, $3, $4 FROM patients WHERE id = $1 AND deleted_at IS NULL
         RETURNING id, kind, text, recorded_at, ended_at",
    ))
    .bind(patient_id)
    .bind(body.kind.trim())
    .bind(&text)
    .bind(actor)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiFailure::NotFound("Patient nicht gefunden"))?;
    // Audit ohne den Text: nur Art des Hinweises.
    audit(
        &mut tx,
        &user.auth,
        "patient.alert_create",
        "patient",
        patient_id,
        Some(serde_json::json!({ "kind": body.kind.trim() })),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

pub(crate) async fn end(
    State(db): State<Arc<PgPool>>,
    user: CurrentUser,
    Path((patient_id, alert_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = db.begin().await?;
    let done = sqlx::query(
        "UPDATE patient_alerts SET ended_at = now(), ended_by = $3
         WHERE id = $2 AND patient_id = $1 AND ended_at IS NULL",
    )
    .bind(patient_id)
    .bind(alert_id)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound(
            "Hinweis nicht gefunden oder bereits beendet",
        ));
    }
    audit(
        &mut tx,
        &user.auth,
        "patient.alert_end",
        "patient",
        patient_id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "success": true })))
}
