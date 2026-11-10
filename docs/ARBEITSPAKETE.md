# Arbeitspakete

Offene Arbeit, so geschnitten, dass ein Agent (Hermes, Qwen, Claude) oder
ein Mensch jedes Paket für sich erledigen kann. Übersicht des Gesamtstands:
[FUNKTIONSLANDKARTE.md](FUNKTIONSLANDKARTE.md). Regeln für Module:
[MODULE.md](MODULE.md).

## So wird ein Paket bearbeitet

1. **Ein Paket, ein Branch** (`ap-07-icd-katalog`), ausgehend vom aktuellen
   `main`. Nicht mehrere Pakete mischen.
2. **Vorher lesen:** [AGENTS.md](../AGENTS.md), [MODULE.md](MODULE.md), die
   unter „Lies“ genannten Dateien des Pakets. Nicht das ganze Repository.
3. **Nur die genannten Bereiche ändern.** Braucht das Paket eine Änderung
   am Kern (`kis-core`, Migrationen 001–006, Ereignisse), dann anhalten und
   die Änderung als eigenen Vorschlag beschreiben.
4. **Fertig heißt:** Die Abnahmepunkte sind als Tests vorhanden und grün,
   und dieser Befehl läuft ohne Fehler durch:

   ```sh
   KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh
   ```

5. **Bericht** im Pull Request: was geändert wurde, Ausgabe von
   `scripts/check.sh` (letzte Zeilen), was bewusst nicht gemacht wurde.
   Nichts als erledigt melden, was nicht getestet ist.

**Nicht erfinden:** medizinische Skalen, Grenzwerte, Abrechnungsregeln und
amtliche Kataloge werden aus einer benannten Quelle übernommen (Quelle im
Code-Kommentar und im Bericht). Fehlt die Quelle oder braucht es eine
Entscheidung des Hauses, steht das Paket auf „braucht Entscheidung“ — dann
die Frage stellen statt raten.

**Größe:** S = wenige Stunden, M = ein bis zwei Tage, L = mehrere Tage und
vorher in Teilpakete zerlegen.

---

## Aufräumen und Härten

### AP-01 · Zeitplan-Route ins Terminmodul verlagern · S
- **Ziel:** `GET /api/v1/doctors/:id/schedule` liest Termine, liegt aber im
  Modul `doctors`. Die Route gehört ins Modul `appointments` (Pfad und
  Antwort bleiben gleich).
- **Lies:** `crates/kis-doctors/src/lib.rs` (`doctor_schedule`),
  `crates/kis-appointments/src/lib.rs`, `config/module_tables.toml`.
- **Abnahme:** Die Ausnahme `reads = ["appointments"]` bei `doctors` ist
  entfernt und `scripts/check_modules.py` bleibt grün; ein Test ruft die
  Route auf und prüft Termine im Zeitraum.

### AP-02 · Anmeldetoken nicht mehr im `localStorage` · M
- **Ziel:** Die Oberfläche hält Access- und Refresh-Token im
  `localStorage`; ein eingeschleustes Skript könnte sie lesen. Refresh-Token
  in ein `HttpOnly; Secure; SameSite=Strict`-Cookie, Access-Token nur im
  Speicher der Seite.
- **Lies:** `crates/kis-app/src/core_routes.rs` (login, refresh, logout),
  `static/js/api.js`, `static/js/arbeitsplatz.js`, `docs/SECURITY.md`.
- **Abnahme:** Tests für Login/Refresh/Logout mit Cookie; Refresh ohne
  Cookie → 401; bestehende Tests in `api_pg.rs` bleiben grün (Token im
  Antwortkörper darf für Schnittstellenkonten bestehen bleiben).

### AP-03 · Auskunft als CSV und FHIR um Falldaten ergänzen · M
- **Ziel:** Der JSON-Export enthält alle Falldaten, CSV und FHIR nur
  Stammdaten, Termine und Dauermedikation.
- **Lies:** `crates/kis-core/src/data_portability.rs`.
- **Abnahme:** CSV enthält je Fall Zeilen für Aufenthalte, Diagnosen,
  Dokumentation, Labor, Vitalwerte, Verordnungen; FHIR-Bundle enthält
  `Encounter`, `Condition`, `Observation`, `MedicationRequest`; Tests
  vergleichen die Anzahl der Datensätze mit dem JSON-Export.

### AP-04 · Anonymisierung erfasst Falldaten · M
- **Ziel:** `anonymize_patient` ersetzt Stammdaten; Freitexte der Fälle
  (Dokumentation, Auftragsdetails, Hinweise) bleiben erhalten und können
  Namen enthalten.
- **Braucht Entscheidung:** löschen, schwärzen oder Fälle vom Patienten
  trennen? Bis dahin nur den Ist-Zustand in `docs/DSGVO/` beschreiben.

