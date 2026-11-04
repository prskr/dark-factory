//! The inference endpoint against a fake upstream, in process (no containers).

use std::net::IpAddr;
use std::sync::Arc;

use df_agent_channel::metrics::probe::Probe;
use df_proto::gateway::v1 as pb;
use http_body_util::BodyExt;
use hyper::{Method, StatusCode};

use super::Inference;
use super::testing::{Collector, FakeUpstream, Front, Reply};
use crate::metrics::Metrics;
use crate::policy::{Policy, Sandbox};
use crate::upstream::UpstreamConnector;

const POD: &str = "127.0.0.1";
const CREDENTIAL: &str = "sk-provider-secret";

fn route(upstreams: &[(pb::WireApi, String)], scheme: pb::CredentialScheme) -> pb::InferenceRoute {
    pb::InferenceRoute {
        wire_api: upstreams[0].0.into(),
        upstreams: upstreams
            .iter()
            .map(|(api, url)| pb::ApiUpstream {
                wire_api: (*api).into(),
                upstream_url: url.clone(),
            })
            .collect(),
        credential_ref: "cred".into(),
        scheme: scheme.into(),
        ..pb::InferenceRoute::default()
    }
}

fn policy(route: Option<pb::InferenceRoute>, credential: bool) -> Arc<Policy> {
    let ip: IpAddr = POD.parse().unwrap();
    let sandbox = Sandbox {
        run_id: "run-1".into(),
        task_key: "DF-1".into(),
        run_number: 1,
        project_key: "DF".into(),
        persona_id: "p".into(),
        hosts: Vec::new(),
    };
    let credentials = credential.then(|| {
        (
            "cred".to_string(),
            pb::ProviderCredential {
                value: CREDENTIAL.into(),
            },
        )
    });
    Arc::new(
        Policy::new(1, [(ip, sandbox)]).with_inference(route.map(|route| (ip, route)), credentials),
    )
}

struct Setup {
    front: Front,
    usage: Arc<Collector>,
    probe: Probe,
}

async fn setup(policy: Option<Arc<Policy>>) -> Setup {
    let usage = Arc::new(Collector::default());
    let probe = Probe::new();
    let inference = Arc::new(Inference::new(
        UpstreamConnector::new().unwrap().allowing_plain_http(true),
        usage.clone(),
        Arc::new(Metrics::new(&probe.meter())),
    ));
    let front = Front::start(inference, policy, POD.parse().unwrap()).await;
    Setup {
        front,
        usage,
        probe,
    }
}

async fn anthropic(upstream: &FakeUpstream, scheme: pb::CredentialScheme) -> (Setup, Arc<Policy>) {
    let policy = policy(
        Some(route(
            &[(pb::WireApi::AnthropicMessages, upstream.url())],
            scheme,
        )),
        true,
    );
    (setup(Some(policy.clone())).await, policy)
}

const STREAM_REQUEST: &str = r#"{"model":"claude","stream":true,"messages":[]}"#;

fn header(request: &super::testing::Seen, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .map(|value| value.to_str().unwrap().to_string())
}

#[tokio::test]
async fn an_anthropic_request_reaches_the_upstream_path_with_its_query_string() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    let (status, _, body) = setup
        .front
        .send(Method::POST, "/inference/v1/messages?beta=true", &[], "{}")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("hello"));
    let seen = upstream.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, Method::POST);
    assert_eq!(seen[0].path_and_query, "/v1/messages?beta=true");
}

#[tokio::test]
async fn an_upstream_base_path_is_kept_in_front_of_the_request_path() {
    let upstream = FakeUpstream::start().await;
    let policy = policy(
        Some(route(
            &[(
                pb::WireApi::AnthropicMessages,
                format!("{}/inference/go/anthropic/", upstream.url()),
            )],
            pb::CredentialScheme::Bearer,
        )),
        true,
    );
    let setup = setup(Some(policy)).await;

    setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;

    assert_eq!(
        upstream.requests()[0].path_and_query,
        "/inference/go/anthropic/v1/messages"
    );
}

