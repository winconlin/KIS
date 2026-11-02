// ============================================================
// KIS — Query-Optimizer (Task 5.5: Index-Strategie für 10.000+ Patienten)
//
// Hilfsfunktionen zur Analyse und Optimierung von SQL-Queries
// gegen PostgreSQL:
//   - `QueryPlan`: Struktur die den Index-Nutzungsgrad schätzt
//   - `suggest_indexes()`: Index-Empfehlungen für häufige Suchfelder
//   - `estimate_rows()`: EXPLAIN ANALYZE zur Zeilen-Schätzung
//   - `explain_query()`: Query-Plan als JSON (EXPLAIN-Profiler)
//
// Die Module funktioniert mit `Database::empty_for_tests()` in
// Unit-Tests — EXPLAIN-Queries erfordern eine echte Verbindung,
// daher sind die Tests auf `suggest_indexes()` und `QueryPlan`-
// Schätzung fokussiert.
// ============================================================

use sqlx::PgPool;
use sqlx::Row;
use tracing::debug;

/// Geschätzter Query-Plan mit Index-Nutzungsgrad (Task 5.5).
///
/// `index_usage` ist ein Wert zwischen 0.0 (kein Index genutzt,
/// Seq-Scan) und 1.0 (perfekter Index-Hit). Die Schätzung basiert
/// auf Heuristiken aus dem EXPLAIN-Output:
/// - `Seq Scan` auf großen Tabellen → niedriger Wert
/// - `Index Scan` / `Index Only Scan` → hoher Wert
/// - `Bitmap Index Scan` → mittlerer Wert
/// - `Rows Removed by Filter` → Strafe
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueryPlan {
    /// Der EXPLAIN-Text (human-readable)
    pub explain_text: String,
    /// Geschätzter Index-Nutzungsgrad (0.0 – 1.0)
    pub index_usage: f64,
    /// Geschätzte Zeilenanzahl (aus "rows=" im EXPLAIN-Output)
    pub estimated_rows: i64,
    /// Geschätzte Kosten (aus "cost=" im EXPLAIN-Output)
    pub estimated_cost: f64,
    /// Enthält der Plan einen Seq-Scan?
    pub has_seq_scan: bool,
    /// Enthält der Plan einen Index-Scan?
    pub has_index_scan: bool,
    /// Enthält der Plan einen Sort?
    pub has_sort: bool,
    /// Enthält der Plan einen Join?
    pub has_join: bool,
}

impl QueryPlan {
    /// Schätzt den Index-Nutzungsgrad aus dem EXPLAIN-Text.
    ///
    /// Heuristik:
    /// - Startwert 1.0 (perfekt)
    /// - `-0.5` pro `Seq Scan` auf einer Tabelle mit >1000 geschätzten Zeilen
    /// - `-0.3` pro `Rows Removed by Filter`
    /// - `-0.1` pro `Sort`
    /// - Minimum 0.0
    pub fn estimate_index_usage(explain_text: &str) -> f64 {
        let mut score = 1.0;

        // Seq Scan: Haupt-Kosten-Treiber
        if explain_text.contains("Seq Scan on") {
            score -= 0.5;
        }

        // Rows Removed by Filter: Daten gefiltert, aber nicht vorab eingeschränkt
        let filter_count = explain_text
            .lines()
            .filter(|l| l.contains("Rows Removed by Filter"))
            .count();
        score -= filter_count as f64 * 0.15;

        // Sort: zusätzlicher Overhead
        if explain_text.contains("Sort Method") || explain_text.contains("Sort Key") {
            score -= 0.1;
        }

        // Clamp auf [0.0, 1.0]
        score.clamp(0.0, 1.0)
    }

