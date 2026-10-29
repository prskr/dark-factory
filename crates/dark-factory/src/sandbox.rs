//! Sandbox backends of the control plane: the Kubernetes backend with its provider, secret
//! store, reconciliation tasks and Service lookup, and the placeholder backend without a cluster.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context as _;
use df_core::InstanceConfig;
use df_db::{Notification, PgPool};
use df_k8s::{
    ClusterServiceRef, ClusterTargetResolver, ClusterTargetsProvider, K8S_API_TIMEOUT,
    K8sSandboxProvider, K8sSecretStore, ProvisioningConfig, ResolvedClusterTarget,
    run_leader_election, run_policy_reconcile_loop, verify_startup, watch_cluster_target_changes,
};
use df_telemetry::logging;
use df_web::egress::LookupError;
use kube::Client;
use kube::api::Api;
use tokio::sync::watch;
use tracing::error;

#[cfg(any(test, feature = "dev-tools"))]
use crate::unconfigured;
use crate::until_stopped;

type Task = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The sandbox provider, secret store and background tasks the control plane runs with.
pub(crate) struct SandboxWiring {
    pub(crate) provider: Arc<dyn df_core::SandboxProvider>,
    pub(crate) secrets: Arc<dyn df_core::SecretStore>,
    /// Tasks that end themselves when `stop` is set (see [`SandboxBackend::start`]).
    pub(crate) tasks: Vec<Task>,
    /// Checks `cluster-service` allowlist entries against the cluster; `None` without a cluster.
    pub(crate) services: Option<Arc<dyn df_web::egress::ServiceLookup>>,
}

/// The live egress log: the relay to the gateway pods.
pub(crate) struct RelayFlows(pub(crate) df_gateway::control::Relay);

impl df_web::egress::FlowSource for RelayFlows {
    fn subscribe(&self) -> futures::stream::BoxStream<'static, df_proto::gateway::v1::FlowEvent> {
        self.0.subscribe()
    }
}

/// Where sandboxes come from.
#[async_trait::async_trait]
pub(crate) trait SandboxBackend {
    /// Verifies the environment and builds the provider. The returned tasks must end once
    /// `stop` is set (or its sender is gone).
    async fn start(
        self,
        cfg: &InstanceConfig,
        pool: &PgPool,
        notifier: &df_db::Notifier,
        stop: watch::Receiver<bool>,
    ) -> anyhow::Result<SandboxWiring>;
}

/// Backend without a cluster (`serve --no-cluster`, tests): every sandbox operation fails.
#[cfg(any(test, feature = "dev-tools"))]
pub(crate) struct UnconfiguredBackend;

#[cfg(any(test, feature = "dev-tools"))]
#[async_trait::async_trait]
impl SandboxBackend for UnconfiguredBackend {
    async fn start(
        self,
        _cfg: &InstanceConfig,
        _pool: &PgPool,
        _notifier: &df_db::Notifier,
        _stop: watch::Receiver<bool>,
    ) -> anyhow::Result<SandboxWiring> {
        Ok(SandboxWiring {
            provider: Arc::new(unconfigured::sandbox_placeholders::UnconfiguredSandboxProvider),
            secrets: Arc::new(unconfigured::sandbox_placeholders::UnconfiguredSecretStore),
            tasks: Vec::new(),
            services: None,
        })
    }
}

/// Sandboxes as agent-sandbox resources in the connected Kubernetes cluster.
pub(crate) struct KubernetesBackend {
    pub(crate) client: Client,
    /// The control plane's own log format, passed on to every sandbox.
    pub(crate) log_format: logging::LogFormat,
}

