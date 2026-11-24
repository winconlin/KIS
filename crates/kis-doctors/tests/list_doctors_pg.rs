//! PostgreSQL-Integrationstests für `GET /api/v1/doctors` (list_doctors):
//! parametrisierte SQL-Werte (keine String-Interpolation), Suchfilter,
//! Fachgebietsfilter, kombinierte Filter, Sonderzeichen, leere Ergebnisse
//! und Cursor-Pagination.
//!
//! Voraussetzung: `KIS_TEST_DATABASE_URL` zeigt auf einen PostgreSQL-Server,
//! auf dem der Benutzer Datenbanken anlegen darf, z. B.
//! `postgres://postgres@127.0.0.1:5432/postgres`. Ohne die Variable werden
//! die Tests mit deutlichem Hinweis übersprungen; mit `KIS_REQUIRE_PG_TESTS=1`
//! (CI, `scripts/check.sh`) schlagen sie stattdessen fehl. Übersprungene Tests
//! gelten nicht als bestanden.
//!
//! Alle Testdaten sind synthetisch.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use kis_core::events::EventBus;
use kis_core::module::{Module, ModuleContext};
use kis_core::platform::Platform;
use kis_core::security::Security;
use kis_doctors::DoctorsModule;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

mod common;

use common::{config, TestDb};

// ===========================================================================
// Test-App: echter Modul-Router (list/get) gegen frische Test-DB
// ===========================================================================

struct DocApp {
    router: Router,
    _tdb: TestDb,
}

impl DocApp {
    /// Baut den echten Modul-Router des Doctors-Moduls mit echter DB.
    /// `routes()` bindet den Pool bereits als State (`with_state`),
    /// daher ist der Router direkt startbar. Ohne `KIS_TEST_DATABASE_URL`
    /// wird `None` geliefert und der aufrufende Test überspringt.
    async fn new() -> Option<Self> {
        let tdb = TestDb::new().await?;
        let cfg = config();
        let security = Arc::new(Security::new(&cfg.security).expect("Security"));
        let platform = Arc::new(
            Platform::load(tdb.db.write(), &cfg.security)
                .await
                .expect("Platform"),
        );
        let ctx = ModuleContext {
            db: tdb.db.clone(),
            security,
            config: Arc::new(cfg),
            platform,
            events: Arc::new(EventBus::new()),
        };
        let (router, _rules) = DoctorsModule.routes(&ctx).into_parts();
        Some(Self { router, _tdb: tdb })
    }
}

/// GET-Aufruf gegen den Router (RBAC läuft als separate Middleware;
/// der Handler wird hier ohne Auth-Schicht getestet).
async fn get(app: &DocApp, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("Request");
    let resp = app.router.clone().oneshot(req).await.expect("Antwort");
    let status = resp.status();
    let body = resp.into_body().collect().await.expect("Body").to_bytes();
    let v: Value = serde_json::from_slice(&body)
        .unwrap_or(Value::String(String::from_utf8_lossy(&body).into_owned()));
    (status, v)
}

/// Legt per SQL einen Arzt an (direkt gegen die DB) und liefert dessen ID.
async fn create_doctor(
    pool: &sqlx::PgPool,
    family: &str,
    given: &str,
    specialty: Option<&str>,
) -> Uuid {
    let (id,): (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO doctors (
            external_id, name_family, name_given,
            specialty, specialties, created_at, updated_at, version
        ) VALUES (NULL, $1, $2, $3, $4, now(), now(), 1)
        RETURNING id
        "#,
    )
    .bind(family)
    .bind(given)
    .bind(specialty)
    .bind(json!([]))
    .fetch_one(pool)
    .await
    .expect("Arzt anlegen");
    id
}

/// IDs der Seite als String-Vektor.
fn page_ids(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .expect("items-Array")
        .iter()
        .map(|i| i["id"].as_str().expect("id").to_string())
        .collect()
}

