//! Rules for the MCP servers an admin registers (iter4-mcp-gateway): server names, upstream URLs,
//! credential schemes and the tool names agents see.
//!
//! The gateway prefixes every upstream tool with `<server>__`; the runtime registers the relay as
//! `ext`, so a harness shows `mcp__ext__<server>__<tool>`.

use std::fmt;
use std::str::FromStr;

use serde::Serialize;
use thiserror::Error;
use url::Url;

use crate::domain::ParseEnumError;
use crate::header_template::{self, HeaderError};

/// Longest server name.
pub const MAX_NAME_CHARS: usize = 20;
/// Longest tool name a harness accepts, counted over `mcp__ext__<server>__<tool>`.
pub const MAX_HARNESS_TOOL_NAME_CHARS: usize = 64;
/// Separates the server name from the upstream tool name.
pub const TOOL_SEPARATOR: &str = "__";
/// What a harness puts in front of `<server>__<tool>` for the relay `ext`.
pub const HARNESS_TOOL_PREFIX: &str = "mcp__ext__";
/// Path of the MCP endpoint on the gateway's Git port.
pub const MCP_PATH: &str = "/mcp";
/// The `tool` of the call record of a server left out of a `tools/list`.
pub const LIST_TOOL: &str = "tools/list";

/// Headers the gateway sets itself on every MCP request, besides the connection headers.
const GATEWAY_HEADERS: [&str; 4] = [
    "content-type",
    "accept",
    "mcp-session-id",
    "mcp-protocol-version",
];

/// How the gateway presents the credential to the upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "scheme", rename_all = "kebab-case")]
pub enum McpCredentialScheme {
    /// `Authorization: Bearer <value>`
    Bearer,
    /// `<name>: <prefix><value>`
    Header { name: String, prefix: String },
    /// `?<parameter>=<value>` added after the URL's own parameters, URL-encoded.
    Query { parameter: String },
}

impl McpCredentialScheme {
    /// The stored name of the scheme.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::Header { .. } => "header",
            Self::Query { .. } => "query",
        }
    }
}

impl fmt::Display for McpCredentialScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why an MCP server configuration is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum McpError {
    #[error(
        "Use lowercase letters, digits and single hyphens: start with a letter, end with a letter or digit, at most 20 characters."
    )]
    InvalidName,
    #[error("The URL must be an absolute https URL without user information or fragment.")]
    InvalidUrl,
    #[error("http URLs are only accepted while network.allowPlainHttp is on.")]
    PlainHttpNotAllowed,
    #[error("A header scheme needs a header name.")]
    HeaderNameMissing,
    #[error("Header name: {0}")]
    HeaderName(HeaderError),
    #[error("`{0}` is set by the gateway and cannot carry the credential.")]
    ReservedHeader(String),
    #[error("The header prefix must be a single line without control characters.")]
    InvalidPrefix,
    #[error("A query scheme needs a parameter name.")]
    ParameterMissing,
    #[error("The URL already contains the parameter `{0}`.")]
    ParameterInUrl(String),
}

/// How one reported call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum McpCallOutcome {
    Ok,
    /// The upstream answered with `isError: true` or a JSON-RPC error.
    ToolError,
    /// The server is not enabled for the run, or too many calls were in progress.
    Rejected,
    UpstreamError,
    Timeout,
    TooLarge,
    Cancelled,
}

impl McpCallOutcome {
    pub const ALL: [Self; 7] = [
        Self::Ok,
        Self::ToolError,
        Self::Rejected,
        Self::UpstreamError,
        Self::Timeout,
        Self::TooLarge,
        Self::Cancelled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::ToolError => "tool-error",
            Self::Rejected => "rejected",
            Self::UpstreamError => "upstream-error",
            Self::Timeout => "timeout",
            Self::TooLarge => "too-large",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for McpCallOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for McpCallOutcome {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "McpCallOutcome",
                value: s.to_string(),
            })
    }
}

/// `url` without its query and fragment, for logs and `Debug` output: the query may hold a
/// token or an identifier.
pub fn url_without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or_default()
}

/// Checks a server name: `^[a-z](-?[a-z0-9])*$`, at most [`MAX_NAME_CHARS`] characters.
pub fn validate_name(name: &str) -> Result<(), McpError> {
    let mut chars = name.chars();
    let valid = name.chars().count() <= MAX_NAME_CHARS
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && !name.contains("--")
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(McpError::InvalidName)
    }
}

