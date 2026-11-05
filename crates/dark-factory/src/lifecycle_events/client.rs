//! The HTTP clients of lifecycle event delivery (`lifecycle-events` design D4).
//!
//! Project subscriptions are untrusted: their client resolves through [`PublicOnly`], which
//! refuses a host with any internal address, so the address checked is the address connected
//! to. A literal IP URL
//! skips the resolver and is checked in [`Clients::post`]. Instance subscriptions come from the
//! operator and may reach anything. Neither client follows redirects: a `3xx` is a failed attempt.

use std::error::Error;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use df_core::public_address::{ProjectTargets, is_public, literal_host_is_public};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use reqwest::{StatusCode, Url, redirect};

/// Total time of one attempt, connect included.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Why an attempt got no HTTP status.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PostError {
    #[error("address refused: {0} is not a public address")]
    Refused(String),
    #[error(transparent)]
    Transport(reqwest::Error),
}

/// Which client sends an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trust {
    /// A project subscription: public addresses only, no proxy.
    Project,
    /// An instance subscription from the configuration: any address.
    Instance,
}

/// One client per [`Trust`]. With [`ProjectTargets::Unrestricted`] (development build) project
/// subscriptions use the instance client.
#[derive(Debug, Clone)]
pub(crate) struct Clients {
    project: reqwest::Client,
    instance: reqwest::Client,
    project_targets: ProjectTargets,
}

impl Clients {
    pub(crate) fn new(project_targets: ProjectTargets) -> Result<Self, reqwest::Error> {
        let project = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .redirect(redirect::Policy::none())
            .no_proxy()
            .dns_resolver(Arc::new(PublicOnly {
                lookup: Arc::new(SystemLookup),
            }))
            .build()?;
        let instance = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .redirect(redirect::Policy::none())
            .build()?;
        Ok(Self {
            project,
            instance,
            project_targets,
        })
    }

    /// POSTs `body` as JSON with `headers` and returns the response status, whatever it is.
    pub(crate) async fn post(
        &self,
        trust: Trust,
        url: &Url,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<StatusCode, PostError> {
        let trust = match self.project_targets {
            ProjectTargets::PublicHttps => trust,
            ProjectTargets::Unrestricted => Trust::Instance,
        };
        let client = match trust {
            Trust::Project if !literal_host_is_public(url) => {
                return Err(PostError::Refused(
                    url.host_str().unwrap_or_default().into(),
                ));
            }
            Trust::Project => &self.project,
            Trust::Instance => &self.instance,
        };
        let response = client
            .post(url.clone())
            .headers(headers)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(classify)?;
        Ok(response.status())
    }
}

/// Resolves a host name to its addresses; [`SystemLookup`] in production.
#[async_trait]
trait Lookup: Send + Sync {
    async fn lookup(&self, host: &str) -> io::Result<Vec<SocketAddr>>;
}

/// The system resolver.
#[derive(Debug)]
struct SystemLookup;

#[async_trait]
impl Lookup for SystemLookup {
    async fn lookup(&self, host: &str) -> io::Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host((host, 0)).await?.collect())
    }
}

