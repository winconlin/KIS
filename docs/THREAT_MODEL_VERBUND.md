# Bedrohungsmodell Klinikverbund, MVZ & Zentrale Dienste

Stand: 24.11.2026 (konsistenzbereinigte Fassung v2) · Arbeitspaket: `EP-00` · Autor: `google-neon` · Status: **Im Review (`G6` offen)** · Bezug: [SECURITY.md](SECURITY.md), [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md), [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md), [CLINICAL_WORKFLOWS.md](CLINICAL_WORKFLOWS.md)

Dieses Dokument analysiert die Sicherheits- und Patientensicherheitsrisiken beim Ausbau von der Ein-Haus-Grundlage zu einer mandantenfähigen Verbundplattform mit getrennten Rechtsträgern, Schwesterkliniken, MVZ, zentralen Diensten, klinischen Dokumenten-/Notaufnahme-Workflows und TI-Schnittstellen. Alle konkreten Ist-Befunde beruhen auf der Code- und Schema-Analyse in [EP00_ARCHITEKTUR_UND_ANALYSE.md](EP00_ARCHITEKTUR_UND_ANALYSE.md).

---

## 1. Vertrauensgrenzen (Trust Boundaries)

```text
[TB-1: Browser / Klinik-Endgerät / MVZ-Arbeitsplatz / Patientenportal / Großbild-Warteraum]
   |  TLS 1.3 (HttpOnly-Cookie + CSRF / mTLS / Smartcard-Terminal)
   v
[TB-2: Reverse-Proxy / Ingress (nginx)]
   |  Vertrauenswürdige Client-IP (nur konfigurierte Proxy-Grenze), strikte CSP/Header
   v
[TB-3: KIS-Applikationsserver (kis-app, DB-Rolle kis_app)]
   |  +-- TB-3a: Mandantengrenze (legal_entity_id: RT-NORD vs. RT-SUED)
   |  +-- TB-3b: Standort-/Sektorgrenze (Akutklinik vs. MVZ vs. Zentraldienst vs. Portal)
   |  +-- TB-3c: Zweck- & Behandlungszusammenhangsgrenze (TreatmentContext / Break-Glass)
   |  +-- TB-3d: Domänentrennung (Klinik vs. platform_admin/tenant_admin vs. HR vs. Finanzen vs. CIRS vs. Forschung)
   |  +-- TB-3e: Modulgrenze (Tabellen-Eigentum gemäß config/module_tables.toml)
   v
[TB-4: PostgreSQL (kis_app Laufzeit vs. kis_owner Migrationen)]
   |  Append-Only Audit-Trigger, Constraints, AEAD-Feldverschlüsselung + Key-Rotation
   v
[TB-5: Externe Netze, Geräte & Dienste]
   +-- TI / Konnektor / TI-Gateway (mTLS, SMC-B, eHBA, eGK)
   +-- Fremdsysteme & Medizingeräte (HL7 / FHIR / DICOM / IHE / xDT / MEDICO-Koexistenz)
   +-- Optionale lokale/genehmigte KI-/ASR-Modelle (keine ungenehmigte PHI-Weitergabe)
   +-- Externer Audit-Prüfanker (WORM) & verschlüsselte Backups (age)
```

---

## 2. Angreiferprofile

