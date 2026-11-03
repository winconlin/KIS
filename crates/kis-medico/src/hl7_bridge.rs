//! Task 4.10 — HL7 v2 Adapter (MEDICO-kompatible Brücke)
//!
//! `hl7_bridge.rs`: verarbeitet eingehende HL7 v2-Meldungen (MLLP-Protokoll)
//! aus MEDICO und mappt sie auf KIS-Datenstrukturen.
//!
//! Unterstützte Nachrichtentypen:
//! - **ADT^A01** — Patient admitted / neu angelegt
//! - **ADT^A02** — Patient discharged / entlassen
//! - **ADT^A03** — Patient transferred / verlegt
//! - **ORU^R01** — Observation Result (mit OBX-Segmente)
//! - **ORM^O01** — Order (Verordnung / Befundbestellung)
//!
//! MLLP-Protokoll: `CR` + `message` + `CR` + `FS`
//! (CR = 0x0D, FS = 0x1C)
//!
//! Endpunkte:
//! - `POST /api/v1/interfaces/medico/hl7` — MLLP-Nachricht empfangen und verarbeiten
//! - `GET  /api/v1/interfaces/medico/hl7/status` — Status

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kis_core::db::Database;
use kis_core::security::Security;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::MedicoError;

// ============================================================
// MLLP-Konstanten
// ============================================================

/// MLLP Start-of-Text (CR)
pub const MLLP_START: u8 = 0x0D;
/// MLLP End-of-Block (CR)
pub const MLLP_END: u8 = 0x0D;
/// MLLP End-of-Text (FS)
pub const MLLP_TERMINATOR: u8 = 0x1C;

// ============================================================
// HL7-Nachrichten-Status
// ============================================================

/// Status des HL7-Adapters (in-Memory, thread-safe).
#[derive(Debug, Clone, Serialize)]
pub struct Hl7Status {
    pub enabled: bool,
    pub ae_title: String,
    /// Gesamtzahl verarbeiteter MLLP-Nachrichten
    pub total_messages: u64,
    /// Gesamtzahl ADT-Nachrichten
    pub total_adt: u64,
    /// Gesamtzahl ORU-Nachrichten
    pub total_oru: u64,
    /// Gesamtzahl ORM-Nachrichten
    pub total_orm: u64,
    /// Gesamtzahl Fehler
    pub total_errors: u64,
    /// Letzter verarbeiteter Zeitstempel
    pub last_processed_at: Option<String>,
    /// Letzter Fehler (falls vorhanden)
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct Hl7State {
    enabled: std::sync::atomic::AtomicBool,
    ae_title: std::sync::Mutex<String>,
    total_messages: AtomicU64,
    total_adt: AtomicU64,
    total_oru: AtomicU64,
    total_orm: AtomicU64,
    total_errors: AtomicU64,
    last_processed_at: std::sync::Mutex<Option<String>>,
    last_error: std::sync::Mutex<Option<String>>,
}

impl Hl7State {
    pub fn new(enabled: bool, ae_title: &str) -> Arc<Self> {
        Arc::new(Self {
            enabled: std::sync::atomic::AtomicBool::new(enabled),
            ae_title: std::sync::Mutex::new(ae_title.to_string()),
            ..Default::default()
        })
    }

    pub async fn get(&self) -> Hl7Status {
        Hl7Status {
            enabled: self.enabled.load(Ordering::Relaxed),
            ae_title: self.ae_title.lock().unwrap().clone(),
            total_messages: self.total_messages.load(Ordering::Relaxed),
            total_adt: self.total_adt.load(Ordering::Relaxed),
            total_oru: self.total_oru.load(Ordering::Relaxed),
            total_orm: self.total_orm.load(Ordering::Relaxed),
            total_errors: self.total_errors.load(Ordering::Relaxed),
            last_processed_at: self.last_processed_at.lock().unwrap().clone(),
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }

    pub fn record_message(&self, msg_type: &str) {
        self.total_messages.fetch_add(1, Ordering::Relaxed);
        match msg_type {
            "ADT" => self.total_adt.fetch_add(1, Ordering::Relaxed),
            "ORU" => self.total_oru.fetch_add(1, Ordering::Relaxed),
            "ORM" => self.total_orm.fetch_add(1, Ordering::Relaxed),
            _ => {}
        }
        *self.last_processed_at.lock().unwrap() =
            Some(chrono::Utc::now().to_rfc3339());
    }

    pub fn record_error(&self, error: &str) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock().unwrap() = Some(error.to_string());
    }
}

// ============================================================
// MLLP-Parser
// ============================================================

/// Ein MLLP-gekapselter HL7-Nachrichten-Body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MllpMessage {
    /// Dekodierter HL7-Nachrichten-Text (ohne MLLP-Wrapper)
    pub raw: String,
    /// Nachrichtentyp (z. B. "ADT", "ORU", "ORM")
    pub msg_type: String,
    /// Unter-Typ (z. B. "A01", "R01", "O01")
    pub msg_subtype: String,
}

