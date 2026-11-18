//! Error type shared by the modules of this crate.

use df_core::ParseEnumError;
use df_db::DbError;
use df_orchestrator::OrchestratorError;
use thiserror::Error;

/// Failure inside the agent channel; converted to a gRPC status at the stream boundary.
#[derive(Debug, Error)]
pub enum ChannelError {
    /// Every failure the orchestration layer knows: database, serialization, invalid data, ...
    #[error(transparent)]
    Orchestrator(#[from] OrchestratorError),
    #[error(transparent)]
    Request(#[from] df_db::requests::RequestError),
}

// These kinds have one variant, in [`OrchestratorError`]; `?` routes them there. No blanket
// impl over `Into<OrchestratorError>`: it conflicts with core's `From<T> for T`.

impl From<DbError> for ChannelError {
    fn from(err: DbError) -> Self {
        Self::Orchestrator(err.into())
    }
}

impl From<sqlx::Error> for ChannelError {
    fn from(err: sqlx::Error) -> Self {
        Self::Orchestrator(err.into())
    }
}

impl From<serde_json::Error> for ChannelError {
    fn from(err: serde_json::Error) -> Self {
        Self::Orchestrator(err.into())
    }
}

impl From<ParseEnumError> for ChannelError {
    fn from(err: ParseEnumError) -> Self {
        Self::Orchestrator(err.into())
    }
}

impl From<ChannelError> for tonic::Status {
    fn from(err: ChannelError) -> Self {
        tracing::error!(error = %err, "agent channel internal error");
        tonic::Status::internal("internal error")
    }
}

#[cfg(test)]
mod tests {
    use df_core::{IntegrationError, ProviderError};

    use super::*;

    fn code(err: impl Into<ChannelError>) -> tonic::Code {
        tonic::Status::from(err.into()).code()
    }

    #[test]
    fn every_kind_of_failure_reaches_the_client_as_an_opaque_internal_error() {
        assert_eq!(
            code(DbError::InvalidData("x".into())),
            tonic::Code::Internal
        );
        assert_eq!(
            code(OrchestratorError::Db(DbError::InvalidData("x".into()))),
            tonic::Code::Internal
        );
        assert_eq!(code(sqlx::Error::PoolClosed), tonic::Code::Internal);
        assert_eq!(
            code(OrchestratorError::Provider(ProviderError::NotFound(
                "x".into()
            ))),
            tonic::Code::Internal
        );
        assert_eq!(
            code(OrchestratorError::Integration(
                IntegrationError::Unauthorized("x".into())
            )),
            tonic::Code::Internal
        );
        assert_eq!(
            code(OrchestratorError::InvalidData("x".into())),
            tonic::Code::Internal
        );
        assert_eq!(
            code(serde_json::from_str::<u8>("x").unwrap_err()),
            tonic::Code::Internal
        );
        let status = tonic::Status::from(ChannelError::from(OrchestratorError::InvalidData(
            "secret detail".into(),
        )));
        assert_eq!(status.message(), "internal error");
    }
}
