# Git-, GitHub- & CI-Übergabe-Playbook (`EP-00` → `G6`-Review → `M01`)

Stand: 26.11.2026 · Koordinator: `google-neon` · Ziel-Repository: `https://github.com/winconlin/KIS`

Dieses Playbook ermöglicht die saubere, nachvollziehbare Übernahme des lokal geprüften `EP-00`- und `M01`-Vertragsstandes aus einem toolchain-freien Snapshot auf einen vollständigen Git-, GitHub-, Rust- und PostgreSQL-Host — ohne unbeabsichtigte Änderungen an `crates/` oder `migrations/`.

---

## 1. Vorbedingungen auf dem Ziel-Host

- Authentifiziertes Git (`git`) und GitHub-CLI (`gh auth status`) mit Zugriff auf `winconlin/KIS`
- Rust-Toolchain gemäß `rust-toolchain.toml` (`cargo`, `rustc`, `rustfmt`, `clippy`) und `Cargo.lock` aus `origin/main`
- Laufender PostgreSQL-16+-Server (mit Erstellungsrecht für Wegwerf-Testdatenbanken via `KIS_TEST_DATABASE_URL`)
- `python3` (3.11+) sowie optional `age` / `pg_dump` / `docker` für vollständige Backup- und Container-Tests

---

## 2. Schritt 1 — Frischen Koordinations-Branch anlegen

Immer vom aktuellen `origin/main` abzweigen (niemals direkt auf `main` committen):

```bash
git fetch origin
git checkout -b agent/google-neon/ep00-architecture-coordination origin/main
git rev-parse HEAD
```

---

## 3. Schritt 2 — Ausschließlich reservierte Architektur- & Prüfdateien übernehmen

Nach dem Kopieren der neuen und aktualisierten Dateien aus `docs/` sowie `scripts/check_vertraege.py` zwingend sicherstellen, dass **`crates/`**, **`migrations/`** und **`migrations/CHECKSUMS`** vollständig unverändert sind:

```bash
git status -sb
git diff --stat -- crates/ migrations/
```

Erwartetes Ergebnis für `git diff --stat -- crates/ migrations/`: **leer** (0 geänderte Dateien).

---

## 4. Schritt 3 — Vollständige Gesamtprüfung (`G4`) ausführen

Erst wenn sämtliche Prüfungen ohne Abschwächung fehlerfrei durchlaufen, wird der Commit erstellt:

```bash
KIS_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres scripts/check.sh
python3 scripts/check_vertraege.py
```

---

## 5. Schritt 4 — Commit, Push & Zentrales GitHub-Koordinations-Issue anlegen

```bash
git add docs/ scripts/check_vertraege.py
git commit -m "docs(ep-00): Zielarchitektur, Konsistenzbereinigung, M01-Vertraege und Koordinationsregister"
git push -u origin agent/google-neon/ep00-architecture-coordination

gh issue create \
  --repo winconlin/KIS \
  --title "[Koordination] Zentrales Multi-Agenten-Arbeitspaketregister (EP-00 bis EP-26 / AP-*)" \
  --body-file docs/KOORDINATIONSREGISTER.md
```

---

## 6. Schritt 5 — Draft-PR eröffnen & Unabhängiges `G6`-Gegenreview (`hermes`) anfordern

Da `google-neon` Erstautor von `EP-00` und den `M01`-Umsetzungsverträgen ist, vergibt `google-neon` ausdrücklich **keine** eigene `G6`-Freigabe. Das unabhängige Gegenreview (`EP-00-REV`) wird auf dem Draft-PR an `hermes` (oder eine andere unabhängige Prüfinstanz) übergeben:

```bash
gh pr create \
  --repo winconlin/KIS \
  --base main \
  --head agent/google-neon/ep00-architecture-coordination \
  --draft \
  --title "EP-00 (M00): Zielarchitektur, Konsistenzbereinigung, M01-Vertraege & Koordinationsregister" \
  --body-file docs/abnahme/EP-00-konsistenzpruefung.md
```

---

## 7. Schritt 6 — Stand `origin/main` (`2d0e4479f9`), Nachbesserung `AP-09b` & Startfolge nach `G6`-Freeze

1. **Aktueller GitHub-Stand (geprüft am 26.11.2026):**
   - `origin/main` steht auf **`2d0e4479f9`** (PR #9 von `hermes-qwen38` für `list_doctors` / `AP-09` wurde mit 5/5 grünen CI-Checks gemerged; Issue #10 ist geschlossen; 0 offene PRs/Issues).
   - Das unabhängige `G6`-Review liegt unter [abnahme/PR-09-review-list-doctors.md](abnahme/PR-09-review-list-doctors.md) vor. Der verbleibende Architektur-Befund `R-01` (Entfernung der unbenutzten zirkulären Dev-Dependency `kis-app` aus `crates/kis-doctors/Cargo.toml` + `Cargo.lock`) wird als kleines Folgepaket **`AP-09b`** auf einem Rust-Host nachgezogen:

```bash
gh issue create \
  --repo winconlin/KIS \
  --title "[AP-09b] Unbenutzte Dev-Dependency kis-app aus crates/kis-doctors/Cargo.toml entfernen (G6-Befund R-01 zu PR #9)" \
  --body-file docs/abnahme/PR-09-review-list-doctors.md
```

2. **Erst nach grünem `G4` und unabhängigem `G6`-Gegenreview von `EP-00`:**
   - Exklusive Migrationsreservierung **`012`** für **`EP-01a`** gemäß [EP-01_MANDANTEN_UND_ORG_VERTRAG.md](vertraege/EP-01_MANDANTEN_UND_ORG_VERTRAG.md).
   - Koordinierter Parallelstart **`EP-06a`** (`crates/kis-catalogs` + Normenregister) gemäß [EP-06_KATALOG_NORMEN_UND_ZEITVERTRAG.md](vertraege/EP-06_KATALOG_NORMEN_UND_ZEITVERTRAG.md), sobald die freie Migrationsnummer vergeben ist.
   - Anschließend streng koordinierte Abarbeitung `EP-01b/c` → `EP-02a–c` → `EP-03a–c` → `EP-04a–d` → `EP-05a–b` → `EP-07a–b`.