/// Resolves a name on every connect and refuses it with [`AddressRefused`] unless every address
/// is public; the client connects only to the addresses checked.
struct PublicOnly {
    lookup: Arc<dyn Lookup>,
}

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let lookup = self.lookup.clone();
        Box::pin(async move {
            let addrs = lookup.lookup(&host).await?;
            if addrs.is_empty() || !addrs.iter().all(|addr| is_public(addr.ip())) {
                return Err(Box::new(AddressRefused(host)) as Box<dyn Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0} resolves to an address that is not public")]
struct AddressRefused(String);

/// [`PostError::Refused`] when the resolver refused the host, otherwise the transport error.
fn classify(err: reqwest::Error) -> PostError {
    let mut source = err.source();
    while let Some(cause) = source {
        if let Some(AddressRefused(host)) = cause.downcast_ref::<AddressRefused>() {
            return PostError::Refused(host.clone());
        }
        source = cause.source();
    }
    PostError::Transport(err)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use axum::Router;
    use axum::response::Redirect;
    use axum::routing::post;
    use tokio::net::TcpListener;

    use super::*;

    /// Answers each lookup with the next address list.
    struct Rebinding(Mutex<VecDeque<Vec<SocketAddr>>>);

    #[async_trait]
    impl Lookup for Rebinding {
        async fn lookup(&self, _host: &str) -> io::Result<Vec<SocketAddr>> {
            Ok(self.0.lock().unwrap().pop_front().unwrap_or_default())
        }
    }

    async fn resolve(resolver: &PublicOnly, host: &str) -> Result<Vec<SocketAddr>, String> {
        let name: Name = host.parse().unwrap();
        resolver
            .resolve(name)
            .await
            .map(Iterator::collect)
            .map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn a_name_rebound_to_an_internal_address_is_refused_on_the_next_connect() {
        let public: SocketAddr = "93.184.215.14:0".parse().unwrap();
        let internal: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let resolver = PublicOnly {
            lookup: Arc::new(Rebinding(Mutex::new(VecDeque::from([
                vec![public],
                vec![internal],
                vec![public, internal],
            ])))),
        };

        assert_eq!(
            resolve(&resolver, "hooks.example.org").await,
            Ok(vec![public])
        );
        for answer in ["only internal", "one internal of two"] {
            let refused = resolve(&resolver, "hooks.example.org").await;
            assert_eq!(
                refused,
                Err("hooks.example.org resolves to an address that is not public".into()),
                "{answer}"
            );
        }
    }

    /// A receiver on loopback: `/hook` answers 204, `/moved` redirects to `/hook`.
    async fn receiver() -> SocketAddr {
        let app = Router::new()
            .route("/hook", post(|| async { StatusCode::NO_CONTENT }))
            .route("/moved", post(|| async { Redirect::temporary("/hook") }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    async fn post_to(trust: Trust, url: &str) -> Result<StatusCode, PostError> {
        post_with(ProjectTargets::PublicHttps, trust, url).await
    }

    async fn post_with(
        targets: ProjectTargets,
        trust: Trust,
        url: &str,
    ) -> Result<StatusCode, PostError> {
        let clients = Clients::new(targets).unwrap();
        let url = Url::parse(url).unwrap();
        clients
            .post(trust, &url, HeaderMap::new(), b"{}".to_vec())
            .await
    }

    #[tokio::test]
    async fn unrestricted_project_targets_deliver_to_loopback() {
        let port = dual_stack_receiver().await;
        for url in [
            format!("http://127.0.0.1:{port}/hook"),
            format!("http://localhost:{port}/hook"),
        ] {
            let status = post_with(ProjectTargets::Unrestricted, Trust::Project, &url).await;
            assert_eq!(status.unwrap(), StatusCode::NO_CONTENT, "{url}");
        }
    }

    /// A receiver on `127.0.0.1` and, where IPv6 exists, `[::1]` with the same port, so
    /// `localhost` reaches it whichever address resolves first.
    async fn dual_stack_receiver() -> u16 {
        loop {
            let addr = receiver().await;
            match TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, addr.port())).await {
                Ok(v6) => {
                    let app =
                        Router::new().route("/hook", post(|| async { StatusCode::NO_CONTENT }));
                    tokio::spawn(async move { axum::serve(v6, app).await.unwrap() });
                    return addr.port();
                }
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => {}
                Err(_no_ipv6) => return addr.port(),
            }
        }
    }

    #[tokio::test]
    async fn the_instance_client_delivers_to_internal_addresses() {
        let addr = receiver().await;
        let status = post_to(Trust::Instance, &format!("http://{addr}/hook")).await;
        assert_eq!(status.unwrap(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn the_project_client_refuses_loopback_by_name_and_by_literal() {
        let addr = receiver().await;
        let port = addr.port();
        for url in [
            format!("http://127.0.0.1:{port}/hook"),
            format!("http://[::ffff:127.0.0.1]:{port}/hook"),
            format!("http://localhost:{port}/hook"),
        ] {
            let err = post_to(Trust::Project, &url).await.unwrap_err();
            assert!(matches!(err, PostError::Refused(_)), "{url}: {err:?}");
        }
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let addr = receiver().await;
        let status = post_to(Trust::Instance, &format!("http://{addr}/moved")).await;
        assert_eq!(status.unwrap(), StatusCode::TEMPORARY_REDIRECT);
    }
}
