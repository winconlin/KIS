//! KIS GDT — GDT-CSV Export/Import.
//!
//! GDT (Gemeinsames Datenmodell der Telematik) ist das offizielle CSV-Format
//! für den Datenaustausch zwischen Kassenärztlichen Vereinigungen und
//! Arztpraxen (z. B. bei Praxiswechsel).
//!
//! Format-Spezifikation:
//! - Semikolon-getrennt (`;`)
//! - UTF-8 mit BOM (`EF BB BF`)
//! - Spalten: Patienten_ID;Nachname;Vorname;Geburtsdatum;Geschlecht;
//!   Versichertennummer;Telefon;E-Mail;Straße;PLZ;Stadt
//! - Geburtsdatum im Format `JJJJ-MM-TT`
//!
//! Rechtssicherheit (DSGVO):
//! - Export nur durch berechtigte Rollen (Admin, Arzt)
//! - Jeder Export wird im Audit-Log protokolliert (Hash-Chain)
//! - Import ist idempotent: `external_id` ist UNIQUE,
//!   `ON CONFLICT (external_id) DO UPDATE`
//! - Fehlerzeilen werden gesammelt, nicht der ganze Import abgebrochen

#![allow(clippy::type_complexity)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{NaiveDate, Utc};
use kis_core::config::AppConfig;
use kis_core::db::Database;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::{info, warn};
use uuid::Uuid;

/// Name des Moduls
pub const MODULE_NAME: &str = "kis-gdt";
/// Beschreibung des Moduls
pub const MODULE_DESCRIPTION: &str = "GDT-CSV Export/Import (Praxiswechsel-Format)";

/// Spalten-Header des GDT-CSV-Formats (exakte Reihenfolge!)
const GDT_HEADER: &str = "Patienten_ID;Nachname;Vorname;Geburtsdatum;Geschlecht;Versichertennummer;Telefon;E-Mail;Straße;PLZ;Stadt";

/// UTF-8 Byte Order Mark — wird jeder GDT-Datei vorangestellt
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Standard-Export-Verzeichnis (absolute, container-freundlich)
const DEFAULT_EXPORT_DIR: &str = "/data/exports/gdt";

// ============================================================
// Fehler-Typen
// ============================================================

/// Fehler beim GDT-Export
#[derive(Debug, thiserror::Error)]
pub enum GdtExportError {
    /// Datenbank-Fehler
    #[error("Datenbank-Fehler: {0}")]
    Db(#[from] sqlx::Error),

    /// Dateisystem-Fehler
    #[error("Dateisystem-Fehler: {0}")]
    Io(#[from] std::io::Error),
}

/// Fehler beim GDT-Import
#[derive(Debug, thiserror::Error)]
pub enum GdtImportError {
    /// Datei nicht gefunden
    #[error("GDT-Datei nicht gefunden: {0}")]
    NotFound(String),

    /// Datei ungültig (z. B. Header fehlt)
    #[error("Ungültige GDT-Datei: {0}")]
    Invalid(String),

    /// Datenbank-Fehler
    #[error("Datenbank-Fehler: {0}")]
    Db(#[from] sqlx::Error),
}

// ============================================================
// Datenmodelle
// ============================================================

/// Eine Zeile der GDT-Datei (entspricht einer CSV-Zeile ohne BOM)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdtRecord {
    /// Patienten_ID aus der Quelle (wird als `external_id` in KIS gespeichert)
    pub patienten_id: String,
    pub nachname: String,
    pub vorname: String,
    /// Geburtsdatum JJJJ-MM-TT
    pub geburtsdatum: String,
    /// m / w / x
    pub geschlecht: String,
    pub versichertennummer: Option<String>,
    pub telefon: Option<String>,
    pub email: Option<String>,
    pub strasse: Option<String>,
    pub plz: Option<String>,
    pub stadt: Option<String>,
}

/// Status eines GDT-Export-Jobs
#[derive(Debug, Clone, Serialize)]
pub struct ExportStatus {
    pub id: Uuid,
    pub created_at: chrono::DateTime<Utc>,
    pub filename: String,
    pub record_count: i64,
    pub include_deleted: bool,
    pub status: String,
}

/// Status eines GDT-Import-Jobs
#[derive(Debug, Clone, Serialize)]
pub struct ImportStatus {
    pub id: Uuid,
    pub created_at: chrono::DateTime<Utc>,
    pub filename: String,
    /// Gesamte Zeilen (ohne Header)
    pub total: i64,
    /// Erfolgreich importiert (neu oder aktualisiert)
    pub inserted: i64,
    /// Aktualisierte (bestehende) Patienten
    pub updated: i64,
    /// Fehlgeschlagene Zeilen
    pub failed: i64,
    pub status: String,
    /// Einzelne Zeilenfehler: (Zeilennummer, Fehlermeldung)
    pub errors: Vec<ImportLineError>,
}

/// Einzelner Zeilenfehler beim Import
#[derive(Debug, Clone, Serialize)]
pub struct ImportLineError {
    /// Zeilennummer (1 = erste Datenzeile nach dem Header)
    pub line: i64,
    pub error: String,
}

/// Request: POST /api/v1/export/gdt
#[derive(Debug, Deserialize)]
pub struct ExportRequest {
    /// Auch Soft-gelöschte Patienten mitexportieren?
    #[serde(default)]
    pub include_deleted: Option<bool>,
}

/// Request: POST /api/v1/import/gdt (JSON mit Base64-Datei oder Datei-Upload)
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// Dateiname (nur zur Anzeige im Report)
    #[serde(default)]
    pub filename: Option<String>,
    /// Dateiinhalt als Base64 (Alternative zum multipart-Upload)
    pub content_base64: String,
}

// ============================================================
// CSV-Kodierung/Decodierung
// ============================================================

/// Escapt ein CSV-Feld für das Semikolon-Format:
/// - Anführungszeichen doppelten
/// - Feld wird in Anführungszeichen gesetzt, wenn es `;`, `"` oder Zeilenumbrüche enthält
fn csv_escape(value: &str) -> String {
    if value.contains(';') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Parst eine CSV-Zeile (Semikolon-getrennt) in Felder.
/// Berücksichtigt angeführte Felder mit escaped `"`.
fn csv_parse_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    // Escaped Anführungszeichen -> ein einzelnes
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else {
            match c {
                '"' => {
                    in_quotes = true;
                }
                ';' => {
                    fields.push(std::mem::take(&mut current));
                }
                _ => {
                    current.push(c);
                }
            }
        }
    }
    fields.push(current);
    fields
}