// ===========================================================================
// Test 1: Grundliste ohne Filter liefert alle aktiven Ärzte
// ===========================================================================
#[tokio::test]
async fn list_no_filter_returns_all_active() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let a = create_doctor(pool, "Muster", "Anna", Some("Kardiologie")).await;
    let b = create_doctor(pool, "Beispiel", "Ben", Some("Dermatologie")).await;
    let c = create_doctor(pool, "Test", "Carla", None).await;

    let (status, v) = get(&app, "/api/v1/doctors").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&a.to_string()), "Arzt A fehlt: {ids:?}");
    assert!(ids.contains(&b.to_string()), "Arzt B fehlt: {ids:?}");
    assert!(ids.contains(&c.to_string()), "Arzt C fehlt: {ids:?}");
    assert_eq!(v["count"], 3);
}

// ===========================================================================
// Test 2: Suchfilter — Nachname (case-insensitive)
// ===========================================================================
#[tokio::test]
async fn list_search_by_family_name() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let hit = create_doctor(pool, "Mustermann", "Anna", None).await;
    let miss = create_doctor(pool, "Beispiel", "Ben", None).await;

    let (status, v) = get(&app, "/api/v1/doctors?search=mustermann").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&hit.to_string()), "Treffer fehlt: {ids:?}");
    assert!(
        !ids.contains(&miss.to_string()),
        "Nicht-Treffer vorhanden: {ids:?}"
    );
}

// ===========================================================================
// Test 3: Suchfilter — Vorname
// ===========================================================================
#[tokio::test]
async fn list_search_by_given_name() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let hit = create_doctor(pool, "Muster", "Sofia", None).await;
    let miss = create_doctor(pool, "Muster", "Anna", None).await;

    let (status, v) = get(&app, "/api/v1/doctors?search=sofia").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&hit.to_string()), "Treffer fehlt: {ids:?}");
    assert!(
        !ids.contains(&miss.to_string()),
        "Nicht-Treffer vorhanden: {ids:?}"
    );
}

// ===========================================================================
// Test 4: Suchfilter — Teilstring
// ===========================================================================
#[tokio::test]
async fn list_search_partial_string() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let hit = create_doctor(pool, "Schwarzmüller", "Anna", None).await;
    let _ = create_doctor(pool, "Weiss", "Ben", None).await;

    let (status, v) = get(&app, "/api/v1/doctors?search=schwarz").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(page_ids(&v).contains(&hit.to_string()), "Teilstring: {v}");
}

// ===========================================================================
// Test 5: Fachgebietsfilter — exakte Übereinstimmung
// ===========================================================================
#[tokio::test]
async fn list_specialty_exact_match() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let hit = create_doctor(pool, "Muster", "Anna", Some("Kardiologie")).await;
    let miss = create_doctor(pool, "Muster", "Ben", Some("Dermatologie")).await;
    let none = create_doctor(pool, "Muster", "Carla", None).await;

    let (status, v) = get(&app, "/api/v1/doctors?specialty=Kardiologie").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&hit.to_string()), "Treffer fehlt: {ids:?}");
    assert!(
        !ids.contains(&miss.to_string()),
        "anderes Fachgebiet: {ids:?}"
    );
    assert!(!ids.contains(&none.to_string()), "ohne Fachgebiet: {ids:?}");
}

// ===========================================================================
// Test 6: Fachgebietsfilter — case-sensitive (Handler nutzt `=`)
// ===========================================================================
#[tokio::test]
async fn list_specialty_case_sensitive() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let hit = create_doctor(pool, "Muster", "Anna", Some("Kardiologie")).await;

    // Andere Schreibweise: kein Treffer
    let (status, v) = get(&app, "/api/v1/doctors?specialty=KARDIOLOGIE").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(page_ids(&v).is_empty(), "case-sensitive: {v}");

    // Gleiche Schreibweise: Treffer
    let (_, v2) = get(&app, "/api/v1/doctors?specialty=Kardiologie").await;
    assert!(page_ids(&v2).contains(&hit.to_string()), "Treffer: {v2}");
}

