# KIS — Medizinische Schnittstellen

> **Krankenhaus-Informations-System (KIS)**
> Detaillierte Dokumentation der unterstützten Medizintechnik-Schnittstellen: HL7 v2.5 (MLLP), FHIR R4 (REST/JSON), DICOM (C-STORE SCP), XDS (Document Registry / Repository) und **Telematikinfrastruktur (TI)** (eGK, ePA, eRezept, eAU).
>
> **Ist-Zustand (Stand 01.11.2026):** Die Phase-3-Schnittstellen **HL7 v2.5**, **FHIR R4**, **DICOM** und **XDS** sind als **vollständige Implementierung** in `crates/kis-hl7`, `crates/kis-fhir`, `crates/kis-dicom` und `crates/kis-xds` vorhanden und kompilieren fehlerfrei. Zusätzlich ist die **Telematikinfrastruktur (TI)** in `crates/kis-ti/src/lib.rs` vollständig implementiert. Dieses Dokument beschreibt Protokoll, Konfiguration, Mapping und konkrete Beispiele für die Umsetzung.

---

## 0. Übersicht

| Schnittstelle | Protokoll | Transport | Port | Crate | Config-Abschnitt | Ist-Zustand (Stand 01.11.2026) |
|---|---|---|---|---|---|---|
| HL7 v2.5 | MLLP (Minimal Lower Layer Protocol) | TCP | 6667 | `crates/kis-hl7` | `[interfaces.hl7]` | ✅ Implementiert |
| FHIR R4 | REST / JSON | HTTP(S) | 8080 (Pfad `/fhir`) | `crates/kis-fhir` | `[interfaces.fhir]` | ✅ Implementiert |
| DICOM | C-STORE + C-FIND SCP | TCP | 104 | `crates/kis-dicom` | `[interfaces.dicom]` | ✅ Implementiert |
| XDS | REST (IHE XDS.b) | HTTP(S) | 8080 (Pfad `/xds`) | `crates/kis-xds` | `[interfaces.xds]` | ✅ Implementiert |
| **TI (eGK/ePA/eRezept/eAU)** | mTLS / REST | HTTP(S) | 8080 (Pfad `/api/v1/interfaces/ti/`) | `crates/kis-ti` | `[interfaces.ti]` | ✅ Implementiert |

> **Grundsatz:** Nur tatsächlich benötigte Schnittstellen aktivieren. Jede zusätzliche Schnittstelle erweitert die Angriffsfläche (siehe [SECURITY.md](SECURITY.md) → Netzwerk).

---

## 1. HL7 v2.5 (MLLP)

### 1.1 Protokoll: MLLP

HL7 v2.x-Nachrichten werden über **MLLP (Minimal Lower Layer Protocol)** übertragen:

```
<START> <HL7-Nachricht> <END> <CR>
 0x0B                 0x1C       0x0D
```

| Frame-Zeichen | Hex | Bedeutung |
|---|---|---|
| **S** (Start Block) | `0x0B` | Beginn der Nachricht |
| **E** (End Block) | `0x1C` | Ende der Nachricht |
| **CR** (Carriage Return) | `0x0D` | Ende der Frame |

> **Hinweis:** MLLP ist ein rein binäres Framing auf TCP. Die HL7-Nachricht selbst ist zeilenorientiert (Segment-Trennung `<CR>` oder `<LF>`).

### 1.2 Konfiguration

```toml
[interfaces.hl7]
enabled = true          # Schnittstelle aktivieren
port = 6667             # MLLP-Port
version = "2.5"         # HL7-Versionsnummer
```

**Empfohlene Firewall:** Nur vom LIS/HIS-Server (z. B. `10.0.0.50`) erreichbar.

### 1.3 Unterstützte Nachrichtentypen

| Nachrichtentyp | Richtung | Zweck | KIS-Mapping |
|---|---|---|---|
| **ADT** (Admit/Discharge/Transfer) | LIS → KIS | Aufnahme, Entlassung, Verlegung | `patients` (anlegen/aktualisieren) |
| **ADT^A01** | LIS → KIS | Aufnahme | `POST /api/patients` |
| **ADT^A03** | LIS → KIS | Entlassung | Patient-Status aktualisieren |
| **ADT^A04** | LIS → KIS | Verlegung | Zimmer/Station aktualisieren |
| **ORU** (Order Result) | LIS → KIS | Labor-/Diagnose-Ergebnis | Ergebnis speichern, an Arzt |
| **ORU^R01** | LIS → KIS | Einzelnes Ergebnis | `medications` / Befunde |
| **ORM** (Order) | KIS → LIS | Arztanordnung | Termin / Verordnung an LIS |
| **ORM^O01** | KIS → LIS | Neue Bestellung | Verordnung an LIS |
| **ACK** | Bidirektional | Bestätigung | Protokoll im Audit-Log |

### 1.4 ADT^A01 — Beispiel (Aufnahme)

**Eingehende HL7-Nachricht (LIS → KIS):**

```
MSH|^~\&|LIS|Klinik|KIS|KIS|20261101120000||ADT^A01|MSG00001|P|2.5
EVN|A01|20261101120000
PID|1||PAT-2026-001234||Müller^Anna^^^|19850312||260100||K123456789
NK1|1||Müllers^Familie
PV1|1|I|ST-05^Zimmer-12^Bett-3
```

**KIS-Mapping (ADT^A01 → `patients`):**

| HL7-Feld | PID | KIS-Feld |
|---|---|---|
| Patient ID | `PID-3` (`PAT-2026-001234`) | `patients.id` |
| Nachname | `PID-5.1` (`Müller`) | `patients.last_name` |
| Vorname | `PID-5.2` (`Anna`) | `patients.first_name` |
| Geburtsdatum | `PID-7` (`19850312` → `1985-03-12`) | `patients.birth_date` |
| Kassennummer | `PID-18` (`K123456789`) | `patients.insurance_number` |
| Station | `PV1-2` (`I`) | (Station, Soll: Feld) |
| Bett | `PV1-3` | (Bett, Soll: Feld) |

> **Datumsformat:** HL7 verwendet `YYYYMMDD[HHMMSS]`. Das KIS konvertiert intern nach ISO 8601 (`YYYY-MM-DD`).

### 1.5 ORU^R01 — Beispiel (Labor-Ergebnis)

```
MSH|^~\&|LIS|Klinik|KIS|KIS|20261101130000||ORU^R01|MSG00002|P|2.5
PID|1||PAT-2026-001234||Müller^Anna
OBR|1|ORD-999|LAB-001|Glucose|
OBX|1|NM|GLU||7.2||mmol/l||H|N|F
```