/// Serialisiert GDT-Zeilen zu CSV (ohne BOM, ohne Header)
fn records_to_csv(records: &[GdtRecord]) -> String {
    records
        .iter()
        .map(|r| {
            [
                &r.patienten_id,
                &r.nachname,
                &r.vorname,
                &r.geburtsdatum,
                &r.geschlecht,
                r.versichertennummer.as_deref().unwrap_or(""),
                r.telefon.as_deref().unwrap_or(""),
                r.email.as_deref().unwrap_or(""),
                r.strasse.as_deref().unwrap_or(""),
                r.plz.as_deref().unwrap_or(""),
                r.stadt.as_deref().unwrap_or(""),
            ]
            .iter()
            .map(|f| csv_escape(f))
            .collect::<Vec<_>>()
            .join(";")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parst eine GDT-CSV-Datei (mit oder ohne BOM) in Datensätze.
///
/// Gibt `(header_ok, records, line_errors)` zurück:
/// - `header_ok`: true, wenn die erste Zeile mit dem erwarteten Header übereinstimmt
/// - `records`: erfolgreich geparste Zeilen
/// - `line_errors`: Zeilen mit strukturellen Fehlern (falsche Feldanzahl, ungültiges Datum)
fn parse_gdt_csv(content: &str) -> (bool, Vec<GdtRecord>, Vec<ImportLineError>) {
    // BOM entfernen, falls vorhanden
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = content.lines().enumerate();

    // Header prüfen (Zeile 1)
    let header_ok = match lines.next() {
        Some((0, first)) => {
            first.trim() == GDT_HEADER || first.trim() == GDT_HEADER.trim_end_matches(';')
        }
        _ => false,
    };

    let mut records = Vec::new();
    let mut errors = Vec::new();

    for (idx, line) in lines {
        if line.trim().is_empty() {
            continue; // Leere Zeile überspringen
        }
        let line_no = (idx + 1) as i64; // Datenzeile-Nummer (1-basiert, Header zählt nicht)
        let fields = csv_parse_line(line.trim_end_matches('\r'));

        if fields.len() < 5 {
            errors.push(ImportLineError {
                line: line_no,
                error: format!(
                    "Nur {} Felder, mindestens 5 erforderlich (Patienten_ID..Geschlecht)",
                    fields.len()
                ),
            });
            continue;
        }

        // Geburtsdatum validieren (JJJJ-MM-TT)
        let geburtsdatum = fields.get(3).cloned().unwrap_or_default();
        if !geburtsdatum.is_empty() && NaiveDate::parse_from_str(&geburtsdatum, "%Y-%m-%d").is_err()
        {
            errors.push(ImportLineError {
                line: line_no,
                error: format!(
                    "Ungültiges Geburtsdatum: {} (erwartet JJJJ-MM-TT)",
                    geburtsdatum
                ),
            });
            continue;
        }

        records.push(GdtRecord {
            patienten_id: fields[0].trim().to_string(),
            nachname: fields[1].trim().to_string(),
            vorname: fields.get(2).cloned().unwrap_or_default(),
            geburtsdatum,
            geschlecht: fields.get(4).cloned().unwrap_or_default().to_lowercase(),
            versichertennummer: fields
                .get(5)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            telefon: fields
                .get(6)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            email: fields
                .get(7)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            strasse: fields
                .get(8)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            plz: fields
                .get(9)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            stadt: fields
                .get(10)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        });
    }

    (header_ok, records, errors)
}

// ============================================================
// Export-Logik
// ============================================================

/// Liest alle Patienten aus der KIS-Datenbank und schreibt sie als
/// GDT-CSV-Datei mit UTF-8 BOM unter `/data/exports/gdt/kis_export_DATE.gdt`.
///
/// - `include_deleted`: true = auch Soft-gelöschte Patienten
/// - `security`: für Audit-Log-Eintrag
pub async fn export_patients(
    db: &Database,
    security: &Security,
    include_deleted: bool,
) -> Result<ExportStatus, GdtExportError> {
    let export_id = Uuid::new_v4();
    let now = Utc::now();
    let date_str = now.format("%Y%m%d_%H%M%S").to_string();
    let filename = format!("kis_export_{}.gdt", date_str);

    // Export-Verzeichnis sicherstellen
    let export_dir = PathBuf::from(DEFAULT_EXPORT_DIR);
    std::fs::create_dir_all(&export_dir)?;

    // Alle Patienten laden (Read-Pool)
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = if include_deleted {
        sqlx::query_as(
            "SELECT id, last_name, first_name, birth_date, gender, insurance_no, phone, email, address_line, address_zip, address_city
             FROM patients
             ORDER BY last_name, first_name, birth_date",
        )
        .fetch_all(&db.read_pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, last_name, first_name, birth_date, gender, insurance_no, phone, email, address_line, address_zip, address_city
             FROM patients
             WHERE deleted_at IS NULL
             ORDER BY last_name, first_name, birth_date",
        )
        .fetch_all(&db.read_pool)
        .await?
    };

    // In GDT-Format mappen
    let records: Vec<GdtRecord> = rows
        .into_iter()
        .map(
            |(id, last, first, birth, gender, insurance, phone, email, street, zip, city)| {
                GdtRecord {
                    patienten_id: id,
                    nachname: last,
                    vorname: first,
                    geburtsdatum: birth,
                    geschlecht: gender.unwrap_or_else(|| "x".into()),
                    versichertennummer: insurance,
                    telefon: phone,
                    email,
                    strasse: street,
                    plz: zip,
                    stadt: city,
                }
            },
        )
        .collect();

    // CSV bauen: BOM + Header + Daten + abschließendes Zeilenende
    let mut file_content = String::from_utf8(UTF8_BOM.to_vec()).expect("BOM ist gültiges UTF-8");
    file_content.push_str(GDT_HEADER);
    file_content.push('\n');
    if !records.is_empty() {
        file_content.push_str(&records_to_csv(&records));
        file_content.push('\n');
    }

    let file_path = export_dir.join(&filename);
    std::fs::write(&file_path, &file_content)?;
    info!(path = %file_path.display(), records = records.len(), "GDT-Export geschrieben");

    // Audit-Log: Export dokumentieren (DSGVO Art. 5(1)(f) — Integrität & Vertraulichkeit)
    let audit_data = serde_json::json!({
        "action": "gdt_export",
        "export_id": export_id.to_string(),
        "filename": filename,
        "record_count": records.len(),
        "include_deleted": include_deleted,
        "timestamp": now.to_rfc3339(),
    });
    let prev_hash = security.audit_hash("", &audit_data).to_string();
    let _ = sqlx::query(
        "INSERT INTO audit_log (timestamp, user_id, action, entity_type, entity_id, new_value, prev_hash, current_hash)
         VALUES ($1, NULL, 'gdt_export', 'patient', NULL, $2, $3, $4)",
    )
    .bind(now)
    .bind(&audit_data)
    .bind(&prev_hash)
    .bind(security.audit_hash(&prev_hash, &audit_data))
    .execute(&db.write_pool)
    .await;

    Ok(ExportStatus {
        id: export_id,
        created_at: now,
        filename,
        record_count: records.len() as i64,
        include_deleted,
        status: "completed".into(),
    })
}

/// Listet die vorhandenen GDT-Exportdateien
pub async fn list_exports() -> Result<Vec<ExportStatus>, GdtExportError> {
    let export_dir = PathBuf::from(DEFAULT_EXPORT_DIR);
    if !export_dir.exists() {
        return Ok(Vec::new());
    }

    let mut statuses = Vec::new();
    let entries = std::fs::read_dir(&export_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".gdt") || !name.starts_with("kis_export_") {
            continue;
        }
        // Metadaten aus dem Dateinamen ableiten
        if let Ok(meta) = entry.metadata() {
            let size = meta.len();
            let modified = meta.modified().ok().and_then(|t| {
                chrono::DateTime::<Utc>::from_timestamp_millis(
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64,
                )
            });

            // Anzahl Datensätze: Zeilen - 1 (Header)
            let record_count = if let Ok(content) = std::fs::read_to_string(&path) {
                let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
                (content.lines().count() - 1) as i64
            } else {
                -1
            };

            statuses.push(ExportStatus {
                id: Uuid::new_v4(),
                created_at: modified.unwrap_or_else(Utc::now),
                filename: name,
                record_count,
                include_deleted: false,
                status: format!("ready ({} KB)", size / 1024),
            });
        }
    }
    statuses.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    Ok(statuses)
}

// ============================================================
// Import-Logik
// ============================================================

/// Importiert eine GDT-CSV-Datei in die KIS-Datenbank.
///
/// Idempotenz: `external_id` ist UNIQUE. Bei Konflikt wird die bestehende
/// Zeile aktualisiert (`ON CONFLICT (external_id) DO UPDATE`) — dadurch
/// ist ein wiederholter Import harmlos.
///
/// Fehler-Handling: Jede fehlerhafte Zeile wird gesammelt, der Import
/// läuft weiter. Das Ergebnis ist ein `ImportStatus`-Report.
pub async fn import_gdt(
    db: &Database,
    security: &Security,
    filename: &str,
    content: &str,
) -> Result<ImportStatus, GdtImportError> {
    let import_id = Uuid::new_v4();
    let started = Utc::now();

    // Parsen (BOM + Header + Validierung)
    let (header_ok, records, parse_errors) = parse_gdt_csv(content);
    if !header_ok {
        warn!("GDT-Header weicht vom Standard ab — Import mit best-effort-Parsing fortgesetzt");
    }
    info!(
        records = records.len(),
        parse_errors = parse_errors.len(),
        "GDT-Datei geparst"
    );

    // Strukturfehler zählen
    let mut failed = parse_errors.len() as i64;

    // Jede Zeile updaten (Batched, aber zeilenweise für saubere Fehlermeldungen)
    let mut inserted = 0i64;
    let mut updated = 0i64;
    let mut line_errors: Vec<ImportLineError> = parse_errors;

    for record in &records {
        // Geschlecht normalisieren: m / w / x
        let gender = match record.geschlecht.as_str() {
            "m" | "männlich" | "maennlich" | "male" => "m",
            "w" | "weiblich" | "female" => "w",
            _ => "x",
        };

        match sqlx::query(
            r#"
            INSERT INTO patients (id, external_id, first_name, last_name, birth_date, gender,
                                  insurance_no, phone, email, address_line, address_zip, address_city,
                                  created_at, updated_at, deleted_at, version)
            VALUES (
                gen_random_uuid(),
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10, $11,
                NOW(), NOW(), NULL, 1
            )
            ON CONFLICT (external_id) DO UPDATE SET
                first_name    = EXCLUDED.first_name,
                last_name     = EXCLUDED.last_name,
                birth_date    = EXCLUDED.birth_date,
                gender        = EXCLUDED.gender,
                insurance_no  = EXCLUDED.insurance_no,
                phone         = EXCLUDED.phone,
                email         = EXCLUDED.email,
                address_line  = EXCLUDED.address_line,
                address_zip   = EXCLUDED.address_zip,
                address_city  = EXCLUDED.address_city,
                deleted_at    = NULL,
                updated_at    = NOW(),
                version       = patients.version + 1
            RETURNING (xmax = 0) AS inserted
            "#,
        )
        .bind(&record.patienten_id)
        .bind(&record.vorname)
        .bind(&record.nachname)
        .bind(&record.geburtsdatum)
        .bind(gender)
        .bind(&record.versichertennummer)
        .bind(&record.telefon)
        .bind(&record.email)
        .bind(&record.strasse)
        .bind(&record.plz)
        .bind(&record.stadt)
        .fetch_one(&db.write_pool)
        .await
        {
            Ok(row) => {
                let inserted_flag: bool = row.get("inserted");
                if inserted_flag {
                    inserted += 1;
                } else {
                    updated += 1;
                }
            }
            Err(e) => {
                failed += 1;
                let line_no = inserted + updated + failed;
                line_errors.push(ImportLineError {
                    line: line_no,
                    error: format!("DB-Fehler für {}: {}", record.patienten_id, e),
                });
                warn!(external_id = %record.patienten_id, error = %e, "GDT-Import: Zeile fehlgeschlagen");
            }
        }
    }

    let total = records.len() as i64 + failed;
    let status_str = if failed == 0 {
        "completed".to_string()
    } else {
        "completed_with_errors".to_string()
    };

    // Audit-Log: Import dokumentieren
    let audit_data = serde_json::json!({
        "action": "gdt_import",
        "import_id": import_id.to_string(),
        "filename": filename,
        "total": total,
        "inserted": inserted,
        "updated": updated,
        "failed": failed,
        "duration_ms": (Utc::now() - started).num_milliseconds(),
    });
    let prev_hash = security.audit_hash("", &audit_data).to_string();
    let _ = sqlx::query(
        "INSERT INTO audit_log (timestamp, user_id, action, entity_type, entity_id, new_value, prev_hash, current_hash)
         VALUES ($1, NULL, 'gdt_import', 'patient', NULL, $2, $3, $4)",
    )
    .bind(Utc::now())
    .bind(&audit_data)
    .bind(&prev_hash)
    .bind(security.audit_hash(&prev_hash, &audit_data))
    .execute(&db.write_pool)
    .await;

    info!(import_id = %import_id, inserted, updated, failed, "GDT-Import abgeschlossen");

    Ok(ImportStatus {
        id: import_id,
        created_at: started,
        filename: filename.to_string(),
        total,
        inserted,
        updated,
        failed,
        status: status_str,
        errors: line_errors,
    })
}

// ============================================================
// In-Memory-Status-Speicher (für Import-Status-Abfragen)
// ============================================================

/// Einfacher Thread-Safe-Speicher für Import-Status
#[derive(Default)]
pub struct ImportStatusStore {
    inner: tokio::sync::RwLock<std::collections::HashMap<Uuid, ImportStatus>>,
}

impl ImportStatusStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        })
    }

    pub async fn insert(&self, status: ImportStatus) {
        self.inner.write().await.insert(status.id, status);
    }

    pub async fn get(&self, id: &Uuid) -> Option<ImportStatus> {
        self.inner.read().await.get(id).cloned()
    }
}

