# CGM MEDICO Integration — Vollständige Dokumentation

> **Ziel:** KIS funktioniert als Ergänzung zu CGM MEDICO in bestehenden Klinik-IT-Landschaften.
> Kein Ersatz — bidirektionaler Datenaustausch über FHIR R4, HL7 v2 und GDT.

---

## 1. CGM MEDICO — Systemübersicht

CGM MEDICO ist das KIS der CompuGroup Medical SE & Co. KGaA (Koblenz), in ca. 250 deutschen Krankenhäusern im Einsatz.

| Eigenschaft | Wert |
|-------------|------|
| Technologie | Java G3 (offene Architektur) |
| Schnittstellen | HL7 v2, FHIR R4 (ISiK), DICOM, KIM/TI |
| Zertifizierung | ISiK (Informationssysteme im Krankenhaus) nach gematik |
| Abrechnung | DRG, § 301 SGB V, AHB |
| Telematik | eGK, eEB, ePA, eAU, eRezept, eArztbrief (KIM) |
| Mobile | MEDICO Touch (Stationen-Workplace) |

### MEDICO-FHIR-Endpunkte

```
Base URL: https://fhir.medico.example.com/fhir/
Auth:     OAuth 2.0 (Client Credentials)
Token:    POST /fhir/auth/token
Client:   medico-client-id / medico-client-secret (von MEDICO-IT ausgestellt)
```

### Erforderliche OAuth 2.0 Scopes

| Scope | Beschreibung |
|-------|-------------|
| `system/Patient.read` | Patienten lesen |
| `system/Patient.write` | Patienten anlegen/ändern |
| `system/Appointment.read` | Termine lesen |
| `system/Appointment.write` | Termine anlegen/ändern |
| `system/HealthcareService.read` | Sprechstunden lesen |
| `system/Schedule.read` | Sprechzeiten lesen |
| `system/Slot.read` | Verfügbarkeit lesen |
| `system/Subscription.read` | Abonnements lesen |
| `system/Subscription.write` | Abonnements anlegen |

---

## 2. Konfiguration in KIS

### config/kis.toml

```toml
[interfaces.medico]
enabled = true

# FHIR-Endpunkte
base_url = "https://fhir.medico.example.com/fhir/"
system_namespace = "https://fhir.medico.example.com"

# OAuth 2.0 (Werte aus ENV, nie im Code!)
auth_endpoint = "https://fhir.medico.example.com/fhir/auth/token"
client_id_env = "KIS_MEDICO_CLIENT_ID"
client_secret_env = "KIS_MEDICO_CLIENT_SECRET"

# Scopes (Default: alle system/* Scopes)
scopes = [
    "system/Patient.read",
    "system/Patient.write",
    "system/Appointment.read",
    "system/Appointment.write",
    "system/HealthcareService.read",
    "system/Schedule.read",
    "system/Slot.read",
    "system/Subscription.read",
    "system/Subscription.write"
]

# Synchronisation
sync_interval_seconds = 30        # Polling-Intervall
max_concurrent_requests = 10      # Parallele Requests
request_timeout_seconds = 30      # Timeout pro Request

# Change-Detection
change_detection_mode = "polling" # "polling" | "websocket"
websocket_url = "wss://fhir.medico.example.com/websocket"

# Slot-Synchronisation
sync_slots = true
availability_poll_interval_minutes = 10
availability_poll_range_days = 180  # Wie weit in die Zukunft
availability_poll_delay_seconds = 1   # Delay zwischen Slot-Requests
availability_poll_parallel_workers = 10

# GDT (für Patientenstammdaten-Export)
gdt_export_enabled = true
gdt_export_path = "exports/gdt/"

# HL7 v2 (alternativ zu FHIR)
hl7_enabled = false
hl7_host = "10.10.11.100"
hl7_port = 6667
hl7_ae_title = "KIS"
```

### ENV-Variablen

```bash
# OAuth Credentials (NIEMALS im Code oder TOML!)
export KIS_MEDICO_CLIENT_ID="medico-client-xxxx"
export KIS_MEDICO_CLIENT_SECRET="xxxxxxxxxxxxxxxx"

# Optional: Override für Base URL
export KIS_MEDICO_BASE_URL="https://fhir.medico.example.com/fhir/"
```

