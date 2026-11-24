# Enterprise-Backlog — Ausbauprogramm ab 24.11.2026
Verbindliche Ergänzung zu ARBEITSPAKETE.md. Alle Pakete sind offen; keine Implementierungsbehauptung. E01–E24 aus ENTERPRISE_KIS.md und TI01–TI15 aus TI_COMPLIANCE.md sind Traceability-IDs. Bestehende AP-Pakete bleiben erhalten; bei Überschneidung erweitern statt doppelt implementieren. Jedes große Paket vor Codebeginn in kleine, einzeln reviewbare Pakete mit Datei-/Migrationszuständigkeit und Tests zerlegen. Ein Paket, ein Branch. Kernverträge zuerst, unabhängige Fachpakete danach. Quellen, offene Entscheidungen, externe Abnahmen und Evidenz pro Paket dokumentieren.

| Paket | Priorität | Abhängigkeit | Lieferumfang und Mindestabnahme |
|---|---|---|---|
| EP-00 | P0 | – | Architekturentscheidungen zu Mandanten, Organisations-/Fallmodell, Datenverantwortung, Native/Adapter, Regelwerksversionen; Threat Model und synthetisches Verbund-Testmodell (2 Rechtsträger, 4 Kliniken, mehrere MVZ/Filialen, gemeinsame Dienste). Kein Produktivzugang. |
| EP-01 | P0 | EP-00 | Mandanten-/Organisationskontext, Standort-/Institutionskennungen, Hierarchie und Scoped Config; Tests gegen IDOR, unzulässige Parent-Vererbung, Servicekonten und standortfremde Suche. |
| EP-02 | P0 | EP-01 | MPI, lokale Identitäten, Merge/Unmerge, temporäre Identität, Kontakte/AP-12; Dubletten, falsche Zuordnung, konkurrierende Zusammenführung und Rechte testen. |
| EP-03 | P0 | EP-01/02 | Episode/Encounter/klinischer und Abrechnungsfall, ambulante Kontakte, zulässige parallele Fälle, Standortwechsel/Rechtsträgerwechsel, Patientenservice/AP-13; Tests ohne Bett, Transfer, Storno, Parallelfälle. |
| EP-04 | P0 | EP-01 | IAM/SSO, rollen- und zweckgebundene Objektberechtigung, Break-glass, Audit auch Lesen/Export, externe Prüfanker, Retention/Legal Hold, AP-02/03/04; Negativtests über alle Datenwege. |
| EP-05 | P0 | EP-01/03/04 | Sichere Ereignis-/Inbox-/Outbox-Verträge, Idempotenz, Quarantäne, Reconciliation, sicherheitskritische Invarianten; Crash/Retry/Duplikat/Out-of-order und tenantübergreifende Sperren testen. |
| EP-06 | P0 | EP-00 | Normen-/Terminologie-/Lizenzregister, ICD/OPS/AP-14, UCUM, SNOMED/LOINC, PZN etc.; versionsgebundene Imports, validierte Mappings und Ablauf alter Kataloge. |
| EP-07 | P0 | EP-01/04/05 | HA/Backup/DR, Monitoring/AP-05/06, Downtime, SBOM/Release, Schlüsselverwaltung, Lasttest Verbund; dokumentierter Restore und WAN-/Failover-Test mit definierten RTO/RPO. |
| EP-08 | P1 | EP-01–06 | FHIR/ISiK Basis, weitere Module nach TI10, HL7 ADT/AP-30, ORU/AP-31, ORM/OML/MDM/SIU, DICOM/DICOMweb, IHE und xDT jeweils Gegenstellenprofil; Validator und ACK/Retry/Access-Tests. |
| EP-09 | P1 | EP-01–06 | TI01–03 Zugang/Identitäten/VSDM, TI04 ePA, TI05 eRezept, TI06 KIM/eArztbrief, TI07 eAU, TI08 Messenger, TI09 NFDM/eMP, TI11–15 weitere Anforderungen; je Dienst separates Unterpaket mit offizieller Testumgebung und erforderlichen externen Nachweisen. |
| EP-10 | P1 | EP-03/06/08/09 | MVZ/PVS E02: Quartal, Kontakt, Versicherungsdaten, Überweisung, Formulare, Sprechstunde, Recall, KV/EBM, Privat, DMP; vollständiger synthetischer Quartalsabschluss inkl. Korrektur, offizielle Prüfmodule und erforderliche Zertifizierung. |
| EP-11 | P1 | EP-03/06/08 | Stationäre und sektorübergreifende Abrechnung E20: §301/§21, Grouper DRG/PEPP, AOP/ASV/Hybrid-DRG, MD und Kostenträger; versionierte Regeln, plausibilisierte Testfälle, Storno/Quittung, Abrechnungsfachabnahme. |
| EP-12 | P1 | EP-03/04/06 | Arzt-/Pflegearbeitsplatz E01/E03/E04, AP-11/17/40/41, longitudinaler Verlauf, strukturierte Dokumente, Brief, Kurve, Pflegeplanung; Browser-E2E von Aufnahme bis Entlassung/ambulanter Kontrolle, Freigaben und Korrekturen. |
| EP-13 | P1 | EP-03/06/08 | E05 AMTS/eMAR/BCMA/Apotheke; Medikationsabgleich, Katalog, Dosis/Interaktionen nur validiert, Gabe/Verweigerung, Unit Dose, BTM, Recall; falsch-Patient/falsch-Dosis und Ausfall testen; klinische/medizinprodukterechtliche Abnahme. |
| EP-14 | P1 | EP-03/05/06/08 | E06 Labor/LIS/POCT, Probenlogistik, kritische Werte; separate Blutbank/Transfusion mit Fachreview, Barcode, Korrektur, Quittierung und Eskalation; keine kritischen Ergebnisse still verlieren. |
| EP-15 | P1 | EP-03/08 | E07/E08 Radiologie/PACS, Kardiologie, Lungenfunktion, andere Funktionsdiagnostik; Befund-/Signal-/Bildreferenzen und Viewer, Provenienz, Arbeitsliste, AP-15; Patient-/Auftragszuordnung und korrigierte Befunde testen. |
| EP-16 | P1 | EP-03/05/08 | E09 Notaufnahme/Rettungsdienst, E10 OP/AIMS/PDMS/ICU, AP-18/19; je Bereich gesonderte Pakete, validierte Scores, Gerätedaten, Zeitreihen, OP-Sicherheit und klinischer Downtime-Test. |
| EP-17 | P1 | EP-03/04/08 | E11 Aufklärung/Einwilligung, E12 Therapie/Geriatrie/Palliativ/Entlassmanagement, E13 Onkologie/Fachregister; signierte Versionen, Vertreter/Widerruf, Überleitung und Registerquittung; Fach- und Lizenzprüfung. |
| EP-18 | P2 | EP-03/05 | E14/E16 Transport, Bettenaufbereitung, Küche/AP-16, Technik, Facility, Dosimetrie; minimale Datenfreigabe, Ressourcen-/Prioritätskonflikte, sichere Sperrbettlogik. |
| EP-19 | P2 | EP-01/05/06 | E15 Einkauf, Material, ERP, AEMP, Inventar, Charge, Rechnung; Traceability bis Patient/OP, Recall und getrennte Finanzrechte. |
| EP-20 | P2 | EP-01/04 | E17 Dienstplan/AP-10/42, HR, Zeitwirtschaft, Weiterbildung, Qualifikation und Payroll-Adapter; Standortwechsel, Schichtbesetzung und tariflich geprüfte Regeln. |
| EP-21 | P2 | EP-01/04/05 | E18 QM/CIRS/Intranet/DMS/Collaboration, Nextcloud/SSO-Adapter, SOPs, Meldungen, Projektmanagement; Freigabe-/Versionierung, getrennte CIRS-Identität, ACL-Synchronisation ohne Rechteausweitung. |
| EP-22 | P2 | EP-03/04/08/09 | E21 Patientenportal, Termine/Recall, Vertreter, sichere Nachrichten, PROM/PREM, Nachsorge; Identitätsprüfung, Widerruf, keine Fremdakten. |
| EP-23 | P2 | EP-04/06/12 | E19 Wissens-/KI-Assistent optional, E23 BI/Forschung; Quellen, Version, getrennte Zwecke, menschliche Freigabe, Prompt-Injection-/Datenabfluss-Tests, keine autonome klinische Entscheidung. |
| EP-24 | P1 | EP-01–08 | E22 externe Meldewege (TI14), versionierte Formate, Meldepflicht/Frist, fachliche und technische Quittung, Korrektur; je Register eigener Adapter und offizielle Nachweise. |
| EP-25 | P1 | EP-01–08 | E24 Migrations-/Koexistenzstrategie: nur autorisierte Exporte, synthetische Proben, Mapping, Reconciliation, Cutover/Rollback und Altarchiv, pro Fachsystem eigener Plan; keine Produktivmigration im aktuellen Auftrag. |
| EP-26 | P0/P1 | alle betroffenen | Freigabeprogramm: unabhängige Security-Reviews, klinische Usability, DPIA, Medizinprodukt-/AI-Act-Einordnung, Penetrationstest, Betreiber- und externe Nachweise; pro Release verpflichtendes Gate, keine Selbstzertifizierung. |

