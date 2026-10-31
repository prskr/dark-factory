//! Waiting for a condition in the cluster.

use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

/// Cold start of a sandbox plus a run of the fake agent (design: 5 minutes per run).
pub const RUN_TIMEOUT: Duration = Duration::from_secs(300);

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Calls `check` until it returns `Some`, or fails naming `what` after `timeout`. An error of
/// `check` is kept as the failure reason of the last attempt.
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
            Err(e) => format!("{e:#}"),
        };
        if Instant::now() >= deadline {
            bail!("timed out after {timeout:?} waiting for {what} (last check: {last})");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
