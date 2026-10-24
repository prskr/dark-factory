//! Authorization of mTLS peers by the DNS name in their certificate.
//!
//! The TLS layer only proves that a certificate chains to the instance CA, which also signs the
//! sandbox pods' certificates. The internal ports therefore additionally require the name of the
//! one peer that may call them.

use tonic::{Request, Status};
use x509_parser::extensions::GeneralName;
use x509_parser::parse_x509_certificate;

/// Whether the certificate `der` has `name` as DNS subject alternative name.
pub fn has_dns_san(der: &[u8], name: &str) -> bool {
    let Ok((_, cert)) = parse_x509_certificate(der) else {
        return false;
    };
    let Ok(Some(san)) = cert.subject_alternative_name() else {
        return false;
    };
    san.value.general_names.iter().any(
        |general| matches!(general, GeneralName::DNSName(dns) if dns.eq_ignore_ascii_case(name)),
    )
}

/// `Ok` if the peer of `request` presented a certificate with DNS name `name`.
pub fn require_dns_san<T>(request: &Request<T>, name: &str) -> Result<(), Status> {
    let certs = request
        .peer_certs()
        .ok_or_else(|| Status::unauthenticated("no client certificate"))?;
    match certs.first() {
        Some(leaf) if has_dns_san(leaf, name) => Ok(()),
        _ => Err(Status::permission_denied(
            "the client certificate is not accepted here",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_agent_channel::testing::TestPki;
    use rustls::pki_types::{CertificateDer, pem::PemObject};

    #[test]
    fn matches_the_dns_name_of_the_certificate_only() {
        let pki = TestPki::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let files = pki.write_peer_tls(dir.path(), "df-gateway").unwrap();
        let pem = std::fs::read(&files.cert_file).unwrap();
        let der = CertificateDer::pem_slice_iter(&pem)
            .next()
            .unwrap()
            .unwrap();
        assert!(has_dns_san(&der, "df-gateway"));
        assert!(has_dns_san(&der, "DF-Gateway"));
        assert!(!has_dns_san(&der, "df-control-plane"));
        assert!(!has_dns_san(b"garbage", "df-gateway"));

        // A sandbox pod's certificate has only a URI name.
        let pod = pki
            .pod_client("spiffe://dark-factory/ns/sb/pod/run/uid")
            .unwrap();
        assert!(!has_dns_san(&pod.cert_der, "df-gateway"));
    }

    #[test]
    fn a_request_without_certificate_is_unauthenticated() {
        let status = require_dns_san(&Request::new(()), "df-gateway").unwrap_err();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);
    }
}
