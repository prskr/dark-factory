//! Egress pages: the allowlist editor and the live egress log (`specs/egress-allowlist`,
//! `specs/egress-log`). Both are desktop-only: they are not in the phone navigation.

use async_trait::async_trait;
use axum::Router;
use axum::routing::{delete, get, post};
use df_proto::gateway::v1::FlowEvent;
use futures::stream::BoxStream;

use crate::WebState;
use crate::components::ProjectHosts;

mod allowlist;
mod log;
#[cfg(test)]
mod tests;

/// The events of the egress proxy, from all gateway pods.
pub trait FlowSource: Send + Sync {
    /// A viewer's stream: recent events, then an event with `backlog_end` set, then live ones.
    /// Dropping it ends the viewer.
    ///
    /// Implementations MUST send `backlog_end` within a short bound (the gateway relay's
    /// `BACKLOG_WAIT`): `GET /egress/log` waits for it before it responds.
    fn subscribe(&self) -> BoxStream<'static, FlowEvent>;
}

/// Looks up Services of the cluster, to check `cluster-service` entries when they are saved.
#[async_trait]
pub trait ServiceLookup: Send + Sync {
    /// Whether the Service exists; `Err` if the cluster could not be asked.
    async fn exists(&self, namespace: &str, service: &str) -> Result<bool, LookupError>;
}

/// Why a Service could not be looked up.
#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    /// The control plane has no permission to read Services of the namespace.
    #[error("namespace `{0}` is not one of the configured cluster target namespaces")]
    NamespaceNotAllowed(String),
    #[error("the Kubernetes API did not answer in time")]
    Timeout,
    #[error("Kubernetes API error: {0}")]
    Api(String),
}

/// The hosts of the projects, for the host icons. They only decorate, so a failed query is logged
/// and the icons fall back to the generic ones.
async fn project_hosts(state: &WebState) -> ProjectHosts {
    match df_db::catalog::list_projects(&state.pool).await {
        Ok(projects) => ProjectHosts::from_projects(&state.forges, &projects),
        Err(error) => {
            tracing::warn!(%error, "listing projects for the host icons");
            ProjectHosts::default()
        }
    }
}

pub fn routes() -> Router<WebState> {
    Router::new()
        .route("/egress/allowlist", get(allowlist::list))
        .route("/egress/allowlist/host/new", get(allowlist::new_host))
        .route("/egress/allowlist/host", post(allowlist::add_host))
        .route("/egress/allowlist/service/new", get(allowlist::new_service))
        .route("/egress/allowlist/service", post(allowlist::add_service))
        .route("/egress/allowlist/{id}", delete(allowlist::remove))
        .route("/egress/log", get(log::page))
        .route("/egress/log/events", get(log::events))
}
