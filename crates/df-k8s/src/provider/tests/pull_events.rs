//! Image pull failures reported as core v1 Events, on the recorded kind fixtures.

use super::*;
use k8s_openapi::api::core::v1::Event;

const HARNESS_IMAGE: &str = "ghcr.io/prskr/dark-factory/claude-harness:does-not-exist";

/// The events recorded for a pod whose harness image volume could not be pulled.
fn recorded_events() -> Vec<Event> {
    serde_json::from_str(include_str!(
        "../fixtures/image_volume_pull_failed_events.json"
    ))
    .unwrap()
}

/// The recorded pod, labelled with `run_id` as the provider labels its pods.
fn recorded_pod(run_id: RunId) -> Pod {
    let mut pod: Pod = serde_json::from_str(include_str!(
        "../fixtures/image_volume_pull_failed_pod.json"
    ))
    .unwrap();
    pod.metadata.labels = Some(BTreeMap::from([(
        "dark-factory.io/run-id".to_string(),
        run_id.to_string(),
    )]));
    pod
}

/// The recorded event with `reason` whose message starts with `message_prefix`.
fn recorded_event(reason: &str, message_prefix: &str) -> Event {
    recorded_events()
        .into_iter()
        .find(|event| {
            event.reason.as_deref() == Some(reason)
                && event
                    .message
                    .as_deref()
                    .is_some_and(|message| message.starts_with(message_prefix))
        })
        .unwrap()
}

fn pull_failed_event() -> Event {
    recorded_event("Failed", "Failed to pull image")
}

#[test]
fn a_failed_pull_event_of_a_pending_pod_is_gone_and_names_the_image() {
    let run_id = RunId::new();
    let event = pull_failed_event();

    let mapped = map_image_pull_event(&event, Some(&recorded_pod(run_id)));

    let Some(SandboxEvent::Gone {
        run_id: gone,
        reason,
    }) = mapped
    else {
        panic!("expected Gone, got {mapped:?}");
    };
    assert_eq!(gone, run_id);
    assert!(
        reason.starts_with(&format!("image pull failed: {HARNESS_IMAGE}: rpc error:")),
        "{reason}"
    );
}

#[test]
fn a_failed_pull_of_the_project_image_names_that_image() {
    let run_id = RunId::new();
    let mut event = pull_failed_event();
    event.message = event
        .message
        .map(|message| message.replace(HARNESS_IMAGE, "registry.example.org/project:missing"));

    let mapped = map_image_pull_event(&event, Some(&recorded_pod(run_id)));

    assert!(
        matches!(&mapped, Some(SandboxEvent::Gone { reason, .. })
            if reason.starts_with("image pull failed: registry.example.org/project:missing: ")),
        "{mapped:?}"
    );
}

#[test]
fn unrelated_events_of_the_pod_are_ignored() {
    let pod = recorded_pod(RunId::new());
    for event in [
        recorded_event("Scheduled", ""),
        recorded_event("Pulling", ""),
        recorded_event("Failed", "Error: ErrImagePull"),
        recorded_event("Failed", "Error: ImagePullBackOff"),
    ] {
        assert_eq!(map_image_pull_event(&event, Some(&pod)), None, "{event:?}");
    }
}

#[test]
fn a_failed_pull_event_of_an_unknown_pod_is_ignored() {
    assert_eq!(map_image_pull_event(&pull_failed_event(), None), None);
}

#[test]
fn a_failed_pull_event_of_a_running_pod_is_ignored() {
    let mut pod = recorded_pod(RunId::new());
    if let Some(status) = pod.status.as_mut() {
        status.phase = Some("Running".to_string());
    }

    assert_eq!(map_image_pull_event(&pull_failed_event(), Some(&pod)), None);
}

#[test]
fn a_failed_pull_event_of_an_earlier_pod_with_the_same_name_is_ignored() {
    let mut pod = recorded_pod(RunId::new());
    pod.metadata.uid = Some("uid-of-the-recreated-pod".to_string());

    assert_eq!(map_image_pull_event(&pull_failed_event(), Some(&pod)), None);
}

#[test]
fn tracker_reports_one_gone_when_status_and_event_report_the_same_pull_failure() {
    let run_id = RunId::new();
    let pod = recorded_pod(run_id);
    let event = pull_failed_event();

    // Container status first, then the Event.
    let mut tracker = EventTracker::default();
    let from_status = tracker.pod_event(watcher::Event::Apply(pod.clone()));
    let from_event = tracker.image_pull_event(&event, Some(&pod));
    assert!(
        matches!(from_status.as_slice(), [SandboxEvent::Gone { run_id: gone, .. }] if *gone == run_id),
        "{from_status:?}"
    );
    assert_eq!(from_event, None);

    // The Event first, then the container status.
    let mut tracker = EventTracker::default();
    let from_event = tracker.image_pull_event(&event, Some(&pod));
    let from_status = tracker.pod_event(watcher::Event::Apply(pod.clone()));
    assert!(
        matches!(&from_event, Some(SandboxEvent::Gone { run_id: gone, .. }) if *gone == run_id),
        "{from_event:?}"
    );
    assert!(from_status.is_empty(), "{from_status:?}");
}

#[tokio::test]
async fn event_loop_reports_a_pull_failure_event_of_a_stored_pod_once() {
    let run_id = RunId::new();
    // A pending pod whose status does not report the failed pull (other runtimes).
    let mut pod = recorded_pod(run_id);
    if let Some(status) = pod.status.as_mut() {
        status.container_statuses = None;
    }
    let namespace = pod.metadata.namespace.clone().unwrap();
    let (store, mut writer) = reflector::store();
    writer.apply_watcher_event(&watcher::Event::Apply(pod));
    let event = pull_failed_event();
    let k8s_events = futures::stream::iter([Ok(event.clone()), Ok(event)]);

    let emitted = std::sync::Mutex::new(Vec::new());
    forward_events(
        futures::stream::empty(),
        k8s_events,
        &store,
        &namespace,
        |event| emitted.lock().unwrap().push(event),
    )
    .await;

    let emitted = emitted.into_inner().unwrap();
    assert!(
        matches!(emitted.as_slice(), [SandboxEvent::Gone { run_id: gone, reason }]
            if *gone == run_id && reason.starts_with(&format!("image pull failed: {HARNESS_IMAGE}: "))),
        "{emitted:?}"
    );
}

#[tokio::test]
async fn event_loop_ignores_a_pull_failure_event_of_a_pod_not_in_the_store() {
    let (store, _writer) = reflector::store::<Pod>();
    let k8s_events = futures::stream::iter([Ok(pull_failed_event())]);

    let emitted = std::sync::Mutex::new(Vec::new());
    forward_events(
        futures::stream::empty(),
        k8s_events,
        &store,
        SANDBOX_NS,
        |event| emitted.lock().unwrap().push(event),
    )
    .await;

    assert!(emitted.into_inner().unwrap().is_empty());
}

#[test]
fn the_recorded_pull_failed_pod_is_gone_and_names_the_image() {
    let run_id = RunId::new();

    let mapped = map_pod_event(&recorded_pod(run_id));

    assert!(
        matches!(&mapped, Some(SandboxEvent::Gone { run_id: gone, reason })
            if *gone == run_id && reason.contains(HARNESS_IMAGE)),
        "{mapped:?}"
    );
}