#[tokio::test]
async fn an_unknown_pod_gets_403_and_no_upstream_is_contacted() {
    let upstream = FakeUpstream::start().await;
    let inference = Arc::new(Inference::new(
        UpstreamConnector::new().unwrap().allowing_plain_http(true),
        Arc::new(Collector::default()),
        Arc::default(),
    ));
    let policy = anthropic(&upstream, pb::CredentialScheme::XApiKey).await.1;
    let front = Front::start(inference, Some(policy), "10.9.9.9".parse().unwrap()).await;

    let (status, ..) = front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(upstream.requests().is_empty());
}

#[tokio::test]
async fn before_the_first_snapshot_the_answer_is_503() {
    let setup = setup(None).await;
    let (status, ..) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn a_run_without_a_model_provider_gets_403() {
    let setup = setup(Some(policy(None, true))).await;
    let (status, ..) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_api_the_provider_does_not_offer_and_paths_outside_the_list_get_404() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    for (method, path) in [
        (Method::POST, "/inference/v1/chat/completions"),
        (Method::POST, "/inference/v1/messages/batches"),
        (Method::GET, "/inference/v1/messages"),
        (Method::POST, "/inference"),
    ] {
        let (status, ..) = setup.front.send(method.clone(), path, &[], "{}").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    assert!(upstream.requests().is_empty());
}

#[tokio::test]
async fn models_of_a_provider_with_two_apis_go_to_the_upstream_of_the_chosen_api() {
    let anthropic_upstream = FakeUpstream::start().await;
    let openai_upstream = FakeUpstream::start().await;
    let policy = policy(
        Some(route(
            &[
                (pb::WireApi::AnthropicMessages, anthropic_upstream.url()),
                (pb::WireApi::OpenaiChat, openai_upstream.url()),
            ],
            pb::CredentialScheme::Bearer,
        )),
        true,
    );
    let setup = setup(Some(policy)).await;

    let (status, ..) = setup
        .front
        .send(Method::GET, "/inference/v1/models", &[], "")
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(anthropic_upstream.requests().len(), 1);
    assert!(openai_upstream.requests().is_empty());
    assert!(setup.usage.records().is_empty(), "models carry no usage");
}

#[tokio::test]
async fn the_placeholder_key_is_replaced_and_harness_headers_pass_through() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    setup
        .front
        .send(
            Method::POST,
            "/inference/v1/messages",
            &[
                ("x-api-key", "df-placeholder"),
                ("anthropic-beta", "oauth-2025-04-20,claude-code-20250219"),
            ],
            "{}",
        )
        .await;

    let seen = &upstream.requests()[0];
    assert_eq!(header(seen, "x-api-key").as_deref(), Some(CREDENTIAL));
    assert_eq!(
        header(seen, "anthropic-beta").as_deref(),
        Some("oauth-2025-04-20,claude-code-20250219")
    );
    assert!(!format!("{:?}", seen.headers).contains("df-placeholder"));
}

#[tokio::test]
async fn claude_subscription_sends_a_bearer_token_and_no_api_key() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::ClaudeSubscription).await;

    setup
        .front
        .send(
            Method::POST,
            "/inference/v1/messages",
            &[("authorization", "Bearer df-placeholder")],
            "{}",
        )
        .await;

    let seen = &upstream.requests()[0];
    assert_eq!(
        header(seen, "authorization").as_deref(),
        Some(format!("Bearer {CREDENTIAL}").as_str())
    );
    assert!(seen.headers.get("x-api-key").is_none());
}