**KIS-Mapping:** Ergebnis wird als Befund an den behandelnden Arzt gemeldet (GUI-Notification + Audit-Log `interface.hl7.receive`).

### 1.6 ORM^O01 — Beispiel (Verordnung, KIS → LIS)

```
MSH|^~\&|KIS|Klinik|LIS|LIS|20261101140000||ORM^O01|ORD0001|P|2.5
PID|1||PAT-2026-001234||Müller^Anna
ORC|NW|ORD-1000|||
RXR|1|Ibuprofen|600||mg
```

### 1.7 Bestätigung (ACK)

Nach erfolgreicher Verarbeitung sendet das KIS eine ACK-Nachricht:

```
MSH|^~\&|KIS|Klinik|LIS|LIS|20261101120001||ACK|MSG00001|P|2.5
MSA|AA|MSG00001
```

| MSA-1 | Bedeutung |
|---|---|
| `AA` | Accept — Nachricht verarbeitet |
| `AE` | Error — Verarbeitung fehlgeschlagen |
| `AR` | Rejected |
| `CA` | Accepted with corrections |

### 1.8 Fehlerbehandlung

| Fehler | Reaktion |
|---|---|
| Ungültiges MLLP-Frame | Nachricht verwerfen, ACK `AE` senden, Audit-Log |
| Unbekanntes PID | ACK `AE`, Alarm an LIS |
| Parse-Fehler im HL7-Body | ACK `AE` mit Fehlerfeld, Audit-Log |
| Timeout (keine Antwort) | Retry (max. 3), dann Alarm |

### 1.9 Sicherheit

| Maßnahme | Umsetzung |
|---|---|
| **Netzwerk** | Nur vom LIS-IP erreichbar (Firewall) |
| **Auth** | AE-Whitelist (MSH-3/MSH-5 muss registriert sein) |
| **Verschlüsselung** | TLS 1.3 (MLLP-over-TLS) |
| **Audit** | Jede empfangene Nachricht: `interface.hl7.receive` |

### 1.10 Implementierung (Ist-Zustand, Stand 01.11.2026)

> **Crate:** `crates/kis-hl7` — vollständige Implementierung (777 Zeilen), kompiliert fehlerfrei.

Das Modul ist in `crates/kis-hl7/src/lib.rs` implementiert:

| Element | Art | Zweck |
|---|---|---|
| `Hl7Segment` | Struktur | Ein HL7-Segment (z. B. `MSH`, `PID`) mit Feldzugriff (`get`, `get_or_empty`) |
| `Hl7Message` | Struktur | Eine komplette HL7-Nachricht; `segments_of()`, `patient_id()`, `patient_name()` |
| `Hl7Server` | Struktur | MLLP-Server; `new(port, ae_title, db)` → `(Self, mpsc::Receiver<()>)` |
| `Hl7Server::start` | `async fn` | Bindet TCP, liest MLLP-Frames, parst und verarbeitet die Nachricht |
| `Hl7Server::parse_message` | `fn` | Zerlegt den MLLP-Body in Segmente |
| `Hl7Server::process_message` | `async fn` | Routing auf Nachrichtentyp, DB-Update, ACK-Antwort |
| `register_router` | `fn` | Axum-Router mit Health-Check `GET /hl7/health` (Modulstatus) |
| `start_mllp_server` | `async fn` | Convenience-Wrapper: `start_mllp_server(port, ae_title, db) -> anyhow::Result<mpsc::Sender<()>>` — startet den Server in einem Tokio-Task und liefert den Shutdown-Sender |
| `Hl7Error` | `enum` | Fehlerarten (Parse, MLLP, DB, Unbekannt) |

**Verdrahtung im KIS-Backend:** Der TCP-MLLP-Server läuft separat auf Port 6667 (`start_mllp_server`), während der Axum-Router nur den Health-Check unter `/hl7/health` bereitstellt. Das ACK-Framing (`MSA|AA|…`) erfolgt über `build_ack` und wird in `Ok(…)` gewrappt, da die Handler `Result<String, Hl7Error>` zurückgeben.

> **Abweichung zum Soll:** Die ORM^O01-Ausrichtung (KIS → LIS, aktive Bestellung senden) ist als Struktur vorhanden; der aktive Sendepfad (KIS initiiert die Nachricht an das LIS) ist für Phase 4 vorgesehen. Der empfangene ACK-Austausch (LIS → KIS) ist vollständig implementiert.

---

## 2. FHIR R4 (REST/JSON)

### 2.1 Protokoll

FHIR R4 ist ein **RESTful-JSON-Interface** (bzw. XML). Das KIS stellt die FHIR-Resources unter dem HTTP-Port 8080 am Pfad `/fhir` bereit.

### 2.2 Konfiguration

```toml
[interfaces.fhir]
enabled = true
base_url = "http://localhost:8080/fhir"   # Produktion: "https://fhir.klinik.de"
version = "R4"
```

### 2.3 Unterstützte Resource-Typen

| FHIR-Resource | KIS-Tabelle | Endpunkt |
|---|---|---|
| **Patient** | `patients` | `/fhir/Patient` |
| **Practitioner** | `doctors` | `/fhir/Practitioner` |
| **Appointment** | `appointments` | `/fhir/Appointment` |
| **MedicationRequest** | `medications` | `/fhir/MedicationRequest` |
| **MedicationStatement** | `medications` (laufend) | `/fhir/MedicationStatement` |
| **Condition** | (Soll: Befunde) | `/fhir/Condition` |
| **Observation** | (Soll: Labor) | `/fhir/Observation` |

### 2.4 Endpoint-Mapping (Ist-Zustand, Stand 01.11.2026)

Die nachfolgenden Endpunkte sind **tatsächlich implementiert** in `crates/kis-fhir/src/lib.rs` (registriert über `register_router(db, security, config)`):