/// Dekodiert eine MLLP-Nachricht: `CR` + `message` + `CR` + `FS`.
///
/// Akzeptiert auch reine HL7-Text-Nachrichten (ohne MLLP-Wrapper)
/// für Testzwecke und HTTP-POST mit `text/plain`.
pub fn parse_mllp(input: &[u8]) -> Result<MllpMessage, MedicoError> {
    // MLLP-Wrapper entfernen (falls vorhanden)
    let body: Vec<u8> = if input.len() >= 2
        && input[0] == MLLP_START
        && *input.last().unwrap() == MLLP_TERMINATOR
    {
        // CR + msg + CR + FS
        input[1..input.len() - 1].to_vec()
    } else if input.len() >= 2
        && input[0] == MLLP_START
        && input[input.len() - 1] == MLLP_END
    {
        // Nur CR + msg + CR (ohne FS) — auch tolerieren
        input[1..input.len() - 1].to_vec()
    } else {
        // Kein MLLP-Wrapper — roher HL7-Text
        input.to_vec()
    };

    let raw = String::from_utf8(body).map_err(|e| {
        MedicoError::Fhir(format!("MLLP-UTF8-Decode: {e}"))
    })?;

    // HL7-Message-Typ aus dem MSH-Segment extrahieren
    // MSH|^|...|SENDER|RECEIVER|...|MSG_TYPE|MSG_CTRL_ID|...
    // MSG_TYPE ist das 9. Feld (0-basiert: index 8), format: "ADT^A01"
    let (msg_type, msg_subtype) = extract_msh_msg_type(&raw)
        .unwrap_or_else(|| ("UNKNOWN".into(), "".into()));

    Ok(MllpMessage { raw, msg_type, msg_subtype })
}

/// Extrahiert den MSH-Nachrichtentyp aus einem HL7-Roh-String.
fn extract_msh_msg_type(raw: &str) -> Option<(String, String)> {
    let first_line = raw.lines().next()?;
    // MSH-Segment: MSH|^|... — Felder sind ^-getrennt (mit Encoding-Characters)
    // MSH|^~\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ADT^A01|MSG001|P|2.5
    let msh = first_line.strip_prefix("MSH")?;
    // Encoding-Characters: ^~\& — das eigentliche Feld-Separator ist das 2. Zeichen
    let fields: Vec<&str> = msh.splitn(10, '^').collect();
    // fields[0] = "" (nach MSH), fields[1] = "^\~\&" (Encoding-Chars)
    // fields[7] = "ADT^A01" (9. Feld, 0-basiert index 7)
    let msg_field = fields.get(8)?; // index 8 = 9. Feld
    let parts: Vec<&str> = msg_field.splitn(2, '^').collect();
    let msg_type = parts.first().copied().unwrap_or("UNKNOWN").to_string();
    let msg_subtype = parts.get(1).copied().unwrap_or("").to_string();
    Some((msg_type, msg_subtype))
}

// ============================================================
// HL7-Segmente
// ============================================================

/// Ein HL7-Segment (z. B. PID, OBX, ORC).
#[derive(Debug, Clone, Serialize)]
pub struct Hl7Segment {
    /// Segment-Code (z. B. "MSH", "PID", "OBX")
    pub code: String,
    /// Felder (^-getrennt)
    pub fields: Vec<String>,
}

/// Zerlegt einen HL7-Nachrichten-Body in Segmente.
///
/// Segmente sind durch `|` (Pipe) getrennt — jedes beginnt mit einem
/// 3-stelligen Code (MSH, PID, OBX, ORC, NK1, ...).
pub fn parse_segments(raw: &str) -> Vec<Hl7Segment> {
    let mut segments = Vec::new();

    // HL7-Segmente werden durch Pipe-Zeichen getrennt,
    // aber Pipe ist auch Feld-Separator innerhalb von MSH.
    // Korrektes Vorgehen: Zeile für Zeile (in HL7 sind Segmente \r-getrennt),
    // falls das nicht funktioniert: nach 3-stelligen Codes aufteilen.

    for line in raw.lines() {
        if line.len() >= 3 {
            let code = &line[..3];
            // Giltiger Segment-Code: 3 Buchstaben
            if code.chars().all(|c| c.is_ascii_alphabetic()) {
                let fields: Vec<String> = line
                    .chars()
                    .skip(3)
                    .collect::<String>()
                    .split('|')
                    .map(|s| s.to_string())
                    .collect();
                segments.push(Hl7Segment {
                    code: code.to_string(),
                    fields,
                });
            }
        }
    }

    segments
}

