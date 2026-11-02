//! Audit-Log (append-only) mit Hash-Kette gegen Manipulation
//!
//! Jeder Eintrag enthält den SHA-256-Hash des vorherigen Eintrags (prev_hash).
//! Inhaltsprüfung und Verkettung; für Schutz vor vollständigem Umschreiben
//! oder Abschneiden braucht der Betreiber einen getrennten externen Prüfanker.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

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
    pub fn parse(s: &str) -> Option<Self> {
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
    #[serde(default = "legacy_hash_version")]
    pub hash_version: u32,
}

fn legacy_hash_version() -> u32 {
    1
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

/// Ein serieller Writer je Instanz. Die Datenbank ist das maßgebliche Audit-Log.
/// Die JSONL-Datei ist eine ergänzende Kopie und kann bei Dateifehlern zurückliegen.
pub struct AuditLogger {
    pool: sqlx::SqlitePool,
    path: std::path::PathBuf,
    writer: tokio::sync::Mutex<()>,
}

impl AuditLogger {
    pub async fn start(path: &Path, pool: sqlx::SqlitePool) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                tokio::fs::create_dir_all(dir).await?;
            }
        }
        Self::verify_chain(&pool).await?;
        Ok(Self {
            pool,
            path: path.into(),
            writer: tokio::sync::Mutex::new(()),
        })
    }

    /// Wartet auf den DB-Commit. Kein stilles Verwerfen bei Überlast.
    #[allow(clippy::too_many_arguments)]
    pub async fn log(
        &self,
        user_id: Option<&str>,
        action: AuditAction,
        entity_type: &str,
        entity_id: Option<&str>,
        old_value: Option<&str>,
        new_value: Option<&str>,
        ip_address: Option<&str>,
    ) -> anyhow::Result<AuditLog> {
        let _writer = self.writer.lock().await;
        let mut tx = self.pool.begin().await?;
        // SQLx 0.7 beginnt SQLite-Transaktionen deferred. Vor dem Lesen den
        // Writer reservieren, damit parallele Session-Writes keinen veralteten
        // Read-Snapshot erzeugen. WHERE 0 verändert keine Audit-Einträge.
        sqlx::query("UPDATE audit_log SET current_hash = current_hash WHERE 0")
            .execute(&mut *tx)
            .await?;
        let previous: Option<String> =
            sqlx::query_scalar("SELECT current_hash FROM audit_log ORDER BY id DESC LIMIT 1")
                .fetch_optional(&mut *tx)
                .await?;
        let mut entry = AuditLog {
            timestamp: Utc::now(),
            user_id: user_id.map(String::from),
            action,
            entity_type: entity_type.into(),
            entity_id: entity_id.map(String::from),
            old_value: old_value.map(String::from),
            new_value: new_value.map(String::from),
            ip_address: ip_address.map(String::from),
            prev_hash: previous.unwrap_or_else(genesis_hash),
            hash: String::new(),
            hash_version: 2,
        };
        entry.hash = compute_entry_hash(&entry);
        sqlx::query("INSERT INTO audit_log
            (timestamp, user_id, action, entity_type, entity_id, old_value, new_value,
             ip_address, prev_hash, current_hash, hash_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(entry.timestamp.to_rfc3339()).bind(&entry.user_id).bind(entry.action.as_str())
            .bind(&entry.entity_type).bind(&entry.entity_id).bind(&entry.old_value).bind(&entry.new_value)
            .bind(&entry.ip_address).bind(&entry.prev_hash).bind(&entry.hash).bind(entry.hash_version)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        if let Err(error) = self.mirror(&entry).await {
            tracing::error!(%error, "Audit-Dateikopie fehlgeschlagen; DB-Eintrag ist gespeichert");
        }
        Ok(entry)
    }

    async fn mirror(&self, entry: &AuditLog) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(&line).await?;
        Ok(())
    }

    /// Prüft Inhalt und Verkettung. Ohne externen Prüfanker ist das Entfernen
    /// des Kettenendes oder vollständiges Neuberechnen durch DB-Admins nicht erkennbar.
    pub async fn verify_chain(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
        use sqlx::Row;
        let rows = sqlx::query("SELECT id, timestamp, user_id, action, entity_type, entity_id,
            old_value, new_value, ip_address, prev_hash, current_hash, hash_version FROM audit_log ORDER BY id")
            .fetch_all(pool).await?;
        let mut previous = genesis_hash();
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let action: String = row.try_get("action")?;
            let timestamp: String = row.try_get("timestamp")?;
            let entry = AuditLog {
                timestamp: DateTime::parse_from_rfc3339(&timestamp)?.with_timezone(&Utc),
                user_id: row.try_get("user_id")?,
                action: AuditAction::parse(&action)
                    .ok_or_else(|| anyhow::anyhow!("Unbekannte Audit-Aktion bei {id}"))?,
                entity_type: row.try_get("entity_type")?,
                entity_id: row.try_get("entity_id")?,
                old_value: row.try_get("old_value")?,
                new_value: row.try_get("new_value")?,
                ip_address: row.try_get("ip_address")?,
                prev_hash: row.try_get("prev_hash")?,
                hash: row.try_get("current_hash")?,
                hash_version: row.try_get("hash_version")?,
            };
            anyhow::ensure!(
                matches!(entry.hash_version, 1 | 2),
                "Unbekannte Hash-Version bei {id}"
            );
            anyhow::ensure!(
                entry.prev_hash == previous && entry.hash == compute_entry_hash(&entry),
                "Audit-Inhalt oder Kette manipuliert bei Eintrag {id}"
            );
            previous = entry.hash;
        }
        Ok(())
    }

    pub async fn count_expired(
        pool: &sqlx::SqlitePool,
        retention_days: i64,
    ) -> anyhow::Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_log WHERE julianday(timestamp) < julianday('now', ?)",
        )
        .bind(format!("-{retention_days} days"))
        .fetch_one(pool)
        .await?)
    }
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
    if entry.hash_version == 2 {
        hasher.update(b"KIS-AUDIT-V2");
        let timestamp = entry.timestamp.to_rfc3339();
        for value in [
            Some(timestamp.as_str()),
            entry.user_id.as_deref(),
            Some(entry.action.as_str()),
            Some(entry.entity_type.as_str()),
            entry.entity_id.as_deref(),
            entry.old_value.as_deref(),
            entry.new_value.as_deref(),
            entry.ip_address.as_deref(),
            Some(entry.prev_hash.as_str()),
        ] {
            match value {
                Some(text) => {
                    hasher.update([1]);
                    hasher.update((text.len() as u64).to_be_bytes());
                    hasher.update(text.as_bytes());
                }
                None => hasher.update([0]),
            }
        }
        return B64.encode(hasher.finalize());
    }
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
            hash_version: 2,
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
