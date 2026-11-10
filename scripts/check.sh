#!/bin/sh
# Ein Befehl, der alles prüft. Ein Arbeitspaket ist erst fertig, wenn
# dieser Befehl ohne Fehler durchläuft.
#
#   KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh
#
# Die Variable zeigt auf einen PostgreSQL-Server (16+), auf dem der
# Benutzer Datenbanken anlegen darf. Jeder Test legt eine eigene
# Wegwerf-Datenbank an; bestehende Datenbanken werden nicht berührt.
# Ohne Datenbank bricht das Skript ab — übersprungene Tests gelten nicht
# als bestanden.
set -eu
cd "$(dirname "$0")/.."

: "${KIS_TEST_DATABASE_URL:?KIS_TEST_DATABASE_URL fehlt (PostgreSQL-Server für die Tests)}"
export KIS_REQUIRE_PG_TESTS=1

step() { printf '\n== %s ==\n' "$1"; }

step "1/8 Format"
cargo fmt --all -- --check

step "2/8 Leitplanken (Migrationen, unfertiger Code, CI)"
python3 scripts/check_guardrails.py

step "3/8 Modulgrenzen"
python3 scripts/check_modules.py

step "4/8 Dokumentation"
python3 scripts/check_docs.py

step "5/8 Audit-Ausnahmen (ausgenommene Crates werden nicht gebaut)"
python3 scripts/check_audit_exceptions.py

step "6/8 Clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings

step "7/8 Tests (Unit + PostgreSQL)"
cargo test --locked --workspace

step "8/8 SQL gegen das migrierte Schema"
python3 scripts/check_sql.py --server-url "$KIS_TEST_DATABASE_URL"

printf '\nAlle Prüfungen bestanden.\n'
