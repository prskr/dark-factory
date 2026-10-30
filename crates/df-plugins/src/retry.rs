//! Retry policy of the contract: `rate-limited` (honouring `retry-after` up to a cap) and
//! `transport` errors are retried, everything else is not.

use std::time::Duration;

use df_core::IntegrationError;

const MAX_RETRIES: usize = 3;
/// Longest `retry-after` hint the host waits for; a longer one returns the error at once.
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
            Err(IntegrationError::RateLimited(limit)) if retries < MAX_RETRIES => {
                let wait = limit
                    .retry_after_seconds
                    .map_or(DEFAULT_BACKOFF, |s| Duration::from_secs(s.into()));
                if wait > RETRY_AFTER_CAP {
                    return Err(IntegrationError::RateLimited(limit));
                }
                wait
            }
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
    async fn rate_limit_without_hint_waits_default_backoff_then_succeeds() {
        let calls = Cell::new(0);
        let start = Instant::now();
        let result = with_retries(|| {
            calls.set(calls.get() + 1);
            let first = calls.get() == 1;
            std::future::ready(if first {
                Err(rate_limited(None))
            } else {
                Ok(7)
            })
        })
        .await;
        assert_eq!(result, Ok(7));
        assert_eq!(calls.get(), 2);
        assert_eq!(start.elapsed(), DEFAULT_BACKOFF);
    }

    #[tokio::test(start_paused = true)]
    async fn persistent_rate_limit_gives_up_after_three_retries() {
        let calls = Cell::new(0);
        let start = Instant::now();
        let result: Result<(), _> = with_retries(|| {
            calls.set(calls.get() + 1);
            std::future::ready(Err(rate_limited(Some(1))))
        })
        .await;
        assert_eq!(result, Err(rate_limited(Some(1))));
        assert_eq!(calls.get(), 4);
        assert_eq!(start.elapsed(), Duration::from_secs(3));
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
    async fn retry_after_beyond_the_cap_is_returned_at_once() {
        let start = Instant::now();
        let calls = Cell::new(0);
        let result = with_retries(|| {
            calls.set(calls.get() + 1);
            let first = calls.get() == 1;
            std::future::ready(if first {
                Err(rate_limited(Some(600)))
            } else {
                Ok(())
            })
        })
        .await;
        assert_eq!(result, Err(rate_limited(Some(600))));
        assert_eq!(calls.get(), 1);
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_equal_to_the_cap_is_retried() {
        let start = Instant::now();
        let calls = Cell::new(0);
        let result = with_retries(|| {
            calls.set(calls.get() + 1);
            let first = calls.get() == 1;
            std::future::ready(if first {
                Err(rate_limited(Some(30)))
            } else {
                Ok(())
            })
        })
        .await;
        assert_eq!(result, Ok(()));
        assert_eq!(calls.get(), 2);
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
