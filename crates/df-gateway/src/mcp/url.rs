//! The URL of an upstream MCP server (iter4-mcp-gateway design D7): unlike the inference and Git
//! upstream URLs it keeps path and query, and its `Debug` never prints the query.

use std::fmt;

use url::Url;

/// An absolute `https` URL (`http` with `network.allowPlainHttp`) with a host, without user
/// information or fragment. The query may hold the server's own parameters and, with the `query`
/// credential scheme, the credential.
#[derive(Clone)]
pub(crate) struct McpUrl(Url);

impl McpUrl {
    pub(crate) fn parse(value: &str, allow_plain_http: bool) -> Option<Self> {
        let url = Url::parse(value).ok()?;
        let scheme_allowed = match url.scheme() {
            "https" => true,
            "http" => allow_plain_http,
            _ => false,
        };
        let usable = scheme_allowed
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none();
        usable.then_some(Self(url))
    }

    /// The URL with `name=value` (URL-encoded) after its own parameters.
    #[must_use]
    pub(crate) fn with_query_param(&self, name: &str, value: &str) -> Self {
        let mut url = self.0.clone();
        url.query_pairs_mut().append_pair(name, value);
        Self(url)
    }

    pub(crate) fn is_https(&self) -> bool {
        self.0.scheme() == "https"
    }

    /// The host without IPv6 brackets.
    pub(crate) fn host(&self) -> &str {
        self.0
            .host_str()
            .unwrap_or_default()
            .trim_matches(['[', ']'])
    }

    pub(crate) fn port(&self) -> u16 {
        self.0
            .port_or_known_default()
            .unwrap_or(if self.is_https() { 443 } else { 80 })
    }

    /// `host[:port]` as written in the URL, for the `Host` header.
    pub(crate) fn authority(&self) -> String {
        let host = self.0.host_str().unwrap_or_default();
        self.0
            .port()
            .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"))
    }

    /// Path and query, as sent in the request line.
    pub(crate) fn request_target(&self) -> String {
        self.0.query().map_or_else(
            || self.0.path().to_owned(),
            |query| format!("{}?{query}", self.0.path()),
        )
    }
}

impl fmt::Debug for McpUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}://{}{}",
            self.0.scheme(),
            self.authority(),
            self.0.path()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_and_query_are_kept_and_the_credential_comes_last_url_encoded() {
        let url = McpUrl::parse(
            "https://design.example.com/mcp/stream?file=abc&mode=read",
            false,
        )
        .unwrap();
        assert_eq!(url.request_target(), "/mcp/stream?file=abc&mode=read");
        let with = url.with_query_param("token", "a b&c=d/é");
        assert_eq!(
            with.request_target(),
            "/mcp/stream?file=abc&mode=read&token=a+b%26c%3Dd%2F%C3%A9"
        );
        let bare = McpUrl::parse("https://h.example", false).unwrap();
        assert_eq!(bare.with_query_param("t", "v").request_target(), "/?t=v");
    }

    #[test]
    fn debug_prints_scheme_host_and_path_only() {
        let url = McpUrl::parse("https://h.example:8443/mcp?token=sekret&file=abc", false)
            .unwrap()
            .with_query_param("extra", "sekret2");
        let debug = format!("{url:?}");
        assert_eq!(debug, "https://h.example:8443/mcp");
    }

    #[test]
    fn unusable_urls_are_refused() {
        for value in [
            "https://user:pw@h.example/mcp",
            "https://h.example/mcp#x",
            "http://h.example/mcp",
            "ftp://h.example/",
            "not a url",
        ] {
            assert!(McpUrl::parse(value, false).is_none(), "{value}");
        }
        assert!(McpUrl::parse("http://h.example/mcp", true).is_some());
    }

    #[test]
    fn host_port_and_authority_follow_the_url() {
        let url = McpUrl::parse("https://[::1]:8443/mcp", false).unwrap();
        assert_eq!(
            (url.host(), url.port(), url.authority()),
            ("::1", 8443, "[::1]:8443".into())
        );
        let url = McpUrl::parse("https://h.example/mcp", false).unwrap();
        assert_eq!(
            (url.host(), url.port(), url.authority()),
            ("h.example", 443, "h.example".into())
        );
        assert_eq!(McpUrl::parse("http://h.example/", true).unwrap().port(), 80);
    }
}
