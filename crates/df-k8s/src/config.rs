//! Provisioning configuration for `df-k8s`.
//!
//! Defaults live in [`df_core::config::SandboxProvisioningConfig`]; this struct is only built in code
//! from that section and the runtime network parameters.

use std::net::IpAddr;

pub use df_core::RuntimeDelivery;
use df_core::config::SandboxProvisioningConfig;

/// Control plane agent endpoint reachable by sandboxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneEndpoint {
    pub ip: IpAddr,
    pub port: u16,
    /// TLS server name presented by the endpoint.
    pub tls_server_name: String,
}

/// Gateway egress proxy endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayEndpoint {
    pub ip: IpAddr,
    pub proxy_port: u16,
}

/// Settings governing the rendering and provisioning of sandboxes and task volumes: the
/// `sandbox` section of the instance configuration plus the resolved network parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisioningConfig {
    /// Release or instance identifier, e.g. "homelab-dev".
    pub instance_name: String,
    /// The `sandbox` configuration section (namespaces, storage, runtime class, issuer, ...).
    pub sandbox: SandboxProvisioningConfig,
    /// IP of the control plane agent endpoint reachable by sandboxes.
    pub control_plane_ip: IpAddr,
    /// Port of the control plane agent endpoint.
    pub control_plane_port: u16,
    /// TLS server name presented by the control plane agent endpoint.
    pub control_plane_tls_server_name: String,
    /// IP of the gateway egress proxy.
    pub gateway_ip: IpAddr,
    /// Port of the gateway egress proxy.
    pub gateway_proxy_port: u16,
    /// Log format (`text` or `json`) the sandbox runtime is started with: the control plane's own.
    pub log_format: String,
}

impl ProvisioningConfig {
    /// Creates a [`ProvisioningConfig`] from [`SandboxProvisioningConfig`] and runtime network parameters.
    pub fn from_sandbox_config(
        instance_name: String,
        sandbox: &SandboxProvisioningConfig,
        control_plane: ControlPlaneEndpoint,
        gateway: GatewayEndpoint,
    ) -> Self {
        Self {
            instance_name,
            sandbox: sandbox.clone(),
            control_plane_ip: control_plane.ip,
            control_plane_port: control_plane.port,
            control_plane_tls_server_name: control_plane.tls_server_name,
            gateway_ip: gateway.ip,
            gateway_proxy_port: gateway.proxy_port,
            log_format: "text".to_string(),
        }
    }

    /// Sets the log format passed to sandboxes (`DF_LOG_FORMAT`).
    pub fn with_log_format(mut self, log_format: &str) -> Self {
        self.log_format = log_format.to_string();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_sandbox_config_copies_section_values() {
        let section = SandboxProvisioningConfig {
            namespace: "sb".into(),
            storage_class: Some("fast".into()),
            runtime_class_name: None,
            ..Default::default()
        };
        let cfg = ProvisioningConfig::from_sandbox_config(
            "inst".into(),
            &section,
            ControlPlaneEndpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 9443,
                tls_server_name: "cp.local".into(),
            },
            GatewayEndpoint {
                ip: "10.0.0.2".parse().unwrap(),
                proxy_port: 3128,
            },
        );

        assert_eq!(cfg.instance_name, "inst");
        assert_eq!(cfg.sandbox.namespace, "sb");
        assert_eq!(cfg.sandbox.storage_class.as_deref(), Some("fast"));
        assert_eq!(cfg.sandbox.runtime_class_name, None);
        assert_eq!(cfg.sandbox.issuer_kind, section.issuer_kind);
        assert_eq!(cfg.control_plane_port, 9443);
        assert_eq!(cfg.gateway_proxy_port, 3128);
    }
}
