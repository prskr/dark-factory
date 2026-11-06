//! Task volumes and the sandbox lifecycle: provision, existence check, teardown.

use super::*;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a live cluster (DF_LIVE_* variables, see README)"]
async fn test_provider_live_cluster_integration() {
    use futures::StreamExt as _;

    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let client = Client::try_default().await.expect("cluster connection");
    let config = crate::test_support::live_config(&client).await;

    let targets = Arc::new(StaticClusterTargetsProvider::new(vec![]));
    let (provider, pod_watcher) =
        K8sSandboxProvider::start(client.clone(), config.clone(), targets);
    let pod_watcher = tokio::spawn(pod_watcher);
    let mut events = provider.events();

    let task_key = "DF-999".parse().unwrap();
    let task = TaskVolumeSpec::for_task(task_key);
    provider
        .ensure_task_volume(&task)
        .await
        .expect("ensure task volume");

    let run_id = RunId::new();
    let spec = RunSandboxSpec {
        run_id,
        task_key: task.task_key.clone(),
        volume_name: task.volume_name.clone(),
        ..df1_run_spec(None)
    };

    provider.provision(&spec).await.expect("provision sandbox");

    // The pod watcher reports the running pod with its IP.
    let started = tokio::time::timeout(std::time::Duration::from_secs(180), async {
        while let Some(event) = events.next().await {
            if let SandboxEvent::Started {
                run_id: started,
                pod_ip,
            } = event
                && started == run_id
            {
                return Some(pod_ip);
            }
        }
        None
    })
    .await;
    let pod_ip = started
        .expect("Started event within 180 s")
        .expect("event stream open");
    assert!(!pod_ip.is_unspecified(), "{pod_ip}");

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
    let pods: Api<Pod> = Api::namespaced(client.clone(), &config.sandbox.namespace);
    let pod = pods.get("run-df-999-2").await.expect("second run's pod");
    assert_eq!(
        pod.spec.as_ref().and_then(|s| s.restart_policy.as_deref()),
        Some("Never")
    );
    let claims: Vec<String> = pod
        .spec
        .iter()
        .flat_map(|s| s.volumes.iter().flatten())
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
    pod_watcher.abort();
}

#[tokio::test]
async fn ensure_task_volume_times_out_when_the_api_does_not_answer() {
    tokio::time::pause();
    // Keep the handle alive but never answer.
    let (provider, _handle) = mock_provider(test_config());

    let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
    let err = provider.ensure_task_volume(&task).await.unwrap_err();
    assert!(
        matches!(&err, ProviderError::Backend(msg) if msg.contains("timeout")),
        "{err}"
    );
}

/// Deletes the volume of `DF-1` against a mock API that answers the request with `status` and
/// `body`; returns the result and the method and path of the request.
async fn delete_volume_answered_with(
    status: u16,
    body: serde_json::Value,
) -> (Result<(), ProviderError>, String, String) {
    let (mock_service, mut handle) =
        tower_test::mock::pair::<http::Request<kube::client::Body>, http::Response<String>>();
    let (pod_store, _) = reflector::store();
    let provider = K8sSandboxProvider::new(
        Client::new(mock_service, "default"),
        test_config(),
        Arc::new(StaticClusterTargetsProvider::new(vec![])),
        pod_store,
    );
    let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
    let call = tokio::spawn(async move { provider.delete_task_volume(&task).await });

    let (req, send) = handle.next_request().await.expect("PVC DELETE");
    let request = (req.method().to_string(), req.uri().path().to_string());
    send.send_response(
        http::Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .unwrap(),
    );
    (call.await.unwrap(), request.0, request.1)
}

#[tokio::test]
async fn delete_task_volume_deletes_the_pvc_of_the_task() {
    let namespace = test_config().sandbox.namespace;
    let pvc = serde_json::json!({
        "apiVersion": "v1", "kind": "PersistentVolumeClaim",
        "metadata": { "name": "task-df-1", "namespace": namespace }
    });

    let (result, method, path) = delete_volume_answered_with(200, pvc).await;

    assert_eq!(result, Ok(()));
    assert_eq!(method, "DELETE");
    assert_eq!(
        path,
        format!("/api/v1/namespaces/{namespace}/persistentvolumeclaims/task-df-1")
    );
}