#[tokio::test]
async fn provider_headers_replace_the_clients_and_a_custom_credential_header_is_used() {
    let upstream = FakeUpstream::start().await;
    let mut route = route(
        &[(pb::WireApi::AnthropicMessages, upstream.url())],
        pb::CredentialScheme::Header,
    );
    route.credential_header = "api-key".into();
    route.headers = vec![pb::Header {
        name: "x-opencode-session".into(),
        value: "3f1c".into(),
    }];
    let setup = setup(Some(policy(Some(route), true))).await;

    setup
        .front
        .send(
            Method::POST,
            "/inference/v1/messages",
            &[("x-opencode-session", "abc")],
            "{}",
        )
        .await;

    let seen = &upstream.requests()[0];
    let sessions: Vec<_> = seen.headers.get_all("x-opencode-session").iter().collect();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0], "3f1c");
    assert_eq!(header(seen, "api-key").as_deref(), Some(CREDENTIAL));
}

#[tokio::test]
async fn a_credential_missing_from_the_snapshot_gives_502_without_contacting_the_upstream() {
    let upstream = FakeUpstream::start().await;
    let policy = policy(
        Some(route(
            &[(pb::WireApi::AnthropicMessages, upstream.url())],
            pb::CredentialScheme::XApiKey,
        )),
        false,
    );
    let setup = setup(Some(policy)).await;

    let (status, _, body) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(!body.contains(CREDENTIAL));
    assert!(upstream.requests().is_empty());
}

#[tokio::test]
async fn the_request_body_reaches_the_upstream_unchanged() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;
    let body = r#"{"model":"claude","messages":[{"role":"user","content":"hi"}]}"#;

    setup
        .front
        .send(
            Method::POST,
            "/inference/v1/messages",
            &[("content-type", "application/json")],
            body,
        )
        .await;

    assert_eq!(upstream.requests()[0].body, body.as_bytes());
}

#[tokio::test]
async fn a_streamed_event_arrives_before_the_upstream_response_ends() {
    let upstream = FakeUpstream::start().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    upstream.reply_with(Reply::Events(
        super::testing::ANTHROPIC_STREAM.to_vec(),
        Some(gate.clone()),
    ));
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    let response = setup
        .front
        .open(Method::POST, "/inference/v1/messages", &[], STREAM_REQUEST)
        .await;
    let mut body = response.into_body();
    let first = body.frame().await.unwrap().unwrap();

    assert!(
        std::str::from_utf8(first.data_ref().unwrap())
            .unwrap()
            .contains("message_start")
    );
    assert!(
        setup.usage.records().is_empty(),
        "the upstream has not ended"
    );
    gate.notify_one();
    while body.frame().await.is_some() {}
}

#[tokio::test]
async fn an_upstream_rate_limit_is_relayed_and_counted() {
    let upstream = FakeUpstream::start().await;
    upstream.reply_with(Reply::Fixed(
        StatusCode::TOO_MANY_REQUESTS,
        vec![("retry-after", "30")],
        r#"{"error":"slow down"}"#,
    ));
    let policy = policy(
        Some(route(
            &[(pb::WireApi::OpenaiChat, upstream.url())],
            pb::CredentialScheme::Bearer,
        )),
        true,
    );
    let setup = setup(Some(policy)).await;

    let (status, headers, _) = setup
        .front
        .send(Method::POST, "/inference/v1/chat/completions", &[], "{}")
        .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(headers["retry-after"], "30");
    let attrs = [("wire_api", "openai-chat"), ("status_class", "4xx")];
    assert_eq!(
        setup.probe.total("df.gateway.inference.requests", &attrs),
        1.0
    );
    assert_eq!(
        setup.probe.total(
            "df.gateway.inference.duration",
            &[("wire_api", "openai-chat")]
        ),
        1.0
    );
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert!(!records[0].usage_known);
    assert_eq!(records[0].upstream_status, 429);
}

/// Port 0 never has a listener: on Linux, connecting to it is refused at once, without binding
/// and releasing a port that another test could take.
#[tokio::test]
async fn a_provider_host_that_refuses_the_connection_gives_502_and_a_record_with_unknown_usage() {
    let policy = policy(
        Some(route(
            &[(pb::WireApi::AnthropicMessages, "http://127.0.0.1:0".into())],
            pb::CredentialScheme::XApiKey,
        )),
        true,
    );
    let setup = setup(Some(policy)).await;

    let (status, _, body) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body.contains("model provider unreachable"), "{body}");
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].upstream_status, records[0].usage_known),
        (0, false)
    );
}

