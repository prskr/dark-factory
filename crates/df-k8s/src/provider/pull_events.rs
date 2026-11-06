//! Image pull failures reported as core v1 Events.
//!
//! An image volume (the harness) that cannot be pulled leaves the pod `Pending`. On containerd
//! the container status reports `ErrImagePull` as well, which the pod watcher already maps. This
//! module is defense in depth for runtimes that report the failure only as an Event.

use df_core::seams::SandboxEvent;
use k8s_openapi::api::core::v1::{Event, Pod};

use super::run_id_from_metadata;

/// Message prefix of the kubelet event for a failed pull; the image ref follows in quotes.
const PULL_FAILED_PREFIX: &str = "Failed to pull image \"";

/// Maps a kubelet image pull event of a sandbox pod to [`SandboxEvent::Gone`].
///
/// `pod` is the pod the event is about, looked up in the pod store. Only events with reason
/// `Failed` and a message `Failed to pull image "<ref>": <message>` match; the reason is then
/// `image pull failed: <ref>: <message>`, where `<message>` is the runtime's error (for example
/// `rpc error: code = NotFound desc = ...`). The follow-up events `Error: ErrImagePull` and
/// `Error: ImagePullBackOff` name no image and are ignored.
///
/// Returns `None` for other events, for a pod that is not in the store or no longer `Pending`,
/// and for an event of an earlier pod with the same name (the UIDs differ).
pub fn map_image_pull_event(event: &Event, pod: Option<&Pod>) -> Option<SandboxEvent> {
    if event.reason.as_deref() != Some("Failed") {
        return None;
    }
    let (image, error) = parse_pull_failed(event.message.as_deref()?)?;

    let pod = pod?;
    if let (Some(event_uid), Some(pod_uid)) = (&event.involved_object.uid, &pod.metadata.uid)
        && event_uid != pod_uid
    {
        return None;
    }
    if pod.status.as_ref()?.phase.as_deref() != Some("Pending") {
        return None;
    }

    let run_id = run_id_from_metadata(&pod.metadata)?;
    Some(SandboxEvent::Gone {
        run_id,
        reason: format!("image pull failed: {image}: {error}"),
    })
}

/// Splits `Failed to pull image "<ref>": <message>` into the ref and the message.
fn parse_pull_failed(message: &str) -> Option<(&str, &str)> {
    let (image, rest) = message.strip_prefix(PULL_FAILED_PREFIX)?.split_once('"')?;
    if image.is_empty() {
        return None;
    }
    let error = rest.strip_prefix(": ").unwrap_or(rest).trim();
    Some((image, error))
}
