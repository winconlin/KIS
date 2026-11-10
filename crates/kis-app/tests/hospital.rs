//! Ein Krankenhaustag über alle Berufsgruppen, gegen echte PostgreSQL.
//!
//! Der Test spielt den Weg eines Patienten durch das Haus nach und prüft
//! an jeder Stelle, dass die Bereiche mit denselben Daten arbeiten und
//! jeder nur sieht, was er für seine Arbeit braucht:
//! Aufnahme → Station → Diagnostik → Labor → Küche → Verlegung →
//! Entlassung → Bettenaufbereitung → Schnittstelle.
//!
//! Testgerüst: `tests/common/mod.rs`. Alle Daten sind synthetisch.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use common::{id_of, App};

/// Kleines Haus: zwei Fachabteilungen mit je einer Station, Radiologie,
/// Reinigung, Küche — und je Berufsgruppe ein angemeldeter Benutzer.
struct Hospital {
    admin: String,
    reception: String,
    physician: String,
    physician_id: Uuid,
    nurse: String,
    surgeon: String,
    radiology: String,
    cleaning: String,
    kitchen: String,
    interface: String,
    auditor: String,
    internal: Uuid,
    surgery: Uuid,
    ward1: Uuid,
    ward2: Uuid,
    radiology_unit: Uuid,
    cleaning_unit: Uuid,
    bed1: Uuid,
    bed2: Uuid,
    bed_surgery: Uuid,
}

async fn unit(
    app: &App,
    admin: &str,
    kind: &str,
    code: &str,
    name: &str,
    parent: Option<Uuid>,
) -> Uuid {
    let v = app
        .ok(
            "POST",
            "/api/v1/org/units",
            admin,
            Some(json!({ "kind": kind, "code": code, "name": name, "parentId": parent })),
        )
        .await;
    id_of(&v)
}

