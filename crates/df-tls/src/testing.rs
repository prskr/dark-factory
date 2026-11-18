//! A private PKI for tests. Enabled by the `test-util` feature.

use rcgen::string::Ia5String;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};

use crate::TlsPaths;

/// Server name of the test server certificate.
pub const SERVER_NAME: &str = "localhost";

/// `Result` with a boxed error, for fixtures.
pub type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A client certificate and its key.
pub struct ClientCert {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: Vec<u8>,
}

/// An instance CA with a server certificate for [`SERVER_NAME`].
pub struct TestPki {
    pub ca_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
    issuer: Issuer<'static, KeyPair>,
}

impl TestPki {
    pub fn new() -> FixtureResult<Self> {
        let ca_params = ca_params("Dark Factory Test CA");
        let ca_key = KeyPair::generate()?;
        let ca_cert = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params, ca_key);

        let mut server = CertificateParams::new(vec![SERVER_NAME.to_string()])?;
        server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate()?;
        let server_cert = server.signed_by(&server_key, &issuer)?;

        Ok(Self {
            ca_pem: ca_cert.pem(),
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
            issuer,
        })
    }

    /// A client certificate for a sandbox pod, signed by this CA.
    pub fn pod_client(&self, uri_san: &str) -> FixtureResult<ClientCert> {
        client_cert(&self.issuer, uri_san)
    }

    /// A client certificate signed by an unrelated CA.
    pub fn rogue_client(uri_san: &str) -> FixtureResult<ClientCert> {
        let params = ca_params("Rogue CA");
        let key = KeyPair::generate()?;
        params.self_signed(&key)?;
        client_cert(&Issuer::new(params, key), uri_san)
    }

    /// Like [`Self::write_server_tls`], but the certificate is for `dns_name` and also valid for
    /// client authentication: the identity of a peer that is both client and server (gateway,
    /// control plane).
    pub fn write_peer_tls(&self, dir: &std::path::Path, dns_name: &str) -> FixtureResult<TlsPaths> {
        let mut params = CertificateParams::new(vec![dns_name.to_string()])?;
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &self.issuer)?;
        let paths = paths_in(dir);
        std::fs::write(&paths.cert_file, cert.pem())?;
        std::fs::write(&paths.key_file, key.serialize_pem())?;
        std::fs::write(&paths.client_ca_file, &self.ca_pem)?;
        Ok(paths)
    }

    /// Writes `tls.crt`, `tls.key` (server) and `ca.crt` into `dir` and returns their paths.
    pub fn write_server_tls(&self, dir: &std::path::Path) -> std::io::Result<TlsPaths> {
        let paths = paths_in(dir);
        std::fs::write(&paths.cert_file, &self.server_cert_pem)?;
        std::fs::write(&paths.key_file, &self.server_key_pem)?;
        std::fs::write(&paths.client_ca_file, &self.ca_pem)?;
        Ok(paths)
    }
}

fn paths_in(dir: &std::path::Path) -> TlsPaths {
    TlsPaths {
        cert_file: dir.join("tls.crt"),
        key_file: dir.join("tls.key"),
        client_ca_file: dir.join("ca.crt"),
    }
}

fn ca_params(common_name: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, common_name);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params
}

fn client_cert(issuer: &Issuer<'_, KeyPair>, uri_san: &str) -> FixtureResult<ClientCert> {
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "sandbox");
    params.subject_alt_names = vec![SanType::URI(Ia5String::try_from(uri_san.to_string())?)];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, issuer)?;
    Ok(ClientCert {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
        cert_der: cert.der().to_vec(),
    })
}
