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
use k8s_openapi::api::core::v1::{PersistentVolumeClaim, Pod, Service};
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
/// A named `targetPort` is resolved through the Service's EndpointSlices, whose port entries are
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
        let slices: Api<EndpointSlice> = Api::namespaced(client.clone(), namespace);
        let lp = kube::api::ListParams::default()
            .labels(&format!("kubernetes.io/service-name={service}"));
        resolve_call(
            &format!("list EndpointSlices for `{namespace}/{service}`"),
            slices.list(&lp),
        )
        .await?
        .items
    } else {
        Vec::new()
    };

    let service_ports = svc_spec
        .and_then(|s| s.ports.as_deref())
        .unwrap_or_default();
    let mut target_ports: Vec<u16> = Vec::new();
    for p in service_ports {
        let port_num = u16::try_from(p.port)
            .map_err(|_| ResolveError::Permanent(format!("invalid Service port: {}", p.port)))?;
        if !configured_ports.is_empty() && !configured_ports.contains(&port_num) {
            continue;
        }
        match &p.target_port {
            Some(IntOrString::Int(tp)) => target_ports.push(u16::try_from(*tp).map_err(|_| {
                ResolveError::Permanent(format!("invalid numeric target port: {tp}"))
            })?),
            Some(IntOrString::String(target_name)) => {
                // EndpointSlice ports carry the Service port's name, not the targetPort name.
                // Pods may use different numbers for the same name (separate slices): take all.
                let service_port_name = p.name.as_deref().unwrap_or_default();
                let before = target_ports.len();
                for ep in endpoint_slices
                    .iter()
                    .filter_map(|s| s.ports.as_ref())
                    .flatten()
                    .filter(|ep| ep.name.as_deref().unwrap_or_default() == service_port_name)
                {
                    if let Some(pt) = ep.port.and_then(|pt| u16::try_from(pt).ok()) {
                        target_ports.push(pt);
                    }
                }
                if target_ports.len() == before {
                    return Err(ResolveError::Transient(format!(
                        "cannot resolve named targetPort `{target_name}` of port `{service_port_name}` of service `{namespace}/{service}`: no matching EndpointSlice port"
                    )));
                }
            }
            None => target_ports.push(port_num),
        }
    }
    if service_ports.is_empty() {
        // A Service without ports (headless): the operator's configured ports are the source.
        target_ports.extend_from_slice(configured_ports);
    } else if target_ports.is_empty() {
        return Err(ResolveError::Permanent(format!(
            "none of the configured ports {configured_ports:?} is a port of service `{namespace}/{service}`"
        )));
    }
    target_ports.sort_unstable();
    target_ports.dedup();
    if target_ports.is_empty() {
        // An empty list would render a rule that allows every port.
        return Err(ResolveError::Permanent(format!(
            "service `{namespace}/{service}` has no ports and none are configured"
        )));
    }

    let mut endpoint_ips = Vec::new();
    if pod_selector.is_none() || is_headless {
        for slice in &endpoint_slices {
            for ep in slice.endpoints.iter().flatten() {
                endpoint_ips.extend(ep.addresses.iter().filter_map(|a| a.parse::<IpAddr>().ok()));
            }
        }
    }

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
            last_good: Default::default(),
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
            {
                match waiting.reason.as_deref() {
                    Some("ErrImagePull")
                    | Some("ImagePullBackOff")
                    | Some("CreateContainerConfigError") => {
                        let msg = waiting.message.as_deref().unwrap_or_default();
                        let reason = if msg.is_empty() {
                            waiting.reason.clone().unwrap_or_default()
                        } else {
                            format!("{}: {}", waiting.reason.as_deref().unwrap_or_default(), msg)
                        };
                        return Some(SandboxEvent::Gone { run_id, reason });
                    }
                    _ => {}
                }
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
        let pod_watcher = watcher::watcher(pod_api, watcher::Config::default().labels(&selector))
            .default_backoff();
        let pod_reflector = reflector::reflector(pod_writer, pod_watcher);

        let (sb_store, sb_writer) = reflector::store();
        let sb_api: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
        let sb_watcher = watcher::watcher(sb_api, watcher::Config::default().labels(&selector))
            .default_backoff();
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
        let _ = self.events_tx.send(event);
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
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateWaiting, ContainerStatus, PodStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
    use kube::runtime::reflector;
    use std::collections::BTreeMap;
    use std::net::Ipv4Addr;
    use uuid::Uuid;

    fn test_config() -> ProvisioningConfig {
        ProvisioningConfig::from_sandbox_config(
            "test-instance".to_string(),
            &df_core::SandboxProvisioningConfig::default(),
            crate::config::ControlPlaneEndpoint {
                ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                port: 9443,
                tls_server_name: "cp.local".to_string(),
            },
            crate::config::GatewayEndpoint {
                ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                proxy_port: 3128,
            },
        )
    }

    #[test]
    fn test_parse_spiffe_san() {
        assert_eq!(
            parse_spiffe_san("spiffe://dark-factory/ns/test-ns/pod/my-pod/uid-123"),
            Some(("test-ns", "my-pod", "uid-123"))
        );
        assert_eq!(parse_spiffe_san("https://example.com"), None);
        assert_eq!(
            parse_spiffe_san("spiffe://dark-factory/ns/test-ns/pod/my-pod"),
            None
        );
        assert_eq!(
            parse_spiffe_san("spiffe://dark-factory/ns/test-ns/pod/my-pod/uid/extra"),
            None
        );
    }

    #[test]
    fn test_resolve_identity_scenarios() {
        let (pod_store, mut pod_writer) = reflector::store();
        let (sandbox_store, mut sb_writer) = reflector::store();

        let run_id = RunId::from_uuid(Uuid::from_u128(42));
        let sandbox_ns = "dark-factory-dev-sandboxes";
        let instance_name = "test-instance";

        // Seed Sandbox
        let sandbox = Sandbox {
            metadata: ObjectMeta {
                name: Some("run-df-1-1".to_string()),
                namespace: Some(sandbox_ns.to_string()),
                uid: Some("sb-uid-1".to_string()),
                labels: Some(BTreeMap::from([
                    (
                        "dark-factory.io/instance".to_string(),
                        instance_name.to_string(),
                    ),
                    ("dark-factory.io/run-id".to_string(), run_id.to_string()),
                ])),
                ..Default::default()
            },
            spec: crate::crd::SandboxSpec {
                pod_template: crate::crd::SandboxPodTemplate::default(),
                ..Default::default()
            },
            status: None,
        };
        sb_writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(sandbox));

        // Seed Pod
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("run-df-1-1".to_string()),
                namespace: Some(sandbox_ns.to_string()),
                uid: Some("pod-uid-1".to_string()),
                owner_references: Some(vec![OwnerReference {
                    api_version: "agents.x-k8s.io/v1beta1".to_string(),
                    kind: "Sandbox".to_string(),
                    name: "run-df-1-1".to_string(),
                    uid: "sb-uid-1".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            status: Some(PodStatus {
                pod_ip: Some("10.0.3.7".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        pod_writer.apply_watcher_event(&kube::runtime::watcher::Event::Apply(pod));

        // Scenario: Valid peer
        let valid_peer = PeerIdentity {
            uri_san: format!("spiffe://dark-factory/ns/{sandbox_ns}/pod/run-df-1-1/pod-uid-1"),
            peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
        };
        assert_eq!(
            resolve_identity_from_stores(
                &valid_peer,
                sandbox_ns,
                instance_name,
                &pod_store,
                &sandbox_store
            ),
            Some(run_id)
        );

        // Scenario: Peer IP mismatch
        let wrong_ip_peer = PeerIdentity {
            uri_san: format!("spiffe://dark-factory/ns/{sandbox_ns}/pod/run-df-1-1/pod-uid-1"),
            peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 99)),
        };
        assert_eq!(
            resolve_identity_from_stores(
                &wrong_ip_peer,
                sandbox_ns,
                instance_name,
                &pod_store,
                &sandbox_store
            ),
            None
        );

        // Scenario: UID mismatch after pod recreation
        let old_uid_peer = PeerIdentity {
            uri_san: format!("spiffe://dark-factory/ns/{sandbox_ns}/pod/run-df-1-1/old-uid"),
            peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
        };
        assert_eq!(
            resolve_identity_from_stores(
                &old_uid_peer,
                sandbox_ns,
                instance_name,
                &pod_store,
                &sandbox_store
            ),
            None
        );

        // Scenario: Malformed SAN
        let malformed_peer = PeerIdentity {
            uri_san: "spiffe://other/bad".to_string(),
            peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
        };
        assert_eq!(
            resolve_identity_from_stores(
                &malformed_peer,
                sandbox_ns,
                instance_name,
                &pod_store,
                &sandbox_store
            ),
            None
        );

        // Scenario: Pod not created by this instance
        assert_eq!(
            resolve_identity_from_stores(
                &valid_peer,
                sandbox_ns,
                "other-instance",
                &pod_store,
                &sandbox_store
            ),
            None
        );
    }

    #[test]
    fn test_map_pod_event() {
        let run_id = RunId::from_uuid(Uuid::from_u128(99));
        let mut pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                labels: Some(BTreeMap::from([(
                    "dark-factory.io/run-id".to_string(),
                    run_id.to_string(),
                )])),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                pod_ip: Some("10.0.3.7".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // 1. Running with IP -> Started
        assert_eq!(
            map_pod_event(&pod),
            Some(SandboxEvent::Started {
                run_id,
                pod_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
            })
        );

        // 2. Image pull failure -> Gone
        pod.status = Some(PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "agent".to_string(),
                image: "bad:tag".to_string(),
                image_id: "".to_string(),
                ready: false,
                restart_count: 0,
                state: Some(ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("ImagePullBackOff".to_string()),
                        message: Some("Back-off pulling image bad:tag".to_string()),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        });
        assert_eq!(
            map_pod_event(&pod),
            Some(SandboxEvent::Gone {
                run_id,
                reason: "ImagePullBackOff: Back-off pulling image bad:tag".to_string(),
            })
        );

        // 3. Deletion timestamp -> Gone
        pod.metadata.deletion_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                k8s_openapi::jiff::Timestamp::now(),
            ));
        assert_eq!(
            map_pod_event(&pod),
            Some(SandboxEvent::Gone {
                run_id,
                reason: "pod deleted".to_string(),
            })
        );
    }

    #[tokio::test]
    #[ignore = "requires live dev cluster"]
    async fn test_provider_live_cluster_integration() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::try_default().await.expect("cluster connection");
        let config = crate::test_support::live_config();

        let (pod_store, _) = reflector::store();
        let (sb_store, _) = reflector::store();
        let targets = Arc::new(StaticClusterTargetsProvider::new(vec![]));
        let provider =
            K8sSandboxProvider::new(client.clone(), config.clone(), targets, pod_store, sb_store);

        let task_key = "DF-999".parse().unwrap();
        let task = TaskVolumeSpec::for_task(task_key);
        provider
            .ensure_task_volume(&task)
            .await
            .expect("ensure task volume");

        let run_id = RunId::new();
        let spec = RunSandboxSpec {
            run_id,
            run_number: 1,
            task_key: task.task_key.clone(),
            project_id: df_core::ids::ProjectId::new(),
            project_key: df_core::keys::ProjectKey::new("DF").unwrap(),
            persona_id: df_core::ids::PersonaId::new(),
            image: "alpine:latest".to_string(),
            harness: None,
            volume_name: task.volume_name.clone(),
            labels: BTreeMap::new(),
        };

        provider.provision(&spec).await.expect("provision sandbox");

        // Idempotent teardown
        provider.teardown(run_id).await.expect("teardown run");
        provider
            .teardown(run_id)
            .await
            .expect("teardown second time idempotent");

        // The task volume outlives the run ("Run ended (PVC kept)") ...
        let pvcs: Api<PersistentVolumeClaim> =
            Api::namespaced(client.clone(), &config.sandbox.namespace);
        assert_eq!(task.volume_name, "task-df-999");
        pvcs.get(&task.volume_name)
            .await
            .expect("the PVC survives the teardown of its run");

        // ... and the next run of the task mounts the same claim ("Second run reuses the volume").
        provider
            .ensure_task_volume(&task)
            .await
            .expect("ensure task volume again");
        let second = RunSandboxSpec {
            run_id: RunId::new(),
            run_number: 2,
            ..spec.clone()
        };
        provider.provision(&second).await.expect("provision run 2");
        let sandboxes: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
        let sandbox = sandboxes
            .get("run-df-999-2")
            .await
            .expect("second run's sandbox");
        let claims: Vec<String> = sandbox
            .spec
            .pod_template
            .spec
            .volumes
            .iter()
            .flatten()
            .filter_map(|v| v.persistent_volume_claim.as_ref())
            .map(|pvc| pvc.claim_name.clone())
            .collect();
        assert_eq!(claims, std::slice::from_ref(&task.volume_name));
        let task_pvcs = pvcs
            .list(&kube::api::ListParams::default().labels("dark-factory.io/task-key=DF-999"))
            .await
            .expect("list task PVCs");
        assert_eq!(task_pvcs.items.len(), 1, "one PVC per task");

        // Cleanup
        provider
            .teardown(second.run_id)
            .await
            .expect("teardown run 2");
        let _ = pvcs
            .delete(&task.volume_name, &DeleteParams::default())
            .await;
    }

    /// Resolves a Service whose port `grpc` has the named targetPort `grpc-internal`, answering
    /// the EndpointSlice list with a port called `slice_port_name`.
    async fn resolve_named_target_port(
        slice_port_name: &str,
    ) -> Result<ResolvedClusterTarget, ResolveError> {
        use tower_test::mock;

        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "default");

        let resolve_task = tokio::spawn(async move {
            resolve_cluster_target(&client, "target-ns", "my-svc", &[50051], false).await
        });

        let (req, send_resp) = handle.next_request().await.expect("received svc GET");
        assert_eq!(
            req.uri().path(),
            "/api/v1/namespaces/target-ns/services/my-svc"
        );
        let svc_json = serde_json::json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": { "name": "my-svc", "namespace": "target-ns" },
            "spec": {
                "clusterIP": "10.96.10.20",
                "selector": { "app": "my-svc" },
                "ports": [
                    { "name": "grpc", "port": 50051, "targetPort": "grpc-internal" }
                ]
            }
        });
        send_resp.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&svc_json).unwrap())
                .unwrap(),
        );

        // The targetPort is a name, so the EndpointSlices are listed.
        let (req, send_resp) = handle.next_request().await.expect("received slice LIST");
        assert_eq!(
            req.uri().path(),
            "/apis/discovery.k8s.io/v1/namespaces/target-ns/endpointslices"
        );
        let slice_list_json = serde_json::json!({
            "apiVersion": "discovery.k8s.io/v1",
            "kind": "EndpointSliceList",
            "metadata": {},
            "items": [{
                "metadata": { "name": "my-svc-abc", "namespace": "target-ns" },
                "addressType": "IPv4",
                "ports": [{ "name": slice_port_name, "port": 8080 }],
                "endpoints": [{ "addresses": ["10.244.1.5"] }]
            }]
        });
        send_resp.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&slice_list_json).unwrap())
                .unwrap(),
        );

        resolve_task.await.unwrap()
    }

    #[tokio::test]
    async fn named_target_port_is_resolved_via_the_service_port_name() {
        // Kubernetes names the EndpointSlice port after the Service port (`grpc`), not after the
        // targetPort (`grpc-internal`).
        let target = resolve_named_target_port("grpc")
            .await
            .expect("resolve success");
        assert_eq!(target.service, "my-svc");
        assert_eq!(target.namespace, "target-ns");
        assert_eq!(
            target.cluster_ip,
            Some(IpAddr::V4(Ipv4Addr::new(10, 96, 10, 20)))
        );
        assert_eq!(target.target_ports, vec![8080]);
    }

    #[tokio::test]
    async fn unresolvable_named_target_port_is_an_error_not_a_guess() {
        let err = resolve_named_target_port("other")
            .await
            .expect_err("no EndpointSlice port is called `grpc`");
        assert!(
            matches!(&err, ResolveError::Transient(msg) if msg.contains("grpc-internal")),
            "{err}"
        );
    }

    fn pod_for(run_id: RunId, phase: &str, ip: Option<&str>) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(format!("pod-{run_id}")),
                labels: Some(BTreeMap::from([(
                    "dark-factory.io/run-id".to_string(),
                    run_id.to_string(),
                )])),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some(phase.to_string()),
                pod_ip: ip.map(str::to_string),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn image_pull_failure_pod(run_id: RunId) -> Pod {
        let mut pod = pod_for(run_id, "Pending", None);
        pod.status = Some(PodStatus {
            phase: Some("Pending".to_string()),
            container_statuses: Some(vec![ContainerStatus {
                name: "agent".to_string(),
                image: "bad:tag".to_string(),
                image_id: String::new(),
                ready: false,
                restart_count: 0,
                state: Some(ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("ErrImagePull".to_string()),
                        message: Some("pull access denied".to_string()),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        });
        pod
    }

    /// Feeds `pod_events` through [`forward_events`] and returns what was emitted.
    async fn run_loop(pod_events: Vec<watcher::Event<Pod>>) -> Vec<SandboxEvent> {
        let emitted = std::sync::Mutex::new(Vec::new());
        let pods = futures::stream::iter(pod_events.into_iter().map(Ok));
        let sandboxes = futures::stream::pending();
        forward_events(pods, sandboxes, |event| emitted.lock().unwrap().push(event)).await;
        emitted.into_inner().unwrap()
    }

    #[tokio::test]
    async fn event_loop_emits_gone_for_image_pull_failure_without_a_prior_started() {
        // Also the state after a control plane restart: nothing was ever `Started`.
        let run_id = RunId::from_uuid(Uuid::from_u128(1));
        let events = run_loop(vec![
            watcher::Event::Apply(image_pull_failure_pod(run_id)),
            // The kubelet keeps updating the waiting pod; the run must be reported once.
            watcher::Event::Apply(image_pull_failure_pod(run_id)),
        ])
        .await;

        assert_eq!(
            events,
            vec![SandboxEvent::Gone {
                run_id,
                reason: "ErrImagePull: pull access denied".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn event_loop_emits_gone_for_failed_pod_and_started_once() {
        let started = RunId::from_uuid(Uuid::from_u128(2));
        let failed = RunId::from_uuid(Uuid::from_u128(3));
        let events = run_loop(vec![
            watcher::Event::Apply(pod_for(started, "Running", Some("10.0.0.5"))),
            watcher::Event::Apply(pod_for(started, "Running", Some("10.0.0.5"))),
            watcher::Event::Apply(pod_for(failed, "Failed", None)),
            watcher::Event::Delete(pod_for(failed, "Failed", None)),
        ])
        .await;

        assert_eq!(
            events,
            vec![
                SandboxEvent::Started {
                    run_id: started,
                    pod_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))
                },
                SandboxEvent::Gone {
                    run_id: failed,
                    reason: "pod failed".to_string()
                },
            ]
        );
    }

    #[tokio::test]
    async fn event_loop_emits_gone_for_pods_that_vanished_during_a_relist() {
        let kept = RunId::from_uuid(Uuid::from_u128(4));
        let vanished = RunId::from_uuid(Uuid::from_u128(5));
        let events = run_loop(vec![
            watcher::Event::Apply(pod_for(kept, "Running", Some("10.0.0.1"))),
            watcher::Event::Apply(pod_for(vanished, "Running", Some("10.0.0.2"))),
            // Watch dropped; the relist only returns `kept` and never sends a `Delete`.
            watcher::Event::Init,
            watcher::Event::InitApply(pod_for(kept, "Running", Some("10.0.0.1"))),
            watcher::Event::InitDone,
        ])
        .await;

        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!(
            events[2],
            SandboxEvent::Gone {
                run_id: vanished,
                reason: "pod no longer present".to_string()
            }
        );
    }

    fn sandbox_for(run_id: RunId) -> Sandbox {
        Sandbox {
            metadata: pod_for(run_id, "Pending", None).metadata,
            spec: crate::crd::SandboxSpec {
                pod_template: crate::crd::SandboxPodTemplate::default(),
                ..Default::default()
            },
            status: None,
        }
    }

    #[test]
    fn relist_reports_never_running_pods_and_sandboxes_that_vanished() {
        let run_id = RunId::from_uuid(Uuid::from_u128(8));
        let mut tracker = EventTracker::default();
        // Pending pod: no event, but the pod and its Sandbox are known.
        assert!(
            tracker
                .pod_event(watcher::Event::Apply(pod_for(run_id, "Pending", None)))
                .is_empty()
        );
        assert!(
            tracker
                .sandbox_event(watcher::Event::Apply(sandbox_for(run_id)))
                .is_empty()
        );

        // Both watches restart and the relists return nothing.
        assert!(tracker.sandbox_event(watcher::Event::Init).is_empty());
        let from_sandboxes = tracker.sandbox_event(watcher::Event::InitDone);
        assert!(tracker.pod_event(watcher::Event::Init).is_empty());
        let from_pods = tracker.pod_event(watcher::Event::InitDone);

        // One Gone for the run, whichever watch reports it first.
        let all: Vec<_> = from_sandboxes.into_iter().chain(from_pods).collect();
        assert_eq!(all.len(), 1, "{all:?}");
        assert!(matches!(&all[0], SandboxEvent::Gone { run_id: r, .. } if *r == run_id));
    }

    #[tokio::test]
    async fn tracker_emits_one_gone_when_the_sandbox_is_deleted() {
        let run_id = RunId::from_uuid(Uuid::from_u128(6));
        let mut tracker = EventTracker::default();
        let sandbox = Sandbox {
            metadata: pod_for(run_id, "Running", None).metadata,
            spec: crate::crd::SandboxSpec {
                pod_template: crate::crd::SandboxPodTemplate::default(),
                ..Default::default()
            },
            status: None,
        };

        let first = tracker.sandbox_event(watcher::Event::Delete(sandbox.clone()));
        assert_eq!(
            first,
            vec![SandboxEvent::Gone {
                run_id,
                reason: "sandbox deleted".to_string()
            }]
        );
        assert!(
            tracker
                .sandbox_event(watcher::Event::Delete(sandbox))
                .is_empty()
        );
        assert!(
            tracker
                .pod_event(watcher::Event::Delete(pod_for(run_id, "Running", None)))
                .is_empty()
        );
    }

    #[tokio::test]
    async fn ensure_task_volume_times_out_when_the_api_does_not_answer() {
        use tower_test::mock;

        tokio::time::pause();
        // Keep the handle alive but never answer.
        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let client = Client::new(mock_service, "default");
        let (pod_store, _) = reflector::store();
        let (sb_store, _) = reflector::store();
        let provider = K8sSandboxProvider::new(
            client,
            test_config(),
            Arc::new(StaticClusterTargetsProvider::new(vec![])),
            pod_store,
            sb_store,
        );

        let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
        let err = provider.ensure_task_volume(&task).await.unwrap_err();
        assert!(
            matches!(&err, ProviderError::Backend(msg) if msg.contains("timeout")),
            "{err}"
        );
    }

    #[tokio::test]
    async fn provision_without_a_harness_image_is_invalid_and_names_the_harness() {
        let (mock_service, _handle) =
            tower_test::mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let (pod_store, _) = reflector::store();
        let (sb_store, _) = reflector::store();
        let config = ProvisioningConfig {
            harness_images: BTreeMap::new(),
            ..test_config()
        };
        let provider = K8sSandboxProvider::new(
            Client::new(mock_service, "default"),
            config,
            Arc::new(StaticClusterTargetsProvider::new(vec![])),
            pod_store,
            sb_store,
        );
        let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
        let spec = RunSandboxSpec {
            run_id: RunId::new(),
            run_number: 1,
            task_key: task.task_key.clone(),
            project_id: df_core::ids::ProjectId::new(),
            project_key: df_core::keys::ProjectKey::new("DF").unwrap(),
            persona_id: df_core::ids::PersonaId::new(),
            image: "alpine:latest".to_string(),
            harness: Some(df_core::CLAUDE_CODE_HARNESS.to_string()),
            volume_name: task.volume_name.clone(),
            labels: BTreeMap::new(),
        };

        let err = provider.provision(&spec).await.unwrap_err();
        assert!(
            matches!(&err, ProviderError::InvalidSpec(msg) if msg.contains("claude-code")),
            "{err}"
        );
    }

    #[tokio::test]
    async fn events_stream_survives_lag_and_keeps_later_events() {
        use futures::StreamExt as _;
        let (pod_store, _) = reflector::store();
        let (sb_store, _) = reflector::store();
        let (mock_service, _handle) =
            tower_test::mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let provider = K8sSandboxProvider::new(
            Client::new(mock_service, "default"),
            test_config(),
            Arc::new(StaticClusterTargetsProvider::new(vec![])),
            pod_store,
            sb_store,
        );
        let mut events = provider.events();
        // The channel holds 128 events: overflow it, then send one more that must still arrive.
        for n in 0..200_u128 {
            provider.emit_event(SandboxEvent::Gone {
                run_id: RunId::from_uuid(Uuid::from_u128(n)),
                reason: "x".to_string(),
            });
        }
        // The lagged-over events are dropped; the 128 newest still arrive, newest last.
        let mut run_ids = Vec::new();
        for _ in 0..128 {
            let event = events.next().await.expect("stream stays open after lag");
            run_ids.push(match event {
                SandboxEvent::Gone { run_id, .. } | SandboxEvent::Started { run_id, .. } => run_id,
            });
        }
        assert_eq!(
            run_ids.first(),
            Some(&RunId::from_uuid(Uuid::from_u128(72)))
        );
        assert_eq!(
            run_ids.last(),
            Some(&RunId::from_uuid(Uuid::from_u128(199)))
        );
    }

    fn rule(ns: &str, svc: &str) -> ClusterServiceRef {
        ClusterServiceRef {
            namespace: ns.to_string(),
            service: svc.to_string(),
            ports: vec![80],
            allow_dns: false,
        }
    }

    type MockHandle =
        tower_test::mock::Handle<http::Request<kube::client::Body>, http::Response<String>>;

    /// Answers the next request with `status`: a Service for 200, an API error Status otherwise.
    /// With `headless_without_ports` the answered Service is headless.
    async fn answer_service(handle: &mut MockHandle, status: u16) {
        answer_service_as(handle, status, false).await;
    }

    async fn answer_service_as(handle: &mut MockHandle, status: u16, headless: bool) {
        let (req, send) = handle.next_request().await.expect("service GET");
        let name = req.uri().path().rsplit('/').next().unwrap().to_string();
        let body = if status == 200 {
            serde_json::json!({
                "apiVersion": "v1", "kind": "Service",
                "metadata": { "name": name },
                "spec": {
                    "clusterIP": if headless { "None" } else { "10.0.0.9" },
                    "selector": { "app": "x" },
                    "ports": [{ "port": 80 }]
                }
            })
        } else {
            serde_json::json!({ "kind": "Status", "apiVersion": "v1", "code": status, "message": "boom" })
        };
        send.send_response(
            http::Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .unwrap(),
        );
    }

    /// Runs `resolve(scope, rules)` and answers its Service lookups with `statuses`.
    async fn resolve_answering(
        resolver: &Arc<ClusterTargetResolver>,
        handle: &mut MockHandle,
        scope: Option<&'static str>,
        rules: &[ClusterServiceRef],
        statuses: &[u16],
    ) -> Vec<ResolvedClusterTarget> {
        let (r, rl) = (resolver.clone(), rules.to_vec());
        let task = tokio::spawn(async move { r.resolve(scope, &rl).await });
        for status in statuses {
            answer_service(handle, *status).await;
        }
        task.await.unwrap()
    }

    fn new_resolver() -> (Arc<ClusterTargetResolver>, MockHandle) {
        let (mock_service, handle) = tower_test::mock::pair();
        (
            Arc::new(ClusterTargetResolver::new(Client::new(
                mock_service,
                "default",
            ))),
            handle,
        )
    }

    #[tokio::test]
    async fn one_unresolvable_rule_does_not_block_the_others() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "broken"), rule("b", "ok")];
        let targets = resolve_answering(&resolver, &mut handle, None, &rules, &[404, 200]).await;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].service, "ok");
    }

    #[tokio::test]
    async fn a_server_error_reuses_the_last_good_resolution() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        assert_eq!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[200])
                .await
                .len(),
            1
        );
        let kept = resolve_answering(&resolver, &mut handle, None, &rules, &[500]).await;
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].service, "svc");
    }

    #[tokio::test]
    async fn a_timeout_reuses_the_last_good_resolution() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

        tokio::time::pause();
        let (r, rl) = (resolver.clone(), rules.clone());
        let task = tokio::spawn(async move { r.resolve(None, &rl).await });
        // Receive the request but never answer it; the call runs into K8S_API_TIMEOUT.
        let _unanswered = handle.next_request().await.expect("service GET");
        let kept = task.await.unwrap();
        assert_eq!(kept.len(), 1);
    }

    #[tokio::test]
    async fn a_404_evicts_the_cache_and_skips_the_rule() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

        assert!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[404])
                .await
                .is_empty()
        );
        // Evicted: a later server error has nothing to fall back to.
        assert!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[500])
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_forbidden_answer_evicts_the_cache_and_skips_the_rule() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;
        assert!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[403])
                .await
                .is_empty()
        );
        assert!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[500])
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_service_that_became_headless_evicts_the_cache_and_skips_the_rule() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

        // Same rule, but the Service is headless now and the rule has no `allowDns`.
        let (r, rl) = (resolver.clone(), rules.clone());
        let task = tokio::spawn(async move { r.resolve(None, &rl).await });
        answer_service_as(&mut handle, 200, true).await;
        assert!(task.await.unwrap().is_empty());

        assert!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[500])
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_rule_that_was_removed_and_added_again_has_no_history() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

        // Removed from the allowlist: no lookups, and the cache entry is pruned.
        assert!(
            resolve_answering(&resolver, &mut handle, None, &[], &[])
                .await
                .is_empty()
        );
        // Added again while the API fails: must not resurrect the old resolution.
        assert!(
            resolve_answering(&resolver, &mut handle, None, &rules, &[500])
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn pruning_one_project_scope_keeps_the_cache_of_the_others() {
        let (resolver, mut handle) = new_resolver();
        let rules = vec![rule("a", "svc")];
        resolve_answering(&resolver, &mut handle, Some("ALPHA"), &rules, &[200]).await;
        // Another project, with a different rule set, resolves in between.
        resolve_answering(&resolver, &mut handle, Some("BETA"), &[], &[]).await;

        let kept = resolve_answering(&resolver, &mut handle, Some("ALPHA"), &rules, &[500]).await;
        assert_eq!(kept.len(), 1);
    }

    async fn resolve_with_service(
        spec: serde_json::Value,
        ports: &[u16],
        dns: bool,
        lists_slices: bool,
    ) -> Result<ResolvedClusterTarget, ResolveError> {
        let (mock_service, mut handle) = tower_test::mock::pair();
        let client = Client::new(mock_service, "default");
        let ports = ports.to_vec();
        let task =
            tokio::spawn(
                async move { resolve_cluster_target(&client, "ns", "svc", &ports, dns).await },
            );
        let (_, send) = handle.next_request().await.expect("service GET");
        send.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::json!({
                        "apiVersion": "v1", "kind": "Service",
                        "metadata": { "name": "svc", "namespace": "ns" },
                        "spec": spec
                    })
                    .to_string(),
                )
                .unwrap(),
        );
        // Headless / selector-less services also list EndpointSlices.
        if lists_slices {
            let (_, send) = handle.next_request().await.expect("slice LIST");
            send.send_response(
                http::Response::builder()
                    .status(200)
                    .header("Content-Type", "application/json")
                    .body(
                        serde_json::json!({
                            "apiVersion": "discovery.k8s.io/v1", "kind": "EndpointSliceList",
                            "metadata": {}, "items": []
                        })
                        .to_string(),
                    )
                    .unwrap(),
            );
        }
        task.await.unwrap()
    }

    #[tokio::test]
    async fn headless_service_without_allow_dns_is_rejected_at_resolution() {
        let err = resolve_with_service(
            serde_json::json!({ "clusterIP": "None", "ports": [{ "port": 80 }] }),
            &[],
            false,
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ResolveError::Permanent(m) if m.contains("allowDns")));
    }

    #[tokio::test]
    async fn headless_service_without_any_port_is_rejected_not_rendered_portless() {
        let err = resolve_with_service(serde_json::json!({ "clusterIP": "None" }), &[], true, true)
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::Permanent(m) if m.contains("no ports")));
    }

    #[tokio::test]
    async fn configured_ports_that_match_no_service_port_are_an_error() {
        let err = resolve_with_service(
            serde_json::json!({ "clusterIP": "10.0.0.1", "selector": {"a": "b"}, "ports": [{ "port": 80 }] }),
            &[9999],
            false,
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ResolveError::Permanent(m) if m.contains("9999")));
    }

    #[tokio::test]
    async fn named_target_port_collects_the_ports_of_all_slices() {
        use tower_test::mock;
        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "default");
        let task = tokio::spawn(async move {
            resolve_cluster_target(&client, "ns", "svc", &[80], false).await
        });
        let (_, send) = handle.next_request().await.unwrap();
        send.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::json!({
                        "apiVersion": "v1", "kind": "Service",
                        "metadata": { "name": "svc", "namespace": "ns" },
                        "spec": { "clusterIP": "10.0.0.1", "selector": {"a": "b"},
                                  "ports": [{ "name": "web", "port": 80, "targetPort": "http" }] }
                    })
                    .to_string(),
                )
                .unwrap(),
        );
        let (_, send) = handle.next_request().await.unwrap();
        let slice = |name: &str, port: i32| {
            serde_json::json!({
                "metadata": { "name": name, "namespace": "ns" },
                "addressType": "IPv4",
                "ports": [{ "name": "web", "port": port }],
                "endpoints": [{ "addresses": ["10.1.0.1"] }]
            })
        };
        send.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::json!({
                        "apiVersion": "discovery.k8s.io/v1", "kind": "EndpointSliceList",
                        "metadata": {}, "items": [slice("a", 8080), slice("b", 9090)]
                    })
                    .to_string(),
                )
                .unwrap(),
        );
        assert_eq!(task.await.unwrap().unwrap().target_ports, vec![8080, 9090]);
    }
}
