//! Fachmodul **Auftragswesen**: ein Mechanismus für alle Leistungen, die
//! ein Bereich bei einem anderen anfordert.
//!
//! Labor, Bildgebung, Konsil, Physiotherapie, Sozialdienst, Transport,
//! Reinigung, Technik, Apotheke — fachlich verschieden, im Ablauf gleich:
//! *anfordern → annehmen → erledigen (mit Ergebnis) oder stornieren*.
//! Was es im Haus gibt, steht im **Leistungskatalog** (Daten, vom Haus
//! gepflegt); jede Leistung nennt die erbringende Einheit, in deren
//! **Arbeitsliste** der Auftrag erscheint.
//!
//! Zusammenspiel mit anderen Teilen des Systems — ohne sie zu kennen:
//!
//! - Ein Auftrag zu einem Fall macht die erbringende Einheit für die Dauer
//!   des Auftrags zur **mitbehandelnden Einheit** (Kernfunktion
//!   `treatment::add_participant`). Die Radiologie sieht den Fall also
//!   genau so lange, wie sie für ihn arbeitet.
//! - **Automatik-Regeln** erzeugen Aufträge aus Ereignissen: „Bei
//!   Entlassung → Bettenaufbereitung, Bett bis zur Erledigung gesperrt“.
//!   Die Regeln sind Daten; das Fallmodul weiß nichts davon. Schlägt die
//!   Automatik fehl, läuft die Entlassung trotzdem durch (siehe
//!   `kis_core::events`).
//!
//! | Route | Recht |
//! |---|---|
//! | `GET  /api/v1/catalog/services` | angemeldet |
//! | `POST /api/v1/catalog/services`, `PUT …/:id` | `catalog.write` |
//! | `GET/POST /api/v1/catalog/automation-rules`, `DELETE …/:id` | `catalog.write` |
//! | `GET  /api/v1/cases/:case_id/orders` | `order.read` |
//! | `POST /api/v1/cases/:case_id/orders`, `…/:id/cancel` | `order.write` |
//! | `POST /api/v1/units/:unit_id/requests` (ohne Fallbezug) | `order.write` |
//! | `GET  /api/v1/units/:unit_id/worklist` | `order.process` |
//! | `POST /api/v1/units/:unit_id/worklist/:id/accept`, `…/complete`, `…/reject` | `order.process` |

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use kis_core::events::{topics, Event, EventSubscriber};
use kis_core::http::{
    actor_id, audit, camel, json_sql, list, missing_or_stale, optional_text, required_text, row_id,
    ApiFailure, ApiResult,
};
use kis_core::identity::CurrentUser;
use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;
use kis_core::{facility, treatment};

pub struct OrdersModule;

impl Module for OrdersModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "orders",
            title: "Auftragswesen",
            description: "Leistungskatalog, Anforderungen, Arbeitslisten, Automatik-Regeln",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let catalog = Access::Permission(P::CatalogWrite);
        let request = Access::Permission(P::OrderWrite);
        let process = Access::Permission(P::OrderProcess);
        ModuleRoutes::new()
            .get(
                "/api/v1/catalog/services",
                Access::Authenticated,
                list_services,
            )
            .post("/api/v1/catalog/services", catalog, create_service)
            .put("/api/v1/catalog/services/:id", catalog, update_service)
            .get("/api/v1/catalog/automation-rules", catalog, list_rules)
            .post("/api/v1/catalog/automation-rules", catalog, create_rule)
            .delete(
                "/api/v1/catalog/automation-rules/:id",
                catalog,
                deactivate_rule,
            )
            .get(
                "/api/v1/cases/:case_id/orders",
                Access::Permission(P::OrderRead),
                case_orders,
            )
            .post("/api/v1/cases/:case_id/orders", request, order_for_case)
            .post(
                "/api/v1/cases/:case_id/orders/:id/cancel",
                request,
                cancel_by_requester,
            )
            .post("/api/v1/units/:unit_id/requests", request, order_for_unit)
            .get("/api/v1/units/:unit_id/worklist", process, worklist)
            .post(
                "/api/v1/units/:unit_id/worklist/:id/accept",
                process,
                accept,
            )
            .post(
                "/api/v1/units/:unit_id/worklist/:id/complete",
                process,
                complete,
            )
            .post(
                "/api/v1/units/:unit_id/worklist/:id/reject",
                process,
                reject,
            )
            .with_state(ctx.clone())
    }

    fn subscribers(&self, _ctx: &ModuleContext) -> Vec<Arc<dyn EventSubscriber>> {
        vec![Arc::new(Automation)]
    }
}

