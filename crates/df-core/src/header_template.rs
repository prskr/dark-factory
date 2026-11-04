//! Header templates of model providers (iter3-inference-gateway design D7).
//!
//! A template is a minijinja expression over the values of a run and the `sha256` filter
//! (lowercase hex). Nothing else is available: no loader, no other filters, undefined variables
//! are errors and the engine runs with a fuel limit.

use minijinja::{Environment, UndefinedBehavior, context};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Names usable in a template.
pub const VARIABLES: [&str; 5] = ["task_id", "task_key", "run_id", "project_key", "persona_id"];

/// Instructions one render may execute.
const FUEL: u64 = 10_000;
/// Longest header value, rendered or not.
const MAX_VALUE_BYTES: usize = 4096;

/// Headers that belong to one connection; the gateway never forwards them, in both directions.
pub const HOP_BY_HOP_HEADERS: [&str; 9] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Credentials of the client; it only holds a placeholder, the gateway strips them.
pub const CLIENT_CREDENTIAL_HEADERS: [&str; 4] = [
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "api-key",
];

/// Whether the gateway owns the header `lower` (lower case) because it belongs to the connection
/// to the upstream: hop-by-hop headers, `Host` and `Content-Length`.
pub fn is_connection_header(lower: &str) -> bool {
    HOP_BY_HOP_HEADERS.contains(&lower) || matches!(lower, "host" | "content-length")
}

/// Whether the gateway never takes the header `lower` (lower case) from a provider's extra
/// headers: connection headers and the client credentials.
pub fn is_reserved_header(lower: &str) -> bool {
    is_connection_header(lower) || CLIENT_CREDENTIAL_HEADERS.contains(&lower)
}

/// The values a template can use, all of one run.
#[derive(Debug, Clone, Copy)]
pub struct TemplateVars<'a> {
    pub task_id: &'a str,
    pub task_key: &'a str,
    pub run_id: &'a str,
    pub project_key: &'a str,
    pub persona_id: &'a str,
}

impl TemplateVars<'static> {
    /// Values for checking a template on save.
    pub const DUMMY: Self = Self {
        task_id: "00000000-0000-7000-8000-000000000001",
        task_key: "DF-1",
        run_id: "00000000-0000-7000-8000-000000000002",
        project_key: "DF",
        persona_id: "00000000-0000-7000-8000-000000000003",
    };
}

/// Why a header name or template is not usable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HeaderError {
    #[error("The header name is required.")]
    EmptyName,
    #[error("`{0}` is not a valid header name.")]
    InvalidName(String),
    #[error("`{0}` is set by the gateway and cannot be a provider header.")]
    ReservedName(String),
    #[error("The header value is invalid: {0}")]
    InvalidTemplate(String),
    #[error("The header value must be a single line of at most {MAX_VALUE_BYTES} characters.")]
    InvalidValue,
}

/// Renders `template` with `vars`.
pub fn render(template: &str, vars: &TemplateVars<'_>) -> Result<String, HeaderError> {
    let mut env = Environment::empty();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_fuel(Some(FUEL));
    env.add_filter("sha256", |value: String| {
        hex::encode(Sha256::digest(value.as_bytes()))
    });
    let rendered = env
        .render_str(
            template,
            context! {
                task_id => vars.task_id,
                task_key => vars.task_key,
                run_id => vars.run_id,
                project_key => vars.project_key,
                persona_id => vars.persona_id,
            },
        )
        .map_err(|error| HeaderError::InvalidTemplate(describe(&error)))?;
    let usable =
        rendered.len() <= MAX_VALUE_BYTES && rendered.chars().all(|c| c == '\t' || !c.is_control());
    if usable {
        Ok(rendered)
    } else {
        Err(HeaderError::InvalidValue)
    }
}

/// Checks a header of a provider: its name against the reserved ones and `credential_header`
/// (the header the provider's scheme sets), its template by rendering it with dummy values.
pub fn validate(
    name: &str,
    template: &str,
    credential_header: Option<&str>,
) -> Result<(), HeaderError> {
    validate_name(name, credential_header)?;
    render(template, &TemplateVars::DUMMY).map(|_| ())
}

/// Checks a header name: a token, not reserved and not `credential_header`.
pub fn validate_name(name: &str, credential_header: Option<&str>) -> Result<(), HeaderError> {
    if name.is_empty() {
        return Err(HeaderError::EmptyName);
    }
    if !is_token(name) {
        return Err(HeaderError::InvalidName(name.to_string()));
    }
    let lower = name.to_ascii_lowercase();
    let is_credential = credential_header.is_some_and(|header| header.eq_ignore_ascii_case(&lower));
    if is_reserved_header(&lower) || is_credential {
        return Err(HeaderError::ReservedName(name.to_string()));
    }
    Ok(())
}