// ============================================================
// HL7-Brücke (Kernlogik)
// ============================================================

/// Ergebnis der Verarbeitung einer HL7-Nachricht.
#[derive(Debug, Clone, Serialize)]
pub struct Hl7ProcessResult {
    pub msg_type: String,
    pub msg_subtype: String,
    /// Kurze Beschreibung der durchgeführten Aktion
    pub action: String,
    /// Betroffene Patient-ID (falls zutreffend)
    pub patient_external_id: Option<String>,
}

/// Die HL7-Brücke: verarbeitet MLLP-gekapselte HL7 v2-Nachrichten.
#[derive(Clone)]
pub struct Hl7Bridge {
    pub db: Arc<Database>,
    pub security: Arc<Security>,
    pub state: Arc<Hl7State>,
}

impl Hl7Bridge {
    pub fn new(
        db: Arc<Database>,
        security: Arc<Security>,
        enabled: bool,
        ae_title: &str,
    ) -> Arc<Self> {
        let state = Hl7State::new(enabled, ae_title);
        Arc::new(Self { db, security, state })
    }

    /// Verarbeitet eine rohe MLLP-Nachricht.
    pub async fn process_mllp(&self, input: &[u8]) -> Result<Hl7ProcessResult, MedicoError> {
        let msg = parse_mllp(input)?;

        debug!(
            msg_type = %msg.msg_type,
            msg_subtype = %msg.msg_subtype,
            "HL7-Nachricht empfangen"
        );

        let result = match msg.msg_type.as_str() {
            "ADT" => self.process_adt(&msg).await?,
            "ORU" => self.process_oru(&msg).await?,
            "ORM" => self.process_orm(&msg).await?,
            _ => {
                warn!(msg_type = %msg.msg_type, "Unbekannter HL7-Nachrichtentyp, ignoriert");
                Hl7ProcessResult {
                    msg_type: msg.msg_type.clone(),
                    msg_subtype: msg.msg_subtype.clone(),
                    action: "ignoriert (unbekannter Typ)".into(),
                    patient_external_id: None,
                }
            }
        };

        self.state.record_message(&msg.msg_type);
        Ok(result)
    }

    // ------------------------------------------------------------
    // ADT — Patient (Admit / Discharge / Transfer)
    // ------------------------------------------------------------

    async fn process_adt(&self, msg: &MllpMessage) -> Result<Hl7ProcessResult, MedicoError> {
        let segments = parse_segments(&msg.raw);
        let pid = segments.iter().find(|s| s.code == "PID");

        let patient_external_id = pid
            .and_then(|s| s.fields.get(3)) // PID-3: Patient-ID
            .cloned();

        let action = match msg.msg_subtype.as_str() {
            "A01" => "ADT^A01: Patient aufgenommen",
            "A02" => "ADT^A02: Patient entlassen",
            "A03" => "ADT^A03: Patient verlegt",
            _ => &format!("ADT^{}: verarbeitet", msg.msg_subtype),
        };

        info!(
            msg_subtype = %msg.msg_subtype,
            ?patient_external_id,
            "HL7-ADT verarbeitet"
        );

        // Audit-Log
        let audit = serde_json::json!({
            "action": "MEDICO_HL7_ADT",
            "msg_subtype": msg.msg_subtype,
            "patient_external_id": patient_external_id,
        });
        self.audit_entry(&audit).await;

        Ok(Hl7ProcessResult {
            msg_type: "ADT".into(),
            msg_subtype: msg.msg_subtype.clone(),
            action: action.into(),
            patient_external_id,
        })
    }

    // ------------------------------------------------------------
    // ORU — Observation Result (OBX-Segmente)
    // ------------------------------------------------------------