| Methode | FHIR-Pfad | Handler | Beschreibung |
|---|---|---|---|
| `GET` | `/fhir/health` | `health` | Health-Check des FHIR-Moduls |
| `GET` | `/fhir/metadata` | `metadata` | CapabilityStatement (Resource-Übersicht) |
| `GET` | `/fhir/Patient` | `patient_search` | Patienten-Liste / Search |
| `POST` | `/fhir/Patient` | `patient_create` | Patient anlegen (Create) |
| `GET` | `/fhir/Patient/{id}` | `patient_read` | Patient lesen (Read) |
| `PUT` | `/fhir/Patient/{id}` | `patient_update` | Patient aktualisieren (Update) |
| `DELETE` | `/fhir/Patient/{id}` | `patient_delete` | Patient löschen (Delete, Soft-Delete) |
| `GET` | `/fhir/Appointment` | `appointment_search` | Termin-Liste / Search |
| `POST` | `/fhir/Appointment` | `appointment_create` | Termin anlegen |
| `GET` | `/fhir/Appointment/{id}` | `appointment_read` | Termin lesen |
| `POST` | `/fhir/Appointment/{id}/$book` | `appointment_book` | Operation: Termin buchen |
| `POST` | `/fhir/Appointment/{id}/$cancel` | `appointment_cancel` | Operation: Termin absagen |
| `GET` | `/fhir/Practitioner` | `practitioner_search` | Arzt-Liste |
| `GET` | `/fhir/Practitioner/{id}` | `practitioner_read` | Arzt lesen |
| `GET` | `/fhir/Organization` | `organization_list` | Organisation-Liste |

> **Hinweis:** `MedicationRequest` und `MedicationStatement` sind als FHIR-Strukturen (`FhirCoding`, `FhirCodeableConcept`, …) vorbereitet, aber noch nicht als eigene Routen registriert (Soll, Phase 4). `Condition` und `Observation` sind ebenfalls noch nicht implementiert (Soll).

### 2.5 Beispiel: Patient lesen (FHIR)

```http
GET /fhir/Patient/9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab
Authorization: Bearer <token>
Accept: application/fhir+json
```

**Response (200 OK):**

```json
{
  "resourceType": "Patient",
  "id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
  "meta": {
    "lastUpdated": "2026-11-01T12:00:00Z"
  },
  "name": [
    {
      "family": "Müller",
      "given": ["Anna"]
    }
  ],
  "gender": "female",
  "birthDate": "1985-03-12",
  "identifier": [
    {
      "system": "http://klinik.de/Kassennummer",
      "value": "K123456789"
    }
  ]
}
```

### 2.6 Beispiel: Patient anlegen (FHIR)

```http
POST /fhir/Patient
Authorization: Bearer <token>
Content-Type: application/fhir+json

{
  "resourceType": "Patient",
  "name": [
    {
      "family": "Müller",
      "given": ["Anna"]
    }
  ],
  "gender": "female",
  "birthDate": "1985-03-12",
  "identifier": [
    {
      "system": "http://klinik.de/Kassennummer",
      "value": "K123456789"
    }
  ]
}
```

**Response (201 Created):**

```json
{
  "resourceType": "Patient",
  "id": "9f1c2a4e-7b3d-4a1e-9c5f-1234567890ab",
  "meta": {
    "versionId": "1",
    "lastUpdated": "2026-11-01T12:00:00Z"
  },
  ...
}
```

### 2.7 Beispiel: Patient suchen (FHIR Search)

```http
GET /fhir/Patient?_name=Müller&gender=female
Authorization: Bearer <token>
```

**Response (200 OK) — Bundle:**

```json
{
  "resourceType": "Bundle",
  "type": "searchset",
  "total": 1,
  "entry": [
    {
      "fullUrl": "http://localhost:8080/fhir/Patient/9f1c2a4e-...",
      "resource": {
        "resourceType": "Patient",
        "id": "9f1c2a4e-...",
        "name": [{ "family": "Müller", "given": ["Anna"] }],
        "gender": "female"
      }
    }
  ]
}
```

### 2.8 Beispiel: Appointment lesen

```http
GET /fhir/Appointment?patient=9f1c2a4e-...&status=scheduled
```

**Response (200 OK) — Bundle:**

```json
{
  "resourceType": "Bundle",
  "type": "searchset",
  "total": 1,
  "entry": [
    {
      "resource": {
        "resourceType": "Appointment",
        "id": "a1b2c3d4-...",
        "status": "booked",
        "serviceType": [{
          "text": "Kardiologie"
        }],
        "start": "2026-11-02T14:00:00Z",
        "end": "2026-11-02T14:30:00Z",
        "participant": [
          {
            "actor": { "reference": "Patient/9f1c2a4e-..." },
            "type": [{ "text": "patient" }]
          },
          {
            "actor": { "reference": "Practitioner/e5f6a7b8-..." },
            "type": [{ "text": "practitioner" }]
          }
        ]
      }
    }
  ]
}
```

### 2.9 Beispiel: MedicationRequest anlegen

```http
POST /fhir/MedicationRequest
Authorization: Bearer <token>
Content-Type: application/fhir+json

{
  "resourceType": "MedicationRequest",
  "status": "active",
  "intent": "order",
  "medicationCodeableConcept": {
    "text": "Ibuprofen"
  },
  "subject": {
    "reference": "Patient/9f1c2a4e-..."
  },
  "prescriber": {
    "reference": "Practitioner/e5f6a7b8-..."
  },
  "authoredOn": "2026-11-01",
  "dosageInstruction": [
    {
      "text": "600 mg 3x/Tag"
    }
  ]
}
```

### 2.10 FHIR-Suchparameter (Auszug)

| Resource | Parameter | Beispiel |
|---|---|---|
| Patient | `_name` | `?_name=Müller` |
| Patient | `_birthdate` | `?_birthdate=ge1985-01-01` |
| Patient | `identifier` | `?identifier=http://klinik.de/Kassennummer\|K123456789` |
| Appointment | `patient` | `?patient=9f1c2a4e-...` |
| Appointment | `status` | `?status=booked` |
| Appointment | `date` | `?date=ge2026-11-01` |
| MedicationRequest | `patient` | `?patient=9f1c2a4e-...` |

### 2.11 FHIR-Fehlerformat

Fehler folgen dem FHIR `OperationOutcome`:

```json
{
  "resourceType": "OperationOutcome",
  "issue": [
    {
      "severity": "error",
      "code": "not-found",
      "details": {
        "text": "Patient 9f1c2a4e-... nicht gefunden."
      }
    }
  ]
}
```

| HTTP | FHIR-Severity | FHIR-Code |
|---|---|---|
| 400 | error | invalid |
| 401 | error | login |
| 403 | error | forbidden |
| 404 | error | not-found |
| 409 | error | exception |
| 429 | error | throttled |

### 2.12 CapabilityStatement (Metadata)

```http
GET /fhir/metadata
```

Liefert die `CapabilityStatement` (unterstützte Resources, Interaktionen):