async fn bed(app: &App, admin: &str, ward: Uuid, room: &str, label: &str) -> Uuid {
    let (s, r) = app
        .call(
            "POST",
            "/api/v1/org/rooms",
            Some(admin),
            Some(json!({ "wardId": ward, "name": room })),
        )
        .await;
    let room_id = if s == StatusCode::OK {
        id_of(&r)
    } else {
        // Zimmer existiert bereits
        let rooms = app
            .ok(
                "GET",
                &format!("/api/v1/org/wards/{ward}/rooms"),
                admin,
                None,
            )
            .await;
        let found = rooms["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == room)
            .expect("Zimmer vorhanden");
        id_of(found)
    };
    let b = app
        .ok(
            "POST",
            "/api/v1/org/beds",
            admin,
            Some(json!({ "roomId": room_id, "label": label })),
        )
        .await;
    id_of(&b)
}

async fn hospital(app: &App) -> Hospital {
    let (_, admin) = app.session("admin1", "admin").await;
    let house = unit(app, &admin, "einrichtung", "KH", "Testklinik", None).await;
    let internal = unit(
        app,
        &admin,
        "fachabteilung",
        "INN",
        "Innere Medizin",
        Some(house),
    )
    .await;
    let surgery = unit(
        app,
        &admin,
        "fachabteilung",
        "CHI",
        "Chirurgie",
        Some(house),
    )
    .await;
    let ward1 = unit(app, &admin, "station", "ST1", "Station 1", Some(internal)).await;
    let ward2 = unit(app, &admin, "station", "ST2", "Station 2", Some(surgery)).await;
    let radiology_unit = unit(
        app,
        &admin,
        "funktionsbereich",
        "RAD",
        "Radiologie",
        Some(house),
    )
    .await;
    let cleaning_unit = unit(app, &admin, "versorgung", "REIN", "Reinigung", Some(house)).await;
    let bed1 = bed(app, &admin, ward1, "Zimmer 1", "A").await;
    let bed2 = bed(app, &admin, ward1, "Zimmer 1", "B").await;
    let bed_surgery = bed(app, &admin, ward2, "Zimmer 1", "A").await;

    let (_, reception) = app.session("aufnahme1", "rezeption").await;
    let (physician_id, physician) = app.session("arzt1", "arzt").await;
    app.assign(physician_id, internal).await;
    let (nurse_id, nurse) = app.session("pflege1", "pflege").await;
    app.assign(nurse_id, ward1).await;
    let (surgeon_id, surgeon) = app.session("arzt2", "arzt").await;
    app.assign(surgeon_id, surgery).await;
    let (rad_id, radiology) = app.session("mtr1", "funktionsdienst").await;
    app.assign(rad_id, radiology_unit).await;
    let (clean_id, cleaning) = app.session("reinigung1", "hauswirtschaft").await;
    app.assign(clean_id, cleaning_unit).await;
    let (_, kitchen) = app.session("kueche1", "kueche").await;
    let (_, interface) = app.session("lis1", "schnittstelle").await;
    let (_, auditor) = app.session("revision1", "auditor").await;

    Hospital {
        admin,
        reception,
        physician,
        physician_id,
        nurse,
        surgeon,
        radiology,
        cleaning,
        kitchen,
        interface,
        auditor,
        internal,
        surgery,
        ward1,
        ward2,
        radiology_unit,
        cleaning_unit,
        bed1,
        bed2,
        bed_surgery,
    }
}

async fn patient(app: &App, token: &str, family: &str) -> Uuid {
    let v = app
        .ok(
            "POST",
            "/api/v1/patients",
            token,
            Some(json!({ "nameFamily": family, "nameGiven": "Test", "birthDate": "1950-03-04" })),
        )
        .await;
    id_of(&v)
}

async fn admit(app: &App, h: &Hospital, patient_id: Uuid, bed_id: Uuid) -> Value {
    app.ok(
        "POST",
        "/api/v1/cases",
        &h.reception,
        Some(json!({
            "patientId": patient_id, "kind": "stationaer",
            "departmentId": h.internal, "wardId": h.ward1, "bedId": bed_id,
            "admissionReason": "Thoraxschmerz",
        })),
    )
    .await
}

fn count(v: &Value) -> i64 {
    v["count"]
        .as_i64()
        .unwrap_or_else(|| panic!("count fehlt in {v}"))
}

#[tokio::test]
async fn a_patient_journey_across_all_professions() {
    let app = app!();
    let h = hospital(&app).await;

    // Kataloge des Hauses: Leistungen, Automatik, Kostformen
    let xray = id_of(
        &app.ok(
            "POST",
            "/api/v1/catalog/services",
            &h.admin,
            Some(json!({
                "code": "RX-THORAX", "name": "Röntgen Thorax", "category": "bildgebung",
                "providerUnitId": h.radiology_unit, "requiresResult": true,
            })),
        )
        .await,
    );
    let bed_prep = id_of(
        &app.ok(
            "POST",
            "/api/v1/catalog/services",
            &h.admin,
            Some(json!({
                "code": "BETT-AUFB", "name": "Bettenaufbereitung", "category": "reinigung",
                "providerUnitId": h.cleaning_unit, "requiresCase": false,
            })),
        )
        .await,
    );
    app.ok(
        "POST",
        "/api/v1/catalog/automation-rules",
        &h.admin,
        Some(json!({
            "name": "Aufbereitung nach Entlassung", "onTopic": "case.discharged",
            "serviceId": bed_prep, "blockBed": true,
        })),
    )
    .await;
    app.ok(
        "POST",
        "/api/v1/meals/diets",
        &h.admin,
        Some(json!({ "code": "VK", "name": "Vollkost", "isDefault": true })),
    )
    .await;
    let diabetic = id_of(
        &app.ok(
            "POST",
            "/api/v1/meals/diets",
            &h.admin,
            Some(json!({ "code": "DIAB", "name": "Diabeteskost" })),
        )
        .await,
    );

    // --- Patientenservice: Aufnahme -----------------------------------------
    let pid = patient(&app, &h.reception, "Musterfrau").await;
    let case = admit(&app, &h, pid, h.bed1).await;
    let cid = id_of(&case);
    let case_path = format!("/api/v1/cases/{cid}");
    let number = case["caseNumber"].as_str().unwrap();
    assert_eq!(number.len(), 12, "Fallnummer JJJJ-NNNNNNN: {number}");
    assert_eq!(case["wardName"], "Station 1");
    assert_eq!(case["bedLabel"], "A");

    // Regeln der Datenbank: ein offener stationärer Fall je Patient, ein Fall je Bett
    let (s, v) = app
        .call(
            "POST",
            "/api/v1/cases",
            Some(&h.reception),
            Some(json!({ "patientId": pid, "kind": "stationaer", "departmentId": h.internal })),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    let other = patient(&app, &h.reception, "Zweitpatient").await;
    let (s, v) = app
        .call(
            "POST",
            "/api/v1/cases",
            Some(&h.reception),
            Some(json!({
                "patientId": other, "kind": "stationaer", "departmentId": h.internal,
                "wardId": h.ward1, "bedId": h.bed1,
            })),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "belegtes Bett: {v}");
    // Station muss eine Station sein, nicht irgendeine Einheit
    let (s, _) = app
        .call(
            "POST",
            "/api/v1/cases",
            Some(&h.reception),
            Some(json!({
                "patientId": other, "kind": "stationaer", "departmentId": h.internal,
                "wardId": h.radiology_unit,
            })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // --- Behandlungszusammenhang --------------------------------------------
    app.ok("GET", &case_path, &h.physician, None).await; // Abteilung
    app.ok("GET", &case_path, &h.nurse, None).await; // Station
    let v = app
        .fails(StatusCode::FORBIDDEN, "GET", &case_path, &h.surgeon, None)
        .await;
    assert_eq!(v["code"], "no_treatment_context");
    app.fails(StatusCode::FORBIDDEN, "GET", &case_path, &h.radiology, None)
        .await;
    app.fails(StatusCode::FORBIDDEN, "GET", &case_path, &h.kitchen, None)
        .await;
    app.fails(
        StatusCode::NOT_FOUND,
        "GET",
        &format!("/api/v1/cases/{}", Uuid::new_v4()),
        &h.physician,
        None,
    )
    .await;

    // Stationsliste: Station sieht ihre Fälle, die Abteilung auch, Fremde nicht
    let census = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/census", h.ward1),
            &h.nurse,
            None,
        )
        .await;
    assert_eq!(count(&census), 1);
    assert_eq!(census["items"][0]["nameFamily"], "Musterfrau");
    let census = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/census", h.internal),
            &h.physician,
            None,
        )
        .await;
    assert_eq!(count(&census), 1);
    let v = app
        .fails(
            StatusCode::FORBIDDEN,
            "GET",
            &format!("/api/v1/units/{}/census", h.ward1),
            &h.surgeon,
            None,
        )
        .await;
    assert_eq!(v["code"], "not_member_of_unit");

    // --- Warnhinweise (Kern): einmal erfasst, überall wirksam -----------------
    app.ok(
        "POST",
        &format!("/api/v1/patients/{pid}/alerts"),
        &h.nurse,
        Some(json!({ "kind": "nahrungsmittelallergie", "text": "Erdnuss" })),
    )
    .await;
    app.ok(
        "POST",
        &format!("/api/v1/patients/{pid}/alerts"),
        &h.physician,
        Some(json!({ "kind": "allergie", "text": "Penicillin" })),
    )
    .await;
    let v = app.ok("GET", &case_path, &h.nurse, None).await;
    assert_eq!(v["alerts"].as_array().unwrap().len(), 2);

    // --- Ärztlicher Dienst: Diagnosen und Dokumentation -----------------------
    let dx = format!("{case_path}/diagnoses");
    app.ok(
        "POST",
        &dx,
        &h.physician,
        Some(json!({ "kind": "haupt", "icdCode": "i21.0", "text": "Akuter Vorderwandinfarkt", "certainty": "G" })),
    )
    .await;
    app.fails(
        StatusCode::CONFLICT,
        "POST",
        &dx,
        &h.physician,
        Some(json!({ "kind": "haupt", "icdCode": "I10", "text": "Hypertonie" })),
    )
    .await;
    app.fails(
        StatusCode::BAD_REQUEST,
        "POST",
        &dx,
        &h.physician,
        Some(json!({ "kind": "neben", "icdCode": "kein-kode", "text": "x" })),
    )
    .await;
    app.fails(
        StatusCode::FORBIDDEN,
        "POST",
        &dx,
        &h.nurse,
        Some(json!({ "kind": "neben", "icdCode": "I10", "text": "Hypertonie" })),
    )
    .await;

    let notes = format!("{case_path}/notes");
    let draft = app
        .ok(
            "POST",
            &notes,
            &h.physician,
            Some(json!({ "kind": "verlauf", "body": "Beschwerdefrei, EKG unverändert." })),
        )
        .await;
    let note_id = id_of(&draft);
    assert_eq!(draft["status"], "entwurf");
    // Entwürfe sieht nur der Autor
    assert_eq!(count(&app.ok("GET", &notes, &h.nurse, None).await), 0);
    let signed = app
        .ok(
            "POST",
            &format!("{notes}/{note_id}/sign"),
            &h.physician,
            Some(json!({ "version": draft["version"] })),
        )
        .await;
    assert_eq!(signed["status"], "freigegeben");
    assert_eq!(count(&app.ok("GET", &notes, &h.nurse, None).await), 1);
    // Freigegebenes ist unveränderlich — über die API und direkt in der Datenbank
    app.fails(
        StatusCode::CONFLICT,
        "PUT",
        &format!("{notes}/{note_id}"),
        &h.physician,
        Some(json!({ "body": "nachträglich geändert", "version": signed["version"] })),
    )
    .await;
    assert!(
        sqlx::query("UPDATE clinical_notes SET body = 'manipuliert' WHERE id = $1")
            .bind(note_id)
            .execute(app.pool())
            .await
            .is_err()
    );
    assert!(sqlx::query("DELETE FROM clinical_notes WHERE id = $1")
        .bind(note_id)
        .execute(app.pool())
        .await
        .is_err());
    // Korrektur: neue Fassung ersetzt die alte erst mit ihrer Freigabe
    let amended = app
        .ok(
            "POST",
            &format!("{notes}/{note_id}/amend"),
            &h.physician,
            None,
        )
        .await;
    let amended_id = id_of(&amended);
    app.ok(
        "POST",
        &format!("{notes}/{amended_id}/sign"),
        &h.physician,
        Some(json!({ "version": amended["version"] })),
    )
    .await;
    let list = app.ok("GET", &notes, &h.nurse, None).await;
    assert_eq!(count(&list), 1, "ersetzte Fassung erscheint nicht mehr");
    assert_eq!(list["items"][0]["id"], amended["id"]);
    // Pflege schreibt Pflegeberichte, keine ärztlichen Einträge
    app.fails(
        StatusCode::FORBIDDEN,
        "POST",
        &notes,
        &h.nurse,
        Some(json!({ "kind": "verlauf", "body": "x" })),
    )
    .await;
    app.ok(
        "POST",
        &notes,
        &h.nurse,
        Some(json!({ "kind": "pflege", "body": "Mobilisation an die Bettkante." })),
    )
    .await;

    // Aufgabe vom Arzt, erledigt von der Pflege
    let task = app
        .ok(
            "POST",
            &format!("{case_path}/tasks"),
            &h.physician,
            Some(json!({ "text": "Troponin-Kontrolle 18 Uhr", "assigneeLabel": "Spätdienst" })),
        )
        .await;
    let unit_tasks = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/tasks", h.ward1),
            &h.nurse,
            None,
        )
        .await;
    assert_eq!(count(&unit_tasks), 1);
    app.ok(
        "POST",
        &format!("{case_path}/tasks/{}/done", id_of(&task)),
        &h.nurse,
        Some(json!({ "version": task["version"] })),
    )
    .await;
    let unit_tasks = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/tasks", h.ward1),
            &h.nurse,
            None,
        )
        .await;
    assert_eq!(count(&unit_tasks), 0);

    // --- Pflege: Vitalwerte und Bilanz ----------------------------------------
    let vitals = format!("{case_path}/vitals");
    app.ok(
        "POST",
        &vitals,
        &h.nurse,
        Some(json!({ "systolic": 125, "diastolic": 80, "heartRate": 72, "temperature": 36.8, "spo2": 97 })),
    )
    .await;
    for invalid in [
        json!({ "systolic": 125 }),         // Blutdruck nur paarweise
        json!({ "temperature": 50.0 }),     // außerhalb des Wertebereichs
        json!({ "note": "kein Messwert" }), // mindestens ein Wert
    ] {
        app.fails(
            StatusCode::BAD_REQUEST,
            "POST",
            &vitals,
            &h.nurse,
            Some(invalid),
        )
        .await;
    }
    let list = app.ok("GET", &vitals, &h.physician, None).await;
    assert_eq!(count(&list), 1);
    assert_eq!(list["items"][0]["temperature"], 36.8);
    let fluid = format!("{case_path}/fluid-balance");
    app.ok(
        "POST",
        &fluid,
        &h.nurse,
        Some(json!({ "direction": "einfuhr", "category": "oral", "volumeMl": 500 })),
    )
    .await;
    app.ok(
        "POST",
        &fluid,
        &h.nurse,
        Some(json!({ "direction": "ausfuhr", "category": "urin", "volumeMl": 200 })),
    )
    .await;
    let balance = app.ok("GET", &fluid, &h.nurse, None).await;
    assert_eq!(balance["balanceMl"], 300);

    // --- Verordnung mit Allergie-Abgleich -------------------------------------
    let rx = format!("{case_path}/prescriptions");
    let body = json!({ "drugName": "Penicillin V 1,5 Mega", "dose": "1 Tbl", "schedule": "1-1-1",
                       "route": "po", "isAntiinfective": true, "plannedDays": 7 });
    let v = app
        .fails(
            StatusCode::CONFLICT,
            "POST",
            &rx,
            &h.physician,
            Some(body.clone()),
        )
        .await;
    assert!(v["error"].as_str().unwrap().contains("Allergie"), "{v}");
    app.fails(
        StatusCode::FORBIDDEN,
        "POST",
        &rx,
        &h.nurse,
        Some(body.clone()),
    )
    .await;
    let mut confirmed = body;
    confirmed["acknowledgeAllergy"] = json!(true);
    let prescription = app.ok("POST", &rx, &h.physician, Some(confirmed)).await;
    assert_eq!(prescription["therapyDay"], 1);
    assert_eq!(prescription["overdue"], false);
    let rx_id = id_of(&prescription);
    app.ok(
        "POST",
        &format!("{rx}/{rx_id}/administrations"),
        &h.nurse,
        Some(json!({})),
    )
    .await;
    // nicht gegeben: nur mit Begründung
    app.fails(
        StatusCode::BAD_REQUEST,
        "POST",
        &format!("{rx}/{rx_id}/administrations"),
        &h.nurse,
        Some(json!({ "outcome": "verweigert" })),
    )
    .await;
    let stopped = app
        .ok(
            "PUT",
            &format!("{rx}/{rx_id}/status"),
            &h.physician,
            Some(json!({ "status": "abgesetzt", "version": prescription["version"] })),
        )
        .await;
    assert_eq!(stopped["status"], "abgesetzt");
    app.fails(
        StatusCode::CONFLICT,
        "POST",
        &format!("{rx}/{rx_id}/administrations"),
        &h.nurse,
        Some(json!({})),
    )
    .await;

    // --- Anforderung an die Radiologie ----------------------------------------
    let order = app
        .ok(
            "POST",
            &format!("{case_path}/orders"),
            &h.physician,
            Some(json!({ "serviceId": xray, "details": "Stauung? Infiltrat?", "priority": "dringend" })),
        )
        .await;
    let order_id = id_of(&order);
    assert_eq!(order["requestingUnitName"], "Station 1");
    // Mit dem Auftrag behandelt die Radiologie mit — und sieht den Fall
    app.ok("GET", &case_path, &h.radiology, None).await;
    let worklist_path = format!("/api/v1/units/{}/worklist", h.radiology_unit);
    let worklist = app.ok("GET", &worklist_path, &h.radiology, None).await;
    assert_eq!(count(&worklist), 1);
    assert_eq!(worklist["items"][0]["nameFamily"], "Musterfrau");
    assert_eq!(worklist["items"][0]["bedLabel"], "A");
    // Fremde Arbeitsliste bleibt verschlossen
    app.fails(
        StatusCode::FORBIDDEN,
        "GET",
        &worklist_path,
        &h.cleaning,
        None,
    )
    .await;
    let complete = format!("{worklist_path}/{order_id}/complete");
    app.fails(
        StatusCode::BAD_REQUEST,
        "POST",
        &complete,
        &h.radiology,
        Some(json!({ "version": order["version"] })),
    )
    .await;
    let done = app
        .ok(
            "POST",
            &complete,
            &h.radiology,
            Some(json!({ "resultText": "Keine Stauung, kein Infiltrat.", "version": order["version"] })),
        )
        .await;
    assert_eq!(done["status"], "erledigt");
    let orders = app
        .ok("GET", &format!("{case_path}/orders"), &h.physician, None)
        .await;
    assert_eq!(
        orders["items"][0]["resultText"],
        "Keine Stauung, kein Infiltrat."
    );

    // Nach Abschluss läuft der Zugriff mit dem eingestellten Nachlauf aus.
    // Die Einstellung wirkt sofort, ohne Neustart und ohne neue Anmeldung.
    app.ok("GET", &case_path, &h.radiology, None).await;
    app.ok(
        "PUT",
        "/api/v1/admin/settings/security.treatment_context.grace_days",
        &h.admin,
        Some(json!({ "value": 0 })),
    )
    .await;
    app.fails(StatusCode::FORBIDDEN, "GET", &case_path, &h.radiology, None)
        .await;

    // --- Labor über ein Schnittstellenkonto -----------------------------------
    let lab = format!("{case_path}/lab/specimens");
    let earlier = (chrono::Utc::now() - chrono::Duration::hours(26)).to_rfc3339();
    let later = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    let specimen = json!({
        "collectedAt": earlier, "externalId": "LAB-0001",
        "results": [
            { "analyteCode": "hb", "name": "Hämoglobin", "valueNum": 6.4, "unit": "g/dl",
              "refLow": 12.0, "refHigh": 16.0, "flag": "LL", "status": "vorlaeufig" },
            { "analyteCode": "KREA", "name": "Kreatinin", "valueNum": 1.4, "unit": "mg/dl",
              "refLow": 0.5, "refHigh": 1.1 },
        ],
    });
    let first = app
        .ok("POST", &lab, &h.interface, Some(specimen.clone()))
        .await;
    assert_eq!(first["replaced"], false);
    // Erneute Lieferung derselben Probe ersetzt die Werte (keine Dublette)
    let mut corrected = specimen;
    corrected["results"][0]["valueNum"] = json!(6.6);
    corrected["results"][0]["status"] = json!("final");
    let second = app.ok("POST", &lab, &h.interface, Some(corrected)).await;
    assert_eq!(second["replaced"], true);
    assert_eq!(second["id"], first["id"]);
    app.ok(
        "POST",
        &lab,
        &h.interface,
        Some(json!({
            "collectedAt": later, "externalId": "LAB-0002",
            "results": [{ "analyteCode": "HB", "valueNum": 8.1, "unit": "g/dl", "refLow": 12.0, "refHigh": 16.0 }],
        })),
    )
    .await;
    let specimens = app.ok("GET", &lab, &h.physician, None).await;
    assert_eq!(count(&specimens), 2);
    let cumulative = app
        .ok(
            "GET",
            &format!("{case_path}/lab/cumulative?analytes=hb"),
            &h.physician,
            None,
        )
        .await;
    let hb = &cumulative["items"][0]["values"];
    assert_eq!(hb.as_array().unwrap().len(), 2);
    assert_eq!(hb[0]["valueNum"], 6.6);
    assert_eq!(
        hb[1]["flag"], "L",
        "Bewertung aus Referenzbereich abgeleitet"
    );
    assert!((hb[1]["delta"].as_f64().unwrap() - 1.5).abs() < 1e-9);
    // Die Pflege schreibt keine Laborwerte
    app.fails(
        StatusCode::FORBIDDEN,
        "POST",
        &lab,
        &h.nurse,
        Some(json!({})),
    )
    .await;
    // Kritische Werte der Station: im Zeitraum sichtbar, danach nicht mehr
    let critical = format!("/api/v1/units/{}/lab/critical", h.ward1);
    let found = app
        .ok("GET", &format!("{critical}?hours=48"), &h.nurse, None)
        .await;
    assert_eq!(count(&found), 1);
    assert_eq!(found["items"][0]["analyteCode"], "HB");
    assert_eq!(
        count(
            &app.ok("GET", &format!("{critical}?hours=12"), &h.nurse, None)
                .await
        ),
        0
    );

    // --- Küche: Produktionsliste ohne Namen -----------------------------------
    app.ok(
        "PUT",
        &format!("{case_path}/diet"),
        &h.nurse,
        Some(json!({ "dietId": diabetic, "note": "weiche Kost" })),
    )
    .await;
    let (s, text) = app
        .raw("GET", "/api/v1/meals/production", Some(&h.kitchen), None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        !text.contains("Musterfrau"),
        "Produktionsliste nennt keine Namen"
    );
    assert!(!text.contains("Vorderwandinfarkt"), "und keine Diagnosen");
    let production: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(production["trays"], 1);
    let tray = &production["items"][0];
    assert_eq!(tray["wardName"], "Station 1");
    assert_eq!(tray["bedLabel"], "A");
    assert_eq!(tray["dietCode"], "DIAB");
    assert_eq!(tray["foodAllergies"], json!(["Erdnuss"]));
    // Die Pflege setzt „nüchtern“ — die Küche sieht es sofort
    let current = app.ok("GET", &case_path, &h.nurse, None).await;
    let status = json!({
        "stability": "beobachtung", "fasting": true, "dnr": false, "dni": false,
        "contingency": "Bei erneutem Thoraxschmerz: EKG, Troponin, Dienstarzt",
        "version": current["version"],
    });
    let updated = app
        .ok(
            "PUT",
            &format!("{case_path}/clinical-status"),
            &h.nurse,
            Some(status),
        )
        .await;
    let production = app
        .ok("GET", "/api/v1/meals/production", &h.kitchen, None)
        .await;
    assert_eq!(production["trays"], 0);
    assert_eq!(production["fasting"], 1);
    // DNR legt nur der ärztliche Dienst fest
    let mut dnr = json!({ "stability": "beobachtung", "fasting": false, "dnr": true, "dni": false,
                          "version": updated["version"] });
    app.fails(
        StatusCode::FORBIDDEN,
        "PUT",
        &format!("{case_path}/clinical-status"),
        &h.nurse,
        Some(dnr.clone()),
    )
    .await;
    dnr["attendingUserId"] = json!(h.physician_id);
    let updated = app
        .ok(
            "PUT",
            &format!("{case_path}/clinical-status"),
            &h.physician,
            Some(dnr),
        )
        .await;
    assert_eq!(updated["dnr"], true);
    assert_eq!(updated["attendingName"], "arzt1");

    // Übergabe der Station: alles Wesentliche in einer Liste
    let handover = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/handover", h.ward1),
            &h.nurse,
            None,
        )
        .await;
    let entry = &handover["items"][0];
    assert_eq!(entry["stability"], "beobachtung");
    assert_eq!(entry["leadingDiagnosis"], "Akuter Vorderwandinfarkt");
    assert_eq!(entry["lastNote"], "Beschwerdefrei, EKG unverändert.");

    // --- Notfallzugriff --------------------------------------------------------
    app.fails(
        StatusCode::BAD_REQUEST,
        "POST",
        "/api/v1/emergency-access",
        &h.surgeon,
        Some(json!({ "caseId": cid, "reason": "kurz" })),
    )
    .await;
    app.ok(
        "POST",
        "/api/v1/emergency-access",
        &h.surgeon,
        Some(json!({ "caseId": cid, "reason": "Reanimation auf Station 1, Diensthabender" })),
    )
    .await;
    app.ok("GET", &case_path, &h.surgeon, None).await;
    let grants = app
        .ok("GET", "/api/v1/admin/emergency-access", &h.auditor, None)
        .await;
    assert_eq!(count(&grants), 1);
    let grant_id = grants["grants"][0]["id"].as_str().unwrap().to_string();
    app.ok(
        "POST",
        &format!("/api/v1/admin/emergency-access/{grant_id}/review"),
        &h.auditor,
        Some(json!({ "note": "plausibel" })),
    )
    .await;
    assert_eq!(
        count(
            &app.ok("GET", "/api/v1/admin/emergency-access", &h.auditor, None)
                .await
        ),
        0
    );

    // --- Verlegung in die Chirurgie --------------------------------------------
    let transferred = app
        .ok(
            "POST",
            &format!("{case_path}/transfer"),
            &h.nurse,
            Some(json!({
                "departmentId": h.surgery, "wardId": h.ward2, "bedId": h.bed_surgery,
                "version": updated["version"],
            })),
        )
        .await;
    assert_eq!(transferred["wardName"], "Station 2");
    let stays = app
        .ok("GET", &format!("{case_path}/stays"), &h.surgeon, None)
        .await;
    assert_eq!(count(&stays), 2);
    // Veraltete Version wird abgelehnt
    app.fails(
        StatusCode::CONFLICT,
        "POST",
        &format!("{case_path}/transfer"),
        &h.reception,
        Some(
            json!({ "departmentId": h.internal, "wardId": h.ward1, "bedId": h.bed2, "version": 1 }),
        ),
    )
    .await;

    // --- Entlassung löst die Bettenaufbereitung aus -----------------------------
    let discharged = app
        .ok(
            "POST",
            &format!("{case_path}/discharge"),
            &h.reception,
            Some(json!({ "dischargeType": "regulaer", "version": transferred["version"] })),
        )
        .await;
    assert_eq!(discharged["status"], "entlassen");
    let cleaning_list = format!("/api/v1/units/{}/worklist", h.cleaning_unit);
    let jobs = app.ok("GET", &cleaning_list, &h.cleaning, None).await;
    assert_eq!(count(&jobs), 1, "Automatik hat den Auftrag erzeugt");
    let job = &jobs["items"][0];
    assert_eq!(job["serviceCode"], "BETT-AUFB");
    assert_eq!(job["wardName"], "Station 2");
    assert_eq!(job["bedLabel"], "A");
    assert!(job["caseId"].is_null(), "Reinigung erhält keinen Fallbezug");
    assert!(job["nameFamily"].is_null());
    // Das Bett ist gesperrt, bis die Aufbereitung erledigt ist
    let blocked = json!({
        "patientId": other, "kind": "stationaer", "departmentId": h.surgery,
        "wardId": h.ward2, "bedId": h.bed_surgery,
    });
    let v = app
        .fails(
            StatusCode::CONFLICT,
            "POST",
            "/api/v1/cases",
            &h.reception,
            Some(blocked.clone()),
        )
        .await;
    assert!(v["error"].as_str().unwrap().contains("gesperrt"), "{v}");
    app.ok(
        "POST",
        &format!("{cleaning_list}/{}/complete", id_of(job)),
        &h.cleaning,
        Some(json!({ "version": job["version"] })),
    )
    .await;
    app.ok("POST", "/api/v1/cases", &h.reception, Some(blocked))
        .await;
    // Für entlassene Fälle wird nichts mehr angefordert oder verordnet
    app.fails(
        StatusCode::CONFLICT,
        "POST",
        &format!("{case_path}/orders"),
        &h.surgeon,
        Some(json!({ "serviceId": xray })),
    )
    .await;

    // --- Schnittstelle liest die Ereignisse ------------------------------------
    let events = app
        .ok(
            "GET",
            "/api/v1/interfaces/events?consumer=hl7-adt",
            &h.interface,
            None,
        )
        .await;
    let topics: Vec<&str> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["topic"].as_str().unwrap())
        .collect();
    assert_eq!(
        topics,
        [
            "case.admitted",
            "order.completed",
            "case.transferred",
            "case.discharged",
            "order.completed",
            "case.admitted"
        ]
    );
    let (_, raw) = app
        .raw(
            "GET",
            "/api/v1/interfaces/events?consumer=hl7-adt",
            Some(&h.interface),
            None,
        )
        .await;
    assert!(
        !raw.contains("Musterfrau"),
        "Ereignisse tragen keine Personendaten"
    );
    let last = events["events"].as_array().unwrap().last().unwrap()["id"].clone();
    app.ok(
        "POST",
        "/api/v1/interfaces/events/ack",
        &h.interface,
        Some(json!({ "consumer": "hl7-adt", "upTo": last })),
    )
    .await;
    let events = app
        .ok(
            "GET",
            "/api/v1/interfaces/events?consumer=hl7-adt",
            &h.interface,
            None,
        )
        .await;
    assert_eq!(count(&events), 0);
    app.fails(
        StatusCode::FORBIDDEN,
        "GET",
        "/api/v1/interfaces/events?consumer=x",
        &h.physician,
        None,
    )
    .await;

    app.finish().await;
}

