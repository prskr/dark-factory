//! Shared fixtures for tests, including fixtures for fake providers.
//!
//! The `#[ignore]` live tests run against any cluster with a Dark Factory release (Helm release
//! name `dark-factory`) and its sandbox namespace. The cluster-specific settings come from
//! `DF_LIVE_*` environment variables (see the crate README, "Live cluster tests").

use std::net::IpAddr;

use k8s_openapi::api::core::v1::Service;
use kube::{Api, Client};

use crate::config::{ControlPlaneEndpoint, GatewayEndpoint};
use crate::render::ResolvedClusterTarget;
use crate::{ClusterServiceRef, ClusterTargetResolution, ProvisioningConfig};
use df_core::seams::ProviderError;

/// Environment variable naming the runtime image of the live tests (required).
pub(crate) const RUNTIME_IMAGE_VAR: &str = "DF_LIVE_RUNTIME_IMAGE";
/// Namespace of the Dark Factory release (default `dark-factory-dev`).
const INSTANCE_NAMESPACE_VAR: &str = "DF_LIVE_INSTANCE_NAMESPACE";
/// Storage class of the task volumes; empty selects the cluster default (default `longhorn`).
const STORAGE_CLASS_VAR: &str = "DF_LIVE_STORAGE_CLASS";
/// Runtime class of the sandbox pods; empty sets none (default `kata`).
const RUNTIME_CLASS_VAR: &str = "DF_LIVE_RUNTIME_CLASS";
/// Name prefix of the release's resources (Helm fullname).
const RELEASE: &str = "dark-factory";

/// Configuration for the `#[ignore]` tests that run against a live cluster.
///
/// Panics with the variable name if `DF_LIVE_RUNTIME_IMAGE` is unset and if the release's control
/// plane or gateway Service has no cluster IP.
///
/// The instance name is not the deployed one: its leader would reconcile the test's policy with
/// the DB's targets and overwrite what the test expects.
pub(crate) async fn live_config(client: &Client) -> ProvisioningConfig {
    let env =
        LiveEnv::from_lookup(|name| std::env::var(name).ok()).unwrap_or_else(|err| panic!("{err}"));
    let control_plane_ip = service_ip(
        client,
        &env.instance_namespace,
        &format!("{RELEASE}-control-plane"),
    )
    .await;
    let gateway_ip = service_ip(
        client,
        &env.instance_namespace,
        &format!("{RELEASE}-gateway"),
    )
    .await;
    env.config(control_plane_ip, gateway_ip)
}

/// The cluster-specific settings of the live tests, read from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveEnv {
    runtime_image: String,
    instance_namespace: String,
    storage_class: Option<String>,
    runtime_class: Option<String>,
}

impl LiveEnv {
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let runtime_image = lookup(RUNTIME_IMAGE_VAR)
            .filter(|image| !image.is_empty())
            .ok_or_else(|| {
                format!(
                    "{RUNTIME_IMAGE_VAR} is not set: the live tests need the runtime image of the \
                     cluster's release (for example ghcr.io/prskr/dark-factory/runtime:dev)"
                )
            })?;
        let optional = |name: &str, default: &str| {
            let value = lookup(name).unwrap_or_else(|| default.to_string());
            (!value.is_empty()).then_some(value)
        };
        Ok(Self {
            runtime_image,
            instance_namespace: lookup(INSTANCE_NAMESPACE_VAR)
                .unwrap_or_else(|| "dark-factory-dev".to_string()),
            storage_class: optional(STORAGE_CLASS_VAR, "longhorn"),
            runtime_class: optional(RUNTIME_CLASS_VAR, "kata"),
        })
    }

    fn config(&self, control_plane_ip: IpAddr, gateway_ip: IpAddr) -> ProvisioningConfig {
        let ns = &self.instance_namespace;
        ProvisioningConfig::from_sandbox_config(
            "df-live-test".to_string(),
            &df_core::SandboxProvisioningConfig {
                namespace: format!("{ns}-sandboxes"),
                instance_namespace: ns.clone(),
                service_account_name: "dark-factory-sandbox".to_string(),
                storage_class: self.storage_class.clone(),
                pvc_size: "1Gi".to_string(),
                runtime_class_name: self.runtime_class.clone(),
                issuer_name: format!("{ns}-{RELEASE}-ca-issuer"),
                issuer_kind: "ClusterIssuer".to_string(),
                issuer_group: "cert-manager.io".to_string(),
                certificate_duration: "6h".to_string(),
                certificate_renew_before: "2h".to_string(),
                runtime_image: self.runtime_image.clone(),
                cluster_domain: "cluster.local".to_string(),
                log_level: "info".to_string(),
            },
            ControlPlaneEndpoint {
                ip: control_plane_ip,
                port: 9443,
                tls_server_name: format!("{RELEASE}-control-plane.{ns}.svc"),
            },
            GatewayEndpoint {
                ip: gateway_ip,
                proxy_port: 3128,
                git_host: "df-live-test-gateway".to_string(),
                git_port: 8443,
            },
        )
    }
}

