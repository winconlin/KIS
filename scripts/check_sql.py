#!/usr/bin/env python3
"""Prüft statische SQL-Strings im Rust-Code gegen das migrierte PostgreSQL-Schema.

Jede gefundene Abfrage wird mit ``PREPARE`` (ohne Ausführung) gegen eine
Datenbank vorbereitet, auf die alle Migrationen angewendet wurden. Gemeldet
werden fehlende Tabellen/Spalten und Syntaxfehler. Dynamisch zusammengesetzte
SQL-Fragmente (``format!``) werden nur geprüft, wenn sie vollständig sind.

Verwendung:
    python3 scripts/check_sql.py --database-url postgres://.../kis_schema [--crate kis-core ...]

Die Datenbank muss leer und frisch migriert sein (z. B. per ``kis-admin`` oder
den Integrationstests). Es werden keine Daten verändert: jede Prüfung läuft in
einer Transaktion, die zurückgerollt wird.
"""

from __future__ import annotations

import argparse
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


def extract(path: pathlib.Path):
    text = path.read_text(encoding="utf-8")
    # Testmodule auslassen
    cut = text.find("#[cfg(test)]\nmod tests")
    if cut != -1:
        text = text[:cut]
    found = []
    for m in RAW_STR.finditer(text):
        found.append((text.count("\n", 0, m.start()) + 1, m.group(1)))
    for m in PLAIN_STR.finditer(text):
        found.append((text.count("\n", 0, m.start()) + 1, rust_unescape(m.group(1))))
    for line, sql in found:
        if SQL_START.match(sql) and "{" not in sql and len(sql) > 15:
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


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--database-url", required=True)
    ap.add_argument("--crate", action="append", help="nur diese Crates (Standard: alle)")
    args = ap.parse_args()

    crates = sorted((ROOT / "crates").iterdir())
    if args.crate:
        crates = [c for c in crates if c.name in args.crate]
    failures = 0
    total = 0
    for crate in crates:
        for path in sorted(crate.rglob("src/**/*.rs")):
            for line, sql in extract(path):
                total += 1
                err = check(args.database_url, sql)
                if err:
                    failures += 1
                    print(f"{path.relative_to(ROOT)}:{line}: {err}")
    print(f"{total} SQL-Anweisungen geprüft, {failures} Fehler", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
