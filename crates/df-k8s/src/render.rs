//! Pure functions rendering Kubernetes PVCs, agent-sandbox Sandboxes, and NetworkPolicies.

use std::collections::BTreeMap;
use std::net::IpAddr;

use df_core::seams::RunSandboxSpec;
use df_core::{RunId, TaskKey, TaskVolumeSpec};
use k8s_openapi::api::core::v1::{
    PersistentVolumeClaim, PersistentVolumeClaimSpec, VolumeResourceRequirements,
};
use k8s_openapi::api::networking::v1::{
    IPBlock, NetworkPolicy, NetworkPolicyEgressRule, NetworkPolicyPeer, NetworkPolicyPort,
    NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use thiserror::Error;

use crate::config::{ProvisioningConfig, RuntimeDelivery};
use crate::crd::{
    Sandbox, SandboxOperatingMode, SandboxPodTemplate, SandboxPodTemplateMetadata,
    SandboxPodTemplateSpec, SandboxPodTemplateSpecContainers, SandboxPodTemplateSpecContainersEnv,
    SandboxPodTemplateSpecContainersSecurityContext,
    SandboxPodTemplateSpecContainersSecurityContextCapabilities,
    SandboxPodTemplateSpecContainersVolumeMounts, SandboxPodTemplateSpecHostAliases,
    SandboxPodTemplateSpecInitContainers, SandboxPodTemplateSpecInitContainersSecurityContext,
    SandboxPodTemplateSpecInitContainersSecurityContextCapabilities,
    SandboxPodTemplateSpecInitContainersVolumeMounts, SandboxPodTemplateSpecSecurityContext,
    SandboxPodTemplateSpecSecurityContextSeccompProfile, SandboxPodTemplateSpecVolumes,
    SandboxPodTemplateSpecVolumesCsi, SandboxPodTemplateSpecVolumesEmptyDir,
    SandboxPodTemplateSpecVolumesImage, SandboxPodTemplateSpecVolumesPersistentVolumeClaim,
    SandboxShutdownPolicy, SandboxSpec,
};

/// Errors encountered while rendering resources.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderError {
    #[error("headless service `{namespace}/{service}` is not allowed without allowDns")]
    HeadlessServiceNotAllowed { namespace: String, service: String },
}

/// A resolved cluster service target for network wiring and policies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClusterTarget {
    pub namespace: String,
    pub service: String,
    pub cluster_ip: Option<IpAddr>,
    pub is_headless: bool,
    pub allow_dns: bool,
    pub pod_selector: Option<BTreeMap<String, String>>,
    pub target_ports: Vec<u16>,
    pub endpoint_ips: Vec<IpAddr>,
}

