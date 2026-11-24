# Synthetisches Verbund-Testmodell (`EP-00`)

Stand: 24.11.2026 · Arbeitspaket: `EP-00` · Datenschutz-Regel: **Ausschließlich synthetische Testdaten** ([AGENTS.md](../AGENTS.md) § 5)

Dieses Modell definiert die verbindliche Referenz-Mandantenlandschaft für alle PostgreSQL-Integrationstests, Berechtigungsnachweise und negativen Verbundtests ab `EP-01`. Sämtliche Namen, Kennungen, Adressen, IK-/BSNR-/LANR-/KVNR-Nummern sind **frei erfunden** und dürfen niemals aus Produktivsystemen stammen.

---

## 1. Verbundtopologie (2 Rechtsträger, 4 Akutkliniken, 3 MVZ, 3 zentrale Dienste)

```text
Verbund-Plattform: „Testverbund Gesundheit“ (rein organisatorische Plattform, KEIN Daten-Freifahrtschein)
|
+-- Rechtsträger 1: RT-NORD („Nordverbund Kliniken gGmbH“, legal_entity_id: 10000000-0000-4000-8000-000000000001)
|   +-- KL-N1: Klinikum Nordstadt (Maximalversorger, IK 260000101)
|   |   +-- ZNA-N1 (Notaufnahme)
|   |   +-- INN-N1 (Fachabteilung Innere Medizin -> Station N1-S1, Betten N1-S1-01..04)
|   |   +-- CHI-N1 (Fachabteilung Chirurgie -> Station N1-S2, OP-N1)
|   |   +-- ITS-N1 (Intensivstation)
|   +-- KL-N2: Krankenhaus Heideblick (Grund-/Regelversorger, IK 260000102)
|   |   +-- INN-N2 (Fachabteilung Innere -> Station N2-S1)
|   +-- MVZ-N1: MVZ Nordstadt Innere/Kardiologie (Hauptbetriebsstätte BSNR 990001100)
|   |   +-- NBS-N1a: Nebenbetriebsstätte Hafen (NBSNR 990001101)
|   +-- MVZ-N2: MVZ Heideblick Chirurgie/Orthopädie (Hauptbetriebsstätte BSNR 990001200)
|
+-- Rechtsträger 2: RT-SUED („Südregion Gesundheits-AG“, legal_entity_id: 10000000-0000-4000-8000-000000000002)
|   +-- KL-S1: Klinikum Südtal (Schwerpunktversorger, IK 260000201)
|   |   +-- ZNA-S1 (Notaufnahme)
|   |   +-- INN-S1 (Fachabteilung Innere -> Station S1-S1)
|   |   +-- RAD-S1 (Radiologie)
|   +-- KL-S2: Klinik Bergfried (Fachklinik Psychiatrie/Psychosomatik, IK 260000202)
|   |   +-- PSY-S2 (Fachabteilung Psychiatrie -> Station S2-P1)
|   +-- MVZ-S1: MVZ Südtal Neurologie/Psychiatrie (Hauptbetriebsstätte BSNR 990002100)
|       +-- NBS-S1a: Nebenbetriebsstätte Talblick (NBSNR 990002101)
|
+-- Zentrale Dienste (mit expliziten service_agreements je Rechtsträger)
    +-- ZD-LAB: Verbund-Zentrallabor (Vertrag mit RT-NORD und RT-SUED, Zweck: nur aktive Laboraufträge)
    +-- ZD-IT:  Zentraler Plattformbetrieb (Rolle platform_admin, KEIN klinisches Leserecht)
    +-- ZD-ABR: Zentrale Abrechnungsstelle (getrennte Mandanten-Konten je RT-NORD / RT-SUED)
```

---

## 2. Synthetische Testbenutzer und Rollen

