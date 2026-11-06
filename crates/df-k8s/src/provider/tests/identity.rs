//! Peer identity: SPIFFE SAN parsing and resolution through the pod store.

use super::*;
use uuid::Uuid;

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

/// A store holding one pod `run-df-1-1` with UID `pod-uid-1` and IP 10.0.3.7; `labels` are its labels.
fn store_with_pod(labels: Option<BTreeMap<String, String>>) -> Store<Pod> {
    let (pod_store, mut pod_writer) = reflector::store();
    let pod = Pod {
        metadata: ObjectMeta {
            name: Some("run-df-1-1".to_string()),
            namespace: Some(SANDBOX_NS.to_string()),
            uid: Some("pod-uid-1".to_string()),
            labels,
            ..Default::default()
        },
        status: Some(PodStatus {
            pod_ip: Some("10.0.3.7".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    pod_writer.apply_watcher_event(&watcher::Event::Apply(pod));
    pod_store
}

fn labels_of(instance: Option<&str>, run_id: Option<&str>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    if let Some(instance) = instance {
        labels.insert("dark-factory.io/instance".to_string(), instance.to_string());
    }
    if let Some(run_id) = run_id {
        labels.insert("dark-factory.io/run-id".to_string(), run_id.to_string());
    }
    labels
}

fn peer(uid: &str, ip: [u8; 4]) -> PeerIdentity {
    PeerIdentity {
        uri_san: format!("spiffe://dark-factory/ns/{SANDBOX_NS}/pod/run-df-1-1/{uid}"),
        peer_ip: IpAddr::V4(Ipv4Addr::from(ip)),
    }
}

#[test]
fn identity_resolves_only_for_matching_uid_ip_and_instance() {
    let run_id = RunId::from_uuid(Uuid::from_u128(42));
    let store = store_with_pod(Some(labels_of(
        Some("test-instance"),
        Some(&run_id.to_string()),
    )));
    let resolve = |peer: &PeerIdentity, instance: &str| {
        resolve_identity_from_pod_store(peer, SANDBOX_NS, instance, &store)
    };

    // Valid peer
    assert_eq!(
        resolve(&peer("pod-uid-1", [10, 0, 3, 7]), "test-instance"),
        Some(run_id)
    );
    // Peer IP mismatch
    assert_eq!(
        resolve(&peer("pod-uid-1", [10, 0, 3, 99]), "test-instance"),
        None
    );
    // UID mismatch after pod recreation
    assert_eq!(
        resolve(&peer("old-uid", [10, 0, 3, 7]), "test-instance"),
        None
    );
    // Malformed SAN
    let malformed = PeerIdentity {
        uri_san: "spiffe://other/bad".to_string(),
        peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
    };
    assert_eq!(resolve(&malformed, "test-instance"), None);
    // Matching pod, UID and IP, but a SAN for another namespace
    let other_namespace = PeerIdentity {
        uri_san: "spiffe://dark-factory/ns/other-ns/pod/run-df-1-1/pod-uid-1".to_string(),
        peer_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 3, 7)),
    };
    assert_eq!(resolve(&other_namespace, "test-instance"), None);
    // Pod not created by this instance
    assert_eq!(
        resolve(&peer("pod-uid-1", [10, 0, 3, 7]), "other-instance"),
        None
    );
}

#[test]
fn pod_without_instance_label_or_valid_run_id_resolves_to_nothing() {
    let run_id = RunId::from_uuid(Uuid::from_u128(42)).to_string();
    for labels in [
        None,
        Some(labels_of(None, Some(&run_id))),
        Some(labels_of(Some("test-instance"), None)),
        Some(labels_of(Some("test-instance"), Some("not-a-run-id"))),
    ] {
        let store = store_with_pod(labels.clone());
        assert_eq!(
            resolve_identity_from_pod_store(
                &peer("pod-uid-1", [10, 0, 3, 7]),
                SANDBOX_NS,
                "test-instance",
                &store
            ),
            None,
            "{labels:?}"
        );
    }
}
