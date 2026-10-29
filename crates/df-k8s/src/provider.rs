//! Sandbox provider implementation for Kubernetes and agent-sandbox CRDs.

use std::collections::HashSet;
use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use df_core::TaskVolumeSpec;
use df_core::ids::RunId;
use df_core::seams::{PeerIdentity, ProviderError, RunSandboxSpec, SandboxEvent, SandboxProvider};
use futures::stream::{BoxStream, Stream};
use futures::{StreamExt as _, future};
use k8s_openapi::api::core::v1::{PersistentVolumeClaim, Pod, Service, ServicePort};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::networking::v1::NetworkPolicy;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::Client;
use kube::api::{Api, DeleteParams, Patch, PatchParams};
use kube::runtime::WatchStreamExt as _;
use kube::runtime::reflector::{self, ObjectRef, Store};
use kube::runtime::watcher;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tracing::{info, warn};

use crate::K8S_API_TIMEOUT;
use crate::config::ProvisioningConfig;
use crate::crd::Sandbox;
use crate::render::{
    RenderError, ResolvedClusterTarget, render_network_policy, render_pvc, render_sandbox,
};

/// Trait providing resolved cluster targets for sandbox provisioning.
///
/// An `Err` means the targets could not be determined at all (for example the allowlist cannot
/// be read): provisioning fails and reconciliation keeps the run's existing policy. Individual
/// targets that cannot be resolved are not an error; the implementation skips them or reuses
/// their last good resolution (see [`ClusterTargetResolver`]), so the list may be partial.
#[async_trait]
pub trait ClusterTargetsProvider: Send + Sync {
    async fn get_targets(
        &self,
        project_key: Option<&str>,
    ) -> Result<Vec<ResolvedClusterTarget>, ProviderError>;
}

/// Static cluster targets provider for testing and simple setups.
#[cfg(test)]
pub struct StaticClusterTargetsProvider {
    targets: Vec<ResolvedClusterTarget>,
}

#[cfg(test)]
impl StaticClusterTargetsProvider {
    pub fn new(targets: Vec<ResolvedClusterTarget>) -> Self {
        Self { targets }
    }
}

#[cfg(test)]
#[async_trait]
impl ClusterTargetsProvider for StaticClusterTargetsProvider {
    async fn get_targets(
        &self,
        _project_key: Option<&str>,
    ) -> Result<Vec<ResolvedClusterTarget>, ProviderError> {
        Ok(self.targets.clone())
    }
}

/// Why a rule could not be resolved, which decides whether an earlier resolution may be reused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// Timeout, `5xx`/`408`/`429` answer, connection error, or a named port without a ready
    /// endpoint: the Service is probably unchanged, an earlier resolution may be reused.
    #[error("{0}")]
    Transient(String),
    /// `404`/`403`/other `4xx` answer, or a Service that no longer satisfies the rule (headless
    /// without `allowDns`, ports that do not match, no ports): the earlier resolution is stale.
    #[error("{0}")]
    Permanent(String),
}

fn is_permanent(err: &kube::Error) -> bool {
    matches!(err, kube::Error::Api(status)
        if (400..500).contains(&status.code) && status.code != 408 && status.code != 429)
}

/// Like [`api_call`], but keeps the error class.
async fn resolve_call<T>(
    what: &str,
    call: impl Future<Output = Result<T, kube::Error>>,
) -> Result<T, ResolveError> {
    match tokio::time::timeout(K8S_API_TIMEOUT, call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) if is_permanent(&err) => {
            Err(ResolveError::Permanent(format!("failed to {what}: {err}")))
        }
        Ok(Err(err)) => Err(ResolveError::Transient(format!("failed to {what}: {err}"))),
        Err(_) => Err(ResolveError::Transient(format!("timeout trying to {what}"))),
    }
}

