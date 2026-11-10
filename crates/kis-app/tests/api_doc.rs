//! Hält `docs/API.md` aktuell: Die Routentabelle dort wird aus dem
//! laufenden System erzeugt (`GET /api/v1/routes`) und hier verglichen.
//!
//! Weicht sie ab, schlägt der Test fehl. Aktualisieren mit:
//!
//! ```sh
//! KIS_UPDATE_API_DOC=1 cargo test -p kis-app --test api_doc
//! ```

mod common;

const START: &str = "<!-- ROUTEN-ANFANG (erzeugt, nicht von Hand ändern) -->";
const END: &str = "<!-- ROUTEN-ENDE -->";

#[tokio::test]
async fn api_doc_lists_exactly_the_registered_routes() {
    // Alle Module einschalten, auch experimentelle, damit nichts fehlt.
    let mut cfg = common::config();
    cfg.modules.experimental = kis_app::modules::module_infos()
        .iter()
        .filter(|m| m.maturity == kis_core::module::Maturity::Experimental)
        .map(|m| m.id.to_string())
        .collect();
    let app = app!(cfg);
    let (_, admin) = app.session("admin1", "admin").await;
    let overview = app.ok("GET", "/api/v1/routes", &admin, None).await;
    let modules = app.ok("GET", "/api/v1/modules", &admin, None).await;
    app.finish().await;

    let routes = overview["routes"].as_array().unwrap();
    let mut order: Vec<(String, String)> = vec![(
        "kern".into(),
        "Kern (Anmeldung, Verwaltung, Datenschutz)".into(),
    )];
    for m in modules["modules"].as_array().unwrap() {
        order.push((
            m["id"].as_str().unwrap().to_string(),
            m["title"].as_str().unwrap().to_string(),
        ));
    }
    let mut table = String::new();
    for (id, title) in &order {
        let mut rows: Vec<(&str, &str, &str, &str)> = routes
            .iter()
            .filter(|r| r["module"] == id.as_str())
            .map(|r| {
                (
                    r["path"].as_str().unwrap(),
                    r["method"].as_str().unwrap(),
                    r["access"].as_str().unwrap(),
                    r["scope"].as_str().unwrap(),
                )
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort();
        table.push_str(&format!("\n### {title} (`{id}`)\n\n| Methode | Pfad | Recht | Zusätzlich geprüft |\n|---|---|---|---|\n"));
        for (path, method, access, scope) in rows {
            let check = match scope {
                "fall" => "Behandlungszusammenhang",
                "einheit" => "Zuordnung zur Einheit",
                _ => "",
            };
            table.push_str(&format!("| {method} | `{path}` | {access} | {check} |\n"));
        }
    }
    table.push('\n');

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/API.md");
    let doc = std::fs::read_to_string(&path).expect("docs/API.md lesbar");
    let begin = doc.find(START).expect("Startmarke in docs/API.md") + START.len();
    let end = doc.find(END).expect("Endmarke in docs/API.md");
    if std::env::var("KIS_UPDATE_API_DOC").as_deref() == Ok("1") {
        let updated = format!("{}{}{}", &doc[..begin], table, &doc[end..]);
        std::fs::write(&path, updated).expect("docs/API.md schreibbar");
        return;
    }
    assert!(
        doc[begin..end] == table,
        "docs/API.md ist nicht aktuell. Aktualisieren mit:\n  \
         KIS_UPDATE_API_DOC=1 cargo test -p kis-app --test api_doc"
    );
}
