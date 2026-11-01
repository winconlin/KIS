use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub ui: UiConfig,
    pub interfaces: InterfaceConfig,
    pub llm: LlmConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8080,
            workers: 4,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub pool_size: usize,
    pub journal_mode: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "data/kis.db".into(),
            pool_size: 8,
            journal_mode: "wal".into(),
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

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            hl7: Hl7Config::default(),
            fhir: FhirConfig::default(),
            dicom: DicomConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key_env: String,
    pub models: Vec<String>,
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            ui: UiConfig::default(),
            interfaces: InterfaceConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config = figment::Figment::new()
            .merge(figment::providers::Env::prefixed("KIS_"))
            .merge(
                figment::providers::Toml::file("config/kis.toml")
                    .map(|f| figment::providers::Data::from(f))
                    .unwrap_or_else(|_| figment::providers::Data::from(AppConfig::default())),
            )
            .extract::<AppConfig>()?;
        Ok(config)
    }
}
