//! Exporte: Stundenliste für die Zeitwirtschaft und eigener Dienstkalender.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use kis_core::http::{actor_id, ApiFailure};
use kis_core::identity::CurrentUser;
use kis_core::module::ModuleContext;

use crate::{Range, UNIT_SUBTREE};

/// Stundenliste der Einheit als CSV: `Personalnummer;Datum;Stunden;Lohnart`
/// (Datum `TT.MM.JJJJ`, Stunden mit Dezimalkomma). An Samstagen, Sonntagen
/// und Feiertagen gelten die abweichenden anrechenbaren Stunden, falls
/// die Dienstart welche nennt.
///
/// Fehlt bei einer eingeplanten Person die Personalnummer, wird nichts
/// exportiert, sondern die Lücke gemeldet — eine unvollständige Liste
/// würde in der Zeitwirtschaft nicht auffallen.
pub(crate) async fn timesheet_csv(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Path(unit_id): Path<Uuid>,
    Query(range): Query<Range>,
) -> Result<Response, ApiFailure> {
    range.check()?;
    let rows: Vec<(Option<String>, String, String, String, String)> = sqlx::query_as(&format!(
        "{UNIT_SUBTREE}
         SELECT s.personnel_number, s.display_name,
                to_char(a.duty_date, 'DD.MM.YYYY'),
                replace(trim(to_char(
                    CASE WHEN extract(isodow FROM a.duty_date) >= 6 OR h.holiday_date IS NOT NULL
                         THEN COALESCE(t.weekend_credited_hours, t.credited_hours)
                         ELSE t.credited_hours END, 'FM990.0')), '.', ','),
                t.wage_type
         FROM duty_assignments a
         JOIN duty_types t ON t.id = a.duty_type_id
         JOIN staff_members s ON s.id = a.staff_id
         LEFT JOIN public_holidays h ON h.holiday_date = a.duty_date
         WHERE t.unit_id IN (SELECT id FROM sub) AND a.duty_date BETWEEN $2 AND $3
         ORDER BY a.duty_date, s.personnel_number, t.code"
    ))
    .bind(unit_id)
    .bind(range.from)
    .bind(range.to)
    .fetch_all(ctx.pool())
    .await?;

    let mut missing: Vec<&str> = rows
        .iter()
        .filter(|r| r.0.is_none())
        .map(|r| r.1.as_str())
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if !missing.is_empty() {
        return Err(ApiFailure::conflict(format!(
            "Export nicht möglich: Personalnummer fehlt bei {}",
            missing.join(", ")
        )));
    }

    let mut csv = String::from("Personalnummer;Datum;Stunden;Lohnart\r\n");
    for (number, _, date, hours, wage_type) in &rows {
        csv.push_str(&format!(
            "{};{date};{hours};{wage_type}\r\n",
            number.as_deref().unwrap_or_default()
        ));
    }
    // Der Export personenbezogener Arbeitszeitdaten wird protokolliert.
    ctx.security
        .log_audit(
            ctx.pool(),
            &user.auth.user_id,
            "roster.timesheet_export",
            "org_unit",
            Some(&unit_id.to_string()),
            None,
            Some(&serde_json::json!({ "from": range.from, "to": range.to, "rows": rows.len() })),
            None,
        )
        .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"stunden_{}_{}.csv\"",
                    range.from, range.to
                ),
            ),
        ],
        csv,
    )
        .into_response())
}

/// Eigene Dienste als iCalendar-Datei (für den persönlichen Kalender).
/// Zeiten sind Ortszeit des Hauses („floating“), wie im Dienstplan
/// eingetragen.
pub(crate) async fn my_duties_ics(
    State(ctx): State<ModuleContext>,
    user: CurrentUser,
    Query(range): Query<Range>,
) -> Result<Response, ApiFailure> {
    range.check()?;
    let rows: Vec<(Uuid, String, String, String, String)> = sqlx::query_as(
        "SELECT a.id, t.code || ' ' || t.name, u.name,
                to_char(x.starts, 'YYYYMMDD\"T\"HH24MISS'),
                to_char(x.starts + make_interval(mins => (x.hours * 60)::int), 'YYYYMMDD\"T\"HH24MISS')
         FROM duty_assignments a
         JOIN duty_types t ON t.id = a.duty_type_id
         JOIN org_units u ON u.id = t.unit_id
         JOIN staff_members s ON s.id = a.staff_id
         LEFT JOIN public_holidays h ON h.holiday_date = a.duty_date
         CROSS JOIN LATERAL (
             SELECT CASE WHEN (extract(isodow FROM a.duty_date) >= 6 OR h.holiday_date IS NOT NULL)
                              AND t.weekend_start_time IS NOT NULL
                         THEN a.duty_date + t.weekend_start_time
                         ELSE a.duty_date + t.start_time END AS starts,
                    CASE WHEN (extract(isodow FROM a.duty_date) >= 6 OR h.holiday_date IS NOT NULL)
                              AND t.weekend_duration_hours IS NOT NULL
                         THEN t.weekend_duration_hours
                         ELSE t.duration_hours END AS hours) x
         WHERE s.user_id = $1 AND a.duty_date BETWEEN $2 AND $3
         ORDER BY a.duty_date",
    )
    .bind(actor_id(&user.auth)?)
    .bind(range.from)
    .bind(range.to)
    .fetch_all(ctx.pool())
    .await?;

    let mut ics =
        String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//KIS//Dienstplan//DE\r\n");
    for (id, summary, unit, start, end) in &rows {
        ics.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:{id}@kis\r\nDTSTAMP:{start}\r\nDTSTART:{start}\r\nDTEND:{end}\r\nSUMMARY:{}\r\nLOCATION:{}\r\nEND:VEVENT\r\n",
            ics_text(summary),
            ics_text(unit)
        ));
    }
    ics.push_str("END:VCALENDAR\r\n");
    Ok((
        [
            (header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"dienste.ics\"",
            ),
        ],
        ics,
    )
        .into_response())
}

/// Maskiert Text nach RFC 5545 (Backslash, Semikolon, Komma, Zeilenende).
fn ics_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str(r"\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ics_text_is_escaped() {
        assert_eq!(ics_text("AA; Visite, 7\nB\\C"), r"AA\; Visite\, 7\nB\\C");
    }
}
