use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub ui: UiConfig,
    pub interfaces: InterfaceConfig,
    pub llm: LlmConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            workers: 4,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Pfad zur SQLite-Datei (oder ENV `KIS_DATABASE_PATH`)
    #[serde(default = "default_db_path")]
    pub path: String,
    /// Maximale Connections im Pool (Standard: 8)
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    /// SQLite Journal-Modus: "wal" (Standard), "delete", "memory"
    #[serde(default = "default_journal_mode")]
    pub journal_mode: String,
}

fn default_db_path() -> String {
    "data/kis.db".into()
}
fn default_pool_size() -> u32 {
    8
}
fn default_journal_mode() -> String {
    "wal".into()
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
            pool_size: default_pool_size(),
            journal_mode: default_journal_mode(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct UiConfig {
    pub theme: String,
    pub language: String,
    pub auto_refresh: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "de".into(),
            auto_refresh: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct InterfaceConfig {
    pub hl7: Hl7Config,
    pub fhir: FhirConfig,
    pub dicom: DicomConfig,
    pub medico: MedicoInterfaceConfig,
}

/// CGM MEDICO Integration (FHIR R4 + OAuth 2.0 + GDT)
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct MedicoInterfaceConfig {
    pub enabled: bool,
    pub base_url: String,
    pub system_namespace: String,
    pub auth_endpoint: String,
    pub client_id_env: String,
    pub client_secret_env: String,
    pub scopes: Vec<String>,
    pub sync_interval_seconds: u64,
    pub max_concurrent_requests: usize,
    pub request_timeout_seconds: u64,
    pub change_detection_mode: String,
    pub sync_slots: bool,
    pub availability_poll_interval_minutes: u64,
    pub availability_poll_range_days: u32,
    pub gdt_export_enabled: bool,
    pub gdt_export_path: String,
    pub hl7_enabled: bool,
    pub hl7_host: String,
    pub hl7_port: u16,
    pub hl7_ae_title: String,
}

impl Default for MedicoInterfaceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://fhir.medico.example.com/fhir/".into(),
            system_namespace: "https://fhir.medico.example.com".into(),
            auth_endpoint: "https://fhir.medico.example.com/fhir/auth/token".into(),
            client_id_env: "KIS_MEDICO_CLIENT_ID".into(),
            client_secret_env: "KIS_MEDICO_CLIENT_SECRET".into(),
            scopes: vec![
                "system/Patient.read".into(),
                "system/Patient.write".into(),
                "system/Appointment.read".into(),
                "system/Appointment.write".into(),
                "system/HealthcareService.read".into(),
                "system/Schedule.read".into(),
                "system/Slot.read".into(),
            ],
            sync_interval_seconds: 30,
            max_concurrent_requests: 10,
            request_timeout_seconds: 30,
            change_detection_mode: "polling".into(),
            sync_slots: true,
            availability_poll_interval_minutes: 10,
            availability_poll_range_days: 180,
            gdt_export_enabled: true,
            gdt_export_path: "exports/gdt/".into(),
            hl7_enabled: false,
            hl7_host: "10.10.11.100".into(),
            hl7_port: 6667,
            hl7_ae_title: "KIS".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Hl7Config {
    pub enabled: bool,
    pub port: u16,
    pub version: String,
}

impl Default for Hl7Config {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 6667,
            version: "2.5".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct FhirConfig {
    pub enabled: bool,
    pub base_url: String,
    pub version: String,
}

impl Default for FhirConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "http://localhost:8080/fhir".into(),
            version: "R4".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DicomConfig {
    pub enabled: bool,
    pub port: u16,
    pub ae_title: String,
}

impl Default for DicomConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 104,
            ae_title: "KIS".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct SecurityConfig {
    /// Umgebungsvariable, aus der der JWT-Signaturschlüssel gelesen wird
    pub jwt_secret_env: String,
    /// Gültigkeitsdauer des Access-Tokens in Sekunden (Standard: 15 Minuten)
    pub token_expiry: u64,
    /// Gültigkeitsdauer des Refresh-Tokens in Sekunden (Standard: 24 Stunden)
    pub refresh_expiry: u64,
    /// Maximale fehlgeschlagene Login-Versuche vor Sperre
    pub max_login_attempts: u32,
    /// Dauer der Kontosperrung in Sekunden
    pub lockout_duration: u64,
    pub audit: AuditConfig,
    pub rate_limit: RateLimitConfig,
    pub encryption: EncryptionConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AuditConfig {
    /// Betriebliche Aufbewahrungsfrist; keine pauschale gesetzliche Vorgabe.
    pub retention_days: u64,
    pub log_file: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub auth_requests_per_minute: u32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct EncryptionConfig {
    /// Umgebungsvariable, aus der der AES-256-Masterkey (Base64, 32 Bytes) gelesen wird
    pub master_key_env: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            jwt_secret_env: "KIS_JWT_SECRET".into(),
            token_expiry: 900,
            refresh_expiry: 86400,
            max_login_attempts: 5,
            lockout_duration: 900,
            audit: AuditConfig::default(),
            rate_limit: RateLimitConfig::default(),
            encryption: EncryptionConfig::default(),
        }
    }
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            retention_days: 3650,
            log_file: "logs/audit.log".into(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100,
            auth_requests_per_minute: 10,
        }
    }
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            master_key_env: "KIS_MASTER_KEY".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct LlmConfig {
    /// LLM-Provider (z. B. "openrouter")
    pub provider: String,
    /// Umgebungsvariable, aus der der API-Key gelesen wird
    pub api_key_env: String,
    /// Verfügbare Modelle (für Rotation/Selection)
    pub models: Vec<String>,
    /// Interval in Sekunden, nach dem das Modell rotiert wird
    pub rotation_interval: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "openrouter".into(),
            api_key_env: "OPENROUTER_API_KEY".into(),
            models: vec![
                "deepseek/deepseek-chat-v3-0324".into(),
                "qwen/qwen3-coder".into(),
                "meta-llama/llama-4-maverick".into(),
            ],
            rotation_interval: 300,
        }
    }
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        use figment::providers::{Env, Format, Toml};

        // Konfiguration aus TOML-Datei laden (falls vorhanden)
        let config_path =
            std::env::var("KIS_CONFIG_PATH").unwrap_or_else(|_| "config/kis.toml".into());
        let toml_provider = Toml::file(config_path);
        let figment = figment::Figment::new()
            .merge(toml_provider)
            // Umgebungsvariablen mit Präfix KIS_ überlagern die Datei
            .merge(Env::prefixed("KIS_").split("__"));

        let config: AppConfig = figment
            .extract()
            .map_err(|e| anyhow::anyhow!("Konfigurationsfehler: {e}"))?;
        config.validate()?;
        Ok(config)
    }
}

impl AppConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.server.port > 0, "server.port muss größer als 0 sein");
        anyhow::ensure!(
            self.server.workers > 0,
            "server.workers muss größer als 0 sein"
        );
        anyhow::ensure!(
            self.database.pool_size > 0,
            "database.pool_size muss größer als 0 sein"
        );
        anyhow::ensure!(
            matches!(self.database.journal_mode.as_str(), "wal" | "delete"),
            "database.journal_mode muss wal oder delete sein"
        );
        anyhow::ensure!(
            self.security.token_expiry > 0
                && self.security.refresh_expiry >= self.security.token_expiry,
            "Token-Laufzeiten müssen positiv sein; Refresh darf nicht kürzer als Access sein"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use figment::providers::{Format, Toml};

    #[test]
    fn partial_sections_keep_nested_defaults() {
        let config: AppConfig = figment::Figment::new()
            .merge(Toml::string(
                "[server]\nport=9090\n[security.audit]\nlog_file='custom.log'",
            ))
            .extract()
            .unwrap();
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.server.workers, 4);
        assert_eq!(config.security.audit.retention_days, 3650);
        config.validate().unwrap();
    }

    #[test]
    fn invalid_connection_and_token_settings_are_rejected() {
        let mut config = AppConfig::default();
        config.database.pool_size = 0;
        assert!(config.validate().is_err());
        config.database.pool_size = 8;
        config.database.journal_mode = "off".into();
        assert!(config.validate().is_err());
        config.database.journal_mode = "wal".into();
        config.security.refresh_expiry = 1;
        assert!(config.validate().is_err());
    }
}
