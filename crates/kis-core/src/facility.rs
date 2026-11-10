//! Kernfunktionen für Betten.
//!
//! Betten gehören zum Kern (`beds`). Fachmodule lesen sie, ändern sie aber
//! nie direkt: Sperren und Freigeben läuft über diese Funktionen, damit
//! die Regeln an einer Stelle stehen. Beispiel: Das Auftragswesen sperrt
//! nach einer Entlassung das Bett bis zur Aufbereitung und gibt es mit der
//! Erledigung des Auftrags wieder frei.

use sqlx::PgConnection;
use uuid::Uuid;

/// Sperrt ein Bett mit Begründung. Ein bereits gesperrtes Bett behält
/// seine ursprüngliche Begründung. Liefert `false`, wenn es das Bett nicht
/// gibt.
pub async fn block_bed(
    tx: &mut PgConnection,
    bed_id: Uuid,
    reason: &str,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE beds SET blocked_reason = COALESCE(blocked_reason, $2),
                         updated_at = now(), version = version + 1
         WHERE id = $1",
    )
    .bind(bed_id)
    .bind(reason.trim())
    .execute(tx)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Hebt die Sperre eines Bettes auf (ohne Wirkung, wenn es nicht gesperrt
/// ist).
pub async fn release_bed(tx: &mut PgConnection, bed_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE beds SET blocked_reason = NULL, updated_at = now(), version = version + 1
         WHERE id = $1 AND blocked_reason IS NOT NULL",
    )
    .bind(bed_id)
    .execute(tx)
    .await?;
    Ok(())
}

/// Prüft, ob ein Bett für eine Belegung in Frage kommt, und sperrt die
/// Zeile bis zum Ende der Transaktion. Liefert die Station des Bettes.
/// Fehlertext für den Benutzer bei „nicht aktiv“ oder „gesperrt“.
pub async fn lock_bed_for_occupancy(
    tx: &mut PgConnection,
    bed_id: Uuid,
) -> Result<Result<Uuid, String>, sqlx::Error> {
    let row: Option<(Uuid, bool, bool, Option<String>)> = sqlx::query_as(
        "SELECT r.ward_id, b.active, r.active, b.blocked_reason
         FROM beds b JOIN rooms r ON r.id = b.room_id
         WHERE b.id = $1 FOR UPDATE OF b",
    )
    .bind(bed_id)
    .fetch_optional(tx)
    .await?;
    Ok(match row {
        None => Err("Bett nicht gefunden".to_string()),
        Some((_, false, _, _)) | Some((_, _, false, _)) => Err("Bett ist nicht aktiv".to_string()),
        Some((_, _, _, Some(reason))) => Err(format!("Bett ist gesperrt: {reason}")),
        Some((ward_id, true, true, None)) => Ok(ward_id),
    })
}
