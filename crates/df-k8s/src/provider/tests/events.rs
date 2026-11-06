//! Sandbox events: mapping pod states, deduplication, relists and the event stream.

use super::*;
use k8s_openapi::api::core::v1::{ContainerState, ContainerStateWaiting, ContainerStatus};
use uuid::Uuid;

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

    // 3. Completed without a run report -> Gone
    pod.status = Some(PodStatus {
        phase: Some("Succeeded".to_string()),
        ..Default::default()
    });
    assert_eq!(
        map_pod_event(&pod),
        Some(SandboxEvent::Gone {
            run_id,
            reason: "pod completed".to_string(),
        })
    );

    // 4. Deletion timestamp -> Gone
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
    let (store, _writer) = reflector::store();
    forward_events(
        pods,
        futures::stream::empty(),
        &store,
        SANDBOX_NS,
        |event| {
            emitted.lock().unwrap().push(event);
        },
    )
    .await;
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

#[test]
fn relist_reports_never_running_pods_that_vanished() {
    let run_id = RunId::from_uuid(Uuid::from_u128(8));
    let mut tracker = EventTracker::default();
    // Pending pod: no event, but the pod is known.
    assert!(
        tracker
            .pod_event(watcher::Event::Apply(pod_for(run_id, "Pending", None)))
            .is_empty()
    );

    // The watch restarts and the relist returns nothing: no `Delete`, only the comparison.
    assert!(tracker.pod_event(watcher::Event::Init).is_empty());
    let from_pods = tracker.pod_event(watcher::Event::InitDone);

    assert_eq!(
        from_pods,
        vec![SandboxEvent::Gone {
            run_id,
            reason: "pod no longer present".to_string()
        }]
    );
}

#[tokio::test]
async fn tracker_emits_one_gone_when_the_pod_is_deleted() {
    let run_id = RunId::from_uuid(Uuid::from_u128(6));
    let mut tracker = EventTracker::default();
    let pod = pod_for(run_id, "Running", None);

    assert_eq!(
        tracker.pod_event(watcher::Event::Delete(pod.clone())),
        vec![SandboxEvent::Gone {
            run_id,
            reason: "pod deleted".to_string()
        }]
    );
    assert!(tracker.pod_event(watcher::Event::Delete(pod)).is_empty());
}

#[tokio::test]
async fn completed_pod_emits_one_gone() {
    let run_id = RunId::from_uuid(Uuid::from_u128(9));
    let events = run_loop(vec![
        watcher::Event::Apply(pod_for(run_id, "Succeeded", None)),
        watcher::Event::Delete(pod_for(run_id, "Succeeded", None)),
    ])
    .await;
    assert_eq!(
        events,
        vec![SandboxEvent::Gone {
            run_id,
            reason: "pod completed".to_string()
        }]
    );
}

#[tokio::test]
async fn events_stream_survives_lag_and_keeps_later_events() {
    use futures::StreamExt as _;
    let (provider, _handle) = mock_provider(test_config());
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
