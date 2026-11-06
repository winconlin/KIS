# KIS — Betrieb und Wiederherstellung

> **Hinweis (06.11.2026):** Teile dieses Dokuments beschreiben den entfernten SQLite-Prototyp oder ein Zielbild. Maßgeblich für den Ist-Zustand sind [INSTALLATION](INSTALLATION.md), [API](API.md), [DATABASE](DATABASE.md) und [REVIEW_STATUS](REVIEW_STATUS.md).


Stand: 03.11.2026 · Technische Abläufe für Testinstanzen

## Backup: 3-2-1-Regel (Task 5.12)

### 3-2-1-Regel

| Regel | Umsetzung |
|-------|-----------|
| **3 Kopien** | 1. Produktion (PostgreSQL Primary), 2. Backup-Server (lokal), 3. Offsite (Cloud) |
| **2 Medien** | 1. PostgreSQL `pg_dump` (logisch), 2. Disk-Image (physisch, optional) |
| **1 Offsite** | Cloud (verschlüsselt, AES-256-GCM) |

### Backup-Plan

| Typ | Häufigkeit | Uhrzeit | Aufbewahrung |
|-----|------------|---------|--------------|
| **Voll-Backup** | Wöchentlich | Sonntag 02:00 | 12 Monate |
| **Inkrementell** | Täglich | 02:00 | 30 Tage |
| **Jährlich** | Jährlich | 01.01., 02:00 | 5 Jahre |

### Verschlüsselung

- **Algorithmus**: AES-256-GCM
- **Key**: Aus ENV-Variable `KIS_BACKUP_KEY` (nicht im Code, nicht im Repo)
- **Key-Management**: HashiCorp Vault / AWS Secrets Manager
- **Key-Rotation**: Jährlich (mit Datenmigration)

### Offsite-Backup (Cloud)

| Parameter | Wert |
|-----------|------|
| **Provider** | [AWS S3 / Azure Blob / GCP Storage] |
| **Region** | EU (eu-central-1 / eu-central-2) |
| **Verschlüsselung** | AES-256-GCM (Client-seitig) + Server-seitig (Provider) |
| **Retention** | 5 Jahre (jährlich), 12 Monate (wöchentlich), 30 Tage (täglich) |
| **Lifecycle** | Automatische Löschung nach Retention-Ende |

### Restore-Test

**Monatlich** (dokumentiert):

1. Voll-Backup vom letzten Sonntag auswählen
2. Entschlüsseln und Integrität prüfen (SHA-256)
3. Restore in Testinstanz
4. Funktionstests: Login, Patientensuche, Audit-Log
5. Ergebnis dokumentieren: `restore_test_$(date +%Y-%m).md`

**Inhalt des Reports:**
- Datum, Zeit
- Backup-Datei (Name, Größe, Hash)
- Restore-Dauer
- Datenkonsistenz (Zeilenzahlen, Prüfsummen)
- Funktionstests (bestanden/gescheitert)
- Probleme und Maßnahmen

### Backup-Skript

```bash
#!/bin/bash
# KIS Backup — 3-2-1-Regel
# Aufruf: cron 0 2 * * * /opt/kis/scripts/backup.sh

set -euo pipefail

BACKUP_DIR="/var/lib/kis-backup"
DATE=$(date +%Y%m%d_%H%M)
FULL_BACKUP="/tmp/kis_full_${DATE}.sql"
INCR_BACKUP="/tmp/kis_incr_${DATE}.sql"

# Voll-Backup (wöchentlich, Sonntag)
if [ "$(date +%u)" -eq 7 ]; then
    pg_dump -h localhost -U kis -F c -f "$FULL_BACKUP" kis_db
    age --recipient "$BACKUP_RECIPIENT" -o "$BACKUP_DIR/kis_full_${DATE}.age" "$FULL_BACKUP"
    sha256sum "$BACKUP_DIR/kis_full_${DATE}.age" > "$BACKUP_DIR/kis_full_${DATE}.age.sha256"
    rm -f "$FULL_BACKUP"
    
    # Offsite (Cloud)
    aws s3 cp "$BACKUP_DIR/kis_full_${DATE}.age" s3://kis-backup/full/
    aws s3 cp "$BACKUP_DIR/kis_full_${DATE}.age.sha256" s3://kis-backup/full/
fi

# Inkrementell (täglich)
pg_dump -h localhost -U kis -F c --data-only -f "$INCR_BACKUP" kis_db
age --recipient "$BACKUP_RECIPIENT" -o "$BACKUP_DIR/kis_incr_${DATE}.age" "$INCR_BACKUP"
sha256sum "$BACKUP_DIR/kis_incr_${DATE}.age" > "$BACKUP_DIR/kis_incr_${DATE}.age.sha256"
rm -f "$INCR_BACKUP"

# Offsite (Cloud)
aws s3 cp "$BACKUP_DIR/kis_incr_${DATE}.age" s3://kis-backup/incremental/
aws s3 cp "$BACKUP_DIR/kis_incr_${DATE}.age.sha256" s3://kis-backup/incremental/

# Retention (30 Tage)
find "$BACKUP_DIR" -name "kis_incr_*.age" -mtime +30 -delete
find "$BACKUP_DIR" -name "kis_incr_*.age.sha256" -mtime +30 -delete
```

