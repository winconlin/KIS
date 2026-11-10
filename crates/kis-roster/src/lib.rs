//! Fachmodul **Dienstplan**: Personal, Dienste, Abwesenheiten,
//! Wochenbesetzung, Prüfung und Export.
//!
//! Gilt für jede Berufsgruppe und jede Einheit — ärztlicher Dienst,
//! Pflege, Funktionsdienst, Küche, Technik. Was ein „Dienst“ ist (Kürzel,
//! Wochentage, Zeiten, zulässige Qualifikationen, Ruhezeit danach,
//! anrechenbare Stunden, Lohnart), legt jede Einheit als **Dienstart**
//! selbst fest.
//!
//! - **Plan** je Einheit und Zeitraum: Dienstarten, Besetzung,
//!   Abwesenheiten, Feiertage.
//! - **Prüfung** (`…/roster/validate`): unbesetzte Pflichtdienste,
//!   Doppelbesetzung, verletzte Ruhezeit, Dienst trotz Abwesenheit,
//!   unpassende Qualifikation. Die Prüfung meldet, sie verhindert nicht —
//!   die Entscheidung bleibt bei den Planenden.
//! - **Wochenbesetzung**: wer besetzt in welcher Woche welchen Platz
//!   (Station, Funktionsbereich), und wer ist noch nicht eingeteilt.
//! - **Export**: Stundenliste für die Zeitwirtschaft
//!   (`Personalnummer;Datum;Stunden;Lohnart`) und eigener Dienstkalender
//!   (iCalendar).
//!
//! Pläne sind an Einheiten gebunden (`/api/v1/units/:unit_id/roster/…`):
//! planen kann nur, wer der Einheit oder einer übergeordneten zugeordnet
//! ist.

mod export;
mod plan;
mod staff;

use kis_core::module::{Access, Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes};
use kis_core::rbac::Permission as P;

pub struct RosterModule;

impl Module for RosterModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "roster",
            title: "Dienstplan",
            description: "Personal, Dienstarten, Besetzung, Abwesenheiten, Prüfung, Export",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["org"],
            maturity: Maturity::Stable,
            kernel: false,
        }
    }

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {
        let read = Access::Permission(P::RosterRead);
        let write = Access::Permission(P::RosterWrite);
        ModuleRoutes::new()
            // Personal, Abwesenheiten, Feiertage (hausweit)
            .get("/api/v1/roster/staff", read, staff::list_staff)
            .post("/api/v1/roster/staff", write, staff::create_staff)
            .put("/api/v1/roster/staff/:id", write, staff::update_staff)
            .get("/api/v1/roster/absences", read, staff::list_absences)
            .post("/api/v1/roster/absences", write, staff::create_absence)
            .delete("/api/v1/roster/absences/:id", write, staff::delete_absence)
            .get("/api/v1/roster/holidays", read, staff::list_holidays)
            .put("/api/v1/roster/holidays/:date", write, staff::put_holiday)
            .delete(
                "/api/v1/roster/holidays/:date",
                write,
                staff::delete_holiday,
            )
            // Plan einer Einheit
            .get(
                "/api/v1/units/:unit_id/roster/duty-types",
                read,
                plan::list_duty_types,
            )
            .post(
                "/api/v1/units/:unit_id/roster/duty-types",
                write,
                plan::create_duty_type,
            )
            .put(
                "/api/v1/units/:unit_id/roster/duty-types/:id",
                write,
                plan::update_duty_type,
            )
            .get("/api/v1/units/:unit_id/roster/plan", read, plan::get_plan)
            .put(
                "/api/v1/units/:unit_id/roster/assignments",
                write,
                plan::put_assignment,
            )
            .delete(
                "/api/v1/units/:unit_id/roster/assignments/:id",
                write,
                plan::delete_assignment,
            )
            .get(
                "/api/v1/units/:unit_id/roster/validate",
                read,
                plan::validate,
            )
            // Wochenbesetzung
            .get(
                "/api/v1/units/:unit_id/roster/staffing",
                read,
                plan::get_staffing,
            )
            .post(
                "/api/v1/units/:unit_id/roster/positions",
                write,
                plan::create_position,
            )
            .put(
                "/api/v1/units/:unit_id/roster/staffing",
                write,
                plan::put_staffing,
            )
            // Export
            .get(
                "/api/v1/units/:unit_id/roster/timesheet.csv",
                read,
                export::timesheet_csv,
            )
            .get(
                "/api/v1/roster/my-duties.ics",
                Access::Authenticated,
                export::my_duties_ics,
            )
            .with_state(ctx.clone())
    }
}

/// Einheit `$1` mit allen untergeordneten Einheiten.
pub(crate) const UNIT_SUBTREE: &str = "WITH RECURSIVE sub AS (
        SELECT id FROM org_units WHERE id = $1
      UNION
        SELECT o.id FROM org_units o JOIN sub x ON o.parent_id = x.id)";

/// Zeitraum einer Abfrage, höchstens 400 Tage.
#[derive(serde::Deserialize)]
pub(crate) struct Range {
    pub from: chrono::NaiveDate,
    pub to: chrono::NaiveDate,
}

impl Range {
    pub(crate) fn check(&self) -> Result<(), kis_core::http::ApiFailure> {
        let days = (self.to - self.from).num_days();
        if !(0..=400).contains(&days) {
            return Err(kis_core::http::ApiFailure::bad_request(
                "Zeitraum: from ≤ to, höchstens 400 Tage",
            ));
        }
        Ok(())
    }
}
