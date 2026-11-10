# Funktionslandkarte

Ziel des Systems: Alle Beschäftigten eines Krankenhauses erledigen ihre
tägliche Arbeit in einem System, und alle Daten greifen ineinander — von
der kleinen Klinik bis zum Maximalversorger. Diese Seite zeigt je
Arbeitsbereich, was heute vorhanden ist und was fehlt.

Stand: 10.11.2026.

| Status | Bedeutung |
|---|---|
| **vorhanden** | implementiert und gegen echtes PostgreSQL getestet |
| **Grundlage** | Datenmodell oder Teilfunktion vorhanden, wesentliche Teile fehlen |
| **offen** | nicht vorhanden |

> **Nicht für den Echtbetrieb freigegeben.** Der vorhandene Teil ist
> getestet, aber es fehlen für einen Krankenhausbetrieb unverzichtbare
> Bereiche (Abrechnung, Datenaustausch mit Kostenträgern, Anbindung von
> Labor- und Bildsystemen, Erfassungsmasken) sowie Abnahmen, die kein Test
> ersetzt: Datenschutz-Folgenabschätzung, Penetrationstest, Lasttest mit
> realistischen Datenmengen, Prüfung als Medizinprodukt für
> entscheidungsunterstützende Funktionen.

## Wie die Daten ineinandergreifen

Diese Abläufe sind durchgängig getestet
([`tests/hospital.rs`](../crates/kis-app/tests/hospital.rs)):

| Auslöser | Wirkung an anderer Stelle |
|---|---|
| Aufnahme auf ein Bett | Stationsliste, Bettenübersicht, Produktionsliste der Küche |
| Allergie erfasst | Warnhinweis in der Fallakte, Abgleich bei der Verordnung |
| Nahrungsmittelallergie, Isolation | Hinweis auf der Produktionsliste der Küche |
| „Nüchtern“ gesetzt | kein Tablett auf der Produktionsliste |
| Leistung angefordert | Arbeitsliste der erbringenden Einheit; diese sieht den Fall für die Dauer des Auftrags |
| Auftrag erledigt | Befund in der Fallakte; Zugriff der erbringenden Einheit läuft aus |
| Entlassung / Verlegung | Bett gesperrt, Auftrag an die Bettenaufbereitung; nach Erledigung ist das Bett frei |
| Laborwerte geliefert | Kumulativbefund in der Fallakte, kritische Werte auf der Station |
| jeder Schritt am Fall | Ereignis für Schnittstellen (ohne Personendaten) |

## Patientenservice und Aufnahme

| Aufgabe | Status | Modul |
|---|---|---|
| Patientenstamm anlegen, suchen, ändern | vorhanden | `patients` |
| Aufnahme, Verlegung, Entlassung, Storno; Fallnummer | vorhanden | `cases` |
| Bettenübersicht, gesperrte Betten | vorhanden | `cases`, `org` |
| Terminplanung mit Überschneidungsschutz | vorhanden | `appointments`, `doctors` |
| Kostenträger, Versicherungsverhältnis, Zuzahlung | offen | — |
| Einweiser, Hausarzt, Angehörige | offen | — |
| Wahlleistungen, Behandlungsvertrag, Formulardruck | offen | — |
| Belegungsplanung, Warteliste, geplante Aufnahmen | offen | — |

## Ärztlicher Dienst

| Aufgabe | Status | Modul |
|---|---|---|
| Stationsliste mit Stabilität, Wenn–dann, DNR/DNI | vorhanden | `cases` |
| Übergabe je Einheit | vorhanden | `clinical` |
| Diagnosen (ICD-10-GM), Prozeduren (OPS) | vorhanden (Formatprüfung; kein Abgleich mit dem amtlichen Katalog) | `clinical` |
| Dokumentation mit Freigabe und Korrekturfassung | vorhanden | `clinical` |
| Aufgaben | vorhanden | `clinical` |
| Leistungen anfordern, Befund lesen | vorhanden | `orders` |
| Labor: Verlauf je Analyt, kritische Werte | vorhanden | `lab` |
| Verordnung, Antiinfektiva-Kurs, Allergie-Abgleich | vorhanden (Textabgleich, keine Arzneimitteldatenbank) | `prescriptions` |
| Wechselwirkungen, Dosisprüfung, Arzneimittelkatalog | offen | — |
| Arztbrief aus Falldaten erzeugen, Textbausteine, Druck | offen | — |
| OP-Planung und OP-Dokumentation | offen | — |
| Notaufnahme: Ersteinschätzung, Behandlungsplätze | offen | — |
| Intensivdokumentation | offen | — |
| Aufklärung und Einwilligung (Formulare, Unterschrift) | Grundlage (Dokumentationsart) | `clinical` |
| Bildbetrachtung | offen | — |

## Pflege

| Aufgabe | Status | Modul |
|---|---|---|
| Vitalwerte, Flüssigkeitsbilanz | vorhanden | `nursing` |
| Pflegebericht | vorhanden | `clinical` |
| Gabe von Medikamenten dokumentieren | vorhanden | `prescriptions` |
| Kost anordnen | vorhanden | `meals` |
| Aufgaben, Übergabe | vorhanden | `clinical` |
| Assessments (Barthel, Braden, Sturzrisiko, Schmerz) | offen | — |
| Pflegeplanung, Maßnahmen, Wunddokumentation | offen | — |
| Pflegepersonalbemessung | offen | — |

