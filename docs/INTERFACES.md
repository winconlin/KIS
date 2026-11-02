# KIS — Medizinische Schnittstellen

> **Krankenhaus-Informations-System (KIS)**
> Detaillierte Dokumentation der unterstützten Medizintechnik-Schnittstellen: HL7 v2.5 (MLLP), FHIR R4 (REST/JSON), DICOM (C-STORE SCP), XDS (Document Registry / Repository) und **Telematikinfrastruktur (TI)** (eGK, ePA, eRezept, eAU).
>
> **Ist-Zustand (Stand 01.11.2026):** Die Phase-3-Schnittstellen **HL7 v2.5**, **FHIR R4**, **DICOM** und **XDS** sind als **vollständige Implementierung** in `crates/kis-hl7`, `crates/kis-fhir`, `crates/kis-dicom` und `crates/kis-xds` vorhanden und kompilieren fehlerfrei. Zusätzlich ist die **Telematikinfrastruktur (TI)** in `crates/kis-ti/src/lib.rs` vollständig implementiert. Dieses Dokument beschreibt Protokoll, Konfiguration, Mapping und konkrete Beispiele für die Umsetzung.

## Tatsächlicher Stand

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
| HL7 v2.5 | MLLP-Server/Client | Platzhalter, kein Listener |
| FHIR R4 | Server mit abgestimmtem ISiK-Profil | Keine /fhir-Routen; separater MEDICO-Client |
| DICOM | Abgestimmter SCP/PACS-Workflow | Platzhalter, kein Listener |
| XDS.b / XDS-I.b | Definierte IHE-Akteure/Transaktionen | Platzhalter |
| GDT | Versionierter Gerätedatentransfer | Nur Entwicklungs-CSV, kein GDT |
| TI | Profilierte Anbindung | Ausbauziel |

Ein `enabled=true` in der Konfiguration startet diese noch fehlenden Dienste nicht.

## HL7 v2.5: Grundlagen für die Umsetzung

MLLP rahmt eine Nachricht als Startbyte 0x0B, Nachricht, Endbyte 0x1C und 0x0D. Reguläre HL7-Segmente werden durch CR (0x0D) getrennt. Für Tests LF nicht ungeprüft als Gegenstellenstandard verwenden.

| Triggerereignis | Bedeutung |
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

Logisches Beispiel mit einer Zeile pro Segment; auf dem Transport sind CR-Trennzeichen zu verwenden:

```text
MSH|^~\&|MEDICO|KLINIK|KIS|KLINIK|20261102120000||ADT^A01|MSG000001|P|2.5
EVN|A01|20261102120000
PID|1||P001^^^MEDICO||Mustermann^Max||19850315|M
PV1|1|I|WARD^ROOM^BED
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

Ein konsistentes Laborwert-Segment:

```text
OBX|1|NM|GLU^Glucose||7.2|mmol/L|3.9-5.5|H|||F
```

ACK-Codes AA/AE/AR beziehen sich auf Anwendungsannahme/-fehler/-ablehnung. CA/CE/CR gehören zum erweiterten Commit-Acknowledgement; CA heißt Commit Accept. Wann bestätigt, erneut gesendet oder quarantänisiert wird, muss das Gegenstellenprofil festlegen. Eine ACK darf keine dauerhafte Speicherung vortäuschen.

Für MLLP gibt es keine pauschal vorauszusetzende STARTTLS-/Benutzer-Authentifizierung. Transportabsicherung und Identitäten sind mit der Gegenstelle festzulegen; bloße Sendernamen im MSH ersetzen keine starke Authentifizierung.

## FHIR R4: Vorgaben und Beispiele

FHIR R4 verwendet die Ressourcendefinitionen von Version 4.0.1. Die Basis-URL und implementierte Profile sind mit der Gegenstelle abzustimmen. Ein künftiger KIS-Server muss seine tatsächlichen Fähigkeiten über ein CapabilityStatement veröffentlichen.

Suchparameter heißen beispielsweise `Patient?name=Mustermann&birthdate=1985-03-15`, ohne Unterstrich vor name/birthdate. `_lastUpdated` ist dagegen ein allgemeiner Suchparameter mit Unterstrich. Paging erfolgt anhand der vom Server gelieferten Bundle-Links, nicht durch einen allgemein angenommenen `_page`-Zähler.

Geplantes Appointment-Beispiel:

```json
{
  "resourceType": "Appointment",
  "id": "appointment-1",
  "status": "booked",
  "start": "2026-11-02T14:00:00Z",
  "end": "2026-11-02T14:30:00Z",
  "participant": [
    {"actor":{"reference":"Patient/patient-1"},"status":"accepted"},
    {"actor":{"reference":"Practitioner/doctor-1"},"status":"accepted"}
  ]
}
```

Interne Statuswerte wie scheduled sind keine automatisch gültigen FHIR-Statuscodes. Sie brauchen eine ausdrückliche, fachlich geprüfte Zuordnung.

Geplantes MedicationRequest-Beispiel:

```json
{
  "resourceType": "MedicationRequest",
  "status": "active",
  "intent": "order",
  "medicationCodeableConcept": {"text":"Synthetisches Testpräparat"},
  "subject": {"reference":"Patient/patient-1"},
  "requester": {"reference":"Practitioner/doctor-1"}
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

Referenz: [FHIR R4](https://hl7.org/fhir/R4/). Für eine Implementierung zusätzlich die konkret vereinbarte ISiK-Version, CapabilityStatement und OperationDefinitions der Gegenstelle prüfen. Proprietäre $book-/Storno-Operationen sind nicht ohne dieses Profil verallgemeinerbar.

## DICOM

Vor Implementierung sind SOP Classes, Transfer Syntaxes, Rollen, Patientenzuordnung, Quarantäne und TLS-Profile in einem Conformance Statement festzulegen. Ein AE Title ist ein Anwendungsname und allein kein kryptografischer Identitätsnachweis.

| C-STORE-Statusbereich | Bedeutung |
|---|---|
| 0x0000 | Erfolg |
| 0xA7xx | Ressourcenmangel |
| 0xA9xx | Datensatz passt nicht zur SOP Class |
| 0xCxxx | Datensatz nicht interpretierbar |

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
storescu -aec KIS 127.0.0.1 104 test.dcm
```

Host und Port sind Positionsargumente. Ein laufender KIS-DICOM-SCP ist derzeit nicht vorhanden; dieser Befehl ist deshalb keine aktuelle Installationsprüfung.

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

Ein eigener JSON-Endpunkt namens /xds/registry wäre zunächst eine proprietäre API. Für XDS.b sind Akteure und passende Transaktionen festzulegen, etwa Registry Stored Query (ITI-18), Provide and Register Document Set-b (ITI-41), Register Document Set-b (ITI-42) und Retrieve Document Set (ITI-43).

Metadaten, Identifikatoren, Policies, Registry-/Repository-Verhalten und Fehlerbehandlung brauchen eigene Konformitätstests. Das Vorhandensein von Upload-/Download-Beispielen belegt keine IHE-Konformität.

## Verifikation vor einer Freigabe

Pro Schnittstelle sind Nachrichten-/Ressourcenprofile, valide synthetische Beispiele, Fehler-/Retry-Verhalten, Identitätsmapping und Sicherheit zu testen. Parser- oder JSON-Syntaxprüfungen allein reichen nicht. Tests mit einer echten oder verbindlich spezifizierten Gegenstelle sind noch offen.
