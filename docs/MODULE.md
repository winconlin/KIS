# Module bauen und ändern

Das KIS besteht aus einem kleinen Kern und Fachmodulen. Jedes Modul ist
eine eigene Crate und kann für sich geändert, ersetzt oder abgeschaltet
werden, ohne dass ein anderes Modul oder der Ablauf im Haus davon
betroffen ist. Diese Seite beschreibt die Regeln, die das sicherstellen —
und die Prüfungen, die sie erzwingen.

Vorlage zum Nachbauen: [`crates/kis-nursing`](../crates/kis-nursing/src/lib.rs)
(klein, zeigt jede Regel einmal).

## Aufbau

| Schicht | Crates | Darf |
|---|---|---|
| Kern | `kis-core` | nichts Fachliches; hängt von keinem Modul ab |
| Kernmodule | `kis-org`, `kis-patients`, `kis-cases` | Kern-Tabellen schreiben; nicht abschaltbar |
| Fachmodule | `kis-clinical`, `kis-orders`, `kis-lab`, `kis-nursing`, `kis-prescriptions`, `kis-meals`, `kis-roster`, `kis-doctors`, `kis-appointments`, `kis-medications`, `kis-ui` | eigene Tabellen schreiben, Kern-Tabellen lesen |
| Anwendung | `kis-app` | setzt alles zusammen, enthält keine Fachlogik |

Abhängigkeiten zeigen immer vom Modul auf den Kern. Nie von Modul zu Modul.

## Die sechs Regeln

1. **Keine Route ohne Zugriffsregel.** Routen entstehen nur über
   `ModuleRoutes`; jede Registrierung verlangt ein `Access`. Unbekannte
   Pfade unter `/api/` sind nur für Admins erreichbar.
2. **Falldaten liegen unter `/api/v1/cases/:case_id/…`**, Arbeitslisten
   einer Einheit unter `/api/v1/units/:unit_id/…`. Dann prüft der Kern vor
   dem Handler den Behandlungszusammenhang bzw. die Zugehörigkeit zur
   Einheit. Der Handler filtert zusätzlich mit `WHERE case_id = $1`.
3. **Eigene Tabellen, fremde tabu.** Ein Modul schreibt nur in seine
   Tabellen ([`config/module_tables.toml`](../config/module_tables.toml))
   und liest daneben nur Kern-Tabellen.
4. **Zusammenspiel über Ereignisse und Kernfunktionen.** Ein Modul meldet,
   was geschehen ist (`ctx.events.publish(…)`); andere reagieren
   (`Module::subscribers`). Schlägt eine Reaktion fehl, läuft der
   auslösende Vorgang weiter, und der Fehler steht in
   `/api/v1/admin/event-failures`. Kern-Daten ändert ein Fachmodul nur über
   Kernfunktionen:

   | Zweck | Funktion |
   |---|---|
   | Einheit behandelt mit (Zugriff auf den Fall) | `treatment::add_participant`, `end_participant` |
   | Bett sperren / freigeben | `facility::block_bed`, `release_bed` |
   | Audit-Eintrag | `http::audit` |
   | Ereignis melden | `events::EventBus::publish` |

5. **Schreiben = Transaktion + Audit.** Fachliche Änderung und
   Audit-Eintrag in derselben Transaktion. Im Audit stehen Schlüssel und
   Codes, keine Messwerte und kein Freitext.
6. **Die Datenbank schützt die Daten.** Wertebereiche, Eindeutigkeit und
   zeitliche Überschneidungen stehen als Constraints in der Migration. Der
   Handler reicht Fehler mit `?` durch; daraus werden 400 bzw. 409.
   Angewandte Migrationen werden nie geändert.

## Ein neues Modul in sieben Schritten

```sh
python3 scripts/new_module.py pharmacy "Apotheke"
```

Das legt die Crate an, trägt sie überall ein und erzeugt einen lauffähigen
Test. Das Modul ist zunächst `Experimental` (gebaut und getestet, aber nur
aktiv, wenn es in `[modules] experimental` steht). Danach:

1. **Tabellen**: neue Datei `migrations/NNN_<thema>.sql` (nächste Nummer).
   Fallbezogene Tabellen bekommen `case_id UUID NOT NULL REFERENCES
   cases(id) ON DELETE CASCADE`. Tabellen in `config/module_tables.toml`
   dem Modul zuordnen.
