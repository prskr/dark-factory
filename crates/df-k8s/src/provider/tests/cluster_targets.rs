//! Cluster targets: resolving allowlisted Services and caching the last good resolution.

use super::*;

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

fn rule(ns: &str, svc: &str) -> ClusterServiceRef {
    ClusterServiceRef {
        namespace: ns.to_string(),
        service: svc.to_string(),
        ports: vec![80],
        allow_dns: false,
    }
}

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
        status_body(status, "boom")
    };
    send_json(send, status, &body);
}

/// Runs `resolve(scope, rules)` and answers its Service lookups with `statuses`.
async fn resolve_answering(
    resolver: &Arc<ClusterTargetResolver>,
    handle: &mut MockHandle,
    scope: Option<&'static str>,
    rules: &[ClusterServiceRef],
    statuses: &[u16],
) -> Vec<ResolvedClusterTarget> {
    resolution_answering(resolver, handle, scope, rules, statuses)
        .await
        .targets
}

/// Like [`resolve_answering`], but returns the whole resolution.
async fn resolution_answering(
    resolver: &Arc<ClusterTargetResolver>,
    handle: &mut MockHandle,
    scope: Option<&'static str>,
    rules: &[ClusterServiceRef],
    statuses: &[u16],
) -> ClusterTargetResolution {
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
async fn a_transient_error_without_a_cached_resolution_reports_the_rule_unavailable() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "broken"), rule("b", "ok")];
    let resolution = resolution_answering(&resolver, &mut handle, None, &rules, &[500, 200]).await;
    assert_eq!(resolution.targets.len(), 1);
    assert_eq!(resolution.targets[0].service, "ok");
    assert_eq!(resolution.unavailable, vec![rule("a", "broken")]);
}

#[tokio::test]
async fn a_transient_error_with_a_cached_resolution_reports_nothing_unavailable() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    resolution_answering(&resolver, &mut handle, None, &rules, &[200]).await;
    let resolution = resolution_answering(&resolver, &mut handle, None, &rules, &[500]).await;
    assert_eq!(resolution.targets.len(), 1);
    assert!(resolution.unavailable.is_empty());
}

#[tokio::test]
async fn a_permanent_error_skips_the_rule_without_reporting_it_unavailable() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    let resolution = resolution_answering(&resolver, &mut handle, None, &rules, &[404]).await;
    assert!(resolution.targets.is_empty());
    assert!(resolution.unavailable.is_empty());
}

#[test]
fn a_resolution_with_unavailable_rules_is_an_unavailable_error_naming_each_rule() {
    let resolution = ClusterTargetResolution {
        targets: Vec::new(),
        unavailable: vec![rule("nativelink", "scheduler"), rule("forgejo", "ssh")],
    };
    let err = resolution
        .into_targets()
        .expect_err("rules are unavailable");
    assert!(
        matches!(&err, ProviderError::Unavailable(m)
            if m.contains("nativelink/scheduler") && m.contains("forgejo/ssh")),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_resolution_without_unavailable_rules_yields_its_targets() {
    let (resolver, mut handle) = new_resolver();
    let rules = vec![rule("a", "svc")];
    let resolution = resolution_answering(&resolver, &mut handle, None, &rules, &[200]).await;
    let targets = resolution.into_targets().expect("nothing unavailable");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].service, "svc");
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
    let kept = task.await.unwrap().targets;
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
    assert!(task.await.unwrap().targets.is_empty());

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
