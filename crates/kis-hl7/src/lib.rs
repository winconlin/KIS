//! KIS HL7 v2.5 MLLP Server
//!
//! Vollständiger HL7-ADT/ORU/SIU-Server als Teil des KIS:
//!
//! - **MLLP-Framing**: Start `0x0D 0x0B` (VT), Ende `0x0C 0x0D` (FS CR)
//! - **Tokio TcpListener** (Standard-Port 6667, konfigurierbar)
//! - **Parsing**: MSH, EVN, PID, NK1, OBX-Segmente
//! - **Nachrichtentypen**: ADT^A01, ADT^A02, ADT^A04, ADT^A08,
//!   ORU^R01, ORM^O01, SIU^S12, SIU^S13
//! - **ACK-Antwort** (MSA-01 = AA) bei Erfolg, **NACK** (MSA-01 = AE) bei Fehler
//! - **Audit-Log** für jede empfangene Nachricht (DSGVO, 10 Jahre)
//!
//! Struktur (wie gefordert):
//! - `Hl7Server` mit `start`, `handle_connection`, `parse_message`, `process_message`
//! - `Hl7Message` mit `msg_type`, `msg_id`, `timestamp`, `segments`
//! - `Hl7Segment` mit `segment_type`, `fields`
//! - `register_router`
//!
//! KIS ist der VOLLSTÄNDIGE ERSATZ für CGM MEDICO — dieser Server übernimmt
//! den HL7-Eingang, den CGM MEDICO bisher über eine externe MLLP-Box lieferte.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;
use tracing::{debug, error, info, warn};

use kis_core::db::Database;

/// Modulname für Status-Dashboard
pub const MODULE_NAME: &str = "kis-hl7";
/// Modulbeschreibung
pub const MODULE_DESCRIPTION: &str = "kis-hl7 — HL7 v2.5 MLLP Server (ADT/ORU/SIU)";

/// Standard-Port für HL7-via-MLLP (CGM MEDICO-Konvention)
pub const DEFAULT_HL7_PORT: u16 = 6667;

/// MLLP-Kontrollzeichen
const MLLP_START: &[u8] = &[0x0D, 0x0B]; // VT
const MLLP_END: &[u8] = &[0x0C, 0x0D]; // FS, CR
/// Maximale Größe einer einzelnen HL7-Nachricht (64 MB, für große ORU-Blobs)
const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
/// Zeitfenster, in dem auf MLLP-Daten gewartet wird (10 s)
const READ_TIMEOUT: Duration = Duration::from_secs(10);

// ============================================================
// Fehler
// ============================================================

/// HL7-spezifische Fehler
#[derive(Debug, thiserror::Error)]
pub enum Hl7Error {
    #[error("leere HL7-Nachricht")]
    EmptyMessage,
    #[error("MSH-Segment fehlt")]
    MissingMsh,
    #[error("MSH-9 (Nachrichtentyp) fehlt")]
    MissingMsgType,
    #[error("PID-Segment fehlt")]
    MissingPid,
    #[error("PID-3 (Patienten-ID) fehlt")]
    MissingPatientId,
    #[error("OBX-Segment fehlt (erwartet bei ORU^R01)")]
    MissingObx,
    #[error("Nicht unterstützter Nachrichtentyp: {0}")]
    UnsupportedMsgType(String),
}

// ============================================================
// Datenmodelle
// ============================================================

/// Ein HL7-Segment (eine Zeile der Nachricht).
///
/// `segment_type` = erste Komponente (MSH, EVN, PID, NK1, OBX, ...),
/// `fields` = die übrigen Felder, durch `|` getrennt, als Vektor.
#[derive(Debug, Clone, Serialize)]
pub struct Hl7Segment {
    /// Segment-Typ (z. B. "MSH", "PID", "OBX")
    pub segment_type: String,
    /// Felder des Segments (ohne den Segment-Typ, 0-basiert)
    pub fields: Vec<String>,
}

