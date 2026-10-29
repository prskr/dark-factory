//! Retry policy of the contract: `rate-limited` (honouring `retry-after`, capped) and `transport`
//! errors are retried, everything else is not.

use std::time::Duration;

use df_core::IntegrationError;

const MAX_RETRIES: usize = 3;
/// Longest wait taken from a `retry-after` hint.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(30);
/// Wait for rate limits without a hint and between transport retries.
const DEFAULT_BACKOFF: Duration = Duration::from_secs(1);

pub(crate) async fn with_retries<T, F, Fut>(mut call: F) -> Result<T, IntegrationError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, IntegrationError>>,
{
    let mut retries = 0;
    loop {
        let wait = match call().await {
            Err(IntegrationError::RateLimited(limit)) if retries < MAX_RETRIES => limit
                .retry_after_seconds
                .map_or(DEFAULT_BACKOFF, |s| Duration::from_secs(s.into()))
                .min(RETRY_AFTER_CAP),
            Err(IntegrationError::Transport(_)) if retries < MAX_RETRIES => DEFAULT_BACKOFF,
            result => return result,
        };
        retries += 1;
        tracing::warn!(
            retries,
            wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
            "retrying plugin call"
        );
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use df_core::RateLimit;
    use tokio::time::Instant;

    use super::*;

    fn rate_limited(seconds: Option<u32>) -> IntegrationError {
        IntegrationError::RateLimited(RateLimit {
            retry_after_seconds: seconds,
            message: "slow down".into(),
        })
    }

    #[tokio::test(start_paused = true)]
    async fn waits_for_retry_after_then_succeeds() {
        let calls = Cell::new(0);
        let start = Instant::now();
        let result = with_retries(|| {
            calls.set(calls.get() + 1);
            let first = calls.get() == 1;
            std::future::ready(if first {
                Err(rate_limited(Some(2)))
            } else {
                Ok(7)
            })
        })
        .await;
        assert_eq!(result, Ok(7));
        assert_eq!(calls.get(), 2);
        assert_eq!(start.elapsed(), Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_three_retries() {
        let calls = Cell::new(0);
        let result: Result<(), _> = with_retries(|| {
            calls.set(calls.get() + 1);
            async { Err(IntegrationError::Transport("down".into())) }
        })
        .await;
        assert!(matches!(result, Err(IntegrationError::Transport(_))));
        assert_eq!(calls.get(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn caps_the_retry_after_hint() {
        let start = Instant::now();
        let calls = Cell::new(0);
        with_retries(|| {
            calls.set(calls.get() + 1);
            let first = calls.get() == 1;
            std::future::ready(if first {
                Err(rate_limited(Some(3600)))
            } else {
                Ok(())
            })
        })
        .await
        .ok();
        assert_eq!(start.elapsed(), RETRY_AFTER_CAP);
    }

    #[tokio::test(start_paused = true)]
    async fn other_errors_are_not_retried() {
        let calls = Cell::new(0);
        let result: Result<(), _> = with_retries(|| {
            calls.set(calls.get() + 1);
            async { Err(IntegrationError::Conflict("dup".into())) }
        })
        .await;
        assert!(matches!(result, Err(IntegrationError::Conflict(_))));
        assert_eq!(calls.get(), 1);
    }
}
