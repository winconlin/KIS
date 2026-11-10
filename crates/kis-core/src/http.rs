//! HTTP-Bausteine für Fachmodule: einheitliche Fehlerantworten,
//! Audit-Helfer und Eingabeprüfungen.
//!
//! Handler neuer Module geben [`ApiResult`] zurück und verwenden `?`:
//! Datenbankfehler werden automatisch in passende Statuscodes übersetzt
//! (Eindeutigkeit/Überschneidung → 409, Fremdschlüssel/Plausibilität →
//! 400). Interne Details gelangen nie in die Antwort, nur ins Log.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use uuid::Uuid;

use crate::identity::AuthUser;

/// Ergebnis eines Handlers.
pub type ApiResult<T> = Result<Json<T>, ApiFailure>;

/// Fehler eines Handlers. Jede Variante entspricht einem Statuscode.
#[derive(Debug)]
pub enum ApiFailure {
    /// 400 — Eingabe fachlich oder formal ungültig
    BadRequest(String),
    /// 403 — Aktion für diesen Benutzer nicht zulässig
    Forbidden(String),
    /// 404 — Datensatz nicht vorhanden (oder nicht in diesem Fall)
    NotFound(&'static str),
    /// 409 — Konflikt (veraltete Version, Überschneidung, Zustand)
    Conflict(String),
    /// 500 — Detail nur im Log, Antwort generisch
    Internal(String),
}

impl ApiFailure {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    pub fn internal(detail: impl std::fmt::Display) -> Self {
        Self::Internal(detail.to_string())
    }

    /// 409 für fehlgeschlagenes Optimistic Locking.
    pub fn stale_version() -> Self {
        Self::Conflict(
            "Der Datensatz wurde zwischenzeitlich geändert (veraltete Version)".to_string(),
        )
    }

