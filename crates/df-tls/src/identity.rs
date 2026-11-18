//! Extraction of the sandbox pod identity from a client certificate.

use thiserror::Error;
use x509_parser::extensions::GeneralName;
use x509_parser::parse_x509_certificate;

const SPIFFE_PREFIX: &str = "spiffe://dark-factory/ns/";

/// Why a client certificate does not identify a sandbox pod.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityError {
    #[error("client certificate is not parsable")]
    Unparsable,
    #[error("client certificate has no `spiffe://dark-factory/ns/<ns>/pod/<name>/<uid>` URI SAN")]
    NoPodSan,
}

/// Returns the pod URI SAN of the leaf certificate `der`.
///
/// The SAN must have the shape `spiffe://dark-factory/ns/<ns>/pod/<name>/<uid>`; whether the
/// pod exists and belongs to a run is decided by `SandboxProvider::resolve_identity`.
pub fn pod_uri_san(der: &[u8]) -> Result<String, IdentityError> {
    let (_, cert) =
        parse_x509_certificate(der).map_err(|_parse_error| IdentityError::Unparsable)?;
    let san = cert
        .subject_alternative_name()
        .map_err(|_parse_error| IdentityError::Unparsable)?
        .ok_or(IdentityError::NoPodSan)?;
    san.value
        .general_names
        .iter()
        .find_map(|name| match name {
            GeneralName::URI(uri) if is_pod_san(uri) => Some((*uri).to_string()),
            _ => None,
        })
        .ok_or(IdentityError::NoPodSan)
}

fn is_pod_san(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix(SPIFFE_PREFIX) else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    matches!(parts.as_slice(), [ns, "pod", name, uid] if ![ns, name, uid].iter().any(|p| p.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestPki;

    #[test]
    fn extracts_pod_san_from_client_certificate() {
        let pki = TestPki::new().unwrap();
        let client = pki
            .pod_client("spiffe://dark-factory/ns/sandboxes/pod/run-1/uid-1")
            .unwrap();
        assert_eq!(
            pod_uri_san(&client.cert_der).unwrap(),
            "spiffe://dark-factory/ns/sandboxes/pod/run-1/uid-1"
        );
    }

    #[test]
    fn rejects_certificates_without_a_pod_san() {
        let pki = TestPki::new().unwrap();
        for san in [
            "spiffe://other/ns/a/pod/b/c",
            "spiffe://dark-factory/ns/a/pod/b",
            "spiffe://dark-factory/ns/a/pod//c",
        ] {
            let client = pki.pod_client(san).unwrap();
            assert_eq!(
                pod_uri_san(&client.cert_der),
                Err(IdentityError::NoPodSan),
                "{san}"
            );
        }
        assert_eq!(pod_uri_san(b"garbage"), Err(IdentityError::Unparsable));
    }
}