### AP-05 · Kennzahlen für die Überwachung · S
- **Ziel:** `GET /api/v1/metrics` (Recht `config.read`) im
  Prometheus-Textformat: Anfragen je Statusklasse, Pool-Belegung, offene
  Folgeaktions-Fehler, ungeprüfte Notfallzugriffe. Ohne zusätzliche
  Abhängigkeit (Zähler als `AtomicU64`).
- **Lies:** `crates/kis-app/src/core_routes.rs`, `modules.rs`.
- **Abnahme:** Test prüft Format und dass Zähler nach Anfragen steigen;
  keine Personendaten in Labels.

### AP-06 · Lasttest der Kernabläufe · M
- **Ziel:** Skript `scripts/loadtest.py`, das ein Haus mit 1 000 Betten
  und 200 Benutzern synthetisch füllt und Stationsliste, Fallakte und
  Arbeitsliste unter gleichzeitiger Last misst (p50/p95, Speicher des
  Servers). Ergebnis nach `docs/SCALING.md`.
- **Abnahme:** Skript läuft gegen eine Wegwerf-Datenbank; Messwerte mit
  Rechnerangabe dokumentiert; auffällige Abfragen mit `EXPLAIN` belegt.

## Fachmodule erweitern

### AP-10 · Dienstplan: mehrere Personen je Dienst und Tag · M
- **Ziel:** Heute gilt je Dienstart und Tag eine Person
  (`UNIQUE (duty_type_id, duty_date)`). Schichten der Pflege brauchen
  mehrere Plätze.
- **Lies:** `migrations/011_dienstplan.sql`, `crates/kis-roster/src/plan.rs`,
  `crates/kis-app/tests/roster.rs`.
- **Schritte:** neue Migration: `duty_types.slots` (Standard 1),
  `duty_assignments.slot`, Eindeutigkeit auf `(duty_type_id, duty_date,
  slot)`; Prüfung „unbesetzt“ zählt fehlende Plätze.
- **Abnahme:** Test mit einer Dienstart mit 3 Plätzen: 2 besetzt → eine
  Meldung „unbesetzt“ mit Anzahl; Export enthält alle drei.

### AP-11 · Pflege-Assessments · M
- **Ziel:** Neues Modul `assessments`: Barthel-Index, Braden-Skala,
  Sturzrisiko als strukturierte Erhebungen je Fall mit serverseitig
  berechneter Summe und Verlauf.
- **Quelle nötig:** Items und Punktwerte aus der jeweils benannten
  Originalquelle; nicht aus dem Gedächtnis.
- **Abnahme:** Summe wird nicht vom Client übernommen; unvollständige
  Erhebung → 400; Verlauf je Skala; Rechte `nursing.read/write`.

### AP-12 · Kontaktpersonen und Einweiser · M
- **Ziel:** Neues Modul `contacts`: Angehörige, gesetzliche Betreuung,
  Hausarzt, Einweiser je Patient; Kennzeichen „darf Auskunft erhalten“.
- **Abnahme:** CRUD mit Rechten `patient.read/write`; erscheint in der
  Auskunft (AP-03 beachten); Löschung des Patienten entfernt die Kontakte.

### AP-13 · Kostenträger und Versicherungsverhältnis · M
- **Ziel:** Neues Modul `coverage`: Kostenträger (IK, Name, Art) und je
  Fall das Versicherungsverhältnis (Versichertennummer, Status, gültig
  von/bis).
- **Abnahme:** IK-Format geprüft; je Fall höchstens ein Hauptkostenträger
  zur selben Zeit (Constraint); Recht neu: `coverage.read/write`
  (Grundausstattung: `rezeption`, `verwaltung`).

### AP-14 · ICD- und OPS-Katalog · M
- **Ziel:** Amtliche Kataloge als Kern-Tabellen `icd_codes`, `ops_codes`
  mit Jahrgang; Import über `kis-admin import-catalog <datei>`; Suche
  `GET /api/v1/catalog/icd?q=`; Diagnosen und Prozeduren mit unbekanntem
  Kode werden abgelehnt, sobald ein Katalog des Jahres geladen ist.
- **Braucht Entscheidung:** Bezug der Katalogdateien (BfArM, Lizenz).
- **Abnahme:** Import ist wiederholbar (gleiches Ergebnis); Test mit einer
  kleinen synthetischen Katalogdatei.

### AP-15 · Untersuchungstermine · M
- **Ziel:** Aufträge an Funktionsbereiche terminieren: Ressourcen
  (Gerät/Raum) je Einheit, Termin je Auftrag, keine Überschneidung je
  Ressource (Exclusion-Constraint wie bei `appointments`).
- **Lies:** `crates/kis-orders/src/lib.rs`, `migrations/001_init.sql`
  (Terminüberschneidung).
- **Abnahme:** paralleler Test: zwei Termine auf derselben Ressource →
  genau einer gelingt; Arbeitsliste nach Termin sortierbar.

### AP-16 · Speiseplan und Menüwahl · M
- **Ziel:** `meals` um Speiseplan (Tag, Mahlzeit, Gerichte je Kostform)
  und Menüwahl je Fall erweitern; die Produktionsliste zählt Gerichte.
