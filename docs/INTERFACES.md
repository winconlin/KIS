# KIS — Medizinische Schnittstellen

Stand: 02.11.2026 · Technische Ausbauvorgaben, keine Konformitätserklärung

## Tatsächlicher Stand

| Schnittstelle | Geplant | Aktuell |
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
| ADT^A01 | Aufnahme |
| ADT^A02 | Verlegung |
| ADT^A03 | Entlassung |
| ADT^A04 | Registrierung |
| ADT^A08 | Aktualisierung von Patientendaten |
| ORU^R01 | Befundübermittlung |
| ORM^O01 | Allgemeine Auftragsnachricht |
| SIU^S12 | Neue Terminbuchung |
| SIU^S13 | Umplanung |
| SIU^S15 | Stornierung |

Logisches Beispiel mit einer Zeile pro Segment; auf dem Transport sind CR-Trennzeichen zu verwenden:

```text
MSH|^~\&|MEDICO|KLINIK|KIS|KLINIK|20261102120000||ADT^A01|MSG000001|P|2.5
EVN|A01|20261102120000
PID|1||P001^^^MEDICO||Mustermann^Max||19850315|M
PV1|1|I|WARD^ROOM^BED
```

PID-3 ist ein externer Patientenidentifikator einschließlich zuständiger Vergabestelle. Er darf nicht ungeprüft eine interne UUID ersetzen. PID-5.1/-5.2 enthalten Familien-/Vornamen, PID-7 das Geburtsdatum. PV1-2 ist die Patientenklasse; PV1-3 enthält den Aufenthaltsort. PID-18 ist eine Patientenkontonummer, keine universelle Versicherungsnummer. Versicherungsdaten brauchen ein abgestimmtes IN1-/IN2-/Profil-Mapping.

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

`requester` ist das R4-Feld für den Verordnenden. Ein CapabilityStatement benötigt unter anderem status, date, kind, fhirVersion und format; security.service enthält CodeableConcept-Objekte. Die Beispiele sind technische Entwürfe und noch nicht gegen ein projektspezifisches ISiK-Paket validiert.

Referenz: [FHIR R4](https://hl7.org/fhir/R4/). Für eine Implementierung zusätzlich die konkret vereinbarte ISiK-Version, CapabilityStatement und OperationDefinitions der Gegenstelle prüfen. Proprietäre $book-/Storno-Operationen sind nicht ohne dieses Profil verallgemeinerbar.

## DICOM

Vor Implementierung sind SOP Classes, Transfer Syntaxes, Rollen, Patientenzuordnung, Quarantäne und TLS-Profile in einem Conformance Statement festzulegen. Ein AE Title ist ein Anwendungsname und allein kein kryptografischer Identitätsnachweis.

| C-STORE-Statusbereich | Bedeutung |
|---|---|
| 0x0000 | Erfolg |
| 0xA7xx | Ressourcenmangel |
| 0xA9xx | Datensatz passt nicht zur SOP Class |
| 0xCxxx | Datensatz nicht interpretierbar |

Nicht jedem beliebigen Cxxx-Wert darf eine allgemeingültige Detailbedeutung zugewiesen werden. Gegenstellen können ihn implementierungsspezifisch verwenden.

Beispiel für einen künftig vorhandenen Test-SCP mit DCMTK:

```bash
storescu -aec KIS 127.0.0.1 104 test.dcm
```

Host und Port sind Positionsargumente. Ein laufender KIS-DICOM-SCP ist derzeit nicht vorhanden; dieser Befehl ist deshalb keine aktuelle Installationsprüfung.

## XDS.b und XDS-I.b

XDS ist „Cross-Enterprise Document Sharing“. XDS-I ergänzt den Austausch von Bildgebungsinformationen. Es ist keine alternative Versionskennung für „Institutionell“.

Ein eigener JSON-Endpunkt namens /xds/registry wäre zunächst eine proprietäre API. Für XDS.b sind Akteure und passende Transaktionen festzulegen, etwa Registry Stored Query (ITI-18), Provide and Register Document Set-b (ITI-41), Register Document Set-b (ITI-42) und Retrieve Document Set (ITI-43).

Metadaten, Identifikatoren, Policies, Registry-/Repository-Verhalten und Fehlerbehandlung brauchen eigene Konformitätstests. Das Vorhandensein von Upload-/Download-Beispielen belegt keine IHE-Konformität.

## Verifikation vor einer Freigabe

Pro Schnittstelle sind Nachrichten-/Ressourcenprofile, valide synthetische Beispiele, Fehler-/Retry-Verhalten, Identitätsmapping und Sicherheit zu testen. Parser- oder JSON-Syntaxprüfungen allein reichen nicht. Tests mit einer echten oder verbindlich spezifizierten Gegenstelle sind noch offen.
