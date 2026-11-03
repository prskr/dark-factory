//! The instance signing key of lifecycle event deliveries (design D5): Ed25519 from a PEM PKCS#8
//! file, signatures and public key in the Standard Webhooks form (`v1a,…`, `whpk_…`).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use rustls::pki_types::PrivatePkcs8KeyDer;
use rustls::pki_types::pem::PemObject as _;

/// Why the signing key cannot be used.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SigningKeyError {
    #[error("cannot read the lifecycle event signing key {}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the lifecycle event signing key is not a PEM `PRIVATE KEY` (PKCS#8): {0}")]
    Pem(String),
    #[error("the lifecycle event signing key is not an Ed25519 key: {0}")]
    NotEd25519(String),
}

/// The Ed25519 key that signs every delivery attempt.
#[derive(Debug)]
pub(crate) struct SigningKey {
    pair: Ed25519KeyPair,
    public_key: String,
}

impl SigningKey {
    /// Reads the key from `path` (`lifecycleEvents.signingKeyFile`).
    pub(crate) fn load(path: &Path) -> Result<Self, SigningKeyError> {
        let pem = std::fs::read(path).map_err(|source| SigningKeyError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::from_pem(&pem)
    }

    /// Parses a PEM `PRIVATE KEY`. cert-manager writes PKCS#8 v1 without the public key, which
    /// the unchecked variant accepts.
    pub(crate) fn from_pem(pem: &[u8]) -> Result<Self, SigningKeyError> {
        let der = PrivatePkcs8KeyDer::from_pem_slice(pem)
            .map_err(|e| SigningKeyError::Pem(e.to_string()))?;
        let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(der.secret_pkcs8_der())
            .map_err(|e| SigningKeyError::NotEd25519(e.to_string()))?;
        let public_key = format!("whpk_{}", STANDARD.encode(pair.public_key().as_ref()));
        Ok(Self { pair, public_key })
    }

    /// The public key as receivers configure it: `whpk_<base64>`.
    pub(crate) fn public_key(&self) -> &str {
        &self.public_key
    }

    /// The `webhook-signature` value of one attempt: `v1a,<base64>` over `{id}.{timestamp}.{body}`.
    pub(crate) fn sign(&self, id: &str, timestamp: i64, body: &[u8]) -> String {
        let mut message = format!("{id}.{timestamp}.").into_bytes();
        message.extend_from_slice(body);
        format!("v1a,{}", STANDARD.encode(self.pair.sign(&message)))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Keys and signature checks for tests.

    use ring::rand::{SecureRandom as _, SystemRandom};
    use ring::signature::{ED25519, UnparsedPublicKey};

    use super::*;

    /// The RFC 8410 PKCS#8 v1 prefix of an Ed25519 private key; the 32-byte seed follows.
    pub(crate) const PKCS8_V1_PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];

    pub(crate) fn pem(der: &[u8]) -> String {
        let body = STANDARD.encode(der);
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(64)
            .map(|line| std::str::from_utf8(line).unwrap())
            .collect();
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            lines.join("\n")
        )
    }

    /// A key as cert-manager writes it, and its seed.
    pub(crate) fn cert_manager_key() -> (String, [u8; 32]) {
        let mut seed = [0; 32];
        SystemRandom::new().fill(&mut seed).unwrap();
        let der: Vec<u8> = PKCS8_V1_PREFIX.iter().chain(&seed).copied().collect();
        (pem(&der), seed)
    }

    pub(crate) fn public_key_bytes(key: &SigningKey) -> Vec<u8> {
        let encoded = key.public_key().strip_prefix("whpk_").unwrap();
        STANDARD.decode(encoded).unwrap()
    }

    pub(crate) fn verifies(key: &SigningKey, signature: &str, message: &[u8]) -> bool {
        let signature = STANDARD
            .decode(signature.strip_prefix("v1a,").unwrap())
            .unwrap();
        UnparsedPublicKey::new(&ED25519, public_key_bytes(key))
            .verify(message, &signature)
            .is_ok()
    }

    /// A fresh key as cert-manager writes it.
    pub(crate) fn generate() -> SigningKey {
        SigningKey::from_pem(cert_manager_key().0.as_bytes()).expect("generated key")
    }
}

#[cfg(test)]
mod tests {
    use rsa::pkcs8::{EncodePrivateKey as _, LineEnding};

    use super::testing::*;
    use super::*;

    #[test]
    fn cert_manager_pkcs8_v1_key_loads() {
        let (pem, seed) = cert_manager_key();
        let key = SigningKey::from_pem(pem.as_bytes()).unwrap();
        let expected = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        assert_eq!(public_key_bytes(&key), expected.public_key().as_ref());
    }

    #[test]
    fn rsa_key_and_garbage_are_rejected() {
        let rsa = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 1024).unwrap();
        let rsa_pem = rsa.to_pkcs8_pem(LineEnding::LF).unwrap();
        assert!(matches!(
            SigningKey::from_pem(rsa_pem.as_bytes()),
            Err(SigningKeyError::NotEd25519(_))
        ));
        assert!(matches!(
            SigningKey::from_pem(b"not a key"),
            Err(SigningKeyError::Pem(_))
        ));
    }

    #[test]
    fn missing_key_file_names_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tls.key");
        let err = SigningKey::load(&path).unwrap_err();
        assert!(matches!(err, SigningKeyError::Read { .. }));
        assert!(err.to_string().contains("tls.key"), "{err}");
    }

    #[test]
    fn public_key_is_whpk_encoded() {
        let key = SigningKey::from_pem(cert_manager_key().0.as_bytes()).unwrap();
        assert!(key.public_key().starts_with("whpk_"));
        assert_eq!(public_key_bytes(&key).len(), 32);
    }

    #[test]
    fn signature_covers_id_timestamp_and_body() {
        let key = SigningKey::from_pem(cert_manager_key().0.as_bytes()).unwrap();
        let signature = key.sign("evt_1", 1_700_000_000, br#"{"a":1}"#);
        assert!(verifies(&key, &signature, br#"evt_1.1700000000.{"a":1}"#));
        assert!(!verifies(&key, &signature, br#"evt_1.1700000000.{"a":2}"#));
    }
}
