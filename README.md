# KIS — Krankenhaus-Informations-System

KIS ist ein Rust-Prototyp für ein Krankenhaus-Informations-System auf PostgreSQL. Langfristiges Ziel ist ein eigenständiges System, das eine Migration von CGM MEDICO ermöglicht.

**Stand: Entwicklungsprototyp, nur für Test- und Entwicklungsinstallationen mit synthetischen Daten.** Keine klinische Freigabe, keine Zertifizierung, kein unabhängiges Sicherheitsreview.

## Was tatsächlich läuft

Der Server `kis` (Crate `kis-app`) registriert:

- **Authentifizierung** mit persistenten Sitzungen in PostgreSQL: Login (Argon2id, Kontosperre, optional TOTP), Refresh mit atomarer Rotation, Logout/Widerruf, 30-Minuten-Inaktivitäts-Logout. Rolle und Kontostatus werden bei jeder Anfrage aus der Datenbank gelesen.
- **Zentrale Autorisierung** (deny-by-default) für alle Routen über eine serverseitige Route→Berechtigung-Tabelle ([`access_policy.rs`](crates/kis-core/src/access_policy.rs)).
- **Fachmodule** Patienten, Ärzte, Termine und Medikation (CRUD, Optimistic Locking, Soft-Delete). Doppelbuchungen verhindert eine Exclusion-Constraint in der Datenbank.
- **Audit-Trail** als Hash-Kette: jede Änderung wird in derselben Transaktion protokolliert, Schreiber sind per Advisory-Lock serialisiert, die Tabelle ist per Trigger und Rechtevergabe append-only. Personendaten stehen nicht im Audit.
- **DSGVO-Werkzeuge**: Export (JSON/CSV/FHIR-Bundle), Hard-Delete mit Namensbestätigung, Anonymisierung (zufällige Ersatzwerte) und Pseudonymisierung (AES-256-GCM).
- **Web-GUI** als statische Dateien unter `/` (strikte Content-Security-Policy).
- **TLS 1.3** direkt (rustls, optional mTLS) oder HTTP hinter einem TLS-Proxy.

Nicht eingebunden und nicht lauffähig sind die Schnittstellen-Crates `kis-hl7`, `kis-fhir`, `kis-dicom`, `kis-xds`, `kis-gdt`, `kis-medico` und `kis-ti`. Sie kompilieren, verweisen aber teils auf nicht existierende Tabellen und sind im Router nicht registriert (Details in [REVIEW_STATUS.md](docs/REVIEW_STATUS.md)).

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

## Prüfungen

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres KIS_REQUIRE_PG_TESTS=1 \
  cargo test --locked --workspace
python3 scripts/check_docs.py
```

Weitere Prüfungen (SQL-Schema-Abgleich, Backup/Restore, Container) stehen in [DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Dokumentation

- [Dokumentationsübersicht](docs/README.md)
- [Prüfstatus und offene Punkte](docs/REVIEW_STATUS.md)
- [Agentenregeln](AGENTS.md)

## Lizenzangabe

`Cargo.toml` nennt MIT. Eine vollständige `LICENSE`-Datei mit den vom Rechteinhaber festgelegten Angaben ist noch zu ergänzen; siehe Fahrplan 7.8.