/// Resolves a cluster service rule into a `ResolvedClusterTarget` by querying Kubernetes.
///
/// Errors are classified for [`ClusterTargetResolver`]: see [`ResolveError`].
///
/// A named `targetPort` is resolved through the Service's `EndpointSlices`, whose port entries are
/// named after the Service port's `name`. If it cannot be resolved the call fails instead of
/// guessing the Service port number.
pub async fn resolve_cluster_target(
    client: &Client,
    namespace: &str,
    service: &str,
    configured_ports: &[u16],
    allow_dns: bool,
) -> Result<ResolvedClusterTarget, ResolveError> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let svc = resolve_call(
        &format!("get service `{namespace}/{service}`"),
        services.get(service),
    )
    .await?;

    let svc_spec = svc.spec.as_ref();
    let cluster_ip_str = svc_spec.and_then(|s| s.cluster_ip.as_deref());
    let is_headless = cluster_ip_str.is_none_or(|ip| ip.is_empty() || ip == "None");
    let cluster_ip = if is_headless {
        None
    } else {
        cluster_ip_str.and_then(|ip| ip.parse::<IpAddr>().ok())
    };

    if is_headless && !allow_dns {
        return Err(ResolveError::Permanent(format!(
            "headless service `{namespace}/{service}` not allowed without allowDns"
        )));
    }

    let pod_selector = svc_spec
        .and_then(|s| s.selector.clone())
        .filter(|sel| !sel.is_empty());

    let has_named_target_port = svc_spec
        .and_then(|s| s.ports.as_ref())
        .is_some_and(|ports| {
            ports
                .iter()
                .any(|p| matches!(p.target_port, Some(IntOrString::String(_))))
        });

    let needs_endpoints = pod_selector.is_none() || is_headless || has_named_target_port;
    let endpoint_slices = if needs_endpoints {
        list_endpoint_slices(client, namespace, service).await?
    } else {
        Vec::new()
    };

    let service_ports = svc_spec
        .and_then(|s| s.ports.as_deref())
        .unwrap_or_default();
    let target_ports = resolve_target_ports(
        service_ports,
        &endpoint_slices,
        configured_ports,
        &format!("{namespace}/{service}"),
    )?;

    let endpoint_ips = if pod_selector.is_none() || is_headless {
        endpoint_ips(&endpoint_slices)
    } else {
        Vec::new()
    };

    Ok(ResolvedClusterTarget {
        namespace: namespace.to_string(),
        service: service.to_string(),
        cluster_ip,
        is_headless,
        allow_dns,
        pod_selector,
        target_ports,
        endpoint_ips,
    })
}

/// Lists the `EndpointSlices` of a Service.
async fn list_endpoint_slices(
    client: &Client,
    namespace: &str,
    service: &str,
) -> Result<Vec<EndpointSlice>, ResolveError> {
    let slices: Api<EndpointSlice> = Api::namespaced(client.clone(), namespace);
    let lp =
        kube::api::ListParams::default().labels(&format!("kubernetes.io/service-name={service}"));
    Ok(resolve_call(
        &format!("list EndpointSlices for `{namespace}/{service}`"),
        slices.list(&lp),
    )
    .await?
    .items)
}

/// The sorted, distinct target ports of the Service ports the operator allows
/// (`configured_ports`; empty means all). `name` is `namespace/service`, for errors.
fn resolve_target_ports(
    service_ports: &[ServicePort],
    endpoint_slices: &[EndpointSlice],
    configured_ports: &[u16],
    name: &str,
) -> Result<Vec<u16>, ResolveError> {
    let mut target_ports: Vec<u16> = Vec::new();
    for p in service_ports {
        let port_num = u16::try_from(p.port).map_err(|_out_of_range| {
            ResolveError::Permanent(format!("invalid Service port: {}", p.port))
        })?;
        if !configured_ports.is_empty() && !configured_ports.contains(&port_num) {
            continue;
        }
        match &p.target_port {
            Some(IntOrString::Int(tp)) => {
                target_ports.push(u16::try_from(*tp).map_err(|_error| {
                    ResolveError::Permanent(format!("invalid numeric target port: {tp}"))
                })?);
            }
            Some(IntOrString::String(target_name)) => {
                target_ports.extend(named_target_ports(p, target_name, endpoint_slices, name)?);
            }
            None => target_ports.push(port_num),
        }
    }
    if service_ports.is_empty() {
        // A Service without ports (headless): the operator's configured ports are the source.
        target_ports.extend_from_slice(configured_ports);
    } else if target_ports.is_empty() {
        return Err(ResolveError::Permanent(format!(
            "none of the configured ports {configured_ports:?} is a port of service `{name}`"
        )));
    }
    target_ports.sort_unstable();
    target_ports.dedup();
    if target_ports.is_empty() {
        // An empty list would render a rule that allows every port.
        return Err(ResolveError::Permanent(format!(
            "service `{name}` has no ports and none are configured"
        )));
    }
    Ok(target_ports)
}

