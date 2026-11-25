//! The persona signing key: Ed25519 SSH keys and SSHSIG signatures in namespace `git`
//! (`specs/commit-signing/spec.md`). The private key stays in a Secret and in memory of the control
//! plane; this module holds the pure key and signature handling shared by the control plane and
//! the gateway.

use std::collections::BTreeMap;

use secrecy::{ExposeSecret, SecretString};
use ssh_key::{
    Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey, SshSig, private::Ed25519Keypair,
    private::KeypairData,
};
use thiserror::Error;

use crate::PersonaId;
use crate::seams::{SecretError, SecretKeyRef, SecretStore};

/// The SSHSIG namespace Git uses for commit signatures.
pub const GIT_NAMESPACE: &str = "git";

/// The longest Forgejo token that is signed.
const MAX_FORGEJO_TOKEN_CHARS: usize = 256;

/// The longest hostname.
const MAX_DOMAIN_CHARS: usize = 253;

/// Where the signing key of `persona` is stored: Secret `df-persona-key-<id>`, key `id_ed25519`.
#[must_use]
pub fn secret_ref(persona: PersonaId) -> SecretKeyRef {
    SecretKeyRef {
        name: format!("df-persona-key-{persona}"),
        key: "id_ed25519".into(),
    }
}

/// Labels of the persona key Secrets (backup and RBAC selection).
#[must_use]
pub fn secret_labels() -> BTreeMap<String, String> {
    BTreeMap::from([("dark-factory.io/persona-key".into(), "true".into())])
}

/// Why a key or signature operation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SigningKeyError {
    #[error("the random generator failed: {0}")]
    Random(String),
    #[error("the signing key is not a valid Ed25519 OpenSSH key: {0}")]
    InvalidKey(String),
    #[error("signing failed: {0}")]
    Sign(String),
    #[error("the token is not valid: {0}")]
    InvalidToken(String),
    #[error("{0}")]
    InvalidDomain(String),
    #[error(transparent)]
    Secret(#[from] SecretError),
}

/// Reads the signing key of `persona` from its Secret.
pub async fn load(
    secrets: &dyn SecretStore,
    persona: PersonaId,
) -> Result<SigningKey, SigningKeyError> {
    SigningKey::from_openssh(&secrets.get(&secret_ref(persona)).await?)
}

/// A persona's signing key pair: the private key in OpenSSH format, the public half and its
/// fingerprint.
pub struct SigningKey {
    private: SecretString,
    key: PrivateKey,
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("fingerprint", &self.fingerprint())
            .finish_non_exhaustive()
    }
}

impl SigningKey {
    /// Generates a new key from the operating system's random generator.
    pub fn generate() -> Result<Self, SigningKeyError> {
        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed).map_err(|e| SigningKeyError::Random(e.to_string()))?;
        let pair = Ed25519Keypair::from_seed(&seed);
        let key = PrivateKey::new(KeypairData::Ed25519(pair), "")
            .map_err(|e| SigningKeyError::InvalidKey(e.to_string()))?;
        Self::from_key(key)
    }

    /// Parses an OpenSSH private key.
    pub fn from_openssh(private: &SecretString) -> Result<Self, SigningKeyError> {
        let key = PrivateKey::from_openssh(private.expose_secret())
            .map_err(|e| SigningKeyError::InvalidKey(e.to_string()))?;
        if key.algorithm() != Algorithm::Ed25519 {
            return Err(SigningKeyError::InvalidKey(
                "the key is not an Ed25519 key".into(),
            ));
        }
        Self::from_key(key)
    }

    fn from_key(key: PrivateKey) -> Result<Self, SigningKeyError> {
        let private = key
            .to_openssh(LineEnding::LF)
            .map_err(|e| SigningKeyError::InvalidKey(e.to_string()))?;
        Ok(Self {
            private: SecretString::from(private.to_string()),
            key,
        })
    }

    /// The private key in OpenSSH format, for the Secret.
    #[must_use]
    pub fn private_openssh(&self) -> &SecretString {
        &self.private
    }

    /// The public key in OpenSSH format (`ssh-ed25519 AAAA…`, no comment).
    #[must_use]
    pub fn public_key(&self) -> String {
        public_key_line(self.key.public_key())
    }

    /// The SHA-256 fingerprint (`SHA256:…`).
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.key.fingerprint(HashAlg::Sha256).to_string()
    }

    /// Signs `payload` as `ssh-keygen -Y sign -n git` does (SHA-512) and returns the armored
    /// signature.
    pub fn sign(&self, payload: &[u8]) -> Result<String, SigningKeyError> {
        self.sign_in(GIT_NAMESPACE, payload)
    }

    /// Signs the token Forgejo shows when it verifies an SSH key, in the namespace `domain` (the
    /// domain of the Forgejo instance); the armored signature. Surrounding whitespace of a pasted
    /// token is dropped: Forgejo verifies the token without it.
    pub fn sign_forgejo_token(&self, domain: &str, token: &str) -> Result<String, SigningKeyError> {
        let domain = domain.trim();
        check_forgejo_domain(domain)?;
        let token = token.trim();
        if token.is_empty() || token.chars().count() > MAX_FORGEJO_TOKEN_CHARS {
            return Err(SigningKeyError::InvalidToken(format!(
                "it must have 1 to {MAX_FORGEJO_TOKEN_CHARS} characters"
            )));
        }
        if token.chars().any(char::is_control) {
            return Err(SigningKeyError::InvalidToken(
                "it contains characters that cannot be printed".into(),
            ));
        }
        self.sign_in(domain, token.as_bytes())
    }

    fn sign_in(&self, namespace: &str, payload: &[u8]) -> Result<String, SigningKeyError> {
        let sig = self
            .key
            .sign(namespace, HashAlg::Sha512, payload)
            .map_err(|e| SigningKeyError::Sign(e.to_string()))?;
        sig.to_pem(LineEnding::LF)
            .map_err(|e| SigningKeyError::Sign(e.to_string()))
    }
}