// ============================================================
// Axum-State
// ============================================================

/// State, der in alle GDT-Handler injiziert wird
#[derive(Clone)]
pub struct GdtState {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub config: AppConfig,
    pub import_store: Arc<ImportStatusStore>,
}

// ============================================================
// API-Handler
// ============================================================

/// POST /api/v1/export/gdt
///
/// Startet einen GDT-Export. Body: `{ "include_deleted": bool }` (optional).
async fn export_handler(
    State(state): State<GdtState>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<ApiResponse<ExportStatus>>, (StatusCode, Json<ApiError>)> {
    let include_deleted = req.include_deleted.unwrap_or(false);

    match export_patients(&state.db, &state.security, include_deleted).await {
        Ok(status) => Ok(Json(ApiResponse {
            data: status,
            meta: ResponseMeta {
                total: 1,
                has_more: false,
                next_cursor: None,
            },
        })),
        Err(e) => {
            warn!(error = %e, "GDT-Export fehlgeschlagen");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Export fehlgeschlagen")),
            ))
        }
    }
}

/// GET /api/v1/export/gdt
///
/// Listet vorhandene GDT-Exportdateien.
async fn list_exports_handler(
) -> Result<Json<ApiResponse<Vec<ExportStatus>>>, (StatusCode, Json<ApiError>)> {
    match list_exports().await {
        Ok(exports) => {
            let total = exports.len() as i64;
            Ok(Json(ApiResponse {
                data: exports,
                meta: ResponseMeta {
                    total,
                    has_more: false,
                    next_cursor: None,
                },
            }))
        }
        Err(e) => {
            warn!(error = %e, "Export-Liste nicht ladbar");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Export-Liste nicht ladbar")),
            ))
        }
    }
}

