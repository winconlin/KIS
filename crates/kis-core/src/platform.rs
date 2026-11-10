//! Plattformzustand je Instanz: Rollenmatrix und Laufzeit-Einstellungen.
//!
//! Beides sind Daten des Hauses (Tabellen `roles`, `role_permissions`,
//! `settings`) und zur Laufzeit änderbar. Damit nicht jede Anfrage sie neu
//! liest, hält jede Instanz einen Schnappschuss im Speicher. Die
//! Sitzungsprüfung liefert die Versionszähler aus `platform_state` in
//! derselben Abfrage mit; nur wenn sich einer geändert hat, wird neu
//! geladen ([`Platform::refresh_if_stale`]). Änderungen wirken dadurch
//! sofort auf allen Instanzen, ohne Zusatzabfragen im Normalbetrieb.
//!
//! Einstellungen: gültige Schlüssel, Typen, Grenzen und Standardwerte
//! stehen in [`SETTINGS`]. Infrastruktur (Datenbank, Ports, Module) gehört
//! nicht hierher, sondern in die Konfigurationsdatei.

use std::sync::{Arc, RwLock};

use serde_json::Value;
use sqlx::PgPool;

use crate::config::SecurityConfig;
use crate::rbac::{PermissionSet, RoleMatrix};

/// Wertebereich einer Einstellung.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingKind {
    Bool,
    Int { min: i64, max: i64 },
}

/// Beschreibung einer Einstellung.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct SettingDef {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
}

pub const TREATMENT_ENFORCE: &str = "security.treatment_context.enforce";
pub const TREATMENT_GRACE_DAYS: &str = "security.treatment_context.grace_days";
pub const EMERGENCY_MINUTES: &str = "security.emergency_access.minutes";
pub const INACTIVITY_MINUTES: &str = "security.session.inactivity_minutes";
pub const LOGIN_MAX_ATTEMPTS: &str = "security.login.max_attempts";
pub const LOGIN_LOCKOUT_MINUTES: &str = "security.login.lockout_minutes";
pub const PASSWORD_MIN_LENGTH: &str = "security.password.min_length";

/// Alle zur Laufzeit änderbaren Einstellungen.
pub const SETTINGS: &[SettingDef] = &[
    SettingDef {
        key: TREATMENT_ENFORCE,
        label: "Zugriff auf Falldaten nur im Behandlungszusammenhang",
        kind: SettingKind::Bool,
    },
    SettingDef {
        key: TREATMENT_GRACE_DAYS,
        label: "Nachlauf des Zugriffs nach Verlegung/Entlassung (Tage)",
        kind: SettingKind::Int { min: 0, max: 365 },
    },
    SettingDef {
        key: EMERGENCY_MINUTES,
        label: "Dauer eines Notfallzugriffs (Minuten)",
        kind: SettingKind::Int { min: 5, max: 1440 },
    },
    SettingDef {
        key: INACTIVITY_MINUTES,
        label: "Automatische Abmeldung nach Inaktivität (Minuten)",
        kind: SettingKind::Int { min: 1, max: 720 },
    },
    SettingDef {
        key: LOGIN_MAX_ATTEMPTS,
        label: "Fehlversuche bis zur Kontosperre",
        kind: SettingKind::Int { min: 1, max: 100 },
    },
    SettingDef {
        key: LOGIN_LOCKOUT_MINUTES,
        label: "Dauer der Kontosperre (Minuten)",
        kind: SettingKind::Int { min: 1, max: 1440 },
    },
    SettingDef {
        key: PASSWORD_MIN_LENGTH,
        label: "Mindestlänge neuer Passwörter",
        kind: SettingKind::Int { min: 8, max: 128 },
    },
];

/// Wirksame Richtlinien (Standardwerte, überlagert durch `settings`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Policies {
    pub treatment_enforce: bool,
    pub treatment_grace_days: i64,
    pub emergency_minutes: i64,
    pub inactivity_minutes: i64,
    pub login_max_attempts: i64,
    pub login_lockout_minutes: i64,
    pub password_min_length: i64,
}

impl Policies {
    /// Standardwerte; Anmelde-Grenzen kommen aus der Konfigurationsdatei.
    pub fn defaults(cfg: &SecurityConfig) -> Self {
        Self {
            treatment_enforce: true,
            treatment_grace_days: 14,
            emergency_minutes: 240,
            inactivity_minutes: 30,
            login_max_attempts: i64::from(cfg.max_login_attempts).clamp(1, 100),
            login_lockout_minutes: (cfg.lockout_duration as i64 / 60).clamp(1, 1440),
            password_min_length: 12,
        }
    }

