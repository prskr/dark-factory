//! Mutual TLS (rustls/ring) with PEM files that are reloaded when cert-manager rotates them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;
use tokio_stream::wrappers::ReceiverStream;

/// Time a client has to complete the TLS handshake.
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections whose handshake completed and wait for the HTTP/2 server to pick them up.
const ACCEPTED_BACKLOG: usize = 16;
/// Handshakes in progress at the same time; further connections wait in the accept queue.
const MAX_HANDSHAKES: usize = 256;
/// How often connections look at the PEM files for changes.
const TLS_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Failure to load or build the TLS material.
#[derive(Debug, Error)]
pub enum TlsError {
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid TLS material: {0}")]
    Invalid(String),
}

/// The PEM files of a mutual-TLS endpoint: its certificate chain, its key and the CA bundle that
/// client certificates must chain to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub client_ca_file: PathBuf,
}

/// Serves TLS handshakes from the current contents of the PEM files: a connection that arrives
/// at least the configured check interval (10 s by default) after the last look compares the
/// files with what was loaded and switches to the new material if they changed. At most one
/// reload reads the files at a time; connections that arrive meanwhile use the installed
/// material. Material that does not parse is ignored (and logged) until it is fixed.
pub struct TlsReloader {
    paths: TlsPaths,
    check_every: Duration,
    current: Mutex<Loaded>,
}

impl TlsReloader {
    /// Loads the material; fails if it is missing or invalid, so a bad deployment stops at startup.
    pub async fn load(paths: &TlsPaths) -> Result<Self, TlsError> {
        let files = TlsFiles::read(paths).await?;
        let acceptor = files.acceptor()?;
        Ok(Self {
            paths: paths.clone(),
            check_every: TLS_CHECK_INTERVAL,
            current: Mutex::new(Loaded {
                files,
                acceptor,
                checked_at: Instant::now(),
                reloading: false,
            }),
        })
    }

    /// Looks at the files at most every `interval` (default: 10 s).
    #[must_use]
    pub fn check_every(mut self, interval: Duration) -> Self {
        self.check_every = interval;
        self
    }

    fn loaded(&self) -> std::sync::MutexGuard<'_, Loaded> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The acceptor for the next connection, after picking up changed files.
    async fn acceptor(&self) -> TlsAcceptor {
        let _reload = {
            let mut current = self.loaded();
            if current.reloading || current.checked_at.elapsed() < self.check_every {
                return current.acceptor.clone();
            }
            // Claimed before reading, so concurrent connections don't read the files as well.
            current.checked_at = Instant::now();
            current.reloading = true;
            ReloadClaim(self)
        };
        let latest = TlsFiles::read(&self.paths).await;
        let mut current = self.loaded();
        match latest {
            Ok(files) if files != current.files => match files.acceptor() {
                Ok(acceptor) => {
                    tracing::info!("mTLS material reloaded");
                    current.files = files;
                    current.acceptor = acceptor;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "keeping the previous mTLS material");
                }
            },
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "keeping the previous mTLS material"),
        }
        current.acceptor.clone()
    }
}

/// Connections of `listener` with completed mutual TLS handshakes, as input of a tonic server.
/// Accepting stops when the stream is dropped.
pub fn mtls_incoming(listener: TcpListener, tls: TlsReloader) -> MtlsIncoming {
    incoming_with_limits(listener, tls, ACCEPTED_BACKLOG, MAX_HANDSHAKES)
}

/// [`mtls_incoming`] with the queue length and the number of connections in flight (handshaking
/// or waiting to be queued) as parameters, so tests can reach the limits.
fn incoming_with_limits(
    listener: TcpListener,
    tls: TlsReloader,
    backlog: usize,
    max_handshakes: usize,
) -> MtlsIncoming {
    let (accepted, incoming) = mpsc::channel(backlog);
    MtlsIncoming {
        incoming: ReceiverStream::new(incoming),
        acceptor: tokio::spawn(accept_loop(
            listener,
            Arc::new(tls),
            accepted,
            max_handshakes,
        )),
    }
}

/// See [`mtls_incoming`].
pub struct MtlsIncoming {
    incoming: ReceiverStream<Result<TlsStream<TcpStream>, std::io::Error>>,
    acceptor: tokio::task::JoinHandle<()>,
}

impl futures::Stream for MtlsIncoming {
    type Item = Result<TlsStream<TcpStream>, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.incoming).poll_next(cx)
    }
}

