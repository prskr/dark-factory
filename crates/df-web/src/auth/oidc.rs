//! OIDC relying party: authorization code flow with PKCE, lazy refresh and end-session URL.
//!
//! The provider is discovered on every operation, so rotated signing keys are picked up without
//! caching rules. Roles are read from the payload of an ID token that `openidconnect` has verified.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use df_core::AuthConfig;
use openidconnect::core::CoreClient;
use openidconnect::reqwest;
use openidconnect::url::Url;
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, ProviderMetadataWithLogout, RedirectUrl, RefreshToken, Scope, TokenResponse,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::now_unix;

/// Seconds a token is assumed to be valid if the provider does not say.
const DEFAULT_TOKEN_LIFETIME: i64 = 300;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("invalid auth configuration: {0}")]
    Config(String),
    #[error("provider discovery failed: {0}")]
    Discovery(String),
    #[error("token request failed: {0}")]
    Token(String),
    #[error("ID token rejected: {0}")]
    IdToken(String),
    #[error("the provider returned no ID token")]
    MissingIdToken,
}

/// State kept in the session between the redirect to the provider and the callback.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingLogin {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
    /// Local path to return to after the login.
    pub next: String,
}

/// Verified identity and tokens of a login or refresh.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub issuer: String,
    pub subject: String,
    pub display_name: String,
    pub roles: Vec<String>,
    pub id_token: String,
    pub refresh_token: Option<String>,
    /// Unix time at which the access token expires.
    pub expires_at: i64,
}

// Tokens and the PKCE verifier must never reach logs, so `Debug` leaves them out.
impl std::fmt::Debug for PendingLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingLogin")
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("issuer", &self.issuer)
            .field("subject", &self.subject)
            .field("roles", &self.roles)
            .finish_non_exhaustive()
    }
}

pub struct OidcClient {
    issuer: IssuerUrl,
    client_id: ClientId,
    secret: ClientSecret,
    redirect: RedirectUrl,
    public_url: String,
    roles_claim: String,
    extra_scopes: Vec<String>,
    http: reqwest::Client,
}

