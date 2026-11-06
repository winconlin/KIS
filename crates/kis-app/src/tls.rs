//! TLS 1.3 für den KIS-Server (rustls, ring-Backend).
//!
//! Zertifikat, Key und optionales Client-CA-Bundle werden aus den in der
//! Konfiguration benannten ENV-Variablen gelesen (Standard: `KIS_TLS_CERT`,
//! `KIS_TLS_KEY`, `KIS_TLS_CA`).
//!
//! - Nur TLS 1.3 wird angeboten.
//! - Ist ein CA-Bundle gesetzt, wird ein Client-Zertifikat **verlangt**
//!   (mTLS). Ein gesetztes, aber nicht verwendbares Bundle bricht den Start
//!   ab — es gibt keinen stillen Rückfall auf TLS ohne Client-Prüfung.
//! - Fehlen Zertifikat und Key, läuft der Server über HTTP. Das ist nur
//!   hinter einem TLS-terminierenden Reverse-Proxy (siehe
//!   `config/nginx/kis.conf`) oder für lokale Entwicklung zulässig.

use std::sync::Arc;

use anyhow::Context;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tracing::{info, warn};

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Erzeugt einen rustls `ServerConfig` oder `None`, wenn TLS nicht
/// konfiguriert ist.
pub fn build_server_config(
    tls_config: &kis_core::config::TlsConfig,
) -> anyhow::Result<Option<Arc<rustls::ServerConfig>>> {
    let cert_path = std::env::var(&tls_config.cert_env).unwrap_or_default();
    let key_path = std::env::var(&tls_config.key_env).unwrap_or_default();

    match (cert_path.is_empty(), key_path.is_empty()) {
        (true, true) => {
            warn!("TLS nicht konfiguriert — HTTP (nur hinter TLS-Proxy oder lokal)");
            return Ok(None);
        }
        (false, false) => {}
        _ => anyhow::bail!(
            "{} und {} müssen gemeinsam gesetzt sein",
            tls_config.cert_env,
            tls_config.key_env
        ),
    }

    let cert_data = std::fs::read(&cert_path)
        .with_context(|| format!("Zertifikat nicht lesbar: {cert_path}"))?;
    let key_data = std::fs::read(&key_path)
        .with_context(|| format!("Private Key nicht lesbar: {key_path}"))?;

    let certs = CertificateDer::pem_slice_iter(&cert_data)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("PEM-Zertifikat konnte nicht geparst werden: {e:?}"))?;
    if certs.is_empty() {
        anyhow::bail!("Kein Zertifikat im PEM gefunden: {cert_path}");
    }
    let key = PrivateKeyDer::from_pem_slice(&key_data)
        .map_err(|e| anyhow::anyhow!("Kein gültiger Private Key in {key_path}: {e:?}"))?;

    let builder = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .context("TLS-1.3-Konfiguration fehlgeschlagen")?;

    let ca_path = std::env::var(&tls_config.ca_env).unwrap_or_default();
    let builder = if ca_path.is_empty() {
        builder.with_no_client_auth()
    } else {
        let ca_data = std::fs::read(&ca_path)
            .with_context(|| format!("CA-Bundle nicht lesbar: {ca_path}"))?;
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(&ca_data) {
            let cert =
                cert.map_err(|e| anyhow::anyhow!("CA-Bundle konnte nicht geparst werden: {e:?}"))?;
            roots.add(cert).context("CA-Zertifikat ungültig")?;
        }
        if roots.is_empty() {
            anyhow::bail!("CA-Bundle enthält keine Zertifikate: {ca_path}");
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            provider(),
        )
        .build()
        .context("Client-Zertifikatsprüfung konnte nicht erstellt werden")?;
        info!(ca = %ca_path, "mTLS aktiv: Client-Zertifikat erforderlich");
        builder.with_client_cert_verifier(verifier)
    };

    let mut config = builder
        .with_single_cert(certs, key)
        .context("Zertifikat und Key passen nicht zusammen")?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    info!(cert = %cert_path, "TLS 1.3 aktiv (rustls)");
    Ok(Some(Arc::new(config)))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(prefix: &str) -> kis_core::config::TlsConfig {
        kis_core::config::TlsConfig {
            cert_env: format!("{prefix}_CERT"),
            key_env: format!("{prefix}_KEY"),
            ca_env: format!("{prefix}_CA"),
            ..Default::default()
        }
    }

    #[test]
    fn no_tls_when_unset() {
        assert!(build_server_config(&cfg("KIS_TLS_TEST_UNSET"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn half_configured_tls_is_rejected() {
        std::env::set_var("KIS_TLS_TEST_HALF_CERT", "/nonexistent.pem");
        assert!(build_server_config(&cfg("KIS_TLS_TEST_HALF")).is_err());
    }

    #[test]
    fn hsts_value() {
        let c = kis_core::config::TlsConfig {
            hsts_max_age: 10,
            hsts_include_subdomains: true,
            hsts_preload: false,
            ..Default::default()
        };
        assert_eq!(hsts_header(&c), "max-age=10; includeSubDomains");
    }
}