impl Drop for MtlsIncoming {
    fn drop(&mut self) {
        self.acceptor.abort();
    }
}

/// Raw contents of the three PEM files.
#[derive(PartialEq, Eq)]
struct TlsFiles {
    cert: Vec<u8>,
    key: Vec<u8>,
    client_ca: Vec<u8>,
}

impl TlsFiles {
    async fn read(paths: &TlsPaths) -> Result<Self, TlsError> {
        async fn read(path: &Path) -> Result<Vec<u8>, TlsError> {
            tokio::fs::read(path)
                .await
                .map_err(|source| TlsError::Read {
                    path: path.display().to_string(),
                    source,
                })
        }
        Ok(Self {
            cert: read(&paths.cert_file).await?,
            key: read(&paths.key_file).await?,
            client_ca: read(&paths.client_ca_file).await?,
        })
    }

    /// A TLS acceptor that requires client certificates chaining to the CA bundle.
    fn acceptor(&self) -> Result<TlsAcceptor, TlsError> {
        let invalid =
            |what: &str, e: &dyn std::fmt::Display| TlsError::Invalid(format!("{what}: {e}"));
        let certs = CertificateDer::pem_slice_iter(&self.cert)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid("server certificate", &e))?;
        let key =
            PrivateKeyDer::from_pem_slice(&self.key).map_err(|e| invalid("server key", &e))?;
        let mut roots = RootCertStore::empty();
        for ca in CertificateDer::pem_slice_iter(&self.client_ca) {
            let ca = ca.map_err(|e| invalid("client CA", &e))?;
            roots.add(ca).map_err(|e| invalid("client CA", &e))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier =
            WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .map_err(|e| invalid("client CA", &e))?;
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| invalid("protocol versions", &e))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .map_err(|e| invalid("server certificate and key", &e))?;
        config.alpn_protocols = vec![b"h2".to_vec()];
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

/// What the reloader serves: the files, the acceptor built from them and when they were last read.
struct Loaded {
    files: TlsFiles,
    acceptor: TlsAcceptor,
    checked_at: Instant,
    /// A reload is reading the files.
    reloading: bool,
}

/// Marks a reload as finished when dropped: after success, failure and cancellation alike.
struct ReloadClaim<'a>(&'a TlsReloader);

impl Drop for ReloadClaim<'_> {
    fn drop(&mut self) {
        self.0.loaded().reloading = false;
    }
}