#[tokio::test]
async fn a_provider_that_closes_the_connection_gives_502_and_a_record_with_unknown_usage() {
    let upstream = FakeUpstream::start().await;
    upstream.reply_with(Reply::Hangup);
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    let (status, ..) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], "{}")
        .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].upstream_status, records[0].usage_known),
        (0, false)
    );
    assert_eq!(
        setup.probe.total(
            "df.gateway.inference.requests",
            &[("status_class", "error")]
        ),
        1.0
    );
}

#[tokio::test]
async fn a_streamed_anthropic_message_is_reported_with_model_and_tokens() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    let (_, _, body) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], STREAM_REQUEST)
        .await;

    assert_eq!(
        body,
        super::testing::ANTHROPIC_STREAM.concat(),
        "relayed unchanged"
    );
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.run_id, "run-1");
    assert_eq!(record.model, "claude-sonnet-5-5");
    assert!(record.usage_known);
    assert_eq!(
        (
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens
        ),
        (120, 40, 15)
    );
    assert_eq!(record.wire_api(), pb::WireApi::AnthropicMessages);
    assert_eq!(record.upstream_status, 200);
    assert!(uuid::Uuid::parse_str(&record.request_id).is_ok());
}

#[tokio::test]
async fn a_chat_stream_without_usage_is_reported_as_unknown() {
    let upstream = FakeUpstream::start().await;
    let policy = policy(
        Some(route(
            &[(pb::WireApi::OpenaiChat, upstream.url())],
            pb::CredentialScheme::Bearer,
        )),
        true,
    );
    let setup = setup(Some(policy)).await;

    setup
        .front
        .send(
            Method::POST,
            "/inference/v1/chat/completions",
            &[],
            r#"{"stream":true}"#,
        )
        .await;

    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert!(!records[0].usage_known);
    assert_eq!(records[0].input_tokens, 0);
}

#[tokio::test]
async fn counting_tokens_reports_no_usage_record() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;

    let (status, ..) = setup
        .front
        .send(
            Method::POST,
            "/inference/v1/messages/count_tokens",
            &[],
            "{}",
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(setup.usage.records().is_empty());
}

#[tokio::test]
async fn a_harness_that_goes_away_mid_stream_still_gets_its_request_recorded() {
    let upstream = FakeUpstream::start().await;
    let gate = Arc::new(tokio::sync::Notify::new());
    upstream.reply_with(Reply::Events(
        super::testing::ANTHROPIC_STREAM.to_vec(),
        Some(gate),
    ));
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;
    let response = setup
        .front
        .open(Method::POST, "/inference/v1/messages", &[], STREAM_REQUEST)
        .await;
    let mut body = response.into_body();
    body.frame().await.unwrap().unwrap();

    drop(body);

    setup.usage.recorded(1).await;
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].usage_known, "message_start was seen");
}

/// Collects what the `tracing` macros emit.
#[derive(Clone, Default)]
struct LogCapture(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn no_log_line_contains_the_provider_credential() {
    let logs = LogCapture::default();
    let writer = logs.clone();
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish(),
    );
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;
    let down = Setup {
        front: Front::start(
            Arc::new(Inference::new(
                UpstreamConnector::new().unwrap().allowing_plain_http(true),
                Arc::new(Collector::default()),
                Arc::default(),
            )),
            Some(policy(
                Some(route(
                    &[(pb::WireApi::AnthropicMessages, "http://127.0.0.1:9".into())],
                    pb::CredentialScheme::XApiKey,
                )),
                true,
            )),
            POD.parse().unwrap(),
        )
        .await,
        usage: Arc::default(),
        probe: Probe::new(),
    };

    for front in [&setup.front, &down.front] {
        front
            .send(Method::POST, "/inference/v1/messages", &[], STREAM_REQUEST)
            .await;
        front
            .send(Method::POST, "/inference/v1/nope", &[], "{}")
            .await;
    }

    let captured = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(!captured.is_empty(), "the test must capture something");
    assert!(!captured.contains(CREDENTIAL), "{captured}");
}