| Profil | Beschreibung | Relevante Fähigkeiten |
|---|---|---|
| **A1 — Mandantenfremder Beschäftigter** | Regulär authentifizierter Mitarbeiter aus Rechtsträger A (`RT-NORD`) | Gültige Sitzung in `RT-NORD`; kennt oder errät UUIDs/Fallnummern/KVNR aus Rechtsträger B (`RT-SUED`) |
| **A2 — Neugieriger Beschäftigter im selben Rechtsträger** | Authentifizierter Mitarbeiter derselben Klinik ohne Behandlungszusammenhang | Versucht direkten Fall-, Patienten-, Dokument-/OCR-, Such- oder Exportzugriff oder Missbrauch von Break-Glass |
| **A3 — Privilegierter IT-/Plattform- oder Bereichs-Administrator** | Betreiber, Helpdesk-, HR- oder Finanz-Admin | Versucht über globale Adminrolle, Settings-Änderung (`treatment_context.enforce=false`), HR-/Finanz-/CIRS-Konten oder Rollenmanipulation klinische Daten einzusehen |
| **A4 — Kompromittiertes Schnittstellen-, Zentraldienst- oder Portal-Konto** | Servicekonto eines zentralen Labors (`ZD-LAB`), HL7-/FHIR-Adapters, Event-Consumers oder Patientenportal-Kontos | Legitimer Auftragskanal, versucht mandantenfremde Akten abzurufen, fremde Events zu konsumieren, Replay oder Prompt-Injection |
| **A5 — Externer Angreifer / XSS / Session-Hijacking** | Angreifer im lokalen Netz oder über kompromittiertes Browser-Script / manipulierte Upload-Datei | Versucht Token-Diebstahl (`localStorage`), CSRF, Rate-Limit-Umgehung, SQL-/Filter-Injection, Malware-Upload ins DMS |
| **A6 — Datenbank-/Infrastruktur-Administrator** | Zugriff auf DB-Host, Backups oder Logs | Versucht Auslesen sensibler Patientenfelder, stille Manipulation/Abschneiden von `audit_logs` oder Umgehen von Legal Holds |

---

## 3. Bedrohungsmatrix (`T01`–`T24`)

