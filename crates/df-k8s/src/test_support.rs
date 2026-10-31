//! Shared fixtures for tests.

use std::net::{IpAddr, Ipv4Addr};

use crate::ProvisioningConfig;
use crate::config::{ControlPlaneEndpoint, GatewayEndpoint};

/// Configuration for the `#[ignore]` tests that run against the dev cluster.
///
/// The instance name is not the deployed one: its leader would reconcile the test's policy with
/// the DB's targets and overwrite what the test expects.
pub(crate) fn live_config() -> ProvisioningConfig {
    ProvisioningConfig::from_sandbox_config(
        "df-live-test".to_string(),
        &df_core::SandboxProvisioningConfig {
            namespace: "dark-factory-dev-sandboxes".to_string(),
            instance_namespace: "dark-factory-dev".to_string(),
            service_account_name: "dark-factory-sandbox".to_string(),
            storage_class: Some("longhorn".to_string()),
            pvc_size: "1Gi".to_string(),
            runtime_class_name: Some("kata".to_string()),
            issuer_name: "dark-factory-dev-dark-factory-ca-issuer".to_string(),
            issuer_kind: "ClusterIssuer".to_string(),
            issuer_group: "cert-manager.io".to_string(),
            certificate_duration: "6h".to_string(),
            certificate_renew_before: "2h".to_string(),
            runtime_image: "alpine:latest".to_string(),
            cluster_domain: "cluster.local".to_string(),
            log_level: "info".to_string(),
        },
        ControlPlaneEndpoint {
            ip: IpAddr::V4(Ipv4Addr::new(10, 103, 178, 202)),
            port: 9443,
            tls_server_name: "dark-factory-control-plane.dark-factory-dev.svc".to_string(),
        },
        GatewayEndpoint {
            ip: IpAddr::V4(Ipv4Addr::new(10, 105, 68, 86)),
            proxy_port: 3128,
            git_host: "df-live-test-gateway".to_string(),
            git_port: 8443,
        },
    )
}
