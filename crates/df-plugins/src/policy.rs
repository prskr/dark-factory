//! Which outgoing HTTP requests a plugin instance may make, and the credential they carry.

use http::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use http::uri::{Authority, Scheme, Uri};
use secrecy::{ExposeSecret, SecretString};

/// Outgoing requests of one plugin instance: HTTPS to one authority, with the host's credential.
pub struct HttpPolicy {
    authority: Authority,
    credential: SecretString,
    /// Test servers speak plain HTTP; production policies never set this.
    allow_plain_http: bool,
}

impl HttpPolicy {
    /// Allows requests to the authority of `base_url`.
    pub fn for_base_url(base_url: &str, credential: SecretString) -> Result<Self, String> {
        let uri: Uri = base_url
            .parse()
            .map_err(|e| format!("invalid base URL `{base_url}`: {e}"))?;
        let authority = uri
            .authority()
            .cloned()
            .ok_or_else(|| format!("base URL `{base_url}` has no host"))?;
        Ok(Self {
            authority,
            credential,
            allow_plain_http: false,
        })
    }

    /// Also allows `http` (for tests against a local mock server).
    pub fn allow_plain_http(mut self) -> Self {
        self.allow_plain_http = true;
        self
    }

    /// Checks scheme and authority of `uri`, then sets `Authorization` in `headers`, replacing
    /// whatever the plugin sent.
    pub fn admit(&self, uri: &Uri, headers: &mut HeaderMap) -> Result<(), Denied> {
        let scheme = uri.scheme().cloned().unwrap_or(Scheme::HTTPS);
        if scheme != Scheme::HTTPS && !(self.allow_plain_http && scheme == Scheme::HTTP) {
            return Err(Denied::Scheme);
        }
        let target = uri.authority().ok_or(Denied::Host)?;
        let default_port = if scheme == Scheme::HTTP { 80 } else { 443 };
        let port = |a: &Authority| a.port_u16().unwrap_or(default_port);
        if !target.host().eq_ignore_ascii_case(self.authority.host())
            || port(target) != port(&self.authority)
        {
            return Err(Denied::Host);
        }
        // Forgejo token format.
        let mut value = HeaderValue::try_from(format!("token {}", self.credential.expose_secret()))
            .map_err(|_| Denied::Credential)?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
        Ok(())
    }
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    Scheme,
    Host,
    Credential,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HttpPolicy {
        HttpPolicy::for_base_url("https://git.example.org", SecretString::from("s3cret")).unwrap()
    }

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn admits_the_configured_host_and_injects_the_credential() {
        let mut headers = HeaderMap::new();
        policy()
            .admit(&uri("https://git.example.org/api/v1/x"), &mut headers)
            .unwrap();
        assert_eq!(headers[AUTHORIZATION], "token s3cret");
        assert!(headers[AUTHORIZATION].is_sensitive());
    }

    #[test]
    fn replaces_an_authorization_header_set_by_the_plugin() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer fake"));
        policy()
            .admit(&uri("https://git.example.org:443/"), &mut headers)
            .unwrap();
        assert_eq!(headers.get_all(AUTHORIZATION).iter().count(), 1);
        assert_eq!(headers[AUTHORIZATION], "token s3cret");
    }

    #[test]
    fn denies_other_hosts_ports_and_plain_http() {
        let mut headers = HeaderMap::new();
        let p = policy();
        assert_eq!(
            p.admit(&uri("https://evil.example.com/"), &mut headers),
            Err(Denied::Host)
        );
        assert_eq!(
            p.admit(&uri("https://git.example.org:8443/"), &mut headers),
            Err(Denied::Host)
        );
        assert_eq!(
            p.admit(&uri("http://git.example.org/"), &mut headers),
            Err(Denied::Scheme)
        );
        assert!(headers.is_empty());
    }

    #[test]
    fn plain_http_only_when_enabled() {
        let mut headers = HeaderMap::new();
        let p = HttpPolicy::for_base_url("http://127.0.0.1:8080", SecretString::from("t"))
            .unwrap()
            .allow_plain_http();
        p.admit(&uri("http://127.0.0.1:8080/x"), &mut headers)
            .unwrap();
    }

    #[test]
    fn rejects_base_url_without_host() {
        assert!(HttpPolicy::for_base_url("/relative", SecretString::from("t")).is_err());
    }
}