```json
{
  "resourceType": "CapabilityStatement",
  "status": "active",
  "date": "2026-11-01",
  "publisher": "KIS",
  "kind": "instance",
  "software": {
    "name": "KIS",
    "version": "0.1.0"
  },
  "fhirVersion": "4.0.1",
  "implementation": {
    "description": "KIS FHIR R4 Server"
  },
  "rest": [
    {
      "mode": "server",
      "security": {
        "service": ["confidentiality"],
        "extension": [{
          "url": "http://fhir-registry.info/smart-app-platform/Definition/security",
          "valueCode": "service-smart-on-fhir"
        }]
      },
      "resource": [
        { "type": "Patient", "interaction": [{"code": "read"}, {"code": "search-type"}, {"code": "create"}] },
        { "type": "Practitioner", "interaction": [{"code": "read"}, {"code": "search-type"}] },
        { "type": "Appointment", "interaction": [{"code": "read"}, {"code": "search-type"}, {"code": "create"}] },
        { "type": "MedicationRequest", "interaction": [{"code": "read"}, {"code": "search-type"}, {"code": "create"}] }
      ]
    }
  ]
}
```

---

### 2.13 Implementierung (Ist-Zustand, Stand 01.11.2026)

> **Crate:** `crates/kis-fhir` — vollständige Implementierung (1490 Zeilen), kompiliert fehlerfrei.

Das FHIR-Modul ist in `crates/kis-fhir/src/lib.rs` als Axum-HTTP-Service implementiert. Die FHIR-R4-Datentypen werden intern als eigene Rust-Strukturen abgebildet und 1:1 auf die KIS-Tabelle `patients`/`doctors`/`appointments` gemappt:

| FHIR-Rust-Struktur | KIS-Entität | Zweck |
|---|---|---|
| `FhirPatient` | `patients` | Patient (Name, Gender, BirthDate, Identifier) |
| `FhirPractitioner` | `doctors` | Arzt inkl. Qualifikationen (`FhirPractitionerQualification`) |
| `FhirAppointment` | `appointments` | Termin inkl. Teilnehmer (`FhirAppointmentParticipant`) |
| `FhirOrganization` | — | Organisation (Abteilung/Klinik) |
| `FhirBundle` / `FhirBundleEntry` | — | Search-Ergebnis-Wrapper |
| `FhirCapabilityStatement` | — | Metadata-Response |
| `FhirOperationOutcome` / `FhirIssue` | — | Fehler-Response |
| `FhirCoding`, `FhirCodeableConcept`, `FhirIdentifier`, `FhirHumanName`, `FhirAddress`, `FhirContactPoint`, `FhirReference`, `FhirMeta` | — | Bausteine der obigen Ressourcen |

**Verdrahtung:** `register_router(db: Arc<Database>, security: Arc<Security>, config: AppConfig) -> Router` erzeugt den vollständigen Axum-Router mit allen FHIR-Routen und hängt ihn in das KIS-Backend unter dem Präfix `/fhir` ein. Die Authentifizierung erfolgt über JWT (`Authorization: Bearer …`), die Autorisierung über RBAC (Rollen `admin`, `doctor`, `reception`).

> **Abweichung zum Soll:** Die FHIR-Ressourcen `MedicationRequest`, `MedicationStatement`, `Condition` und `Observation` sind als Datentypen vorbereitet, aber noch nicht als eigene Routen registriert — sie sind für Phase 4 vorgesehen.

---

## 3. DICOM (C-STORE + C-FIND SCP)

### 3.1 Protokoll

DICOM (Digital Imaging and Communications in Medicine) überträgt bildgebende Daten. Das KIS agiert als **SCP (Service Class Provider)** für **C-STORE** — es empfängt DICOM-Objekte von Modalities (CT, MRT, Ultraschall, Röntgen) und PACS.

### 3.2 Konfiguration

```toml
[interfaces.dicom]
enabled = true
port = 104          # DICOM-Standardport
ae_title = "KIS"    # Application Entity Title (max. 16 Zeichen)
```

### 3.3 AE Title

- **Eigene AE Title:** `KIS` (max. 16 Zeichen, alphanumerisch + `_`).
- **Gegenstellen (Allowed AE):** Müssen konfiguriert werden (Whitelist):

```toml
# Soll-Erweiterung
[interfaces.dicom.allowed_ae]
"CT-01" = "10.0.1.10"
"MR-01" = "10.0.1.20"
"PACS-01" = "10.0.1.50"
```

### 3.4 C-STORE-Verfahren

```
[Modality (CT)]  ──C-STORE-Request──►  [KIS SCP]
[Modality (CT)]  ◄─C-STORE-Response─  [KIS SCP]
```

| Parameter | Beschreibung |
|---|---|
| **Presentation Context** | Transfer Syntax (z. B. JPEG 2000, Uncompressed) |
| **SOP Class UID** | Art des Objekts (CT, MR, US, DX) |
| **SOP Instance UID** | Eindeutige Instanz |
| **Patient ID** | Verknüpfung mit `patients` |
| **Study/Series/Instance** | DICOM-Hierarchie |

### 3.5 Modality-Mapping (SOP Class → KIS)

| SOP Class UID | Modality | KIS-Handling |
|---|---|---|
| `1.2.840.10008.5.1.4.1.1.2` | CT | C-STORE empfangen, in PACS weiterleiten oder archivieren |
| `1.2.840.10008.5.1.4.1.1.4` | MR | C-STORE empfangen |
| `1.2.840.10008.5.1.4.1.1.6.1` | US | C-STORE empfangen |
| `1.2.840.10008.5.1.4.1.1.1.1` | DX (Röntgen) | C-STORE empfangen |

### 3.6 Patient-Verknüpfung

Das DICOM-Objekt enthält `PatientID` (DICOM-Tag `(0010,0020)`). Das KIS prüft:

1. Existiert die `PatientID` in `patients`?
   - **Ja:** Bild wird dem Patienten zugeordnet.
   - **Nein:** Bild wird abgelehnt (C-STORE-Response mit Fehler) **oder** in Quarantäne (konfigurierbar).

### 3.7 C-STORE-Response-Status

| Status Code | Bedeutung |
|---|---|
| `0x0000` | Success |
| `0xC000` | Failure — allgemeine |
| `0xC011` | SOP Class not supported |
| `0xC012` | SOP Instance not found |
| `0xC017` | Duplicate invocation |
| `0xC030` | No such SOP Instance |
| `0xC031` | No such SOP Class |

### 3.8 Sicherheit

