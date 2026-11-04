//! Model providers: wire APIs, credential schemes and the rules a provider configuration must
//! meet (iter3-inference-gateway, `specs/model-providers`).

use std::fmt;
use std::str::FromStr;

use thiserror::Error;
use url::Url;

use crate::domain::ParseEnumError;
use crate::header_template::{self, HeaderError};

/// The API protocol of a model endpoint. The gateway forwards each one unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WireApi {
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
}

impl WireApi {
    pub const ALL: [Self; 3] = [
        Self::AnthropicMessages,
        Self::OpenAiChat,
        Self::OpenAiResponses,
    ];

    /// The stored and displayed identifier (`model_provider_apis.wire_api`, metric attribute).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenAiChat => "openai-chat",
            Self::OpenAiResponses => "openai-responses",
        }
    }

    /// Name for the UI.
    pub const fn label(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "Anthropic Messages",
            Self::OpenAiChat => "OpenAI Chat Completions",
            Self::OpenAiResponses => "OpenAI Responses",
        }
    }
}

impl fmt::Display for WireApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for WireApi {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|api| api.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "WireApi",
                value: s.to_string(),
            })
    }
}

/// How the gateway presents the provider credential to the upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialScheme {
    /// `x-api-key: <value>`
    XApiKey,
    /// `Authorization: Bearer <value>`
    Bearer,
    /// `<name>: <prefix><value>`
    Header,
    /// `Authorization: Bearer <value>`, Anthropic Messages and Claude Code only.
    ClaudeSubscription,
}

impl CredentialScheme {
    pub const ALL: [Self; 4] = [
        Self::XApiKey,
        Self::Bearer,
        Self::Header,
        Self::ClaudeSubscription,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::XApiKey => "x-api-key",
            Self::Bearer => "bearer",
            Self::Header => "header",
            Self::ClaudeSubscription => "claude-subscription",
        }
    }

    /// The request header the scheme sets, as written in the HTTP spec: fixed, or `custom` for
    /// [`Self::Header`] (`None` without a name). Header names are case-insensitive.
    pub fn header_name(self, custom: Option<&str>) -> Option<&str> {
        match self {
            Self::XApiKey => Some("x-api-key"),
            Self::Bearer | Self::ClaudeSubscription => Some("Authorization"),
            Self::Header => custom.filter(|name| !name.is_empty()),
        }
    }

    /// The text in front of the credential in the header value, exactly as sent (`Bearer ` with
    /// its space); `None` for [`Self::Header`], where the provider sets it.
    pub const fn fixed_prefix(self) -> Option<&'static str> {
        match self {
            Self::XApiKey => Some(""),
            Self::Bearer | Self::ClaudeSubscription => Some("Bearer "),
            Self::Header => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::XApiKey => "x-api-key header",
            Self::Bearer => "Authorization: Bearer",
            Self::Header => "Custom header",
            Self::ClaudeSubscription => "Claude subscription",
        }
    }
}

impl fmt::Display for CredentialScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CredentialScheme {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|scheme| scheme.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "CredentialScheme",
                value: s.to_string(),
            })
    }
}

impl serde::Serialize for WireApi {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl serde::Serialize for CredentialScheme {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// An upstream base URL per wire API.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProviderApi {
    pub wire_api: WireApi,
    pub upstream_url: String,
}

/// An extra header: a name and a template over the values of a run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeaderTemplate {
    pub name: String,
    pub template: String,
}

/// The configuration of a model provider that is checked as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    pub apis: Vec<ProviderApi>,
    pub scheme: CredentialScheme,
    /// Header name of [`CredentialScheme::Header`].
    pub credential_header: Option<String>,
    pub headers: Vec<HeaderTemplate>,
}

/// Why a provider configuration is invalid. The `field` names the form field it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderError {
    #[error("Offer at least one wire API.")]
    NoApi,
    #[error("{0} is offered twice.")]
    DuplicateApi(WireApi),
    #[error("The upstream URL of {0} must be an HTTPS URL, for example https://api.example.org.")]
    InvalidUrl(WireApi),
    #[error("A Claude subscription can only be used with Anthropic Messages.")]
    SubscriptionNeedsAnthropic,
    #[error("Extra headers are not available with the Claude subscription scheme.")]
    SubscriptionNoHeaders,
    #[error("A custom header scheme needs a header name.")]
    CredentialHeaderMissing,
    #[error("Credential header: {0}")]
    CredentialHeader(HeaderError),
    #[error("Header `{name}`: {error}")]
    Header { name: String, error: HeaderError },
    #[error("Header `{0}` is set twice.")]
    DuplicateHeader(String),
}

