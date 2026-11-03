# KIS — Verarbeitungsvorgänge (DSGVO Art. 30)

**Stand:** 02.11.2026 · Entwicklungsprototyp

> **Wichtig:** Dieses Verzeichnis ist ein Arbeitsergebnis aus dem
> Entwicklungsprototyp. Es dokumentiert die im Code und Schema tatsächlich
> verarbeiteten Datenkategorien. Es ist **kein** rechtsverbindliches
> Verzeichnis nach Art. 30 DSGVO, solange nicht:
>
> 1. der Verantwortliche (Krankenhaus / Betreiber) seine Kontaktdaten und
>    eventuelle DSB-Kontaktdaten eingetragen hat,
> 2. die Rechtsgrundlagen mit dem Datenschutzbeauftragten abgestimmt sind,
> 3. die tatsächlichen Empfänger / Auftragsverarbeiter (z. B.
>    Rechenzentrum, Backup-Anbieter, FHIR/MEDICO-Gateway) benannt sind.
>
> Die Einträge „[zu ergänzen]" markieren Stellen, die ohne den
> konkreten Betreiber nicht verlässlich beantwortet werden können.

---

## 1. Verantwortlicher

| Feld | Wert |
|------|------|
| Name / Organisation | [zu ergänzen — z. B. „Krankenhaus XY gGmbH"] |
| Anschrift | [zu ergänzen] |
| Kontakt (Telefon, E-Mail) | [zu ergänzen] |
| Datenschutzbeauftragter | [zu ergänzen — Name, Anschrift, Kontakt] |
| Vertretung im Drittland (Art. 27) | [nicht erforderlich, falls keine Drittlandverarbeitung] |
| Rechtsform des Systems | KIS — Krankenhaus-Informations-System (Rust, Axum, PostgreSQL) |
| Systemversion | `kis-app` 0.1.0 (Cargo Workspace) |

**Zweckbestimmung (Art. 4 Nr. 2 DSGVO):**
Elektronische Erfassung, Verwaltung und Bereitstellung von
Patienten-, Termin-, Medikamenten- und Organisationsdaten zur
Durchführung der vertrags- und stationären medizinischen Versorgung
(Terminverwaltung, Medikamentenpflege, Arzt-/Rezeptionsverwaltung,
Schnittstellen zu CGM MEDICO / FHIR / Telematikinfrastruktur).

---

## 2. Verarbeitungsvorgänge im Überblick

| # | Vorgang | Rechtsgrundlage (Art. 6 / Art. 9) | Betroffene |
|---|---------|-----------------------------------|------------|
| 1 | Patientenstammdaten erfassen & pflegen | Art. 9 Abs. 2 H i. V. m. SGB V (Behandlung) | Patienten |
| 2 | Termine anlegen, ändern, stornieren | Art. 6 Abs. 1 b / c (Vertrag / Rechtsvorschrift) | Patienten, Ärzte |
| 3 | Medikamente dokumentieren | Art. 9 Abs. 2 H (Behandlung) | Patienten |
| 4 | Benutzer- & Rollenumwaltung (RBAC) | Art. 6 Abs. 1 b / c | Mitarbeiter |
| 5 | Authentifizierung & Sitzungsmanagement | Art. 6 Abs. 1 c (Sicherheitspflicht) | Mitarbeiter |
| 6 | Audit-Log (Hash-Chain) | Art. 6 Abs. 1 c + Art. 5 (Integrität, Nachweis) | Patienten, Mitarbeiter |
| 7 | Einverständniserklärungen (Consent) | Art. 6 / 9 (Dokumentationspflicht) | Patienten |
| 8 | Schnittstellen: FHIR / HL7 / DICOM / GDT / MEDICO / TI | Art. 6 Abs. 1 f (legitimate interest: Interoperabilität) + Art. 9 | Patienten |
| 9 | Backup & Wiederaufbereitung | Art. 5 Abs. 1 f (Integrität/Vertraulichkeit) | Alle |
| 10 | In-Memory-Cache (moka) | Art. 5 Abs. 1 e (Speicherbegrenzung, TTL ≤ 5 min) | Patienten |

---

## 3. Detaillierte Verarbeitungsvorgänge

### 3.1 Patientenstammdaten (Tabelle `patients`)

| Feld | Datentyp | Kategorie (Art. 4 Nr. 1) | Besonderheit |
|------|----------|--------------------------|--------------|
| `id` | UUID | Pseudonymisierter Schlüssel | Technischer Identifikator |
| `external_id` | TEXT | Pseudonymisierter Schlüssel | Fremdschlüssel aus CGM MEDICO |
| `name_family` | TEXT | Personenbezug | Klartext |
| `name_given` | TEXT | Personenbezug | Klartext |
| `birth_date` | DATE | Personenbezug | Klartext |
| `gender` | TEXT | Personenbezug | Klartext |
| `phone` | TEXT | Personenbezug | Klartext (Spalte, s. u.) |
| `phone_enc` | TEXT | Personenbezug (verschlüsselt) | AES-256-GCM, Base64 |
| `email` | TEXT | Personenbezug | Klartext |
| `email_enc` | TEXT | Personenbezug (verschlüsselt) | AES-256-GCM, Base64 |
| `address_street` / `address_zip` / `address_city` | TEXT | Personenbezug | Klartext |
| `insurance_number` | TEXT | Versicherung / Personenbezug | Klartext |
| `insurance_number_enc` | TEXT | Versicherung (verschlüsselt) | AES-256-GCM, Base64 |
| `allergies` | JSONB | **Gesundheitsdaten (Art. 9)** | Klartext |
| `created_at` / `updated_at` | TIMESTAMPTZ | Metadaten | — |
| `deleted_at` | TIMESTAMPTZ | Metadaten (Soft-Delete) | DSGVO-Art. 17 |
| `version` | INT | Metadaten (Optimistic Locking) | — |

**Rechtsgrundlage:** Art. 9 Abs. 2 Buchst. H DSGVO i. V. m. § 229 SGB V
(Behandlungsdaten).
**Aufbewahrung:** So lange wie für die Behandlung und
Dokumentationspflichten erforderlich; danach anonymisieren oder löschen
(DSGVO-Art. 17; Frist nach Fachbereich, [zu ergänzen]).

---

### 3.2 Termine (Tabelle `appointments`)

| Feld | Kategorie |
|------|-----------|
| `id` | Technischer Schlüssel |
| `patient_id` | Personenbezug (Indirekt) |
| `doctor_id` | Personenbezug (Indirekt) |
| `category` | Metadaten |
| `status` | Metadaten |
| `start_time` / `end_time` | Metadaten |
| `reason` | **Gesundheitsdaten** (wenn behandlungsbezogen) |
| `cancelled_at` | Metadaten |

**Rechtsgrundlage:** Art. 6 Abs. 1 Buchst. b DSGVO (Vertragsdurchführung).
**Aufbewahrung:** Mit Patientenbezug gleich wie 3.1; reine
Organisationsdaten (z. B. Stornierungen ohne Grund) können nach
Auftragsende gelöscht werden.

---

### 3.3 Medikamente (Tabelle `medications`)

| Feld | Kategorie |
|------|-----------|
| `id` | Technischer Schlüssel |
| `patient_id` | Personenbezug (Indirekt) |
| `name` | **Gesundheitsdaten** (Arzneimittelbezug) |
| `dosage` / `frequency` | **Gesundheitsdaten** |
| `start_date` / `end_date` | **Gesundheitsdaten** |

**Rechtsgrundlage:** Art. 9 Abs. 2 Buchst. H DSGVO.
**Aufbewahrung:** Wie 3.1.

---

### 3.4 Benutzer & RBAC (Tabelle `users`)

| Feld | Kategorie |
|------|-----------|
| `id` | Technischer Schlüssel |
| `username` / `email` | Personenbezug |
| `password_hash` | **Besonders schützend** (Argon2id) |
| `display_name` | Personenbezug |
| `role` | Metadaten (RBAC) |
| `is_active` | Metadaten |
| `mfa_secret` | **Besonders schützend** (TOTP) |
| `mfa_enabled` | Metadaten |
| `last_login_at` | **Verhaltensdaten** (Log) |
| `login_attempts` / `locked_until` | **Verhaltensdaten** (Security) |

**Betroffene:** Mitarbeiter (nicht Patienten).
**Rechtsgrundlage:** Art. 6 Abs. 1 Buchst. c (Sicherheitspflicht) und
b (Arbeitsverhältnis).
**Aufbewahrung:** Während des Arbeitsverhältnisses; danach Löschung oder
Anonymisierung (Sperrfristen nach Arbeitsrecht, [zu ergänzen]).

---

### 3.5 Audit-Log (Tabelle `audit_logs`)

| Feld | Kategorie |
|------|-----------|
| `id` | BIGSERIAL |
| `user_id` | Personenbezug (User) |
| `action` | Metadaten |
| `entity_type` / `entity_id` | Personenbezug (indirekt, wenn Patient) |
| `old_data` / `new_data` | **Kann Gesundheitsdaten enthalten** (JSONB, Vorher/Nachher) |
| `ip_address` | **Verhaltensdaten** (DSGVO-Art. 30) |
| `timestamp` | Metadaten |
| `prev_hash` / `hash` | Metadaten (Integrität, Hash-Chain) |

**Rechtsgrundlage:** Art. 6 Abs. 1 Buchst. c (Integrität,
Nachweispflicht) + Art. 5 Abs. 1 f DSGVO.
**Aufbewahrung:** Mindestens 10 Jahre (3650 Tage) gemäß
`security.audit.retention_days`; danach Archivierung (keine Löschung,
weil Hash-Chain nur lückenlos verifizierbar bleibt).
**Sonderstatus:** Diese Tabelle ist **append-only** und wird
**nie** gelöscht — sie ist die Nachweismöglichkeit für alle
Betroffenenrechte.

---

### 3.6 Sitzungen (Tabelle `sessions`)

| Feld | Kategorie |
|------|-----------|
| `id` | Technischer Schlüssel |
| `user_id` | Personenbezug (indirekt) |
| `jti` / `access_jti` | Token-Identifikator (JWT) |
| `issued_at` / `expires_at` | Metadaten |
| `revoked_at` | Metadaten |
| `ip_address` | **Verhaltensdaten** |

**Rechtsgrundlage:** Art. 6 Abs. 1 Buchst. c (Sicherheitspflicht).
**Aufbewahrung:** Bis zum Token-Ablauf bzw. Widerruf; danach Löschung.

---

### 3.7 Einwilligungen (Tabelle `consents`)

| Feld | Kategorie |
|------|-----------|
| `id` | Technischer Schlüssel |
| `patient_id` | Personenbezug |
| `type` | Metadaten (Art der Einwilligung) |
| `granted` / `granted_at` / `granted_by` | **Einwilligungsnachweis** |
| `withdrawn_at` / `withdrawn_by` | **Widerrufsnachweis** (DSGVO-Art. 7 Abs. 3) |
| `legal_basis` | Metadaten |

**Rechtsgrundlage:** Dokumentationspflicht nach Art. 7 DSGVO.
**Aufbewahrung:** Unbegrenzt (Nachweispflicht).

---

### 3.8 In-Memory-Cache (moka, `kis-core::cache`)

| Eigenschaft | Wert |
|-------------|------|
| Speicher | RAM (nicht persistent) |
| TTL | 300 s (5 min, konfigurierbar) |
| Max. Einträge | 10.000 (konfigurierbar) |
| Invalidierung | Bei jeder Write-Operation (Active Invalidation) |
| Inhaltskategorien | Gleiche wie 3.1–3.3 (Patienten, Termine, Medikamente) |

**Rechtsgrundlage:** Gleiche wie der zugehörige Verarbeitungsvorgang
(3.1–3.3).
**Aufbewahrung:** Maximal 5 Minuten (TTL) oder bis zur Write-Invalidierung.

---

### 3.9 Schnittstellen (Phase 2–3, Stubs)

| Schnittstelle | Crate | Status | Verarbeitete Daten |
|---------------|-------|--------|--------------------|
| FHIR R4 | `kis-fhir` | Stub | Patienten, Termine (FHIR-Resources) |
| HL7 v2.5 | `kis-hl7` | Stub | ADT-Feeds (Patienten, Aufnahmen) |
| DICOM | `kis-dicom` | Stub | **Medizinische Bilddaten** (PACS) |
| XDS | `kis-xds` | Stub | Dokumentreferenzen |
| GDT | `kis-gdt` | Stub | GDT-Export (Datenaustausch) |
| CGM MEDICO | `kis-medico` | Stub | Synchronisation mit Bestands-System |
| Telematikinfrastruktur | `kis-ti` | Stub | eGK, ePA, eRezept, eAU |

**Rechtsgrundlage:** Art. 6 Abs. 1 Buchst. f DSGVO (Interoperabilität)
+ ggf. Art. 9 Abs. 2 H.
**Aufbewahrung:** Wie der zugehörige Verarbeitungsvorgang.
**Empfänger:** Externe Systeme (Krankenkassen, Fachpraxen, PACS) —
Auftragsverarbeitungsverträge gemäß [AVV.md](AVV.md) erforderlich.

---

## 4. Kategorien von Empfängern (Art. 13/14)

| Empfänger | Daten | Zweck |
|-----------|-------|-------|
| Behandelnde Ärzte (interner Zugriff) | Patienten, Termine, Medikamente | Behandlung |
| Rezeption / Verwaltung | Patienten (Stammdaten), Termine | Organisation |
| IT-Betrieb (Systemadministrator) | Alle (inkl. Audit) | Betrieb & Wartung |
| Externe Rechenzentren | Alle | Hosting |
| CGM MEDICO / FHIR-Gateway | Patienten, Termine | Synchronisation |
| Telematikinfrastruktur (gematik) | eGK, ePA | Interoperabilität |
| Datenschutzbeauftragter | Alle (auf Antrag) | Überwachung |
| Aufsichtsbehörde | Alle (auf Anfrage) | Rechtsdurchsetzung |

---

## 5. Übertragungen in Drittländer (Art. 44–49)

| Transfer | Basis |
|----------|-------|
| Cloud-Hosting außerhalb EU/EEA | [zu ergänzen — ggf. SCC / EU-US DPF erforderlich] |
| Telematikinfrastruktur | Innerhalb DE (gematik), keine Drittlandübertragung |
| FHIR-Gateway (extern) | [zu ergänzen — je nach Betreiberstandort] |

---

## 6. Technische & organisatorische Maßnahmen (TOM, Art. 32)

| Maßnahme | Umsetzung | Status |
|----------|-----------|--------|
| Verschlüsselung at rest | AES-256-GCM pro Datensatz (Spalten `phone_enc`, `email_enc`, `insurance_number_enc`) | ✅ |
| Verschlüsselung at rest (DB-Datei) | [zu ergänzen — SQLCipher / pgcrypto-TDE] | 🔲 |
| Verschlüsselung in transit | TLS 1.3 (rustls) | 🔲 (Task 5.1) |
| Passwortspeicherung | Argon2id | ✅ |
| Zugangskontrolle | RBAC (Rollen: Arzt, Rezeption, Admin) | ✅ (Grund) |
| Audit-Log | Hash-Chain (SHA-256), append-only | ✅ (Grund) |
| Backup | Voll + inkrementell, age-verschlüsselt, 3-2-1 | 🔲 (Task 5.12) |
| Anonymisierung | Nach Aufbewahrungsfrist | 🔲 (Task 5.8) |
| Datenlöschung | Soft-Delete (DSGVO-Art. 17) + Hard-Delete nach Frist | 🔲 (Task 5.6) |
| Datenportabilität | Export JSON/CSV (Art. 15/20) | 🔲 (Task 5.7) |
| Rate-Limiting | Sliding-Window (100 API / 10 Auth req/min) | ✅ |
| Session-Management | JWT (Access 15 min, Refresh 24 h), Blacklist | ✅ (Grund) |
| MFA | TOTP (Setup noch offen) | 🔲 |
| Inaktivitäts-Timeout | 30 min | 🔲 (Task 5.5) |
| Concurrent-Login-Limit | [zu ergänzen] | 🔲 (Task 5.5) |

---

## 7. Betroffenenrechte (Art. 15–22)

| Recht | Umsetzung | Status |
|-------|-----------|--------|
| Auskunft (Art. 15) | [zu implementieren] | 🔲 |
| Berichtigung (Art. 16) | [zu implementieren] | 🔲 |
| Löschung (Art. 17) | Soft-Delete vorhanden; Hard-Delete nach Frist | 🔲 (Task 5.6) |
| Einschränkung (Art. 18) | [zu implementieren] | 🔲 |
| Datenübertragbarkeit (Art. 20) | Export JSON/CSV | 🔲 (Task 5.7) |
| Widerspruch (Art. 21) | [zu implementieren] | 🔲 |
| Widerruf der Einwilligung | `consents.withdrawn_at` vorhanden | ✅ (Grund) |

---

## 8. Änderungsprotokoll dieses Verzeichnisses

| Datum | Änderung | Autor |
|-------|----------|-------|
| 02.11.2026 | Erstellt (Task 5.9) | [Agent / DSB] |