/// Parses the upstream URL: absolute `https` (`http` only with `allow_plain_http`), a host, no
/// user information, no fragment. Path and query are kept as entered.
pub fn validate_url(value: &str, allow_plain_http: bool) -> Result<Url, McpError> {
    let url = Url::parse(value).map_err(|_error| McpError::InvalidUrl)?;
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" if allow_plain_http => true,
        "http" => return Err(McpError::PlainHttpNotAllowed),
        _ => false,
    };
    if scheme_ok
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
    {
        Ok(url)
    } else {
        Err(McpError::InvalidUrl)
    }
}

/// Checks a scheme against the URL it is used with.
pub fn validate_scheme(scheme: &McpCredentialScheme, url: &Url) -> Result<(), McpError> {
    match scheme {
        McpCredentialScheme::Bearer => Ok(()),
        McpCredentialScheme::Header { name, prefix } => {
            if name.is_empty() {
                return Err(McpError::HeaderNameMissing);
            }
            header_template::validate_credential_name(name).map_err(McpError::HeaderName)?;
            if GATEWAY_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(McpError::ReservedHeader(name.clone()));
            }
            if prefix.chars().any(char::is_control) {
                return Err(McpError::InvalidPrefix);
            }
            Ok(())
        }
        McpCredentialScheme::Query { parameter } => {
            if parameter.is_empty() {
                return Err(McpError::ParameterMissing);
            }
            // `query_pairs` percent-decodes, so `tok%65n` counts as `token`.
            if url
                .query_pairs()
                .any(|(key, _value)| key == parameter.as_str())
            {
                return Err(McpError::ParameterInUrl(parameter.clone()));
            }
            Ok(())
        }
    }
}

/// Whether `tool` of `server` can be used by an agent: a name of 1 to 64 characters of letters,
/// digits, `_` and `-`, which stays within 64 characters as `mcp__ext__<server>__<tool>`.
pub fn is_agent_usable_tool(server: &str, tool: &str) -> bool {
    let pattern_ok = (1..=MAX_HARNESS_TOOL_NAME_CHARS).contains(&tool.len())
        && tool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    pattern_ok
        && HARNESS_TOOL_PREFIX.len() + server.len() + TOOL_SEPARATOR.len() + tool.len()
            <= MAX_HARNESS_TOOL_NAME_CHARS
}

/// The name of `tool` in the aggregated list: `<server>__<tool>`.
pub fn prefixed_tool_name(server: &str, tool: &str) -> String {
    format!("{server}{TOOL_SEPARATOR}{tool}")
}