| Kürzel | Synthetische Kennung | Rechtsträger | Einheit / Standort | Rolle(n) | Zweck im Testmodell |
|---|---|---|---|---|---|
| `U-PLAT` | `platform.ops@test.invalid` | Plattform (`ZD-IT`) | — | `platform_admin` | Technischer Betrieb; darf **keine** Patientendaten/Fälle lesen (`NVT-03`) |
| `U-ADM-N` | `admin.nord@test.invalid` | `RT-NORD` | `RT-NORD` | `tenant_admin` | Mandanten-Admin Nord; kein Zugriff auf `RT-SUED`, kein klinisches Leserecht (`NVT-01`, `NVT-03`) |
| `U-ADM-S` | `admin.sued@test.invalid` | `RT-SUED` | `RT-SUED` | `tenant_admin` | Mandanten-Admin Süd; strikt isoliert von `RT-NORD` |
| `U-AUD-N` | `audit.nord@test.invalid` | `RT-NORD` | `RT-NORD` | `privacy_auditor` | Audit- & Break-Glass-Prüfung ausschließlich für `RT-NORD` |
| `U-DOC-N1` | `dr.mertens@test.invalid` | `RT-NORD` | `INN-N1` / `N1-S1` | `arzt` | Stationsärztin Nordstadt (`LANR 990010101`); darf nur eigene Fälle in `RT-NORD` sehen |
| `U-DOC-N2` | `dr.keller@test.invalid` | `RT-NORD` | `INN-N2` / `N2-S1` | `arzt` | Stationsarzt Schwesterklinik Heideblick (selber Rechtsträger, anderer Standort) |
| `U-MVZ-N1` | `dr.vogt@test.invalid` | `RT-NORD` | `MVZ-N1` / `NBS-N1a` | `arzt`, `mvz_behandler` | MVZ-Arzt Nordstadt (`LANR 990010202`, ambulant) |
| `U-PFL-N1` | `pflege.n1@test.invalid` | `RT-NORD` | `N1-S1` | `pflege` | Pflegekraft Station `N1-S1` |
| `U-DOC-S1` | `dr.lindner@test.invalid` | `RT-SUED` | `INN-S1` / `S1-S1` | `arzt` | Stationsarzt Südtal (`RT-SUED`); darf **nichts** aus `RT-NORD` sehen |
| `U-DOC-S2` | `dr.falk@test.invalid` | `RT-SUED` | `PSY-S2` / `S2-P1` | `arzt` | Fachärztin Psychiatrie Bergfried (`RT-SUED`, besonders sensible Daten) |
| `U-LAB-Z` | `labor.zentral@test.invalid` | `ZD-LAB` | `ZD-LAB` | `funktionsdienst` | Zentrales Labor; sieht nur aktive Aufträge je Vertrag (`NVT-09`) |
| `U-EXT-N` | `export.nord@test.invalid` | `RT-NORD` | `RT-NORD` | `datenschutz_export` | DSGVO-Auskunft ausschließlich für `RT-NORD` (`NVT-12`) |

---

## 3. Synthetische Patienten- und Fall-Szenarien

| Szenario | Synthetischer Patient | Konstellation | Getestete Invariante |
|---|---|---|---|
| **PAT-01** | „Muster, Anna“ (`1984-05-12`, synthetische KVNR `T000000001`) | Gleichzeitig stationärer Fall in `KL-N1` (`RT-NORD`) **und** ambulanter MVZ-Fall in `MVZ-N1` (`RT-NORD`) | Zwei parallele Fälle (stationär + MVZ) im selben Rechtsträger (`EP-03`) |
| **PAT-02** | „Beispiel, Bernd“ (`1972-11-03`, synthetische KVNR `T000000002`) | Gemeinsamer MPI-Eintrag, **aber** je ein aktiver stationärer Fall in `KL-N1` (`RT-NORD`) und in `KL-S1` (`RT-SUED`) | Zwei parallele stationäre Fälle in getrennten Rechtsträgern ohne Quersicht (`EP-02`, `EP-03`, `NVT-05`, `NVT-07`) |
| **PAT-03** | „Unbekannt-ZNA-01“ (`identity_status = unknown_emergency`, `birth_date = NULL`) | Notfallaufnahme in `ZNA-N1`, anschließender Merge auf identifizierten Datensatz „Testfall, Clara“ | Kein fiktives Geburtsdatum; auditierter Merge/Unmerge (`EP-02c`, `ED-01`) |
| **PAT-04** | „Verlegung, David“ (`1961-02-20`) | (a) Interne Verlegung `KL-N1` → `KL-N2` (selber Rechtsträger `RT-NORD`), (b) Überleitung `KL-N2` (`RT-NORD`) → `KL-S1` (`RT-SUED`) | Intern = Verlegung; Rechtsträgerwechsel = zwingende Entlassung + Neuaufnahme (`ADR-003`) |
| **PAT-05** | „Psych-Sensibel, Eva“ (`1990-08-15`) | Stationärer Fall in `KL-S2` (`PSY-S2`, `RT-SUED`) mit aktivem `legal_hold` | Strikte Isolation gegen `RT-NORD`, gegen `KL-S1` ohne Behandlungszusammenhang und Löschsperre trotz Ablaufdatum (`NVT-08`, `NVT-13`) |
| **PAT-06** | „Auftrag-Labor, Frank“ (`1955-09-01`) | Stationärer Fall in `KL-N1` mit aktivem Laborauftrag an `ZD-LAB`; nach Abschluss + kritischem Kaliumwert | Temporärer Auftragszugriff für `ZD-LAB` erlischt nach Abschluss; kritischer Wert erzeugt Klasse-A-Pflichtquittierung (`NVT-09`, `NVT-14`) |

---

## 4. Verbindlicher Katalog negativer Verbundtests (`NVT-01`–`NVT-16`)

Jedes Paket `EP-01` bis `EP-07` ist erst abgeschlossen, wenn die ihm zugeordneten Negativtests gegen echtes PostgreSQL implementiert und bestanden sind:

