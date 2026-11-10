//! Konfiguration der Installation (Infrastruktur).
//!
//! Hier steht nur, was beim Start feststehen muss: Netzwerk, Datenbank,
//! Schlüsselquellen, aktive Module. Richtlinien des Hauses (Zeitlimits,
//! Behandlungszusammenhang, Rollen) sind Laufzeitdaten und liegen in der
//! Datenbank ([`crate::platform`]).
//!
//! Reihenfolge (spätere Quellen überschreiben frühere):
//! 1. eingebaute Standardwerte — bewusst sparsam, für ein kleines Haus
//!    ohne Anpassung lauffähig,
//! 2. TOML-Datei (`KIS_CONFIG`, sonst `config/kis.toml`, falls vorhanden),
//! 3. Umgebungsvariablen `KIS_<ABSCHNITT>__<SCHLÜSSEL>`, z. B.
//!    `KIS_DATABASE__WRITE_URL` oder `KIS_SERVER__PORT`.
//!
//! Unbekannte Schlüssel in der Datei führen zu einem Fehler beim Start
//! (Tippfehler bleiben nicht unbemerkt).

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
    pub modules: ModulesConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Wartezeit auf laufende Anfragen beim Beenden
    pub shutdown_timeout_seconds: u64,
    /// Obergrenze je Anfrage; schützt vor hängenden Handlern
    pub request_timeout_seconds: u64,
    /// Maximale Größe eines Request-Bodys in Bytes
    pub max_body_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            shutdown_timeout_seconds: 30,
            request_timeout_seconds: 30,
            max_body_bytes: 1024 * 1024,
        }
    }
}

/// PostgreSQL-Verbindungen.
///
/// Größenordnung für `pool_size` (Verbindungen je Instanz): kleines Haus
/// 5–10, mittleres 20–40, Maximalversorger 50–100 je Instanz bei mehreren
/// Instanzen. Verbindungen werden erst bei Bedarf aufgebaut und nach
/// `idle_timeout_seconds` wieder geschlossen.
#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Laufzeitverbindung (Rolle mit DML-Rechten)
    pub write_url: String,
    /// Optionale Lese-Replica für Auswertungen
    pub read_url: Option<String>,
    /// Optionale Verbindung des Schema-Eigentümers nur für Migrationen
    pub migration_url: Option<String>,
    pub pool_size: u32,
    /// Dauerhaft offen gehaltene Verbindungen (0 = keine)
    pub min_connections: u32,
    pub read_max_connections: Option<u32>,
    pub acquire_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            write_url: "postgres://kis_app@localhost:5432/kis".into(),
            read_url: None,
            migration_url: None,
            pool_size: 10,
            min_connections: 0,
            read_max_connections: None,
            acquire_timeout_seconds: 10,
            idle_timeout_seconds: 300,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct SecurityConfig {
    /// Umgebungsvariable mit dem JWT-Signaturschlüssel (mind. 32 Bytes)
    pub jwt_secret_env: String,
    /// Gültigkeit des Access-Tokens in Sekunden
    pub token_expiry: u64,
    /// Gültigkeit des Refresh-Tokens (= maximale Sitzungsdauer) in Sekunden
    pub refresh_expiry: u64,
    /// Standardwert für die Laufzeit-Einstellung `security.login.max_attempts`
    pub max_login_attempts: u32,
    /// Standardwert für `security.login.lockout_minutes`, in Sekunden
    pub lockout_duration: u64,
    pub rate_limit: RateLimitConfig,
    pub encryption: EncryptionConfig,
    pub tls: TlsConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            jwt_secret_env: "KIS_JWT_SECRET".into(),
            token_expiry: 900,
            refresh_expiry: 86_400,
            max_login_attempts: 5,
            lockout_duration: 900,
            rate_limit: RateLimitConfig::default(),
            encryption: EncryptionConfig::default(),
            tls: TlsConfig::default(),
        }
    }
}

/// TLS direkt im Server. Pfade kommen aus den benannten
/// Umgebungsvariablen; ohne Zertifikat und Key lauscht der Server per HTTP
/// (nur hinter einem TLS-Proxy oder lokal).
#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
    pub cert_env: String,
    pub key_env: String,
    /// Gesetzt = Client-Zertifikat verpflichtend (mTLS)
    pub ca_env: String,
    pub hsts_max_age: u64,
    pub hsts_include_subdomains: bool,
    pub hsts_preload: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_env: "KIS_TLS_CERT".into(),
            key_env: "KIS_TLS_KEY".into(),
            ca_env: "KIS_TLS_CA".into(),
            hsts_max_age: 31_536_000,
            hsts_include_subdomains: true,
            hsts_preload: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct EncryptionConfig {
    /// Umgebungsvariable mit dem Master-Key (32 Bytes, Base64)
    pub master_key_env: String,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            master_key_env: "KIS_MASTER_KEY".into(),
        }
    }
}

