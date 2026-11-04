use df_proto::gateway::v1::{CredentialScheme, Header};

use super::*;

fn route(scheme: CredentialScheme) -> InferenceRoute {
    InferenceRoute {
        scheme: scheme.into(),
        ..InferenceRoute::default()
    }
}

fn client(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    pairs
        .iter()
        .map(|(name, value)| {
            (
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            )
        })
        .collect()
}

fn values(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(name)
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect()
}

#[test]
fn placeholder_key_never_reaches_the_upstream() {
    let sent = client(&[
        ("x-api-key", "df-placeholder"),
        ("authorization", "Bearer df-placeholder"),
        ("api-key", "df-placeholder"),
    ]);

    let headers = upstream_headers(&sent, &route(CredentialScheme::XApiKey), "sk-real").unwrap();

    assert_eq!(values(&headers, "x-api-key"), ["sk-real"]);
    assert!(headers.get("authorization").is_none());
    assert!(headers.get("api-key").is_none());
    assert!(!format!("{headers:?}").contains("df-placeholder"));
}

#[test]
fn claude_subscription_sends_a_bearer_token_and_no_api_key() {
    let sent = client(&[("x-api-key", "df-placeholder")]);

    let headers =
        upstream_headers(&sent, &route(CredentialScheme::ClaudeSubscription), "oauth").unwrap();

    assert_eq!(values(&headers, "authorization"), ["Bearer oauth"]);
    assert!(headers.get("x-api-key").is_none());
}

#[test]
fn custom_header_scheme_uses_name_and_prefix() {
    let mut custom = route(CredentialScheme::Header);
    custom.credential_header = "api-key".into();
    let without_prefix = upstream_headers(&HeaderMap::new(), &custom, "k").unwrap();
    assert_eq!(values(&without_prefix, "api-key"), ["k"]);

    custom.credential_header = "x-token".into();
    custom.credential_prefix = "Token ".into();
    let sent = client(&[("x-token", "df-placeholder")]);
    let with_prefix = upstream_headers(&sent, &custom, "k").unwrap();
    assert_eq!(values(&with_prefix, "x-token"), ["Token k"]);
}

#[test]
fn provider_header_replaces_the_clients_of_the_same_name() {
    let mut with_header = route(CredentialScheme::Bearer);
    with_header.headers = vec![Header {
        name: "x-opencode-session".into(),
        value: "3f1c".into(),
    }];
    let sent = client(&[("x-opencode-session", "abc")]);

    let headers = upstream_headers(&sent, &with_header, "k").unwrap();

    assert_eq!(values(&headers, "x-opencode-session"), ["3f1c"]);
}

#[test]
fn a_provider_header_cannot_replace_the_credential() {
    let mut with_header = route(CredentialScheme::XApiKey);
    with_header.headers = vec![
        Header {
            name: "x-api-key".into(),
            value: "evil".into(),
        },
        Header {
            name: "host".into(),
            value: "evil.test".into(),
        },
    ];

    let headers = upstream_headers(&HeaderMap::new(), &with_header, "sk-real").unwrap();

    assert_eq!(values(&headers, "x-api-key"), ["sk-real"]);
    assert!(headers.get("host").is_none());
}

#[test]
fn harness_headers_pass_through() {
    let sent = client(&[
        ("anthropic-beta", "oauth-2025-04-20,claude-code-20250219"),
        ("anthropic-version", "2023-06-01"),
        ("content-type", "application/json"),
    ]);

    let headers = upstream_headers(&sent, &route(CredentialScheme::Bearer), "k").unwrap();

    assert_eq!(
        values(&headers, "anthropic-beta"),
        ["oauth-2025-04-20,claude-code-20250219"]
    );
    assert_eq!(values(&headers, "anthropic-version"), ["2023-06-01"]);
    assert_eq!(values(&headers, "content-type"), ["application/json"]);
}

#[test]
fn connection_headers_host_and_length_are_dropped_and_compression_is_refused() {
    let sent = client(&[
        ("connection", "keep-alive"),
        ("transfer-encoding", "chunked"),
        ("host", "gateway:8443"),
        ("content-length", "12"),
        ("accept-encoding", "gzip, br"),
    ]);

    let headers = upstream_headers(&sent, &route(CredentialScheme::Bearer), "k").unwrap();

    for name in ["connection", "transfer-encoding", "host", "content-length"] {
        assert!(headers.get(name).is_none(), "{name}");
    }
    assert_eq!(values(&headers, "accept-encoding"), ["identity"]);
}

#[test]
fn the_credential_is_marked_sensitive_for_debug_output() {
    let headers = upstream_headers(
        &HeaderMap::new(),
        &route(CredentialScheme::XApiKey),
        "sk-real",
    )
    .unwrap();
    assert!(!format!("{headers:?}").contains("sk-real"));
}

#[test]
fn an_unusable_credential_is_an_error() {
    let unspecified = route(CredentialScheme::Unspecified);
    assert_eq!(
        upstream_headers(&HeaderMap::new(), &unspecified, "k"),
        Err(CredentialUnusable)
    );
    assert_eq!(
        upstream_headers(
            &HeaderMap::new(),
            &route(CredentialScheme::Bearer),
            "line\nbreak"
        ),
        Err(CredentialUnusable)
    );
}