### Backup-Monitoring

- **Alerting**: Backup-Fehler → Alert (E-Mail, Slack)
- **Monitoring**: Backup-Größe, Dauer, letzter erfolgreicher Backup
- **Reporting**: Wöchensicherungsreport (E-Mail an IT-Leitung)

### Backup-Notfall

**Bei Datenverlust:**

1. Letztes gescheitertes Backup identifizieren
2. Restore in Testinstanz
3. Datenverlust quantifizieren (Zeitrahmen, betroffene Daten)
4. Incident-Response-Plan starten (Level 3)
5. Restore in Produktion (nach Freigabe)
6. Datenverlust dokumentieren und melden (DSGVO Art. 33)

## Erreichbarkeit und Logs

`GET /api/health` liefert Klartext `OK`. Das prüft die HTTP-Erreichbarkeit, nicht laufend alle Datenbankfunktionen. Der Listener startet erst nach Migrationen und Audit-Prüfung. Ein künftiger Readiness-Endpunkt muss DB- und Audit-Verfügbarkeit gesondert prüfen.

```bash
curl -fsS --max-time 5 http://127.0.0.1:8080/api/health
journalctl -u kis -f
```

Anwendungslogs gehen über tracing in die Standardausgabe, bei systemd ins Journal. `RUST_LOG` steuert die Filter. Eine allgemeine JSON-Logdatei oder `/metrics`, Prometheus-Regeln und Grafana-Dashboards sind noch nicht implementiert. Diagnostik darf keine Schlüssel, Passwörter oder Patientendaten ausgeben.

## Backup: Voraussetzungen

Benötigt werden Python 3.11+, age und ein geschütztes Backup-Verzeichnis. Die Datenbank wird über die SQLite-Backup-API konsistent kopiert, auch bei WAL-Nutzung. Verschlüsselt wird mit age; das ist nicht dasselbe wie die AES-GCM-Spaltenverschlüsselung des Krypto-Moduls.

Für ein neues Schlüsselmaterial einmalig auf einer dafür vorgesehenen sicheren Maschine:

```bash
umask 077
age-keygen -o backup-identity.txt
age-keygen -y backup-identity.txt > backup-recipients.txt
```

Die private Identity getrennt und gesichert aufbewahren. Auf dem Backup-Host genügt die öffentliche Empfängerdatei. Die Identity gehört nicht ins Repo, ins Container-Image oder neben allgemein zugängliche Backups.

## Verschlüsselten Snapshot erzeugen

Beispiel für absolute Pfade; Benutzer und Pfade an die Testinstanz anpassen:

```bash
python3 /opt/kis/scripts/backup.py backup \
  --database /var/lib/kis/kis.db \
  --destination /var/lib/kis-backup \
  --recipients /etc/kis/backup-recipients.txt
```

Für diesen Aufruf muss `scripts/backup.py` aus dem Checkout nach `/opt/kis/scripts/` übernommen sein. Der ausführende Benutzer braucht Lesezugriff auf die bestehende DB samt WAL-Zustand und Schreibzugriff auf das geschützte Zielverzeichnis.

Es entstehen ein eindeutig benanntes `kis-*.db.age` und genau dazu `kis-*.db.age.sha256`. Nach erfolgreichem Verschlüsseln bleibt kein Klartext-Snapshot im Arbeitsverzeichnis zurück. Die Ausgabe nennt den Archivpfad und Zeilenzahlen, keine Datensätze.

Bei Stromausfall kann ein unvollständiges Artefaktpaar zurückbleiben. Ein Archiv ohne passende Prüfsummendatei gilt nicht als geprüft. Das Werkzeug setzt keine automatische Aufbewahrungs-/Löschpolitik um.