/// Accepts TCP connections and completes their TLS handshakes concurrently, each bounded by
/// [`TLS_HANDSHAKE_TIMEOUT`]. Failed handshakes only concern the client and are not forwarded.
async fn accept_loop(
    listener: TcpListener,
    tls: Arc<TlsReloader>,
    accepted: mpsc::Sender<Result<TlsStream<TcpStream>, std::io::Error>>,
    max_handshakes: usize,
) {
    let mut handshakes = JoinSet::new();
    let slots = Arc::new(Semaphore::new(max_handshakes));
    loop {
        while handshakes.try_join_next().is_some() {}
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return;
        };
        let (tcp, remote) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "accept failed on an mTLS port; backing off");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let (tls, accepted) = (tls.clone(), accepted.clone());
        handshakes.spawn(async move {
            let acceptor = tls.acceptor().await;
            let handshake = tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await;
            // The slot is held until the connection is queued (or dropped), so connections
            // that wait for a slow consumer count against the limit.
            let _slot = slot;
            match handshake {
                Ok(Ok(stream)) => {
                    // The receiver is gone when the server stopped.
                    accepted.send(Ok(stream)).await.ok(); // receiver may be gone
                }
                Ok(Err(e)) => tracing::debug!(error = %e, %remote, "TLS handshake failed"),
                Err(_) => tracing::debug!(%remote, "TLS handshake timed out"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestPki;

    #[tokio::test]
    async fn pem_files_are_not_read_for_every_connection() {
        let dir = tempfile::tempdir().unwrap();
        let paths = TestPki::new()
            .unwrap()
            .write_server_tls(dir.path())
            .unwrap();
        let reloader = TlsReloader::load(&paths)
            .await
            .unwrap()
            .check_every(Duration::from_secs(3600));
        let loaded = |r: &TlsReloader| r.loaded().files.cert.clone();
        let before = loaded(&reloader);

        // cert-manager renewed the certificate.
        TestPki::new()
            .unwrap()
            .write_server_tls(dir.path())
            .unwrap();
        reloader.acceptor().await;
        assert_eq!(
            loaded(&reloader),
            before,
            "looked at the files again too soon"
        );

        let reloader = reloader.check_every(Duration::ZERO);
        reloader.acceptor().await;
        assert_ne!(loaded(&reloader), before);
    }

    #[tokio::test]
    async fn a_connection_during_a_reload_neither_reads_the_files_nor_waits() {
        let dir = tempfile::tempdir().unwrap();
        let paths = TestPki::new()
            .unwrap()
            .write_server_tls(dir.path())
            .unwrap();
        let reloader = TlsReloader::load(&paths)
            .await
            .unwrap()
            .check_every(Duration::ZERO);

        // The first poll starts the reload; reading the files is still pending.
        let mut first = Box::pin(reloader.acceptor());
        assert!(futures::poll!(first.as_mut()).is_pending());

        let mut second = Box::pin(reloader.acceptor());
        assert!(
            futures::poll!(second.as_mut()).is_ready(),
            "the second connection must use the installed acceptor at once"
        );

        // A cancelled reload releases its claim, so a later connection can retry.
        drop(first);
        let mut third = Box::pin(reloader.acceptor());
        assert!(futures::poll!(third.as_mut()).is_pending());
    }

    #[tokio::test]
    async fn connections_waiting_for_a_slow_consumer_count_against_the_handshake_limit() {
        use rustls::pki_types::ServerName;
        use rustls::pki_types::pem::PemObject;

        let dir = tempfile::tempdir().unwrap();
        let pki = TestPki::new().unwrap();
        let paths = pki.write_server_tls(dir.path()).unwrap();
        let client = pki
            .pod_client("spiffe://dark-factory/ns/sb/pod/run/uid")
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_slice(pki.ca_pem.as_bytes()).unwrap())
            .unwrap();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_client_auth_cert(
            vec![CertificateDer::from(client.cert_der.clone())],
            PrivateKeyDer::from_pem_slice(client.key_pem.as_bytes()).unwrap(),
        )
        .unwrap();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // The consumer never polls: one connection fits the queue, one waits for it while holding
        // a handshake slot, the next one takes the second slot and waits as well.
        let _incoming =
            incoming_with_limits(listener, TlsReloader::load(&paths).await.unwrap(), 1, 2);

        let (done, mut completed) = mpsc::unbounded_channel();
        for _ in 0..6 {
            let (connector, done) = (connector.clone(), done.clone());
            tokio::spawn(async move {
                let tcp = TcpStream::connect(addr).await.unwrap();
                let name = ServerName::try_from("localhost").unwrap();
                let stream = connector.connect(name, tcp).await.unwrap();
                // The client keeps the connection open until the test ends.
                done.send(stream).ok(); // receiver gone when the test ended
            });
        }
        let mut streams = Vec::new();
        for _ in 0..3 {
            streams.push(completed.recv().await.unwrap());
        }
        // Without the limit on waiting connections, the others would be served as well.
        let more = tokio::time::timeout(Duration::from_millis(300), completed.recv()).await;
        assert!(more.is_err(), "a fourth connection was served");
    }

    #[tokio::test]
    async fn material_that_does_not_parse_is_ignored_until_it_is_fixed() {
        let dir = tempfile::tempdir().unwrap();
        let pki = TestPki::new().unwrap();
        let paths = pki.write_server_tls(dir.path()).unwrap();
        let reloader = TlsReloader::load(&paths)
            .await
            .unwrap()
            .check_every(Duration::ZERO);
        let before = reloader.loaded().files.cert.clone();

        std::fs::write(&paths.cert_file, "not a certificate").unwrap();
        reloader.acceptor().await;
        assert_eq!(reloader.loaded().files.cert, before);

        let renewed = TestPki::new().unwrap();
        renewed.write_server_tls(dir.path()).unwrap();
        reloader.acceptor().await;
        assert_ne!(reloader.loaded().files.cert, before);
    }

    #[tokio::test]
    async fn loading_fails_for_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = TlsPaths {
            cert_file: dir.path().join("tls.crt"),
            key_file: dir.path().join("tls.key"),
            client_ca_file: dir.path().join("ca.crt"),
        };
        assert!(matches!(
            TlsReloader::load(&paths).await,
            Err(TlsError::Read { .. })
        ));
    }
}