/// The SHA-256 fingerprint of a public key in OpenSSH format.
pub fn fingerprint_of(public_key: &str) -> Result<String, SigningKeyError> {
    let key = PublicKey::from_openssh(public_key)
        .map_err(|e| SigningKeyError::InvalidKey(e.to_string()))?;
    Ok(key.fingerprint(HashAlg::Sha256).to_string())
}

/// Why a signature does not verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum VerifyError {
    /// A valid signature envelope made by a different key.
    #[error("signed by another key")]
    OtherKey,
    /// Not a signature, a wrong namespace, or a signature that does not match the payload.
    #[error("invalid signature")]
    Invalid,
}

/// Verifies the armored SSH signature `signature` over `payload` against `public_key` (OpenSSH
/// format) in namespace `git`.
pub fn verify(public_key: &str, payload: &[u8], signature: &[u8]) -> Result<(), VerifyError> {
    verify_in(GIT_NAMESPACE, public_key, payload, signature)
}

/// Verifies a signature over a Forgejo key-verification token in the namespace `domain`.
pub fn verify_forgejo_token(
    public_key: &str,
    domain: &str,
    token: &str,
    signature: &[u8],
) -> Result<(), VerifyError> {
    verify_in(domain, public_key, token.as_bytes(), signature)
}

/// A Git commit object split into what was signed and its signature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SplitCommit {
    /// The commit object without its signature header: the bytes that were signed.
    pub payload: Vec<u8>,
    /// The armored signature of the `gpgsig` (or `gpgsig-sha256`) header; `None` if unsigned.
    pub signature: Option<Vec<u8>>,
}

/// Splits the raw commit object `raw` into the signed payload and the signature of its `gpgsig`
/// header (continuation lines start with a space).
#[must_use]
pub fn split_commit(raw: &[u8]) -> SplitCommit {
    // The header block ends at the first empty line; a message may contain anything.
    let header_end = raw
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .map_or(raw.len(), |at| at + 1);
    let (headers, rest) = raw.split_at(header_end);
    let (mut payload, mut signature) = (Vec::with_capacity(raw.len()), Vec::new());
    let mut in_signature = false;
    let mut is_signed = false;
    for line in headers.split_inclusive(|byte| *byte == b'\n') {
        if !in_signature && !is_signed {
            let value = line
                .strip_prefix(b"gpgsig ")
                .or_else(|| line.strip_prefix(b"gpgsig-sha256 "));
            if let Some(value) = value {
                in_signature = true;
                is_signed = true;
                signature.extend_from_slice(value);
                continue;
            }
        } else if in_signature {
            if let Some(continued) = line.strip_prefix(b" ") {
                signature.extend_from_slice(continued);
                continue;
            }
            in_signature = false;
        }
        payload.extend_from_slice(line);
    }
    payload.extend_from_slice(rest);
    SplitCommit {
        payload,
        signature: is_signed.then_some(signature),
    }
}

fn public_key_line(key: &PublicKey) -> String {
    let mut key = key.clone();
    key.set_comment("");
    key.to_string().trim_end().to_string()
}

fn verify_in(
    namespace: &str,
    public_key: &str,
    payload: &[u8],
    signature: &[u8],
) -> Result<(), VerifyError> {
    let key = PublicKey::from_openssh(public_key).map_err(|_invalid| VerifyError::Invalid)?;
    let sig = SshSig::from_pem(signature).map_err(|_invalid| VerifyError::Invalid)?;
    if sig.public_key() != key.key_data() {
        return Err(VerifyError::OtherKey);
    }
    key.verify(namespace, payload, &sig)
        .map_err(|_invalid| VerifyError::Invalid)
}