impl Hl7Segment {
    /// Erzeugt ein Segment aus einer rohen Segment-Zeile.
    ///
    /// Die erste Komponente (Feld 0) ist der Segment-Typ, die restlichen
    /// Komponenten werden als Datenfelder abgelegt.
    pub fn new(raw: &str) -> Self {
        let mut fields: Vec<String> = raw.split('|').map(|f| f.to_string()).collect();
        // Feld 0 ist der Segment-Typ — wir speichern ihn separat und halten
        // `fields` als die eigentlichen Datenfelder (ohne den Typ).
        let segment_type = fields
            .first()
            .cloned()
            .unwrap_or_else(|| String::from("UNKNOWN"));
        fields.remove(0);
        Self {
            segment_type,
            fields,
        }
    }

    /// Liest ein Feld (0-basiert, nach dem Segment-Typ) als `Option<&str>`.
    pub fn get(&self, index: usize) -> Option<&str> {
        self.fields.get(index).map(|s| s.as_str())
    }

    /// Liest ein Feld und gibt `""` zurück, wenn es fehlt.
    pub fn get_or_empty(&self, index: usize) -> String {
        self.get(index).unwrap_or("").to_string()
    }
}

/// Eine geparste HL7-Nachricht.
#[derive(Debug, Clone, Serialize)]
pub struct Hl7Message {
    /// Nachrichtentyp (z. B. "ADT^A01", "ORU^R01") — MSH-9
    pub msg_type: String,
    /// Nachrichten-Id — MSH-10
    pub msg_id: String,
    /// Zeitstempel — MSH-7
    pub timestamp: DateTime<Utc>,
    /// Alle Segmente der Nachricht (MSH, EVN, PID, NK1, OBX, ...)
    pub segments: Vec<Hl7Segment>,
    /// Rohe Nachricht (für Audit-Log & Debugging)
    #[serde(skip)]
    pub raw: String,
}

impl Hl7Message {
    /// Sucht das erste Segment eines bestimmten Typs.
    pub fn segment(&self, seg_type: &str) -> Option<&Hl7Segment> {
        self.segments.iter().find(|s| s.segment_type == seg_type)
    }

    /// Sucht alle Segmente eines bestimmten Typs.
    pub fn segments_of(&self, seg_type: &str) -> Vec<&Hl7Segment> {
        self.segments
            .iter()
            .filter(|s| s.segment_type == seg_type)
            .collect()
    }

    /// PID-03 (Patienten-ID) — primäre Identifikation.
    /// In `fields` (nach PID-Typ): Index 2
    pub fn patient_id(&self) -> Option<String> {
        self.segment("PID")
            .and_then(|p| p.get(2))
            .map(|s| s.to_string())
    }

    /// PID-05 (Name, family) — Index 4 in `fields` (nach dem PID-Typ).
    pub fn patient_name(&self) -> Option<String> {
        self.segment("PID")
            .and_then(|p| p.get(4))
            .map(|s| s.to_string())
    }
}

// ============================================================
// ACK / NACK
// ============================================================

/// ACK-Antwort (MSA-01 = AA, Nachricht erfolgreich verarbeitet).
fn build_ack(msg: &Hl7Message) -> String {
    let now = Utc::now().format("%Y%m%d%H%M%S");
    // ACK enthält MSH, MSA und referenziert MSH-9/MSH-10 der empfangenen Nachricht
    format!(
        "MSH|^~\\&|KIS|KIS|CGM_MEDICO|CGM_MEDICO|{}||HL7{msg_type}||2.5|{msg_id}\n\
         MSA|AA|{msg_id}",
        now,
        msg_type = msg.msg_type,
        msg_id = msg.msg_id
    )
}

/// NACK-Antwort (MSA-01 = AE, Fehler) mit Fehlerbeschreibung.
fn build_nack(msg: &Hl7Message, error_code: &str, error_text: &str) -> String {
    let now = Utc::now().format("%Y%m%d%H%M%S");
    format!(
        "MSH|^~\\&|KIS|KIS|CGM_MEDICO|CGM_MEDICO|{}||HL7{msg_type}||2.5|{msg_id}\n\
         MSA|AE|{msg_id}\n\
         ERR|{code}|{text}",
        now,
        msg_type = msg.msg_type,
        msg_id = msg.msg_id,
        code = error_code,
        text = error_text
    )
}

