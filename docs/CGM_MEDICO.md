# CGM MEDICO Integration — KIS als vollumfänglicher Ersatz

> **Dieses Dokument beschreibt, wie KIS CGM MEDICO vollständig ersetzt.**
> KIS ist **kein** Ergänzungssystem — KIS übernimmt **alle** Funktionen von CGM MEDICO.
> Der Import aus MEDICO dient ausschließlich dem Daten-Übergang bei der Migration.
> Nach erfolgreicher Migration wird MEDICO nicht mehr benötigt.

---

## 1. Positionierung: KIS ersetzt MEDICO vollständig

| Aspekt | CGM MEDICO | KIS (Ziel) |
|--------|-----------|------------|
| **Patientenverwaltung** | ✅ | ✅ (vollständig) |
| **Terminplanung** | ✅ | ✅ (vollständig, Kalender + Drag&Drop) |
| **Termineinträge (Slot)** | ✅ | ✅ (vollständig) |
| **Terminkategorien** | ✅ | ✅ (vollständig) |
| **Terminbuchung ($book)** | ✅ | ✅ (vollständig, ISiK-konform) |
| **Sprechstunden** | ✅ | ✅ (vollständig) |
| **Praxen** | ✅ | ✅ (vollständig) |
| **Versicherungen** | ✅ | ✅ (vollständig) |
| **Laborwerte / Befunde** | ✅ | ✅ (FHIR Observation) |
| **Arztbriefe** | ✅ | ✅ (XDS + FHIR DocumentReference) |
| **DICOM** | ✅ | ✅ (C-STORE, C-FIND, C-MOVE) |
| **HL7 v2** | ✅ | ✅ (MLLP Server) |
| **FHIR R4 (ISiK)** | ✅ | ✅ (Server + Client) |
| **Telematikinfrastruktur** | ✅ | ✅ (eGK, ePA, eRezept, eAU) |
| **GUI** | ✅ (Windows) | ✅ (Web, responsive, Dark/Light) |
| **Multi-User** | ✅ | ✅ (RBAC, tausende User) |
| **Skalierung** | Begrenzt | PostgreSQL, Read-Replicas, horizontal |
| **DSGVO / Compliance** | Teilweise | Vollständig (AES-256-GCM, Audit 10J) |

**KIS deckt 100% der MEDICO-Funktionen ab. MEDICO wird nach Migration abgeschaltet.**

---

## 2. Architektur der MEDICO-Integration

```
┌─────────────────────────────────────────────────────────┐
│                      KIS                                 │
│                                                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
│  │ patients │  │appointm. │  │medicatio │  │  fhir   │ │
│  │  (CRUD)  │  │ (Slots)  │  │  (CRUD)  │  │  (R4)   │ │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬────┘ │
│       │              │              │              │     │
│  ┌────▼──────────────▼──────────────▼──────────────▼──┐ │
│  │                    kis-core                          │ │
│  │  Config, DB, Security, Audit, API-Router            │ │
│  └────────────────────┬────────────────────────────────┘ │
│                       │                                  │
│  ┌────────────────────▼────────────────────────────────┐ │
│  │              kis-medico (Crate)                      │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌───────────┐ │ │
│  │  │ FHIR Client  │  │  GDT Export  │  │ HL7 v2    │ │ │
│  │  │ (MEDICO)     │  │  (Datei)     │  │  (MLLP)   │ │ │
│  │  └──────┬───────┘  └──────┬───────┘  └─────┬─────┘ │ │
│  └─────────┼─────────────────┼────────────────┼────────┘ │
└────────────┼─────────────────┼────────────────┼──────────┘
             │                 │                │
     ┌───────▼─────────────────▼────────────────▼───────┐
     │              CGM MEDICO (alt, nur Migration)       │
     │         Nach Migration: NICHT MEHR AKTIV          │
     └──────────────────────────────────────────────────┘
```

---

## 3. FHIR R4 Schnittstelle zu MEDICO (OAuth 2.0)

### 3.1 Authentication Flow (Client Credentials)

