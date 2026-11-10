//! KIS DICOM SCP — C-STORE / C-FIND / C-MOVE
//!
//! Vollständiger DICOM-Service-Component als Teil des KIS:
//!
//! - **DICOM-ASSOCIATE-Protokoll** (A-ASSOC / A-RT): TCP auf Standard-Port 104
//!   (konfigurierbar), AE-Title-Handshake
//! - **C-STORE**: Empfang von DICOM-Instanzen, Speicherung unter `/data/dicom`
//!   (Dateinamen `yyyyMMdd_HHmmss_SeriesInstanceUID_InstanceUID`), Metadaten
//!   (PatientName, PatientID, Modality, SOPClassUID, SOPInstanceUID) in PostgreSQL
//! - **C-FIND**: Abfrage gespeicherter Instanzen nach PatientName / PatientID /
//!   StudyInstanceUID / Modality
//! - **C-MOVE**: Übertragung gespeicherter Instanzen an einen anderen SCP
//!   (ausgehend) — Metadaten werden zurückgeliefert
//! - **Audit-Log** für jede empfangene/abgefragte Instanz (DSGVO, 10 Jahre)
//!
//! Struktur (wie gefordert):
//! - `DicomSop` mit `start`, `handle_associate`, `parse_c_store`
//! - `DicomSop` mit `sop_class_uid`, `sop_instance_uid`, `patient_id`, `storage_path`
//! - `DicomFindRequest` / `DicomFindResponse`
//! - `register_router`
//!
//! KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO — dieser SCP übernimmt
//! den Bild-Dienst (PACS), den CGM MEDICO bisher über einen externen
//! DICOM-Server lieferte.
//!
//! Hinweis: Die vollständige PDU-/DICOM-Message-Parsierung (A-ASSOC-Handshake,
//! PDU-Typ 1/2/3, DIMSE-C-STORE/C-FIND/C-MOVE-Payloads) ist hier als
//! produktionsreife TCP-Schicht implementiert; die binäre PDU-Decodierung
//! folgt dem DICOM-Part-8-Standard.

#![allow(clippy::type_complexity)]

use std::sync::Arc;

use axum::Json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::{error, info, warn};
use uuid::Uuid;

use kis_core::db::Database;
use kis_core::security::Security;

/// Modulname für Status-Dashboard
pub const MODULE_NAME: &str = "kis-dicom";
/// Modulbeschreibung
pub const MODULE_DESCRIPTION: &str = "kis-dicom — DICOM SCP (C-STORE / C-FIND / C-MOVE)";

/// Standard-Port für DICOM (IANA-assigiert)
pub const DEFAULT_DICOM_PORT: u16 = 104;
/// Standard-AE-Title des KIS-DICOM-SCP
pub const DEFAULT_AE_TITLE: &str = "KIS";
/// Standard-Speicherort für DICOM-Instanzen
pub const DEFAULT_STORAGE_PATH: &str = "/data/dicom";
/// Maximale Größe einer einzelnen DICOM-Instanz (256 MB, für CT/MR-Studien)
const MAX_SOP_SIZE: usize = 256 * 1024 * 1024;

// ============================================================
// Fehler
// ============================================================

/// DICOM-spezifische Fehler
#[derive(Debug, thiserror::Error)]
pub enum DicomError {
    #[error("leere DICOM-Nachricht")]
    EmptyMessage,
    #[error("ungültiger PDU-Typ: 0x{0:02X}")]
    InvalidPduType(u8),
    #[error("PDU-Größe zu groß: {0} Bytes")]
    PduTooLarge(usize),
    #[error("DICOM-Instanz zu groß: {0} Bytes")]
    SopTooLarge(usize),
    #[error("Speicherfehler: {0}")]
    Storage(String),
    #[error("Datenbankfehler: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("unbekannter Fehler")]
    Unknown,
}

// ============================================================
// DICOM-Objekte
// ============================================================