// --- Leistungskatalog --------------------------------------------------------

const SERVICE_COLUMNS: &str =
    "s.id, s.code, s.name, s.category, s.provider_unit_id, s.requires_case,
    s.requires_result, s.default_priority, s.active, s.version";

#[derive(Deserialize)]
struct ServiceFilter {
    category: Option<String>,
    all: Option<bool>,
}

async fn list_services(
    State(ctx): State<ModuleContext>,
    Query(f): Query<ServiceFilter>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "SELECT {SERVICE_COLUMNS}, u.name AS provider_unit_name
         FROM service_catalog s JOIN org_units u ON u.id = s.provider_unit_id
         WHERE ($1::text IS NULL OR s.category = $1) AND ($2 OR s.active)
         ORDER BY s.category, s.name"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(f.category)
        .bind(f.all.unwrap_or(false))
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceBody {
    code: String,
    name: String,
    category: String,
    provider_unit_id: Uuid,
    requires_case: Option<bool>,
    requires_result: Option<bool>,
    default_priority: Option<String>,
    active: Option<bool>,
    /// nur beim Ändern
    version: Option<i32>,
}

async fn create_service(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<ServiceBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "INSERT INTO service_catalog AS s (code, name, category, provider_unit_id, requires_case,
                                          requires_result, default_priority)
         VALUES ($1, $2, $3, $4, COALESCE($5, TRUE), COALESCE($6, FALSE), COALESCE($7, 'routine'))
         RETURNING {SERVICE_COLUMNS}"
    ));
    let row: Value = sqlx::query_scalar(&sql)
        .bind(body.code.trim())
        .bind(&name)
        .bind(body.category.trim())
        .bind(body.provider_unit_id)
        .bind(body.requires_case)
        .bind(body.requires_result)
        .bind(optional_text(body.default_priority))
        .fetch_one(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &user.auth,
        "catalog.service_create",
        "service",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn update_service(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ServiceBody>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let version = body
        .version
        .ok_or_else(|| ApiFailure::bad_request("version fehlt"))?;
    let mut tx = ctx.pool().begin().await?;
    let sql = json_sql(&format!(
        "UPDATE service_catalog s
         SET name = $2, category = $3, provider_unit_id = $4,
             requires_case = COALESCE($5, requires_case),
             requires_result = COALESCE($6, requires_result),
             default_priority = COALESCE($7, default_priority),
             active = COALESCE($8, active), updated_at = now(), version = version + 1
         WHERE id = $1 AND version = $9 RETURNING {SERVICE_COLUMNS}"
    ));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(id)
        .bind(&name)
        .bind(body.category.trim())
        .bind(body.provider_unit_id)
        .bind(body.requires_case)
        .bind(body.requires_result)
        .bind(optional_text(body.default_priority))
        .bind(body.active)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Err(
            missing_or_stale(&mut tx, "service_catalog", id, "Leistung nicht gefunden").await?,
        );
    };
    audit(
        &mut tx,
        &user.auth,
        "catalog.service_update",
        "service",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

// --- Automatik-Regeln --------------------------------------------------------

async fn list_rules(State(ctx): State<ModuleContext>) -> ApiResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(&json_sql(
        "SELECT r.id, r.name, r.on_topic, r.service_id, s.name AS service_name, r.case_kinds,
                r.block_bed, r.details, r.active
         FROM automation_rules r JOIN service_catalog s ON s.id = r.service_id
         ORDER BY r.active DESC, r.on_topic, r.name",
    ))
    .fetch_all(ctx.pool())
    .await?;
    Ok(list(rows))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewRule {
    name: String,
    /// `case.admitted`, `case.transferred` oder `case.discharged`
    on_topic: String,
    service_id: Uuid,
    /// Fallarten, für die die Regel gilt (leer = alle)
    #[serde(default)]
    case_kinds: Vec<String>,
    /// Sperrt das frei werdende Bett, bis der Auftrag erledigt ist
    #[serde(default)]
    block_bed: bool,
    details: Option<String>,
}

async fn create_rule(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Json(body): Json<NewRule>,
) -> ApiResult<Value> {
    let name = required_text("name", &body.name)?;
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let row: Value = sqlx::query_scalar(&json_sql(
        "INSERT INTO automation_rules (name, on_topic, service_id, case_kinds, block_bed, details, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, name, on_topic, service_id, case_kinds, block_bed, details, active",
    ))
    .bind(&name)
    .bind(body.on_topic.trim())
    .bind(body.service_id)
    .bind(&body.case_kinds)
    .bind(body.block_bed)
    .bind(optional_text(body.details))
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "catalog.rule_create",
        "automation_rule",
        row_id(&row)?,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(camel(row)))
}

async fn deactivate_rule(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let done = sqlx::query(
        "UPDATE automation_rules SET active = FALSE, updated_by = $2, updated_at = now()
         WHERE id = $1 AND active",
    )
    .bind(id)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Err(ApiFailure::NotFound(
            "Regel nicht gefunden oder bereits inaktiv",
        ));
    }
    audit(
        &mut tx,
        &user.auth,
        "catalog.rule_deactivate",
        "automation_rule",
        id,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "success": true })))
}

