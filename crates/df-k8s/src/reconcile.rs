//! Lease leader election and active run `NetworkPolicy` reconciliation.

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use df_core::config::ClusterTargetsConfig;
use df_core::ids::RunId;
use df_core::keys::TaskKey;
use df_core::seams::ProviderError;
use futures::stream::{self, BoxStream};
use futures::{Stream, StreamExt as _, future};
use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::Service;
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::networking::v1::NetworkPolicy;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta};
use kube::api::{Api, Patch, PatchParams, PostParams};
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
/// pass (with `anyNamespace`, every `EndpointSlice` change in the cluster notifies).
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

    let Ok(get_res) = tokio::time::timeout(K8S_API_TIMEOUT, leases.get(lease_name)).await else {
        warn!(lease_name, "timeout querying lease");
        return Ok(false);
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

                if let Ok(res) = tokio::time::timeout(
                    K8S_API_TIMEOUT,
                    leases.replace(lease_name, &PostParams::default(), &lease),
                )
                .await
                {
                    res?;
                    Ok(true)
                } else {
                    warn!(lease_name, "timeout renewing lease");
                    Ok(false)
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
            let Ok(create_res) = tokio::time::timeout(
                K8S_API_TIMEOUT,
                leases.create(&PostParams::default(), &lease),
            )
            .await
            else {
                warn!(lease_name, "timeout creating lease");
                return Ok(false);
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
            leases.replace(lease_name, &PostParams::default(), &lease),
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

/// Reconciles all active runs' `NetworkPolicies` in `config.sandbox.namespace`.
pub async fn reconcile_active_runs(
    client: &Client,
    config: &ProvisioningConfig,
    targets_provider: &dyn ClusterTargetsProvider,
) -> Result<usize, ProviderError> {
    let sandboxes: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
    let netpols: Api<NetworkPolicy> = Api::namespaced(client.clone(), &config.sandbox.namespace);

    let mut count = 0;
    for sb in list_instance_sandboxes(&sandboxes, config).await? {
        if sb.metadata.deletion_timestamp.is_some() {
            continue;
        }
        let Some((run_id, netpol)) = render_run_policy(&sb, targets_provider, config).await else {
            continue;
        };
        if apply_network_policy(&netpols, run_id, &netpol).await {
            count += 1;
        }
    }
    Ok(count)
}

/// Lists the Sandboxes that belong to this instance.
async fn list_instance_sandboxes(
    sandboxes: &Api<Sandbox>,
    config: &ProvisioningConfig,
) -> Result<Vec<Sandbox>, ProviderError> {
    let list_params = kube::api::ListParams::default().labels(&format!(
        "dark-factory.io/instance={}",
        config.instance_name
    ));
    match tokio::time::timeout(K8S_API_TIMEOUT, sandboxes.list(&list_params)).await {
        Ok(Ok(list)) => Ok(list.items),
        Ok(Err(e)) => Err(ProviderError::Backend(format!(
            "failed to list sandboxes for reconcile: {e}"
        ))),
        Err(_) => Err(ProviderError::Backend(
            "timeout listing sandboxes for reconcile".to_string(),
        )),
    }
}

/// Renders the `NetworkPolicy` of a run from its Sandbox. `None` (after a warning) if the Sandbox
/// lacks the labels, the cluster targets can't be fetched, or the policy has no name: the run is
/// skipped and keeps its existing policy.
async fn render_run_policy(
    sb: &Sandbox,
    targets_provider: &dyn ClusterTargetsProvider,
    config: &ProvisioningConfig,
) -> Option<(RunId, NetworkPolicy)> {
    let sb_name = sb.metadata.name.as_deref().unwrap_or("<unknown>");
    let Some(run_id) = run_id_from_metadata(&sb.metadata) else {
        warn!(
            sandbox = sb_name,
            "sandbox has a missing or invalid dark-factory.io/run-id label; skipping reconcile"
        );
        return None;
    };
    let labels = sb.metadata.labels.clone().unwrap_or_default();

    let Some(task_key_str) = labels.get("dark-factory.io/task-key") else {
        warn!(
            sandbox = sb_name,
            "sandbox missing dark-factory.io/task-key label; skipping reconcile"
        );
        return None;
    };
    let task_key = match task_key_str.parse::<TaskKey>() {
        Ok(k) => k,
        Err(e) => {
            warn!(sandbox = sb_name, task_key = task_key_str, error = %e, "invalid dark-factory.io/task-key label; skipping reconcile");
            return None;
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
            return None;
        }
    };

    let netpol = render_network_policy(run_id, &task_key, sb, &targets, config);
    if netpol.name_any().is_empty() {
        warn!(
            sandbox = sb_name,
            "failed to derive network policy name; skipping reconcile"
        );
        return None;
    }
    Some((run_id, netpol))
}

/// Server-side applies the `NetworkPolicy`. False (after a warning) if that failed.
async fn apply_network_policy(
    netpols: &Api<NetworkPolicy>,
    run_id: RunId,
    netpol: &NetworkPolicy,
) -> bool {
    let netpol_name = netpol.name_any();
    let patch = Patch::Apply(netpol);
    let params = PatchParams::apply("dark-factory").force();
    let patch_fut = tokio::time::timeout(
        K8S_API_TIMEOUT,
        netpols.patch(&netpol_name, &params, &patch),
    );
    match patch_fut.await {
        Ok(Ok(_)) => true,
        Ok(Err(err)) => {
            warn!(
                run_id = %run_id,
                netpol = %netpol_name,
                error = %err,
                "failed to patch NetworkPolicy during reconcile; skipping run"
            );
            false
        }
        Err(_) => {
            warn!(
                run_id = %run_id,
                netpol = %netpol_name,
                "timeout patching NetworkPolicy during reconcile; skipping run"
            );
            false
        }
    }
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
                        release_lease(&client, &namespace, &lease_name, &identity).await.ok();
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

/// Watches Services and `EndpointSlices` in the cluster-target namespaces (every namespace with
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
    watcher(api, watcher::Config::default())
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
                notify.try_send(()).ok(); // receiver may be gone
            }
            Ok(false) => {}
            Err(err) => warn!(error = %err, "cluster target watch error"),
        }
    }
}

#[cfg(test)]
mod tests;