impl OidcClient {
    /// Builds the client from the configuration and reads the client secret file.
    pub fn new(cfg: &AuthConfig) -> Result<Self, OidcError> {
        let secret = cfg
            .read_client_secret()
            .map_err(|e| OidcError::Config(e.to_string()))?;
        let public_url = cfg.public_url.trim_end_matches('/').to_string();
        // Following redirects would open the token requests to SSRF.
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| OidcError::Config(e.to_string()))?;
        Ok(Self {
            issuer: IssuerUrl::new(cfg.issuer_url.clone())
                .map_err(|e| OidcError::Config(format!("issuerUrl: {e}")))?,
            client_id: ClientId::new(cfg.client_id.clone()),
            secret: ClientSecret::new(secret.expose_secret().to_string()),
            redirect: RedirectUrl::new(format!("{public_url}/auth/callback"))
                .map_err(|e| OidcError::Config(format!("publicUrl: {e}")))?,
            public_url,
            roles_claim: cfg.roles_claim.clone(),
            extra_scopes: cfg.extra_scopes.clone(),
            http,
        })
    }

    /// Whether the session cookie gets the `Secure` flag: always, unless `publicUrl` is plain
    /// `http://` (local development without TLS).
    pub fn secure_cookies(&self) -> bool {
        self.redirect.url().scheme() != "http"
    }

    async fn discover(&self) -> Result<ProviderMetadataWithLogout, OidcError> {
        ProviderMetadataWithLogout::discover_async(self.issuer.clone(), &self.http)
            .await
            .map_err(|e| OidcError::Discovery(e.to_string()))
    }

    async fn client(&self) -> Result<DiscoveredClient, OidcError> {
        Ok(CoreClient::from_provider_metadata(
            self.discover().await?,
            self.client_id.clone(),
            Some(self.secret.clone()),
        )
        .set_redirect_uri(self.redirect.clone()))
    }

    /// Builds the URL to send the browser to and the state to keep in the session.
    pub async fn authorize(&self, next: &str) -> Result<(Url, PendingLogin), OidcError> {
        let client = self.client().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                AuthenticationFlow::<openidconnect::core::CoreResponseType>::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("openid".into()))
            .add_scope(Scope::new("profile".into()))
            .add_scopes(self.extra_scopes.iter().cloned().map(Scope::new))
            .set_pkce_challenge(challenge)
            .url();
        Ok((
            url,
            PendingLogin {
                state: state.secret().clone(),
                nonce: nonce.secret().clone(),
                pkce_verifier: verifier.secret().clone(),
                next: next.to_string(),
            },
        ))
    }

    /// Exchanges the authorization code and verifies the ID token (signature, issuer, audience,
    /// expiry, nonce).
    pub async fn complete_login(
        &self,
        code: String,
        pending: &PendingLogin,
    ) -> Result<Identity, OidcError> {
        let client = self.client().await?;
        let token = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|e| OidcError::Token(e.to_string()))?
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier.clone()))
            .request_async(&self.http)
            .await
            .map_err(|e| OidcError::Token(e.to_string()))?;
        let id_token = token.id_token().ok_or(OidcError::MissingIdToken)?;
        let nonce = Nonce::new(pending.nonce.clone());
        let claims = id_token
            .claims(&client.id_token_verifier(), &nonce)
            .map_err(|e| OidcError::IdToken(e.to_string()))?;
        Ok(self.identity(
            claims.issuer().to_string(),
            claims.subject().to_string(),
            id_token.to_string(),
            token.refresh_token().map(|t| t.secret().clone()),
            token.expires_in(),
        ))
    }

    /// Refreshes the tokens; roles are read again from the new ID token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Identity, OidcError> {
        let client = self.client().await?;
        let refresh_token = RefreshToken::new(refresh_token.to_string());
        let token = client
            .exchange_refresh_token(&refresh_token)
            .map_err(|e| OidcError::Token(e.to_string()))?
            .request_async(&self.http)
            .await
            .map_err(|e| OidcError::Token(e.to_string()))?;
        let id_token = token.id_token().ok_or(OidcError::MissingIdToken)?;
        // A refreshed ID token carries no nonce.
        let claims = id_token
            .claims(&client.id_token_verifier(), |_: Option<&Nonce>| Ok(()))
            .map_err(|e| OidcError::IdToken(e.to_string()))?;
        let new_refresh = token
            .refresh_token()
            .map(|t| t.secret().clone())
            .unwrap_or_else(|| refresh_token.secret().clone());
        Ok(self.identity(
            claims.issuer().to_string(),
            claims.subject().to_string(),
            id_token.to_string(),
            Some(new_refresh),
            token.expires_in(),
        ))
    }

    fn identity(
        &self,
        issuer: String,
        subject: String,
        id_token: String,
        refresh_token: Option<String>,
        expires_in: Option<Duration>,
    ) -> Identity {
        let payload = jwt_payload(&id_token).unwrap_or_default();
        let display_name = ["name", "preferred_username", "email"]
            .iter()
            .find_map(|k| payload.get(*k).and_then(|v| v.as_str()))
            .unwrap_or(&subject)
            .to_string();
        let lifetime = expires_in.map_or(DEFAULT_TOKEN_LIFETIME, |d| {
            i64::try_from(d.as_secs()).unwrap_or(DEFAULT_TOKEN_LIFETIME)
        });
        Identity {
            roles: roles_from_payload(&payload, &self.roles_claim),
            issuer,
            subject,
            display_name,
            id_token,
            refresh_token,
            expires_at: now_unix() + lifetime,
        }
    }

    /// End-session URL of the provider (RP-initiated logout) or `None` if it has none.
    pub async fn end_session_url(&self, id_token: &str) -> Result<Option<Url>, OidcError> {
        let metadata = self.discover().await?;
        let Some(endpoint) = metadata.additional_metadata().end_session_endpoint.clone() else {
            return Ok(None);
        };
        let mut url = endpoint.url().clone();
        url.query_pairs_mut()
            .append_pair("id_token_hint", id_token)
            .append_pair("client_id", self.client_id.as_str())
            .append_pair("post_logout_redirect_uri", &format!("{}/", self.public_url));
        Ok(Some(url))
    }
}

