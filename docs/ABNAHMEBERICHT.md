# Abnahmebericht — Verbindliche Vorlage (`G0`–`G7`)

Stand: 24.11.2026 · Gilt für alle Arbeitspakete (`EP-00` bis `EP-26`, `DOC-01`–`07`, `ED-01`–`06`, `AP-*`) gemäß [AGENTS.md](../AGENTS.md) § 7, [ARBEITSPAKETE.md](ARBEITSPAKETE.md) § 5, [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md) § 9 und [ROADMAP.md](ROADMAP.md).

## Grundregeln für jeden Abnahmebericht

1. **Nur tatsächlich ausgeführte Prüfungen** auf dem konkreten Ergebnis-Stand dokumentieren. Nicht ausgeführte, übersprungene oder wegen fehlender Infrastruktur blockierte Prüfungen als **offen** kennzeichnen.
2. **Kein Selbst-Review:** Wer eine sicherheitskritische Änderung implementiert, vergibt nicht selbst die unabhängige Freigabe (`G6`).
3. **Keine Produktivfreigabe:** Lokale und CI-Tests ersetzen keine externe Bestätigung, Datenschutz-Folgenabschätzung, Penetrationstest, MDR-Bewertung oder gematik-/KBV-Zulassung (`G7`).

---

## Vorlage (für jedes neue Arbeitspaket unter `docs/abnahme/<PAKET-ID>.md` kopieren)

```text
# Abnahmebericht — <PAKET-ID> (<Kurztitel>)

- **Arbeitspaket / Meilenstein:** <z. B. EP-01a / M01>
- **Datum:** <TT.MM.JJJJ>
- **Branch / PR:** <feature/... · PR #...>
- **Basis-SHA:** <exakter Commit-SHA von main>
- **Ergebnis-SHA:** <exakter Ergebnis-Commit-SHA>
- **Implementierer:** <Rolle / Name>
- **Unabhängiger Reviewer (G6):** <offen | Rolle / Name — nicht identisch mit Implementierer>

## 1. Problemstellung & Zielumfang
- Welches Problem / welche Anforderung wird gelöst?
- Welche ADRs / Verträge gelten?
- Native-First-Prüfung (`N` / `H` / `X` gemäß docs/NATIVE_FIRST_REGISTER.md): ...

## 2. Durchgeführte Änderungen
- Geänderte / neue Dateien (Code, Migrationen, Konfiguration, Tests, Dokumentation)
- Neue oder geänderte API-Routen inkl. Zugriffsregel (`Access`)
- Datenbankmigrationen & aktualisierte `migrations/CHECKSUMS`

## 3. Tatsächlich ausgeführte Prüfungen (mit genauem Befehl und Ergebnis)
| Prüfung | Befehl | Status | Befund / Umfang |
|---|---|---|---|
| Gesamtprüfung (`check.sh`) | `KIS_TEST_DATABASE_URL=... scripts/check.sh` | bestanden / fehlgeschlagen / offen | ... |
| Negative Verbundtests (`NVT-*`) | ... | bestanden / offen | ... |
| Sicherheitsabnahme | ... | bestanden / offen | ... |
| Backup / Restore | ... | bestanden / nicht betroffen / offen | ... |
| Container-Rauchtest | ... | bestanden / nicht betroffen / offen | ... |

## 4. Quality-Gate-Checkliste (`G0`–`G7`)
- [ ] G0: Auftrag, Grenzen & Native-First geprüft
- [ ] G1: Architektur-, Modul- & Schema-Integrität geprüft
- [ ] G2: Sicherheit, Mandantenisolation & Negativtests erfüllt
- [ ] G3: Echte PostgreSQL-, Funktions- & Fehlerpfad-Tests bestanden
- [ ] G4: `scripts/check.sh` vollständig fehlerfrei
- [ ] G5: Dokumentation, Funktionslandkarte & Prüfstatus aktualisiert
- [ ] G6: Unabhängiges Review abgeschlossen (Pflicht bei Sicherheit/Kern)
- [ ] G7: Fach-, Betriebs- & Regulatorik-Freigabe (nur vor Pilot-/Produktivbetrieb)

## 5. Verbleibende Risiken, Abweichungen und offene Punkte
- Konkrete offene Punkte, Abhängigkeiten und ggf. begründete Ausnahmen (mit Verantwortlichem und Frist)

## 6. Ergebnisstatus
- [ ] Bereit für unabhängiges Review (`G6` offen)
- [ ] Technisch abgenommen (keine Produktivfreigabe)
- [ ] Blockiert (Grund angeben)
```

---

## Verzeichnis erstellter Abnahmeberichte

| Paket | Meilenstein | Datum | Status | Bericht |
|---|---|---|---|---|
| `EP-00` | `M00` | 24.11.2026 | Entwurf fertig — wartet auf unabhängiges Review (`G6`) & vollständige CI-Umgebung (`G4`) | [abnahme/EP-00.md](abnahme/EP-00.md) |