/// Checks the name of the header a provider's scheme sets: a token that is no connection header.
/// Unlike an extra header it may be a client credential header such as `api-key`: the gateway
/// strips the client's and sets its own.
pub fn validate_credential_name(name: &str) -> Result<(), HeaderError> {
    if name.is_empty() {
        return Err(HeaderError::EmptyName);
    }
    if !is_token(name) {
        return Err(HeaderError::InvalidName(name.to_string()));
    }
    if is_connection_header(&name.to_ascii_lowercase()) {
        return Err(HeaderError::ReservedName(name.to_string()));
    }
    Ok(())
}

/// Whether `name` is a valid HTTP header name (RFC 9110 token).
pub fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// The part of a minijinja error worth showing: its kind and detail, without the source excerpt.
fn describe(error: &minijinja::Error) -> String {
    match error.detail() {
        Some(detail) => format!("{}: {detail}", error.kind()),
        None => error.kind().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(task_id: &'a str, run_id: &'a str) -> TemplateVars<'a> {
        TemplateVars {
            task_id,
            task_key: "DF-7",
            run_id,
            project_key: "DF",
            persona_id: "p",
        }
    }

    #[test]
    fn sha256_session_is_stable_across_the_runs_of_a_task() {
        let template = "{{ task_id | sha256 }}";
        let first = render(template, &vars("task-1", "run-1")).unwrap();
        let second = render(template, &vars("task-1", "run-2")).unwrap();
        let other = render(template, &vars("task-2", "run-1")).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }

    #[test]
    fn sha256_filter_matches_the_known_digest() {
        let rendered = render("{{ 'abc' | sha256 }}", &vars("t", "r")).unwrap();
        assert_eq!(
            rendered,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn unknown_filter_is_rejected_and_named() {
        let error = validate("x-session", "{{ task_id | md5 }}", None).unwrap_err();
        assert!(matches!(error, HeaderError::InvalidTemplate(_)));
        assert!(error.to_string().contains("md5"), "{error}");
    }

    #[test]
    fn undefined_variable_is_rejected() {
        let error = validate("x-session", "{{ nope }}", None).unwrap_err();
        assert!(matches!(error, HeaderError::InvalidTemplate(_)), "{error}");
    }

    #[test]
    fn syntax_error_is_rejected() {
        assert!(validate("x-session", "{{ task_id", None).is_err());
    }

    #[test]
    fn endless_loop_runs_out_of_fuel() {
        let template = "{% for i in range(100000000) %}x{% endfor %}";
        assert!(validate("x-session", template, None).is_err());
    }

    #[test]
    fn only_the_sha256_filter_exists() {
        assert!(validate("x-session", "{{ task_id | upper }}", None).is_err());
    }

    #[test]
    fn plain_text_is_a_valid_template() {
        assert_eq!(render("fixed", &vars("t", "r")).unwrap(), "fixed");
    }

    #[test]
    fn credential_and_hop_by_hop_names_are_reserved() {
        for name in [
            "Host",
            "Content-Length",
            "Authorization",
            "x-api-key",
            "Connection",
        ] {
            assert!(
                matches!(validate_name(name, None), Err(HeaderError::ReservedName(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn credential_header_may_be_a_client_credential_name_but_no_connection_header() {
        for name in ["api-key", "Authorization", "x-api-key", "Api-Token"] {
            assert_eq!(validate_credential_name(name), Ok(()), "{name}");
        }
        for name in [
            "Host",
            "content-length",
            "Connection",
            "Proxy-Authorization",
        ] {
            assert!(
                matches!(
                    validate_credential_name(name),
                    Err(HeaderError::ReservedName(_))
                ),
                "{name}"
            );
        }
        assert_eq!(validate_credential_name(""), Err(HeaderError::EmptyName));
        assert!(validate_credential_name("bad name").is_err());
    }

    #[test]
    fn name_of_the_scheme_header_is_reserved_in_any_case() {
        let error = validate_name("X-Custom-Key", Some("x-custom-key")).unwrap_err();
        assert!(matches!(error, HeaderError::ReservedName(_)));
        assert!(validate_name("x-custom-session", Some("x-custom-key")).is_ok());
    }

    #[test]
    fn invalid_names_are_rejected() {
        assert_eq!(validate_name("", None), Err(HeaderError::EmptyName));
        assert!(matches!(
            validate_name("bad name", None),
            Err(HeaderError::InvalidName(_))
        ));
    }

    #[test]
    fn rendered_value_with_a_line_break_is_rejected() {
        assert_eq!(
            render("a{{ '\\n' }}b", &vars("t", "r")),
            Err(HeaderError::InvalidValue)
        );
    }
}
