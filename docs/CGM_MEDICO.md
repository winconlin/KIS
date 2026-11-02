# CGM MEDICO — Client-Prototyp und geplante Ablösung

Stand: 02.11.2026

**Projektziel:** KIS soll langfristig ein eigenständiges System werden und eine vollständige Ablösung von CGM MEDICO ermöglichen. Der aktuelle Prototyp erreicht diese Funktionsabdeckung noch nicht. Eine Abschaltung von MEDICO lässt sich aus diesem Repository-Stand nicht begründen.

## Was bereits vorhanden ist

Der separat konstruierbare Client in `src/schnittstellen/fhir/medico.rs` bietet OAuth-Client-Credentials und FHIR-Leseoperationen für Patient, Appointment und Slot. Der Server startet ihn noch nicht als Import-/Pollingdienst.

| Teil | Stand |
|---|---|
| OAuth-Token | Abruf mit Client Credentials; Erneuerung vor Ablauf |
| FHIR-Listen | Verfolgen next-Links, HTTP-/Parsefehler werden zurückgegeben |
| Paging-Schutz | Zyklen und Wechsel aus der konfigurierten FHIR-Basis werden abgelehnt |
| Feldnamen | postalCode, serviceType, fullUrl und telecom.use abgebildet |
| Change Detector | Wiederholte IDs werden nicht dauerhaft ausgefiltert |
| CSV | Entwicklungs-Export von Patientenstammdaten |
| Persistenter Import | Noch nicht implementiert |
| GDT/HL7-Adapter | Noch nicht implementiert |
| Vollständiger MEDICO-Ersatz | Ausbauziel, nicht nachgewiesen |

Die vorhandenen schreibenden Client-Methoden sind Prototypen. Insbesondere Buchungs-/Storno-Operationen müssen vor Nutzung anhand des tatsächlichen Gegenstellenprofils und dessen OperationDefinitions validiert werden; es gibt keine Freigabe für klinische Datenänderungen.

## Konfiguration und OAuth

`MedicoClient::from_config` übernimmt die unterstützten Einstellungen. Ein Auszug:

```toml
[interfaces.medico]
enabled = false
base_url = "https://fhir.medico.example.com/fhir/"
auth_endpoint = "https://fhir.medico.example.com/fhir/auth/token"
client_id_env = "KIS_MEDICO_CLIENT_ID"
client_secret_env = "KIS_MEDICO_CLIENT_SECRET"
scopes = ["system/Patient.read", "system/Appointment.read", "system/Slot.read"]
request_timeout_seconds = 30
```

Die Domain ist ein Platzhalter. Token-Endpunkt, Scopes und Zugriffsrechte müssen vom Betreiber der konkreten MEDICO-Installation bestätigt werden. Die Scope-Namen sind keine allgemein gültige Zusage der Gegenstelle.

Credentials werden aus den benannten Umgebungsvariablen gelesen. Der Client stellt keine interaktive Benutzersitzung her. Tokens liegen im Speicher; ein neues Client-Credentials-Token ersetzt ein ablaufendes Token. OAuth-Fehlerkörper werden nicht ungeprüft in Logs geschrieben.

enabled, sync_interval_seconds, Slot-Polling, HL7- und GDT-Felder ergeben noch keinen laufenden Dienst. `health_check()` prüft derzeit den Tokenabruf und ist kein umfassender FHIR-/Datenabgleich.

## Zu vereinbarendes Datenmapping

Das aktuelle SQL-Schema ist in [DATABASE.md](DATABASE.md) dokumentiert.

| FHIR-Wert | Vorgesehenes KIS-Ziel | Regel |
|---|---|---|
| Patient.id und identifier | Eigene Zuordnung zu patients.id/external_id | Ressourcentyp, Quellsystem und Identifier-Namensraum erhalten |
| Patient.name.family/given | patients.last_name/first_name | Namensverwendung und mehrere Vornamen berücksichtigen |
| Patient.birthDate | patients.birth_date | FHIR erlaubt auch unvollständige Datumsangaben; Importpolitik fehlt noch |
| Patient.telecom | patients.phone/email | Nach system/use auswählen, nicht nach Arrayposition |
| Patient.address.line/city/postalCode | patients.address_line/address_city/address_zip | Adresstyp und Historie beachten |
| Versicherungskennungen | patients.insurance_no | Nur nach bestätigtem Identifier-/Profil-Mapping |
| Appointment.participant | appointments.patient_id/doctor_id | Referenztyp auflösen, Reihenfolge ist keine Rollenkennung |
| Appointment.status | appointments.status | Explizites Statusmapping erforderlich |
| Practitioner.name | doctors.first_name/last_name | Fachgebiet ggf. über PractitionerRole/Profil abbilden |