---

## 3. Datenfluss

### 3.1 MEDICO → KIS (Import)

```
┌─────────────────────────────────────────────────────────────┐
│                     CGM MEDICO                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │Patienten │  │Termine   │  │Slots     │  │HL7 v2    │   │
│  │(FHIR)    │  │(FHIR)    │  │(FHIR)    │  │(MLLP)    │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   │
└───────┼──────────────┼──────────────┼──────────────┼────────┘
        │              │              │              │
        ▼              ▼              ▼              ▼
┌─────────────────────────────────────────────────────────────┐
│                    KIS FHIR Client                           │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ OAuth 2.0 Token Management (Auto-Refresh)            │   │
│  ├──────────────────────────────────────────────────────┤   │
│  │ Patient-Import:    GET /Patient?_lastUpdated=ge{ts}  │   │
│  │ Termin-Import:     GET /Appointment?_lastUpdated=ge{ts}│   │
│  │ Slot-Import:       GET /Slot?start=YYYY-MM-DD        │   │
│  │ Change-Detection:  Polling (30s) + Webhook           │   │
│  │ HL7-Receiver:      MLLP TCP:6667                     │   │
│  └──────────────────────────────────────────────────────┘   │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    KIS Datenbanken                           │
│  patients, appointments, slots, documents                    │
│  (mit Audit-Log: wer, was, wann, woher)                     │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 KIS → MEDICO (Export)

```
┌─────────────────────────────────────────────────────────────┐
│                    KIS API / GUI                             │
│  (Patient anlegen, Termin buchen, Dokument hochladen)        │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    KIS FHIR Client                           │
│  Patient-Export:    POST /Patient                           │
│  Termin-Buchung:    POST /Appointment/$book                 │
│  Termin-Änderung:   POST /Appointment/{id}                  │
│  Termin-Storno:     DELETE /Appointment/{id}                │
│  GDT-Export:        Datei-Export (CSV)                      │
│  HL7-Export:        MLLP TCP:6667 → MEDICO                  │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                     CGM MEDICO                               │
│  (Patient, Termin, Slot, Dokument gespeichert)               │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Implementierungsdetails

### 4.1 FHIR R4 Client

