// ============================================================
// KIS — k6 Load-Test: Termin-Endpunkte (Task 5.6, analog zu patients.k6.js)
//
// Szenario 1: 1.000 VUs, 60s — GET  /api/v1/appointments (Liste + Filter)
// Szenario 2:  500 VUs, 60s — POST /api/v1/appointments (Create, Doppelbuchungsprüfung)
// Szenario 3:  500 VUs, 60s — PUT  /api/v1/appointments/:id (Update + Optimistic Locking)
//
// Thresholds: p95 < 200ms, p99 < 500ms, error_rate < 1%
//
// Setup:    Erzeugt 200 Test-Ärzte, 1000 Test-Patienten und 1000
//           Test-Termine (alle im "Test-Zeitraum" 2099-01-01 … 2099-12-31,
//           damit sie niemals mit echten Terminen kollidieren).
// Teardown: Löscht die Test-Termine, Test-Patienten und Test-Ärzte (Soft-Delete).
//
// Start:
//   KIS_BASE_URL=http://localhost:8080 k6 run load-tests/appointments.k6.js
//
// Hinweise:
// - Doppelbuchungsprüfung: Termine liegen in einem fiktiven Jahr (2099)
//   mit pro VU eigener Slot-Variante → minimale 409-Raten, realistisch.
// - Rate-Limiting (100 req/min pro IP) in der Test-Instanz entsprechend
//   erhöhen: KIS_SECURITY__RATE_LIMIT__NORMAL_PER_MIN=1000000
// ============================================================

import http from 'k6/http';
import { check } from 'k6';

// --- Konfiguration (via ENV) ---
const BASE_URL = __ENV.KIS_BASE_URL || 'http://localhost:8080';
const SEED_DOCTORS = parseInt(__ENV.KIS_LOAD_DOCTORS || '200', 10);
const SEED_PATIENTS = parseInt(__ENV.KIS_LOAD_PATIENTS || '1000', 10);
const SEED_APPTS = parseInt(__ENV.KIS_LOAD_APPTS || '1000', 10);
const PAGE_LIMIT = 50;

const HEADERS = { 'Content-Type': 'application/json' };
const TEST_YEAR = '2099'; // Fiktiver Zeitraum, keine Kollision mit echten Terminen

// Realistische Namen (wie in patients.k6.js)
const FAMILY_NAMES = ['Muster', 'Schmidt', 'Müller', 'Schneider', 'Fischer', 'Weber', 'Meyer', 'Wagner', 'Becker', 'Schulz'];
const GIVEN_NAMES = ['Anna', 'Lukas', 'Maria', 'Jonas', 'Laura', 'Felix', 'Julia', 'Maximilian', 'Sophie', 'David'];
const SPECIALTIES = ['Innere Medizin', 'Chirurgie', 'Kardiologie', 'Neurologie', 'Orthopädie', 'Hämatologie'];
const CATEGORIES = ['ordination', 'konsultation', 'kontrolle', 'nachsorge'];
const REASONS = ['Regulärer Check-up', 'Blutdruckkontrolle', 'Erstberatung', 'Verordnung Erneuerung', 'Beschwerde: Rückenschmerzen'];

function randInt(max) { return Math.floor(Math.random() * max); }
function randItem(arr) { return arr[randInt(arr.length)]; }

function isoDate(year, month, day) {
  return `${year}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`;
}

function randomTestDateTime() {
  // Zufälliger Termin im Test-Jahr, 08:00–17:00 Uhr (Werktag-Nähe)
  const month = 1 + randInt(12);
  const day = 1 + randInt(28);
  const hour = 8 + randInt(10); // 08–17
  const minute = randInt(60);
  const d = new Date(Date.UTC(2099, month - 1, day, hour, minute, 0));
  return d.toISOString();
}

function buildPatientPayload() {
  return {
    externalId: `EXT-APPT-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    nameFamily: `${randItem(FAMILY_NAMES)}_LT`,
    nameGiven: randItem(GIVEN_NAMES),
    birthDate: isoDate(1950 + randInt(60), 1 + randInt(12), 1 + randInt(28)),
    gender: randInt(2) ? 'male' : 'female',
    phone: `+49 1${randInt(9)} ${randInt(9000)} ${randInt(9900)}`,
    allergies: [],
  };
}

function buildDoctorPayload() {
  return {
    externalId: `EXT-DOC-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    nameFamily: `${randItem(FAMILY_NAMES)}_LT`,
    nameGiven: randItem(GIVEN_NAMES),
    specialty: randItem(SPECIALTIES),
  };
}

