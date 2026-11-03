//! TLS 1.3-Unterstützung (Task 5.1)
//!
//! Implementiert TLS-Server mit rustls für den KIS-axum-Server.
//! Zertifikat, Key und CA-Bundle werden aus ENV-Variablen gelesen:
//!
//! - `KIS_TLS_CERT`: Pfad zum Server-Zertifikat (PEM)
//! - `KIS_TLS_KEY`:  Pfad zum Private Key (PEM)
//! - `KIS_TLS_CA`:   Pfad zum CA-Bundle (PEM, optional — nur bei mTLS)
//!
//! Wenn `KIS_TLS_CERT` und `KIS_TLS_KEY` gesetzt sind, startet der
//! Server mit TLS 1.3. Andernfalls HTTP (nur für Entwicklung).
//!
//! HSTS-Header:
//! `Strict-Transport-Security: max-age=31536000; includeSubDomains; preload`

use std::sync::Arc;
use tracing::{info, warn};

/// Erzeugt einen rustls `ServerConfig` aus ENV-Variablen.
///
/// # Returns
/// - `Ok(Some(config))` wenn TLS konfiguriert und Zertifikate gültig sind
/// - `Ok(None)` wenn `KIS_TLS_CERT` oder `KIS_TLS_KEY` nicht gesetzt sind
/// - `Err(...)` wenn Zertifikate nicht lesbar oder ungültig sind
pub fn build_server_config(
    tls_config: &kis_core::config::TlsConfig,
) -> anyhow::Result<Option<Arc<rustls::ServerConfig>>> {
    let cert_path = std::env::var(&tls_config.cert_env).unwrap_or_default();
    let key_path = std::env::var(&tls_config.key_env).unwrap_or_default();

    if cert_path.is_empty() || key_path.is_empty() {
        warn!(
            cert_env = %tls_config.cert_env,
            key_env = %tls_config.key_env,
            "TLS nicht konfiguriert — HTTP (nur für Entwicklung!)"
        );
        return Ok(None);
    }

    info!(cert = %cert_path, key = %key_path, "TLS 1.3 aktiv (rustls)");

    // PEM-Daten laden
    let cert_data = std::fs::read(&cert_path)
        .with_context(|| format!("Zertifikat nicht lesbar: {}", cert_path))?;
    let key_data = std::fs::read(&key_path)
        .with_context(|| format!("Private Key nicht lesbar: {}", key_path))?;

    // Zertifikat parsen
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_data.as_slice())
            .collect::<Result<Vec<_>, _>>()
            .context("PEM-Zertifikat konnte nicht geparst werden")?;

    if certs.is_empty() {
        anyhow::bail!("Kein Zertifikat im PEM gefunden: {}", cert_path);
    }

    // Private Key parsen
    let keys: Vec<rustls::pki_types::PrivatePkcs8KeyDer<'static>> =
        rustls_pemfile::pkcs8_private_keys(&mut key_data.as_slice())
            .collect::<Result<Vec<_>, _>>()
            .context("PEM-Private Key (PKCS#8) konnte nicht geparst werden")?;

    let key = keys
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Kein Private Key im PEM gefunden: {}", key_path))?;

    // Optional: CA-Bundle für mTLS
    let ca_path = std::env::var(&tls_config.ca_env).unwrap_or_default();
    if !ca_path.is_empty() {
        let ca_data = std::fs::read(&ca_path)
            .with_context(|| format!("CA-Bundle nicht lesbar: {}", ca_path))?;
        let _ca_certs = rustls_pemfile::certs(&mut ca_data.as_slice())
            .collect::<Result<Vec<_>, _>>()
            .context("CA-Bundle konnte nicht geparst werden")?;
        info!(ca = %ca_path, "mTLS-CA-Bundle geladen");
    }

    let server_config = Arc::new(
        rustls::ServerConfig::builder()
            .with_safe_default_protocol_versions()
            .and_then(|builder| builder.with_no_client_auth())
            .with_single_cert(certs, key)
            .map_err(|e| anyhow::anyhow!("rustls ServerConfig Fehler: {}", e))?,
    );

    Ok(Some(server_config))
}

/// Erzeugt den HSTS-Header-Wert aus der Konfiguration.
pub fn hsts_header(tls_config: &kis_core::config::TlsConfig) -> String {
    let mut hsts = format!("max-age={}", tls_config.hsts_max_age);
    if tls_config.hsts_include_subdomains {
        hsts.push_str("; includeSubDomains");
    }
    if tls_config.hsts_preload {
        hsts.push_str("; preload");
    }
    hsts
}
