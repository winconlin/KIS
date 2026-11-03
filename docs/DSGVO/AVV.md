# Auftragsverarbeitungsvertrag (AVV) — KIS

**Vertrag nach Art. 28 DSGVO zwischen**

| | |
|---|---|
| **Verantwortlicher (Auftraggeber)** | [Klinikname], [Adresse], [PLZ Ort] |
| **Auftragsverarbeiter (Auftragnehmer)** | [KIS-Anbieter], [Adresse], [PLZ Ort] |

**Vertragsnummer:** AVV-KIS-2026-001
**Geltungsbereich:** Verarbeitung personenbezogener Daten im KIS-System
**Datum:** [Datum]

---

## 1. Gegenstand und Dauer der Verarbeitung

### 1.1 Gegenstand

Der Auftragsverarbeiter erbringt folgende Dienstleistungen im Auftrag des Verantwortlichen:

- Betrieb und Wartung des KIS (Klinik-Informationssystem)
- Speicherung und Verarbeitung von Patientendaten, Terminen, Medikamenten, Dokumenten
- Bereitstellung einer REST-API für den Zugriff auf Patientendaten
- Schnittstellen zu MEDICO, HL7, FHIR, DICOM, XDS
- Backup und Wiederherstellung von Patientendaten
- Logging und Audit-Trail für Compliance-Zwecke

### 1.2 Dauer

Der Vertrag beginnt am [Datum] und läuft auf unbestimmte Zeit.

**Kündigungsfrist:** 6 Monate zum Quartalsende (31.03., 30.06., 30.09., 31.12.).

**Datenlöschung nach Vertragsende:**

Nach Vertragsende werden alle personenbezogenen Daten innerhalb von 30 Tagen gelöscht oder an den Verantwortlichen zurückgegeben (Art. 28 Abs. 3 lit. g DSGVO).

---

## 2. Art und Zweck der Verarbeitung

### 2.1 Art der Verarbeitung

- **Erhebung**: Patientendaten, Termin, Medikation, Dokumente
- **Speicherung**: PostgreSQL-Datenbank (verschlüsselt, AES-256-GCM)
- **Nutzung**: Behandlung, Terminverwaltung, Medikamentenverwaltung
- **Übermittlung**: An behandelnde Ärzte, Pflegepersonal, Administration (nach RBAC)
- **Löschung**: Nach Aufbewahrungsfrist (10 Jahre, Art. 630f BGB)
- **Backup**: Täglich inkrementell, wöchentlich vollständig (verschlüsselt, 3-2-1-Regel)

### 2.2 Zweck

- **Behandlung von Patienten** (Art. 9 Abs. 2 lit. h DSGVO)
- **Vollzug von Berufspflichten** (Art. 9 Abs. 2 lit. h DSGVO)
- **Erfüllung von Behandlungsverträgen** (Art. 6 Abs. 1 lit. b DSGVO)
- **Compliance und Audit** (Art. 30, Art. 33 DSGVO)
- **Sicherstellung der Datensicherheit** (Art. 32 DSGVO)

---

## 3. Art der personenbezogenen Daten

### 3.1 Kategorien der Daten

| Kategorie | Daten | Sensibilität |
|-----------|-------|--------------|
| **Patientenidentität** | Name, Vorname, Geburtsdatum, Kassennummer, Adresse, Telefon | Hoch (Gesundheitsdaten) |
| **Behandlung** | Diagnosen, Befunde, Medikation, Allergien, Vitalwerte | Sehr hoch (Art. 9 DSGVO) |
| **Termine** | Terminzeit, Status, behandelnder Arzt | Mittel |
| **Dokumente** | Arztbriefe, Laborbefunde, Bilddateien | Sehr hoch (Art. 9 DSGVO) |
| **Administration** | Benutzerkonten, Rollen, Zugriffsrechte | Mittel |
| **Audit** | Zugriffsprotokolle, Änderungen, IP-Adressen | Mittel |

### 3.2 Besondere Kategorien (Art. 9 DSGVO)