type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Payload of a JWT whose signature has already been verified.
fn jwt_payload(jwt: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let payload = jwt.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

/// The flat role list of `claim`; a single string counts as a list of one.
fn roles_from_payload(
    payload: &serde_json::Map<String, serde_json::Value>,
    claim: &str,
) -> Vec<String> {
    match payload.get(claim) {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(role)) => vec![role.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().cloned().unwrap()
    }

    #[test]
    fn roles_come_from_the_configured_flat_claim() {
        let p = payload(
            serde_json::json!({"roles": ["board-member", "x"], "groups": ["g"], "one": "solo", "n": 3}),
        );
        assert_eq!(roles_from_payload(&p, "roles"), ["board-member", "x"]);
        assert_eq!(roles_from_payload(&p, "groups"), ["g"]);
        assert_eq!(roles_from_payload(&p, "one"), ["solo"]);
        assert!(roles_from_payload(&p, "n").is_empty());
        assert!(roles_from_payload(&p, "missing").is_empty());
    }

    #[tokio::test]
    async fn authorize_requests_the_extra_scopes() {
        let idp = crate::testkit::Idp::start().await;
        let dir = tempfile::tempdir().unwrap();
        let secret_file = dir.path().join("secret");
        std::fs::write(&secret_file, "s3cret\n").unwrap();
        let client = OidcClient::new(&AuthConfig {
            issuer_url: idp.server.uri(),
            client_id: "dark-factory".into(),
            client_secret_file: secret_file,
            public_url: "https://factory.test".into(),
            roles_claim: "groups".into(),
            extra_scopes: vec!["groups".into(), "offline_access".into()],
        })
        .unwrap();

        let (url, _) = client.authorize("/tasks").await.unwrap();

        let scope = url
            .query_pairs()
            .find(|(k, _)| k == "scope")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let scopes: Vec<&str> = scope.split(' ').collect();
        assert!(
            scopes.ends_with(&["profile", "groups", "offline_access"]),
            "{scope}"
        );
    }

    #[test]
    fn cookies_are_secure_unless_the_public_url_is_plain_http() {
        let dir = tempfile::tempdir().unwrap();
        let secret_file = dir.path().join("secret");
        std::fs::write(&secret_file, "s3cret\n").unwrap();
        let client = |public_url: &str| {
            OidcClient::new(&AuthConfig {
                issuer_url: "https://idp.test".into(),
                client_id: "dark-factory".into(),
                client_secret_file: secret_file.clone(),
                public_url: public_url.into(),
                roles_claim: "roles".into(),
                extra_scopes: Vec::new(),
            })
            .unwrap()
        };
        assert!(client("https://factory.test").secure_cookies());
        assert!(client("HTTPS://factory.test").secure_cookies());
        assert!(!client("http://127.0.0.1:8080").secure_cookies());
    }

    #[test]
    fn debug_output_hides_tokens_and_the_pkce_verifier() {
        let identity = Identity {
            issuer: "i".into(),
            subject: "s".into(),
            display_name: "n".into(),
            roles: Vec::new(),
            id_token: "ID-TOKEN-VALUE".into(),
            refresh_token: Some("REFRESH-VALUE".into()),
            expires_at: 0,
        };
        let pending = PendingLogin {
            state: "st".into(),
            nonce: "no".into(),
            pkce_verifier: "VERIFIER-VALUE".into(),
            next: "/".into(),
        };
        let out = format!("{identity:?} {pending:?}");
        for secret in ["ID-TOKEN-VALUE", "REFRESH-VALUE", "VERIFIER-VALUE"] {
            assert!(!out.contains(secret), "{out}");
        }
    }

    #[test]
    fn jwt_payload_decodes_the_middle_segment() {
        let body = URL_SAFE_NO_PAD.encode(br#"{"sub":"a"}"#);
        assert_eq!(jwt_payload(&format!("h.{body}.s")).unwrap()["sub"], "a");
        assert!(jwt_payload("garbage").is_none());
    }
}
