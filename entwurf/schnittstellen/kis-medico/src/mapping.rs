//! FHIR→KIS-Mapping-Logik für den MEDICO-Import (Tasks 4.2–4.6).
//!
//! Jede Mapping-Funktion ist rein (keine DB-Zugriffe), dadurch unit-testbar.
//! FHIR-Ressourcen werden als `serde_json::Value` übergeben (der FHIR-Server
//! liefert beliebige R4-Profil-Erweiterungen; wir mappen nur die Felder, die
//! KIS kennt, und ignorieren den Rest).

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;

use kis_patients::Patient;

/// Fehler bei der FHIR→KIS-Conversion (fehlende Pflichtfelder).
#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    #[error("Feld '{0}' fehlt oder hat ungültiges Format")]
    Field(String),
}

/// Liest einen String-Value aus einem FHIR-Objekt.
fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(|s| s.to_string())
}

/// Liest einen String aus einem FHIR-CodeableConcept (`coding[0].display`
/// oder `text`).
fn concept_display(v: &Value, key: &str) -> Option<String> {
    let cc = v.get(key)?;
    if let Some(text) = cc.get("text").and_then(|t| t.as_str()) {
        return Some(text.to_string());
    }
    cc.get("coding")?
        .as_array()?
        .first()?
        .get("display")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
}

/// FHIR `instant`/`dateTime` → `DateTime<Utc>` (ISO-8601).
fn dt_of(v: &Value, key: &str) -> Result<Option<DateTime<Utc>>, MappingError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => raw
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| Ok(Some(d.with_timezone(&Utc))))
            .or_else(|| {
                // Fallback: Datum ohne Zeit (z. B. "2025-06-15")
                raw.as_str()
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .map(|d| Ok(Some(d.and_hms_opt(0, 0, 0).unwrap().and_utc())))
            })
            .unwrap_or(Err(MappingError::Field(format!(
                "{key} (ungültiges ISO-8601-Datum: {raw:?})"
            )))),
    }
}

/// FHIR-Geschlecht (male/female/other/unknown) → KIS-Konvention (m/w/x).
pub fn fhir_gender(v: &Value) -> &'static str {
    match v.get("gender").and_then(|g| g.as_str()) {
        Some("male") => "m",
        Some("female") => "w",
        Some("other") => "x",
        _ => "x",
    }
}

