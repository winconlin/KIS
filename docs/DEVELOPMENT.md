# KIS — Entwicklung

Stand: 06.11.2026

Für Agenten gelten [AGENTS.md](../AGENTS.md), der [operative Auftrag](AGENTEN_AUFTRAG.md) und die [Sicherheits-Abnahme](SICHERHEITS_ABNAHME.md).

## Struktur

| Pfad | Rolle | Status |
|---|---|---|
| `crates/kis-app` | Server `kis`, Werkzeug `kis-admin`, Router, Auth-Middleware, TLS, Shutdown | aktiv |
| `crates/kis-core` | Konfiguration, DB-Pools/Migrationen, Security (Tokens, Audit, Krypto), Auth, Zugriffsregeln, DSGVO-Dienste | aktiv |
| `crates/kis-patients`, `kis-doctors`, `kis-appointments`, `kis-medications` | Fachmodule (CRUD) | aktiv |
| `crates/kis-ui` | Auslieferung von `static/` | aktiv |
| `crates/kis-hl7`, `kis-fhir`, `kis-dicom`, `kis-xds`, `kis-gdt`, `kis-medico`, `kis-ti` | Schnittstellen-Entwürfe | **nicht eingebunden**, Schema unvollständig |
| `migrations/` | Eingebettete PostgreSQL-Migrationen | aktiv |
| `config/postgres/` | Rollen- und Init-Skripte | aktiv |
| `scripts/` | Backup, SQL-Schema-Abgleich, Doku-Check, Container-Smoke-Test | aktiv |
| `static/` | Web-GUI | aktiv |

Abhängigkeiten zeigen nur von Fachmodulen und `kis-app` auf `kis-core`, nie umgekehrt.

## Sicherheitsarchitektur in Kürze

- Jede Anfrage durchläuft `auth_middleware` in `kis-app/src/api.rs`. Sie fragt [`access_policy::required_access`](../crates/kis-core/src/access_policy.rs) und lehnt unbekannte API-Routen für Nicht-Admins ab.
- Handler erhalten den geprüften Benutzer über den Extractor `RequireRole`. Dieser prüft kein Token selbst, sondern akzeptiert nur den von der Middleware hinterlegten `AuthUser`.
- Neue Routen: Eintrag in `access_policy.rs` **und** Test in dessen Testmodul. Schreibende Handler protokollieren mit `kis_core::security::append_audit` in derselben Transaktion.

## Prüfungen

PostgreSQL für Tests lokal starten, z. B. `docker run --rm -d -p 5432:5432 -e POSTGRES_HOST_AUTH_METHOD=trust postgres:16-alpine`.

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
export KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres
export KIS_REQUIRE_PG_TESTS=1     # fehlende DB = Fehler statt Überspringen
cargo test --locked --workspace
cargo build --locked --bins
python3 -m unittest discover -s tests -p 'test_*.py' -v      # Backup/Restore (age, pg_dump)
python3 scripts/check_docs.py
cargo audit                                                   # Ausnahmen: .cargo/audit.toml
```

SQL-Schema-Abgleich (alle statischen SQL-Strings werden gegen eine frisch migrierte Datenbank vorbereitet):

```bash
python3 scripts/check_sql.py --database-url postgres://…/kis \
  --crate kis-core --crate kis-app --crate kis-patients --crate kis-doctors \
  --crate kis-appointments --crate kis-medications --crate kis-ui
```

Ohne `--crate` werden auch die nicht eingebundenen Schnittstellen-Crates geprüft; dort sind derzeit Fehler bekannt.

Container: `docker build -t kis:ci . && python3 scripts/check_container.py --image kis:ci` (startet eine eigene PostgreSQL-Instanz und räumt danach auf).

## Integrationstests

[`crates/kis-app/tests/api_pg.rs`](../crates/kis-app/tests/api_pg.rs) legt je Test eine eigene Datenbank an, wendet die echten Migrationen an und prüft über den echten Router unter anderem: Token-Zwecke, Rotation inklusive paralleler Wiederverwendung, Logout-Widerruf, Kontosperre, sofortige Wirkung von Rollen-/Statusänderungen, Inaktivitäts-Logout, RBAC der Fachmodule, Optimistic Locking, Doppelbuchung bei parallelen Anfragen, Export/Hard-Delete/Anonymisierung/Pseudonymisierung, Audit-Serialisierung, Append-only und Manipulationserkennung.
