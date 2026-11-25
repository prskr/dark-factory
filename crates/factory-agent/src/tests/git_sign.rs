//! The `git-sign` shim against a runtime socket, and the relay of sign requests to the control
//! plane (design D1 and D2 of `persona-commit-signing`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use df_proto::agent::v1::{AgentMessage, SignResponse, agent_message, sign_response};
use tokio::sync::mpsc;

use crate::git_sign;
use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};
use crate::sign_relay;

/// Answers every `git_sign` request with a fixed result and remembers the payloads.
struct FixedSigner {
    answer: Result<String, String>,
    payloads: std::sync::Mutex<Vec<Vec<u8>>>,
}

#[tonic::async_trait]
impl RuntimeHandler for FixedSigner {
    async fn handle_git_sign(&self, payload: &[u8]) -> Result<String, String> {
        self.payloads.lock().unwrap().push(payload.to_vec());
        self.answer.clone()
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    socket: PathBuf,
    signer: Arc<FixedSigner>,
    _server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn new(answer: Result<&str, &str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("runtime.sock");
        let signer = Arc::new(FixedSigner {
            answer: answer.map(str::to_string).map_err(str::to_string),
            payloads: std::sync::Mutex::default(),
        });
        let server = RuntimeSocketServer::bind(&socket, signer.clone())
            .unwrap()
            .spawn();
        Self {
            dir,
            socket,
            signer,
            _server: server,
        }
    }

    /// A data file like the buffer git writes.
    fn data_file(&self, content: &[u8]) -> PathBuf {
        let file = self.dir.path().join(".git_signing_buffer_tmp");
        std::fs::write(&file, content).unwrap();
        file
    }
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn sign_args(key: &str, data: &Path, literal: bool) -> Vec<OsString> {
    let mut args = os(&["-Y", "sign", "-n", "git", "-f", key]);
    if literal {
        args.push("-U".into());
    }
    args.push(data.into());
    args
}

#[tokio::test]
async fn git_sign_writes_the_signature_next_to_the_data_file() {
    let fixture = Fixture::new(Ok(
        "-----BEGIN SSH SIGNATURE-----\nabc\n-----END SSH SIGNATURE-----\n",
    ));
    let data = fixture.data_file(b"tree 1\n\nmsg\n");

    git_sign::run(&fixture.socket, &sign_args("/tmp/key", &data, false))
        .await
        .unwrap();

    let sig = std::fs::read_to_string(format!("{}.sig", data.display())).unwrap();
    assert!(sig.starts_with("-----BEGIN SSH SIGNATURE-----"));
    assert_eq!(
        *fixture.signer.payloads.lock().unwrap(),
        [b"tree 1\n\nmsg\n".to_vec()]
    );
}

#[tokio::test]
async fn git_sign_accepts_the_literal_key_flag() {
    let fixture = Fixture::new(Ok("sig"));
    let data = fixture.data_file(b"payload");

    git_sign::run(&fixture.socket, &sign_args("/tmp/key", &data, true))
        .await
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(format!("{}.sig", data.display())).unwrap(),
        "sig"
    );
}

#[tokio::test]
async fn git_sign_refuses_other_modes_and_namespaces_without_asking_the_runtime() {
    let fixture = Fixture::new(Ok("sig"));
    let data = fixture.data_file(b"payload");
    let file: OsString = data.clone().into();

    for (args, expected) in [
        (
            os(&["-Y", "verify", "-n", "git", "-f", "allowed", "-s", "sig"]),
            "unsupported mode verify",
        ),
        (
            os(&["-Y", "find-principals", "-s", "sig", "-f", "allowed"]),
            "unsupported mode find-principals",
        ),
        (
            vec![
                "-Y".into(),
                "sign".into(),
                "-n".into(),
                "file".into(),
                "-f".into(),
                "k".into(),
                file.clone(),
            ],
            "unsupported namespace file",
        ),
        (os(&["-n", "git", "x"]), "unsupported mode (none)"),
    ] {
        let err = git_sign::run(&fixture.socket, &args).await.unwrap_err();
        assert!(err.contains(expected), "{err}");
    }

    assert!(fixture.signer.payloads.lock().unwrap().is_empty());
    assert!(!Path::new(&format!("{}.sig", data.display())).exists());
}

#[tokio::test]
async fn git_sign_reports_the_refusal_of_the_control_plane() {
    let fixture = Fixture::new(Err("author does not match the persona identity"));
    let data = fixture.data_file(b"payload");

    let err = git_sign::run(&fixture.socket, &sign_args("/tmp/key", &data, false))
        .await
        .unwrap_err();

    assert_eq!(err, "author does not match the persona identity");
    assert!(!Path::new(&format!("{}.sig", data.display())).exists());
}

#[tokio::test]
async fn git_sign_fails_when_the_runtime_socket_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("buffer");
    std::fs::write(&data, b"payload").unwrap();