    pub fn status(&self) -> StatusCode {
        match self {
            ApiFailure::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiFailure::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiFailure::NotFound(_) => StatusCode::NOT_FOUND,
            ApiFailure::Conflict(_) => StatusCode::CONFLICT,
            ApiFailure::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = match self {
            ApiFailure::BadRequest(m) | ApiFailure::Forbidden(m) | ApiFailure::Conflict(m) => m,
            ApiFailure::NotFound(m) => m.to_string(),
            ApiFailure::Internal(detail) => {
                tracing::error!(%detail, "Interner Fehler");
                "Interner Serverfehler".to_string()
            }
        };
        (
            status,
            Json(serde_json::json!({
                "success": false,
                "error": message,
                "status": status.as_u16(),
            })),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for ApiFailure {
    fn from(e: sqlx::Error) -> Self {
        if matches!(e, sqlx::Error::RowNotFound) {
            return ApiFailure::NotFound("Nicht gefunden");
        }
        if let Some(db) = e.as_database_error() {
            let constraint = db.constraint().unwrap_or("").to_string();
            let suffix = if constraint.is_empty() {
                String::new()
            } else {
                format!(" ({constraint})")
            };
            match db.code().as_deref() {
                Some("23505") => {
                    return ApiFailure::Conflict(format!(
                        "Datensatz kollidiert mit einem vorhandenen{suffix}"
                    ))
                }
                Some("23P01") => {
                    return ApiFailure::Conflict(format!(
                        "Zeitliche Überschneidung mit einem vorhandenen Eintrag{suffix}"
                    ))
                }
                Some("23503") => {
                    return ApiFailure::BadRequest(format!(
                        "Verweis auf einen nicht vorhandenen Datensatz{suffix}"
                    ))
                }
                Some("23514") | Some("23502") => {
                    return ApiFailure::BadRequest(format!(
                        "Eingabe verletzt eine Plausibilitätsregel{suffix}"
                    ))
                }
                // Schutz-Trigger (append-only, freigegebene Dokumentation)
                Some("42501") => {
                    return ApiFailure::Conflict(
                        "Der Datensatz ist geschützt und kann so nicht geändert werden".to_string(),
                    )
                }
                _ => {}
            }
        }
        ApiFailure::Internal(format!("DB: {e}"))
    }
}

impl From<anyhow::Error> for ApiFailure {
    fn from(e: anyhow::Error) -> Self {
        ApiFailure::Internal(e.to_string())
    }
}

/// Name der verletzten Constraint, falls der Fehler eine ist. Damit kann
/// ein Handler für bestimmte Regeln eine genauere Meldung liefern.
pub fn violated_constraint(e: &sqlx::Error) -> Option<&str> {
    e.as_database_error().and_then(|db| db.constraint())
}

/// Benutzer-ID als UUID (für `…_by`-Spalten).
pub fn actor_id(user: &AuthUser) -> Result<Uuid, ApiFailure> {
    user.uuid()
        .ok_or_else(|| ApiFailure::internal("Benutzer-ID ist keine UUID"))
}

/// Schreibt einen Audit-Eintrag in die laufende Transaktion.
///
/// `details` enthält nur Schlüssel, Codes und Feldnamen — keine
/// Personendaten, keine Freitexte.
pub async fn audit(
    tx: &mut sqlx::PgConnection,
    user: &AuthUser,
    action: &str,
    entity_type: &str,
    entity_id: Uuid,
    details: Option<serde_json::Value>,
) -> Result<(), ApiFailure> {
    crate::audit::append_audit(
        tx,
        &user.user_id,
        action,
        entity_type,
        Some(&entity_id.to_string()),
        None,
        details.as_ref(),
        None,
    )
    .await
    .map_err(ApiFailure::from)
}

/// Kürzt Leerraum und verlangt nicht leeren Text.
pub fn required_text(field: &str, value: &str) -> Result<String, ApiFailure> {
    let v = value.trim();
    if v.is_empty() {
        return Err(ApiFailure::BadRequest(format!(
            "{field} darf nicht leer sein"
        )));
    }
    Ok(v.to_string())
}

/// Kürzt Leerraum; leerer Text wird zu `None`.
pub fn optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Macht aus einer Abfrage eine, die jede Ergebniszeile als JSON-Objekt
/// liefert. Funktioniert für `SELECT` und für `INSERT/UPDATE … RETURNING`.
/// PostgreSQL übernimmt die Umwandlung aller Spaltentypen (Datum, Zahl,
/// Array, JSONB); im Handler ist kein Zeilen-Struct nötig:
///
/// ```text
/// let sql = json_sql("SELECT id, measured_at FROM vital_signs WHERE case_id = $1");
/// let rows: Vec<Value> = sqlx::query_scalar(&sql).bind(case_id).fetch_all(pool).await?;
/// Ok(list(rows))
/// ```
pub fn json_sql(statement: &str) -> String {
    format!("WITH q AS ({statement}) SELECT to_jsonb(q) FROM q")
}

/// Benennt die Schlüssel eines Zeilen-Objekts von `snake_case` in
/// `camelCase` um (nur oberste Ebene — Inhalte von JSONB-Spalten bleiben
/// unverändert). So heißen Felder in der API einheitlich, egal wie die
/// Spalte heißt.
pub fn camel(row: serde_json::Value) -> serde_json::Value {
    match row {
        serde_json::Value::Object(map) => {
            serde_json::Value::Object(map.into_iter().map(|(k, v)| (camel_key(&k), v)).collect())
        }
        other => other,
    }
}

fn camel_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// [`camel`] für alle Zeilen.
pub fn camel_rows(rows: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    rows.into_iter().map(camel).collect()
}

/// Einheitliche Listenantwort: `{ "count": n, "items": [...] }`.
pub fn list(rows: Vec<serde_json::Value>) -> Json<serde_json::Value> {
    let items = camel_rows(rows);
    Json(serde_json::json!({ "count": items.len(), "items": items }))
}

/// Einheitliche Einzelantwort; `None` wird zu 404.
pub fn one(
    row: Option<serde_json::Value>,
    not_found: &'static str,
) -> ApiResult<serde_json::Value> {
    row.map(|r| Json(camel(r)))
        .ok_or(ApiFailure::NotFound(not_found))
}

/// `id` einer mit [`json_sql`] gelesenen Zeile (für Audit und Ereignisse).
pub fn row_id(row: &serde_json::Value) -> Result<Uuid, ApiFailure> {
    row.get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| ApiFailure::internal("Zeile ohne id"))
}

/// Unterscheidet nach einem `UPDATE … WHERE id = … AND version = …` ohne
/// Treffer, ob der Datensatz fehlt (404) oder nur die Version veraltet
/// ist (409). `table` ist ein fester Tabellenname aus dem Code, nie eine
/// Eingabe.
pub async fn missing_or_stale(
    tx: &mut sqlx::PgConnection,
    table: &'static str,
    id: Uuid,
    not_found: &'static str,
) -> Result<ApiFailure, ApiFailure> {
    let exists: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM {table} WHERE id = $1)"
    ))
    .bind(id)
    .fetch_one(tx)
    .await?;
    Ok(if exists {
        ApiFailure::stale_version()
    } else {
        ApiFailure::NotFound(not_found)
    })
}

/// Begrenzt eine Seitengröße auf `1..=max` (Standard `default`).
pub fn page_limit(requested: Option<i64>, default: i64, max: i64) -> i64 {
    requested.unwrap_or(default).clamp(1, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses() {
        assert_eq!(
            ApiFailure::bad_request("x").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(ApiFailure::NotFound("x").status(), StatusCode::NOT_FOUND);
        assert_eq!(ApiFailure::stale_version().status(), StatusCode::CONFLICT);
        assert_eq!(
            ApiFailure::internal("geheim").status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            ApiFailure::from(sqlx::Error::RowNotFound).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn internal_details_are_not_exposed() {
        let res = ApiFailure::internal("SELECT geheim FROM users").into_response();
        let body = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("geheim"));
        assert!(text.contains("Interner Serverfehler"));
    }

    #[test]
    fn json_helpers() {
        assert_eq!(
            json_sql("SELECT 1 AS a"),
            "WITH q AS (SELECT 1 AS a) SELECT to_jsonb(q) FROM q"
        );
        let row =
            serde_json::json!({ "case_id": 1, "name_family": "x", "details": { "raw_key": 2 } });
        assert_eq!(
            camel(row),
            serde_json::json!({ "caseId": 1, "nameFamily": "x", "details": { "raw_key": 2 } })
        );
        assert_eq!(
            list(vec![serde_json::json!({ "a_b": 1 })]).0["items"][0]["aB"],
            1
        );
        assert!(one(None, "fehlt").is_err());
    }

    #[test]
    fn text_helpers() {
        assert_eq!(required_text("Name", "  a ").unwrap(), "a");
        assert!(required_text("Name", "   ").is_err());
        assert_eq!(optional_text(Some("  ".into())), None);
        assert_eq!(optional_text(Some(" x ".into())), Some("x".into()));
        assert_eq!(page_limit(None, 50, 200), 50);
        assert_eq!(page_limit(Some(0), 50, 200), 1);
        assert_eq!(page_limit(Some(9999), 50, 200), 200);
    }
}
