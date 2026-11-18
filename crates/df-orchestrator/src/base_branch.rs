//! The branch of a base task, created when a run of a task that builds on it starts (`task-tree`
//! "Base branch created on start").

use df_core::{IntegrationError, TaskStatus};
use df_db::get_task;

use crate::OrchestratorContext;
use crate::error::OrchestratorError;
use crate::run_context::RunContext;

/// Creates the base task's branch from that task's own base branch at every start of a task with
/// a base task that is not Done (a Done base task is replaced by its own base, which exists). A
/// branch that exists already is fine; any other forge error is
/// [`OrchestratorError::BaseBranch`]. Every start asks, so a run that failed at the forge does not
/// leave the next one without the branch.
pub async fn ensure_base_branch(
    ctx: &OrchestratorContext,
    rc: &RunContext,
) -> Result<(), OrchestratorError> {
    let Some(base_id) = rc.task.base_task_id else {
        return Ok(());
    };
    let Some(base) = get_task(&ctx.pool, base_id).await? else {
        return Ok(());
    };
    if base.status == TaskStatus::Completed {
        return Ok(());
    }
    let from = df_db::base_branch(&ctx.pool, base_id).await?;
    match ctx
        .forge
        .create_branch(&rc.project.id, &base.branch, &from)
        .await
    {
        Ok(()) | Err(IntegrationError::Conflict(_)) => Ok(()),
        Err(error) => Err(OrchestratorError::BaseBranch(error)),
    }
}