```
KIS (kis-medico)
    │
    │  POST /oauth2/token
    │  grant_type=client_credentials
    │  client_id=<MEDICO_CLIENT_ID>
    │  client_secret=<MEDICO_CLIENT_SECRET>
    │  scope=openid profile patient.read patient.write appointment.read
    │
    ▼
┌─────────────────────────────┐
│   CGM MEDICO OAuth2 Server  │
└──────────────┬──────────────┘
               │
               │  { "access_token": "eyJhbG...",
               │    "token_type": "Bearer",
               │    "expires_in": 3600 }
               │
               ▼
KIS speichert Token, nutzt für alle FHIR-Calls,
erfrischt 5 Minuten vor Ablauf
```

### 3.2 Konfiguration

```toml
# config/kis.toml
[interfaces.medico]
enabled = true
base_url = "https://fhir.medico.example.com/fhir/"
system_namespace = "1.2.276.0.79.3.7.1"
auth_endpoint = "https://fhir.medico.example.com/oauth2/token"

# Credentials aus ENV (NIEMALS im Code oder TOML):
# MEDICO_CLIENT_ID=xxx
# MEDICO_CLIENT_SECRET=xxx

scopes = ["openid", "profile", "patient.read", "patient.write", "appointment.read"]
sync_interval_seconds = 30
max_concurrent_requests = 10
request_timeout_seconds = 30
change_detection_mode = "polling"  # oder "webhook"
```

### 3.3 FHIR Endpunkte (MEDICO → KIS Import)

| Endpunkt | Methode | Zweck |
|----------|---------|-------|
| `/Patient` | GET | Alle Patienten |
| `/Patient?identifier=system\|value` | GET | Patient nach ID |
| `/Appointment` | GET | Alle Termine |
| `/Appointment?_lastUpdated=ge{timestamp}` | GET | Geänderte Termine (Change-Detection) |
| `/Slot?start=YYYY-MM-DD` | GET | Verfügbarkeit |
| `/Practitioner` | GET | Ärzte |
| `/PractitionerRole` | GET | Sprechstunden |
| `/Organization` | GET | Praxen |
| `/Observation` | GET | Laborwerte / Befunde |
| `/DocumentReference` | GET | Arztbriefe |
| `/Encounter` | GET | Aufenthalte |
| `/MedicationRequest` | GET | Medikamente |

### 3.4 FHIR Endpunkte (KIS → MEDICO Export)

| Endpunkt | Methode | Zweck |
|----------|---------|-------|
| `/Patient` | POST | Neuen Patienten in MEDICO anlegen |
| `/Patient/{id}` | PUT | Patienten aktualisieren |
| `/Appointment` | POST | Neuen Termin anlegen |
| `/Appointment/{id}/$book` | POST | Termin buchen (ISiK) |
| `/Appointment/{id}/$cancel` | POST | Termin stornieren |
| `/Observation` | POST | Befund anlegen |
| `/DocumentReference` | POST | Arztbrief hochladen |
| `/MedicationRequest` | POST | Verordnung anlegen |

---

## 4. Datenmapping MEDICO → KIS

### 4.1 Patient

| MEDICO FHIR Feld | KIS Feld | Bemerkung |
|-----------------|----------|-----------|
| `Patient.id` | `patients.id` (UUID, neu) | MEDICO-ID in `external_id` |
| `Patient.identifier[0].value` | `patients.external_id` | MEDICO-Patienten-ID |
| `Patient.name[0].family` | `patients.name_family` | Nachname |
| `Patient.name[0].given[0]` | `patients.name_given` | Vorname |
| `Patient.gender` | `patients.gender` | male/female/other |
| `Patient.birthDate` | `patients.birth_date` | ISO 8601 |
| `Patient.telecom[0].value` | `patients.phone` | Telefon |
| `Patient.telecom[1].value` | `patients.email` | E-Mail |
| `Patient.address[0].city` | `patients.address_city` | Stadt |
| `Patient.address[0].postalCode` | `patients.address_zip` | PLZ |
| `Patient.address[0].line[0]` | `patients.address_street` | Straße |
| `Patient.extension[insurance]` | `patients.insurance_number` | Versicherten-Nr. |
| `Patient.extension[allergies]` | `patients.allergies` (JSON) | Allergien |