- **Abnahme:** Wahl nur aus Gerichten der angeordneten Kostform; ohne
  Wahl gilt das Standardgericht; nüchterne Fälle zählen nicht.

### AP-17 · Arztbrief aus Falldaten · L
- **Ziel:** `POST /api/v1/cases/:case_id/letters/draft` erzeugt aus
  Diagnosen, Prozeduren, Verlauf, Labor und Entlassmedikation einen
  Entwurf der Dokumentationsart `arztbrief` (Text mit festen Abschnitten),
  der wie jede Dokumentation bearbeitet und freigegeben wird.
- **Zerlegen in:** (a) Zusammenstellung der Abschnitte, (b) Vorlage des
  Hauses als Einstellung, (c) Ausgabe als PDF.
- **Hinweis:** `clinical` darf Labor und Verordnungen nicht direkt lesen.
  Weg: Kernfunktion `kis_core::record::summary(case_id)` nach dem Muster
  von `CASES_EXPORT_SQL` — als Kern-Änderung vorab abstimmen.

### AP-18 · Notaufnahme: Ersteinschätzung · M
- **Ziel:** Neues Modul `emergency`: Ersteinschätzung mit Dringlichkeit
  und Zeitstempeln (Ankunft, Einschätzung, Arztkontakt), Liste der
  Notaufnahme nach Dringlichkeit und Wartezeit.
- **Quelle nötig:** das im Haus verwendete Einschätzungssystem.

### AP-19 · OP-Planung · L
- **Braucht Entscheidung:** Umfang (nur Plan oder auch OP-Dokumentation).
  Erst danach zerlegen.

## Schnittstellen

Schnittstellen sind eigene Programme oder Module, die die Ereignis-Outbox
lesen (`GET /api/v1/interfaces/events`) und über die normale API schreiben
(Konto mit Rolle `schnittstelle`). Die Entwürfe unter `entwurf/` sind
Anschauungsmaterial, kein Ausgangscode.

### AP-30 · HL7 v2 ADT ausgehend · L
- **Ziel:** Aus `case.admitted/transferred/discharged/cancelled` werden
  ADT-Nachrichten (A01, A02, A03, A11) für einen Kommunikationsserver.
- **Braucht Entscheidung:** Profil der Gegenstelle (Segmente, Zeichensatz).
- **Abnahme:** Nachrichten gegen aufgezeichnete Beispiele des Profils
  geprüft; kein Ereignis geht bei Neustart verloren oder doppelt hinaus
  (Cursor erst nach Quittung der Gegenstelle).

### AP-31 · Laborbefunde eingehend (HL7 ORU) · L
- **Ziel:** ORU-Nachrichten werden zu `POST …/lab/specimens`.
- **Braucht Entscheidung:** Profil des Laborsystems, Zuordnung
  Auftragsnummer ↔ Fall.

## Oberfläche

Die Oberfläche ist statisch (`static/`), ohne Build-Schritt und ohne
Inline-Skripte (Content-Security-Policy). Inhalte nur mit `textContent`.
Muster: `static/js/arbeitsplatz.js`. Jede Maske prüft im Browser nichts,
was der Server nicht auch prüft.

### AP-40 · Maske Aufnahme, Verlegung, Entlassung · M
- **Abnahme:** Browsertest (Playwright) legt einen Patienten an, nimmt ihn
  auf ein freies Bett auf, verlegt und entlässt ihn; Fehlermeldungen des
  Servers (belegtes Bett, gesperrtes Bett) werden angezeigt.

### AP-41 · Fallakte bearbeiten · M
- Diagnosen, Dokumentation (Entwurf, Freigabe, Korrektur), Verordnung mit
  Allergie-Rückfrage, Anforderung aus dem Katalog.

### AP-42 · Dienstplan · M
- Monatsraster je Einheit, Besetzen per Auswahl, Prüfmeldungen am Tag,
  Export-Schaltflächen.

### AP-43 · Administration · M
- Benutzer, Rollen mit Rechte-Matrix, Einstellungen, Notfallzugriffe
  prüfen, Folgeaktions-Fehler, Modulübersicht.

## Braucht eine Entscheidung des Hauses

Diese Bereiche sind groß und hängen von Vorgaben ab, die nicht aus dem
Code folgen. Vor Beginn klären:

| Bereich | Offene Frage |
|---|---|
| Abrechnung (DRG, PEPP, ambulant), § 301 | Eigenentwicklung oder Anbindung eines Abrechnungssystems? |
| Arzneimitteldatenbank, Wechselwirkungen | Welcher Datenlieferant? |
| Bildarchiv, Befundung | Anbindung eines vorhandenen PACS/RIS? |
| Telematikinfrastruktur | Konnektor, Fachdienste, Zulassung |
| Anmeldung über Verzeichnisdienst | LDAP, OIDC, Smartcard? |
| Medizinprodukt-Einordnung | Welche Funktionen unterstützen Entscheidungen? |
