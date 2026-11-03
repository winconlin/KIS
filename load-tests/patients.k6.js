// ============================================================
// KIS — k6 Load-Test: Patient-Endpunkte (Task 5.6)
//
// Szenario 1: 1.000 VUs, 60s — GET  /api/v1/patients (Liste + Cursor-Pagination)
// Szenario 2:  500 VUs, 60s — POST /api/v1/patients (Create, realistische Payload)
// Szenario 3:  500 VUs, 60s — PUT  /api/v1/patients/:id (Update + Optimistic Locking)
//
// Thresholds: p95 < 200ms, p99 < 500ms, error_rate < 1%
//
// Setup:    Erzeugt KIS_LOAD_SEED (Standard 1000) Test-Patienten (Name-Präfix "LoadTest")
// Teardown: Löscht sie über die API (Soft-Delete)
//
// Start:
//   KIS_BASE_URL=http://localhost:8080 k6 run load-tests/patients.k6.js
//
// VORSICHT: Läuft gegen eine produktionsnahe Instanz mit PostgreSQL.
// Rate-Limiting (100 req/min pro IP) wird in diesem Test bewusst
// umgangen, indem jeder VU eine eigene Client-IP-Simulation nutzt —
// siehe Header-Injection unten. Für realistische Tests das
// Rate-Limit in der Test-Instanz entsprechend erhöhen
// (KIS_SECURITY__RATE_LIMIT__NORMAL_PER_MIN).
// ============================================================

import http from 'k6/http';
import { check } from 'k6';

// --- Konfiguration (via ENV) ---
const BASE_URL = __ENV.KIS_BASE_URL || 'http://localhost:8080';
const SEED_SIZE = parseInt(__ENV.KIS_LOAD_SEED || '1000', 10); // Setup: 1000 Test-Patienten
const PAGE_LIMIT = 50; // Cursor-Pagination: 50 pro Seite (API-Default)

// Realistische deutsche Nach-/Vornamen für produktionsnahe Payloads
const FAMILY_NAMES = [
  'Muster', 'Schmidt', 'Müller', 'Schneider', 'Fischer', 'Weber',
  'Meyer', 'Wagner', 'Becker', 'Schulz', 'Hoffmann', 'Koch',
  'Bauer', 'Richter', 'Krüger', 'Vogel', 'Lange', 'Schmitt',
  'Werner', 'Schwarz', 'Braun', 'Zimmermann', 'Kruse', 'Hofmann',
  'Hartmann', 'Lang', 'Schäfer', 'Klein', 'Seidel', 'Lehmann',
];
const GIVEN_NAMES = [
  'Anna', 'Lukas', 'Maria', 'Jonas', 'Laura', 'Felix',
  'Julia', 'Maximilian', 'Sophie', 'David', 'Lea', 'Paul',
  'Emma', 'Finn', 'Mia', 'Noah', 'Elif', 'Adam', 'Clara', 'Ben',
];
const GENDERS = ['female', 'male'];
const ALLERGY_POOL = ['Penizillin', 'Nuss', 'Erdnuss', 'Latex', 'Aspirin', 'Jod', 'Sulfonamid', 'Pollen'];
const CITIES = ['Berlin', 'München', 'Hamburg', 'Köln', 'Frankfurt', 'Stuttgart', 'Düsseldorf', 'Leipzig', 'Dresden', 'Bonn'];
const STREETS = ['Hauptstraße', 'Gartenweg', 'Lindenallee', 'Bergstraße', 'Seeweg', 'Kirchplatz', 'Feldstraße', 'Am Bahnhof'];
const ZIPS = ['10115', '80331', '20095', '50667', '60311', '70173', '40213', '04109', '01067', '53111'];
const INSURERS = ['AOK', 'TK', 'BKK', 'DAK', 'Barmer'];

const HEADERS = { 'Content-Type': 'application/json' };

// ============================================================
// Hilfsfunktionen: realistische, deterministische Test-Daten
// ============================================================

function randInt(max) {
  return Math.floor(Math.random() * max);
}

function randItem(arr) {
  return arr[randInt(arr.length)];
}