### 4.2 Appointment

| MEDICO FHIR Feld | KIS Feld |
|-----------------|----------|
| `Appointment.id` | `appointments.id` (UUID, neu) |
| `Appointment.status` | `appointments.status` |
| `Appointment.start` | `appointments.start_time` |
| `Appointment.end` | `appointments.end_time` |
| `Appointment.participant[0].actor.reference` | `appointments.doctor_id` |
| `Appointment.participant[1].actor.reference` | `appointments.patient_id` |
| `Appointment.serviceType` | `appointments.category` |
| `Appointment.description` | `appointments.reason` |

### 4.3 Practitioner

| MEDICO FHIR Feld | KIS Feld |
|-----------------|----------|
| `Practitioner.id` | `doctors.id` (UUID, neu) |
| `Practitioner.name[0].family` | `doctors.name_family` |
| `Practitioner.name[0].given[0]` | `doctors.name_given` |
| `Practitioner.specialty` | `doctors.specialty` |
| `PractitionerRole.serviceType` | `doctors.specialties` (JSON) |

---

## 5. Import-Strategie (Migration)

### 5.1 Vollständiger Initial-Import

```
1. Alle Patienten:      GET /Patient?_count=100&_page=0,1,2,...
2. Alle Ärzte:          GET /Practitioner
3. Alle Sprechstunden:  GET /PractitionerRole
4. Alle Praxen:         GET /Organization
5. Alle Termine:        GET /Appointment?_count=100&_page=0,1,2,...
6. Alle Slot:           GET /Slot?start={heute}
7. Alle Befunde:        GET /Observation
8. Alle Dokumente:      GET /DocumentReference
9. Alle Medikamente:    GET /MedicationRequest

Jeder Record:
  - FHIR-Ressource parsen
  - Auf KIS-Modell mappen
  - In KIS-DB schreiben (INSERT ... ON CONFLICT DO NOTHING)
  - Audit-Log-Eintrag schreiben
  - Fehler sammeln, nicht abbrechen
```

### 5.2 Inkrementeller Sync (während Migrationsphase)

```
Alle 30 Sekunden:
  GET /Appointment?_lastUpdated=ge{letzte_sync_zeit}
  GET /Patient?_lastUpdated=ge{letzte_sync_zeit}

Nur geänderte Records werden aktualisiert.
Idempotenz: external_id + entity_type als Unique Constraint.
```

### 5.3 Konflikt-Handling

```
Wenn KIS-Record und MEDICO-Record denselben external_id haben:
  1. Vergleiche updated_at
  2. Älterer Record gewinnt (Last-Write-Wins)
  3. Konflikt in Audit-Log dokumentieren
  4. Option: Manuelle Review-Queue für Admins
```

---

## 6. GDT-Export (MEDICO-kompatibel)

### 6.1 Dateiformat

GDT (Gelbe Datei) ist ein CSV-Format:

```csv
Patienten_ID;Nachname;Vorname;Geburtsdatum;Geschlecht;Versichertennummer;Telefon;E-Mail;Straße;PLZ;Stadt
P001;Mustermann;Max;1985-03-15;m;A12345678901;01234/56789;max@example.com;Musterstr. 1;12345;Beispielstadt
P002;Schmidt;Anna;1990-11-22;w;B9876543210;09876/54321;anna@example.com;Beispielweg 2;54321;Teststadt
```

### 6.2 Export-Konfiguration

```toml
[interfaces.medico]
gdt_export_enabled = true
gdt_export_path = "/data/exports/gdt/"
gdt_export_filename = "kis_export_{date}.gdt"
```

### 6.3 Export-API

```
POST /api/v1/export/gdt
Content-Type: application/json

{ "include_deleted": false, "date_from": "2024-01-01" }

Response: 200 OK
{ "filename": "kis_export_2026-11-01.gdt", "record_count": 15432, "download_url": "/api/v1/export/gdt/download/..." }
```

