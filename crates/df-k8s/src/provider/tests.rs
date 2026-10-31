//! Tests of the Kubernetes sandbox provider.

#![expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use super::*;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateWaiting, ContainerStatus, PodStatus,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::runtime::reflector;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use uuid::Uuid;

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

#[test]
fn test_parse_spiffe_san() {
    assert_eq!(
        parse_spiffe_san("spiffe://dark-factory/ns/test-ns/pod/my-pod/uid-123"),
        Some(("test-ns", "my-pod", "uid-123"))
    );
    assert_eq!(parse_spiffe_san("https://example.com"), None);
    assert_eq!(
        parse_spiffe_san("spiffe://dark-factory/ns/test-ns/pod/my-pod"),
        None
    );
    assert_eq!(
        parse_spiffe_san("spiffe://dark-factory/ns/test-ns/pod/my-pod/uid/extra"),
        None
    );
}

#[test]
fn test_resolve_identity_scenarios() {
    let (pod_store, mut pod_writer) = reflector::store();
    let (sandbox_store, mut sb_writer) = reflector::store();

    let run_id = RunId::from_uuid(Uuid::from_u128(42));
    let sandbox_ns = "dark-factory-dev-sandboxes";
    let instance_name = "test-instance";

    // Seed Sandbox
    let sandbox = Sandbox {
        metadata: ObjectMeta {
            name: Some("run-df-1-1".to_string()),
            namespace: Some(sandbox_ns.to_string()),
            uid: Some("sb-uid-1".to_string()),
            labels: Some(BTreeMap::from([
                (
                    "dark-factory.io/instance".to_string(),
                    instance_name.to_string(),
                ),
                ("dark-factory.io/run-id".to_string(), run_id.to_string()),
            ])),
            ..Default::default()
        },
        spec: crate::crd::SandboxSpec {
            pod_template: crate::crd::SandboxPodTemplate::default(),
            ..Default::default()
        },
        status: None,
    };
    sb_writer.apply_watcher_event(&watcher::Event::Apply(sandbox));

    // Seed Pod
    let pod = Pod {
        metadata: ObjectMeta {
            name: Some("run-df-1-1".to_string()),
            namespace: Some(sandbox_ns.to_string()),
            uid: Some("pod-uid-1".to_string()),
            owner_references: Some(vec![OwnerReference {
                api_version: "agents.x-k8s.io/v1beta1".to_string(),
                kind: "Sandbox".to_string(),
                name: "run-df-1-1".to_string(),
                uid: "sb-uid-1".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        },
        status: Some(PodStatus {
            pod_ip: Some("10.0.3.7".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    pod_writer.apply_watcher_event(&watcher::Event::Apply(pod));

    // Scenario: Valid peer
    let valid_peer = PeerIdentity {
        uri_san: format!("spiffe://dark-factory/ns/{sandbox_ns}/pod/run-df-1-1/pod-uid-1"),
        peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
    };
    assert_eq!(
        resolve_identity_from_stores(
            &valid_peer,
            sandbox_ns,
            instance_name,
            &pod_store,
            &sandbox_store
        ),
        Some(run_id)
    );

    // Scenario: Peer IP mismatch
    let wrong_ip_peer = PeerIdentity {
        uri_san: format!("spiffe://dark-factory/ns/{sandbox_ns}/pod/run-df-1-1/pod-uid-1"),
        peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 99)),
    };
    assert_eq!(
        resolve_identity_from_stores(
            &wrong_ip_peer,
            sandbox_ns,
            instance_name,
            &pod_store,
            &sandbox_store
        ),
        None
    );

    // Scenario: UID mismatch after pod recreation
    let old_uid_peer = PeerIdentity {
        uri_san: format!("spiffe://dark-factory/ns/{sandbox_ns}/pod/run-df-1-1/old-uid"),
        peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
    };
    assert_eq!(
        resolve_identity_from_stores(
            &old_uid_peer,
            sandbox_ns,
            instance_name,
            &pod_store,
            &sandbox_store
        ),
        None
    );

    // Scenario: Malformed SAN
    let malformed_peer = PeerIdentity {
        uri_san: "spiffe://other/bad".to_string(),
        peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
    };
    assert_eq!(
        resolve_identity_from_stores(
            &malformed_peer,
            sandbox_ns,
            instance_name,
            &pod_store,
            &sandbox_store
        ),
        None
    );

    // Scenario: Pod not created by this instance
    assert_eq!(
        resolve_identity_from_stores(
            &valid_peer,
            sandbox_ns,
            "other-instance",
            &pod_store,
            &sandbox_store
        ),
        None
    );
}

#[test]
fn test_map_pod_event() {
    let run_id = RunId::from_uuid(Uuid::from_u128(99));
    let mut pod = Pod {
        metadata: ObjectMeta {
            name: Some("test-pod".to_string()),
            labels: Some(BTreeMap::from([(
                "dark-factory.io/run-id".to_string(),
                run_id.to_string(),
            )])),
            ..Default::default()
        },
        status: Some(PodStatus {
            phase: Some("Running".to_string()),
            pod_ip: Some("10.0.3.7".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    // 1. Running with IP -> Started
    assert_eq!(
        map_pod_event(&pod),
        Some(SandboxEvent::Started {
            run_id,
            pod_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
        })
    );

    // 2. Image pull failure -> Gone
    pod.status = Some(PodStatus {
        container_statuses: Some(vec![ContainerStatus {
            name: "agent".to_string(),
            image: "bad:tag".to_string(),
            image_id: String::new(),
            ready: false,
            restart_count: 0,
            state: Some(ContainerState {
                waiting: Some(ContainerStateWaiting {
                    reason: Some("ImagePullBackOff".to_string()),
                    message: Some("Back-off pulling image bad:tag".to_string()),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ..Default::default()
    });
    assert_eq!(
        map_pod_event(&pod),
        Some(SandboxEvent::Gone {
            run_id,
            reason: "ImagePullBackOff: Back-off pulling image bad:tag".to_string(),
        })
    );

    // 3. Deletion timestamp -> Gone
    pod.metadata.deletion_timestamp = Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
        k8s_openapi::jiff::Timestamp::now(),
    ));
    assert_eq!(
        map_pod_event(&pod),
        Some(SandboxEvent::Gone {
            run_id,
            reason: "pod deleted".to_string(),
        })
    );
}

#[tokio::test]
#[ignore = "requires live dev cluster"]
async fn test_provider_live_cluster_integration() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let client = Client::try_default().await.expect("cluster connection");
    let config = crate::test_support::live_config();

    let (pod_store, _) = reflector::store();
    let (sb_store, _) = reflector::store();
    let targets = Arc::new(StaticClusterTargetsProvider::new(vec![]));
    let provider =
        K8sSandboxProvider::new(client.clone(), config.clone(), targets, pod_store, sb_store);

    let task_key = "DF-999".parse().unwrap();
    let task = TaskVolumeSpec::for_task(task_key);
    provider
        .ensure_task_volume(&task)
        .await
        .expect("ensure task volume");

    let run_id = RunId::new();
    let spec = RunSandboxSpec {
        run_id,
        run_number: 1,
        task_key: task.task_key.clone(),
        project_id: df_core::ids::ProjectId::new(),
        project_key: df_core::keys::ProjectKey::new("DF").unwrap(),
        persona_id: df_core::ids::PersonaId::new(),
        image: "alpine:latest".to_string(),
        harness: None,
        volume_name: task.volume_name.clone(),
        labels: BTreeMap::new(),
    };

    provider.provision(&spec).await.expect("provision sandbox");

    // Idempotent teardown
    provider.teardown(run_id).await.expect("teardown run");
    provider
        .teardown(run_id)
        .await
        .expect("teardown second time idempotent");

    // The task volume outlives the run ("Run ended (PVC kept)") ...
    let pvcs: Api<PersistentVolumeClaim> =
        Api::namespaced(client.clone(), &config.sandbox.namespace);
    assert_eq!(task.volume_name, "task-df-999");
    pvcs.get(&task.volume_name)
        .await
        .expect("the PVC survives the teardown of its run");

    // ... and the next run of the task mounts the same claim ("Second run reuses the volume").
    provider
        .ensure_task_volume(&task)
        .await
        .expect("ensure task volume again");
    let second = RunSandboxSpec {
        run_id: RunId::new(),
        run_number: 2,
        ..spec.clone()
    };
    provider.provision(&second).await.expect("provision run 2");
    let sandboxes: Api<Sandbox> = Api::namespaced(client.clone(), &config.sandbox.namespace);
    let sandbox = sandboxes
        .get("run-df-999-2")
        .await
        .expect("second run's sandbox");
    let claims: Vec<String> = sandbox
        .spec
        .pod_template
        .spec
        .volumes
        .iter()
        .flatten()
        .filter_map(|v| v.persistent_volume_claim.as_ref())
        .map(|pvc| pvc.claim_name.clone())
        .collect();
    assert_eq!(claims, std::slice::from_ref(&task.volume_name));
    let task_pvcs = pvcs
        .list(&kube::api::ListParams::default().labels("dark-factory.io/task-key=DF-999"))
        .await
        .expect("list task PVCs");
    assert_eq!(task_pvcs.items.len(), 1, "one PVC per task");

    // Cleanup
    provider
        .teardown(second.run_id)
        .await
        .expect("teardown run 2");
    pvcs.delete(&task.volume_name, &DeleteParams::default())
        .await
        .ok(); // best-effort cleanup
}

/// Resolves a Service whose port `grpc` has the named targetPort `grpc-internal`, answering
/// the `EndpointSlice` list with a port called `slice_port_name`.
async fn resolve_named_target_port(
    slice_port_name: &str,
) -> Result<ResolvedClusterTarget, ResolveError> {
    use tower_test::mock;

    let (mock_service, mut handle) = mock::pair();
    let client = Client::new(mock_service, "default");

    let resolve_task = tokio::spawn(async move {
        resolve_cluster_target(&client, "target-ns", "my-svc", &[50051], false).await
    });

    let (req, send_resp) = handle.next_request().await.expect("received svc GET");
    assert_eq!(
        req.uri().path(),
        "/api/v1/namespaces/target-ns/services/my-svc"
    );
    let svc_json = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": { "name": "my-svc", "namespace": "target-ns" },
        "spec": {
            "clusterIP": "10.96.10.20",
            "selector": { "app": "my-svc" },
            "ports": [
                { "name": "grpc", "port": 50051, "targetPort": "grpc-internal" }
            ]
        }
    });
    send_resp.send_response(
        http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&svc_json).unwrap())
            .unwrap(),
    );

    // The targetPort is a name, so the EndpointSlices are listed.
    let (req, send_resp) = handle.next_request().await.expect("received slice LIST");
    assert_eq!(
        req.uri().path(),
        "/apis/discovery.k8s.io/v1/namespaces/target-ns/endpointslices"
    );
    let slice_list_json = serde_json::json!({
        "apiVersion": "discovery.k8s.io/v1",
        "kind": "EndpointSliceList",
        "metadata": {},
        "items": [{
            "metadata": { "name": "my-svc-abc", "namespace": "target-ns" },
            "addressType": "IPv4",
            "ports": [{ "name": slice_port_name, "port": 8080 }],
            "endpoints": [{ "addresses": ["10.244.1.5"] }]
        }]
    });
    send_resp.send_response(
        http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&slice_list_json).unwrap())
            .unwrap(),
    );

    resolve_task.await.unwrap()
}

#[tokio::test]
async fn named_target_port_is_resolved_via_the_service_port_name() {
    // Kubernetes names the EndpointSlice port after the Service port (`grpc`), not after the
    // targetPort (`grpc-internal`).
    let target = resolve_named_target_port("grpc")
        .await
        .expect("resolve success");
    assert_eq!(target.service, "my-svc");
    assert_eq!(target.namespace, "target-ns");
    assert_eq!(
        target.cluster_ip,
        Some(IpAddr::V4(Ipv4Addr::new(10, 96, 10, 20)))
    );
    assert_eq!(target.target_ports, vec![8080]);
}

#[tokio::test]
async fn unresolvable_named_target_port_is_an_error_not_a_guess() {
    let err = resolve_named_target_port("other")
        .await
        .expect_err("no EndpointSlice port is called `grpc`");
    assert!(
        matches!(&err, ResolveError::Transient(msg) if msg.contains("grpc-internal")),
        "{err}"
    );
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

fn image_pull_failure_pod(run_id: RunId) -> Pod {
    let mut pod = pod_for(run_id, "Pending", None);
    pod.status = Some(PodStatus {
        phase: Some("Pending".to_string()),
        container_statuses: Some(vec![ContainerStatus {
            name: "agent".to_string(),
            image: "bad:tag".to_string(),
            image_id: String::new(),
            ready: false,
            restart_count: 0,
            state: Some(ContainerState {
                waiting: Some(ContainerStateWaiting {
                    reason: Some("ErrImagePull".to_string()),
                    message: Some("pull access denied".to_string()),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ..Default::default()
    });
    pod
}

/// Feeds `pod_events` through [`forward_events`] and returns what was emitted.
async fn run_loop(pod_events: Vec<watcher::Event<Pod>>) -> Vec<SandboxEvent> {
    let emitted = std::sync::Mutex::new(Vec::new());
    let pods = futures::stream::iter(pod_events.into_iter().map(Ok));
    let sandboxes = futures::stream::pending();
    forward_events(pods, sandboxes, |event| emitted.lock().unwrap().push(event)).await;
    emitted.into_inner().unwrap()
}

#[tokio::test]
async fn event_loop_emits_gone_for_image_pull_failure_without_a_prior_started() {
    // Also the state after a control plane restart: nothing was ever `Started`.
    let run_id = RunId::from_uuid(Uuid::from_u128(1));
    let events = run_loop(vec![
        watcher::Event::Apply(image_pull_failure_pod(run_id)),
        // The kubelet keeps updating the waiting pod; the run must be reported once.
        watcher::Event::Apply(image_pull_failure_pod(run_id)),
    ])
    .await;

    assert_eq!(
        events,
        vec![SandboxEvent::Gone {
            run_id,
            reason: "ErrImagePull: pull access denied".to_string()
        }]
    );
}

#[tokio::test]
async fn event_loop_emits_gone_for_failed_pod_and_started_once() {
    let started = RunId::from_uuid(Uuid::from_u128(2));
    let failed = RunId::from_uuid(Uuid::from_u128(3));
    let events = run_loop(vec![
        watcher::Event::Apply(pod_for(started, "Running", Some("10.0.0.5"))),
        watcher::Event::Apply(pod_for(started, "Running", Some("10.0.0.5"))),
        watcher::Event::Apply(pod_for(failed, "Failed", None)),
        watcher::Event::Delete(pod_for(failed, "Failed", None)),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            SandboxEvent::Started {
                run_id: started,
                pod_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))
            },
            SandboxEvent::Gone {
                run_id: failed,
                reason: "pod failed".to_string()
            },
        ]
    );
}

#[tokio::test]
async fn event_loop_emits_gone_for_pods_that_vanished_during_a_relist() {
    let kept = RunId::from_uuid(Uuid::from_u128(4));
    let vanished = RunId::from_uuid(Uuid::from_u128(5));
    let events = run_loop(vec![
        watcher::Event::Apply(pod_for(kept, "Running", Some("10.0.0.1"))),
        watcher::Event::Apply(pod_for(vanished, "Running", Some("10.0.0.2"))),
        // Watch dropped; the relist only returns `kept` and never sends a `Delete`.
        watcher::Event::Init,
        watcher::Event::InitApply(pod_for(kept, "Running", Some("10.0.0.1"))),
        watcher::Event::InitDone,
    ])
    .await;

    assert_eq!(events.len(), 3, "{events:?}");
    assert_eq!(
        events[2],
        SandboxEvent::Gone {
            run_id: vanished,
            reason: "pod no longer present".to_string()
        }
    );
}

fn sandbox_for(run_id: RunId) -> Sandbox {
    Sandbox {
        metadata: pod_for(run_id, "Pending", None).metadata,
        spec: crate::crd::SandboxSpec {
            pod_template: crate::crd::SandboxPodTemplate::default(),
            ..Default::default()
        },
        status: None,
    }
}

#[test]
fn relist_reports_never_running_pods_and_sandboxes_that_vanished() {
    let run_id = RunId::from_uuid(Uuid::from_u128(8));
    let mut tracker = EventTracker::default();
    // Pending pod: no event, but the pod and its Sandbox are known.
    assert!(
        tracker
            .pod_event(watcher::Event::Apply(pod_for(run_id, "Pending", None)))
            .is_empty()
    );
    assert!(
        tracker
            .sandbox_event(watcher::Event::Apply(sandbox_for(run_id)))
            .is_empty()
    );

    // Both watches restart and the relists return nothing.
    assert!(tracker.sandbox_event(watcher::Event::Init).is_empty());
    let from_sandboxes = tracker.sandbox_event(watcher::Event::InitDone);
    assert!(tracker.pod_event(watcher::Event::Init).is_empty());
    let from_pods = tracker.pod_event(watcher::Event::InitDone);

    // One Gone for the run, whichever watch reports it first.
    let all: Vec<_> = from_sandboxes.into_iter().chain(from_pods).collect();
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(matches!(&all[0], SandboxEvent::Gone { run_id: r, .. } if *r == run_id));
}

#[tokio::test]
async fn tracker_emits_one_gone_when_the_sandbox_is_deleted() {
    let run_id = RunId::from_uuid(Uuid::from_u128(6));
    let mut tracker = EventTracker::default();
    let sandbox = Sandbox {
        metadata: pod_for(run_id, "Running", None).metadata,
        spec: crate::crd::SandboxSpec {
            pod_template: crate::crd::SandboxPodTemplate::default(),
            ..Default::default()
        },
        status: None,
    };

    let first = tracker.sandbox_event(watcher::Event::Delete(sandbox.clone()));
    assert_eq!(
        first,
        vec![SandboxEvent::Gone {
            run_id,
            reason: "sandbox deleted".to_string()
        }]
    );
    assert!(
        tracker
            .sandbox_event(watcher::Event::Delete(sandbox))
            .is_empty()
    );
    assert!(
        tracker
            .pod_event(watcher::Event::Delete(pod_for(run_id, "Running", None)))
            .is_empty()
    );
}

#[tokio::test]
async fn ensure_task_volume_times_out_when_the_api_does_not_answer() {
    use tower_test::mock;

    tokio::time::pause();
    // Keep the handle alive but never answer.
    let (mock_service, _handle) =
        mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
    let client = Client::new(mock_service, "default");
    let (pod_store, _) = reflector::store();
    let (sb_store, _) = reflector::store();
    let provider = K8sSandboxProvider::new(
        client,
        test_config(),
        Arc::new(StaticClusterTargetsProvider::new(vec![])),
        pod_store,
        sb_store,
    );

    let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
    let err = provider.ensure_task_volume(&task).await.unwrap_err();
    assert!(
        matches!(&err, ProviderError::Backend(msg) if msg.contains("timeout")),
        "{err}"
    );
}

#[tokio::test]
async fn provision_without_a_harness_image_is_invalid_and_names_the_harness() {
    let (mock_service, _handle) =
        tower_test::mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
    let (pod_store, _) = reflector::store();
    let (sb_store, _) = reflector::store();
    let config = ProvisioningConfig {
        harness_images: BTreeMap::new(),
        ..test_config()
    };
    let provider = K8sSandboxProvider::new(
        Client::new(mock_service, "default"),
        config,
        Arc::new(StaticClusterTargetsProvider::new(vec![])),
        pod_store,
        sb_store,
    );
    let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
    let spec = RunSandboxSpec {
        run_id: RunId::new(),
        run_number: 1,
        task_key: task.task_key.clone(),
        project_id: df_core::ids::ProjectId::new(),
        project_key: df_core::keys::ProjectKey::new("DF").unwrap(),
        persona_id: df_core::ids::PersonaId::new(),
        image: "alpine:latest".to_string(),
        harness: Some(df_core::CLAUDE_CODE_HARNESS.to_string()),
        volume_name: task.volume_name.clone(),
        labels: BTreeMap::new(),
    };

    let err = provider.provision(&spec).await.unwrap_err();
    assert!(
        matches!(&err, ProviderError::InvalidSpec(msg) if msg.contains("claude-code")),
        "{err}"
    );
}

#[tokio::test]
async fn events_stream_survives_lag_and_keeps_later_events() {
    use futures::StreamExt as _;
    let (pod_store, _) = reflector::store();
    let (sb_store, _) = reflector::store();
    let (mock_service, _handle) =
        tower_test::mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
    let provider = K8sSandboxProvider::new(
        Client::new(mock_service, "default"),
        test_config(),
        Arc::new(StaticClusterTargetsProvider::new(vec![])),
        pod_store,
        sb_store,
    );
    let mut events = provider.events();
    // The channel holds 128 events: overflow it, then send one more that must still arrive.
    for n in 0..200_u128 {
        provider.emit_event(SandboxEvent::Gone {
            run_id: RunId::from_uuid(Uuid::from_u128(n)),
            reason: "x".to_string(),
        });
    }
    // The lagged-over events are dropped; the 128 newest still arrive, newest last.
    let mut run_ids = Vec::new();
    for _ in 0..128 {
        let event = events.next().await.expect("stream stays open after lag");
        run_ids.push(match event {
            SandboxEvent::Gone { run_id, .. } | SandboxEvent::Started { run_id, .. } => run_id,
        });
    }
    assert_eq!(
        run_ids.first(),
        Some(&RunId::from_uuid(Uuid::from_u128(72)))
    );
    assert_eq!(
        run_ids.last(),
        Some(&RunId::from_uuid(Uuid::from_u128(199)))
    );
}

fn rule(ns: &str, svc: &str) -> ClusterServiceRef {
    ClusterServiceRef {
        namespace: ns.to_string(),
        service: svc.to_string(),
        ports: vec![80],
        allow_dns: false,
    }
}

type MockHandle =
    tower_test::mock::Handle<http::Request<kube::client::Body>, http::Response<String>>;

/// Answers the next request with `status`: a Service for 200, an API error Status otherwise.
/// With `headless_without_ports` the answered Service is headless.
async fn answer_service(handle: &mut MockHandle, status: u16) {
    answer_service_as(handle, status, false).await;
}

async fn answer_service_as(handle: &mut MockHandle, status: u16, headless: bool) {
    let (req, send) = handle.next_request().await.expect("service GET");
    let name = req.uri().path().rsplit('/').next().unwrap().to_string();
    let body = if status == 200 {
        serde_json::json!({
            "apiVersion": "v1", "kind": "Service",
            "metadata": { "name": name },
            "spec": {
                "clusterIP": if headless { "None" } else { "10.0.0.9" },
                "selector": { "app": "x" },
                "ports": [{ "port": 80 }]
            }
        })
    } else {
        serde_json::json!({ "kind": "Status", "apiVersion": "v1", "code": status, "message": "boom" })
    };
    send.send_response(
        http::Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .unwrap(),
    );
}

/// Runs `resolve(scope, rules)` and answers its Service lookups with `statuses`.
async fn resolve_answering(
    resolver: &Arc<ClusterTargetResolver>,
    handle: &mut MockHandle,
    scope: Option<&'static str>,
    rules: &[ClusterServiceRef],
    statuses: &[u16],
) -> Vec<ResolvedClusterTarget> {
    let (r, rl) = (resolver.clone(), rules.to_vec());
    let task = tokio::spawn(async move { r.resolve(scope, &rl).await });
    for status in statuses {
        answer_service(handle, *status).await;
    }
    task.await.unwrap()
}

fn new_resolver() -> (Arc<ClusterTargetResolver>, MockHandle) {
    let (mock_service, handle) = tower_test::mock::pair();
    (
        Arc::new(ClusterTargetResolver::new(Client::new(
            mock_service,
            "default",
        ))),
        handle,
    )
}

#[tokio::test]
async fn one_unresolvable_rule_does_not_block_the_others() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "broken"), rule("b", "ok")];
    let targets = resolve_answering(&resolver, &mut handle, None, &rules, &[404, 200]).await;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].service, "ok");
}

#[tokio::test]
async fn a_server_error_reuses_the_last_good_resolution() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    assert_eq!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[200])
            .await
            .len(),
        1
    );
    let kept = resolve_answering(&resolver, &mut handle, None, &rules, &[500]).await;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].service, "svc");
}

#[tokio::test]
async fn a_timeout_reuses_the_last_good_resolution() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

    tokio::time::pause();
    let (r, rl) = (resolver.clone(), rules.clone());
    let task = tokio::spawn(async move { r.resolve(None, &rl).await });
    // Receive the request but never answer it; the call runs into K8S_API_TIMEOUT.
    let _unanswered = handle.next_request().await.expect("service GET");
    let kept = task.await.unwrap();
    assert_eq!(kept.len(), 1);
}

#[tokio::test]
async fn a_404_evicts_the_cache_and_skips_the_rule() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

    assert!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[404])
            .await
            .is_empty()
    );
    // Evicted: a later server error has nothing to fall back to.
    assert!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[500])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_forbidden_answer_evicts_the_cache_and_skips_the_rule() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;
    assert!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[403])
            .await
            .is_empty()
    );
    assert!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[500])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_service_that_became_headless_evicts_the_cache_and_skips_the_rule() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

    // Same rule, but the Service is headless now and the rule has no `allowDns`.
    let (r, rl) = (resolver.clone(), rules.clone());
    let task = tokio::spawn(async move { r.resolve(None, &rl).await });
    answer_service_as(&mut handle, 200, true).await;
    assert!(task.await.unwrap().is_empty());

    assert!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[500])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_rule_that_was_removed_and_added_again_has_no_history() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolve_answering(&resolver, &mut handle, None, &rules, &[200]).await;

    // Removed from the allowlist: no lookups, and the cache entry is pruned.
    assert!(
        resolve_answering(&resolver, &mut handle, None, &[], &[])
            .await
            .is_empty()
    );
    // Added again while the API fails: must not resurrect the old resolution.
    assert!(
        resolve_answering(&resolver, &mut handle, None, &rules, &[500])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn pruning_one_project_scope_keeps_the_cache_of_the_others() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolve_answering(&resolver, &mut handle, Some("ALPHA"), &rules, &[200]).await;
    // Another project, with a different rule set, resolves in between.
    resolve_answering(&resolver, &mut handle, Some("BETA"), &[], &[]).await;

    let kept = resolve_answering(&resolver, &mut handle, Some("ALPHA"), &rules, &[500]).await;
    assert_eq!(kept.len(), 1);
}