#[async_trait::async_trait]
impl SandboxBackend for KubernetesBackend {
    async fn start(
        self,
        cfg: &InstanceConfig,
        pool: &PgPool,
        notifier: &df_db::Notifier,
        stop: watch::Receiver<bool>,
    ) -> anyhow::Result<SandboxWiring> {
        let kube_client = self.client;
        let prov_config = provisioning_config(&kube_client, cfg, self.log_format).await?;

        let secrets: Arc<dyn df_core::SecretStore> = Arc::new(K8sSecretStore::new(
            kube_client.clone(),
            cfg.sandbox.provisioning.instance_namespace.clone(),
        ));

        let targets_provider: Arc<dyn ClusterTargetsProvider> =
            Arc::new(DbClusterTargetsProvider {
                pool: pool.clone(),
                resolver: ClusterTargetResolver::new(kube_client.clone()),
            });

        let (provider, kube_watcher_task) = K8sSandboxProvider::start(
            kube_client.clone(),
            prov_config.clone(),
            targets_provider.clone(),
        );

        let (is_leader_tx, is_leader_rx) = watch::channel(false);
        let (notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);

        let forwarder_task = forward_policy_notifications(notifier.subscribe(), notify_tx.clone());

        // Service/EndpointSlice changes in the cluster-target namespaces trigger a reconcile.
        let target_watch_task = watch_cluster_target_changes(
            kube_client.clone(),
            cfg.cluster_targets.clone(),
            notify_tx,
        );

        let leader_task = leader_election_task(
            kube_client.clone(),
            cfg.instance_name.clone(),
            cfg.sandbox.provisioning.instance_namespace.clone(),
            is_leader_tx,
            stop.clone(),
        );

        let services: Arc<dyn df_web::egress::ServiceLookup> = Arc::new(K8sServices {
            client: kube_client.clone(),
            targets: cfg.cluster_targets.clone(),
        });

        let reconcile_task = run_policy_reconcile_loop(
            kube_client,
            prov_config,
            targets_provider,
            is_leader_rx,
            notify_rx,
            stop.clone(),
        );

        Ok(SandboxWiring {
            provider: provider as Arc<dyn df_core::SandboxProvider>,
            secrets,
            services: Some(services),
            tasks: vec![
                Box::pin(until_stopped(stop.clone(), kube_watcher_task)),
                Box::pin(until_stopped(stop.clone(), forwarder_task)),
                Box::pin(until_stopped(stop, target_watch_task)),
                Box::pin(leader_task),
                Box::pin(reconcile_task),
            ],
        })
    }
}

/// Cluster targets from the egress allowlist in the database, resolved against the cluster.
struct DbClusterTargetsProvider {
    pool: PgPool,
    resolver: ClusterTargetResolver,
}

#[async_trait::async_trait]
impl ClusterTargetsProvider for DbClusterTargetsProvider {
    /// Fails only if the allowlist cannot be read (provisioning fails; reconciliation keeps the
    /// run's current policy). A rule whose Service cannot be resolved is skipped, or kept at its
    /// last good resolution, with a warning naming the rule (see [`ClusterTargetResolver`]).
    async fn get_targets(
        &self,
        project_key: Option<&str>,
    ) -> Result<Vec<ResolvedClusterTarget>, df_core::seams::ProviderError> {
        let rules: Vec<ClusterServiceRef> =
            df_db::egress::list_cluster_services(&self.pool, project_key)
                .await
                .map_err(|e| {
                    df_core::seams::ProviderError::Backend(format!(
                        "db error querying egress allowlist: {e}"
                    ))
                })?
                .into_iter()
                .map(|rule| ClusterServiceRef {
                    namespace: rule.namespace,
                    service: rule.service,
                    ports: rule.ports,
                    allow_dns: rule.allow_dns,
                })
                .collect();
        Ok(self.resolver.resolve(project_key, &rules).await)
    }
}

/// Looks up Services in the cluster the control plane runs in, in the namespaces it may read
/// (`clusterTargets`).
struct K8sServices {
    client: Client,
    targets: df_core::config::ClusterTargetsConfig,
}