---

## 7. HL7 v2 Adapter

### 7.1 MLLP Server (Port 6667)

KIS empfängt HL7 v2.5-Nachrichten von MEDICO (oder anderen Systemen):

```
\x0d\x0b
MSH|^~\&|MEDICO|1.2.276.0.79.3.7.1|KIS|1.2.276.0.79.3.7.1|20261101120000||ADT^A01|MSG000001|P|2.5
EVN|ADT^A01|20261101120000
PID|1||P001^^^MEDICO||Mustermann^Max||19850315|M
NK1|1|Mustermann^Hansi|SPO||0123456789
OBX|1|TX|Allergien||Penicillin, Penicillinallergie
\x1c
\x0d
```

### 7.2 Unterstützte Nachrichten

| Typ | Zweck |
|-----|-------|
| ADT^A01 | Patientenaufnahme |
| ADT^A02 | Patientenausgabe |
| ADT^A04 | Patientenidentifikation |
| ADT^A08 | Terminankündigung |
| ORU^R01 | Befundübermittlung |
| ORM^O01 | Terminbestellung |
| SIU^S12 | Terminänderung |
| SIU^S13 | Terminstorno |

### 7.3 HL7 → KIS Mapping

```
PID-3.1  → patients.external_id
PID-5.1  → patients.name_family
PID-5.2  → patients.name_given
PID-7    → patients.birth_date
PID-8    → patients.gender
PID-11.1 → patients.address_street
PID-11.5 → patients.address_zip
PID-11.6 → patients.address_city
PID-13   → patients.phone
PID-18   → patients.insurance_number
OBX-5    → patients.allergies (bei OBX-3 = Allergien)
```

---

## 8. Monitoring & Betrieb

### 8.1 Status-Endpoint

```
GET /api/v1/interfaces/medico/status

{
  "connected": true,
  "last_sync": "2026-11-01T12:00:00Z",
  "sync_interval_seconds": 30,
  "patients_synced": 15432,
  "appointments_synced": 89234,
  "errors_last_24h": 3,
  "token_valid_until": "2026-11-01T13:00:00Z",
  "gdt_export_enabled": true
}
```

### 8.2 Alerting

| Ereignis | Schwere | Aktion |
|----------|---------|--------|
| OAuth-Refresh fehlgeschlagen | CRITICAL | Alert: MEDICO nicht erreichbar |
| 3 aufeinanderfolgende Sync-Fehler | WARNING | Alert: Datenabgleich gestört |
| Sync-Verschiebung > 5 min | WARNING | Alert: Sync verzögert |
| Token abgelaufen, Refresh nicht möglich | CRITICAL | Alert: Manuelle Intervention nötig |
| GDT-Export fehlgeschlagen | ERROR | Alert: Export prüfen |

---

## 9. Migration von MEDICO zu KIS

### 9.1 Migrationsplan

```
Woche 1:  Test-Setup
  - KIS in Staging-Umgebung installieren
  - MEDICO-Sandbox mit Testdaten anlegen
  - Initial-Import testen
  - Datenkonsistenz prüfen (Anzahl Patienten, Termine)

Woche 2:  Produktiv-Import
  - Vollständiger Initial-Import aus MEDICO
  - Inkrementeller Sync starten
  - KIS und MEDICO parallel betreiben (2 Wochen)
  - Abgleich: Alle neuen Patienten/Termine in beiden Systemen

Woche 3:  Parallelbetrieb
  - KIS wird produktiv
  - MEDICO nur noch als Fallback
  - Alle Benutzer auf KIS umstellen

Woche 4:  MEDICO-Abschaltung
  - MEDICO schreibgeschützt
  - Letzter GDT-Export
  - MEDICO abschalten
  - Datenarchiv: MEDICO-DB wird archiviert (10 Jahre)

Ab Woche 5:  KIS ist das einzige System
  - MEDICO-Datenarchiv wird nach 10 Jahren vernichtet
  - KIS betreibt alle Funktionen
```

