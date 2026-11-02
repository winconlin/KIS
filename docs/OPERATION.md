# KIS — Betrieb und Wiederherstellung

Stand: 02.11.2026 · Technische Abläufe für Testinstanzen

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
