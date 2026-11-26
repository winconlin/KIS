# Abnahmebericht — Verbindliche Vorlage (`G0`–`G7`)

Stand: 24.11.2026 (v2) · Gilt für alle Arbeitspakete (`EP-00` bis `EP-26`, `DOC-01`–`07`, `ED-01`–`06`, `AP-*`) gemäß [AGENTS.md](../AGENTS.md) § 7, [ARBEITSPAKETE.md](ARBEITSPAKETE.md) § 5, [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md) § 9, [ROADMAP.md](ROADMAP.md) und [KOORDINATIONSREGISTER.md](KOORDINATIONSREGISTER.md).

## Grundregeln für jeden Abnahmebericht

1. **Nur tatsächlich ausgeführte Prüfungen** auf dem konkreten Ergebnis-Commit dokumentieren. Nicht ausgeführte, übersprungene oder wegen fehlender Infrastruktur blockierte Prüfungen als **offen** oder **blockiert** kennzeichnen.
2. **Kein Selbst-Review (`G6`):** Wer Code, Migrationen oder Architekturdokumente selbst verfasst hat, darf seine eigene Prüfung niemals als unabhängiges `G6`-Review ausgeben. Das unabhängige Gegenreview muss explizit einer anderen Instanz zugewiesen werden.
3. **Ehrliche Status-Taxonomie:** Strikt trennen zwischen `Geplant`, `Spezifiziert`, `In Bearbeitung`, `Blockiert`, `Im Review`, `Technisch integriert`, `Lokal getestet`, `Gegenstelle getestet`, `Fachlich abgenommen`, `Extern bestätigt` und `Betriebsfreigegeben`.
4. **Keine Produktivfreigabe:** Lokale und CI-Tests ersetzen keine ärztliche, pflegerische, datenschutzrechtliche, betriebliche oder regulatorische Freigabe (`G7`).

---

## Vorlage (für jedes Arbeitspaket unter `docs/abnahme/<PAKET-ID>.md` kopieren)

```text
# Abnahmebericht — <PAKET-ID> (<Kurztitel>)

- **Arbeitspaket / Meilenstein:** <z. B. EP-01a / M01>
- **Fachlicher Umfang:** <eindeutige Kurzbeschreibung, nicht nur Nummer>
- **Datum:** <TT.MM.JJJJ>
- **Agentenidentität (Implementierer):** <z. B. google-neon | hermes-qwen38>
- **Koordinations-Issue / Zuweisung:** <#Issue-Nr. / Bestätigung durch Koordinator>
- **Branch / Pull Request:** <agent/<agent>/<id>-<kurzbeschreibung> · PR #...>
- **Basis-SHA (`origin/main`):** <exakter Commit-SHA>
- **Ergebnis-SHA:** <exakter Ergebnis-Commit-SHA>
- **Unabhängiger Reviewer (`G6`):** <offen (zugewiesen an ...) | andere Instanz>

## 1. Problemstellung, Verträge & Native-First-Prüfung
- Welches Problem / welche Anforderung wird gelöst?
- Maßgebliche ADRs, Traceability-IDs (`E*`/`TI*`/`DOC*`/`ED*`/`AP*`) und Abhängigkeiten
- Native-First-Prüfung (`N` / `H` / `X` gemäß docs/NATIVE_FIRST_REGISTER.md): ...

## 2. Durchgeführte Änderungen
- Geänderte / neue Dateien (Code, Migrationen, Konfiguration, Tests, Dokumentation)
- Neue oder geänderte API-Routen inkl. Zugriffsregel (`Access`)
- Datenbankmigrationen & aktualisierte `migrations/CHECKSUMS`

## 3. Tatsächlich ausgeführte Prüfungen (Befehl & tatsächliches Ergebnis)
| Prüfung | Befehl | Status | Befund / Umfang |
|---|---|---|---|
| Gesamtprüfung (`check.sh`) | `KIS_TEST_DATABASE_URL=... scripts/check.sh` | bestanden / fehlgeschlagen / offen | ... |
| Format & Clippy | `cargo fmt --all -- --check` & `cargo clippy --locked --workspace --all-targets -- -D warnings` | bestanden / offen | ... |
| Cargo Tests & Audit | `cargo test --locked --workspace` & `cargo audit` | bestanden / offen | ... |
| Python-Guardrails | `check_guardrails.py`, `check_modules.py`, `check_docs.py`, `check_audit_exceptions.py` | bestanden / offen | ... |
| Negative Verbundtests (`NVT-*`) & `SICHERHEITS_ABNAHME` | ... | bestanden / offen | ... |
| Backup / Restore & Container | ... | bestanden / nicht betroffen / offen | ... |

## 4. Quality-Gate-Checkliste (`G0`–`G7`)
- [ ] G0: Auftrag, Reservierung, Grenzen & Native-First geprüft
- [ ] G1: Architektur-, Modul- & Schema-Integrität geprüft
- [ ] G2: Sicherheit, Mandantenisolation & Negativtests erfüllt
- [ ] G3: Echte PostgreSQL-, Funktions-, Konkurrenz- & Ausfalltests bestanden
- [ ] G4: `scripts/check.sh` & CI-Jobs vollständig fehlerfrei
- [ ] G5: Dokumentation, Funktionslandkarte, Prüfstatus & Koordinationsregister aktualisiert
- [ ] G6: Unabhängiges Review abgeschlossen (nicht durch Implementierer!)
- [ ] G7: Fach-, Betriebs- & Regulatorik-Freigabe (nur vor Pilot-/Produktivbetrieb)

## 5. Verbleibende Risiken, offene Entscheidungen & Blocker
- Konkrete offene Punkte, Abhängigkeiten und ggf. begründete Ausnahmen (mit Verantwortlichem und Frist)

## 6. Ergebnisstatus
- Status gemäß Taxonomie: <In Bearbeitung | Im Review | Blockiert | Technisch integriert | ...>
- Produktivfreigabe: **ausdrücklich NICHT erteilt** (oder Verweis auf externen G7-Nachweis)
```

---

## Verzeichnis erstellter Abnahme- und Prüfberichte

| Paket | Meilenstein | Datum | Bearbeiter | Unabhängiges `G6`-Review | Status | Bericht |
|---|---|---|---|---|---|---|
| `EP-00` (Erstfassung) | `M00` | 24.11.2026 | `google-neon` | offen (zugewiesen an `hermes`) | Im Review (durch v2-Bereinigung aktualisiert) | [abnahme/EP-00.md](abnahme/EP-00.md) |
| `EP-00` (Konsistenz- & Koordinationsprüfung) | `M00` | 24.11.2026 | `google-neon` *(Selbstprüfung, kein `G6`!)* | **offen — zugewiesen an `hermes-qwen38` / unabhängige Instanz** | Im Review | [abnahme/EP-00-konsistenzpruefung.md](abnahme/EP-00-konsistenzpruefung.md) |
| `AP-09` / PR #9 (`list_doctors` Fix & PG-Tests) | Bestandshärtung | 26.11.2026 | `hermes-qwen38` (`2d0e4479f9`) | **`google-neon` (unabhängig geprüft — Nachbesserung `R-01` als `AP-09b` eingetragen)** | Technisch integriert (PR #9) | [abnahme/PR-09-review-list-doctors.md](abnahme/PR-09-review-list-doctors.md) |