function buildAppointmentPayload(patientId, doctorId) {
  const start = new Date(randomTestDateTime());
  const end = new Date(start.getTime() + 30 * 60 * 1000); // 30-Min-Termin
  return {
    patientId,
    doctorId,
    category: randItem(CATEGORIES),
    startTime: start.toISOString(),
    endTime: end.toISOString(),
    reason: randItem(REASONS),
  };
}

// ============================================================
// Setup: Ärzte → Patienten → Termine
// ============================================================

export function setup() {
  console.log(`[setup] Erzeuge ${SEED_DOCTORS} Ärzte, ${SEED_PATIENTS} Patienten, ${SEED_APPTS} Termine ...`);

  // 1. Ärzte
  const doctors = [];
  for (let i = 0; i < SEED_DOCTORS; i++) {
    const res = http.post(`${BASE_URL}/api/v1/doctors`, JSON.stringify(buildDoctorPayload()), {
      headers: HEADERS, tags: { name: 'setup:create-doctor' },
    });
    if (res.status === 200 || res.status === 201) {
      const b = JSON.parse(res.body);
      doctors.push(b.id);
    }
  }
  console.log(`[setup] ${doctors.length}/${SEED_DOCTORS} Ärzte angelegt`);

  // 2. Patienten
  const patients = [];
  for (let i = 0; i < SEED_PATIENTS; i++) {
    const res = http.post(`${BASE_URL}/api/v1/patients`, JSON.stringify(buildPatientPayload()), {
      headers: HEADERS, tags: { name: 'setup:create-patient' },
    });
    if (res.status === 200 || res.status === 201) {
      const b = JSON.parse(res.body);
      patients.push(b.id);
    }
  }
  console.log(`[setup] ${patients.length}/${SEED_PATIENTS} Patienten angelegt`);

  // 3. Termine (jeder VU bekommt eigene Slots → minimale 409-Rate)
  const appts = [];
  for (let i = 0; i < SEED_APPTS; i++) {
    const payload = buildAppointmentPayload(patients[i % patients.length], doctors[i % doctors.length]);
    // Jede Iteration bekommt eine eindeutige Zeit (Minute + i), um Doppelbuchung zu vermeiden
    const start = new Date(Date.UTC(2099, 0, 1 + (i % 28), 8, i % 60, 0));
    const end = new Date(start.getTime() + 30 * 60 * 1000);
    payload.startTime = start.toISOString();
    payload.endTime = end.toISOString();

    const res = http.post(`${BASE_URL}/api/v1/appointments`, JSON.stringify(payload), {
      headers: HEADERS, tags: { name: 'setup:create-appointment' },
    });
    if (res.status === 200 || res.status === 201) {
      const b = JSON.parse(res.body);
      appts.push({ id: b.id, version: b.version, doctorId: payload.doctorId, patientId: payload.patientId });
    }
  }
  console.log(`[setup] ${appts.length}/${SEED_APPTS} Termine angelegt`);

  return { doctors, patients, appts };
}

// ============================================================
// Teardown: Termine → Patienten → Ärzte (FK-Abhängigkeit)
// ============================================================

export function teardown(data) {
  console.log(`[teardown] Lösche ${data.appts.length} Termine, ${data.patients.length} Patienten, ${data.doctors.length} Ärzte ...`);
  let okA = 0, okP = 0, okD = 0;
  for (const a of data.appts) {
    const res = http.delete(`${BASE_URL}/api/v1/appointments/${a.id}`, null, { headers: HEADERS, tags: { name: 'teardown:delete-appointment' } });
    if (res.status === 200) okA++;
  }
  for (const p of data.patients) {
    const res = http.delete(`${BASE_URL}/api/v1/patients/${p}`, null, { headers: HEADERS, tags: { name: 'teardown:delete-patient' } });
    if (res.status === 200) okP++;
  }
  for (const d of data.doctors) {
    const res = http.delete(`${BASE_URL}/api/v1/doctors/${d}`, null, { headers: HEADERS, tags: { name: 'teardown:delete-doctor' } });
    if (res.status === 200) okD++;
  }
  console.log(`[teardown] ${okA}/${data.appts.length} Termine, ${okP}/${data.patients.length} Patienten, ${okD}/${data.doctors.length} Ärzte gelöscht`);
}

// ============================================================
// Thresholds: p95 < 200ms, p99 < 500ms, error_rate < 1%
// ============================================================