/// Anfragen je Minute und Client-IP (Sliding Window, je Instanz).
#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimitConfig {
    #[serde(alias = "requests_per_minute")]
    pub normal_per_min: u32,
    /// Strengeres Limit für `/api/v1/auth/*` (Anmeldeschutz)
    #[serde(alias = "auth_requests_per_minute")]
    pub auth_per_min: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            normal_per_min: 600,
            auth_per_min: 10,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// trace, debug, info, warn, error
    pub level: String,
    /// "json" (Betrieb) oder "pretty" (Entwicklung)
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            format: "json".into(),
        }
    }
}

/// Auswahl der Fachmodule. Kennungen siehe `GET /api/v1/modules`.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModulesConfig {
    /// Abgeschaltete Module (Kernmodule lassen sich nicht abschalten)
    pub disabled: Vec<String>,
    /// Ausdrücklich eingeschaltete experimentelle Module
    pub experimental: Vec<String>,
}

impl AppConfig {
    /// Lädt die Konfiguration (siehe Modulbeschreibung).
    pub fn load() -> anyhow::Result<Self> {
        let explicit = std::env::var("KIS_CONFIG").ok();
        let toml_path = explicit
            .clone()
            .unwrap_or_else(|| "config/kis.toml".to_string());
        if explicit.is_some() && !std::path::Path::new(&toml_path).exists() {
            anyhow::bail!("KIS_CONFIG zeigt auf eine nicht vorhandene Datei: {toml_path}");
        }
        Self::load_from(Some(&toml_path))
    }

    /// Lädt mit expliziter TOML-Datei (oder ohne) und ENV-Overrides.
    pub fn load_from(toml_path: Option<&str>) -> anyhow::Result<Self> {
        use figment::providers::Format;
        let mut figment = figment::Figment::new();
        if let Some(path) = toml_path {
            if std::path::Path::new(path).exists() {
                figment = figment.merge(figment::providers::Toml::file(path));
            }
        }
        // Nur verschachtelte Schlüssel (mit __) gehören zur Konfiguration;
        // KIS_JWT_SECRET, KIS_CONFIG u. ä. sind eigenständige Variablen.
        figment = figment.merge(
            figment::providers::Env::prefixed("KIS_")
                .filter(|key| key.as_str().contains("__"))
                .split("__"),
        );
        let mut config = figment.extract::<AppConfig>()?;
        for url in [
            &mut config.database.read_url,
            &mut config.database.migration_url,
        ] {
            if url.as_deref().map(str::trim) == Some("") {
                *url = None;
            }
        }
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.database.pool_size == 0 {
            anyhow::bail!("database.pool_size muss größer als 0 sein");
        }
        if self.database.min_connections > self.database.pool_size {
            anyhow::bail!("database.min_connections darf pool_size nicht übersteigen");
        }
        if self.database.read_max_connections == Some(0) {
            anyhow::bail!("database.read_max_connections muss größer als 0 sein");
        }
        if !matches!(self.logging.format.as_str(), "json" | "pretty") {
            anyhow::bail!("logging.format muss json oder pretty sein");
        }
        if self.security.token_expiry == 0
            || self.security.refresh_expiry < self.security.token_expiry
        {
            anyhow::bail!("security: refresh_expiry muss mindestens token_expiry betragen");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die mitgelieferte Beispielkonfiguration muss sich laden lassen.
    #[test]
    fn shipped_config_file_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/kis.toml");
        let cfg = AppConfig::load_from(Some(path)).expect("config/kis.toml lädt");
        assert_eq!(cfg.server.port, 8080);
        assert!(
            cfg.database.read_url.is_none(),
            "leere read_url wird zu None"
        );
        assert_eq!(cfg.security.jwt_secret_env, "KIS_JWT_SECRET");
    }

    #[test]
    fn defaults_are_lean_and_valid() {
        let cfg = AppConfig::load_from(None).unwrap();
        assert_eq!(cfg.security.token_expiry, 900);
        assert!(cfg.database.pool_size <= 10, "sparsamer Standard-Pool");
        assert_eq!(cfg.database.min_connections, 0);
        assert!(cfg.modules.disabled.is_empty());
    }

    #[test]
    fn unknown_keys_and_invalid_values_are_rejected() {
        let dir = std::env::temp_dir().join(format!("kis-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("typo.toml");
        std::fs::write(&file, "[server]\nprot = 1\n").unwrap();
        assert!(AppConfig::load_from(file.to_str()).is_err());
        std::fs::write(&file, "[database]\npool_size = 0\n").unwrap();
        assert!(AppConfig::load_from(file.to_str()).is_err());
        std::fs::write(&file, "[modules]\ndisabled = [\"roster\"]\n").unwrap();
        let cfg = AppConfig::load_from(file.to_str()).unwrap();
        assert_eq!(cfg.modules.disabled, ["roster"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