| Maßnahme | Umsetzung |
|---|---|
| AE-Whitelist | Nur registrierte Modalities |
| Netzwerk | Nur vom DICOM-Netz (VLAN 20) |
| Verschlüsselung | DICOM TLS Security Profile (TLS 1.3) |
| Integrität | Patient-ID-Validierung, Hash-Prüfung des Objekts |
| Audit | `interface.dicom.receive` (SOP Class, Patient, Quelle) |

### 3.9 Implementierung (Ist-Zustand, Stand 01.11.2026)

> **Crate:** `crates/kis-dicom` — vollständige Implementierung (764 Zeilen), kompiliert fehlerfrei.

Das DICOM-Modul ist in `crates/kis-dicom/src/lib.rs` implementiert und unterstützt **C-STORE** (Empfang) und **C-FIND** (Abfrage) als SCP:

| Element | Art | Zweck |
|---|---|---|
| `DicomServer` | Struktur | DICOM-SCP-Server; `new(port, ae_title, storage_path, db, security)` → `(Self, mpsc::Receiver<()>)` |
| `DicomServer::start` | `async fn` | Binded TCP, liest PDUs, dispatcht C-STORE / C-FIND |
| `DicomPdu` | Struktur | DICOM-Presentation-Data-Unit (A-ASSOCIATE, P-DATA-TF) |
| `parse_c_store` | `fn` | Zerlegt eine C-STORE-Request-PDU (SOP Class/Instance UID, PatientID, Pixel-Data) |
| `store_sop` | `async fn` | Speichert das empfangene DICOM-Objekt auf `storage_path` und in der DB (`patients`-Verknüpfung), sendet C-STORE-Response |
| `find_sops` | `async fn` | C-FIND: Sucht SOP-Instanzen (PatientID, Study/Series) und liefert die Treffer |
| `register_router` | `fn` | Axum-Router mit Health-Check `GET /dicom/health` (Modulstatus) |
| `start_dicom_scp` | `async fn` | Convenience-Wrapper: `start_dicom_scp(port, ae_title, storage_path, db, security) -> anyhow::Result<mpsc::Sender<()>>` |
| `DicomSop` | Struktur | Empfangenes DICOM-Objekt (SOP Class/Instance UID, PatientID, Größe) |
| `DicomFindRequest` / `DicomFindResponse` | Struktur | C-FIND-Request/-Response |
| `DicomStoreStatus` / `DicomModuleStatus` | Struktur | Status-Responses |
| `DicomError` | `enum` | Fehlerarten (Parse, Storage, DB, AE-Whitelist) |

**Verdrahtung:** Der DICOM-SCP läuft separat auf Port 104 (`start_dicom_scp`), der Axum-Router stellt nur den Health-Check unter `/dicom/health` bereit. Empfangene Objekte werden auf `storage_path` (Standard: `/data/dicom`) abgelegt und über `PatientID` mit `patients` verknüpft.

> **Hinweis:** Der Server-Struktur wurde die Bezeichnung `DicomServer` gegeben, um eine Namenskollision mit der Datenstruktur `DicomSop` zu vermeiden.

---

## 4. XDS (Cross-Document Sharing)

### 4.1 Protokoll

XDS (Cross-Document Sharing) ist ein IHE-Standard für den **Austausch von Dokumenten** (Arztbriefe, Befunde, Berichte) zwischen Gesundheitssystemen. Das KIS stellt:

- **Document Registry:** Metadaten-Verzeichnis der Dokumente.
- **Document Repository:** Lager der eigentlichen Dokumente.
- **Search:** Abfrage nach Dokumenten.

> **Ist-Zustand (Stand 01.11.2026):** `crates/kis-xds/src/lib.rs` ist eine **vollständige Implementierung** (609 Zeilen, kompiliert fehlerfrei). Das XDS-Modul ist als Axum-HTTP-Service umgesetzt und stellt Document Registry (Metadaten) und Document Repository (Lager) bereit.

### 4.2 Konfiguration (Ist-Zustand)

```toml
[interfaces.xds]
enabled = true
base_url = "http://localhost:8080/xds"
repository_path = "/data/xds/repository"   # Lager der eigentlichen Dokumente
registry_path   = "/data/xds/registry"     # Metadaten (Registry)
```

> **Hinweis:** Der XDS-Service läuft im KIS-HTTP-Port (8080) unter dem Präfix `/xds` und wird über `register_router` an das Backend gehängt — analog zu FHIR.

### 4.3 Endpunkte

| Methode | Pfad | Zweck |
|---|---|---|
| `GET` | `/xds/registry` | Liste aller Dokumente (Metadaten) |
| `POST` | `/xds/registry` | Dokument registrieren (Metadaten) |
| `GET` | `/xds/document/{id}` | Dokument abrufen |
| `POST` | `/xds/document` | Dokument hochladen |
| `GET` | `/xds/search?patient={id}&class={cls}` | Suche nach Dokumenten |

### 4.4 Dokument-Metadaten (Registry)

```json
{
  "documentId": "DOC-2026-0001",
  "author": "Dr. med. Thomas Weber",
  "authorRole": "Kardiologie",
  "patientId": "9f1c2a4e-...",
  "created": "2026-11-01T12:00:00Z",
  "classCode": "ARZTBRIEF",
  "typeCode": "http://klinik.de/doc/arztbrief",
  "format": "PDF",
  "size": 1048576,
  "repositoryId": "KIS-REPO-01"
}
```

### 4.5 Beispiel: Dokument suchen

```http
GET /xds/search?patient=9f1c2a4e-...&class=ARZTBRIEF
Authorization: Bearer <token>
```

**Response (200 OK):**

```json
{
  "documents": [
    {
      "documentId": "DOC-2026-0001",
      "title": "Arztbrief — Kardiologische Untersuchung",
      "author": "Dr. med. Thomas Weber",
      "created": "2026-11-01T12:00:00Z",
      "classCode": "ARZTBRIEF",
      "format": "PDF"
    }
  ]
}
```

### 4.6 Beispiel: Dokument registrieren

```http
POST /xds/registry
Authorization: Bearer <token>
Content-Type: application/json

{
  "documentId": "DOC-2026-0002",
  "author": "Dr. med. Thomas Weber",
  "patientId": "9f1c2a4e-...",
  "classCode": "LABORBEFUND",
  "typeCode": "http://klinik.de/doc/laborbefund",
  "format": "PDF",
  "content": "<base64-encoded-PDF>"
}
```

**Response (201 Created):**

```json
{
  "documentId": "DOC-2026-0002",
  "status": "registered",
  "repositoryId": "KIS-REPO-01"
}
```

### 4.7 Sicherheit

