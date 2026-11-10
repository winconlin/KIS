//! Dienstplan: Dienstarten, Besetzung, Prüfung, Wochenbesetzung, Export —
//! gegen echte PostgreSQL. Testgerüst: `tests/common/mod.rs`.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use common::id_of;

const WEEK: &str = "from=2026-11-16&to=2026-11-22";

fn kinds(validation: &Value) -> Vec<(String, String, String)> {
    validation["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().unwrap().to_string(),
                i["dutyDate"].as_str().unwrap().to_string(),
                i["dutyCode"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn roster_planning_validation_and_export() {
    let app = app!();
    let (_, admin) = app.session("admin1", "admin").await;
    let mut units = Vec::new();
    for (kind, code, name) in [
        ("fachabteilung", "INN", "Innere Medizin"),
        ("fachabteilung", "CHI", "Chirurgie"),
    ] {
        let v = app
            .ok(
                "POST",
                "/api/v1/org/units",
                &admin,
                Some(json!({ "kind": kind, "code": code, "name": name })),
            )
            .await;
        units.push(id_of(&v));
    }
    let (internal, surgery) = (units[0], units[1]);
    let (planner_id, planner) = app.session("planer1", "dienstplaner").await;
    app.assign(planner_id, internal).await;
    let (physician_id, physician) = app.session("arzt1", "arzt").await;
    app.assign(physician_id, internal).await;

    // Personal
    let a = app
        .ok(
            "POST",
            "/api/v1/roster/staff",
            &planner,
            Some(json!({
                "displayName": "Dr. A", "personnelNumber": "p1001", "qualification": "aa",
                "unitId": internal, "userId": physician_id,
            })),
        )
        .await;
    assert_eq!(a["personnelNumber"], "P1001");
    assert_eq!(a["qualification"], "AA");
    let b = app
        .ok(
            "POST",
            "/api/v1/roster/staff",
            &planner,
            Some(json!({ "displayName": "Dr. B", "qualification": "OA", "unitId": internal })),
        )
        .await;
    let (a_id, b_id) = (id_of(&a), id_of(&b));

    // Dienstarten der Abteilung
    let base = format!("/api/v1/units/{internal}/roster");
    let bd = app
        .ok(
            "POST",
            &format!("{base}/duty-types"),
            &planner,
            Some(json!({
                "code": "bd", "name": "Bereitschaftsdienst", "qualifications": ["aa"],
                "blocksNextDay": true, "startTime": "08:00:00", "durationHours": 24.0,
                "creditedHours": 16.0, "weekendCreditedHours": 20.0, "wageType": "bd",
            })),
        )
        .await;
    assert_eq!(bd["code"], "BD");
    let vis = app
        .ok(
            "POST",
            &format!("{base}/duty-types"),
            &planner,
            Some(json!({
                "code": "VIS", "name": "Visitendienst", "weekdays": [6, 7],
                "startTime": "09:00:00", "durationHours": 4.0, "creditedHours": 4.0, "wageType": "VD",
            })),
        )
        .await;
    let (bd_id, vis_id) = (id_of(&bd), id_of(&vis));
    // unzulässiger Wochentag
    app.fails(
        StatusCode::BAD_REQUEST,
        "POST",
        &format!("{base}/duty-types"),
        &planner,
        Some(
            json!({ "code": "X", "name": "x", "weekdays": [8], "startTime": "08:00:00",
                     "durationHours": 8.0, "creditedHours": 8.0 }),
        ),
    )
    .await;

    // Feiertag am Mittwoch
    app.ok(
        "PUT",
        "/api/v1/roster/holidays/2026-11-18",
        &planner,
        Some(json!({ "name": "Buß- und Bettag" })),
    )
    .await;

    // Leerer Plan: jeder fällige Pflichtdienst ist unbesetzt.
    // BD täglich (7), VIS am Wochenende und am Feiertag (3).
    let validation = app
        .ok("GET", &format!("{base}/validate?{WEEK}"), &planner, None)
        .await;
    assert_eq!(validation["ok"], false);
    assert_eq!(validation["count"], 10, "{validation}");
    assert!(kinds(&validation).contains(&("unbesetzt".into(), "2026-11-18".into(), "VIS".into())));

    // Besetzen — mit allen Konfliktarten
    let assign = |duty: Uuid, date: &'static str, staff: Uuid| {
        let (app, base, planner) = (&app, &base, &planner);
        async move {
            app.ok(
                "PUT",
                &format!("{base}/assignments"),
                planner,
                Some(json!({ "dutyTypeId": duty, "dutyDate": date, "staffId": staff })),
            )
            .await
        }
    };
    assign(bd_id, "2026-11-16", a_id).await;
    assign(bd_id, "2026-11-17", a_id).await; // Ruhezeit nach Montag
    assign(bd_id, "2026-11-18", b_id).await; // OA im AA-Dienst
    app.ok(
        "POST",
        "/api/v1/roster/absences",
        &planner,
        Some(json!({ "staffId": a_id, "kind": "urlaub", "fromDate": "2026-11-19", "toDate": "2026-11-20" })),
    )
    .await;
    assign(bd_id, "2026-11-19", a_id).await; // trotz Urlaub
    assign(bd_id, "2026-11-21", a_id).await;
    assign(vis_id, "2026-11-21", a_id).await; // doppelt am Samstag
    assign(vis_id, "2026-11-22", a_id).await; // Ruhezeit nach Samstag

    let validation = app
        .ok("GET", &format!("{base}/validate?{WEEK}"), &planner, None)
        .await;
    let mut found = kinds(&validation);
    found.sort();
    let expected: Vec<(String, String, String)> = [
        ("abwesend", "2026-11-19", "BD"),
        ("doppelt", "2026-11-21", "BD + VIS"),
        ("qualifikation", "2026-11-18", "BD"),
        ("ruhezeit", "2026-11-17", "BD"),
        ("ruhezeit", "2026-11-22", "VIS"),
        ("unbesetzt", "2026-11-18", "VIS"),
        ("unbesetzt", "2026-11-20", "BD"),
        ("unbesetzt", "2026-11-22", "BD"),
    ]
    .iter()
    .map(|(k, d, c)| (k.to_string(), d.to_string(), c.to_string()))
    .collect();
    assert_eq!(found, expected, "{validation}");

    // Besetzung ersetzen statt verdoppeln
    assign(bd_id, "2026-11-30", b_id).await;
    assign(bd_id, "2026-11-30", a_id).await;
    let plan = app
        .ok(
            "GET",
            &format!("{base}/plan?from=2026-11-30&to=2026-11-30"),
            &planner,
            None,
        )
        .await;
    assert_eq!(plan["assignments"].as_array().unwrap().len(), 1);
    assert_eq!(plan["assignments"][0]["displayName"], "Dr. A");
    let plan = app
        .ok("GET", &format!("{base}/plan?{WEEK}"), &physician, None)
        .await;
    assert_eq!(plan["assignments"].as_array().unwrap().len(), 7);
    assert_eq!(plan["absences"].as_array().unwrap().len(), 1);
    assert_eq!(plan["holidays"][0]["name"], "Buß- und Bettag");

    // Zuständigkeit: nur die eigene Einheit, nur mit Schreibrecht
    app.fails(
        StatusCode::FORBIDDEN,
        "GET",
        &format!("/api/v1/units/{surgery}/roster/plan?{WEEK}"),
        &planner,
        None,
    )
    .await;
    app.fails(
        StatusCode::FORBIDDEN,
        "PUT",
        &format!("{base}/assignments"),
        &physician,
        Some(json!({ "dutyTypeId": bd_id, "dutyDate": "2026-11-20", "staffId": b_id })),
    )
    .await;
    let foreign = app
        .ok(
            "POST",
            &format!("/api/v1/units/{surgery}/roster/duty-types"),
            &admin,
            Some(
                json!({ "code": "BD", "name": "Chirurgischer Dienst", "startTime": "08:00:00",
                         "durationHours": 24.0, "creditedHours": 16.0 }),
            ),
        )
        .await;
    app.fails(
        StatusCode::NOT_FOUND,
        "PUT",
        &format!("{base}/assignments"),
        &planner,
        Some(json!({ "dutyTypeId": id_of(&foreign), "dutyDate": "2026-11-20", "staffId": b_id })),
    )
    .await;

    // Stundenexport: unvollständige Stammdaten werden gemeldet, nicht übergangen
    let export = format!("{base}/timesheet.csv?{WEEK}");
    let v = app
        .fails(StatusCode::CONFLICT, "GET", &export, &planner, None)
        .await;
    assert!(v["error"].as_str().unwrap().contains("Dr. B"), "{v}");
    app.ok(
        "PUT",
        &format!("/api/v1/roster/staff/{b_id}"),
        &planner,
        Some(
            json!({ "displayName": "Dr. B", "qualification": "OA", "unitId": internal,
                     "personnelNumber": "P2002", "version": b["version"] }),
        ),
    )
    .await;
    let (s, csv) = app.raw("GET", &export, Some(&planner), None).await;
    assert_eq!(s, StatusCode::OK);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "Personalnummer;Datum;Stunden;Lohnart");
    assert_eq!(lines.len(), 8, "{csv}");
    assert!(
        lines.contains(&"P1001;16.11.2026;16,0;BD"),
        "Werktag: {csv}"
    );
    assert!(
        lines.contains(&"P2002;18.11.2026;20,0;BD"),
        "Feiertag: {csv}"
    );
    assert!(
        lines.contains(&"P1001;21.11.2026;20,0;BD"),
        "Samstag: {csv}"
    );
    assert!(lines.contains(&"P1001;21.11.2026;4,0;VD"), "{csv}");

    // Eigener Dienstkalender
    let (s, ics) = app
        .raw(
            "GET",
            &format!("/api/v1/roster/my-duties.ics?{WEEK}"),
            Some(&physician),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ics.matches("BEGIN:VEVENT").count(), 6, "{ics}");
    assert!(ics.contains("DTSTART:20261116T080000"));
    assert!(ics.contains("DTEND:20261117T080000"));
    assert!(ics.contains("SUMMARY:BD Bereitschaftsdienst"));
    let (_, ics) = app
        .raw(
            "GET",
            &format!("/api/v1/roster/my-duties.ics?{WEEK}"),
            Some(&planner),
            None,
        )
        .await;
    assert_eq!(
        ics.matches("BEGIN:VEVENT").count(),
        0,
        "Planer hat keine eigenen Dienste"
    );

    // Wochenbesetzung
    let position = app
        .ok(
            "POST",
            &format!("{base}/positions"),
            &planner,
            Some(json!({ "code": "ST1-1", "name": "Stationsarzt 1", "category": "Station 1" })),
        )
        .await;
    let staffing =
        json!({ "positionId": id_of(&position), "weekStart": "2026-11-16", "staffId": a_id });
    app.ok("PUT", &format!("{base}/staffing"), &planner, Some(staffing))
        .await;
    app.fails(
        StatusCode::BAD_REQUEST,
        "PUT",
        &format!("{base}/staffing"),
        &planner,
        Some(json!({ "positionId": id_of(&position), "weekStart": "2026-11-17", "staffId": a_id })),
    )
    .await;
    let matrix = app
        .ok(
            "GET",
            &format!("{base}/staffing?weekStart=2026-11-16"),
            &planner,
            None,
        )
        .await;
    assert_eq!(matrix["positions"][0]["displayName"], "Dr. A");
    assert_eq!(matrix["unassigned"].as_array().unwrap().len(), 1);
    assert_eq!(matrix["unassigned"][0]["displayName"], "Dr. B");
    app.ok(
        "PUT",
        &format!("{base}/staffing"),
        &planner,
        Some(json!({ "positionId": id_of(&position), "weekStart": "2026-11-16" })),
    )
    .await;
    let matrix = app
        .ok(
            "GET",
            &format!("{base}/staffing?weekStart=2026-11-16"),
            &planner,
            None,
        )
        .await;
    assert!(matrix["positions"][0]["staffId"].is_null());
    assert_eq!(matrix["unassigned"].as_array().unwrap().len(), 2);
    assert_eq!(
        matrix["unassigned"][0]["absentThisWeek"], true,
        "Dr. A hat Urlaub"
    );

    // Besetzung löschen
    let id = plan["assignments"][0]["id"].as_str().unwrap();
    app.ok(
        "DELETE",
        &format!("{base}/assignments/{id}"),
        &planner,
        None,
    )
    .await;
    app.fails(
        StatusCode::NOT_FOUND,
        "DELETE",
        &format!("{base}/assignments/{id}"),
        &planner,
        None,
    )
    .await;

    app.finish().await;
}