/// The targets result when `nativelink/scheduler` cannot be resolved right now.
pub(crate) fn unavailable_scheduler_targets() -> Result<Vec<ResolvedClusterTarget>, ProviderError> {
    ClusterTargetResolution {
        targets: Vec::new(),
        unavailable: vec![ClusterServiceRef {
            namespace: "nativelink".to_string(),
            service: "scheduler".to_string(),
            ports: Vec::new(),
            allow_dns: false,
        }],
    }
    .into_targets()
}

async fn service_ip(client: &Client, namespace: &str, name: &str) -> IpAddr {
    let svc = tokio::time::timeout(
        crate::K8S_API_TIMEOUT,
        Api::<Service>::namespaced(client.clone(), namespace).get(name),
    )
    .await
    .unwrap_or_else(|_elapsed| panic!("Service {namespace}/{name} of the live cluster: timed out"))
    .unwrap_or_else(|err| panic!("Service {namespace}/{name} of the live cluster: {err}"));
    svc.spec
        .and_then(|spec| spec.cluster_ip)
        .and_then(|ip| ip.parse().ok())
        .unwrap_or_else(|| panic!("Service {namespace}/{name} has no ClusterIP"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    #[test]
    fn missing_runtime_image_names_the_variable() {
        let err = LiveEnv::from_lookup(lookup(&[])).unwrap_err();
        assert!(err.contains(RUNTIME_IMAGE_VAR), "{err}");
    }

    #[test]
    fn defaults_target_the_dev_cluster() {
        let env = LiveEnv::from_lookup(lookup(&[(RUNTIME_IMAGE_VAR, "rt:1")])).unwrap();
        assert_eq!(
            env,
            LiveEnv {
                runtime_image: "rt:1".to_string(),
                instance_namespace: "dark-factory-dev".to_string(),
                storage_class: Some("longhorn".to_string()),
                runtime_class: Some("kata".to_string()),
            }
        );
    }

    #[test]
    fn empty_classes_mean_cluster_default_and_none() {
        let env = LiveEnv::from_lookup(lookup(&[
            (RUNTIME_IMAGE_VAR, "rt:1"),
            ("DF_LIVE_INSTANCE_NAMESPACE", "dark-factory"),
            ("DF_LIVE_STORAGE_CLASS", ""),
            ("DF_LIVE_RUNTIME_CLASS", ""),
        ]))
        .unwrap();
        assert_eq!(env.instance_namespace, "dark-factory");
        assert_eq!(env.storage_class, None);
        assert_eq!(env.runtime_class, None);
    }

    #[test]
    fn config_derives_namespaces_and_issuer_from_the_instance_namespace() {
        let env = LiveEnv::from_lookup(lookup(&[
            (RUNTIME_IMAGE_VAR, "rt:1"),
            ("DF_LIVE_INSTANCE_NAMESPACE", "dark-factory"),
        ]))
        .unwrap();
        let cp: IpAddr = "10.0.0.1".parse().unwrap();
        let gw: IpAddr = "10.0.0.2".parse().unwrap();
        let config = env.config(cp, gw);
        assert_eq!(config.instance_name, "df-live-test");
        assert_eq!(config.sandbox.runtime_image, "rt:1");
        assert_eq!(config.sandbox.namespace, "dark-factory-sandboxes");
        assert_eq!(config.sandbox.instance_namespace, "dark-factory");
        assert_eq!(
            config.sandbox.issuer_name,
            "dark-factory-dark-factory-ca-issuer"
        );
        assert_eq!(config.control_plane_ip, cp);
        assert_eq!(config.gateway_ip, gw);
        assert_eq!(
            config.control_plane_tls_server_name,
            "dark-factory-control-plane.dark-factory.svc"
        );
    }
}