// ===========================================================================
// Test 7: Kombinierte Filter (Suche + Fachgebiet)
// ===========================================================================
#[tokio::test]
async fn list_combined_search_and_specialty() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let hit = create_doctor(pool, "Muster", "Anna", Some("Kardiologie")).await;
    let wrong_spec = create_doctor(pool, "Muster", "Ben", Some("Neurologie")).await;
    let wrong_name = create_doctor(pool, "Beispiel", "Carla", Some("Kardiologie")).await;
    let _ = create_doctor(pool, "Weiss", "David", Some("Neurologie")).await;

    let (status, v) = get(&app, "/api/v1/doctors?search=muste&specialty=Kardiologie").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(
        ids.contains(&hit.to_string()),
        "Kombi-Treffer fehlt: {ids:?}"
    );
    assert!(
        !ids.contains(&wrong_spec.to_string()),
        "falsches Fachgebiet: {ids:?}"
    );
    assert!(
        !ids.contains(&wrong_name.to_string()),
        "falscher Name: {ids:?}"
    );
}

// ===========================================================================
// Test 8: SQL-Injection-Versuch im Suchbegriff
// ===========================================================================
#[tokio::test]
async fn list_search_injection_safe() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let d = create_doctor(pool, "Muster", "Anna", None).await;

    // x' OR '1'='1 — als Parameter gebunden, liefert 200 und KEINE Treffer
    let (status, v) = get(&app, "/api/v1/doctors?search=x%27+OR+%271%27%3D%271").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(
        !page_ids(&v).contains(&d.to_string()),
        "Injection lieferte unerwartet Treffer: {v}"
    );

    // Semikolon + Kommentar: ebenfalls harmlos
    let (status2, v2) = get(&app, "/api/v1/doctors?search=abc%3B%2D%2D").await;
    assert_eq!(status2, StatusCode::OK, "{v2}");
    assert!(page_ids(&v2).is_empty(), "Sonderzeichen: {v2}");
}

// ===========================================================================
// Test 9: SQL-Injection-Versuch im Cursor
// ===========================================================================
#[tokio::test]
async fn list_cursor_injection_safe() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let d = create_doctor(pool, "Muster", "Anna", None).await;

    // Ungültiger Cursor → 400 (Uuid-Typ im Query-Extractor)
    let (status, _) = get(&app, "/api/v1/doctors?cursor=not-a-uuid").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "Ungültiger Cursor → 400");

    // Injection-Versuch im Cursor → ebenfalls 400 (kein gültiges Uuid)
    let (status2, _) = get(
        &app,
        "/api/v1/doctors?cursor=1%29%3B+DROP+TABLE+doctors%3B--",
    )
    .await;
    assert_eq!(status2, StatusCode::BAD_REQUEST, "Injection-Cursor → 400");

    // Gültiger Cursor: funktioniert
    let (status3, v3) = get(&app, &format!("/api/v1/doctors?cursor={d}")).await;
    assert_eq!(status3, StatusCode::OK, "{v3}");
    assert!(
        !page_ids(&v3).contains(&d.to_string()),
        "eigener Cursor: {v3}"
    );
}

// ===========================================================================
// Test 10: Umlaute in Suchbegriff
// ===========================================================================
#[tokio::test]
async fn list_search_umlaut() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let umlaut = create_doctor(pool, "Grüninger", "Anna", None).await;

    let (status, v) = get(&app, "/api/v1/doctors?search=gr%C3%BCnger").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(page_ids(&v).contains(&umlaut.to_string()), "Umlaut: {v}");
}

// ===========================================================================
// Test 11: Leeres Ergebnis
// ===========================================================================
#[tokio::test]
async fn list_empty_result() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    create_doctor(pool, "Muster", "Anna", None).await;

    let (status, v) = get(&app, "/api/v1/doctors?search=existiert_nicht_xyz").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["count"], 0);
    assert!(page_ids(&v).is_empty());
    assert!(v["next_cursor"].is_null());
}

// ===========================================================================
// Test 12: Soft-Delete — active=true (Default) schließt gelöschte aus
// ===========================================================================
#[tokio::test]
async fn list_excludes_soft_deleted_by_default() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let active = create_doctor(pool, "Muster", "Anna", None).await;
    let deleted = create_doctor(pool, "Muster", "Ben", None).await;

    sqlx::query("UPDATE doctors SET deleted_at = now() WHERE id = $1")
        .bind(deleted)
        .execute(pool)
        .await
        .expect("soft-delete");

    let (status, v) = get(&app, "/api/v1/doctors").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&active.to_string()));
    assert!(!ids.contains(&deleted.to_string()), "soft-deleted: {ids:?}");
}