    fn apply(&mut self, key: &str, value: &Value) {
        let int = value.as_i64();
        match key {
            TREATMENT_ENFORCE => {
                if let Some(b) = value.as_bool() {
                    self.treatment_enforce = b;
                }
            }
            TREATMENT_GRACE_DAYS => {
                self.treatment_grace_days = int.unwrap_or(self.treatment_grace_days)
            }
            EMERGENCY_MINUTES => self.emergency_minutes = int.unwrap_or(self.emergency_minutes),
            INACTIVITY_MINUTES => self.inactivity_minutes = int.unwrap_or(self.inactivity_minutes),
            LOGIN_MAX_ATTEMPTS => self.login_max_attempts = int.unwrap_or(self.login_max_attempts),
            LOGIN_LOCKOUT_MINUTES => {
                self.login_lockout_minutes = int.unwrap_or(self.login_lockout_minutes)
            }
            PASSWORD_MIN_LENGTH => {
                self.password_min_length = int.unwrap_or(self.password_min_length)
            }
            other => tracing::debug!(key = other, "Unbekannte Einstellung ignoriert"),
        }
    }

    /// Aktueller Wert einer Einstellung als JSON (für die Verwaltung).
    pub fn value_of(&self, key: &str) -> Option<Value> {
        Some(match key {
            TREATMENT_ENFORCE => Value::from(self.treatment_enforce),
            TREATMENT_GRACE_DAYS => Value::from(self.treatment_grace_days),
            EMERGENCY_MINUTES => Value::from(self.emergency_minutes),
            INACTIVITY_MINUTES => Value::from(self.inactivity_minutes),
            LOGIN_MAX_ATTEMPTS => Value::from(self.login_max_attempts),
            LOGIN_LOCKOUT_MINUTES => Value::from(self.login_lockout_minutes),
            PASSWORD_MIN_LENGTH => Value::from(self.password_min_length),
            _ => return None,
        })
    }
}

/// Prüft Schlüssel, Typ und Wertebereich einer Einstellung.
pub fn validate_setting(key: &str, value: &Value) -> Result<(), String> {
    let def = SETTINGS
        .iter()
        .find(|d| d.key == key)
        .ok_or_else(|| format!("Unbekannte Einstellung: {key}"))?;
    match def.kind {
        SettingKind::Bool => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| format!("{key} erwartet true oder false")),
        SettingKind::Int { min, max } => match value.as_i64() {
            Some(v) if (min..=max).contains(&v) => Ok(()),
            _ => Err(format!(
                "{key} erwartet eine ganze Zahl von {min} bis {max}"
            )),
        },
    }
}

#[derive(Debug, Clone)]
struct Snapshot {
    rbac_version: i64,
    settings_version: i64,
    roles: RoleMatrix,
    policies: Policies,
}

/// Schnappschuss von Rollenmatrix und Richtlinien einer Instanz.
#[derive(Debug)]
pub struct Platform {
    defaults: Policies,
    state: RwLock<Arc<Snapshot>>,
}

impl Platform {
    /// Lädt den aktuellen Stand aus der Datenbank.
    pub async fn load(pool: &PgPool, security: &SecurityConfig) -> anyhow::Result<Self> {
        let defaults = Policies::defaults(security);
        let snapshot = Self::read(pool, defaults).await?;
        Ok(Self {
            defaults,
            state: RwLock::new(Arc::new(snapshot)),
        })
    }

    /// Plattform ohne Datenbank: Grundausstattung und Standardwerte
    /// (Unit-Tests).
    pub fn builtin(security: &SecurityConfig) -> Self {
        let defaults = Policies::defaults(security);
        Self {
            defaults,
            state: RwLock::new(Arc::new(Snapshot {
                rbac_version: 0,
                settings_version: 0,
                roles: RoleMatrix::builtin(),
                policies: defaults,
            })),
        }
    }

