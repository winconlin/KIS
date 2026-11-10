#!/usr/bin/env python3
"""Prüft die Ausnahmen des Abhängigkeits-Audits.

Eine Ausnahme in .cargo/audit.toml ist nur zulässig, wenn die betroffene
Crate zwar im Cargo.lock steht, aber nicht gebaut wird. Dieses Skript
stellt sicher, dass

  1. jede Ausnahme in config/audit_exceptions.toml begründet ist,
  2. die genannte Crate in keinem Build des Workspaces vorkommt
     (alle Features, alle Zielplattformen),
  3. keine Ausnahme übrig bleibt, deren Crate gar nicht mehr im
     Cargo.lock steht.

Verwendung:  python3 scripts/check_audit_exceptions.py
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent


def in_build(crate: str) -> bool:
    """Kommt die Crate im Abhängigkeitsbaum irgendeines Builds vor?"""
    proc = subprocess.run(
        ["cargo", "tree", "--locked", "--workspace", "--all-features", "--target", "all", "-i", crate],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0 and "did not match any packages" not in proc.stderr:
        sys.exit(f"cargo tree für {crate} fehlgeschlagen:\n{proc.stderr.strip()}")
    return bool(proc.stdout.strip())


def main() -> int:
    ignored = tomllib.loads((ROOT / ".cargo/audit.toml").read_text(encoding="utf-8"))
    ignored = set(ignored.get("advisories", {}).get("ignore", []))
    entries = tomllib.loads((ROOT / "config/audit_exceptions.toml").read_text(encoding="utf-8"))
    entries = entries.get("not_built", [])
    lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    errors: list[str] = []

    explained = {e["advisory"] for e in entries}
    for advisory in sorted(ignored - explained):
        errors.append(f"{advisory}: Ausnahme in .cargo/audit.toml ohne Begründung in config/audit_exceptions.toml")
    for advisory in sorted(explained - ignored):
        errors.append(f"{advisory}: steht in config/audit_exceptions.toml, aber nicht in .cargo/audit.toml")

    for entry in entries:
        crate, advisory = entry["crate"], entry["advisory"]
        if not re.search(rf'^name = "{re.escape(crate)}"$', lock, flags=re.MULTILINE):
            errors.append(f"{advisory}: {crate} steht nicht mehr im Cargo.lock — Ausnahme entfernen")
        elif in_build(crate):
            errors.append(
                f"{advisory}: {crate} ist Teil des Builds — die Ausnahme gilt nicht mehr, "
                "die Schwachstelle muss behoben werden"
            )

    for e in errors:
        print(e)
    print(f"{len(entries)} Audit-Ausnahmen geprüft, {len(errors)} Verstöße", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
