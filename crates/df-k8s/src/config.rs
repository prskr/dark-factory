//! Provisioning configuration for `df-k8s`.
//!
//! Defaults live in [`df_core::config::SandboxProvisioningConfig`]; this struct is only built in code
//! from that section and the runtime network parameters.

use std::collections::BTreeMap;
use std::net::IpAddr;

use df_core::config::SandboxProvisioningConfig;
use df_telemetry::logging::LogFormat;
use k8s_openapi::api::core::v1::Affinity;

use crate::affinity::{AffinityError, parse_affinity};

/// Control plane agent endpoint reachable by sandboxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPlaneEndpoint {
    pub ip: IpAddr,
    pub port: u16,
    /// TLS server name presented by the endpoint.
    pub tls_server_name: String,
}

/// Gateway endpoints: egress proxy and Git proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayEndpoint {
    pub ip: IpAddr,
    pub proxy_port: u16,
    /// Host name of the Git proxy URL, mapped to `ip` in the sandbox's `/etc/hosts`.
    pub git_host: String,
    pub git_port: u16,
}

/// Settings governing the rendering and provisioning of sandboxes and task volumes: the
/// `sandbox` section of the instance configuration plus the resolved network parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvisioningConfig {
    /// Release or instance identifier, e.g. "homelab-dev".
    pub instance_name: String,
    /// The `sandbox` configuration section (namespaces, storage, runtime class, issuer, ...).
    pub sandbox: SandboxProvisioningConfig,
    /// `sandbox.affinity`, parsed and checked; `None` when unset.
    pub affinity: Option<Affinity>,
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
    /// Host name of the gateway Git proxy, resolved through `hostAliases` and kept out of the
    /// egress proxy (`NO_PROXY`).
    pub gateway_git_host: String,
    /// Port of the gateway Git proxy.
    pub gateway_git_port: u16,
    /// Image per harness name (`claude-code`), mounted at `/opt/harness/<name>` in the sandbox of
    /// a run whose project has no adapter command of its own.
    pub harness_images: BTreeMap<String, String>,
    /// Log format the sandbox runtime is started with: the control plane's own.
    pub log_format: LogFormat,
}

impl ProvisioningConfig {
    /// Creates a [`ProvisioningConfig`] from [`SandboxProvisioningConfig`] and runtime network
    /// parameters.
    ///
    /// # Errors
    ///
    /// `sandbox.affinity` is not a valid Kubernetes affinity.
    pub fn from_sandbox_config(
        instance_name: String,
        sandbox: &SandboxProvisioningConfig,
        control_plane: ControlPlaneEndpoint,
        gateway: GatewayEndpoint,
    ) -> Result<Self, AffinityError> {
        let affinity = sandbox.affinity.as_ref().map(parse_affinity).transpose()?;
        Ok(Self {
            instance_name,
            sandbox: sandbox.clone(),
            affinity,
            control_plane_ip: control_plane.ip,
            control_plane_port: control_plane.port,
            control_plane_tls_server_name: control_plane.tls_server_name,
            gateway_ip: gateway.ip,
            gateway_proxy_port: gateway.proxy_port,
            gateway_git_host: gateway.git_host,
            gateway_git_port: gateway.git_port,
            harness_images: BTreeMap::new(),
            log_format: LogFormat::Text,
        })
    }

    /// Sets the image per harness name.
    #[must_use]
    pub fn with_harness_images(mut self, harness_images: BTreeMap<String, String>) -> Self {
        self.harness_images = harness_images;
        self
    }

