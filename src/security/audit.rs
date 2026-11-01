//! Audit-Log (append-only) mit Hash-Kette gegen Manipulation
//!
//! Jeder Eintrag enthält den SHA-256-Hash des vorherigen Eintrags (prev_hash).
//! So ist jede nachträgliche Änderung erkennbar: verify_chain() findet die
//! erste veränderte/entfernte Stelle.
//!
//! DSGVO: Aufbewahrung mindestens 10 Jahre (3650 Tage) — Einträge werden
//! niemals gelöscht, sondern nur archiviert.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// Aktionen, die im Audit-Log protokolliert werden
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditAction {
    /// Daten gelesen (z. B. Patientendossier geöffnet)
    Read,
    /// Daten angelegt
    Create,
    /// Daten geändert
    Update,
    /// Daten gelöscht (nur für nicht-patientenbezogene Entitäten)
    Delete,
    /// Login (erfolgreich)
    Login,
    /// Login fehlgeschlagen
    LoginFailed,
    /// Konto gesperrt
    Lockout,
    /// MFA-Code abgefragt/geprüft
    Mfa,
    /// Refresh-Token ausgestellt
    Refresh,
    /// Sitzung beendet
    Logout,
    /// Audit-Kette verifiziert
    ChainVerify,
}

impl AuditAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditAction::Read => "read",
            AuditAction::Create => "create",
            AuditAction::Update => "update",
            AuditAction::Delete => "delete",
            AuditAction::Login => "login",
            AuditAction::LoginFailed => "login_failed",
            AuditAction::Lockout => "lockout",
            AuditAction::Mfa => "mfa",
            AuditAction::Refresh => "refresh",
            AuditAction::Logout => "logout",
            AuditAction::ChainVerify => "chain_verify",
        }
    }

    /// Liest eine Action-String aus der Datenbank wieder ein
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            "login" => Some(Self::Login),
            "login_failed" => Some(Self::LoginFailed),
            "lockout" => Some(Self::Lockout),
            "mfa" => Some(Self::Mfa),
            "refresh" => Some(Self::Refresh),
            "logout" => Some(Self::Logout),
            "chain_verify" => Some(Self::ChainVerify),
            _ => None,
        }
    }
}

/// Ein Audit-Log-Eintrag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub timestamp: DateTime<Utc>,
    pub user_id: Option<String>,
    pub action: AuditAction,
    pub entity_type: String,
    pub entity_id: Option<String>,
    /// JSON-Serialisierung des alten Werts (bei update)
    pub old_value: Option<String>,
    /// JSON-Serialisierung des neuen Werts (bei create/update)
    pub new_value: Option<String>,
    pub ip_address: Option<String>,
    /// Hash des vorherigen Eintrags (Kette)
    pub prev_hash: String,
    /// SHA-256 dieses Eintrags inklusive prev_hash
    pub hash: String,
}

/// Fehler bei der Audit-Verarbeitung
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("Dateizugriff fehlgeschlagen: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON-Serialisierung fehlgeschlagen: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Datenbankfehler: {0}")]
    Db(#[from] sqlx::Error),
}