export const options = {
  scenarios: {
    // Szenario 1: 1.000 VUs × 60s — GET /api/v1/appointments (Liste + Filter)
    list_appointments: {
      executor: 'constant-vus',
      vus: 1000,
      duration: '60s',
      startGrace: '3s',
      tags: { scenario: 'list' },
    },
    // Szenario 2: 500 VUs × 60s — POST /api/v1/appointments (Create)
    create_appointments: {
      executor: 'constant-vus',
      vus: 500,
      duration: '60s',
      startGrace: '5s',
      tags: { scenario: 'create' },
    },
    // Szenario 3: 500 VUs × 60s — PUT /api/v1/appointments/:id (Update + Optimistic Locking)
    update_appointments: {
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
  maxVUs: 2000,
};

// ============================================================
// Default-Iteration: Szenario-Tag bestimmt die Aktion
// ============================================================

export default function (data) {
  const tag = (typeof __TAGS !== 'undefined' && __TAGS && __TAGS.scenario) || 'list';
  if (tag === 'list') {
    doList();
  } else if (tag === 'create') {
    doCreate(data);
  } else {
    doUpdate(data);
  }
}

// --- Szenario 1: GET /api/v1/appointments (Liste + Cursor-Pagination) ---
function doList() {
  const pages = 1 + randInt(3);
  let cursor = null;
  for (let page = 0; page < pages; page++) {
    let url = `${BASE_URL}/api/v1/appointments?limit=${PAGE_LIMIT}`;
    if (cursor) url += `&cursor=${cursor}`;
    const res = http.get(url, { headers: HEADERS, tags: { name: 'GET /api/v1/appointments' } });
    const ok = check(res, {
      'list: status 200': (r) => r.status === 200,
      'list: items-Array': (r) => { try { return Array.isArray(JSON.parse(r.body).items); } catch { return false; } },
    });
    if (ok) {
      const body = JSON.parse(res.body);
      cursor = body.nextCursor;
      if (!cursor) break;
    }
  }
}

// --- Szenario 2: POST /api/v1/appointments (Create + Doppelbuchungsprüfung) ---
function doCreate(data) {
  if (!data.patients || !data.doctors) return;
  const patientId = data.patients[randInt(data.patients.length)];
  const doctorId = data.doctors[randInt(data.doctors.length)];

  // Fiktives Jahr 2099 + VU-eigene Slot-Variante → realistische, aber
  // minimale Doppelbuchungs-Rate (409 = erwartet, kein Fehler)
  const start = new Date(Date.UTC(2099, 0, 1 + randInt(28), 8 + randInt(10), __VU % 60, 0));
  const end = new Date(start.getTime() + 30 * 60 * 1000);

  const payload = {
    patientId,
    doctorId,
    category: randItem(CATEGORIES),
    startTime: start.toISOString(),
    endTime: end.toISOString(),
    reason: randItem(REASONS),
  };

  const res = http.post(`${BASE_URL}/api/v1/appointments`, JSON.stringify(payload), {
    headers: HEADERS, tags: { name: 'POST /api/v1/appointments' },
  });
  check(res, {
    'create: status 200/201': (r) => r.status === 200 || r.status === 201,
    'create: hat id': (r) => { try { return !!JSON.parse(r.body).id; } catch { return false; } },
  });
}

// --- Szenario 3: PUT /api/v1/appointments/:id (Update + Optimistic Locking) ---
function doUpdate(data) {
  const appts = data.appts;
  if (!appts || appts.length === 0) return;

  const a = appts[randInt(appts.length)];

  // 1. GET: aktuelle Version holen
  const getRes = http.get(`${BASE_URL}/api/v1/appointments/${a.id}`, {
    headers: HEADERS, tags: { name: 'GET /api/v1/appointments/:id' },
  });
  if (getRes.status !== 200) return;
  const current = JSON.parse(getRes.body);

  // 2. PUT: Status aktualisieren (z. B. geplant → bestaetigt)
  const payload = {
    version: current.version, // PFLICHT: Optimistic Locking
    reason: `Load-Test-Update ${Date.now()}`,
  };

  const res = http.put(`${BASE_URL}/api/v1/appointments/${a.id}`, JSON.stringify(payload), {
    headers: HEADERS, tags: { name: 'PUT /api/v1/appointments/:id' },
  });
  // 409 = Versionenkonflikt (erwartet bei parallelen Updates, kein Load-Test-Fehler)
  check(res, {
    'update: status 200 oder 409': (r) => r.status === 200 || r.status === 409,
    'update: bei 200 version gesteigert': (r) => r.status === 409 || (() => {
      try { return JSON.parse(r.body).version === current.version + 1; } catch { return false; }
    })(),
  });
}