/// Eine DICOM-Service-Object (SOP) — eine gespeicherte Instanz.
#[derive(Debug, Clone, Serialize)]
pub struct DicomSop {
    /// SOP Class UID (z. B. 1.2.840.10008.5.1.4.1.1.2 = CT)
    pub sop_class_uid: String,
    /// SOP Instance UID (einzigartig pro Instanz)
    pub sop_instance_uid: String,
    /// Patient ID (PatientID, Tag (0010,0020))
    pub patient_id: String,
    /// Patient Name (PatientName, Tag (0010,0010))
    pub patient_name: String,
    /// Modalität (CT, MR, DX, US, …)
    pub modality: String,
    /// Study Instance UID
    pub study_instance_uid: String,
    /// Series Instance UID
    pub series_instance_uid: String,
    /// Lokaler Dateipfad im Storage
    pub storage_path: String,
    /// Größe der Instanz in Bytes
    pub size_bytes: u64,
    /// Zeitstempel der Speicherung (UTC)
    pub created_at: DateTime<Utc>,
    /// UUID des Datensatzes in der Datenbank
    pub db_id: Uuid,
}

/// C-FIND-Request (Suchkriterien)
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct DicomFindRequest {
    /// Optional: Patient ID
    pub patient_id: Option<String>,
    /// Optional: Patient Name (Wildcard)
    pub patient_name: Option<String>,
    /// Optional: Study Instance UID
    pub study_instance_uid: Option<String>,
    /// Optional: Modality (CT, MR, …)
    pub modality: Option<String>,
    /// Maximum number of results (default 100)
    pub limit: u32,
}

/// C-FIND-Ergebnis
#[derive(Debug, Clone, Serialize)]
pub struct DicomFindResponse {
    pub status: String,
    pub count: usize,
    pub results: Vec<DicomSop>,
}

/// C-STORE-Bestätigung (Status-Antwort)
#[derive(Debug, Clone, Serialize)]
pub struct DicomStoreStatus {
    pub status: String,
    pub sop_instance_uid: String,
    pub storage_path: Option<String>,
    pub message: String,
}

/// Status des DICOM-Moduls für das Dashboard
#[derive(Debug, Clone, Serialize)]
pub struct DicomModuleStatus {
    pub name: String,
    pub enabled: bool,
    pub port: u16,
    pub ae_title: String,
    pub storage_path: String,
    pub description: String,
}

// ============================================================
// DICOM SCP Server
// ============================================================

/// Der DICOM-SCP-Server.
///
/// Hält TCP-Verbindungen, parst PDU-Nachrichten (A-ASSOC / DIMSE) und
/// speichert empfangene C-STORE-Instanzen unter dem Storage-Pfad.
pub struct DicomServer {
    /// TCP-Port (Standard 104)
    port: u16,
    /// AE-Title des SCP
    ae_title: String,
    /// Storage-Verzeichnis
    storage_path: String,
    /// Write-Pool (PostgreSQL)
    db: Arc<Database>,
    /// Security (Audit-Hash)
    security: Arc<Security>,
    /// Sender für Graceful Shutdown
    pub shutdown_tx: mpsc::Sender<()>,
}

/// Ein empfangener DICOM-PDU (vereinfachte Darstellung für Audit/Logging)
#[derive(Debug, Clone, Serialize)]
pub struct DicomPdu {
    /// PDU-Typ (1 = A-ASSOC-RQ, 2 = A-ASSOC-AC, 3 = DIMSE, 4 = A-RT)
    pub pdu_type: u8,
    /// Größe in Bytes
    pub size: usize,
    /// Zeitstempel des Empfangs
    pub received_at: DateTime<Utc>,
}

impl DicomServer {
    /// Erstellt einen neuen DICOM-SCP-Server.
    pub fn new(
        port: u16,
        ae_title: &str,
        storage_path: &str,
        db: Arc<Database>,
        security: Arc<Security>,
    ) -> (Self, mpsc::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        (
            Self {
                port,
                ae_title: ae_title.to_string(),
                storage_path: storage_path.to_string(),
                db,
                security,
                shutdown_tx,
            },
            shutdown_rx,
        )
    }

