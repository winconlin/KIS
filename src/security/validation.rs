//! Eingabevalidierung: E-Mail, Telefon, Datum, Namens-Sanitization
//!
//! Alle Funktionen sind reine Validatoren/Sanitizer — sie ändern den
//! Originalwert nicht, sondern liefern einen bereinigten Wert zurück.

use chrono::NaiveDate;

/// Validiert eine E-Mail-Adresse (einfaches, robustes Pattern).
///
/// Akzeptiert: local@domain.tld — lokal max. 64, Domain mit mind. 2 Zeichen
/// pro Label, TLD mind. 2 Zeichen.
pub fn validate_email(email: &str) -> bool {
    let email = email.trim();
    let at_pos = email.find('@');
    let (Some(at), _) = (at_pos, email.len()) else {
        return false;
    };
    if at == 0 || at == email.len() - 1 {
        return false;
    }
    let local = &email[..at];
    let domain = &email[at + 1..];

    // Lokalteil: 1..64, erlaubte Zeichen
    if local.is_empty() || local.len() > 64 {
        return false;
    }
    if !local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '!'))
    {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }

    // Domain: Labels getrennt durch Punkte, mind. 2 Labels, TLD mind. 2 Zeichen
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for label in &labels {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
    }
    let tld = *labels.last().unwrap();
    tld.len() >= 2 && !tld.chars().next().unwrap().is_ascii_digit()
}

/// Validiert eine Telefonnummer (deutsches Format, auch international).
///
/// Akzeptiert: +49 30 123456, 030 123456, (030) 123456, +4930123456
pub fn validate_phone(phone: &str) -> bool {
    let digits_only: String = phone
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+' && phone.starts_with('+'))
        .collect();
    // Mind. 6, max. 15 Ziffern (nach +)
    let count = digits_only.chars().filter(|c| c.is_ascii_digit()).count();
    if !(6..=15).contains(&count)
        || phone.matches('+').count() > 1
        || (phone.contains('+') && !phone.starts_with('+'))
    {
        return false;
    }
    // Nur erlaubte Zeichen: Ziffern, Leerzeichen, Klammern, Bindestrich, Plus
    phone
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '(' | ')' | '-' | '+'))
}

/// Validiert ein Datum im Format YYYY-MM-DD und prüft die Plausibilität.
pub fn validate_date(date: &str) -> bool {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
}

/// Sanitisiert einen Namen: entfernt Steuerzeichen, begrenzt die Länge,
/// normalisiert Leerzeichen.
///
/// Erlaubt: Buchstaben (inkl. Umlaute), Leerzeichen, Bindestrich, Punkt,
/// Anführungszeichen für Mittelnamen.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            c.is_alphanumeric()
                || matches!(
                    c,
                    ' ' | '-' | '.' | '\'' | '’' | 'à' | 'ä' | 'ö' | 'ü' | 'ß' | 'Ä' | 'Ö' | 'Ü'
                )
        })
        .collect();
    // Mehrere Leerzeichen zu einem reduzieren
    let normalized: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // Auf 200 Zeichen begrenzen (Name sollte nicht länger sein)
    normalized.chars().take(200).collect()
}

/// Sanitisiert einen freien Text (z. B. Notizen): entfernt Steuerzeichen
/// (außer Zeilenumbruch und Tab), begrenzt die Länge.
pub fn sanitize_text(text: &str, max_len: usize) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    cleaned.chars().take(max_len).collect()
}

/// Prüft, ob eine String-Eingabe leer (oder nur Leerzeichen) ist.
pub fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_valid() {
        assert!(validate_email("a@b.de"));
        assert!(validate_email("test.name+tag@sub.example.com"));
    }

    #[test]
    fn email_invalid() {
        assert!(!validate_email(""));
        assert!(!validate_email("no-at"));
        assert!(!validate_email("@no-local.de"));
        assert!(!validate_email("no-domain@"));
        assert!(!validate_email("double..dot@b.de"));
        assert!(!validate_email("a@b"));
    }

    #[test]
    fn phone_valid() {
        assert!(validate_phone("+49 30 123456"));
        assert!(validate_phone("030 123456"));
        assert!(validate_phone("(030) 123456"));
        assert!(validate_phone("+4930123456"));
    }

    #[test]
    fn phone_invalid() {
        assert!(!validate_phone(""));
        assert!(!validate_phone("123"));
        assert!(!validate_phone("abc def ghi"));
    }

    #[test]
    fn date_valid() {
        assert!(validate_date("1990-05-15"));
        assert!(!validate_date("1990-13-45"));
        assert!(!validate_date("15.05.1990"));
    }

    #[test]
    fn name_sanitization() {
        assert_eq!(sanitize_name("  Max  Mustermann "), "Max Mustermann");
        assert_eq!(sanitize_name("Hans<>\nMüller"), "HansMüller");
        let long = "a".repeat(500);
        assert_eq!(sanitize_name(&long).len(), 200);
    }
}