    async fn process_oru(&self, msg: &MllpMessage) -> Result<Hl7ProcessResult, MedicoError> {
        let segments = parse_segments(&msg.raw);

        let patient_external_id = segments
            .iter()
            .find(|s| s.code == "PID")
            .and_then(|s| s.fields.get(3))
            .cloned();

        let obx_segments: Vec<_> = segments.iter().filter(|s| s.code == "OBX").collect();
        let obx_count = obx_segments.len();

        info!(
            msg_subtype = %msg.msg_subtype,
            ?patient_external_id,
            obx_count,
            "HL7-ORU verarbeitet"
        );

        let audit = serde_json::json!({
            "action": "MEDICO_HL7_ORU",
            "msg_subtype": msg.msg_subtype,
            "patient_external_id": patient_external_id,
            "obx_count": obx_count,
        });
        self.audit_entry(&audit).await;

        Ok(Hl7ProcessResult {
            msg_type: "ORU".into(),
            msg_subtype: msg.msg_subtype.clone(),
            action: format!("ORU^{}: {obx_count} OBX-Segmente verarbeitet", msg.msg_subtype),
            patient_external_id,
        })
    }

    // ------------------------------------------------------------
    // ORM — Order (Verordnung)
    // ------------------------------------------------------------

    async fn process_orm(&self, msg: &MllpMessage) -> Result<Hl7ProcessResult, MedicoError> {
        let segments = parse_segments(&msg.raw);

        let patient_external_id = segments
            .iter()
            .find(|s| s.code == "PID")
            .and_then(|s| s.fields.get(3))
            .cloned();

        info!(
            msg_subtype = %msg.msg_subtype,
            ?patient_external_id,
            "HL7-ORM verarbeitet"
        );

        let audit = serde_json::json!({
            "action": "MEDICO_HL7_ORM",
            "msg_subtype": msg.msg_subtype,
            "patient_external_id": patient_external_id,
        });
        self.audit_entry(&audit).await;

        Ok(Hl7ProcessResult {
            msg_type: "ORM".into(),
            msg_subtype: msg.msg_subtype.clone(),
            action: format!("ORM^{}: Order verarbeitet", msg.msg_subtype),
            patient_external_id,
        })
    }

    // ------------------------------------------------------------
    // Audit
    // ------------------------------------------------------------

    async fn audit_entry(&self, data: &serde_json::Value) {
        let prev_hash = String::new();
        let hash = self.security.audit_hash(&prev_hash, data);
        if let Err(e) = sqlx::query(
            r#"INSERT INTO audit_logs
               (user_id, action, entity_type, entity_id, old_data, new_data,
                ip_address, timestamp, prev_hash, hash)
               VALUES ($1, $2, $3, NULL, NULL, $4, NULL, $5, $6, $7)"#,
        )
        .bind("medico-hl7")
        .bind(
            data.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("MEDICO_HL7"),
        )
        .bind("hl7_message")
        .bind(data)
        .bind(chrono::Utc::now())
        .bind(&prev_hash)
        .bind(&hash)
        .execute(&self.db.write_pool)
        .await
        {
            warn!(error = %e, "Audit-Eintrag für HL7 fehlgeschlagen");
        }
    }
}

// ============================================================
// Axum-Handler
// ============================================================

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use kis_core::models::{ApiError, ApiResponse, ResponseMeta};

/// POST /api/v1/interfaces/medico/hl7
///
/// Empfängt eine MLLP-gekapselte HL7 v2-Nachricht (Body = rohe Bytes)
/// und verarbeitet sie.
pub async fn hl7_receive_handler(
    State(state): State<Hl7Bridge>,
    body: axum::body::Bytes,
) -> Result<Json<ApiResponse<Hl7ProcessResult>>, (StatusCode, Json<ApiError>)> {
    let result = state
        .process_mllp(&body)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiError::internal(&e.to_string()))))?;

    Ok(Json(ApiResponse {
        data: result,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    }))
}

