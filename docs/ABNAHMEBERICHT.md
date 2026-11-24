# Vorlage — Abnahmebericht für Arbeitspaket / Meilenstein
Die Vorlage wird im PR oder verlinkten Nachweisbericht ausgefüllt. Keine echten Patientendaten, Secrets, vertraulichen Infrastrukturdetails oder personenbezogenen Logs im öffentlichen Repository. Nur tatsächlich ausgeführte Nachweise eintragen. Leere Felder und nicht ausgeführte Prüfungen bleiben offen. Regeln: [ROADMAP.md](ROADMAP.md), [AGENTS.md](../AGENTS.md), [SICHERHEITS_ABNAHME.md](SICHERHEITS_ABNAHME.md).

## Identität und Scope
- Paket-ID / Meilenstein / betroffene E-, TI-, DOC-, ED-IDs:
- Basis-SHA / Ergebnis-SHA / PR:
- Verantwortlicher Implementierer / technischer Gegenreview / zuständige Fach- und Security-Reviewer:
- Ziel, Nicht-Ziele, betroffene Dateien/Migrationen/Kernverträge:
- Abhängigkeiten und deren geprüfte Vertragsversionen:
- N/H/X je Teilfähigkeit, begründete Ausnahmen, Reviewdatum und Exit-Plan:
- Relevante Normen, Kataloge, Versionen, Lizenzen, Quellen:
- Risiko, Rechte-/Mandantenkontext, Audit, Betriebs-/Ausfallfolgen:
- Erwartete Akzeptanzszenarien und Messziele (vor Implementierung festlegen):

## Änderungen und Nachweise
| Gate | Status | Konkreter Nachweis / SHA / Datum / Umgebung | Verantwortlicher / unabhängiger Reviewer |
|---|---|---|---|
| G0 Definition of Ready | offen | – | – |
| G1 Architektur und Daten | offen | – | – |
| G2 Technik | offen | – | – |
| G3 Sicherheit und Lieferkette | offen | – | – |
| G4 Fachliche Funktion / Native First | offen | – | – |
| G5 Interoperabilität / Regulatorik | offen | – | – |
| G6 Betrieb | offen | – | – |
| G7 Gesamtfreigabe | offen | – | – |

| Prüfung | Status | Befehl/Test-ID, Ergebnis, SHA und Link |
|---|---|---|
| scripts/check.sh (echtes PostgreSQL) | offen | – |
| CI lint/test/audit/docs/container | offen | – |
| Paket-/Regressionstests | offen | – |
| Rechte-/Tenant-/Negativtests | offen | – |
| Browser-E2E / Native-First-Szenario | offen | – |
| Nebenläufigkeit / Fehler / Ausfall | offen | – |
| Migration / Export / Wiederherstellung | offen | – |
| Offizielle Validatoren / Gegenstellen | offen | – |
| Fachliche und regulatorische Abnahme | offen | – |

Nicht anwendbar nur mit konkretem Grund und unabhängiger Bestätigung; kein fehlender Test als N/A umdeklarieren. Keine fiktiven Reviewer. Falls ein Test wegen fehlender Infrastruktur nicht ausführbar ist, ist er offen bzw. blockiert. Relevante Nachänderung macht den bisherigen Nachweis ungültig.

## Reviews und Entscheidungen
- Feststellungen des Gegenreviews, Schweregrad, Behebung und Re-Test:
- Sicherheits-/Fach-/Datenschutzrisiken, Verantwortlicher, Frist und Nachweis:
- Externe Freigaben (zuständige Stelle, Scope, Version, Beleg) oder offen:
- Zulässige Ausnahme (Grund, Umfang, Ablaufdatum, Reviewer) oder keine:
- Update von Funktionslandkarte, Paketstatus und Nachweisregister:
- Entscheidung: Review offen / technisch integrierbar / technisch integriert / fachlich abgenommen / extern bestätigt / betriebsfreigegeben (Scope ausdrücklich nennen):
- Restarbeiten/Blocker, nächstes Paket und Abhängigkeit:
- Merge- und main-CI-Nachweis (erst nach tatsächlicher Integration):

**Technisch integriert bedeutet nicht produktionsreif.** Für echte Patientennutzung gelten G5–G7 und die ausdrückliche Betreiberfreigabe; im aktuellen Entwicklungsauftrag ausschließlich synthetische Daten.
