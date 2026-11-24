# Bedrohungsmodell Klinikverbund, MVZ & Zentrale Dienste

Stand: 24.11.2026 · Arbeitspaket: `EP-00` · Bezug: [SECURITY.md](SECURITY.md), [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md), [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md)

Dieses Dokument analysiert die Sicherheits- und Patientensicherheitsrisiken beim Ausbau von der Ein-Haus-Grundlage zu einer mandantenfähigen Verbundplattform mit getrennten Rechtsträgern, Schwesterkliniken, MVZ, zentralen Diensten und TI-Schnittstellen. Alle konkreten Ist-Befunde beruhen auf der Code- und Schema-Analyse in [EP00_ARCHITEKTUR_UND_ANALYSE.md](EP00_ARCHITEKTUR_UND_ANALYSE.md).

---

## 1. Vertrauensgrenzen (Trust Boundaries)

```text
[TB-1: Browser / Klinik-Endgerät / MVZ-Arbeitsplatz]
   |  TLS 1.3 (+ perspektivisch HttpOnly-Cookie / mTLS / Smartcard-Terminal)
   v
[TB-2: Reverse-Proxy / Ingress (nginx)]
   |  Vertrauenswürdige Client-IP (nur Loopback/internes Netz), strikte CSP/Header
   v
[TB-3: KIS-Applikationsserver (kis-app, DB-Rolle kis_app)]
   |  +-- TB-3a: Mandantengrenze (legal_entity_id: RT-NORD vs. RT-SUED)
   |  +-- TB-3b: Standort-/Sektorgrenze (Akutklinik vs. MVZ vs. Zentraldienst)
   |  +-- TB-3c: Zweck- & Behandlungszusammenhangsgrenze (TreatmentContext / Break-Glass)
   |  +-- TB-3d: Administrationsgrenze (platform_admin vs. tenant_admin vs. Klinik)
   |  +-- TB-3e: Modulgrenze (Tabellen-Eigentum gemäß config/module_tables.toml)
   v
[TB-4: PostgreSQL (kis_app Laufzeit vs. kis_owner Migrationen)]
   |  Append-Only Audit-Trigger, Constraints, Feldverschlüsselung (AEAD)
   v
[TB-5: Externe Netze & Dienste]
   +-- TI / Konnektor / TI-Gateway (mTLS, SMC-B, HBA)
   +-- Fremdsysteme (HL7 / FHIR / DICOM / GDT / MEDICO-Koexistenz)
   +-- Externer Audit-Prüfanker (WORM) & verschlüsselte Backups (age)
```

---

## 2. Angreiferprofile

| Profil | Beschreibung | Relevante Fähigkeiten |
|---|---|---|
| **A1 — Mandantenfremder Beschäftigter** | Regulär authentifizierter Kliniker oder Verwaltungsmitarbeiter aus Rechtsträger A (`RT-NORD`) | Gültige Sitzung in `RT-NORD`; kennt oder errät UUIDs/Fallnummern/KVNR aus Rechtsträger B (`RT-SUED`) |
| **A2 — Neugieriger Beschäftigter im selben Rechtsträger** | Authentifizierter Mitarbeiter derselben Klinik ohne Behandlungszusammenhang (z. B. Kollege/Prominenter auf anderer Station) | Gültige Rolle `arzt`/`pflege`, versucht direkten Fall-, Patienten-, Such- oder Exportzugriff oder Missbrauch von Break-Glass |
| **A3 — Privilegierter IT-/Plattform-Administrator** | Betreiber oder Helpdesk-Admin mit administrativen Rechten | Versucht über globale Adminrolle, Settings-Änderung (`treatment_context.enforce=false`) oder Rollenmanipulation klinische Daten einzusehen |
| **A4 — Kompromittiertes Schnittstellen- oder Zentraldienst-Konto** | Servicekonto eines zentralen Labors (`ZD-LAB`), eines HL7-/FHIR-Adapters oder eines Event-Consumers | Legitimer Auftragskanal, versucht mandantenfremde Akten abzurufen, fremde Ereignisse zu konsumieren oder Replay-Angriffe |
| **A5 — Externer Angreifer / XSS / Session-Hijacking** | Angreifer im lokalen Netz oder über kompromittiertes Browser-Script | Versucht Token-Diebstahl (`localStorage`), CSRF, Rate-Limit-Umgehung, SQL-/Filter-Injection oder Brute-Force |
| **A6 — Datenbank-/Infrastruktur-Administrator** | Zugriff auf DB-Host, Backups oder Logs | Versucht Auslesen sensibler Patientenfelder, stille Manipulation von `audit_logs` oder Aushebeln von Aufbewahrungsfristen |