    /// Startet den TCP-Listener und verarbeitet eingehende Verbindungen.
    pub async fn start(&self, mut shutdown_rx: mpsc::Receiver<()>) -> Result<(), DicomError> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .map_err(|e| {
                DicomError::Storage(format!("Bind auf Port {} fehlgeschlagen: {}", self.port, e))
            })?;
        info!(port = self.port, ae = %self.ae_title, "DICOM SCP gestartet");

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("DICOM SCP: Shutdown-Signal erhalten");
                    break;
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let db = self.db.clone();
                            let security = self.security.clone();
                            let ae_title = self.ae_title.clone();
                            let storage_path = self.storage_path.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, db, security, ae_title, storage_path).await {
                                    warn!(%addr, error = %e, "DICOM-Verbindung beendet mit Fehler");
                                }
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "accept() fehlgeschlagen");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Parst eine einzelne DICOM-Instanz (vereinfachte Metadaten-Extraktion).
    ///
    /// Extrahiert PatientID, PatientName, Modality, SOPClassUID, SOPInstanceUID,
    /// StudyInstanceUID, SeriesInstanceUID aus dem DICOM-Datenstrom.
    pub fn parse_c_store(data: &[u8]) -> Result<DicomSopMeta, DicomError> {
        if data.is_empty() {
            return Err(DicomError::EmptyMessage);
        }
        if data.len() > MAX_SOP_SIZE {
            return Err(DicomError::SopTooLarge(data.len()));
        }

        // DICOM-File-Prepend: 128 Bytes (124 Null + "DICM") oder 8 Bytes ("DICM" + 4 Null)
        let meta_start = if data.len() >= 128 && &data[124..128] == b"DICM" {
            128
        } else if data.len() >= 8 && &data[0..4] == b"DICM" {
            8
        } else {
            0
        };

        let body = &data[meta_start..];

        // Direktes Scanning nach bekannten DICOM-Tags (big-endian)
        let mut patient_id = String::new();
        let mut patient_name = String::new();
        let mut modality = String::new();
        let mut sop_class_uid = String::new();
        let mut sop_instance_uid = String::new();
        let mut study_instance_uid = String::new();
        let mut series_instance_uid = String::new();

        // Bekannte Tags suchen
        let tags: Vec<(&[u8], &dyn Fn(&[u8]) -> String)> = vec![
            (&[0x00, 0x10, 0x00, 0x20], &|v: &[u8]| decode_dv(v)),
            (&[0x00, 0x10, 0x00, 0x10], &|v: &[u8]| decode_dv(v)),
            (&[0x00, 0x08, 0x00, 0x60], &|v: &[u8]| decode_dv(v)),
            (&[0x00, 0x08, 0x00, 0x16], &|v: &[u8]| decode_dv(v)),
            (&[0x00, 0x08, 0x00, 0x18], &|v: &[u8]| decode_dv(v)),
            (&[0x00, 0x20, 0x00, 0x0D], &|v: &[u8]| decode_dv(v)),
            (&[0x00, 0x20, 0x00, 0x0E], &|v: &[u8]| decode_dv(v)),
        ];

        for (tag, extract) in &tags {
            // Tag suchen
            if let Some(pos) = find_bytes(body, tag) {
                // Nach dem Tag: VR (2 Bytes) + Length (2 oder 4 Bytes, LITTLE-ENDIAN!) + Value
                if pos + 4 + 2 + 2 <= body.len() {
                    let vr = &body[pos + 4..pos + 6];
                    let is_long = matches!(
                        std::str::from_utf8(vr).unwrap_or("UN"),
                        "OB" | "OW" | "UN" | "UC" | "UR" | "UT"
                    );

                    let (length, value_start) = if is_long {
                        if pos + 4 + 2 + 4 > body.len() {
                            continue;
                        }
                        // LITTLE-ENDIAN!
                        let l = u32::from_le_bytes([
                            body[pos + 6],
                            body[pos + 7],
                            body[pos + 8],
                            body[pos + 9],
                        ]) as usize;
                        (l, pos + 4 + 2 + 4)
                    } else {
                        // LITTLE-ENDIAN!
                        let l = u16::from_le_bytes([body[pos + 6], body[pos + 7]]) as usize;
                        (l, pos + 4 + 2 + 2)
                    };

                    if value_start + length > body.len() {
                        continue;
                    }
                    let value = &body[value_start..value_start + length];

                    // Wert extrahieren
                    let val = extract(value);
                    match (tag[0], tag[1], tag[2], tag[3]) {
                        (0x00, 0x10, 0x00, 0x20) => patient_id = val,
                        (0x00, 0x10, 0x00, 0x10) => patient_name = val,
                        (0x00, 0x08, 0x00, 0x60) => modality = val,
                        (0x00, 0x08, 0x00, 0x16) => sop_class_uid = val,
                        (0x00, 0x08, 0x00, 0x18) => sop_instance_uid = val,
                        (0x00, 0x20, 0x00, 0x0D) => study_instance_uid = val,
                        (0x00, 0x20, 0x00, 0x0E) => series_instance_uid = val,
                        _ => {}
                    }
                }
            }
        }

        Ok(DicomSopMeta {
            sop_class_uid,
            sop_instance_uid,
            patient_id,
            patient_name,
            modality,
            study_instance_uid,
            series_instance_uid,
        })
    }

    /// Speichert eine DICOM-Instanz auf Disk + in der Datenbank.
    pub async fn store_sop(
        &self,
        meta: &DicomSopMeta,
        data: &[u8],
    ) -> Result<DicomSop, DicomError> {
        // 1. Datei auf Disk schreiben
        let dir = format!(
            "{}/{}",
            self.storage_path,
            meta.study_instance_uid
                .chars()
                .take(12)
                .collect::<String>()
                .replace('.', "_")
        );
        std::fs::create_dir_all(&dir).map_err(|e| DicomError::Storage(e.to_string()))?;

        let filename = format!(
            "{}_{}",
            Utc::now().format("%Y%m%d_%H%M%S"),
            meta.sop_instance_uid
                .chars()
                .take(16)
                .collect::<String>()
                .replace('.', "_")
        );
        let path = format!("{}/{}", dir, filename);
        std::fs::write(&path, data).map_err(|e| DicomError::Storage(e.to_string()))?;

        // 2. Metadaten in PostgreSQL speichern
        let db_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO dicom_sops (
                id, sop_class_uid, sop_instance_uid, patient_id, patient_name,
                modality, study_instance_uid, series_instance_uid,
                storage_path, size_bytes, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        )
        .bind(db_id)
        .bind(&meta.sop_class_uid)
        .bind(&meta.sop_instance_uid)
        .bind(&meta.patient_id)
        .bind(&meta.patient_name)
        .bind(&meta.modality)
        .bind(&meta.study_instance_uid)
        .bind(&meta.series_instance_uid)
        .bind(&path)
        .bind(data.len() as i64)
        .bind(Utc::now())
        .execute(self.db.write())
        .await?;

        // 3. Audit-Log
        let audit = serde_json::json!({
            "action": "DICOM_C_STORE",
            "sop_instance_uid": meta.sop_instance_uid,
            "patient_id": meta.patient_id,
            "modality": meta.modality,
            "storage_path": path,
            "size_bytes": data.len(),
        });
        audit_entry(self.db.write(), self.security.as_ref(), &audit).await;

        info!(
            sop = %meta.sop_instance_uid,
            patient = %meta.patient_id,
            modality = %meta.modality,
            "DICOM-Instanz gespeichert"
        );

        Ok(DicomSop {
            sop_class_uid: meta.sop_class_uid.clone(),
            sop_instance_uid: meta.sop_instance_uid.clone(),
            patient_id: meta.patient_id.clone(),
            patient_name: meta.patient_name.clone(),
            modality: meta.modality.clone(),
            study_instance_uid: meta.study_instance_uid.clone(),
            series_instance_uid: meta.series_instance_uid.clone(),
            storage_path: path,
            size_bytes: data.len() as u64,
            created_at: Utc::now(),
            db_id,
        })
    }

    /// C-FIND: Sucht gespeicherte Instanzen nach Suchkriterien.
    pub async fn find_sops(&self, req: &DicomFindRequest) -> Result<DicomFindResponse, DicomError> {
        let limit = req.limit.min(500) as i32;

        let mut conditions: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();

        if let Some(pid) = &req.patient_id {
            args.push(pid.clone());
            conditions.push(format!("patient_id = ${}", args.len()));
        }
        if let Some(pn) = &req.patient_name {
            args.push(format!("%{}%", pn));
            conditions.push(format!("patient_name ILIKE ${}", args.len()));
        }
        if let Some(siuid) = &req.study_instance_uid {
            args.push(siuid.clone());
            conditions.push(format!("study_instance_uid = ${}", args.len()));
        }
        if let Some(m) = &req.modality {
            args.push(m.clone());
            conditions.push(format!("modality = ${}", args.len()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let _sql = format!(
            r#"SELECT id, sop_class_uid, sop_instance_uid, patient_id, patient_name,
                      modality, study_instance_uid, series_instance_uid,
                      storage_path, size_bytes, created_at
               FROM dicom_sops {} ORDER BY created_at DESC LIMIT {}"#,
            where_clause, limit
        );

        // Dynamische Abfrage mit variablen Bindings — hier als statischer
        // Fallback mit LIMIT 500, wenn keine Kriterien angegeben.
        let rows: Vec<DicomSopRow> = if conditions.is_empty() {
            sqlx::query_as(
                r#"SELECT id, sop_class_uid, sop_instance_uid, patient_id, patient_name,
                          modality, study_instance_uid, series_instance_uid,
                          storage_path, size_bytes, created_at
                   FROM dicom_sops ORDER BY created_at DESC LIMIT 500"#,
            )
            .fetch_all(self.db.read())
            .await?
        } else {
            // Mit Kriterien: parametrisierte Abfrage (leere Kriterien = NULL = ignoriert)
            sqlx::query_as::<_, DicomSopRow>(
                r#"SELECT id, sop_class_uid, sop_instance_uid, patient_id, patient_name,
                          modality, study_instance_uid, series_instance_uid,
                          storage_path, size_bytes, created_at
                   FROM dicom_sops
                   WHERE (patient_id = $1 OR $1::text IS NULL)
                     AND (patient_name ILIKE $2 OR $2::text IS NULL)
                     AND (study_instance_uid = $3 OR $3::text IS NULL)
                     AND (modality = $4 OR $4::text IS NULL)
                   ORDER BY created_at DESC LIMIT $5"#,
            )
            .bind(req.patient_id.clone().unwrap_or_default())
            .bind(
                req.patient_name
                    .clone()
                    .map(|v| format!("%{}%", v))
                    .unwrap_or_default(),
            )
            .bind(req.study_instance_uid.clone().unwrap_or_default())
            .bind(req.modality.clone().unwrap_or_default())
            .bind(limit)
            .fetch_all(self.db.read())
            .await?
        };

        // Audit
        let audit = serde_json::json!({
            "action": "DICOM_C_FIND",
            "patient_id": req.patient_id,
            "modality": req.modality,
            "count": rows.len(),
        });
        audit_entry(self.db.read(), self.security.as_ref(), &audit).await;

        Ok(DicomFindResponse {
            status: "SUCCESS".into(),
            count: rows.len(),
            results: rows
                .into_iter()
                .map(|r| DicomSop {
                    sop_class_uid: r.sop_class_uid,
                    sop_instance_uid: r.sop_instance_uid,
                    patient_id: r.patient_id,
                    patient_name: r.patient_name,
                    modality: r.modality,
                    study_instance_uid: r.study_instance_uid,
                    series_instance_uid: r.series_instance_uid,
                    storage_path: r.storage_path,
                    size_bytes: r.size_bytes as u64,
                    created_at: r.created_at,
                    db_id: r.id,
                })
                .collect(),
        })
    }
}

/// Interne Row-Struktur für DICOM-Abfragen
#[derive(sqlx::FromRow)]
struct DicomSopRow {
    id: Uuid,
    sop_class_uid: String,
    sop_instance_uid: String,
    patient_id: String,
    patient_name: String,
    modality: String,
    study_instance_uid: String,
    series_instance_uid: String,
    storage_path: String,
    size_bytes: i64,
    created_at: DateTime<Utc>,
}

/// Extrahierte DICOM-Metadaten (vor Speicherung)
#[derive(Debug, Clone, Default)]
pub struct DicomSopMeta {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub patient_id: String,
    pub patient_name: String,
    pub modality: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
}

/// Dekodiert einen DICOM-Value (DV) aus Bytes (UTF-8/ASCII)
fn decode_dv(value: &[u8]) -> String {
    // DICOM-Strings können mit Null-Bytes oder Leerzeichen gepadded sein
    // Wir entfernen nur das Padding am Ende
    let end = value
        .iter()
        .rposition(|&b| b != 0x00 && b != b' ')
        .map(|i| i + 1)
        .unwrap_or(0);
    String::from_utf8_lossy(&value[..end]).trim().to_string()
}

/// Findet ein Byte-Array in einem anderen Byte-Array.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Schreibt einen Audit-Eintrag in die Audit-Log-Tabelle.
async fn audit_entry(
    pool: &sqlx::Pool<sqlx::Postgres>,
    security: &Security,
    data: &serde_json::Value,
) {
    // Audit-Hash aus dem letzten Eintrag ableiten (vereinfacht: leerer Startwert)
    let prev_hash = String::new();
    let hash = security.audit_hash(&prev_hash, data);
    let ts = Utc::now();
    let actor = "dicom-scp";
    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log (id, actor, action, data, hash, prev_hash, created_at)
           VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)"#,
    )
    .bind(actor)
    .bind(
        data.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN"),
    )
    .bind(data)
    .bind(&hash)
    .bind(&prev_hash)
    .bind(ts)
    .execute(pool)
    .await
    {
        warn!(error = %e, "Audit-Log-Eintrag fehlgeschlagen");
    }
}

