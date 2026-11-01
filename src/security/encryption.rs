//! AES-256-GCM-Verschlüsselung sensibler Patientendaten (at rest)
//!
//! Masterkey kommt aus der Umgebung (Base64, 32 Bytes).
//! Pro Datensatz wird ein Schlüssel aus Masterkey + Datensatz-ID (HKDF-artige
//! Ableitung über HMAC-SHA-256) abgeleitet, damit ein Kompromittieren eines
//! einzigen per-Record-Schlüssels nicht alle Datensätze preisgibt.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm,
    Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::Mac;
use rand::RngCore;
use sha2::Sha256;
use std::sync::OnceLock;

/// Fehler bei der Verschlüsselung/Entschlüsselung
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Masterkey fehlt oder ungültig: {0}")]
    InvalidMasterKey(String),
    #[error("Entschlüsselung fehlgeschlagen (Zugriff verweigert oder Daten manipuliert)")]
    DecryptionFailed,
    #[error("Base64-Dekodierung fehlgeschlagen: {0}")]
    Base64(String),
    #[error("Zu kurzes Verschlüsselungs-Plaintext/Nonce-Problem: {0}")]
    InvalidPayload(String),
}

/// Globaler Verschlüsselungsdienst (einmalig initialisiert, thread-sicher)
pub static ENCRYPTOR: OnceLock<CryptoService> = OnceLock::new();

/// Initialisiert den globalen Verschlüsselungsdienst.
/// Ruft man die Funktion mehrfach auf, gewinnt der erste Aufruf.
pub fn init_crypto(master_key_env: &str) -> anyhow::Result<&'static CryptoService> {
    let key_bytes = std::env::var(master_key_env)
        .map_err(|_| CryptoError::InvalidMasterKey(format!("{master_key_env} nicht gesetzt")))?;
    let key = B64
        .decode(key_bytes)
        .map_err(|e| CryptoError::Base64(e.to_string()))?;
    if key.len() != 32 {
        return Err(CryptoError::InvalidMasterKey(format!(
            "Masterkey muss 32 Bytes (256 Bit) sein, sind aber {} Bytes",
            key.len()
        ))
        .into());
    }
    ENCRYPTOR
        .set(CryptoService { master_key: key })
        .map_err(|_| anyhow::anyhow!("Verschlüsselungsdienst bereits initialisiert"))?;
    Ok(ENCRYPTOR.get().unwrap())
}

/// Liefert den globalen Verschlüsselungsdienst (panics, wenn init_crypto nicht lief)
pub fn get() -> &'static CryptoService {
    ENCRYPTOR
        .get()
        .expect("init_crypto() muss vor get() aufgerufen werden")
}

/// Verschlüsselungsdienst: Masterkey + per-Record-Ableitung
#[derive(Clone)]
pub struct CryptoService {
    master_key: Vec<u8>,
}

impl CryptoService {
    /// Verschlüsselt einen Wert mit AES-256-GCM.
    ///
    /// Ausgabeformat (Base64): nonce (12 Bytes) || ciphertext + tag (16 Bytes)
    /// Der per-Record-Schlüssel wird aus Masterkey + record_id abgeleitet.
    pub fn encrypt(&self, record_id: &str, plaintext: &str) -> Result<String, CryptoError> {
        let key = self.derive_record_key(record_id);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CryptoError::InvalidMasterKey(e.to_string()))?;
        // Zufällige 96-Bit-Nonce je Verschlüsselung
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| CryptoError::DecryptionFailed)?;

        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ciphertext);
        Ok(B64.encode(out))
    }

    /// Entschlüsselt einen per encrypt() erzeugten Wert.
    pub fn decrypt(&self, record_id: &str, encoded: &str) -> Result<String, CryptoError> {
        let data = B64
            .decode(encoded)
            .map_err(|e| CryptoError::Base64(e.to_string()))?;
        if data.len() < 28 {
            return Err(CryptoError::InvalidPayload(format!(
                "Payload zu kurz: {} Bytes (min. 28)",
                data.len()
            )));
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let key = self.derive_record_key(record_id);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CryptoError::InvalidMasterKey(e.to_string()))?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)?;
        String::from_utf8(plaintext).map_err(|e| CryptoError::InvalidPayload(e.to_string()))
    }

    /// Leitet einen per-Record-Schlüssel aus Masterkey und Datensatz-ID ab (HMAC-SHA-256).
    fn derive_record_key(&self, record_id: &str) -> [u8; 32] {
        let mut mac = <hmac::Hmac<Sha256> as Mac>::new_from_slice(&self.master_key)
            .expect("HMAC akzeptiert jede Schlüsselgröße");
        mac.update(b"KIS-RECORD-KEY-V1");
        mac.update(record_id.as_bytes());
        mac.finalize().into_bytes().to_vec()[..32].try_into().unwrap()
    }

    /// Erzeugt einen zufälligen 32-Byte-Masterkey (Base64) — für Initialisierung/Rotation.
    pub fn generate_master_key() -> String {
        let mut rng = rand::thread_rng();
        let mut key = [0u8; 32];
        rng.fill_bytes(&mut key);
        B64.encode(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> CryptoService {
        CryptoService {
            master_key: vec![7u8; 32],
        }
    }

    #[test]
    fn roundtrip() {
        let s = svc();
        let enc = s.encrypt("patient-1", "A1B2C3D4").unwrap();
        assert_ne!(enc, "A1B2C3D4");
        let dec = s.decrypt("patient-1", &enc).unwrap();
        assert_eq!(dec, "A1B2C3D4");
    }

    #[test]
    fn wrong_record_id_fails() {
        let s = svc();
        let enc = s.encrypt("patient-1", "geheim").unwrap();
        assert!(s.decrypt("patient-2", &enc).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let s = svc();
        let mut enc = s.encrypt("patient-1", "geheim").unwrap();
        // Ersten Base64-Char ändern →Nonce/Text manipulier
        enc.insert(2, 'A');
        assert!(s.decrypt("patient-1", &enc).is_err());
    }
}