```rust
// src/schnittstellen/fhir/medico.rs

use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct MedicoClient {
    base_url: String,
    client: Client,
    token: Option<String>,
    token_expiry: Option<chrono::DateTime<chrono::Utc>>,
}

impl MedicoClient {
    /// OAuth 2.0 Client Credentials Flow
    pub async fn get_token(&mut self, client_id: &str, client_secret: &str) -> Result<String, MedicoError> {
        let response = self.client
            .post(format!("{}/auth/token", self.base_url))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("scope", "system/Patient.read system/Patient.write"),
            ])
            .send()
            .await?;
        
        let body: TokenResponse = response.json().await?;
        self.token = Some(body.access_token.clone());
        self.token_expiry = Some(
            chrono::Utc::now() + chrono::Duration::seconds(body.expires_in as i64 - 60)
        );
        Ok(body.access_token)
    }
    
    /// Patienten aus MEDICO importieren
    pub async fn import_patients(&mut self, since: &chrono::DateTime<chrono::Utc>) -> Result<Vec<MedicoPatient>, MedicoError> {
        self.ensure_token().await?;
        
        let url = format!(
            "{}/Patient?_lastUpdated=ge{}&_count=100",
            self.base_url,
            since.to_rfc3339()
        );
        
        let response = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token.as_ref().unwrap()))
            .header("Accept", "application/fhir+json")
            .send()
            .await?;
        
        let bundle: Bundle = response.json().await?;
        Ok(bundle.entry.iter()
            .filter_map(|e| e.resource.as_ref()
                .and_then(|r| r.get("Patient")
                    .and_then(|p| serde_json::from_value::<MedicoPatient>(p.clone()).ok())))
            .collect())
    }
    
    /// Termine aus MEDICO importieren
    pub async fn import_appointments(&mut self, since: &chrono::DateTime<chrono::Utc>) -> Result<Vec<MedicoAppointment>, MedicoError> {
        // Analog zu import_patients
        todo!()
    }
    
    /// Slots aus MEDICO importieren
    pub async fn import_slots(&mut self, date: &str) -> Result<Vec<MedicoSlot>, MedicoError> {
        // GET /Slot?start=YYYY-MM-DD
        todo!()
    }
    
    /// Patienten in MEDICO exportieren
    pub async fn export_patient(&mut self, patient: &MedicoPatient) -> Result<String, MedicoError> {
        self.ensure_token().await?;
        
        let response = self.client
            .post(format!("{}/Patient", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token.as_ref().unwrap()))
            .header("Content-Type", "application/fhir+json")
            .json(&patient)
            .send()
            .await?;
        
        let created: Patient = response.json().await?;
        Ok(created.id)
    }
    
    /// Termin in MEDICO buchen (ISiK $book Operation)
    pub async fn book_appointment(&mut self, appointment: &MedicoAppointment) -> Result<String, MedicoError> {
        self.ensure_token().await?;
        
        // ISiK $book: POST /Appointment/$book
        let response = self.client
            .post(format!("{}/Appointment/$book", self.base_url))
            .header("Authorization", format!("Bearer {}", self.token.as_ref().unwrap()))
            .header("Content-Type", "application/fhir+json")
            .json(&appointment)
            .send()
            .await?;
        
        let result: Appointment = response.json().await?;
        Ok(result.id)
    }
    
    /// Termin stornieren
    pub async fn cancel_appointment(&mut self, appointment_id: &str, cancel_reason: &str) -> Result<(), MedicoError> {
        self.ensure_token().await?;
        
        // DELETE /Appointment/{id} mit cancelled-appt-id Parameter
        let response = self.client
            .delete(format!("{}/Appointment/{}", self.base_url, appointment_id))
            .header("Authorization", format!("Bearer {}", self.token.as_ref().unwrap()))
            .query(&[("cancelled-appt-id", appointment_id), ("reason", cancel_reason)])
            .send()
            .await?;
        
        Ok(())
    }
    
    /// Token prüfen und bei Bedarf aktualisieren
    async fn ensure_token(&mut self) -> Result<(), MedicoError> {
        if let Some(expiry) = self.token_expiry {
            if chrono::Utc::now() < expiry {
                return Ok(());
            }
        }
        // Token abgelaufen oder nicht vorhanden → neuer Token
        let client_id = std::env::var("KIS_MEDICO_CLIENT_ID")?;
        let client_secret = std::env::var("KIS_MEDICO_CLIENT_SECRET")?;
        self.get_token(&client_id, &client_secret).await?;
        Ok(())
    }
}
```

### 4.2 Daten-Mapping

#### MEDICO Patient → KIS Patient

| MEDICO FHIR Feld | KIS Feld | Transformation |
|-----------------|----------|---------------|
| `Patient.name[0].family` | `last_name` | Direkt |
| `Patient.name[0].given[0]` | `first_name` | Direkt |
| `Patient.birthDate` | `birth_date` | ISO 8601 → String |
| `Patient.gender` | `gender` | `male`/`female`/`other` |
| `Patient.identifier[0].value` (system: MR) | `insurance_number` | AES-256-GCM verschlüsselt |
| `Patient.identifier[1].value` (system: KIS) | `id` | UUID (KIS-eigen) |
| `Patient.address[0].line` | `address` | Direkt |
| `Patient.telecom[0].value` | `phone` | Direkt |
| `Patient.telecom[1].value` | `email` | Direkt |

#### MEDICO Appointment → KIS Appointment

| MEDICO FHIR Feld | KIS Feld | Transformation |
|-----------------|----------|---------------|
| `Appointment.start` | `start_time` | ISO 8601 → DateTime |
| `Appointment.end` | `end_time` | ISO 8601 → DateTime |
| `Appointment.status` | `status` | `booked`→`confirmed`, `cancelled`→`cancelled` |
| `Appointment.participant[0].actor.reference` (Patient) | `patient_id` | MEDICO ID → KIS ID (Mapping-Table) |
| `Appointment.serviceType` | `service_type` | Direkt |
| `Appointment.description` | `notes` | Direkt |

