# Penetration-Test-Setup — KIS

**Wichtig:** Penetration-Tests nur in isolierten Testumgebungen. Keine Produktionssysteme ohne Genehmigung.

## Voraussetzungen

### Software

```bash
# Nuclei (HTTP-Scanner)
go install -v github.com/projectdiscovery/nuclei/v3/cmd/nuclei@latest

# OWASP ZAP (Web-Application-Scanner)
docker pull ghcr.io/zaproxy/zaproxy:stable

# Burp Suite (optional, kommerziell)
# Community Edition: https://portswigger.net/burp/communitydownload

# ffuf (Fuzzing)
go install -v github.com/ffuf/ffuf/v2@latest

# sqlmap (SQL-Injection)
pip install sqlmap

# nikto (Legacy-Scanner)
apt-get install nikto
```

### Testumgebung

1. **Isolierte Testinstanz**: KIS in Docker/VM mit Testdaten
2. **Keine echten Patientendaten**: Nur synthetische Testdaten
3. **Netzwerk-Trennung**: Getrenntes VLAN/Subnetz
4. **Backup**: Vorheriges Backup der Testinstanz

## Scan-Ablauf

### 1. Nuclei (HTTP-Endpunkte)

```bash
# Alle KIS-Endpunkte scannen
nuclei -u https://kis-test.local \
  -t nuclei-templates/kis/ \
  -o nuclei_results.json \
  -severity info,low,medium,high,critical

# Nur kritische/schwere Findings
nuclei -u https://kis-test.local \
  -t nuclei-templates/kis/ \
  -severity high,critical \
  -o critical_findings.json
```

### 2. OWASP ZAP (Web-App-Scan)

```bash
# ZAP starten
docker run -it --rm \
  -p 8080:8080 \
  ghcr.io/zaproxy/zaproxy:stable \
  zap-baseline.py -t https://kis-test.local \
  -J zap_results.json

# Vollständiger Scan (langsam)
docker run -it --rm \
  -p 8080:8080 \
  ghcr.io/zaproxy/zaproxy:stable \
  zap-full-scan.py -t https://kis-test.local \
  -J zap_full_results.json
```

### 3. Automatisierter Scan (scan.sh)

```bash
./security/penetration/scan.sh
```

Das Skript führt alle Scans aus und erstellt einen Report in `reports/`.

### 4. Manuelle Tests

Siehe `endpoints.md` für die Liste aller testbaren Endpunkte mit Risikobewertung.

## Nuclei-Templates

Die Templates in `nuclei-templates/kis/` decken ab:

| Template | Zweck |
|----------|-------|
| `auth-bypass.yml` | Auth-Bypass-Tests (JWT, Session) |
| `idor.yml` | Insecure Direct Object Reference |
| `injection.yml` | SQL/Command Injection |
| `info-leak.yml` | Informationslecks (Version, Debug) |
| `rbac.yml` | RBAC-Verletzungen (Rollen-Eskalation) |
| `tls.yml` | TLS-Konfiguration (Protokolle, Zertifikate) |

## Report-Erstellung

Nach dem Scan:

1. Ergebnisse aus `nuclei_results.json` und `zap_results.json` extrahieren
2. In `report-template.md` eintragen
3. Findings priorisieren (CVSS-Score)
4. Maßnahmen vorschlagen
5. Report an Security-Team übergeben

## Regelmäßige Scans

| Scan | Häufigkeit |
|------|------------|
| **Nuclei** | Täglich (CI/CD) |
| **ZAP Baseline** | Wöchentlich |
| **ZAP Full** | Monatlich |
| **Manuell** | Bei größeren Änderungen |

## Dokumentation

- **Scan-Reports**: In `reports/` speichern (versioniert)
- **Findings**: In Issue-Tracker eintragen (Jira, GitHub Issues)
- **Maßnahmen**: Im Audit-Log dokumentieren

## Warnung

**Vor einem Penetration-Test:**

1. Genehmigung des verantwortlichen IT-Leiters einholen
2. Testumgebung vorbereiten (keine Produktion!)
3. Backup erstellen
4. Scan-Zeitfenster koordinieren
5. Notfallplan erstellen (Rollback)

**Nach einem Penetration-Test:**

1. Alle Findings dokumentieren
2. Kritische/schwere Findings sofort beheben
3. Re-Scan durchführen
4. Report archivieren
5. Lessons Learned dokumentieren
