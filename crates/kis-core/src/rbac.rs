//! Rollen und Berechtigungen (RBAC) — Rollen sind Daten, Rechte sind Code.
//!
//! - [`Permission`]: die Rechte, die der Code kennt und an Routen prüft.
//!   Jedes hat einen stabilen Schlüssel ([`Permission::key`], z. B.
//!   `case.read`).
//! - **Rollen** stehen in der Datenbank (`roles`, `role_permissions`) und
//!   werden vom Haus gepflegt: neue Rollen anlegen, Rechte zuordnen —
//!   ohne Programmänderung. Ein Benutzer hat eine Hauptrolle
//!   (`users.role`) und beliebig viele Zusatzrollen (`user_roles`);
//!   wirksam ist die Vereinigung als [`PermissionSet`].
//! - [`BUILTIN_ROLES`] ist nur die **Grundausstattung**: Beim ersten Start
//!   werden diese Rollen mit ihren Standardrechten angelegt
//!   ([`seed_builtin_roles`]). Danach gehören sie dem Haus und werden nie
//!   überschrieben.
//! - Die Rolle [`ADMIN`] hat immer alle Rechte (nicht einschränkbar), damit
//!   sich niemand aussperren kann.
//! - *Welche Fälle und Einheiten* jemand sehen darf, regelt nicht die
//!   Rolle, sondern der Behandlungszusammenhang ([`crate::treatment`]).
//!
//! Neues Recht hinzufügen: Variante + `ALL` + `key()` ergänzen (der Test
//! `all_is_complete` erzwingt Vollständigkeit) und bei Bedarf in die
//! Grundausstattung aufnehmen. Bestehende Installationen erhalten neue
//! Rechte nicht automatisch (außer `admin`) — das Haus ordnet sie zu.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Schlüssel der Rolle mit allen Rechten.
pub const ADMIN: &str = "admin";

/// Einzelnes Recht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // --- Patientenstamm ---
    PatientRead,
    PatientWrite,
    PatientCreate,
    PatientDelete,
    // --- Ärzte-Stammdaten, Termine ---
    DoctorRead,
    DoctorWrite,
    AppointmentRead,
    AppointmentWrite,
    AppointmentBook,
    AppointmentCancel,
    // --- Dauermedikation (patientenbezogen) ---
    MedicationRead,
    MedicationWrite,
    // --- Dokumente, Datenaustausch, Betrieb ---
    DocumentRead,
    DocumentWrite,
    Export,
    Import,
    ConfigRead,
    ConfigWrite,
    AuditRead,
    /// Administration (Benutzer, Rollen, Einstellungen, Löschung)
    Admin,
    // --- Organisation ---
    OrgRead,
    OrgWrite,
    /// Arbeitslisten und Belegung aller Einheiten ohne Zuordnung
    UnitAccessAll,
    // --- Fälle ---
    /// Falldaten (administrativ) lesen
    CaseRead,
    /// Aufnahme und Entlassung
    CaseAdmit,
    /// Verlegung (Fachabteilung, Station, Bett)
    CaseTransfer,
    /// Zugriff auf alle Fälle ohne Behandlungszusammenhang
    CaseAccessAll,
    // --- Klinische Akte ---
    ClinicalRead,
    ClinicalWrite,
    /// Diagnosen und Prozeduren kodieren
    CodingWrite,
    /// Aufgaben anlegen und abhaken
    TaskWrite,
    // --- Auftragswesen ---
    OrderRead,
    /// Leistungen anfordern
    OrderWrite,
    /// Aufträge der eigenen Einheit annehmen und erledigen
    OrderProcess,
    /// Kataloge pflegen: Leistungen, Automatik-Regeln, Kostformen
    CatalogWrite,
    // --- Labor ---
    LabRead,
    LabWrite,
    // --- Pflege ---
    NursingRead,
    NursingWrite,
    // --- Stationäre Verordnung ---
    PrescriptionRead,
    PrescriptionWrite,
    /// Gabe eines Medikaments dokumentieren
    MedicationAdminister,
    // --- Verpflegung ---
    /// Produktionsliste der Küche
    MealRead,
    /// Kost für einen Fall anordnen
    MealWrite,
    // --- Dienstplan ---
    RosterRead,
    RosterWrite,
    // --- Sonderrechte ---
    /// Notfallzugriff außerhalb des Behandlungszusammenhangs anfordern
    BreakGlass,
    /// Ereignis-Outbox lesen (Schnittstellenkonten)
    InterfaceConsume,
}