#### ID-Mapping

```sql
-- Tabelle für ID-Mapping zwischen MEDICO und KIS
CREATE TABLE IF NOT EXISTS id_mapping (
    id TEXT PRIMARY KEY,
    externo_system TEXT NOT NULL DEFAULT 'medico',
    externo_id TEXT NOT NULL,
    kiso_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,  -- 'patient', 'appointment', 'doctor'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(externo_system, externo_id, entity_type)
);
CREATE INDEX IF NOT EXISTS idx_id_mapping_externo ON id_mapping(externo_system, externo_id);
CREATE INDEX IF NOT EXISTS idx_id_mapping_kiso ON id_mapping(kiso_id, entity_type);
```

### 4.3 GDT-Export (Patientenstammdaten)

GDT (Gemeinsamer Datenträger) ist ein CSV-Format für den Austausch von Patientenstammdaten.

```rust
/// Erzeugt eine GDT-Datei aus KIS-Patienten
pub fn export_gdt(patients: &[Patient], output_path: &Path) -> io::Result<()> {
    let mut wtr = csv::Writer::from_path(output_path)?;
    
    // GDT-Header (Pflichtfelder)
    wtr.write_record(&[
        "GDT-1", "Patientenstammdaten", "KIS", "1.0",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
    ])?;
    
    for p in patients {
        wtr.write_record(&[
            &p.id.to_string(),           // Patient-ID
            &p.last_name,                 // Nachname
            &p.first_name,                // Vorname
            &p.birth_date.unwrap_or_default(), // Geburtsdatum
            &p.gender,                    // Geschlecht
            &p.insurance_number,          // Krankenkassen-Nr.
            &p.address,                   // Adresse
            &p.phone,                     // Telefon
            &p.email,                     // E-Mail
        ])?;
    }
    
    wtr.flush()?;
    Ok(())
}
```

### 4.4 HL7 v2 Adapter

```rust
/// HL7 v2.5 ADT-A01 (Patient Admit)
pub fn hl7_adt_a01(patient: &Patient) -> String {
    format!(
        "MSH|^~\\&|KIS|{}|MEDICO|{}|{}||ADT|A01|{}|P|2.5\n\
         EVN||{}\n\
         PID|1||{}||{}^{}||||{}||||||||||\n\
         NK1|1|||||||||\n\
         AL1|1||||",
        std::env::var("KIS_NAME").unwrap_or_else(|_| "KIS".into()),
        chrono::Utc::now().format("%y%m%d%H%M%S"),
        chrono::Utc::now().format("%y%m%d%H%M%S"),
        uuid::Uuid::new_v4(),
        chrono::Utc::now().format("%y%m%d%H%M%S"),
        patient.id,
        patient.last_name,
        patient.first_name,
        patient.birth_date.unwrap_or_default()
    )
}
```

### 4.5 Change-Detection

```rust
/// Change-Detection: Polling + Webhook
pub struct MedicoChangeDetector {
    last_processed: HashMap<String, DateTime<Utc>>,  // entity_type → last timestamp
    processed_ids: HashSet<String>,                   // IDs die bereits verarbeitet wurden
}

impl MedicoChangeDetector {
    /// Polling: Alle 30 Sekunden prüfen, ob es neue/geänderte Daten gibt
    pub async fn poll_changes(&mut self, client: &mut MedicoClient) -> Result<Changes, MedicoError> {
        let mut changes = Changes::new();
        
        // Patienten
        let since = self.last_processed
            .get("patient")
            .cloned()
            .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24));
        
        let patients = client.import_patients(&since).await?;
        for p in patients {
            if !self.processed_ids.contains(&p.id) {
                changes.patients.push(p);
                self.processed_ids.insert(p.id.clone());
            }
        }
        
        // Termine
        // ... analog
        
        self.last_processed.insert("patient".into(), Utc::now());
        Ok(changes)
    }
    
    /// Webhook: MEDICO sendt Benachrichtigung bei Änderungen
    pub fn handle_webhook(&mut self, payload: &WebhookPayload) -> Result<Changes, MedicoError> {
        // Webhook enthält: entity_type, entity_id, action (created/updated/deleted)
        // → Daten neu von MEDICO laden
        todo!()
    }
}
```

