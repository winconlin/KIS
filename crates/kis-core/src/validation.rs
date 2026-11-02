// ============================================================
// KIS — Input-Validierung (Task 1.13)
//
// Zentrale Validierungsfunktionen für Patient-, Arzt- und
// Medikations-Service. Jede Funktion gibt `Ok(())` bei validen
// Inputs und `Err(String)` mit DEUTSCHER Fehlermeldung zurück.
//
// Die Handler sammeln alle Fehler in einem Array und antworten mit
// HTTP 400 + `{"success": false, "errors": [...]}`.
// ============================================================

use chrono::{Datelike, NaiveDate, Utc};

/// Ergebnis einer einzelnen Validierung.
pub type ValidationResult = Result<(), String>;

// ----------------------------------------------------------------
// Name: 2-100 Zeichen, nur Buchstaben (inkl. Umlaute), Bindestriche,
// Punkte, Leerzeichen (z. B. "Dr. Müller-Schmidt").
// ----------------------------------------------------------------
pub fn validate_name(s: &str) -> ValidationResult {
    let s = s.trim();
    if s.is_empty() {
        return Err("Name darf nicht leer sein".into());
    }
    let len = s.chars().count();
    if len < 2 || len > 100 {
        return Err(format!(
            "Name muss 2-100 Zeichen lang sein (aktuell: {len})"
        ));
    }
    // Erlaubt: Buchstaben (Unicode, deckt äöüÄÖÜß ab), Leerzeichen,
    // Bindestrich, Punkt, Apostroph (z. B. O'Brien), Bindestrich.
    for c in s.chars() {
        if !(c.is_alphabetic() || c == ' ' || c == '-' || c == '.' || c == '\'' || c == '’') {
            return Err(format!("Name enthält unzulässiges Zeichen: '{c}'"));
        }
    }
    Ok(())
}

// ----------------------------------------------------------------
// E-Mail: RFC-5322 vereinfacht — genau ein '@', Punkt nach dem '@'.
// ----------------------------------------------------------------
pub fn validate_email(s: &str) -> ValidationResult {
    let s = s.trim();
    let parts: Vec<&str> = s.splitn(2, '@').collect();
    if parts.len() != 2 {
        return Err("E-Mail muss genau ein '@'-Zeichen enthalten".into());
    }
    let (local, domain) = (parts[0], parts[1]);
    if local.is_empty() {
        return Err("E-Mail-Adresse: Teil vor '@' darf nicht leer sein".into());
    }
    if local.len() > 64 {
        return Err("E-Mail-Adresse: Local-Part zu lang (max. 64 Zeichen)".into());
    }
    if domain.len() < 3 || !domain.contains('.') {
        return Err("E-Mail-Adresse: Domain muss mindestens ein Punkt enthalten".into());
    }
    // Domain: nur Buchstaben, Ziffern, Punkte, Bindestriche
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("E-Mail-Adresse: Domain enthält unzulässige Zeichen".into());
    }
    // Kein Punkt als erstes/letztes Zeichen
    if domain.starts_with('.') || domain.ends_with('.') {
        return Err("E-Mail-Adresse: Domain darf nicht mit/ohne Punkt beginnen/endigen".into());
    }
    // Keine doppelten Punkte
    if domain.contains("..") {
        return Err("E-Mail-Adresse: Domain enthält doppelte Punkte".into());
    }
    Ok(())
}