/// Resolves a named `targetPort` of the Service port `p` through the `EndpointSlices`.
fn named_target_ports(
    p: &ServicePort,
    target_name: &str,
    endpoint_slices: &[EndpointSlice],
    name: &str,
) -> Result<Vec<u16>, ResolveError> {
    // EndpointSlice ports carry the Service port's name, not the targetPort name.
    // Pods may use different numbers for the same name (separate slices): take all.
    let service_port_name = p.name.as_deref().unwrap_or_default();
    let ports: Vec<u16> = endpoint_slices
        .iter()
        .filter_map(|s| s.ports.as_ref())
        .flatten()
        .filter(|ep| ep.name.as_deref().unwrap_or_default() == service_port_name)
        .filter_map(|ep| ep.port.and_then(|pt| u16::try_from(pt).ok()))
        .collect();
    if ports.is_empty() {
        return Err(ResolveError::Transient(format!(
            "cannot resolve named targetPort `{target_name}` of port `{service_port_name}` of service `{name}`: no matching EndpointSlice port"
        )));
    }
    Ok(ports)
}

/// The addresses of all endpoints of the slices.
fn endpoint_ips(endpoint_slices: &[EndpointSlice]) -> Vec<IpAddr> {
    endpoint_slices
        .iter()
        .flat_map(|slice| slice.endpoints.iter().flatten())
        .flat_map(|ep| ep.addresses.iter().filter_map(|a| a.parse::<IpAddr>().ok()))
        .collect()
}

/// A cluster Service an egress rule allows, as configured (not yet resolved).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClusterServiceRef {
    pub namespace: String,
    pub service: String,
    /// Allowed Service ports; empty means all of the Service's ports.
    pub ports: Vec<u16>,
    pub allow_dns: bool,
}

/// Resolves allowlist rules against the cluster, one rule at a time.
///
/// A rule that cannot be resolved never blocks the others:
/// - on a transient error ([`ResolveError::Transient`]) the last good resolution of the rule is
///   used, with a warning, so a hiccup does not revoke access a live run needs;
/// - on a permanent error ([`ResolveError::Permanent`], e.g. the Service was deleted) the cached
///   resolution is evicted and the rule is skipped with a warning naming it, so stale access is
///   not kept;
/// - without a cached resolution a failing rule is skipped (fail closed).
///
/// The cache is kept per scope (the project key of the call; per-project calls pass different
/// rule sets) and pruned to the rules of the latest call of that scope, so a removed rule that
/// is added again later starts without history.
pub struct ClusterTargetResolver {
    client: Client,
    last_good: std::sync::Mutex<LastGood>,
}

type LastGood = std::collections::HashMap<
    Option<String>,
    std::collections::HashMap<ClusterServiceRef, ResolvedClusterTarget>,
>;

