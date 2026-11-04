//! A fake Anthropic upstream for the end-to-end cluster (`--serve-fake-anthropic`): answers
//! `POST /v1/messages` with a canned server-sent-events stream including usage, and `401` when
//! the request does not carry the key the gateway is supposed to inject.

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The text of the canned answer.
pub const ANSWER: &str = "Hello from the fake upstream.";

const MAX_HEAD_BYTES: usize = 64 * 1024;

/// Serves on `address` until the process ends. Requests need `x-api-key: <expected_key>`.
pub async fn serve(address: &str, expected_key: String) -> Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("bind {address}"))?;
    serve_on(listener, expected_key).await
}

/// Serves on an already bound `listener`; connections wait in its backlog until they are accepted.
async fn serve_on(listener: TcpListener, expected_key: String) -> Result<()> {
    loop {
        let (stream, _peer) = listener.accept().await.context("accept")?;
        let key = expected_key.clone();
        tokio::spawn(async move {
            if let Err(error) = answer(stream, &key).await {
                tracing_free_log(&error);
            }
        });
    }
}

#[expect(clippy::print_stderr, reason = "the fake upstream has no logger")]
fn tracing_free_log(error: &anyhow::Error) {
    eprintln!("fake upstream: {error:#}");
}

async fn answer(mut stream: TcpStream, expected_key: &str) -> Result<()> {
    let (head, mut body) = read_head(&mut stream).await?;
    let lower = head.to_ascii_lowercase();
    let length = lower
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while body.len() < length {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(buffer.get(..read).unwrap_or_default());
    }
    let key_ok = lower
        .lines()
        .any(|line| line == format!("x-api-key: {}", expected_key.to_ascii_lowercase()));
    let response = if !head.starts_with("POST /v1/messages ") {
        plain("404 Not Found", "{}")
    } else if !key_ok {
        plain(
            "401 Unauthorized",
            r#"{"type":"error","error":{"type":"authentication_error"}}"#,
        )
    } else {
        events()
    };
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await.ok();
    Ok(())
}

/// The request head and the body bytes read with it.
async fn read_head(stream: &mut TcpStream) -> Result<(String, Vec<u8>)> {
    let mut received = Vec::new();
    loop {
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).await?;
        anyhow::ensure!(read > 0, "connection closed before the request head");
        received.extend_from_slice(buffer.get(..read).unwrap_or_default());
        if let Some(end) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            let head =
                String::from_utf8_lossy(received.get(..end).unwrap_or_default()).into_owned();
            let body = received.get(end + 4..).unwrap_or_default().to_vec();
            return Ok((head, body));
        }
        anyhow::ensure!(received.len() < MAX_HEAD_BYTES, "request head too large");
    }
}

fn plain(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn events() -> String {
    let body = format!(
        concat!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"model\":\"fake-model\",",
            "\"usage\":{{\"input_tokens\":12,\"output_tokens\":1}}}}}}\n\n",
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"{}\"}}}}\n\n",
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"usage\":{{\"output_tokens\":7}}}}\n\n",
            "event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
        ),
        ANSWER
    );
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n{body}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn request(address: &str, key: &str, path: &str) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let body = "{}";
        let request = format!(
            "POST {path} HTTP/1.1\r\nhost: x\r\nx-api-key: {key}\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn answers_the_canned_stream_only_for_the_expected_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(serve_on(listener, "secret".into()));

        let good = request(&address, "secret", "/v1/messages").await;
        let wrong_key = request(&address, "df-placeholder", "/v1/messages").await;
        let other_path = request(&address, "secret", "/v1/other").await;

        assert!(
            good.starts_with("HTTP/1.1 200") && good.contains(ANSWER),
            "{good}"
        );
        assert!(wrong_key.starts_with("HTTP/1.1 401"), "{wrong_key}");
        assert!(other_path.starts_with("HTTP/1.1 404"), "{other_path}");
        server.abort();
    }
}
