//! Pure functions rendering Kubernetes PVCs, agent-sandbox Sandboxes, and `NetworkPolicies`.

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
    let labels = sandbox_labels(spec, config);
    let (host_aliases, no_proxy_entries) = cluster_target_networking(cluster_targets, config);
    let (volume_mounts, volumes) = agent_volumes(spec, config)?;

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
        containers: vec![agent_container(
            spec,
            agent_env(config, &no_proxy_entries.join(",")),
            volume_mounts,
        )],
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

/// Labels of the Sandbox and its pod: the user labels plus the reserved system labels.
fn sandbox_labels(spec: &RunSandboxSpec, config: &ProvisioningConfig) -> BTreeMap<String, String> {
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
    labels
}

/// Host aliases for the cluster targets and the `NO_PROXY` entries (control plane and targets).
fn cluster_target_networking(
    cluster_targets: &[ResolvedClusterTarget],
    config: &ProvisioningConfig,
) -> (Vec<SandboxPodTemplateSpecHostAliases>, Vec<String>) {
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
    (host_aliases, no_proxy_entries)
}

/// Environment of the agent container: proxy settings, control plane address and logging.
fn agent_env(
    config: &ProvisioningConfig,
    no_proxy: &str,
) -> Vec<SandboxPodTemplateSpecContainersEnv> {
    let proxy_url = format!("http://{}:{}", config.gateway_ip, config.gateway_proxy_port);
    let var = |name: &str, value: String| SandboxPodTemplateSpecContainersEnv {
        name: name.to_string(),
        value: Some(value),
        ..Default::default()
    };
    vec![
        var("HTTP_PROXY", proxy_url.clone()),
        var("HTTPS_PROXY", proxy_url.clone()),
        var("http_proxy", proxy_url.clone()),
        var("https_proxy", proxy_url),
        var("NO_PROXY", no_proxy.to_string()),
        var("no_proxy", no_proxy.to_string()),
        // Names and meaning: crates/factory-agent/docs/configuration.md.
        var(
            "DF_CONTROL_PLANE_ADDR",
            format!("{}:{}", config.control_plane_ip, config.control_plane_port),
        ),
        var(
            "DF_CONTROL_PLANE_SERVER_NAME",
            config.control_plane_tls_server_name.clone(),
        ),
        var("DF_LOG_FORMAT", config.log_format.as_str().to_string()),
        var("RUST_LOG", config.sandbox.log_level.clone()),
    ]
}

/// Volume mounts of the agent container and the volumes of the pod.
fn agent_volumes(
    spec: &RunSandboxSpec,
    config: &ProvisioningConfig,
) -> Result<
    (
        Vec<SandboxPodTemplateSpecContainersVolumeMounts>,
        Vec<SandboxPodTemplateSpecVolumes>,
    ),
    RenderError,
> {
    let mount = |name: &str, path: &str, read_only: Option<bool>| {
        SandboxPodTemplateSpecContainersVolumeMounts {
            name: name.to_string(),
            mount_path: path.to_string(),
            read_only,
            ..Default::default()
        }
    };
    let mut volume_mounts = vec![
        mount("factory-runtime", "/opt/factory", Some(true)),
        mount("tls", "/run/factory/tls", Some(true)),
        mount("task-data", "/data", None),
        mount("tmpfs", "/run/factory", None),
    ];

    let mut volumes = vec![image_volume(
        "factory-runtime".to_string(),
        &config.sandbox.runtime_image,
    )];

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
        volume_mounts.push(mount(&name, &format!("/opt/harness/{harness}"), Some(true)));
        volumes.push(image_volume(name, image));
    }

    volumes.push(tls_volume(config));
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
    Ok((volume_mounts, volumes))
}

/// A volume that mounts an OCI image.
fn image_volume(name: String, reference: &str) -> SandboxPodTemplateSpecVolumes {
    SandboxPodTemplateSpecVolumes {
        name,
        image: Some(SandboxPodTemplateSpecVolumesImage {
            reference: Some(reference.to_string()),
            pull_policy: Some("IfNotPresent".to_string()),
        }),
        ..Default::default()
    }
}

