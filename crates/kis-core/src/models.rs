//! Gemeinsame Datenmodelle: UUIDs, Timestamps, Pagination.
//!
//! Jedes Modul definiert seine eigenen Modelle in seiner Crate.
//! Hier stehen nur die gemeinsamen Basis-Typen.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// UUID-Typ für alle Entitäten
pub type EntityId = Uuid;

/// Zeitstempel für created_at / updated_at
pub type Timestamp = DateTime<Utc>;

/// Soft-Delete Zeitstempel (None = aktiv, Some = gelöscht)
pub type DeletedAt = Option<DateTime<Utc>>;

/// Optimistic Locking: Versionsnummer
pub type Version = i32;

/// Cursor-basierte Pagination
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    /// Letzter empfangener ID (für "next page")
    #[serde(default)]
    pub after: Option<Uuid>,
    /// Maximale Ergebnisse pro Seite (Default: 50, Max: 200)
    #[serde(default = "default_page_size")]
    pub limit: i64,
}

fn default_page_size() -> i64 {
    50
}

impl PaginationParams {
    pub fn new() -> Self {
        Self {
            after: None,
            limit: default_page_size(),
        }
    }

    pub fn with_cursor(cursor: Uuid) -> Self {
        Self {
            after: Some(cursor),
            limit: default_page_size(),
        }
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = limit.min(200).max(1);
        self
    }
}

/// Standard-Antwort-Format für alle API-Endpunkte
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub data: T,
    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub struct ResponseMeta {
    pub total: i64,
    pub has_more: bool,
    pub next_cursor: Option<Uuid>,
}

/// Fehlerantwort (OWASP-konform: keine internen Details)
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    pub status: u16,
}

impl ApiError {
    pub fn new(status: u16, error: &str) -> Self {
        Self {
            error: error.to_string(),
            status,
        }
    }

    pub fn bad_request(msg: &str) -> Self {
        Self::new(400, msg)
    }

    pub fn unauthorized(msg: &str) -> Self {
        Self::new(401, msg)
    }

    pub fn forbidden(msg: &str) -> Self {
        Self::new(403, msg)
    }

    pub fn not_found(msg: &str) -> Self {
        Self::new(404, msg)
    }

    pub fn conflict(msg: &str) -> Self {
        Self::new(409, msg)
    }

    pub fn too_many_requests(msg: &str) -> Self {
        Self::new(429, msg)
    }

    pub fn internal(msg: &str) -> Self {
        Self::new(500, msg)
    }
}

/// Basisstruktur für alle Entitäten
pub trait Entity {
    fn id(&self) -> EntityId;
    fn version(&self) -> Version;
}
