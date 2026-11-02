# KIS — HTTP-API

Stand: 02.11.2026 · Aktiver Root-Build

Basis für lokale Entwicklung: `http://127.0.0.1:8080`. Das Präfix `/api` wird genau einmal registriert. FHIR- und XDS-Routen sind noch nicht vorhanden.

## Registrierte Routen

| Methode | Pfad | Zugriff | Verhalten |
|---|---|---|---|
| GET | `/` | Öffentlich | Text mit Prototyp-Hinweis |
| GET | `/api/health` | Öffentlich | 200, Klartext `OK` |
| POST | `/api/auth/login` | Öffentlich | Passwort prüfen, Token-Paar ausstellen |
| POST | `/api/auth/refresh` | Refresh-Token im JSON-Body | Atomare Rotation |
| POST | `/api/auth/logout` | Access-Token + eigenes Refresh-Token | Sitzung widerrufen, 204 |
| POST | `/api/auth/mfa/setup` | Authentifiziert | 501, Setup noch nicht implementiert |
| GET | `/api/patients` | Alle vier Rollen | 501 |
| GET | `/api/patients/:id` | Alle vier Rollen | 501 |
| POST | `/api/patients` | admin, reception | 501 |
| GET, POST | `/api/appointments` | Alle vier Rollen | 501 |
| GET | `/api/medications` | admin, doctor, nurse | 501 |

„Alle vier Rollen“ meint admin, doctor, nurse und reception. Eine Rollenangabe in der URL verändert keine Berechtigungen. Patientenbezogene Fall-/Teamzuordnungen sind vor einer späteren CRUD-Implementierung zusätzlich umzusetzen.

CRUD, Paginierung, Idempotency-Key, Maskierung, Termin-Konfliktprüfung und die weiteren PUT/DELETE-/Arzt-Routen sind geplant. Aus einer registrierten Platzhalterroute folgt noch keine fachliche Funktion.

## Login

```http
POST /api/auth/login
Content-Type: application/json

{"username":"admin","password":"ein-lokal-gesetztes-passwort"}
```

Ein erfolgreiches Login liefert folgende Struktur; Tokenwerte sind hier Platzhalter:

```json
{
  "access_token": "<access-token>",
  "refresh_token": "<refresh-token>",
  "token_type": "Bearer",
  "expires_in": 900,
  "role": "admin"
}
```

Der Administrator wird über die lokale CLI angelegt; es gibt keinen Standardbenutzer und kein Standardpasswort. Die Anleitung steht in [INSTALLATION.md](INSTALLATION.md).

## Tokens und Sitzungen

HS256 verwendet den aus `KIS_JWT_SECRET` gelesenen Schlüssel. Standardlaufzeiten: Access 900 Sekunden, Refresh 86400 Sekunden. Die Token-IDs beider Tokens werden einer persistenten Sitzung zugeordnet.

Datenrouten verlangen `Authorization: Bearer ACCESS_TOKEN`. Ein Refresh-Token ist dort ungültig. Signatur und Ablauf allein reichen nicht aus: Sitzung, aktives Benutzerkonto und aktuelle Rolle werden ebenfalls geprüft.

Refresh erwartet `{"refresh_token":"<refresh-token>"}`. Die alte Sitzung wird atomar widerrufen und durch ein neues Token-Paar ersetzt. Das macht auch das alte Access-Token ungültig. Zwei gleichzeitige Refresh-Anfragen können nicht beide gewinnen. Bei Logout wird die eigene Sitzung ebenfalls für beide Tokens widerrufen. Der Widerruf überlebt einen Neustart.

Tokens werden als JSON zurückgegeben; automatische HttpOnly-Cookies, Concurrent-Session-Limit und Inaktivitäts-Logout sind nicht implementiert. Der Client muss Tokens sicher behandeln.

## Fehler und Limits

Eigene API-Fehler verwenden diese Struktur:

```json
{"error":"Bearer-Token erforderlich","code":"missing_token","status":401}
```

Extractor-Fehler von Axum, etwa syntaktisch ungültiges JSON, sowie nicht registrierte Routen können andere Antwortkörper liefern.

| Status | Bedeutung |
|---|---|
| 401 | Zugangsdaten oder Token fehlen/ungültig |
| 403 | Rolle nicht berechtigt |
| 423 | Benutzerkonto nach Fehlversuchen gesperrt |
| 429 | Rate-Limit, mit Retry-After |
| 500 | Interner Auth-/Datenbankfehler |
| 501 | Registrierte Funktion noch nicht implementiert |
| 503 | Audit-DB-Schreibvorgang fehlgeschlagen |

Der Sliding-Window-Limiter zählt pro tatsächlicher TCP-Gegenstellen-IP: 100 Anfragen/Minute für die API und 10/Minute für Auth-Routen. Er liegt vor der Authentifizierung. Hinter einem Proxy teilen Anfragen dessen IP-Limit; Forwarded-/X-Forwarded-For-Header werden nicht als vertrauenswürdig ausgewertet. Zusätzliche Proxy-Limits müssen dazu passen.

Cross-Origin-Zugriff ist nicht freigegeben. Es werden keine permissiven CORS-Header gesetzt.

## Lokal prüfen

```bash
curl -fsS http://127.0.0.1:8080/api/health
```

Für Login-/Refresh- und Negativtests siehe `src/security/tests.rs`. Die erfolgreiche Antwort eines Daten-Platzhalters ist weiterhin HTTP 501, nicht ein erfundenes JSON-Datenobjekt.
