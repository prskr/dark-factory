//! Ending a run on behalf of the channel: the transition itself is `df_db::runs::end_run`; this
//! module adds the channel configuration and the sandbox teardown (design D2, D4).

use chrono::{DateTime, Utc};
use df_core::RunId;
use df_core::run_transition::RunEnd;
use df_db::run_events::RunEnv;
use df_db::runs::{self, EndedRun};
use sqlx::{PgPool, Postgres, Transaction};

use crate::error::ChannelError;
use crate::{ChannelConfig, ChannelContext};

/// Ends a non-terminal run and sets the task status; see [`df_db::runs::end_run`]. The caller
/// tears the sandbox down.
pub async fn end_run(
    pool: &PgPool,
    end: &RunEnd,
    config: &ChannelConfig,
) -> Result<Option<EndedRun>, ChannelError> {
    Ok(runs::end_run(pool, end, &run_env(config)).await?)
}

/// The sinks and the resume policy of the configuration.
fn run_env(config: &ChannelConfig) -> RunEnv<'_> {
    RunEnv::new(
        &config.events,
        config.runs.max_auto_resumes,
        config.scheduling.resume_backoff(),
    )
}

/// Transactional variant of [`end_run`].
pub async fn end_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    end: &RunEnd,
    config: &ChannelConfig,
) -> Result<Option<EndedRun>, ChannelError> {
    Ok(runs::end_run_tx(tx, end, &run_env(config)).await?)
}

/// Ends the run and, if this call ended it, deletes its sandbox. The task volume is kept.
pub async fn finish_run(
    ctx: &ChannelContext,
    end: &RunEnd,
) -> Result<Option<EndedRun>, ChannelError> {
    let ended = end_run(&ctx.pool, end, &ctx.config).await?;
    if ended.is_some() {
        teardown(ctx, end.run_id).await;
    }
    Ok(ended)
}

/// Ends the run if it is overdue at `now` on its locked row and, if this call ended it, deletes
/// its sandbox; see [`df_db::runs::end_overdue`].
pub async fn finish_overdue(
    ctx: &ChannelContext,
    run_id: RunId,
    now: DateTime<Utc>,
) -> Result<Option<EndedRun>, ChannelError> {
    let ended = runs::end_overdue(
        &ctx.pool,
        run_id,
        now,
        &ctx.config.runs,
        &run_env(&ctx.config),
    )
    .await?;
    if ended.is_some() {
        teardown(ctx, run_id).await;
    }
    Ok(ended)
}

/// Deletes the sandbox of `run_id`; a failure is logged (the provider reports leftovers as `Gone`).
pub async fn teardown(ctx: &ChannelContext, run_id: RunId) {
    if let Err(e) = ctx.provider.teardown(run_id).await {
        tracing::warn!(error = %e, run_id = %run_id, "sandbox teardown failed");
    }
}