impl K8sServices {
    fn check_namespace(&self, namespace: &str) -> Result<(), LookupError> {
        if self.targets.any_namespace || self.targets.namespaces.iter().any(|n| n == namespace) {
            Ok(())
        } else {
            Err(LookupError::NamespaceNotAllowed(namespace.to_string()))
        }
    }
}

#[async_trait::async_trait]
impl df_web::egress::ServiceLookup for K8sServices {
    async fn exists(&self, namespace: &str, service: &str) -> Result<bool, LookupError> {
        self.check_namespace(namespace)?;
        let api: Api<k8s_openapi::api::core::v1::Service> =
            Api::namespaced(self.client.clone(), namespace);
        tokio::time::timeout(K8S_API_TIMEOUT, api.get_opt(service))
            .await
            .map_err(|_elapsed| LookupError::Timeout)?
            .map(|found| found.is_some())
            .map_err(|e| LookupError::Api(e.to_string()))
    }
}

/// Verifies the cluster setup and builds the provisioning config for sandboxes (endpoints of the
/// control plane and the gateway included).
async fn provisioning_config(
    kube_client: &Client,
    cfg: &InstanceConfig,
    log_format: logging::LogFormat,
) -> anyhow::Result<ProvisioningConfig> {
    verify_startup(
        kube_client,
        &cfg.sandbox.provisioning.namespace,
        &cfg.sandbox.provisioning.instance_namespace,
        &cfg.cluster_targets,
    )
    .await
    .context("startup check failed")?;

    let (cp_ip, gw_ip) = resolve_cluster_ips(kube_client, &cfg.instance_name, &cfg.sandbox).await?;

    let tls_server_name = cfg
        .sandbox
        .endpoints
        .control_plane_tls_server_name
        .clone()
        .unwrap_or_else(|| format!("{}-control-plane", cfg.instance_name));

    Ok(ProvisioningConfig::from_sandbox_config(
        cfg.instance_name.clone(),
        &cfg.sandbox.provisioning,
        df_k8s::ControlPlaneEndpoint {
            ip: cp_ip,
            port: cfg.server.agent_port,
            tls_server_name,
        },
        df_k8s::GatewayEndpoint {
            ip: gw_ip,
            proxy_port: cfg.gateway.proxy_port,
        },
    )
    .with_harness_images(cfg.harness.images())
    .with_log_format(log_format))
}

