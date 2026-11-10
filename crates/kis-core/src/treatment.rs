//! Behandlungszusammenhang: wer darf welchen Fall und welche Einheit sehen?
//!
//! Die Rolle regelt, *welche Art* von Daten jemand bearbeiten darf. Der
//! Behandlungszusammenhang regelt, *für welche Patienten*. Die zentrale
//! Middleware ruft [`check_case`] bzw. [`check_unit`] für jede Route mit
//! `:case_id` bzw. `:unit_id` auf — Fachmodule müssen (und können) das
//! nicht selbst prüfen.
//!
//! Zugriff auf einen Fall hat, wer
//! - einer Einheit zugeordnet ist, in der der Fall liegt oder bis vor
//!   kurzem lag (Fachabteilung oder Station; Zuordnung gilt auch für
//!   untergeordnete Einheiten; Nachlauf einstellbar),
//! - einer Einheit angehört, die mitbehandelt ([`add_participant`], z. B.
//!   über einen Auftrag),
//! - einen gültigen Notfallzugriff besitzt ([`grant_emergency_access`]),
//! - oder das Recht `case.access_all` hat.
//!
//! Die Prüfung ist über die Einstellung
//! `security.treatment_context.enforce` abschaltbar (kleine Häuser).
//! Alle Abfragen nutzen ausschließlich Kern-Tabellen.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::identity::AuthUser;
use crate::platform::Policies;
use crate::rbac::Permission;

/// Ergebnis einer Zugriffsprüfung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessCheck {
    Allowed,
    /// Fall bzw. Einheit existiert nicht
    NotFound,
    /// Existiert, aber kein Behandlungszusammenhang
    Denied,
}

/// Einheiten des Benutzers inklusive aller untergeordneten.
const MY_UNITS: &str = "WITH RECURSIVE mine AS (
        SELECT unit_id AS id FROM user_org_units
        WHERE user_id = $1 AND valid_from <= now()
          AND (valid_until IS NULL OR valid_until > now())
      UNION
        SELECT o.id FROM org_units o JOIN mine m ON o.parent_id = m.id
    )";

/// Darf der Benutzer den Fall sehen?
pub async fn check_case(
    pool: &PgPool,
    user: &AuthUser,
    case_id: Uuid,
    policies: &Policies,
) -> Result<AccessCheck, sqlx::Error> {
    let unrestricted = !policies.treatment_enforce || user.can(&Permission::CaseAccessAll);
    let Some(user_id) = user.uuid() else {
        return Ok(AccessCheck::Denied);
    };
    let sql = format!(
        "{MY_UNITS}
         SELECT
           EXISTS (SELECT 1 FROM cases WHERE id = $2) AS found,
           $3 OR EXISTS (
               SELECT 1 FROM case_stays s JOIN mine m ON m.id IN (s.department_id, s.ward_id)
               WHERE s.case_id = $2
                 AND (s.ended_at IS NULL OR s.ended_at > now() - make_interval(days => $4))
           ) OR EXISTS (
               SELECT 1 FROM case_participants p JOIN mine m ON m.id = p.unit_id
               WHERE p.case_id = $2 AND p.valid_from <= now()
                 AND (p.valid_until IS NULL OR p.valid_until > now() - make_interval(days => $4))
           ) OR EXISTS (
               SELECT 1 FROM emergency_access_grants g
               WHERE g.case_id = $2 AND g.user_id = $1 AND g.expires_at > now()
           ) AS allowed"
    );
    let (found, allowed): (bool, bool) = sqlx::query_as(&sql)
        .bind(user_id)
        .bind(case_id)
        .bind(unrestricted)
        .bind(policies.treatment_grace_days as i32)
        .fetch_one(pool)
        .await?;
    Ok(match (found, allowed) {
        (false, _) => AccessCheck::NotFound,
        (true, true) => AccessCheck::Allowed,
        (true, false) => AccessCheck::Denied,
    })
}

/// Darf der Benutzer die Arbeitslisten/Belegung der Einheit sehen?
/// Erlaubt bei Zuordnung zur Einheit oder einer übergeordneten, oder mit
/// dem Recht `unit.access_all`.
pub async fn check_unit(
    pool: &PgPool,
    user: &AuthUser,
    unit_id: Uuid,
    policies: &Policies,
) -> Result<AccessCheck, sqlx::Error> {
    let unrestricted = !policies.treatment_enforce || user.can(&Permission::UnitAccessAll);
    let Some(user_id) = user.uuid() else {
        return Ok(AccessCheck::Denied);
    };
    let sql = format!(
        "{MY_UNITS}
         SELECT EXISTS (SELECT 1 FROM org_units WHERE id = $2) AS found,
                $3 OR EXISTS (SELECT 1 FROM mine WHERE id = $2) AS allowed"
    );
    let (found, allowed): (bool, bool) = sqlx::query_as(&sql)
        .bind(user_id)
        .bind(unit_id)
        .bind(unrestricted)
        .fetch_one(pool)
        .await?;
    Ok(match (found, allowed) {
        (false, _) => AccessCheck::NotFound,
        (true, true) => AccessCheck::Allowed,
        (true, false) => AccessCheck::Denied,
    })
}

/// IDs der Einheiten, denen der Benutzer (direkt oder über eine
/// übergeordnete Einheit) angehört. Für Listen „meine Fälle/Aufträge“.
pub async fn unit_ids_of(pool: &PgPool, user: &AuthUser) -> Result<Vec<Uuid>, sqlx::Error> {
    let Some(user_id) = user.uuid() else {
        return Ok(Vec::new());
    };
    let sql = format!("{MY_UNITS} SELECT id FROM mine");
    sqlx::query_scalar(&sql).bind(user_id).fetch_all(pool).await
}

/// Trägt eine Einheit als mitbehandelnd ein (idempotent je `source`).
/// `source` benennt den Anlass, z. B. `order:<uuid>`.
pub async fn add_participant(
    tx: &mut PgConnection,
    case_id: Uuid,
    unit_id: Uuid,
    source: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO case_participants (case_id, unit_id, source) VALUES ($1, $2, $3)
         ON CONFLICT (case_id, unit_id, source) DO UPDATE SET valid_until = NULL",
    )
    .bind(case_id)
    .bind(unit_id)
    .bind(source)
    .execute(tx)
    .await?;
    Ok(())
}

/// Beendet die Mitbehandlung eines Anlasses (Zugriff läuft mit dem
/// eingestellten Nachlauf aus).
pub async fn end_participant(tx: &mut PgConnection, source: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE case_participants SET valid_until = now()
         WHERE source = $1 AND valid_until IS NULL",
    )
    .bind(source)
    .execute(tx)
    .await?;
    Ok(())
}

/// Gewährt einen Notfallzugriff und liefert die ID der Gewährung. Die
/// Begründung (mindestens 10 Zeichen) erzwingt die Datenbank. Der Aufrufer
/// schreibt den Audit-Eintrag in derselben Transaktion.
pub async fn grant_emergency_access(
    tx: &mut PgConnection,
    user_id: Uuid,
    case_id: Uuid,
    reason: &str,
    minutes: i64,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO emergency_access_grants (user_id, case_id, reason, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(mins => $4))
         RETURNING id",
    )
    .bind(user_id)
    .bind(case_id)
    .bind(reason.trim())
    .bind(minutes as i32)
    .fetch_one(tx)
    .await
}
