//! Maps GitHub's error responses to the `error` variant of the contract.

use crate::api::Response;
use crate::darkfactory::integrations::types::{Error, RateLimit, UpstreamError};

/// Wait when a rate limit gives neither `retry-after` nor `x-ratelimit-reset`.
const DEFAULT_RETRY_AFTER: u32 = 60;

/// Appended to 404s: GitHub hides private repositories the token cannot see.
const NOT_FOUND_HINT: &str =
    "GitHub also answers 404 when the token cannot see a private repository";

/// Error of the non-2xx `response`; `now` is the Unix time in seconds.
pub(crate) fn from_response(response: &Response, now: u64) -> Error {
    let body = Body::parse(&response.body);
    let status = response.status;
    if matches!(status, 403 | 429) && is_rate_limited(response, &body.message) {
        return Error::RateLimited(RateLimit {
            retry_after_seconds: Some(retry_after(response, now)),
            message: body.message,
        });
    }
    match status {
        401 | 403 => Error::Unauthorized(body.message),
        404 => Error::NotFound(format!("{} ({NOT_FOUND_HINT})", body.message)),
        409 => Error::Conflict(body.message),
        422 => validation_failed(body),
        _ => Error::Upstream(UpstreamError {
            status,
            message: body.message,
        }),
    }
}

/// Errors of a GraphQL response with HTTP 200, mapped by the first error's `type`.
pub(crate) fn from_graphql(body: &[u8]) -> Result<(), Error> {
    let body: GraphqlBody = serde_json::from_slice(body)
        .map_err(|e| Error::Internal(format!("unexpected GitHub response: {e}")))?;
    let Some(first) = body.errors.first() else {
        return Ok(());
    };
    let message = body
        .errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    Err(match first.kind.as_deref() {
        Some("NOT_FOUND") => Error::NotFound(message),
        Some("FORBIDDEN") => Error::Unauthorized(message),
        Some("RATE_LIMITED") => Error::RateLimited(RateLimit {
            retry_after_seconds: Some(DEFAULT_RETRY_AFTER),
            message,
        }),
        _ => Error::Upstream(UpstreamError {
            status: 200,
            message,
        }),
    })
}

fn is_rate_limited(response: &Response, message: &str) -> bool {
    response.ratelimit_remaining == Some(0)
        || response.retry_after.is_some()
        || message.to_lowercase().contains("rate limit")
}

/// `retry-after`, else the seconds until `x-ratelimit-reset`, else the default.
fn retry_after(response: &Response, now: u64) -> u32 {
    match (response.retry_after, response.ratelimit_reset) {
        (Some(seconds), _) => seconds,
        (None, Some(reset)) => u32::try_from(reset.saturating_sub(now)).unwrap_or(u32::MAX),
        (None, None) => DEFAULT_RETRY_AFTER,
    }
}

/// A 422 for a duplicate pull request is a conflict, any other is invalid input.
fn validation_failed(body: Body) -> Error {
    let messages: Vec<String> = std::iter::once(body.message)
        .chain(body.errors.into_iter().filter_map(|e| e.message))
        .collect();
    let duplicate = messages
        .iter()
        .any(|m| m.to_lowercase().contains("pull request already exists"));
    let joined = messages.join("; ");
    if duplicate {
        Error::Conflict(joined)
    } else {
        Error::InvalidInput(joined)
    }
}

/// GitHub's error body: `{"message": "...", "errors": [{"message": "..."}]}`.
#[derive(serde::Deserialize)]
struct Body {
    message: String,
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

impl Body {
    /// Anything that is not GitHub's error body is passed on as (truncated) text.
    fn parse(body: &[u8]) -> Self {
        serde_json::from_slice(body).unwrap_or_else(|_| Self {
            message: String::from_utf8_lossy(body).chars().take(200).collect(),
            errors: Vec::new(),
        })
    }
}

#[derive(serde::Deserialize)]
struct ErrorItem {
    message: Option<String>,
}

/// GraphQL's body: `{"data": ..., "errors": [{"type": "...", "message": "..."}]}`.
#[derive(serde::Deserialize)]
struct GraphqlBody {
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(serde::Deserialize)]
struct GraphqlError {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    message: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn response(status: u16, body: serde_json::Value) -> Response {
        Response {
            status,
            body: body.to_string().into_bytes(),
            ..Response::default()
        }
    }

    fn retry_after(err: &Error) -> Option<u32> {
        match err {
            Error::RateLimited(RateLimit {
                retry_after_seconds,
                ..
            }) => *retry_after_seconds,
            other => panic!("expected rate-limited, got {other:?}"),
        }
    }

    #[test]
    fn missing_permission_is_unauthorized_with_githubs_message() {
        let msg = "Resource not accessible by personal access token";
        let err = from_response(&response(403, json!({"message": msg})), NOW);
        assert!(
            matches!(&err, Error::Unauthorized(m) if m == msg),
            "{err:?}"
        );
    }

