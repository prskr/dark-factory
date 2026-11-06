//! Tests of the Kubernetes sandbox provider: shared fixtures, one module per topic.

use super::*;
use k8s_openapi::api::core::v1::PodStatus;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::runtime::reflector;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

mod cluster_targets;
mod events;
mod identity;
mod lifecycle;
mod pull_events;
mod reachability;

const SANDBOX_NS: &str = "dark-factory-dev-sandboxes";

type MockHandle =
    tower_test::mock::Handle<http::Request<kube::client::Body>, http::Response<String>>;

fn test_config() -> ProvisioningConfig {
    ProvisioningConfig::from_sandbox_config(
        "test-instance".to_string(),
        &df_core::SandboxProvisioningConfig::default(),
        crate::config::ControlPlaneEndpoint {
            ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            port: 9443,
            tls_server_name: "cp.local".to_string(),
        },
        crate::config::GatewayEndpoint {
            ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            proxy_port: 3128,
            git_host: "test-instance-gateway".to_string(),
            git_port: 8443,
        },
    )
}

/// Run 1 of task `DF-1` with a fresh run id; `harness` selects the harness volume.
fn df1_run_spec(harness: Option<String>) -> RunSandboxSpec {
    let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
    RunSandboxSpec {
        run_id: RunId::new(),
        run_number: 1,
        task_key: task.task_key.clone(),
        project_id: df_core::ids::ProjectId::new(),
        project_key: df_core::keys::ProjectKey::new("DF").unwrap(),
        persona_id: df_core::ids::PersonaId::new(),
        image: "alpine:latest".to_string(),
        harness,
        volume_name: task.volume_name,
        labels: BTreeMap::new(),
    }
}

fn pod_for(run_id: RunId, phase: &str, ip: Option<&str>) -> Pod {
    Pod {
        metadata: ObjectMeta {
            name: Some(format!("pod-{run_id}")),
            labels: Some(BTreeMap::from([(
                "dark-factory.io/run-id".to_string(),
                run_id.to_string(),
            )])),
            ..Default::default()
        },
        status: Some(PodStatus {
            phase: Some(phase.to_string()),
            pod_ip: ip.map(str::to_string),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Sends `body` as the JSON response with `status`.
fn send_json(
    send: tower_test::mock::SendResponse<http::Response<String>>,
    status: u16,
    body: &serde_json::Value,
) {
    send.send_response(
        http::Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .unwrap(),
    );
}

/// A provider on a mocked Kubernetes API with empty reflector stores.
fn mock_provider(config: ProvisioningConfig) -> (K8sSandboxProvider, MockHandle) {
    mock_provider_with_targets(config, Arc::new(StaticClusterTargetsProvider::new(vec![])))
}

/// [`mock_provider`] with the given cluster targets.
fn mock_provider_with_targets(
    config: ProvisioningConfig,
    targets: Arc<dyn ClusterTargetsProvider>,
) -> (K8sSandboxProvider, MockHandle) {
    let (mock_service, handle) = tower_test::mock::pair();
    let (pod_store, _) = reflector::store();
    let provider = K8sSandboxProvider::new(
        Client::new(mock_service, "default"),
        config,
        targets,
        pod_store,
    );
    (provider, handle)
}

fn status_body(code: u16, reason: &str) -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "v1", "kind": "Status", "status": "Failure",
        "message": reason, "reason": reason, "code": code
    })
}