/// FHIR `Patient` → KIS [`Patient`].
///
/// Mapped werden: `name[0]` (family/given), `birthDate`, `gender`,
/// `identifier` (Versicherten-ID → `insurance_number`), `address[0]`,
/// `telecom` (phone/email) und `id` (→ `external_id`).
///
/// `id` (internes Uuid) und `created_at`/`updated_at`/`deleted_at`/`version`
/// werden vom Importer gesetzt — hier Platzhalter.
pub fn map_patient_fhir(v: &Value) -> Result<Patient, MappingError> {
    let external_id = v
        .get("id")
        .and_then(|i| i.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("identifier")
                .and_then(|ids| ids.as_array())
                .and_then(|ids| ids.first())
                .and_then(|id| id.get("value").and_then(|val| val.as_str()))
                .map(|s| s.to_string())
        });

    let name = v
        .get("name")
        .and_then(|names| names.as_array())
        .and_then(|names| names.first());

    let name_family = name
        .and_then(|n| str_of(n, "family"))
        .unwrap_or_else(|| "Unbekannt".to_string());
    let name_given = name
        .and_then(|n| {
            n.get("given")
                .and_then(|g| g.as_array())
                .and_then(|g| g.first())
                .and_then(|g| g.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let birth_date = match v.get("birthDate").and_then(|b| b.as_str()) {
        Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| MappingError::Field(format!("birthDate ungültig: {s}")))?,
        None => return Err(MappingError::Field("birthDate".into())),
    };

    let gender = Some(fhir_gender(v).to_string());

    // Identifier: Versicherten-ID (KENR/VSZ) → insurance_number
    let mut insurance_number: Option<String> = None;
    if let Some(ids) = v.get("identifier").and_then(|ids| ids.as_array()) {
        for id in ids {
            let sys = id.get("system").and_then(|s| s.as_str()).unwrap_or_default();
            let type_text = id
                .get("type")
                .and_then(|t| t.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            let val = id.get("value").and_then(|val| val.as_str());
            if val.is_none() {
                continue;
            }
            let is_insurance = sys.to_lowercase().contains("kenr")
                || sys.to_lowercase().contains("vsz")
                || type_text.to_lowercase().contains("versicher");
            if is_insurance && insurance_number.is_none() {
                insurance_number = Some(val.unwrap().to_string());
            }
        }
    }

    // Address (FHIR: `line` ist ein String-Array, `text` ist der Gesamtstring)
    let address = v
        .get("address")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first());
    let address_street = address
        .and_then(|a| {
            a.get("line")
                .and_then(|l| l.as_array())
                .and_then(|l| l.first())
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| address.and_then(|a| str_of(a, "text")));
    let address_zip = address.and_then(|a| str_of(a, "postalCode"));
    let address_city = address.and_then(|a| str_of(a, "city"));

    // Telecom
    let mut phone: Option<String> = None;
    let mut email: Option<String> = None;
    if let Some(tel) = v.get("telecom").and_then(|t| t.as_array()) {
        for t in tel {
            let system = t.get("system").and_then(|s| s.as_str()).unwrap_or_default();
            let val = t.get("value").and_then(|v| v.as_str()).map(|s| s.to_string());
            match system {
                "phone" | "mobile" => {
                    if phone.is_none() {
                        phone = val;
                    }
                }
                "email" => {
                    if email.is_none() {
                        email = val;
                    }
                }
                _ => {}
            }
        }
    }

    let now = Utc::now();
    Ok(Patient {
        id: uuid::Uuid::new_v4(), // wird vom Importer bei Insert ersetzt
        external_id,
        name_family,
        name_given,
        birth_date,
        gender,
        phone,
        email,
        address_street,
        address_zip,
        address_city,
        insurance_number,
        allergies: vec![],
        created_at: now,
        updated_at: now,
        deleted_at: None,
        version: 0,
    })
}

/// FHIR `Practitioner` → (external_id, family, given, specialties).
///
/// `specialties` ist hier leer — die kommen aus `PractitionerRole`
/// (siehe [`map_practitioner_role_fhir`]).
pub type PractitionerMapping = (Option<String>, String, String, Vec<String>);

pub fn map_practitioner_fhir(v: &Value) -> Result<PractitionerMapping, MappingError> {
    let external_id = v
        .get("id")
        .and_then(|i| i.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("identifier")
                .and_then(|ids| ids.as_array())
                .and_then(|ids| ids.first())
                .and_then(|id| id.get("value").and_then(|val| val.as_str()))
                .map(|s| s.to_string())
        });

    let name = v
        .get("name")
        .and_then(|names| names.as_array())
        .and_then(|names| names.first());

    let name_family = name
        .and_then(|n| str_of(n, "family"))
        .unwrap_or_else(|| "Unbekannt".to_string());
    let name_given = name
        .and_then(|n| {
            n.get("given")
                .and_then(|g| g.as_array())
                .and_then(|g| g.first())
                .and_then(|g| g.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    Ok((external_id, name_family, name_given, vec![]))
}

/// FHIR `PractitionerRole` → (practitioner_external_id, specialties).
///
/// `specialties` wird aus `code` (CodeableConcept-Array) extrahiert.
/// `practitioner` wird aus der Referenz `Practitioner/{id}` extrahiert.
pub type PractitionerRoleMapping = (Option<String>, Vec<String>);

pub fn map_practitioner_role_fhir(v: &Value) -> Result<PractitionerRoleMapping, MappingError> {
    let practitioner_id = v
        .get("practitioner")
        .and_then(|p| p.get("reference"))
        .and_then(|r| r.as_str())
        .map(|s| s.split('/').last().unwrap_or(s).to_string());

    let mut specialties: Vec<String> = vec![];
    if let Some(codes) = v.get("code").and_then(|c| c.as_array()) {
        for cc in codes {
            if let Some(d) = concept_display(cc, "") {
                specialties.push(d);
            } else if let Some(coding) =
                cc.get("coding").and_then(|c| c.as_array()).and_then(|c| c.first())
            {
                if let Some(d) = coding.get("display").and_then(|d| d.as_str()) {
                    specialties.push(d.to_string());
                }
            }
        }
    }

    Ok((practitioner_id, specialties))
}

/// FHIR `Appointment` → KIS-Termin-Felder.
///
/// Mapped: `id` (→ external_id), `status`, `startDateTime`/`endDateTime`,
/// `participant` (Patient → patient_external_id, Practitioner →
/// doctor_external_id), `reasonCode` (→ reason), `serviceType` (→ category).
///
/// Rückgabe-Tuple:
/// `(external_id, patient_external_id, doctor_external_id, category, status,
///  start, end, reason)`
pub type AppointmentMapping = (
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<String>,
);

const APPT_STATUS_MAP: [(&str, &str); 7] = [
    ("booked", "geplant"),
    ("arrived", "bestaetigt"),
    ("fulfilled", "abgeschlossen"),
    ("canceled", "abgesagt"),
    ("noshow", "abgesagt"),
    ("entered-in-error", "abgesagt"),
    ("pending", "geplant"),
];

/// FHIR `Appointment.status` → KIS-Status (kleingeschriebene KIS-Konvention).
pub fn map_appointment_status(fhir_status: &str) -> &'static str {
    APPT_STATUS_MAP
        .iter()
        .find(|(k, _)| *k == fhir_status)
        .map(|(_, v)| *v)
        .unwrap_or("geplant")
}

pub fn map_appointment_fhir(v: &Value) -> Result<AppointmentMapping, MappingError> {
    let external_id = v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());

    let status = v
        .get("status")
        .and_then(|s| s.as_str())
        .map(map_appointment_status)
        .unwrap_or("geplant")
        .to_string();

    let start = dt_of(v, "startDateTime")?
        .ok_or_else(|| MappingError::Field("startDateTime".into()))?;
    let end = dt_of(v, "endDateTime")?
        .ok_or_else(|| MappingError::Field("endDateTime".into()))?;

    let reason = v
        .get("reasonCode")
        .and_then(|r| r.as_array())
        .and_then(|r| r.first())
        .and_then(|r| {
            r.get("text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    r.get("coding")
                        .and_then(|c| c.as_array())
                        .and_then(|c| c.first())
                        .and_then(|c| c.get("display").and_then(|d| d.as_str()))
                        .map(|s| s.to_string())
                })
        });

    // Participants: Patient & Practitioner extrahieren
    let mut patient_external: Option<String> = None;
    let mut doctor_external: Option<String> = None;
    if let Some(parts) = v.get("participant").and_then(|p| p.as_array()) {
        for part in parts {
            let type_ = part
                .get("type")
                .and_then(|t| t.as_array())
                .and_then(|t| t.first())
                .and_then(|t| t.get("coding"))
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
                .and_then(|c| c.get("code").and_then(|v| v.as_str()))
                .unwrap_or_default();

            let ref_id = part
                .get("individual")
                .and_then(|a| a.get("reference"))
                .and_then(|i| i.as_str())
                .map(|s| s.split('/').last().unwrap_or(s).to_string())
                .or_else(|| {
                    part.get("actor")
                        .and_then(|a| a.get("reference"))
                        .and_then(|i| i.as_str())
                        .map(|s| s.split('/').last().unwrap_or(s).to_string())
                });

            match type_ {
                "patient" => patient_external = ref_id,
                "practitioner" | "individual" => {
                    if doctor_external.is_none() {
                        doctor_external = ref_id;
                    }
                }
                _ => {}
            }
        }
    }

    let category = v
        .get("serviceType")
        .and_then(|st| st.as_array())
        .and_then(|st| st.first())
        .and_then(|st| {
            st.get("text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    st.get("coding")
                        .and_then(|c| c.as_array())
                        .and_then(|c| c.first())
                        .and_then(|c| c.get("display").and_then(|d| d.as_str()))
                        .map(|s| s.to_string())
                })
        })
        .unwrap_or_else(|| "ordination".to_string());

    Ok((
        external_id,
        patient_external,
        doctor_external,
        category,
        status,
        start,
        end,
        reason,
    ))
}