/// Renders the persistent task workspace PVC (`ReadWriteOnce`).
pub fn render_pvc(task: &TaskVolumeSpec, config: &ProvisioningConfig) -> PersistentVolumeClaim {
    let name = task.task_key.volume_name();
    let size = task
        .storage_size
        .as_deref()
        .unwrap_or(&config.sandbox.pvc_size);

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "dark-factory".to_string(),
    );
    labels.insert(
        "dark-factory.io/instance".to_string(),
        config.instance_name.clone(),
    );
    labels.insert(
        "dark-factory.io/task-key".to_string(),
        task.task_key.to_string(),
    );

    let mut requests = BTreeMap::new();
    requests.insert("storage".to_string(), Quantity(size.to_string()));

    PersistentVolumeClaim {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(config.sandbox.namespace.clone()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(PersistentVolumeClaimSpec {
            access_modes: Some(vec!["ReadWriteOnce".to_string()]),
            storage_class_name: config.sandbox.storage_class.clone(),
            resources: Some(VolumeResourceRequirements {
                requests: Some(requests),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Renders the agent-sandbox `Sandbox` CRD object for a run.
pub fn render_sandbox(
    spec: &RunSandboxSpec,
    cluster_targets: &[ResolvedClusterTarget],
    config: &ProvisioningConfig,
) -> Result<Sandbox, RenderError> {
    for target in cluster_targets {
        if target.is_headless && !target.allow_dns {
            return Err(RenderError::HeadlessServiceNotAllowed {
                namespace: target.namespace.clone(),
                service: target.service.clone(),
            });
        }
    }

    let name = format!(
        "run-{}-{}",
        spec.task_key.to_string().to_lowercase(),
        spec.run_number
    );

    let mut labels = BTreeMap::new();
    // User labels are merged first so reserved system labels cannot be overridden
    for (k, v) in &spec.labels {
        labels.insert(k.clone(), v.clone());
    }
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "dark-factory".to_string(),
    );
    labels.insert(
        "dark-factory.io/instance".to_string(),
        config.instance_name.clone(),
    );
    labels.insert(
        "dark-factory.io/run-id".to_string(),
        spec.run_id.to_string(),
    );
    labels.insert(
        "dark-factory.io/task-key".to_string(),
        spec.task_key.to_string(),
    );
    labels.insert(
        "dark-factory.io/project".to_string(),
        spec.project_key.as_str().to_string(),
    );
    labels.insert(
        "dark-factory.io/persona".to_string(),
        spec.persona_id.to_string(),
    );

    // Host aliases for cluster targets
    let mut host_aliases = Vec::new();
    let mut no_proxy_entries = vec![config.control_plane_ip.to_string()];

    for target in cluster_targets {
        let fqdn = format!(
            "{}.{}.svc.{}",
            target.service, target.namespace, config.sandbox.cluster_domain
        );
        let short_svc = format!("{}.{}.svc", target.service, target.namespace);
        // Every target is reached directly, also a headless one (no ClusterIP to alias).
        no_proxy_entries.push(fqdn.clone());
        no_proxy_entries.push(short_svc.clone());

        if let Some(ip) = target.cluster_ip {
            no_proxy_entries.push(ip.to_string());
            host_aliases.push(SandboxPodTemplateSpecHostAliases {
                ip: ip.to_string(),
                hostnames: Some(vec![fqdn, short_svc]),
            });
        }
    }

    let proxy_url = format!("http://{}:{}", config.gateway_ip, config.gateway_proxy_port);
    let no_proxy_str = no_proxy_entries.join(",");

    let env = vec![
        SandboxPodTemplateSpecContainersEnv {
            name: "HTTP_PROXY".to_string(),
            value: Some(proxy_url.clone()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "HTTPS_PROXY".to_string(),
            value: Some(proxy_url.clone()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "http_proxy".to_string(),
            value: Some(proxy_url.clone()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "https_proxy".to_string(),
            value: Some(proxy_url),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "NO_PROXY".to_string(),
            value: Some(no_proxy_str.clone()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "no_proxy".to_string(),
            value: Some(no_proxy_str),
            ..Default::default()
        },
        // Names and meaning: crates/factory-agent/docs/configuration.md.
        SandboxPodTemplateSpecContainersEnv {
            name: "DF_CONTROL_PLANE_ADDR".to_string(),
            value: Some(format!(
                "{}:{}",
                config.control_plane_ip, config.control_plane_port
            )),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "DF_CONTROL_PLANE_SERVER_NAME".to_string(),
            value: Some(config.control_plane_tls_server_name.clone()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "DF_LOG_FORMAT".to_string(),
            value: Some(config.log_format.clone()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "RUST_LOG".to_string(),
            value: Some(config.sandbox.log_level.clone()),
            ..Default::default()
        },
    ];

    let volume_mounts = vec![
        SandboxPodTemplateSpecContainersVolumeMounts {
            name: "factory-runtime".to_string(),
            mount_path: "/opt/factory".to_string(),
            read_only: Some(true),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersVolumeMounts {
            name: "tls".to_string(),
            mount_path: "/run/factory/tls".to_string(),
            read_only: Some(true),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersVolumeMounts {
            name: "task-data".to_string(),
            mount_path: "/data".to_string(),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersVolumeMounts {
            name: "tmpfs".to_string(),
            mount_path: "/run/factory".to_string(),
            ..Default::default()
        },
    ];

    let mut volumes = Vec::new();

    match config.sandbox.runtime_delivery {
        RuntimeDelivery::ImageVolume => {
            volumes.push(SandboxPodTemplateSpecVolumes {
                name: "factory-runtime".to_string(),
                image: Some(SandboxPodTemplateSpecVolumesImage {
                    reference: Some(config.sandbox.runtime_image.clone()),
                    pull_policy: Some("IfNotPresent".to_string()),
                }),
                ..Default::default()
            });
        }
        RuntimeDelivery::InitContainer => {
            volumes.push(SandboxPodTemplateSpecVolumes {
                name: "factory-runtime".to_string(),
                empty_dir: Some(SandboxPodTemplateSpecVolumesEmptyDir::default()),
                ..Default::default()
            });
        }
    }

    let mut csi_attrs = BTreeMap::new();
    csi_attrs.insert(
        "csi.cert-manager.io/issuer-name".to_string(),
        config.sandbox.issuer_name.clone(),
    );
    csi_attrs.insert(
        "csi.cert-manager.io/issuer-kind".to_string(),
        config.sandbox.issuer_kind.clone(),
    );
    csi_attrs.insert(
        "csi.cert-manager.io/issuer-group".to_string(),
        config.sandbox.issuer_group.clone(),
    );
    csi_attrs.insert(
        "csi.cert-manager.io/uri-sans".to_string(),
        "spiffe://dark-factory/ns/${POD_NAMESPACE}/pod/${POD_NAME}/${POD_UID}".to_string(),
    );
    csi_attrs.insert(
        "csi.cert-manager.io/duration".to_string(),
        config.sandbox.certificate_duration.clone(),
    );
    csi_attrs.insert(
        "csi.cert-manager.io/renew-before".to_string(),
        config.sandbox.certificate_renew_before.clone(),
    );

    volumes.push(SandboxPodTemplateSpecVolumes {
        name: "tls".to_string(),
        csi: Some(SandboxPodTemplateSpecVolumesCsi {
            driver: "csi.cert-manager.io".to_string(),
            read_only: Some(true),
            volume_attributes: Some(csi_attrs),
            ..Default::default()
        }),
        ..Default::default()
    });

    volumes.push(SandboxPodTemplateSpecVolumes {
        name: "task-data".to_string(),
        persistent_volume_claim: Some(SandboxPodTemplateSpecVolumesPersistentVolumeClaim {
            claim_name: spec.task_key.volume_name(),
            ..Default::default()
        }),
        ..Default::default()
    });

    volumes.push(SandboxPodTemplateSpecVolumes {
        name: "tmpfs".to_string(),
        empty_dir: Some(SandboxPodTemplateSpecVolumesEmptyDir {
            medium: Some("Memory".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    });

    let mut init_containers = None;
    if config.sandbox.runtime_delivery == RuntimeDelivery::InitContainer {
        init_containers = Some(vec![SandboxPodTemplateSpecInitContainers {
            name: "install-runtime".to_string(),
            image: Some(config.sandbox.runtime_image.clone()),
            command: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "cp -a /opt/factory/* /target/".to_string(),
            ]),
            volume_mounts: Some(vec![SandboxPodTemplateSpecInitContainersVolumeMounts {
                name: "factory-runtime".to_string(),
                mount_path: "/target".to_string(),
                ..Default::default()
            }]),
            security_context: Some(SandboxPodTemplateSpecInitContainersSecurityContext {
                allow_privilege_escalation: Some(false),
                capabilities: Some(
                    SandboxPodTemplateSpecInitContainersSecurityContextCapabilities {
                        drop: Some(vec!["ALL".to_string()]),
                        ..Default::default()
                    },
                ),
                ..Default::default()
            }),
            ..Default::default()
        }]);
    }

    let agent_container = SandboxPodTemplateSpecContainers {
        name: "agent".to_string(),
        image: Some(spec.image.clone()),
        command: Some(vec![
            "/opt/factory/factory-agent".to_string(),
            "run".to_string(),
        ]),
        env: Some(env),
        volume_mounts: Some(volume_mounts),
        security_context: Some(SandboxPodTemplateSpecContainersSecurityContext {
            allow_privilege_escalation: Some(false),
            capabilities: Some(
                SandboxPodTemplateSpecContainersSecurityContextCapabilities {
                    drop: Some(vec!["ALL".to_string()]),
                    ..Default::default()
                },
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let pod_template_spec = SandboxPodTemplateSpec {
        runtime_class_name: config.sandbox.runtime_class_name.clone(),
        automount_service_account_token: Some(false),
        service_account_name: Some(config.sandbox.service_account_name.clone()),
        security_context: Some(SandboxPodTemplateSpecSecurityContext {
            run_as_non_root: Some(true),
            run_as_user: Some(1000),
            run_as_group: Some(1000),
            fs_group: Some(1000),
            seccomp_profile: Some(SandboxPodTemplateSpecSecurityContextSeccompProfile {
                r#type: "RuntimeDefault".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        host_aliases: if host_aliases.is_empty() {
            None
        } else {
            Some(host_aliases)
        },
        containers: vec![agent_container],
        init_containers,
        volumes: Some(volumes),
        ..Default::default()
    };

    Ok(Sandbox {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(config.sandbox.namespace.clone()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: SandboxSpec {
            operating_mode: Some(SandboxOperatingMode::Running),
            shutdown_policy: Some(SandboxShutdownPolicy::Retain),
            pod_template: SandboxPodTemplate {
                metadata: Some(SandboxPodTemplateMetadata {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: pod_template_spec,
            },
            ..Default::default()
        },
        status: None,
    })
}

/// Renders the per-run `NetworkPolicy` owned by the Sandbox.
pub fn render_network_policy(
    run_id: RunId,
    task_key: &TaskKey,
    sandbox: &Sandbox,
    cluster_targets: &[ResolvedClusterTarget],
    config: &ProvisioningConfig,
) -> NetworkPolicy {
    let name = sandbox
        .metadata
        .name
        .clone()
        .unwrap_or_else(|| format!("run-{}", run_id));

    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "dark-factory".to_string(),
    );
    labels.insert(
        "dark-factory.io/instance".to_string(),
        config.instance_name.clone(),
    );
    labels.insert("dark-factory.io/run-id".to_string(), run_id.to_string());
    labels.insert("dark-factory.io/task-key".to_string(), task_key.to_string());

    let mut owner_references = Vec::new();
    if let (Some(sandbox_name), Some(uid)) = (&sandbox.metadata.name, &sandbox.metadata.uid) {
        owner_references.push(OwnerReference {
            api_version: "agents.x-k8s.io/v1beta1".to_string(),
            kind: "Sandbox".to_string(),
            name: sandbox_name.clone(),
            uid: uid.clone(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        });
    }

    let mut egress_rules = Vec::new();

    // 1. Control plane rule
    let mut cp_ns_selector = BTreeMap::new();
    cp_ns_selector.insert(
        "kubernetes.io/metadata.name".to_string(),
        config.sandbox.instance_namespace.clone(),
    );
    let mut cp_pod_selector = BTreeMap::new();
    cp_pod_selector.insert(
        "app.kubernetes.io/component".to_string(),
        "control-plane".to_string(),
    );

    egress_rules.push(NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(cp_ns_selector),
                ..Default::default()
            }),
            pod_selector: Some(LabelSelector {
                match_labels: Some(cp_pod_selector),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ports: Some(vec![NetworkPolicyPort {
            protocol: Some("TCP".to_string()),
            port: Some(IntOrString::Int(i32::from(config.control_plane_port))),
            ..Default::default()
        }]),
    });

    // 2. Gateway rule
    let mut gw_ns_selector = BTreeMap::new();
    gw_ns_selector.insert(
        "kubernetes.io/metadata.name".to_string(),
        config.sandbox.instance_namespace.clone(),
    );
    let mut gw_pod_selector = BTreeMap::new();
    gw_pod_selector.insert(
        "app.kubernetes.io/component".to_string(),
        "gateway".to_string(),
    );

    egress_rules.push(NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(gw_ns_selector),
                ..Default::default()
            }),
            pod_selector: Some(LabelSelector {
                match_labels: Some(gw_pod_selector),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ports: Some(vec![NetworkPolicyPort {
            protocol: Some("TCP".to_string()),
            port: Some(IntOrString::Int(i32::from(config.gateway_proxy_port))),
            ..Default::default()
        }]),
    });

    // 3. Cluster targets
    let mut has_allow_dns = false;

    for target in cluster_targets {
        if target.allow_dns {
            has_allow_dns = true;
        }

        if target.target_ports.is_empty() {
            // A rule without ports would allow every port; the resolver never returns such a
            // target, so fail closed if one gets here anyway.
            continue;
        }
        let ports: Vec<NetworkPolicyPort> = target
            .target_ports
            .iter()
            .map(|&p| NetworkPolicyPort {
                protocol: Some("TCP".to_string()),
                port: Some(IntOrString::Int(i32::from(p))),
                ..Default::default()
            })
            .collect();

        if let Some(ref sel) = target.pod_selector {
            let mut target_ns_sel = BTreeMap::new();
            target_ns_sel.insert(
                "kubernetes.io/metadata.name".to_string(),
                target.namespace.clone(),
            );

            egress_rules.push(NetworkPolicyEgressRule {
                to: Some(vec![NetworkPolicyPeer {
                    namespace_selector: Some(LabelSelector {
                        match_labels: Some(target_ns_sel),
                        ..Default::default()
                    }),
                    pod_selector: Some(LabelSelector {
                        match_labels: Some(sel.clone()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ports: Some(ports),
            });
        } else if !target.endpoint_ips.is_empty() {
            // Service without selector -> ipBlocks /32 for IPv4, /128 for IPv6
            let peers: Vec<NetworkPolicyPeer> = target
                .endpoint_ips
                .iter()
                .map(|ip| {
                    let cidr = match ip {
                        IpAddr::V4(_) => format!("{}/32", ip),
                        IpAddr::V6(_) => format!("{}/128", ip),
                    };
                    NetworkPolicyPeer {
                        ip_block: Some(IPBlock { cidr, except: None }),
                        ..Default::default()
                    }
                })
                .collect();

            egress_rules.push(NetworkPolicyEgressRule {
                to: Some(peers),
                ports: Some(ports),
            });
        }
    }

    // 4. DNS if allow_dns is set
    if has_allow_dns {
        let mut dns_ns = BTreeMap::new();
        dns_ns.insert(
            "kubernetes.io/metadata.name".to_string(),
            "kube-system".to_string(),
        );
        let mut dns_pod = BTreeMap::new();
        dns_pod.insert("k8s-app".to_string(), "kube-dns".to_string());

        egress_rules.push(NetworkPolicyEgressRule {
            to: Some(vec![NetworkPolicyPeer {
                namespace_selector: Some(LabelSelector {
                    match_labels: Some(dns_ns),
                    ..Default::default()
                }),
                pod_selector: Some(LabelSelector {
                    match_labels: Some(dns_pod),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ports: Some(vec![
                NetworkPolicyPort {
                    protocol: Some("UDP".to_string()),
                    port: Some(IntOrString::Int(53)),
                    ..Default::default()
                },
                NetworkPolicyPort {
                    protocol: Some("TCP".to_string()),
                    port: Some(IntOrString::Int(53)),
                    ..Default::default()
                },
            ]),
        });
    }

    let mut pod_match_labels = BTreeMap::new();
    pod_match_labels.insert("dark-factory.io/run-id".to_string(), run_id.to_string());

    NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(config.sandbox.namespace.clone()),
            labels: Some(labels),
            owner_references: if owner_references.is_empty() {
                None
            } else {
                Some(owner_references)
            },
            ..Default::default()
        },
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(LabelSelector {
                match_labels: Some(pod_match_labels),
                ..Default::default()
            }),
            policy_types: Some(vec!["Ingress".to_string(), "Egress".to_string()]),
            ingress: None,
            egress: Some(egress_rules),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_core::ids::{PersonaId, ProjectId, RunId};
    use df_core::keys::{ProjectKey, TaskKey};
    use std::net::Ipv4Addr;
    use uuid::Uuid;

    fn test_config() -> ProvisioningConfig {
        ProvisioningConfig {
            instance_name: "test-instance".to_string(),
            sandbox: df_core::SandboxProvisioningConfig {
                namespace: "dark-factory-dev-sandboxes".to_string(),
                instance_namespace: "dark-factory-dev".to_string(),
                service_account_name: "dark-factory-sandbox".to_string(),
                storage_class: Some("factory-workspace".to_string()),
                pvc_size: "20Gi".to_string(),
                runtime_class_name: Some("kata".to_string()),
                issuer_name: "dark-factory-ca-issuer".to_string(),
                issuer_kind: "ClusterIssuer".to_string(),
                issuer_group: "cert-manager.io".to_string(),
                certificate_duration: "6h".to_string(),
                certificate_renew_before: "2h".to_string(),
                runtime_image: "ghcr.io/prskr/dark-factory/runtime:dev".to_string(),
                runtime_delivery: RuntimeDelivery::ImageVolume,
                cluster_domain: "cluster.local".to_string(),
                log_level: "info".to_string(),
            },
            control_plane_ip: IpAddr::V4(Ipv4Addr::new(10, 103, 178, 202)),
            control_plane_port: 9443,
            control_plane_tls_server_name: "dark-factory-control-plane.dark-factory-dev.svc"
                .to_string(),
            gateway_ip: IpAddr::V4(Ipv4Addr::new(10, 105, 68, 86)),
            gateway_proxy_port: 3128,
            log_format: "text".to_string(),
        }
    }

    fn test_spec() -> RunSandboxSpec {
        let task_key: TaskKey = "DF-1".parse().unwrap();
        RunSandboxSpec {
            run_id: RunId::from_uuid(Uuid::from_u128(1)),
            run_number: 1,
            task_key,
            project_id: ProjectId::from_uuid(Uuid::from_u128(10)),
            project_key: ProjectKey::new("DF").unwrap(),
            persona_id: PersonaId::from_uuid(Uuid::from_u128(20)),
            image: "test-project:latest".to_string(),
            volume_name: "task-df-1".to_string(),
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn unset_storage_class_and_runtime_class_render_as_absent_fields() {
        let config = ProvisioningConfig {
            sandbox: df_core::SandboxProvisioningConfig {
                storage_class: None,
                runtime_class_name: None,
                ..Default::default()
            },
            ..test_config()
        };
        let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
        let pvc = render_pvc(&task, &config);
        assert_eq!(pvc.spec.unwrap().storage_class_name, None);

        let sandbox = render_sandbox(&test_spec(), &[], &config).unwrap();
        let pod_spec = sandbox.spec.pod_template.spec;
        assert_eq!(pod_spec.runtime_class_name, None);
        let json = serde_json::to_value(&pod_spec).unwrap();
        assert!(json.get("runtimeClassName").is_none(), "{json}");
    }

    #[test]
    fn headless_target_is_listed_in_no_proxy_without_a_host_alias() {
        let target = ResolvedClusterTarget {
            namespace: "ns".to_string(),
            service: "db".to_string(),
            cluster_ip: None,
            is_headless: true,
            allow_dns: true,
            pod_selector: None,
            target_ports: vec![5432],
            endpoint_ips: vec![IpAddr::V4(Ipv4Addr::new(10, 1, 0, 1))],
        };
        let sandbox = render_sandbox(&test_spec(), &[target], &test_config()).unwrap();
        let container = &sandbox.spec.pod_template.spec.containers[0];
        let no_proxy = container
            .env
            .as_ref()
            .unwrap()
            .iter()
            .find(|e| e.name == "NO_PROXY")
            .and_then(|e| e.value.clone())
            .unwrap();
        assert!(no_proxy.contains("db.ns.svc.cluster.local"), "{no_proxy}");
        assert!(no_proxy.split(',').any(|e| e == "db.ns.svc"), "{no_proxy}");
        assert_eq!(sandbox.spec.pod_template.spec.host_aliases, None);
    }

    #[test]
    fn target_without_ports_gets_no_egress_rule() {
        let config = test_config();
        let sandbox = render_sandbox(&test_spec(), &[], &config).unwrap();
        let target = ResolvedClusterTarget {
            namespace: "ns".to_string(),
            service: "svc".to_string(),
            cluster_ip: None,
            is_headless: false,
            allow_dns: false,
            pod_selector: Some(BTreeMap::from([("a".to_string(), "b".to_string())])),
            target_ports: vec![],
            endpoint_ips: vec![],
        };
        let with = render_network_policy(
            test_spec().run_id,
            &test_spec().task_key,
            &sandbox,
            &[target],
            &config,
        );
        let without = render_network_policy(
            test_spec().run_id,
            &test_spec().task_key,
            &sandbox,
            &[],
            &config,
        );
        assert_eq!(with.spec, without.spec);
    }

    #[test]
    fn test_render_pvc_default() {
        let config = test_config();
        let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
        let pvc = render_pvc(&task, &config);

        assert_eq!(pvc.metadata.name.as_deref(), Some("task-df-1"));
        assert_eq!(
            pvc.metadata.namespace.as_deref(),
            Some("dark-factory-dev-sandboxes")
        );
        let labels = pvc.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get("dark-factory.io/task-key").map(String::as_str),
            Some("DF-1")
        );
        assert_eq!(
            labels.get("dark-factory.io/instance").map(String::as_str),
            Some("test-instance")
        );

        let spec = pvc.spec.unwrap();
        assert_eq!(
            spec.storage_class_name.as_deref(),
            Some("factory-workspace")
        );
        assert_eq!(spec.access_modes.unwrap(), vec!["ReadWriteOnce"]);
        let req = spec.resources.unwrap().requests.unwrap();
        assert_eq!(req.get("storage").unwrap().0, "20Gi");
    }

    #[test]
    fn test_render_sandbox_spec_scenario() {
        let config = test_config();
        let spec = test_spec();
        let targets = vec![ResolvedClusterTarget {
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

        let sandbox = render_sandbox(&spec, &targets, &config).expect("render_sandbox failed");

        assert_eq!(sandbox.metadata.name.as_deref(), Some("run-df-1-1"));
        let labels = sandbox.metadata.labels.as_ref().unwrap();
        let run_id_str = spec.run_id.to_string();
        let persona_id_str = spec.persona_id.to_string();
        assert_eq!(
            labels.get("dark-factory.io/run-id").map(String::as_str),
            Some(run_id_str.as_str())
        );
        assert_eq!(
            labels.get("dark-factory.io/task-key").map(String::as_str),
            Some("DF-1")
        );
        assert_eq!(
            labels.get("dark-factory.io/project").map(String::as_str),
            Some("DF")
        );
        assert_eq!(
            labels.get("dark-factory.io/persona").map(String::as_str),
            Some(persona_id_str.as_str())
        );

        let pod_spec = sandbox.spec.pod_template.spec;
        assert_eq!(pod_spec.runtime_class_name.as_deref(), Some("kata"));
        assert_eq!(pod_spec.automount_service_account_token, Some(false));
        assert_eq!(
            pod_spec.service_account_name.as_deref(),
            Some("dark-factory-sandbox")
        );

        let sec_ctx = pod_spec.security_context.unwrap();
        assert_eq!(sec_ctx.run_as_non_root, Some(true));
        assert_eq!(sec_ctx.run_as_user, Some(1000));
        assert_eq!(sec_ctx.seccomp_profile.unwrap().r#type, "RuntimeDefault");

        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "agent");
        assert_eq!(container.image.as_deref(), Some("test-project:latest"));
        assert_eq!(
            container.command.as_ref().unwrap(),
            &vec!["/opt/factory/factory-agent", "run"]
        );
        let cont_sec = container.security_context.as_ref().unwrap();
        assert_eq!(cont_sec.allow_privilege_escalation, Some(false));
        assert_eq!(
            cont_sec
                .capabilities
                .as_ref()
                .unwrap()
                .drop
                .as_ref()
                .unwrap(),
            &vec!["ALL"]
        );

        let mounts = container.volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 4);
        assert!(
            mounts
                .iter()
                .any(|m| m.mount_path == "/opt/factory" && m.read_only == Some(true))
        );
        assert!(
            mounts
                .iter()
                .any(|m| m.mount_path == "/run/factory/tls" && m.read_only == Some(true))
        );
        assert!(
            mounts
                .iter()
                .any(|m| m.mount_path == "/data" && m.name == "task-data")
        );
        assert!(
            mounts
                .iter()
                .any(|m| m.mount_path == "/run/factory" && m.name == "tmpfs")
        );

        // Host aliases
        let aliases = pod_spec.host_aliases.unwrap();
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].ip, "10.96.0.50");
        let hostnames = aliases[0].hostnames.as_ref().unwrap();
        assert!(hostnames.contains(&"scheduler.nativelink.svc.cluster.local".to_string()));
        assert!(hostnames.contains(&"scheduler.nativelink.svc".to_string()));

        // Volumes
        let volumes = pod_spec.volumes.unwrap();
        let runtime_vol = volumes
            .iter()
            .find(|v| v.name == "factory-runtime")
            .unwrap();
        assert_eq!(
            runtime_vol.image.as_ref().unwrap().reference.as_deref(),
            Some("ghcr.io/prskr/dark-factory/runtime:dev")
        );

        let tls_vol = volumes.iter().find(|v| v.name == "tls").unwrap();
        let csi = tls_vol.csi.as_ref().unwrap();
        assert_eq!(csi.driver, "csi.cert-manager.io");
        let attrs = csi.volume_attributes.as_ref().unwrap();
        assert_eq!(
            attrs
                .get("csi.cert-manager.io/issuer-name")
                .map(String::as_str),
            Some("dark-factory-ca-issuer")
        );
        assert_eq!(
            attrs
                .get("csi.cert-manager.io/duration")
                .map(String::as_str),
            Some("6h")
        );

        // Environment
        let env = container.env.as_ref().unwrap();
        assert!(
            env.iter().any(|e| e.name == "HTTP_PROXY"
                && e.value.as_deref() == Some("http://10.105.68.86:3128"))
        );
        assert!(env.iter().any(|e| e.name == "DF_CONTROL_PLANE_ADDR"
            && e.value.as_deref() == Some("10.103.178.202:9443")));
        assert!(env.iter().any(|e| e.name == "DF_CONTROL_PLANE_SERVER_NAME"
            && e.value.as_deref() == Some("dark-factory-control-plane.dark-factory-dev.svc")));
        assert!(env.iter().any(|e| {
            e.name == "NO_PROXY"
                && e.value
                    .as_ref()
                    .unwrap()
                    .contains("scheduler.nativelink.svc")
        }));
    }

    /// The variable table of the runtime's configuration reference; `factory-agent` checks that
    /// it matches what the runtime reads.
    const AGENT_CONFIGURATION: &str = include_str!("../../factory-agent/docs/configuration.md");

    /// Returns `(name, required)` for every `DF_*` row of the reference's variable table.
    fn documented_agent_variables() -> Vec<(&'static str, bool)> {
        AGENT_CONFIGURATION
            .lines()
            .filter_map(|line| {
                let mut cells = line.split('|').map(str::trim).skip(1);
                let name = cells.next()?.trim_matches('`');
                name.starts_with("DF_")
                    .then(|| Some((name, cells.next()? == "yes")))
                    .flatten()
            })
            .collect()
    }

    fn sandbox_env(config: &ProvisioningConfig, name: &str) -> Option<String> {
        let sandbox = render_sandbox(&test_spec(), &[], config).unwrap();
        sandbox.spec.pod_template.spec.containers[0]
            .env
            .iter()
            .flatten()
            .find(|e| e.name == name)
            .and_then(|e| e.value.clone())
    }

    #[test]
    fn sandbox_inherits_the_control_plane_log_format() {
        let json = test_config().with_log_format("json");
        assert_eq!(sandbox_env(&json, "DF_LOG_FORMAT").as_deref(), Some("json"));
        assert_eq!(
            sandbox_env(&test_config(), "DF_LOG_FORMAT").as_deref(),
            Some("text")
        );
    }

    #[test]
    fn sandbox_log_level_comes_from_the_sandbox_config() {
        let mut config = test_config();
        assert_eq!(sandbox_env(&config, "RUST_LOG").as_deref(), Some("info"));
        config.sandbox.log_level = "debug".to_string();
        assert_eq!(sandbox_env(&config, "RUST_LOG").as_deref(), Some("debug"));
    }

    #[test]
    fn sandbox_sets_the_variables_the_runtime_requires() {
        let sandbox = render_sandbox(&test_spec(), &[], &test_config()).unwrap();
        let env = sandbox.spec.pod_template.spec.containers[0]
            .env
            .clone()
            .unwrap();
        let documented = documented_agent_variables();
        assert!(
            documented.iter().any(|(_, required)| *required),
            "no required variable found in the runtime's configuration reference"
        );

        for (name, _) in documented.iter().filter(|(_, required)| *required) {
            assert!(
                env.iter()
                    .any(|e| e.name == *name && e.value.as_deref().is_some_and(|v| !v.is_empty())),
                "sandbox does not set {name}, which factory-agent requires"
            );
        }
        for e in env.iter().filter(|e| e.name.starts_with("DF_")) {
            assert!(
                documented.iter().any(|(name, _)| *name == e.name),
                "sandbox sets {}, which factory-agent does not read",
                e.name
            );
        }
    }

    #[test]
    fn test_render_sandbox_rejects_headless_without_allow_dns() {
        let config = test_config();
        let spec = test_spec();
        let targets = vec![ResolvedClusterTarget {
            namespace: "default".to_string(),
            service: "headless-svc".to_string(),
            cluster_ip: None,
            is_headless: true,
            allow_dns: false,
            pod_selector: None,
            target_ports: vec![80],
            endpoint_ips: vec![],
        }];

        let result = render_sandbox(&spec, &targets, &config);
        assert!(matches!(
            result,
            Err(RenderError::HeadlessServiceNotAllowed { .. })
        ));
    }

    #[test]
    fn test_render_sandbox_init_container_fallback() {
        let mut config = test_config();
        config.sandbox.runtime_delivery = RuntimeDelivery::InitContainer;
        let spec = test_spec();
        let sandbox = render_sandbox(&spec, &[], &config).unwrap();

        let pod_spec = sandbox.spec.pod_template.spec;
        let init_containers = pod_spec.init_containers.unwrap();
        assert_eq!(init_containers.len(), 1);
        assert_eq!(init_containers[0].name, "install-runtime");

        let volumes = pod_spec.volumes.unwrap();
        let runtime_vol = volumes
            .iter()
            .find(|v| v.name == "factory-runtime")
            .unwrap();
        assert!(runtime_vol.empty_dir.is_some());
    }

    #[test]
    fn test_render_network_policy_scenarios() {
        let config = test_config();
        let spec = test_spec();
        let mut sandbox = render_sandbox(&spec, &[], &config).unwrap();
        sandbox.metadata.uid = Some("sandbox-uid-123".to_string());

        let targets = vec![
            // Target 1: Target port mapping scenario (service port 50051 maps to target port 8080)
            ResolvedClusterTarget {
                namespace: "nativelink".to_string(),
                service: "scheduler".to_string(),
                cluster_ip: Some(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 50))),
                is_headless: false,
                allow_dns: false,
                pod_selector: Some(BTreeMap::from([(
                    "app".to_string(),
                    "scheduler".to_string(),
                )])),
                target_ports: vec![8080],
                endpoint_ips: vec![],
            },
            // Target 2: Service without selector scenario (rendered as /32 ipBlocks)
            ResolvedClusterTarget {
                namespace: "external".to_string(),
                service: "custom-db".to_string(),
                cluster_ip: Some(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 99))),
                is_headless: false,
                allow_dns: true, // Also exercises DNS scenario
                pod_selector: None,
                target_ports: vec![5432],
                endpoint_ips: vec![
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101)),
                ],
            },
        ];

        let netpol =
            render_network_policy(spec.run_id, &spec.task_key, &sandbox, &targets, &config);

        assert_eq!(netpol.metadata.name.as_deref(), Some("run-df-1-1"));
        assert_eq!(
            netpol.metadata.namespace.as_deref(),
            Some("dark-factory-dev-sandboxes")
        );

        let owner = &netpol.metadata.owner_references.unwrap()[0];
        assert_eq!(owner.kind, "Sandbox");
        assert_eq!(owner.name, "run-df-1-1");
        assert_eq!(owner.uid, "sandbox-uid-123");

        let pol_spec = netpol.spec.unwrap();
        assert_eq!(pol_spec.policy_types.unwrap(), vec!["Ingress", "Egress"]);
        assert!(pol_spec.ingress.is_none());

        let egress = pol_spec.egress.unwrap();
        // Rule 1: Control plane
        assert_eq!(
            egress[0].ports.as_ref().unwrap()[0].port,
            Some(IntOrString::Int(9443))
        );
        // Rule 2: Gateway proxy
        assert_eq!(
            egress[1].ports.as_ref().unwrap()[0].port,
            Some(IntOrString::Int(3128))
        );
        // Rule 3: Nativelink with mapped target port 8080
        assert_eq!(
            egress[2].ports.as_ref().unwrap()[0].port,
            Some(IntOrString::Int(8080))
        );
        assert!(egress[2].to.as_ref().unwrap()[0].pod_selector.is_some());
        // Rule 4: Custom DB without selector (/32 ipBlocks)
        let db_peers = egress[3].to.as_ref().unwrap();
        assert_eq!(db_peers.len(), 2);
        assert_eq!(
            db_peers[0].ip_block.as_ref().unwrap().cidr,
            "192.168.1.100/32"
        );
        assert_eq!(
            db_peers[1].ip_block.as_ref().unwrap().cidr,
            "192.168.1.101/32"
        );
        assert_eq!(
            egress[3].ports.as_ref().unwrap()[0].port,
            Some(IntOrString::Int(5432))
        );
        // Rule 5: DNS rule (kube-system kube-dns port 53)
        let dns_rule = &egress[4];
        let dns_ports = dns_rule.ports.as_ref().unwrap();
        assert!(
            dns_ports
                .iter()
                .any(|p| p.port == Some(IntOrString::Int(53))
                    && p.protocol.as_deref() == Some("UDP"))
        );
        assert!(
            dns_ports
                .iter()
                .any(|p| p.port == Some(IntOrString::Int(53))
                    && p.protocol.as_deref() == Some("TCP"))
        );
    }

    #[test]
    fn test_snapshots() {
        let config = test_config();
        let spec = test_spec();
        let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
        let pvc = render_pvc(&task, &config);
        insta::assert_yaml_snapshot!("pvc_default", pvc);

        let targets = vec![ResolvedClusterTarget {
            namespace: "nativelink".to_string(),
            service: "scheduler".to_string(),
            cluster_ip: Some(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 50))),
            is_headless: false,
            allow_dns: false,
            pod_selector: Some(BTreeMap::from([(
                "app".to_string(),
                "scheduler".to_string(),
            )])),
            target_ports: vec![8080],
            endpoint_ips: vec![],
        }];

        let sandbox = render_sandbox(&spec, &targets, &config).unwrap();
        insta::assert_yaml_snapshot!("sandbox_image_volume", sandbox);

        let mut config_init = config.clone();
        config_init.sandbox.runtime_delivery = RuntimeDelivery::InitContainer;
        let sandbox_init = render_sandbox(&spec, &targets, &config_init).unwrap();
        insta::assert_yaml_snapshot!("sandbox_init_container", sandbox_init);

        // Network policy scenarios
        // 1. Baseline (internet blocked - only cp & gw)
        let mut sandbox_with_uid = sandbox.clone();
        sandbox_with_uid.metadata.uid = Some("test-uid-123".to_string());
        let netpol_baseline =
            render_network_policy(spec.run_id, &spec.task_key, &sandbox_with_uid, &[], &config);
        insta::assert_yaml_snapshot!("netpol_baseline", netpol_baseline);

        // 2. Target port mapping
        let netpol_target = render_network_policy(
            spec.run_id,
            &spec.task_key,
            &sandbox_with_uid,
            &targets,
            &config,
        );
        insta::assert_yaml_snapshot!("netpol_target_port_mapping", netpol_target);

        // 3. Service without selector (/32 ipBlocks) & allow_dns
        let targets_no_sel = vec![ResolvedClusterTarget {
            namespace: "external".to_string(),
            service: "custom-db".to_string(),
            cluster_ip: None,
            is_headless: false,
            allow_dns: true,
            pod_selector: None,
            target_ports: vec![5432],
            endpoint_ips: vec![
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101)),
            ],
        }];
        let netpol_no_sel = render_network_policy(
            spec.run_id,
            &spec.task_key,
            &sandbox_with_uid,
            &targets_no_sel,
            &config,
        );
        insta::assert_yaml_snapshot!("netpol_selectorless_and_dns", netpol_no_sel);
    }
}
