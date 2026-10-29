//! Reads the TLS `ClientHello` of a tunnel and extracts its SNI.

use std::io;

use tls_parser::{
    SNIType, TlsExtension, TlsMessage, TlsMessageHandshake, parse_tls_extensions,
    parse_tls_plaintext,
};
use tokio::io::{AsyncRead, AsyncReadExt};

const RECORD_HEADER: usize = 5;
/// Largest TLS record payload (RFC 8446 §5.1).
const MAX_RECORD: usize = 16 * 1024;
const SNI_HOST_NAME: SNIType = SNIType(0);

/// Reads the first TLS record from `stream`, the one that carries the `ClientHello`. The bytes are
/// returned unmodified so they can be forwarded.
pub(crate) async fn read_first_record<R: AsyncRead + Unpin>(stream: &mut R) -> io::Result<Vec<u8>> {
    let mut header = [0; RECORD_HEADER];
    stream.read_exact(&mut header).await?;
    let [content_type, _, _, len_high, len_low] = header;
    let len = usize::from(u16::from_be_bytes([len_high, len_low]));
    if content_type != 0x16 || len > MAX_RECORD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a TLS handshake record",
        ));
    }
    let mut payload = vec![0; len];
    stream.read_exact(&mut payload).await?;
    let mut record = header.to_vec();
    record.extend(payload);
    Ok(record)
}

/// The host name of the SNI extension of the `ClientHello` in `record`, lower case.
pub(crate) fn server_name(record: &[u8]) -> Option<String> {
    let (_, plaintext) = parse_tls_plaintext(record).ok()?;
    let extensions = plaintext.msg.iter().find_map(|msg| match msg {
        TlsMessage::Handshake(TlsMessageHandshake::ClientHello(hello)) => hello.ext,
        _ => None,
    })?;
    let (_, extensions) = parse_tls_extensions(extensions).ok()?;
    extensions.iter().find_map(|ext| match ext {
        TlsExtension::SNI(names) => names
            .iter()
            .find(|(kind, _)| *kind == SNI_HOST_NAME)
            .and_then(|(_, name)| std::str::from_utf8(name).ok())
            .map(df_core::host::normalize_host),
        _ => None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use super::*;

    /// The first flight of a real TLS client (rustls) connecting to `server_name`.
    pub(crate) fn client_hello(server_name: &str) -> Vec<u8> {
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(server_name.to_string()).unwrap();
        let mut conn = rustls::ClientConnection::new(Arc::new(config), name).unwrap();
        let mut hello = Vec::new();
        conn.write_tls(&mut hello).unwrap();
        hello
    }

    #[tokio::test]
    async fn extracts_the_sni_of_a_real_client_hello() {
        let hello = client_hello("API.Anthropic.com");
        let mut reader = &hello[..];
        let record = read_first_record(&mut reader).await.unwrap();
        assert_eq!(record, hello);
        assert_eq!(server_name(&record).as_deref(), Some("api.anthropic.com"));
    }

    #[tokio::test]
    async fn non_tls_data_is_rejected() {
        let mut reader = &b"GET / HTTP/1.1\r\n\r\n"[..];
        let err = read_first_record(&mut reader).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(server_name(b"GET / HTTP/1.1\r\n\r\n"), None);
    }

    #[test]
    fn a_client_hello_without_sni_has_no_server_name() {
        // rustls omits the extension for IP addresses.
        assert_eq!(server_name(&client_hello("192.0.2.1")), None);
    }
}