    let err = git_sign::run(
        &dir.path().join("none.sock"),
        &sign_args("/tmp/key", &data, false),
    )
    .await
    .unwrap_err();

    assert!(err.starts_with("signing is unavailable"), "{err}");
}

#[tokio::test(start_paused = true)]
async fn git_sign_gives_up_when_the_runtime_accepts_the_request_and_never_answers() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("runtime.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let _silent = tokio::spawn(async move {
        // Holds the connection open without a word.
        let held = listener.accept().await.unwrap();
        std::future::pending::<()>().await;
        drop(held);
    });
    let data = dir.path().join("buffer");
    std::fs::write(&data, b"payload").unwrap();

    // Tokio's paused clock jumps to the shim's 12 s bound once the connection is idle.
    let err = git_sign::run(&socket, &sign_args("/tmp/key", &data, false))
        .await
        .unwrap_err();

    assert!(err.starts_with("signing is unavailable"), "{err}");
    assert!(err.contains("within 12 s"), "{err}");
    assert!(!Path::new(&format!("{}.sig", data.display())).exists());
}

/// The sign request the relay put on the outbound channel.
async fn sent_request(rx: &mut mpsc::Receiver<AgentMessage>) -> df_proto::agent::v1::SignRequest {
    let msg = rx.recv().await.unwrap();
    assert_eq!(msg.seq, 0, "a sign request takes no place in the sequence");
    match msg.body {
        Some(agent_message::Body::SignRequest(request)) => request,
        other => panic!("not a sign request: {other:?}"),
    }
}

#[tokio::test]
async fn the_relay_sends_the_payload_and_answers_from_the_matching_response() {
    let (tx, mut rx) = mpsc::channel(4);
    let call = tokio::spawn(async move {
        sign_relay::request_signature(&tx, b"commit bytes", Duration::from_secs(10)).await
    });

    let request = sent_request(&mut rx).await;
    assert_eq!(request.payload.as_ref(), b"commit bytes");
    // A response for another request is ignored.
    sign_relay::complete(SignResponse {
        request_id: "other".into(),
        result: Some(sign_response::Result::Signature("wrong".into())),
    });
    assert!(sign_relay::is_open(&request.request_id));
    sign_relay::complete(SignResponse {
        request_id: request.request_id.clone(),
        result: Some(sign_response::Result::Signature("sig".into())),
    });

    assert_eq!(call.await.unwrap().unwrap(), "sig");
    assert!(!sign_relay::is_open(&request.request_id));
}

#[tokio::test]
async fn the_relay_reports_a_refusal_with_its_reason() {
    let (tx, mut rx) = mpsc::channel(4);
    let call = tokio::spawn(async move {
        sign_relay::request_signature(&tx, b"x", Duration::from_secs(10)).await
    });

    let request = sent_request(&mut rx).await;
    sign_relay::complete(SignResponse {
        request_id: request.request_id,
        result: Some(sign_response::Result::RefusedReason(
            "the run has ended".into(),
        )),
    });

    assert_eq!(call.await.unwrap().unwrap_err(), "the run has ended");
}