#[tokio::test(start_paused = true)]
async fn an_upstream_that_sends_no_response_head_gets_504_after_the_head_timeout() {
    let upstream = FakeUpstream::start().await;
    upstream.reply_with(Reply::NoHead);
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;
    let started = tokio::time::Instant::now();

    let (status, _, body) = setup
        .front
        .send(Method::POST, "/inference/v1/messages", &[], STREAM_REQUEST)
        .await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert!(body.contains("did not answer in time"), "{body}");
    assert!(started.elapsed() >= super::RESPONSE_HEAD_TIMEOUT);
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].upstream_status, 0);
    assert!(!records[0].usage_known);
}

#[tokio::test(start_paused = true)]
async fn an_upstream_that_stalls_mid_stream_ends_the_response_after_the_idle_timeout() {
    let upstream = FakeUpstream::start().await;
    // The gate is never opened: the answer stops after its first chunk.
    upstream.reply_with(Reply::Events(
        super::testing::ANTHROPIC_STREAM.to_vec(),
        Some(Arc::new(tokio::sync::Notify::new())),
    ));
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;
    let started = tokio::time::Instant::now();
    let response = setup
        .front
        .open(Method::POST, "/inference/v1/messages", &[], STREAM_REQUEST)
        .await;
    let mut body = response.into_body();
    body.frame().await.unwrap().unwrap();

    let next = body.frame().await;

    assert!(matches!(next, Some(Err(_))), "{next:?}");
    assert!(started.elapsed() >= super::RESPONSE_IDLE_TIMEOUT);
    setup.usage.recorded(1).await;
    let records = setup.usage.records();
    assert_eq!(records[0].upstream_status, 200);
    assert!(records[0].usage_known, "message_start was seen");
}

#[tokio::test]
async fn a_custom_credential_header_may_be_api_key_or_authorization_with_a_prefix() {
    for (name, prefix, sent) in [
        ("api-key", "", CREDENTIAL.to_string()),
        ("Authorization", "Token ", format!("Token {CREDENTIAL}")),
    ] {
        let upstream = FakeUpstream::start().await;
        let mut route = route(
            &[(pb::WireApi::AnthropicMessages, upstream.url())],
            pb::CredentialScheme::Header,
        );
        route.credential_header = name.into();
        route.credential_prefix = prefix.into();
        let setup = setup(Some(policy(Some(route), true))).await;

        setup
            .front
            .send(
                Method::POST,
                "/inference/v1/messages",
                &[
                    ("api-key", "df-placeholder"),
                    ("authorization", "Bearer df-placeholder"),
                ],
                "{}",
            )
            .await;

        let seen = upstream.requests();
        assert_eq!(header(&seen[0], name), Some(sent), "{name}");
        let credentials = seen[0]
            .headers
            .iter()
            .filter(|(header, _)| ["api-key", "authorization"].contains(&header.as_str()))
            .count();
        assert_eq!(
            credentials, 1,
            "only the provider's credential header is sent"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_request_body_that_stops_sending_is_answered_408_with_a_record_of_unknown_usage() {
    let upstream = FakeUpstream::start().await;
    let (setup, _) = anthropic(&upstream, pb::CredentialScheme::XApiKey).await;
    let started = tokio::time::Instant::now();

    let response = setup
        .front
        .open_with_stalled_body(Method::POST, "/inference/v1/messages", "{\"model\":")
        .await;

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("request body timed out"));
    assert!(started.elapsed() >= super::UPLOAD_IDLE_TIMEOUT);
    let records = setup.usage.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        (records[0].upstream_status, records[0].usage_known),
        (0, false)
    );
}
