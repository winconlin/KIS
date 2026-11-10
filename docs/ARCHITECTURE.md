# KIS — Architektur

Ein Programm (`kis`), eine PostgreSQL-Datenbank. Kein Nachrichtenbroker,
kein Cache-Dienst, kein Build-Schritt für die Oberfläche.

## Aufbau

```text
                       ┌────────────────────────────────────────────┐
  Browser / Geräte ──▶ │ kis-app   Zusammenbau, Middleware, Admin   │
  Schnittstellen       ├────────────────────────────────────────────┤
                       │ Fachmodule (je eine Crate, abschaltbar)    │
                       │  clinical  orders  lab  nursing            │
                       │  prescriptions  meals  roster              │
                       │  appointments  doctors  medications  ui    │
                       ├────────────────────────────────────────────┤
                       │ Kernmodule   org   patients   cases        │
                       ├────────────────────────────────────────────┤
                       │ kis-core  Anmeldung, Rechte, Behandlungs-  │
                       │  zusammenhang, Ereignisse, Audit, Betten,  │
                       │  Einstellungen, Datenschutz-Funktionen     │
                       └──────────────────────┬─────────────────────┘
                                              ▼
                                         PostgreSQL
```

Abhängigkeiten zeigen nur nach unten. Module kennen einander nicht; was
sie voneinander brauchen, läuft über den Kern (Regeln und Prüfungen:
[MODULE.md](MODULE.md)).

## Weg einer Anfrage

Jede Anfrage durchläuft dieselben Schichten
([`modules.rs`](../crates/kis-app/src/modules.rs),
[`middleware.rs`](../crates/kis-app/src/middleware.rs)):

1. Protokoll, Schutz vor Abstürzen (ein Fehler in einem Handler betrifft
   nur diese Anfrage), Sicherheits-Header, Zeit- und Größenlimit.
2. Anfragebegrenzung je Client.
3. **Zugriffskontrolle:** Was verlangt die Route? (Tabelle aus den
   Registrierungen aller aktiven Module; ohne Eintrag: nur Admin.) Dann
   Token, Sitzung und Kontostatus prüfen — eine Datenbankabfrage, die auch
   die wirksamen Rollen liefert. Dann das Recht prüfen. Enthält der Pfad
   `:case_id` oder `:unit_id`, zusätzlich Behandlungszusammenhang bzw.
   Zuordnung zur Einheit.
4. Handler des Moduls: eine Transaktion für fachliche Änderung,
   Audit-Eintrag und Ereignis.

## Wer darf was sehen

| Frage | Antwort durch |
|---|---|
| Welche Art von Daten darf jemand bearbeiten? | Rechte aus seinen Rollen (Daten des Hauses) |
| Für welche Patienten? | Behandlungszusammenhang: Fall liegt in einer Einheit des Benutzers, seine Einheit behandelt mit, Notfallzugriff, oder Recht `case.access_all` |
| Für welche Arbeitslisten? | Zuordnung zur Einheit oder einer übergeordneten |

Rollenmatrix und Richtlinien hält jede Instanz im Speicher. Die
Sitzungsprüfung liefert Versionszähler mit; nur wenn sich einer geändert
hat, wird neu geladen. Änderungen wirken dadurch sofort und kosten im
Normalbetrieb keine zusätzliche Abfrage.

## Ereignisse

Ein Modul meldet, was fachlich geschehen ist. `EventBus::publish` schreibt
das Ereignis in die Tabelle `domain_events` und ruft die angemeldeten
Reaktionen anderer Module auf — alles in der Transaktion des Auslösers:

- Gelingt die Reaktion, gilt sie zusammen mit dem Auslöser oder gar nicht.
- Scheitert sie, wird nur sie zurückgerollt (Savepoint). Der Auslöser läuft
  weiter; der Fehler steht in `event_failures` und ist für die
  Administration sichtbar.
- Externe Systeme lesen dieselben Ereignisse über einen Lesezeiger
  (Outbox). Es geht nichts verloren, auch wenn die Gegenstelle ausfällt.

Beispiel: Die Entlassung (Modul `cases`) meldet `case.discharged`. Das
Auftragswesen legt nach den Regeln des Hauses einen Auftrag zur
Bettenaufbereitung an und sperrt das Bett. Die Küche braucht kein
Ereignis: Ihre Produktionsliste wird aus der aktuellen Belegung berechnet.

## Datenmodell in einem Satz je Baustein

- **Organisation:** ein Baum aus Einheiten (Einrichtung, Fachabteilung,
  Station, Funktionsbereich, Verwaltung, Versorgung) — bildet die
  Einzelklinik wie den Verbund ab.
- **Fall:** Klammer um alles, was an einem Patienten geschieht; lückenlose
  Folge von Aufenthaltsabschnitten.
- **Auftrag:** eine Leistung, die ein Bereich bei einem anderen anfordert;
  ein Mechanismus für Diagnostik, Konsil, Therapie, Transport, Reinigung,
  Technik.
- Alles Weitere hängt am Fall oder an der Einheit.

Einzelheiten: [DATABASE.md](DATABASE.md).

## Nicht Teil des Systems

Unter `entwurf/` liegen ältere Entwürfe für Schnittstellen (HL7, FHIR,
DICOM, XDS, GDT, MEDICO, Telematikinfrastruktur). Sie sind nicht Teil des
Builds und nicht betriebsbereit.