impl ClusterTargetResolver {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            last_good: std::sync::Mutex::default(),
        }
    }

    /// Resolves `rules`, which apply to `scope` (the project key, `None` for instance-wide).
    // ponytail: rules resolve one after another (worst case 2 API calls of K8S_API_TIMEOUT per
    // rule); resolve concurrently if allowlists grow large.
    pub async fn resolve(
        &self,
        scope: Option<&str>,
        rules: &[ClusterServiceRef],
    ) -> Vec<ResolvedClusterTarget> {
        let scope = scope.map(str::to_string);
        let mut targets = Vec::with_capacity(rules.len());
        for rule in rules {
            let resolved = resolve_cluster_target(
                &self.client,
                &rule.namespace,
                &rule.service,
                &rule.ports,
                rule.allow_dns,
            )
            .await;
            // The guard is never held across an await.
            let mut cache = self
                .last_good
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let scoped = cache.entry(scope.clone()).or_default();
            match resolved {
                Ok(target) => {
                    scoped.insert(rule.clone(), target.clone());
                    targets.push(target);
                }
                Err(err @ ResolveError::Transient(_)) => match scoped.get(rule) {
                    Some(previous) => {
                        warn!(error = %err, namespace = %rule.namespace, service = %rule.service, "cannot resolve allowlist rule; using its last good resolution");
                        targets.push(previous.clone());
                    }
                    None => {
                        warn!(error = %err, namespace = %rule.namespace, service = %rule.service, "cannot resolve allowlist rule; skipping it");
                    }
                },
                Err(err @ ResolveError::Permanent(_)) => {
                    scoped.remove(rule);
                    warn!(error = %err, namespace = %rule.namespace, service = %rule.service, "allowlist rule no longer resolves; dropping it from the policy");
                }
            }
        }
        let mut cache = self
            .last_good
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .entry(scope)
            .or_default()
            .retain(|rule, _| rules.contains(rule));
        targets
    }
}

/// Parses a SPIFFE URI SAN formatted as `spiffe://dark-factory/ns/<ns>/pod/<name>/<uid>`.
pub fn parse_spiffe_san(san: &str) -> Option<(&str, &str, &str)> {
    let rest = san.strip_prefix("spiffe://dark-factory/ns/")?;
    let (ns, rest) = rest.split_once("/pod/")?;
    let (name, uid) = rest.split_once('/')?;
    if ns.is_empty() || name.is_empty() || uid.is_empty() || uid.contains('/') {
        return None;
    }
    Some((ns, name, uid))
}

/// Resolves peer mTLS identity using cached Pod and Sandbox reflectors.
pub fn resolve_identity_from_stores(
    peer: &PeerIdentity,
    sandbox_namespace: &str,
    instance_name: &str,
    pod_store: &Store<Pod>,
    sandbox_store: &Store<Sandbox>,
) -> Option<RunId> {
    let (ns, pod_name, uid) = parse_spiffe_san(&peer.uri_san)?;
    if ns != sandbox_namespace {
        return None;
    }

    let pod_ref = ObjectRef::new(pod_name).within(sandbox_namespace);
    let pod = pod_store.get(&pod_ref)?;

    // Verify UID
    if pod.metadata.uid.as_deref() != Some(uid) {
        return None;
    }

    // Verify peer IP matches pod IP
    let pod_ip_str = pod.status.as_ref().and_then(|s| s.pod_ip.as_ref())?;
    let pod_ip: IpAddr = pod_ip_str.parse().ok()?;
    if pod_ip != peer.peer_ip {
        return None;
    }

    // Verify ownership by a Sandbox
    let owner = pod
        .metadata
        .owner_references
        .as_ref()?
        .iter()
        .find(|o| o.kind == "Sandbox" && o.api_version.starts_with("agents.x-k8s.io"))?;

    let sb_ref = ObjectRef::new(&owner.name).within(sandbox_namespace);
    let sandbox = sandbox_store.get(&sb_ref)?;

    // Verify owner UID matches Sandbox UID
    if sandbox.metadata.uid.as_deref() != Some(&owner.uid) {
        return None;
    }

    // Verify instance label
    let sb_labels = sandbox.metadata.labels.as_ref()?;
    if sb_labels
        .get("dark-factory.io/instance")
        .map(String::as_str)
        != Some(instance_name)
    {
        return None;
    }

    run_id_from_metadata(&sandbox.metadata)
}