impl Permission {
    /// Alle Rechte. Der Test `all_is_complete` stellt sicher, dass keine
    /// Variante fehlt.
    pub const ALL: &'static [Permission] = &[
        Permission::PatientRead,
        Permission::PatientWrite,
        Permission::PatientCreate,
        Permission::PatientDelete,
        Permission::DoctorRead,
        Permission::DoctorWrite,
        Permission::AppointmentRead,
        Permission::AppointmentWrite,
        Permission::AppointmentBook,
        Permission::AppointmentCancel,
        Permission::MedicationRead,
        Permission::MedicationWrite,
        Permission::DocumentRead,
        Permission::DocumentWrite,
        Permission::Export,
        Permission::Import,
        Permission::ConfigRead,
        Permission::ConfigWrite,
        Permission::AuditRead,
        Permission::Admin,
        Permission::OrgRead,
        Permission::OrgWrite,
        Permission::UnitAccessAll,
        Permission::CaseRead,
        Permission::CaseAdmit,
        Permission::CaseTransfer,
        Permission::CaseAccessAll,
        Permission::ClinicalRead,
        Permission::ClinicalWrite,
        Permission::CodingWrite,
        Permission::TaskWrite,
        Permission::OrderRead,
        Permission::OrderWrite,
        Permission::OrderProcess,
        Permission::CatalogWrite,
        Permission::LabRead,
        Permission::LabWrite,
        Permission::NursingRead,
        Permission::NursingWrite,
        Permission::PrescriptionRead,
        Permission::PrescriptionWrite,
        Permission::MedicationAdminister,
        Permission::MealRead,
        Permission::MealWrite,
        Permission::RosterRead,
        Permission::RosterWrite,
        Permission::BreakGlass,
        Permission::InterfaceConsume,
    ];

    /// Stabiler Schlüssel, z. B. `case.read`. Steht so in
    /// `role_permissions.permission`.
    pub fn key(&self) -> &'static str {
        match self {
            Permission::PatientRead => "patient.read",
            Permission::PatientWrite => "patient.write",
            Permission::PatientCreate => "patient.create",
            Permission::PatientDelete => "patient.delete",
            Permission::DoctorRead => "doctor.read",
            Permission::DoctorWrite => "doctor.write",
            Permission::AppointmentRead => "appointment.read",
            Permission::AppointmentWrite => "appointment.write",
            Permission::AppointmentBook => "appointment.book",
            Permission::AppointmentCancel => "appointment.cancel",
            Permission::MedicationRead => "medication.read",
            Permission::MedicationWrite => "medication.write",
            Permission::DocumentRead => "document.read",
            Permission::DocumentWrite => "document.write",
            Permission::Export => "data.export",
            Permission::Import => "data.import",
            Permission::ConfigRead => "config.read",
            Permission::ConfigWrite => "config.write",
            Permission::AuditRead => "audit.read",
            Permission::Admin => "admin",
            Permission::OrgRead => "org.read",
            Permission::OrgWrite => "org.write",
            Permission::UnitAccessAll => "unit.access_all",
            Permission::CaseRead => "case.read",
            Permission::CaseAdmit => "case.admit",
            Permission::CaseTransfer => "case.transfer",
            Permission::CaseAccessAll => "case.access_all",
            Permission::ClinicalRead => "clinical.read",
            Permission::ClinicalWrite => "clinical.write",
            Permission::CodingWrite => "coding.write",
            Permission::TaskWrite => "task.write",
            Permission::OrderRead => "order.read",
            Permission::OrderWrite => "order.write",
            Permission::OrderProcess => "order.process",
            Permission::CatalogWrite => "catalog.write",
            Permission::LabRead => "lab.read",
            Permission::LabWrite => "lab.write",
            Permission::NursingRead => "nursing.read",
            Permission::NursingWrite => "nursing.write",
            Permission::PrescriptionRead => "prescription.read",
            Permission::PrescriptionWrite => "prescription.write",
            Permission::MedicationAdminister => "medication.administer",
            Permission::MealRead => "meal.read",
            Permission::MealWrite => "meal.write",
            Permission::RosterRead => "roster.read",
            Permission::RosterWrite => "roster.write",
            Permission::BreakGlass => "access.break_glass",
            Permission::InterfaceConsume => "interface.consume",
        }
    }

    /// Recht zu einem Schlüssel; `None` bei unbekanntem Schlüssel.
    pub fn from_key(key: &str) -> Option<Permission> {
        Permission::ALL.iter().copied().find(|p| p.key() == key)
    }

    fn bit(self) -> u128 {
        1u128 << (self as u32)
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// Menge von Rechten als Bitfeld (16 Byte, ohne Allokation).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PermissionSet(u128);

impl PermissionSet {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn all() -> Self {
        Permission::ALL.iter().copied().collect()
    }

    pub fn insert(&mut self, p: Permission) {
        self.0 |= p.bit();
    }

    pub fn contains(&self, p: Permission) -> bool {
        self.0 & p.bit() != 0
    }

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Enthaltene Rechte in der Reihenfolge von [`Permission::ALL`].
    pub fn iter(&self) -> impl Iterator<Item = Permission> + '_ {
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| self.contains(*p))
    }

    /// Schlüssel der enthaltenen Rechte.
    pub fn keys(&self) -> Vec<&'static str> {
        self.iter().map(|p| p.key()).collect()
    }
}