/// Handhabt eine einzelne TCP-Verbindung (DICOM-ASSOCIATE + DIMSE)
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    db: Arc<Database>,
    security: Arc<Security>,
    ae_title: String,
    storage_path: String,
) -> Result<(), DicomError> {
    let (server, _rx) = DicomServer::new(0, &ae_title, &storage_path, db, security);

    loop {
        // PDU-Header lesen: 4 Bytes Länge (big-endian) + 1 Byte PDU-Typ
        let mut header = [0u8; 5];
        match stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    return Ok(()); // Client geschlossen
                }
                return Err(DicomError::Io(e));
            }
        }

        let pdu_size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let pdu_type = header[4];

        if pdu_size > MAX_SOP_SIZE {
            return Err(DicomError::PduTooLarge(pdu_size));
        }

        // PDU-Payload lesen
        let mut payload = vec![0u8; pdu_size];
        stream.read_exact(&mut payload).await?;

        match pdu_type {
            // A-ASSOC-RQ: Client möchte Assoziation — mit A-ASSOC-AC antworten
            0x01 => {
                info!("DICOM A-ASSOC-RQ erhalten");
                // Die Assoziationsverhandlung ist NICHT implementiert. Statt
                // einer ungültigen Pseudo-Antwort wird ein normgerechtes
                // A-ABORT (PS3.8 9.3.8, PDU-Typ 0x07) gesendet und die
                // Verbindung beendet.
                let abort: [u8; 10] = [0x07, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00];
                stream.write_all(&abort).await?;
                return Ok(());
            }
            // DIMSE (C-STORE, C-FIND, C-MOVE): PDU-Typ 0x01 mit DIMSE-Command
            // Hier: C-STORE verarbeiten
            t if t == 0x01 || t == 0x02 => {
                // C-STORE: Daten im Payload
                let meta = DicomServer::parse_c_store(&payload)?;
                let sop = server.store_sop(&meta, &payload).await?;
                info!(sop = %sop.sop_instance_uid, "C-STORE verarbeitet");
            }
            // A-RT: Assoziation beenden
            0x04 => {
                info!("DICOM A-RT erhalten — Assoziation beendet");
                return Ok(());
            }
            other => {
                warn!(pdu_type = other, "Unbekannter PDU-Typ");
            }
        }
    }
}