#[tokio::test(start_paused = true)]
async fn the_relay_fails_without_an_answer_and_a_late_answer_is_dropped() {
    let (tx, mut rx) = mpsc::channel(4);
    let call = tokio::spawn(async move {
        sign_relay::request_signature(&tx, b"x", Duration::from_secs(10)).await
    });
    let request = sent_request(&mut rx).await;

    let err = call.await.unwrap().unwrap_err();

    assert!(err.starts_with("signing is unavailable"), "{err}");
    assert!(
        !sign_relay::is_open(&request.request_id),
        "nothing is replayed"
    );
    sign_relay::complete(SignResponse {
        request_id: request.request_id,
        result: Some(sign_response::Result::Signature("late".into())),
    });
}

#[tokio::test]
async fn a_sign_request_is_sent_without_the_outbox_and_one_nobody_waits_for_is_never_sent() {
    use crate::connection::{CommandFilter, send_outbound};
    use crate::outbox::Outbox;
    use df_proto::agent::v1::{SignRequest, ToolCallRequest};
    use tokio::sync::{Mutex, watch};

    let dir = tempfile::tempdir().unwrap();
    let outbox = Arc::new(Mutex::new(
        Outbox::open(dir.path(), "run-1", 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let (out_tx, out_rx) = mpsc::channel(8);
    let (stream_tx, mut stream_rx) = mpsc::channel(8);
    let (disconnect_tx, _disconnect_rx) = mpsc::channel(1);
    let (stop_tx, stop_rx) = watch::channel(false);
    let message = |body| AgentMessage {
        seq: 0,
        body: Some(body),
    };
    // Queued while no stream existed: its shim gave up (the request is not registered any more).
    out_tx
        .send(message(agent_message::Body::SignRequest(SignRequest {
            request_id: "stale".into(),
            payload: b"old".to_vec().into(),
        })))
        .await
        .unwrap();
    out_tx
        .send(message(agent_message::Body::ToolCallRequest(
            ToolCallRequest {
                call_id: "call-1".into(),
                tool_name: "report_progress".into(),
                ..Default::default()
            },
        )))
        .await
        .unwrap();
    let waiting = tokio::spawn({
        let out_tx = out_tx.clone();
        async move { sign_relay::request_signature(&out_tx, b"new", Duration::from_secs(10)).await }
    });
    let worker = tokio::spawn(send_outbound(
        Arc::clone(&outbox),
        Arc::new(Mutex::new(CommandFilter::new())),
        Arc::new(Mutex::new(out_rx)),
        stream_tx,
        disconnect_tx,
        stop_rx,
    ));

    let first = stream_rx.recv().await.unwrap();
    let second = stream_rx.recv().await.unwrap();

    assert!(matches!(
        first.body,
        Some(agent_message::Body::ToolCallRequest(_))
    ));
    assert_eq!(first.seq, 1, "the tool call is stored and numbered");
    let Some(agent_message::Body::SignRequest(request)) = second.body else {
        panic!("the waiting sign request was not sent: {second:?}");
    };
    assert_eq!((second.seq, request.payload.as_ref()), (0, b"new".as_ref()));
    let stored = outbox.lock().await.read_unacknowledged(0).unwrap();
    assert_eq!(stored.len(), 1, "a sign request is never stored for replay");
    sign_relay::complete(SignResponse {
        request_id: request.request_id,
        result: Some(sign_response::Result::Signature("sig".into())),
    });
    assert_eq!(waiting.await.unwrap().unwrap(), "sig");
    stop_tx.send(true).unwrap();
    worker.await.unwrap();
}

#[tokio::test]
async fn the_relay_fails_when_the_connection_is_closed() {
    let (tx, rx) = mpsc::channel(4);
    drop(rx);

    let err = sign_relay::request_signature(&tx, b"x", Duration::from_secs(10))
        .await
        .unwrap_err();

    assert!(err.starts_with("signing is unavailable"), "{err}");
}