impl FromIterator<Permission> for PermissionSet {
    fn from_iter<I: IntoIterator<Item = Permission>>(iter: I) -> Self {
        let mut set = Self::empty();
        for p in iter {
            set.insert(p);
        }
        set
    }
}

impl std::fmt::Debug for PermissionSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.keys()).finish()
    }
}

/// Rollen → Rechte, wie in der Datenbank gepflegt. Wird je Instanz im
/// Speicher gehalten ([`crate::platform::Platform`]).
#[derive(Debug, Clone, Default)]
pub struct RoleMatrix {
    roles: HashMap<String, PermissionSet>,
}

impl RoleMatrix {
    pub fn from_rows(rows: impl IntoIterator<Item = (String, Option<String>)>) -> Self {
        let mut roles: HashMap<String, PermissionSet> = HashMap::new();
        for (role, permission) in rows {
            let set = roles.entry(role.clone()).or_default();
            if let Some(key) = permission {
                match Permission::from_key(&key) {
                    Some(p) => set.insert(p),
                    // Recht eines abgeschalteten oder älteren Moduls: ohne Wirkung.
                    None => tracing::debug!(role, key, "Unbekannter Rechteschlüssel ignoriert"),
                }
            }
        }
        Self { roles }
    }

    /// Wirksame Rechte aus Hauptrolle und Zusatzrollen. Unbekannte Rollen
    /// tragen nichts bei.
    pub fn effective<'a>(&self, roles: impl IntoIterator<Item = &'a str>) -> PermissionSet {
        let mut set = PermissionSet::empty();
        for role in roles {
            if role == ADMIN {
                return PermissionSet::all();
            }
            if let Some(p) = self.roles.get(role) {
                set = set.union(*p);
            }
        }
        set
    }

    pub fn contains_role(&self, role: &str) -> bool {
        self.roles.contains_key(role)
    }

    /// Matrix der Grundausstattung (für Tests und Werkzeuge ohne Datenbank).
    pub fn builtin() -> Self {
        Self {
            roles: BUILTIN_ROLES
                .iter()
                .map(|r| (r.key.to_string(), r.permissions.iter().copied().collect()))
                .collect(),
        }
    }
}

/// Rolle der Grundausstattung.
pub struct BuiltinRole {
    pub key: &'static str,
    pub label: &'static str,
    pub permissions: &'static [Permission],
}

use Permission as P;