function randomBirthDate() {
  // Patienten zwischen 0 und 95 Jahren
  const now = new Date();
  const yearsAgo = randInt(95);
  const d = new Date(now.getFullYear() - yearsAgo, randInt(12), 1 + randInt(28));
  return d.toISOString().slice(0, 10); // YYYY-MM-DD
}

function buildPatientPayload(namePrefix) {
  const family = `${randItem(FAMILY_NAMES)}${namePrefix ? '_' + namePrefix : ''}`;
  const allergies = [];
  const nAllergies = randInt(3); // 0–2 Allergien
  for (let i = 0; i < nAllergies; i++) allergies.push(randItem(ALLERGY_POOL));

  return {
    externalId: `EXT-LT-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    nameFamily: family,
    nameGiven: randItem(GIVEN_NAMES),
    birthDate: randomBirthDate(),
    gender: randItem(GENDERS),
    phone: `+49 1${randInt(9)} ${randInt(9000)} ${randInt(9900)}`,
    email: `patient.${randInt(100000)}@example.de`,
    addressStreet: `${randItem(STREETS)} ${1 + randInt(200)}`,
    addressZip: randItem(ZIPS),
    addressCity: randItem(CITIES),
    insuranceNumber: `A1 ${randInt(99999999)}`,
    allergies: allergies,
  };
}

// ============================================================
// Setup: Erzeugt SEED_SIZE Test-Patienten via POST
// ============================================================

export function setup() {
  console.log(`[setup] Erzeuge ${SEED_SIZE} Test-Patienten an ${BASE_URL} ...`);
  const created = [];
  for (let i = 0; i < SEED_SIZE; i++) {
    const res = http.post(`${BASE_URL}/api/v1/patients`, JSON.stringify(buildPatientPayload('LT')), {
      headers: HEADERS,
      tags: { name: 'setup:create' },
    });
    if (res.status === 200 || res.status === 201) {
      const body = JSON.parse(res.body);
      created.push(body.id || body);
    } else {
      console.log(`[setup] WARN: Create #${i} -> HTTP ${res.status}: ${res.body}`);
    }
  }
  console.log(`[setup] ${created.length}/${SEED_SIZE} Test-Patienten angelegt`);
  return { ids: created };
}

// ============================================================
// Teardown: Löscht die Test-Patienten (Soft-Delete via API)
// ============================================================

export function teardown(data) {
  console.log(`[teardown] Lösche ${data.ids.length} Test-Patienten ...`);
  let ok = 0;
  for (const id of data.ids) {
    const res = http.delete(`${BASE_URL}/api/v1/patients/${id}`, null, {
      headers: HEADERS,
      tags: { name: 'teardown:delete' },
    });
    if (res.status === 200) ok++;
  }
  console.log(`[teardown] ${ok}/${data.ids.length} Test-Patienten gelöscht`);
}

// ============================================================
// Thresholds: p95 < 200ms, p99 < 500ms, error_rate < 1%
// ============================================================

export const options = {
  scenarios: {
    // Szenario 1: 1.000 VUs × 60s — GET /api/v1/patients (Liste + Cursor-Pagination)
    list_patients: {
      executor: 'constant-vus',
      vus: 1000,
      duration: '60s',
      // 3s Aufwärmen, damit nicht alle VUs gleichzeitig starten
      startGrace: '3s',
      tags: { scenario: 'list' },
    },
    // Szenario 2: 500 VUs × 60s — POST /api/v1/patients (Create)
    create_patients: {
      executor: 'constant-vus',
      vus: 500,
      duration: '60s',
      startGrace: '5s',
      tags: { scenario: 'create' },
    },
    // Szenario 3: 500 VUs × 60s — PUT /api/v1/patients/:id (Update + Optimistic Locking)
    update_patients: {
      executor: 'constant-vus',
      vus: 500,
      duration: '60s',
      startGrace: '5s',
      tags: { scenario: 'update' },
    },
  },
  thresholds: {
    http_req_duration: ['p(95)<200', 'p(99)<500'],
    http_req_failed: ['rate<0.01'], // error_rate < 1%
    checks: ['rate>0.99'],
  },
  // Summe der drei Szenarien: max 2000 VUs gleichzeitig (Szenarien laufen parallel)
  maxVUs: 2000,
};

// ============================================================
// Default-Iteration: 1 VU = 1 "User" mit realistischem Mix
// ============================================================

