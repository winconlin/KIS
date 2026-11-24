#!/usr/bin/env python3
"""Leitplanken, die ohne Compiler und ohne Datenbank prüfbar sind.

  1. Migrationen: eine einmal aufgenommene Migration wird nie mehr
     geändert (Prüfsummen in migrations/CHECKSUMS); Nummern sind lückenlos.
  2. Kein unfertiger Code im Produktivpfad: todo!, unimplemented!, dbg!,
     TODO/FIXME-Vermerke, println! außerhalb der Werkzeuge.
  3. Audit-Ergebnisse werden nicht verworfen.
  4. Tests werden nicht stillgelegt (#[ignore]) und CI-Schritte nicht
     abgeschwächt (continue-on-error, `|| true`).

Verwendung:
    python3 scripts/check_guardrails.py                  prüfen
    python3 scripts/check_guardrails.py --add-migration  neue Migration(en)
        in migrations/CHECKSUMS aufnehmen (bestehende Einträge bleiben
        unverändert)
"""

from __future__ import annotations

import hashlib
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHECKSUMS = ROOT / "migrations" / "CHECKSUMS"

FORBIDDEN = [
    (re.compile(r"\b(todo|unimplemented|dbg)!\s*\("), "unfertiger Code (todo!/unimplemented!/dbg!)"),
    (re.compile(r"\b(TODO|FIXME|XXX|HACK)\b"), "offener Vermerk im Code — gehört nach docs/ARBEITSPAKETE.md"),
    (re.compile(r"let\s+_\s*=\s*[^;]*\baudit\w*\s*\("), "Audit-Ergebnis verworfen"),
    (re.compile(r"\.ok\(\)\s*;\s*//.*audit", re.IGNORECASE), "Audit-Ergebnis verworfen"),
]
PRINT = re.compile(r"\b(println|eprintln|print)!\s*\(")


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_checksums() -> dict[str, str]:
    known: dict[str, str] = {}
    if CHECKSUMS.is_file():
        for line in CHECKSUMS.read_text(encoding="utf-8").splitlines():
            if line.strip() and not line.startswith("#"):
                digest, name = line.split()
                known[name] = digest
    return known


def check_migrations(add: bool) -> list[str]:
    errors: list[str] = []
    files = sorted((ROOT / "migrations").glob("*.sql"))
    known = read_checksums()
    numbers = []
    for path in files:
        m = re.match(r"^(\d{3})_[a-z0-9_]+\.sql$", path.name)
        if not m:
            errors.append(f"migrations/{path.name}: Name muss NNN_beschreibung.sql sein")
            continue
        numbers.append(int(m.group(1)))
    if numbers != list(range(1, len(numbers) + 1)):
        errors.append("migrations/: Nummern müssen lückenlos und eindeutig bei 001 beginnen")

    new_entries = []
    for path in files:
        digest = sha256(path)
        if path.name not in known:
            if add:
                new_entries.append(f"{digest}  {path.name}")
            else:
                errors.append(
                    f"migrations/{path.name}: nicht in migrations/CHECKSUMS — "
                    "nach Fertigstellung `python3 scripts/check_guardrails.py --add-migration`"
                )
        elif known[path.name] != digest:
            errors.append(
                f"migrations/{path.name}: Inhalt geändert. Aufgenommene Migrationen sind "
                "unveränderlich — Änderungen als neue Migration anlegen."
            )
    for name in sorted(set(known) - {p.name for p in files}):
        errors.append(f"migrations/{name}: steht in CHECKSUMS, Datei fehlt")
    if add and new_entries:
        with CHECKSUMS.open("a", encoding="utf-8") as fh:
            fh.write("\n".join(new_entries) + "\n")
        print(f"{len(new_entries)} Migration(en) in CHECKSUMS aufgenommen", file=sys.stderr)
    return errors


def production_code(text: str) -> str:
    """Schneidet Testmodule ab."""
    cut = text.find("#[cfg(test)]")
    return text if cut == -1 else text[:cut]


def check_code() -> list[str]:
    errors: list[str] = []
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        rel = path.relative_to(ROOT)
        is_tool = "/bin/" in rel.as_posix()
        for number, line in enumerate(production_code(path.read_text(encoding="utf-8")).splitlines(), 1):
            for pattern, message in FORBIDDEN:
                if pattern.search(line):
                    errors.append(f"{rel}:{number}: {message}")
            if not is_tool and PRINT.search(line) and not line.lstrip().startswith("//"):
                errors.append(f"{rel}:{number}: Ausgabe mit println!/eprintln! — bitte tracing verwenden")
    for path in sorted((ROOT / "crates").glob("**/*.rs")):
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if re.search(r"#\[\s*ignore", line):
                errors.append(f"{path.relative_to(ROOT)}:{number}: Test stillgelegt (#[ignore])")
    return errors


def check_ci() -> list[str]:
    errors: list[str] = []
    for path in sorted((ROOT / ".github" / "workflows").glob("*.yml")):
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if line.lstrip().startswith("#"):
                continue
            if "continue-on-error" in line or re.search(r"\|\|\s*(true|echo)\b", line):
                errors.append(f"{path.relative_to(ROOT)}:{number}: CI-Schritt abgeschwächt")
    return errors


def main() -> int:
    add = "--add-migration" in sys.argv[1:]
    errors = check_migrations(add) + check_code() + check_ci()
    for e in errors:
        print(e)
    print(f"Leitplanken geprüft, {len(errors)} Verstöße", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