impl ProviderConfig {
    /// Checks the configuration; the first problem found is returned.
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.apis.is_empty() {
            return Err(ProviderError::NoApi);
        }
        let mut seen: Vec<WireApi> = Vec::new();
        for api in &self.apis {
            if seen.contains(&api.wire_api) {
                return Err(ProviderError::DuplicateApi(api.wire_api));
            }
            seen.push(api.wire_api);
            if !is_https_url(&api.upstream_url) {
                return Err(ProviderError::InvalidUrl(api.wire_api));
            }
        }
        if self.scheme == CredentialScheme::ClaudeSubscription
            && self
                .apis
                .iter()
                .any(|api| api.wire_api != WireApi::AnthropicMessages)
        {
            return Err(ProviderError::SubscriptionNeedsAnthropic);
        }
        if self.scheme == CredentialScheme::ClaudeSubscription && !self.headers.is_empty() {
            return Err(ProviderError::SubscriptionNoHeaders);
        }
        let credential_header = self.credential_header()?;
        let mut seen: Vec<String> = Vec::new();
        for header in &self.headers {
            let lower = header.name.to_ascii_lowercase();
            header_template::validate(&header.name, &header.template, credential_header.as_deref())
                .map_err(|error| ProviderError::Header {
                    name: header.name.clone(),
                    error,
                })?;
            if seen.contains(&lower) {
                return Err(ProviderError::DuplicateHeader(header.name.clone()));
            }
            seen.push(lower);
        }
        Ok(())
    }

    /// The header the scheme sets, in lower case.
    fn credential_header(&self) -> Result<Option<String>, ProviderError> {
        let name = self
            .scheme
            .header_name(self.credential_header.as_deref())
            .ok_or(ProviderError::CredentialHeaderMissing)?;
        if self.scheme == CredentialScheme::Header {
            header_template::validate_credential_name(name)
                .map_err(ProviderError::CredentialHeader)?;
        }
        Ok(Some(name.to_ascii_lowercase()))
    }

    /// The wire APIs the provider offers.
    pub fn wire_apis(&self) -> Vec<WireApi> {
        self.apis.iter().map(|api| api.wire_api).collect()
    }
}