---

## 3. Konkrete Bedrohungsmatrix (`T01`–`T18`)

| ID | Bedrohung (STRIDE) | Angreifer | Ist-Befund im Bestandscode (24.11.2026) | Ziel-Gegenmaßnahme | Zielpaket | Pflicht-Negativtest |
|---|---|---|---|---|---|---|
| **T01** | **Mandantenübergreifender Datenabfluss via Org-Baum** *(Information Disclosure)* | A1, A3 | `org_units` hat kein `legal_entity_id`; `MY_UNITS` (`treatment.rs`) traversiert ab `traeger` rekursiv über alle Häuser; `GET /api/v1/org/units` liefert global alle Einheiten. | Harte `legal_entity_id`-Grenze auf `org_units`, DB-Constraint gegen rechtsträgerübergreifende Eltern-Kind-Kanten, mandantengebundener `TenantContext`. | `EP-01` | `NVT-01`, `NVT-02` |
| **T02** | **Globaler Patienten- und Fall-IDOR** *(Information Disclosure / Tampering)* | A1, A2 | `GET /api/v1/patients`, `/patients/:id`, `/cases?caseNumber=`, `/patients/:id/cases` haben `Scope::None` ohne Mandanten- oder Behandlungszusammenhangsprüfung. | Mandanten-Scoping + Trennung MPI vs. lokale Patientenakte (`ADR-002`); verpflichtende Behandlungszusammenhangs-/Zweckprüfung auf Fall- und Suchrouten. | `EP-02`, `EP-03`, `EP-04` | `NVT-05`, `NVT-07`, `NVT-08` |
| **T03** | **Klinischer Datenzugriff durch IT-/Global-Admin** *(Elevation of Privilege)* | A3 | `RoleMatrix::effective` (`rbac.rs`) gewährt `admin` pauschal `PermissionSet::all()`. | Rollenaufteilung in `platform_admin` / `tenant_admin` / `privacy_auditor` ohne klinisches Leserecht; Abschaffung der hartcodierten Admin-Allmacht (`ADR-004`). | `EP-04a` | `NVT-03` |
| **T04** | **Abschalten von Datenschutz-Schutzregeln zur Laufzeit** *(Defense Evasion)* | A3 | `security.treatment_context.enforce` (`platform.rs`) kann auf `false` gesetzt werden. | Unveränderliche Schutzklasse (`policy_floor`) für Mandantenisolation, Behandlungszusammenhang und Pflicht-Audit (`ADR-001`/`ADR-004`). | `EP-01c`, `EP-04a` | `NVT-04` |
| **T05** | **MPI-Missbrauch als Datenbrücke zwischen Rechtsträgern** *(Information Disclosure)* | A1, A2 | Globale Tabelle `patients` ohne Trennung von Identitätsindex und rechtsträgerlokaler Akte. | Zweistufiges MPI (`ADR-002`): rechtsträgerlokale Akten getrennt, MPI-Suche minimal und zwingend lesend auditiert, keine Quersicht auf fremde Fälle. | `EP-02` | `NVT-05`, `NVT-06` |
| **T06** | **Fehlzuordnung / Verwechslung bei Merge oder Notfallpatienten** *(Patient Safety / Tampering)* | A2 (Fehler) | `birth_date NOT NULL` erzwingt fiktive Daten bei unbekannten Notfällen; kein Merge/Unmerge-Workflow. | Expliziter `unknown_emergency`-Status, Vier-Augen- bzw. privilegierter Merge/Unmerge mit Sperre bei offenen kritischen Prozessen (Transfusion/OP) und Audit. | `EP-02c` | `NVT-06`, Merge-Integritätstest |
| **T07** | **Zentraldienst-Überzugriff (z. B. Zentrallabor sieht fremde Akten)** *(Elevation of Privilege)* | A4 | Kein Konzept für `service_agreements`; Auftragszugriff nur über einheitliche `org_units`. | Explizite zeitgebundene `service_agreements` je Rechtsträger; Zugriff ausschließlich auf aktive Auftragsfälle des jeweiligen Mandanten und für den vereinbarten Zweck. | `EP-01b`, `EP-04b` | `NVT-09` |
| **T08** | **Unbemerkter Lesezugriff (Schnüffeln) & Break-Glass-Missbrauch** *(Repudiation / Disclosure)* | A2 | Lesezugriffe auf Patienten, Fälle, Labor und Dokumentation erzeugen aktuell keinen Audit-Eintrag; Break-Glass-Review nicht eskaliert. | Verpflichtendes Lese-Audit auf allen sensiblen Lese-/Suchrouten inkl. `legal_entity_id` und Zweck; verpflichtende Break-Glass-Nachprüfung mit Frist/Eskalation. | `EP-04c` | `NVT-10`, `NVT-11` |
| **T09** | **DSGVO-Export über Rechtsträgergrenzen / Replica-Lag** *(Information Disclosure)* | A1, A3 | `export_patient` (`data_portability.rs`) liest von `db.read()` und exportiert global alle Fälle ohne `legal_entity_id`-Filter. | Zwingende Ausführung auf `db.write()`, strikte Mandantenbindung je Rechtsträger, vollständige JSON/CSV/FHIR-Abdeckung. | `EP-04d` | `NVT-12` |
| **T10** | **Unzulässige Löschung trotz Aufbewahrungspflicht / Legal Hold** *(Tampering / Compliance)* | A3 | `retention_check()` (`data_deletion.rs`) prüft nur `appointments.start_time`; keine Legal Holds; `anonymization.rs` belässt Fall-Freitexte. | Vollständige Retention-Matrix (§ 630f BGB etc.), `legal_holds`-Sperre, mandantengebundene Anonymisierung inkl. Freitext-Redaktion. | `EP-04d` | `NVT-13` |
| **T11** | **Sitzungs-/Token-Diebstahl & fehlende MFA-Härtung** *(Spoofing)* | A5, A6 | Frontend speichert Tokens in `localStorage` (`AP-02`); `users.mfa_secret` unverschlüsselt; MFA-Setup fehlt; `core_routes.rs` übergibt `ip = None` an Auth. | `HttpOnly; Secure; SameSite=Strict`-Cookies + CSRF, AEAD-verschlüsseltes `mfa_secret`, MFA-Enrollment-Flow, reale Client-IP-Auswertung (nur Proxy-Allowlist). | `EP-04a`, `AP-02` | `SICHERHEITS_ABNAHME` Auth/Session |
| **T12** | **Klartext-PII in der Datenbank trotz vorhandener `*_enc`-Spalten** *(Information Disclosure)* | A6 | `kis-patients` liest/schreibt Klartextspalten `insurance_number`, `phone`, `email` statt `*_enc`. | Aktive AEAD-Verschlüsselung inkl. Key-Versionierung und Blind-Index; Bereinigung der Klartextpfade. | `EP-02a` | DB-Rohinspektionstest |
| **T13** | **Manipulation der Audit-Kette durch DB-Admin ohne Erkennung** *(Repudiation / Tampering)* | A6 | Hash-Kette existiert intern in `audit_logs`, besitzt aber keinen externen Prüfanker; keine Partitionierung. | Regelmäßiger signierter Root-Hash-Export an externen WORM-Prüfanker + monatliche Partitionierung + automatisierte Kettenprüfung. | `EP-04c`, `EP-07` | Manipulations-/Anker-Verifikationstest |
| **T14** | **Stilles Scheitern sicherheitskritischer Folgeaktionen** *(Patient Safety / Integrity)* | Systemfehler / A4 | `EventBus::publish` (`events.rs`) schluckt Fehler im Savepoint; Bettensperre bei Entlassung (`kis-orders`) läuft best-effort; `resolve_event_failure` ohne Audit. | Zwei-Klassen-Ereignisvertrag (`ADR-005`): atomare Sicherheitsinvarianten / sicherer Sperrzustand (Klasse A); Outbox/Inbox mit Quarantäne und auditiertem Replay (Klasse B). | `EP-05` | `NVT-14`, Fehler-Injektionstest |
| **T15** | **Mandantenübergreifende Ereignis-/Outbox-Vermischung** *(Information Disclosure)* | A4 | `domain_events` und `event_consumers` haben kein `legal_entity_id`-Feld. | Pflichtfeld `legal_entity_id` auf `domain_events` und `event_consumers`; Consumer dürfen ausschließlich eigene Mandanten-Events abrufen. | `EP-05b` | `NVT-15` |
| **T16** | **Cache-, Export- oder Hintergrundjob-Leck zwischen Mandanten** *(Information Disclosure)* | A1, A4 | Noch keine verteilten Caches/Batch-Jobs im Kern, aber Risiko bei Einführung von Berichten, Brief-Rendering, Katalog-/Statistik-Caches. | Mandantenschlüssel (`legal_entity_id`) zwingender Präfix für jeden Cache-Key, Export-Pfad und Job-Kontext; kein gemeinsamer Ergebnis-Cache über Rechtsträger. | `EP-01`, `EP-07`, `EP-24` | `NVT-16` |
| **T17** | **SQL-Parameterfehler & ungebundene Filter-Interpolation** *(Denial of Service / Injection)* | A5 | `kis-doctors/src/lib.rs` (`list_doctors`) bindet `$1`/`$2` nicht (500-Fehler) und interpoliert `cursor` per `format!`; `kis-patients` interpoliert `cursor` ebenfalls. | Strikte Parametrisierung aller SQL-Werte (auch typisierter UUIDs) über `sqlx::QueryBuilder` bzw. `.bind()`; Ergänzung von Filter-Regressionstests. | `AP-01`, `EP-02a` | Filter- & Fuzz-Tests |
| **T18** | **Kompromittierung externer Schnittstellen / TI-Komponenten** *(Spoofing / Tampering)* | A4, A5 | Schnittstellen-Crates in `entwurf/` ungeprüft und nicht im Build. | Striktes mTLS, Signaturprüfung (QES/SMC-B), Profilvalidierung, Mandantenzuordnung pro Verbindung/BSNR/IK, Quittierungs- und Quarantäne-Queue für eingehende Nachrichten. | `EP-22`, `EP-23` | Schnittstellen-Negativtests + RU-Nachweis |

