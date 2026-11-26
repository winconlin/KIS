#!/usr/bin/env python3
"""Prüft die kanonischen M01-JSON-Fixtures und die dokumentenübergreifende
ID-Konsistenz von EP-00 bis EP-26 (ohne Rust-Compiler oder PostgreSQL).

Prüfumfang:
  1. docs/vertraege/verbund_test_fixture.json:
     - synthetic_only == true
     - UUID-Formatprüfung für organization_group, legal_entities, sites
     - vollständige Referenzintegrität (sites -> legal_entities,
       org_units -> sites/legal_entities/parent_code inkl. Zyklenfreiheit
       und Mandantengrenze, service_agreements, users, patients, NVT-01..18)
  2. docs/vertraege/normative_requirements_seed.json:
     - eindeutige requirement_code, lückenloses TI01..TI15
     - gültige https://-Quellen, N/H/X-Klassifizierung
     - ehrlicher Status (keine unbewiesene externe Bestätigung / Betriebsfreigabe)
  3. Dokumentenübergreifende Konsistenzprüfung:
     - EP-00..EP-26 in docs/ROADMAP.md und docs/NATIVE_FIRST_REGISTER.md
     - E01..E24 und TI01..TI15 in docs/NATIVE_FIRST_REGISTER.md
     - T01..T24 in docs/THREAT_MODEL_VERBUND.md
     - NVT-01..NVT-18 in docs/VERBUND_TESTMODELL.md
     - DEC-01..DEC-09 in docs/EP00_ARCHITEKTUR_UND_ANALYSE.md
     - keine veralteten v1-Angaben (z. B. M00–M10)

Verwendung:
    python3 scripts/check_vertraege.py
    python3 scripts/check_vertraege.py --json
"""

from __future__ import annotations

import json
import pathlib
import re
import sys
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERBUND_FIXTURE = ROOT / "docs" / "vertraege" / "verbund_test_fixture.json"
NORMATIVE_SEED = ROOT / "docs" / "vertraege" / "normative_requirements_seed.json"

ALLOWED_PRE_IMPL_STATUSES = {
    "geplant",
    "spezifiziert",
    "in_bearbeitung",
    "blockiert",
    "im_review",
}


def is_valid_uuid(value: str) -> bool:
    try:
        uuid.UUID(str(value))
        return True
    except ValueError:
        return False