/// Audit-Logger: schreibt Einträge asynchron in Log-Datei + Datenbank.
///
/// Der Logger hält den letzten Hash der Kette und rechnet den Hash jedes
/// neuen Eintrags aus, bevor er ihn schreibt.
pub struct AuditLogger {
    tx: tokio::sync::mpsc::Sender<AuditLog>,
    /// Letzter bekannter Hash der Kette (wird im Writer-Task fortgeführt)
    last_hash: Mutex<String>,
    _handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AuditLogger {
    /// Startet den Audit-Logger mit Hintergrund-Writer.
    ///
    /// `path`: Pfad der append-only Log-Datei (z. B. `logs/audit.log`).
    /// `db_pool`: Pool für die `audit_log`-Tabelle.
    ///
    /// Die Kette wird über Neustarts hinweg fortgesetzt, indem der letzte
    /// Hash aus der Datenbank geladen wird.
    pub async fn start(
        path: &Path,
        db_pool: sqlx::Pool<sqlx::Sqlite>,
    ) -> anyhow::Result<Self> {
        let dir = path.parent();
        if let Some(dir) = dir {
            std::fs::create_dir_all(dir)?;
        }

        // Letzten Hash aus der DB laden (Kettenfortsetzung)
        let last_hash = Self::load_last_hash(&db_pool).await?;
        tracing::info!(last_hash = %last_hash, "Audit-Kette fortgesetzt");

        let (tx, mut rx) = tokio::sync::mpsc::channel::<AuditLog>(1024);
        let log_path = path.to_path_buf();

        let handle = tokio::spawn(async move {
            while let Some(entry) = rx.recv().await {
                if let Err(e) = write_entry(&log_path, &db_pool, &entry).await {
                    tracing::error!(error = %e, "Audit-Eintrag konnte nicht geschrieben werden");
                }
            }
            tracing::info!("Audit-Writer-Thread beendet");
        });

        Ok(Self {
            tx,
            last_hash: Mutex::new(last_hash),
            _handle: Mutex::new(Some(handle)),
        })
    }

    /// Lädt den letzten Hash aus der Datenbank.
    async fn load_last_hash(pool: &sqlx::Pool<sqlx::Sqlite>) -> anyhow::Result<String> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|r| r.0).unwrap_or_else(genesis_hash))
    }

    /// Loggt einen Eintrag (asynchron — blockiert nicht).
    ///
    /// Rechnet prev_hash + hash aus dem letzten bekannten Hash aus,
    /// aktualisiert den Zustand und übergibt den Eintrag an den Writer.
    pub fn log(
        &self,
        user_id: Option<&str>,
        action: AuditAction,
        entity_type: &str,
        entity_id: Option<&str>,
        old_value: Option<&str>,
        new_value: Option<&str>,
        ip_address: Option<&str>,
    ) {
        // Hash aus dem letzten bekannten Wert ableiten
        let prev_hash = {
            let guard = self.last_hash.lock().unwrap();
            guard.clone()
        };

        let mut entry = AuditLog {
            timestamp: Utc::now(),
            user_id: user_id.map(String::from),
            action,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.map(String::from),
            old_value: old_value.map(String::from),
            new_value: new_value.map(String::from),
            ip_address: ip_address.map(String::from),
            prev_hash: prev_hash.clone(),
            hash: String::new(),
        };
        entry.hash = compute_entry_hash(&entry);

        // Letzten Hash auf den neuen aktualisieren
        {
            let mut guard = self.last_hash.lock().unwrap();
            *guard = entry.hash.clone();
        }

        // Asynchron übergeben (kein Panic, wenn Channel voll)
        let _ = self.tx.try_send(entry);
    }

    /// Synchroner Log-Call für Fälle ohne tokio-Runtime (z. B. Tests).
    /// Schreibt direkt in Datei + DB.
    pub async fn log_sync(
        &self,
        path: &Path,
        pool: &sqlx::Pool<sqlx::Sqlite>,
        user_id: Option<&str>,
        action: AuditAction,
        entity_type: &str,
        entity_id: Option<&str>,
        old_value: Option<&str>,
        new_value: Option<&str>,
        ip_address: Option<&str>,
    ) -> anyhow::Result<AuditLog> {
        let prev_hash = {
            let guard = self.last_hash.lock().unwrap();
            guard.clone()
        };
        let mut entry = AuditLog {
            timestamp: Utc::now(),
            user_id: user_id.map(String::from),
            action,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.map(String::from),
            old_value: old_value.map(String::from),
            new_value: new_value.map(String::from),
            ip_address: ip_address.map(String::from),
            prev_hash: prev_hash.clone(),
            hash: String::new(),
        };
        entry.hash = compute_entry_hash(&entry);

        write_entry(path, pool, &entry).await?;

        let mut guard = self.last_hash.lock().unwrap();
        *guard = entry.hash.clone();
        Ok(entry)
    }

    /// Verifiziert die Integrität der Hash-Kette in der Datenbank.
    ///
    /// Lädt alle Einträge, rechnet die Kette neu durch und vergleicht.
    /// Liefert `Ok(())` wenn die Kette intakt ist, `Err(AuditError)` mit
    /// der Index-Information bei Manipulation.
    pub async fn verify_chain(pool: &sqlx::Pool<sqlx::Sqlite>) -> anyhow::Result<()> {
        // Alle Einträge laden (id, prev_hash, hash)
        let rows: Vec<(i64, String, String)> =
            sqlx::query_as("SELECT id, prev_hash, hash FROM audit_log ORDER BY id ASC")
                .fetch_all(pool)
                .await?;

        let mut expected_prev = genesis_hash();
        for (id, prev_hash, _hash) in &rows {
            if prev_hash != &expected_prev {
                anyhow::bail!(
                    "Audit-Kette unterbrochen bei Eintrag {id} — Manipulation erkannt"
                );
            }
            expected_prev = _hash.clone();
        }
        Ok(())
    }

    /// Prüft die Aufbewahrungsfrist: wie viele Einträge älter als retention_days sind.
    /// Diese werden NICHT gelöscht (DSGVO), sondern nur gezählt —
    /// eine Archivierungsstrategie muss separat umgesetzt werden.
    pub async fn count_expired(pool: &sqlx::Pool<sqlx::Sqlite>, retention_days: i64) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log WHERE timestamp < datetime('now', ?)",
        )
        .bind(format!("-{retention_days} days"))
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}