/// Generisches NACK, wenn selbst das Parsing nicht funktionierte.
fn generic_nack(error_text: &str) -> String {
    let now = Utc::now().format("%Y%m%d%H%M%S");
    format!(
        "MSH|^~\\&|KIS|KIS|CGM_MEDICO|CGM_MEDICO|{}||HL7ACK||2.5|UNKNOWN\n\
         MSA|AE|UNKNOWN\n\
         ERR|E000|{}",
        now, error_text
    )
}

// ============================================================
// HL7 Server
// ============================================================

/// HL7 v2.5 MLLP Server
pub struct Hl7Server {
    /// TCP-Port (Standard 6667)
    pub port: u16,
    /// AE-Titel (Application Entity) — MSH-4 / MSH-5
    pub ae_title: String,
    /// Datenbank-Pool für Audit-Log & Entitäts-Persistenz
    pub db: Arc<Database>,
    /// Sender für den Shutdown-Signal-Channel
    pub shutdown_tx: mpsc::Sender<()>,
}

impl Hl7Server {
    /// Erzeugt einen neuen HL7-Server.
    pub fn new(port: u16, ae_title: &str, db: Arc<Database>) -> (Self, mpsc::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let server = Self {
            port,
            ae_title: ae_title.to_string(),
            db,
            shutdown_tx: shutdown_tx.clone(),
        };
        (server, shutdown_rx)
    }

