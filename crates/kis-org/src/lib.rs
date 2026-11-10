//! Kernmodul **Organisation**: der Aufbau des Hauses.
//!
//! Ein Baum aus Einheiten (`org_units`) bildet jedes Haus ab — von der
//! Einzelklinik bis zum Verbund: Träger → Einrichtung → Fachabteilung →
//! Station, daneben Funktionsbereiche, Ambulanzen, Verwaltung und
//! Versorgung (Küche, Reinigung, Technik, Transport). Stationen haben
//! Zimmer und Betten.
//!
//! Alles Weitere hängt an diesen Einheiten: Zuordnung der Beschäftigten,
//! Behandlungszusammenhang, Arbeitslisten, Aufträge, Dienstpläne.
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/org/units` | `org.read` |
//! | `POST /api/v1/org/units`, `PUT /api/v1/org/units/:id` | `org.write` |
//! | `GET  /api/v1/org/wards/:ward_id/rooms` | `org.read` |
//! | `POST /api/v1/org/rooms`, `POST /api/v1/org/beds`, `PUT /api/v1/org/beds/:id` | `org.write` |
//! | `GET  /api/v1/me/units` | angemeldet |

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use kis_core::http::{
    audit, camel, camel_rows, json_sql, list, missing_or_stale, optional_text, required_text,
    row_id, ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;
use kis_core::treatment;

pub struct OrgModule;

impl Module for OrgModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "org",
            title: "Organisation",
            description: "Einrichtungen, Fachabteilungen, Stationen, Bereiche, Zimmer und Betten",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &[],
            maturity: Maturity::Stable,
            kernel: true,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::Permission(P::OrgRead);
        let write = Access::Permission(P::OrgWrite);
        ModuleRoutes::new()
            .get("/api/v1/org/units", read, list_units)
            .post("/api/v1/org/units", write, create_unit)
            .put("/api/v1/org/units/:id", write, update_unit)
            .get("/api/v1/org/wards/:ward_id/rooms", read, list_rooms)
            .post("/api/v1/org/rooms", write, create_room)
            .post("/api/v1/org/beds", write, create_bed)
            .put("/api/v1/org/beds/:id", write, update_bed)
            .get("/api/v1/me/units", Access::Authenticated, my_units)
            .with_state(ctx.clone())
    }
}

const UNIT_COLUMNS: &str =
    "id, parent_id, kind, code, name, ik_number, specialty_key, care_level, active, version";

#[derive(Deserialize)]
struct UnitFilter {
    kind: Option<String>,
    /// `true`: auch stillgelegte Einheiten
    all: Option<bool>,
}

async fn list_units(
    State(ctx): State<ModuleContext>,
    Query(f): Query<UnitFilter>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {UNIT_COLUMNS} FROM org_units
         WHERE ($1::text IS NULL OR kind = $1) AND ($2 OR active)
         ORDER BY kind, name"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(f.kind)
        .bind(f.all.unwrap_or(false))
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewUnit {
    parent_id: Option<Uuid>,
    kind: String,
    code: String,
    name: String,
    ik_number: Option<String>,
    specialty_key: Option<String>,
    care_level: Option<String>,
}

async fn create_unit(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<NewUnit>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO org_units (parent_id, kind, code, name, ik_number, specialty_key, care_level)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {UNIT_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(body.parent_id)
        .bind(body.kind.trim())
        .bind(body.code.trim())
        .bind(&name)
        .bind(optional_text(body.ik_number))
        .bind(optional_text(body.specialty_key))
        .bind(optional_text(body.care_level))
        .fetch_one(&mut *tx)
        .await?;
    let id = row_id(&row)?;
    audit(&mut tx, &user.auth, "org.unit_create", "org_unit", id, None).await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnitUpdate {
    parent_id: Option<Uuid>,
    name: String,
    ik_number: Option<String>,
    specialty_key: Option<String>,
    care_level: Option<String>,
    active: bool,
    version: i32,
}

/// Ersetzt die änderbaren Angaben einer Einheit. Art und Kürzel bleiben
/// fest (an ihnen hängen Fremdschlüssel und Schnittstellen).
async fn update_unit(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UnitUpdate>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    if let Some(parent) = body.parent_id {
        // Die neue übergeordnete Einheit darf nicht unterhalb dieser liegen.
        let cycle: bool = sqlx::query_scalar(
            "WITH RECURSIVE sub AS (
                 SELECT id FROM org_units WHERE id = $1
               UNION
                 SELECT o.id FROM org_units o JOIN sub s ON o.parent_id = s.id)
             SELECT EXISTS (SELECT 1 FROM sub WHERE id = $2)",
        )
        .bind(id)
        .bind(parent)
        .fetch_one(&mut *tx)
        .await?;
        if cycle {
            return Err(ApiFailure::bad_request(
                "Eine Einheit kann nicht sich selbst oder einer eigenen Untereinheit untergeordnet werden",
            ));
        }
    }
    let sql = json_sql(&format!(
        "UPDATE org_units SET parent_id = $2, name = $3, ik_number = $4, specialty_key = $5,
                care_level = $6, active = $7, updated_at = now(), version = version + 1
         WHERE id = $1 AND version = $8 RETURNING {UNIT_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(body.parent_id)
        .bind(&name)
        .bind(optional_text(body.ik_number))
        .bind(optional_text(body.specialty_key))
        .bind(optional_text(body.care_level))
        .bind(body.active)
        .bind(body.version)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(missing_or_stale(&mut tx, "org_units", id, "Einheit nicht gefunden").await?);
    };
    audit(&mut tx, &user.auth, "org.unit_update", "org_unit", id, None).await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Zimmer und Betten einer Station.
async fn list_rooms(
    State(ctx): State<ModuleContext>,
    Path(ward_id): Path<Uuid>,
) -> ApiResult<Value> {
    let rooms: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT id, ward_id, name, active FROM rooms WHERE ward_id = $1 ORDER BY name",
    ))
    .bind(ward_id)
    .fetch_all(ctx.pool())
    .await?;
    let beds: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT b.id, b.room_id, b.label, b.blocked_reason, b.active, b.version
         FROM beds b JOIN rooms r ON r.id = b.room_id
         WHERE r.ward_id = $1 ORDER BY r.name, b.label",
    ))
    .bind(ward_id)
    .fetch_all(ctx.pool())
    .await?;
    Ok(Json(serde_json::json!({
        "rooms": camel_rows(rooms),
        "beds": camel_rows(beds),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewRoom {
    ward_id: Uuid,
    name: String,
}

async fn create_room(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<NewRoom>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO rooms (ward_id, name) VALUES ($1, $2) RETURNING id, ward_id, name, active",
    ))
    .bind(body.ward_id)
    .bind(&name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match kis_core::http::violated_constraint(&e) {
        Some("rooms_ward_id_ward_kind_fkey") => {
            ApiFailure::bad_request("Zimmer können nur auf Stationen angelegt werden")
        }
        _ => e.into(),
    })?;
    audit(
        &mut tx,
        &user.auth,
        "org.room_create",
        "room",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewBed {
    room_id: Uuid,
    label: String,
}

async fn create_bed(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<NewBed>,
) -> ApiResult<Value> {
    let label = required_text("label", &body.label)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO beds (room_id, label) VALUES ($1, $2)
         RETURNING id, room_id, label, blocked_reason, active, version",
    ))
    .bind(body.room_id)
    .bind(&label)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "org.bed_create",
        "bed",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedUpdate {
    /// Text = gesperrt mit dieser Begründung; fehlt/leer = nicht gesperrt
    blocked_reason: Option<String>,
    active: bool,
    version: i32,
}

/// Sperrt, entsperrt oder legt ein Bett still. Ein belegtes Bett kann
/// nicht stillgelegt werden.
async fn update_bed(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<BedUpdate>,
) -> ApiResult<Value> {
    let mut tx = ctx.pool().begin().await?;
    if !body.active {
        let occupied: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM case_stays WHERE bed_id = $1 AND ended_at IS NULL)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if occupied {
            return Err(ApiFailure::conflict("Das Bett ist belegt"));
        }
    }
    let row: Option<Value> = sqlx::query_scalar(&json_sql(
        "UPDATE beds SET blocked_reason = $2, active = $3, updated_at = now(), version = version + 1
         WHERE id = $1 AND version = $4
         RETURNING id, room_id, label, blocked_reason, active, version",
    ))
    .bind(id)
    .bind(optional_text(body.blocked_reason))
    .bind(body.active)
    .bind(body.version)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(missing_or_stale(&mut tx, "beds", id, "Bett nicht gefunden").await?);
    };
    audit(&mut tx, &user.auth, "org.bed_update", "bed", id, None).await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

/// Einheiten, denen der angemeldete Benutzer angehört (direkt oder über
/// eine übergeordnete Einheit). Grundlage für „meine Station“ in der
/// Oberfläche.
async fn my_units(State(ctx): State<ModuleContext>, user: CurrentUser) -> ApiResult<Value> {
    let ids = treatment::unit_ids_of(ctx.pool(), &user.auth).await?;
    let sql = json_sql(&format!(
        "SELECT {UNIT_COLUMNS} FROM org_units WHERE id = ANY($1) AND active ORDER BY kind, name"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(&ids)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}