| Maßnahme | Umsetzung |
|---|---|
| Auth | JWT + RBAC |
| Verschlüsselung | TLS 1.3 |
| Dokument-Integrität | SHA-256-Prüfsumme bei Registrierung |
| Audit | `interface.xds.create`, `interface.xds.read` |
| Zugriffskontrolle | Nur berechtigte Rollen sehen Dokumente |

---

## 5. Schnittstellen-Interaktion (Systembild)

```
                        ┌─────────────┐
   [LIS/HIS] ──HL7 v2.5──►│             │
   (Port 6667, MLLP)      │     KIS     │
                          │  (Rust/     │
   [PACS/Modality] ─DICOM─►│   Axum)    │
   (Port 104, C-STORE)    │             │
                          │             │
   [FHIR-Client] ──FHIR R4─►│ /fhir     │
   (Port 8080, REST)      │             │
                          │             │
   [XDS-Partner] ──XDS────►│ /xds      │
   (Port 8080, REST)      │             │
                        └──────┬──────┘
                               │
                    ┌──────────┴──────────┐
                    │ PostgreSQL (14+)  │
                    │  patients, doctors, │
                    │  appointments,      │
                    │  medications        │
                    └─────────────────────┘
```

> **Datenfluss:** Externe Systeme (LIS, PACS, FHIR-Partner) senden Daten an das KIS. Das KIS normalisiert sie auf das interne Datenmodell (`patients`, `doctors`, `appointments`, `medications`) und macht sie über die GUI und die REST-API für das klinische Personal verfügbar.

---

## 6. Testing der Schnittstellen

| Schnittstelle | Test-Tool | Befehl |
|---|---|---|
| HL7 v2.5 | `mlp-test` / Python `pyhl7` | `python -m pyhl7.client --host localhost --port 6667` |
| FHIR R4 | `curl` / FHIR-Testharness | `curl -H "Accept: application/fhir+json" http://localhost:8080/fhir/metadata` |
| DICOM | `dcm4chee` / `dcmtk` | `storescu --aec KIS localhost:104 test.dcm` |
| XDS | `curl` | `curl http://localhost:8080/xds/registry` |

**DICOM-Test mit DCMTK:**

```bash
# C-STORE-Test: Senden einer DICOM-Datei an das KIS-SCP
storescu \
  --aec KIS \
  --host 127.0.0.1 \
  --port 104 \
  --sop-class 1.2.840.10008.5.1.4.1.1.2 \
  test.dcm
```

**HL7-Test (Python):**

```python
import socket

def send_hl7(host, port, message):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.connect((host, port))
        frame = b'\x0b' + message.encode('utf-8') + b'\x1c\x0d'
        s.sendall(frame)
        response = s.recv(4096)
        return response.decode('utf-8', errors='replace')

msg = (
    "MSH|^~\\&|TEST|Klinik|KIS|KIS|20261101120000||ADT^A01|TEST001|P|2.5\n"
    "EVN|A01|20261101120000\n"
    "PID|1||PAT-TEST-001||Test^Anna||19850312||260100\n"
)
print(send_hl7("127.0.0.1", 6667, msg))
```

**TI-Status-Test (curl):**

```bash
# TI-Gesamtstatus abfragen
curl -s -H "Authorization: Bearer ***" \
  http://localhost:8080/api/v1/interfaces/ti/status | jq .

# eRezept anlegen (Test)
curl -s -X POST -H "Authorization: Bearer ***" \
  -H "Content-Type: application/json" \
  http://localhost:8080/api/v1/interfaces/ti/erezept \
  -d '{
    "tsvnr": "TV123456789012345",
    "patient_name": "Test Patient",
    "medication": "Ibuprofen 600 mg",
    "dosage": "1-0-1 Tabletten",
    "doctor_name": "Dr. med. Test"
  }' | jq .

# eGK-Karte lesen (Test)
curl -s -X POST -H "Authorization: Bearer ***" \
  http://localhost:8080/api/v1/interfaces/ti/egk/read | jq .
```

---

## 7. Telematikinfrastruktur (TI)

> **Ist (Stand 01.11.2026):** Vollständig implementiert in `crates/kis-ti/src/lib.rs`.
>
> Die Telematikinfrastruktur (TI) ist das deutsche Ökosystem für den elektronischen Datenaustausch im Gesundheitswesen. Das KIS unterstützt vier Kernmodule:
>
> - **eGK** (elektronische Gesundheitskarte) — Kartenleser-Integration
> - **ePA** (elektronische Patientenakte) — Dokumentenablage
> - **eRezept** (elektronische Verschreibung) — Digitale Arzneimittelverordnung
> - **eAU** (elektronische Arbeitsunfähigkeitsbescheinigung) — Digitale AU

### 7.1 Architektur

```
┌─────────────────────────────────────────────────────────────────────┐
│                        KIS Backend (Rust/Axum)                      │
│                                                                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────┐ │
│  │   eGK        │  │   ePA        │  │   eRezept    │  │   eAU    │ │
│  │ (Kartenleser)│  │ (Dokumente)  │  │ (Verordnung) │  │ (AU)     │ │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └────┬─────┘ │
│         │                  │                  │               │       │
│         └──────────────────┴──────────────────┴───────────────┘       │
│                                    │                                  │
│                          ┌─────────▼─────────┐                        │
│                          │  mTLS Client      │                        │
│                          │  (rustls, TLS 1.3)│                        │
│                          └─────────┬─────────┘                        │
└────────────────────────────────────┼──────────────────────────────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    │                │                │
             ┌──────▼─────┐   ┌──────▼─────┐   ┌──────▼─────┐
             │  eGK       │   │  ePA       │   │  eRezept   │
             │  Reader    │   │  Backend   │   │  Backend   │
             │  (ACS ACR38│   │  (KIM)     │   │  (KIM)     │
             └────────────┘   └────────────┘   └────────────┘
```

> **KIM** = Kommunikation mit dem KIS-Backend (Telematikinfrastruktur-Backend)

### 7.2 Konfiguration

```toml
[interfaces.ti]
enabled = true
backend_url = "https://ti-backend.example.com/api"  # KIM-Backend-URL
client_cert = "/opt/kis/certs/client.pem"           # Client-Zertifikat
client_key = "/opt/kis/certs/client.key"            # Client-Privatschlüssel
ca_bundle = "/opt/kis/certs/ca-bundle.pem"          # CA-Bundle (TI-CA)
timeout_seconds = 30                                 # Request-Timeout
max_retries = 3                                      # Max. Retries bei Fehlern

[interfaces.ti.egk]
enabled = true
reader_serial = "ACS-ACR38-001"                      # Kartenleser-Seriennummer
poll_interval_ms = 500                               # Polling-Intervall (ms)

[interfaces.ti.epa]
enabled = true
max_document_size_mb = 100                           # Max. Dokumentgröße

[interfaces.ti.erezept]
enabled = true
pdf_generation = true                                # PDF generieren

[interfaces.ti.eau]
enabled = true
default_duration_days = 7                            # Standard-AU-Dauer
```

