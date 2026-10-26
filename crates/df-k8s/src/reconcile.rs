//! Lease leader election and active run NetworkPolicy reconciliation.

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use df_core::config::ClusterTargetsConfig;
use df_core::keys::TaskKey;
use df_core::seams::ProviderError;
use futures::stream::{self, BoxStream};
use futures::{Stream, StreamExt as _, future};
use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::Service;
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::networking::v1::NetworkPolicy;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta};
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::{WatchStreamExt as _, watcher};
use kube::{Client, Resource, ResourceExt};
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::K8S_API_TIMEOUT;
use crate::config::ProvisioningConfig;
use crate::crd::Sandbox;
use crate::provider::{ClusterTargetsProvider, run_id_from_metadata};
use crate::render::render_network_policy;

/// Default duration in seconds for leader election coordination leases.
pub const DEFAULT_LEASE_DURATION_SECONDS: i32 = 15;

/// Notifications arriving within this window of the first one are served by a single reconcile
/// pass (with `anyNamespace`, every EndpointSlice change in the cluster notifies).
const NOTIFY_DEBOUNCE: Duration = Duration::from_secs(1);

/// Attempts to acquire or renew a Kubernetes coordination Lease.
pub async fn try_acquire_or_renew_lease(
    client: &Client,
    namespace: &str,
    lease_name: &str,
    identity: &str,
    duration_seconds: i32,
) -> Result<bool, kube::Error> {
    let leases: Api<Lease> = Api::namespaced(client.clone(), namespace);
    let now = k8s_openapi::jiff::Timestamp::now();
    let micro_now = MicroTime(now);

    let get_res = match tokio::time::timeout(K8S_API_TIMEOUT, leases.get(lease_name)).await {
        Ok(res) => res,
        Err(_) => {
            warn!(lease_name, "timeout querying lease");
            return Ok(false);
        }
    };

    match get_res {
        Ok(mut lease) => {
            let spec = lease.spec.get_or_insert_with(Default::default);
            let is_mine = spec.holder_identity.as_deref() == Some(identity);
            let is_expired = if let Some(ref renew) = spec.renew_time {
                let duration = i64::from(spec.lease_duration_seconds.unwrap_or(duration_seconds));
                let elapsed_secs = (now.as_second() - renew.0.as_second()).max(0);
                elapsed_secs > duration
            } else {
                true
            };

            if is_mine || is_expired {
                if !is_mine {
                    spec.acquire_time = Some(micro_now.clone());
                    spec.lease_transitions = Some(spec.lease_transitions.unwrap_or(0) + 1);
                }
                spec.holder_identity = Some(identity.to_string());
                spec.lease_duration_seconds = Some(duration_seconds);
                spec.renew_time = Some(micro_now);

                match tokio::time::timeout(
                    K8S_API_TIMEOUT,
                    leases.replace(lease_name, &Default::default(), &lease),
                )
                .await
                {
                    Ok(res) => {
                        res?;
                        Ok(true)
                    }
                    Err(_) => {
                        warn!(lease_name, "timeout renewing lease");
                        Ok(false)
                    }
                }
            } else {
                Ok(false)
            }
        }
        Err(kube::Error::Api(err)) if err.code == 404 => {
            let lease = Lease {
                metadata: ObjectMeta {
                    name: Some(lease_name.to_string()),
                    namespace: Some(namespace.to_string()),
                    ..Default::default()
                },
                spec: Some(LeaseSpec {
                    holder_identity: Some(identity.to_string()),
                    lease_duration_seconds: Some(duration_seconds),
                    acquire_time: Some(micro_now.clone()),
                    renew_time: Some(micro_now),
                    lease_transitions: Some(1),
                    ..Default::default()
                }),
            };
            let create_res = match tokio::time::timeout(
                K8S_API_TIMEOUT,
                leases.create(&Default::default(), &lease),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => {
                    warn!(lease_name, "timeout creating lease");
                    return Ok(false);
                }
            };
            match create_res {
                Ok(_) => Ok(true),
                Err(kube::Error::Api(e)) if e.code == 409 => Ok(false),
                Err(e) => Err(e),
            }
        }
        Err(err) => Err(err),
    }
}

/// Releases the lease by clearing the holder identity and renew time on graceful shutdown.
pub async fn release_lease(
    client: &Client,
    namespace: &str,
    lease_name: &str,
    identity: &str,
) -> Result<(), kube::Error> {
    let leases: Api<Lease> = Api::namespaced(client.clone(), namespace);
    let get_res = tokio::time::timeout(K8S_API_TIMEOUT, leases.get(lease_name)).await;
    let mut lease = match get_res {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            warn!(lease_name, "timeout reading lease during release");
            return Ok(());
        }
    };

    if let Some(ref mut spec) = lease.spec
        && spec.holder_identity.as_deref() == Some(identity)
    {
        spec.holder_identity = None;
        spec.renew_time = None; // Reset renew_time so successors immediately see it expired without waiting
        let replace_res = tokio::time::timeout(
            K8S_API_TIMEOUT,
            leases.replace(lease_name, &Default::default(), &lease),
        )
        .await;
        match replace_res {
            Ok(Ok(_)) => {
                info!(
                    lease_name,
                    identity, "successfully released leadership lease"
                );
            }
            Ok(Err(e)) => {
                warn!(lease_name, error = %e, "failed to update lease during release");
                return Err(e);
            }
            Err(_) => {
                warn!(lease_name, "timeout replacing lease during release");
            }
        }
    }
    Ok(())
}

