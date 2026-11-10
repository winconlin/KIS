# KIS — Entwicklung

## Struktur

| Pfad | Inhalt |
|---|---|
| `crates/kis-core` | Kern: Anmeldung, Rechte, Behandlungszusammenhang, Ereignisse, Audit, Datenschutz |
| `crates/kis-app` | Server `kis`, Werkzeug `kis-admin`, Zusammenbau, Integrationstests |
| `crates/kis-org`, `kis-patients`, `kis-cases` | Kernmodule |
| `crates/kis-<name>` | Fachmodule |
| `migrations/` | Schema; `CHECKSUMS` schützt aufgenommene Migrationen |
| `config/kis.toml` | Infrastruktur-Konfiguration |
| `config/module_tables.toml` | Welche Tabelle gehört welchem Modul |
| `static/` | Weboberfläche (ohne Build-Schritt) |
| `scripts/` | Prüfungen, Modul-Gerüst, Backup |
| `entwurf/` | Ältere Schnittstellen-Entwürfe, nicht im Build |

## Voraussetzungen

Rust (stabil, CI: 1.99.1), PostgreSQL 16 mit den Erweiterungen `pgcrypto`
und `btree_gist`, Python 3.11+, `psql`.

## Prüfen

```sh
KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh
```

Die Variable zeigt auf einen PostgreSQL-Server, auf dem der Benutzer
Datenbanken anlegen darf. Jeder Test legt eine eigene Wegwerf-Datenbank an
und entfernt sie wieder. Ohne Datenbank bricht das Skript ab; übersprungene
Tests gelten nicht als bestanden.

| Schritt | Einzeln |
|---|---|
| Format | `cargo fmt --all -- --check` |
| Leitplanken | `python3 scripts/check_guardrails.py` |
| Modulgrenzen | `python3 scripts/check_modules.py` |
| Dokumentation | `python3 scripts/check_docs.py` |
| Clippy | `cargo clippy --locked --workspace --all-targets -- -D warnings` |
| Tests | `KIS_REQUIRE_PG_TESTS=1 cargo test --locked --workspace` |
| SQL gegen Schema | `python3 scripts/check_sql.py --server-url "$KIS_TEST_DATABASE_URL"` |

Zusätzlich in der CI: Abhängigkeits-Audit (`cargo audit`), Backup- und
Restore-Tests (`python3 -m unittest discover -s tests -p 'test_*.py'`),
Container-Bau mit Rauchtest (`scripts/check_container.py`).

## Tests

| Datei | prüft |
|---|---|
| `crates/kis-app/tests/api_pg.rs` | Anmeldung, Sitzungen, Kontosperre, Rechte, Datenschutz-Funktionen, Audit-Kette |
| `crates/kis-app/tests/hospital.rs` | Weg eines Patienten über alle Berufsgruppen; Ausfall einer Folgeaktion; abgeschaltete Module; Rollen und Richtlinien zur Laufzeit; Löschung |
| `crates/kis-app/tests/roster.rs` | Dienstplan, Prüfung, Export |
| `crates/kis-app/tests/api_doc.rs` | `docs/API.md` entspricht den registrierten Routen |
| Unit-Tests in den Crates | Rechte, Zugriffsregeln, Einstellungen, Anfragebegrenzung, Exporte |

Das Testgerüst steht in `crates/kis-app/tests/common/mod.rs`: echter
Router, echte Datenbank, Benutzer je Rolle, Aufrufe mit erwarteten
Statuscodes. Alle Testdaten sind synthetisch.

## Arbeitsweise

- Neue Funktion: [MODULE.md](MODULE.md). Offene Arbeit:
  [ARBEITSPAKETE.md](ARBEITSPAKETE.md).
- Die Routentabelle in `docs/API.md` wird erzeugt:
  `KIS_UPDATE_API_DOC=1 cargo test -p kis-app --test api_doc`.
- Nach einer neuen Migration:
  `python3 scripts/check_guardrails.py --add-migration`.
- `kis-admin modules` und `GET /api/v1/routes` zeigen, was eine Instanz
  enthält.