/// Derives a `SandboxEvent` transition from a Pod's current state.
pub fn map_pod_event(pod: &Pod) -> Option<SandboxEvent> {
    let run_id = run_id_from_metadata(&pod.metadata)?;

    // 1. Deletion timestamp set
    if pod.metadata.deletion_timestamp.is_some() {
        return Some(SandboxEvent::Gone {
            run_id,
            reason: "pod deleted".to_string(),
        });
    }

    let status = pod.status.as_ref()?;

    // 2. Failed phase
    if status.phase.as_deref() == Some("Failed") {
        let reason = status
            .reason
            .clone()
            .or_else(|| status.message.clone())
            .unwrap_or_else(|| "pod failed".to_string());
        return Some(SandboxEvent::Gone { run_id, reason });
    }

    // 3. Container waiting errors (e.g. ErrImagePull, ImagePullBackOff, CreateContainerConfigError)
    {
        for cs in status.container_statuses.iter().flatten() {
            if let Some(ref state) = cs.state
                && let Some(ref waiting) = state.waiting
                && let Some("ErrImagePull" | "ImagePullBackOff" | "CreateContainerConfigError") =
                    waiting.reason.as_deref()
            {
                let msg = waiting.message.as_deref().unwrap_or_default();
                let reason = if msg.is_empty() {
                    waiting.reason.clone().unwrap_or_default()
                } else {
                    format!("{}: {}", waiting.reason.as_deref().unwrap_or_default(), msg)
                };
                return Some(SandboxEvent::Gone { run_id, reason });
            }
        }
    }

    // 4. Running with IP -> Started
    if status.phase.as_deref() == Some("Running")
        && let Some(ref ip_str) = status.pod_ip
        && let Ok(ip) = ip_str.parse::<IpAddr>()
    {
        return Some(SandboxEvent::Started { run_id, pod_ip: ip });
    }

    None
}

/// Extracts `RunId` from an object's `dark-factory.io/run-id` label.
pub fn run_id_from_metadata(meta: &ObjectMeta) -> Option<RunId> {
    meta.labels
        .as_ref()
        .and_then(|l| l.get("dark-factory.io/run-id"))
        .and_then(|id_str| id_str.parse::<RunId>().ok())
}

/// Derives a `SandboxEvent::Gone` event from a Pod deletion.
pub fn map_pod_delete_event(pod: &Pod) -> Option<SandboxEvent> {
    let run_id = run_id_from_metadata(&pod.metadata)?;
    Some(SandboxEvent::Gone {
        run_id,
        reason: "pod deleted".to_string(),
    })
}

/// Kubernetes sandbox and workspace volume provider.
#[derive(Clone)]
pub struct K8sSandboxProvider {
    client: Client,
    config: ProvisioningConfig,
    targets_provider: Arc<dyn ClusterTargetsProvider>,
    pod_store: Store<Pod>,
    sandbox_store: Store<Sandbox>,
    events_tx: broadcast::Sender<SandboxEvent>,
}

impl K8sSandboxProvider {
    /// Creates a new `K8sSandboxProvider`.
    pub fn new(
        client: Client,
        config: ProvisioningConfig,
        targets_provider: Arc<dyn ClusterTargetsProvider>,
        pod_store: Store<Pod>,
        sandbox_store: Store<Sandbox>,
    ) -> Self {
        let (events_tx, _) = broadcast::channel(128);
        Self {
            client,
            config,
            targets_provider,
            pod_store,
            sandbox_store,
            events_tx,
        }
    }