/// Grundausstattung. Deckt die Berufsgruppen eines Krankenhauses ab; das
/// Haus kann jede Rolle anpassen und eigene ergänzen.
pub const BUILTIN_ROLES: &[BuiltinRole] = &[
    BuiltinRole {
        key: ADMIN,
        label: "Administration",
        // Wirksam sind immer alle Rechte (siehe RoleMatrix::effective).
        permissions: &[P::Admin],
    },
    BuiltinRole {
        key: "arzt",
        label: "Ärztlicher Dienst",
        permissions: &[
            P::PatientRead,
            P::PatientWrite,
            P::PatientCreate,
            P::DoctorRead,
            P::AppointmentRead,
            P::AppointmentWrite,
            P::AppointmentBook,
            P::AppointmentCancel,
            P::MedicationRead,
            P::MedicationWrite,
            P::DocumentRead,
            P::OrgRead,
            P::CaseRead,
            P::CaseAdmit,
            P::CaseTransfer,
            P::ClinicalRead,
            P::ClinicalWrite,
            P::CodingWrite,
            P::TaskWrite,
            P::OrderRead,
            P::OrderWrite,
            P::OrderProcess,
            P::LabRead,
            P::NursingRead,
            P::PrescriptionRead,
            P::PrescriptionWrite,
            P::MedicationAdminister,
            P::MealWrite,
            P::RosterRead,
            P::BreakGlass,
        ],
    },
    BuiltinRole {
        key: "pflege",
        label: "Pflegedienst",
        permissions: &[
            P::PatientRead,
            P::AppointmentRead,
            P::MedicationRead,
            P::OrgRead,
            P::CaseRead,
            P::CaseTransfer,
            P::ClinicalRead,
            P::TaskWrite,
            P::OrderRead,
            P::OrderWrite,
            P::OrderProcess,
            P::LabRead,
            P::NursingRead,
            P::NursingWrite,
            P::PrescriptionRead,
            P::MedicationAdminister,
            P::MealWrite,
            P::RosterRead,
            P::BreakGlass,
        ],
    },
    BuiltinRole {
        key: "rezeption",
        label: "Patientenservice / Aufnahme",
        permissions: &[
            P::PatientRead,
            P::PatientCreate,
            P::AppointmentBook,
            P::AppointmentCancel,
            P::OrgRead,
            P::CaseRead,
            P::CaseAdmit,
            P::CaseTransfer,
            P::CaseAccessAll,
            P::OrderWrite,
        ],
    },
    BuiltinRole {
        key: "verwaltung",
        label: "Verwaltung",
        permissions: &[
            P::PatientRead,
            P::PatientWrite,
            P::PatientCreate,
            P::OrgRead,
            P::CaseRead,
            P::CaseAdmit,
            P::CaseAccessAll,
            P::OrderWrite,
            P::RosterRead,
        ],
    },
    BuiltinRole {
        key: "kodierung",
        label: "Medizincontrolling / Kodierung",
        permissions: &[
            P::PatientRead,
            P::OrgRead,
            P::CaseRead,
            P::CaseAccessAll,
            P::ClinicalRead,
            P::CodingWrite,
            P::OrderRead,
            P::LabRead,
        ],
    },
    BuiltinRole {
        key: "funktionsdienst",
        label: "Funktionsdienst (Labor, Radiologie, Diagnostik)",
        permissions: &[
            P::PatientRead,
            P::OrgRead,
            P::CaseRead,
            P::OrderRead,
            P::OrderWrite,
            P::OrderProcess,
            P::LabRead,
            P::LabWrite,
        ],
    },
    BuiltinRole {
        key: "therapie",
        label: "Therapie (Physio-, Ergotherapie, Logopädie)",
        permissions: &[
            P::PatientRead,
            P::OrgRead,
            P::CaseRead,
            P::ClinicalRead,
            P::OrderRead,
            P::OrderWrite,
            P::OrderProcess,
        ],
    },
    BuiltinRole {
        key: "sozialdienst",
        label: "Sozialdienst / Entlassmanagement",
        permissions: &[
            P::PatientRead,
            P::OrgRead,
            P::CaseRead,
            P::OrderRead,
            P::OrderWrite,
            P::OrderProcess,
            P::TaskWrite,
        ],
    },
    BuiltinRole {
        key: "kueche",
        label: "Küche / Verpflegung",
        permissions: &[
            P::OrgRead,
            P::MealRead,
            P::OrderRead,
            P::OrderWrite,
            P::OrderProcess,
        ],
    },
    BuiltinRole {
        key: "hauswirtschaft",
        label: "Hauswirtschaft / Reinigung / Bettenaufbereitung",
        permissions: &[P::OrgRead, P::OrderRead, P::OrderWrite, P::OrderProcess],
    },
    BuiltinRole {
        key: "transport",
        label: "Hol- und Bringdienst",
        permissions: &[P::OrgRead, P::OrderRead, P::OrderWrite, P::OrderProcess],
    },
    BuiltinRole {
        key: "technik",
        label: "Technik / Medizintechnik",
        permissions: &[P::OrgRead, P::OrderRead, P::OrderWrite, P::OrderProcess],
    },
    BuiltinRole {
        key: "dienstplaner",
        label: "Dienstplanung",
        permissions: &[P::OrgRead, P::RosterRead, P::RosterWrite],
    },
    BuiltinRole {
        key: "viewer",
        label: "Lesezugriff Stammdaten",
        permissions: &[P::PatientRead, P::AppointmentRead],
    },
    BuiltinRole {
        key: "auditor",
        label: "Revision / Datenschutz",
        permissions: &[P::AuditRead],
    },
    BuiltinRole {
        key: "llm_agent",
        label: "Assistenzsystem (nur lesend)",
        permissions: &[
            P::PatientRead,
            P::AppointmentRead,
            P::MedicationRead,
            P::DocumentRead,
        ],
    },
    BuiltinRole {
        key: "schnittstelle",
        label: "Technisches Schnittstellenkonto",
        permissions: &[
            P::InterfaceConsume,
            P::PatientRead,
            P::OrgRead,
            P::CaseRead,
            P::CaseAccessAll,
            P::OrderRead,
            P::LabRead,
            P::LabWrite,
        ],
    },
];