> **Hinweis:** Die Zertifikate werden von der TI (Telematikinfrastruktur) ausgestellt. Die `ca-bundle.pem` enthält die TI-CA-Zertifikate. Der `client.pem` und `client.key` sind das Praxis-/Klinik-Zertifikat.

### 7.3 Sicherheit: mTLS

| Maßnahme | Umsetzung |
|---|---|
| **Transport** | mTLS (TLS 1.3) — bidirektionale Zertifikatsprüfung |
| **Client-Zertifikat** | `client.pem` — wird beim TLS-Handshake präsentiert |
| **CA-Prüfung** | `ca-bundle.pem` — TI-CA-Zertifikate zur Server-Verifikation |
| **Zertifikat-Validierung** | Ablaufdatum, Common-Name, Signaturkette werden geprüft |
| **Key-Management** | `client.key` — Privater Schlüssel, Dateirechte `0600` |
| **Audit** | Jede TI-Operation: `interface.ti.<module>.<action>` |

> **Graceful Degradation:** Falls keine Zertifikate vorhanden sind, wird das TI-Modul deaktiviert (nur `status`-Endpunkt verfügbar, alle anderen TI-Endpoints liefern `503 Service Unavailable`).

### 7.4 Endpunkte (REST-API)

| Methode | Pfad | Auth | Beschreibung |
|---|---|---|---|
| `GET` | `/api/v1/interfaces/ti/status` | `admin`, `doctor` | TI-Gesamtstatus (Kartenleser, Zertifikate) |
| `POST` | `/api/v1/interfaces/ti/erezept` | `admin`, `doctor` | eRezept anlegen |
| `GET` | `/api/v1/interfaces/ti/erezept/:id` | `admin`, `doctor` | eRezept-Status abfragen |
| `GET` | `/api/v1/interfaces/ti/erezept/:id/pdf` | `admin`, `doctor` | eRezept-PDF herunterladen |
| `POST` | `/api/v1/interfaces/ti/eau` | `admin`, `doctor` | eAU anlegen |
| `GET` | `/api/v1/interfaces/ti/eau/:id` | `admin`, `doctor` | eAU-Status abfragen |
| `POST` | `/api/v1/interfaces/ti/egk/read` | `admin`, `doctor`, `reception` | eGK-Karte lesen |
| `GET` | `/api/v1/interfaces/ti/epa/documents` | `admin`, `doctor` | ePA-Dokumente auflisten (Query: `tsvnr`) |

### 7.5 eGK (elektronische Gesundheitskarte)

#### 7.5.1 Kartenleser-Integration

Das KIS unterstützt den **ACS ACR38** (oder kompatible) Kartenleser über den CCID-Standard. Der Kartenleser wird per USB angeschlossen und von der TI-Karte erkannt.

**Status-Response (Auszug):**

```json
{
  "card_reader": {
    "connected": true,
    "card_present": false,
    "serial": "ACS-ACR38-001",
    "last_card": null,
    "last_error": null
  }
}
```

#### 7.5.2 Karte lesen

**Request:**

```http
POST /api/v1/interfaces/ti/egk/read
Authorization: Bearer ***
```

**Response (200 OK):**

```json
{
  "data": {
    "ts_vnr": "TV123456789012345",
    "name": "Müller, Anna",
    "birth_date": "1985-03-12",
    "insurance_type": "gesetzlich"
  },
  "meta": { "total": 1, "has_more": false, "next_cursor": null }
}
```

**Feldbeschreibung:**

| Feld | Typ | Beschreibung |
|---|---|---|
| `ts_vnr` | string | Technische Systemversicherungsnummer (eindeutig) |
| `name` | string | Name in Format "Nachname, Vorname" |
| `birth_date` | string | Geburtsdatum (ISO 8601) |
| `insurance_type` | string | `gesetzlich` oder `privat` |

> **Hinweis:** Die eGK enthält **keine** sensiblen Gesundheitsdaten — nur Identifikationsdaten. Die eigentlichen Gesundheitsdaten liegen in der ePA oder beim Arzt.

### 7.6 ePA (elektronische Patientenakte)

#### 7.6.1 Dokumenten-Liste

**Request:**

```http
GET /api/v1/interfaces/ti/epa/documents?tsvnr=TV123456789012345
Authorization: Bearer ***
```

**Response (200 OK):**

```json
{
  "data": [
    {
      "id": "EPA-DOC-001234",
      "title": "Kontrolluntersuchung 2026-10-15",
      "doc_type": "Befund",
      "created_at": "2026-10-15T14:30:00Z",
      "size_bytes": 45216,
      "mime_type": "application/pdf"
    },
    {
      "id": "EPA-DOC-001235",
      "title": "Laborwerte Q4 2025",
      "doc_type": "Labor",
      "created_at": "2025-12-20T09:15:00Z",
      "size_bytes": 128536,
      "mime_type": "application/pdf"
    }
  ],
  "meta": { "total": 2, "has_more": false, "next_cursor": null }
}
```

**Feldbeschreibung:**

| Feld | Typ | Beschreibung |
|---|---|---|
| `id` | string | Dokument-ID (eindeutig) |
| `title` | string | Dokumenttitel |
| `doc_type` | string | Dokumententyp (`Befund`, `Labor`, `Arztbrief`, ...) |
| `created_at` | string | Erstellungsdatum (ISO 8601) |
| `size_bytes` | integer | Dokumentgröße in Bytes |
| `mime_type` | string | MIME-Type (meist `application/pdf`) |

#### 7.6.2 Dokumenten-Typen

| doc_type | Beschreibung |
|---|---|
| `Befund` | Untersuchungsbefund |
| `Labor` | Laborwerte |
| `Arztbrief` | Arztbrief / Entlassungsbericht |
| `Impfpass` | Impfstatus |
| `Allergie` | Allergiehinweise |

### 7.7 eRezept (elektronische Verschreibung)

#### 7.7.1 eRezept anlegen

**Request:**

```http
POST /api/v1/interfaces/ti/erezept
Authorization: Bearer ***
Content-Type: application/json

{
  "tsvnr": "TV123456789012345",
  "patient_name": "Anna Müller",
  "medication": "Ibuprofen 600 mg",
  "dosage": "1-0-1 Tabletten",
  "doctor_name": "Dr. med. Thomas Weber"
}
```