/// Triggers a policy reconciliation for every database notification that changes the egress
/// policy, also after a lag or a reconnect. Ends when the notifier is gone.
async fn forward_policy_notifications(
    mut db_notify_sub: tokio::sync::broadcast::Receiver<Notification>,
    n_tx_db: tokio::sync::mpsc::Sender<()>,
) {
    loop {
        match db_notify_sub.recv().await {
            Ok(Notification::EgressPolicy | Notification::Reconnected) => {
                // Ignored on purpose: a full channel means a pass is already pending,
                // a closed one means the loop stopped.
                n_tx_db.try_send(()).ok(); // receiver may be gone
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(
                    skipped_notifications = n,
                    "database notification subscriber lagged; triggering policy reconciliation"
                );
                // Ignored on purpose: a full channel means a pass is already pending,
                // a closed one means the loop stopped.
                n_tx_db.try_send(()).ok(); // receiver may be gone
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

/// Runs the leader election for the policy reconciliation until `stop` is set.
async fn leader_election_task(
    client: Client,
    instance_name: String,
    instance_namespace: String,
    is_leader_tx: watch::Sender<bool>,
    stop: watch::Receiver<bool>,
) {
    let lease_name = format!("{instance_name}-leader");
    let identity = format!("{instance_name}-{}", uuid::Uuid::now_v7());
    if let Err(err) = run_leader_election(
        client,
        instance_namespace,
        lease_name,
        identity,
        df_k8s::reconcile::DEFAULT_LEASE_DURATION_SECONDS,
        is_leader_tx,
        stop,
    )
    .await
    {
        error!(error = %err, "leader election failed");
    }
}

async fn resolve_cluster_ips(
    client: &Client,
    instance_name: &str,
    sandbox: &df_core::SandboxSectionConfig,
) -> anyhow::Result<(IpAddr, IpAddr)> {
    async fn resolve_service_ip(
        client: &Client,
        namespace: &str,
        names: &[String],
    ) -> anyhow::Result<IpAddr> {
        let svcs: Api<k8s_openapi::api::core::v1::Service> =
            Api::namespaced(client.clone(), namespace);
        let mut last_error = None;
        for name in names {
            let get_fut = tokio::time::timeout(K8S_API_TIMEOUT, svcs.get(name));
            match get_fut.await {
                Ok(Ok(svc)) => {
                    if let Some(ip) = svc
                        .spec
                        .as_ref()
                        .and_then(|s| s.cluster_ip.as_ref())
                        .and_then(|ip| ip.parse::<IpAddr>().ok())
                    {
                        return Ok(ip);
                    }
                }
                Ok(Err(err)) => {
                    last_error = Some(anyhow::anyhow!(err));
                }
                Err(_) => {
                    last_error = Some(anyhow::anyhow!("timeout resolving service `{name}`"));
                }
            }
        }
        let tried = names.join(", ");
        if let Some(err) = last_error {
            anyhow::bail!(
                "failed to resolve cluster IP for service in namespace `{namespace}` (tried: {tried}): {err:#}"
            );
        }
        anyhow::bail!(
            "service in namespace `{namespace}` has no valid cluster IP (tried: {tried})"
        );
    }

    let instance_namespace = sandbox.provisioning.instance_namespace.as_str();
    let (configured_cp_ip, configured_gw_ip) = (
        sandbox.endpoints.control_plane_ip,
        sandbox.endpoints.gateway_ip,
    );
    let (cp_service_name, gw_service_name) = (
        sandbox.endpoints.control_plane_service_name.as_deref(),
        sandbox.endpoints.gateway_service_name.as_deref(),
    );

    let mut cp_names = Vec::new();
    if let Some(name) = cp_service_name {
        cp_names.push(name.to_string());
    }
    cp_names.push(format!("{instance_name}-control-plane"));
    cp_names.push("dark-factory-control-plane".to_string());

    let cp_ip = match configured_cp_ip {
        Some(ip) => ip,
        None => resolve_service_ip(client, instance_namespace, &cp_names)
            .await
            .context("failed to resolve control plane service IP")?,
    };

    let mut gw_names = Vec::new();
    if let Some(name) = gw_service_name {
        gw_names.push(name.to_string());
    }
    gw_names.push(format!("{instance_name}-gateway"));
    gw_names.push(format!("{instance_name}-egress-gateway"));
    gw_names.push("dark-factory-gateway".to_string());

    let gw_ip = match configured_gw_ip {
        Some(ip) => ip,
        None => resolve_service_ip(client, instance_namespace, &gw_names)
            .await
            .context("failed to resolve egress gateway service IP")?,
    };

    Ok((cp_ip, gw_ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn service_lookup_is_limited_to_the_cluster_target_namespaces() {
        // No request reaches the API for a namespace that is not allowed.
        let client =
            Client::try_from(kube::Config::new("http://127.0.0.1:1".parse().unwrap())).unwrap();
        let limited = K8sServices {
            client: client.clone(),
            targets: df_core::config::ClusterTargetsConfig {
                any_namespace: false,
                namespaces: vec!["nativelink".into()],
            },
        };
        assert!(limited.check_namespace("nativelink").is_ok());
        let denied =
            df_web::egress::ServiceLookup::exists(&limited, "kube-system", "kube-dns").await;
        assert!(matches!(denied, Err(LookupError::NamespaceNotAllowed(ns)) if ns == "kube-system"));
        let any = K8sServices {
            client,
            targets: df_core::config::ClusterTargetsConfig {
                any_namespace: true,
                namespaces: vec![],
            },
        };
        assert!(any.check_namespace("anything").is_ok());
    }
}
