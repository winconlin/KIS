//! Das Werkzeug `kis-admin` als echtes Programm.
//! Testgerüst: `tests/common/mod.rs`.

mod common;

use std::process::Command;

fn kis_admin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kis-admin"));
    // Arbeitsverzeichnis = Repository-Wurzel (dort liegt config/kis.toml)
    cmd.current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    cmd.env_remove("KIS_DATABASE__WRITE_URL")
        .env_remove("KIS_DATABASE__MIGRATION_URL")
        .env_remove("KIS_CONFIG_PATH");
    cmd
}

/// `kis-admin migrate` braucht nur die Migrationsverbindung. Beim ersten
/// Einrichten (und in der CI) ist die Laufzeitverbindung nicht nutzbar;
/// das Werkzeug darf sie dafür nicht anfassen.
#[tokio::test]
async fn migrate_needs_only_the_migration_connection() {
    let Some(tdb) = common::TestDb::new().await else {
        return;
    };
    let count = |table: &'static str| {
        let pool = tdb.db.write().clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        count("role_permissions").await,
        0,
        "frische Datenbank: Rollen noch ohne Rechte"
    );

    let output = kis_admin()
        .arg("migrate")
        .env("KIS_DATABASE__MIGRATION_URL", tdb.url())
        // nicht erreichbar: würde das Werkzeug sie nutzen, schlüge es fehl
        .env(
            "KIS_DATABASE__WRITE_URL",
            "postgres://kis_app:falsch@127.0.0.1:1/kis",
        )
        .output()
        .expect("kis-admin startet");
    assert!(
        output.status.success(),
        "kis-admin migrate: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        count("roles").await >= 18,
        "Grundausstattung der Rollen angelegt"
    );
    let permissions = count("role_permissions").await;
    assert!(
        permissions > 100,
        "Rechte der Grundausstattung zugeordnet: {permissions}"
    );
    // Wiederholung ist unschädlich
    let again = kis_admin()
        .arg("migrate")
        .env("KIS_DATABASE__MIGRATION_URL", tdb.url())
        .env(
            "KIS_DATABASE__WRITE_URL",
            "postgres://kis_app:falsch@127.0.0.1:1/kis",
        )
        .output()
        .expect("kis-admin startet");
    assert!(again.status.success());
    assert_eq!(
        count("role_permissions").await,
        permissions,
        "Wiederholung ändert nichts"
    );
    tdb.drop().await;
}

/// `kis-admin modules` kommt ohne Datenbank und ohne Secrets aus.
#[test]
fn modules_are_listed_without_database() {
    let output = kis_admin()
        .arg("modules")
        .output()
        .expect("kis-admin startet");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for id in ["org", "patients", "cases", "orders", "roster"] {
        assert!(
            text.lines().any(|l| l.starts_with(id)),
            "Modul {id} gelistet:\n{text}"
        );
    }
}