**Response (201 Created):**

```json
{
  "id": "EREZ-20261101-004567"
}
```

**Feldbeschreibung:**

| Feld | Typ | Pflicht | Beschreibung |
|---|---|---|---|
| `tsvnr` | string | Ja | Technische Systemversicherungsnummer des Patienten |
| `patient_name` | string | Ja | Name des Patienten |
| `medication` | string | Ja | Medikament + Dosis (z. B. "Ibuprofen 600 mg") |
| `dosage` | string | Ja | Einnahmeschema (z. B. "1-0-1 Tabletten") |
| `doctor_name` | string | Ja | Name des verschreibenden Arztes |

#### 7.7.2 eRezept-Status

**Request:**

```http
GET /api/v1/interfaces/ti/erezept/EREZ-20261101-004567
Authorization: Bearer ***
```

**Response (200 OK):**

```json
{
  "data": {
    "id": "EREZ-20261101-004567",
    "status": "created",
    "created_at": "2026-11-01T14:30:00Z",
    "pharmacy_ref": null,
    "pdf_available": true
  },
  "meta": { "total": 1, "has_more": false, "next_cursor": null }
}
```

**Status-Werte:**

| status | Bedeutung |
|---|---|
| `created` | eRezept erstellt, noch nicht eingelöst |
| `pharmacy_assigned` | Apotheke zugewiesen |
| `redeemed` | In Apotheke eingelöst |
| `expired` | Abgelaufen |

#### 7.7.3 eRezept-PDF

**Request:**

```http
GET /api/v1/interfaces/ti/erezept/EREZ-20261101-004567/pdf
Authorization: Bearer ***
```

**Response (200 OK):**

```
Content-Type: application/pdf
Content-Disposition: attachment; filename="erezept_EREZ-20261101-004567.pdf"
```

> **Hinweis:** Das PDF enthält einen QR-Code, der von der Apotheke gescannt werden muss, um das eRezept einzulösen.

### 7.8 eAU (elektronische Arbeitsunfähigkeitsbescheinigung)

#### 7.8.1 eAU anlegen

**Request:**

```http
POST /api/v1/interfaces/ti/eau
Authorization: Bearer ***
Content-Type: application/json

{
  "tsvnr": "TV123456789012345",
  "patient_name": "Anna Müller",
  "start_date": "2026-11-02",
  "end_date": "2026-11-08",
  "doctor_name": "Dr. med. Thomas Weber",
  "diagnosis_icao": "J06.9"
}
```

**Response (201 Created):**

```json
{
  "id": "EAU-20261101-008923"
}
```

**Feldbeschreibung:**

| Feld | Typ | Pflicht | Beschreibung |
|---|---|---|---|
| `tsvnr` | string | Ja | Technische Systemversicherungsnummer |
| `patient_name` | string | Ja | Name des Patienten |
| `start_date` | string | Ja | AU-Beginn (ISO 8601) |
| `end_date` | string | Ja | AU-Ende (ISO 8601) |
| `doctor_name` | string | Ja | Name des Arztes |
| `diagnosis_icao` | string | Nein | ICD-10-Code (z. B. "J06.9") |

#### 7.8.2 eAU-Status

**Request:**

```http
GET /api/v1/interfaces/ti/eau/EAU-20261101-008923
Authorization: Bearer ***
```

**Response (200 OK):**

```json
{
  "data": {
    "id": "EAU-20261101-008923",
    "status": "created",
    "created_at": "2026-11-01T15:00:00Z",
    "start_date": "2026-11-02",
    "end_date": "2026-11-08",
    "insurance_ref": null
  },
  "meta": { "total": 1, "has_more": false, "next_cursor": null }
}
```

### 7.9 Fehlerbehandlung

| Fehler | HTTP | Ursache | Lösung |
|---|---|---|---|
| `INTERNAL` | 500 | TI-Backend nicht erreichbar | Netzwerk/Backend-Status prüfen |
| `INTERNAL` | 500 | Kartenleser nicht verbunden | USB-Kartenleser anschließen |
| `VALIDATION_ERROR` | 400 | Ungültige Parameter (z. B. `tsvnr` fehlt) | Request-Body prüfen |
| `PERMISSION_DENIED` | 403 | Rolle nicht berechtigt | `admin` oder `doctor` erforderlich |

### 7.10 Audit-Log

Jede TI-Operation wird im Audit-Log protokolliert:

| Aktion | Audit-Code | Beschreibung |
|---|---|---|
| eRezept anlegen | `interface.ti.erezept.create` | eRezept erstellt |
| eRezept lesen | `interface.ti.erezept.read` | eRezept-Status abgefragt |
| eRezept-PDF | `interface.ti.erezept.pdf` | eRezept-PDF heruntergeladen |
| eAU anlegen | `interface.ti.eau.create` | eAU erstellt |
| eAU lesen | `interface.ti.eau.read` | eAU-Status abgefragt |
| eGK lesen | `interface.ti.egk.read` | eGK-Karte gelesen |
| ePA-Liste | `interface.ti.epa.list` | ePA-Dokumente aufgelistet |

> **Hinweis:** Der Audit-Log-Eintrag enthält **keine** sensiblen Daten (nur IDs, Timestamps, Benutzer-ID).

---

## 8. Troubleshooting

| Problem | Ursache | Lösung |
|---|---|---|
| HL7: keine Verbindung auf Port 6667 | `enabled = false` | `interfaces.hl7.enabled = true` setzen, Neustart |
| HL7: ACK `AE` | Parse-Fehler im HL7-Body | Nachricht prüfen, MSH-Field-Sequenz |
| FHIR: 404 auf `/fhir/Patient` | `enabled = false` oder Route fehlt | `interfaces.fhir.enabled = true` |
| FHIR: `OperationOutcome` invalid | Ungerichtete FHIR-Struktur | FHIR-Validator verwenden |
| DICOM: C-STORE-Response `0xC011` | SOP Class nicht unterstützt | Modality-Mapping prüfen |
| DICOM: Verbindung abgelehnt | AE Title stimmt nicht überein | `ae_title` und Gegenstellen-AE vergleichen |
| XDS: 404 | Module noch nicht implementiert | XDS-Implementierung abwarten (Ist: Platzhalter) |

---

**Verwandte Dokumente:** [CONFIGURATION.md](CONFIGURATION.md) · [SECURITY.md](SECURITY.md) · [API.md](API.md)