// ----------------------------------------------------------------
// Telefon: +49 oder 0049, danach 10-15 Ziffern gesamt.
// Erlaubt: optionale Leerzeichen, Klammern, Bindestriche.
// ----------------------------------------------------------------
pub fn validate_phone(s: &str) -> ValidationResult {
    let s = s.trim();
    if s.is_empty() {
        return Err("Telefonnummer darf nicht leer sein".into());
    }

    // Nur Ziffern, '+', '-', ' ', '(' ')' erlauben
    if !s
        .chars()
        .all(|c| c.is_ascii_digit() || c == '+' || c == '-' || c == ' ' || c == '(' || c == ')')
    {
        return Err("Telefonnummer enthält unzulässige Zeichen".into());
    }

    // Ziffern extrahieren
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();

    // Ländercode prüfen: +49 oder 0049
    let normalized = s
        .replace(['+', '-', ' ', '(', ')'], "");
    let has_intl_prefix = normalized.starts_with("49") || normalized.starts_with("0049");
    if !has_intl_prefix {
        return Err(
            "Telefonnummer muss mit +49 oder 0049 beginnen".into(),
        );
    }

    // Ziffernanzahl nach dem Ländercode: 10-15 gesamt (inkl. 49)
    let digit_count = digits.len();
    if digit_count < 10 || digit_count > 15 {
        return Err(format!(
            "Telefonnummer muss 10-15 Ziffern haben (inkl. Ländercode, aktuell: {digit_count})"
        ));
    }
    Ok(())
}

// ----------------------------------------------------------------
// Datum: valides Datum (JJJJ-MM-TT), keine Zukunft (für Geburtsdatum).
// ----------------------------------------------------------------
pub fn validate_date(s: &str) -> ValidationResult {
    let s = s.trim();
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        "Datum muss das Format JJJJ-MM-TT haben (z. B. 1990-05-15)".to_string()
    })?;

    // Keine Zukunft (Geburtsdatum)
    let today = Utc::now().date_naive();
    if date > today {
        return Err("Datum darf nicht in der Zukunft liegen".into());
    }

    // Plausibilität: nicht vor 1900
    if date.year() < 1900 {
        return Err("Datum liegt zu weit in der Vergangenheit (vor 1900)".into());
    }

    Ok(())
}

// ----------------------------------------------------------------
// Versichertennummer: 10-18 Zeichen, nur Ziffern.
// ----------------------------------------------------------------
pub fn validate_id_number(s: &str) -> ValidationResult {
    let s = s.trim();
    if s.is_empty() {
        return Err("Versichertennummer darf nicht leer sein".into());
    }
    let len = s.chars().count();
    if len < 10 || len > 18 {
        return Err(format!(
            "Versichertennummer muss 10-18 Zeichen lang sein (aktuell: {len})"
        ));
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("Versichertennummer darf nur Ziffern enthalten".into());
    }
    Ok(())
}

// ----------------------------------------------------------------
// Adresse: 3-200 Zeichen.
// ----------------------------------------------------------------
pub fn validate_address(s: &str) -> ValidationResult {
    let s = s.trim();
    let len = s.chars().count();
    if len < 3 {
        return Err(format!(
            "Adresse muss mindestens 3 Zeichen lang sein (aktuell: {len})"
        ));
    }
    if len > 200 {
        return Err(format!(
            "Adresse darf höchstens 200 Zeichen lang sein (aktuell: {len})"
        ));
    }
    Ok(())
}

// ----------------------------------------------------------------
// PLZ: 5-stellige Postleitzahl.
// ----------------------------------------------------------------
pub fn validate_plz(s: &str) -> ValidationResult {
    let s = s.trim();
    if s.len() != 5 {
        return Err(format!(
            "PLZ muss genau 5 Stellen haben (aktuell: {})",
            s.len()
        ));
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("PLZ darf nur Ziffern enthalten".into());
    }
    Ok(())
}

// ----------------------------------------------------------------
// Stadt: 2-100 Zeichen (gleiche Regeln wie Name).
// ----------------------------------------------------------------
pub fn validate_stadt(s: &str) -> ValidationResult {
    let s = s.trim();
    let len = s.chars().count();
    if len < 2 {
        return Err(format!(
            "Stadt muss mindestens 2 Zeichen lang sein (aktuell: {len})"
        ));
    }
    if len > 100 {
        return Err(format!(
            "Stadt darf höchstens 100 Zeichen lang sein (aktuell: {len})"
        ));
    }
    // Gleiche Zeichen-Regeln wie Name
    for c in s.chars() {
        if !(c.is_alphabetic() || c == ' ' || c == '-' || c == '.' || c == '\'') {
            return Err(format!("Stadt enthält unzulässiges Zeichen: '{c}'"));
        }
    }
    Ok(())
}