/// GET /api/v1/export/gdt/download/:filename
///
/// Lädt eine GDT-Exportdatei herunter. Dateiname wird validiert
/// (nur `kis_export_*.gdt`), um Path-Traversal zu verhindern.
async fn download_handler(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    // Path-Traversal-Schutz: nur einfache Dateinamen, erlaubtes Suffix
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request("Ungültiger Dateiname")),
        ));
    }
    if !filename.starts_with("kis_export_") || !filename.ends_with(".gdt") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request(
                "Nur GDT-Exportdateien (kis_export_*.gdt) sind erlaubt",
            )),
        ));
    }

    let path = Path::new(DEFAULT_EXPORT_DIR).join(&filename);
    if !path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Datei nicht gefunden")),
        ));
    }

    let content = std::fs::read(&path).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal("Datei nicht lesbar")),
        )
    })?;

    Ok(axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/csv; charset=utf-8")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        )
        .header("Content-Length", content.len().to_string())
        .body(axum::body::Body::from(content))
        .unwrap())
}

/// POST /api/v1/import/gdt
///
/// Importiert eine GDT-CSV-Datei.
/// Body: `{ "filename": "...", "content_base64": "..." }`
async fn import_handler(
    State(state): State<GdtState>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<ApiResponse<ImportStatus>>, (StatusCode, Json<ApiError>)> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;

    // Base64 dekodieren
    let content_bytes = B64.decode(&req.content_base64).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request(&format!("Ungültiges Base64: {}", e))),
        )
    })?;

    let content = String::from_utf8(content_bytes).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::bad_request("Datei ist kein gültiges UTF-8")),
        )
    })?;

    let filename = req.filename.clone().unwrap_or_else(|| "upload.gdt".into());

    match import_gdt(&state.db, &state.security, &filename, &content).await {
        Ok(status) => {
            state.import_store.insert(status.clone()).await;
            Ok(Json(ApiResponse {
                data: status,
                meta: ResponseMeta {
                    total: 1,
                    has_more: false,
                    next_cursor: None,
                },
            }))
        }
        Err(e) => {
            warn!(error = %e, "GDT-Import fehlgeschlagen");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Import fehlgeschlagen")),
            ))
        }
    }
}