    /// Starts the sandbox provider, spawning pod and sandbox reflectors with backoff.
    ///
    /// The returned future drives the reflectors and the event translation; the caller owns it
    /// and must run it for the provider to see changes.
    pub fn start(
        client: Client,
        config: ProvisioningConfig,
        targets_provider: Arc<dyn ClusterTargetsProvider>,
    ) -> (
        Arc<Self>,
        std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    ) {
        let selector = format!("dark-factory.io/instance={}", config.instance_name);

        let (pod_store, pod_writer) = reflector::store();
        let pod_api: Api<Pod> = Api::namespaced(client.clone(), &config.sandbox.namespace);
        let pod_watcher =
            watcher(pod_api, watcher::Config::default().labels(&selector)).default_backoff();
        let pod_reflector = reflector::reflector(pod_writer, pod_watcher);

        let (sb_store, sb_writer) = reflector::store();
        let sb_api: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
        let sb_watcher =
            watcher(sb_api, watcher::Config::default().labels(&selector)).default_backoff();
        let sb_reflector = reflector::reflector(sb_writer, sb_watcher);

        let provider = Arc::new(Self::new(
            client,
            config,
            targets_provider,
            pod_store,
            sb_store,
        ));

        let prov_for_events = provider.clone();
        let watcher_task = Box::pin(forward_events(
            pod_reflector.boxed(),
            sb_reflector.boxed(),
            move |event| prov_for_events.emit_event(event),
        ));

        (provider, watcher_task)
    }

    /// Emits a sandbox event to listeners.
    pub fn emit_event(&self, event: SandboxEvent) {
        // Best-effort broadcast; receivers might be closed or dropped.
        self.events_tx.send(event).ok(); // receiver may be gone
    }
}

#[async_trait]
impl SandboxProvider for K8sSandboxProvider {
    async fn ensure_task_volume(&self, task: &TaskVolumeSpec) -> Result<(), ProviderError> {
        let pvcs: Api<PersistentVolumeClaim> =
            Api::namespaced(self.client.clone(), &self.config.sandbox.namespace);
        let pvc = render_pvc(task, &self.config);
        let name = task.task_key.volume_name();

        let patch = Patch::Apply(&pvc);
        let params = PatchParams::apply("dark-factory").force();
        api_call(
            &format!("ensure task PVC `{name}`"),
            pvcs.patch(&name, &params, &patch),
        )
        .await?;

        info!(task_key = %task.task_key, volume = %name, "task PVC ensured");
        Ok(())
    }

    async fn provision(&self, spec: &RunSandboxSpec) -> Result<(), ProviderError> {
        let sandboxes: Api<Sandbox> =
            Api::namespaced(self.client.clone(), &self.config.sandbox.namespace);
        let netpols: Api<NetworkPolicy> =
            Api::namespaced(self.client.clone(), &self.config.sandbox.namespace);

        let targets = self
            .targets_provider
            .get_targets(Some(spec.project_key.as_str()))
            .await?;

        let sandbox = render_sandbox(spec, &targets, &self.config).map_err(|e| match e {
            RenderError::HeadlessServiceNotAllowed { namespace, service } => {
                ProviderError::InvalidSpec(format!(
                    "headless service `{namespace}/{service}` not allowed without allowDns"
                ))
            }
            e @ RenderError::NoHarnessImage { .. } => ProviderError::InvalidSpec(e.to_string()),
        })?;

        let sandbox_name = sandbox
            .metadata
            .name
            .clone()
            .ok_or_else(|| ProviderError::InvalidSpec("sandbox name is missing".to_string()))?;
        let patch = Patch::Apply(&sandbox);
        let params = PatchParams::apply("dark-factory").force();

        let applied_sandbox = api_call(
            &format!("apply Sandbox `{sandbox_name}`"),
            sandboxes.patch(&sandbox_name, &params, &patch),
        )
        .await?;

        let netpol = render_network_policy(
            spec.run_id,
            &spec.task_key,
            &applied_sandbox,
            &targets,
            &self.config,
        );
        let netpol_name = netpol.metadata.name.clone().ok_or_else(|| {
            ProviderError::InvalidSpec("network policy name is missing".to_string())
        })?;
        let netpol_patch = Patch::Apply(&netpol);
        api_call(
            &format!("apply NetworkPolicy `{netpol_name}`"),
            netpols.patch(&netpol_name, &params, &netpol_patch),
        )
        .await?;

        info!(run_id = %spec.run_id, sandbox = %sandbox_name, "sandbox and network policy provisioned");
        Ok(())
    }

