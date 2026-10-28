//! Tests of the Kubernetes resource rendering.

#![allow(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

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
            cluster_domain: "cluster.local".to_string(),
            log_level: "info".to_string(),
        },
        control_plane_ip: IpAddr::V4(Ipv4Addr::new(10, 103, 178, 202)),
        control_plane_port: 9443,
        control_plane_tls_server_name: "dark-factory-control-plane.dark-factory-dev.svc"
            .to_string(),
        gateway_ip: IpAddr::V4(Ipv4Addr::new(10, 105, 68, 86)),
        gateway_proxy_port: 3128,
        harness_images: BTreeMap::from([(
            df_core::CLAUDE_CODE_HARNESS.to_string(),
            "registry.example.org/claude-harness:0.95.0".to_string(),
        )]),
        log_format: df_telemetry::logging::LogFormat::Text,
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
        harness: Some(df_core::CLAUDE_CODE_HARNESS.to_string()),
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
    assert_eq!(mounts.len(), 5);
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
        env.iter()
            .any(|e| e.name == "HTTP_PROXY"
                && e.value.as_deref() == Some("http://10.105.68.86:3128"))
    );
    assert!(
        env.iter().any(|e| e.name == "DF_CONTROL_PLANE_ADDR"
            && e.value.as_deref() == Some("10.103.178.202:9443"))
    );
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
const AGENT_CONFIGURATION: &str = include_str!("../../../factory-agent/docs/configuration.md");

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
    let json = test_config().with_log_format(df_telemetry::logging::LogFormat::Json);
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
fn harness_image_is_a_read_only_volume_below_opt_harness() {
    let sandbox = render_sandbox(&test_spec(), &[], &test_config()).unwrap();
    let pod_spec = sandbox.spec.pod_template.spec;

    let volume = pod_spec
        .volumes
        .unwrap()
        .into_iter()
        .find(|v| v.name == "harness-claude-code")
        .unwrap();
    let image = volume.image.unwrap();
    assert_eq!(
        image.reference.as_deref(),
        Some("registry.example.org/claude-harness:0.95.0")
    );
    assert_eq!(image.pull_policy.as_deref(), Some("IfNotPresent"));
    let mount = pod_spec.containers[0]
        .volume_mounts
        .as_ref()
        .unwrap()
        .iter()
        .find(|m| m.name == "harness-claude-code")
        .unwrap();
    assert_eq!(mount.mount_path, "/opt/harness/claude-code");
    assert_eq!(mount.read_only, Some(true));
}

#[test]
fn project_with_its_own_adapter_gets_no_harness_volume() {
    let spec = RunSandboxSpec {
        harness: None,
        ..test_spec()
    };
    let sandbox = render_sandbox(&spec, &[], &test_config()).unwrap();
    let pod_spec = sandbox.spec.pod_template.spec;
    assert!(
        !pod_spec
            .volumes
            .unwrap()
            .iter()
            .any(|v| v.name.starts_with("harness-"))
    );
    assert!(
        !pod_spec.containers[0]
            .volume_mounts
            .as_ref()
            .unwrap()
            .iter()
            .any(|m| m.mount_path.starts_with("/opt/harness"))
    );
}

#[test]
fn missing_harness_image_names_the_harness() {
    let config = ProvisioningConfig {
        harness_images: BTreeMap::new(),
        ..test_config()
    };
    let err = render_sandbox(&test_spec(), &[], &config).unwrap_err();
    assert_eq!(
        err,
        RenderError::NoHarnessImage {
            harness: df_core::CLAUDE_CODE_HARNESS.to_string()
        }
    );
    assert!(err.to_string().contains("claude-code"));
}

#[test]
fn pod_has_no_init_containers() {
    let sandbox = render_sandbox(&test_spec(), &[], &test_config()).unwrap();
    assert!(sandbox.spec.pod_template.spec.init_containers.is_none());
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

    let netpol = render_network_policy(spec.run_id, &spec.task_key, &sandbox, &targets, &config);

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
            .any(|p| p.port == Some(IntOrString::Int(53)) && p.protocol.as_deref() == Some("UDP"))
    );
    assert!(
        dns_ports
            .iter()
            .any(|p| p.port == Some(IntOrString::Int(53)) && p.protocol.as_deref() == Some("TCP"))
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