/// Whether `value` is an `https` URL with a host and no credentials, query or fragment.
pub fn is_https_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ProviderConfig {
        ProviderConfig {
            apis: vec![ProviderApi {
                wire_api: WireApi::AnthropicMessages,
                upstream_url: "https://api.anthropic.com".into(),
            }],
            scheme: CredentialScheme::XApiKey,
            credential_header: None,
            headers: Vec::new(),
        }
    }

    fn header(name: &str, template: &str) -> HeaderTemplate {
        HeaderTemplate {
            name: name.into(),
            template: template.into(),
        }
    }

    #[test]
    fn valid_provider_passes() {
        assert_eq!(config().validate(), Ok(()));
    }

    #[test]
    fn plain_http_upstream_is_rejected() {
        let mut config = config();
        config.apis[0].upstream_url = "http://llm.internal".into();
        assert_eq!(
            config.validate(),
            Err(ProviderError::InvalidUrl(WireApi::AnthropicMessages))
        );
    }

    #[test]
    fn upstream_url_with_credentials_or_query_is_rejected() {
        for url in ["https://u:p@x.test", "https://x.test?a=1", "not a url"] {
            let mut config = config();
            config.apis[0].upstream_url = url.into();
            assert!(config.validate().is_err(), "{url}");
        }
    }

    #[test]
    fn claude_subscription_with_another_api_is_rejected() {
        let mut config = config();
        config.scheme = CredentialScheme::ClaudeSubscription;
        config.apis.push(ProviderApi {
            wire_api: WireApi::OpenAiChat,
            upstream_url: "https://api.openai.com".into(),
        });
        assert_eq!(
            config.validate(),
            Err(ProviderError::SubscriptionNeedsAnthropic)
        );
    }

    #[test]
    fn claude_subscription_with_extra_headers_is_rejected() {
        let mut config = config();
        config.scheme = CredentialScheme::ClaudeSubscription;
        config.headers = vec![header("x-session", "{{ task_id | sha256 }}")];
        assert_eq!(config.validate(), Err(ProviderError::SubscriptionNoHeaders));
    }

    #[test]
    fn claude_subscription_without_extra_headers_is_accepted() {
        let mut config = config();
        config.scheme = CredentialScheme::ClaudeSubscription;
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn header_scheme_named_host_is_rejected() {
        let mut config = config();
        config.scheme = CredentialScheme::Header;
        config.credential_header = Some("Host".into());
        assert!(matches!(
            config.validate(),
            Err(ProviderError::CredentialHeader(HeaderError::ReservedName(
                _
            )))
        ));
    }

    #[test]
    fn header_scheme_may_set_api_key_or_authorization() {
        for name in ["api-key", "Authorization", "x-api-key"] {
            let mut config = config();
            config.scheme = CredentialScheme::Header;
            config.credential_header = Some(name.into());
            assert_eq!(config.validate(), Ok(()), "{name}");
        }
    }

    #[test]
    fn schemes_fix_the_prefix_except_the_custom_header() {
        assert_eq!(CredentialScheme::XApiKey.fixed_prefix(), Some(""));
        assert_eq!(CredentialScheme::Bearer.fixed_prefix(), Some("Bearer "));
        assert_eq!(
            CredentialScheme::ClaudeSubscription.fixed_prefix(),
            Some("Bearer ")
        );
        assert_eq!(CredentialScheme::Header.fixed_prefix(), None);
    }

    #[test]
    fn header_scheme_name_comes_from_the_scheme_or_the_custom_name() {
        assert_eq!(
            CredentialScheme::XApiKey.header_name(None),
            Some("x-api-key")
        );
        assert_eq!(
            CredentialScheme::ClaudeSubscription.header_name(Some("ignored")),
            Some("Authorization")
        );
        assert_eq!(
            CredentialScheme::Header.header_name(Some("api-key")),
            Some("api-key")
        );
        assert_eq!(CredentialScheme::Header.header_name(Some("")), None);
        assert_eq!(CredentialScheme::Header.header_name(None), None);
    }

    #[test]
    fn header_scheme_needs_a_name() {
        let mut config = config();
        config.scheme = CredentialScheme::Header;
        assert_eq!(
            config.validate(),
            Err(ProviderError::CredentialHeaderMissing)
        );
    }

    #[test]
    fn extra_header_colliding_with_the_credential_header_is_rejected() {
        let mut config = config();
        config.headers = vec![header("X-Api-Key", "x")];
        assert!(matches!(
            config.validate(),
            Err(ProviderError::Header {
                error: HeaderError::ReservedName(_),
                ..
            })
        ));
    }

    #[test]
    fn extra_header_named_like_the_custom_credential_header_is_rejected() {
        let mut config = config();
        config.scheme = CredentialScheme::Header;
        config.credential_header = Some("api-token".into());
        config.headers = vec![header("API-Token", "x")];
        assert!(config.validate().is_err());
    }

    #[test]
    fn invalid_template_is_rejected_and_names_the_filter() {
        let mut config = config();
        config.headers = vec![header("x-session", "{{ task_id | md5 }}")];
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("md5"), "{error}");
    }

    #[test]
    fn duplicate_header_and_api_are_rejected() {
        let mut twice = config();
        twice.headers = vec![header("x-a", "1"), header("X-A", "2")];
        assert!(matches!(
            twice.validate(),
            Err(ProviderError::DuplicateHeader(_))
        ));
        let mut apis = config();
        apis.apis.push(apis.apis[0].clone());
        assert!(matches!(
            apis.validate(),
            Err(ProviderError::DuplicateApi(_))
        ));
    }

    #[test]
    fn provider_without_api_is_rejected() {
        let mut config = config();
        config.apis.clear();
        assert_eq!(config.validate(), Err(ProviderError::NoApi));
    }

    #[test]
    fn enums_round_trip_through_their_identifiers() {
        for api in WireApi::ALL {
            assert_eq!(api.as_str().parse::<WireApi>(), Ok(api));
        }
        for scheme in CredentialScheme::ALL {
            assert_eq!(scheme.as_str().parse::<CredentialScheme>(), Ok(scheme));
        }
        assert!("grpc".parse::<WireApi>().is_err());
    }
}
