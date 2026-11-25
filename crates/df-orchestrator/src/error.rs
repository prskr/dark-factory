//! Error type shared by the modules of this crate.

use df_core::{IntegrationError, ParseEnumError, ProviderError, SecretError};
use df_db::DbError;
use thiserror::Error;

/// Failure of an orchestration step: scheduling, run end, PR lifecycle, webhooks, retention.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Volume(#[from] df_db::volumes::VolumeError),
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// The signing key of the run's persona could not be created.
    #[error("signing key: {0}")]
    SigningKey(#[from] df_db::persona_keys::KeyError),
    #[error(transparent)]
    Integration(#[from] IntegrationError),
    /// The forge refused to create the branch of a base task; the run fails with this reason.
    #[error("base branch: {0}")]
    BaseBranch(IntegrationError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    /// A stored value is not a known enum value.
    #[error(transparent)]
    Parse(#[from] ParseEnumError),
    /// A row violates an invariant (for example a missing relation).
    #[error("invalid data: {0}")]
    InvalidData(String),
}