def check_verbund_fixture() -> tuple[list[str], dict[str, int]]:
    errors: list[str] = []
    stats: dict[str, int] = {}
    if not VERBUND_FIXTURE.is_file():
        return [f"{VERBUND_FIXTURE.relative_to(ROOT)}: Datei fehlt"], stats

    try:
        data = json.loads(VERBUND_FIXTURE.read_text(encoding="utf-8"))
    except ValueError as exc:
        return [f"{VERBUND_FIXTURE.relative_to(ROOT)}: Ungültiges JSON: {exc}"], stats

    if data.get("synthetic_only") is not True:
        errors.append("verbund_test_fixture.json: synthetic_only muss true sein")

    group = data.get("organization_group", {})
    if not is_valid_uuid(group.get("id", "")):
        errors.append("verbund_test_fixture.json: organization_group.id ist keine gültige UUID")
    if group.get("grants_data_access") is not False:
        errors.append("verbund_test_fixture.json: organization_group.grants_data_access muss false sein")

    legal_entities = data.get("legal_entities", [])
    le_codes: set[str] = set()
    for le in legal_entities:
        code = le.get("code", "")
        if not code or code in le_codes:
            errors.append(f"verbund_test_fixture.json: ungültiger/doppelter legal_entity code '{code}'")
        le_codes.add(code)
        if not is_valid_uuid(le.get("id", "")):
            errors.append(f"verbund_test_fixture.json: legal_entity '{code}' hat ungültige UUID")

    sites = data.get("sites", [])
    site_map: dict[str, str] = {}
    for site in sites:
        code = site.get("code", "")
        le_code = site.get("legal_entity_code", "")
        if not code or code in site_map:
            errors.append(f"verbund_test_fixture.json: ungültiger/doppelter site code '{code}'")
        if le_code not in le_codes:
            errors.append(f"verbund_test_fixture.json: site '{code}' verweist auf unbekannten Rechtsträger '{le_code}'")
        if not is_valid_uuid(site.get("id", "")):
            errors.append(f"verbund_test_fixture.json: site '{code}' hat ungültige UUID")
        site_map[code] = le_code

    units = data.get("org_units", [])
    unit_map: dict[str, dict] = {}
    for unit in units:
        code = unit.get("code", "")
        if not code or code in unit_map:
            errors.append(f"verbund_test_fixture.json: ungültiger/doppelter org_unit code '{code}'")
        unit_map[code] = unit

    for code, unit in unit_map.items():
        le_code = unit.get("legal_entity_code", "")
        site_code = unit.get("site_code", "")
        if le_code not in le_codes:
            errors.append(f"verbund_test_fixture.json: org_unit '{code}' hat unbekannten Rechtsträger '{le_code}'")
        if site_code not in site_map:
            errors.append(f"verbund_test_fixture.json: org_unit '{code}' hat unbekannten Standort '{site_code}'")
        elif site_map[site_code] != le_code:
            errors.append(
                f"verbund_test_fixture.json: org_unit '{code}' ({le_code}) verletzt Mandantengrenze zu site '{site_code}' ({site_map[site_code]})"
            )
        parent = unit.get("parent_code")
        if parent is not None:
            if parent not in unit_map:
                errors.append(f"verbund_test_fixture.json: org_unit '{code}' hat unbekannte Elterneinheit '{parent}'")
            elif unit_map[parent].get("legal_entity_code") != le_code:
                errors.append(
                    f"verbund_test_fixture.json: org_unit '{code}' überschreitet Rechtsträgergrenze zu parent '{parent}'"
                )

    # Zyklenfreiheit auf org_units.parent_code prüfen
    for code in unit_map:
        visited: set[str] = set()
        curr = code
        while curr is not None:
            if curr in visited:
                errors.append(f"verbund_test_fixture.json: Zyklus in org_units erkannt ab '{code}'")
                break
            visited.add(curr)
            curr = unit_map[curr].get("parent_code") if curr in unit_map else None

    for sa in data.get("service_agreements", []):
        p_le = sa.get("provider_legal_entity_code")
        c_le = sa.get("consumer_legal_entity_code")
        p_unit = sa.get("provider_unit_code")
        if p_le not in le_codes or c_le not in le_codes:
            errors.append(f"verbund_test_fixture.json: service_agreement '{sa.get('code')}' hat ungültigen Rechtsträger")
        if p_le == c_le:
            errors.append(f"verbund_test_fixture.json: service_agreement '{sa.get('code')}' muss zwei verschiedene Rechtsträger verbinden")
        if p_unit not in unit_map:
            errors.append(f"verbund_test_fixture.json: service_agreement '{sa.get('code')}' hat unbekannte provider_unit '{p_unit}'")

    users = data.get("users", [])
    user_codes: set[str] = set()
    for user in users:
        code = user.get("code", "")
        if not code or code in user_codes:
            errors.append(f"verbund_test_fixture.json: ungültiger/doppelter user code '{code}'")
        user_codes.add(code)
        le_code = user.get("legal_entity_code")
        if le_code is not None and le_code not in le_codes:
            errors.append(f"verbund_test_fixture.json: user '{code}' verweist auf unbekannten Rechtsträger '{le_code}'")
        if not str(user.get("email", "")).endswith("@test.invalid"):
            errors.append(f"verbund_test_fixture.json: user '{code}' muss synthetische Domain '@test.invalid' nutzen")
        for uc in user.get("unit_codes", []):
            if uc not in unit_map:
                errors.append(f"verbund_test_fixture.json: user '{code}' hat unbekannte Einheit '{uc}'")

    patients = data.get("patients", [])
    pat_codes: set[str] = set()
    for pat in patients:
        code = pat.get("code", "")
        if not code or code in pat_codes:
            errors.append(f"verbund_test_fixture.json: ungültiger/doppelter patient code '{code}'")
        pat_codes.add(code)
        for lc in pat.get("legal_entities", []):
            if lc not in le_codes:
                errors.append(f"verbund_test_fixture.json: patient '{code}' hat unbekannten Rechtsträger '{lc}'")
    for pat in patients:
        mother = pat.get("mother_patient_code")
        if mother is not None and mother not in pat_codes:
            errors.append(f"verbund_test_fixture.json: patient '{pat.get('code')}' hat unbekannte mother_patient_code '{mother}'")

    nvts = data.get("negative_tests", [])
    nvt_ids = [n.get("id") for n in nvts]
    expected_nvt_ids = [f"NVT-{i:02d}" for i in range(1, 19)]
    if nvt_ids != expected_nvt_ids:
        errors.append(f"verbund_test_fixture.json: negative_tests müssen exakt NVT-01..NVT-18 sein (gefunden: {len(nvt_ids)})")
    for nvt in nvts:
        for actor in nvt.get("actors", []):
            if actor not in user_codes:
                errors.append(f"verbund_test_fixture.json: {nvt.get('id')} verweist auf unbekannten Akteur '{actor}'")
        pat_ref = nvt.get("patient")
        if pat_ref is not None and pat_ref not in pat_codes:
            errors.append(f"verbund_test_fixture.json: {nvt.get('id')} verweist auf unbekannten Patienten '{pat_ref}'")

    stats = {
        "legal_entities": len(le_codes),
        "sites": len(site_map),
        "org_units": len(unit_map),
        "users": len(user_codes),
        "patients": len(pat_codes),
        "negative_tests": len(nvts),
    }
    return errors, stats