/// FHIR `Slot` → KIS-Verfügbarkeits-Slot.
///
/// Rückgabe-Tuple: `(external_id, practitioner_external_id, start, end,
///  available)`
pub type SlotMapping = (Option<String>, Option<String>, DateTime<Utc>, DateTime<Utc>, bool);

const SLOT_STATUS_MAP: [(&str, bool); 4] = [
    ("free", true),
    ("busy", false),
    ("blocked", false),
    ("cancelled", false),
];

/// FHIR `Slot.status` → `available` (bool).
pub fn map_slot_status(fhir_status: &str) -> bool {
    SLOT_STATUS_MAP
        .iter()
        .find(|(k, _)| *k == fhir_status)
        .map(|(_, v)| *v)
        .unwrap_or(false)
}

pub fn map_slot_fhir(v: &Value) -> Result<SlotMapping, MappingError> {
    let external_id = v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());

    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("free");
    let available = map_slot_status(status);

    let start = dt_of(v, "startDateTime")?
        .ok_or_else(|| MappingError::Field("startDateTime".into()))?;
    let end = dt_of(v, "endDateTime")?
        .ok_or_else(|| MappingError::Field("endDateTime".into()))?;

    let practitioner = v
        .get("practitioner")
        .and_then(|p| p.get("reference"))
        .and_then(|r| r.as_str())
        .map(|s| s.split('/').last().unwrap_or(s).to_string());

    Ok((external_id, practitioner, start, end, available))
}