#[tokio::test]
async fn delete_task_volume_succeeds_when_the_pvc_is_already_gone() {
    let (result, method, _) = delete_volume_answered_with(404, status_body(404, "NotFound")).await;

    assert_eq!(method, "DELETE");
    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn delete_task_volume_reports_other_api_errors() {
    let (result, _, _) = delete_volume_answered_with(403, status_body(403, "Forbidden")).await;

    assert!(
        matches!(&result, Err(ProviderError::Backend(msg)) if msg.contains("task-df-1")),
        "{result:?}"
    );
}

#[tokio::test]
async fn provision_without_a_harness_image_is_invalid_and_names_the_harness() {
    let (provider, _handle) = mock_provider(ProvisioningConfig {
        harness_images: BTreeMap::new(),
        ..test_config()
    });
    let spec = df1_run_spec(Some(df_core::CLAUDE_CODE_HARNESS.to_string()));

    let err = provider.provision(&spec).await.unwrap_err();
    assert!(
        matches!(&err, ProviderError::InvalidSpec(msg) if msg.contains("claude-code")),
        "{err}"
    );
}

/// Provider whose Kubernetes API answers the next request with a `Status` of code 403.
async fn ensure_volume_rejected_with(reason: &str, message: &str) -> ProviderError {
    let (provider, mut handle) = mock_provider(test_config());
    let task = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
    let call = tokio::spawn(async move { provider.ensure_task_volume(&task).await });
    let (_req, send) = handle.next_request().await.expect("PVC patch");
    let body = serde_json::json!({
        "kind": "Status", "apiVersion": "v1", "status": "Failure",
        "code": 403, "reason": reason, "message": message,
    });
    send_json(send, 403, &body);
    call.await.unwrap().unwrap_err()
}

#[tokio::test]
async fn a_quota_rejection_is_a_capacity_error() {
    let err = ensure_volume_rejected_with(
        "Forbidden",
        "persistentvolumeclaims \"df-df-1\" is forbidden: exceeded quota: sandboxes, requested: requests.storage=10Gi",
    )
    .await;
    assert!(matches!(err, ProviderError::Capacity(_)), "{err}");
}

#[tokio::test]
async fn a_forbidden_response_without_quota_text_is_a_backend_error() {
    let err = ensure_volume_rejected_with(
        "Forbidden",
        "persistentvolumeclaims is forbidden: User cannot patch resource",
    )
    .await;
    assert!(matches!(err, ProviderError::Backend(_)), "{err}");
}

/// Provider on a mocked Kubernetes API; `sandbox_exists` runs against the answer that
/// the mock gives to the pod list request, and the request's query is returned with it.
async fn sandbox_exists_answered(
    run_id: RunId,
    status: u16,
    items: Vec<Pod>,
) -> (Result<bool, ProviderError>, String) {
    let (provider, mut handle) = mock_provider(test_config());
    let call = tokio::spawn(async move { provider.sandbox_exists(run_id).await });
    let (req, send) = handle.next_request().await.expect("pod list");
    let query = req.uri().query().unwrap_or_default().to_string();
    let body = if status == 200 {
        serde_json::json!({
            "apiVersion": "v1", "kind": "PodList",
            "metadata": {}, "items": items,
        })
    } else {
        status_body(status, "boom")
    };
    send_json(send, status, &body);
    (call.await.unwrap(), query)
}

#[tokio::test]
async fn sandbox_exists_is_true_for_a_listed_pod_and_selects_by_instance_and_run() {
    let run_id = RunId::from_uuid(Uuid::from_u128(7));
    let (exists, query) =
        sandbox_exists_answered(run_id, 200, vec![pod_for(run_id, "Pending", None)]).await;
    assert!(exists.unwrap());
    let selector =
        format!("dark-factory.io%2Finstance%3Dtest-instance%2Cdark-factory.io%2Frun-id%3D{run_id}");
    assert!(query.contains(&selector), "{query}");
}
#[tokio::test]
async fn sandbox_exists_is_false_for_an_empty_list() {
    let run_id = RunId::from_uuid(Uuid::from_u128(7));
    let (exists, _) = sandbox_exists_answered(run_id, 200, vec![]).await;
    assert!(!exists.unwrap());
}

#[tokio::test]
async fn sandbox_exists_reports_an_api_error() {
    let run_id = RunId::from_uuid(Uuid::from_u128(7));
    let (exists, _) = sandbox_exists_answered(run_id, 500, vec![]).await;
    assert!(
        matches!(exists, Err(ProviderError::Backend(_))),
        "{exists:?}"
    );
}

/// What `provision` did against a mock API.
struct Provisioned {
    result: Result<(), ProviderError>,
    /// `METHOD path` of each request, in order.
    requests: Vec<String>,
    /// JSON body of each request (null if it had none), in order.
    bodies: Vec<serde_json::Value>,
}

/// Provisions run 1 of `DF-1` against a mock API that answers the requests with `first` (the
/// Pod create) and then `followups`.
async fn provision_with_pod_create_answered(
    first: (u16, serde_json::Value),
    followups: Vec<(u16, serde_json::Value)>,
) -> Provisioned {
    let (provider, mut handle) = mock_provider(test_config());
    let spec = df1_run_spec(None);
    let call = tokio::spawn(async move { provider.provision(&spec).await });
    let mut requests = Vec::new();
    let mut bodies = Vec::new();
    for (status, body) in std::iter::once(first).chain(followups) {
        let (req, send) = handle.next_request().await.expect("API request");
        requests.push(format!("{} {}", req.method(), req.uri().path()));
        let bytes = req.into_body().collect_bytes().await.unwrap();
        bodies.push(serde_json::from_slice(&bytes).unwrap_or_default());
        send_json(send, status, &body);
    }
    Provisioned {
        result: call.await.unwrap(),
        requests,
        bodies,
    }
}

fn existing_pod_json() -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "v1", "kind": "Pod",
        "metadata": { "name": "run-df-1-1", "uid": "existing-uid" },
    })
}