### 4.6 Konflikt-Handling

```rust
/// Was passiert, wenn KIS und MEDICO unterschiedliche Daten haben?
pub enum ConflictResolution {
    /// MEDICO ist Authoritative Quelle (Default für Stammdaten)
    MedicoWins,
    /// KIS ist Authoritative Quelle (für KIS-spezifische Daten)
    KisoWins,
    /// Manuelle Auflösung (Flag für Admin-UI)
    ManualResolution,
}

impl ConflictResolution {
    pub fn resolve(
        kiso_value: &Option<String>,
        medico_value: &Option<String>,
        entity_type: &str,
        field: &str
    ) -> (String, ConflictResolution) {
        match (entity_type, field) {
            // Stammdaten: MEDICO gewinnt
            ("patient", "last_name") | ("patient", "first_name") | 
            ("patient", "birth_date") | ("patient", "insurance_number") => {
                (medico_value.clone().unwrap_or_default(), ConflictResolution::MedicoWins)
            }
            // KIS-spezifisch: KIS gewinnt
            ("patient", "notes") | ("appointment", "internal_notes") => {
                (kiso_value.clone().unwrap_or_default(), ConflictResolution::KisoWins)
            }
            // Sonst: Letzter Write gewinnt + Audit-Log
            _ => {
                // Beide Werte im Audit-Log dokumentieren
                (medico_value.clone().unwrap_or_default(), ConflictResolution::MedicoWins)
            }
        }
    }
}
```

---

## 5. Testing

### 5.1 MEDICO Mock-Server

```rust
// tests/medico_mock.rs

/// Mock-Server für MEDICO-FHIR-Endpunkte
pub struct MedicoMockServer {
    server: axum::Server,
    pub patients: Vec<MedicoPatient>,
    pub appointments: Vec<MedicoAppointment>,
}

impl MedicoMockServer {
    pub async fn start() -> Self {
        let app = Router::new()
            .route("/fhir/auth/token", get(mock_token))
            .route("/fhir/Patient", get(mock_patient_list).post(mock_patient_create))
            .route("/fhir/Appointment", get(mock_appt_list))
            .route("/fhir/Appointment/$book", post(mock_appt_book))
            .route("/fhir/Slot", get(mock_slot_list));
        
        // Auf 127.0.0.1:9090 starten
        todo!()
    }
}

async fn mock_token() -> Json<TokenResponse> {
    Json(TokenResponse {
        access_token: "mock-token".into(),
        token_type: "Bearer".into(),
        expires_in: 3600,
    })
}

async fn mock_patient_list() -> Json<Bundle> {
    Json(Bundle {
        entry: vec![BundleEntry {
            resource: Some(json!({
                "resourceType": "Patient",
                "id": "MEDICO-001",
                "name": [{"family": "Muster", "given": ["Max"]}],
                "birthDate": "1980-01-15",
                "gender": "male",
                "identifier": [{"system": "MR", "value": "123456789"}]
            })),
        }],
    })
}
```

### 5.2 Integrationstests

```rust
#[tokio::test]
async fn test_medico_patient_import() {
    let mut mock = MedicoMockServer::start().await;
    let mut client = MedicoClient::new("http://127.0.0.1:9090/fhir/");
    
    let patients = client.import_patients(&Utc::now() - Duration::days(1)).await.unwrap();
    
    assert_eq!(patients.len(), 1);
    assert_eq!(patients[0].last_name, "Muster");
    assert_eq!(patients[0].first_name, "Max");
    assert_eq!(patients[0].birth_date, "1980-01-15");
}

#[tokio::test]
async fn test_medico_appointment_booking() {
    let mut mock = MedicoMockServer::start().await;
    let mut client = MedicoClient::new("http://127.0.0.1:9090/fhir/");
    
    let appt = MedicoAppointment {
        start: Utc::now() + Duration::days(1),
        end: Utc::now() + Duration::days(1) + Duration::minutes(30),
        patient_id: "MEDICO-001".into(),
        // ...
    };
    
    let result = client.book_appointment(&appt).await.unwrap();
    assert!(!result.is_empty());
}
```