/// FHIR `Observation` → KIS-Befund-Felder.
///
/// Mapped: `id` (→ external_id), `code` (→ name), `status`,
/// `valueQuantity`/`valueString` (→ value), `effectiveDateTime` (→
/// observed_at), `subject` (→ patient_external_id), `performer`
/// (→ doctor_external_id).
///
/// Rückgabe-Tuple:
/// `(external_id, patient_external_id, doctor_external_id, name, value,
///  status, observed_at)`
pub type ObservationMapping = (
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    DateTime<Utc>,
);

const OBS_STATUS_MAP: [(&str, &str); 7] = [
    ("registered", "geplant"),
    ("preliminary", "vorlaeufig"),
    ("final", "final"),
    ("amended", "final"),
    ("corrected", "final"),
    ("cancelled", "storniert"),
    ("entered-in-error", "storniert"),
];

/// FHIR `Observation.status` → KIS-Status.
pub fn map_observation_status(fhir_status: &str) -> &'static str {
    OBS_STATUS_MAP
        .iter()
        .find(|(k, _)| *k == fhir_status)
        .map(|(_, v)| *v)
        .unwrap_or("geplant")
}

pub fn map_observation_fhir(v: &Value) -> Result<ObservationMapping, MappingError> {
    let external_id = v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());

    let status = v
        .get("status")
        .and_then(|s| s.as_str())
        .map(map_observation_status)
        .unwrap_or("geplant")
        .to_string();

    let name = concept_display(v, "code").unwrap_or_else(|| "Befund".to_string());

    // valueQuantity bevorzugt, dann valueString, dann valueCodeableConcept
    let value = v
        .get("valueQuantity")
        .and_then(|vq| {
            let val = vq
                .get("value")
                .and_then(|val| val.as_f64().map(|f| f.to_string()).or_else(|| val.as_str().map(|s| s.to_string())))?;
            let unit = vq.get("unit").and_then(|u| u.as_str()).unwrap_or("");
            Some(format!("{val} {unit}").trim().to_string())
        })
        .or_else(|| v.get("valueString").and_then(|vs| vs.as_str()).map(|s| s.to_string()))
        .or_else(|| {
            v.get("valueCodeableConcept")
                .and_then(|cc| concept_display(cc, ""))
        })
        .unwrap_or_default();

    let observed_at = dt_of(v, "effectiveDateTime")?
        .or_else(|| dt_of(v, "issued").ok()?)
        .unwrap_or_else(Utc::now);

    let patient_external = v
        .get("subject")
        .and_then(|s| s.get("reference"))
        .and_then(|r| r.as_str())
        .map(|s| s.split('/').last().unwrap_or(s).to_string());

    let doctor_external = v
        .get("performer")
        .and_then(|p| p.as_array())
        .and_then(|p| p.first())
        .and_then(|p| p.get("reference"))
        .and_then(|r| r.as_str())
        .map(|s| s.split('/').last().unwrap_or(s).to_string());

    Ok((
        external_id,
        patient_external,
        doctor_external,
        name,
        value,
        status,
        observed_at,
    ))
}