def check_normative_seed() -> tuple[list[str], int]:
    errors: list[str] = []
    if not NORMATIVE_SEED.is_file():
        return [f"{NORMATIVE_SEED.relative_to(ROOT)}: Datei fehlt"], 0

    try:
        data = json.loads(NORMATIVE_SEED.read_text(encoding="utf-8"))
    except ValueError as exc:
        return [f"{NORMATIVE_SEED.relative_to(ROOT)}: Ungültiges JSON: {exc}"], 0

    reqs = data.get("requirements", [])
    seen: set[str] = set()
    for req in reqs:
        code = req.get("requirement_code", "")
        if not code or code in seen:
            errors.append(f"normative_requirements_seed.json: doppelter/leerer Code '{code}'")
        seen.add(code)
        if req.get("native_class") not in {"N", "H", "X"}:
            errors.append(f"normative_requirements_seed.json: '{code}' hat ungültige native_class")
        status = req.get("status", "")
        if status not in ALLOWED_PRE_IMPL_STATUSES:
            errors.append(
                f"normative_requirements_seed.json: '{code}' hat unzulässigen vorzeitigen Status '{status}'"
            )
        url = str(req.get("source_url", ""))
        if not url.startswith("https://"):
            errors.append(f"normative_requirements_seed.json: '{code}' braucht https://-Quelle")

    for idx in range(1, 16):
        ti_code = f"TI{idx:02d}"
        if ti_code not in seen:
            errors.append(f"normative_requirements_seed.json: Pflicht-Anforderung {ti_code} fehlt")

    return errors, len(reqs)


def check_doc_id_consistency() -> list[str]:
    errors: list[str] = []
    checks = [
        ("docs/ROADMAP.md", [f"EP-{i:02d}" for i in range(0, 27)]),
        ("docs/NATIVE_FIRST_REGISTER.md", [f"E{i:02d}" for i in range(1, 25)] + [f"TI{i:02d}" for i in range(1, 16)]),
        ("docs/THREAT_MODEL_VERBUND.md", [f"T{i:02d}" for i in range(1, 25)]),
        ("docs/VERBUND_TESTMODELL.md", [f"NVT-{i:02d}" for i in range(1, 19)] + [f"PAT-{i:02d}" for i in range(1, 8)]),
        ("docs/EP00_ARCHITEKTUR_UND_ANALYSE.md", [f"ADR-{i:03d}" for i in range(1, 8)] + [f"DEC-{i:02d}" for i in range(1, 10)]),
    ]
    for rel_path, expected_ids in checks:
        path = ROOT / rel_path
        if not path.is_file():
            errors.append(f"{rel_path}: Datei fehlt")
            continue
        text = path.read_text(encoding="utf-8")
        for item_id in expected_ids:
            if not re.search(rf"\b{re.escape(item_id)}\b", text):
                errors.append(f"{rel_path}: Pflicht-Kennung '{item_id}' fehlt")

    # Schutz gegen bekannte v1-Altlasten
    for path in sorted((ROOT / "docs").rglob("*.md")):
        if path.name == "EP-00-konsistenzpruefung.md":
            continue
        text = path.read_text(encoding="utf-8")
        if "M00–M10" in text or "M00-M10" in text:
            errors.append(f"{path.relative_to(ROOT)}: veralteter Meilensteinbereich M00–M10 gefunden (korrekt: M00–M07)")

    return errors


def main() -> int:
    as_json = "--json" in sys.argv[1:]
    v_errors, v_stats = check_verbund_fixture()
    n_errors, n_count = check_normative_seed()
    c_errors = check_doc_id_consistency()
    all_errors = v_errors + n_errors + c_errors

    summary = {
        "verbund_fixture": v_stats,
        "normative_requirements": n_count,
        "errors": all_errors,
        "error_count": len(all_errors),
    }

    if as_json:
        print(json.dumps(summary, ensure_ascii=False))
    else:
        for err in all_errors:
            print(err)
        print(
            f"Verträge & Fixtures geprüft ({v_stats.get('legal_entities', 0)} Rechtsträger, "
            f"{v_stats.get('negative_tests', 0)} NVTs, {n_count} Normen), {len(all_errors)} Fehler",
            file=sys.stderr,
        )
    return 1 if all_errors else 0


if __name__ == "__main__":
    sys.exit(main())