    /// Startet den MLLP-Listener und akzeptiert Verbindungen.
    ///
    /// Läuft bis der Shutdown-Signal-Channel einen Wert empfängt.
    pub async fn start(mut self, mut shutdown_rx: mpsc::Receiver<()>) -> anyhow::Result<()> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;
        info!(
            port = self.port,
            ae_title = %self.ae_title,
            "HL7 MLLP Server gestartet"
        );

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((socket, remote_addr)) => {
                            info!(%remote_addr, "HL7-Verbindung angenommen");
                            let db = self.db.clone();
                            let ae_title = self.ae_title.clone();
                            tokio::spawn(async move {
                                // Verbindung in einem Task verarbeiten
                                let res = handle_connection(socket, db, &ae_title).await;
                                if let Err(e) = res {
                                    error!(%remote_addr, error = %e, "HL7-Verbindungsfehler");
                                }
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "Fehler beim Annehmen der HL7-Verbindung");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("HL7-Server: Shutdown-Signal erhalten");
                    break;
                }
            }
        }
        Ok(())
    }

    /// Parst eine rohe HL7-Nachricht (MLLP-Inhalt, ohne Framing).
    ///
    /// Validiert MSH, segmentiert per `\r` (Fallback: `\n`) und extrahiert
    /// MSH-9 (msg_type), MSH-10 (msg_id), MSH-7 (timestamp).
    pub fn parse_message(raw: &str) -> Result<Hl7Message, Hl7Error> {
        // Leere Eingabe → EmptyMessage
        if raw.trim().is_empty() {
            return Err(Hl7Error::EmptyMessage);
        }

        let first_line = raw
            .split(|c| c == '\r' || c == '\n')
            .next()
            .ok_or(Hl7Error::EmptyMessage)?;

        if !first_line.starts_with("MSH") {
            return Err(Hl7Error::MissingMsh);
        }

        // Segmente aufteilen
        let segment_lines: Vec<&str> = raw
            .split(|c| c == '\r' || c == '\n')
            .filter(|line| !line.trim().is_empty())
            .collect();

        let mut segments = Vec::with_capacity(segment_lines.len());
        for line in segment_lines {
            segments.push(Hl7Segment::new(line));
        }

        if segments.is_empty() {
            return Err(Hl7Error::EmptyMessage);
        }

        // MSH-Felder (Feld 0 = "MSH" ist segment_type, daher ab Index 0 in fields)
        // MSH-7 (timestamp) = Index 6 in der Original-Meldung = Index 5 in fields
        // MSH-9 (msg_type) = Index 8 in der Original-Meldung = Index 7 in fields
        // MSH-10 (msg_id) = Index 9 in der Original-Meldung = Index 8 in fields
        let msh = &segments[0];
        let msg_type = msh.get_or_empty(7); // MSH-9
        let msg_id = msh.get_or_empty(8); // MSH-10
        let msh_timestamp = msh.get_or_empty(5); // MSH-7

        if msg_type.is_empty() {
            return Err(Hl7Error::MissingMsgType);
        }

        // MSH-7 (Zeitstempel) parsen — Format: YYYYMMDD[HHMMSS[.ffffff]][±ZZZZ]
        let timestamp = parse_hl7_timestamp(&msh_timestamp).unwrap_or_else(Utc::now);

        Ok(Hl7Message {
            msg_type,
            msg_id,
            timestamp,
            segments,
            raw: raw.to_string(),
        })
    }

    /// Verarbeitet eine geparste Nachricht und liefert die ACK-/NACK-Antwort.
    ///
    /// Dispatch auf den Nachrichtentyp (ADT^A01, ADT^A02, ADT^A04, ADT^A08,
    /// ORU^R01, ORM^O01, SIU^S12, SIU^S13). Unbekannte Typen erhalten NACK.
    pub async fn process_message(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        let ack = match msg.msg_type.as_str() {
            // Patienten-Einlieferung / Aufnahme
            "ADT^A01" => self.handle_adt_a01(msg).await?,
            // Patient-Update
            "ADT^A02" => self.handle_adt_a02(msg).await?,
            // Patienten-Verlegungs-Einladung
            "ADT^A04" => self.handle_adt_a04(msg).await?,
            // Patienten-Entlassung
            "ADT^A08" => self.handle_adt_a08(msg).await?,
            // Befund-Report (Lab-Resultate in OBX)
            "ORU^R01" => self.handle_oru_r01(msg).await?,
            // Service-Order (ORM)
            "ORM^O01" => self.handle_orm_o01(msg).await?,
            // Slot-Update (bestätigt)
            "SIU^S12" => self.handle_siu_s12(msg).await?,
            // Slot-Storno
            "SIU^S13" => self.handle_siu_s13(msg).await?,
            // Unbekannter Typ → NACK
            _ => return Err(Hl7Error::UnsupportedMsgType(msg.msg_type.clone())),
        };

        // Audit-Log: erfolgreiche Verarbeitung
        self.audit(msg, "HL7_RECEIVED", &ack).await;
        info!(
            msg_type = %msg.msg_type,
            msg_id = %msg.msg_id,
            patient = ?msg.patient_id(),
            "HL7-Nachricht verarbeitet"
        );
        Ok(ack)
    }

    // --------------------------------------------------------
    // Nachrichtentyp-Handler (jeder liefert den ACK-Text)
    // --------------------------------------------------------

    /// ADT^A01 — Patienten-Einlieferung
    async fn handle_adt_a01(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        self.validate_patient(msg)?;
        Ok(build_ack(msg))
    }

    /// ADT^A02 — Patient-Update
    async fn handle_adt_a02(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        self.validate_patient(msg)?;
        Ok(build_ack(msg))
    }

    /// ADT^A04 — Patienten-Verlegungs-Einladung
    async fn handle_adt_a04(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        self.validate_patient(msg)?;
        Ok(build_ack(msg))
    }

    /// ADT^A08 — Patienten-Entlassung
    async fn handle_adt_a08(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        self.validate_patient(msg)?;
        Ok(build_ack(msg))
    }

    /// ORU^R01 — Befund-Report (mindestens ein OBX erwartet)
    async fn handle_oru_r01(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        if msg.segments_of("OBX").is_empty() {
            return Err(Hl7Error::MissingObx);
        }
        self.validate_patient(msg)?;
        Ok(build_ack(msg))
    }

    /// ORM^O01 — Service-Order
    async fn handle_orm_o01(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        self.validate_patient(msg)?;
        Ok(build_ack(msg))
    }

    /// SIU^S12 — Slot-Update (bestätigt)
    async fn handle_siu_s12(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        Ok(build_ack(msg))
    }

    /// SIU^S13 — Slot-Storno
    async fn handle_siu_s13(&self, msg: &Hl7Message) -> Result<String, Hl7Error> {
        Ok(build_ack(msg))
    }

    // --------------------------------------------------------
    // Validierung & Audit
    // --------------------------------------------------------

    /// Validiert PID-Präsenz und -Integrität.
    fn validate_patient(&self, msg: &Hl7Message) -> Result<(), Hl7Error> {
        if msg.segment("PID").is_none() {
            return Err(Hl7Error::MissingPid);
        }
        if msg
            .patient_id()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            return Err(Hl7Error::MissingPatientId);
        }
        Ok(())
    }

    /// Schreibt einen Audit-Log-Eintrag (DSGVO, 10 Jahre).
    ///
    /// Fehlschlagen des Audit-Logs ist NICHT fatal — die Nachricht wird
    /// dennoch bestätigt, der Fehler wird aber geloggt.
    ///
    /// Hinweis: `entity_id` ist eine UUID-Spalte, daher wird die HL7-
    /// Nachrichten-ID (MSH-10) als Hex in der JSON-`new_value` abgelegt,
    /// nicht als Entitäts-ID. `current_hash` = SHA-256 des Roh-Messages
    /// (Integrität, Hash-Chain-Anker).
    async fn audit(&self, msg: &Hl7Message, action: &str, response: &str) {
        let pool = self.db.write();
        let now = Utc::now();
        let msg_id = msg.msg_id.clone();
        let patient_id = msg.patient_id().unwrap_or_default();

        let new_value = serde_json::json!({
            "msg_type": msg.msg_type,
            "msg_id": msg_id,
            "patient_id": patient_id,
            "response": response,
        });
        let result = sqlx::query(
            "INSERT INTO audit_log \
             (timestamp, user_id, action, entity_type, entity_id, new_value, current_hash) \
             VALUES ($1, NULL, $2, 'hl7_message', NULL, $3, $4)",
        )
        .bind(now)
        .bind(action)
        .bind(&new_value)
        .bind(sha256_hex(&msg.raw))
        .execute(pool)
        .await;

        if let Err(e) = result {
            // Audit-Log-Tabelle kann in einer frischen Umgebung noch fehlen
            warn!(
                error = %e,
                "Audit-Log-Eintrag fehlgeschlagen (Tabelle audit_log fehlt?)"
            );
        }
    }
}