---

## 4. Sicherheitskritische klinische Invarianten (niemals Best-Effort)

Folgende Abläufe fallen zwingend unter **Klasse A** ([ADR-005](EP00_ARCHITEKTUR_UND_ANALYSE.md#adr-005-sicherheitskritische-invarianten-vs-isolierte-dom%C3%A4nenereignisse--inboxoutbox-ep-05)):

1. **Bettensperre und Hygienefreigabe** bei Entlassung, Verlegung oder Isolationsstatus — ein Bett darf niemals freigegeben werden, solange die Aufbereitung nicht bestätigt ist.
2. **Kritische Labor- und Pathologiebefunde** — Eskalation und Pflichtquittierung dürfen nicht spurlos in einer Fehlerqueue versanden.
3. **Allergie-, AMTS- und Transfusionssperren** — Warnungen und harte Kontraindikationen müssen atomar bei Verordnung/Freigabe wirksam sein.
4. **Entlassung + Neuaufnahme bei Rechtsträgerwechsel** — keine halbfertige Verlegung zwischen zwei rechtlich getrennten Trägern.
5. **Revisionssicheres Audit bei sicherheitsrelevanten Mutationen und Lesezugriffen** — bei Audit-Fehler erfolgt Rollback bzw. Ablehnung der Auslieferung (Fail-Closed).

---

## 5. Review- und Freigabepflicht

Jede Umsetzung eines in Abschnitt 3 adressierten Pakets (`EP-01` bis `EP-05`, `EP-07`, `EP-22`, `EP-23`) ist sicherheitskritisch im Sinne von [AGENTS.md](../AGENTS.md) § 3 und § 7 und erfordert zwingend:
- echte PostgreSQL-Integrationstests inklusive aller zugeordneten Negativtests (`NVT-*`),
- einen ausgefüllten [Abnahmebericht](ABNAHMEBERICHT.md),
- ein **unabhängiges Sicherheitsreview** durch eine nicht-implementierende Person oder Instanz (Quality Gate `G6`).
