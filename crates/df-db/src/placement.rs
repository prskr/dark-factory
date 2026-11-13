//! Placing a new task on an existing branch: the open pull request it attaches to
//! (`task-branches`, "Pull request attached at creation"; design D3).

use df_core::{ExistingBranch, ForgeClient, IntegrationError, ProjectId};

use crate::tasks::{AttachedPr, TaskPlacement};

/// Why a branch can't be placed. The messages are shown to the board member or the agent.
#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
    #[error("looking up the pull requests of `{branch}` failed: {source}")]
    Forge {
        branch: String,
        source: IntegrationError,
    },
    #[error(
        "`{branch}` has several open pull requests ({numbers}); the factory can't tell which one to attach"
    )]
    SeveralPullRequests { branch: String, numbers: String },
    #[error("the pull request number {0} is out of range")]
    NumberOutOfRange(u64),
}

/// The placement of a task on `branch`: attached to its one open pull request, or without one.
/// The forge is asked before any transaction starts, so no lock is held across the call.
pub async fn place_on_branch(
    forge: &dyn ForgeClient,
    project: &ProjectId,
    branch: ExistingBranch,
) -> Result<TaskPlacement, PlacementError> {
    let open = forge
        .list_open_pull_requests(project, branch.as_str())
        .await
        .map_err(|source| PlacementError::Forge {
            branch: branch.to_string(),
            source,
        })?;
    let attached_pr = match open.as_slice() {
        [] => None,
        [pr] => Some(AttachedPr {
            number: i64::try_from(pr.id.number)
                .map_err(|_range| PlacementError::NumberOutOfRange(pr.id.number))?,
            url: pr.url.clone(),
            base: pr.base.clone(),
        }),
        several => {
            let numbers: Vec<_> = several
                .iter()
                .map(|pr| format!("#{}", pr.id.number))
                .collect();
            return Err(PlacementError::SeveralPullRequests {
                branch: branch.to_string(),
                numbers: numbers.join(", "),
            });
        }
    };
    Ok(TaskPlacement {
        branch: Some(branch),
        attached_pr,
    })
}