    /// Sets the log format passed to sandboxes (`DF_LOG_FORMAT`).
    #[must_use]
    pub fn with_log_format(mut self, log_format: LogFormat) -> Self {
        self.log_format = log_format;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build(affinity: Option<serde_json::Value>) -> Result<ProvisioningConfig, AffinityError> {
        ProvisioningConfig::from_sandbox_config(
            "inst".into(),
            &SandboxProvisioningConfig {
                affinity,
                ..Default::default()
            },
            ControlPlaneEndpoint {
                ip: "10.0.0.1".parse().unwrap(),
                port: 9443,
                tls_server_name: "cp.local".into(),
            },
            GatewayEndpoint {
                ip: "10.0.0.2".parse().unwrap(),
                proxy_port: 3128,
                git_host: "gw.svc".into(),
                git_port: 8443,
            },
        )
    }

    fn anti_affinity_term(topology_key: Option<&str>) -> serde_json::Value {
        let mut term =
            json!({"labelSelector": {"matchLabels": {"dark-factory.io/instance": "homelab"}}});
        if let Some(key) = topology_key {
            term["topologyKey"] = json!(key);
        }
        term
    }

    fn preferred(weight: i64, term: serde_json::Value) -> serde_json::Value {
        json!({"podAntiAffinity": {"preferredDuringSchedulingIgnoredDuringExecution": [
            {"weight": weight, "podAffinityTerm": term}
        ]}})
    }

    #[test]
    fn unset_affinity_stays_unset() {
        assert_eq!(build(None).unwrap().affinity, None);
    }

    #[test]
    fn valid_node_affinity_and_pod_anti_affinities_are_parsed() {
        let node = json!({"nodeAffinity": {"requiredDuringSchedulingIgnoredDuringExecution": {
        "nodeSelectorTerms": [{"matchExpressions": [
            {"key": "disk", "operator": "In", "values": ["ssd"]},
            {"key": "gpu", "operator": "Exists"},
            {"key": "cores", "operator": "Gt", "values": ["4"]}
        ]}]}}});
        assert!(
            build(Some(node))
                .unwrap()
                .affinity
                .unwrap()
                .node_affinity
                .is_some()
        );

        let soft = preferred(100, anti_affinity_term(Some("kubernetes.io/hostname")));
        let parsed = build(Some(soft)).unwrap().affinity.unwrap();
        let weighted = parsed
            .pod_anti_affinity
            .unwrap()
            .preferred_during_scheduling_ignored_during_execution
            .unwrap();
        assert_eq!(weighted[0].weight, 100);

        let required = json!({"podAntiAffinity": {"requiredDuringSchedulingIgnoredDuringExecution": [
            anti_affinity_term(Some("kubernetes.io/hostname"))
        ]}});
        assert!(build(Some(required)).is_ok());
    }

    #[test]
    fn null_and_empty_lists_in_known_fields_are_accepted() {
        let affinity = json!({"podAntiAffinity": {
            "requiredDuringSchedulingIgnoredDuringExecution": null,
            "preferredDuringSchedulingIgnoredDuringExecution": []
        }});
        assert!(build(Some(affinity)).is_ok());
    }

    #[test]
    fn unknown_affinity_fields_are_rejected_whatever_their_value() {
        for value in [json!([{"weight": 1}]), json!([]), json!(null)] {
            let affinity = json!({"podAntiAffinity": {"preferredDuringScheduling": value}});
            let err = build(Some(affinity)).unwrap_err().to_string();
            assert!(err.contains("sandbox.affinity"), "{err}");
            assert!(
                err.contains("podAntiAffinity.preferredDuringScheduling"),
                "{err}"
            );
        }
    }

    #[test]
    fn affinity_values_of_the_wrong_type_are_rejected_naming_their_path() {
        let affinity = json!({"podAntiAffinity": {"preferredDuringSchedulingIgnoredDuringExecution": [
            {"weight": "x", "podAffinityTerm": anti_affinity_term(Some("h"))}
        ]}});
        let err = build(Some(affinity)).unwrap_err().to_string();
        assert!(err.contains("sandbox.affinity"), "{err}");
        assert!(
            err.contains(
                "podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].weight"
            ),
            "{err}"
        );

        let err = build(Some(json!({"podAntiAffinity": []})))
            .unwrap_err()
            .to_string();
        assert!(err.contains("podAntiAffinity"), "{err}");
    }

    #[test]
    fn an_affinity_of_the_wrong_top_level_type_gives_a_message_without_a_path() {
        for affinity in [json!("x"), json!([])] {
            let err = build(Some(affinity)).unwrap_err().to_string();
            assert!(err.starts_with("`sandbox.affinity` is invalid: "), "{err}");
            assert!(!err.contains("at `.`"), "{err}");
        }
    }

    #[test]
    fn invalid_affinity_values_are_rejected_naming_option_and_entry() {
        let cases = [
            (
                preferred(101, anti_affinity_term(Some("kubernetes.io/hostname"))),
                "weight",
            ),
            (
                preferred(0, anti_affinity_term(Some("kubernetes.io/hostname"))),
                "weight",
            ),
            (preferred(100, anti_affinity_term(None)), "topologyKey"),
            (preferred(100, anti_affinity_term(Some(""))), "topologyKey"),
            (
                preferred(
                    100,
                    json!({"topologyKey": "h", "labelSelector": {"matchExpressions": [
                        {"key": "a", "operator": "Equals", "values": ["b"]}]}}),
                ),
                "Equals",
            ),
            (
                preferred(
                    100,
                    json!({"topologyKey": "h", "labelSelector": {"matchExpressions": [
                        {"key": "a", "operator": "In"}]}}),
                ),
                "values",
            ),
            (
                preferred(
                    100,
                    json!({"topologyKey": "h", "labelSelector": {"matchExpressions": [
                        {"key": "a", "operator": "Exists", "values": ["x"]}]}}),
                ),
                "values",
            ),
            (
                json!({"nodeAffinity": {"requiredDuringSchedulingIgnoredDuringExecution": {
                    "nodeSelectorTerms": [{"matchExpressions": [
                        {"key": "a", "operator": "Between", "values": ["1"]}]}]}}}),
                "Between",
            ),
        ];
        for (affinity, needle) in cases {
            let err = build(Some(affinity.clone())).unwrap_err().to_string();
            assert!(err.contains("sandbox.affinity"), "{affinity}: {err}");
            assert!(err.contains(needle), "{affinity}: {err}");
        }
    }

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
                git_host: "gw.svc".into(),
                git_port: 8443,
            },
        )
        .unwrap();

        assert_eq!(cfg.instance_name, "inst");
        assert_eq!(cfg.sandbox.namespace, "sb");
        assert_eq!(cfg.sandbox.storage_class.as_deref(), Some("fast"));
        assert_eq!(cfg.sandbox.runtime_class_name, None);
        assert_eq!(cfg.sandbox.issuer_kind, section.issuer_kind);
        assert_eq!(cfg.control_plane_port, 9443);
        assert_eq!(cfg.gateway_proxy_port, 3128);
        assert_eq!(cfg.gateway_git_host, "gw.svc");
        assert_eq!(cfg.gateway_git_port, 8443);
    }
}