    /// Löst den geschätzten Index-Nutzungsgrad aus einem EXPLAIN-Text.
    pub fn parse(explain_text: &str) -> Self {
        let has_seq_scan = explain_text.contains("Seq Scan on");
        let has_index_scan = explain_text
            .contains("Index Scan using")
            || explain_text.contains("Index Only Scan using")
            || explain_text.contains("Bitmap Index Scan on");
        let has_sort = explain_text.contains("Sort Method") || explain_text.contains("Sort Key");
        let has_join = explain_text.contains("Hash Join")
            || explain_text.contains("Merge Join")
            || explain_text.contains("Nested Loop");

        // Geschätzte Zeilen: letzte "rows=" Angabe
        let estimated_rows = explain_text
            .lines()
            .rev()
            .find_map(|line| {
                line.split("rows=").nth(1).and_then(|r| {
                    r.split_whitespace().next()?.parse::<i64>().ok()
                })
            })
            .unwrap_or(0);

        // Geschätzte Kosten: "cost=... total=..."
        let estimated_cost = explain_text
            .lines()
            .find_map(|line| {
                let cost_part = line.split("cost=").nth(1)?;
                cost_part
                    .split_whitespace()
                    .next()?
                    .split('.')
                    .next()?
                    .parse::<f64>()
                    .ok()
            })
            .unwrap_or(0.0);

        let index_usage = Self::estimate_index_usage(explain_text);

        Self {
            explain_text: explain_text.to_string(),
            index_usage,
            estimated_rows,
            estimated_cost,
            has_seq_scan,
            has_index_scan,
            has_sort,
            has_join,
        }
    }
}

/// Empfohlene Indexe für häufige Suchfelder (Task 5.5).
///
/// Basierend auf den KIS-Domain-Typen und den Migrationen:
///
/// | Tabelle            | Häufige Suchfelder                          |
/// |--------------------|---------------------------------------------|
/// | patients           | personal_number, date_of_birth, family_name  |
/// | doctors            | personal_number, specialty                   |
/// | departments        | name                                         |
/// | medications        | name, medication_class                       |
/// | lab_tests          | order_date, result_value, test_name          |
/// | diagnoses          | ics_code, diagnosis_date, patient_id         |
/// | appointments       | appointment_date, patient_id, doctor_id      |
/// | medication_orders  | order_date, patient_id, medication_id        |
/// | clinical_notes     | note_date, patient_id                        |
/// | visits             | visit_date, patient_id                       |
///
/// Gibt `CREATE INDEX`-Statements zurück (nur die, die noch nicht
/// existieren — die Existenz wird hier NICHT geprüft, das ist
/// Aufgabe des Aufrufers).
pub fn suggest_indexes(table: &str, columns: &[&str]) -> Vec<String> {
    let table = table.to_lowercase();
    let columns_lower: Vec<String> = columns.iter().map(|c| c.to_lowercase()).collect();

    // Basiskolonen die immer indiziert werden sollten (PK/FK)
    let mut base_columns: Vec<String> = vec![];
    let mut search_columns: Vec<String> = vec![];

    match table.as_str() {
        "patients" => {
            base_columns = vec!["id".into()];
            search_columns = vec![
                "personal_number".into(),
                "date_of_birth".into(),
                "family_name".into(),
            ];
        }
        "doctors" => {
            base_columns = vec!["id".into()];
            search_columns = vec!["personal_number".into(), "specialty".into()];
        }
        "departments" => {
            base_columns = vec!["id".into()];
            search_columns = vec!["name".into()];
        }
        "medications" => {
            base_columns = vec!["id".into()];
            search_columns = vec!["name".into(), "medication_class".into()];
        }
        "lab_tests" => {
            base_columns = vec!["id".into(), "patient_id".into()];
            search_columns = vec!["order_date".into(), "test_name".into()];
        }
        "diagnoses" => {
            base_columns = vec!["id".into(), "patient_id".into()];
            search_columns = vec!["ics_code".into(), "diagnosis_date".into()];
        }
        "appointments" => {
            base_columns = vec!["id".into(), "patient_id".into(), "doctor_id".into()];
            search_columns = vec!["appointment_date".into()];
        }
        "medication_orders" => {
            base_columns = vec!["id".into(), "patient_id".into(), "medication_id".into()];
            search_columns = vec!["order_date".into()];
        }
        "clinical_notes" => {
            base_columns = vec!["id".into(), "patient_id".into()];
            search_columns = vec!["note_date".into()];
        }
        "visits" => {
            base_columns = vec!["id".into(), "patient_id".into()];
            search_columns = vec!["visit_date".into()];
        }
        _ => {
            // Unbekannte Tabelle: nutze die übergebenen Spalten
            search_columns = columns_lower.clone();
        }
    }

    // Merge: basiskolonen + relevante Suchfelder + explizit angeforderte
    let mut all_columns: Vec<String> = base_columns.clone();
    for col in &columns_lower {
        if !all_columns.contains(col) && !search_columns.contains(col) {
            all_columns.push(col.clone());
        }
    }
    for col in &search_columns {
        if !all_columns.contains(col) {
            all_columns.push(col.clone());
        }
    }

    // Deduplizieren, Order erhalten
    let mut seen = std::collections::HashSet::new();
    let unique_columns: Vec<String> = all_columns
        .into_iter()
        .filter(|c| seen.insert(c.clone()))
        .collect();

    if unique_columns.is_empty() {
        return vec![];
    }

    // Ein zusammengesetzter Index über alle relevanten Spalten
    let cols_str = unique_columns.join(", ");
    let idx_name = format!("idx_{}_{}", table, unique_columns.join("_"));

    vec![format!(
        "CREATE INDEX IF NOT EXISTS {} ON {} ({})",
        idx_name, table, cols_str
    )]
}