#[tokio::test]
async fn provision_creates_the_pod_and_owns_the_policy_by_it() {
    let Provisioned {
        result,
        requests,
        bodies,
    } = provision_with_pod_create_answered(
        (201, existing_pod_json()),
        vec![(
            200,
            serde_json::json!({
                "apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicy",
                "metadata": { "name": "run-df-1-1" },
            }),
        )],
    )
    .await;
    result.unwrap();
    assert!(
        requests[0].starts_with("POST ") && requests[0].ends_with("/pods"),
        "{requests:?}"
    );
    assert!(requests[1].starts_with("PATCH ") && requests[1].contains("networkpolicies"));
    assert_eq!(bodies[0]["spec"]["restartPolicy"], "Never");
    assert_eq!(
        bodies[1]["metadata"]["ownerReferences"][0]["uid"],
        "existing-uid"
    );
}

#[tokio::test]
async fn provisioning_an_existing_pod_succeeds_and_keeps_it() {
    let Provisioned { result, requests, bodies } = provision_with_pod_create_answered(
        (409, serde_json::json!({
            "kind": "Status", "apiVersion": "v1", "status": "Failure",
            "code": 409, "reason": "AlreadyExists", "message": "pods \"run-df-1-1\" already exists",
        })),
        vec![
            (200, existing_pod_json()),
            (200, serde_json::json!({
                "apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicy",
                "metadata": { "name": "run-df-1-1" },
            })),
        ],
    )
    .await;
    result.unwrap();
    assert!(
        requests[1].starts_with("GET ") && requests[1].ends_with("/pods/run-df-1-1"),
        "{requests:?}"
    );
    assert_eq!(
        bodies[2]["metadata"]["ownerReferences"][0]["uid"],
        "existing-uid"
    );
}

#[tokio::test]
async fn a_pod_create_rejected_by_quota_is_a_capacity_error() {
    let Provisioned {
        result, requests, ..
    } = provision_with_pod_create_answered(
        (
            403,
            serde_json::json!({
                "kind": "Status", "apiVersion": "v1", "status": "Failure", "code": 403,
                "reason": "Forbidden", "message": "pods is forbidden: exceeded quota: sandboxes",
            }),
        ),
        vec![],
    )
    .await;
    assert!(
        matches!(result, Err(ProviderError::Capacity(_))),
        "{result:?}"
    );
    assert_eq!(requests.len(), 1, "no policy is applied without a pod");
}

#[tokio::test]
async fn other_pod_create_rejections_are_backend_errors() {
    for (code, reason) in [(422, "Invalid"), (403, "Forbidden"), (409, "Conflict")] {
        let Provisioned {
            result, requests, ..
        } = provision_with_pod_create_answered((code, status_body(code, reason)), vec![]).await;
        assert!(
            matches!(result, Err(ProviderError::Backend(_))),
            "{code}: {result:?}"
        );
        assert_eq!(requests.len(), 1);
    }
}