    async fn read(pool: &PgPool, defaults: Policies) -> anyhow::Result<Snapshot> {
        // Versionen zuerst lesen: ändert sich danach etwas, ist der
        // Schnappschuss höchstens „zu neu für seine Version“ und wird beim
        // nächsten Zugriff erneut geladen — nie umgekehrt.
        let (rbac_version, settings_version): (i64, i64) =
            sqlx::query_as("SELECT rbac_version, settings_version FROM platform_state")
                .fetch_one(pool)
                .await?;
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT r.key, p.permission FROM roles r
             LEFT JOIN role_permissions p ON p.role_key = r.key",
        )
        .fetch_all(pool)
        .await?;
        let settings: Vec<(String, Value)> = sqlx::query_as("SELECT key, value FROM settings")
            .fetch_all(pool)
            .await?;
        let mut policies = defaults;
        for (key, value) in &settings {
            if validate_setting(key, value).is_ok() {
                policies.apply(key, value);
            } else {
                tracing::warn!(key, "Ungültige Einstellung in der Datenbank ignoriert");
            }
        }
        Ok(Snapshot {
            rbac_version,
            settings_version,
            roles: RoleMatrix::from_rows(rows),
            policies,
        })
    }

    fn snapshot(&self) -> Arc<Snapshot> {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Lädt neu, wenn die übergebenen Versionen von den gehaltenen
    /// abweichen. Im Normalfall ein Vergleich zweier Zahlen.
    pub async fn refresh_if_stale(
        &self,
        pool: &PgPool,
        rbac_version: i64,
        settings_version: i64,
    ) -> anyhow::Result<()> {
        let current = self.snapshot();
        if current.rbac_version == rbac_version && current.settings_version == settings_version {
            return Ok(());
        }
        self.reload(pool).await
    }

    /// Lädt den Stand unbedingt neu.
    pub async fn reload(&self, pool: &PgPool) -> anyhow::Result<()> {
        let fresh = Arc::new(Self::read(pool, self.defaults).await?);
        *self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = fresh;
        Ok(())
    }

    /// Wirksame Rechte aus Haupt- und Zusatzrollen.
    pub fn permissions<'a>(&self, roles: impl IntoIterator<Item = &'a str>) -> PermissionSet {
        self.snapshot().roles.effective(roles)
    }

    /// Aktuelle Richtlinien.
    pub fn policies(&self) -> Policies {
        self.snapshot().policies
    }
}

/// Setzt eine Einstellung (geprüft) und erhöht die Version. In der
/// Transaktion des Aufrufers, zusammen mit dem Audit-Eintrag.
pub async fn write_setting(
    conn: &mut sqlx::PgConnection,
    key: &str,
    value: &Value,
    updated_by: uuid::Uuid,
) -> Result<(), String> {
    validate_setting(key, value)?;
    sqlx::query(
        "INSERT INTO settings (key, value, updated_by) VALUES ($1, $2, $3)
         ON CONFLICT (key) DO UPDATE
         SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .bind(updated_by)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("DB: {e}"))?;
    sqlx::query("UPDATE platform_state SET settings_version = settings_version + 1")
        .execute(conn)
        .await
        .map_err(|e| format!("DB: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validation() {
        assert!(validate_setting(TREATMENT_ENFORCE, &json!(false)).is_ok());
        assert!(validate_setting(TREATMENT_ENFORCE, &json!(1)).is_err());
        assert!(validate_setting(INACTIVITY_MINUTES, &json!(30)).is_ok());
        assert!(validate_setting(INACTIVITY_MINUTES, &json!(0)).is_err());
        assert!(validate_setting(INACTIVITY_MINUTES, &json!("30")).is_err());
        assert!(validate_setting(PASSWORD_MIN_LENGTH, &json!(4)).is_err());
        assert!(validate_setting("gibt.es.nicht", &json!(1)).is_err());
    }

    #[test]
    fn every_setting_has_default_and_applies() {
        let mut p = Policies::defaults(&SecurityConfig::default());
        for def in SETTINGS {
            let current = p.value_of(def.key).expect("Standardwert vorhanden");
            assert!(
                validate_setting(def.key, &current).is_ok(),
                "Standardwert von {} ist gültig",
                def.key
            );
        }
        p.apply(TREATMENT_ENFORCE, &json!(false));
        p.apply(EMERGENCY_MINUTES, &json!(60));
        assert!(!p.treatment_enforce);
        assert_eq!(p.emergency_minutes, 60);
        assert_eq!(p.value_of(EMERGENCY_MINUTES), Some(json!(60)));
    }

    #[test]
    fn builtin_platform() {
        let p = Platform::builtin(&SecurityConfig::default());
        assert!(p.policies().treatment_enforce);
        assert!(p
            .permissions(["arzt"])
            .contains(crate::rbac::Permission::ClinicalWrite));
        assert!(p.permissions(["unbekannt"]).is_empty());
    }
}