Spalten wie name_family, name_given, address_street oder appointments.category sind im aktuellen Schema nicht vorhanden. Ein eigener Identifier-Mapping-Speicher mit geeigneter Unique-Constraint fehlt noch. Ressourcen-ID und fachlicher Identifier sind nicht generell austauschbar.

## Import und Änderungen

FHIR-Paging folgt `Bundle.link[relation=next]`. Antworten werden nicht still auf die erste Seite begrenzt, und fehlerhaft deserialisierbare Ressourcen werden nicht als erfolgreich importiert gezählt.

Der einfache Change Detector startet ohne Cursor am Unix-Epoch und fragt inklusive Zeitgrenze ab. Er verwendet die Startzeit eines erfolgreichen Polls als nächste Grenze. Dadurch bleiben Änderungen während eines Abrufs im nächsten Abfragefenster erreichbar; Wiederholungen sind möglich.

**Noch offen:** Cursor ist nur im Speicher, es gibt keinen verbindlichen Import-Commit-/Acknowledgement-Ablauf. In einem vollständigen Dienst darf die Grenze erst nach dauerhaftem, idempotentem DB-Upsert und erfolgreichem Audit fortgeschrieben werden. Löschungen, Versionen, Historie, Retry-Queue und Fehlerfreigaben benötigen eigene Regeln.

Ein Patient, der später unter derselben ID aktualisiert wird, darf erneut verarbeitet werden. Dauerhafte Deduplication nur nach ID würde echte Änderungen verlieren. Ein „ON CONFLICT DO NOTHING“ allein ist deshalb keine Synchronisationsstrategie.

## Konflikte und Migration

„Last Write Wins“ würde den neueren bestätigten Stand bevorzugen, nicht den älteren. Ob dieses Verfahren fachlich zulässig ist, hängt von der Datenkategorie ab. Für kritische Änderungen sind Quellsystem-Verantwortung und eine manuelle Konfliktklärung festzulegen; Uhren allein sind kein verlässlicher Beweis der fachlichen Gültigkeit.

Vor einer Ablösung werden mindestens benötigt:

1. Funktionsinventar der tatsächlich eingesetzten MEDICO-Module und definiertes Zielsystem.
2. Vollständige Extraktion mit Zuordnung von Patienten, Fällen, Terminen, Befunden, Verordnungen, Dokumenten und benötigten Bildreferenzen.
3. Geprüfte Identitäten, Daten-/Versionsabgleiche, Fehlerlisten und fachliche Freigaben.
4. Abgestimmter Parallelbetrieb mit klarer Quelle pro Datenkategorie.
5. Vollständiger, getesteter Rückweg für sämtliche während der Migration entstandenen Änderungen.
6. Abnahme mit betroffenen Fachbereichen und gesicherte Lesbarkeit des erforderlichen Altarchivs.

Dauer und Abschalttermin ergeben sich aus diesen Nachweisen, nicht aus einem pauschalen Vierwochenplan. Eine garantierte einstündige Rückkehr ist bisher nicht belegt.

## CSV ist kein GDT

GDT steht für **Gerätedatentransfer** und verwendet versionierte Satz-/Feldstrukturen. `export_patients_csv` erzeugt ein separat benanntes, korrekt gequotetes Semikolon-CSV für Entwicklungszwecke. Es ist kein standardkonformer GDT-Export und darf nicht durch Umbenennen in .gdt als kompatibel ausgegeben werden.

Der Patienten-CSV-Export überträgt keine vollständige Termin-, Befund-, Medikations- oder Dokumenthistorie und ist daher kein vollständiger MEDICO-Rollback.

## Tests und verbleibende Abnahmen

Automatische Client-Tests decken unter anderem mehrere Seiten mit 151 Patienten, spätere Versionen derselben ID, HTTP-/Parsefehler, fremde Paging-Links und CamelCase-Felder ab. Eine echte MEDICO-Sandbox, das konkrete ISiK-Paket, Schreiboperationen, vollständige Migration und fachliche Datenabnahme sind noch zu prüfen.

Anforderungen an Verschlüsselung, Aufbewahrung und Schlüssel stehen in [SECURITY.md](SECURITY.md). Eine vollständige DSGVO-Konformität oder Überlegenheit gegenüber einem Fremdprodukt wird hier nicht behauptet.