| Test-ID | Paket | Szenario (synthetisch) | Erwartetes Ergebnis |
|---|---|---|---|
| **NVT-01** | `EP-01` | `U-ADM-N` oder `U-DOC-N1` fragt `GET /api/v1/org/units` oder direkte `unit_id` aus `RT-SUED` ab | `403` bzw. nur Einheiten von `RT-NORD`; keine Metadaten aus `RT-SUED` |
| **NVT-02** | `EP-01` | Versuch, `org_unit` aus `RT-SUED` unter Elterneinheit aus `RT-NORD` einzuhängen oder `MY_UNITS` über `traeger` auf fremde Rechtsträger auszuweiten | DB-Constraint/API weist mit `400`/`403` ab; kein Zugriff |
| **NVT-03** | `EP-04a` | `U-PLAT` (`platform_admin`) oder `U-ADM-N` (`tenant_admin`) ruft `GET /api/v1/patients/:id` oder `GET /api/v1/cases/:id` auf | `403 Forbidden` (Adminrolle ist kein klinisches Leserecht) |
| **NVT-04** | `EP-01c` | `U-PLAT` oder `U-ADM-N` versucht `security.treatment_context.enforce = false` oder Abschaltung der Mandantenprüfung via Settings-API | `400`/`403` (unveränderliche Schutzklasse `policy_floor`) |
| **NVT-05** | `EP-02` | `U-DOC-N1` sucht `PAT-02` über MPI/KVNR und versucht, dessen lokale Akte oder Fälle in `RT-SUED` zu lesen | Nur lokale `RT-NORD`-Akte sichtbar; `RT-SUED`-Fälle verborgen (`403`/`404`); Suche wird auditiert |
| **NVT-06** | `EP-02c` | Versuch, zwei Patientenakten aus unterschiedlichen Rechtsträgern (`RT-NORD` + `RT-SUED`) per Merge zusammenzuführen oder Merge bei laufender Transfusions-/Abrechnungssperre | `400`/`409` Ablehnung + Audit-Eintrag |
| **NVT-07** | `EP-03` | `U-DOC-N1` ruft `GET /api/v1/cases?caseNumber=<Fallnummer aus RT-SUED>` oder direkte `case_id` von `PAT-02` in `RT-SUED` auf | `403`/`404`; kein Existenz-Leak fremder Fallnummern |
| **NVT-08** | `EP-04b` | `U-DOC-S1` (`KL-S1`, `RT-SUED`) ruft Fall `PAT-05` in `KL-S2` (`PSY-S2`, gleicher Rechtsträger, aber anderer Standort ohne Behandlungszusammenhang) auf | `403 Forbidden` ohne gültigen Behandlungszusammenhang/Break-Glass |
| **NVT-09** | `EP-04b` | `U-LAB-Z` (`ZD-LAB`) versucht Zugriff auf (a) Fall ohne Auftrag in `RT-SUED` oder (b) Fall `PAT-06` nach Erledigung des Auftrags | `403 Forbidden` in beiden Fällen |
| **NVT-10** | `EP-04c` | `U-DOC-N1` liest Patientenstamm und Fallakte von `PAT-01`; Prüfung von `audit_logs` | Lese-Audit-Einträge mit `legal_entity_id`, `purpose_of_use`, ohne PII-Volldump vorhanden |
| **NVT-11** | `EP-04c` | `U-DOC-S1` löst Break-Glass auf `PAT-05` aus; Ablauf der Frist und Prüfung der Nachprüfungs-Queue durch `U-AUD-N` (fremder Rechtsträger) | Break-Glass nach Frist automatisch gesperrt; `U-AUD-N` darf `RT-SUED`-Break-Glass nicht einsehen (`403`) |
| **NVT-12** | `EP-04d` | `U-EXT-N` startet DSGVO-Export für `PAT-02` (hat Fälle in `RT-NORD` und `RT-SUED`) | Export enthält ausschließlich Daten von `RT-NORD` (JSON/CSV/FHIR), gelesen über Write-Pool |
| **NVT-13** | `EP-04d` | Versuch, `PAT-05` trotz aktivem `legal_hold` oder laufender §-630f-Aufbewahrungsfrist zu löschen/anonymisieren | `409 Conflict` / Ablehnung mit Verweis auf Sperrgrund; Daten unverändert |
| **NVT-14** | `EP-05a` | Fehler-Injektion bei Entlassung von `PAT-01` (Störung des nachgelagerten Auftrags-Subscribers) | Entlassung lässt Bett **niemals** frei: entweder atomarer Sperrzustand `cleaning_required` oder vollständiger Transaktions-Rollback |
| **NVT-15** | `EP-05b` | Event-Consumer von `RT-NORD` fragt `domain_events` mit Cursor ab, während Events für `RT-SUED` anfallen | Ausschließlich Events mit `legal_entity_id = RT-NORD`; `RT-SUED`-Events vollständig ausgeblendet |
| **NVT-16** | `EP-07` | Parallele Berichts-/Export-/Cache-Anfragen für `RT-NORD` und `RT-SUED` unter identischen Filterparametern | Strikte Mandantentrennung im Cache-Schlüssel und Job-Ergebnis; keine Kreuzkontamination |