// ----------------------------------------------------------------
// Sammel-Validierung für Patient-Felder.
//
// Gibt ein VEC der deutschen Fehlermeldungen zurück; leer = valide.
// ----------------------------------------------------------------
pub fn validate_patient_fields(
    vorname: Option<&str>,
    nachname: Option<&str>,
    email: Option<&str>,
    telefon: Option<&str>,
    versichertennummer: Option<&str>,
    strasse: Option<&str>,
    plz: Option<&str>,
    stadt: Option<&str>,
    geburtsdatum: Option<&str>,
) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    if let Some(v) = vorname {
        if let Err(e) = validate_name(v) {
            errors.push(format!("Vorname: {e}"));
        }
    }
    if let Some(v) = nachname {
        if let Err(e) = validate_name(v) {
            errors.push(format!("Nachname: {e}"));
        }
    }
    if let Some(v) = email {
        if let Err(e) = validate_email(v) {
            errors.push(format!("E-Mail: {e}"));
        }
    }
    if let Some(v) = telefon {
        if let Err(e) = validate_phone(v) {
            errors.push(format!("Telefon: {e}"));
        }
    }
    if let Some(v) = versichertennummer {
        if let Err(e) = validate_id_number(v) {
            errors.push(format!("Versichertennummer: {e}"));
        }
    }
    if let Some(v) = strasse {
        if let Err(e) = validate_address(v) {
            errors.push(format!("Straße: {e}"));
        }
    }
    if let Some(v) = plz {
        if let Err(e) = validate_plz(v) {
            errors.push(format!("PLZ: {e}"));
        }
    }
    if let Some(v) = stadt {
        if let Err(e) = validate_stadt(v) {
            errors.push(format!("Stadt: {e}"));
        }
    }
    if let Some(v) = geburtsdatum {
        if let Err(e) = validate_date(v) {
            errors.push(format!("Geburtsdatum: {e}"));
        }
    }
    errors
}

