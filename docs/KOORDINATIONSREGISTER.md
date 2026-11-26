# Zentrales Koordinations- und Arbeitspaketregister (`KIS`)

Stand: 26.11.2026 · Alleiniger schreibender Koordinator: **`google-neon`** · Ziel-Repository: `https://github.com/winconlin/KIS` · Geprüfter `origin/main`-HEAD (via GitHub-API am 26.11.2026): **`2d0e4479f91cba42f4ede2249488e3d73e12c094`** *(PR #9 gemerged, Issue #10 geschlossen, 0 offene PRs/Issues)*

> **Wichtiger Umgebungshinweis (Ehrlichkeitsgebot):** Der lokale Arbeitsbereich unter `/app/applet` ist ein Repository-Snapshot **ohne** `.git`-Verzeichnis, ohne `gh`-CLI und ohne GitHub-Schreibzugang (keine Tokens/SSH-Keys). Der öffentliche Zustand von `winconlin/KIS` wurde am **26.11.2026 lesend über die GitHub-API geprüft** und der aktuelle `origin/main`-Stand (`2d0e4479f9`) wurde über die Workspace-Synchronisation übernommen. Das Pushen nach GitHub erfolgt über die GitHub-Sync-Funktion der Entwicklungsumgebung oder gemäß [UEBERGABE_GIT_CI.md](UEBERGABE_GIT_CI.md).

---

## 1. Koordinations- und Reservierungsregeln (verbindlich)

1. **Alleiniger Schreiber des Aufgabenregisters:** `google-neon` verwaltet Zuweisungen, Dateibereiche, Vertrags- und Migrationsreservierungen. Andere Agenten (z. B. `hermes-qwen38`) fordern Pakete an und beginnen erst nach bestätigter Reservierung mit schreibenden Änderungen.
2. **Kein Selbst-Review (`G6`):** Wer ein Paket oder Architekturdokument erstellt hat, vergibt nicht selbst die unabhängige `G6`-Freigabe.
3. **Maximal ein aktiv schreibendes Arbeitspaket pro Agent.**
4. **Branch-Namensschema:** `agent/<agentenidentität>/<arbeitspaket-id>-<kurzbeschreibung>` (immer ausgehend vom aktuellen `origin/main`; niemals direkt auf `main`).
5. **Isolierte Agenten-Infrastruktur:** Eigener Git-Worktree/Checkout, eigene PostgreSQL-Testdatenbank (`KIS_TEST_DATABASE_URL`), eigene Docker-Compose-Projektnamen/Ports/Volumes.
6. **Status-Taxonomie:** `Geplant` · `Spezifiziert` · `In Bearbeitung` · `Blockiert` · `Im Review` · `Technisch integriert` · `Lokal getestet` · `Gegenstelle getestet` · `Fachlich abgenommen` · `Extern bestätigt` · `Betriebsfreigegeben`.

---

## 2. Exklusive Sperrtabelle gemeinsamer Ressourcen

Folgende Ressourcen dürfen **niemals** ohne aktive, bestätigte Einzelreservierung geändert werden:

| Gemeinsame Ressource | Aktiver Halter | Reservierungsgrund / Paket | Status | Hinweis |
|---|---|---|---|---|
| `docs/ROADMAP.md`, `docs/KOORDINATIONSREGISTER.md`, `docs/EP00_*`, `docs/NATIVE_FIRST_REGISTER.md`, `docs/THREAT_MODEL_VERBUND.md`, `docs/VERBUND_TESTMODELL.md`, `docs/vertraege/*`, `docs/ABNAHMEBERICHT.md`, `docs/abnahme/*.md`, `docs/UEBERGABE_GIT_CI.md` | **`google-neon`** | `EP-00` Architektur-, Konsistenz-, Vertrags- & Koordinationspaket + unabhängiges `G6`-Review zu PR #9 | **Aktiv (Im Review)** | Kein Eingriff in Rust-Code oder Migrationen |
| `AGENTS.md`, `docs/ENTERPRISE_BACKLOG.md`, `docs/ENTERPRISE_KIS.md`, `docs/TI_COMPLIANCE.md`, `docs/CLINICAL_WORKFLOWS.md` | *frei (geschützt)* | Verbindliche Auftragsdokumente | **Gesperrt gegen eigenmächtige Änderungen** | Anforderungen nicht entfernen oder abschwächen |
| `migrations/`, `migrations/CHECKSUMS` (nächste freie Nummer: **`012`**) | *frei (wartet auf `EP-00`-`G6`)* | Reserviert für `EP-01a` (`012_mandanten_organisation.sql`) | **Gesperrt bis `EP-00`-Freeze** | Immer nur **ein** aktiver Schreiber für Migrationen |
| `config/module_tables.toml`, `config/postgres/` | *frei (wartet auf `EP-00`-`G6`)* | Koordiniert mit `EP-01a` / `EP-06a` | **Gesperrt bis `EP-00`-Freeze** | — |
| `Cargo.toml`, `Cargo.lock` | *frei (Nachbesserung `AP-09b`)* | Koordiniert für `AP-09b`, `AP-08` oder neue Modul-Crates (`EP-06a`) | **Reserviert auf Antrag** | `AP-09b`: unbenutzte Dev-Dependency `kis-app` aus `crates/kis-doctors/Cargo.toml` + `Cargo.lock` entfernen |
| `crates/kis-core/`, `crates/kis-app/src/modules.rs` | *frei (wartet auf `EP-00`-`G6`)* | Kernverträge (`EP-01`–`EP-05`) | **Gesperrt bis `EP-00`-Freeze** | — |
| `.github/workflows/`, `scripts/check*.py`, `scripts/check.sh` | *frei* | CI & Guardrails | **Reserviert auf Antrag** | Keine Abschwächung von Prüfungen zulässig |

---

## 3. Aktive Agenten-Zuweisungen (Stand: 26.11.2026)

| Paket-ID | Fachlicher Umfang | Agent | Rolle | Basis-Commit | Vorgesehener Branch | Reservierte Dateien / Verzeichnisse | Erforderliche Migrationen | Abhängigkeiten | Status | PR-Link | Offene Blocker | Letzte Aktualisierung |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **`EP-00`** | Ist-Analyse, ADRs `ADR-001`–`007`, M01-Umsetzungsverträge (`EP-01`–`07`) & JSON-Fixtures, Native-First-Register, Threat Model, Verbund-Testmodell, Roadmap & Koordinationsregister | `google-neon` | Architektur & Koordination | `2d0e4479f9` (`origin/main`) | `agent/google-neon/ep00-architecture-coordination` | `docs/ROADMAP.md`, `docs/KOORDINATIONSREGISTER.md`, `docs/EP00_ARCHITEKTUR_UND_ANALYSE.md`, `docs/NATIVE_FIRST_REGISTER.md`, `docs/THREAT_MODEL_VERBUND.md`, `docs/VERBUND_TESTMODELL.md`, `docs/vertraege/*`, `docs/ABNAHMEBERICHT.md`, `docs/abnahme/*.md`, `docs/UEBERGABE_GIT_CI.md`, `docs/README.md`, `docs/FUNKTIONSLANDKARTE.md`, `docs/REVIEW_STATUS.md`, `scripts/check_vertraege.py` | keine | — | **Im Review** | *Push über GitHub-Sync / Git-Host* | (1) Push & Draft-PR auf GitHub; (2) Unabhängiges Gegenreview `G6` durch `hermes-qwen38`; (3) `G4` CI-Lauf | 2026-11-26 |
| **`EP-00-REV`** | **Unabhängiges Gegenreview (`G6`) von `EP-00`** (Prüfung ADRs `001`–`007`, ID-Abgleich, Threat Model, Native-First-Register, Verbund-Testmodell) | **`hermes-qwen38`** *(zugewiesen als unabhängiger Reviewer)* | Unabhängiges Review (lesend) | Basis von `EP-00`-PR | *(Review auf `EP-00`-PR; kein eigener Code-Branch nötig)* | lesend auf alle `EP-00`-Artefakte; Review-Bericht unter `docs/abnahme/EP-00-review.md` | keine | `EP-00` im Review | **Zugewiesen / Offen** | *nach `EP-00`-PR* | Bereitstellung des `EP-00`-PR auf GitHub | 2026-11-26 |
| **`AP-09b`** *(Post-Merge-Nachbesserung zu PR #9 / R-01)* | Entfernung der unbenutzten zirkulären `[dev-dependencies] kis-app` aus `crates/kis-doctors/Cargo.toml` + `Cargo.lock` sowie ungenutzter Helfer (`app!`) in `crates/kis-doctors/tests/common.rs` gemäß [abnahme/PR-09-review-list-doctors.md](abnahme/PR-09-review-list-doctors.md) | **`hermes-qwen38`** *(freigegeben für parallele Bereinigung auf Rust-Host)* | Implementierung | `2d0e4479f9` (`origin/main`) | `agent/hermes-qwen38/ap09b-remove-unused-kis-app-devdep` | `crates/kis-doctors/Cargo.toml`, `Cargo.lock`, `crates/kis-doctors/tests/common.rs` | keine | keine (erfordert Rust-Toolchain für `Cargo.lock`) | **Freigegeben für parallele Bearbeitung** | *offen* | Ausführung auf Rust-/PostgreSQL-Host + `scripts/check.sh` | 2026-11-26 |

*(Hinweis: PR #6 [`AP-01`], PR #8 [`ROADMAP`] und PR #9 [`AP-09` / `list_doctors`] sind auf `origin/main` `2d0e4479f9` gemerged; Issue #10 ist geschlossen. Aktuell bestehen 0 offene PRs und 0 offene Issues.)*

---

## 4. Vollständiges Arbeitspaket-Register (`M00`–`M07` & `AP-*`)

### 4.1 Unabhängige Bestandshärtung (parallel zu `M00`/`M01` zulässig)

| Paket-ID | Fachlicher Umfang | Abhängigkeit | Migration | Status | Geeignet für parallele Vergabe? |
|---|---|---|---|---|---|
| `AP-01` | Verlagerung Route `GET /api/v1/doctors/:id/schedule` nach `kis-appointments` (über PR #6 auf `origin/main` gemerged) | — | nein | Technisch integriert (PR #6) | Erledigt |
| `AP-09` | Fix `kis-doctors::list_doctors` (`$1`/`$2`/`cursor` parametrisiert) + 20 PG-Integrationstests (`G-13`, PR #9 @ `2d0e4479f9`, 5/5 CI-Checks grün) | — | nein | Technisch integriert (PR #9 · G6-Nachbesserung R-01 als `AP-09b`) | Erledigt (`AP-09b` offen) |
| `AP-09b` | Post-Merge-Bereinigung zu PR #9 (`R-01`): unbenutzte zirkuläre Dev-Dependency `kis-app` aus `crates/kis-doctors/Cargo.toml`/`Cargo.lock` + `app!`-Makro entfernen | Exklusive Reservierung `Cargo.lock` | nein | Freigegeben (`hermes-qwen38`) | Ja (auf Rust-Host) |
| `AP-02` | Browser-Session von `localStorage` auf `HttpOnly; Secure; SameSite=Strict`-Cookie + CSRF (Abstimmung mit `EP-04a`) | Koordination mit `EP-04a` | nein | Geplant | Ja (nach Reservierung `core_routes.rs` & `static/js/`) |
| `AP-05` | `GET /api/v1/metrics` im Prometheus-Format ohne PII (koordiniert mit `EP-07a`) | — | nein | Geplant | Ja (nach Reservierung `core_routes.rs`) |
| `AP-06` | Lasttestskript `scripts/loadtest.py` + `EXPLAIN`-Auswertung (koordiniert mit `EP-07b`) | — | nein | Geplant | Ja |
| `AP-08` | Evaluierung & Ablösung ungewarteter Crates (`chrono`, `rustls-pemfile`, `G-14`) | Exklusive Reservierung `Cargo.toml`/`Cargo.lock` | nein | Geplant | Ja (exklusiv auf Lockfile) |

### 4.2 P0-Plattform- und Verbund-Kernverträge (`M01` — erst nach `EP-00`-`G6`)

| Paket-ID | Fachlicher Umfang | Abhängigkeit | Geplante Migration | Status | Parallelisierbar mit |
|---|---|---|---|---|---|
| `EP-01a` | Schema & Kernvertrag `legal_entities`, `sites`, `org_identifiers`, `org_units.legal_entity_id`, `TenantContext` | `EP-00` (`G6`) | **`012`** (exklusiv) | Spezifiziert | `AP-09`, `AP-06` |
| `EP-01b` | `kis-org` mandantenfähig, MVZ-Kennungen, `devices`, `user_org_assignments`, `service_agreements` | `EP-01a` | optional `013` | Spezifiziert | `EP-01c`, `EP-06a` (ohne Migrationskonflikt) |
| `EP-01c` | Scoped Config (`verbund → legal_entity → site → unit`) & `policy_floor` | `EP-01a` | optional | Spezifiziert | `EP-01b` |
| `EP-06a` | `kis-catalogs`: `catalog_sources`, `catalog_versions`, `catalog_entries`, `normative_requirements`, `AP-14`-Import | `EP-00` (`G6`) | koordiniert nach `012` | Spezifiziert | `EP-01b`/`01c` (nach Migrationsnummer-Vergabe) |
| `EP-02a` | MPI-Schema, Identitätsräume, temporäre IDs (Notfall/Neugeborene), aktive AEAD-`*_enc` | `EP-01a` | koordiniert | Spezifiziert | `EP-04a` (getrennte Crates) |
| `EP-02b` | Mandantengetrennte Patientensuche, MPI-API & Kontakte (`AP-12`) | `EP-02a` | koordiniert | Spezifiziert | `EP-04a` |
| `EP-02c` | Dubletten-Arbeitsliste & auditierter Merge/Unmerge (inkl. Race-Schutz) | `EP-02b` | koordiniert | Spezifiziert | — |
| `EP-03a` | Fall-/Episoden-/Encounter-/Abrechnungs-/Kostenträger-Schema (`AP-13`) | `EP-01a`, `EP-02a` | koordiniert | Spezifiziert | — |
| `EP-03b` | Sektorübergreifende `kis-cases`-API (stationär, MVZ ohne Bett, Parallelfälle, Rechtsträgerwechsel) | `EP-03a` | nein | Spezifiziert | — |
| `EP-03c` | Mandanten-/Fallvertrag in Bestandsmodulen durchreichen | `EP-03b` | koordiniert | Spezifiziert | `EP-04c` |
| `EP-04a` | Admin-/Domänentrennung, MFA-Härtung, reale Proxy-IP, SSO-Adapter-Schnittstelle (`AP-02`) | `EP-01a`, `EP-01c` | koordiniert | Spezifiziert | `EP-02b` |
| `EP-04b` | 4-Faktoren-Zugriffsprüfung (`treatment.rs`, `middleware.rs`), `service_agreements`, Write-Pool | `EP-01b`, `EP-03b`, `EP-04a` | nein | Spezifiziert | — |
| `EP-04c` | Lese-Audit, Break-Glass-Governance, Audit-Partitionierung & WORM-Prüfanker | `EP-04b` | koordiniert | Spezifiziert | `EP-03c` |
| `EP-04d` | Retention-Matrix (`DEC-06`), Legal Hold, Mandanten-DSGVO JSON/CSV/FHIR (`AP-03`/`04`/`DEC-08`) | `EP-03b`, `EP-04b`, `DEC-06`/`08` | koordiniert | Blockiert durch `DEC-06`/`08` + `EP-04b` | — |
| `EP-05a` | Klasse-A-Sicherheitsinvarianten (Bettensperre, kritische Laborwerte, Modul-Schutzprüfung) | `EP-01a`, `EP-03b`, `EP-04b` | nein | Spezifiziert | `EP-06b` |
| `EP-05b` | Mandantenfähige Outbox, `event_inbox`, Quarantäne, Reconciliation, Replay-Audit | `EP-05a` | koordiniert | Spezifiziert | `EP-06b` |
| `EP-06b` | Stichtagsbindung ICD/OPS/UCUM/LOINC/ATC, Zeit-/Einheiten-/Statusvertrag in Fachmodulen | `EP-06a`, `EP-03c` | koordiniert | Spezifiziert | `EP-05a`/`05b` |
| `EP-07a` | Monitoring (`AP-05`), Schlüsselrotation, SBOM, Mandanten-Caching | `EP-01c`, `EP-04c`, `EP-05b` | nein | Spezifiziert | — |
| `EP-07b` | HA, Backup/DR, Downtime-Read-Only, Verbund-Lasttest (`AP-06`) | `EP-07a` | nein | Spezifiziert | — |

### 4.3 Folgepakete (`EP-08`–`EP-26`, `DOC-01`–`07`, `ED-01`–`06`)

Sämtliche Folgepakete `EP-08` bis `EP-26` (exakte IDs und Abhängigkeiten gemäß [ROADMAP.md](ROADMAP.md) § 4 und [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md)) stehen bis zum Abschluss der jeweils erforderlichen `M01`-Kernverträge auf Status **`Geplant`** (bzw. **`Spezifiziert`** für `DOC-01`–`07` und `ED-01`–`06` aus [CLINICAL_WORKFLOWS.md](CLINICAL_WORKFLOWS.md)). Ihre Feinzerlegung in Teilpakete erfolgt vor Vergabe durch den Koordinator.

---

## 5. Übergabe- und Reservierungsprotokoll für Agenten

Ein Agent ohne Schreibrecht auf das zentrale Koordinations-Issue fordert ein Paket per Issue-Kommentar mit folgendem Minimalformat an:

```text
RESERVIERUNGSANFRAGE
- Agent: <z. B. hermes-qwen38>
- Gewünschtes Paket: <Paket-ID + fachlicher Umfang>
- Basis-Commit (origin/main): <SHA>
- Vorgesehener Branch: agent/<agent>/<id>-<kurzbeschreibung>
- Benötigte Dateien/Verzeichnisse: <Liste>
- Benötigte Migration / gemeinsame Ressourcen: <keine | konkrete Liste>
- Lokale Toolchain geprüft: git / cargo / rustc / postgres / docker
```

Erst nach schriftlicher Bestätigung durch den Koordinator (`google-neon`) beginnt die schreibende Bearbeitung.

---

## 6. Vorlage zur Anlage des zentralen GitHub-Koordinations-Issues (auf GitHub-fähigem Host)

Die vollständige Übergabe-Checkliste (Branch, `crates/`-/`migrations/`-Sperrprüfung, `scripts/check.sh`, `scripts/check_vertraege.py`, Draft-PR und `G6`-Zuweisung an `hermes`) steht in [UEBERGABE_GIT_CI.md](UEBERGABE_GIT_CI.md). Sobald ein Host mit authentifizierter `gh`-CLI verfügbar ist, legt der Koordinator das zentrale Issue an:

```bash
gh issue create \
  --repo winconlin/KIS \
  --title "[Koordination] Zentrales Multi-Agenten-Arbeitspaketregister (EP-00 bis EP-26 / AP-*)" \
  --body-file docs/KOORDINATIONSREGISTER.md
```
