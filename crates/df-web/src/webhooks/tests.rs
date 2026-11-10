//! Tests of the webhook route: status codes, empty bodies and the request counter.

use std::sync::Mutex;

use axum::body::Body;
use axum::http::Request;
use df_agent_channel::metrics::probe::Probe;
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

use super::*;

/// Plugin, headers and body of a delivery the receiver got.
type Received = (String, Vec<(String, String)>, Vec<u8>);

/// Loads `forgejo` with an endpoint and `github` without one; answers every delivery with
/// `result`.
struct FakeReceiver {
    result: Result<(), WebhookRejection>,
    received: Mutex<Vec<Received>>,
}

#[async_trait]
impl WebhookReceiver for FakeReceiver {
    fn is_loaded(&self, plugin: &str) -> bool {
        matches!(plugin, "forgejo" | "github")
    }

    /// `github` is loaded but has no endpoint: no resolved secret.
    fn setup(&self, plugin: &str) -> WebhookSetup {
        match plugin {
            "forgejo" => WebhookSetup::Ready,
            "github" => WebhookSetup::NoSecret,
            _ => WebhookSetup::NotLoaded,
        }
    }

    fn secret(&self, _plugin: &str) -> Option<SecretString> {
        None
    }

    async fn receive(
        &self,
        plugin: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<(), WebhookRejection> {
        self.received
            .lock()
            .unwrap()
            .push((plugin.to_string(), headers.to_vec(), body.to_vec()));
        self.result
    }
}

struct Harness {
    receiver: Arc<FakeReceiver>,
    probe: Probe,
    router: Router,
}

fn harness(result: Result<(), WebhookRejection>) -> Harness {
    let receiver = Arc::new(FakeReceiver {
        result,
        received: Mutex::default(),
    });
    let probe = Probe::new();
    let router = routes(Webhooks::new(receiver.clone(), &probe.meter()));
    Harness {
        receiver,
        probe,
        router,
    }
}

fn delivery(plugin: &str, body: impl Into<Body>) -> Request<Body> {
    Request::post(format!("/webhooks/{plugin}"))
        .header("x-forgejo-signature", "abc")
        .body(body.into())
        .unwrap()
}

impl Harness {
    /// Status and body of `request`.
    async fn send(&self, request: Request<Body>) -> (StatusCode, Bytes) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        (
            status,
            response.into_body().collect().await.unwrap().to_bytes(),
        )
    }

    fn requests(&self, plugin: &str, outcome: &str) -> f64 {
        self.probe.total(
            "df.webhook.requests",
            &[("plugin", plugin), ("outcome", outcome)],
        )
    }
}

#[tokio::test]
async fn webhook_delivery_is_accepted_with_an_empty_body_and_counted() {
    let h = harness(Ok(()));

    let (status, body) = h.send(delivery("forgejo", "{}")).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(body.is_empty());
    assert_eq!(h.requests("forgejo", "accepted"), 1.0);
    let received = h.receiver.received.lock().unwrap();
    let (plugin, headers, body) = &received[0];
    assert_eq!(plugin, "forgejo");
    assert!(headers.contains(&("x-forgejo-signature".into(), "abc".into())));
    assert_eq!(body, b"{}");
}

#[tokio::test]
async fn webhook_for_a_plugin_without_endpoint_is_not_found_before_its_size() {
    let h = harness(Ok(()));

    let (status, body) = h
        .send(delivery("gitlab", vec![b'x'; MAX_BODY_BYTES + 1]))
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_empty());
    assert_eq!(h.requests("unknown", "not_found"), 1.0);
    assert!(
        h.probe
            .values("df.webhook.requests", "plugin")
            .contains("unknown")
    );
    assert!(
        !h.probe
            .values("df.webhook.requests", "plugin")
            .contains("gitlab")
    );
    assert!(h.receiver.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn webhook_for_a_loaded_plugin_without_endpoint_is_counted_under_its_name() {
    let h = harness(Ok(()));

    let (status, _) = h.send(delivery("github", "{}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(h.requests("github", "not_found"), 1.0);
    assert!(h.receiver.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn webhook_body_over_one_mib_is_too_large() {
    let h = harness(Ok(()));

    let (status, body) = h
        .send(delivery("forgejo", vec![b'x'; MAX_BODY_BYTES + 1]))
        .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body.is_empty());
    assert_eq!(h.requests("forgejo", "too_large"), 1.0);
    assert!(h.receiver.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn webhook_body_of_exactly_one_mib_is_passed_on() {
    let h = harness(Ok(()));

    let (status, _) = h
        .send(delivery("forgejo", vec![b'x'; MAX_BODY_BYTES]))
        .await;

    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn webhook_rejections_map_to_their_status_and_outcome() {
    let cases = [
        (
            WebhookRejection::Unauthorized,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            WebhookRejection::Invalid,
            StatusCode::BAD_REQUEST,
            "invalid",
        ),
        (
            WebhookRejection::Failed,
            StatusCode::INTERNAL_SERVER_ERROR,
            "error",
        ),
        (
            WebhookRejection::NotFound,
            StatusCode::NOT_FOUND,
            "not_found",
        ),
    ];
    for (rejection, expected, outcome) in cases {
        let h = harness(Err(rejection));

        let (status, body) = h.send(delivery("forgejo", "{}")).await;

        assert_eq!(status, expected, "{rejection:?}");
        assert!(body.is_empty(), "{rejection:?}");
        assert_eq!(h.requests("forgejo", outcome), 1.0, "{rejection:?}");
    }
}

#[tokio::test]
async fn webhook_route_accepts_only_post() {
    let h = harness(Ok(()));

    let (status, _) = h
        .send(
            Request::get("/webhooks/forgejo")
                .body(Body::empty())
                .unwrap(),
        )
        .await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}
