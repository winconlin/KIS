#!/usr/bin/env python3
"""Legt das Gerüst für ein neues Fachmodul an und bindet es ein.

    python3 scripts/new_module.py <name> "<Titel>"

Beispiel:  python3 scripts/new_module.py pharmacy "Apotheke"

Erzeugt und trägt ein:
  - crates/kis-<name>/ (Cargo.toml, src/lib.rs nach dem Muster von kis-nursing)
  - Workspace (Cargo.toml), Anwendung (crates/kis-app/Cargo.toml, src/modules.rs)
  - config/module_tables.toml (noch ohne Tabellen)
  - crates/kis-app/tests/<name>.rs (ein lauffähiger Test zum Erweitern)

Das neue Modul ist `Experimental`: Es wird gebaut und getestet, ist aber
nur aktiv, wenn es in der Konfiguration unter `[modules] experimental`
steht. Erst wenn es fertig ist, wird es auf `Stable` gestellt.
Die weiteren Schritte stehen in docs/MODULE.md.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
MARKER = "NEUE-MODULE"

CARGO = '''[package]
name = "kis-{name}"
version = "0.1.0"
edition = "2021"
description = "KIS-Fachmodul {title}"
license = "MIT"

[dependencies]
kis-core = {{ workspace = true }}
axum = {{ workspace = true }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}
sqlx = {{ workspace = true }}
uuid = {{ workspace = true }}
chrono = {{ workspace = true }}

[lints]
workspace = true
'''

LIB = '''//! Fachmodul **{title}**.
//!
//! Neu angelegt mit `scripts/new_module.py` und noch ohne Routen. Das Modul
//! ist `Experimental` und damit nur aktiv, wenn `{name}` in der
//! Konfiguration unter `[modules] experimental` steht.
//!
//! Vorgehen (ausführlich in `docs/MODULE.md`, Muster: `crates/kis-nursing`):
//! Tabellen in einer neuen Migration anlegen und in
//! `config/module_tables.toml` diesem Modul zuordnen, Rechte in
//! `kis-core/src/rbac.rs` ergänzen, Routen unten registrieren, Tests in
//! `crates/kis-app/tests/{name}.rs` schreiben, mit `scripts/check.sh`
//! prüfen und zuletzt `Maturity::Stable` setzen.

use kis_core::module::{{Maturity, Module, ModuleContext, ModuleInfo, ModuleRoutes}};

pub struct {type_name}Module;

impl Module for {type_name}Module {{
    fn info(&self) -> ModuleInfo {{
        ModuleInfo {{
            id: "{name}",
            title: "{title}",
            description: "{title}",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &["cases"],
            maturity: Maturity::Experimental,
            kernel: false,
        }}
    }}

    fn routes(&self, ctx: &ModuleContext) -> ModuleRoutes {{
        ModuleRoutes::<ModuleContext>::new().with_state(ctx.clone())
    }}
}}
'''

TEST = '''//! Tests des Moduls {title}. Testgerüst: `tests/common/mod.rs`.

mod common;

/// Das Modul ist experimentell: ohne Eintrag in der Konfiguration aus,
/// mit Eintrag an.
#[tokio::test]
async fn {name}_is_opt_in() {{
    let enabled = |modules: &serde_json::Value| {{
        modules["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "{name}")
            .expect("Modul gelistet")["enabled"]
            .as_bool()
            .unwrap()
    }};

    let app = app!();
    let (_, admin) = app.session("admin1", "admin").await;
    assert!(!enabled(&app.ok("GET", "/api/v1/modules", &admin, None).await));
    app.finish().await;

    let mut cfg = common::config();
    cfg.modules.experimental = vec!["{name}".to_string()];
    let app = app!(cfg);
    let (_, admin) = app.session("admin1", "admin").await;
    assert!(enabled(&app.ok("GET", "/api/v1/modules", &admin, None).await));
    app.finish().await;
}}
'''


def insert_before_marker(path: pathlib.Path, line: str) -> None:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    for i, existing in enumerate(lines):
        if MARKER in existing:
            indent = existing[: len(existing) - len(existing.lstrip())]
            lines.insert(i, f"{indent}{line}\n")
            path.write_text("".join(lines), encoding="utf-8")
            return
    sys.exit(f"{path.relative_to(ROOT)}: Markierung {MARKER} fehlt")


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    name, title = sys.argv[1], sys.argv[2].strip()
    if not re.fullmatch(r"[a-z][a-z0-9]{2,23}", name):
        sys.exit("Name: 3–24 Zeichen, Kleinbuchstaben und Ziffern, beginnt mit einem Buchstaben")
    if not title or '"' in title or "\\" in title:
        sys.exit("Titel darf nicht leer sein und keine Anführungszeichen enthalten")
    crate = ROOT / "crates" / f"kis-{name}"
    if crate.exists():
        sys.exit(f"crates/kis-{name} existiert bereits")
    type_name = name.capitalize()

    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text(CARGO.format(name=name, title=title), encoding="utf-8")
    (crate / "src" / "lib.rs").write_text(
        LIB.format(name=name, title=title, type_name=type_name), encoding="utf-8"
    )
    (ROOT / "crates/kis-app/tests" / f"{name}.rs").write_text(
        TEST.format(name=name, title=title), encoding="utf-8"
    )
    insert_before_marker(ROOT / "Cargo.toml", f'"crates/kis-{name}",')
    insert_before_marker(ROOT / "crates/kis-app/Cargo.toml", f'kis-{name} = {{ path = "../kis-{name}" }}')
    insert_before_marker(
        ROOT / "crates/kis-app/src/modules.rs", f"Box::new(kis_{name}::{type_name}Module),"
    )
    with (ROOT / "config/module_tables.toml").open("a", encoding="utf-8") as fh:
        fh.write(f'\n[modules.{name}]\ncrate = "kis-{name}"\ntables = []\n')

    print(f"Modul {name} angelegt: crates/kis-{name}/, crates/kis-app/tests/{name}.rs")
    print("Weiter mit docs/MODULE.md; prüfen mit scripts/check.sh")
    return 0


if __name__ == "__main__":
    sys.exit(main())