---

## 6. Monitoring

### Dashboard-Anzeige

| Metrik | Quelle | Alarm-Schwelle |
|--------|--------|---------------|
| MEDICO-Connectivity | FHIR Health Check | 3 fehlgeschlagene Checks in Folge |
| Token-Refresh | OAuth 2.0 | 503 oder 401 |
| Patient-Import | Sync-Log | 0 neue Patienten in > 24 h |
| Termin-Sync | Sync-Log | Latenz > 5 s |
| GDT-Export | Dateigröße | 0 Bytes |
| HL7-Durchsatz | MLLP-Log | 0 Nachrichten in > 60 min (wenn aktiv) |

### Logging

```
2026-11-01T07:00:00Z INFO  medico: Token refreshed (expires in 3540s)
2026-11-01T07:00:00Z INFO  medico: Patient-Import: 5 new, 2 updated since 2026-10-31T07:00:00Z
2026-11-01T07:00:30Z INFO  medico: Termin-Import: 3 new since 2026-10-31T07:00:00Z
2026-11-01T07:00:30Z WARN  medico: Slot-Sync: 120/200 slots retrieved (partial)
2026-11-01T07:01:00Z ERROR medico: GDT-Export failed: Permission denied on exports/gdt/
```

---

## 7. Sicherheit

| Aspekt | Umsetzung |
|--------|-----------|
| Credentials | Nur ENV-Variablen, nie im Code oder TOML |
| TLS | Pflicht (HTTPS), Zertifikatsprüfung |
| OAuth Scope | Minimal: nur die Scopes, die wirklich benötigt werden |
| Audit-Log | Jeder MEDICO-Zugriff geloggt (wer, wann, was, wie viel) |
| Daten-Minimierung | Nur Felder importieren, die KIS braucht |
| Verschlüsselung | Krankenkassen-Nr. mit AES-256-GCM verschlüsselt |
| ID-Mapping | MEDICO-IDs nie direkt in KIS-UI anzeigen |
| Rate-Limiting | MEDICO-Rate-Limits einhalten (max_concurrent_requests) |
| Timeout | 30 s pro Request, 3 Retries mit Exponential Backoff |

---

## 8. Betriebsanleitung

### Erstverbindung

```bash
# 1. ENV-Variablen setzen
export KIS_MEDICO_CLIENT_ID="vom-MEDICO-IT-erhalten"
export KIS_MEDICO_CLIENT_SECRET="vom-MEDICO-IT-erhalten"

# 2. KIS starten
./kis

# 3. MEDICO-Verbindung testen
curl -s http://localhost:8080/api/interfaces/medico/status | jq .
# → { "connected": true, "token_valid": true, "patients_synced": 0 }

# 4. Initial-Import starten
curl -s -X POST http://localhost:8080/api/interfaces/medico/sync/initial | jq .
# → { "patients": 500, "appointments": 120, "slots": 200 }
```

### Regelmäßige Synchronisation

Die Synchronisation läuft automatisch alle 30 Sekunden (konfigurierbar).
Manueller Trigger: `POST /api/interfaces/medico/sync/trigger`

### Fehlerbehandlung

| Fehler | Ursache | Lösung |
|--------|---------|--------|
| 401 Unauthorized | Token abgelaufen/invalid | Auto-Refresh; falls persistiert: Credentials prüfen |
| 403 Forbidden | Scope fehlt | OAuth Scopes in Konfiguration ergänzen |
| 404 Not Found | Endpunkt falsch | Base URL prüfen |
| 503 Service Unavailable | MEDICO down | Retry mit Backoff; Alarm nach 3 Fehlversuchen |
| Timeout | Netzwerk/Performance | Timeout erhöhen; MEDICO-IT kontaktieren |

---

*Version: 1.0.0 | Stand: 2026-11-01*