## Entschlüsselung und Inhalt prüfen

Den exakten Archivpfad aus dem Backup auswählen:

```bash
python3 /opt/kis/scripts/backup.py verify \
  --archive /var/lib/kis-backup/kis-ausgewaehltes-backup.db.age \
  --identity /sicherer/pfad/backup-identity.txt
```

Der Beispielname ist durch einen existierenden Archivnamen zu ersetzen. Der Test prüft Dateiexistenz, Prüfsumme, Entschlüsselung, SQLite-Integrität, Fremdschlüssel und die erforderlichen KIS-Tabellen. Ein fehlender Pfad wird nicht als leere DB angelegt. Temporärer Klartext wird nach dem Test entfernt.

Zeilenzahlen anschließend mit dem erwarteten Bestand vergleichen. Ein strukturell gültiger Snapshot beweist allein weder vollständige fachliche Historie noch Erreichbarkeit eines festgelegten RPO.

## Restore

1. Archiv einschließlich Prüfsumme mit dem Werkzeug prüfen. Passendes Release, Konfiguration und Anwendungsschlüssel bereithalten.
2. Wartungsfenster beginnen, KIS und alle anderen Writer/DB-Clients stoppen.
3. Den bisherigen vollständigen Datenbestand einschließlich noch vorhandener WAL-/SHM-Dateien und Dateirechte in einem geschützten Verzeichnis sichern. Nicht nur die Hauptdatei wegkopieren und WAL ungeprüft löschen.
4. Das gewählte Archiv in ein privates temporäres Verzeichnis entschlüsseln. Dateninhalt und Integrität nochmals prüfen.
5. Den geprüften Snapshot als neue Hauptdatei installieren. Alte WAL-/SHM-Dateien dürfen nicht zusammen mit der neuen DB wiederverwendet werden. Besitzer-/Gruppenrechte und die geschützte Ablage wiederherstellen.
6. Dienst starten, Migration-/Audit-Logs, Login, Health sowie fachliche Zählungen prüfen.
7. Temporäre Klartextdateien entfernen und Wiederherstellung protokollieren.

Für die Entschlüsselung mit gesetzten Pfadvariablen:

```bash
umask 077
RESTORE_DIR="$(mktemp -d)"
age --decrypt --identity "$BACKUP_IDENTITY" \
  --output "$RESTORE_DIR/kis.db" "$BACKUP_ARCHIVE"
sqlite3 "$RESTORE_DIR/kis.db" 'PRAGMA integrity_check;'
```

`BACKUP_IDENTITY` und `BACKUP_ARCHIVE` müssen vorab auf die ausgewählten existierenden Dateien gesetzt sein. Dateiinstallation und Dienststeuerung hängen vom tatsächlichen Deployment ab. Die private temporäre Ablage nach erfolgreichem Restore vollständig entfernen.

Ein Checkpoint mit anschließendem sequenziellem Kopieren einer laufend beschriebenen DB ist kein konsistentes Rohdatei-Backup.

## Upgrade und Rollback

Vor dem Upgrade Release/Commit, bestehende Migrationen, Schlüssel und getesteten Snapshot sichern. Die vorherige Binärdatei vor Austausch aufbewahren. Der Server führt eingebettete SQLx-Migrationen aus und bricht bei Fehlern ab.

Für einen DB-Rollback den zur vorherigen Version passenden Snapshot zurückspielen; nicht Tabellen per improvisiertem DROP löschen. Bestehende angewandte Migrationen unterliegen SQLx-Prüfsummen. Ein neues Binary mit alten bzw. manuell geänderten Schemas braucht einen geprüften Migrationsplan.

Es existiert noch keine nachgewiesene signierte Release-Infrastruktur. Beispiel-Domains sind keine Downloadquellen.

## Betriebsgrenzen und offene Aufgaben

Aufbewahrung, Offsite-Kopien, Wiederherstellungsziele, Alarmierung und Verantwortliche sind betrieblich festzulegen. Regelmäßige Restore-Übungen gehören dazu. Für verschlüsselte Daten sind auch die Anwendungsschlüssel wiederherzustellen.

Der aktuelle Prototyp besitzt keine freigegebene Mehrinstanz-Skalierung, klinischen Workflows oder aktivierten Medizinschnittstellen. Eine Konfigurationsänderung kann diese fehlenden Implementierungen nicht ersetzen.