/// Schätzt die Zeilenanzahl einer Query mit `EXPLAIN ANALYZE` (Task 5.5).
///
/// Führt `EXPLAIN ANALYZE <query>` aus und parst die "actual rows"
/// aus dem Output. Fallback auf "estimated rows", falls ANALYZE
/// nicht verfügbar ist.
///
/// ⚠️ Führt die Query wirklich aus — nur für read-only Queries
/// verwenden!
pub async fn estimate_rows(table: &str, filter: &str, pool: &PgPool) -> anyhow::Result<i64> {
    // Escape: doppelte Single-Quotes
    let table = table.replace('\'', "''");
    let filter = filter.replace('\'', "''");

    // EXPLAIN (FORMAT JSON) liefert strukturiertes JSON mit "actual rows"
    let explain_sql = format!(
        "EXPLAIN (ANALYZE, FORMAT JSON) SELECT count(*) FROM {} WHERE {}",
        table, filter
    );

    let row = sqlx::query(&explain_sql)
        .fetch_one(pool)
        .await
        .map_err(|e| anyhow::anyhow!("EXPLAIN ANALYZE für '{}' fehlgeschlagen: {}", table, e))?;

    let json_text: String = row.try_get(0).map_err(|e: sqlx::Error| {
        anyhow::anyhow!("EXPLAIN-Row konnte nicht gelesen werden: {}", e)
    })?;
    let json: serde_json::Value = serde_json::from_str(&json_text)?;

    // EXPLAIN (FORMAT JSON) liefert ein Array mit einem Element
    let plan = json
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("Unerwartetes EXPLAIN-Format"))?;

    // "Actual Total Time" und "actual rows" finden
    extract_actual_rows(plan)
        .or_else(|| extract_estimated_rows(plan))
        .ok_or_else(|| anyhow::anyhow!("Keine rows-Angabe im EXPLAIN-Output für '{}'", table))
}

/// Liest "actual rows" aus einem EXPLAIN-JSON-Node (rekursiv).
fn extract_actual_rows(node: &serde_json::Value) -> Option<i64> {
    if let Some(rows) = node.get("actual rows").and_then(|v| v.as_f64()) {
        return Some(rows.round() as i64);
    }
    if let Some(children) = node.get("Plans").and_then(|v| v.as_array()) {
        for child in children {
            if let Some(rows) = extract_actual_rows(child) {
                return Some(rows);
            }
        }
    }
    None
}

/// Liest "Plan Rows" (geschätzte Zeilen) aus einem EXPLAIN-JSON-Node.
fn extract_estimated_rows(node: &serde_json::Value) -> Option<i64> {
    if let Some(rows) = node.get("Plan Rows").and_then(|v| v.as_f64()) {
        return Some(rows.round() as i64);
    }
    if let Some(children) = node.get("Plans").and_then(|v| v.as_array()) {
        for child in children {
            if let Some(rows) = extract_estimated_rows(child) {
                return Some(rows);
            }
        }
    }
    None
}