// ----------------------------------------------------------------
// Tests
// ----------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_name ---
    #[test]
    fn test_name_valid() {
        assert!(validate_name("Müller").is_ok());
        assert!(validate_name("Anna-Lisa Schmidt").is_ok());
        assert!(validate_name("Dr. O'Brien").is_ok());
        assert!(validate_name("ÄÖÜß").is_ok());
        assert!(validate_name("A").is_err()); // zu kurz
        let long = "a".repeat(101);
        assert!(validate_name(&long).is_err()); // zu lang
    }

    #[test]
    fn test_name_invalid() {
        assert!(validate_name("").is_err());
        assert!(validate_name("A1").is_err()); // Ziffer
        assert!(validate_name("Müller 123").is_err());
        assert!(validate_name("John@Doe").is_err());
    }

    // --- validate_email ---
    #[test]
    fn test_email_valid() {
        assert!(validate_email("a@b.de").is_ok());
        assert!(validate_email("user.name+tag@example.com").is_ok());
        assert!(validate_email("test@sub.domain.org").is_ok());
    }

    #[test]
    fn test_email_invalid() {
        assert!(validate_email("invalid").is_err());
        assert!(validate_email("@domain.de").is_err());
        assert!(validate_email("user@").is_err());
        assert!(validate_email("user@.de").is_err());
        assert!(validate_email("user@domain..de").is_err());
        assert!(validate_email("user@domain").is_err()); // kein Punkt
    }

    // --- validate_phone ---
    #[test]
    fn test_phone_valid() {
        assert!(validate_phone("+49 170 1234567").is_ok());
        assert!(validate_phone("00491701234567").is_ok());
        assert!(validate_phone("+49-170-1234567").is_ok());
    }

    #[test]
    fn test_phone_invalid() {
        assert!(validate_phone("").is_err());
        assert!(validate_phone("1701234567").is_err()); // kein +49
        assert!(validate_phone("+49 123").is_err()); // zu kurz
        assert!(validate_phone("+49 abc").is_err());
        let long = format!("+49 {}", "1".repeat(20));
        assert!(validate_phone(&long).is_err()); // zu lang
    }

    // --- validate_date ---
    #[test]
    fn test_date_valid() {
        assert!(validate_date("1990-05-15").is_ok());
        assert!(validate_date("2000-01-01").is_ok());
    }

    #[test]
    fn test_date_invalid() {
        assert!(validate_date("2999-01-01").is_err()); // Zukunft
        assert!(validate_date("1800-01-01").is_err()); // vor 1900
        assert!(validate_date("15-05-1990").is_err()); // falsches Format
        assert!(validate_date("not-a-date").is_err());
        assert!(validate_date("1990-13-01").is_err()); // ungültiger Monat
    }

    // --- validate_id_number ---
    #[test]
    fn test_id_number_valid() {
        assert!(validate_id_number("1234567890").is_ok());
        assert!(validate_id_number("123456789012345678").is_ok()); // 18 Ziffern
    }

    #[test]
    fn test_id_number_invalid() {
        assert!(validate_id_number("12345").is_err()); // zu kurz
        let long = "1".repeat(19);
        assert!(validate_id_number(&long).is_err()); // zu lang
        assert!(validate_id_number("123456789a").is_err()); // Buchstabe
    }

    // --- validate_address ---
    #[test]
    fn test_address_valid() {
        assert!(validate_address("Musterstraße 1").is_ok());
        assert!(validate_address("A 1").is_ok());
    }

    #[test]
    fn test_address_invalid() {
        assert!(validate_address("ab").is_err()); // zu kurz
        let long = "a".repeat(201);
        assert!(validate_address(&long).is_err()); // zu lang
    }

    // --- validate_plz ---
    #[test]
    fn test_plz_valid() {
        assert!(validate_plz("10115").is_ok());
        assert!(validate_plz("99999").is_ok());
    }

    #[test]
    fn test_plz_invalid() {
        assert!(validate_plz("1011").is_err()); // 4 Stellen
        assert!(validate_plz("101155").is_err()); // 6 Stellen
        assert!(validate_plz("1011a").is_err()); // Buchstabe
    }

    // --- validate_stadt ---
    #[test]
    fn test_stadt_valid() {
        assert!(validate_stadt("Berlin").is_ok());
        assert!(validate_stadt("Münster").is_ok());
    }

    #[test]
    fn test_stadt_invalid() {
        assert!(validate_stadt("B").is_err()); // zu kurz
        let long = "a".repeat(101);
        assert!(validate_stadt(&long).is_err()); // zu lang
    }

    // --- Sammel-Validierung ---
    #[test]
    fn test_patient_fields_all_valid() {
        let errors = validate_patient_fields(
            Some("Anna"),
            Some("Müller"),
            Some("anna@example.com"),
            Some("+49 170 1234567"),
            Some("1234567890"),
            Some("Musterstraße 1"),
            Some("10115"),
            Some("Berlin"),
            Some("1990-05-15"),
        );
        assert!(errors.is_empty(), "erwartete keine Fehler: {errors:?}");
    }

    #[test]
    fn test_patient_fields_collects_multiple_errors() {
        let errors = validate_patient_fields(
            Some("A"),             // Name zu kurz
            None,
            Some("invalid"),       // E-Mail falsch
            Some("+12 345"),       // Telefon falsch
            Some("12345"),         // ID zu kurz
            None,
            Some("1011"),          // PLZ zu kurz
            Some("B"),             // Stadt zu kurz
            Some("2999-01-01"),    // Datum Zukunft
        );
        assert!(errors.len() >= 7, "erwartete ≥7 Fehler, bekam: {errors:?}");
        let felder = [
            "Vorname", "E-Mail", "Telefon", "Versichertennummer",
            "PLZ", "Stadt", "Geburtsdatum"
        ];
        for e in &errors {
            assert!(
                felder.iter().any(|f| e.starts_with(f)),
                "Fehler muss mit Feldnamen beginnen, war: {e}"
            );
        }
    }
}