- **Gesundheitsdaten**: Diagnosen, Medikation, Laborwerte, Allergien
- **Behandlungsdaten**: Arztbriefe, Befunde, Therapiepläne
- **Identitätsdaten**: Name, Geburtsdatum, Kassennummer

**Rechtsgrundlage:** Art. 9 Abs. 2 lit. h DSGVO (Vollzug von Berufspflichten)

---

## 4. Kategorien betroffener Personen

| Kategorie | Beschreibung |
|-----------|--------------|
| **Patienten** | Personen, die behandelt werden oder behandelt wurden |
| **Ärzte** | Behandelnde Ärzte, Fachärzte, Sprechstundenärzte |
| **Pflegepersonal** | Krankenpfleger, Pflegekräfte, Praxisassistenten |
| **Administration** | Rezeption, Verwaltung, IT-Mitarbeiter |
| **Externe Stellen** | Kassen, Aufsichtsbehörden (bei Meldung), Sub-Verarbeiter |

---

## 5. Pflichten und Rechte des Verantwortlichen

### 5.1 Pflichten

1. **Rechtsgrundlage sicherstellen**: Der Verantwortliche stellt sicher, dass eine gültige Rechtsgrundlage für die Verarbeitung vorliegt (Art. 6, Art. 9 DSGVO).
2. **DSB benennen**: Ein Datenschutzbeauftragter wird benannt (Art. 37 DSGVO) und im KIS-System als `Dsb`-Rolle hinterlegt.
3. **Betroffenenrechte**: Der Verantwortliche ist für die Bearbeitung von Betroffenenanfragen verantwortlich (Art. 15–22 DSGVO).
4. **DSFA**: Bei erforderlichem Umfang wird eine Datenschutz-Folgenabschätzung durchgeführt (Art. 35 DSGVO).
5. **Schulung**: Das Personal wird regelmäßig im Datenschutz geschult (Art. 39 DSGVO).
6. **Monitoring**: Der Verantwortliche überwacht die Einhaltung der DSGVO und des AVV.

### 5.2 Rechte

1. **Auskunft**: Der Verantwortliche kann jederzeit Auskunft über die Verarbeitung erfordern (Art. 28 Abs. 3 lit. a).
2. **Audit-Recht**: Der Verantwortliche kann die Einhaltung der DSGVO und dieses Vertrags prüfen (Art. 28 Abs. 3 lit. h).
3. **Anweisungen**: Der Verantwortliche kann verbindliche Anweisungen zur Verarbeitung erteilen (Art. 28 Abs. 3 lit. a).
4. **Löschung**: Der Verantwortliche kann die Löschung von Daten anordnen (Art. 28 Abs. 3 lit. g).

---

## 6. Pflichten des Auftragsverarbeiters (Art. 28 Abs. 3 DSGVO)

### 6.1 Grundpflichten

1. **Nur nach Anweisung**: Der Auftragsverarbeiter verarbeitet personenbezogene Daten nur nach der dokumentierten Anweisung des Verantwortlichen (Art. 28 Abs. 3 lit. a).
2. **Vertraulichkeit**: Alle Personen, die im Auftrag des Auftragsverarbeiters Zugang zu personenbezogenen Daten haben, sind zur Vertraulichkeit verpflichtet (Art. 28 Abs. 3 lit. b).
3. **Sicherheit**: Der Auftragsverarbeiter trifft alle technischen und organisatorischen Maßnahmen zur Sicherung der Daten (Art. 32 DSGVO):
   - Verschlüsselung at rest (AES-256-GCM)
   - Verschlüsselung in transit (TLS 1.3)
   - RBAC (Rollenbasierte Zugriffskontrolle)
   - Audit-Trail mit Hash-Chain
   - Backup und Wiederherstellung