/// Reconciles all active runs' NetworkPolicies in `config.sandbox.namespace`.
pub async fn reconcile_active_runs(
    client: &Client,
    config: &ProvisioningConfig,
    targets_provider: &dyn ClusterTargetsProvider,
) -> Result<usize, ProviderError> {
    let sandboxes: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
    let netpols: Api<NetworkPolicy> = Api::namespaced(client.clone(), &config.sandbox.namespace);

    let list_params = kube::api::ListParams::default().labels(&format!(
        "dark-factory.io/instance={}",
        config.instance_name
    ));

    let list_fut = tokio::time::timeout(K8S_API_TIMEOUT, sandboxes.list(&list_params));
    let matching = match list_fut.await {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            return Err(ProviderError::Backend(format!(
                "failed to list sandboxes for reconcile: {e}"
            )));
        }
        Err(_) => {
            return Err(ProviderError::Backend(
                "timeout listing sandboxes for reconcile".to_string(),
            ));
        }
    };

    let mut count = 0;
    for sb in matching {
        if sb.metadata.deletion_timestamp.is_some() {
            continue;
        }

        let sb_name = sb.metadata.name.as_deref().unwrap_or("<unknown>");
        let Some(run_id) = run_id_from_metadata(&sb.metadata) else {
            warn!(
                sandbox = sb_name,
                "sandbox has a missing or invalid dark-factory.io/run-id label; skipping reconcile"
            );
            continue;
        };
        let labels = sb.metadata.labels.clone().unwrap_or_default();

        let task_key_str = match labels.get("dark-factory.io/task-key") {
            Some(s) => s,
            None => {
                warn!(
                    sandbox = sb_name,
                    "sandbox missing dark-factory.io/task-key label; skipping reconcile"
                );
                continue;
            }
        };
        let task_key = match task_key_str.parse::<TaskKey>() {
            Ok(k) => k,
            Err(e) => {
                warn!(sandbox = sb_name, task_key = task_key_str, error = %e, "invalid dark-factory.io/task-key label; skipping reconcile");
                continue;
            }
        };

        let project_key_str = labels.get("dark-factory.io/project").map(String::as_str);

        let targets = match targets_provider.get_targets(project_key_str).await {
            Ok(t) => t,
            Err(err) => {
                warn!(
                    run_id = %run_id,
                    error = %err,
                    "failed to get cluster targets during reconcile; skipping run to keep existing policy"
                );
                continue;
            }
        };

        let netpol = render_network_policy(run_id, &task_key, &sb, &targets, config);
        let netpol_name = match netpol.name_any() {
            name if !name.is_empty() => name,
            _ => {
                warn!(
                    sandbox = sb_name,
                    "failed to derive network policy name; skipping reconcile"
                );
                continue;
            }
        };

        let patch = Patch::Apply(&netpol);
        let params = PatchParams::apply("dark-factory").force();
        let patch_fut = tokio::time::timeout(
            K8S_API_TIMEOUT,
            netpols.patch(&netpol_name, &params, &patch),
        );
        match patch_fut.await {
            Ok(Ok(_)) => {
                count += 1;
            }
            Ok(Err(err)) => {
                warn!(
                    run_id = %run_id,
                    netpol = %netpol_name,
                    error = %err,
                    "failed to patch NetworkPolicy during reconcile; skipping run"
                );
                continue;
            }
            Err(_) => {
                warn!(
                    run_id = %run_id,
                    netpol = %netpol_name,
                    "timeout patching NetworkPolicy during reconcile; skipping run"
                );
                continue;
            }
        }
    }

    Ok(count)
}

/// Runs leader election loop renewing the Lease periodically.
///
/// Returns once `shutdown` is set or its sender is gone, after stepping down (and releasing the
/// lease if leader). Fails right away if `duration_seconds` is not positive.
pub async fn run_leader_election(
    client: Client,
    namespace: String,
    lease_name: String,
    identity: String,
    duration_seconds: i32,
    leader_tx: watch::Sender<bool>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), ProviderError> {
    let duration = u64::try_from(duration_seconds)
        .ok()
        .filter(|d| *d > 0)
        .ok_or_else(|| {
            ProviderError::InvalidSpec(format!(
                "lease duration must be a positive number of seconds, got {duration_seconds}"
            ))
        })?;
    let mut ticker = tokio::time::interval(Duration::from_secs((duration / 3).max(1)));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                // An `Err` means the sender is gone, which is a shutdown as well.
                if changed.is_err() || *shutdown.borrow() {
                    let was_leader = *leader_tx.borrow();
                    if was_leader {
                        // Notify that we are stepping down
                        publish_leadership(&leader_tx, false);
                        // Best-effort release lease so successor can acquire quickly
                        let _ = release_lease(&client, &namespace, &lease_name, &identity).await;
                    }
                    return Ok(());
                }
            }
            _ = ticker.tick() => {
                match try_acquire_or_renew_lease(
                    &client,
                    &namespace,
                    &lease_name,
                    &identity,
                    duration_seconds,
                ).await {
                    Ok(is_leader) => publish_leadership(&leader_tx, is_leader),
                    Err(err) => {
                        warn!(error = %err, "leader election renewal failed");
                        publish_leadership(&leader_tx, false);
                    }
                }
            }
        }
    }
}