/// Ein Fehler in einer Folgeaktion eines anderen Moduls darf den
/// auslösenden Vorgang nicht stören — und nicht still verloren gehen.
#[tokio::test]
async fn a_failing_module_does_not_break_the_workflow() {
    let app = app!();
    let h = hospital(&app).await;
    let service = id_of(
        &app.ok(
            "POST",
            "/api/v1/catalog/services",
            &h.admin,
            Some(json!({
                "code": "BETT-AUFB", "name": "Bettenaufbereitung", "category": "reinigung",
                "providerUnitId": h.cleaning_unit, "requiresCase": false,
            })),
        )
        .await,
    );
    app.ok(
        "POST",
        "/api/v1/catalog/automation-rules",
        &h.admin,
        Some(json!({ "name": "Aufbereitung", "onTopic": "case.discharged", "serviceId": service, "blockBed": true })),
    )
    .await;
    // Simulierter Defekt im Auftragswesen: jedes Anlegen eines Auftrags scheitert.
    sqlx::raw_sql(
        "CREATE FUNCTION test_defekt() RETURNS trigger LANGUAGE plpgsql AS
         $$ BEGIN RAISE EXCEPTION 'simulierter Defekt'; END; $$;
         CREATE TRIGGER test_defekt BEFORE INSERT ON orders
             FOR EACH ROW EXECUTE FUNCTION test_defekt();",
    )
    .execute(app.pool())
    .await
    .unwrap();

    let pid = patient(&app, &h.reception, "Ausfalltest").await;
    let case = admit(&app, &h, pid, h.bed1).await;
    let discharged = app
        .ok(
            "POST",
            &format!("/api/v1/cases/{}/discharge", id_of(&case)),
            &h.reception,
            Some(json!({ "dischargeType": "regulaer", "version": case["version"] })),
        )
        .await;
    assert_eq!(
        discharged["status"], "entlassen",
        "Entlassung gelingt trotz Defekt"
    );
    // Das Bett ist frei (keine halbe Wirkung der gescheiterten Automatik)
    let beds = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/beds", h.ward1),
            &h.nurse,
            None,
        )
        .await;
    assert!(beds["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["blockedReason"].is_null() && b["caseId"].is_null()));
    // Der Fehler ist für die Administration sichtbar und wird quittiert
    let failures = app
        .ok("GET", "/api/v1/admin/event-failures", &h.admin, None)
        .await;
    assert_eq!(count(&failures), 1);
    assert_eq!(failures["failures"][0]["subscriber"], "orders.automation");
    assert_eq!(failures["failures"][0]["topic"], "case.discharged");
    let id = failures["failures"][0]["id"].as_i64().unwrap();
    app.ok(
        "POST",
        &format!("/api/v1/admin/event-failures/{id}/resolve"),
        &h.admin,
        None,
    )
    .await;
    assert_eq!(
        count(
            &app.ok("GET", "/api/v1/admin/event-failures", &h.admin, None)
                .await
        ),
        0
    );

    app.finish().await;
}

/// Module lassen sich abschalten, ohne dass der Rest davon erfährt.
#[tokio::test]
async fn modules_can_be_switched_off() {
    let mut cfg = common::config();
    cfg.modules.disabled = [
        "orders",
        "meals",
        "lab",
        "roster",
        "prescriptions",
        "nursing",
        "clinical",
    ]
    .map(String::from)
    .to_vec();
    let app = app!(cfg);
    let h = hospital(&app).await;

    // Kern läuft: Aufnahme, Stationsliste, Entlassung
    let pid = patient(&app, &h.reception, "Schlankbetrieb").await;
    let case = admit(&app, &h, pid, h.bed1).await;
    let cid = id_of(&case);
    let census = app
        .ok(
            "GET",
            &format!("/api/v1/units/{}/census", h.ward1),
            &h.nurse,
            None,
        )
        .await;
    assert_eq!(count(&census), 1);
    app.ok(
        "POST",
        &format!("/api/v1/cases/{cid}/discharge"),
        &h.reception,
        Some(json!({ "dischargeType": "regulaer", "version": case["version"] })),
    )
    .await;

    // Routen abgeschalteter Module gibt es nicht: für Nicht-Admins 403
    // (unbekannte API-Pfade sind gesperrt), für Admins 404.
    for path in [
        "/api/v1/catalog/services",
        "/api/v1/meals/production",
        "/api/v1/roster/staff",
    ] {
        app.fails(StatusCode::FORBIDDEN, "GET", path, &h.physician, None)
            .await;
        app.fails(StatusCode::NOT_FOUND, "GET", path, &h.admin, None)
            .await;
    }
    let modules = app.ok("GET", "/api/v1/modules", &h.nurse, None).await;
    let state = |id: &str| -> bool {
        modules["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("Modul {id} gelistet"))["enabled"]
            .as_bool()
            .unwrap()
    };
    assert!(state("cases") && state("org") && state("patients"));
    assert!(!state("orders") && !state("meals") && !state("roster"));
    app.finish().await;

    // Ungültige Auswahl bricht den Start ab
    for invalid in [vec!["cases"], vec!["doctors"], vec!["gibt-es-nicht"]] {
        let Some(tdb) = common::TestDb::new().await else {
            return;
        };
        let mut cfg = common::config();
        cfg.modules.disabled = invalid.iter().map(|s| s.to_string()).collect();
        let security =
            std::sync::Arc::new(kis_core::security::Security::new(&cfg.security).unwrap());
        assert!(
            kis_app::build_app(cfg, tdb.db.clone(), security)
                .await
                .is_err(),
            "{invalid:?} muss abgelehnt werden"
        );
        tdb.drop().await;
    }
}

/// Rollen und Richtlinien gehören dem Haus: Änderungen wirken sofort,
/// ohne Neustart und ohne neue Anmeldung.
#[tokio::test]
async fn roles_and_policies_are_configured_at_runtime() {
    let app = app!();
    let h = hospital(&app).await;

    // Hauseigene Rolle
    let role = "/api/v1/admin/roles/stationssekretariat";
    app.fails(
        StatusCode::BAD_REQUEST,
        "PUT",
        role,
        &h.admin,
        Some(json!({ "label": "Stationssekretariat", "permissions": ["patient.read", "gibt.es.nicht"] })),
    )
    .await;
    app.ok(
        "PUT",
        role,
        &h.admin,
        Some(
            json!({ "label": "Stationssekretariat", "permissions": ["patient.read", "org.read"] }),
        ),
    )
    .await;
    let (clerk_id, clerk) = app.session("sekretariat1", "stationssekretariat").await;
    app.ok("GET", "/api/v1/patients", &clerk, None).await;
    let new_patient =
        json!({ "nameFamily": "Neu", "nameGiven": "Anna", "birthDate": "1980-01-01" });
    app.fails(
        StatusCode::FORBIDDEN,
        "POST",
        "/api/v1/patients",
        &clerk,
        Some(new_patient.clone()),
    )
    .await;
    // Recht ergänzen → dieselbe Sitzung darf sofort mehr
    app.ok(
        "PUT",
        role,
        &h.admin,
        Some(json!({ "label": "Stationssekretariat", "permissions": ["patient.read", "patient.create", "org.read"] })),
    )
    .await;
    app.ok("POST", "/api/v1/patients", &clerk, Some(new_patient))
        .await;
    // Zusatzrolle
    app.fails(
        StatusCode::FORBIDDEN,
        "GET",
        "/api/v1/roster/staff",
        &clerk,
        None,
    )
    .await;
    app.ok(
        "PUT",
        &format!("/api/v1/admin/users/{clerk_id}/roles"),
        &h.admin,
        Some(json!({ "roles": ["dienstplaner"] })),
    )
    .await;
    app.ok("GET", "/api/v1/roster/staff", &clerk, None).await;
    let me = app.ok("GET", "/api/v1/me", &clerk, None).await;
    assert!(
        me["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "roster.write"),
        "{me}"
    );

    // Schutz vor Aussperren und vor verwaisten Rollen
    app.fails(
        StatusCode::CONFLICT,
        "PUT",
        "/api/v1/admin/roles/admin",
        &h.admin,
        Some(json!({ "label": "x", "permissions": [] })),
    )
    .await;
    app.fails(StatusCode::CONFLICT, "DELETE", role, &h.admin, None)
        .await;
    app.fails(
        StatusCode::CONFLICT,
        "DELETE",
        "/api/v1/admin/roles/arzt",
        &h.admin,
        None,
    )
    .await;
    let users = app.ok("GET", "/api/v1/admin/users", &h.admin, None).await;
    let admin_id = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "admin1")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    app.fails(
        StatusCode::CONFLICT,
        "PUT",
        &format!("/api/v1/admin/users/{admin_id}"),
        &h.admin,
        Some(json!({ "isActive": false })),
    )
    .await;
    // Nicht-Admins verwalten nichts
    app.fails(
        StatusCode::FORBIDDEN,
        "GET",
        "/api/v1/admin/roles",
        &h.physician,
        None,
    )
    .await;
    app.fails(
        StatusCode::FORBIDDEN,
        "PUT",
        "/api/v1/admin/settings/security.treatment_context.enforce",
        &h.physician,
        Some(json!({ "value": false })),
    )
    .await;

    // Richtlinie: Behandlungszusammenhang abschaltbar (kleine Häuser)
    let pid = patient(&app, &h.reception, "Richtlinie").await;
    let case = admit(&app, &h, pid, h.bed1).await;
    let case_path = format!("/api/v1/cases/{}", id_of(&case));
    app.fails(StatusCode::FORBIDDEN, "GET", &case_path, &h.surgeon, None)
        .await;
    let setting = "/api/v1/admin/settings/security.treatment_context.enforce";
    app.fails(
        StatusCode::BAD_REQUEST,
        "PUT",
        setting,
        &h.admin,
        Some(json!({ "value": "nein" })),
    )
    .await;
    app.fails(
        StatusCode::BAD_REQUEST,
        "PUT",
        "/api/v1/admin/settings/gibt.es.nicht",
        &h.admin,
        Some(json!({ "value": 1 })),
    )
    .await;
    app.ok("PUT", setting, &h.admin, Some(json!({ "value": false })))
        .await;
    app.ok("GET", &case_path, &h.surgeon, None).await;
    app.ok("PUT", setting, &h.admin, Some(json!({ "value": true })))
        .await;
    app.fails(StatusCode::FORBIDDEN, "GET", &case_path, &h.surgeon, None)
        .await;
    let settings = app
        .ok("GET", "/api/v1/admin/settings", &h.admin, None)
        .await;
    assert!(settings["settings"].as_array().unwrap().len() >= 7);

    // Zuordnung zu einer Einheit über die Verwaltung öffnet den Zugriff
    let surgeon_id = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "arzt2")
        .unwrap()["id"]
        .clone();
    app.ok(
        "PUT",
        &format!("/api/v1/admin/users/{}/units", surgeon_id.as_str().unwrap()),
        &h.admin,
        Some(json!({ "unitIds": [h.surgery, h.internal] })),
    )
    .await;
    app.ok("GET", &case_path, &h.surgeon, None).await;
    let mine = app.ok("GET", "/api/v1/me/units", &h.surgeon, None).await;
    assert_eq!(count(&mine), 4, "zwei Abteilungen mit je einer Station");

    app.finish().await;
}

/// Löschung nach Art. 17 entfernt den Patienten mit allen Falldaten —
/// auch freigegebene Dokumentation, die sonst unlöschbar ist.
#[tokio::test]
async fn erasure_removes_all_case_data() {
    let app = app!();
    let h = hospital(&app).await;
    let pid = patient(&app, &h.reception, "Loeschtest").await;
    let case = admit(&app, &h, pid, h.bed1).await;
    let cid = id_of(&case);
    let case_path = format!("/api/v1/cases/{cid}");
    let note = app
        .ok(
            "POST",
            &format!("{case_path}/notes"),
            &h.physician,
            Some(json!({ "kind": "verlauf", "body": "x" })),
        )
        .await;
    app.ok(
        "POST",
        &format!("{case_path}/notes/{}/sign", id_of(&note)),
        &h.physician,
        Some(json!({ "version": note["version"] })),
    )
    .await;
    app.ok(
        "POST",
        &format!("{case_path}/vitals"),
        &h.nurse,
        Some(json!({ "heartRate": 80 })),
    )
    .await;
    app.ok(
        "POST",
        &format!("{case_path}/discharge"),
        &h.reception,
        Some(json!({ "dischargeType": "regulaer", "version": case["version"] })),
    )
    .await;

    // Auskunft nach Art. 15 enthält die Falldaten vollständig
    let export = app
        .ok(
            "GET",
            &format!("/api/v1/patients/{pid}/export"),
            &h.admin,
            None,
        )
        .await;
    let exported = &export["cases"][0];
    assert_eq!(exported["status"], "entlassen");
    assert_eq!(exported["stays"][0]["ward"], "Station 1");
    assert_eq!(exported["notes"][0]["body"], "x");
    assert_eq!(exported["vitals"][0]["heartRate"], 80);
    // Die Pflege darf nicht exportieren
    app.fails(
        StatusCode::FORBIDDEN,
        "GET",
        &format!("/api/v1/patients/{pid}/export"),
        &h.nurse,
        None,
    )
    .await;

    app.ok(
        "POST",
        &format!("/api/v1/patients/{pid}/hard-delete"),
        &h.admin,
        Some(json!({ "confirmation": "Loeschtest" })),
    )
    .await;
    for table in ["cases", "case_stays", "clinical_notes", "vital_signs"] {
        let left: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(app.pool())
            .await
            .unwrap();
        assert_eq!(left, 0, "{table} ist leer");
    }
    app.finish().await;
}