// --- Aufträge anlegen --------------------------------------------------------

const ORDER_SELECT: &str = "
    SELECT o.id, o.case_id, o.service_id, s.code AS service_code, s.name AS service_name,
           s.category, o.requesting_unit_id, ru.name AS requesting_unit_name,
           o.provider_unit_id, pu.name AS provider_unit_name, o.bed_id, o.releases_bed,
           o.details, o.priority, o.status, o.scheduled_for, o.requested_at,
           rb.display_name AS requested_by_name, o.automation_rule_id,
           o.accepted_at, o.result_text, o.completed_at, o.cancelled_reason, o.version
    FROM orders o
    JOIN service_catalog s ON s.id = o.service_id
    JOIN org_units pu ON pu.id = o.provider_unit_id
    LEFT JOIN org_units ru ON ru.id = o.requesting_unit_id
    LEFT JOIN users rb ON rb.id = o.requested_by";

fn source_of(order_id: Uuid) -> String {
    format!("order:{order_id}")
}

async fn load_order(conn: &mut PgConnection, id: Uuid) -> Result<Value, ApiFailure> {
    let sql = json_sql(&format!("{ORDER_SELECT} WHERE o.id = $1"));
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(conn)
        .await?;
    row.map(camel)
        .ok_or(ApiFailure::NotFound("Auftrag nicht gefunden"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewOrder {
    service_id: Uuid,
    /// Fragestellung / Auftragstext
    details: Option<String>,
    priority: Option<String>,
    scheduled_for: Option<DateTime<Utc>>,
}

struct Service {
    provider_unit_id: Uuid,
    requires_case: bool,
    default_priority: String,
}

async fn active_service(conn: &mut PgConnection, id: Uuid) -> Result<Service, ApiFailure> {
    let row: Option<(Uuid, bool, String)> = sqlx::query_as(
        "SELECT provider_unit_id, requires_case, default_priority
         FROM service_catalog WHERE id = $1 AND active",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    let (provider_unit_id, requires_case, default_priority) =
        row.ok_or_else(|| ApiFailure::bad_request("Leistung nicht gefunden oder nicht aktiv"))?;
    Ok(Service {
        provider_unit_id,
        requires_case,
        default_priority,
    })
}

/// Anforderung zu einem Fall. Anfordernde Einheit ist der aktuelle Ort
/// des Falls (Station, sonst Fachabteilung).
async fn order_for_case(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(case_id): Path<Uuid>,
    Json(body): Json<NewOrder>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let service = active_service(&mut tx, body.service_id).await?;
    let place: Option<(String, Uuid)> =
        sqlx::query_as("SELECT status, COALESCE(ward_id, department_id) FROM cases WHERE id = $1")
            .bind(case_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (status, requesting_unit) = place.ok_or(ApiFailure::NotFound("Fall nicht gefunden"))?;
    if status != "aufgenommen" {
        return Err(ApiFailure::conflict(
            "Für abgeschlossene Fälle können keine Leistungen angefordert werden",
        ));
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO orders (service_id, case_id, requesting_unit_id, provider_unit_id, details,
                             priority, scheduled_for, requested_by)
         VALUES ($1, $2, $3, $4, $5, COALESCE($6, $7), $8, $9) RETURNING id",
    )
    .bind(body.service_id)
    .bind(case_id)
    .bind(requesting_unit)
    .bind(service.provider_unit_id)
    .bind(optional_text(body.details))
    .bind(optional_text(body.priority))
    .bind(&service.default_priority)
    .bind(body.scheduled_for)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    // Die erbringende Einheit behandelt mit, solange der Auftrag offen ist.
    treatment::add_participant(&mut tx, case_id, service.provider_unit_id, &source_of(id)).await?;
    audit(
        &mut tx,
        &user.auth,
        "order.create",
        "order",
        id,
        Some(json!({ "serviceId": body.service_id, "caseId": case_id })),
    )
    .await?;
    let order = load_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
}

/// Anforderung ohne Fallbezug (Störmeldung, Materialtransport …) aus
/// einer Einheit heraus.
async fn order_for_unit(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(unit_id): Path<Uuid>,
    Json(body): Json<NewOrder>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let service = active_service(&mut tx, body.service_id).await?;
    if service.requires_case {
        return Err(ApiFailure::bad_request(
            "Diese Leistung kann nur zu einem Fall angefordert werden",
        ));
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO orders (service_id, requesting_unit_id, provider_unit_id, details, priority,
                             scheduled_for, requested_by)
         VALUES ($1, $2, $3, $4, COALESCE($5, $6), $7, $8) RETURNING id",
    )
    .bind(body.service_id)
    .bind(unit_id)
    .bind(service.provider_unit_id)
    .bind(optional_text(body.details))
    .bind(optional_text(body.priority))
    .bind(&service.default_priority)
    .bind(body.scheduled_for)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &user.auth,
        "order.create",
        "order",
        id,
        Some(json!({ "serviceId": body.service_id })),
    )
    .await?;
    let order = load_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
}

async fn case_orders(
    State(ctx): State<ModuleContext>,
    Path(case_id): Path<Uuid>,
) -> ApiResult<Value> {
    let sql = json_sql(&format!(
        "{ORDER_SELECT} WHERE o.case_id = $1 ORDER BY o.requested_at DESC LIMIT 500"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(case_id)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

// --- Aufträge bearbeiten -----------------------------------------------------

#[derive(Deserialize)]
struct WorklistQuery {
    /// `open` (Standard), `done` oder `all`
    status: Option<String>,
}

/// Arbeitsliste der Einheit und ihrer Untereinheiten: dringende zuerst.
/// Zu fallbezogenen Aufträgen stehen Patient und Ort dabei.
async fn worklist(
    State(ctx): State<ModuleContext>,
    Path(unit_id): Path<Uuid>,
    Query(q): Query<WorklistQuery>,
) -> ApiResult<Value> {
    let statuses: &[&str] = match q.status.as_deref().unwrap_or("open") {
        "open" => &["angefordert", "angenommen"],
        "done" => &["erledigt", "storniert"],
        "all" => &["angefordert", "angenommen", "erledigt", "storniert"],
        _ => {
            return Err(ApiFailure::bad_request(
                "status muss open, done oder all sein",
            ))
        }
    };
    let sql = json_sql(&format!(
        "WITH RECURSIVE sub AS (
             SELECT id FROM org_units WHERE id = $1
           UNION
             SELECT u.id FROM org_units u JOIN sub s ON u.parent_id = s.id),
         o2 AS ({ORDER_SELECT} WHERE o.provider_unit_id IN (SELECT id FROM sub) AND o.status = ANY($2))
         SELECT o2.*, c.case_number, p.name_family, p.name_given, p.birth_date,
                w.name AS ward_name, r.name AS room_name, b.label AS bed_label
         FROM o2
         LEFT JOIN cases c ON c.id = o2.case_id
         LEFT JOIN patients p ON p.id = c.patient_id
         LEFT JOIN beds b ON b.id = COALESCE(o2.bed_id, c.bed_id)
         LEFT JOIN rooms r ON r.id = b.room_id
         LEFT JOIN org_units w ON w.id = COALESCE(r.ward_id, c.ward_id)
         ORDER BY array_position(ARRAY['notfall', 'dringend', 'routine'], o2.priority),
                  o2.requested_at
         LIMIT 1000"
    ));
    let rows: Vec<Value> = sqlx::query_scalar(&sql)
        .bind(unit_id)
        .bind(statuses)
        .fetch_all(ctx.pool())
        .await?;
    Ok(list(rows))
}

#[derive(sqlx::FromRow)]
struct LockedOrder {
    status: String,
    case_id: Option<Uuid>,
    bed_id: Option<Uuid>,
    releases_bed: bool,
    requires_result: bool,
    version: i32,
}

/// Sperrt einen Auftrag. `unit_id` gesetzt: der Auftrag muss in die
/// Arbeitsliste dieser Einheit (oder einer Untereinheit) gehören.
/// `case_id` gesetzt: der Auftrag muss zu diesem Fall gehören.
async fn lock_order(
    tx: &mut PgConnection,
    id: Uuid,
    unit_id: Option<Uuid>,
    case_id: Option<Uuid>,
    version: i32,
) -> Result<LockedOrder, ApiFailure> {
    let order: Option<LockedOrder> = sqlx::query_as(
        "WITH RECURSIVE sub AS (
             SELECT id FROM org_units WHERE id = $2
           UNION
             SELECT u.id FROM org_units u JOIN sub s ON u.parent_id = s.id)
         SELECT o.status, o.case_id, o.bed_id, o.releases_bed, s.requires_result, o.version
         FROM orders o JOIN service_catalog s ON s.id = o.service_id
         WHERE o.id = $1
           AND ($2::uuid IS NULL OR o.provider_unit_id IN (SELECT id FROM sub))
           AND ($3::uuid IS NULL OR o.case_id = $3)
         FOR UPDATE OF o",
    )
    .bind(id)
    .bind(unit_id)
    .bind(case_id)
    .fetch_optional(tx)
    .await?;
    let order = order.ok_or(ApiFailure::NotFound("Auftrag nicht gefunden"))?;
    if order.version != version {
        return Err(ApiFailure::stale_version());
    }
    Ok(order)
}

fn expect_status(order: &LockedOrder, allowed: &[&str]) -> Result<(), ApiFailure> {
    if allowed.contains(&order.status.as_str()) {
        Ok(())
    } else {
        Err(ApiFailure::conflict(format!(
            "Der Auftrag ist bereits {}",
            order.status
        )))
    }
}

#[derive(Deserialize)]
struct VersionOnly {
    version: i32,
}

async fn accept(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((unit_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<VersionOnly>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let mut tx = ctx.pool().begin().await?;
    let order = lock_order(&mut tx, id, Some(unit_id), None, body.version).await?;
    expect_status(&order, &["angefordert"])?;
    sqlx::query(
        "UPDATE orders SET status = 'angenommen', accepted_by = $2, accepted_at = now(),
                updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &user.auth, "order.accept", "order", id, None).await?;
    let order = load_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Completion {
    /// Befund / Ergebnis; Pflicht, wenn die Leistung es verlangt
    result_text: Option<String>,
    version: i32,
}

/// Erledigt einen Auftrag: Ergebnis festhalten, Mitbehandlung beenden,
/// gesperrtes Bett freigeben, Ereignis `order.completed` melden.
async fn complete(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((unit_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Completion>,
) -> ApiResult<Value> {
    let actor = actor_id(&user.auth)?;
    let result = optional_text(body.result_text);
    let mut tx = ctx.pool().begin().await?;
    let order = lock_order(&mut tx, id, Some(unit_id), None, body.version).await?;
    expect_status(&order, &["angefordert", "angenommen"])?;
    if order.requires_result && result.is_none() {
        return Err(ApiFailure::bad_request(
            "Diese Leistung verlangt einen Befund (resultText)",
        ));
    }
    sqlx::query(
        "UPDATE orders SET status = 'erledigt', result_text = $2, completed_by = $3,
                completed_at = now(), updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(&result)
    .bind(actor)
    .execute(&mut *tx)
    .await?;
    close_order(&mut tx, id, &order).await?;
    audit(&mut tx, &user.auth, "order.complete", "order", id, None).await?;
    ctx.events
        .publish(
            &mut tx,
            topics::ORDER_COMPLETED,
            "order",
            id,
            json!({ "case_id": order.case_id, "has_result": result.is_some() }),
        )
        .await?;
    let order = load_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
}

#[derive(Deserialize)]
struct Cancellation {
    reason: String,
    version: i32,
}

/// Ablehnung durch die erbringende Einheit.
async fn reject(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((unit_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Cancellation>,
) -> ApiResult<Value> {
    cancel(&ctx, &user, id, Some(unit_id), None, body, "order.reject").await
}

/// Stornierung durch die anfordernde Seite.
async fn cancel_by_requester(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path((case_id, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Cancellation>,
) -> ApiResult<Value> {
    cancel(&ctx, &user, id, None, Some(case_id), body, "order.cancel").await
}

async fn cancel(
    ctx: &ModuleContext,
    user: &CurrentUser,
    id: Uuid,
    unit_id: Option<Uuid>,
    case_id: Option<Uuid>,
    body: Cancellation,
    action: &str,
) -> ApiResult<Value> {
    let reason = required_text("reason", &body.reason)?;
    let mut tx = ctx.pool().begin().await?;
    let order = lock_order(&mut tx, id, unit_id, case_id, body.version).await?;
    expect_status(&order, &["angefordert", "angenommen"])?;
    sqlx::query(
        "UPDATE orders SET status = 'storniert', cancelled_reason = $2,
                updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(&reason)
    .execute(&mut *tx)
    .await?;
    close_order(&mut tx, id, &order).await?;
    audit(&mut tx, &user.auth, action, "order", id, None).await?;
    let order = load_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
}

/// Folgen eines abgeschlossenen Auftrags: Mitbehandlung endet, ein für
/// den Auftrag gesperrtes Bett wird frei.
async fn close_order(
    tx: &mut PgConnection,
    id: Uuid,
    order: &LockedOrder,
) -> Result<(), ApiFailure> {
    if order.case_id.is_some() {
        treatment::end_participant(tx, &source_of(id)).await?;
    }
    if let (true, Some(bed_id)) = (order.releases_bed, order.bed_id) {
        facility::release_bed(tx, bed_id).await?;
    }
    Ok(())
}

// --- Automatik ---------------------------------------------------------------

/// Erzeugt Aufträge aus Fall-Ereignissen nach den Regeln des Hauses.
struct Automation;

#[derive(sqlx::FromRow)]
struct Rule {
    id: Uuid,
    name: String,
    service_id: Uuid,
    provider_unit_id: Uuid,
    priority: String,
    block_bed: bool,
    details: Option<String>,
}

#[axum::async_trait]
impl EventSubscriber for Automation {
    fn name(&self) -> &'static str {
        "orders.automation"
    }

    fn topics(&self) -> &'static [&'static str] {
        &[
            topics::CASE_ADMITTED,
            topics::CASE_TRANSFERRED,
            topics::CASE_DISCHARGED,
        ]
    }

    async fn handle(&self, tx: &mut PgConnection, event: &Event) -> anyhow::Result<()> {
        let kind = event.payload["kind"].as_str().unwrap_or_default();
        let freed_bed = event.payload["freed_bed_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok());
        let unit_key = if event.topic == topics::CASE_ADMITTED {
            "ward_id"
        } else {
            "from_ward_id"
        };
        let requesting_unit = event.payload[unit_key]
            .as_str()
            .or_else(|| event.payload["ward_id"].as_str())
            .and_then(|s| Uuid::parse_str(s).ok());

        let rules: Vec<Rule> = sqlx::query_as(
            "SELECT r.id, r.name, r.service_id, s.provider_unit_id, s.default_priority AS priority,
                    r.block_bed, r.details
             FROM automation_rules r JOIN service_catalog s ON s.id = r.service_id
             WHERE r.active AND s.active AND r.on_topic = $1
               AND (cardinality(r.case_kinds) = 0 OR $2 = ANY(r.case_kinds))",
        )
        .bind(&event.topic)
        .bind(kind)
        .fetch_all(&mut *tx)
        .await?;

        for rule in rules {
            // Eine Bett-Regel ohne frei gewordenes Bett hat nichts zu tun.
            let bed = if rule.block_bed { freed_bed } else { None };
            if rule.block_bed && bed.is_none() {
                continue;
            }
            // Bett-Aufträge hängen am Bett, nicht am (beendeten) Fall: die
            // Reinigung braucht den Ort, keine Patientendaten.
            let case_id = if rule.block_bed {
                None
            } else {
                Some(event.entity_id)
            };
            let order_id: Uuid = sqlx::query_scalar(
                "INSERT INTO orders (service_id, case_id, requesting_unit_id, provider_unit_id,
                                     bed_id, releases_bed, details, priority, automation_rule_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
            )
            .bind(rule.service_id)
            .bind(case_id)
            .bind(requesting_unit)
            .bind(rule.provider_unit_id)
            .bind(bed)
            .bind(rule.block_bed)
            .bind(&rule.details)
            .bind(&rule.priority)
            .bind(rule.id)
            .fetch_one(&mut *tx)
            .await?;
            if let Some(bed_id) = bed {
                facility::block_bed(tx, bed_id, &rule.name).await?;
            }
            if let Some(case_id) = case_id {
                treatment::add_participant(
                    tx,
                    case_id,
                    rule.provider_unit_id,
                    &source_of(order_id),
                )
                .await?;
            }
        }
        Ok(())
    }
}
