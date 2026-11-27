//! One upstream MCP server as the gateway uses it: the snapshot entry with its resolved
//! credential, how the credential is sent, and the redaction of the credential in everything that
//! comes back from the upstream (iter4-mcp-gateway design D7).

use std::borrow::Cow;
use std::fmt;

use df_proto::gateway::v1 as pb;
use hyper::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;
use url::form_urlencoded::byte_serialize;

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

/// Replaces a credential value, raw and URL-encoded, with `[redacted]`.
#[derive(Debug, Clone)]
pub(crate) struct Redactor {
    /// Longest first, so a form that contains another is replaced whole.
    forms: Vec<Secret>,
}

impl Redactor {
    fn new(secret: &str) -> Self {
        let mut forms = Vec::new();
        if !secret.is_empty() {
            let form: String = byte_serialize(secret.as_bytes()).collect();
            let percent = utf8_percent_encode(secret, NON_ALPHANUMERIC).to_string();
            for candidate in [secret.to_owned(), form, percent] {
                if !forms.iter().any(|known: &Secret| known.0 == candidate) {
                    forms.push(Secret(candidate));
                }
            }
            forms.sort_by_key(|form| std::cmp::Reverse(form.0.len()));
        }
        Self { forms }
    }

    pub(crate) fn text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut result = Cow::Borrowed(text);
        for form in &self.forms {
            if result.contains(form.0.as_str()) {
                result = Cow::Owned(result.replace(form.0.as_str(), REDACTED));
            }
        }
        result
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
    fn an_empty_credential_redacts_nothing() {
        assert_eq!(Redactor::new("").text("abc"), "abc");
    }
}
