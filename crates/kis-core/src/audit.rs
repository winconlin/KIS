//! Audit-Trail: Hash-Kette mit transaktionalem Anhängen.
//!
//! Regeln für Fachmodule:
//! - Audit immer in derselben Transaktion wie die fachliche Änderung
//!   schreiben ([`append_audit`] oder [`crate::http::audit`]).
//! - Schlägt das Audit fehl, wird die Änderung verworfen. Kein
//!   „best effort“, kein `let _ =`.
//! - Keine Personendaten in `old_data`/`new_data` (nur Schlüssel,
//!   Feldnamen, Codes): der Trail ist append-only und überdauert eine
//!   Löschung nach Art. 17 DSGVO.

use sha2::{Digest, Sha256};

/// Genesis-Wert der Audit-Hash-Kette.
pub const AUDIT_GENESIS_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Schlüssel der transaktionsgebundenen Advisory-Lock, mit der alle
/// Audit-Schreiber serialisiert werden (`'KISAUDIT'` als i64).
const AUDIT_CHAIN_LOCK_KEY: i64 = 0x4b49_5341_5544_4954;

/// Hängt einen Eintrag an die Audit-Hash-Kette an.
///
/// Läuft auf der übergebenen Verbindung bzw. Transaktion. Eine
/// transaktionsgebundene Advisory-Lock serialisiert parallele Schreiber,
/// damit zwei Einträge nie denselben Vorgänger verwenden. Die Lock wird mit
/// dem Commit oder Rollback der umgebenden Transaktion freigegeben. Ohne
/// umgebende Transaktion wäre die Lock wirkungslos — deshalb immer
/// innerhalb einer Transaktion aufrufen.
#[allow(clippy::too_many_arguments)]
pub async fn append_audit(
    conn: &mut sqlx::PgConnection,
    user_id: &str,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    old_data: Option<&serde_json::Value>,
    new_data: Option<&serde_json::Value>,
    ip_address: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AUDIT_CHAIN_LOCK_KEY)
        .execute(&mut *conn)
        .await
        .map_err(|e| anyhow::anyhow!("Audit-Lock fehlgeschlagen: {e}"))?;

    let last =
        sqlx::query_scalar::<_, String>("SELECT hash FROM audit_logs ORDER BY id DESC LIMIT 1")
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| anyhow::anyhow!("Audit-Chain-Read fehlgeschlagen: {e}"))?;
    let prev_hash = last.unwrap_or_else(|| AUDIT_GENESIS_HASH.to_string());

    // PostgreSQL speichert TIMESTAMPTZ mit Mikrosekunden. Der Hash muss
    // über exakt den gespeicherten Wert gebildet werden, sonst schlägt die
    // spätere Verifikation fehl.
    let now = truncate_to_micros(chrono::Utc::now());
    let hash = audit_hash_v1(
        &prev_hash,
        user_id,
        action,
        &now,
        entity_type,
        entity_id,
        old_data,
        new_data,
    );

    sqlx::query(
        "INSERT INTO audit_logs \
         (user_id, action, entity_type, entity_id, old_data, new_data, \
          ip_address, timestamp, prev_hash, hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(user_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(old_data)
    .bind(new_data)
    .bind(ip_address)
    .bind(now)
    .bind(&prev_hash)
    .bind(&hash)
    .execute(&mut *conn)
    .await
    .map_err(|e| anyhow::anyhow!("Audit-Insert fehlgeschlagen: {e}"))?;

    tracing::debug!(user_id, action, entity_type, %hash, "Audit-Eintrag geschrieben");
    Ok(())
}

fn truncate_to_micros(ts: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    use chrono::Timelike;
    let micros = ts.nanosecond() / 1_000 * 1_000;
    ts.with_nanosecond(micros).unwrap_or(ts)
}

/// Berechnet den Kettenhash eines Audit-Eintrags.
///
/// Jedes Feld wird mit seiner Byte-Länge präfixiert. Damit können Felder,
/// die selbst `|` enthalten, keine Kollision mit anders aufgeteilten
/// Eingaben erzeugen.
#[allow(clippy::too_many_arguments)]
pub fn audit_hash_v1(
    prev_hash: &str,
    user_id: &str,
    action: &str,
    timestamp: &chrono::DateTime<chrono::Utc>,
    entity_type: &str,
    entity_id: Option<&str>,
    old_data: Option<&serde_json::Value>,
    new_data: Option<&serde_json::Value>,
) -> String {
    let ts = timestamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let old_str = old_data
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".into());
    let new_str = new_data
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".into());
    let mut hasher = Sha256::new();
    hasher.update(b"kis-audit-v1");
    for field in [
        prev_hash,
        user_id,
        action,
        ts.as_str(),
        entity_type,
        entity_id.unwrap_or(""),
        old_str.as_str(),
        new_str.as_str(),
    ] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    hex_sha256(&hasher.finalize())
}

pub(crate) fn hex_sha256(hash: &[u8]) -> String {
    let mut hex_str = String::with_capacity(hash.len() * 2);
    for b in hash {
        hex_str.push_str(&format!("{:02x}", b));
    }
    hex_str
}