/// The CSI volume that issues the pod certificate through cert-manager.
fn tls_volume(config: &ProvisioningConfig) -> SandboxPodTemplateSpecVolumes {
    let sandbox = &config.sandbox;
    let csi_attrs = BTreeMap::from([
        (
            "csi.cert-manager.io/issuer-name".to_string(),
            sandbox.issuer_name.clone(),
        ),
        (
            "csi.cert-manager.io/issuer-kind".to_string(),
            sandbox.issuer_kind.clone(),
        ),
        (
            "csi.cert-manager.io/issuer-group".to_string(),
            sandbox.issuer_group.clone(),
        ),
        (
            "csi.cert-manager.io/uri-sans".to_string(),
            "spiffe://dark-factory/ns/${POD_NAMESPACE}/pod/${POD_NAME}/${POD_UID}".to_string(),
        ),
        (
            "csi.cert-manager.io/duration".to_string(),
            sandbox.certificate_duration.clone(),
        ),
        (
            "csi.cert-manager.io/renew-before".to_string(),
            sandbox.certificate_renew_before.clone(),
        ),
    ]);
    SandboxPodTemplateSpecVolumes {
        name: "tls".to_string(),
        csi: Some(SandboxPodTemplateSpecVolumesCsi {
            driver: "csi.cert-manager.io".to_string(),
            read_only: Some(true),
            volume_attributes: Some(csi_attrs),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The agent container: unprivileged, all capabilities dropped.
fn agent_container(
    spec: &RunSandboxSpec,
    env: Vec<SandboxPodTemplateSpecContainersEnv>,
    volume_mounts: Vec<SandboxPodTemplateSpecContainersVolumeMounts>,
) -> SandboxPodTemplateSpecContainers {
    SandboxPodTemplateSpecContainers {
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
    }
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
        .unwrap_or_else(|| format!("run-{run_id}"));

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
            egress: Some(egress_rules(cluster_targets, config)),
        }),
    }
}

/// The egress rules: control plane, gateway, the cluster targets and, if a target allows DNS,
/// the cluster DNS. Ingress is denied by having no rule.
fn egress_rules(
    cluster_targets: &[ResolvedClusterTarget],
    config: &ProvisioningConfig,
) -> Vec<NetworkPolicyEgressRule> {
    let instance_namespace = &config.sandbox.instance_namespace;
    let mut rules = vec![
        component_rule(
            instance_namespace,
            "app.kubernetes.io/component",
            "control-plane",
            vec![tcp_port(config.control_plane_port)],
        ),
        component_rule(
            instance_namespace,
            "app.kubernetes.io/component",
            "gateway",
            vec![tcp_port(config.gateway_proxy_port)],
        ),
    ];
    rules.extend(cluster_targets.iter().filter_map(cluster_target_rule));

    if cluster_targets.iter().any(|target| target.allow_dns) {
        rules.push(component_rule(
            "kube-system",
            "k8s-app",
            "kube-dns",
            vec![
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
            ],
        ));
    }
    rules
}

fn tcp_port(port: u16) -> NetworkPolicyPort {
    NetworkPolicyPort {
        protocol: Some("TCP".to_string()),
        port: Some(IntOrString::Int(i32::from(port))),
        ..Default::default()
    }
}

/// A rule that allows `ports` to the pods labeled `label=value` in `namespace`.
fn component_rule(
    namespace: &str,
    label: &str,
    value: &str,
    ports: Vec<NetworkPolicyPort>,
) -> NetworkPolicyEgressRule {
    namespaced_pods_rule(
        namespace,
        BTreeMap::from([(label.to_string(), value.to_string())]),
        ports,
    )
}

fn namespaced_pods_rule(
    namespace: &str,
    pod_labels: BTreeMap<String, String>,
    ports: Vec<NetworkPolicyPort>,
) -> NetworkPolicyEgressRule {
    NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([(
                    "kubernetes.io/metadata.name".to_string(),
                    namespace.to_string(),
                )])),
                ..Default::default()
            }),
            pod_selector: Some(LabelSelector {
                match_labels: Some(pod_labels),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ports: Some(ports),
    }
}

/// The rule for one cluster target: its pods if the Service has a selector, else its endpoint IPs.
/// `None` if the target allows nothing (no ports, or neither selector nor endpoints).
fn cluster_target_rule(target: &ResolvedClusterTarget) -> Option<NetworkPolicyEgressRule> {
    if target.target_ports.is_empty() {
        // A rule without ports would allow every port; the resolver never returns such a
        // target, so fail closed if one gets here anyway.
        return None;
    }
    let ports: Vec<NetworkPolicyPort> = target.target_ports.iter().map(|&p| tcp_port(p)).collect();

    if let Some(selector) = &target.pod_selector {
        return Some(namespaced_pods_rule(
            &target.namespace,
            selector.clone(),
            ports,
        ));
    }
    if target.endpoint_ips.is_empty() {
        return None;
    }
    // Service without selector -> ipBlocks /32 for IPv4, /128 for IPv6
    let peers: Vec<NetworkPolicyPeer> = target
        .endpoint_ips
        .iter()
        .map(|ip| {
            let cidr = match ip {
                IpAddr::V4(_) => format!("{ip}/32"),
                IpAddr::V6(_) => format!("{ip}/128"),
            };
            NetworkPolicyPeer {
                ip_block: Some(IPBlock { cidr, except: None }),
                ..Default::default()
            }
        })
        .collect();
    Some(NetworkPolicyEgressRule {
        to: Some(peers),
        ports: Some(ports),
    })
}

#[cfg(test)]
mod tests;
