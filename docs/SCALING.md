# KIS — Ressourcenbedarf und Skalierung

## Messwerte

Gemessen am 10.11.2026 mit dem Release-Build auf einem Linux-Container mit
2 Kernen, PostgreSQL 16 auf demselben Rechner, synthetische Daten. Die
Werte sind eine Momentaufnahme dieses Rechners, kein Lasttest.

| Größe | Wert |
|---|---|
| Programmdatei `kis` | 14,4 MB |
| Programmdatei `kis-admin` | 9,0 MB |
| Arbeitsspeicher nach dem Start | 14 MB |
| Arbeitsspeicher nach einer Anmeldung und einigen Anfragen | 34 MB |
| Arbeitsspeicher nach dem Anlegen von Demodaten (11 Passwort-Hashes, rund 100 Anfragen) | 55 MB (Höchstwert) |
| Datenbankverbindungen während der Arbeit | 2 |
| Datenbankverbindungen im Leerlauf | 0 (gemessen 40 s nach der letzten Anfrage bei `idle_timeout_seconds = 5`) |
| Registrierte Routen, aktive Module | 163, 14 |

Der Sprung beim Arbeitsspeicher kommt von der Passwortprüfung: Argon2id
belegt je Anmeldung bewusst rund 19 MB. Gleichzeitige Anmeldungen sind
durch die Anfragebegrenzung für Anmelde-Routen gedeckelt.

Ein Lasttest mit realistischen Datenmengen steht aus
([ARBEITSPAKETE.md](ARBEITSPAKETE.md), AP-06). Aussagen zu Antwortzeiten
unter Last wären ohne ihn geraten.

## Warum der Bedarf klein ist

- Ein Prozess, keine weiteren Dienste. Ereignisse laufen über eine
  Tabelle, nicht über einen Broker.
- Verbindungen zur Datenbank entstehen erst bei Bedarf und werden im
  Leerlauf geschlossen (`min_connections = 0`).
- Je Anfrage eine Abfrage für Sitzung, Kontostatus, Rollen und
  Versionszähler; Rollenmatrix und Richtlinien liegen im Speicher.
- Kein eigener Daten-Cache: PostgreSQL hält die häufig gelesenen Seiten
  ohnehin im Speicher, und ein zweiter Cache wäre eine Quelle veralteter
  Daten.
- Listen werden in einer Abfrage zusammengestellt (Stationsliste,
  Übergabe, Produktionsliste), nicht in einer Abfrage je Zeile.
- Nicht benötigte Module lassen sich abschalten.

## Kleine Klinik

Ein Rechner mit 2 Kernen und 2 GB Arbeitsspeicher für Server und
Datenbank genügt für den Betrieb des Programms selbst. Der
Behandlungszusammenhang lässt sich abschalten, wenn jede Person jeden
Patienten betreut (`security.treatment_context.enforce`).

## Großes Haus

- **Mehrere Instanzen** hinter einem Lastverteiler: Der Server hält
  keinen Zustand, der nicht in der Datenbank steht. Sitzungen, Rollen und
  Einstellungen gelten sofort auf allen Instanzen. Die Anfragebegrenzung
  zählt je Instanz.
- **Lese-Replica** für Auswertungen über `database.read_url`.
  Zugriffsentscheidungen und Schreibvorgänge nutzen immer den
  maßgeblichen Stand.
- **Verbund mit mehreren Einrichtungen:** Der Organisationsbaum bildet
  Träger, Einrichtungen und ihre Abteilungen ab; Zuordnung und Zugriff
  gelten jeweils für eine Einheit und alles darunter.
- **Geordnetes Beenden:** Auf SIGTERM nimmt der Server keine neuen
  Verbindungen an und lässt laufende Anfragen bis
  `shutdown_timeout_seconds` zu Ende laufen. Damit sind Aktualisierungen
  ohne abgebrochene Anfragen möglich.

Noch nicht vorhanden und für sehr große Häuser nötig: Partitionierung der
wachsenden Tabellen (Audit, Ereignisse, Vitalwerte), Archivierung
abgeschlossener Fälle, Kennzahlen für die Überwachung (AP-05).