2. **Rechte**: in [`kis-core/src/rbac.rs`](../crates/kis-core/src/rbac.rs)
   Variante, `ALL` und `key()` ergänzen (der Test `all_is_complete`
   erzwingt Vollständigkeit) und bei Bedarf in `BUILTIN_ROLES` aufnehmen.
3. **Routen und Handler** in `src/lib.rs` nach dem Muster von
   `kis-nursing`. Helfer aus `kis_core::http`: `json_sql`, `list`, `one`,
   `camel`, `audit`, `actor_id`, `required_text`, `optional_text`,
   `missing_or_stale`, `page_limit`.
4. **Auskunft**: fallbezogene Tabellen in `CASES_EXPORT_SQL`
   ([`data_portability.rs`](../crates/kis-core/src/data_portability.rs))
   aufnehmen. Der Test `export_covers_all_case_tables` schlägt sonst fehl.
5. **Tests** in `crates/kis-app/tests/<name>.rs`: Erfolgsfall,
   Rechteprüfung (403), Behandlungszusammenhang (403 für Fremde),
   ungültige Eingaben (400), Konflikte (409). Muster:
   [`tests/hospital.rs`](../crates/kis-app/tests/hospital.rs).
6. **Prüfen**: `scripts/check.sh` läuft ohne Fehler durch. Dann
   `python3 scripts/check_guardrails.py --add-migration`.
7. **Freigeben**: `Maturity::Stable` setzen, Eintrag in
   [`FUNKTIONSLANDKARTE.md`](FUNKTIONSLANDKARTE.md) und
   [`API.md`](API.md) aktualisieren.

## Ein bestehendes Modul ändern

- Änderungen bleiben in der Crate des Moduls. Wer dafür eine andere Crate
  anfassen muss, hat meist eine Modulgrenze verletzt — dann ist ein
  Ereignis oder eine Kernfunktion der richtige Weg.
- Schema-Änderungen sind neue Migrationen. Bestehende Spalten werden nicht
  umgedeutet; Antworten der API behalten ihre Felder (neue Felder dürfen
  hinzukommen).
- Bestehende Tests werden nicht abgeschwächt oder stillgelegt. Ändert sich
  gewolltes Verhalten, wird der Test mit Begründung im Commit angepasst.

## Der Vertrag des Kerns

Darauf verlassen sich alle Module; Änderungen hier sind
Integrationsarbeit und werden einzeln abgestimmt:

- Kern-Tabellen und ihre Spalten (Migrationen 001–006),
- Ereignisse und ihre Nutzlast:

  | Thema | Nutzlast (nur Schlüssel und Codes) |
  |---|---|
  | `case.admitted` | `kind`, `department_id`, `ward_id`, `bed_id` |
  | `case.transferred` | `kind`, `from_department_id`, `from_ward_id`, `freed_bed_id`, `department_id`, `ward_id`, `bed_id` |
  | `case.discharged` | `kind`, `department_id`, `ward_id`, `freed_bed_id`, `discharge_type` |
  | `case.cancelled` | `kind`, `department_id`, `ward_id`, `freed_bed_id` |
  | `order.completed` | `case_id`, `has_result` |

- die Typen und Funktionen aus `kis_core::{module, http, identity, rbac,
  treatment, facility, events}`.

## Was die Prüfungen abdecken

| Prüfung | erzwingt |
|---|---|
| Typsystem (`ModuleRoutes`) | Regel 1; Namenskonvention `:case_id` / `:unit_id` |
| `scripts/check_modules.py` | Regeln 3 und 4, Abhängigkeiten, Registrierung |
| `scripts/check_guardrails.py` | unveränderte Migrationen, kein `todo!`/TODO, keine stillgelegten Tests |
| `scripts/check_sql.py` | jede SQL-Anweisung passt zum migrierten Schema |
| Test `export_covers_all_case_tables` | Auskunft nach Art. 15 bleibt vollständig |
| Integrationstests | Rechte, Behandlungszusammenhang, Abläufe über Modulgrenzen |
| `scripts/check.sh` | alles zusammen, ein Befehl |

Nicht automatisch prüfbar und deshalb Sache des Reviews: ob ein Handler
wirklich mit `WHERE case_id = $1` filtert, ob Audit-Details frei von
Personendaten sind, ob eine fachliche Regel medizinisch richtig ist.