/// Checks the Forgejo domain, which is the SSHSIG namespace of a key verification: a hostname
/// (letters, digits, `.` and `-`), and never [`GIT_NAMESPACE`], so that a signature over a token
/// can never pass as a commit signature.
fn check_forgejo_domain(domain: &str) -> Result<(), SigningKeyError> {
    let reason = if domain.is_empty() {
        "The Forgejo domain is empty".to_string()
    } else if domain.eq_ignore_ascii_case(GIT_NAMESPACE) {
        "The Forgejo domain cannot be `git`".to_string()
    } else if domain.len() > MAX_DOMAIN_CHARS {
        format!("The Forgejo domain has more than {MAX_DOMAIN_CHARS} characters")
    } else if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        "The Forgejo domain may only have letters, digits, `.` and `-`".to_string()
    } else {
        return Ok(());
    };
    Err(SigningKeyError::InvalidDomain(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\nauthor A <a@example.org> 1 +0000\ncommitter A <a@example.org> 1 +0000\n";

    #[test]
    fn a_signed_commit_splits_into_the_signed_payload_and_the_signature() {
        let key = SigningKey::generate().unwrap();
        let unsigned = format!("{COMMIT}\nFix login\n");
        let signature = key.sign(unsigned.as_bytes()).unwrap();
        let armored: Vec<String> = signature.lines().map(|line| format!(" {line}")).collect();
        let signed = format!(
            "{COMMIT}gpgsig {}\n\nFix login\n",
            armored.join("\n").trim_start()
        );

        let split = split_commit(signed.as_bytes());

        assert_eq!(split.payload, unsigned.as_bytes());
        let found = String::from_utf8(split.signature.unwrap()).unwrap();
        assert_eq!(found.trim_end(), signature.trim_end());
        verify(&key.public_key(), &split.payload, found.as_bytes()).unwrap();
    }

    #[test]
    fn an_unsigned_commit_has_no_signature_and_a_message_that_mimics_the_header_is_kept() {
        let raw = format!("{COMMIT}\ngpgsig fake\n gpgsig more\n");

        let split = split_commit(raw.as_bytes());

        assert_eq!(split.payload, raw.as_bytes());
        assert_eq!(split.signature, None);
    }

    #[test]
    fn a_forgejo_token_signature_verifies_in_the_domain_namespace_and_never_as_a_commit_signature()
    {
        let key = SigningKey::generate().unwrap();
        let domain = "forgejo.example.org";

        let signature = key.sign_forgejo_token(domain, "token-1").unwrap();

        verify_forgejo_token(&key.public_key(), domain, "token-1", signature.as_bytes()).unwrap();
        assert!(
            verify_forgejo_token(
                &key.public_key(),
                "other.example.org",
                "token-1",
                signature.as_bytes()
            )
            .is_err()
        );
        assert_eq!(
            verify(&key.public_key(), b"token-1", signature.as_bytes()),
            Err(VerifyError::Invalid)
        );
    }

    #[test]
    fn a_forgejo_domain_that_is_empty_git_or_no_hostname_signs_nothing() {
        let key = SigningKey::generate().unwrap();
        let too_long = format!("{}.org", "a".repeat(250));

        for domain in [
            "",
            "git",
            "GIT",
            "forgejo_http",
            "a b",
            "host/path",
            "host:3000",
            &too_long,
        ] {
            let error = key.sign_forgejo_token(domain, "token-1").unwrap_err();
            assert!(
                matches!(error, SigningKeyError::InvalidDomain(_)),
                "{domain:?}: {error}"
            );
        }
        key.sign_forgejo_token("forgejo-http.forgejo.svc", "token-1")
            .unwrap();
    }

    #[test]
    fn generated_key_round_trips_through_openssh_and_signs() {
        let key = SigningKey::generate().unwrap();
        let parsed = SigningKey::from_openssh(key.private_openssh()).unwrap();
        assert_eq!(parsed.public_key(), key.public_key());
        assert!(key.public_key().starts_with("ssh-ed25519 AAAA"));
        assert_eq!(key.public_key().matches(' ').count(), 1, "no comment");
        assert!(key.fingerprint().starts_with("SHA256:"));
        assert_eq!(
            fingerprint_of(&key.public_key()).unwrap(),
            key.fingerprint()
        );

        let signature = key.sign(b"payload").unwrap();
        assert!(signature.starts_with("-----BEGIN SSH SIGNATURE-----"));
        verify(&key.public_key(), b"payload", signature.as_bytes()).unwrap();
    }

    #[test]
    fn verify_names_the_reason() {
        let key = SigningKey::generate().unwrap();
        let other = SigningKey::generate().unwrap();
        let signature = key.sign(b"payload").unwrap();
        assert_eq!(
            verify(&other.public_key(), b"payload", signature.as_bytes()),
            Err(VerifyError::OtherKey)
        );
        assert_eq!(
            verify(&key.public_key(), b"other payload", signature.as_bytes()),
            Err(VerifyError::Invalid)
        );
        assert_eq!(
            verify(&key.public_key(), b"payload", b"garbage"),
            Err(VerifyError::Invalid)
        );
    }

    #[test]
    fn debug_output_has_no_private_key() {
        let key = SigningKey::generate().unwrap();
        let debug = format!("{key:?}");
        assert!(!debug.contains("PRIVATE"));
        assert!(debug.contains("SHA256:"));
    }

    #[test]
    fn rejects_a_key_that_is_not_ed25519_openssh() {
        let err = SigningKey::from_openssh(&SecretString::from("nope")).unwrap_err();
        assert!(matches!(err, SigningKeyError::InvalidKey(_)));
    }
}