// ===========================================================================
// Unit-Tests: FHIR→KIS-Mapping
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Vollständiges FHIR-Patient-Beispiel (R4 + KENR-Identifier).
    fn sample_patient_json() -> Value {
        json!({
            "resourceType": "Patient",
            "id": "p-4711",
            "identifier": [
                {
                    "system": "http://fhir.de/Naming/kenr",
                    "type": { "text": "Versichertennummer" },
                    "value": "V123456789"
                },
                {
                    "type": { "text": "Krankenkasse" },
                    "value": "KB-999"
                }
            ],
            "name": [
                {
                    "family": "Muster",
                    "given": ["Anna", "Maria"]
                }
            ],
            "gender": "female",
            "birthDate": "1985-04-12",
            "address": [
                {
                    "line": ["Musterstraße 1", "2. OG"],
                    "postalCode": "10115",
                    "city": "Berlin"
                }
            ],
            "telecom": [
                { "system": "phone", "value": "+49 30 1234567" },
                { "system": "email", "value": "anna.muster@example.de" }
            ]
        })
    }

    #[test]
    fn test_map_patient_complete() {
        let v = sample_patient_json();
        let p = map_patient_fhir(&v).expect("Mapping");

        assert_eq!(p.external_id.as_deref(), Some("p-4711"));
        assert_eq!(p.name_family, "Muster");
        assert_eq!(p.name_given, "Anna");
        assert_eq!(p.birth_date, NaiveDate::from_ymd_opt(1985, 4, 12).unwrap());
        assert_eq!(p.gender.as_deref(), Some("w")); // KIS-Konvention
        assert_eq!(p.phone.as_deref(), Some("+49 30 1234567"));
        assert_eq!(p.email.as_deref(), Some("anna.muster@example.de"));
        assert_eq!(p.address_street.as_deref(), Some("Musterstraße 1"));
        assert_eq!(p.address_zip.as_deref(), Some("10115"));
        assert_eq!(p.address_city.as_deref(), Some("Berlin"));
        assert_eq!(p.insurance_number.as_deref(), Some("V123456789"));
        assert!(p.allergies.is_empty());
        assert_eq!(p.version, 0);
        assert!(p.deleted_at.is_none());
    }

    #[test]
    fn test_map_patient_missing_birthdate_fails() {
        let v = json!({
            "resourceType": "Patient",
            "id": "p-x",
            "name": [{ "family": "Ohne" }],
            "gender": "male"
        });
        let err = map_patient_fhir(&v).unwrap_err();
        assert!(matches!(err, MappingError::Field(_)), "erwartet Feldfehler: {err}");
    }

    #[test]
    fn test_map_patient_empty_name() {
        let v = json!({
            "resourceType": "Patient",
            "id": "p-y",
            "birthDate": "2000-01-01"
        });
        let p = map_patient_fhir(&v).expect("Mapping");
        assert_eq!(p.name_family, "Unbekannt");
        assert_eq!(p.name_given, "");
    }

    fn sample_practitioner_json() -> Value {
        json!({
            "resourceType": "Practitioner",
            "id": "pr-1",
            "identifier": [{ "system": "http://fhir.de/Naming/kenntnis-id", "value": "KEN-001" }],
            "name": [{ "family": "Beispiel", "given": ["Dr. Max"] }],
            "gender": "male"
        })
    }

    #[test]
    fn test_map_practitioner() {
        let v = sample_practitioner_json();
        let (ext, fam, given, specs) = map_practitioner_fhir(&v).expect("Mapping");
        assert_eq!(ext.as_deref(), Some("pr-1"));
        assert_eq!(fam, "Beispiel");
        assert_eq!(given, "Dr. Max");
        assert!(specs.is_empty());
    }

    #[test]
    fn test_map_practitioner_role() {
        let v = json!({
            "resourceType": "PractitionerRole",
            "id": "role-1",
            "practitioner": { "reference": "Practitioner/pr-1" },
            "code": [
                {
                    "text": "HNO",
                    "coding": [{ "system": "http://snomed.info/sct", "code": "110309006", "display": "HNO-Arzt" }]
                }
            ]
        });
        let (pid, specs) = map_practitioner_role_fhir(&v).expect("Mapping");
        assert_eq!(pid.as_deref(), Some("pr-1"));
        assert_eq!(specs, vec!["HNO"]);
    }

    fn sample_appointment_json() -> Value {
        json!({
            "resourceType": "Appointment",
            "id": "apt-1",
            "status": "booked",
            "startDateTime": "2025-06-15T09:00:00+02:00",
            "endDateTime": "2025-06-15T09:20:00+02:00",
            "serviceType": [{ "text": "Kontrolle" }],
            "reasonCode": [{ "text": "Wiedervorstellung", "coding": [{ "display": "Wiedervorstellung" }] }],
            "participant": [
                {
                    "type": [{ "coding": [{ "code": "patient" }] }],
                    "individual": { "reference": "Patient/p-4711" }
                },
                {
                    "type": [{ "coding": [{ "code": "practitioner" }] }],
                    "individual": { "reference": "Practitioner/pr-1" }
                }
            ]
        })
    }

    #[test]
    fn test_map_appointment() {
        let v = sample_appointment_json();
        let (ext, pat, doc, cat, status, start, end, reason) =
            map_appointment_fhir(&v).expect("Mapping");

        assert_eq!(ext.as_deref(), Some("apt-1"));
        assert_eq!(pat.as_deref(), Some("p-4711"));
        assert_eq!(doc.as_deref(), Some("pr-1"));
        assert_eq!(cat, "Kontrolle");
        assert_eq!(status, "geplant");
        assert_eq!(
            start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2025-06-15T07:00:00Z"
        );
        assert_eq!(
            end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2025-06-15T07:20:00Z"
        );
        assert_eq!(reason.as_deref(), Some("Wiedervorstellung"));
    }

    #[test]
    fn test_map_appointment_status_mapping() {
        assert_eq!(map_appointment_status("booked"), "geplant");
        assert_eq!(map_appointment_status("canceled"), "abgesagt");
        assert_eq!(map_appointment_status("fulfilled"), "abgeschlossen");
        assert_eq!(map_appointment_status("unknown"), "geplant");
    }

    #[test]
    fn test_map_appointment_missing_end_fails() {
        let v = json!({
            "resourceType": "Appointment",
            "id": "apt-x",
            "status": "booked",
            "startDateTime": "2025-06-15T09:00:00Z"
        });
        let err = map_appointment_fhir(&v).unwrap_err();
        assert!(matches!(err, MappingError::Field(_)), "erwartet Feldfehler: {err}");
    }

    #[test]
    fn test_map_slot() {
        let v = json!({
            "resourceType": "Slot",
            "id": "slot-1",
            "status": "free",
            "startDateTime": "2025-06-15T09:00:00Z",
            "endDateTime": "2025-06-15T09:20:00Z",
            "practitioner": { "reference": "Practitioner/pr-1" }
        });
        let (ext, pid, start, end, avail) = map_slot_fhir(&v).expect("Mapping");
        assert_eq!(ext.as_deref(), Some("slot-1"));
        assert_eq!(pid.as_deref(), Some("pr-1"));
        assert!(avail);
        assert_eq!(end - start, chrono::Duration::minutes(20));
    }

    #[test]
    fn test_map_slot_status_mapping() {
        assert!(map_slot_status("free"));
        assert!(!map_slot_status("busy"));
        assert!(!map_slot_status("blocked"));
        assert!(!map_slot_status("cancelled"));
        assert!(!map_slot_status("weird"));
    }

    fn sample_observation_json() -> Value {
        json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "status": "final",
            "code": { "text": "Hämoglobin", "coding": [{ "display": "Hämoglobin (Blut)" }] },
            "valueQuantity": { "value": 14.2, "unit": "g/dL" },
            "effectiveDateTime": "2025-06-10T08:00:00Z",
            "subject": { "reference": "Patient/p-4711" },
            "performer": [{ "reference": "Practitioner/pr-1" }]
        })
    }

    #[test]
    fn test_map_observation() {
        let v = sample_observation_json();
        let (ext, pat, doc, name, value, status, observed_at) =
            map_observation_fhir(&v).expect("Mapping");

        assert_eq!(ext.as_deref(), Some("obs-1"));
        assert_eq!(pat.as_deref(), Some("p-4711"));
        assert_eq!(doc.as_deref(), Some("pr-1"));
        assert_eq!(name, "Hämoglobin");
        assert_eq!(value, "14.2 g/dL");
        assert_eq!(status, "final");
        assert_eq!(
            observed_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2025-06-10T08:00:00Z"
        );
    }

    #[test]
    fn test_map_observation_value_string() {
        let v = json!({
            "resourceType": "Observation",
            "id": "obs-2",
            "status": "preliminary",
            "code": { "text": "Befundtext" },
            "valueString": "Unbefundlich",
            "effectiveDateTime": "2025-06-10T08:00:00Z"
        });
        let (_, _, _, _, value, status, _) = map_observation_fhir(&v).expect("Mapping");
        assert_eq!(value, "Unbefundlich");
        assert_eq!(status, "vorlaeufig");
    }

    #[test]
    fn test_map_observation_status_mapping() {
        assert_eq!(map_observation_status("registered"), "geplant");
        assert_eq!(map_observation_status("final"), "final");
        assert_eq!(map_observation_status("cancelled"), "storniert");
        assert_eq!(map_observation_status("bogus"), "geplant");
    }
}