| ID | Bedrohung (STRIDE) | Angreifer | Ist-Befund / Risiko | Ziel-Gegenmaßnahme | Zielpaket | Pflicht-Nachweis (`NVT` / `SICHERHEITS_ABNAHME`) |
|---|---|---|---|---|---|---|
| **T01** | **Mandantenübergreifender Datenabfluss via Org-Baum** *(Disclosure)* | A1, A3 | `org_units` ohne `legal_entity_id`; `MY_UNITS` (`treatment.rs`) traversiert ab `traeger` rekursiv über alle Häuser; `GET /api/v1/org/units` global. | Harte `legal_entity_id`-Grenze, zyklenfreier DB-Constraint gegen rechtsträgerübergreifende Kanten, serverseitiger `TenantContext`. | `EP-01` | `NVT-01`, `NVT-02` |
| **T02** | **Globaler Patienten- und Fall-IDOR** *(Disclosure / Tampering)* | A1, A2 | `GET /api/v1/patients`, `/patients/:id`, `/cases?caseNumber=`, `/patients/:id/cases` laufen mit `Scope::None`. | Mandanten-Scoping, zweistufiges MPI (`ADR-002`), verpflichtende Behandlungszusammenhangs-/Zweckprüfung auf allen Fall- und Suchrouten. | `EP-02`, `EP-03`, `EP-04` | `NVT-05`, `NVT-07`, `NVT-08` |
| **T03** | **Klinischer Datenzugriff durch IT-/Global-Admin oder HR/Finanzen** *(Elevation)* | A3 | `RoleMatrix::effective` (`rbac.rs`) gewährt `admin` pauschal `PermissionSet::all()`. | Rollenaufteilung `platform_admin` / `tenant_admin` / `privacy_auditor` / `hr_manager` / `finance_clerk` ohne pauschales klinisches Leserecht (`ADR-004`). | `EP-04a`, `EP-19`, `EP-20` | `NVT-03`, `A-09`, `A-10` |
| **T04** | **Abschalten rechtlicher Mindestschutzregeln zur Laufzeit** *(Defense Evasion)* | A3 | `security.treatment_context.enforce` (`platform.rs`) kann auf `false` gesetzt werden. | Unveränderliche Schutzklasse (`policy_floor`); Modulabschaltung darf Schutzfunktionen nie still deaktivieren. | `EP-01c`, `EP-04a`, `EP-05` | `NVT-04`, Szenario 7 |
| **T05** | **MPI-Missbrauch als Datenbrücke zwischen Rechtsträgern** *(Disclosure)* | A1, A2 | Globale Tabelle `patients` ohne Trennung von Identitätsindex und rechtsträgerlokaler Akte. | Zweistufiges MPI + Betriebsoption getrennter Identitätsräume (`ADR-002`); MPI-Suche minimal und lesend auditiert. | `EP-02` | `NVT-05`, `NVT-06`, `D-03` |
| **T06** | **Fehlzuordnung / Verwechslung bei Merge, Notfallpatienten oder Neugeborenen** *(Patient Safety)* | A2 | `birth_date NOT NULL` erzwingt fiktive Daten; kein Merge/Unmerge-Workflow. | `unknown_emergency`/`newborn_provisional`, kein Auto-Merge nach Name+Geburtsdatum, geprüfter Merge/Unmerge mit Sperre bei offenen kritischen Prozessen. | `EP-02c`, `EP-16` | `NVT-06`, `D-05`, Merge-Race-Test |
| **T07** | **Zentraldienst-Überzugriff (z. B. Zentrallabor sieht fremde Akten)** *(Elevation)* | A4 | Kein Konzept für `service_agreements`; Auftragszugriff nur über einheitliche `org_units`. | Zeitgebundene `service_agreements` je Rechtsträger; Zugriff strikt auf aktive Auftragsfälle des jeweiligen Mandanten. | `EP-01b`, `EP-04b` | `NVT-09` |
| **T08** | **Unbemerkter Lesezugriff & Break-Glass-Missbrauch** *(Repudiation / Disclosure)* | A2 | Lesezugriffe auf Patienten/Fälle/Akten aktuell nicht auditiert; Break-Glass ohne Eskalation. | Verpflichtendes Lese-Audit (mit Mandant & Zweck, ohne PII-Volldump); befristetes Break-Glass mit mandantengetrennter Nachprüfung. | `EP-04c` | `NVT-10`, `NVT-11`, `L-05` |
| **T09** | **DSGVO-Export über Rechtsträgergrenzen / Replica-Lag** *(Disclosure)* | A1, A3 | `export_patient` (`data_portability.rs`) liest von `db.read()` und exportiert global alle Fälle. | Zwingende Ausführung auf `db.write()`, strikte Mandantenbindung, vollständige JSON/CSV/FHIR-Abdeckung (`AP-03`). | `EP-04d` | `NVT-12`, `A-14` |
| **T10** | **Unzulässige Löschung trotz Aufbewahrungspflicht / Legal Hold** *(Tampering)* | A3 | `retention_check()` prüft nur `appointments.start_time`; keine Legal Holds; Freitexte bleiben bei Anonymisierung. | Vollständige Retention-Matrix (`DEC-06`), `legal_holds`-Sperre, abgestimmte Freitext-Anonymisierung (`AP-04`/`DEC-08`). | `EP-04d` | `NVT-13`, `D-08` |
| **T11** | **Sitzungs-/Token-Diebstahl & fehlende MFA-Härtung** *(Spoofing)* | A5, A6 | Frontend nutzt `localStorage` (`AP-02`); `users.mfa_secret` Klartext; MFA-Setup fehlt; `core_routes.rs` übergibt `ip = None`. | `HttpOnly; Secure; SameSite=Strict`-Cookies + CSRF (`AP-02`), AEAD-verschlüsseltes `mfa_secret`, MFA-Enrollment, reale Proxy-IP (`F-03`). | `EP-04a`, `AP-02` | `A-01`–`A-14`, `F-02`, `F-03` |
| **T12** | **Klartext-PII in der DB trotz `*_enc`-Spalten** *(Disclosure)* | A6 | `kis-patients` liest/schreibt Klartextspalten statt `*_enc`. | Aktive AEAD-Verschlüsselung mit `key_version` (Rotation) und Blind-Index; Klartextpfade ablösen. | `EP-02a`, `EP-07` | DB-Rohinspektionstest, `B-07` |
| **T13** | **Manipulation/Abschneiden der Audit-Kette durch DB-Admin** *(Repudiation)* | A6 | Hash-Kette ohne externen Prüfanker; keine Partitionierung. | Signierter Root-Hash-Export an externen WORM-Prüfanker + monatliche Partitionierung + Verifikationstest. | `EP-04c`, `EP-07` | `L-01`–`L-09`, `P-01`–`P-03` |
| **T14** | **Stilles Scheitern sicherheitskritischer Folgeaktionen** *(Patient Safety)* | System / A4 | `EventBus::publish` schluckt Fehler im Savepoint; Bettensperre bei Entlassung läuft best-effort; `resolve_event_failure` ohne Audit. | Klasse-A-Sicherheitsinvarianten (atomar / sicherer Sperrzustand); Klasse-B-Outbox/Inbox mit Quarantäne, Reconciliation und auditiertem Replay. | `EP-05` | `NVT-14`, Fehler-Injektionstest |
| **T15** | **Mandantenübergreifende Ereignis-/Outbox-Vermischung** *(Disclosure)* | A4 | `domain_events` und `event_consumers` ohne `legal_entity_id`. | Pflichtfeld `legal_entity_id` auf Events und Consumern; strenge Mandantenfilterung + Deduplizierung. | `EP-05b` | `NVT-15`, Szenario 3 |
| **T16** | **Cache-, Export-, BI- oder Hintergrundjob-Leck zwischen Mandanten** *(Disclosure)* | A1, A4 | Risiko bei Berichten, Brief-Rendering, Caches, BI und Forschungszonen. | `legal_entity_id` Pflichtpräfix für jeden Cache-Key, Job und Export; getrennte Forschungs-/Pseudonymisierungszone. | `EP-01`, `EP-07`, `EP-23` | `NVT-16` |
| **T17** | **SQL-Parameterfehler & ungebundene Filter-Interpolation** *(DoS / Injection)* | A5 | `kis-doctors/src/lib.rs` (`list_doctors`) bindet `$1`/`$2` nicht (`500`) und interpoliert `cursor`; `kis-patients` interpoliert `cursor`. | Strikte Parametrisierung aller SQL-Werte über `sqlx::QueryBuilder` / `.bind()`; Regressionstests. | **`AP-09`**, `EP-02a` | `D-09`, Filter-Regressionstests |
| **T18** | **Kompromittierung externer Schnittstellen / TI-Komponenten** *(Spoofing / Tampering)* | A4, A5 | Entwürfe in `entwurf/` nicht im Build; keine Gegenstellenvalidierung. | mTLS, QES/SMC-B-Prüfung, gepinnte FHIR-Profile, mandantenbezogener Kontext, ACK/Retry/Quarantäne. | `EP-08`, `EP-09`, `EP-24` | Szenario 6, RU-Tests |
| **T19** | **DMS-Upload-Malware, OCR-Suchleck & Weiterverwendung veralteter Signaturen** *(Tampering / Disclosure)* | A2, A5 | Dokumenten-/Workflow-Plattform (`DOC-01`–`07`) noch offen. | Quarantäne/Malwareprüfung, MIME-/Größenlimits, objektbezogene ACL auf Suche/OCR/Vorschau, unveränderliche Revisionen, Signatur-Invalidierung bei Nachbearbeitung. | `EP-12`, `EP-21`, `DOC-01`–`06` | `NVT-17`, `CLINICAL_WORKFLOWS` § 6 |
| **T20** | **Barcode-, Medikations- oder Blutprodukt-Verwechslung** *(Patient Safety)* | Klinischer Fehler | Allergieabgleich aktuell rein textuell; keine BCMA-/Blutbank-Sperren. | Atomare Identitäts-/Barcode-Prüfung, geprüfte AMTS-Datenbasis (`DEC-02`), separate sicherheitskritische Transfusions-/Kreuzproben-Sperre. | `EP-13`, `EP-14` | Falsch-Patient-/Falsch-Barcode-Tests (Szenario 6) |
| **T21** | **CIRS-Deanonymisierung oder Vermischung mit Patientenakte** *(Privacy / Retaliation)* | A2, A3 | CIRS (`E18`) noch offen. | Logisch und rechtlich getrennte CIRS-Identität und Tabellen ohne Verknüpfungspflicht zur Patientenakte; eigene ACL. | `EP-21` | CIRS-Trennungs-Negativtest |
| **T22** | **Prompt-Injection & ungenehmigter PHI-Abfluss an KI-Modelle** *(Disclosure / Tampering)* | A4, A5 | Optionale KI-Entwürfe (`E19`/`DOC-07`) noch offen. | Kein Versand von PHI an nicht genehmigte Modelle, strikte System-/Kontexttrennung, Quellen-/Modellversionspflicht, verpflichtende menschliche Freigabe. | `EP-23`, `DOC-07` | `NVT-18`, Prompt-Injection-Tests |
| **T23** | **Offenlegung von Namen/Diagnosen auf Großbild- oder Warteraumanzeigen** *(Disclosure)* | Physisch / A5 | ZNA-Live-Board & Patientenleitsystem (`ED-02`/`ED-06`) noch offen. | Pseudonyme Aufrufnummern, minimierte Großbildansichten ohne Diagnosen/Klarnamen, automatisches Reconnect + Veraltungsanzeige. | `EP-16` (`ED-02`/`06`) | ZNA-Warteraum-Prüfung |
| **T24** | **Datenverlust oder Konflikt bei TI-/Netz-Downtime & Wiederanlauf** *(Availability / Integrity)* | Infrastruktur | Kein klinikspezifisches Downtime-/Reconciliation-Verfahren im Kern. | Freigegebener Read-Only-Notfallmodus, Offline-Schreiben nur nach expliziter Konfliktanalyse, idempotenter Wiederanlauf ohne Doppelversand. | `EP-07`, `EP-09`, `EP-16` | Szenario 5, `B-01`–`B-07` |