### 9.2 Rollback

```
Falls KIS in Woche 1-3 ausfällt:
  - MEDICO ist noch aktiv (parallel)
  - KIS-Änderungen werden via GDT-Export zurück in MEDICO importiert
  - Rollback < 1 Stunde
```

### 9.3 Datenarchiv (MEDICO)

```
Nach MEDICO-Abschaltung:
  - MEDICO-DB wird als Read-Only-Backup archiviert
  - Aufbewahrung: 10 Jahre (DSGVO / Medizinalrecht)
  - Verschlüsselt (AES-256)
  - Nach 10 Jahren: Vernichtung mit Protokoll
```

---

## 10. Sicherheit

### 10.1 OAuth 2.0

```
- Client Credentials Flow (keine User-Session)
- Token-Validity: 1 Stunde
- Auto-Refresh: 5 Minuten vor Ablauf
- Token-Storage: In-Memory (nicht auf Disk)
- Client Secret: NUR aus ENV (MEDICO_CLIENT_SECRET)
- TLS 1.3 für alle FHIR-Calls
- Zertifikats-Verifikation: Strikt
```

### 10.2 Datenverschlüsselung

```
- Im Transit: TLS 1.3 (rustls)
- At Rest: AES-256-GCM (sensible Felder)
- Master Key: KIS_MASTER_KEY aus ENV
- Key-Derivation: HMAC-SHA256 (per Record)
- Key-Rotation: Jährlich, alte Keys 10 Jahre aufbewahrt
```

### 10.3 Audit-Log

```
Jeder MEDICO-Import wird geloggt:
  - Timestamp
  - Source: medico
  - Record-Typ: patient/appointment/...
  - Record-ID: external_id
  - Aktion: create/update/sync
  - IP-Adresse des MEDICO-Servers
  - Hash-Chain-Eintrag

Aufbewahrung: 10 Jahre (DSGVO)
```

---

## 11. Test-Setup

### 11.1 MEDICO Mock-Server

```rust
// tests/medico_mock.rs
// Mock-Server simuliert MEDICO FHIR-Endpunkte

#[tokio::test]
async fn test_patient_import() {
    let mock = MockMedico::new();
    let client = MedicoClient::new(mock.url());
    
    let patients = client.fetch_patients().await.unwrap();
    assert_eq!(patients.len(), 100);
    assert!(patients.iter().all(|p| p.external_id.is_some()));
}
```

### 11.2 Testdaten

```
- 1000 Testpatienten (synthetisch)
- 5000 Testtermine
- 100 Testärzte
- 50 Testpraxen
- 2000 Testbefunde
- 500 Testdokumente
```

### 11.3 Test-Szenarien

| Szenario | Erwartetes Ergebnis |
|----------|-------------------|
| Initial-Import 1000 Patienten | Alle in KIS, 0 Duplikate |
| Inkrementeller Sync (5 geänderte Patienten) | Nur 5 aktualisiert |
| OAuth-Token abgelaufen | Auto-Refresh, kein Fehler |
| MEDICO nicht erreichbar | Fehler geloggt, KIS läuft weiter |
| GDT-Export 10000 Records | Datei erzeugt, alle Records enthalten |
| HL7 ADT^A01 | Patient in KIS angelegt |
| HL7 SIU^S13 (Storno) | Termin in KIS storniert |

---

## 12. Zusammenfassung

**KIS ist der vollumfängliche Ersatz für CGM MEDICO.**

- Alle MEDICO-Funktionen sind in KIS implementiert
- Der MEDICO-Import dient ausschließlich der Migration
- Nach erfolgreicher Migration wird MEDICO abgeschaltet
- KIS deckt Patienten, Termine, Ärzte, Befunde, DICOM, HL7, FHIR, XDS, GDT, TI ab
- KIS ist skalierbarer, sicherer und DSGVO-konformer als MEDICO
- KIS ist modular und erweiterbar — MEDICO war es nicht