/// Legt fehlende Rollen der Grundausstattung an und spielt ihre
/// Standardrechte genau einmal ein (`roles.seeded`). Bereits vorhandene,
/// vom Haus angepasste Rechte werden nicht angefasst. Idempotent und
/// sicher bei gleichzeitigem Start mehrerer Instanzen.
pub async fn seed_builtin_roles(conn: &mut sqlx::PgConnection) -> sqlx::Result<()> {
    let mut changed = false;
    for role in BUILTIN_ROLES {
        sqlx::query(
            "INSERT INTO roles (key, label, is_system) VALUES ($1, $2, TRUE)
             ON CONFLICT (key) DO UPDATE SET is_system = TRUE",
        )
        .bind(role.key)
        .bind(role.label)
        .execute(&mut *conn)
        .await?;
        let seeded: bool = sqlx::query_scalar("SELECT seeded FROM roles WHERE key = $1 FOR UPDATE")
            .bind(role.key)
            .fetch_one(&mut *conn)
            .await?;
        if seeded {
            continue;
        }
        let keys: Vec<&str> = role.permissions.iter().map(|p| p.key()).collect();
        sqlx::query(
            "INSERT INTO role_permissions (role_key, permission)
             SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
        )
        .bind(role.key)
        .bind(&keys)
        .execute(&mut *conn)
        .await?;
        sqlx::query("UPDATE roles SET seeded = TRUE, updated_at = now() WHERE key = $1")
            .bind(role.key)
            .execute(&mut *conn)
            .await?;
        changed = true;
    }
    if changed {
        bump_rbac_version(conn).await?;
    }
    Ok(())
}