## Funktionsdienste: Labor, Radiologie, Diagnostik

| Aufgabe | Status | Modul |
|---|---|---|
| Arbeitsliste: annehmen, erledigen, ablehnen | vorhanden | `orders` |
| Befund als Text | vorhanden | `orders` |
| Laborwerte annehmen (REST), Korrektur per erneuter Lieferung | vorhanden | `lab` |
| Anbindung von Laborsystemen (HL7), Analysegeräten | offen (Entwurf außerhalb des Builds) | — |
| Untersuchungstermine planen, Geräte belegen | Grundlage (Feld „geplant für“) | `orders` |
| Strukturierte Befunde, Bildarchiv (DICOM) | offen (Entwurf außerhalb des Builds) | — |

## Therapie, Sozialdienst, Apotheke

| Aufgabe | Status | Modul |
|---|---|---|
| Anforderung und Arbeitsliste | vorhanden | `orders` |
| Therapiedokumentation, Leistungserfassung | Grundlage (Dokumentation als Freitext) | `clinical` |
| Entlassmanagement als geführter Ablauf | offen | — |
| Arzneimittellager, Stationsanforderung, Betäubungsmittelbuch | offen | — |

## Küche, Hauswirtschaft, Transport, Technik

| Aufgabe | Status | Modul |
|---|---|---|
| Kostformen pflegen | vorhanden | `meals` |
| Produktionsliste (ohne Namen und Diagnosen) | vorhanden | `meals` |
| Speiseplan, Menüwahl, Bestellwesen | offen | — |
| Bettenaufbereitung automatisch nach Entlassung/Verlegung | vorhanden | `orders` |
| Aufträge ohne Fallbezug (Störmeldung, Transport) | vorhanden | `orders` |
| Disposition von Transporten, Wegezeiten | offen | — |
| Gerätebestand, Prüfintervalle | offen | — |

## Verwaltung und Abrechnung

| Aufgabe | Status | Modul |
|---|---|---|
| Kodierung erfassen | vorhanden | `clinical` |
| Gruppierung (DRG, PEPP), Plausibilitätsprüfung | offen | — |
| Rechnungen, Datenaustausch mit Kostenträgern (§ 301) | offen | — |
| Statistik, Berichtswesen, Controlling | offen | — |

## Personal

| Aufgabe | Status | Modul |
|---|---|---|
| Personal, Dienstarten, Besetzung, Abwesenheiten, Feiertage | vorhanden | `roster` |
| Prüfung: unbesetzt, doppelt, Ruhezeit, Abwesenheit, Qualifikation | vorhanden | `roster` |
| Wochenbesetzung der Arbeitsplätze | vorhanden | `roster` |
| Stundenliste für die Zeitwirtschaft, eigener Dienstkalender | vorhanden | `roster` |
| Mehrere Personen je Dienst und Tag (Schichtbesetzung Pflege) | Grundlage (je Dienstart und Tag eine Person; mehrere Plätze = mehrere Dienstarten) | `roster` |
| Wünsche, Arbeitszeitkonten, Planvorschläge | offen | — |

## Administration, Datenschutz, Betrieb

| Aufgabe | Status | Ort |
|---|---|---|
| Benutzer, Rollen, Zusatzrollen, Zuordnung zu Einheiten | vorhanden | `kis-app` |
| Rollen und Rechte des Hauses zur Laufzeit ändern | vorhanden | `kis-app`, `kis-core` |
| Richtlinien zur Laufzeit (Behandlungszusammenhang, Sitzung, Passwort) | vorhanden | `kis-core` |
| Module zu- und abschalten | vorhanden (Konfiguration, Neustart) | `kis-core` |
| Behandlungszusammenhang, Notfallzugriff mit Prüfung | vorhanden | `kis-core` |
| Audit-Trail mit Hash-Kette | vorhanden | `kis-core` |
| Auskunft (JSON vollständig), Löschung, Anonymisierung der Stammdaten | vorhanden | `kis-core` |
| Auskunft als CSV/FHIR | Grundlage (ohne Falldaten) | `kis-core` |
| Anonymisierung von Freitexten | offen | — |
| Anmeldung über Verzeichnisdienst (LDAP, SSO) | offen | — |
| Kennzahlen für die Überwachung | Grundlage (Health-Endpunkte) | `kis-app` |

## Schnittstellen

| Aufgabe | Status |
|---|---|
| Ereignisse für externe Systeme abholen (REST, Cursor) | vorhanden |
| HL7 v2, FHIR, DICOM, XDS, GDT, Telematikinfrastruktur, MEDICO | offen — ältere Entwürfe liegen unter `entwurf/` und sind nicht Teil des Builds |

## Oberfläche

| Ansicht | Status |
|---|---|
| Arbeitsplatz: Belegung, Übergabe, Aufgaben, Arbeitsliste, Betten, Küche, Fallübersicht | vorhanden (lesend, mit Schnellerfassung für Aufgaben und Vitalwerte) |
| Patienten, Termine, Ärzte, Dauermedikation | vorhanden |
| Erfassungsmasken für Aufnahme, Diagnosen, Dokumentation, Verordnung, Anforderung | offen |
| Dienstplan, Administration | offen |

Die offenen Punkte sind in [ARBEITSPAKETE.md](ARBEITSPAKETE.md) als
einzeln bearbeitbare Pakete geschnitten.
