# KIS — Architektur

> **Hinweis (06.11.2026):** Teile dieses Dokuments beschreiben den entfernten SQLite-Prototyp oder ein Zielbild. Maßgeblich für den Ist-Zustand sind [INSTALLATION](INSTALLATION.md), [API](API.md), [DATABASE](DATABASE.md) und [REVIEW_STATUS](REVIEW_STATUS.md).


Stand: 02.11.2026. Dieses Dokument trennt den aktiven Prototyp vom geplanten Ausbau.

## Aktiver Root-Build

`Cargo.toml` definiert das Paket `kis`, kein Cargo-Workspace und keine Modul-Features. `cargo build` kompiliert `src/` sowie die Administrations-CLI.

```text
KIS_CONFIG_PATH oder config/kis.toml + KIS_*-Umgebung
    → AppConfig mit Defaults und Validierung
    → Tokio-Runtime mit server.workers
    → SQLite-Pool + eingebettete Migrationen
    → Krypto-Initialisierung + Audit-Prüfung + Auth-Kontext
    → Axum-HTTP-Server
```

| Baustein | Verantwortung |
|---|---|
| `src/main.rs` | Konfiguration, Runtime, Start und geordnetes Beenden |
| `src/api/mod.rs` | Routen mit einem `/api`-Präfix |
| `src/config/mod.rs` | TOML, Defaults, ENV-Mapping und Validierung |
| `src/db/mod.rs` | SQLite-Pool, Dateianlage und verbindliche Migrationen |
| `src/security/` | Auth, Rollenprüfung, Audit, Limits und Krypto-Hilfen |
| `src/bin/kis-admin.rs` | Lokale Anlage eines Administrators |
| `scripts/backup.py` | Online-Snapshot, age-Verschlüsselung und Restore-Prüfung |

Der API-Stack verarbeitet Anfragen von außen nach innen als Trace → RateLimit → Auth → RBAC → Audit → Handler. Öffentliche Routen sind exakt Health, Login und Refresh. Die Root-Textseite gehört nicht zu diesem Stack.

## Datenbank und Audit

SQLite ist der aktive Speicher. Die konfigurierte Pool-Größe und der Journal-Modus werden angewendet; Foreign Keys, Busy-Timeout und `synchronous=FULL` werden für Verbindungen gesetzt. Der Server startet bei Migrationsfehlern nicht.

Audit-Schreibvorgänge warten auf den DB-Commit und werden innerhalb der Instanz serialisiert. Die JSONL-Datei ist eine zusätzliche Kopie. Für Abschneiden oder vollständiges Umschreiben der Hash-Kette fehlt noch ein externer Prüfanker. Parallele KIS-Instanzen mit einer gemeinsamen DB sind nicht als Betriebsmodell freigegeben.

## Noch nicht angeschlossene Entwürfe

`src/ui/mod.rs` und `static/index.html` enthalten einen GUI-Entwurf. Der aktive Server liefert unter `/` einen Text. Fachliche Handler liefern HTTP 501, keine Patientendaten.

Die Schnittstellen-Module enthalten Platzhalter sowie einen getrennt verwendbaren MEDICO-Client. Konfiguration allein startet keinen Schnittstellen-Server oder Pollingdienst. Auch für OpenRouter gibt es noch keinen ausführenden Client oder Modellwechsel.

## Geplantes Ziel

Der [Fahrplan](FAHRPLAN.md) sieht einen PostgreSQL-Workspace, eigenständige Module, Read-Replicas, Optimistic Locking, GUI und medizinische Schnittstellen vor. Die Dateien unter `crates/` sind Vorarbeiten und noch kein gültig verdrahteter Workspace.

Vor der Umstellung sind gültige Paket-Manifeste, Modulverträge, Datenmigration und Tests erforderlich. Zustandsabhängige Funktionen wie Token-Widerruf, Audit und Rate-Limits brauchen dabei ein abgestimmtes Mehrinstanz-Konzept. Das Ziel eines vollständigen MEDICO-Ersatzes ist noch nicht erreicht.
