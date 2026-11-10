# KIS — Krankenhaus-Informations-System

Ein modulares Krankenhaus-Informations-System in Rust auf PostgreSQL. Ziel:
Alle Beschäftigten eines Hauses — ärztlicher Dienst, Pflege,
Patientenservice, Funktionsdienste, Verwaltung, Küche, Hauswirtschaft,
Technik — erledigen ihre tägliche Arbeit in einem System, und alle Daten
greifen ineinander. Ausgelegt für kleine Kliniken bis Maximalversorger.

**Stand: in Entwicklung, nicht für den Echtbetrieb freigegeben.** Nur für
Test- und Entwicklungsinstallationen mit synthetischen Daten. Was vorhanden
ist und was fehlt, steht ohne Beschönigung in der
[Funktionslandkarte](docs/FUNKTIONSLANDKARTE.md).

## Grundsätze

- **Modular.** Ein kleiner Kern und je Fachgebiet ein Modul in einer
  eigenen Crate. Module kennen einander nicht; sie arbeiten über
  Ereignisse zusammen. Jedes Modul kann geändert, ersetzt oder
  abgeschaltet werden, ohne den Ablauf im Haus zu stören — fällt eine
  Folgeaktion aus, läuft der auslösende Vorgang weiter
  ([Module](docs/MODULE.md)).
- **Daten greifen ineinander.** Eine Allergie wird einmal erfasst und
  wirkt bei der Verordnung und in der Küche; eine Entlassung sperrt das
  Bett und beauftragt die Aufbereitung; ein Auftrag gibt der erbringenden
  Einheit für seine Dauer Zugriff auf den Fall.
- **Datenschutz ist eingebaut und einstellbar.** Jede Route verlangt ein
  Recht; Falldaten verlangen zusätzlich den Behandlungszusammenhang;
  Notfallzugriff ist möglich, begründungspflichtig und wird geprüft. Rollen,
  Rechte und Richtlinien sind Daten des Hauses und zur Laufzeit änderbar.
  Jede Änderung steht im Audit-Trail (Hash-Kette), ohne Personendaten.
- **Sparsam.** Ein Programm, eine Datenbank, keine weiteren Dienste. Nach
  dem Start rund 14 MB Arbeitsspeicher, im Leerlauf keine offene
  Datenbankverbindung; Programmdatei rund 14 MB
  ([Messwerte und ihre Grenzen](docs/SCALING.md)).

## Was läuft

| Bereich | Module |
|---|---|
| Aufbau des Hauses, Zimmer, Betten | `org` |
| Patientenstamm, Warnhinweise | `patients` |
| Aufnahme, Verlegung, Entlassung, Stationsliste, Bettenübersicht | `cases` |
| Diagnosen, Prozeduren, Dokumentation, Aufgaben, Übergabe | `clinical` |
| Leistungskatalog, Anforderungen, Arbeitslisten, Automatik | `orders` |
| Laborbefunde, Verlauf, kritische Werte | `lab` |
| Vitalwerte, Flüssigkeitsbilanz | `nursing` |
| Verordnung, Antiinfektiva-Kurse, Gabedokumentation | `prescriptions` |
| Kostformen, Produktionsliste der Küche | `meals` |
| Dienstplan, Prüfung, Wochenbesetzung, Export | `roster` |
| Termine, Behandler, Dauermedikation | `appointments`, `doctors`, `medications` |
| Weboberfläche | `ui` |
| Anmeldung, Rollen, Einstellungen, Audit, Auskunft, Löschung | Kern |

## Schnellstart (Entwicklung)

```bash
cp .env.example .env && chmod 600 .env     # Secrets eintragen (openssl rand …)
docker compose up -d db
docker compose run --rm --entrypoint kis-admin kis migrate
docker compose run --rm --entrypoint kis-admin kis create-user admin admin@example.test "Admin" admin
docker compose up -d kis
curl -fsS http://127.0.0.1:8080/health
```

Ohne Docker und alle Details: [Installation](docs/INSTALLATION.md).
Oberfläche: `http://127.0.0.1:8080/`, tägliche Arbeitsansicht unter
„Arbeitsplatz“.

## Prüfen

Ein Befehl prüft alles (Format, Leitplanken, Modulgrenzen, Dokumentation,
Clippy, Tests gegen echtes PostgreSQL, SQL gegen das Schema):

```bash
KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh
```

## Dokumentation

| Für | Dokument |
|---|---|
| Überblick über den Stand | [Funktionslandkarte](docs/FUNKTIONSLANDKARTE.md), [Prüfstatus](docs/REVIEW_STATUS.md) |
| Weiterentwicklung | [Module](docs/MODULE.md), [Arbeitspakete](docs/ARBEITSPAKETE.md), [Agentenregeln](AGENTS.md) |
| Technik | [Architektur](docs/ARCHITECTURE.md), [API](docs/API.md), [Datenbank](docs/DATABASE.md), [Sicherheit](docs/SECURITY.md) |
| Betrieb | [Installation](docs/INSTALLATION.md), [Konfiguration](docs/CONFIGURATION.md), [Betrieb](docs/OPERATION.md), [Skalierung](docs/SCALING.md) |
| Alles | [Dokumentationsübersicht](docs/README.md) |

## Lizenzangabe

`Cargo.toml` nennt MIT. Eine vollständige `LICENSE`-Datei mit den vom
Rechteinhaber festgelegten Angaben ist noch zu ergänzen.
