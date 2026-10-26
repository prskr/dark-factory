//! Kubernetes client, rendering, and provisioning logic for Dark Factory sandboxes.

pub mod config;
pub mod crd;
pub mod provider;
pub mod reconcile;
pub mod render;
pub mod secrets;
pub mod startup;
#[cfg(test)]
mod test_support;

/// Timeout applied to every single Kubernetes API call made by this crate.
pub const K8S_API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub use config::{ControlPlaneEndpoint, GatewayEndpoint, ProvisioningConfig};
pub use crd::{
    Sandbox, SandboxOperatingMode, SandboxPodTemplate, SandboxPodTemplateMetadata,
    SandboxPodTemplateSpec, SandboxShutdownPolicy, SandboxSpec, SandboxStatus,
};
pub use provider::{
    ClusterServiceRef, ClusterTargetResolver, ClusterTargetsProvider, K8sSandboxProvider,
    ResolveError, map_pod_delete_event, map_pod_event, parse_spiffe_san, resolve_cluster_target,
    resolve_identity_from_stores, run_id_from_metadata,
};
pub use reconcile::{
    reconcile_active_runs, run_leader_election, run_policy_reconcile_loop,
    try_acquire_or_renew_lease, watch_cluster_target_changes,
};
pub use render::{
    RenderError, ResolvedClusterTarget, render_network_policy, render_pvc, render_sandbox,
};
pub use secrets::K8sSecretStore;
pub use startup::{
    ALL_NAMESPACES, INSTANCE_PERMISSIONS, RequiredPermission, SANDBOX_PERMISSIONS,
    StartupCheckError, TARGET_PERMISSIONS, verify_startup,
};
