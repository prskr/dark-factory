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

use crate::config::ProvisioningConfig;
use crate::crd::{
    Sandbox, SandboxOperatingMode, SandboxPodTemplate, SandboxPodTemplateMetadata,
    SandboxPodTemplateSpec, SandboxPodTemplateSpecContainers, SandboxPodTemplateSpecContainersEnv,
    SandboxPodTemplateSpecContainersSecurityContext,
    SandboxPodTemplateSpecContainersSecurityContextCapabilities,
    SandboxPodTemplateSpecContainersVolumeMounts, SandboxPodTemplateSpecHostAliases,
    SandboxPodTemplateSpecSecurityContext, SandboxPodTemplateSpecSecurityContextSeccompProfile,
    SandboxPodTemplateSpecVolumes, SandboxPodTemplateSpecVolumesCsi,
    SandboxPodTemplateSpecVolumesEmptyDir, SandboxPodTemplateSpecVolumesImage,
    SandboxPodTemplateSpecVolumesPersistentVolumeClaim, SandboxShutdownPolicy, SandboxSpec,
};

/// Errors encountered while rendering resources.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderError {
    #[error("headless service `{namespace}/{service}` is not allowed without allowDns")]
    HeadlessServiceNotAllowed { namespace: String, service: String },
    #[error("no image is configured for harness `{harness}`")]
    NoHarnessImage { harness: String },
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
#[expect(
    clippy::too_many_lines,
    reason = "split render_sandbox into one builder per resource section"
)]
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
            value: Some(config.log_format.as_str().to_string()),
            ..Default::default()
        },
        SandboxPodTemplateSpecContainersEnv {
            name: "RUST_LOG".to_string(),
            value: Some(config.sandbox.log_level.clone()),
            ..Default::default()
        },
    ];

    let mut volume_mounts = vec![
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

    let mut volumes = vec![SandboxPodTemplateSpecVolumes {
        name: "factory-runtime".to_string(),
        image: Some(SandboxPodTemplateSpecVolumesImage {
            reference: Some(config.sandbox.runtime_image.clone()),
            pull_policy: Some("IfNotPresent".to_string()),
        }),
        ..Default::default()
    }];

    // Below /opt/harness/<harness> so the mount does not hide other content of /opt/harness.
    if let Some(harness) = &spec.harness {
        let image =
            config
                .harness_images
                .get(harness)
                .ok_or_else(|| RenderError::NoHarnessImage {
                    harness: harness.clone(),
                })?;
        let name = format!("harness-{harness}");
        volume_mounts.push(SandboxPodTemplateSpecContainersVolumeMounts {
            name: name.clone(),
            mount_path: format!("/opt/harness/{harness}"),
            read_only: Some(true),
            ..Default::default()
        });
        volumes.push(SandboxPodTemplateSpecVolumes {
            name,
            image: Some(SandboxPodTemplateSpecVolumesImage {
                reference: Some(image.clone()),
                pull_policy: Some("IfNotPresent".to_string()),
            }),
            ..Default::default()
        });
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
#[expect(
    clippy::too_many_lines,
    reason = "split render_network_policy into ingress and egress rule builders"
)]
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
mod tests;
