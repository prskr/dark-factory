//! One upstream MCP server as the gateway uses it: the snapshot entry with its resolved
//! credential, how the credential is sent, and the redaction of the credential in everything that
//! comes back from the upstream (iter4-mcp-gateway design D7).

use std::borrow::Cow;
use std::fmt;

use df_proto::gateway::v1 as pb;
use hyper::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use super::url::McpUrl;

const REDACTED: &str = "[redacted]";

/// Why a server cannot be used; the endpoint answers every such call like a missing credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetError {
    /// The snapshot carries no credential for the server.
    CredentialMissing,
    /// The credential scheme is not set.
    SchemeUnset,
    /// The URL is not usable (not https, or http without `network.allowPlainHttp`).
    InvalidUrl,
    /// The credential cannot be sent as the scheme demands (invalid header name or value).
    CredentialUnusable,
}

/// How the credential travels.
#[derive(Debug, Clone)]
enum Placement {
    /// `Authorization: Bearer <value>`
    Bearer,
    /// `<name>: <prefix><value>`
    Header { name: HeaderName, prefix: String },
    /// `?<parameter>=<value>`
    Query { parameter: String },
}

/// A credential value; never printed.
#[derive(Clone)]
struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// An upstream MCP server of the snapshot with its credential.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub(crate) name: String,
    pub(crate) private_network: bool,
    pub(crate) config_version: u64,
    url: McpUrl,
    placement: Placement,
    secret: Secret,
}

impl Target {
    /// `server` with its credential value (`None` if the snapshot has none).
    pub(crate) fn from_proto(
        server: &pb::McpServer,
        credential: Option<&pb::ProviderCredential>,
        allow_plain_http: bool,
    ) -> Result<Self, TargetError> {
        let scheme = server
            .credential_scheme
            .as_ref()
            .ok_or(TargetError::SchemeUnset)?;
        let secret = credential
            .ok_or(TargetError::CredentialMissing)?
            .value
            .clone();
        let url = McpUrl::parse(&server.url, allow_plain_http).ok_or(TargetError::InvalidUrl)?;
        let placement = match scheme {
            pb::mcp_server::CredentialScheme::Bearer(_) => Placement::Bearer,
            pb::mcp_server::CredentialScheme::Header(header) => Placement::Header {
                name: HeaderName::from_bytes(header.name.as_bytes())
                    .map_err(|_invalid| TargetError::CredentialUnusable)?,
                prefix: header.prefix.clone(),
            },
            pb::mcp_server::CredentialScheme::Query(query) => Placement::Query {
                parameter: query.parameter.clone(),
            },
        };
        let target = Self {
            name: server.name.clone(),
            private_network: server.private_network,
            config_version: server.config_version,
            url,
            placement,
            secret: Secret(secret),
        };
        // A value that cannot be a header fails here, not on the first call.
        target.credential_header()?;
        Ok(target)
    }

    /// The URL requests go to: the registered URL, with the credential as last parameter for the
    /// `query` scheme.
    pub(crate) fn request_url(&self) -> McpUrl {
        match &self.placement {
            Placement::Query { parameter } => self.url.with_query_param(parameter, &self.secret.0),
            Placement::Bearer | Placement::Header { .. } => self.url.clone(),
        }
    }

    /// Adds the credential header of the scheme, if it has one.
    pub(crate) fn add_credential(&self, headers: &mut HeaderMap) -> Result<(), TargetError> {
        if let Some((name, value)) = self.credential_header()? {
            headers.insert(name, value);
        }
        Ok(())
    }

    fn credential_header(&self) -> Result<Option<(HeaderName, HeaderValue)>, TargetError> {
        let (name, prefix) = match &self.placement {
            Placement::Bearer => (AUTHORIZATION, "Bearer "),
            Placement::Header { name, prefix } => (name.clone(), prefix.as_str()),
            Placement::Query { .. } => return Ok(None),
        };
        let mut value = HeaderValue::from_str(&format!("{prefix}{}", self.secret.0))
            .map_err(|_invalid| TargetError::CredentialUnusable)?;
        value.set_sensitive(true);
        Ok(Some((name, value)))
    }

    /// The URL without its query, for logs.
    pub(crate) fn url_for_logs(&self) -> String {
        format!("{:?}", self.url)
    }

    pub(crate) fn redactor(&self) -> Redactor {
        Redactor::new(&self.secret.0)
    }
}

/// Replaces a credential value with `[redacted]`: raw, and percent-encoded in any way an
/// upstream may echo it. Every byte of the credential matches itself, its escape `%XX` (hex digits
/// of either case, and any subset of the bytes escaped) and, for a space, `+`.
#[derive(Debug, Clone)]
pub(crate) struct Redactor {
    secret: Secret,
}

impl Redactor {
    fn new(secret: &str) -> Self {
        Self {
            secret: Secret(secret.to_owned()),
        }
    }