4. **Sub-Verarbeiter**: Der Auftragsverarbeiter bezieht Sub-Verarbeiter nur mit vorheriger Genehmigung des Verantwortlichen ein (Art. 28 Abs. 3 lit. d).
5. **Betroffenenrechte**: Der Auftragsverarbeiter unterstützt den Verantwortlichen bei der Erfüllung von Betroffenenanfragen (Art. 28 Abs. 3 lit. e).
6. **DSGVO-Konformität**: Der Auftragsverarbeiter stellt sicher, dass alle Personen, die Zugang zu personenbezogenen Daten haben, die DSGVO kennen (Art. 28 Abs. 3 lit. f).
7. **Löschung**: Nach Vertragsende werden alle Daten gelöscht oder zurückgegeben (Art. 28 Abs. 3 lit. g).
8. **Audit**: Der Auftragsverarbeiter stellt Informationen zur Verfügung, die für die Einhaltung der DSGVO erforderlich sind, und ermöglicht Audits (Art. 28 Abs. 3 lit. h).

### 6.2 Technische Maßnahmen

| Maßnahme | Umsetzung |
|----------|-----------|
| **Verschlüsselung at rest** | AES-256-GCM für alle Patientendaten |
| **Verschlüsselung in transit** | TLS 1.3 (rustls) für alle API-Kommunikation |
| **Zugriffskontrolle** | RBAC mit 7 Rollen (Admin, Arzt, Pflege, Rezeption, Viewer, Auditor, LLM-Agent) |
| **Audit-Trail** | Hash-Chain, 10 Jahre online, 30 Jahre Archiv |
| **Backup** | 3-2-1-Regel, verschlüsselt, wöchentlich vollständig, täglich inkrementell |
| **Incident-Response** | 72-h-Meldung, Forensik, Lessons Learned |
| **Secret-Management** | Alle Secrets aus ENV-Variablen, nie im Code |
| **Signed Releases** | GPG-Signatur, SHA-256-Checksums |

### 6.3 Organisatorische Maßnahmen

| Maßnahme | Umsetzung |
|----------|-----------|
| **DSB** | Datenschutzbeauftragter benannt (Verantwortlicher) |
| **Schulung** | Regelmäßige Datenschutz-Schulung für alle Mitarbeiter |
| **Vertraulichkeit** | NDA für alle Mitarbeiter mit Datenzugriff |
| **Zugriffskontrolle** | Least-Privilege-Prinzip, regelmäßige Review |
| **Incident-Response** | Dokumentierter IR-Plan (SECURITY.md § Incident) |
| **DSFA** | Regelmäßige Datenschutz-Folgenabschätzung |

---

## 7. Sub-Verarbeiter (Cloud-Anbieter, Hosting)

### 7.1 Bestehende Sub-Verarbeiter

| Sub-Verarbeiter | Dienst | Datenkategorie | Standort |
|-----------------|--------|----------------|----------|
| [Cloud-Anbieter] | Hosting, Storage | Alle (verschlüsselt) | EU (Deutschland) |
| [Backup-Anbieter] | Offsite-Backup | Alle (verschlüsselt) | EU (Deutschland) |
| [Notar-Service] | Audit-Hash-Anchor | Hash-Values (keine Patientendaten) | EU (Deutschland) |

### 7.2 Änderungen

Der Auftragsverarbeiter muss den Verantwortlichen 30 Tage vor der Einbeziehung eines neuen Sub-Verarbeiters informieren.

**Recht des Verantwortlichen:**
- Ausdrückliche Genehmigung erforderlich (Art. 28 Abs. 3 lit. d)
- Recht auf Widerspruch bei neuen Sub-Verarbeitern
- Wenn Widerspruch: Vertrag muss gekündigt oder angepasst werden

### 7.3 Pflichten der Sub-Verarbeiter

Alle Sub-Verarbeiter unterliegen den gleichen Pflichten wie der Auftragsverarbeiter (Art. 28 Abs. 4 DSGVO).

---

## 8. Löschpflicht bei Vertragsende

### 8.1 Löschung

Nach Vertragsende werden alle personenbezogenen Daten innerhalb von 30 Tagen gelöscht, es sei denn:

1. Eine gesetzliche Aufbewahrungspflicht besteht (z.B. § 630f BGB: 10 Jahre)
2. Der Verantwortliche eine andere Aufbewahrung anordnet

### 8.2 Rückgabe

Auf Anforderung des Verantwortlichen werden die Daten in einem strukturierten, maschinenlesbaren Format zurückgegeben (JSON, CSV, GDT).

### 8.3 Bestätigung

