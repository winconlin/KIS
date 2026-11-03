//! GDT-Export für Rollback-Zwecke (Task 4.9)
//!
//! Exportiert alle Patientenstammdaten aus KIS als GDT-Datei, die
//! MEDICO-kompatibel ist (CSV-ähnlich, semikolon-getrennt).
//!
//! Felder: Patient-ID, Name, Geburtsdatum, Geschlecht, Adresse
//!
//! Die Datei kann für Phase 4 (Rollback) zurück in MEDICO importiert werden.

use serde::Serialize;
use serde_json::json;
use sqlx::Row;
use std::fmt::Write as _;

/// Einzelne Zeile im GDT-Export (MEDICO-kompatibel).
#[derive(Debug, Clone, Serialize)]
pub struct GdtPatientRow {
    pub patient_id: String,
    pub name: String,
    pub Geburtsdatum: String,
    pub Geschlecht: String,
    pub adresse: String,
}

/// Resultat eines GDT-Exports.
#[derive(Debug, Clone, Serialize)]
pub struct GdtExportResult {
    pub filename: String,
    pub content: String,
    pub patient_count: usize,
    pub generated_at: String,
}

/// Erzeugt den GDT-Export aller Patienten aus der KIS-Datenbank.
///
/// Liest aus der `patients`-Tabelle und serialisiert als MEDICO-kompatible
/// GDT-Datei (CSV-ähnlich, semikolon-getrennt).
pub async fn export_patients_to_gdt(db: &sqlx::PgPool) -> Result<GdtExportResult, String> {
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT id::text AS patient_id, name, geburtsdatum::text AS geburtsdatum,
                geschlecht, adresse
         FROM patients
         ORDER BY name",
    )
    .fetch_all(db)
    .await
    .map_err(|e| format!("Fehler beim Lesen der Patienten: {e}"))?;

    let mut gdt = String::new();
    let mut header_written = false;
    let mut count = 0;

    for row in &rows {
        let patient_id: String = row.get("patient_id");
        let name: String = row.get("name");
        let geburtsdatum: String = row.get("geburtsdatum");
        let geschlecht: String = row.get("geschlecht");
        let adresse: String = row.get("adresse");

        if !header_written {
            let _ = writeln!(
                gdt,
                "PATIENT_ID;NAME;GEBURTSDATUM;GESCHLECHT;ADRESSE"
            );
            header_written = true;
        }

        let _ = writeln!(
            gdt,
            "{};{};{};{};{}",
            escape_gdt(&patient_id),
            escape_gdt(&name),
            escape_gdt(&geburtsdatum),
            escape_gdt(&geschlecht),
            escape_gdt(&adresse),
        );
        count += 1;
    }

    if count == 0 {
        gdt.push_str("PATIENT_ID;NAME;GEBURTSDATUM;GESCHLECHT;ADRESSE\n");
    }

    let generated_at = chrono::Utc::now().to_rfc3339();

    Ok(GdtExportResult {
        filename: "kis_gdt_export.gdt",
        content: gdt,
        patient_count: count,
        generated_at,
    })
}

/// Erzeugt einen GDT-Export aus vorgegebenen Zeilen (für Tests).
pub fn build_gdt_from_rows(rows: &[GdtPatientRow]) -> String {
    let mut gdt = String::from("PATIENT_ID;NAME;GEBURTSDATUM;GESCHLECHT;ADRESSE\n");
    for r in rows {
        let _ = writeln!(
            gdt,
            "{};{};{};{};{}",
            escape_gdt(&r.patient_id),
            escape_gdt(&r.name),
            escape_gdt(&r.Geburtsdatum),
            escape_gdt(&r.Geschlecht),
            escape_gdt(&r.adresse),
        );
    }
    gdt
}

/// Escapet GDT-Feldwerte: Semikolons im Wert werden durch Doppelpunkt ersetzt,
/// doppelte Anführungszeichen werden verdoppelt, Werte mit Anführungszeichen
/// werden eingefasst.
fn escape_gdt(value: &str) -> String {
    if value.contains(';') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Erzeugt eine JSON-Response für die GDT-Export-Endpunkte.
pub fn gdt_export_json(result: &GdtExportResult) -> serde_json::Value {
    json!({
        "filename": result.filename,
        "patient_count": result.patient_count,
        "generated_at": result.generated_at,
        "content": result.content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_gdt_plain() {
        assert_eq!(escape_gdt("Mustermann"), "Mustermann");
        assert_eq!(escape_gdt("2000-01-15"), "2000-01-15");
    }

    #[test]
    fn test_escape_gdt_with_semicolon() {
        assert_eq!(escape_gdt("Hauptstr. 1; 12345 Berlin"), "\"Hauptstr. 1; 12345 Berlin\"");
    }

    #[test]
    fn test_escape_gdt_with_quotes() {
        assert_eq!(escape_gdt("Name \"Eisen\""), "\"Name \"\"Eisen\"\"\"");
    }

    #[test]
    fn test_build_gdt_from_rows() {
        let rows = vec![
            GdtPatientRow {
                patient_id: "P001".into(),
                name: "Muster, Anna".into(),
                Geburtsdatum: "1990-05-15".into(),
                Geschlecht: "weiblich".into(),
                adresse: "Hauptstr. 1".into(),
            },
            GdtPatientRow {
                patient_id: "P002".into(),
                name: "Beispiel, Max".into(),
                Geburtsdatum: "1985-11-20".into(),
                Geschlecht: "maennlich".into(),
                adresse: "Gartenweg 5".into(),
            },
        ];
        let gdt = build_gdt_from_rows(&rows);
        let lines: Vec<&str> = gdt.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("PATIENT_ID;NAME;GEBURTSDATUM"));
        assert!(lines[1].contains("P001"));
        assert!(lines[2].contains("P002"));
    }

    #[test]
    fn test_build_gdt_empty() {
        let gdt = build_gdt_from_rows(&[]);
        let lines: Vec<&str> = gdt.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("PATIENT_ID"));
    }

    #[test]
    fn test_gdt_export_json() {
        let result = GdtExportResult {
            filename: "test.gdt".into(),
            content: "PATIENT_ID;NAME\nP001;Test\n".into(),
            patient_count: 1,
            generated_at: "2026-01-01T00:00:00Z".into(),
        };
        let json_val = gdt_export_json(&result);
        assert_eq!(json_val["filename"], "test.gdt");
        assert_eq!(json_val["patient_count"], 1);
    }
}