    pub(crate) fn text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let secret = self.secret.0.as_bytes();
        let Some(first) = secret.first().copied() else {
            return Cow::Borrowed(text);
        };
        let haystack = text.as_bytes();
        let mut output: Option<Vec<u8>> = None;
        let mut copied = 0;
        let mut at = 0;
        while at < haystack.len() {
            // Cheap test first: the match starts with the first byte, or with its escape.
            let starts = haystack.get(at).is_some_and(|byte| {
                *byte == first || *byte == b'%' || (first == b' ' && *byte == b'+')
            });
            let matched = starts.then(|| match_secret(haystack, at, secret)).flatten();
            let Some(end) = matched else {
                at += 1;
                continue;
            };
            let out = output.get_or_insert_with(Vec::new);
            out.extend_from_slice(haystack.get(copied..at).unwrap_or_default());
            out.extend_from_slice(REDACTED.as_bytes());
            copied = end;
            at = end;
        }
        match output {
            None => Cow::Borrowed(text),
            Some(mut out) => {
                out.extend_from_slice(haystack.get(copied..).unwrap_or_default());
                Cow::Owned(String::from_utf8_lossy(&out).into_owned())
            }
        }
    }

    /// Redacts every string of `value`, object keys included.
    pub(crate) fn value(&self, value: &mut Value) {
        match value {
            Value::String(text) => {
                if let Cow::Owned(redacted) = self.text(text) {
                    *text = redacted;
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.value(item)),
            Value::Object(map) => {
                let entries = std::mem::take(map);
                for (key, mut item) in entries {
                    self.value(&mut item);
                    map.insert(self.text(&key).into_owned(), item);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

/// The end of a match of `secret` in `haystack` that starts at `start`, if there is one. A byte
/// of the credential may appear literally or as `%XX`; a `%` of the credential can be read both
/// ways (`%` and `%25`), so the readings are tried one after the other. The two hex digits that
/// follow a `%` of the credential look like an escape, so their case does not matter either.
fn match_secret(haystack: &[u8], start: usize, secret: &[u8]) -> Option<usize> {
    match_from(haystack, start, secret, 0)
}

/// [`match_secret`] with `fold` bytes of `secret` left whose ASCII case is ignored.
fn match_from(haystack: &[u8], start: usize, secret: &[u8], fold: u8) -> Option<usize> {
    let Some((byte, rest)) = secret.split_first() else {
        return Some(start);
    };
    let looks_like_escape = *byte == b'%'
        && rest
            .get(..2)
            .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit));
    let next_fold = if looks_like_escape {
        2
    } else {
        fold.saturating_sub(1)
    };
    let here = *haystack.get(start)?;
    let same = here == *byte || (fold > 0 && here.eq_ignore_ascii_case(byte));
    if (same || (*byte == b' ' && here == b'+'))
        && let Some(end) = match_from(haystack, start + 1, rest, next_fold)
    {
        return Some(end);
    }
    let (high, low) = (*haystack.get(start + 1)?, *haystack.get(start + 2)?);
    let escaped = here == b'%' && (hex_value(high)? << 4 | hex_value(low)?) == *byte;
    if escaped {
        match_from(haystack, start + 3, rest, next_fold)
    } else {
        None
    }
}

const fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn server(scheme: pb::mcp_server::CredentialScheme) -> pb::McpServer {
        pb::McpServer {
            id: "s1".into(),
            name: "penpot".into(),
            url: "https://design.example.com/mcp/stream?file=abc".into(),
            credential_ref: "c1".into(),
            credential_scheme: Some(scheme),
            private_network: false,
            config_version: 3,
        }
    }

    fn credential(value: &str) -> pb::ProviderCredential {
        pb::ProviderCredential {
            value: value.into(),
        }
    }

    fn bearer() -> pb::mcp_server::CredentialScheme {
        pb::mcp_server::CredentialScheme::Bearer(pb::McpBearerScheme {})
    }

    #[test]
    fn bearer_and_header_credentials_become_one_sensitive_header() {
        let target =
            Target::from_proto(&server(bearer()), Some(&credential("tok")), false).unwrap();
        let mut headers = HeaderMap::new();
        target.add_credential(&mut headers).unwrap();
        assert_eq!(headers["authorization"], "Bearer tok");
        assert!(headers["authorization"].is_sensitive());

        let header = pb::mcp_server::CredentialScheme::Header(pb::McpHeaderScheme {
            name: "CONTEXT7_API_KEY".into(),
            prefix: "Key ".into(),
        });
        let target = Target::from_proto(&server(header), Some(&credential("tok")), false).unwrap();
        let mut headers = HeaderMap::new();
        target.add_credential(&mut headers).unwrap();
        assert_eq!(headers["context7_api_key"], "Key tok");
        assert_eq!(
            target.request_url().request_target(),
            "/mcp/stream?file=abc"
        );
    }

    #[test]
    fn query_credential_is_appended_url_encoded_and_sets_no_header() {
        let query = pb::mcp_server::CredentialScheme::Query(pb::McpQueryScheme {
            parameter: "token".into(),
        });
        let target = Target::from_proto(&server(query), Some(&credential("a b/c")), false).unwrap();
        assert_eq!(
            target.request_url().request_target(),
            "/mcp/stream?file=abc&token=a+b%2Fc"
        );
        let mut headers = HeaderMap::new();
        target.add_credential(&mut headers).unwrap();
        assert!(headers.is_empty());
        assert_eq!(
            target.url_for_logs(),
            "https://design.example.com/mcp/stream"
        );
    }

    #[test]
    fn a_server_without_credential_scheme_or_value_or_usable_url_is_refused() {
        let mut unset = server(bearer());
        unset.credential_scheme = None;
        assert_eq!(
            Target::from_proto(&unset, Some(&credential("x")), false).unwrap_err(),
            TargetError::SchemeUnset
        );
        assert_eq!(
            Target::from_proto(&server(bearer()), None, false).unwrap_err(),
            TargetError::CredentialMissing
        );
        let mut plain = server(bearer());
        plain.url = "http://h.example/mcp".into();
        assert_eq!(
            Target::from_proto(&plain, Some(&credential("x")), false).unwrap_err(),
            TargetError::InvalidUrl
        );
        assert!(Target::from_proto(&plain, Some(&credential("x")), true).is_ok());
        assert_eq!(
            Target::from_proto(&server(bearer()), Some(&credential("line\nbreak")), false)
                .unwrap_err(),
            TargetError::CredentialUnusable
        );
    }

    #[test]
    fn debug_output_has_neither_the_credential_nor_the_query() {
        let query = pb::mcp_server::CredentialScheme::Query(pb::McpQueryScheme {
            parameter: "token".into(),
        });
        let target =
            Target::from_proto(&server(query), Some(&credential("sekret")), false).unwrap();
        let debug = format!("{target:?}");
        assert!(!debug.contains("sekret"), "{debug}");
        assert!(!debug.contains("file=abc"), "{debug}");
    }

    #[test]
    fn redaction_replaces_the_raw_and_the_encoded_credential_everywhere() {
        let redactor = Redactor::new("a b/c");
        assert_eq!(redactor.text("none here"), "none here");
        assert_eq!(
            redactor.text("raw a b/c, form a+b%2Fc, percent a%20b%2Fc"),
            "raw [redacted], form [redacted], percent [redacted]"
        );
        let mut value = json!({
            "a b/c": 1,
            "content": [{"type": "text", "text": "url ?token=a+b%2Fc"}],
            "n": 5, "ok": true, "nothing": null
        });
        redactor.value(&mut value);
        assert_eq!(
            value,
            json!({
                "[redacted]": 1,
                "content": [{"type": "text", "text": "url ?token=[redacted]"}],
                "n": 5, "ok": true, "nothing": null
            })
        );
    }

    #[test]
    fn redaction_matches_percent_escapes_of_any_case_and_any_subset_of_the_bytes() {
        let redactor = Redactor::new("a b/c?\u{e9}");
        for echoed in [
            "a%20b%2fc%3f%c3%a9",
            "a%20b%2Fc%3F%C3%A9",
            "a%20b%2fc%3F%c3%A9",
            "a+b/c?%c3%A9",
            "a b%2fc?\u{e9}",
            "a%20b/c%3F\u{e9}",
        ] {
            assert_eq!(
                redactor.text(&format!("url ?t={echoed}&x=1")),
                "url ?t=[redacted]&x=1",
                "{echoed}"
            );
        }
        assert_eq!(
            redactor.text("a%20b%2"),
            "a%20b%2",
            "an incomplete escape is no match"
        );
        assert_eq!(redactor.text("a%20b%2gc?\u{e9}"), "a%20b%2gc?\u{e9}");
    }

    #[test]
    fn a_credential_with_a_percent_sign_is_redacted_in_the_form_it_is_sent_and_in_other_escapings()
    {
        // `tok%!` is sent as `tok%25%21`; `a%2Fb` as `a%252Fb`.
        for (secret, echoed) in [
            ("tok%!", "tok%25%21"),
            ("tok%!", "tok%25%21".to_lowercase().as_str()),
            ("tok%!", "tok%!"),
            ("tok%!", "tok%25!"),
            ("a%2Fb", "a%252Fb"),
            ("a%2Fb", "a%252fb"),
            ("a%2Fb", "a%2Fb"),
            ("a%2Fb", "a%252F%62"),
        ] {
            let redactor = Redactor::new(secret);
            assert_eq!(
                redactor.text(&format!("url ?t={echoed}&x=1")),
                "url ?t=[redacted]&x=1",
                "{secret} as {echoed}"
            );
        }
    }

    #[test]
    fn an_empty_credential_redacts_nothing() {
        assert_eq!(Redactor::new("").text("abc"), "abc");
    }
}
