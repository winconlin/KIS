# Unabhängiger G6-Review-Bericht — AP-09 / PR #9 (`list_doctors` Parametrisierung & PG-Tests)

- **Arbeitspaket:** `AP-09` *(im PR-Titel aktuell noch `AP-01 (KIS-22)` genannt — siehe Befund R-02)*
- **Fachlicher Umfang:** Parametrisierung der SQL-Abfrage in `crates/kis-doctors/src/lib.rs::list_doctors` (`G-13`: fehlende `.bind(...)` für `$1`/`$2` und direkte String-Formatierung von `cursor`) sowie PostgreSQL-Integrationstests (`crates/kis-doctors/tests/list_doctors_pg.rs`)
- **Datum:** 26.11.2026
- **Implementierer:** `hermes-qwen38`
- **Unabhängiger Reviewer (`G6`):** **`google-neon`** *(nicht am Implementierungscode von PR #9 beteiligt)*
- **Referenzierte GitHub-Artefakte (live geprüft am 26.11.2026 via GitHub-API):**
  - `origin/main` Basis-SHA: `df53cfdae864c5b351027d628400a25f88f35ae6`
  - Branch: `hermes-qwen38/base-20261124`
  - Geprüfter Ergebnis-Commit: `398904174cd1ef9683fee72e31132896e7ae8aa7` (inkl. Vorgänger-Commit `8725388baa`)
  - Pull Request: `https://github.com/winconlin/KIS/pull/9`
  - Koordinations-Issue-Anfrage: `https://github.com/winconlin/KIS/issues/10`

---

## 1. Koordinationsentscheidung (Reservierung Issue #10)

Die Reservierungsanfrage von `hermes-qwen38` (Issue #10) für die unabhängige Bestandskorrektur in `crates/kis-doctors/src/lib.rs::list_doctors` wird hiermit durch den Koordinator (`google-neon`) **formell bestätigt und unter der kollisionsfreien Paket-ID `AP-09` (`G-13`)** im [KOORDINATIONSREGISTER.md](../KOORDINATIONSREGISTER.md) geführt. Die Arbeiten kollidieren weder mit `EP-00` noch mit gesperrten Migrationen (`012+`) oder `kis-core`.

---

## 2. Prüfgrundlage & tatsächliche Nachweise

| Prüfung | Quelle / Methode | Ergebnis |
|---|---|---|
| Vollständiger Diff `df53cfdae8..398904174c` (`Cargo.lock`, `crates/kis-doctors/Cargo.toml`, `crates/kis-doctors/src/lib.rs`, `crates/kis-doctors/tests/common.rs`, `crates/kis-doctors/tests/list_doctors_pg.rs`) | Manuelle Code-, Architektur- und Sicherheitsprüfung via GitHub-API (`google-neon`) | Geprüft (3 Befunde: 1 Architektur-Blocker, 2 geringe Befunde) |
| GitHub-CI-Check-Runs auf Commit `398904174c` | Live-Abfrage `/commits/398904174c/check-runs` | **5/5 bestanden** (`container`, `test`, `lint`, `audit`, `docs` jeweils `completed / success`) |
| Vorheriges automatisiertes Review (`chatgpt-codex-connector[bot]` auf `8725388baa`) | Live-Abfrage PR #9 Review-Kommentare vs. Commit `398904174c` | Alle 4 Codex-Befunde (1× P1 Cursor-`Uuid`-Bindung, 3× P2 `nextCursor`, `gr%C3%BCninger`, geordnete Test-UUIDs) nachvollziehbar behoben |
| Lokale `cargo test`-Ausführung durch `google-neon` | Container-Snapshot ohne `cargo`/`postgres` | **Nicht lokal wiederholt** — stützt sich für die Ausführung auf die grünen GitHub-CI-Check-Runs auf `398904174c` |

---

## 3. Positiv geprüfte Punkte (Sicherheit & Korrektheit)

1. **SQL-Parametrisierung in `crates/kis-doctors/src/lib.rs::list_doctors`:**
   - `search_bind` (`Option<String>`), `specialty_bind` (`Option<String>`) und `cursor_bind` (`Option<Uuid>`) werden typsicher und exakt in der Reihenfolge der generierten Platzhalter `$1..$n` gebunden.
   - Die frühere String-Formatierung `format!("id > '{}'", cursor)` sowie die vergessenen `.bind(...)`-Aufrufe (`G-13`) sind vollständig behoben.
   - `limit` wird vor dem Anhängen über `.clamp(1, 200)` als Integer begrenzt; `active` ist ein `bool`.
2. **Qualität der PostgreSQL-Integrationstests (`crates/kis-doctors/tests/list_doctors_pg.rs`):**
   - 20 Tests gegen echte Wegwerf-PostgreSQL-Datenbanken (`KIS_REQUIRE_PG_TESTS=1` erzwungen), ausschließlich synthetische Daten.
   - Deckt Filterkombinationen, Soft-Delete (`active=false`), Sonderzeichen/Injection-Payloads, Umlaute (`gr%C3%BCninger`), leere Ergebnisse, Limit-Clamping, deterministische UUID-Cursor-Pagination und `camelCase`-Serialisierung (`nextCursor`) ab.

---

## 4. Befunde des unabhängigen G6-Reviews

### R-01 — [Mittel / Architektur-Leitplanke · Vor Merge beheben] Unbenutzte zirkuläre Dev-Abhängigkeit `kis-app` in `crates/kis-doctors/Cargo.toml`

- **Fundstelle:** `crates/kis-doctors/Cargo.toml` (`[dev-dependencies] kis-app = { path = "../kis-app" }`), `Cargo.lock` (+3 Zeilen) sowie das unbenutzte Makro `app!` in `crates/kis-doctors/tests/common.rs`.
- **Befund:** `crates/kis-doctors/tests/list_doctors_pg.rs` nutzt `kis_app` überhaupt nicht, sondern baut `DoctorsModule.routes(&ctx)` direkt auf `kis-core` auf. Die Deklaration von `kis-app` unter `[dev-dependencies]` eines Fachmoduls erzeugt eine unnötige zyklische Workspace-Abhängigkeit (`kis-app → kis-doctors → [dev-dependencies] kis-app`), widerspricht dem Modulprinzip in [AGENTS.md](../../AGENTS.md) § 4 / [MODULE.md](../MODULE.md) („Jedes Fachmodul hängt nur von `kis-core` ab“) und verursacht unnötige `Cargo.lock`-Änderungen entgegen der Sparsamkeitsregel.
- **Erforderliche Maßnahme:**
  1. `kis-app = { path = "../kis-app" }` (samt veraltetem Kommentar) aus `crates/kis-doctors/Cargo.toml` entfernen.
  2. Das unbenutzte Makro `app!` (und ggf. ungenutzte `patient_body`/`id_of`-Helfer) aus `crates/kis-doctors/tests/common.rs` entfernen.
  3. `Cargo.lock` aktualisieren und `KIS_TEST_DATABASE_URL=... scripts/check.sh` erneut grün durchlaufen lassen.

### R-02 — [Gering / Koordination · Vor Merge bereinigen] Paket-ID-Kollision (`AP-01` vs. `AP-09`) & Abnahme-Nachweis

- **Fundstelle:** PR-#9-Titel (`AP-01 (KIS-22): list_doctors — Cursor-Injection + Integrationstests`).
- **Befund:** `AP-01` („`GET /doctors/:id/schedule` nach `kis-appointments` verschoben“) wurde bereits mit PR #6 auf `main` gemerged. Die Behebung des `list_doctors`-Parameterfehlers (`G-13`) ist im zentralen [KOORDINATIONSREGISTER.md](../KOORDINATIONSREGISTER.md) kollisionsfrei als **`AP-09`** geführt.
- **Erforderliche Maßnahme:** PR-#9-Titel auf `AP-09 (G-13 / KIS-22): list_doctors — SQL-Parametrisierung + PG-Integrationstests` umbenennen und im PR-Body kurz die Basis-SHA (`df53cfdae8`), Ergebnis-SHA und Gate-Übersicht (`G0`–`G6`) dokumentieren.

### R-03 — [Hinweis / Test- & Suchhygiene · Optional / Nachverfolgbar] `TestDb`-Cleanup & `ILIKE`-Wildcards

- **Befund (a):** `DocApp` hält `_tdb: TestDb`, ruft aber `tdb.drop().await` in den Tests nicht auf. In ephemerer CI ist das unkritisch; bei wiederholten lokalen Läufen bleiben je Durchlauf 20 `kis_it_*`-Datenbanken liegen.
- **Befund (b):** `list_doctors_pg.rs` testet den Modul-Router ohne zentrale Auth-/RBAC-Middleware. Da `GET /api/v1/doctors` eine Bestandsroute ist und RBAC in `kis-app/tests/` geprüft wird, ist das für `AP-09` akzeptabel — sollte im PR-Body kurz vermerkt bleiben.
- **Befund (c):** `format!("%{}%", search)` ist gegen SQL-Injection sicher parametrisiert, behandelt aber literale `%`- oder `_`-Zeichen im Suchstring als `ILIKE`-Wildcards (kein Sicherheitsfehler, aber bei späterer Mandanten-Suche `EP-02b` bewusst zu berücksichtigen).

---

## 5. G6-Review-Urteil & Post-Merge-Nachverfolgung

- **Merge-Status auf `origin/main` (26.11.2026):** PR #9 wurde als Merge-Commit **`2d0e4479f91cba42f4ede2249488e3d73e12c094`** nach `main` übernommen (5/5 CI-Checks grün); Issue #10 wurde geschlossen.
- **Unabhängige G6-Bewertung (`google-neon`):** Der fachliche SQL-Parametrisierungsfix (`list_doctors`, `G-13`) und die 20 PostgreSQL-Integrationstests sind sicherheitstechnisch korrekt und im Produktcode unbedenklich.
- **Offene Nachbesserung (`AP-09b`):** Befund **R-01** (Entfernung der unbenutzten zirkulären `[dev-dependencies] kis-app` aus `crates/kis-doctors/Cargo.toml` + `Cargo.lock` sowie ungenutzter Helfer in `crates/kis-doctors/tests/common.rs`) ist im [KOORDINATIONSREGISTER.md](../KOORDINATIONSREGISTER.md) als eigenständiges Folgepaket **`AP-09b`** auf Basis `2d0e4479f9` zur Bereinigung auf einem Rust-Host eingetragen (kein Eingriff ohne `cargo`/`Cargo.lock`, damit `cargo test --locked` in der CI grün bleibt). Keine Produktivfreigabe (`G7`).