// ===========================================================================
// Test 13: active=false zeigt auch soft-deleted Ärzte
// ===========================================================================
#[tokio::test]
async fn list_active_false_includes_deleted() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let active = create_doctor(pool, "Vis", "Anna", None).await;
    let deleted = create_doctor(pool, "Vis", "Ben", None).await;
    sqlx::query("UPDATE doctors SET deleted_at = now() WHERE id = $1")
        .bind(deleted)
        .execute(pool)
        .await
        .expect("soft-delete");

    let (status, v) = get(&app, "/api/v1/doctors?active=false").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&active.to_string()));
    assert!(ids.contains(&deleted.to_string()), "active=false: {ids:?}");
}

// ===========================================================================
// Test 14: Cursor-Pagination — Limit 2 von 5 (4 Seiten)
// ===========================================================================
#[tokio::test]
async fn list_pagination_limit_2_of_5() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let mut ids = Vec::new();
    for i in 0..5 {
        ids.push(create_doctor(pool, &format!("Muster{i}"), &format!("Name{i}"), None).await);
    }
    ids.sort();

    // Seite 1
    let (s1, v1) = get(&app, "/api/v1/doctors?limit=2").await;
    assert_eq!(s1, StatusCode::OK, "{v1}");
    assert_eq!(page_ids(&v1), vec![ids[0].to_string(), ids[1].to_string()]);
    assert_eq!(v1["next_cursor"], json!(ids[1]));

    // Seite 2
    let (s2, v2) = get(&app, &format!("/api/v1/doctors?limit=2&cursor={}", ids[1])).await;
    assert_eq!(s2, StatusCode::OK, "{v2}");
    assert_eq!(page_ids(&v2), vec![ids[2].to_string(), ids[3].to_string()]);

    // Seite 3
    let (s3, v3) = get(&app, &format!("/api/v1/doctors?limit=2&cursor={}", ids[3])).await;
    assert_eq!(s3, StatusCode::OK, "{v3}");
    assert_eq!(page_ids(&v3), vec![ids[4].to_string()]);
    assert_eq!(v3["count"], 1);

    // Seite 4: leer
    let (s4, v4) = get(&app, &format!("/api/v1/doctors?limit=2&cursor={}", ids[4])).await;
    assert_eq!(s4, StatusCode::OK, "{v4}");
    assert_eq!(v4["count"], 0);
    assert!(v4["next_cursor"].is_null());
}

// ===========================================================================
// Test 15: Pagination + Suchfilter kombiniert
// ===========================================================================
#[tokio::test]
async fn list_pagination_with_search() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let mut musters = Vec::new();
    for i in 0..4 {
        musters.push(create_doctor(pool, &format!("Muster{i}"), "Anna", None).await);
    }
    let other = create_doctor(pool, "Weiss", "Ben", None).await;
    musters.sort();

    let (s1, v1) = get(&app, "/api/v1/doctors?search=muster&limit=2").await;
    assert_eq!(s1, StatusCode::OK, "{v1}");
    let ids1 = page_ids(&v1);
    assert_eq!(ids1, vec![musters[0].to_string(), musters[1].to_string()]);
    assert!(!ids1.contains(&other.to_string()));

    let (s2, v2) = get(
        &app,
        &format!(
            "/api/v1/doctors?search=muster&limit=2&cursor={}",
            musters[1]
        ),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "{v2}");
    let ids2 = page_ids(&v2);
    assert_eq!(ids2, vec![musters[2].to_string(), musters[3].to_string()]);
    assert!(!ids2.contains(&other.to_string()));
}

// ===========================================================================
// Test 16: Limit-Clamping (limit=0 → 1, limit=1000 → max 200)
// ===========================================================================
#[tokio::test]
async fn list_limit_clamping() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    for i in 0..3 {
        create_doctor(pool, &format!("Clamp{i}"), "A", None).await;
    }

    // limit=0 → 1
    let (s, v) = get(&app, "/api/v1/doctors?limit=0").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["count"], 1, "limit=0 → 1 Eintrag");

    // limit=1000 → max 200, hier nur 3 vorhanden
    let (s2, v2) = get(&app, "/api/v1/doctors?limit=1000").await;
    assert_eq!(s2, StatusCode::OK, "{v2}");
    assert_eq!(v2["count"], 3);
}