/// Watches Services and EndpointSlices in the cluster-target namespaces (every namespace with
/// `any_namespace`) and sends a notification on `notify` whenever one changed, so the policies
/// follow the targets. Watch errors are logged; the watchers retry with backoff on their own.
///
/// The returned future never completes on its own.
pub async fn watch_cluster_target_changes(
    client: Client,
    targets: ClusterTargetsConfig,
    notify: tokio::sync::mpsc::Sender<()>,
) {
    let mut changes: Vec<BoxStream<'static, Result<bool, watcher::Error>>> = Vec::new();
    if targets.any_namespace {
        changes.push(change_stream(Api::<Service>::all(client.clone())));
        changes.push(change_stream(Api::<EndpointSlice>::all(client.clone())));
    } else {
        for ns in &targets.namespaces {
            changes.push(change_stream(Api::<Service>::namespaced(
                client.clone(),
                ns,
            )));
            changes.push(change_stream(Api::<EndpointSlice>::namespaced(
                client.clone(),
                ns,
            )));
        }
    }
    if changes.is_empty() {
        // Nothing to watch; an ended future would look like a failed task to the caller.
        return future::pending().await;
    }
    forward_changes(stream::select_all(changes), notify).await;
}

/// Background loop running network policy reconciliation when leader.
///
/// Reconciles when this instance becomes leader, on every notification, and every 30 s while
/// leader. Ends when `shutdown` is set, or when the `shutdown` or `is_leader` sender is gone
/// (the leader election task has ended, so there is no leadership to act on anymore). A closed
/// notification channel only disables that trigger; the periodic resync continues. A reconcile
/// pass is not interrupted by shutdown: the caller's grace period applies, and aborting a pass
/// is safe because every patch is idempotent.
pub async fn run_policy_reconcile_loop(
    client: Client,
    config: ProvisioningConfig,
    targets_provider: Arc<dyn ClusterTargetsProvider>,
    mut is_leader: watch::Receiver<bool>,
    mut notify_rx: tokio::sync::mpsc::Receiver<()>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(30));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut notify_active = true;

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            changed = is_leader.changed() => {
                if changed.is_err() {
                    warn!("leader election ended; stopping policy reconciliation");
                    break;
                }
                if *is_leader.borrow() {
                    info!("became leader; running initial policy reconciliation");
                    trigger_reconcile(
                        &client,
                        &config,
                        targets_provider.as_ref(),
                        "leader acquired",
                    ).await;
                }
            }
            res = notify_rx.recv(), if notify_active => {
                match res {
                    Some(()) => {
                        // Coalesce a burst into one pass. Shutdown waits at most this long.
                        tokio::time::sleep(NOTIFY_DEBOUNCE).await;
                        while notify_rx.try_recv().is_ok() {}
                        if *is_leader.borrow() {
                            trigger_reconcile(
                                &client,
                                &config,
                                        targets_provider.as_ref(),
                                "notification received",
                            ).await;
                        }
                    }
                    None => {
                        // Channel closed: `recv` would now complete immediately on every
                        // iteration, so disable this branch to avoid spinning.
                        notify_active = false;
                    }
                }
            }
            _ = ticker.tick() => {
                if *is_leader.borrow() {
                    trigger_reconcile(
                        &client,
                        &config,
                        targets_provider.as_ref(),
                        "periodic 30s resync",
                    ).await;
                }
            }
        }
    }
}

/// Publishes the leadership state, waking receivers only when it actually changes. A plain
/// `send` would mark every renewal as a change and make the reconcile loop re-run its
/// "became leader" pass on each renewal tick.
fn publish_leadership(leader_tx: &watch::Sender<bool>, is_leader: bool) {
    leader_tx.send_if_modified(|current| std::mem::replace(current, is_leader) != is_leader);
}

async fn trigger_reconcile(
    client: &Client,
    config: &ProvisioningConfig,
    targets_provider: &dyn ClusterTargetsProvider,
    reason: &str,
) {
    debug!(reason, "reconciling active network policies");
    if let Err(err) = reconcile_active_runs(client, config, targets_provider).await {
        warn!(error = %err, reason, "network policy reconciliation failed");
    }
}

/// Watches `api` and yields `true` for every event that may have changed an object (including
/// the end of a relist, which can hide changes made while the watch was down).
fn change_stream<K>(api: Api<K>) -> BoxStream<'static, Result<bool, watcher::Error>>
where
    K: Resource + Clone + serde::de::DeserializeOwned + Debug + Send + 'static,
    K::DynamicType: Default,
{
    watcher::watcher(api, watcher::Config::default())
        .default_backoff()
        .map(|res| {
            res.map(|event| {
                matches!(
                    event,
                    watcher::Event::Apply(_) | watcher::Event::Delete(_) | watcher::Event::InitDone
                )
            })
        })
        .boxed()
}

