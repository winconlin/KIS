use serde::Deserialize;
use std::path::PathBuf;
use figment::providers::Format;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub ui: UiConfig,
    pub interfaces: InterfaceConfig,
    pub llm: LlmConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
    /// In-Memory-Cache (Task 1.17)
    pub cache: CacheConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    /// Graceful-Shutdown-Timeout in Sekunden (Task 5.12).
    /// Nach SIGTERM: laufende Requests werden bis zu dieser Dauer abgewartet.
    /// Danach wird der Prozess erzwungen beendet.
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_seconds: u64,
}

fn default_shutdown_timeout() -> u64 { 30 }

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8080,
            workers: 4,
            shutdown_timeout_seconds: default_shutdown_timeout(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    /// PostgreSQL Write-Connection-String (oder ENV `KIS_DB_WRITE_URL` / `DATABASE_URL`)
    #[serde(default = "default_write_url")]
    pub write_url: String,
    /// PostgreSQL Read-Replica-Connection-String (optional, None = Write-Pool)
    #[serde(default)]
    pub read_url: Option<String>,
    /// Maximale Connections pro Pool (Standard: 200, für tausende User)
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    /// Timeout für Connection-Acquire in Sekunden
    #[serde(default = "default_acquire_timeout")]
    pub acquire_timeout_seconds: u64,
    /// Idle-Timeout für Connections in Sekunden
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_seconds: u64,
    /// Maximale Connections für den Read-Pool (Task 5.3, optional).
    /// `None` = gleiche Größe wie der Write-Pool (`pool_size`).
    /// Hat nur einen Effekt, wenn `read_url` gesetzt ist.
    #[serde(default)]
    pub read_max_connections: Option<u32>,
}

fn default_write_url() -> String {
    "postgres://kis:kis@localhost:5432/kis".into()
}
fn default_pool_size() -> u32 { 200 }
fn default_acquire_timeout() -> u64 { 10 }
fn default_idle_timeout() -> u64 { 300 }

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            write_url: default_write_url(),
            read_url: None,
            pool_size: default_pool_size(),
            acquire_timeout_seconds: default_acquire_timeout(),
            idle_timeout_seconds: default_idle_timeout(),
            read_max_connections: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct InterfaceConfig {
    pub hl7: Hl7Config,
    pub fhir: FhirConfig,
    pub dicom: DicomConfig,
    pub medico: MedicoInterfaceConfig,
    pub ti: TiInterfaceConfig,
}

/// CGM MEDICO Integration (FHIR R4 + OAuth 2.0 + GDT)
#[derive(Debug, Deserialize, Clone)]
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

/// Telematikinfrastruktur (eGK, ePA, eRezept, eAU, mTLS)
#[derive(Debug, Deserialize, Clone)]
pub struct TiInterfaceConfig {
    pub enabled: bool,
    pub base_url: String,
    pub tls_cert_path: String,
    pub tls_key_path: String,
    pub tls_ca_path: String,
    pub egk_reader_path: String,
    pub request_timeout_seconds: u64,
    pub max_concurrent_requests: usize,
}

impl Default for TiInterfaceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://ti.backend.example.com".into(),
            tls_cert_path: "/etc/kis/tls/cert.pem".into(),
            tls_key_path: "/etc/kis/tls/key.pem".into(),
            tls_ca_path: "/etc/kis/tls/ca.pem".into(),
            egk_reader_path: "/dev/ttyUSB0".into(),
            request_timeout_seconds: 30,
            max_concurrent_requests: 10,
        }
    }
}

impl TiInterfaceConfig {
    /// Pfad zum Client-Zertifikat (PEM)
    pub fn cert_path(&self) -> PathBuf {
        PathBuf::from(&self.tls_cert_path)
    }

    /// Pfad zum Private Key (PEM)
    pub fn key_path(&self) -> PathBuf {
        PathBuf::from(&self.tls_key_path)
    }

    /// Pfad zum CA-Bundle (PEM)
    pub fn ca_path(&self) -> PathBuf {
        PathBuf::from(&self.tls_ca_path)
    }
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            hl7: Hl7Config::default(),
            fhir: FhirConfig::default(),
            dicom: DicomConfig::default(),
            medico: MedicoInterfaceConfig::default(),
            ti: TiInterfaceConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
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
pub struct FhirConfig {
    pub enabled: bool,
    pub base_url: String,
    pub version: String,
}

impl Default for FhirConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: "http://localhost:8080/fhir".into(),
            version: "R4".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
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
pub struct AuditConfig {
    /// Aufbewahrungsfrist in Tagen — DSGVO: mindestens 3650 (10 Jahre)
    pub retention_days: u64,
    pub log_file: String,
}

#[derive(Debug, Deserialize, Clone)]
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

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            master_key_env: "KIS_MASTER_KEY".into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
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

/// Logging-Konfiguration
#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    /// Log-Level: trace, debug, info, warn, error
    pub level: String,
    /// Ausgabe-Format: "json" (Production) oder "pretty" (Entwicklung)
    pub format: String,
}

/// Rate-Limiting-Konfiguration (Task 1.11)
///
/// Sliding-Window-Prinzip pro User:
/// - `normal_per_min`: allgemeine /api/v1/*-Endpunkte
/// - `auth_per_min`:   /auth/*-Endpunkte (Login, Logout, Token-Erneuerung)
///
/// Bei Überschreitung antwortet die Middleware mit HTTP 429 +
/// `Retry-After`-Header.
#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    /// Max. Requests pro Minute pro User (normale Endpunkte)
    pub normal_per_min: u32,
    /// Max. Requests pro Minute pro User (Auth-Endpunkte, Login-Schutz)
    pub auth_per_min: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self { normal_per_min: 100, auth_per_min: 10 }
    }
}

/// In-Memory-Cache-Konfiguration (Task 1.17)
///
/// moka-basierter LRU-Cache für wiederholte READ-Queries:
/// - `ttl_seconds`:  Lebensdauer eines Eintrags (Default: 300s = 5 min)
/// - `max_entries`:  maximale Anzahl an Einträgen (Default: 10.000)
///
/// Bei WRITE-Queries wird der betroffene Eintrag aktiv invalidiert,
/// damit Clients niemals veraltete Daten sehen.
#[derive(Debug, Deserialize, Clone)]
pub struct CacheConfig {
    /// TTL in Sekunden
    pub ttl_seconds: u64,
    /// Maximale Anzahl an Cache-Einträgen
    pub max_entries: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self { ttl_seconds: 300, max_entries: 10_000 }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            format: "json".into(),
        }
    }
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            ui: UiConfig::default(),
            interfaces: InterfaceConfig::default(),
            llm: LlmConfig::default(),
            security: SecurityConfig::default(),
            logging: LoggingConfig::default(),
            cache: CacheConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        // TOML-Datei optional: wird nur gemergt, wenn die Datei existiert.
        // Env-Variablen (KIS_*) ergänzen.
        let mut figment = figment::Figment::new().merge(
            figment::providers::Env::prefixed("KIS_").global(),
        );
        let toml_path = "config/kis.toml";
        if std::path::Path::new(toml_path).exists() {
            figment = figment.merge(figment::providers::Toml::file(toml_path));
        }
        let config = figment.extract::<AppConfig>()?;
        Ok(config)
    }
}