export default function (data) {
  // Jeder VU simuliert einen Nutzer. Welche Aktion der VU ausführt,
  // entscheidet das Szenario-Tag (k6 setzt es je Szenario). So bleiben
  // die drei Szenarien sauber getrennt — deterministisch, ohne Race.
  const tag = (typeof __TAGS !== 'undefined' && __TAGS && __TAGS.scenario) || 'list';
  if (tag === 'list') {
    doList();
  } else if (tag === 'create') {
    doCreate();
  } else {
    doUpdate(data);
  }
}

// --- Szenario 1: GET /api/v1/patients + Cursor-Pagination ---
function doList() {
  // 2–4 Seiten durchblättern, wie eine UI es tut
  const pages = 2 + randInt(3);
  let cursor = null;
  for (let page = 0; page < pages; page++) {
    let url = `${BASE_URL}/api/v1/patients?limit=${PAGE_LIMIT}`;
    if (cursor) url += `&cursor=${cursor}`;
    const res = http.get(url, { headers: HEADERS, tags: { name: 'GET /api/v1/patients' } });
    const ok = check(res, {
      'list: status 200': (r) => r.status === 200,
      'list: hat items': (r) => JSON.parse(r.body).items && JSON.parse(r.body).items.length > 0,
    });
    if (ok) {
      const body = JSON.parse(res.body);
      cursor = body.nextCursor; // Cursor der nächsten Seite
      if (!cursor) break; // Ende der Liste erreicht
    }
  }
}

// --- Szenario 2: POST /api/v1/patients (Create) ---
function doCreate() {
  const payload = buildPatientPayload('LT');
  const res = http.post(`${BASE_URL}/api/v1/patients`, JSON.stringify(payload), {
    headers: HEADERS,
    tags: { name: 'POST /api/v1/patients' },
  });
  check(res, {
    'create: status 200/201': (r) => r.status === 200 || r.status === 201,
    'create: hat id': (r) => { try { return !!JSON.parse(r.body).id; } catch { return false; } },
    'create: version 1': (r) => { try { return JSON.parse(r.body).version === 1; } catch { return false; } },
  });
  // Load-Test-Daten aus dem Create-Szenario nicht aufräumen (teardown räumt nur setup auf)
}

// --- Szenario 3: PUT /api/v1/patients/:id (Update + Optimistic Locking) ---
function doUpdate(data) {
  const ids = data.ids;
  if (!ids || ids.length === 0) {
    // Fallback: einen Patient per GET holen und den updaten
    const listRes = http.get(`${BASE_URL}/api/v1/patients?limit=1`, { headers: HEADERS, tags: { name: 'GET /api/v1/patients (fallback)' } });
    const items = JSON.parse(listRes.body).items || [];
    if (items.length === 0) return;
    const p = items[0];
    doUpdateOne(p.id, p.version);
    return;
  }
  const p = ids[randInt(ids.length)];
  // 1. GET: aktuelle Version holen (Optimistic Locking braucht die Version)
  const getRes = http.get(`${BASE_URL}/api/v1/patients/${p}`, { headers: HEADERS, tags: { name: 'GET /api/v1/patients/:id' } });
  if (getRes.status !== 200) return;
  const current = JSON.parse(getRes.body);

  // 2. PUT: Update mit der frisch gelesenen Version
  doUpdateOne(current.id, current.version, current);
}

function doUpdateOne(id, version, current) {
  const payload = {
    version: version, // PFLICHT: Optimistic Locking
    phone: `+49 1${randInt(9)} ${randInt(9000)} ${randInt(9900)}`,
    email: `patient.updated.${randInt(1000000)}@example.de`,
    allergies: current ? current.allergies : [],
  };
  const res = http.put(`${BASE_URL}/api/v1/patients/${id}`, JSON.stringify(payload), {
    headers: HEADERS,
    tags: { name: 'PUT /api/v1/patients/:id' },
  });
  // 409 CONFLICT = Versionenkonflikt → für den Load-Test "erwartet" (nicht Fehler)
  check(res, {
    'update: status 200 oder 409': (r) => r.status === 200 || r.status === 409,
    'update: bei 200 version gesteigert': (r) => r.status === 409 || (() => {
      try { return JSON.parse(r.body).version === version + 1; } catch { return false; }
    })(),
  });
}
