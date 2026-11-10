#!/usr/bin/env python3
"""Prüft statische SQL-Strings im Rust-Code gegen das migrierte PostgreSQL-Schema.

Jede gefundene Abfrage wird mit ``PREPARE`` (ohne Ausführung) gegen eine
Datenbank vorbereitet, auf die alle Migrationen angewendet wurden. Gemeldet
werden fehlende Tabellen/Spalten und Syntaxfehler. SQL-Bausteine aus
Konstanten (``const NAME: &str``, eingesetzt per ``format!("… {NAME} …")``)
werden aufgelöst; zur Laufzeit zusammengesetztes SQL wird übersprungen.

Verwendung:
    python3 scripts/check_sql.py --server-url postgres://postgres@127.0.0.1/postgres
    python3 scripts/check_sql.py --database-url postgres://.../kis [--crate kis-core ...]

Die Datenbank muss leer und frisch migriert sein (z. B. per ``kis-admin`` oder
den Integrationstests). Es werden keine Daten verändert: jede Prüfung läuft in
einer Transaktion, die zurückgerollt wird.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SQL_START = re.compile(r"^\s*(SELECT|INSERT|UPDATE|DELETE|WITH)\b", re.IGNORECASE | re.DOTALL)
RAW_STR = re.compile(r'r#"(.*?)"#', re.DOTALL)
PLAIN_STR = re.compile(r'"((?:[^"\\]|\\.)*)"', re.DOTALL)
# Fehlerklassen, die nur fehlende Typinformation für Parameter betreffen
IGNORABLE = (
    "could not determine data type of parameter",
    "inconsistent types deduced for parameter",
    "operator is not unique",
    "could not determine polymorphic type",
)


def rust_unescape(s: str) -> str:
    # Zeilenfortsetzung "\\\n   " entfernen, einfache Escapes auflösen
    s = re.sub(r"\\\n\s*", "", s)
    return s.replace('\\"', '"').replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\")


CONST = re.compile(
    r"""const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"((?:[^"\\]|\\.)*)"\s*;""", re.DOTALL
)
PLACEHOLDER = re.compile(r"\{([A-Z][A-Z0-9_]*)\}")


def crate_constants(crate: pathlib.Path) -> dict[str, str]:
    """SQL-Bausteine (`const NAME: &str = "…";`) einer Crate, ineinander aufgelöst."""
    consts: dict[str, str] = {}
    for path in sorted(crate.glob("src/**/*.rs")):
        for m in CONST.finditer(path.read_text(encoding="utf-8")):
            consts[m.group(1)] = rust_unescape(m.group(2))
    for _ in range(5):
        for name, value in consts.items():
            consts[name] = PLACEHOLDER.sub(lambda m: consts.get(m.group(1), m.group(0)), value)
    return consts


def resolve(sql: str, consts: dict[str, str]) -> str | None:
    """Setzt `{NAME}`-Bausteine ein. `None`, wenn danach noch Platzhalter
    offen sind (zur Laufzeit zusammengesetztes SQL)."""
    sql = PLACEHOLDER.sub(lambda m: consts.get(m.group(1), m.group(0)), sql)
    sql = sql.replace("{{", "\x00").replace("}}", "\x01")
    if "{" in sql or "}" in sql:
        return None
    return sql.replace("\x00", "{").replace("\x01", "}")


def extract(path: pathlib.Path, consts: dict[str, str]):
    text = path.read_text(encoding="utf-8")
    # Testmodule auslassen
    cut = text.find("#[cfg(test)]\nmod tests")
    if cut != -1:
        text = text[:cut]
    # Bausteine sind keine vollständigen Anweisungen
    text = CONST.sub(lambda m: "\n" * m.group(0).count("\n"), text)
    found = []
    for m in RAW_STR.finditer(text):
        found.append((text.count("\n", 0, m.start()) + 1, m.group(1)))
    for m in PLAIN_STR.finditer(text):
        found.append((text.count("\n", 0, m.start()) + 1, rust_unescape(m.group(1))))
    for line, sql in found:
        sql = resolve(sql, consts)
        if sql and SQL_START.match(sql) and len(sql) > 15:
            yield line, sql.strip()


def check(url: str, sql: str) -> str | None:
    stmt = f"BEGIN; PREPARE kis_check AS {sql}; ROLLBACK;"
    proc = subprocess.run(
        ["psql", url, "-X", "-q", "-v", "ON_ERROR_STOP=1", "-c", stmt],
        capture_output=True,
        text=True,
    )
    if proc.returncode == 0:
        return None
    err = proc.stderr.strip().splitlines()
    msg = next((l for l in err if "ERROR" in l), err[0] if err else "unbekannter Fehler")
    if any(i in msg for i in IGNORABLE):
        return None
    return msg


def scratch_database(server_url: str) -> tuple[str, str]:
    """Legt eine frische Datenbank an und wendet alle Migrationen an."""
    name = f"kis_sqlcheck_{os.getpid()}"
    run = lambda url, *args: subprocess.run(  # noqa: E731
        ["psql", url, "-X", "-q", "-v", "ON_ERROR_STOP=1", *args], check=True, capture_output=True, text=True
    )
    run(server_url, "-c", f"CREATE DATABASE {name}")
    base, _, query = server_url.partition("?")
    url = base[: base.rfind("/") + 1] + name + (f"?{query}" if query else "")
    for migration in sorted((ROOT / "migrations").glob("*.sql")):
        run(url, "-f", str(migration))
    return name, url


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    group = ap.add_mutually_exclusive_group(required=True)
    group.add_argument("--database-url", help="frisch migrierte Datenbank")
    group.add_argument(
        "--server-url",
        help="Server, auf dem eine Wegwerf-Datenbank angelegt, migriert und wieder entfernt wird",
    )
    ap.add_argument("--crate", action="append", help="nur diese Crates (Standard: alle)")
    args = ap.parse_args()

    scratch = None
    url = args.database_url
    if args.server_url:
        scratch, url = scratch_database(args.server_url)
    try:
        crates = sorted(p for p in (ROOT / "crates").iterdir() if p.is_dir())
        if args.crate:
            crates = [c for c in crates if c.name in args.crate]
        failures = 0
        total = 0
        for crate in crates:
            consts = crate_constants(crate)
            for path in sorted(crate.rglob("src/**/*.rs")):
                for line, sql in extract(path, consts):
                    total += 1
                    err = check(url, sql)
                    if err:
                        failures += 1
                        print(f"{path.relative_to(ROOT)}:{line}: {err}")
    finally:
        if scratch:
            subprocess.run(
                ["psql", args.server_url, "-X", "-q", "-c", f"DROP DATABASE IF EXISTS {scratch} WITH (FORCE)"],
                check=False,
                capture_output=True,
            )
    print(f"{total} SQL-Anweisungen geprüft, {failures} Fehler", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