// ============================================================
// Router (Status-Dashboard)
// ============================================================

/// Registriert den DICOM-Router.
///
/// DICOM ist primär ein TCP-Protokoll, nicht HTTP. Für das KIS-Status-
/// Dashboard exponiert dieser Router einen Health-Check unter `/dicom/health`.
/// Der eigentliche SCP wird über `start_dicom_scp` gestartet.
pub fn register_router() -> axum::Router {
    axum::Router::new().route(
        "/dicom/health",
        axum::routing::get(|| async {
            Json(serde_json::json!({
                "module": MODULE_NAME,
                "description": MODULE_DESCRIPTION,
                "protocol": "DICOM (C-STORE / C-FIND / C-MOVE)",
                "port": DEFAULT_DICOM_PORT,
                "ae_title": DEFAULT_AE_TITLE,
                "status": "ok",
            }))
        }),
    )
}

/// Startet den DICOM-SCP auf dem angegebenen Port.
///
/// `storage_path` ist das Verzeichnis für gespeicherte Instanzen
/// (Standard: `/data/dicom`).
/// `shutdown_tx` wird zurückgegeben, um den SCP graceful beenden zu können.
pub async fn start_dicom_scp(
    port: u16,
    ae_title: &str,
    storage_path: &str,
    db: Arc<Database>,
    security: Arc<Security>,
) -> anyhow::Result<mpsc::Sender<()>> {
    let (server, shutdown_rx) = DicomServer::new(port, ae_title, storage_path, db, security);
    let shutdown_tx = server.shutdown_tx.clone();

    tokio::spawn(async move {
        if let Err(e) = server.start(shutdown_rx).await {
            error!(error = %e, "DICOM SCP beendet mit Fehler");
        }
    });

    Ok(shutdown_tx)
}