P0 = Architektur-/Sicherheitsblocker, P1 = Kernversorgung und regulatorische Anforderungen, P2 = organisatorischer Ausbau (abhängig vom Einsatzprofil kann P2 vor Produktivbetrieb verpflichtend sein). Bestehende AP-01/02/05/06 können parallel laufen, soweit sie die neuen Kernverträge nicht vorwegnehmen. Nicht alle P1-Pakete müssen auf die vollständige Umsetzung aller TI-Dienste warten; Abhängigkeiten je Unterpaket konkretisieren.

## Durchgängige Abnahmeszenarien
1. Patient kommt im MVZ an, wird überweisen, erhält Klinikdiagnostik, wird stationär aufgenommen, bei Bedarf verlegt, entlassen und im MVZ nachbetreut; separate Fälle, korrekte Kennungen, zulässiger Datenaustausch, kein Bett im MVZ.
2. Rettungsdienst → Notaufnahme → OP/ICU → Normalstation → Reha/ambulante Nachsorge; nachvollziehbare Aufträge, Medikation und kritische Ergebnisse, sichere Übergaben.
3. Standort A fordert Untersuchung in Standort B an; Ergebnis wird korrigiert, externe Verbindung fällt aus, erneute Zustellung erzeugt kein doppeltes Ergebnis; fremder Rechtsträger erhält keinen pauschalen Zugriff.
4. Versicherungs-/Abrechnungsfälle GKV/PKV, stationär/ambulant, AOP/ASV/Hybrid-DRG nach jeweils anwendbarer Regel; technische und fachliche Rückmeldungen, Storno und erneute Übermittlung.
5. Ausfall eines TI-Dienstes, eines Fachsystems oder eines Standortnetzes; dokumentiertes Ersatzverfahren, keine falsche Erfolgsmeldung, Wiederanlauf und Abgleich.
6. Zugriff ohne Behandlungsbezug, fehlerhaftes FHIR-Profil, widerrufenes Konto, falsche institutionelle Identität, Export über Mandantengrenze, falsche Barcode-Zuordnung: sicher ablehnen und angemessen auditieren.
7. Installation/Upgrade kleiner Praxis und großer Verbund, Abschaltung optionaler Module, Backup/Restore und vollständiger autorisierter Datenexport; keine Datenverluste und keine unbemerkte Abschaltung klinischer Schutzfunktionen.

## Definition of Done
Bestehende scripts/check.sh und Pakettests unverändert verpflichtend für Implementierungen; zusätzlich fachliche Akzeptanztests, Rechte-/Mandantentests, Ausfall- und Konkurrenztests, dokumentierte Norm-/Katalogversionen, Migration/Export, unabhängiges Review und ggf. externe Nachweise. Dokumentationsänderungen getrennt kennzeichnen, keine nicht ausgeführten Tests als bestanden ausgeben. Status in FUNKTIONSLANDKARTE.md und Nachweisregister fortschreiben; keine produktive Freigabe aus einem einzelnen grünen Test ableiten.
