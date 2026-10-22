//! Maps Forgejo HTTP error responses to the `error` variant of the contract.

use crate::darkfactory::integrations::types::{Error, RateLimit, UpstreamError};

/// `status` is a non-2xx response; `retry_after` is the parsed `Retry-After` header in seconds.
pub fn from_status(status: u16, retry_after: Option<u32>, body: &[u8]) -> Error {
    let message = message(body);
    match status {
        404 => Error::NotFound(message),
        401 | 403 => Error::Unauthorized(message),
        409 => Error::Conflict(message),
        _ if message.contains("already exists") => Error::Conflict(message),
        422 => Error::InvalidInput(message),
        429 => Error::RateLimited(RateLimit {
            retry_after_seconds: retry_after,
            message,
        }),
        _ => Error::Upstream(UpstreamError { status, message }),
    }
}

/// Forgejo reports `{"message": "..."}`; anything else is passed on as (truncated) text.
fn message(body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct Body {
        message: String,
    }
    match serde_json::from_slice::<Body>(body) {
        Ok(b) => b.message,
        Err(_) => String::from_utf8_lossy(body).chars().take(200).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_statuses() {
        let body = br#"{"message":"nope"}"#;
        assert!(matches!(from_status(404, None, body), Error::NotFound(m) if m == "nope"));
        assert!(matches!(
            from_status(401, None, body),
            Error::Unauthorized(_)
        ));
        assert!(matches!(
            from_status(403, None, body),
            Error::Unauthorized(_)
        ));
        assert!(matches!(from_status(409, None, body), Error::Conflict(_)));
        assert!(matches!(
            from_status(422, None, body),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            from_status(429, Some(2), body),
            Error::RateLimited(RateLimit {
                retry_after_seconds: Some(2),
                ..
            })
        ));
        assert!(matches!(
            from_status(500, None, b"boom"),
            Error::Upstream(UpstreamError { status: 500, .. })
        ));
    }

    #[test]
    fn already_exists_is_a_conflict_even_as_422() {
        let body = br#"{"message":"pull request already exists for these targets"}"#;
        assert!(matches!(from_status(422, None, body), Error::Conflict(_)));
    }
}