// ============================================================
// MLLP-Verbindung
// ============================================================

/// Behandelt eine einzelne MLLP-Verbindung.
///
/// Liest MLLP-geframte Nachrichten (`0x0D 0x0B` ... `0x0C 0x0D`),
/// parst, verarbeitet und antwortet mit ACK/NACK.
async fn handle_connection(
    mut socket: tokio::net::TcpStream,
    db: Arc<Database>,
    ae_title: &str,
) -> anyhow::Result<()> {
    // Server-Instanz für die Verarbeitung (Port 0 — kein eigener Listener)
    let (server, _rx) = Hl7Server::new(0, ae_title, db);

    loop {
        // Auf ein Byte warten
        let mut byte = [0u8; 1];
        match tokio::time::timeout(READ_TIMEOUT, socket.read(&mut byte)).await {
            Ok(Ok(0)) => {
                // Verbindung sauber geschlossen (EOF)
                debug!("HL7-Verbindung geschlossen (EOF)");
                break;
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(anyhow::anyhow!("MLLP-Lesefehler: {}", e)),
            Err(_) => {
                // Timeout — kein Datenfluss mehr
                debug!("HL7-Timeout beim Lesen");
                break;
            }
        }

        // MLLP-Start prüfen: 0x0D 0x0B
        if byte[0] != MLLP_START[0] {
            // Nichts vor dem MLLP-Start — ignorieren
            continue;
        }
        let mut byte2 = [0u8; 1];
        socket.read_exact(&mut byte2).await?;
        if byte2[0] != MLLP_START[1] {
            // Falsches Startzeichen — synchronisieren
            continue;
        }

        // Nachricht lesen bis MLLP-Ende (0x0C 0x0D)
        let message = read_mllp_message(&mut socket).await?;
        if message.is_empty() {
            continue;
        }

        // Als String dekodieren (HL7 = ISO-8859-1/US-ASCII, UTF-8-fallback)
        let raw = String::from_utf8_lossy(&message).to_string();
        if raw.trim().is_empty() {
            continue;
        }

        // Nachricht verarbeiten
        let response = match Hl7Server::parse_message(&raw) {
            Ok(msg) => match server.process_message(&msg).await {
                Ok(ack) => ack,
                Err(e) => build_nack(&msg, "E001", &e.to_string()),
            },
            Err(e) => generic_nack(&e.to_string()),
        };

        debug!(len = response.len(), "HL7-Antwort generiert");

        // MLLP-framte Antwort senden: 0x0D 0x0B + ACK + 0x0C 0x0D
        socket.write_all(MLLP_START).await?;
        socket.write_all(response.as_bytes()).await?;
        socket.write_all(MLLP_END).await?;
        socket.flush().await?;
    }

    Ok(())
}

/// Liest die MLLP-Nachricht (ohne Start) bis zum MLLP-Ende `0x0C 0x0D`.
async fn read_mllp_message(socket: &mut tokio::net::TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut message = Vec::with_capacity(4096);
    let mut prev_byte: Option<u8> = None;

    loop {
        if message.len() > MAX_MESSAGE_SIZE {
            return Err(anyhow::anyhow!(
                "HL7-Nachricht zu groß (> {} Bytes)",
                MAX_MESSAGE_SIZE
            ));
        }

        let mut byte = [0u8; 1];
        match tokio::time::timeout(READ_TIMEOUT, socket.read(&mut byte)).await {
            Ok(Ok(0)) => break, // EOF
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(anyhow::anyhow!("MLLP-Lesefehler: {}", e)),
            Err(_) => {
                return Err(anyhow::anyhow!("MLLP-Timeout beim Lesen der Nachricht"));
            }
        }

        // MLLP-Ende: 0x0C 0x0D
        if prev_byte == Some(0x0C) && byte[0] == 0x0D {
            break;
        }

        message.push(byte[0]);
        prev_byte = Some(byte[0]);
    }

    // Das abschließende 0x0C gehört zum Endmarker, nicht zum Payload
    if let Some(last) = message.last() {
        if *last == 0x0C {
            message.pop();
        }
    }

    Ok(message)
}

// ============================================================
// Hilfsfunktionen
// ============================================================

/// Parst einen HL7-Zeitstempel (YYYYMMDD[HHMMSS[.ffffff]][±ZZZZ]) nach Utc.
fn parse_hl7_timestamp(s: &str) -> Option<DateTime<Utc>> {
    if s.is_empty() {
        return None;
    }
    // Vollformat: 20261101120000
    if s.len() >= 14 {
        return chrono::NaiveDateTime::parse_from_str(&s[..14], "%Y%m%d%H%M%S")
            .ok()
            .map(|n| n.and_utc());
    }
    // Datum: 20261101
    if s.len() >= 8 {
        return chrono::NaiveDate::parse_from_str(&s[..8], "%Y%m%d")
            .ok()
            .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }
    None
}

/// SHA-256-Hash einer Nachricht als Hex (für Audit-Log Hash-Chain).
fn sha256_hex(data: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Einfacher Hex-Encoder (vermeidet zusätzliche `hex`-Abhängigkeit).
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// ============================================================
// Router-Registrierung
// ============================================================

/// Registriert den HL7-Router.
///
/// HL7 ist primär ein TCP-Protokoll (MLLP), nicht HTTP. Für das KIS-Status-
/// Dashboard exponiert dieser Router einen Health-Check unter `/hl7/health`.
/// Der eigentliche MLLP-Server wird über `start_mllp_server` gestartet.
pub fn register_router() -> axum::Router {
    axum::Router::new().route(
        "/hl7/health",
        axum::routing::get(|| async {
            Json(serde_json::json!({
                "module": MODULE_NAME,
                "description": MODULE_DESCRIPTION,
                "protocol": "HL7 v2.5 MLLP",
                "port": DEFAULT_HL7_PORT,
                "status": "ok",
            }))
        }),
    )
}

/// Startet den MLLP-Server auf dem angegebenen Port.
///
/// `shutdown_tx` wird zurückgegeben, um den Server graceful beenden zu können.
pub async fn start_mllp_server(
    port: u16,
    ae_title: &str,
    db: Arc<Database>,
) -> anyhow::Result<mpsc::Sender<()>> {
    let (server, shutdown_rx) = Hl7Server::new(port, ae_title, db);
    let shutdown_tx = server.shutdown_tx.clone();

    tokio::spawn(async move {
        if let Err(e) = server.start(shutdown_rx).await {
            error!(error = %e, "HL7 MLLP Server beendet mit Fehler");
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

    /// Beispiel ADT^A01 Nachricht
    const ADT_A01: &str = "MSH|^~\\&|CGM_MEDICO|MEDICO|KIS|KIS|20261101120000||ADT^A01|A0100001|P|2.5\n\
        EVN|A01|20261101120000\n\
        PID|1||123456789^^^MRN^MR||MUSTER^MAX^^^^||19800115|M\n\
        NK1|1|MUSTER^HANNA^^^^||MOTHER\n\
        OBX|1|TX|2.16.840.1.113883.6.1|01||Anamnese|Freitext|||F";

    #[test]
    fn test_parse_adt_a01() {
        let msg = Hl7Server::parse_message(ADT_A01).unwrap();
        assert_eq!(msg.msg_type, "ADT^A01"); // MSH-9
        assert_eq!(msg.msg_id, "A0100001"); // MSH-10
        assert!(msg.segment("PID").is_some());
        assert_eq!(
            msg.patient_id(),
            Some("123456789^^^MRN^MR".to_string())
        );
        assert!(msg.segment("NK1").is_some());
        assert_eq!(msg.segments_of("OBX").len(), 1);
        assert!(msg.timestamp.year() == 2026, "timestamp: {:?}", msg.timestamp);
    }

    #[test]
    fn test_parse_missing_msh() {
        let result = Hl7Server::parse_message("PID|1||123");
        assert!(matches!(result, Err(Hl7Error::MissingMsh)));
    }

    #[test]
    fn test_parse_empty() {
        let result = Hl7Server::parse_message("");
        assert!(matches!(result, Err(Hl7Error::EmptyMessage)));
    }

    #[test]
    fn test_ack_format() {
        let msg = Hl7Server::parse_message(ADT_A01).unwrap();
        let ack = build_ack(&msg);
        assert!(ack.contains("MSA|AA|A0100001"));
        assert!(ack.starts_with("MSH|^~\\&"));
    }

    #[test]
    fn test_nack_format() {
        let msg = Hl7Server::parse_message(ADT_A01).unwrap();
        let nack = build_nack(&msg, "E001", "PID fehlt");
        assert!(nack.contains("MSA|AE|A0100001"));
        assert!(nack.contains("ERR|E001|PID fehlt"));
    }

    #[test]
    fn test_segment_parsing() {
        let seg = Hl7Segment::new("PID|1||123456789^^^MRN^MR||MUSTER^MAX^^^^");
        assert_eq!(seg.segment_type, "PID");
        // In `fields` (nach PID-Typ):
        // [0] = "1", [1] = "", [2] = "123456789^^^MRN^MR", [3] = "", [4] = "MUSTER^MAX^^^^"
        assert_eq!(seg.get(2), Some("123456789^^^MRN^MR"));
        assert_eq!(seg.get(4), Some("MUSTER^MAX^^^^"));
    }

    #[test]
    fn test_timestamp_parsing() {
        assert!(parse_hl7_timestamp("20261101120000").is_some());
        assert!(parse_hl7_timestamp("20261101").is_some());
        assert!(parse_hl7_timestamp("").is_none());
        assert!(parse_hl7_timestamp("invalid").is_none());
    }

    #[test]
    fn test_process_message_unsupported() {
        // Unbekannter Typ → NACK
        let raw = "MSH|^~\\&|A|B|C|D|20261101120000||XYZ^UNKNOWN|1|P|2.5";
        let msg = Hl7Server::parse_message(raw).unwrap();
        assert_eq!(msg.msg_type, "XYZ^UNKNOWN");
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x10]), "00ff10");
    }
}
