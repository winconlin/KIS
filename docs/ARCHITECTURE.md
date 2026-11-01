# KIS Architektur

## Komponenten

### Backend (Rust)
- **Axum** — HTTP-Server (Web API + GUI)
- **SQLX** — Async-Datenbankzugriff (SQLite im WAL-Modus)
- **Tokio** — Async Runtime

### Datenbank
SQLite im WAL-Modus für maximale Performance auf minimaler Hardware.
Kein externer DB-Server nötig.

### GUI
Server-generiertes HTML mit Dark Theme.
Später: WebAssembly oder Tauri für native Performance.

### Schnittstellen
| Schnittstelle | Protokoll | Port | Status |
|--------------|-----------|------|--------|
| HL7 v2.5 | MLLP (TCP) | 6667 | TODO |
| FHIR R4 | REST/JSON | 8080/fhir | TODO |
| DICOM | C-STORE SCP | 104 | TODO |
| XDS | REST/SOAP | 8080/xds | TODO |

### LLM-Integration (OpenRouter)
Modelle rotieren automatisch und überprüfen sich gegenseitig:
1. Modell A generiert Code
2. Modell B reviewt und gibt Feedback
3. Modell A integriert Feedback
4. Wiederholung bis Qualitätsschwelle erreicht

Konfiguration in `config/kis.toml` unter `[llm]`.
