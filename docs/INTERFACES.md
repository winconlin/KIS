# KIS — Medizinische Schnittstellen

> **Krankenhaus-Informations-System (KIS)**
> Detaillierte Dokumentation der vier unterstützten Medizintechnik-Schnittstellen: HL7 v2.5 (MLLP), FHIR R4 (REST/JSON), DICOM (C-STORE SCP) und XDS (Document Registry / Repository).
>
> **Ist-Zustand:** Die Module in `src/schnittstellen/` (`hl7.rs`, `fhir.rs`, `dicom.rs`, `xds.rs`) sind als **Grundgerüst / In Entwicklung** vorhanden. Die Konfiguration in `config/kis.toml` ist vollständig definiert. Dieses Dokument beschreibt **Protokoll, Konfiguration, Mapping und konkrete Beispiele** für die Umsetzung.

---

## 0. Übersicht

| Schnittstelle | Protokoll | Transport | Port | Config-Abschnitt | Standard-Status |
|---|---|---|---|---|---|
| HL7 v2.5 | MLLP (Minimal Lower Layer Protocol) | TCP | 6667 | `[interfaces.hl7]` | `enabled = false` |
| FHIR R4 | REST / JSON | HTTP(S) | 8080 (Pfad `/fhir`) | `[interfaces.fhir]` | `enabled = true` |
| DICOM | C-STORE SCP (Service Class Provider) | TCP | 104 | `[interfaces.dicom]` | `enabled = false` |
| XDS | REST / SOAP (IHE XDS.b / XDS-I) | HTTP(S) | 8080 (Pfad `/xds`) | — (Soll: `[interfaces.xds]`) | In Entwicklung |

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
| Netzwerk | Nur vom LIS-IP erreichbar (Firewall) |
| Auth | AE-Whitelist (MSH-3/MSH-5 muss registriert sein) |
| Verschlüsselung | TLS 1.3 (MLLP-over-TLS) |
| Audit | Jede empfangene Nachricht: `interface.hl7.receive` |

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

### 2.4 Endpoint-Mapping

| Methode | FHIR-Pfad | KIS-API | Beschreibung |
|---|---|---|---|
| `GET` | `/fhir/Patient` | `GET /api/patients` | Patienten-Liste (Search) |
| `POST` | `/fhir/Patient` | `POST /api/patients` | Patient anlegen (Create) |
| `GET` | `/fhir/Patient/{id}` | `GET /api/patients/{id}` | Patient lesen (Read) |
| `PUT` | `/fhir/Patient/{id}` | `PUT /api/patients/{id}` | Patient aktualisieren (Update) |
| `DELETE` | `/fhir/Patient/{id}` | `DELETE /api/patients/{id}` | Patient löschen (Delete) |
| `GET` | `/fhir/Patient?_name=Müller` | `GET /api/patients?q=Müller` | Namenssuche |
| `GET` | `/fhir/Appointment` | `GET /api/appointments` | Termin-Liste |
| `POST` | `/fhir/Appointment` | `POST /api/appointments` | Termin anlegen |
| `GET` | `/fhir/Practitioner` | `GET /api/doctors` | Arzt-Liste |
| `GET` | `/fhir/MedicationRequest?patient={id}` | `GET /api/medications?patient_id={id}` | Medikation des Patienten |
| `POST` | `/fhir/MedicationRequest` | `POST /api/medications` | Verordnung |

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

## 3. DICOM (C-STORE SCP)

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

### 3.9 pydicom / rustdicom (Implementierungshinweis)

Für die Implementierung in Rust empfiehlt sich die Crate `dicom` (rustdicom). Beispielhafte C-STORE-SCP-Skizze:

```rust
// Pseudocode (Soll)
use dicom::ops::read::{ReadDataset, ReadOptions};

let config = dicom::config::FileReadConfiguration { ... };
// Auf Port 104 lauschen, C-STORE-Requests empfangen:
// 1. Presentation Context verhandeln
// 2. Dataset lesen
// 3. PatientID extrahieren → DB-Check
// 4. C-STORE-Response senden (0x0000 oder Fehler)
// 5. Audit-Log-Eintrag
```

---

## 4. XDS (Cross-Document Sharing)

### 4.1 Protokoll

XDS (Cross-Document Sharing) ist ein IHE-Standard für den **Austausch von Dokumenten** (Arztbriefe, Befunde, Berichte) zwischen Gesundheitssystemen. Das KIS stellt:

- **Document Registry:** Metadaten-Verzeichnis der Dokumente.
- **Document Repository:** Lager der eigentlichen Dokumente.
- **Search:** Abfrage nach Dokumenten.

> **Ist-Zustand:** `src/schnittstellen/xds.rs` ist ein Platzhalter. Die Konfiguration `[interfaces.xds]` ist noch nicht in `config/kis.toml` enthalten (Soll).

### 4.2 Soll-Konfiguration

```toml
[interfaces.xds]
enabled = false
base_url = "http://localhost:8080/xds"
version = "I"        # XDS-I (Institutionell) oder "b" (XDS.b)
```

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
                    │  SQLite (WAL)       │
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

---

## 7. Troubleshooting

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