/// EXPLAIN-Profiler: liefert den Query-Plan als JSON (Task 5.5).
///
/// Führt `EXPLAIN (FORMAT JSON, ANALYZE) <query>` aus und gibt den
/// kompletten Plan als `serde_json::Value` zurück. Zusätzlich wird
/// ein `QueryPlan`-Objekt mit der Index-Usage-Schätzung eingebettet.
///
/// ⚠️ Führt die Query wirklich aus — nur für read-only Queries!
///
/// Beispiel-Output-Struktur:
/// ```json
/// {
///   "plan": [ ... EXPLAIN-JSON ... ],
///   "query_plan": {
///     "index_usage": 0.5,
///     "estimated_rows": 1234,
///     "has_seq_scan": true,
///     ...
///   }
/// }
/// ```
pub async fn explain_query(query: &str, pool: &PgPool) -> anyhow::Result<serde_json::Value> {
    // EXPLAIN (FORMAT JSON, ANALYZE) liefert ein JSON-Array
    let explain_sql = format!("EXPLAIN (FORMAT JSON, ANALYZE) {}", query);

    let row = sqlx::query(&explain_sql)
        .fetch_one(pool)
        .await
        .map_err(|e| anyhow::anyhow!("EXPLAIN fehlgeschlagen: {}", e))?;

    let json_text: String = row.try_get(0).map_err(|e: sqlx::Error| {
        anyhow::anyhow!("EXPLAIN-Row konnte nicht gelesen werden: {}", e)
    })?;
    let plan_json: serde_json::Value = serde_json::from_str(&json_text)?;

    // Human-readable EXPLAIN für die QueryPlan-Schätzung
    let plain_rows = sqlx::query(&format!("EXPLAIN {}", query))
        .fetch_all(pool)
        .await
        .map_err(|e| anyhow::anyhow!("EXPLAIN (plain) fehlgeschlagen: {}", e))?;
    // EXPLAIN (plain) liefert mehrere Zeilen mit je einer Spalte
    let lines: Vec<String> = plain_rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .collect();
    let explain_text = lines.join("\n");

    let query_plan = QueryPlan::parse(&explain_text);

    Ok(serde_json::json!({
        "plan": plan_json,
        "query_plan": query_plan,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// Task 5.5: `suggest_indexes` liefert sinnvolle Empfehlungen
    /// für die patients-Tabelle.
    #[test]
    fn test_suggest_indexes_patients() {
        let indexes = suggest_indexes("patients", &["personal_number", "date_of_birth"]);
        assert!(!indexes.is_empty());

        let idx = &indexes[0];
        assert!(idx.starts_with("CREATE INDEX IF NOT EXISTS"));
        assert!(idx.contains("patients"));
        assert!(idx.contains("personal_number"));
        assert!(idx.contains("date_of_birth"));
    }

    /// Task 5.5: `suggest_indexes` für diagnoses (FK + ICS-Code).
    #[test]
    fn test_suggest_indexes_diagnoses() {
        let indexes = suggest_indexes("diagnoses", &["ics_code"]);
        assert!(!indexes.is_empty());
        let idx = &indexes[0];
        assert!(idx.contains("diagnoses"));
        assert!(idx.contains("ics_code"));
        assert!(idx.contains("patient_id"), "FK patient_id sollte indiziert sein");
    }

    /// Task 5.5: Unbekannte Tabelle → nutze die übergebenen Spalten.
    #[test]
    fn test_suggest_indexes_unknown_table() {
        let indexes = suggest_indexes("custom_table", &["col_a", "col_b"]);
        assert!(!indexes.is_empty());
        let idx = &indexes[0];
        assert!(idx.contains("custom_table"));
        assert!(idx.contains("col_a"));
        assert!(idx.contains("col_b"));
    }

    /// Task 5.5: Leere Spaltenliste bei unbekannter Tabelle → keine Empfehlung.
    #[test]
    fn test_suggest_indexes_empty_columns() {
        let indexes = suggest_indexes("unknown", &[]);
        assert!(indexes.is_empty());
    }

    /// Task 5.5: `QueryPlan::parse` erkennt Seq-Scan.
    #[test]
    fn test_query_plan_seq_scan() {
        let explain = " Seq Scan on patients  (cost=0.00..350.00 rows=10000 width=80)\n   Filter: (date_of_birth < '2000-01-01')";
        let plan = QueryPlan::parse(explain);
        assert!(plan.has_seq_scan);
        assert!(!plan.has_index_scan);
        assert!(
            plan.index_usage <= 0.5,
            "Reiner Seq-Scan (ohne Filter/Sort) ergibt genau 1.0 − 0.5 = 0.5, ist {}",
            plan.index_usage
        );
        assert_eq!(plan.estimated_rows, 10000);
    }

    /// Task 5.5: `QueryPlan::parse` erkennt Index-Scan.
    #[test]
    fn test_query_plan_index_scan() {
        let explain = " Index Scan using idx_patients_personal_number on patients  (cost=0.42..8.44 rows=1 width=80)\n   Index Cond: (personal_number = '123456789')";
        let plan = QueryPlan::parse(explain);
        assert!(!plan.has_seq_scan);
        assert!(plan.has_index_scan);
        assert!(
            plan.index_usage > 0.5,
            "Index-Scan sollte hohen Index-Usage haben, ist {}",
            plan.index_usage
        );
    }

    /// Task 5.5: `QueryPlan::parse` erkennt Sort.
    #[test]
    fn test_query_plan_sort() {
        let explain = " Sort  (cost=100.00..110.00 rows=1000 width=80)\n   Sort Key: date_of_birth DESC\n   ->  Index Scan using idx on patients";
        let plan = QueryPlan::parse(explain);
        assert!(plan.has_sort);
    }

    /// Task 5.5: `QueryPlan::parse` erkennt Join.
    #[test]
    fn test_query_plan_join() {
        let explain = " Hash Join  (cost=50.00..200.00 rows=500 width=80)\n   ->  Seq Scan on patients\n   ->  Seq Scan on diagnoses";
        let plan = QueryPlan::parse(explain);
        assert!(plan.has_join);
    }

    /// Task 5.5: `QueryPlan` serialisiert (für API-Response).
    #[test]
    fn test_query_plan_serializes() {
        let explain = " Seq Scan on patients  (cost=0.00..100.00 rows=1000 width=80)";
        let plan = QueryPlan::parse(explain);
        let v = serde_json::to_value(&plan).unwrap();
        assert!(v["index_usage"].is_number());
        assert_eq!(v["has_seq_scan"], true);
        assert_eq!(v["estimated_rows"], 1000);
    }

    /// Task 5.5: `QueryPlan::estimate_index_usage` clamp auf [0,1].
    #[test]
    fn test_index_usage_clamped() {
        // Extrem schlechter Plan: Seq Scan + Filter + Sort
        let bad = "Seq Scan on patients\nRows Removed by Filter: 9000\nSort Method: quicksort";
        let usage = QueryPlan::estimate_index_usage(bad);
        assert!(usage >= 0.0 && usage <= 1.0);
        assert!(usage < 0.5);

        // Perfekter Plan: Index Only Scan
        let good = "Index Only Scan using idx on patients";
        let usage_good = QueryPlan::estimate_index_usage(good);
        assert!(usage_good > 0.5);
    }

    /// Task 5.5: `estimate_rows` und `explain_query` geben ohne DB-
    /// Verbindung einen sauberen Fehler zurück.
    #[tokio::test]
    async fn test_estimate_rows_no_connection() {
        let db = Database::empty_for_tests();
        let pool = db.read();

        let result = estimate_rows("patients", "id = 1", pool).await;
        assert!(
            result.is_err(),
            "Ohne DB-Verbindung muss estimate_rows einen Fehler liefern"
        );
    }

    /// Task 5.5: `explain_query` ohne DB-Verbindung → saubere Fehlermeldung.
    #[tokio::test]
    async fn test_explain_query_no_connection() {
        let db = Database::empty_for_tests();
        let pool = db.read();

        let result = explain_query("SELECT * FROM patients", pool).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("EXPLAIN"),
            "Fehlermeldung sollte 'EXPLAIN' enthalten, ist: {}",
            err_msg
        );
    }

    /// Task 5.5: `suggest_indexes` ist idempotent (gleiche Eingabe →
    /// gleiches Ergebnis).
    #[test]
    fn test_suggest_indexes_deterministic() {
        let a = suggest_indexes("patients", &["personal_number"]);
        let b = suggest_indexes("patients", &["personal_number"]);
        assert_eq!(a, b);
    }

    /// Task 5.5: Doppelte Spalten werden dedupliziert.
    #[test]
    fn test_suggest_indexes_dedup() {
        let indexes = suggest_indexes("patients", &["personal_number", "personal_number"]);
        let idx = &indexes[0];
        // Nur die Spaltenliste (nach "ON patients (…)") zählt — der
        // Index-Name enthält die Spalten ebenfalls, aber kein doppeltes
        // Spaltenfeld in der Klammer.
        let cols_part = idx
            .split(" ON ")
            .nth(1)
            .expect("Index-Statement muss 'ON <table> (…)' enthalten")
            .split('(')
            .nth(1)
            .and_then(|c| c.split(')').next())
            .expect("Spaltenliste in Klammern");
        let count = cols_part.split(',').filter(|c| c.trim() == "personal_number").count();
        assert_eq!(count, 1, "personal_number sollte nur einmal in der Spaltenliste vorkommen: {}", idx);
    }
}
