//! Client side of the mTLS links between control plane and gateway pods.

use std::time::Duration;

use async_trait::async_trait;
use df_core::config::AgentTlsConfig;
use thiserror::Error;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

/// Time to establish a connection including the TLS handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Why a channel could not be opened.
#[derive(Debug, Error)]
pub enum LinkError {
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Transport(#[from] tonic::transport::Error),
}

/// Opens gRPC channels to `host:port` addresses.
#[async_trait]
pub trait GrpcConnector: Send + Sync {
    async fn connect(&self, authority: &str) -> Result<Channel, LinkError>;
}

/// Why a call to the control plane failed: worth another attempt, or refused for good.
#[derive(Debug, Error)]
pub enum CallError {
    #[error("{0}")]
    Transient(String),
    #[error("{0}")]
    Permanent(String),
}

impl From<tonic::Status> for CallError {
    fn from(status: tonic::Status) -> Self {
        let transient = matches!(
            status.code(),
            tonic::Code::Unavailable
                | tonic::Code::DeadlineExceeded
                | tonic::Code::ResourceExhausted
                | tonic::Code::Unknown
        );
        if transient {
            Self::Transient(status.to_string())
        } else {
            Self::Permanent(status.to_string())
        }
    }
}

/// mTLS with the instance's certificate, key and CA. The files are read on every connect, so a
/// certificate renewed by cert-manager is used by the next connection.
pub struct MtlsConnector {
    tls: AgentTlsConfig,
    /// Name the peer's certificate must carry.
    server_name: String,
}

impl MtlsConnector {
    pub fn new(tls: AgentTlsConfig, server_name: impl Into<String>) -> Self {
        Self {
            tls,
            server_name: server_name.into(),
        }
    }
}

async fn read(path: &std::path::Path) -> Result<Vec<u8>, LinkError> {
    tokio::fs::read(path)
        .await
        .map_err(|source| LinkError::Read {
            path: path.display().to_string(),
            source,
        })
}

#[async_trait]
impl GrpcConnector for MtlsConnector {
    async fn connect(&self, authority: &str) -> Result<Channel, LinkError> {
        let tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(read(&self.tls.client_ca_file).await?))
            .identity(Identity::from_pem(
                read(&self.tls.cert_file).await?,
                read(&self.tls.key_file).await?,
            ))
            .domain_name(self.server_name.clone());
        Ok(Endpoint::from_shared(format!("https://{authority}"))?
            .tls_config(tls)?
            .connect_timeout(CONNECT_TIMEOUT)
            .http2_keep_alive_interval(KEEPALIVE_INTERVAL)
            .keep_alive_timeout(KEEPALIVE_TIMEOUT)
            .keep_alive_while_idle(true)
            .connect()
            .await?)
    }
}

/// Unencrypted channels, for tests of components that do not care about TLS.
#[cfg(test)]
pub(crate) struct PlainConnector;

#[cfg(test)]
#[async_trait]
impl GrpcConnector for PlainConnector {
    async fn connect(&self, authority: &str) -> Result<Channel, LinkError> {
        Ok(Endpoint::from_shared(format!("http://{authority}"))?
            .connect()
            .await?)
    }
}