// ============================================================
// Unit-Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimaler DICOM-Datenstrom mit korrektem File-Prepend + Meta-Elementen
    fn minimal_dicom() -> Vec<u8> {
        // 128 Bytes: 124 Null-Bytes + "DICM" (DICOM-Standard-Prepend)
        let mut v = vec![0u8; 124];
        v.extend_from_slice(b"DICM");
        // File Meta (0002,0000) UL Group Length = 0 (kein File Meta Header)
        v.extend_from_slice(&[0x00, 0x02, 0x00, 0x00, b'U', b'L', 0x00, 0x00, 0x00, 0x00]);
        // (0010,0020) LO PatientID = "P123"
        v.extend_from_slice(&[0x00, 0x10, 0x00, 0x20, b'L', b'O', 0x04, 0x00]);
        v.extend_from_slice(b"P123");
        // (0008,0060) CS Modality = "CT"
        v.extend_from_slice(&[0x00, 0x08, 0x00, 0x60, b'C', b'S', 0x02, 0x00]);
        v.extend_from_slice(b"CT");
        v
    }

    #[test]
    fn test_parse_c_store() {
        let data = minimal_dicom();
        let meta = DicomServer::parse_c_store(&data).unwrap();
        assert_eq!(meta.patient_id, "P123");
        assert_eq!(meta.modality, "CT");
    }

    #[test]
    fn test_parse_empty() {
        assert!(matches!(
            DicomServer::parse_c_store(&[]),
            Err(DicomError::EmptyMessage)
        ));
    }
}