// ===========================================================================
// Test 17: Cursor als gültige Uuid (Bind als Parameter)
// ===========================================================================
#[tokio::test]
async fn list_cursor_uuid_bound() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let a = create_doctor(pool, "Cursor", "Anna", None).await;
    let b = create_doctor(pool, "Cursor", "Ben", None).await;

    // Cursor = a → nur b
    let (s, v) = get(&app, &format!("/api/v1/doctors?cursor={a}")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ids = page_ids(&v);
    assert!(ids.contains(&b.to_string()), "b nach Cursor a: {ids:?}");
    assert!(
        !ids.contains(&a.to_string()),
        "a nicht nach eigenem Cursor: {ids:?}"
    );

    // Max-UUID als Cursor → leer
    let (s2, v2) = get(&app, &format!("/api/v1/doctors?cursor={:X}", Uuid::max())).await;
    assert_eq!(s2, StatusCode::OK, "{v2}");
    assert_eq!(v2["count"], 0);
}

// ===========================================================================
// Test 18: Multi-Page-Pagination mit Fachgebietsfilter
// ===========================================================================
#[tokio::test]
async fn list_pagination_multi_page_with_specialty() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let mut cardio = Vec::new();
    for i in 0..6 {
        cardio.push(
            create_doctor(
                pool,
                &format!("Kardio{i}"),
                &format!("D{i}"),
                Some("Kardiologie"),
            )
            .await,
        );
    }
    cardio.sort();
    let other = create_doctor(pool, "Anders", "X", Some("Neurologie")).await;
    let _other2 = create_doctor(pool, "Ander2", "Y", None).await;

    let mut collected = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let path = match &cursor {
            Some(c) => format!("/api/v1/doctors?specialty=Kardiologie&limit=2&cursor={c}"),
            None => "/api/v1/doctors?specialty=Kardiologie&limit=2".to_string(),
        };
        let (s, v) = get(&app, &path).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        collected.extend(page_ids(&v));
        cursor = v["next_cursor"].as_str().map(|s| s.to_string());
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(collected.len(), 6, "6 Kardiologen: {collected:?}");
    assert!(!collected.contains(&other.to_string()));
    assert_eq!(
        collected,
        cardio.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
        "Reihenfolge: {collected:?}"
    );
}

// ===========================================================================
// Test 19: Response-Struktur — camelCase-Felder
// ===========================================================================
#[tokio::test]
async fn list_response_camel_case_fields() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    create_doctor(pool, "Feld", "Test", Some("Kardiologie")).await;

    let (s, v) = get(&app, "/api/v1/doctors").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let item = &v["items"][0];
    assert!(item.get("nameFamily").is_some(), "nameFamily: {item}");
    assert!(item.get("nameGiven").is_some(), "nameGiven: {item}");
    assert!(item.get("createdAt").is_some(), "createdAt: {item}");
    assert!(item.get("updatedAt").is_some(), "updatedAt: {item}");
    assert!(v.get("nextCursor").is_some(), "nextCursor fehlt: {v}");
}

// ===========================================================================
// Test 20: Einzelarzt GET /api/v1/doctors/:id (Kontrolltest, gleicher Pool)
// ===========================================================================
#[tokio::test]
async fn get_doctor_by_id() {
    let app = match DocApp::new().await {
        Some(a) => a,
        None => return,
    };
    let pool = app._tdb.db.write();
    let d = create_doctor(pool, "Einzel", "Anna", Some("Kardiologie")).await;

    let (s, v) = get(&app, &format!("/api/v1/doctors/{d}")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["id"], json!(d));
    assert_eq!(v["nameFamily"], "Einzel");

    // Nicht vorhandene ID → 404
    let (s2, _) = get(&app, &format!("/api/v1/doctors/{}", Uuid::new_v4())).await;
    assert_eq!(s2, StatusCode::NOT_FOUND);
}