/// Nach jeder Änderung an Rollen oder Rechten aufrufen (gleiche
/// Transaktion): alle Instanzen laden die Matrix beim nächsten Zugriff neu.
pub async fn bump_rbac_version(conn: &mut sqlx::PgConnection) -> sqlx::Result<()> {
    sqlx::query("UPDATE platform_state SET rbac_version = rbac_version + 1")
        .execute(conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Erzwingt per `match` ohne Wildcard, dass jede Variante in `ALL` steht:
    /// eine neue Variante bricht hier die Übersetzung, bis sie ergänzt ist.
    #[test]
    fn all_is_complete() {
        fn count(p: Permission) -> usize {
            use Permission::*;
            match p {
                PatientRead | PatientWrite | PatientCreate | PatientDelete | DoctorRead
                | DoctorWrite | AppointmentRead | AppointmentWrite | AppointmentBook
                | AppointmentCancel | MedicationRead | MedicationWrite | DocumentRead
                | DocumentWrite | Export | Import | ConfigRead | ConfigWrite | AuditRead
                | Admin | OrgRead | OrgWrite | UnitAccessAll | CaseRead | CaseAdmit
                | CaseTransfer | CaseAccessAll | ClinicalRead | ClinicalWrite | CodingWrite
                | TaskWrite | OrderRead | OrderWrite | OrderProcess | CatalogWrite | LabRead
                | LabWrite | NursingRead | NursingWrite | PrescriptionRead | PrescriptionWrite
                | MedicationAdminister | MealRead | MealWrite | RosterRead | RosterWrite
                | BreakGlass | InterfaceConsume => 48,
            }
        }
        assert_eq!(Permission::ALL.len(), count(Permission::Admin));
        assert!(
            Permission::ALL.len() <= 128,
            "PermissionSet fasst 128 Rechte"
        );
        let keys: HashSet<&str> = Permission::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(
            keys.len(),
            Permission::ALL.len(),
            "Schlüssel sind eindeutig"
        );
        for p in Permission::ALL {
            assert_eq!(Permission::from_key(p.key()), Some(*p));
        }
        assert_eq!(Permission::from_key("gibt.es.nicht"), None);
    }

    #[test]
    fn permission_set_basics() {
        let mut s = PermissionSet::empty();
        assert!(s.is_empty());
        s.insert(P::CaseRead);
        s.insert(P::InterfaceConsume);
        assert!(s.contains(P::CaseRead) && s.contains(P::InterfaceConsume));
        assert!(!s.contains(P::Admin));
        assert_eq!(s.keys(), vec!["case.read", "interface.consume"]);
        let all = PermissionSet::all();
        assert!(Permission::ALL.iter().all(|p| all.contains(*p)));
        assert_eq!(s.union(all), all);
    }

    #[test]
    fn matrix_from_rows_and_effective() {
        let m = RoleMatrix::from_rows([
            ("a".to_string(), Some("case.read".to_string())),
            ("a".to_string(), Some("unbekannt.recht".to_string())),
            ("b".to_string(), Some("roster.write".to_string())),
            ("leer".to_string(), None),
        ]);
        assert!(m.contains_role("leer"));
        let e = m.effective(["a", "b", "gibt-es-nicht"]);
        assert!(e.contains(P::CaseRead) && e.contains(P::RosterWrite));
        assert!(!e.contains(P::Admin));
        assert!(m.effective(["leer"]).is_empty());
        // admin hat immer alles, auch ohne Zeilen
        assert_eq!(m.effective(["a", ADMIN]), PermissionSet::all());
    }

    fn builtin(key: &str) -> PermissionSet {
        RoleMatrix::builtin().effective([key])
    }

    /// Sicherheitskritische Negativfälle der Grundausstattung: diese
    /// Rechte darf keine Standardrolle außer admin mitbringen.
    #[test]
    fn builtin_critical_denials() {
        let keys: HashSet<&str> = BUILTIN_ROLES.iter().map(|r| r.key).collect();
        assert_eq!(keys.len(), BUILTIN_ROLES.len(), "Rollenschlüssel eindeutig");
        for role in BUILTIN_ROLES.iter().filter(|r| r.key != ADMIN) {
            let set: PermissionSet = role.permissions.iter().copied().collect();
            for p in [
                P::Admin,
                P::PatientDelete,
                P::ConfigWrite,
                P::OrgWrite,
                P::CatalogWrite,
                P::Export,
                P::Import,
                P::UnitAccessAll,
            ] {
                assert!(!set.contains(p), "{} darf nicht: {p}", role.key);
            }
            assert_eq!(
                set.contains(P::AuditRead),
                role.key == "auditor",
                "Audit-Einsicht nur Auditor: {}",
                role.key
            );
        }
        // Klinisch Tätige arbeiten nur im Behandlungszusammenhang.
        for key in [
            "arzt",
            "pflege",
            "funktionsdienst",
            "therapie",
            "sozialdienst",
        ] {
            assert!(!builtin(key).contains(P::CaseAccessAll), "{key}");
        }
        // Versorgungsbereiche sehen keine Akte und keine Fälle.
        for key in [
            "kueche",
            "hauswirtschaft",
            "transport",
            "technik",
            "dienstplaner",
        ] {
            for p in [
                P::PatientRead,
                P::CaseRead,
                P::ClinicalRead,
                P::LabRead,
                P::NursingRead,
                P::PrescriptionRead,
            ] {
                assert!(!builtin(key).contains(p), "{key} darf nicht: {p}");
            }
        }
        // Pflege verordnet und kodiert nicht.
        for p in [P::PrescriptionWrite, P::CodingWrite, P::ClinicalWrite] {
            assert!(!builtin("pflege").contains(p), "Pflege darf nicht: {p}");
        }
    }
}
