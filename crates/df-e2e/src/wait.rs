//! Waiting for a condition in the cluster.

use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use df_core::{StatusGroup, TaskStatus};

/// Cold start of a sandbox plus a run of the fake agent (design: 5 minutes per run).
pub const RUN_TIMEOUT: Duration = Duration::from_secs(300);

/// A page that waits for a state the database already shows (a card, a status chip, a delivery
/// that left): the UI follows within seconds, a longer wait only hides a failure.
pub const UI_TIMEOUT: Duration = Duration::from_secs(30);

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A task ended in a final status other than the awaited one: no later check can succeed.
#[derive(Debug)]
pub struct EndedOtherwise {
    found: TaskStatus,
    awaited: TaskStatus,
}

impl std::fmt::Display for EndedOtherwise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the task ended as `{}`, not `{}`",
            self.found, self.awaited
        )
    }
}

impl std::error::Error for EndedOtherwise {}

/// Calls `check` until it returns `Some`, or fails naming `what` after `timeout`. An error of
/// `check` is kept as the failure reason of the last attempt; an [`EndedOtherwise`] ends the wait
/// at once.
pub async fn eventually<T, F, Fut>(what: &str, timeout: Duration, mut check: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        let last = match check().await {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => "not yet".to_owned(),
            Err(e) if e.downcast_ref::<EndedOtherwise>().is_some() => {
                return Err(e.context(format!("waiting for {what}")));
            }
            Err(e) => format!("{e:#}"),
        };
        if Instant::now() >= deadline {
            bail!("timed out after {timeout:?} waiting for {what} (last check: {last})");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// One check of a wait for `awaited`: `Some` once the task has it, `None` while it is on its way,
/// an error naming `found` when the task ended in another final status (a wait for `waiting` of a
/// task that `failed` would otherwise run into the timeout).
pub fn status_reached(found: TaskStatus, awaited: TaskStatus) -> Result<Option<()>> {
    if found == awaited {
        return Ok(Some(()));
    }
    if found.group() == StatusGroup::Done {
        return Err(EndedOtherwise { found, awaited }.into());
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_on_its_way_is_not_yet_reached() {
        let reached = status_reached(TaskStatus::Running, TaskStatus::Waiting).unwrap();

        assert_eq!(reached, None);
    }

    #[test]
    fn the_awaited_status_is_reached() {
        let reached = status_reached(TaskStatus::Waiting, TaskStatus::Waiting).unwrap();

        assert_eq!(reached, Some(()));
    }

    #[test]
    fn a_final_status_other_than_the_awaited_one_fails_naming_it() {
        let error =
            status_reached(TaskStatus::Failed, TaskStatus::Waiting).expect_err("failed is final");

        assert!(error.to_string().contains("`failed`"), "{error}");
        assert!(error.to_string().contains("`waiting`"), "{error}");
    }

    #[tokio::test]
    async fn a_wait_stops_at_once_when_the_task_ended_in_another_final_status() {
        let wait = eventually("waiting", RUN_TIMEOUT, || async {
            status_reached(TaskStatus::Failed, TaskStatus::Waiting)
        });

        let error = tokio::time::timeout(POLL_INTERVAL / 2, wait)
            .await
            .expect("the wait does not poll again")
            .expect_err("failed is final");

        assert!(format!("{error:#}").contains("`failed`"), "{error:#}");
    }

    #[test]
    fn an_awaited_final_status_is_reached() {
        let reached = status_reached(TaskStatus::Cancelled, TaskStatus::Cancelled).unwrap();

        assert_eq!(reached, Some(()));
    }
}