Der Auftragsverarbeiter bestätigt die Löschung schriftlich innerhalb von 14 Tagen nach Vertragsende.

---

## 9. Audit-Recht des Verantwortlichen

### 9.1 Audit

Der Verantwortliche hat das Recht, die Einhaltung dieses Vertrags und der DSGVO zu prüfen.

**Form:**
- Unangemeldete Audits möglich (mindestens 1x jährlich)
- Gemeldete Audits auf Anfrage (max. 1x pro Quartal, außer bei Verdacht auf Verstoß)

**Umfang:**
- Technische Maßnahmen (Verschlüsselung, RBAC, Audit-Trail)
- Organisatorische Maßnahmen (Schulungen, NDA, Prozesse)
- Sub-Verarbeiter (Prüfung der Verträge)

**Kosten:**
- Normale Audits: Kosten trägt der Auftragsverarbeiter
- Bei Verdacht auf Verstoß: Kosten trägt der Auftragsverarbeiter

### 9.2 Informationen

Der Auftragsverarbeiter stellt alle Informationen bereit, die für die Audit erforderlich sind:
- Audit-Logs (10 Jahre)
- Konfigurationsdateien
- Verträge mit Sub-Verarbeitern
- Sicherheitszertifikate (ISO 27001, MDR)

### 9.3 Maßnahmen bei Verstößen

Bei festgestellten Verstößen:
1. Sofortige Benachrichtigung des Verantwortlichen
2. Maßnahmenplan innerhalb von 7 Tagen
3. Umsetzung innerhalb von 30 Tagen
4. Nachweis der Umsetzung

Bei schwerwiegenden Verstößen:
- Recht zur Kündigung des Vertrags (Art. 28 Abs. 3 lit. a)
- Meldung an die Aufsichtsbehörde (Art. 33 DSGVO)

---

## 10. Haftung

### 10.1 Haftung des Auftragsverarbeiters

Der Auftragsverarbeiter haftet für Schäden, die durch Verstöße gegen die DSGVO oder diesen Vertrag verursacht werden.

**Ausnahmen:**
- höhere Gewalt
- Verschulden des Verantwortlichen
- Nicht-Einhaltung der Anweisungen des Verantwortlichen

### 10.2 Haftung des Verantwortlichen

Der Verantwortliche haftet für:
- Fehlende oder ungültige Rechtsgrundlage
- Falsche Anweisungen
- Mangelnde Schulung des Personals

---

## 11. Vertraulichkeit

### 11.1 Vertraulichkeit der Daten

Alle personenbezogenen Daten sind vertraulich zu behandeln und dürfen nur für den vertraglich vereinbarten Zweck verwendet werden.

### 11.2 Vertraulichkeit des Vertrags

Die Inhalte dieses Vertrags sind vertraulich zu behandeln und dürfen nur mit ausdrücklicher Genehmigung des anderen Vertragspartners weitergegeben werden.

---

## 12. Schlussbestimmungen

### 12.1 Änderungen

Änderungen dieses Vertrags sind nur schriftlich gültig (Art. 126 BGB).

### 12.2 Anwendbares Recht

Auf diesen Vertrag findet deutsches Recht Anwendung.

### 12.3 Gerichtsstand

Gerichtsstand ist der Sitz des Verantwortlichen.

### 12.4 Salvatorische Klausel

Sollte eine Bestimmung dieses Vertrags unwirksam sein, so bleibt die Wirksamkeit der übrigen Bestimmungen unberührt.

---

## 13. Unterschriftsblock

| | **Verantwortlicher** | **Auftragsverarbeiter** |
|---|---|---|
| **Name** | | |
| **Position** | | |
| **Unterschrift** | | |
| **Datum** | | |
| **Siegel** | | |

---

**Anlagen:**

1. [ ] Liste der Sub-Verarbeiter
2. [ ] Technische Maßnahmen (SECURITY.md)
3. [ ] Datenschutz-Folgenabschätzung (DSFA)
4. [ ] Verzeichnis der Verarbeitungstätigkeiten (Art. 30 DSGVO)

---

*Version: 1.0 | Stand: 03.11.2026 | KIS Projekt*
