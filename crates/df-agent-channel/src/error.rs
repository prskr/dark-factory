//! Error type shared by the modules of this crate.

use df_core::{IntegrationError, ParseEnumError, ProviderError, SecretError};
use df_db::DbError;
use thiserror::Error;

/// Failure inside the agent channel; converted to a gRPC status at the stream boundary.
#[derive(Debug, Error)]
pub enum ChannelError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Request(#[from] df_db::requests::RequestError),
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error(transparent)]
    Integration(#[from] IntegrationError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    /// A stored value is not a known enum value.
    #[error(transparent)]
    Parse(#[from] ParseEnumError),
    /// A row or message violates an invariant (for example a missing relation).
    #[error("invalid data: {0}")]
    InvalidData(String),
}

impl From<ChannelError> for tonic::Status {
    fn from(err: ChannelError) -> Self {
        tracing::error!(error = %err, "agent channel internal error");
        tonic::Status::internal("internal error")
    }
}
