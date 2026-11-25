//! The sign requests that wait for an answer of the control plane (design D2 of
//! `persona-commit-signing`).
//!
//! A sign request is not stored in the outbox and not sent again after a reconnect: the shim fails
//! the commit and the agent commits again. The socket handler registers a request here, the
//! connection sends it only while it is registered and completes it with the `SignResponse`. One
//! runtime process handles one run, so the registry is process-wide.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

use df_proto::agent::v1::{SignRequest, SignResponse, agent_message, sign_response};
use tokio::sync::{mpsc, oneshot};

/// How long the control plane may take to answer.
pub const SIGN_TIMEOUT: Duration = Duration::from_secs(10);

static OPEN: LazyLock<Mutex<HashMap<String, oneshot::Sender<SignResponse>>>> =
    LazyLock::new(Mutex::default);

fn open() -> std::sync::MutexGuard<'static, HashMap<String, oneshot::Sender<SignResponse>>> {
    // Nothing in the map is left half-updated by a panic.
    OPEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `request_id` still waits for its answer; the connection sends only such requests.
pub fn is_open(request_id: &str) -> bool {
    open().contains_key(request_id)
}

/// Completes the request of `response`; a late or unknown response is dropped.
pub fn complete(response: SignResponse) {
    if let Some(tx) = open().remove(&response.request_id) {
        tx.send(response).ok(); // the waiter may have given up
    }
}

/// Registers a request and returns where its answer arrives.
fn register(request_id: &str) -> oneshot::Receiver<SignResponse> {
    let (tx, rx) = oneshot::channel();
    open().insert(request_id.to_string(), tx);
    rx
}

fn cancel(request_id: &str) {
    open().remove(request_id);
}

/// Sends `payload` to the control plane for a signature: the armored signature, or why there is
/// none. Fails after `timeout` without an answer (the runtime passes [`SIGN_TIMEOUT`]).
pub async fn request_signature(
    outbound_tx: &mpsc::Sender<df_proto::agent::v1::AgentMessage>,
    payload: &[u8],
    timeout: Duration,
) -> Result<String, String> {
    let request_id = uuid::Uuid::now_v7().to_string();
    let answer = register(&request_id);
    let message = df_proto::agent::v1::AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::SignRequest(SignRequest {
            request_id: request_id.clone(),
            payload: payload.to_vec().into(),
        })),
    };
    if outbound_tx.send(message).await.is_err() {
        cancel(&request_id);
        return Err("signing is unavailable: the connection to the control plane is closed".into());
    }
    match tokio::time::timeout(timeout, answer).await {
        Ok(Ok(response)) => match response.result {
            Some(sign_response::Result::Signature(signature)) => Ok(signature),
            Some(sign_response::Result::RefusedReason(reason)) => Err(reason),
            None => Err("the control plane answered without a signature".into()),
        },
        Ok(Err(_closed)) => Err("signing is unavailable: the request was dropped".into()),
        Err(_elapsed) => {
            cancel(&request_id);
            Err(format!(
                "signing is unavailable: the control plane did not answer within {} s",
                timeout.as_secs()
            ))
        }
    }
}
