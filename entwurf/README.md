# Entwürfe (nicht Teil des Builds)

Dieses Verzeichnis enthält ältere Entwürfe für Schnittstellen (HL7 v2,
FHIR, DICOM, XDS, GDT, CGM MEDICO, Telematikinfrastruktur).

**Status:** Materialsammlung. Die Crates gehören nicht zum Workspace,
werden nicht gebaut und nicht ausgeliefert. Sie passen weder zum aktuellen
Datenmodell (sie verweisen auf Tabellen und Spalten, die es nicht gibt)
noch zum Modulsystem (Routen ohne Zugriffsregel, direkter Zugriff auf
fremde Tabellen, eigenes Audit).

**Nicht als Vorlage für neuen Code verwenden.** Vorlage ist das
Mustermodul `crates/kis-nursing` und die Anleitung `docs/MODULE.md`.

Brauchbar sind einzelne fachliche Bausteine, z. B. der HL7-v2-Parser
(`kis-hl7`), das FHIR-Mapping (`kis-medico/src/mapping.rs`) oder der
GDT-Satzaufbau (`kis-gdt`). Wer eine Schnittstelle umsetzt, legt ein neues
Modul an, das Ereignisse aus der Outbox liest (`kis_core::events`), und
übernimmt daraus nur geprüfte Teile. Arbeitspakete: `docs/ARBEITSPAKETE.md`.
