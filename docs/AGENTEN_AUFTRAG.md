# KIS — Auftrag für Agenten

Stand: 24.11.2026

## Auftrag des Benutzers

Ein umfassendes, modulares Krankenhaus-Informations-System, mit dem alle
Beschäftigten eines Hauses ihre tägliche Arbeit erledigen und in dem alle
Daten ineinandergreifen — für kleine Kliniken bis Maximalversorger. Mit
Datenschutz und Datensicherheit, vollständig konfigurierbar und sparsam im
Umgang mit Rechnerressourcen. Einzelne Module müssen jederzeit geändert und
verbessert werden können, ohne den Gesamtablauf zu stören.

Der Auftrag umfasst ausdrücklich vollständige Klinikverbünde, Schwesterkliniken, getrennte Rechtsträger, MVZ und Facharztpraxen, sektorenübergreifende Versorgung sowie sämtliche anwendbaren gematik-/TI-/KBV-Anforderungen. Verbindliche Soll-Anforderungen: [ENTERPRISE_KIS.md](ENTERPRISE_KIS.md), [TI_COMPLIANCE.md](TI_COMPLIANCE.md); Umsetzung: [ENTERPRISE_BACKLOG.md](ENTERPRISE_BACKLOG.md). Keine Produktivfreigabe.

## Wo was steht

| Frage | Dokument |
|---|---|
| Welche Regeln gelten? | [AGENTS.md](../AGENTS.md) |
| Wie baue oder ändere ich ein Modul? | [MODULE.md](MODULE.md) |
| Was ist zu tun? | [ARBEITSPAKETE.md](ARBEITSPAKETE.md) |
| Was gibt es schon, was fehlt? | [FUNKTIONSLANDKARTE.md](FUNKTIONSLANDKARTE.md) |
| Wann ist etwas fertig? | wenn `scripts/check.sh` fehlerfrei durchläuft und die Abnahmepunkte des Pakets als Tests vorhanden sind |

## Reihenfolge

0. EP-00 bis EP-07: Verbundarchitektur, MPI, Fallmodell, Rechte, Ereignisverträge, Normen und Betrieb zuerst abstimmen. Danach EP-Pakete gemäß Abhängigkeiten in Teilpakete zerlegen. Unabhängige Härtung darf parallel erfolgen.
1. Aufräumen und Härten (AP-01 bis AP-06): klein, unabhängig, guter
   Einstieg.
2. Fachmodule erweitern (AP-10 bis AP-19) und Oberfläche (AP-40 bis
   AP-43): voneinander unabhängig, parallel bearbeitbar — je Paket ein
   Branch.
3. Schnittstellen (AP-30, AP-31) und die Bereiche unter „Braucht eine
   Entscheidung des Hauses“ erst nach Klärung der dort genannten Fragen.

Änderungen am Kern (`kis-core`, Kern-Tabellen, Ereignisse) sind
Integrationsarbeit: einzeln vorschlagen, nicht nebenbei in einem
Fachpaket.

Frühere Fassungen dieses Auftrags (Etappe PG-00 bis PG-10, Stand
02.11.2026) sind mit dem Stand vom 06.11.2026 erledigt und in der
Versionsgeschichte nachzulesen.