async fn resolve_with_service(
    spec: serde_json::Value,
    ports: &[u16],
    dns: bool,
    lists_slices: bool,
) -> Result<ResolvedClusterTarget, ResolveError> {
    let (mock_service, mut handle) = tower_test::mock::pair();
    let client = Client::new(mock_service, "default");
    let ports = ports.to_vec();
    let task =
        tokio::spawn(
            async move { resolve_cluster_target(&client, "ns", "svc", &ports, dns).await },
        );
    let (_, send) = handle.next_request().await.expect("service GET");
    send.send_response(
        http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(
                serde_json::json!({
                    "apiVersion": "v1", "kind": "Service",
                    "metadata": { "name": "svc", "namespace": "ns" },
                    "spec": spec
                })
                .to_string(),
            )
            .unwrap(),
    );
    // Headless / selector-less services also list EndpointSlices.
    if lists_slices {
        let (_, send) = handle.next_request().await.expect("slice LIST");
        send.send_response(
            http::Response::builder()
                .status(200)
                .header("Content-Type", "application/json")
                .body(
                    serde_json::json!({
                        "apiVersion": "discovery.k8s.io/v1", "kind": "EndpointSliceList",
                        "metadata": {}, "items": []
                    })
                    .to_string(),
                )
                .unwrap(),
        );
    }
    task.await.unwrap()
}

