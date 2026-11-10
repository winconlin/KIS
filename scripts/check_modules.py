#!/usr/bin/env python3
"""Prüft die Modulgrenzen (siehe config/module_tables.toml).

Fachmodule sollen unabhängig voneinander änderbar sein. Das gilt nur,
solange kein Modul in die Daten eines anderen greift. Dieses Skript
erzwingt das ohne Datenbank, nur aus dem Quelltext:

  1. Jede Tabelle aus migrations/ ist genau einem Eigentümer zugeordnet.
  2. Fachmodule nutzen nur eigene Tabellen, Kern-Tabellen (lesend) und
     ausdrücklich freigegebene Tabellen (`reads`).
  3. Fachmodule schreiben nicht in Kern-Tabellen.
  4. Fachmodule hängen nur von kis-core ab.
  5. Jede Crate unter crates/ ist als Modul eingetragen und in
     kis-app/src/modules.rs registriert.

Verwendung:  python3 scripts/check_modules.py
"""

from __future__ import annotations

import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CREATE = re.compile(r"CREATE TABLE (?:IF NOT EXISTS )?([a-z_][a-z0-9_]*)")
STRING = re.compile(r'r#"(.*?)"#|"((?:[^"\\]|\\.)*)"', re.DOTALL)
USES = re.compile(r"\b(?:FROM|JOIN|INTO|UPDATE|USING)\s+([a-z_][a-z0-9_]*)")
WRITES = re.compile(r"\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+([a-z_][a-z0-9_]*)")


def source_strings(crate: pathlib.Path):
    """Alle Zeichenketten aus dem Nicht-Test-Code einer Crate."""
    for path in sorted(crate.glob("src/**/*.rs")):
        text = path.read_text(encoding="utf-8")
        cut = text.find("#[cfg(test)]")
        if cut != -1:
            text = text[:cut]
        # Kommentare entfernen, damit Beispiele in der Doku nicht zählen
        text = re.sub(r"^\s*//.*$", "", text, flags=re.MULTILINE)
        for m in STRING.finditer(text):
            line = text.count("\n", 0, m.start()) + 1
            yield path.relative_to(ROOT), line, m.group(1) or m.group(2) or ""


def main() -> int:
    cfg = tomllib.loads((ROOT / "config/module_tables.toml").read_text(encoding="utf-8"))
    kernel_tables = set(cfg["kernel"]["tables"])
    kernel_crates = set(cfg["kernel"]["crates"])
    modules = cfg.get("modules", {})
    errors: list[str] = []

    # 1. Vollständige, eindeutige Zuordnung
    migrated: set[str] = set()
    for path in sorted((ROOT / "migrations").glob("*.sql")):
        migrated.update(CREATE.findall(path.read_text(encoding="utf-8")))
    owner: dict[str, str] = {t: "kernel" for t in kernel_tables}
    for name, mod in modules.items():
        for table in mod.get("tables", []):
            if table in owner:
                errors.append(f"Tabelle {table} ist doppelt zugeordnet ({owner[table]} und {name})")
            owner[table] = name
    for table in sorted(migrated - owner.keys()):
        errors.append(f"Tabelle {table} (migrations/) fehlt in config/module_tables.toml")
    for table in sorted(owner.keys() - migrated):
        errors.append(f"Tabelle {table} steht in module_tables.toml, aber in keiner Migration")

    # 5. Jede Crate ist eingetragen und registriert
    crate_dirs = {p.name for p in (ROOT / "crates").iterdir() if (p / "Cargo.toml").is_file()}
    module_crates = {mod["crate"]: name for name, mod in modules.items()}
    registry = (ROOT / "crates/kis-app/src/modules.rs").read_text(encoding="utf-8")
    for crate in sorted(crate_dirs - {"kis-core", "kis-app"}):
        if crate not in module_crates and crate not in kernel_crates:
            errors.append(f"Crate {crate} fehlt in config/module_tables.toml")
        if f"{crate.replace('-', '_')}::" not in registry:
            errors.append(f"Crate {crate} ist nicht in kis-app/src/modules.rs registriert")
    for crate in sorted(set(module_crates) - crate_dirs):
        errors.append(f"module_tables.toml nennt Crate {crate}, die es nicht gibt")

    # 2.–4. Grenzen der Fachmodule
    for crate, name in sorted(module_crates.items()):
        if crate not in crate_dirs:
            continue
        mod = modules[name]
        own = set(mod.get("tables", []))
        allowed = own | kernel_tables | set(mod.get("reads", []))
        for path, line, text in source_strings(ROOT / "crates" / crate):
            for table in USES.findall(text):
                if table in owner and table not in allowed:
                    errors.append(
                        f"{path}:{line}: Modul {name} greift auf {table} zu "
                        f"(gehört {owner[table]}). Zusammenspiel über Ereignisse oder Kernfunktionen."
                    )
            for table in WRITES.findall(text):
                if table in owner and table not in own:
                    errors.append(
                        f"{path}:{line}: Modul {name} schreibt in {table} "
                        f"(gehört {owner[table]}). Dafür gibt es Kernfunktionen in kis-core."
                    )
        manifest = tomllib.loads((ROOT / "crates" / crate / "Cargo.toml").read_text(encoding="utf-8"))
        for dep in manifest.get("dependencies", {}):
            if dep.startswith("kis-") and dep != "kis-core":
                errors.append(f"crates/{crate}/Cargo.toml: Abhängigkeit auf {dep} — Module hängen nur von kis-core ab")

    # kis-core hängt von nichts aus dem Workspace ab
    core = tomllib.loads((ROOT / "crates/kis-core/Cargo.toml").read_text(encoding="utf-8"))
    for dep in core.get("dependencies", {}):
        if dep.startswith("kis-"):
            errors.append(f"crates/kis-core/Cargo.toml: Abhängigkeit auf {dep} — der Kern hängt von keinem Modul ab")

    for e in errors:
        print(e)
    print(
        f"{len(migrated)} Tabellen, {len(modules)} Fachmodule geprüft, {len(errors)} Verstöße",
        file=sys.stderr,
    )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