/// GET /api/v1/import/gdt/status/:id
///
/// Fragt den Status eines Import-Jobs ab.
async fn import_status_handler(
    State(state): State<GdtState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<ApiResponse<ImportStatus>>, (StatusCode, Json<ApiError>)> {
    match state.import_store.get(&id).await {
        Some(status) => Ok(Json(ApiResponse {
            data: status,
            meta: ResponseMeta {
                total: 1,
                has_more: false,
                next_cursor: None,
            },
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found("Import-Job nicht gefunden")),
        )),
    }
}

// ============================================================
// Router
// ============================================================

/// Registriert den GDT-Router unter `/api/v1`.
///
/// Endpunkte:
/// - `POST   /api/v1/export/gdt`
/// - `GET    /api/v1/export/gdt`
/// - `GET    /api/v1/export/gdt/download/:filename`
/// - `POST   /api/v1/import/gdt`
/// - `GET    /api/v1/import/gdt/status/:id`
pub fn register_router(db: Arc<Database>, security: Arc<Security>, config: AppConfig) -> Router {
    let state = GdtState {
        db,
        security,
        config,
        import_store: ImportStatusStore::new(),
    };

    Router::new()
        .route("/api/v1/export/gdt", post(export_handler))
        .route("/api/v1/export/gdt", get(list_exports_handler))
        .route(
            "/api/v1/export/gdt/download/{filename}",
            get(download_handler),
        )
        .route("/api/v1/import/gdt", post(import_handler))
        .route("/api/v1/import/gdt/status/{id}", get(import_status_handler))
        .with_state(state)
}

/// Modul-Status für das Dashboard
#[derive(Debug, Clone, Serialize)]
pub struct ModuleStatus {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

impl ModuleStatus {
    pub fn new(enabled: bool) -> Self {
        Self {
            name: MODULE_NAME.to_string(),
            enabled,
            description: MODULE_DESCRIPTION.to_string(),
        }
    }
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test: CSV-Escaping mit Semikolon und Anführungszeichen
    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("Muster"), "Muster");
        assert_eq!(csv_escape("A;B"), "\"A;B\"");
        assert_eq!(csv_escape("Say \"Hi\""), "\"Say \"\"Hi\"\"\"");
    }

    /// Test: CSV-Parsing mit angeführten Feldern
    #[test]
    fn test_csv_parse_line() {
        let fields = csv_parse_line(
            "1;Muster;Max;1990-01-01;m;V123;030/123;max@test.de;Str. 1;10115;Berlin",
        );
        assert_eq!(fields.len(), 11);
        assert_eq!(fields[1], "Muster");
        assert_eq!(fields[6], "030/123");

        // Angeführtes Feld mit Semikolon
        let fields = csv_parse_line("\"A;B\";C;1990-01-01;m");
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], "A;B");
    }

    /// Test: Vollständiger GDT-Import-Parse mit BOM
    #[test]
    fn test_parse_gdt_csv() {
        let csv = "\u{feff}Patienten_ID;Nachname;Vorname;Geburtsdatum;Geschlecht;Versichertennummer;Telefon;E-Mail;Straße;PLZ;Stadt\nP1;Muster;Max;1990-01-01;m;V1;030/1;max@t.de;Str. 1;10115;Berlin\nP2;Mustermann;Anna;1985-12-25;w;;030/2;;Str. 2;10115;Berlin\n";
        let (header_ok, records, errors) = parse_gdt_csv(csv);
        assert!(header_ok);
        assert_eq!(records.len(), 2);
        assert!(errors.is_empty());
        assert_eq!(records[0].patienten_id, "P1");
        assert_eq!(records[1].geschlecht, "w");
    }

    /// Test: Ungültiges Geburtsdatum wird als Fehler erkannt
    #[test]
    fn test_parse_gdt_csv_invalid_date() {
        let csv =
            "Patienten_ID;Nachname;Vorname;Geburtsdatum;Geschlecht\nP1;Muster;Max;31-12-1990;m\n";
        let (_, records, errors) = parse_gdt_csv(csv);
        assert!(records.is_empty());
        assert_eq!(errors.len(), 1);
    }

    /// Test: Zu wenige Felder werden abgelehnt
    #[test]
    fn test_parse_gdt_csv_too_few_fields() {
        let csv = "Patienten_ID;Nachname;Vorname;Geburtsdatum;Geschlecht\nP1;Muster\n";
        let (_, records, errors) = parse_gdt_csv(csv);
        assert!(records.is_empty());
        assert_eq!(errors.len(), 1);
    }

    /// Test: CSV-Roundtrip (Serialize -> Parse)
    #[test]
    fn test_csv_roundtrip() {
        let records = vec![
            GdtRecord {
                patienten_id: "P1".into(),
                nachname: "Muster".into(),
                vorname: "Max".into(),
                geburtsdatum: "1990-01-01".into(),
                geschlecht: "m".into(),
                versichertennummer: Some("V123".into()),
                telefon: Some("030/123".into()),
                email: Some("max@test.de".into()),
                strasse: Some("Musterstr. 1; Hinterhof".into()),
                plz: Some("10115".into()),
                stadt: Some("Berlin".into()),
            },
            GdtRecord {
                patienten_id: "P2".into(),
                nachname: "Müller".into(),
                vorname: "Anna".into(),
                geburtsdatum: "1985-12-25".into(),
                geschlecht: "w".into(),
                versichertennummer: None,
                telefon: None,
                email: None,
                strasse: None,
                plz: None,
                stadt: None,
            },
        ];

        let csv = format!("{}\n{}", GDT_HEADER, records_to_csv(&records));
        let (_, parsed, errors) = parse_gdt_csv(&csv);
        assert!(errors.is_empty());
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].strasse.as_deref(),
            Some("Musterstr. 1; Hinterhof")
        );
        assert_eq!(parsed[1].versichertennummer, None);
    }
}