#[tokio::test]
async fn headless_service_without_allow_dns_is_rejected_at_resolution() {
    let err = resolve_with_service(
        serde_json::json!({ "clusterIP": "None", "ports": [{ "port": 80 }] }),
        &[],
        false,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ResolveError::Permanent(m) if m.contains("allowDns")));
}

#[tokio::test]
async fn headless_service_without_any_port_is_rejected_not_rendered_portless() {
    let err = resolve_with_service(serde_json::json!({ "clusterIP": "None" }), &[], true, true)
        .await
        .unwrap_err();
    assert!(matches!(err, ResolveError::Permanent(m) if m.contains("no ports")));
}

#[tokio::test]
async fn configured_ports_that_match_no_service_port_are_an_error() {
    let err = resolve_with_service(
            serde_json::json!({ "clusterIP": "10.0.0.1", "selector": {"a": "b"}, "ports": [{ "port": 80 }] }),
            &[9999],
            false,
            false,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ResolveError::Permanent(m) if m.contains("9999")));
}

#[tokio::test]
async fn named_target_port_collects_the_ports_of_all_slices() {
    use tower_test::mock;
    let (mock_service, mut handle) = mock::pair();
    let client = Client::new(mock_service, "default");
    let task =
        tokio::spawn(
            async move { resolve_cluster_target(&client, "ns", "svc", &[80], false).await },
        );
    let (_, send) = handle.next_request().await.unwrap();
    send.send_response(
        http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(
                serde_json::json!({
                    "apiVersion": "v1", "kind": "Service",
                    "metadata": { "name": "svc", "namespace": "ns" },
                    "spec": { "clusterIP": "10.0.0.1", "selector": {"a": "b"},
                              "ports": [{ "name": "web", "port": 80, "targetPort": "http" }] }
                })
                .to_string(),
            )
            .unwrap(),
    );
    let (_, send) = handle.next_request().await.unwrap();
    let slice = |name: &str, port: i32| {
        serde_json::json!({
            "metadata": { "name": name, "namespace": "ns" },
            "addressType": "IPv4",
            "ports": [{ "name": "web", "port": port }],
            "endpoints": [{ "addresses": ["10.1.0.1"] }]
        })
    };
    send.send_response(
        http::Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(
                serde_json::json!({
                    "apiVersion": "discovery.k8s.io/v1", "kind": "EndpointSliceList",
                    "metadata": {}, "items": [slice("a", 8080), slice("b", 9090)]
                })
                .to_string(),
            )
            .unwrap(),
    );
    assert_eq!(task.await.unwrap().unwrap().target_ports, vec![8080, 9090]);
}