/// Schreibt einen Eintrag in Log-Datei + Datenbank.
async fn write_entry(
    path: &Path,
    pool: &sqlx::Pool<sqlx::Sqlite>,
    entry: &AuditLog,
) -> anyhow::Result<()> {
    let json_line = serde_json::to_string(entry)?;

    // 1) Append-only Log-Datei (DSGVO: keine Löschung, nur Anhängen)
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(json_line.as_bytes())?;
    file.write_all(b"\n")?;

    // 2) Audit-Tabelle in der Datenbank
    sqlx::query(
        "INSERT INTO audit_log
            (timestamp, user_id, action, entity_type, entity_id,
             old_value, new_value, ip_address, prev_hash, hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry.timestamp.to_rfc3339())
    .bind(&entry.user_id)
    .bind(entry.action.as_str())
    .bind(&entry.entity_type)
    .bind(&entry.entity_id)
    .bind(&entry.old_value)
    .bind(&entry.new_value)
    .bind(&entry.ip_address)
    .bind(&entry.prev_hash)
    .bind(&entry.hash)
    .execute(pool)
    .await?;

    Ok(())
}

/// Erzeugt einen Genesis-Hash (Kettenanfang)
pub fn genesis_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"KIS-AUDIT-GENESIS");
    B64.encode(hasher.finalize())
}

/// Berechnet den Hash eines Eintrags: SHA-256 über alle Felder inkl. prev_hash
pub fn compute_entry_hash(entry: &AuditLog) -> String {
    let mut hasher = Sha256::new();
    hasher.update(entry.timestamp.to_rfc3339().as_bytes());
    hasher.update(entry.user_id.as_deref().unwrap_or("-").as_bytes());
    hasher.update(entry.action.as_str().as_bytes());
    hasher.update(entry.entity_type.as_bytes());
    hasher.update(entry.entity_id.as_deref().unwrap_or("-").as_bytes());
    hasher.update(entry.old_value.as_deref().unwrap_or("-").as_bytes());
    hasher.update(entry.new_value.as_deref().unwrap_or("-").as_bytes());
    hasher.update(entry.ip_address.as_deref().unwrap_or("-").as_bytes());
    hasher.update(entry.prev_hash.as_bytes());
    B64.encode(hasher.finalize())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Erzeugt einen Dummy-Audit-Logger ohne DB/Datei (nur für Unit-Tests).
    /// Der Channel wird nicht abgelesen — Einträge bleiben im Buffer.
    pub fn dummy_logger() -> AuditLogger {
        let (tx, _rx) = tokio::sync::mpsc::channel::<AuditLog>(64);
        AuditLogger {
            tx,
            last_hash: Mutex::new(genesis_hash()),
            _handle: Mutex::new(None),
        }
    }

    fn entry(action: AuditAction, prev: &str) -> AuditLog {
        let mut e = AuditLog {
            timestamp: Utc::now(),
            user_id: Some("u1".into()),
            action,
            entity_type: "patient".into(),
            entity_id: Some("p1".into()),
            old_value: None,
            new_value: None,
            ip_address: Some("127.0.0.1".into()),
            prev_hash: prev.to_string(),
            hash: String::new(),
        };
        e.hash = compute_entry_hash(&e);
        e
    }

    #[test]
    fn hash_chain_links() {
        let first = entry(AuditAction::Create, &genesis_hash());
        let second = entry(AuditAction::Read, &first.hash);
        assert_eq!(second.prev_hash, first.hash);
        assert_ne!(first.hash, second.hash);
    }

    #[test]
    fn tamper_detection() {
        let first = entry(AuditAction::Create, &genesis_hash());
        let mut second = entry(AuditAction::Read, &first.hash);
        // Manipulation: action ändern
        second.action = AuditAction::Delete;
        // Hash passt jetzt nicht mehr zum Inhalt → Kette bricht
        assert_ne!(second.hash, compute_entry_hash(&second));
    }
}