/// Cluster targets that are temporarily unresolvable.
struct UnavailableTargets;

#[async_trait::async_trait]
impl ClusterTargetsProvider for UnavailableTargets {
    async fn get_targets(
        &self,
        _project_key: Option<&str>,
    ) -> Result<Vec<ResolvedClusterTarget>, ProviderError> {
        crate::test_support::unavailable_scheduler_targets()
    }
}

#[tokio::test]
async fn provision_with_unavailable_targets_creates_neither_pod_nor_policy() {
    let (provider, mut handle) =
        mock_provider_with_targets(test_config(), Arc::new(UnavailableTargets));
    let spec = df1_run_spec(None);
    let call = tokio::spawn(async move { provider.provision(&spec).await });

    // `None`: the provider (and its client) is gone without sending a request.
    let request = handle
        .next_request()
        .await
        .map(|(req, _)| format!("{} {}", req.method(), req.uri()));
    assert_eq!(request, None, "no Pod or NetworkPolicy request expected");
    let result = call.await.unwrap();
    assert!(
        matches!(&result, Err(ProviderError::Unavailable(m)) if m.contains("nativelink/scheduler")),
        "{result:?}"
    );
}

/// Tears down a run against a mock API: the list is answered with `listed` pods, a following
/// DELETE with `delete_status`. Returns the result, the list query and the DELETE path (if any).
async fn teardown_answered(
    listed: &[&str],
    delete_status: u16,
) -> (Result<(), ProviderError>, String, Option<String>) {
    let run_id = RunId::from_uuid(Uuid::from_u128(7));
    let (provider, mut handle) = mock_provider(test_config());
    let call = tokio::spawn(async move { provider.teardown(run_id).await });

    let (req, send) = handle.next_request().await.expect("pod list");
    let query = req.uri().query().unwrap_or_default().to_string();
    let items: Vec<Pod> = listed
        .iter()
        .map(|name| Pod {
            metadata: ObjectMeta {
                name: Some((*name).to_string()),
                ..pod_for(run_id, "Running", None).metadata
            },
            ..Default::default()
        })
        .collect();
    let body = serde_json::json!({
        "apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": items,
    });
    send_json(send, 200, &body);

    let mut deleted = None;
    if !listed.is_empty() {
        let (req, send) = handle.next_request().await.expect("pod DELETE");
        assert_eq!(req.method(), http::Method::DELETE);
        assert!(
            req.uri().path().ends_with(&format!("/pods/{}", listed[0])),
            "expected a pod DELETE, got {}",
            req.uri()
        );
        deleted = Some(req.uri().path().to_string());
        let body = if delete_status == 200 {
            serde_json::json!({ "apiVersion": "v1", "kind": "Pod", "metadata": { "name": listed[0] } })
        } else {
            status_body(delete_status, "NotFound")
        };
        send_json(send, delete_status, &body);
    }
    // Teardown must be done now: any further request fails the test instead of hanging it.
    // `None` means the provider (and its client) is gone without another request.
    let extra = handle
        .next_request()
        .await
        .map(|(req, _)| format!("{} {}", req.method(), req.uri()));
    assert!(
        extra.is_none(),
        "unexpected request after teardown: {extra:?}"
    );
    let result = call.await.unwrap();
    (result, query, deleted)
}

#[tokio::test]
async fn teardown_deletes_the_pod_of_the_run_selected_by_instance_and_run_id() {
    let (result, query, deleted) = teardown_answered(&["run-df-1-1"], 200).await;
    result.unwrap();
    assert!(
        deleted.is_some_and(|path| path.ends_with("/pods/run-df-1-1")),
        "no DELETE of the pod"
    );
    let run_id = RunId::from_uuid(Uuid::from_u128(7));
    let selector =
        format!("dark-factory.io%2Finstance%3Dtest-instance%2Cdark-factory.io%2Frun-id%3D{run_id}");
    assert!(query.contains(&selector), "{query}");
}

#[tokio::test]
async fn teardown_succeeds_when_the_pod_vanishes_before_the_delete() {
    let (result, _, deleted) = teardown_answered(&["run-df-1-1"], 404).await;
    result.unwrap();
    assert!(deleted.is_some());
}

#[tokio::test]
async fn teardown_without_a_pod_succeeds_and_deletes_nothing() {
    let (result, _, deleted) = teardown_answered(&[], 200).await;
    result.unwrap();
    assert_eq!(deleted, None);
}