---

## 4. Sicherheitskritische klinische Invarianten (niemals Best-Effort, `ADR-005`)

1. **Richtiger Patient / Identitätssicherheit** (inkl. Merge-Sperre bei laufenden kritischen Prozessen, Barcode-Verifikation).
2. **Bettensperre und Hygienefreigabe** bei Entlassung, Verlegung oder Isolation.
3. **Kritische Labor-, Pathologie- und ZNA-Befunde** (inkl. Nachbefunden nach Entlassung) mit Pflichtquittierung und Eskalation.
4. **Verordnete/abgegebene Dosis, Allergie-, AMTS- und Blutprodukt-/Transfusionssperren**.
5. **Einwilligungsstatus** vor invasiven/präoperativen Eingriffen und **Signatur-Unveränderlichkeit** bei freigegebenen Dokumenten.
6. **Entlassung + Neuaufnahme bei Rechtsträgerwechsel** (keine halbfertige rechtsträgerübergreifende Verlegung).
7. **Verpflichtendes Audit** bei Mutationen und sensiblen Lesezugriffen (Fail-Closed).

---

## 5. Review- und Freigabepflicht

Jede Umsetzung eines in Abschnitt 3 adressierten Pakets erfordert echte PostgreSQL-Integrationstests inklusive der zugeordneten Negativtests, einen ausgefüllten [Abnahmebericht](ABNAHMEBERICHT.md) und ein **unabhängiges Sicherheitsreview (`G6`)** durch eine nicht-implementierende Instanz.