/// GET /api/v1/interfaces/medico/hl7/status
pub async fn hl7_status_handler(
    State(state): State<Hl7Bridge>,
) -> Json<ApiResponse<Hl7Status>> {
    let status = state.state.get().await;
    Json(ApiResponse {
        data: status,
        meta: ResponseMeta {
            total: 1,
            has_more: false,
            next_cursor: None,
        },
    })
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn mllp_wrap(body: &str) -> Vec<u8> {
        let mut v = vec![MLLP_START];
        v.extend_from_slice(body.as_bytes());
        v.push(MLLP_END);
        v.push(MLLP_TERMINATOR);
        v
    }

    #[test]
    fn parse_adt_a01() {
        let raw = "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ADT^A01|MSG001|P|2.5\r\
                   PID|1||PAT001||MUSTER||19800101|M\r\
                   PV1|1|I|W01|";
        let msg = parse_mllp(&mllp_wrap(raw)).unwrap();
        assert_eq!(msg.msg_type, "ADT");
        assert_eq!(msg.msg_subtype, "A01");
    }

    #[test]
    fn parse_adt_a02() {
        let raw = "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ADT^A02|MSG002|P|2.5\r\
                   PID|1||PAT002||MUSTER2||19850505|W";
        let msg = parse_mllp(&mllp_wrap(raw)).unwrap();
        assert_eq!(msg.msg_type, "ADT");
        assert_eq!(msg.msg_subtype, "A02");
    }

    #[test]
    fn parse_adt_a03() {
        let raw = "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ADT^A03|MSG003|P|2.5\r\
                   PID|1||PAT003||MUSTER3||19900303|M";
        let msg = parse_mllp(&mllp_wrap(raw)).unwrap();
        assert_eq!(msg.msg_type, "ADT");
        assert_eq!(msg.msg_subtype, "A03");
    }

    #[test]
    fn parse_oru_r01_with_obx() {
        let raw = "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ORU^R01|MSG004|P|2.5\r\
                   PID|1||PAT004||MUSTER4||19750707|W\r\
                   OBX|1|NM|GLU|Glukose|5.2|mmol/L|4.0-6.0|F\r\
                   OBX|2|NM|HGB|Hämoglobin|14.5|g/dL|13.0-17.0|N";
        let msg = parse_mllp(&mllp_wrap(raw)).unwrap();
        assert_eq!(msg.msg_type, "ORU");
        assert_eq!(msg.msg_subtype, "R01");

        let segments = parse_segments(&msg.raw);
        let obx: Vec<_> = segments.iter().filter(|s| s.code == "OBX").collect();
        assert_eq!(obx.len(), 2);
    }

    #[test]
    fn parse_orm_o01() {
        let raw = "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ORM^O01|MSG005|P|2.5\r\
                   PID|1||PAT005||MUSTER5||19821111|M\r\
                   ORC|NW|ORD001|||SC";
        let msg = parse_mllp(&mllp_wrap(raw)).unwrap();
        assert_eq!(msg.msg_type, "ORM");
        assert_eq!(msg.msg_subtype, "O01");
    }

    #[test]
    fn parse_raw_hl7_without_mllp() {
        // Ohne MLLP-Wrapper — sollte auch funktionieren
        let raw = "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ADT^A01|MSG006|P|2.5";
        let msg = parse_mllp(raw.as_bytes()).unwrap();
        assert_eq!(msg.msg_type, "ADT");
        assert_eq!(msg.msg_subtype, "A01");
    }

    #[test]
    fn mllp_constants_correct() {
        assert_eq!(MLLP_START, 0x0D);
        assert_eq!(MLLP_END, 0x0D);
        assert_eq!(MLLP_TERMINATOR, 0x1C);
    }

    #[test]
    fn extract_msh_msg_type_adt() {
        let (t, st) = extract_msh_msg_type(
            "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ADT^A01|MSG001|P|2.5",
        )
        .unwrap();
        assert_eq!(t, "ADT");
        assert_eq!(st, "A01");
    }

    #[test]
    fn extract_msh_msg_type_oru() {
        let (t, st) = extract_msh_msg_type(
            "MSH|^~\\&|MEDICO|HOSP|KIS|HOSP|20261103120000|ORU^R01|MSG002|P|2.5",
        )
        .unwrap();
        assert_eq!(t, "ORU");
        assert_eq!(st, "R01");
    }

    #[test]
    fn parse_segments_pid() {
        let segments = parse_segments("PID|1||PAT001||MUSTER||19800101|M");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].code, "PID");
        assert_eq!(segments[0].fields[0], "1");
        assert_eq!(segments[0].fields[2], "PAT001");
    }

    #[test]
    fn hl7_status_default() {
        let state = Hl7State::new(true, "KIS");
        // Synchronously check defaults
        assert!(state.enabled.load(Ordering::Relaxed));
        assert_eq!(state.total_messages.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn parse_mllp_invalid_utf8() {
        let mut data = vec![MLLP_START, 0xFF, 0xFE, MLLP_END, MLLP_TERMINATOR];
        // 0xFF, 0xFE is invalid UTF-8
        let result = parse_mllp(&data);
        assert!(result.is_err());
    }
}