/// Splits `<server>__<tool>` at the first `__`; server names contain no `_`.
pub fn split_tool_name(name: &str) -> Option<(&str, &str)> {
    name.split_once(TOOL_SEPARATOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_names_follow_the_lowercase_hyphen_rule() {
        for name in [
            "context7",
            "a",
            "wiki",
            "my-tools",
            "a1-b2-c3",
            "abcdefghijklmnopqrst",
        ] {
            assert_eq!(validate_name(name), Ok(()), "{name}");
        }
        for name in [
            "",
            "my--tools",
            "tools-",
            "-tools",
            "1tools",
            "Tools",
            "my_tools",
            "abcdefghijklmnopqrstu",
            "tö",
        ] {
            assert_eq!(validate_name(name), Err(McpError::InvalidName), "{name}");
        }
    }

    #[test]
    fn url_keeps_path_and_query_and_refuses_user_information_and_fragment() {
        let url = validate_url(
            "https://design.example.com/mcp/stream?file=abc&mode=read",
            false,
        )
        .unwrap();
        assert_eq!(url.path(), "/mcp/stream");
        assert_eq!(url.query(), Some("file=abc&mode=read"));
        for value in [
            "https://user:secret@mcp.example.com/mcp",
            "https://user@mcp.example.com/mcp",
            "https://mcp.example.com/mcp#frag",
            "ftp://mcp.example.com/mcp",
            "mcp.example.com/mcp",
        ] {
            assert_eq!(
                validate_url(value, true),
                Err(McpError::InvalidUrl),
                "{value}"
            );
        }
    }

    #[test]
    fn plain_http_needs_the_instance_setting() {
        assert_eq!(
            validate_url("http://mcp.internal/mcp", false),
            Err(McpError::PlainHttpNotAllowed)
        );
        assert!(validate_url("http://mcp.internal/mcp", true).is_ok());
    }

    fn url(value: &str) -> Url {
        validate_url(value, false).unwrap()
    }

    #[test]
    fn header_scheme_refuses_names_the_gateway_sets() {
        let url = url("https://mcp.example.com/mcp");
        let header = |name: &str| McpCredentialScheme::Header {
            name: name.into(),
            prefix: String::new(),
        };
        assert_eq!(validate_scheme(&header("CONTEXT7_API_KEY"), &url), Ok(()));
        assert_eq!(validate_scheme(&header("Authorization"), &url), Ok(()));
        for name in [
            "Mcp-Session-Id",
            "mcp-protocol-version",
            "Content-Type",
            "Accept",
        ] {
            assert_eq!(
                validate_scheme(&header(name), &url),
                Err(McpError::ReservedHeader(name.into())),
                "{name}"
            );
        }
        for name in [
            "Host",
            "Content-Length",
            "Connection",
            "Transfer-Encoding",
            "Upgrade",
        ] {
            assert!(
                matches!(
                    validate_scheme(&header(name), &url),
                    Err(McpError::HeaderName(HeaderError::ReservedName(_)))
                ),
                "{name}"
            );
        }
        assert_eq!(
            validate_scheme(&header(""), &url),
            Err(McpError::HeaderNameMissing)
        );
        assert!(matches!(
            validate_scheme(&header("bad name"), &url),
            Err(McpError::HeaderName(HeaderError::InvalidName(_)))
        ));
    }

    #[test]
    fn header_prefix_is_a_single_line() {
        let scheme = McpCredentialScheme::Header {
            name: "X-Key".into(),
            prefix: "Token\r\nX-Evil: 1".into(),
        };
        assert_eq!(
            validate_scheme(&scheme, &url("https://mcp.example.com/mcp")),
            Err(McpError::InvalidPrefix)
        );
    }

    #[test]
    fn query_parameter_already_in_the_url_is_refused_after_percent_decoding() {
        let query = |parameter: &str| McpCredentialScheme::Query {
            parameter: parameter.into(),
        };
        let plain = url("https://design.example.com/mcp/stream?file=abc");
        assert_eq!(validate_scheme(&query("token"), &plain), Ok(()));
        let encoded = url("https://design.example.com/mcp/stream?tok%65n=abc");
        assert_eq!(
            validate_scheme(&query("token"), &encoded),
            Err(McpError::ParameterInUrl("token".into()))
        );
        assert_eq!(
            validate_scheme(&query(""), &plain),
            Err(McpError::ParameterMissing)
        );
    }

    #[test]
    fn tool_names_must_fit_the_harness_limit_with_their_prefix() {
        assert!(is_agent_usable_tool("context7", "get-library-docs"));
        assert!(is_agent_usable_tool("a", "Tool_1-x"));
        assert!(!is_agent_usable_tool("context7", ""));
        assert!(!is_agent_usable_tool("context7", "has space"));
        assert!(!is_agent_usable_tool("context7", "dot.ted"));
        // `mcp__ext__forgejo__` is 19 characters: 45 fit, 46 do not.
        assert!(is_agent_usable_tool("forgejo", &"x".repeat(45)));
        assert!(!is_agent_usable_tool("forgejo", &"x".repeat(46)));
        assert!(!is_agent_usable_tool("a", &"x".repeat(65)));
    }

    #[test]
    fn outcomes_round_trip_through_their_stored_names() {
        for outcome in McpCallOutcome::ALL {
            assert_eq!(outcome.as_str().parse::<McpCallOutcome>(), Ok(outcome));
        }
        assert!("nope".parse::<McpCallOutcome>().is_err());
    }

    #[test]
    fn url_without_query_drops_query_and_fragment() {
        assert_eq!(
            url_without_query("https://h.example/mcp?token=abc#x"),
            "https://h.example/mcp"
        );
        assert_eq!(
            url_without_query("https://h.example/mcp"),
            "https://h.example/mcp"
        );
    }

    #[test]
    fn tool_names_split_at_the_first_separator() {
        assert_eq!(prefixed_tool_name("wiki", "search"), "wiki__search");
        assert_eq!(
            split_tool_name("wiki__search__all"),
            Some(("wiki", "search__all"))
        );
        assert_eq!(split_tool_name("nosep"), None);
    }
}