    async fn teardown(&self, run_id: RunId) -> Result<(), ProviderError> {
        let sandboxes: Api<Sandbox> =
            Api::namespaced(self.client.clone(), &self.config.sandbox.namespace);

        // List sandboxes by instance and run-id label
        let list_params = kube::api::ListParams::default().labels(&format!(
            "dark-factory.io/instance={},dark-factory.io/run-id={run_id}",
            self.config.instance_name
        ));
        let matching =
            api_call("list sandboxes for teardown", sandboxes.list(&list_params)).await?;

        for sb in matching {
            if let Some(name) = sb.metadata.name {
                let deleted = tokio::time::timeout(
                    K8S_API_TIMEOUT,
                    sandboxes.delete(&name, &DeleteParams::default()),
                )
                .await;
                match deleted {
                    Ok(Ok(_)) => {
                        info!(run_id = %run_id, sandbox = %name, "sandbox deleted during teardown");
                    }
                    Ok(Err(kube::Error::Api(e))) if e.code == 404 => {}
                    Ok(Err(e)) => {
                        return Err(ProviderError::Backend(format!(
                            "failed to delete sandbox `{name}`: {e}"
                        )));
                    }
                    Err(_) => {
                        return Err(ProviderError::Backend(format!(
                            "timeout deleting sandbox `{name}`"
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    async fn resolve_identity(&self, peer: &PeerIdentity) -> Option<RunId> {
        resolve_identity_from_stores(
            peer,
            &self.config.sandbox.namespace,
            &self.config.instance_name,
            &self.pod_store,
            &self.sandbox_store,
        )
    }

    fn events(&self) -> BoxStream<'static, SandboxEvent> {
        let rx = self.events_tx.subscribe();
        BroadcastStream::new(rx)
            .filter_map(|res| {
                future::ready(match res {
                    Ok(event) => Some(event),
                    Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                        warn!(skipped, "sandbox event subscriber lagged; events were lost");
                        None
                    }
                })
            })
            .boxed()
    }
}

/// Runs a Kubernetes API call under [`K8S_API_TIMEOUT`] and maps failures to a backend error
/// that names `what`.
async fn api_call<T>(
    what: &str,
    call: impl Future<Output = Result<T, kube::Error>>,
) -> Result<T, ProviderError> {
    match tokio::time::timeout(K8S_API_TIMEOUT, call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(ProviderError::Backend(format!("failed to {what}: {err}"))),
        Err(_) => Err(ProviderError::Backend(format!("timeout trying to {what}"))),
    }
}

/// Translates pod and sandbox watch events into [`SandboxEvent`]s, at most one `Started` and one
/// `Gone` per run.
///
/// A relist (`Init` .. `InitDone`) never sends `Delete`: runs whose pod or Sandbox vanished while
/// the watch was down are only noticed by comparing what was known with what the relist
/// returned. Only objects seen by this process are known; after a control plane restart nothing
/// is known, and a run whose pod disappeared meanwhile ends through the scheduler's timeouts.
#[derive(Default)]
struct EventTracker {
    started: HashSet<RunId>,
    // One entry per run for the process lifetime.
    // ponytail: unbounded; prune when a run's teardown completes if this ever matters.
    gone: HashSet<RunId>,
    /// Runs with a pod / Sandbox seen so far.
    pods: HashSet<RunId>,
    sandboxes: HashSet<RunId>,
    /// Runs returned between `Init` and `InitDone` of the current relist.
    relisted_pods: HashSet<RunId>,
    relisted_sandboxes: HashSet<RunId>,
}

impl EventTracker {
    fn dedupe(&mut self, event: SandboxEvent) -> Option<SandboxEvent> {
        match event {
            SandboxEvent::Started { run_id, .. } => {
                (!self.gone.contains(&run_id) && self.started.insert(run_id)).then_some(event)
            }
            SandboxEvent::Gone { run_id, .. } => {
                self.started.remove(&run_id);
                self.gone.insert(run_id).then_some(event)
            }
        }
    }

    fn gone_events(&mut self, run_ids: Vec<RunId>, reason: &str) -> Vec<SandboxEvent> {
        run_ids
            .into_iter()
            .filter_map(|run_id| {
                self.dedupe(SandboxEvent::Gone {
                    run_id,
                    reason: reason.to_string(),
                })
            })
            .collect()
    }

    fn pod_event(&mut self, event: watcher::Event<Pod>) -> Vec<SandboxEvent> {
        let events: Vec<SandboxEvent> = match event {
            watcher::Event::Init => {
                self.relisted_pods.clear();
                Vec::new()
            }
            watcher::Event::InitApply(pod) => {
                if let Some(run_id) = run_id_from_metadata(&pod.metadata) {
                    self.relisted_pods.insert(run_id);
                    self.pods.insert(run_id);
                }
                map_pod_event(&pod).into_iter().collect()
            }
            watcher::Event::Apply(pod) => {
                if let Some(run_id) = run_id_from_metadata(&pod.metadata) {
                    self.pods.insert(run_id);
                }
                map_pod_event(&pod).into_iter().collect()
            }
            watcher::Event::Delete(pod) => {
                if let Some(run_id) = run_id_from_metadata(&pod.metadata) {
                    self.pods.remove(&run_id);
                }
                map_pod_delete_event(&pod).into_iter().collect()
            }
            watcher::Event::InitDone => {
                let vanished: Vec<RunId> =
                    self.pods.difference(&self.relisted_pods).copied().collect();
                for run_id in &vanished {
                    self.pods.remove(run_id);
                }
                return self.gone_events(vanished, "pod no longer present");
            }
        };
        events
            .into_iter()
            .filter_map(|event| self.dedupe(event))
            .collect()
    }

    fn sandbox_event(&mut self, event: watcher::Event<Sandbox>) -> Vec<SandboxEvent> {
        match event {
            watcher::Event::Init => {
                self.relisted_sandboxes.clear();
                Vec::new()
            }
            watcher::Event::InitApply(sandbox) => {
                if let Some(run_id) = run_id_from_metadata(&sandbox.metadata) {
                    self.relisted_sandboxes.insert(run_id);
                    self.sandboxes.insert(run_id);
                }
                Vec::new()
            }
            watcher::Event::Apply(sandbox) => {
                if let Some(run_id) = run_id_from_metadata(&sandbox.metadata) {
                    self.sandboxes.insert(run_id);
                }
                Vec::new()
            }
            watcher::Event::Delete(sandbox) => {
                let Some(run_id) = run_id_from_metadata(&sandbox.metadata) else {
                    return Vec::new();
                };
                self.sandboxes.remove(&run_id);
                self.gone_events(vec![run_id], "sandbox deleted")
            }
            watcher::Event::InitDone => {
                let vanished: Vec<RunId> = self
                    .sandboxes
                    .difference(&self.relisted_sandboxes)
                    .copied()
                    .collect();
                for run_id in &vanished {
                    self.sandboxes.remove(run_id);
                }
                self.gone_events(vanished, "sandbox no longer present")
            }
        }
    }
}

/// Consumes the pod and sandbox watch streams until one of them ends and calls `emit` for every
/// resulting event. Stream errors are logged; the streams retry with backoff on their own.
async fn forward_events<P, S>(mut pods: P, mut sandboxes: S, emit: impl Fn(SandboxEvent))
where
    P: Stream<Item = Result<watcher::Event<Pod>, watcher::Error>> + Unpin,
    S: Stream<Item = Result<watcher::Event<Sandbox>, watcher::Error>> + Unpin,
{
    let mut tracker = EventTracker::default();
    loop {
        tokio::select! {
            res = pods.next() => match res {
                Some(Ok(event)) => tracker.pod_event(event).into_iter().for_each(&emit),
                Some(Err(err)) => warn!(error = %err, "pod reflector error"),
                None => break,
            },
            res = sandboxes.next() => match res {
                Some(Ok(event)) => tracker.sandbox_event(event).into_iter().for_each(&emit),
                Some(Err(err)) => warn!(error = %err, "sandbox reflector error"),
                None => break,
            },
        }
    }
}

#[cfg(test)]
mod tests;