async fn forward_changes(
    changes: impl Stream<Item = Result<bool, watcher::Error>>,
    notify: tokio::sync::mpsc::Sender<()>,
) {
    let mut changes = std::pin::pin!(changes);
    while let Some(res) = changes.next().await {
        match res {
            // A full channel already has a reconcile pending.
            Ok(true) => {
                let _ = notify.try_send(());
            }
            Ok(false) => {}
            Err(err) => warn!(error = %err, "cluster target watch error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::K8sSandboxProvider;
    use crate::render::ResolvedClusterTarget;
    use df_core::TaskVolumeSpec;
    use df_core::ids::RunId;
    use df_core::seams::{RunSandboxSpec, SandboxProvider};
    use k8s_openapi::api::core::v1::PersistentVolumeClaim;
    use kube::runtime::reflector;
    use std::collections::BTreeMap;
    use std::future::Future as _;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct DynamicTargetsProvider {
        added: AtomicBool,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ClusterTargetsProvider for DynamicTargetsProvider {
        async fn get_targets(
            &self,
            _project_key: Option<&str>,
        ) -> Result<Vec<ResolvedClusterTarget>, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut targets = vec![ResolvedClusterTarget {
                namespace: "nativelink".to_string(),
                service: "scheduler".to_string(),
                cluster_ip: Some(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 50))),
                is_headless: false,
                allow_dns: false,
                pod_selector: Some(BTreeMap::from([(
                    "app".to_string(),
                    "scheduler".to_string(),
                )])),
                target_ports: vec![50051],
                endpoint_ips: vec![],
            }];

            if self.added.load(Ordering::SeqCst) {
                targets.push(ResolvedClusterTarget {
                    namespace: "forgejo".to_string(),
                    service: "ssh".to_string(),
                    cluster_ip: Some(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 77))),
                    is_headless: false,
                    allow_dns: false,
                    pod_selector: Some(BTreeMap::from([(
                        "app".to_string(),
                        "forgejo".to_string(),
                    )])),
                    target_ports: vec![2222],
                    endpoint_ips: vec![],
                });
            }

            Ok(targets)
        }
    }

    #[tokio::test]
    #[ignore = "requires live dev cluster"]
    async fn test_reconciliation_live_cluster() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::try_default().await.expect("cluster connection");
        let live = crate::test_support::live_config();
        let config = ProvisioningConfig {
            sandbox: df_core::SandboxProvisioningConfig {
                runtime_image: "ghcr.io/prskr/dark-factory/runtime:dev".to_string(),
                ..live.sandbox.clone()
            },
            ..live
        };

        let dynamic_targets = Arc::new(DynamicTargetsProvider::default());

        let (pod_store, _) = reflector::store();
        let (sb_store, _) = reflector::store();
        let provider = K8sSandboxProvider::new(
            client.clone(),
            config.clone(),
            dynamic_targets.clone(),
            pod_store,
            sb_store,
        );

        let task = TaskVolumeSpec::for_task("DF-888".parse().unwrap());
        provider
            .ensure_task_volume(&task)
            .await
            .expect("ensure task PVC");

        let spec = RunSandboxSpec {
            run_id: RunId::new(),
            run_number: 1,
            task_key: task.task_key.clone(),
            project_id: df_core::ProjectId::new(),
            project_key: "DF".parse().unwrap(),
            persona_id: df_core::PersonaId::new(),
            image: "alpine:latest".to_string(),
            harness: None,
            volume_name: task.volume_name.clone(),
            labels: BTreeMap::new(),
        };

        provider.provision(&spec).await.expect("provision run");

        let netpols: Api<NetworkPolicy> =
            Api::namespaced(client.clone(), &config.sandbox.namespace);
        let netpol_name = "run-df-888-1".to_string();

        // 1. Verify initial policy exists
        let initial_pol = netpols
            .get(&netpol_name)
            .await
            .expect("initial policy exists");
        let initial_egress_count = initial_pol
            .spec
            .as_ref()
            .unwrap()
            .egress
            .as_ref()
            .unwrap()
            .len();

        // 2. Add target dynamically
        dynamic_targets.added.store(true, Ordering::SeqCst);

        // 3. Trigger reconciliation
        let reconciled_count = reconcile_active_runs(&client, &config, dynamic_targets.as_ref())
            .await
            .expect("reconciliation succeeds");
        assert!(reconciled_count >= 1);

        // 4. Verify updated policy contains added rule
        let updated_pol = netpols
            .get(&netpol_name)
            .await
            .expect("updated policy exists");
        let updated_egress_count = updated_pol
            .spec
            .as_ref()
            .unwrap()
            .egress
            .as_ref()
            .unwrap()
            .len();
        assert_eq!(updated_egress_count, initial_egress_count + 1);

        // Cleanup
        provider.teardown(spec.run_id).await.expect("teardown run");
        let pvcs: Api<PersistentVolumeClaim> =
            Api::namespaced(client.clone(), &config.sandbox.namespace);
        let _ = pvcs
            .delete(&task.volume_name, &kube::api::DeleteParams::default())
            .await;
    }

    /// Task 4.1: the real reconcile loop reverts a manual policy edit and picks up a new target
    /// within one resync period (30 s). Runs `run_policy_reconcile_loop` as in `serve`, with
    /// leadership forced (no Lease involved).
    ///
    /// Changes in the cluster (all in `dark-factory-dev-sandboxes`): a PVC `task-df-889`, a
    /// `Sandbox` and a `NetworkPolicy` named `run-df-889-1` (plus the sandbox's pod). It edits the
    /// policy once by hand. Everything is removed again at the end, also if an assertion failed.
    /// Run with: `cargo test -p df-k8s --lib policy_loop_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requires dev cluster"]
    async fn policy_loop_live_reverts_manual_edit_and_adds_new_target() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::try_default().await.expect("cluster connection");
        let config = crate::test_support::live_config();
        let targets = Arc::new(DynamicTargetsProvider::default());
        let (pod_store, _) = reflector::store();
        let (sb_store, _) = reflector::store();
        let provider = K8sSandboxProvider::new(
            client.clone(),
            config.clone(),
            targets.clone(),
            pod_store,
            sb_store,
        );
        let task = TaskVolumeSpec::for_task("DF-889".parse().unwrap());
        let spec = RunSandboxSpec {
            run_id: RunId::new(),
            run_number: 1,
            task_key: task.task_key.clone(),
            project_id: df_core::ProjectId::new(),
            project_key: "DF".parse().unwrap(),
            persona_id: df_core::PersonaId::new(),
            image: "alpine:latest".to_string(),
            harness: None,
            volume_name: task.volume_name.clone(),
            labels: BTreeMap::new(),
        };

        let outcome = async {
            provider.ensure_task_volume(&task).await?;
            provider.provision(&spec).await?;
            drive_policy_loop(&client, &config, targets.clone()).await
        }
        .await;

        // Cleanup runs whatever happened above.
        let _ = provider.teardown(spec.run_id).await;
        let netpols: Api<NetworkPolicy> =
            Api::namespaced(client.clone(), &config.sandbox.namespace);
        let _ = netpols
            .delete("run-df-889-1", &kube::api::DeleteParams::default())
            .await;
        let pvcs: Api<PersistentVolumeClaim> =
            Api::namespaced(client.clone(), &config.sandbox.namespace);
        let _ = pvcs
            .delete(&task.volume_name, &kube::api::DeleteParams::default())
            .await;

        outcome.expect("policy loop scenario");
    }

    /// Egress rules (as JSON) the policy of `run-df-889-1` must have for the current targets.
    async fn expected_egress(
        client: &Client,
        config: &ProvisioningConfig,
        targets: &DynamicTargetsProvider,
    ) -> Result<serde_json::Value, ProviderError> {
        let sandboxes: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
        let sandbox = sandboxes
            .get("run-df-889-1")
            .await
            .map_err(|e| ProviderError::Backend(e.to_string()))?;
        let run_id = run_id_from_metadata(&sandbox.metadata)
            .ok_or_else(|| ProviderError::Backend("sandbox has no run id".into()))?;
        let task_key = "DF-889"
            .parse()
            .map_err(|e| ProviderError::Backend(format!("{e}")))?;
        let resolved = targets.get_targets(None).await?;
        let policy = render_network_policy(run_id, &task_key, &sandbox, &resolved, config);
        serde_json::to_value(policy.spec.and_then(|s| s.egress))
            .map_err(|e| ProviderError::Backend(e.to_string()))
    }

    async fn live_egress(netpols: &Api<NetworkPolicy>) -> Result<serde_json::Value, ProviderError> {
        let policy = netpols
            .get("run-df-889-1")
            .await
            .map_err(|e| ProviderError::Backend(e.to_string()))?;
        serde_json::to_value(policy.spec.and_then(|s| s.egress))
            .map_err(|e| ProviderError::Backend(e.to_string()))
    }

    /// Waits until the egress rules of `run-df-889-1` equal `expected`, at most `within`.
    async fn wait_for_egress_rules(
        netpols: &Api<NetworkPolicy>,
        expected: &serde_json::Value,
        within: Duration,
    ) -> Result<(), ProviderError> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let live = live_egress(netpols).await?;
            if &live == expected {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ProviderError::Backend(format!(
                    "egress rules not as expected after {within:?}:\nexpected {expected}\nactual   {live}"
                )));
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// One resync period plus a margin for the reconcile pass and API latency.
    const RESYNC_DEADLINE: Duration = Duration::from_secs(35);

    async fn drive_policy_loop(
        client: &Client,
        config: &ProvisioningConfig,
        targets: Arc<DynamicTargetsProvider>,
    ) -> Result<(), ProviderError> {
        let netpols: Api<NetworkPolicy> =
            Api::namespaced(client.clone(), &config.sandbox.namespace);
        let initial = expected_egress(client, config, &targets).await?;
        if initial.as_array().is_none_or(|rules| rules.is_empty()) {
            return Err(ProviderError::Backend(
                "provisioned policy has no egress rules".into(),
            ));
        }
        let calls_before = targets.calls.load(Ordering::SeqCst);

        // The real loop, as in `serve`, with leadership forced.
        let (_leader_tx, leader_rx) = watch::channel(true);
        let (_notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
        let (stop_tx, stop_rx) = watch::channel(false);
        let loop_task = tokio::spawn(run_policy_reconcile_loop(
            client.clone(),
            config.clone(),
            targets.clone(),
            leader_rx,
            notify_rx,
            stop_rx,
        ));

        let scenario = async {
            // The loop reconciles immediately on its first tick. Let that pass finish first, so
            // the edit below can only be reverted by the 30 s drift resync, not by that tick.
            let startup = tokio::time::Instant::now() + Duration::from_secs(20);
            while targets.calls.load(Ordering::SeqCst) == calls_before {
                if tokio::time::Instant::now() >= startup {
                    return Err(ProviderError::Backend(
                        "the loop did not run its first reconcile pass".into(),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            // The pass is a single patch after `get_targets`; give it time to land.
            tokio::time::sleep(Duration::from_secs(3)).await;
            wait_for_egress_rules(&netpols, &initial, Duration::from_secs(5)).await?;

            // 1. Manual edit: drop all egress rules; the next resync must restore them.
            netpols
                .patch(
                    "run-df-889-1",
                    &PatchParams::default(),
                    &Patch::Merge(serde_json::json!({ "spec": { "egress": [] } })),
                )
                .await
                .map_err(|e| ProviderError::Backend(e.to_string()))?;
            if live_egress(&netpols).await? == initial {
                return Err(ProviderError::Backend(
                    "the manual edit did not apply".into(),
                ));
            }
            wait_for_egress_rules(&netpols, &initial, RESYNC_DEADLINE).await?;

            // 2. A new allowlist target appears; the next resync must add its rule.
            targets.added.store(true, Ordering::SeqCst);
            let with_new_target = expected_egress(client, config, &targets).await?;
            if with_new_target == initial {
                return Err(ProviderError::Backend(
                    "the new target does not change the rendered policy".into(),
                ));
            }
            wait_for_egress_rules(&netpols, &with_new_target, RESYNC_DEADLINE).await
        }
        .await;

        stop_tx.send_replace(true);
        let _ = tokio::time::timeout(Duration::from_secs(40), loop_task).await;
        scenario
    }

    #[tokio::test]
    async fn test_lease_acquire_and_release() {
        use tower_test::mock;

        let (mock_service, mut handle) = mock::pair();
        let client = Client::new(mock_service, "default");

        // 1. Acquire lease when it does not exist (404)
        let acquire_client = client.clone();
        let acquire_task = tokio::spawn(async move {
            try_acquire_or_renew_lease(&acquire_client, "default", "test-lease", "inst-1", 15).await
        });

        let (req, send_resp) = handle.next_request().await.expect("received lease GET");
        assert_eq!(
            req.uri().path(),
            "/apis/coordination.k8s.io/v1/namespaces/default/leases/test-lease"
        );
        send_resp.send_response(
            http::Response::builder()
                .status(404)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&serde_json::json!({"code": 404})).unwrap())
                .unwrap(),
        );

        let (req, send_resp) = handle.next_request().await.expect("received lease POST");
        assert_eq!(
            req.uri().path(),
            "/apis/coordination.k8s.io/v1/namespaces/default/leases"
        );
        assert_eq!(req.method(), http::Method::POST);
        send_resp.send_response(
            http::Response::builder()
                .status(201)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&Lease::default()).unwrap())
                .unwrap(),
        );

        let acquired = acquire_task.await.unwrap().expect("acquire success");
        assert!(acquired);

        // 2. Release lease
        let release_client = client.clone();
        let release_task = tokio::spawn(async move {
            release_lease(&release_client, "default", "test-lease", "inst-1").await
        });

        let (req, send_resp) = handle
            .next_request()
            .await
            .expect("received lease GET for release");
        assert_eq!(
            req.uri().path(),
            "/apis/coordination.k8s.io/v1/namespaces/default/leases/test-lease"
        );
        let existing_lease = Lease {
            metadata: ObjectMeta {
                name: Some("test-lease".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            spec: Some(LeaseSpec {
                holder_identity: Some("inst-1".to_string()),
                lease_duration_seconds: Some(15),
                renew_time: Some(MicroTime(k8s_openapi::jiff::Timestamp::now())),
                ..Default::default()
            }),
        };
        send_resp.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&existing_lease).unwrap())
                .unwrap(),
        );

        let (req, send_resp) = handle
            .next_request()
            .await
            .expect("received lease PUT for release");
        assert_eq!(
            req.uri().path(),
            "/apis/coordination.k8s.io/v1/namespaces/default/leases/test-lease"
        );
        assert_eq!(req.method(), http::Method::PUT);
        let put_body = http_body_util::BodyExt::collect(req.into_body())
            .await
            .unwrap()
            .to_bytes();
        let put_lease: serde_json::Value = serde_json::from_slice(&put_body).unwrap();
        assert!(
            put_lease["spec"].get("holderIdentity").is_none(),
            "holder not cleared: {put_lease}"
        );
        assert!(
            put_lease["spec"].get("renewTime").is_none(),
            "renewTime not cleared: {put_lease}"
        );
        send_resp.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&Lease::default()).unwrap())
                .unwrap(),
        );

        release_task.await.unwrap().expect("release success");
    }

    /// Counts `get_targets` calls and returns no targets.
    #[derive(Default)]
    struct CountingTargets {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ClusterTargetsProvider for CountingTargets {
        async fn get_targets(
            &self,
            _project_key: Option<&str>,
        ) -> Result<Vec<ResolvedClusterTarget>, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

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

    /// Answers the API calls of one reconcile pass: a list with one active run, and any
    /// NetworkPolicy patch.
    fn spawn_api_responder(
        mut handle: tower_test::mock::Handle<
            http::Request<kube::client::Body>,
            http::Response<String>,
        >,
    ) {
        tokio::spawn(async move {
            while let Some((req, send)) = handle.next_request().await {
                let body = if req.method() == http::Method::GET {
                    serde_json::json!({
                        "apiVersion": "agents.x-k8s.io/v1beta1",
                        "kind": "SandboxList",
                        "metadata": {},
                        "items": [{
                            "apiVersion": "agents.x-k8s.io/v1beta1",
                            "kind": "Sandbox",
                            "metadata": {
                                "name": "run-df-1-1",
                                "uid": "uid-1",
                                "labels": {
                                    "dark-factory.io/run-id": RunId::from_uuid(uuid::Uuid::from_u128(1)).to_string(),
                                    "dark-factory.io/task-key": "DF-1",
                                    "dark-factory.io/project": "DF"
                                }
                            },
                            "spec": { "podTemplate": { "spec": { "containers": [] } } }
                        }]
                    })
                } else {
                    serde_json::json!({
                        "apiVersion": "networking.k8s.io/v1",
                        "kind": "NetworkPolicy",
                        "metadata": { "name": "run-df-1-1" }
                    })
                };
                send.send_response(
                    http::Response::builder()
                        .status(200)
                        .header("Content-Type", "application/json")
                        .body(body.to_string())
                        .unwrap(),
                );
            }
        });
    }

    /// Lets every runnable task run until the runtime is quiescent.
    async fn settle() {
        for _ in 0..500 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn closed_notification_channel_does_not_spin_the_reconcile_loop() {
        use tower_test::mock;

        tokio::time::pause();
        let (mock_service, handle) = mock::pair();
        spawn_api_responder(handle);
        let client = Client::new(mock_service, "default");
        // Leader from the start, notification sender dropped: `recv()` is ready forever.
        let (_is_leader_tx, is_leader_rx) = watch::channel(true);
        let (notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
        drop(notify_tx);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let targets = Arc::new(CountingTargets::default());

        let mut reconcile = Box::pin(run_policy_reconcile_loop(
            client,
            test_config(),
            targets.clone(),
            is_leader_rx,
            notify_rx,
            shutdown_rx,
        ));
        // A spinning loop re-wakes itself and is polled over and over; an idle one is polled
        // only when a timer or channel wakes it.
        let polls = Arc::new(AtomicUsize::new(0));
        let counted = polls.clone();
        let loop_handle = tokio::spawn(futures::future::poll_fn(move |cx| {
            counted.fetch_add(1, Ordering::SeqCst);
            reconcile.as_mut().poll(cx)
        }));

        settle().await;
        tokio::time::advance(Duration::from_secs(65)).await;
        settle().await;

        let polls_so_far = polls.load(Ordering::SeqCst);
        assert!(
            polls_so_far < 25,
            "reconcile loop is spinning: {polls_so_far} polls"
        );
        let calls = targets.calls.load(Ordering::SeqCst);
        assert!(
            (1..=4).contains(&calls),
            "expected only periodic resyncs, got {calls} get_targets calls"
        );

        shutdown_tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(1), loop_handle)
            .await
            .expect("loop terminates on shutdown")
            .unwrap();
    }

    #[tokio::test]
    async fn reconcile_loop_stops_when_the_leader_channel_sender_is_gone() {
        use tower_test::mock;

        // Real time: if the loop spun, the timeout below must still fire.
        let (mock_service, handle) = mock::pair();
        spawn_api_responder(handle);
        let client = Client::new(mock_service, "default");
        let (is_leader_tx, is_leader_rx) = watch::channel(true);
        let (_notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let targets = Arc::new(CountingTargets::default());

        let loop_handle = tokio::spawn(run_policy_reconcile_loop(
            client,
            test_config(),
            targets.clone(),
            is_leader_rx,
            notify_rx,
            shutdown_rx,
        ));
        settle().await;
        drop(is_leader_tx);

        tokio::time::timeout(Duration::from_secs(5), loop_handle)
            .await
            .expect("loop ends once the leader election task is gone")
            .unwrap();
    }

    #[tokio::test]
    async fn reconcile_loop_stops_when_the_shutdown_sender_is_gone() {
        use tower_test::mock;

        // Real time: if the loop spun, the timeout below must still fire.
        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let client = Client::new(mock_service, "default");
        let (_is_leader_tx, is_leader_rx) = watch::channel(false);
        let (_notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let loop_handle = tokio::spawn(run_policy_reconcile_loop(
            client,
            test_config(),
            Arc::new(CountingTargets::default()),
            is_leader_rx,
            notify_rx,
            shutdown_rx,
        ));
        settle().await;
        drop(shutdown_tx);

        tokio::time::timeout(Duration::from_secs(5), loop_handle)
            .await
            .expect("loop ends once the shutdown sender is gone")
            .unwrap();
    }

    #[tokio::test]
    async fn leader_election_ends_when_the_shutdown_sender_is_gone() {
        use tower_test::mock;

        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let client = Client::new(mock_service, "default");
        let (leader_tx, _leader_rx) = watch::channel(false);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        drop(shutdown_tx);

        let res = tokio::time::timeout(
            Duration::from_secs(1),
            run_leader_election(
                client,
                "default".to_string(),
                "lease".to_string(),
                "me".to_string(),
                15,
                leader_tx,
                shutdown_rx,
            ),
        )
        .await
        .expect("election loop terminates");
        assert!(res.is_ok());
    }

    #[test]
    fn publishing_unchanged_leadership_does_not_wake_receivers() {
        let (tx, mut rx) = watch::channel(false);
        publish_leadership(&tx, true);
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();

        // A renewal while already leader must not look like a new acquisition.
        publish_leadership(&tx, true);
        assert!(!rx.has_changed().unwrap());

        publish_leadership(&tx, false);
        assert!(rx.has_changed().unwrap());
        assert!(!*rx.borrow_and_update());
    }

    #[tokio::test]
    async fn leader_election_rejects_non_positive_lease_duration() {
        use tower_test::mock;

        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let client = Client::new(mock_service, "default");
        let (leader_tx, _leader_rx) = watch::channel(false);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        let err = run_leader_election(
            client,
            "default".to_string(),
            "lease".to_string(),
            "me".to_string(),
            -3,
            leader_tx,
            shutdown_rx,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ProviderError::InvalidSpec(_)), "{err}");
    }

    #[tokio::test]
    async fn forward_changes_notifies_on_changes_and_survives_errors() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let changes = futures::stream::iter(vec![
            Ok(false),
            Err(watcher::Error::NoResourceVersion),
            Ok(true),
        ]);
        forward_changes(changes, tx).await;

        assert_eq!(rx.recv().await, Some(()));
        assert_eq!(rx.recv().await, None, "exactly one notification expected");
    }

    #[tokio::test]
    async fn forward_changes_does_not_block_on_a_full_channel() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let changes = futures::stream::iter((0..10).map(|_| Ok(true)));
        tokio::time::timeout(Duration::from_secs(1), forward_changes(changes, tx))
            .await
            .expect("try_send never blocks");
    }

    #[tokio::test]
    async fn watching_no_namespaces_never_completes() {
        use tower_test::mock;

        let (mock_service, _handle) =
            mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let watch = watch_cluster_target_changes(
            Client::new(mock_service, "default"),
            ClusterTargetsConfig::default(),
            tx,
        );
        tokio::time::pause();
        assert!(
            tokio::time::timeout(Duration::from_secs(3600), watch)
                .await
                .is_err(),
            "the watch future ended"
        );
    }

    #[tokio::test]
    async fn a_burst_of_notifications_causes_a_single_reconcile_pass() {
        use tower_test::mock;

        tokio::time::pause();
        let (mock_service, handle) = mock::pair();
        spawn_api_responder(handle);
        let (_is_leader_tx, is_leader_rx) = watch::channel(true);
        let (notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let targets = Arc::new(CountingTargets::default());
        let loop_handle = tokio::spawn(run_policy_reconcile_loop(
            Client::new(mock_service, "default"),
            test_config(),
            targets.clone(),
            is_leader_rx,
            notify_rx,
            shutdown_rx,
        ));
        settle().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        settle().await;
        let after_startup = targets.calls.load(Ordering::SeqCst);
        assert_eq!(after_startup, 1, "the first tick reconciles once");

        for _ in 0..5 {
            notify_tx.send(()).await.unwrap();
        }
        settle().await;
        // Long enough for five debounced passes, short of the 30 s resync.
        for _ in 0..10 {
            tokio::time::advance(NOTIFY_DEBOUNCE).await;
            settle().await;
        }

        assert_eq!(targets.calls.load(Ordering::SeqCst), after_startup + 1);
        shutdown_tx.send(true).unwrap();
        loop_handle.await.unwrap();
    }

    #[tokio::test]
    async fn notifications_within_the_debounce_window_share_one_pass() {
        use tower_test::mock;

        tokio::time::pause();
        let (mock_service, handle) = mock::pair();
        spawn_api_responder(handle);
        let (_is_leader_tx, is_leader_rx) = watch::channel(true);
        let (notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let targets = Arc::new(CountingTargets::default());
        let loop_handle = tokio::spawn(run_policy_reconcile_loop(
            Client::new(mock_service, "default"),
            test_config(),
            targets.clone(),
            is_leader_rx,
            notify_rx,
            shutdown_rx,
        ));
        settle().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        settle().await;
        let after_startup = targets.calls.load(Ordering::SeqCst);
        assert_eq!(after_startup, 1);

        // The second notification arrives half way into the window of the first.
        notify_tx.send(()).await.unwrap();
        settle().await;
        tokio::time::advance(NOTIFY_DEBOUNCE / 2).await;
        settle().await;
        notify_tx.send(()).await.unwrap();
        settle().await;
        for _ in 0..4 {
            tokio::time::advance(NOTIFY_DEBOUNCE).await;
            settle().await;
        }

        assert_eq!(targets.calls.load(Ordering::SeqCst), after_startup + 1);
        shutdown_tx.send(true).unwrap();
        loop_handle.await.unwrap();
    }
}