    #[test]
    fn primary_rate_limit_waits_until_the_reset() {
        let err = from_response(
            &Response {
                ratelimit_remaining: Some(0),
                ratelimit_reset: Some(NOW + 600),
                ..response(403, json!({"message": "API rate limit exceeded"}))
            },
            NOW,
        );
        assert_eq!(retry_after(&err), Some(600));
    }

    #[test]
    fn exhausted_quota_with_a_past_reset_waits_zero_seconds() {
        let err = from_response(
            &Response {
                ratelimit_remaining: Some(0),
                ratelimit_reset: Some(NOW - 5),
                ..response(403, json!({"message": "forbidden"}))
            },
            NOW,
        );
        assert_eq!(retry_after(&err), Some(0));
    }

    #[test]
    fn secondary_rate_limit_uses_retry_after() {
        let err = from_response(
            &Response {
                retry_after: Some(30),
                ..response(429, json!({"message": "slow down"}))
            },
            NOW,
        );
        assert_eq!(retry_after(&err), Some(30));
    }

    #[test]
    fn secondary_rate_limit_without_hint_waits_sixty_seconds() {
        let body = json!({"message": "You have exceeded a secondary Rate Limit."});
        let err = from_response(&response(403, body), NOW);
        assert_eq!(retry_after(&err), Some(60));
    }

    #[test]
    fn not_found_mentions_private_repositories() {
        let err = from_response(&response(404, json!({"message": "Not Found"})), NOW);
        assert!(
            matches!(&err, Error::NotFound(m) if m.starts_with("Not Found") && m.contains("private repository")),
            "{err:?}"
        );
    }

    #[test]
    fn conflict_status_is_a_conflict() {
        let err = from_response(&response(409, json!({"message": "Merge conflict"})), NOW);
        assert!(
            matches!(&err, Error::Conflict(m) if m == "Merge conflict"),
            "{err:?}"
        );
    }

    #[test]
    fn duplicate_pull_request_is_a_conflict() {
        let body = json!({
            "message": "Validation Failed",
            "errors": [{"resource": "PullRequest", "code": "custom",
                        "message": "A pull request already exists for o:factory/task/DF-1."}],
        });
        let err = from_response(&response(422, body), NOW);
        assert!(
            matches!(&err, Error::Conflict(m) if m.contains("already exists")),
            "{err:?}"
        );
    }

    #[test]
    fn other_already_exists_validation_failure_is_invalid_input() {
        let body = json!({
            "message": "Validation Failed",
            "errors": [{"resource": "Label", "code": "already_exists",
                        "message": "Label already exists"}],
        });
        let err = from_response(&response(422, body), NOW);
        assert!(
            matches!(&err, Error::InvalidInput(m) if m.contains("Label already exists")),
            "{err:?}"
        );
    }

    #[test]
    fn duplicate_pull_request_in_the_top_level_message_is_a_conflict() {
        let body = json!({"message": "a Pull Request Already Exists for o:topic"});
        let err = from_response(&response(422, body), NOW);
        assert!(matches!(&err, Error::Conflict(_)), "{err:?}");
    }

    #[test]
    fn other_validation_failure_is_invalid_input_with_every_message() {
        let body = json!({
            "message": "Validation Failed",
            "errors": [{"resource": "PullRequest", "code": "custom",
                        "message": "No commits between main and topic"}],
        });
        let err = from_response(&response(422, body), NOW);
        assert!(
            matches!(&err, Error::InvalidInput(m)
                if m.contains("Validation Failed") && m.contains("No commits between main and topic")),
            "{err:?}"
        );
    }

    fn graphql(errors: serde_json::Value) -> Result<(), Error> {
        from_graphql(
            json!({"data": null, "errors": errors})
                .to_string()
                .as_bytes(),
        )
    }

    #[test]
    fn graphql_without_errors_succeeds() {
        let body = json!({"data": {"m": {"clientMutationId": null}}}).to_string();
        assert!(from_graphql(body.as_bytes()).is_ok());
    }

    #[test]
    fn graphql_errors_map_by_type() {
        let err = |t: &str| graphql(json!([{"type": t, "message": "m"}])).unwrap_err();
        assert!(matches!(err("NOT_FOUND"), Error::NotFound(m) if m == "m"));
        assert!(matches!(err("FORBIDDEN"), Error::Unauthorized(m) if m == "m"));
        assert_eq!(retry_after(&err("RATE_LIMITED")), Some(60));
    }

    #[test]
    fn other_graphql_errors_are_upstream_with_every_message() {
        let err = graphql(json!([
            {"type": "UNPROCESSABLE", "message": "one"},
            {"message": "two"},
        ]))
        .unwrap_err();
        assert!(
            matches!(&err, Error::Upstream(UpstreamError { status: 200, message }) if message == "one; two"),
            "{err:?}"
        );
    }

    #[test]
    fn server_error_is_upstream_with_status_and_message() {
        let err = from_response(&response(500, json!({"message": "boom"})), NOW);
        assert!(
            matches!(&err, Error::Upstream(UpstreamError { status: 500, message }) if message == "boom"),
            "{err:?}"
        );
    }
}
